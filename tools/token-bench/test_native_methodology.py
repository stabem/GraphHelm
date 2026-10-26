"""Meaningful contracts for native workflow fidelity and fail-closed evidence."""
import tempfile
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import native_methodology as bench


class NativeMethodologyContracts(unittest.TestCase):
    def workflow(self):
        return {"id": "spec-kit", "version": "v1", "framework": "SpecKit", "maxFixAttempts": 2,
                "projectRoot": str(Path.cwd()), "taskRefs": ["task-1"], "pinnedPublicFiles": [], "methodAssetDigests": {},
                "phases": [
                    {"id": "setup", "command": ["native", "init"], "prompt": "setup", "frameworkStage": "specify"},
                    {"id": "planner", "command": ["native", "plan"], "prompt": "plan", "frameworkStage": "plan"},
                    {"id": "author", "command": ["native", "implement"], "prompt": "implement", "frameworkStage": "tasks"},
                    {"id": "independent-review", "command": ["native", "review"], "prompt": "review", "frameworkStage": "review"},
                    {"id": "fix", "command": ["native", "fix"], "prompt": "fix", "frameworkStage": "converge"},
                    {"id": "verify", "command": ["native", "verify"], "prompt": "verify", "frameworkStage": "verify"},
                    {"id": "archive", "command": ["native", "archive"], "prompt": "archive", "frameworkStage": "archive"},
                ]}

    def plan(self):
        return bench.phase_plan(self.workflow(), {"task": "task-1", "scope": ["src.py"],
                                                   "publicTests": ["test_public.py"], "request": "fix it",
                                                   "phase": "setup"}, plan_id="plan-1")

    def test_native_config_keeps_project_skills_and_session_logs(self):
        config = bench.native_cli_config("codex", Path(sys.executable), "pinned")
        self.assertEqual(config["projectSkills"], "enabled")
        self.assertEqual(config["sessionPersistence"], "enabled")
        self.assertIn("--ignore-user-config", config["argv"])
        self.assertNotIn("--disable", config["argv"])

    def test_framework_specific_stage_names_are_preserved(self):
        plan = self.plan()
        self.assertEqual([row["frameworkStage"] for row in plan["phases"]],
                         ["specify", "plan", "tasks", "review", "converge", "verify", "archive"])

    def test_registry_cannot_qualify_from_schema_only(self):
        workflow = self.workflow()
        workflow.pop("pinnedPublicFiles")
        with self.assertRaises(ValueError):
            bench.validate_registry({"schema": bench.SCHEMA, "version": "v1", "workflows": [workflow]})

    def test_private_context_and_self_reported_fidelity_are_rejected(self):
        context = {"task": "task-1", "scope": [], "publicTests": [], "request": "x",
                   "workflow": "spec-kit", "phase": "setup", "apiKey": "secret"}
        with self.assertRaises(ValueError):
            bench.validate_public_context(context)
        self.assertEqual(bench.fidelity_witnesses([{"kind": "model_text", "path": "claim"}])["status"], "incomplete")

    def test_fidelity_requires_real_native_stream_and_artifact_hashes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            stream = root / "native.jsonl"
            artifact = root / "skill-rollout.json"
            stream.write_text('{"type":"tool_use","name":"Read"}\n{"type":"result"}\n', encoding="utf-8")
            artifact.write_text('{"skill":"specify","loaded":true}\n', encoding="utf-8")
            witnesses = [{"kind": "native_stream", "path": str(stream), "sha256": bench.base.digest(stream)},
                         {"kind": "native_skill", "path": str(artifact), "sha256": bench.base.digest(artifact)}]
            self.assertEqual(bench.fidelity_witnesses(witnesses)["status"], "observed")
            stream.write_text('{"type":"result"}\n', encoding="utf-8")
            self.assertEqual(bench.fidelity_witnesses(witnesses)["status"], "incomplete")

    def test_missing_or_duplicate_owned_child_records_keep_usage_unknown(self):
        records = [{"sessionId": "child-a", "owned": True, "usage": {"input": 1, "output": 2}},
                   {"sessionId": "child-a", "owned": True, "usage": {"input": 3, "output": 4}}]
        result = bench.reconcile_nested_usage(records, expected_sessions=["child-a", "child-b"])
        self.assertEqual(result["status"], "unknown")
        self.assertIsNone(result["total"])
        self.assertTrue(result["duplicateSessions"])

    def test_child_usage_rejects_boolean_and_negative_counters(self):
        result = bench.reconcile_nested_usage([{"sessionId": "child-a", "owned": True,
                                                "rawSha256": "a" * 64,
                                                "usage": {"input": True, "output": 1, "cacheRead": 0, "cacheWrite": 0}}],
                                              expected_sessions=["child-a"])
        self.assertEqual(result["status"], "unknown")
        self.assertIsNone(result["total"])

    def test_child_usage_in_parent_pipeline_is_validated_but_not_added_twice(self):
        result = bench.reconcile_nested_usage([{"sessionId": "child-a", "owned": True,
                                                "rawSha256": "a" * 64, "includedInParent": True,
                                                "usage": {"input": 2, "output": 21, "cacheRead": 0, "cacheWrite": 1215}}],
                                              expected_sessions=["child-a"],
                                              parent_usage={"input": 6, "output": 270, "cacheRead": 5773, "cacheWrite": 7381})
        self.assertEqual(result["status"], "observed")
        self.assertEqual(result["total"]["output"], 270)

    def test_owned_child_collector_uses_claude_tool_result_not_forwarded_placeholder(self):
        result = bench.collect_owned_child_usage("claude", [{"owned": True, "sessionId": "parent",
            "parentSessionId": "root", "spawned": ["child-a"], "rawSha256": "p" * 64},
            {"owned": True, "sessionId": "child-a", "parentSessionId": "parent", "rawSha256": "c" * 64,
             "tool_use_result": {"usage": {"input": 2, "output": 21, "cacheRead": 0, "cacheWrite": 1215}},
             "assistantUsage": {"input": 2, "output": 8, "cacheRead": 0, "cacheWrite": 1215},
             "includedInParent": True}], parent_session="parent")
        self.assertEqual(result["status"], "unknown")
        self.assertIsNone(result["total"])

    def test_denied_native_method_operation_is_incomplete(self):
        denied = bench.validate_method_operation_witness(
            {"toolCall": "resolve-template", "toolResult": {"is_error": True}},
            expected_command="resolve-template", expected_artifact="constitution")
        self.assertEqual(denied["status"], "incomplete")
        observed = bench.validate_method_operation_witness(
            {"toolCall": "resolve-template", "toolResult": {"status": "completed",
             "artifact": "constitution", "artifactChanged": True}},
            expected_command="resolve-template", expected_artifact="constitution")
        self.assertEqual(observed["status"], "observed")

    def test_completion_is_incomplete_until_all_controls_and_costs_are_observed(self):
        attempt = bench.phase_attempt(self.plan(), "author", "attempt-1")
        result = bench.complete_attempt(attempt, auth=True, quota=True, cleanup=True,
                                        usage={"status": "unknown", "total": None},
                                        witnesses=[], artifact_hashes_value={}, costs={})
        self.assertEqual(result["status"], "incomplete")
        self.assertEqual(result["completeness"], "unknown")

    def test_broker_writes_fresh_native_receipt_and_preserves_hash(self):
        plan = self.plan()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            def runner(attempt, stage):
                return {"status": "complete", "native": True, "predecessorArtifacts": None}
            result = bench.broker_phase(plan, "setup", "attempt-1", runner=runner, root=root)
            self.assertEqual(result["status"], "complete")
            self.assertTrue(result["artifactHashes"]["native-result.json"])
            self.assertTrue((root / "phase-setup-attempt-1" / "native-result.json").is_file())

    def test_local_reviewed_patch_endpoint_never_claims_published_merge(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            patch = root / "accepted.patch"
            review = root / "review.json"
            evaluator = root / "evaluator.json"
            patch.write_text("local patch\n", encoding="utf-8")
            review.write_text('{"verdict":"APPROVE","reviewer":"reviewer","patchSha256":"%s"}' % bench.base.digest(patch), encoding="utf-8")
            evaluator.write_text('{"status":"passed"}', encoding="utf-8")
            result = bench.accepted_patch_endpoint(patch_path=str(patch), review_path=str(review),
                                                   evaluator_path=str(evaluator), author="author",
                                                   reviewer="reviewer", local_git=True)
            self.assertEqual(result["status"], "observed")
            self.assertFalse(result["published"])
            review.write_text('{"verdict":"APPROVE","reviewer":"author","patchSha256":"%s"}' % bench.base.digest(patch), encoding="utf-8")
            self.assertEqual(bench.accepted_patch_endpoint(patch_path=str(patch), review_path=str(review),
                             evaluator_path=str(evaluator), author="author", reviewer="author", local_git=True)["status"], "incomplete")


if __name__ == "__main__":
    unittest.main()
