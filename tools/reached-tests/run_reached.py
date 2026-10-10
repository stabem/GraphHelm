#!/usr/bin/env python3
"""Run the reached-test plan with a short, per-step build-slot lease.

The selector owns reachability. This runner owns only orchestration: every Cargo
step gets its own slot invocation, while format, Python, and Studio steps run
directly. No command is passed through a shell. Whole-package plans are expanded
by the selector into explicit Cargo targets; targets requiring features are
reported as unsupported and remain incomplete rather than being silently skipped.
"""
from __future__ import annotations

import argparse
import json
import math
import os
import shutil
import subprocess
import sys
import time
import re
from pathlib import Path
from typing import Any


SELECTOR = Path(__file__).with_name("reached_tests.py")
MAX_CAPTURE = 4000
_SHELL_MARKERS = ("&&", "||", "|", ">", "<", "`", "$(")
_SHELL_EXECUTABLES = {"sh", "bash", "cmd", "cmd.exe", "powershell", "powershell.exe", "pwsh", "pwsh.exe"}
_CARGO_TEST_TARGET_FLAGS = ("--test", "--bin", "--lib", "--doc", "--example")
_UNITTEST_ZERO = re.compile(r"\bRan\s+0\s+tests?\b")
_BATCH_SCHEMA = "graphhelm.slot-batch/1"
_BATCH_RESULT_SCHEMA = "graphhelm.slot-batch-result/1"
_BATCH_STOP_REASONS = {"exhausted", "leaseBoundary", "deadline", "childFailure", "spawnFailure"}


