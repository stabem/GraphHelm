"""Exercise calibration, immutable receipts and frozen-cohort integrity offline."""
import argparse
import json
from pathlib import Path
import tempfile
import unittest
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
import methodology as bench


class MethodologyContracts(unittest.TestCase):
    def test_pagination_oracle_rejects_unbounded_otherwise_working_candidate(self):
        case = next(row for row in bench.read(bench.HOME / "methodology_cases.json")["cases"]
                    if row["id"] == "pagination")
        candidate = ("def collect(fetch):\n    result, cursor, seen = [], None, set()\n"
                     "    while True:\n        page, cursor = fetch(cursor)\n        result.extend(page)\n"
                     "        if cursor is None: return result\n"
                     "        if cursor in seen: raise ValueError('cycle')\n        seen.add(cursor)\n")
        with tempfile.TemporaryDirectory() as directory:
            result = bench.evaluate(case, candidate, Path(directory) / "evaluation")
        self.assertTrue(result["public"]["accepted"])
        self.assertFalse(result["hidden"]["accepted"])
        self.assertIn("page bound missing", result["hidden"]["stderr"])

    def test_zero_exit_before_assertions_is_not_acceptance(self):
        case = bench.read(bench.HOME / "methodology_cases.json")["cases"][0]
        for source in ("raise SystemExit(0)\n", "import os;os._exit(0)\n",
                       "from pathlib import Path\nPath('public.completed').write_text('completed')\nPath('hidden.completed').write_text('completed')\nraise SystemExit(0)\n"):
            with self.subTest(source=source), tempfile.TemporaryDirectory() as directory:
                result = bench.evaluate(case, source, Path(directory) / "evaluation")
                for observed in result.values():
                    self.assertEqual(observed["exit"], 0)
                    self.assertFalse(observed["completed"])
                    self.assertFalse(observed["accepted"])

    def test_independent_oracles_calibrate_every_broken_parent_and_known_fix(self):
        with tempfile.TemporaryDirectory() as directory:
            result = bench.qualify(bench.read(bench.HOME / "methodology_cases.json"), Path(directory))
        self.assertTrue(result["passed"], result)

    def test_weakened_hidden_oracle_refuses_calibration(self):
        cases = bench.read(bench.HOME / "methodology_cases.json")
        cases["cases"] = [cases["cases"][0]]
        cases["cases"][0]["hidden"] = "pass\n"
        with tempfile.TemporaryDirectory() as directory:
            result = bench.qualify(cases, Path(directory))
        self.assertFalse(result["passed"])

    def test_publish_keeps_first_attempt_on_duplicate(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "receipt.json"
            bench.publish(path, {"status": "failed"})
            with self.assertRaises(FileExistsError):
                bench.publish(path, {"status": "passed"})
            self.assertEqual(bench.read(path), {"status": "failed"})

    def test_missing_usage_is_unknown_and_codex_cache_is_separate(self):
        self.assertIsNone(bench.usage("codex", "") ["counts"])
        observed = bench.usage("codex", json.dumps({"type": "turn.completed", "usage": {
            "input_tokens": 100, "cached_input_tokens": 60, "output_tokens": 20}}))
        self.assertEqual(observed["counts"]["input_tokens"], 100)
        self.assertEqual(observed["counts"]["cached_input_tokens"], 60)

    def test_partial_cohort_and_tampered_receipt_cannot_be_complete(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cases = root / "cases.json"
            profiles = root / "profiles.json"
            cases.write_text('{"cases": []}', encoding="utf-8")
            profiles.write_text('{"profiles": [{"id": "direct"}]}', encoding="utf-8")
            manifest = root / "manifest.json"
            cell = {"id": "000", "case": "csv", "profile": "direct", "cli": "codex"}
            bench.publish(manifest, {"instrumentSha256": bench.digest(Path(bench.__file__)),
                "cases": {"path": str(cases), "sha256": bench.digest(cases)},
                "profiles": {"path": str(profiles), "sha256": bench.digest(profiles)},
                "plannedAttempts": [cell], "models": {"codex": "pinned", "claude": "pinned"},
                "studyType": "protocol_profile_pilot"})
            args = argparse.Namespace(manifest=manifest, output=root / "results")
            report = bench.report(args)
            self.assertFalse(report["complete"])
            self.assertIsNone(report["winner"])
            bench.publish(args.output / "000.json", {**cell, "status": "passed", "manifestSha256": "wrong"})
            with self.assertRaisesRegex(ValueError, "frozen cell"):
                bench.report(args)
            cases.write_text('{"cases": [1]}', encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "changed after freeze"):
                bench.load_frozen(manifest)


if __name__ == "__main__":
    unittest.main()
