#!/usr/bin/env python3
"""Run the reached-test plan with a short, per-step build-slot lease.

The selector owns reachability. This runner owns only orchestration: every Cargo
step gets its own slot invocation, while format, Python, and Studio steps run
directly. No command is passed through a shell.
"""
from __future__ import annotations

import argparse
import json
import math
import os
import subprocess
import sys
import time
from pathlib import Path
from typing import Any


SELECTOR = Path(__file__).with_name("reached_tests.py")
MAX_CAPTURE = 4000


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


def _selector(repo: Path, base: str, head: str, log: Path) -> dict[str, Any]:
    proc = subprocess.run(
        [sys.executable, str(SELECTOR), "--repo", str(repo), "--base", base, "--head", head, "--json"],
        cwd=repo, capture_output=True, text=True,
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
    if command[:4] != ["graphhelm", "--json", "workspace", "slot"]:
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


def run(args: argparse.Namespace) -> int:
    started = time.monotonic()
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
        if args.plan:
            plan = _selector(repo, args.base, args.head, logs / "selector.log")
            report.update({"plan": plan, "status": "planned"})
        else:
            plan = _selector(repo, args.base, args.head, logs / "selector.log")
            reason = _whole_reason(plan, args.allow_whole_package)
            report["wholePackageReason"] = args.allow_whole_package
            if plan.get("unmapped"):
                raise RuntimeError("selector returned unmapped paths")
            if reason:
                raise RuntimeError(reason)
            steps, error = _steps(plan)
            if error:
                raise RuntimeError(error)
            report["plan"] = {"steps": steps}
            report["pending"] = list(range(len(steps)))
            browser_steps = [i for i, step in enumerate(steps) if step["observer"] == "browser"]
            if browser_steps and not getattr(args, "include_browser", False):
                raise RuntimeError("browser observer steps require --include-browser; they remain pending")
            if browser_steps and not _browser_toolchain_ready():
                raise RuntimeError("--include-browser requires GRAPHHELM_JOURNEY_TOOLCHAIN_PROJECT with local @playwright/test")
            queue: float | None = 0.0
            held: float | None = 0.0
            for index, step in enumerate(steps):
                remaining = args.budget_seconds - (time.monotonic() - started)
                if remaining <= 0:
                    report["status"] = "budgetExceeded"
                    break
                cwd = repo / step["cwd"]
                argv = list(step["argv"])
                command = _resolve_argv(repo, cwd, argv)
                if command and command[0] in ("python", "python3"):
                    command[0] = sys.executable
                if step["slot"]:
                    command = ["graphhelm", "--json", "workspace", "slot", "--root", args.root,
                               "--lane", args.lane, "--jobs", "6", "--label", f"reached-step-{index}",
                               "--max-wait", repr(remaining / 60.0), "--", *command]
                logfile = logs / f"step-{index}.log"
                print(f"[reached-tests] step {index + 1}/{len(steps)}: {' '.join(argv)}", file=sys.stderr, flush=True)
                env = os.environ.copy()
                env.update({"CARGO_BUILD_JOBS": "6", "RUST_TEST_THREADS": "2"})
                with logfile.open("w", encoding="utf-8") as stream:
                    proc = subprocess.run(command, cwd=cwd, stdout=stream, stderr=subprocess.STDOUT, text=True, env=env)
                no_pytest_tests = command[1:3] == ["-m", "pytest"] and (
                    proc.returncode == 5 or "no tests ran" in _tail(logfile).lower())
                if step["slot"]:
                    q, h = _slot_telemetry(logfile, command)
                    if q is None or h is None:
                        queue = None
                        held = None
                    elif queue is not None and held is not None:
                        queue += q
                        held += h
                report["pending"].remove(index)
                entry = {"index": index, "argv": argv, "cwd": step["cwd"], "slot": step["slot"],
                         "returncode": proc.returncode, "late": time.monotonic() - started > args.budget_seconds,
                         "log": str(logfile), "tail": _tail(logfile)}
                if no_pytest_tests:
                    entry["returncode"] = proc.returncode or 1
                    entry["error"] = "pytest collected no tests"
                report["completed"].append(entry)
                if entry["returncode"]:
                    report["status"] = "failed"
                    break
                if entry["late"]:
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
    ap.add_argument("--output", required=True)
    ap.add_argument("--budget-seconds", type=float, default=180)
    ap.add_argument("--allow-whole-package")
    ap.add_argument("--include-browser", action="store_true",
                    help="allow selector steps marked observer=browser when local Playwright is installed")
    ap.add_argument("--plan", action="store_true")
    return run(ap.parse_args(argv))


if __name__ == "__main__":
    raise SystemExit(main())
