"""Browser journey observer: run `playwright test` and turn its JSON report into proof lines.

`npx playwright test --reporter=json` (npm package `@playwright/test`, Apache-2.0) drives a real
browser with no model and no AI cost. It exits 0 when every test passed and 1 when a test failed;
any other exit, a missing runner, or a missing or malformed report is `OBSERVER_MISSING`: the
journey was not seen, so nothing may be claimed about it.

Prints one JSON object: {"verdict": "passed"|"failed"|"observer_missing", "exitCode", "evidence": [...]}.
`evidence` is ready for the `evidence` list of a `keel.proof` signal (docs/keel/RECORDS.md).
Exit code of this script: 0 passed, 1 failed, 2 observer missing.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys

DEFAULT_COMMAND = "npx --no-install playwright test --reporter=json"
REPORT = Path(".graphhelm") / "playwright-report.json"
OBSERVED = {0: "passed", 1: "failed"}


def _split(command: str) -> list[str]:
    """Split a command line; on Windows keep backslashes in paths (posix shlex would eat them)."""
    if os.name != "nt":
        return shlex.split(command)
    return [part[1:-1] if len(part) > 1 and part[0] == part[-1] and part[0] in "\"'" else part
            for part in shlex.split(command, posix=False)]


def _tests(suites: list, path: list[str], out: list[str]) -> None:
    """One line per test (`status: title path`), walking nested suites; capped at 50."""
    for suite in suites:
        here = path + ([suite["title"]] if suite.get("title") else [])
        for spec in suite.get("specs", []):
            for test in spec.get("tests", []):
                if len(out) < 50:
                    out.append(f"{test.get('status', '?')}: {' > '.join(here + [spec.get('title', '?')])}")
        _tests(suite.get("suites", []), here, out)


def observe(project: Path, command: str, timeout: int) -> dict:
    argv = _split(command)
    if shutil.which(argv[0]) is None:
        return {"verdict": "observer_missing", "exitCode": None,
                "evidence": [f"OBSERVER_MISSING: `{argv[0]}` is not on PATH"]}
    report_path = project / REPORT
    # A report left by an earlier run must never stand in for this one.
    report_path.unlink(missing_ok=True)
    report_path.parent.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, PLAYWRIGHT_JSON_OUTPUT_NAME=str(report_path.resolve()))
    try:
        result = subprocess.run(argv, cwd=project, capture_output=True, text=True,
                                timeout=timeout, env=env)
    except subprocess.TimeoutExpired:
        return {"verdict": "observer_missing", "exitCode": None,
                "evidence": [f"OBSERVER_MISSING: `{command}` did not finish in {timeout}s"]}
    verdict = OBSERVED.get(result.returncode, "observer_missing")
    evidence = [f"`{command}` in {project} exited {result.returncode}"]
    if report_path.is_file():
        raw = report_path.read_bytes()
        evidence.append(f"{REPORT.as_posix()} sha256:{hashlib.sha256(raw).hexdigest()}")
        problem = None
        try:
            report = json.loads(raw)
            stats = report.get("stats") if isinstance(report, dict) else None
            if not isinstance(stats, dict) or not isinstance(report.get("suites"), list):
                problem = "is not a Playwright JSON report"
            else:
                ran = sum(int(stats.get(key, 0)) for key in ("expected", "unexpected", "flaky"))
                failed = int(stats.get("unexpected", 0))
                if ran == 0:
                    problem = "lists no tests that ran"
                elif verdict == "passed" and failed:
                    problem = f"counts {failed} unexpected results, but playwright exited 0"
                elif verdict == "failed" and not failed:
                    problem = "counts no unexpected results, but playwright exited 1"
                else:
                    lines: list[str] = []
                    _tests(report["suites"], [], lines)
                    evidence.extend(lines)
        except (ValueError, TypeError):
            problem = "is not JSON"
        if problem:
            verdict = "observer_missing"
            evidence.append(f"{REPORT.as_posix()} {problem}")
    elif verdict != "observer_missing":
        verdict = "observer_missing"
        evidence.append(f"no {REPORT.as_posix()} was written, so no test was observed")
    if verdict == "observer_missing":
        tail = (result.stderr or result.stdout).strip().splitlines()[-1:] or ["no output"]
        evidence.append(f"OBSERVER_MISSING: playwright exit {result.returncode}: {tail[0][:300]}")
    return {"verdict": verdict, "exitCode": result.returncode, "evidence": evidence}


def _shots(suites: list, out: dict) -> None:
    """Map each spec title to its last test's status and screenshot path (a step-named attachment wins)."""
    for suite in suites:
        for spec in suite.get("specs", []):
            for test in spec.get("tests", []):
                results = test.get("results") or [{}]
                attachments = [a for a in results[-1].get("attachments") or []
                               if isinstance(a, dict) and a.get("contentType") == "image/png" and a.get("path")]
                named = [a for a in attachments if a.get("name") == spec.get("title")]
                auto = [a for a in attachments if a.get("name") == "screenshot"]
                chosen = (named or auto or [None])[0]
                out[spec.get("title")] = {"status": test.get("status", "?"),
                                          "path": chosen["path"] if chosen else None}
        _shots(suite.get("suites", []), out)


