"""Contract tests for the Playwright browser observer, with a fake runner in place of `npx playwright`."""

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("playwright_observe.py")


def report(unexpected):
    return {"stats": {"expected": 1, "unexpected": unexpected, "flaky": 0, "skipped": 0},
            "suites": [{"title": "checkout.spec.ts", "specs": [], "suites": [{"title": "checkout", "specs": [
                {"title": "user signs in", "tests": [{"status": "expected"}]},
                {"title": "user pays", "tests": [{"status": "unexpected" if unexpected else "expected"}]}]}]}]}


class PlaywrightObserve(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.project = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def run_observer(self, exit_code, body=None, write=True):
        if body is None:
            body = json.dumps(report(0 if exit_code == 0 else 1))
        (self.project / "next.json").write_text(body)
        fake = self.project / "fake_playwright.py"
        fake.write_text(
            "import os, shutil, sys\n"
            + ("shutil.copy('next.json', os.environ['PLAYWRIGHT_JSON_OUTPUT_NAME'])\n" if write else "")
            + f"sys.stderr.write('runner says {exit_code}\\n'); sys.exit({exit_code})\n")
        result = subprocess.run([sys.executable, str(SCRIPT), "--project", str(self.project),
                                 "--command", f"{sys.executable} {fake}"],
                                capture_output=True, text=True)
        return result.returncode, json.loads(result.stdout)

    def test_passing_run_is_observed(self):
        code, out = self.run_observer(0)
        self.assertEqual((code, out["verdict"]), (0, "passed"))
        self.assertIn("expected: checkout.spec.ts > checkout > user pays", out["evidence"])

    def test_failed_test_is_a_red_observation_with_its_titles(self):
        code, out = self.run_observer(1)
        self.assertEqual((code, out["verdict"]), (1, "failed"))
        self.assertIn("unexpected: checkout.spec.ts > checkout > user pays", out["evidence"])

    def test_other_exits_are_observer_missing(self):
        for runner_exit in (2, 130):
            code, out = self.run_observer(runner_exit)
            self.assertEqual((code, out["verdict"]), (2, "observer_missing"), runner_exit)
            self.assertTrue(out["evidence"][-1].startswith("OBSERVER_MISSING"))

    def test_green_exit_without_report_is_not_a_pass(self):
        code, out = self.run_observer(0, write=False)
        self.assertEqual((code, out["verdict"]), (2, "observer_missing"))

    def test_stale_report_is_not_a_pass(self):
        self.run_observer(0)
        code, out = self.run_observer(0, write=False)
        self.assertEqual(out["verdict"], "observer_missing")

    def test_report_that_disagrees_with_exit_is_not_a_pass(self):
        code, out = self.run_observer(0, body=json.dumps(report(1)))
        self.assertEqual(out["verdict"], "observer_missing")

    def test_empty_or_broken_report_is_not_a_pass(self):
        empty = {"stats": {"expected": 0, "unexpected": 0, "flaky": 0}, "suites": []}
        for body in (json.dumps(empty), "not json", "[]"):
            code, out = self.run_observer(0, body=body)
            self.assertEqual(out["verdict"], "observer_missing", body)

    def test_missing_runner_is_observer_missing(self):
        result = subprocess.run([sys.executable, str(SCRIPT), "--project", str(self.project),
                                 "--command", "definitely-not-a-runner-xyz test"],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)


if __name__ == "__main__":
    unittest.main()
