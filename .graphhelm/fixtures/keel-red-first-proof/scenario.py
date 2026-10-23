#!/usr/bin/env python3
"""Run a bounded red-first specimen through the real GraphHelm CLI."""
from __future__ import annotations

import html
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
TIMEOUT_SECONDS = 180

SPECIMEN_BASE_SOURCE = r'''#!/usr/bin/env python3
import argparse
import html
import sys
import unittest
from pathlib import Path

from sum_fixture import total


class Regression(unittest.TestCase):
    pass

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--test-name", required=True)
    parser.add_argument("--junit", required=True)
    args = parser.parse_args()
    module = sys.modules[__name__]
    test = getattr(module, args.test_name, None)
    if not callable(test) or not args.test_name.startswith("test_"):
        raise SystemExit("the requested test name is missing")
    setattr(Regression, args.test_name, test)
    result = unittest.TestResult()
    unittest.TestSuite([Regression(args.test_name)]).run(result)
    failure = next((detail for _case, detail in result.failures), None)
    error = next((detail for _case, detail in result.errors), None)
    skipped = next((detail for _case, detail in result.skipped), None)
    body = ""
    if failure is not None:
        body = "<failure>" + html.escape(failure) + "</failure>"
    elif error is not None:
        body = "<error>" + html.escape(error) + "</error>"
    elif skipped is not None:
        body = "<skipped>" + html.escape(skipped) + "</skipped>"
    xml = (f'<testsuite tests="1" failures="{int(failure is not None)}" '
           f'errors="{int(error is not None)}" skipped="{int(skipped is not None)}">'
           f'<testcase name="{html.escape(args.test_name, quote=True)}">{body}'
           '</testcase></testsuite>')
    Path(args.junit).write_text(xml, encoding="utf-8")
    raise SystemExit(0 if result.wasSuccessful() else 1)
'''

VACUOUS_TEST_SOURCE = SPECIMEN_BASE_SOURCE + r'''

def test_sum_drops_first_item(self):
    self.assertTrue(True)

if __name__ == "__main__":
    main()
'''

REGRESSION_TEST_SOURCE = SPECIMEN_BASE_SOURCE + r'''

def test_sum_drops_first_item(self):
    self.assertEqual(total([2, 3]), 5)

if __name__ == "__main__":
    main()
'''

BROKEN_IMPLEMENTATION = "def total(values):\n    return sum(values[1:])\n"
FIXED_IMPLEMENTATION = "def total(values):\n    return sum(values)\n"


def run_git(cwd: Path, *args: str) -> str:
    completed = subprocess.run(
        ["git", *args], cwd=cwd, check=True, text=True,
        capture_output=True, encoding="utf-8", errors="replace",
        timeout=30, creationflags=NO_WINDOW,
    )
    return completed.stdout.strip()


def commit(cwd: Path, message: str) -> str:
    run_git(cwd, "add", "scenario.py", "sum_fixture.py")
    subprocess.run(
        ["git", "-c", "user.name=keel-fixture", "-c", "user.email=keel-fixture@invalid",
         "-c", "commit.gpgsign=false", "commit", "-q", "-m", message],
        cwd=cwd, check=True, capture_output=True, timeout=30, creationflags=NO_WINDOW,
    )
    return run_git(cwd, "rev-parse", "HEAD")


def check_cli(repo: Path, base: str, scratch: Path, cli: Path) -> dict:
    env = os.environ.copy()
    env.update({"TEMP": str(scratch), "TMP": str(scratch), "TMPDIR": str(scratch)})
    command = "python {file} --test-name {name} --junit {report}"
    completed = subprocess.run(
        [str(cli), "--json", "keel", "check", "--repo", str(repo), "--diff", f"{base}..HEAD",
         "--prove-new-tests", "--prove-command", command, "--prove-timeout-secs", "30",
         "--prove-target-dir", str(scratch / "prove-target")],
        cwd=repo, env=env, text=True, capture_output=True, encoding="utf-8", errors="replace",
        timeout=TIMEOUT_SECONDS, creationflags=NO_WINDOW,
    )
    try:
        envelope = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError(
            f"keel check returned no JSON (exit {completed.returncode}): "
            f"{completed.stdout[-1000:]} {completed.stderr[-1000:]}"
        ) from error
    if completed.returncode not in (0, 2) or not isinstance(envelope.get("data"), dict):
        raise RuntimeError(f"keel check failed (exit {completed.returncode}): {envelope}")
    return envelope


