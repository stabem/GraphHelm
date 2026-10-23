#!/usr/bin/env python3
"""Run the reached-test plan with bounded build-slot leases.

The selector owns reachability. This runner owns only orchestration: short adjacent Cargo
checks use one bounded slot batch by default; --no-batch-slot opts out and --batch-slot
remains accepted for compatibility. Format, Python, and Studio steps run directly. No command is passed through a shell. Whole-package plans are expanded
by the selector into explicit Cargo targets; a target requiring features runs with
those features (#1280), and anything the selector cannot run is reported as unsupported
and remains incomplete rather than being silently skipped.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import shutil
import subprocess
import sys
import tempfile
import time
import re
from pathlib import Path
from typing import Any

# Owner rule: children run hidden in the calling terminal, never in a new console window.
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0


MAX_CAPTURE = 4000
MAX_VITEST_REPORT = 2 * 1024 * 1024
_SHELL_MARKERS = ("&&", "||", "|", ">", "<", "`", "$(")
_SHELL_EXECUTABLES = {"sh", "bash", "cmd", "cmd.exe", "powershell", "powershell.exe", "pwsh", "pwsh.exe"}
_CARGO_TEST_TARGET_FLAGS = ("--test", "--bin", "--lib", "--doc", "--example")
_UNITTEST_ZERO = re.compile(r"\bRan\s+0\s+tests?\b")
_BATCH_SCHEMA = "graphhelm.slot-batch/1"
_BATCH_RESULT_SCHEMA = "graphhelm.slot-batch-result/1"
_BATCH_STOP_REASONS = {"exhausted", "leaseBoundary", "deadline", "childFailure", "spawnFailure"}


def _git(repo: Path, *args: str) -> str:
    return subprocess.run(["git", *args], cwd=repo, check=True, capture_output=True, text=True, creationflags=NO_WINDOW).stdout.strip()


def _tail(path: Path) -> str:
    try:
        with path.open("rb") as stream:
            stream.seek(0, 2)
            stream.seek(max(0, stream.tell() - MAX_CAPTURE))
            data = stream.read().decode("utf-8", errors="replace")
    except OSError as exc:
        return f"<log unavailable: {exc}>"
    return data[-MAX_CAPTURE:]


def _resolve_graphhelm(args: argparse.Namespace) -> Path:
    """Select and resolve the GraphHelm executable once for this run."""
    explicit = getattr(args, "graphhelm", None)
    configured = explicit or os.environ.get("GRAPHHELM_CLI")
    if configured:
        candidate = Path(configured).expanduser()
        if not candidate.is_file():
            source = "--graphhelm" if explicit else "GRAPHHELM_CLI"
            raise RuntimeError(f"{source} does not name an existing executable: {configured}")
        return candidate if candidate.is_absolute() else Path(os.path.join(os.getcwd(), str(candidate)))
    found = shutil.which("graphhelm")
    if not found:
        raise RuntimeError("graphhelm executable was not found on PATH")
    candidate = Path(found) if os.path.isabs(found) else Path(os.path.join(os.getcwd(), found))
    if not candidate.is_file():
        raise RuntimeError(f"graphhelm PATH entry is not an existing executable: {found}")
    return candidate


def _selector(repo: Path, base: str, head: str, log: Path, graphhelm: Path | None = None) -> dict[str, Any]:
    selector = repo / "tools" / "reached-tests" / "reached_tests.py"
    if not selector.is_file():
        raise RuntimeError(f"selector missing in target repository: {selector}")
    env = os.environ.copy()
    if graphhelm is not None:
        env["GRAPHHELM_CLI"] = str(graphhelm)
        env["PATH"] = str(graphhelm.parent) + os.pathsep + env.get("PATH", "")
    proc = subprocess.run(
        [sys.executable, str(selector), "--repo", str(repo), "--base", base, "--head", head, "--json"],
        cwd=repo, capture_output=True, text=True, env=env, creationflags=NO_WINDOW
    )
    log.write_text(proc.stdout + ("\n[selector stderr]\n" + proc.stderr if proc.stderr else ""), encoding="utf-8")
    if proc.returncode:
        raise RuntimeError(f"selector failed ({proc.returncode}); see {log}")
    try:
        return json.loads(proc.stdout)
    except (OSError, json.JSONDecodeError) as exc:
        raise RuntimeError(f"selector did not produce JSON: {exc}") from exc


def _steps(plan: dict[str, Any]) -> tuple[list[dict[str, Any]], str | None]:
    raw = plan.get("steps")
    if not isinstance(raw, list):
        return [], "selector steps must be a list"
    result = []
    for index, step in enumerate(raw):
        if not isinstance(step, dict) or not isinstance(step.get("argv"), list) or not step["argv"]:
            return [], f"invalid selector step {index}"
        if any(not isinstance(arg, str) or not arg for arg in step["argv"]):
            return [], f"invalid argv in selector step {index}"
        observer_argv = ["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass",
                         "-File", "tools/no-window/test_observe.ps1"]
        allowed_observer = (step["argv"] == observer_argv and step.get("cwd", ".") == "."
                            and step.get("slot") is False)
        if (step["argv"][0].lower() in _SHELL_EXECUTABLES and not allowed_observer) or any(
            any(marker in arg for marker in _SHELL_MARKERS) for arg in step["argv"]
        ):
            return [], f"shell syntax is forbidden in selector step {index}; use argv entries"
        cwd = step.get("cwd", ".")
        if cwd != "." and cwd not in STUDIO_APPS:
            return [], f"invalid cwd in selector step {index}: {cwd!r}"
        if type(step.get("slot")) is not bool:
            return [], f"slot must be boolean in selector step {index}"
        argv = step["argv"]
        if argv[:2] == ["cargo", "+1.97.1"]:
            try:
                command = argv[2]
            except IndexError:
                return [], f"incomplete Cargo argv in selector step {index}"
            if command in ("build", "test", "clippy") and not step["slot"]:
                return [], f"Cargo {command} must use a build slot in selector step {index}"
            if command == "fmt" and step["slot"]:
                return [], f"Cargo fmt must run outside the build slot in selector step {index}"
            if command == "test" and not any(flag in argv for flag in _CARGO_TEST_TARGET_FLAGS):
                return [], f"unscoped Cargo test is forbidden in selector step {index}; use an explicit target"
        observer = step.get("observer")
        if observer not in (None, "browser"):
            return [], f"invalid observer in selector step {index}"
        result.append({"argv": argv, "cwd": cwd, "slot": step["slot"], "observer": observer})
    return result, None


def _whole_reason(plan: dict[str, Any], reason: str | None) -> str | None:
    whole = bool(plan.get("packages")) or bool(plan.get("wholePackage"))
    whole = whole or any(isinstance(step, dict) and step.get("whole") for step in plan.get("steps", []))
    if whole and not reason:
        return "whole-package reach requires --allow-whole-package with an explicit reason"
    return None


def _slot_telemetry(path: Path, command: list[str]) -> tuple[float | None, float | None]:
    """Read only the final slot envelope, after bounded backwards log reading."""
    if len(command) < 4 or command[1:4] != ["--json", "workspace", "slot"]:
        return None, None
    text = _tail(path)
    lines = [line for line in text.splitlines() if line.strip()]
    if not lines:
        return None, None
    try:
        value = json.loads(lines[-1])
    except json.JSONDecodeError:
        return None, None
    if not isinstance(value, dict) or value.get("command") != "workspace.slot":
        return None, None
    data = value.get("data")
    if not isinstance(data, dict) or not ("waitedSeconds" in data and "heldSeconds" in data):
        return None, None
    waited, held = data.get("waitedSeconds"), data.get("heldSeconds")
    if not all(isinstance(v, (int, float)) and math.isfinite(v) and v >= 0 for v in (waited, held)):
        return None, None
    return float(waited), float(held)


# #1829: the Studio app folders whose local Node tools a step may run; each has its own lockfile.
STUDIO_APPS = ("apps/studio",)


def _studio_app(repo: Path, cwd: Path) -> str | None:
    """The Studio app folder `cwd` is, relative to the repo, or None."""
    return next((app for app in STUDIO_APPS if cwd == repo / app), None)


def studio_install_problem(studio: Path) -> str | None:
    """Return why apps/studio/node_modules is not whole, or None. Reads files only; never repairs."""
    modules = studio / "node_modules"
    if not (modules / ".bin").is_dir():
        return "node_modules/.bin is missing"
    try:
        wanted = json.loads((studio / "package-lock.json").read_text(encoding="utf-8")).get("packages", {})
        have = json.loads((modules / ".package-lock.json").read_text(encoding="utf-8")).get("packages", {})
    except (OSError, ValueError):
        return "node_modules/.package-lock.json or package-lock.json is missing or unreadable"
    for key, meta in wanted.items():
        if not key or meta.get("optional") or meta.get("peer"):
            continue
        if key not in have or have[key].get("version") != meta.get("version"):
            return f"{key} is missing or differs from package-lock.json"
    return None


STUDIO_INSTALL_TIMEOUT = 900


def _is_lane_worktree(repo: Path) -> bool:
    """A linked worktree has a `.git` file; the main checkout has a `.git` directory."""
    return (repo / ".git").is_file()


def _install_absent_studio(repo: Path, studio: Path) -> None:
    """Run a hidden `npm ci` when node_modules is wholly absent, in a lane worktree only.

    A half-installed tree and the main checkout (a running Studio may hold locks) are never touched.
    """
    if (studio / "node_modules").exists() or not _is_lane_worktree(repo):
        return
    npm = shutil.which("npm") or "npm"
    app = studio.relative_to(repo).as_posix()
    print(f"[reached-tests] {app}/node_modules is absent: npm ci in {studio}", file=sys.stderr, flush=True)
    try:
        proc = subprocess.run([npm, "ci"], cwd=studio, capture_output=True, text=True,
                              timeout=STUDIO_INSTALL_TIMEOUT, creationflags=NO_WINDOW)
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise RuntimeError(f"npm ci in {app} failed ({type(exc).__name__})") from exc
    if proc.returncode != 0:
        raise RuntimeError(f"npm ci in {app} failed (exit {proc.returncode}): {(proc.stderr or '')[-400:]}")


def _resolve_argv(repo: Path, cwd: Path, argv: list[str]) -> list[str]:
    """Resolve local Node package entrypoints without npx downloading anything."""
    if not argv or argv[0] != "npx":
        return argv
    app = _studio_app(repo, cwd)
    if len(argv) < 2 or app is None:
        raise RuntimeError("npx is allowed only for a local Studio tool")
    package = argv[1]
    _install_absent_studio(repo, cwd)
    problem = studio_install_problem(cwd)
    if problem:
        raise RuntimeError(
            f"{app} install is not whole ({problem}); run `npm ci` in {app}. "
            "A running Studio may lock files in node_modules (EPERM): stop it first, or rename the locked .node file aside. "
            "Not repaired automatically.")
    command = _effective_argv(repo, cwd, argv, graphhelm=None)
    if not Path(command[1]).is_file():
        raise RuntimeError(f"local Studio tool missing: {package}; run npm ci in {app}")
    return command


def _vitest_step(step: dict[str, Any]) -> bool:
    return step["cwd"] in STUDIO_APPS and step["argv"][:2] == ["npx", "vitest"]


def _vitest_evidence(path: Path) -> tuple[dict[str, Any] | None, str | None]:
    try:
        if path.stat().st_size > MAX_VITEST_REPORT:
            return None, "Vitest report exceeds size limit"
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as exc:
        return None, f"Vitest report missing or malformed: {type(exc).__name__}"
    fields = ("numTotalTests", "numPassedTests", "numFailedTests", "numPendingTests", "numTodoTests",
              "numFailedTestSuites")
    if not isinstance(data, dict) or any(type(data.get(key)) is not int or data[key] < 0 for key in fields):
        return None, "Vitest report has invalid counts"
    if type(data.get("success")) is not bool or not isinstance(data.get("testResults"), list):
        return None, "Vitest report has invalid result fields"
    passed, failed = data["numPassedTests"], data["numFailedTests"]
    if passed + failed + data["numPendingTests"] + data["numTodoTests"] > data["numTotalTests"]:
        return None, "Vitest report has inconsistent counts"
    evidence = {"passed": passed, "failed": failed, "skipped": data["numPendingTests"] + data["numTodoTests"],
                "errors": data["numFailedTestSuites"], "total": data["numTotalTests"],
                "success": data["success"], "files": len(data["testResults"]), "report": str(path)}
    return evidence, None


def _browser_toolchain_ready() -> bool:
    project = os.environ.get("GRAPHHELM_JOURNEY_TOOLCHAIN_PROJECT")
    return bool(project) and (Path(project) / "node_modules" / "@playwright" / "test").is_dir()


def _cargo(argv: list[str]) -> bool:
    return len(argv) >= 3 and argv[0] == "cargo" and argv[1].startswith("+")


def _fmt_check(argv: list[str]) -> bool:
    return _cargo(argv) and argv[2:] == ["fmt", "--all", "--", "--check"]


def _batchable(step: dict[str, Any]) -> bool:
    argv = step["argv"]
    return bool(step["slot"] and _cargo(argv) and not _fmt_check(argv))


def _execution_order(steps: list[dict[str, Any]], batch_slot: bool) -> list[dict[str, Any]]:
    """Return execution entries while retaining each selector index."""
    entries = [{**step, "index": index} for index, step in enumerate(steps)]
    if not batch_slot:
        return entries
    fmts = [entry for entry in entries if _fmt_check(entry["argv"])]
    rest = [entry for entry in entries if not _fmt_check(entry["argv"])]
    first_cargo = next((index for index, entry in enumerate(rest) if entry["slot"] and _cargo(entry["argv"])), len(rest))
    return rest[:first_cargo] + fmts + rest[first_cargo:]


# #1892: libtest threads scale with the host instead of a fixed 2. One thread per ~1.5 GB of free
# memory and per two logical cores, never below the old floor of 2 and never above 16.
TEST_THREADS_FLOOR = 2
TEST_THREADS_CEILING = 16
GB_PER_TEST_THREAD = 1.5


def _free_memory_gb() -> float | None:
    """Free physical memory in GB, or None when the host does not say."""
    try:
        if os.name == "nt":
            import ctypes

            class _Status(ctypes.Structure):
                _fields_ = [("dwLength", ctypes.c_ulong), ("dwMemoryLoad", ctypes.c_ulong),
                            ("ullTotalPhys", ctypes.c_ulonglong), ("ullAvailPhys", ctypes.c_ulonglong),
                            ("ullTotalPageFile", ctypes.c_ulonglong), ("ullAvailPageFile", ctypes.c_ulonglong),
                            ("ullTotalVirtual", ctypes.c_ulonglong), ("ullAvailVirtual", ctypes.c_ulonglong),
                            ("ullAvailExtendedVirtual", ctypes.c_ulonglong)]
            status = _Status()
            status.dwLength = ctypes.sizeof(_Status)
            if not ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(status)):
                return None
            return status.ullAvailPhys / 1e9
        return os.sysconf("SC_AVPHYS_PAGES") * os.sysconf("SC_PAGE_SIZE") / 1e9
    except (AttributeError, OSError, ValueError):
        return None


def _test_threads(free_gb: float | None, cores: int | None) -> int:
    """Threads for every cargo test step: the floor when memory or cores are unknown or short."""
    if free_gb is None or not cores:
        return TEST_THREADS_FLOOR
    wanted = min(int(free_gb // GB_PER_TEST_THREAD), cores // 2, TEST_THREADS_CEILING)
    return max(TEST_THREADS_FLOOR, wanted)


def _with_test_threads(argv: list[str], threads: int) -> list[str]:
    """Rewrite the selector's pinned `--test-threads=N` in a cargo test command to `threads`."""
    if not _cargo(argv) or "test" not in argv or "--" not in argv:
        return list(argv)
    split = argv.index("--")
    return argv[:split] + [f"--test-threads={threads}" if arg.startswith("--test-threads=") else arg
                           for arg in argv[split:]]


