"""Offline accounting contract for recorded development-contracts eval outcomes."""

import json
import hashlib
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
REPORT = ROOT / "tools/evals/selection_report.py"
NO_WINDOW = getattr(subprocess, "CREATE_NO_WINDOW", 0)


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
             "judge": "fail", "response_sha256": "c" * 64, "judge_id": "judge-v1"},
            {"case_id": "unknown-case", "status": "completed", "invoked_skills": [],
             "completion": "pass", "rubric": "unknown", "judge": "pass"},
        ]

    def run_report(self):
        self.input_path.write_text(json.dumps(self.observations), encoding="utf-8")
        return subprocess.run([sys.executable, str(REPORT), str(self.input_path)],
                              cwd=ROOT, capture_output=True, text=True,
                              creationflags=NO_WINDOW)

    def copied_report(self, root):
        tools = root / "tools/evals"
        evals = root / "evals/graphhelm-development-contracts"
        tools.mkdir(parents=True)
        shutil.copy2(REPORT, tools / "selection_report.py")
        shutil.copytree(ROOT / "evals/graphhelm-development-contracts", evals)
        return tools / "selection_report.py", evals

    @staticmethod
    def digest(path):
        content = path.read_text(encoding="utf-8").replace("\r\n", "\n").encode("utf-8")
        return hashlib.sha256(content).hexdigest()

    def run_partitioned(self, script, observations, manifest):
        observations_path = Path(self.temp.name) / "partition-observations.json"
        manifest_path = Path(self.temp.name) / "partitions.json"
        observations_path.write_text(json.dumps(observations), encoding="utf-8")
        manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
        return subprocess.run([sys.executable, str(script), str(observations_path),
                               "--partition-manifest", str(manifest_path)],
                              capture_output=True, text=True, creationflags=NO_WINDOW)

    def run_partitioned_text(self, script, observations, manifest_text):
        observations_path = Path(self.temp.name) / "partition-observations.json"
        manifest_path = Path(self.temp.name) / "partitions.json"
        observations_path.write_text(json.dumps(observations), encoding="utf-8")
        manifest_path.write_text(manifest_text, encoding="utf-8")
        return subprocess.run([sys.executable, str(script), str(observations_path),
                               "--partition-manifest", str(manifest_path)],
                              capture_output=True, text=True, creationflags=NO_WINDOW)

    def run_partitioned_observation_text(self, script, observations_text, manifest):
        observations_path = Path(self.temp.name) / "partition-observations.json"
        manifest_path = Path(self.temp.name) / "partitions.json"
        observations_path.write_text(observations_text, encoding="utf-8")
        manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
        return subprocess.run([sys.executable, str(script), str(observations_path),
                               "--partition-manifest", str(manifest_path)],
                              capture_output=True, text=True, creationflags=NO_WINDOW)

    def make_manifest(self, evals, tuning, held_out):
        cases = {}
        for case_id, partition in [(case_id, "tuning") for case_id in tuning] + [
                (case_id, "held_out") for case_id in held_out]:
            case_dir = evals / case_id
            cases[case_id] = {"partition": partition,
                              "promptSha256": self.digest(case_dir / "prompt.md"),
                              "criteriaSha256": self.digest(case_dir / "graders/criteria.md")}
        return {"version": 1, "cases": cases}

    @staticmethod
    def bind_observations(observations, manifest):
        canonical = json.dumps(manifest, sort_keys=True, separators=(",", ":"),
                               ensure_ascii=False).encode("utf-8")
        identity = hashlib.sha256(canonical).hexdigest()
        return [{**item, "partition_manifest_sha256": identity} for item in observations]

    def test_partition_manifest_separates_counts_and_binds_case_content(self):
        with tempfile.TemporaryDirectory() as directory:
            script, evals = self.copied_report(Path(directory))
            manifest = self.make_manifest(evals,
                [case.name for case in evals.iterdir() if case.is_dir() and not case.name.startswith("no-skill-")],
                ["no-skill-arithmetic", "no-skill-audit-grammar"])
            observations = [
                {"case_id": "keel-green-on-parent-unprompted", "status": "completed",
                 "invoked_skills": ["keel"], "completion": "pass", "rubric": "r", "judge": "pass"},
                {"case_id": "no-skill-arithmetic", "status": "completed",
                 "invoked_skills": [], "completion": "fail", "rubric": "r", "judge": "fail"},
                {"case_id": "no-skill-audit-grammar", "status": "timeout",
                 "invoked_skills": [], "completion": "pass", "rubric": "r", "judge": "pass"},
            ]
            observations = self.bind_observations(observations, manifest)
            result = self.run_partitioned(script, observations, manifest)
            self.assertEqual(result.returncode, 0, result.stderr)
            report = json.loads(result.stdout)
            self.assertEqual(report["by_partition"]["tuning"]["selection"]["unprompted"]["correct_trigger"], 1)
            self.assertEqual(report["by_partition"]["held_out"]["selection"]["no_skill"]["incomplete"], 1)
            self.assertEqual(report["by_partition"]["held_out"]["judges"]["by_rubric"]["r"]["observed"], 1)
            self.assertNotIn("selection", report)
            self.assertEqual(report["partitioning"]["optimizer_isolation"], "unobserved")
            again = self.run_partitioned(script, observations, manifest)
            self.assertEqual(result.stdout, again.stdout)

            changed = evals / "no-skill-arithmetic/prompt.md"
            changed.write_text(changed.read_text(encoding="utf-8") + "\n", encoding="utf-8")
            drift = self.run_partitioned(script, observations, manifest)
            self.assertNotEqual(drift.returncode, 0)
            self.assertIn("no-skill-arithmetic", drift.stderr)
            self.assertEqual(drift.stdout, "")

            empty = self.make_manifest(evals,
                [case.name for case in evals.iterdir() if case.is_dir()], [])
            result = self.run_partitioned(script, [], empty)
            self.assertEqual(result.returncode, 0, result.stderr)
            empty_report = json.loads(result.stdout)
            self.assertEqual(empty_report["by_partition"]["held_out"]["selection"], {})
            self.assertEqual(empty_report["by_partition"]["held_out"]["completion"], {})

    def test_partition_manifest_rejects_missing_unknown_invalid_and_split_twins(self):
        with tempfile.TemporaryDirectory() as directory:
            script, evals = self.copied_report(Path(directory))
            manifest = self.make_manifest(evals,
                [case.name for case in evals.iterdir() if case.is_dir() and not case.name.startswith("no-skill-")],
                [case.name for case in evals.iterdir() if case.is_dir() and case.name.startswith("no-skill-")])
            observation = [{"case_id": "no-skill-arithmetic", "status": "completed",
                            "invoked_skills": [], "completion": "pass", "rubric": "r"}]
            observation = self.bind_observations(observation, manifest)
            missing = dict(manifest["cases"])
            missing.pop("no-skill-arithmetic")
            for invalid, expected in [
                ({"version": 1, "cases": missing}, "no-skill-arithmetic"),
                ({"version": 1, "cases": {**manifest["cases"], "unknown": {}}}, "unknown"),
                ({"version": 1, "cases": {**manifest["cases"],
                    "no-skill-arithmetic": {"partition": "other", "promptSha256": "0"*64,
                                             "criteriaSha256": "0"*64}}}, "no-skill-arithmetic"),
                ({"version": 1, "cases": {**manifest["cases"],
                    "no-skill-arithmetic": {"partition": "tuning", "promptSha256": "bad",
                                             "criteriaSha256": "0"*64}}}, "no-skill-arithmetic"),
            ]:
                result = self.run_partitioned(script, observation, invalid)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(expected, result.stderr)

            duplicate_text = json.dumps(manifest)
            duplicate_text = duplicate_text[:-1] + ',"cases":{}}'
            result = self.run_partitioned_text(script, observation, duplicate_text)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("duplicate partition manifest key: cases", result.stderr)

            split = self.make_manifest(evals,
                [case.name for case in evals.iterdir() if case.is_dir() and not case.name.startswith("no-skill-")],
                ["no-skill-arithmetic", "no-skill-audit-grammar"])
            split["cases"]["keel-green-on-parent-unprompted"]["partition"] = "held_out"
            result = self.run_partitioned(script, observation, split)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("keel-green-on-parent", result.stderr)

    def test_partitioned_observations_are_bound_to_manifest_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            script, evals = self.copied_report(Path(directory))
            case_ids = [case.name for case in evals.iterdir() if case.is_dir()]
            manifest_a = self.make_manifest(evals, case_ids, [])
            manifest_b = self.make_manifest(evals, [], case_ids)
            canonical_a = json.dumps(manifest_a, sort_keys=True, separators=(",", ":"),
                                     ensure_ascii=False).encode("utf-8")
            identity_a = hashlib.sha256(canonical_a).hexdigest()
            rows = [
                {"case_id": "no-skill-arithmetic", "status": "completed",
                 "invoked_skills": [], "completion": "pass", "rubric": "r",
                 "partition_manifest_sha256": identity_a},
                {"case_id": "no-skill-audit-grammar", "status": "timeout",
                 "invoked_skills": [], "completion": "pass", "rubric": "r",
                 "partition_manifest_sha256": identity_a},
            ]

            mismatch = self.run_partitioned(script, rows, manifest_b)
            self.assertEqual(mismatch.returncode, 2)
            self.assertIn("observation 0", mismatch.stderr)
            self.assertIn("no-skill-arithmetic", mismatch.stderr)
            self.assertEqual(mismatch.stdout, "")

            matched = self.run_partitioned(script, rows, manifest_a)
            self.assertEqual(matched.returncode, 0, matched.stderr)
            report = json.loads(matched.stdout)
            self.assertEqual(report["by_partition"]["tuning"]["selection"]["no_skill"]["correct_abstain"], 1)
            self.assertEqual(report["by_partition"]["tuning"]["selection"]["no_skill"]["incomplete"], 1)
            reordered_text = json.dumps(manifest_a, indent=2, sort_keys=False)
            reordered = self.run_partitioned_text(script, rows, reordered_text)
            self.assertEqual(reordered.returncode, 0, reordered.stderr)
            self.assertEqual(matched.stdout, reordered.stdout)

            for row, binding, message in [
                ({key: value for key, value in rows[0].items()
                  if key != "partition_manifest_sha256"}, None, "missing"),
                ({**rows[0], "partition_manifest_sha256": "A" * 64}, None, "64 lowercase"),
                ({**rows[1], "partition_manifest_sha256": "0" * 64}, None, "manifest"),
            ]:
                candidate = row if binding is None else {**row, "partition_manifest_sha256": binding}
                result = self.run_partitioned(script, [candidate], manifest_a)
                self.assertEqual(result.returncode, 2)
                self.assertIn("observation 0", result.stderr)
                self.assertIn("no-skill", result.stderr)
                self.assertIn(message, result.stderr)
                self.assertEqual(result.stdout, "")

            duplicate_row = json.dumps(rows[0])[:-1] + ',"partition_manifest_sha256":"' + identity_a + '"}'
            duplicate = self.run_partitioned_observation_text(script, "[" + duplicate_row + "]", manifest_a)
            self.assertEqual(duplicate.returncode, 2)
            self.assertIn("duplicate observation key: partition_manifest_sha256", duplicate.stderr)
            self.assertEqual(duplicate.stdout, "")

            changed = evals / "no-skill-arithmetic/prompt.md"
            changed.write_text(changed.read_text(encoding="utf-8") + "\n", encoding="utf-8")
            updated_manifest = self.make_manifest(evals, case_ids, [])
            drift = self.run_partitioned(script, rows, updated_manifest)
            self.assertEqual(drift.returncode, 2)
            self.assertIn("observation 0", drift.stderr)
            self.assertIn("no-skill-arithmetic", drift.stderr)
            self.assertIn("manifest", drift.stderr)
            self.assertEqual(drift.stdout, "")

            legacy_row = {key: value for key, value in rows[0].items()
                          if key != "partition_manifest_sha256"}
            self.input_path.write_text(json.dumps([{**legacy_row,
                "partition_manifest_sha256": "a" * 64}]), encoding="utf-8")
            legacy = subprocess.run([sys.executable, str(REPORT), str(self.input_path)],
                                    cwd=ROOT, capture_output=True, text=True,
                                    creationflags=NO_WINDOW)
            self.assertEqual(legacy.returncode, 0, legacy.stderr)
            self.assertEqual(json.loads(legacy.stdout)["partitioning"]["status"], "unobserved")
            self.input_path.write_text(json.dumps([{**legacy_row,
                "partition_manifest_sha256": "A" * 64}]), encoding="utf-8")
            malformed = subprocess.run([sys.executable, str(REPORT), str(self.input_path)],
                                       cwd=ROOT, capture_output=True, text=True,
                                       creationflags=NO_WINDOW)
            self.assertEqual(malformed.returncode, 2)
            self.assertIn("partition_manifest_sha256", malformed.stderr)

    def test_legacy_report_marks_partitioning_unobserved(self):
        result = self.run_report()
        self.assertNotEqual(result.returncode, 0)  # default fixture still has its unknown-case guard
        self.observations = self.observations[:-1]
        result = self.run_report()
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(result.stdout)
        self.assertEqual(report["partitioning"]["status"], "unobserved")

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
        self.assertEqual(report["judges"]["by_rubric"]["keel"]["repeat_groups"], 0)
        self.assertEqual(report["judges"]["by_rubric"]["keel"]["repeat_unidentified"], 0)
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
        self.assertEqual(rubric["repeat_groups"], 0)
        self.assertEqual(rubric["repeat_unidentified"], 2)
        self.assertIsNone(rubric["false_pass"])
        self.assertIsNone(rubric["false_fail"])

    def test_human_comparisons_require_paired_completed_verdicts(self):
        base = {"case_id": "no-skill-arithmetic", "status": "completed",
                "invoked_skills": [], "completion": "pass", "rubric": "paired"}
        self.observations = [{**base, "human_label": "pass"}]
        result = self.run_report()
        self.assertEqual(result.returncode, 0, result.stderr)
        rubric = json.loads(result.stdout)["judges"]["by_rubric"]["paired"]
        self.assertEqual(rubric["human_labeled"], 1)
        self.assertEqual(rubric["observed"], 0)
        self.assertEqual(rubric["compared"], 0)
        self.assertEqual(rubric["status"], "unqualified")
        self.assertIsNone(rubric["false_pass"])
        self.assertIsNone(rubric["false_fail"])

        self.observations = [{**base, "human_label": "pass"},
                             {**base, "judge": "fail"}]
        result = self.run_report()
        self.assertEqual(result.returncode, 0, result.stderr)
        rubric = json.loads(result.stdout)["judges"]["by_rubric"]["paired"]
        self.assertEqual(rubric["human_labeled"], 1)
        self.assertEqual(rubric["observed"], 1)
        self.assertEqual(rubric["compared"], 0)
        self.assertEqual(rubric["status"], "unqualified")
        self.assertIsNone(rubric["false_pass"])
        self.assertIsNone(rubric["false_fail"])

        self.observations = [{**base, "human_label": "fail", "judge": "pass"},
                             {**base, "human_label": "pass", "judge": "fail"},
                             {**base, "human_label": "pass", "judge": "pass"}]
        result = self.run_report()
        self.assertEqual(result.returncode, 0, result.stderr)
        rubric = json.loads(result.stdout)["judges"]["by_rubric"]["paired"]
        self.assertEqual(rubric["human_labeled"], 3)
        self.assertEqual(rubric["observed"], 3)
        self.assertEqual(rubric["compared"], 3)
        self.assertEqual(rubric["status"], "comparisons_available")
        self.assertEqual(rubric["false_pass"], 1)
        self.assertEqual(rubric["false_fail"], 1)

        self.observations.append({**base, "status": "timeout", "judge": "pass",
                                  "human_label": "fail"})
        result = self.run_report()
        self.assertEqual(result.returncode, 0, result.stderr)
        rubric = json.loads(result.stdout)["judges"]["by_rubric"]["paired"]
        self.assertEqual(rubric["human_labeled"], 3)
        self.assertEqual(rubric["compared"], 3)
        self.assertEqual(rubric["false_pass"], 1)
        self.assertEqual(rubric["false_fail"], 1)

    def test_repeat_identity_groups_only_same_response_and_judge(self):
        base = {"case_id": "keel-green-on-parent-unprompted", "status": "completed",
                "invoked_skills": ["keel"], "completion": "pass", "rubric": "keel",
                "response_sha256": "a" * 64, "judge_id": "judge-v1"}
        self.observations = [{**base, "judge": "pass"}, {**base, "judge": "fail"},
                             {**base, "judge_id": "judge-v2", "judge": "pass"}]
        result = self.run_report()
        self.assertEqual(result.returncode, 0, result.stderr)
        rubric = json.loads(result.stdout)["judges"]["by_rubric"]["keel"]
        self.assertEqual(rubric["repeat_groups"], 1)
        self.assertEqual(rubric["repeat_disagreement"], 1)

    def test_invalid_repeat_identity_is_rejected(self):
        self.observations = [{"case_id": "no-skill-arithmetic", "status": "completed",
                              "invoked_skills": [], "completion": "pass", "rubric": "r",
                              "judge": "pass", "response_sha256": "a" * 64}]
        result = self.run_report()
        self.assertEqual(result.returncode, 2)
        self.assertIn("must occur together", result.stderr)


if __name__ == "__main__":
    unittest.main()