def proof_tuple(envelope: dict, name: str) -> tuple[str, str, str]:
    proofs = envelope.get("data", {}).get("testProof", {}).get("proofs")
    if not isinstance(proofs, list):
        raise AssertionError(f"data.testProof.proofs is missing: {envelope}")
    matches = [proof for proof in proofs if proof.get("name") == name]
    if len(matches) != 1:
        raise AssertionError(f"expected one proof named {name!r}: {envelope}")
    proof = matches[0]
    # #1918: a runner that could not start is a missing toolchain, not a vacuous result.
    for side in ("parent", "head"):
        detail = str(proof.get(side, {}).get("detail", ""))
        if proof.get(side, {}).get("outcome") == "not_run" and "did not start" in detail:
            raise RuntimeError(f"toolchain missing on the {side} side: {detail}")
    return (proof["parent"]["outcome"], proof["head"]["outcome"], proof["verdict"])


def diagnostic_ids(envelope: dict) -> set[str]:
    data = envelope.get("data", {})
    findings = data.get("testProof", {}).get("findings", [])
    return {finding.get("rule") for finding in findings if isinstance(finding, dict)}


def run_scenario() -> dict:
    cli_value = os.environ.get("GRAPHHELM_CLI") or "graphhelm"
    cli_path = shutil.which(cli_value)
    if cli_path is None:
        raise RuntimeError(f"GraphHelm CLI not found: {cli_value}")
    cli = Path(cli_path).resolve()

    scratch_parent = Path(__file__).resolve().parent
    scratch_parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="scenario-", dir=scratch_parent) as scratch_name:
        scratch = Path(scratch_name)
        repo = scratch / "specimen"
        repo.mkdir()
        run_git(repo, "init", "-q")
        (repo / "scenario.py").write_text(SPECIMEN_BASE_SOURCE, encoding="utf-8")
        (repo / "sum_fixture.py").write_text(BROKEN_IMPLEMENTATION, encoding="utf-8")
        base = commit(repo, "specimen: seed defective parent and runner")

        # Negative control: a vacuous test must be green on both sides and stay unearned.
        (repo / "scenario.py").write_text(VACUOUS_TEST_SOURCE, encoding="utf-8")
        vacuous_head = commit(repo, "specimen: add vacuous control")
        vacuous = check_cli(repo, base, scratch, cli)
        vacuous_tuple = proof_tuple(vacuous, "test_sum_drops_first_item")
        if vacuous_tuple != ("passed", "passed", "green_on_parent"):
            raise AssertionError(f"vacuous control tuple was {vacuous_tuple!r}: {vacuous}")
        vacuous_ids = diagnostic_ids(vacuous)
        if "keel.test.green_on_parent" not in vacuous_ids:
            raise AssertionError(f"vacuous control diagnostic missing: {vacuous_ids!r}")

        # Real regression: reset only the owned specimen, then fix only its implementation.
        run_git(repo, "reset", "--hard", base)
        (repo / "scenario.py").write_text(REGRESSION_TEST_SOURCE, encoding="utf-8")
        (repo / "sum_fixture.py").write_text(FIXED_IMPLEMENTATION, encoding="utf-8")
        real_head = commit(repo, "specimen: prove the sum regression")
        real = check_cli(repo, base, scratch, cli)
        real_tuple = proof_tuple(real, "test_sum_drops_first_item")
        if real_tuple != ("failed", "passed", "earned"):
            raise AssertionError(f"regression tuple was {real_tuple!r}: {real}")
        real_ids = diagnostic_ids(real)
        if "keel.test.green_on_parent" in real_ids:
            raise AssertionError(f"earned regression was also marked green on parent: {real_ids!r}")

        return {
            "status": "passed",
            "summary": "regression=parent-failed,head-passed,earned; "
                       "vacuous=parent-passed,head-passed,green_on_parent",
            "cli": str(cli),
            "vacuousControl": {"head": vacuous_head, "proof": list(vacuous_tuple),
                               "diagnostics": sorted(vacuous_ids)},
            "regression": {"head": real_head, "proof": list(real_tuple),
                           "diagnostics": sorted(real_ids)},
        }


if __name__ == "__main__":
    try:
        print(json.dumps(run_scenario(), indent=2, sort_keys=True))
    except Exception as error:
        print(f"scenario failed: {error}", file=sys.stderr)
        raise