def _batch_result(value: Any, count: int, cli_returncode: int) -> tuple[dict[str, Any] | None, str | None]:
    """Strictly validate the CLI batch envelope and return local indices."""
    if not isinstance(value, dict) or value.get("schema") != _BATCH_RESULT_SCHEMA:
        return None, "missing or malformed batch telemetry"
    completed = value.get("completed")
    remaining = value.get("remaining")
    stop = value.get("stopReason")
    if not isinstance(completed, list) or not isinstance(remaining, list) or stop not in _BATCH_STOP_REASONS:
        return None, "missing or malformed batch telemetry"
    parsed: list[dict[str, Any]] = []
    if len(completed) > count:
        return None, "batch completed entries exceed command count"
    for expected, item in enumerate(completed):
        if not isinstance(item, dict) or type(item.get("index")) is not int or item.get("index") != expected:
            return None, "batch completed entries must be an ordered prefix"
        code = item.get("exitCode")
        elapsed = item.get("elapsedSeconds")
        if type(code) is not int or isinstance(code, bool) or not isinstance(elapsed, (int, float)) or isinstance(elapsed, bool) or not math.isfinite(elapsed) or elapsed <= 0:
            return None, "invalid batch completion telemetry"
        parsed.append(item)
    if any(type(index) is not int for index in remaining) or remaining != list(range(len(parsed), count)):
        return None, "batch remaining entries are not the exact complement"
    codes = [item["exitCode"] for item in parsed]
    if any(code != 0 for code in codes[:-1]):
        return None, "only the final completed command may fail"
    if stop == "exhausted" and (len(parsed) != count or remaining or cli_returncode or any(codes)):
        return None, "exhausted batch must complete successfully"
    if stop == "leaseBoundary" and (not parsed or not remaining or cli_returncode or any(codes)):
        return None, "leaseBoundary must leave successful pending commands"
    if stop == "deadline" and (not remaining or cli_returncode or any(codes)):
        return None, "deadline must leave successful pending commands"
    if stop == "childFailure" and (not parsed or codes[-1] == 0 or codes[-1] != cli_returncode):
        return None, "childFailure must end with the CLI's nonzero child code"
    if stop == "spawnFailure" and (not remaining or cli_returncode == 0 or any(codes)):
        return None, "spawnFailure must leave pending commands without completed failures"
    return {"completed": parsed, "remaining": remaining, "stopReason": stop}, None


