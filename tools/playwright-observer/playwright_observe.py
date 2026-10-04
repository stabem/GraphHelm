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
import shlex
import shutil
import subprocess
import sys

DEFAULT_COMMAND = "npx --no-install playwright test --reporter=json"
REPORT = Path(".graphhelm") / "playwright-report.json"
OBSERVED = {0: "passed", 1: "failed"}


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
    argv = shlex.split(command)
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


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--project", default=".", help="directory holding playwright.config and tests")
    parser.add_argument("--command",
                        default=os.environ.get("GRAPHHELM_PLAYWRIGHT_COMMAND", DEFAULT_COMMAND))
    parser.add_argument("--timeout", type=int, default=1800)
    args = parser.parse_args()
    outcome = observe(Path(args.project), args.command, args.timeout)
    print(json.dumps(outcome, indent=2))
    return {"passed": 0, "failed": 1}.get(outcome["verdict"], 2)


if __name__ == "__main__":
    sys.exit(main())