def _run_cli(graphhelm: str, tail: list[str], base: list[str]) -> str | None:
    """Run one `graphhelm journey ...`; None on success, else the reason it was refused."""
    try:
        result = subprocess.run(_split(graphhelm) + tail[:2] + base + tail[2:],
                                capture_output=True, text=True, timeout=120)
    except FileNotFoundError:
        return "graphhelm not found"
    except subprocess.TimeoutExpired:
        return "capture refused: timed out"
    try:
        envelope = json.loads(result.stdout)
    except ValueError:
        envelope = {}
    if result.returncode == 0 and isinstance(envelope, dict) and envelope.get("ok") is True:
        return None
    diagnostics = envelope.get("diagnostics") if isinstance(envelope, dict) else None
    message = None
    if isinstance(diagnostics, list) and diagnostics and isinstance(diagnostics[0], dict):
        message = diagnostics[0].get("message")
    return f"capture refused: {message or f'exit {result.returncode}'}"


def record_journey(args, project: Path, steps: list[str], outcome: dict) -> None:
    shots: dict = {}
    report_path = project / REPORT
    try:
        report = json.loads(report_path.read_bytes())
        if isinstance(report, dict) and isinstance(report.get("suites"), list):
            _shots(report["suites"], shots)
    except (OSError, ValueError):
        pass
    base = ["--events", args.events, "--execution", args.execution, "--keyring", args.keyring,
            "--key-id", args.key_id, "--project", str(project), "--contract", args.journey]
    captured: list[str] = []
    missing: dict[str, str] = {}
    walked: list[list[str]] = []
    for step in steps:
        shot = shots.get(step)
        if shot is None:
            missing[step] = "no test titled with the step id"
        elif shot["status"] != "expected":
            missing[step] = f"test {shot['status']}"
        elif shot["path"] is None:
            missing[step] = "no screenshot"
        else:
            reason = _run_cli(args.graphhelm, ["journey", "capture", "--step", step, "--image", shot["path"]], base)
            if reason:
                missing[step] = reason
            else:
                captured.append(step)
                outcome["evidence"].append(f"journey {args.journey}: captured {step}")
    for a, b in zip(steps, steps[1:]):
        if a in captured and b in captured:
            if _run_cli(args.graphhelm, ["journey", "walked", "--from", a, "--to", b], base) is None:
                walked.append([a, b])
                outcome["evidence"].append(f"journey {args.journey}: walked {a} -> {b}")
    outcome["journey"] = {"contractId": args.journey, "captured": captured, "walked": walked, "missing": missing}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--project", default=".", help="directory holding playwright.config and tests")
    parser.add_argument("--command",
                        default=os.environ.get("GRAPHHELM_PLAYWRIGHT_COMMAND", DEFAULT_COMMAND))
    parser.add_argument("--timeout", type=int, default=1800)
    parser.add_argument("--journey", help="journey contract id: record a capture per step and a walk per arrow")
    parser.add_argument("--events", help="Runtime events dir (with --journey)")
    parser.add_argument("--execution", help="execution id (with --journey)")
    parser.add_argument("--keyring", help="keyring path (with --journey)")
    parser.add_argument("--key-id", help="signing key id (with --journey)")
    parser.add_argument("--graphhelm", default=os.environ.get("GRAPHHELM_BIN", "graphhelm"))
    args = parser.parse_args()
    steps: list[str] = []
    if args.journey:
        absent = [f"--{f.replace('_', '-')}" for f in ("events", "execution", "keyring", "key_id")
                  if not getattr(args, f)]
        if absent:
            parser.error(f"--journey needs {', '.join(absent)}")
        if not re.fullmatch(r"[a-z0-9][a-z0-9._-]{0,127}", args.journey) or ".." in args.journey:
            parser.error(f"--journey id {args.journey!r} is not a valid journey id")
        try:
            contract = json.loads((Path(args.project) / ".graphhelm" / "journeys" / f"{args.journey}.json").read_bytes())
            steps = [s["stepId"] for s in contract["steps"]]
        except (OSError, ValueError, KeyError, TypeError):
            parser.error(f"cannot read a journey contract with steps at .graphhelm/journeys/{args.journey}.json")
    outcome = observe(Path(args.project), args.command, args.timeout)
    if args.journey and outcome["verdict"] in ("passed", "failed"):
        record_journey(args, Path(args.project), steps, outcome)
    print(json.dumps(outcome, indent=2))
    return {"passed": 0, "failed": 1}.get(outcome["verdict"], 2)


if __name__ == "__main__":
    sys.exit(main())