def _last_json(path: Path) -> Any:
    lines = [line for line in _tail(path).splitlines() if line.strip()]
    if not lines:
        return None
    try:
        return json.loads(lines[-1])
    except json.JSONDecodeError:
        return None


def _slot_refusal(path: Path) -> str | None:
    """The slot's own refusal (cap, floor, wait limit), when its final envelope is one (#1542)."""
    value = _last_json(path)
    if not isinstance(value, dict) or value.get("command") != "workspace.slot" or value.get("ok") is not False:
        return None
    diagnostics = value.get("diagnostics")
    if not isinstance(diagnostics, list):
        return None
    messages = [d["message"] for d in diagnostics if isinstance(d, dict) and isinstance(d.get("message"), str)]
    return "slot refused the batch: " + "; ".join(messages) if messages else None


def _batch_envelope(path: Path, cli_returncode: int | None = None) -> tuple[dict[str, Any] | None, float | None, float | None]:
    value = _last_json(path)
    if not isinstance(value, dict) or value.get("command") != "workspace.slot":
        return None, None, None
    data = value.get("data")
    if not isinstance(data, dict):
        return None, None, None
    waited, held = data.get("waitedSeconds"), data.get("heldSeconds")
    if not all(isinstance(v, (int, float)) and not isinstance(v, bool) and math.isfinite(v) and v >= 0 for v in (waited, held)):
        return None, None, None
    if cli_returncode is not None:
        outer_ok = value.get("ok")
        data_exit = data.get("exitCode")
        if type(outer_ok) is not bool or outer_ok is not True:
            return None, None, None
        if type(data_exit) is not int or data_exit != cli_returncode:
            return None, None, None
    return data.get("batch"), float(waited), float(held)


# Environment names whose values can change a test verdict. A changed value, an added or a removed
# name changes the fingerprint and every reused result reruns. GRAPHHELM_SLOT_BATCH is the runner's
# own per-call channel, never an input.
_ENV_PREFIXES = ("CARGO", "RUST", "GRAPHHELM_", "NODE", "NPM_CONFIG", "PYTHON", "VITEST", "PG", "DATABASE_URL")
_ENV_IGNORED = {"GRAPHHELM_SLOT_BATCH"}


def _sha(value: Any) -> str:
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":")).encode("utf-8")).hexdigest()


def _env_fingerprint(env: dict[str, str]) -> str:
    return _sha(sorted(((k.upper() if os.name == "nt" else k), v) for k, v in env.items()
                       if (k.upper() if os.name == "nt" else k).startswith(_ENV_PREFIXES + ("PATH", "PATHEXT")) and k not in _ENV_IGNORED))


class _MissingExecutable(RuntimeError):
    """No existing invocation can be selected for this required command."""


def _slot_program(program: str) -> str:
    """#1842: Windows resolves `cargo` to `...\\cargo.EXE` (PATHEXT's spelling). The same file under
    its lower-case name, which every `workspace slot` admits; an older slot compares the name with
    case and refused every reached step (GHCLI037)."""
    path = Path(program)
    return str(path.with_name("cargo.exe")) if path.name.lower() == "cargo.exe" else program


def _effective_argv(repo: Path, cwd: Path, argv: list[str], graphhelm: Path | None) -> list[str]:
    """Select existing bounded rewrites, without preparing or installing anything."""
    command = list(argv)
    if command[0] == "npx":
        entries = {"vitest": "vitest/vitest.mjs", "tsc": "typescript/bin/tsc"}
        if _studio_app(repo, cwd) is None or len(command) < 2 or command[1] not in entries:
            raise RuntimeError("unsupported local Studio command identity")
        command = ["node", str(cwd / "node_modules" / entries[command[1]]), *command[2:]]
    if command[0] in ("python", "python3"):
        command[0] = sys.executable
    if graphhelm is not None and Path(command[0]).name.lower() in ("graphhelm", "graphhelm.exe"):
        command[0] = str(graphhelm)
    return command


def _needs_studio_preparation(repo: Path, cwd: Path) -> bool:
    return _studio_app(repo, cwd) is not None and _is_lane_worktree(repo) and not (cwd / "node_modules").exists()


def _discovery_output(argv: list[str], cwd: Path, env: dict[str, str], timeout: float) -> str:
    """Bound only identity discovery, including output held by descendants."""
    import threading
    deadline = time.monotonic() + timeout
    output = bytearray()
    overflow = threading.Event()
    proc = subprocess.Popen(argv, cwd=cwd, env=env, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, creationflags=NO_WINDOW)

    def read():
        try:
            while True:
                chunk = proc.stdout.read(4096)
                if not chunk:
                    break
                if len(output) + len(chunk) > 65536:
                    overflow.set()
                    proc.kill()
                    break
                output.extend(chunk)
        except OSError:
            overflow.set()

    reader = threading.Thread(target=read, daemon=True)
    reader.start()
    try:
        code = proc.wait(timeout=max(0.001, timeout - 0.1))
    except subprocess.TimeoutExpired:
        proc.kill()
        try:
            proc.wait(timeout=max(0, min(0.05, deadline - time.monotonic())))
        except subprocess.TimeoutExpired:
            raise RuntimeError("executable identity discovery child could not be reaped before its cleanup deadline") from None
        reader.join(timeout=max(0, min(0.05, deadline - time.monotonic())))
        if not reader.is_alive():
            proc.stdout.close()
        raise RuntimeError("executable identity discovery exceeded its deadline") from None
    reader.join(timeout=max(0, deadline - time.monotonic()))
    if reader.is_alive() or overflow.is_set():
        raise RuntimeError("executable identity discovery output is unavailable or exceeds its bound")
    proc.stdout.close()
    if code:
        raise RuntimeError("executable identity discovery refused the selected tool")
    return output.decode("utf-8", errors="replace").strip()


