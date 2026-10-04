"""Browser journey observer: run the open-source `e2e` runner and turn its result into proof lines.

`npx e2e run` (npm package `e2e`, github.com/tester-army/e2e, Apache-2.0) drives a real browser,
writes `.e2e/report.json`, and exits 0 (passed), 1 (a test failed), 2 (CLI/config/credential
error), 3 (infrastructure: engine, app, model provider), 4 (runner bug) or 130 (interrupted).
Only 0 and 1 are observations of the journey. Everything else, and a missing runner, is
`OBSERVER_MISSING`: the journey was not seen, so nothing may be claimed about it.

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

DEFAULT_COMMAND = "npx --no-install e2e run --reporter list,json,junit"
REPORT = Path(".e2e") / "report.json"
OBSERVED = {0: "passed", 1: "failed"}


def _tests(report: dict) -> list[str]:
    """One line per result in `run.results` (report-1 schema): status and title path."""
    lines = []
    for result in report.get("run", {}).get("results", [])[:50]:
        title = " > ".join(str(part) for part in result.get("titlePath", [])) or result.get("testId", "?")
        lines.append(f"{result.get('status', '?')}: {title}")
    return lines


def observe(project: Path, command: str, timeout: int) -> dict:
    argv = shlex.split(command)
    if shutil.which(argv[0]) is None:
        return {"verdict": "observer_missing", "exitCode": None,
                "evidence": [f"OBSERVER_MISSING: `{argv[0]}` is not on PATH"]}
    try:
        result = subprocess.run(argv, cwd=project, capture_output=True, text=True, timeout=timeout)
    except subprocess.TimeoutExpired:
        return {"verdict": "observer_missing", "exitCode": None,
                "evidence": [f"OBSERVER_MISSING: `{command}` did not finish in {timeout}s"]}
    verdict = OBSERVED.get(result.returncode, "observer_missing")
    evidence = [f"`{command}` in {project} exited {result.returncode}"]
    report_path = project / REPORT
    if report_path.is_file():
        raw = report_path.read_bytes()
        evidence.append(f"{REPORT.as_posix()} sha256:{hashlib.sha256(raw).hexdigest()}")
        try:
            evidence.extend(_tests(json.loads(raw)))
        except ValueError:
            evidence.append(f"{REPORT.as_posix()} is not JSON")
    elif verdict != "observer_missing":
        verdict = "observer_missing"
        evidence.append(f"no {REPORT.as_posix()} was written, so no test was observed")
    if verdict == "observer_missing":
        tail = (result.stderr or result.stdout).strip().splitlines()[-1:] or ["no output"]
        evidence.append(f"OBSERVER_MISSING: e2e exit {result.returncode}: {tail[0][:300]}")
    return {"verdict": verdict, "exitCode": result.returncode, "evidence": evidence}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--project", default=".", help="directory holding the e2e config and tests")
    parser.add_argument("--command", default=os.environ.get("GRAPHHELM_E2E_COMMAND", DEFAULT_COMMAND))
    parser.add_argument("--timeout", type=int, default=1800)
    args = parser.parse_args()
    outcome = observe(Path(args.project), args.command, args.timeout)
    print(json.dumps(outcome, indent=2))
    return {"passed": 0, "failed": 1}.get(outcome["verdict"], 2)


if __name__ == "__main__":
    sys.exit(main())
