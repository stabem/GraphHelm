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
                "phases": [
                    {"id": "setup", "command": ["native", "init"], "frameworkStage": "specify"},
                    {"id": "planner", "command": ["native", "plan"], "frameworkStage": "plan"},
                    {"id": "author", "command": ["native", "implement"], "frameworkStage": "tasks"},
                    {"id": "independent-review", "command": ["native", "review"], "frameworkStage": "review"},
                    {"id": "fix", "command": ["native", "fix"], "frameworkStage": "converge"},
                    {"id": "verify", "command": ["native", "verify"], "frameworkStage": "verify"},
                    {"id": "archive", "command": ["native", "archive"], "frameworkStage": "archive"},
                ]}

    def plan(self):
        return bench.phase_plan(self.workflow(), {"task": "task-1", "scope": ["src.py"],
                                                   "publicTests": ["test_public.py"], "request": "fix it",
                                                   "phase": "setup"}, plan_id="plan-1")

    def test_native_config_keeps_project_skills_and_session_logs(self):
        config = bench.native_cli_config("codex", Path(sys.executable), "pinned")
        self.assertEqual(config["projectSkills"], "enabled")
        self.assertEqual(config["sessionPersistence"], "enabled")
        self.assertNotIn("--ignore-user-config", config["argv"])
        self.assertNotIn("--disable", config["argv"])

    def test_framework_specific_stage_names_are_preserved(self):
        plan = self.plan()
        self.assertEqual([row["frameworkStage"] for row in plan["phases"]],
                         ["specify", "plan", "tasks", "review", "converge", "verify", "archive"])

    def test_private_context_and_self_reported_fidelity_are_rejected(self):
        context = {"task": "task-1", "scope": [], "publicTests": [], "request": "x",
                   "workflow": "spec-kit", "phase": "setup", "apiKey": "secret"}
        with self.assertRaises(ValueError):
            bench.validate_public_context(context)
        self.assertEqual(bench.fidelity_witnesses([{"kind": "model_text", "path": "claim"}])["status"], "incomplete")

    def test_missing_or_duplicate_owned_child_records_keep_usage_unknown(self):
        records = [{"sessionId": "child-a", "owned": True, "usage": {"input": 1, "output": 2}},
                   {"sessionId": "child-a", "owned": True, "usage": {"input": 3, "output": 4}}]
        result = bench.reconcile_nested_usage(records, expected_sessions=["child-a", "child-b"])
        self.assertEqual(result["status"], "unknown")
        self.assertIsNone(result["total"])
        self.assertTrue(result["duplicateSessions"])

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
        result = bench.accepted_patch_endpoint(patch_path=".git/accepted.patch", patch_sha256="a" * 64,
                                               reviewed=True, local_git=True)
        self.assertEqual(result["status"], "observed")
        self.assertFalse(result["published"])


if __name__ == "__main__":
    unittest.main()