_PYTHON_IMAGE_DISCOVERY = """import ctypes, json
from ctypes import wintypes
kernel = ctypes.WinDLL("kernel32", use_last_error=True)
image = kernel.GetModuleFileNameW
image.argtypes = [wintypes.HMODULE, wintypes.LPWSTR, wintypes.DWORD]
image.restype = wintypes.DWORD
buffer = ctypes.create_unicode_buffer(32768)
current_process_module = None
length = image(current_process_module, buffer, len(buffer))
if not length or length >= len(buffer):
    raise OSError(ctypes.get_last_error(), "loaded image path unavailable or truncated")
print(json.dumps({"loadedImage": buffer.value}, ensure_ascii=True))
"""


class _ExecutableCapture:
    """Capture-local executable resolution, discovery and streamed-byte caches."""

    def __init__(self, env: dict[str, str]):
        self.env = env
        self.deadline = time.monotonic() + 60
        self.candidates = 0
        self.bytes = 0
        self.digests: dict[str, dict[str, Any]] = {}
        self.discovered: dict[tuple, str] = {}
        self.configs: dict[str, str] = {}
        self.config_bytes = 0
        self.programs: dict[tuple[str, str], str] = {}

    def _remaining(self) -> float:
        remaining = self.deadline - time.monotonic()
        if remaining <= 0:
            raise RuntimeError("executable identity capture exceeded its deadline")
        return min(5.0, remaining)

    def _program(self, name: str, cwd: Path) -> str:
        self._remaining()
        cache_key = (name, str(cwd))
        if cache_key in self.programs:
            return self.programs[cache_key]
        env = {key.upper(): value for key, value in self.env.items()} if os.name == "nt" else self.env
        if os.name == "nt" and len(env) != len(self.env):
            raise RuntimeError("case-aliased Windows environment is not attested")
        explicit = os.path.isabs(name) or os.path.dirname(name)
        directories = [""] if explicit else env.get("PATH", os.defpath).split(os.pathsep)
        extensions = [""]
        if os.name == "nt" and not Path(name).suffix:
            extensions = env.get("PATHEXT", ".COM;.EXE;.BAT;.CMD").split(";")
        for directory in directories:
            base = name if explicit else os.path.join(directory, name)
            # Prefixing retains symlink-sensitive '..'; abspath/resolve would change invocation.
            invocation = base if os.path.isabs(base) else os.path.join(str(cwd), base)
            for extension in extensions:
                self.candidates += 1
                if self.candidates > 4096:
                    raise RuntimeError("executable identity PATH search exceeded its bound")
                candidate = invocation + extension
                if os.path.isfile(candidate) and os.access(candidate, os.X_OK):
                    if os.name == "nt" and Path(candidate).suffix.lower() in (".cmd", ".bat"):
                        raise RuntimeError("existing Windows batch invocation runs without reusable identity")
                    self.programs[cache_key] = candidate
                    return candidate
        raise _MissingExecutable("required reached executable is missing or not executable")

    def _identity(self, invocation: str, cwd: Path, *, python_selected: bool = False) -> dict[str, Any]:
        import stat
        self._remaining()
        # AppExecLink may raise WinError1920 on strict resolve; inspect its lexical tag first.
        # Only an explicitly selected Python role permits the fixed isolated child discovery.
        if (sys.platform == "win32" and python_selected
                and getattr(os.lstat(invocation), "st_reparse_tag", None) == 0x8000001B):
            reply = json.loads(self._discover([invocation, "-I", "-S", "-X", "utf8", "-c", _PYTHON_IMAGE_DISCOVERY], cwd))
            image = reply.get("loadedImage") if isinstance(reply, dict) else None
            if (not isinstance(image, str) or not image or not os.path.isabs(image)
                    or set(reply) != {"loadedImage"} or os.path.normcase(image) == os.path.normcase(invocation)):
                raise RuntimeError("selected Python alias did not identify a distinct absolute executable image")
            if getattr(os.lstat(image), "st_reparse_tag", 0):
                raise RuntimeError("selected Python alias image is another unsupported reparse point")
            target = str(Path(image).resolve(strict=True))
            if os.path.normcase(target) == os.path.normcase(invocation):
                raise RuntimeError("selected Python alias image resolves to its unreadable invocation")
        else:
            try:
                target = str(Path(invocation).resolve(strict=True))
            except RuntimeError:
                raise RuntimeError("executable identity has an unsupported link chain") from None
        if target not in self.digests:
            if len(self.digests) >= 128:
                raise RuntimeError("executable identity count exceeds its bound")
            before = os.stat(target)
            if not stat.S_ISREG(before.st_mode) or before.st_size > 256 * 1024 * 1024:
                raise RuntimeError("executable identity requires a bounded regular file")
            digest = hashlib.sha256()
            with open(target, "rb") as stream:
                opened = os.fstat(stream.fileno())
                if not stat.S_ISREG(opened.st_mode) or (opened.st_dev, opened.st_ino) != (before.st_dev, before.st_ino):
                    raise RuntimeError("executable identity changed while opening")
                while True:
                    self._remaining()
                    chunk = stream.read(1 << 20)
                    if not chunk:
                        break
                    self.bytes += len(chunk)
                    if self.bytes > 1024 * 1024 * 1024 or stream.tell() > 256 * 1024 * 1024:
                        raise RuntimeError("executable identity bytes exceed the capture bound")
                    digest.update(chunk)
                after = os.fstat(stream.fileno())
            if (before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (after.st_size, after.st_mtime_ns, after.st_ctime_ns):
                raise RuntimeError("executable identity changed while reading")
            self.digests[target] = {"digestTarget": target, "sha256": digest.hexdigest()}
        return {"invocationPath": invocation, **self.digests[target]}

    def _discover(self, argv: list[str], cwd: Path, env: dict[str, str] | None = None) -> str:
        effective_env = self.env if env is None else env
        key = (str(cwd), tuple(argv), _env_fingerprint(effective_env))
        if key not in self.discovered:
            self.discovered[key] = _discovery_output(argv, cwd, effective_env, self._remaining())
        return self.discovered[key]

    def _rust(self, command: list[str], cwd: Path) -> dict[str, Any]:
        try:
            import tomllib
        except ImportError:
            raise RuntimeError("Cargo configuration attestation needs an available standard-library TOML reader") from None
        override_names = ("RUSTC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "CARGO_BUILD_RUSTC",
                          "CARGO_BUILD_RUSTC_WRAPPER", "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER")
        if any(self.env.get(name) for name in override_names):
            raise RuntimeError("compiler override execution runs without reusable identity")
        paths = {Path(self.env.get("CARGO_HOME", str(Path.home() / ".cargo")))}
        paths.update(parent / ".cargo" for parent in (cwd, *cwd.parents))
        for directory in sorted(paths):
            for filename in ("config", "config.toml"):
                path = directory / filename
                if not path.exists() or str(path) in self.configs:
                    continue
                if not path.is_file():
                    raise RuntimeError("compiler configuration is not a regular readable file")
                if len(self.configs) >= 64 or path.stat().st_size > 65536:
                    raise RuntimeError("compiler configuration exceeds identity bounds")
                with path.open("rb") as stream:
                    data = stream.read(65537)
                if len(data) > 65536:
                    raise RuntimeError("compiler configuration exceeds identity bounds")
                self.config_bytes += len(data)
                if self.config_bytes > 1024 * 1024:
                    raise RuntimeError("compiler configuration capture exceeds its bound")
                self.configs[str(path)] = hashlib.sha256(data).hexdigest()
                config = tomllib.loads(data.decode("utf-8"))
                build = config.get("build", {})
                if not isinstance(build, dict):
                    raise RuntimeError("compiler configuration selection is ambiguous")
                if any(build.get(key) for key in ("rustc", "rustc-wrapper", "rustc-workspace-wrapper")):
                    raise RuntimeError("compiler configuration override runs without reusable identity")
        selection = command[1][1:] if len(command) > 1 and command[1].startswith("+") else None
        kind = command[2 if selection else 1] if len(command) > (2 if selection else 1) else ""
        names = ["cargo", "rustc"] + (["rustfmt"] if kind == "fmt" else ["cargo-clippy", "clippy-driver"] if kind == "clippy" else [])
        try:
            rustup = self._program("rustup", cwd)
        except _MissingExecutable:
            raise RuntimeError("native Rust toolchain execution has no supported identity discovery") from None
        result = {"rustup": self._identity(rustup, cwd)}
        discovery_env = {**self.env, "RUSTUP_AUTO_INSTALL": "0"}
        for name in names:
            proxy = self._program(command[0] if name == "cargo" else name, cwd)
            argv = [rustup, "which", *(["--toolchain", selection] if selection else []), name]
            selected = self._discover(argv, cwd, discovery_env)
            result[name] = {"proxy": self._identity(proxy, cwd), "selected": self._identity(selected, cwd)}
        rustc = self._program("rustc", cwd)
        result["version"] = self._discover([rustc, *(["+" + selection] if selection else []), "-Vv"], cwd)
        return result


def _tool_fingerprint(steps: list[dict[str, Any]], graphhelm: Path | None,
                      repo: Path | None = None, env: dict[str, str] | None = None) -> dict[str, Any]:
    """Unattested existing execution stays runnable, but never supplies reusable proof."""
    repo = repo or Path.cwd()
    capture = _ExecutableCapture(dict(os.environ) if env is None else env)
    tools: dict[str, Any] = {"python": sys.version, "platform": sys.platform, "identityComplete": True,
                             "stepPrograms": [], "executables": [], "preparationSteps": []}
    try:
        tools["runnerPython"] = capture._identity(capture._program(sys.executable, repo), repo, python_selected=True)
        tools["graphhelmExecutable"] = capture._identity(capture._program(str(graphhelm), repo), repo) if graphhelm else None
        tools["graphhelm"] = tools["graphhelmExecutable"]["sha256"] if graphhelm else None
        for index, step in enumerate(steps):
            if step.get("identitySkipped"):
                tools["stepPrograms"] += [None]
                continue
            cwd = repo / step["cwd"]
            command = _effective_argv(repo, cwd, step["argv"], graphhelm)
            invocation = capture._program(command[0], cwd)
            tools["stepPrograms"].append(invocation)
            # The original selected command declares the role. A configured GraphHelm/Rust/npm
            # path named python.exe is not thereby authorized to run Python discovery.
            python_selected = (Path(step["argv"][0]).name.lower() in ("python", "python3", "python.exe", "python3.exe")
                               or os.path.normcase(step["argv"][0]) == os.path.normcase(sys.executable))
            tools["executables"].append(capture._identity(invocation, cwd, python_selected=python_selected))
            name = Path(command[0]).name.lower()
            if name in ("cargo", "cargo.exe"):
                rust = capture._rust(command, cwd)
                tools.setdefault("rust", {})[str(index)] = rust
                selection = command[1] if len(command) > 1 and command[1].startswith("+") else ""
                tools["rustc" + selection] = "0:" + rust["version"]
            elif name in ("node", "node.exe") and "node" not in tools:
                tools["node"] = "0:" + capture._discover([invocation, "--version"], cwd)
            if step["argv"][0] == "npx" and _needs_studio_preparation(repo, cwd):
                tools["preparationSteps"].append(index)
                npm = capture._program("npm", cwd)
                tools["executables"].append(capture._identity(npm, cwd))
                tools["executables"].append(capture._identity(capture._program("node", cwd), cwd))
        tools["compilerConfigs"] = capture.configs
    except _MissingExecutable:
        raise
    except (OSError, RuntimeError, ValueError) as exc:
        tools.update(identityComplete=False, identityReason=str(exc) if isinstance(exc, RuntimeError) else
                     "executable identity cannot be read or decoded", stepPrograms=[])
    return tools

def _fingerprint(repo: Path, steps: list[dict[str, Any]], graphhelm: Path | None, env: dict[str, str]) -> dict[str, Any]:
    """What a proof was taken against: committed code, tool versions and the environment."""
    tools = _tool_fingerprint(steps, graphhelm, repo, env)
    return {"tree": _git(repo, "rev-parse", "HEAD^{tree}"), "tools": tools,
            "toolsSha256": _sha(tools), "envSha256": _env_fingerprint(env)}


def _proof_key(fingerprint: dict[str, Any], step: dict[str, Any]) -> str:
    return _sha({"tree": fingerprint["tree"], "tools": fingerprint["toolsSha256"], "env": fingerprint["envSha256"],
                 "argv": step["argv"], "cwd": step["cwd"], "slot": step["slot"]})


def _reusable_results(output: Path) -> tuple[dict[str, dict[str, Any]], str | None]:
    """Passed results the previous run vouched for, keyed by proof key.

    Only entries the previous run marked `reusable` qualify: that mark is written after the run
    proved HEAD unchanged and the worktree clean. Anything unreadable reuses nothing.
    """
    if not output.is_file():
        return {}, None
    try:
        prior = json.loads(output.read_text(encoding="utf-8"))
    except (OSError, ValueError) as exc:
        return {}, f"previous report unreadable, rerunning everything: {exc}"
    if not isinstance(prior, dict) or prior.get("schema") != "graphhelm-reached-run/1":
        return {}, "previous report has another schema, rerunning everything"
    found: dict[str, dict[str, Any]] = {}
    for entry in prior.get("completed") or []:
        if (isinstance(entry, dict) and entry.get("reusable") is True and entry.get("returncode") == 0
                and isinstance(entry.get("proofKey"), str) and not entry.get("error") and not entry.get("zeroTests")):
            if _vitest_step(entry) and (not isinstance(entry.get("vitest"), dict)
                                        or entry["vitest"].get("passed", 0) + entry["vitest"].get("failed", 0) <= 0
                                        or entry["vitest"].get("failed") != 0 or entry["vitest"].get("errors") != 0):
                continue
            found[entry["proofKey"]] = entry
    return found, None


def _preflight(repo: Path, output: Path, steps: list[dict[str, Any]],
               min_free_gb: float | None, root: str | None = None) -> tuple[list[str], list[str]]:
    """Problems that would stop the proof midway, found before any step runs, and warnings.

    #1459: the build floor (10 GB with Cargo steps) is checked where the builds write, the slot
    root and CARGO_TARGET_DIR; the checkout and the report only need the small floor."""
    problems: list[str] = []
    warnings: list[str] = []
    cargo = any(Path(step["argv"][0]).name.lower() in ("cargo", "cargo.exe") for step in steps)
    small = min_free_gb if min_free_gb is not None else 1.0
    build = min_free_gb if min_free_gb is not None else (10.0 if cargo else 1.0)
    places = {"repository": (repo, small), "report": (output.parent, small)}
    target = os.environ.get("CARGO_TARGET_DIR")
    if cargo and root:
        places["slot root"] = (Path(root), build)
    if cargo and target:
        places["CARGO_TARGET_DIR"] = (Path(target), build)
    for label, (place, floor) in places.items():
        probe = place
        while not probe.exists() and probe.parent != probe:
            probe = probe.parent
        try:
            free = shutil.disk_usage(probe).free / 1e9
        except OSError as exc:
            problems.append(f"disk: cannot read free space for {label} ({place}): {exc}")
            continue
        if free < floor:
            problems.append(f"disk: {label} ({place}) has {free:.1f} GB free, needs {floor:.1f} GB")
    if target and not Path(target).is_absolute():
        problems.append(f"env: CARGO_TARGET_DIR={target} is relative; each step's cwd would get its own target")
    for name in ("GRAPHHELM_CLI", "GRAPHHELM_JOURNEY_TOOLCHAIN_PROJECT"):
        value = os.environ.get(name)
        if value and not Path(value).exists():
            problems.append(f"env: {name}={value} does not exist")
    for key in ("user.name", "user.email"):
        if subprocess.run(["git", "config", key], cwd=repo, capture_output=True, text=True, creationflags=NO_WINDOW).returncode != 0:
            warnings.append(f"identity: git {key} is not set; steps that commit will fail")
    return problems, warnings


def _summary(report: dict[str, Any], output: Path) -> str:
    """The short agent-facing answer: what passed, what failed, what is missing, where to look."""
    completed = report.get("completed", [])
    passed = [e for e in completed if e.get("returncode") == 0 and not e.get("error") and not e.get("zeroTests")]
    reused = [e for e in passed if e.get("reused")]
    unobserved = [e for e in completed if e not in passed and e.get("returncode") == 0 and e.get("error")
                  and not (isinstance(e.get("vitest"), dict) and (e["vitest"]["failed"] or e["vitest"]["errors"]))]
    failed = [e for e in completed if e not in passed and e not in unobserved]
    lines = [f"reached-tests: {report.get('status')} | passed {len(passed)} (reused {len(reused)})"
             f" | failed {len(failed)} | unobserved {len(unobserved)} | pending {len(report.get('pending', []))}"
             f" | {report.get('elapsedSeconds')}s"]
    for problem in report.get("preflight", {}).get("problems", []):
        lines.append(f"  BLOCKED {problem}")
    for entry in failed:
        lines.append(f"  FAILED step {entry['index']}: {' '.join(entry['argv'])} (exit {entry.get('returncode')}) -> {entry.get('log')}")
    for entry in unobserved:
        lines.append(f"  UNOBSERVED step {entry['index']}: {entry.get('error')} -> {entry.get('log')}")
    for index in report.get("pending", []):
        steps = report.get("plan", {}).get("steps", [])
        argv = ' '.join(steps[index]["argv"]) if index < len(steps) else "?"
        lines.append(f"  PENDING step {index}: {argv}")
    if report.get("error"):
        lines.append(f"  error: {report['error']}")
    lines.append(f"  report: {output}")
    return "\n".join(lines)


def run(args: argparse.Namespace) -> int:
    # #1188: the slot's build parallelism. Six when the caller does not choose; one root may go to
    # 16, and the jobs of every root building at the same time add up to at most 24 (LANES.md §3).
    jobs = str(getattr(args, "jobs", 6))
    # #1470: an older caller without `stage` queues as implement work, as before.
    stage = getattr(args, "stage", "implement")
    # The default sends no flag, so a CLI older than #1470 (which refuses `--stage`) still runs
    # every implement plan unchanged.
    stage_args = [] if stage == "implement" else ["--stage", stage]
    started = time.monotonic()
    deadline_unix_ms = time.time() * 1000.0 + args.budget_seconds * 1000.0
    repo = Path(args.repo).resolve()
    output = Path(args.output).resolve()
    if not math.isfinite(args.budget_seconds) or args.budget_seconds <= 0:
        raise ValueError("--budget-seconds must be finite and positive")
    try:
        output.relative_to(repo)
    except ValueError:
        pass
    else:
        raise ValueError("--output must be outside the repository")
    if output.exists() and output.is_symlink():
        raise ValueError("--output may not be a symlink")
    output.parent.mkdir(parents=True, exist_ok=True)
    logs = output.parent / (output.stem + "-steps")
    logs.mkdir(parents=True, exist_ok=True)
    report: dict[str, Any] = {"schema": "graphhelm-reached-run/1", "status": "incomplete", "completed": [], "pending": []}
    try:
        before = _git(repo, "rev-parse", "HEAD")
        requested = _git(repo, "rev-parse", args.head)
        if requested != before:
            raise RuntimeError(f"requested --head {args.head} resolves to {requested}, actual HEAD is {before}")
        status = _git(repo, "status", "--porcelain=v1")
        report["headBefore"] = before
        report["baseSha"] = _git(repo, "rev-parse", args.base)
        report["untracked"] = [line for line in status.splitlines() if line.startswith("??")]
        if status:
            raise RuntimeError("worktree is not clean; tracked or untracked source could affect the proof")
        configured_graphhelm = getattr(args, "graphhelm", None) or os.environ.get("GRAPHHELM_CLI")
        selected_graphhelm = _resolve_graphhelm(args) if configured_graphhelm else None
        if args.plan:
            plan = _selector(repo, args.base, args.head, logs / "selector.log", selected_graphhelm)
            report.update({"plan": plan, "status": "planned"})
            if selected_graphhelm is not None:
                report["graphhelmPath"] = str(selected_graphhelm)
        else:
            plan = _selector(repo, args.base, args.head, logs / "selector.log", selected_graphhelm)
            reason = _whole_reason(plan, args.allow_whole_package)
            report["wholePackageReason"] = args.allow_whole_package
            unsupported = plan.get("unsupported", [])
            if not isinstance(unsupported, list):
                raise RuntimeError("selector unsupported field must be a list")
            report["unsupported"] = unsupported
            if plan.get("unmapped"):
                raise RuntimeError("selector returned unmapped paths")
            if reason:
                raise RuntimeError(reason)
            steps, error = _steps(plan)
            if error:
                raise RuntimeError(error)
            report["plan"] = {"steps": steps, "unsupported": unsupported}
            report["pending"] = list(range(len(steps)))
            needs_graphhelm = any(
                step["slot"] or Path(step["argv"][0]).name.lower() in ("graphhelm", "graphhelm.exe")
                for step in steps
            )
            if needs_graphhelm and selected_graphhelm is None:
                selected_graphhelm = _resolve_graphhelm(args)
            if selected_graphhelm is not None:
                report["graphhelmPath"] = str(selected_graphhelm)
            if unsupported:
                reasons = "; ".join(
                    str(item.get("reason", item)) if isinstance(item, dict) else str(item)
                    for item in unsupported
                )
                raise RuntimeError(f"selector returned unsupported targets; no steps executed: {reasons}")
            browser_steps = [i for i, step in enumerate(steps) if step["observer"] == "browser"]
            skip_browser = browser_steps if not getattr(args, "include_browser", False) else []
            report["skipped"] = [{"index": index, "reason": "no observer"} for index in skip_browser]
            if browser_steps and not _browser_toolchain_ready():
                if getattr(args, "include_browser", False):
                    raise RuntimeError("--include-browser requires GRAPHHELM_JOURNEY_TOOLCHAIN_PROJECT with local @playwright/test")
            for index in skip_browser:
                report["pending"].remove(index)
            child_env = os.environ.copy()
            child_env.update({"CARGO_BUILD_JOBS": jobs, "RUST_TEST_THREADS": "2"})
            identity_steps = [{**step, "identitySkipped": index in skip_browser} for index, step in enumerate(steps)]
            report["fingerprint"] = _fingerprint(repo, identity_steps, selected_graphhelm, child_env)
            # #1892: the thread count is chosen after the fingerprint, so a pass proven at one thread
            # count stays reusable at another; the report names the count that actually ran.
            threads = getattr(args, "test_threads", None) or _test_threads(_free_memory_gb(), os.cpu_count())
            child_env["RUST_TEST_THREADS"] = str(threads)
            report["testThreads"] = threads
            print(f"[reached-tests] test threads: {threads}", file=sys.stderr, flush=True)
            identities = report["fingerprint"].get("tools", {})
            identity_complete = identities.get("identityComplete", True)
            keys = [_proof_key(report["fingerprint"], step) if identity_complete else None for step in steps]
            problems, warnings = _preflight(repo, output, steps, getattr(args, "min_free_gb", None),
                                             getattr(args, "root", None))
            report["preflight"] = {"problems": problems, "warnings": warnings}
            reused_seconds = 0.0
            reuse: dict[str, dict[str, Any]] = {}
            if identity_complete and getattr(args, "resume", True):
                reuse, note = _reusable_results(output)
                if note:
                    report["resumeNote"] = note
            for index in list(report["pending"]):
                prior = reuse.get(keys[index])
                if prior is None:
                    continue
                # Same tree, argv, cwd, tool versions and environment as a vouched pass: reuse it.
                report["pending"].remove(index)
                report["completed"].append({"index": index, "argv": steps[index]["argv"], "cwd": steps[index]["cwd"],
                                            "slot": steps[index]["slot"], "returncode": 0, "reused": True,
                                            "proofKey": keys[index], "log": prior.get("log"),
                                            "stepSeconds": prior.get("stepSeconds"), "vitest": prior.get("vitest")})
                reused_seconds += float(prior.get("stepSeconds") or 0.0)
            report["measure"] = {"plannedSteps": len(steps), "reusedSteps": len(steps) - len(report["pending"]),
                                 "reusedSecondsSaved": round(reused_seconds, 3)}
            queue: float | None = 0.0
            held: float | None = 0.0
            # Batch is the safe default for the official runner. Keep the fallback for
            # tests and embedders that construct an Args object directly.
            batch_mode = bool(getattr(args, "batch_slot", True))
            order = _execution_order(steps, batch_mode)
            work = [entry for entry in order if entry["index"] in report["pending"]]
            if problems and not getattr(args, "skip_preflight", False):
                report["status"] = "blocked"
                work = []
            keep_going, red = bool(getattr(args, "keep_going", False)), False
            while work:
                remaining_budget = args.budget_seconds - (time.monotonic() - started)
                if remaining_budget <= 0:
                    report["status"] = "budgetExceeded"
                    break
                entry = work.pop(0)
                group = [entry]
                if batch_mode and _batchable(entry):
                    while work and len(group) < 4 and _batchable(work[0]) and work[0]["cwd"] == entry["cwd"]:
                        group.append(work.pop(0))
                indices = [item["index"] for item in group]
                cwd = repo / entry["cwd"]
                logfile = logs / (f"batch-{indices[0]}.log" if len(group) > 1 else f"step-{indices[0]}.log")
                print(f"[reached-tests] steps {indices}: {' | '.join(' '.join(item['argv']) for item in group)}", file=sys.stderr, flush=True)
                commands = []
                for item in group:
                    if item["argv"][0] == "npx" and _needs_studio_preparation(repo, cwd) and item["index"] not in identities.get("preparationSteps", []):
                        identity_complete = False
                        identities["identityComplete"] = False
                        identities["identityReason"] = "Studio preparation appeared after identity capture; execution is not reusable"
                        report["fingerprint"]["toolsSha256"] = _sha(identities)
                        reused = [result for result in report["completed"] if result.get("reused")]
                        report["pending"].extend(result["index"] for result in reused)
                        report["completed"] = [result for result in report["completed"] if not result.get("reused")]
                        keys = [None for _ in steps]
                    command = _resolve_argv(repo, cwd, list(item["argv"]))
                    if _vitest_step(item):
                        artifact = logs / f"vitest-{item['index']}.json"
                        artifact.unlink(missing_ok=True)
                        command.extend(["--reporter=default", "--reporter=json", f"--outputFile.json={artifact}"])
                    command = _with_test_threads(_effective_argv(repo, cwd, command, selected_graphhelm), threads)
                    programs = identities.get("stepPrograms", [])
                    if identity_complete and programs:
                        command[0] = _slot_program(programs[item["index"]])
                    commands.append(command)
                is_batch = batch_mode and len(group) > 1 or batch_mode and _batchable(entry)
                if is_batch:
                    # The CLI receives original argv; its Rust worker selects the child cwd/target.
                    command = [str(selected_graphhelm), "--json", "workspace", "slot", "--root", args.root,
                               "--lane", args.lane, "--jobs", jobs, *stage_args, "--label", f"reached-batch-{indices[0]}",
                               "--max-wait", repr(remaining_budget / 60.0), "--", *commands[0]]
                    # Batch commands are carried by the opt-in environment, never by the CLI argv.
                    payload = {"schema": _BATCH_SCHEMA, "commands": commands,
                               "budgetSeconds": remaining_budget, "deadlineUnixMs": deadline_unix_ms,
                               "leaseSeconds": 30}
                elif entry["slot"]:
                    command = [str(selected_graphhelm), "--json", "workspace", "slot", "--root", args.root,
                               "--lane", args.lane, "--jobs", jobs, *stage_args, "--label", f"reached-step-{entry['index']}",
                               "--max-wait", repr(remaining_budget / 60.0), "--", *commands[0]]
                    payload = None
                else:
                    command = commands[0]
                    payload = None
                env = dict(child_env)
                env.pop("GRAPHHELM_SLOT_BATCH", None)
                if payload is not None:
                    env["GRAPHHELM_SLOT_BATCH"] = json.dumps(payload, separators=(",", ":"))
                step_started = time.monotonic()
                with logfile.open("w", encoding="utf-8") as stream:
                    proc = subprocess.run(command, cwd=cwd, stdout=stream, stderr=subprocess.STDOUT, text=True, env=env, creationflags=NO_WINDOW)
                step_seconds = round(time.monotonic() - step_started, 3)
                elapsed = time.monotonic() - started
                # #1892: name the slow step as it finishes, not only in the JSON report.
                print(f"[reached-tests] steps {indices}: {step_seconds:.1f}s (exit {proc.returncode}; "
                      f"total {elapsed:.1f}s)", file=sys.stderr, flush=True)
                if entry["slot"]:
                    if is_batch:
                        batch, q, h = _batch_envelope(logfile, proc.returncode)
                        parsed, error = _batch_result(batch, len(group), proc.returncode) if batch is not None else (None, _slot_refusal(logfile) or "missing or malformed batch telemetry")
                        if q is None or h is None:
                            queue = held = None
                        elif queue is not None and held is not None:
                            queue += q
                            held += h
                        if parsed is None:
                            report["error"] = error
                            report["status"] = "incomplete"
                            break
                        report.setdefault("batches", []).append({"indices": indices, **parsed,
                                                                  "log": str(logfile), "waitedSeconds": q,
                                                                  "heldSeconds": h})
                        completed_count = len(parsed["completed"])
                        for local, child in enumerate(parsed["completed"]):
                            index = indices[local]
                            print(f"[reached-tests]   step {index}: {child['elapsedSeconds']}s (exit {child['exitCode']})",
                                  file=sys.stderr, flush=True)
                            report["pending"].remove(index)
                            report["completed"].append({"index": index, "batchLocalIndex": local,
                                                         "argv": steps[index]["argv"], "cwd": steps[index]["cwd"], "slot": True,
                                                         "returncode": child["exitCode"], "elapsedSeconds": child["elapsedSeconds"],
                                                         "stepSeconds": child["elapsedSeconds"], "proofKey": keys[index],
                                                         "late": elapsed > args.budget_seconds,
                                                         "log": str(logfile), "tail": _tail(logfile)})
                        if parsed["stopReason"] == "childFailure" and keep_going:
                            red = True  # #1459: record the red, then run the batch's remaining steps
                            work[0:0] = group[completed_count:]
                            continue
                        if parsed["stopReason"] in {"childFailure", "spawnFailure"}:
                            report["status"] = "failed"
                            break
                        if parsed["remaining"]:
                            pending_entries = group[completed_count:]
                            work[0:0] = pending_entries
                            if parsed["stopReason"] == "deadline":
                                report["status"] = "budgetExceeded"
                                break
                            if completed_count == 0:
                                report["status"] = "incomplete"
                                report["error"] = "batch boundary made no progress"
                                break
                        continue
                    q, h = _slot_telemetry(logfile, command)
                    if q is None or h is None:
                        queue = held = None
                    elif queue is not None and held is not None:
                        queue += q
                        held += h
                index = entry["index"]
                tail = _tail(logfile)
                no_pytest_tests = command[1:3] == ["-m", "pytest"] and (proc.returncode == 5 or "no tests ran" in tail.lower())
                report["pending"].remove(index)
                result = {"index": index, "argv": entry["argv"], "cwd": entry["cwd"], "slot": entry["slot"],
                          "returncode": proc.returncode, "late": elapsed > args.budget_seconds,
                          "stepSeconds": step_seconds, "proofKey": keys[index], "log": str(logfile), "tail": tail}
                if proc.returncode == 0 and re.search(r"^UNOBSERVED:", tail, re.M):
                    # A step that could not run its observer is neither a pass nor a red.
                    result["error"] = (re.findall(r"^UNOBSERVED:.*$", tail, re.M) or ["UNOBSERVED"])[-1].strip()
                    result["unobserved"] = True
                if no_pytest_tests:
                    result["returncode"] = proc.returncode or 1
                    result["error"] = "pytest collected no tests"
                zero_tests = "-m" in entry["argv"] and "unittest" in entry["argv"] and bool(_UNITTEST_ZERO.search(tail))
                if zero_tests:
                    result["zeroTests"] = True
                if _vitest_step(entry):
                    evidence, evidence_error = _vitest_evidence(logs / f"vitest-{index}.json")
                    result["vitest"] = evidence
                    if evidence_error:
                        result["error"] = evidence_error
                    elif evidence["failed"] or evidence["errors"]:
                        result["error"] = "Vitest reported test or suite failures"
                    elif evidence["passed"] == 0 or not evidence["success"]:
                        result["error"] = "Vitest executed no passing tests or reported unsuccessful"
                report["completed"].append(result)
                if zero_tests:
                    report["status"] = "incomplete"
                    report["error"] = f"step {index} unittest discovered zero tests"
                    break
                vitest_red = bool(result.get("error") and result.get("vitest") and result["vitest"]["failed"])
                if keep_going and (result["returncode"] or vitest_red):
                    red = True  # #1459: a red is recorded and the next step still runs
                    if vitest_red:
                        report["error"] = f"step {index} {result['error']}"
                elif result["returncode"]:
                    report["status"] = "failed"
                    break
                elif result.get("error") and not result.get("unobserved"):
                    report["status"] = "failed" if vitest_red else "incomplete"
                    report["error"] = f"step {index} {result['error']}"
                    break
                if elapsed > args.budget_seconds:
                    report["status"] = "budgetExceeded"
                    break
            else:
                if report["status"] != "blocked":
                    unobserved = any(e.get("unobserved") for e in report["completed"])
                    report["status"] = ("failed" if red else "incomplete" if unobserved or report["pending"]
                                        else "passed")
            if red:
                report["status"] = "failed"  # a later stop never hides a red that already ran
            if not identity_complete:
                report["error"] = identities.get("identityReason", "executable identity is not reusable")
                if report["status"] == "passed":
                    report["status"] = "incomplete"
                for result in report["completed"]:
                    result.pop("proofKey", None)
                    result.pop("reusable", None)
            report["queueSeconds"] = queue
            report["heldSeconds"] = held
        after = _git(repo, "rev-parse", "HEAD")
        final_status = _git(repo, "status", "--porcelain=v1")
        report["headAfter"] = after
        if before != after:
            report["status"] = "incomplete"
            report["error"] = "HEAD changed while running"
        elif final_status:
            report["status"] = "incomplete"
            report["error"] = "worktree became dirty while running"
        elif time.monotonic() - started > args.budget_seconds and report["status"] in ("passed", "planned"):
            report["status"] = "budgetExceeded"
        if before == after and not final_status and report.get("fingerprint", {}).get("tools", {}).get("identityComplete", True) and all(e.get("proofKey") for e in report["completed"]):
            # Only now, with HEAD unchanged and the worktree clean, may a pass be reused later.
            for entry in report["completed"]:
                if entry.get("returncode") == 0 and not entry.get("error") and not entry.get("zeroTests") and entry.get("proofKey"):
                    entry["reusable"] = True
    except Exception as exc:
        report["error"] = str(exc)
        report["status"] = "incomplete"
    report["elapsedSeconds"] = round(time.monotonic() - started, 3)
    # #1516: the receipt splits its wall time into waiting for the slot and working, so a reader of
    # the delivery durations can tell a queued run from a slow one. `slotWaitSeconds` is the time
    # spent queued for the build slot (null when the slot did not report it), `runSeconds` the rest.
    slot_wait = report.get("queueSeconds")
    report["slotWaitSeconds"] = None if slot_wait is None else round(slot_wait, 3)
    report["runSeconds"] = (None if slot_wait is None
                            else round(max(0.0, report["elapsedSeconds"] - slot_wait), 3))
    output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    if getattr(args, "json", False):
        print(json.dumps(report, indent=2))
    else:
        print(_summary(report, output))
    return 0 if report["status"] in ("passed", "planned") else 1


def default_output(repo: Path, lane: str) -> Path:
    """Per-worktree, per-lane report path outside the repository, so `--output` is optional (#1538)."""
    safe = re.sub(r"[^A-Za-z0-9._-]", "_", f"{repo.resolve().name}-{lane}")
    return Path(tempfile.gettempdir()) / "graphhelm-reached" / f"{safe}.json"


def default_lane(repo: Path) -> str:
    """Lane for a run that names none: one per worktree, so lanes never share a build-directory cap (#1555)."""
    name = re.sub(r"[^A-Za-z0-9._-]", "_", repo.resolve().name)[:48] or "worktree"
    return f"reached-{name}"


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    ap.add_argument("--repo", default=".")
    ap.add_argument("--base", default="origin/main")
    ap.add_argument("--head", default="HEAD")
    ap.add_argument("--root", default="D:/gh")
    ap.add_argument("--lane", help="build-slot lane (default: reached-<worktree directory name>)")
    ap.add_argument("--graphhelm", help="GraphHelm executable; overrides GRAPHHELM_CLI and PATH")
    ap.add_argument("--output", help="report path, outside the repository "
                    "(default: <system temp>/graphhelm-reached/<repo dir>-<lane>.json)")
    # #1459: 180 s ended almost every first run `budgetExceeded` after one step, because the budget
    # includes waiting for the shared slot; 900 s fits a warm plan plus an ordinary queue.
    ap.add_argument("--budget-seconds", type=float, default=900)
    ap.add_argument("--jobs", type=int, default=6,
                    help="cargo build jobs for every slot command (1..32; default 6; one root up to 16)")
    ap.add_argument("--test-threads", type=int,
                    help="libtest threads for every cargo test step (1..64; default: from free memory and cores, floor 2)")
    ap.add_argument("--allow-whole-package")
    # #1470: a review or merge run is admitted to the build slot before waiting implement plans.
    ap.add_argument("--stage", choices=("implement", "review", "merge"), default="implement",
                    help="delivery step this run serves; review and merge jobs are served first")
    ap.add_argument("--include-browser", action="store_true",
                    help="allow selector steps marked observer=browser when local Playwright is installed")
    batch = ap.add_mutually_exclusive_group()
    batch.add_argument("--batch-slot", dest="batch_slot", action="store_true",
                       help="use the versioned serial Cargo slot batch protocol (default)")
    batch.add_argument("--no-batch-slot", dest="batch_slot", action="store_false",
                       help="run each Cargo command in its own slot invocation")
    ap.set_defaults(batch_slot=True)
    ap.add_argument("--plan", action="store_true")
    ap.add_argument("--no-resume", dest="resume", action="store_false",
                    help="ignore passes recorded in --output and run every step")
    ap.add_argument("--min-free-gb", type=float,
                    help="free space required before any step runs (default 10 with Cargo steps, else 1)")
    ap.add_argument("--skip-preflight", action="store_true",
                    help="run even when preflight finds a problem; the problems stay in the report")
    ap.add_argument("--json", action="store_true", help="print the full report instead of the short summary")
    ap.add_argument("--keep-going", action="store_true",
                    help="run every step after a failed one and report every red in one pass (status stays failed)")
    args = ap.parse_args(argv)
    if not 1 <= args.jobs <= 32:
        ap.error("--jobs must be between 1 and 32")
    if args.test_threads is not None and not 1 <= args.test_threads <= 64:
        ap.error("--test-threads must be between 1 and 64")
    if not args.lane:
        args.lane = default_lane(Path(args.repo))
    if not args.output:
        args.output = str(default_output(Path(args.repo), args.lane))
    return run(args)


if __name__ == "__main__":
    raise SystemExit(main())
