"""Meaningful contracts for native workflow fidelity and fail-closed evidence."""
import tempfile
from pathlib import Path
import sys
import subprocess
import importlib.util
import shutil
import json
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import native_methodology as bench
import native_session_evidence as evidence


class NativeMethodologyContracts(unittest.TestCase):
    def workflow(self):
        return {"id": "spec-kit", "version": "v1", "framework": "SpecKit", "maxFixAttempts": 2,
                "projectRoot": str(Path.cwd()), "taskRefs": ["task-1"], "pinnedPublicFiles": [], "methodAssetDigests": {},
                "taskContext": {"task": "task-1", "scope": [], "publicTests": [], "request": "fix it"},
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

    def test_endpoint_binds_native_terminal_review_real_git_diff_and_completed_wrappers(self):
        import json
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            git_root = root / "candidate"
            git_root.mkdir()
            def git(*args):
                result = bench.base.capture(["git", *args], git_root, deadline=15)
                self.assertEqual(result["exit"], 0, result["stderr"])
                return result["stdout"]
            git("init", "--quiet")
            git("config", "user.email", "test@example.invalid")
            git("config", "user.name", "native-test")
            (git_root / "candidate.txt").write_text("old", encoding="utf-8")
            git("add", "candidate.txt")
            git("commit", "--quiet", "-m", "baseline")
            baseline = git("rev-parse", "HEAD").strip()
            (git_root / "candidate.txt").write_text("candidate", encoding="utf-8")
            git("add", "candidate.txt")
            git("commit", "--quiet", "-m", "candidate")
            head = git("rev-parse", "HEAD").strip()
            patch = root / "candidate.patch"
            patch.write_bytes(git("-c", "core.autocrlf=false", "diff", "--binary", baseline, head).encode("utf-8"))
            patch_hash = bench.base.digest(patch)
            session = "4fb1b27b-870f-4a50-9989-754a959cfb5e"
            terminal = {"verdict": "APPROVE", "reviewer": "reviewer", "taskId": "task-1",
                        "gitHead": head, "patchSha256": patch_hash}
            native = root / "review.jsonl"
            init = {"type": "system", "subtype": "init", "session_id": session}
            final = {"type": "result", "subtype": "success", "is_error": False,
                     "session_id": session, "result": json.dumps(terminal)}
            native.write_text(json.dumps(init)+"\n"+json.dumps(final)+"\n", encoding="utf-8")
            review = root / "review.json"
            review_data = {**terminal, "baseHead": baseline, "reviewerSessionSha256": bench.base.digest(native),
                           "reviewerRootSessionId": session,
                           "authorRootSessionId": "01a176e5-8357-7001-89bb-8ea26df340cd"}
            review.write_text(json.dumps(review_data), encoding="utf-8")
            registry = root / "registry.json"
            task = {"id": "task-1", "scope": ["candidate.txt"], "completion": {
                    "public": {"stdoutMarker": "PUBLIC_DONE"}, "hidden": {"stdoutMarker": "HIDDEN_DONE"}}}
            registry.write_text(json.dumps({"tasks": [task]}), encoding="utf-8")
            evaluation = {}
            for phase in ("public", "hidden"):
                marker = task["completion"][phase]["stdoutMarker"]
                script = "from pathlib import Path; assert Path('candidate.txt').read_text() == 'candidate'; print("+repr(marker)+")"
                observed = bench.base.capture([sys.executable, "-I", "-c", script], git_root, deadline=15)
                observed["cwd"] = str(git_root)
                evaluation[phase] = observed
            evaluator = root / "evaluator.json"
            evaluator_data = {"status": "passed", "taskId": "task-1", "gitHead": head,
                "baseHead": baseline, "patchSha256": patch_hash, "registryPath": str(registry),
                "registrySha256": bench.base.digest(registry), "evaluation": evaluation,
                "productionDigests": {"candidate.txt": bench.base.digest(git_root / "candidate.txt")}}
            evaluator.write_text(json.dumps(evaluator_data), encoding="utf-8")
            def endpoint():
                return bench.accepted_patch_endpoint(patch_path=str(patch), review_path=str(review),
                    evaluator_path=str(evaluator), author="author", reviewer="reviewer", local_git=True,
                    reviewer_session_path=str(native), git_head_path=str(git_root), task_id="task-1")
            result = endpoint()
            self.assertEqual(result["status"], "observed", result)
            self.assertFalse(endpoint()["published"])
            # A matching declared hash cannot turn unrelated text into this Git patch.
            patch.write_text("invented patch", encoding="utf-8")
            self.assertEqual(endpoint()["status"], "incomplete")
            patch.write_bytes(git("-c", "core.autocrlf=false", "diff", "--binary", baseline, head).encode("utf-8"))
            evaluation["hidden"]["stdout"] = ""
            evaluator.write_text(json.dumps(evaluator_data), encoding="utf-8")
            self.assertEqual(endpoint()["status"], "incomplete")
            evaluation["hidden"]["stdout"] = "HIDDEN_DONE"
            evaluator.write_text(json.dumps(evaluator_data), encoding="utf-8")
            # A top-level self-declared verdict is not a native provider response.
            native.write_text(json.dumps(terminal), encoding="utf-8")
            review_data["reviewerSessionSha256"] = bench.base.digest(native)
            review.write_text(json.dumps(review_data), encoding="utf-8")
            self.assertEqual(endpoint()["status"], "incomplete")

    def test_claude_archived_total_allows_cache_context_above_new_input(self):
        """The real Claude nested-session fixture has cache context larger than new input.

        This protects the provider boundary: Claude reports cache context separately, while
        Codex cache-read remains a subset constraint. Existing tests did not exercise the
        methodology normalizer with an actual archived Claude total.
        """
        fixtures = Path(__file__).resolve().parent / "fixtures" / "native-sessions"
        children = sorted(fixtures.glob("claude-depth2-agent-*.jsonl"))
        archived = evidence.claude_owned_tree(
            fixtures / "claude-depth2-stream.jsonl", children,
            project=Path("D:/gh/codex-scope-cleanup/native-2074-probes/claude-child-depth2-001"),
            session_id="4fb1b27b-870f-4a50-9989-754a959cfb5e",
        )
        self.assertEqual(archived["status"], "observed")
        self.assertEqual(bench._usage_total(archived["total"], cli="claude"), archived["total"])
        self.assertIsNone(bench._usage_total(archived["total"], cli="codex"))
        metrics = bench._usage_metrics(archived["total"], cli="claude")
        self.assertEqual(metrics, {"contextInputTokens": 21134,
                                    "totalProcessedTokens": 21664})

    def test_methodology_usage_imports_without_legacy_tasks_manifest(self):
        """The native parser must load without run.py, whose startup requires tasks.json."""
        source = Path(__file__).resolve().parent
        fixture = source / "fixtures" / "native-sessions" / "claude-depth2-stream.jsonl"
        with tempfile.TemporaryDirectory() as directory:
            isolated = Path(directory)
            shutil.copy2(source / "methodology.py", isolated / "methodology.py")
            shutil.copy2(source / "native_usage.py", isolated / "native_usage.py")
            spec = importlib.util.spec_from_file_location("isolated_methodology", isolated / "methodology.py")
            module = importlib.util.module_from_spec(spec)
            sys.path.insert(0, str(isolated))
            try:
                spec.loader.exec_module(module)
                result = module.usage("claude", fixture.read_text(encoding="utf-8"))
            finally:
                sys.path.pop(0)
            self.assertEqual(result["accountingStatus"], "observed")
            self.assertEqual(result["counts"], {"input": 10, "output": 530,
                                                 "cacheRead": 9256, "cacheWrite": 11868})

    def test_report_replays_captured_claude_stdout_and_archive(self):
        """A captured native stream and archived root transcript produce an observed phase."""
        fixtures = Path(__file__).resolve().parent / "fixtures" / "native-sessions"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "attempts"
            raw = output / "phase-setup-attempt-1"
            raw.mkdir(parents=True)
            stream = fixtures / "claude-child.jsonl"
            stdout = raw / "native.stdout.jsonl"
            stderr = raw / "native.stderr.log"
            stdout.write_bytes(stream.read_bytes())
            stderr.write_text("", encoding="utf-8")
            archive_root = root / "archive" / "setup-attempt-1"
            archive_root.mkdir(parents=True)
            archived_root = archive_root / "root.jsonl"
            archived_root.write_bytes(stream.read_bytes())
            workflow = self.workflow()
            workflow["phases"] = [workflow["phases"][0]]
            project = root / "project"
            (project / "report").mkdir(parents=True)
            artifact = project / "report" / "result.json"
            artifact.write_text('{"ok":true}\n', encoding="utf-8")
            workflow["projectRoot"] = str(project)
            workflow["phases"][0]["requiredArtifactGlobs"] = ["report/*.json"]
            plan = bench.phase_plan(workflow, {**workflow["taskContext"], "phase": "setup"},
                                    plan_id="spec-kit-frozen")
            archive_result = evidence.claude_pipeline(archived_root)
            counts = archive_result["total"]
            candidate = bench.candidate_tree_hash(Path(workflow["projectRoot"]),
                                                   bench._declared_candidate_paths(workflow, project))
            receipt = {"workflow": "spec-kit", "phase": "setup", "manifestSha256": "",
                       "attempt": {"attemptId": "attempt-1", "planId": "spec-kit-frozen",
                                    "planSha256": plan["planSha256"], "phase": "setup"},
                       "capture": {"exit": 0, "timeout": False, "cleanupConfirmed": True, "truncated": False},
                       "rawSha256": {"native.stdout.jsonl": bench.base.digest(stdout),
                                     "native.stderr.log": bench.base.digest(stderr)},
                       "candidateBeforeSha256": candidate, "candidateAfterSha256": candidate,
                       "requiredArtifactGlobHashes": {"report/result.json": bench.base.digest(artifact)},
                       "status": "complete", "fidelity": {"status": "observed"},
                       "cli": "claude", "usage": {"counts": counts},
                       "rootWireUsage": {"counts": counts},
                       "archivedSession": {"status": "observed", "total": counts,
                                            "rootSession": "child-session",
                                            "archivedRoot": str(archived_root),
                                            "archive": {"root.jsonl": bench.base.digest(archived_root)}}}
            manifest = {"schema": "native-methodology-manifest/1", "workflows": [workflow],
                        "inputs": {}, "models": {"claude": "claude-sonnet-5-5"},
                        "instrumentSha256": bench.base.digest(Path(bench.__file__)),
                        "instrumentSources": {name: bench.base.digest(Path(bench.__file__).parent / name)
                                               for name in ("methodology.py", "native_usage.py", "real_methodology.py",
                                                            "native_methodology.py", "native_session_evidence.py",
                                                            "native_session_capture.py", "native_quota.py")},
                        "plannedPhases": [{"workflow": "spec-kit", "phase": "setup",
                                           "promptSha256": bench._json_hash("setup"), "requiredArtifacts": [],
                                           "requiredWitnesses": [], "requiredOperations": [],
                                           "requiredArtifactGlobs": []}], "winner": None}
            manifest_path = root / "manifest.json"
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            receipt["manifestSha256"] = bench._json_hash(manifest)
            destination = output / "spec-kit--setup"
            destination.mkdir()
            (destination / "phase-receipt.json").write_text(json.dumps(receipt), encoding="utf-8")
            report = bench.report_manifest(manifest_path, output)
            self.assertEqual(report["attempts"][0]["status"], "complete")
            self.assertEqual(report["usageAggregate"], counts)


if __name__ == "__main__":
    unittest.main()
