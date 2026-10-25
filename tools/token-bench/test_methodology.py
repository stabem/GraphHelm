"""Exercise calibration, immutable receipts and frozen-cohort integrity offline."""
import argparse
import json
from pathlib import Path
import tempfile
import unittest

import methodology as bench


class MethodologyContracts(unittest.TestCase):
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
