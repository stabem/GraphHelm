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
    def test_child_environment_removes_provider_keys_and_endpoints(self):
        environment = {"CODEX_API_KEY": "secret", "CUSTOM_API_KEY": "secret",
                       "OPENAI_BASE_URL": "https://example", "SAFE_SETTING": "kept"}
        filtered = bench.child_environment(environment)
        self.assertNotIn("CODEX_API_KEY", filtered)
        self.assertNotIn("CUSTOM_API_KEY", filtered)
        self.assertNotIn("OPENAI_BASE_URL", filtered)
        self.assertEqual(filtered["SAFE_SETTING"], "kept")

    def test_native_cli_command_has_frozen_non_bypass_controls(self):
        codex = bench.native_cli_command("codex", Path("codex.exe"), "model")
        claude = bench.native_cli_command("claude", Path("claude.exe"), "model")
        self.assertIn("--ignore-user-config", codex)
        self.assertIn("--skip-git-repo-check", codex)
        self.assertIn("--safe-mode", claude)
        self.assertNotIn("--dangerously-skip-permissions", claude)

    def claude_stream(self, *, auxiliary=False):
        usage = {"input_tokens": 6, "output_tokens": 1184,
                 "cache_read_input_tokens": 20971, "cache_creation_input_tokens": 2542}
        model = {"inputTokens": 6, "outputTokens": 1184,
                 "cacheReadInputTokens": 20971, "cacheCreationInputTokens": 2542}
        models = {"coding-model": model}
        if auxiliary:
            models["auxiliary-model"] = {"inputTokens": 2, "outputTokens": 7,
                                         "cacheReadInputTokens": 0, "cacheCreationInputTokens": 3}
        return [{"type": "assistant", "message": {"id": "same-response", "model": "coding-model",
                 "content": [], "usage": {**usage, "output_tokens": 490}}},
                {"type": "assistant", "message": {"id": "same-response", "model": "coding-model",
                 "content": [], "usage": {**usage, "output_tokens": 490}}},
                {"type": "result", "usage": usage, "modelUsage": models}]

    def test_claude_output_comes_from_native_result_not_message_start_placeholders(self):
        observed = bench.usage("claude", "\n".join(map(json.dumps, self.claude_stream())))
        self.assertEqual(observed["counts"]["output"], 1184)
        self.assertEqual(observed["transcriptCounts"]["output"], 490)
        self.assertEqual(observed["mainLoopCounts"]["output"], 1184)
        self.assertEqual(observed["countsSource"], "claude.result.modelUsage")
        self.assertEqual(observed["accountingStatus"], "observed")
        self.assertEqual(observed["observedModels"], ["coding-model"])

    def test_claude_pipeline_includes_auxiliary_model_without_changing_coding_identity(self):
        observed = bench.usage("claude", "\n".join(map(json.dumps, self.claude_stream(auxiliary=True))))
        self.assertEqual(observed["counts"], {"input": 8, "output": 1191, "cacheRead": 20971, "cacheWrite": 2545})
        self.assertEqual(observed["mainLoopCounts"]["output"], 1184)
        self.assertEqual(observed["accountingModels"], ["auxiliary-model", "coding-model"])
        self.assertEqual(observed["observedModels"], ["coding-model"])

    def test_claude_incomplete_or_conflicting_native_totals_never_use_transcript_fallback(self):
        missing = self.claude_stream()[:-1]
        duplicate = self.claude_stream()
        duplicate.append(duplicate[-1])
        invalid_cases = [missing, duplicate]
        for field in ("inputTokens", "outputTokens", "cacheReadInputTokens", "cacheCreationInputTokens"):
            for value in (None, -1, True, "1"):
                stream = self.claude_stream()
                stream[-1]["modelUsage"]["coding-model"][field] = value
                invalid_cases.append(stream)
        inconsistent = self.claude_stream()
        inconsistent[-1]["modelUsage"]["coding-model"]["outputTokens"] = 1
        invalid_cases.append(inconsistent)
        no_models = self.claude_stream()
        del no_models[-1]["modelUsage"]
        invalid_cases.append(no_models)
        no_loop = self.claude_stream()
        del no_loop[-1]["usage"]
        invalid_cases.append(no_loop)
        for stream in invalid_cases:
            with self.subTest(stream=stream):
                observed = bench.usage("claude", "\n".join(map(json.dumps, stream)))
                self.assertIsNone(observed["counts"])
                self.assertNotEqual(observed["accountingStatus"], "observed")

    def test_corrupt_stream_and_invalid_codex_counters_are_unknown(self):
        raw = "\n".join(map(json.dumps, self.claude_stream())) + "\n{broken"
        self.assertIsNone(bench.usage("claude", raw)["counts"])
        for counts in ({"input_tokens": 10, "cached_input_tokens": 11, "output_tokens": 2},
                       {"input_tokens": True, "cached_input_tokens": 0, "output_tokens": 2},
                       {"input_tokens": 10, "cached_input_tokens": 0, "output_tokens": -1}):
            self.assertIsNone(bench.usage("codex", json.dumps({"type": "turn.completed", "usage": counts}))["counts"])

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
