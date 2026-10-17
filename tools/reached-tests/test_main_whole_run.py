"""Contracts for main_whole_run.py: when a run is due and what a red receipt says; no git, Runtime or cargo."""
import json
import tempfile
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import main_whole_run as mw


class WholeRun(unittest.TestCase):
    def test_due_only_when_main_moved_and_the_hour_passed(self):
        self.assertEqual(mw.due({}, "b", 1000, 3600), (True, "no earlier whole run"))
        self.assertFalse(mw.due({"sha": "a", "startedAt": 0}, "a", 99999, 3600)[0])
        self.assertFalse(mw.due({"sha": "a", "startedAt": 900}, "b", 1000, 3600)[0])
        self.assertTrue(mw.due({"sha": "a", "startedAt": 0}, "b", 3600, 3600)[0])

    def test_red_receipt_is_high_severity_and_names_the_red_step(self):
        report = {"status": "failed", "completed": [{"name": "cargo test -p x", "returncode": 101},
                                                    {"name": "fmt", "returncode": 0}]}
        signal = mw.build_signal("a" * 40, "b" * 40, report, Path("r.json"), "o/r", "2026-01-01T00:00:00Z")["signal"]
        self.assertEqual(signal["type"], "main.whole_run")
        self.assertEqual(signal["severity"], "high")
        self.assertEqual(json.loads(signal["description"])["red"], ["cargo test -p x"])
        green = mw.build_signal("a" * 40, "b" * 40, {"status": "passed"}, Path("r.json"), "o/r", "t")["signal"]
        self.assertEqual(green["severity"], "low")

    def test_state_is_saved_before_the_post_so_a_failed_post_cannot_rerun_the_suite(self):
        def refusing_poster(args, body):
            self.assertEqual(json.loads(state.read_text())["sha"], "a" * 40)
            raise SystemExit("no credential")

        with tempfile.TemporaryDirectory() as temp:
            state = Path(temp) / "state.json"
            with self.assertRaises(SystemExit):
                mw.save_then_post(None, state, "a" * 40, 5.0, {"status": "failed"}, {}, refusing_poster)
            saved = json.loads(state.read_text())
            self.assertEqual((saved["sha"], saved["recorded"]), ("a" * 40, False))
            self.assertFalse(mw.due(saved, "a" * 40, 99999, 3600)[0])


if __name__ == "__main__":
    unittest.main()
