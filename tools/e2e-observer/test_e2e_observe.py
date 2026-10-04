"""Contract tests for the e2e browser observer adapter, with a fake runner in place of `npx e2e`."""

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("e2e_observe.py")
REPORT = {"schemaVersion": "report-1", "run": {"status": "passed", "results": [
    {"titlePath": ["checkout", "user signs in"], "status": "passed"},
    {"titlePath": ["checkout", "user pays"], "status": "failed"}]}}


class E2EObserve(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.project = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def run_observer(self, exit_code, write_report=True, report=None):
        if report is None:
            report = json.dumps(dict(REPORT, run=dict(
                REPORT["run"], status="passed" if exit_code == 0 else "failed")))
        (self.project / "next_report.json").write_text(report)
        fake = self.project / "fake_e2e.py"
        fake.write_text(
            "import os, shutil, sys\n"
            + ("os.makedirs('.e2e', exist_ok=True); shutil.copy('next_report.json', '.e2e/report.json')\n"
               if write_report else "")
            + f"sys.stderr.write('runner says {exit_code}\\n'); sys.exit({exit_code})\n")
        result = subprocess.run([sys.executable, str(SCRIPT), "--project", str(self.project),
                                 "--command", f"{sys.executable} {fake}"],
                                capture_output=True, text=True)
        return result.returncode, json.loads(result.stdout)

    def test_passing_run_is_observed(self):
        code, out = self.run_observer(0)
        self.assertEqual((code, out["verdict"]), (0, "passed"))
        self.assertTrue(any(line.startswith(".e2e/report.json sha256:") for line in out["evidence"]))

    def test_failed_test_is_a_red_observation_with_its_titles(self):
        code, out = self.run_observer(1)
        self.assertEqual((code, out["verdict"]), (1, "failed"))
        self.assertIn("failed: checkout > user pays", out["evidence"])

    def test_infrastructure_failure_is_observer_missing_not_failed(self):
        for runner_exit in (2, 3, 4, 130):
            code, out = self.run_observer(runner_exit)
            self.assertEqual((code, out["verdict"]), (2, "observer_missing"), runner_exit)
            self.assertTrue(out["evidence"][-1].startswith("OBSERVER_MISSING"))

    def test_green_exit_without_report_is_not_a_pass(self):
        code, out = self.run_observer(0, write_report=False)
        self.assertEqual((code, out["verdict"]), (2, "observer_missing"))

    def test_stale_report_from_an_earlier_run_is_not_a_pass(self):
        self.run_observer(0)
        code, out = self.run_observer(0, write_report=False)
        self.assertEqual((code, out["verdict"]), (2, "observer_missing"))
        self.assertFalse((self.project / ".e2e" / "report.json").exists())

    def test_broken_or_incomplete_report_is_never_a_pass(self):
        cases = {"not json": "{oops",
                 "wrong schema": json.dumps({"run": {"status": "passed", "results": [{}]}}),
                 "no results": json.dumps({"schemaVersion": "report-1", "run": {"status": "passed", "results": []}}),
                 "status disagrees": json.dumps({"schemaVersion": "report-1", "run": {
                     "status": "failed", "results": [{"titlePath": ["t"], "status": "failed"}]}})}
        for name, report in cases.items():
            code, out = self.run_observer(0, report=report)
            self.assertEqual((code, out["verdict"]), (2, "observer_missing"), name)

    def test_missing_runner_is_observer_missing(self):
        result = subprocess.run([sys.executable, str(SCRIPT), "--project", str(self.project),
                                 "--command", "definitely-not-installed-e2e run"],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)
        self.assertIn("OBSERVER_MISSING", json.loads(result.stdout)["evidence"][0])


if __name__ == "__main__":
    unittest.main()