def _git(repo: Path, *args: str) -> str:
    return subprocess.run(["git", *args], cwd=repo, check=True, capture_output=True, text=True).stdout.strip()


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
    env = os.environ.copy()
    if graphhelm is not None:
        env["GRAPHHELM_CLI"] = str(graphhelm)
        env["PATH"] = str(graphhelm.parent) + os.pathsep + env.get("PATH", "")
    proc = subprocess.run(
        [sys.executable, str(SELECTOR), "--repo", str(repo), "--base", base, "--head", head, "--json"],
        cwd=repo, capture_output=True, text=True, env=env,
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
        if step["argv"][0].lower() in _SHELL_EXECUTABLES or any(
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


def _resolve_argv(repo: Path, cwd: Path, argv: list[str]) -> list[str]:
    """Resolve local Node package entrypoints without npx downloading anything."""
    if not argv or argv[0] != "npx":
        return argv
    if len(argv) < 2 or cwd != repo / "apps/studio":
        raise RuntimeError("npx is allowed only for a local Studio tool")
    package = argv[1]
    entries = {"vitest": repo / "apps/studio/node_modules/vitest/vitest.mjs",
               "tsc": repo / "apps/studio/node_modules/typescript/bin/tsc"}
    entry = entries.get(package)
    if entry is None or not entry.is_file():
        raise RuntimeError(f"local Studio tool missing: {package}; run npm ci in apps/studio")
    return ["node", str(entry), *argv[2:]]


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


def run(args: argparse.Namespace) -> int:
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
            if browser_steps and not getattr(args, "include_browser", False):
                raise RuntimeError("browser observer steps require --include-browser; they remain pending")
            if browser_steps and not _browser_toolchain_ready():
                raise RuntimeError("--include-browser requires GRAPHHELM_JOURNEY_TOOLCHAIN_PROJECT with local @playwright/test")
            queue: float | None = 0.0
            held: float | None = 0.0
            batch_mode = bool(getattr(args, "batch_slot", False))
            order = _execution_order(steps, batch_mode)
            work = list(order)
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
                    if command and command[0] in ("python", "python3"):
                        command[0] = sys.executable
                    if selected_graphhelm is not None and command and Path(command[0]).name.lower() in ("graphhelm", "graphhelm.exe"):
                        command[0] = str(selected_graphhelm)
                    commands.append(command)
                is_batch = batch_mode and len(group) > 1 or batch_mode and _batchable(entry)
                if is_batch:
                    # The CLI receives original argv; its Rust worker selects the child cwd/target.
                    command = [str(selected_graphhelm), "--json", "workspace", "slot", "--root", args.root,
                               "--lane", args.lane, "--jobs", "6", "--label", f"reached-batch-{indices[0]}",
                               "--max-wait", repr(remaining_budget / 60.0), "--", *commands[0]]
                    # Batch commands are carried by the opt-in environment, never by the CLI argv.
                    payload = {"schema": _BATCH_SCHEMA, "commands": commands,
                               "budgetSeconds": remaining_budget, "deadlineUnixMs": deadline_unix_ms,
                               "leaseSeconds": 30}
                elif entry["slot"]:
                    command = [str(selected_graphhelm), "--json", "workspace", "slot", "--root", args.root,
                               "--lane", args.lane, "--jobs", "6", "--label", f"reached-step-{entry['index']}",
                               "--max-wait", repr(remaining_budget / 60.0), "--", *commands[0]]
                    payload = None
                else:
                    command = commands[0]
                    payload = None
                env = os.environ.copy()
                env.update({"CARGO_BUILD_JOBS": "6", "RUST_TEST_THREADS": "2"})
                env.pop("GRAPHHELM_SLOT_BATCH", None)
                if payload is not None:
                    env["GRAPHHELM_SLOT_BATCH"] = json.dumps(payload, separators=(",", ":"))
                with logfile.open("w", encoding="utf-8") as stream:
                    proc = subprocess.run(command, cwd=cwd, stdout=stream, stderr=subprocess.STDOUT, text=True, env=env)
                elapsed = time.monotonic() - started
                if entry["slot"]:
                    if is_batch:
                        batch, q, h = _batch_envelope(logfile, proc.returncode)
                        parsed, error = _batch_result(batch, len(group), proc.returncode) if batch is not None else (None, "missing or malformed batch telemetry")
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
                                                         "late": elapsed > args.budget_seconds,
                                                         "log": str(logfile), "tail": _tail(logfile)})
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
                          "log": str(logfile), "tail": tail}
                if no_pytest_tests:
                    result["returncode"] = proc.returncode or 1
                    result["error"] = "pytest collected no tests"
                zero_tests = "-m" in entry["argv"] and "unittest" in entry["argv"] and bool(_UNITTEST_ZERO.search(tail))
                if zero_tests:
                    result["zeroTests"] = True
                report["completed"].append(result)
                if zero_tests:
                    report["status"] = "incomplete"
                    report["error"] = f"step {index} unittest discovered zero tests"
                    break
                if result["returncode"]:
                    report["status"] = "failed"
                    break
                if elapsed > args.budget_seconds:
                    report["status"] = "budgetExceeded"
                    break
            else:
                report["status"] = "passed" if not report["pending"] else "incomplete"
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
    except Exception as exc:
        report["error"] = str(exc)
        report["status"] = "incomplete"
    report["elapsedSeconds"] = round(time.monotonic() - started, 3)
    output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2))
    return 0 if report["status"] in ("passed", "planned") else 1


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    ap.add_argument("--repo", default=".")
    ap.add_argument("--base", default="origin/main")
    ap.add_argument("--head", default="HEAD")
    ap.add_argument("--root", default="D:/gh")
    ap.add_argument("--lane", default="reached-fast-feedback")
    ap.add_argument("--graphhelm", help="GraphHelm executable; overrides GRAPHHELM_CLI and PATH")
    ap.add_argument("--output", required=True)
    ap.add_argument("--budget-seconds", type=float, default=180)
    ap.add_argument("--allow-whole-package")
    ap.add_argument("--include-browser", action="store_true",
                    help="allow selector steps marked observer=browser when local Playwright is installed")
    ap.add_argument("--batch-slot", action="store_true",
                    help="opt in to the versioned serial Cargo slot batch protocol")
    ap.add_argument("--plan", action="store_true")
    return run(ap.parse_args(argv))


if __name__ == "__main__":
    raise SystemExit(main())
