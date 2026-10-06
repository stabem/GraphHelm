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


CONTRACT = {"contractId": "checkout", "steps": [{"stepId": "home"}, {"stepId": "cart"}, {"stepId": "pay"}]}


def journey_report(shots, statuses):
    """One spec per step; `shots[step]` is a PNG path or None; `statuses[step]` expected|unexpected."""
    specs = []
    for step, path in shots.items():
        attachments = [] if path is None else [{"name": "screenshot", "contentType": "image/png", "path": str(path)}]
        specs.append({"title": step, "tests": [{"status": statuses.get(step, "expected"),
                                                "results": [{"attachments": attachments}]}]})
    unexpected = sum(1 for s in statuses.values() if s == "unexpected")
    return {"stats": {"expected": len(specs) - unexpected, "unexpected": unexpected, "flaky": 0},
            "suites": [{"title": "checkout.spec.ts", "specs": specs, "suites": []}]}


class JourneyMode(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.project = Path(self.tmp.name)
        (self.project / ".graphhelm" / "journeys").mkdir(parents=True)
        (self.project / ".graphhelm" / "journeys" / "checkout.json").write_text(json.dumps(CONTRACT))
        self.calls = self.project / "calls.jsonl"
        self.fake_cli = self.project / "fake_graphhelm.py"
        # The marker is built from two halves so the fake's own source never contains it.
        self.fake_cli.write_text(
            "import json, sys\n"
            f"open({str(self.calls)!r}, 'a').write(json.dumps(sys.argv[1:]) + '\\n')\n"
            "fail = ('# FAIL_' + 'STEP') in open(sys.argv[0]).read() and '--step' in sys.argv and sys.argv[sys.argv.index('--step') + 1] == 'cart'\n"
            "print(json.dumps({'ok': not fail, 'data': {'signalId': 'sig', 'outcome': 'recorded'}}))\n"
            "sys.exit(1 if fail else 0)\n")

    def tearDown(self):
        self.tmp.cleanup()

    def run_journey(self, shots, statuses, exit_code=0, extra=()):
        for step, path in shots.items():
            if path is not None:
                path.write_bytes(b"\x89PNG\r\n\x1a\n")
        (self.project / "next.json").write_text(json.dumps(journey_report(shots, statuses)))
        fake = self.project / "fake_playwright.py"
        fake.write_text("import os, shutil, sys\n"
                        "shutil.copy('next.json', os.environ['PLAYWRIGHT_JSON_OUTPUT_NAME'])\n"
                        f"sys.exit({exit_code})\n")
        result = subprocess.run([sys.executable, str(SCRIPT), "--project", str(self.project),
                                 "--command", f"{sys.executable} {fake}",
                                 "--journey", "checkout", "--events", "ev", "--execution", "run-1",
                                 "--keyring", "kr", "--key-id", "key",
                                 "--graphhelm", f"{sys.executable} {self.fake_cli}", *extra],
                                capture_output=True, text=True)
        calls = [json.loads(line) for line in self.calls.read_text().splitlines()] if self.calls.exists() else []
        return result.returncode, json.loads(result.stdout), calls

    def test_each_step_is_captured_in_contract_order_then_each_pair_walked(self):
        shots = {s: self.project / f"{s}.png" for s in ("home", "cart", "pay")}
        code, out, calls = self.run_journey(shots, {})
        self.assertEqual((code, out["verdict"]), (0, "passed"))
        self.assertEqual([c[:2] for c in calls], [["journey", "capture"]] * 3 + [["journey", "walked"]] * 2)
        self.assertEqual([c[c.index("--step") + 1] for c in calls[:3]], ["home", "cart", "pay"])
        self.assertEqual(calls[0][calls[0].index("--image") + 1], str(shots["home"]))
        for flag, value in (("--events", "ev"), ("--execution", "run-1"), ("--keyring", "kr"),
                            ("--key-id", "key"), ("--contract", "checkout"), ("--project", str(self.project))):
            self.assertEqual(calls[0][calls[0].index(flag) + 1], value, flag)
        self.assertEqual([(c[c.index("--from") + 1], c[c.index("--to") + 1]) for c in calls[3:]],
                         [("home", "cart"), ("cart", "pay")])
        self.assertEqual(out["journey"]["captured"], ["home", "cart", "pay"])
        self.assertEqual(out["journey"]["walked"], [["home", "cart"], ["cart", "pay"]])

    def test_a_failed_or_unshot_step_is_not_captured_and_breaks_its_arrows(self):
        shots = {"home": self.project / "home.png", "cart": self.project / "cart.png", "pay": None}
        code, out, calls = self.run_journey(shots, {"cart": "unexpected"}, exit_code=1)
        self.assertEqual((code, out["verdict"]), (1, "failed"))
        self.assertEqual([c[c.index("--step") + 1] for c in calls if c[1] == "capture"], ["home"])
        self.assertEqual([c for c in calls if c[1] == "walked"], [])
        self.assertEqual(out["journey"]["missing"], {"cart": "test unexpected", "pay": "no screenshot"})

    def test_a_refused_capture_is_reported_and_the_test_verdict_stands(self):
        self.fake_cli.write_text(self.fake_cli.read_text() + "# FAIL_STEP\n")
        shots = {s: self.project / f"{s}.png" for s in ("home", "cart", "pay")}
        code, out, calls = self.run_journey(shots, {})
        self.assertEqual((code, out["verdict"]), (0, "passed"))
        self.assertEqual(out["journey"]["captured"], ["home", "pay"])
        self.assertIn("cart", out["journey"]["missing"])
        self.assertEqual(out["journey"]["walked"], [])

    def test_journey_without_its_record_flags_is_a_usage_error(self):
        result = subprocess.run([sys.executable, str(SCRIPT), "--project", str(self.project), "--journey", "checkout"],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)
        self.assertIn("--events", result.stderr)

    def test_a_path_like_journey_id_is_refused_before_anything_runs(self):
        # a..b has a valid contract file, so only the id check can refuse it.
        (self.project / ".graphhelm" / "journeys" / "a..b.json").write_text(json.dumps(CONTRACT))
        for bad in ("a..b", "../x"):
            result = subprocess.run([sys.executable, str(SCRIPT), "--project", str(self.project), "--journey", bad,
                                     "--events", "e", "--execution", "x", "--keyring", "k", "--key-id", "i",
                                     "--graphhelm", f"{sys.executable} {self.fake_cli}"],
                                    capture_output=True, text=True)
            self.assertEqual(result.returncode, 2, bad)
            self.assertIn("valid journey id", result.stderr, bad)
            self.assertFalse(self.calls.exists(), bad)

    def test_an_unobserved_run_records_nothing_even_with_a_stale_report(self):
        shots = {s: self.project / f"{s}.png" for s in ("home", "cart", "pay")}
        for path in shots.values():
            path.write_bytes(b"\x89PNG\r\n\x1a\n")
        stale = self.project / ".graphhelm" / "playwright-report.json"
        stale.write_text(json.dumps(journey_report(shots, {})))
        result = subprocess.run([sys.executable, str(SCRIPT), "--project", str(self.project),
                                 "--command", "definitely-not-a-runner-xyz test",
                                 "--journey", "checkout", "--events", "ev", "--execution", "run-1",
                                 "--keyring", "kr", "--key-id", "key",
                                 "--graphhelm", f"{sys.executable} {self.fake_cli}"],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)
        self.assertEqual(json.loads(result.stdout)["verdict"], "observer_missing")
        self.assertFalse(self.calls.exists())


if __name__ == "__main__":
    unittest.main()
