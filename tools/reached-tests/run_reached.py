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
        return candidate.resolve()
    found = shutil.which("graphhelm")
    if not found:
        raise RuntimeError("graphhelm executable was not found on PATH")
    candidate = Path(found).resolve()
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
        if cwd not in (".", "apps/studio"):
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
    print(f"[reached-tests] apps/studio/node_modules is absent: npm ci in {studio}", file=sys.stderr, flush=True)
    try:
        proc = subprocess.run([npm, "ci"], cwd=studio, capture_output=True, text=True,
                              timeout=STUDIO_INSTALL_TIMEOUT, creationflags=NO_WINDOW)
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise RuntimeError(f"npm ci in apps/studio failed ({type(exc).__name__})") from exc
    if proc.returncode != 0:
        raise RuntimeError(f"npm ci in apps/studio failed (exit {proc.returncode}): {(proc.stderr or '')[-400:]}")


def _resolve_argv(repo: Path, cwd: Path, argv: list[str]) -> list[str]:
    """Resolve local Node package entrypoints without npx downloading anything."""
    if not argv or argv[0] != "npx":
        return argv
    if len(argv) < 2 or cwd != repo / "apps/studio":
        raise RuntimeError("npx is allowed only for a local Studio tool")
    package = argv[1]
    _install_absent_studio(repo, repo / "apps/studio")
    problem = studio_install_problem(repo / "apps/studio")
    if problem:
        raise RuntimeError(
            f"apps/studio install is not whole ({problem}); run `npm ci` in apps/studio. "
            "A running Studio may lock files in node_modules (EPERM): stop it first, or rename the locked .node file aside. "
            "Not repaired automatically.")
    entries = {"vitest": repo / "apps/studio/node_modules/vitest/vitest.mjs",
               "tsc": repo / "apps/studio/node_modules/typescript/bin/tsc"}
    entry = entries.get(package)
    if entry is None or not entry.is_file():
        raise RuntimeError(f"local Studio tool missing: {package}; run npm ci in apps/studio")
    return ["node", str(entry), *argv[2:]]


def _vitest_step(step: dict[str, Any]) -> bool:
    return step["cwd"] == "apps/studio" and step["argv"][:2] == ["npx", "vitest"]


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
    return _sha(sorted((k, v) for k, v in env.items()
                       if k.startswith(_ENV_PREFIXES) and k not in _ENV_IGNORED))


def _version(argv: list[str]) -> str:
    try:
        proc = subprocess.run(argv, capture_output=True, text=True, timeout=60, creationflags=NO_WINDOW)
    except (OSError, subprocess.SubprocessError) as exc:
        return f"unavailable: {type(exc).__name__}"
    return f"{proc.returncode}:{proc.stdout.strip()}"


def _file_digest(path: Path | None) -> str | None:
    if path is None:
        return None
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _tool_fingerprint(steps: list[dict[str, Any]], graphhelm: Path | None) -> dict[str, Any]:
    """Versions of every tool a step reaches; an unreadable version is recorded, never guessed."""
    tools: dict[str, Any] = {"python": sys.version, "platform": sys.platform, "graphhelm": _file_digest(graphhelm)}
    for step in steps:
        head = Path(step["argv"][0]).name.lower()
        if head in ("cargo", "cargo.exe"):
            toolchain = [step["argv"][1]] if len(step["argv"]) > 1 and step["argv"][1].startswith("+") else []
            key = "rustc" + (toolchain[0] if toolchain else "")
            if key not in tools:
                tools[key] = _version(["rustc", *toolchain, "-Vv"])
        elif head in ("npx", "npx.cmd", "npm", "npm.cmd", "node", "node.exe") and "node" not in tools:
            tools["node"] = _version(["node", "--version"])
    return tools


def _fingerprint(repo: Path, steps: list[dict[str, Any]], graphhelm: Path | None, env: dict[str, str]) -> dict[str, Any]:
    """What a proof was taken against: committed code, tool versions and the environment."""
    tools = _tool_fingerprint(steps, graphhelm)
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
            report["fingerprint"] = _fingerprint(repo, steps, selected_graphhelm, child_env)
            keys = [_proof_key(report["fingerprint"], step) for step in steps]
            problems, warnings = _preflight(repo, output, steps, getattr(args, "min_free_gb", None),
                                             getattr(args, "root", None))
            report["preflight"] = {"problems": problems, "warnings": warnings}
            reused_seconds = 0.0
            reuse: dict[str, dict[str, Any]] = {}
            if getattr(args, "resume", True):
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
                    command = _resolve_argv(repo, cwd, list(item["argv"]))
                    if _vitest_step(item):
                        artifact = logs / f"vitest-{item['index']}.json"
                        artifact.unlink(missing_ok=True)
                        command.extend(["--reporter=default", "--reporter=json", f"--outputFile.json={artifact}"])
                    if command and command[0] in ("python", "python3"):
                        command[0] = sys.executable
                    if selected_graphhelm is not None and command and Path(command[0]).name.lower() in ("graphhelm", "graphhelm.exe"):
                        command[0] = str(selected_graphhelm)
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
        if before == after and not final_status:
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
    if not args.lane:
        args.lane = default_lane(Path(args.repo))
    if not args.output:
        args.output = str(default_output(Path(args.repo), args.lane))
    return run(args)


if __name__ == "__main__":
    raise SystemExit(main())
