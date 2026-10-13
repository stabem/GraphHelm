"""Offline accounting contract for recorded development-contracts eval outcomes."""

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
REPORT = ROOT / "tools/evals/selection_report.py"


class SelectionReport(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.input_path = Path(self.temp.name) / "observations.json"
        self.observations = [
            {"case_id": "keel-green-on-parent-unprompted", "status": "completed",
             "invoked_skills": ["keel"], "completion": "pass", "rubric": "keel",
             "judge": "pass", "human_label": "fail", "response_sha256": "a" * 64, "judge_id": "judge-v1"},
            {"case_id": "keel-green-on-parent-unprompted", "status": "completed",
             "invoked_skills": ["test-audit"], "completion": "fail", "rubric": "keel",
             "judge": "fail", "human_label": "pass", "response_sha256": "b" * 64, "judge_id": "judge-v1"},
            {"case_id": "no-skill-arithmetic", "status": "completed",
             "invoked_skills": ["keel"], "completion": "fail", "rubric": "arithmetic",
             "judge": "fail"},
            {"case_id": "no-skill-audit-grammar", "status": "completed",
             "invoked_skills": [], "completion": "pass", "rubric": "grammar",
             "judge": "pass"},
            {"case_id": "test-audit-junk-unprompted", "status": "timeout",
             "invoked_skills": ["test-audit"], "completion": "pass", "rubric": "audit",
             "judge": "pass"},
            {"case_id": "no-skill-arithmetic", "status": "executor_error",
             "invoked_skills": [], "completion": "pass", "rubric": "arithmetic",
             "judge": "pass"},
            {"case_id": "keel-green-on-parent-unprompted", "status": "completed",
             "invoked_skills": ["keel"], "completion": "pass", "rubric": "keel",
             "judge": "fail"},
            {"case_id": "unknown-case", "status": "completed", "invoked_skills": [],
             "completion": "pass", "rubric": "unknown", "judge": "pass"},
        ]

    def run_report(self):
        self.input_path.write_text(json.dumps(self.observations), encoding="utf-8")
        return subprocess.run([sys.executable, str(REPORT), str(self.input_path)],
                              cwd=ROOT, capture_output=True, text=True)

    def test_reports_separate_counts_incomplete_judges_and_rejects_unknown_case(self):
        result = self.run_report()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unknown-case", result.stderr)

        self.observations.pop()
        result = self.run_report()
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(result.stdout)
        self.assertEqual(report["selection"], {
            "unprompted": {"correct_trigger": 2, "miss": 1, "false_trigger": 0,
                           "correct_abstain": 0, "incomplete": 1},
            "no_skill": {"correct_trigger": 0, "miss": 0, "false_trigger": 1,
                         "correct_abstain": 1, "incomplete": 1},
        })
        self.assertEqual(report["completion"], {
            "unprompted": {"pass": 2, "fail": 1, "incomplete": 1},
            "no_skill": {"pass": 1, "fail": 1, "incomplete": 1},
        })
        self.assertEqual(report["judges"]["by_rubric"]["keel"]["false_pass"], 1)
        self.assertEqual(report["judges"]["by_rubric"]["keel"]["false_fail"], 1)
        self.assertEqual(report["judges"]["by_rubric"]["keel"]["repeat_disagreement"], 0)
        again = self.run_report()
        self.assertEqual(result.stdout, again.stdout)

    def test_unqualified_rubric_reports_no_accuracy(self):
        self.observations = [{"case_id": "no-skill-arithmetic", "status": "completed",
                             "invoked_skills": [], "completion": "pass", "rubric": "new",
                             "judge": "pass"},
                             {"case_id": "no-skill-audit-grammar", "status": "completed",
                              "invoked_skills": [], "completion": "pass", "rubric": "new",
                              "judge": "fail"}]
        result = self.run_report()
        self.assertEqual(result.returncode, 0, result.stderr)
        rubric = json.loads(result.stdout)["judges"]["by_rubric"]["new"]
        self.assertEqual(rubric["status"], "unqualified")
        self.assertEqual(rubric["observed"], 2)
        self.assertEqual(rubric["pass"], 1)
        self.assertEqual(rubric["fail"], 1)
        self.assertEqual(rubric["repeat_disagreement"], 0)
        self.assertIsNone(rubric["false_pass"])
        self.assertIsNone(rubric["false_fail"])


if __name__ == "__main__":
    unittest.main()
