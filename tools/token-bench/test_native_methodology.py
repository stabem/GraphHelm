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
    def test_codex_root_wire_counters_use_provider_field_names(self):
        self.assertEqual(bench._codex_root_counters({"counts": {
            "input_tokens": 65656, "output_tokens": 72, "cached_input_tokens": 43392}}),
            {"input": 65656, "output": 72, "cacheRead": 43392, "cacheWrite": 0})
        with self.assertRaises(ValueError):
            bench._codex_root_counters({"counts": {"input": 65656, "output": 72}})

    def test_review_binding_requires_committed_patch_and_pins_git_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            def git(*args):
                result = subprocess.run(["git", *args], cwd=root, capture_output=True,
                                        text=True, check=True,
                                        creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
                return result.stdout.strip()
            git("init", "-q")
            git("config", "user.name", "Benchmark")
            git("config", "user.email", "benchmark@example.invalid")
            subject = root / "subject.py"
            subject.write_text("old\n", encoding="utf-8")
            git("add", "subject.py")
            git("commit", "-qm", "baseline")
            baseline = git("rev-parse", "HEAD")
            workflow = {"baselineHead": baseline, "taskRefs": ["task-1"]}
            subject.write_text("new\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "committed candidate"):
                bench._write_native_review_binding(workflow, root)
            git("add", "subject.py")
            git("commit", "-qm", "candidate")
            path = bench._write_native_review_binding(workflow, root)
            binding = json.loads(path.read_text(encoding="utf-8"))
            self.assertEqual(binding["gitHead"], git("rev-parse", "HEAD"))
            patch = subprocess.run(["git", "-c", "core.autocrlf=false", "diff", "--binary",
                                    baseline, binding["gitHead"]], cwd=root, capture_output=True,
                                   check=True, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0)).stdout
            import hashlib
            self.assertEqual(binding["patchSha256"], hashlib.sha256(patch).hexdigest())

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

    def test_candidate_hash_includes_declared_production_and_new_regression(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            production = root / "src" / "binding.py"
            regression = root / "tests" / "test_binding.py"
            production.parent.mkdir()
            regression.parent.mkdir()
            production.write_text("before\n", encoding="utf-8")
            regression.write_text("test before\n", encoding="utf-8")
            workflow = {"pinnedPublicFiles": [], "methodAssetDigests": {}, "phases": [],
                        "taskContext": {"scope": ["src/binding.py"],
                                        "allowedTestPaths": ["tests/test_binding.py"]}}
            paths = bench._declared_candidate_paths(workflow, root)
            self.assertIn("src/binding.py", paths)
            self.assertIn("tests/test_binding.py", paths)
            initial = bench.candidate_tree_hash(root, paths)
            production.write_text("after\n", encoding="utf-8")
            self.assertNotEqual(initial, bench.candidate_tree_hash(root, paths))
            production.write_text("before\n", encoding="utf-8")
            regression.write_text("test after\n", encoding="utf-8")
            self.assertNotEqual(initial, bench.candidate_tree_hash(root, paths))

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
            project = root / "project"
            (project / "report").mkdir(parents=True)
            stream = fixtures / "claude-child.jsonl"
            stdout = raw / "native.stdout.jsonl"
            stderr = raw / "native.stderr.log"
            session = "11111111-1111-4111-8111-111111111111"
            native_rows = [json.loads(line) for line in stream.read_text(encoding="utf-8").splitlines()]
            next(row for row in native_rows if row.get("type") == "result")["session_id"] = session
            stdout.write_text("\n".join(json.dumps(row) for row in native_rows) + "\n", encoding="utf-8")
            stderr.write_text("", encoding="utf-8")
            archive_root = root / "archive" / "setup-attempt-1"
            archive_root.mkdir(parents=True)
            archived_root = archive_root / "root.jsonl"
            archived_root.write_text(json.dumps({"type": "user", "sessionId": session,
                "cwd": str(project), "version": "2.1.310", "isSidechain": False,
                "agentId": None}) + "\n", encoding="utf-8")
            workflow = self.workflow()
            workflow["phases"] = [workflow["phases"][0]]
            contract = project / "PROJECT_CONTRACT.md"
            contract.write_text("frozen public context\n", encoding="utf-8")
            before_dynamic = bench.candidate_tree_hash(project, bench._declared_candidate_paths(workflow, project))
            artifact = project / "report" / "result.json"
            artifact.write_text('{"ok":true}\n', encoding="utf-8")
            workflow["projectRoot"] = str(project)
            workflow["phases"][0]["requiredArtifactGlobs"] = ["report/*.json"]
            self.assertIn("report/result.json", bench._declared_candidate_paths(workflow, project))
            after_dynamic = bench.candidate_tree_hash(project, bench._declared_candidate_paths(workflow, project))
            self.assertNotEqual(before_dynamic, after_dynamic)
            plan = bench.phase_plan(workflow, {**workflow["taskContext"], "phase": "setup"},
                                    plan_id="spec-kit-frozen")
            archive_result = evidence.claude_pipeline(stdout)
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
                       "skillLoad": {"status": "observed"},
                       "cli": "claude", "requestedModel": "claude-sonnet-5-5",
                       "servedModels": ["claude-sonnet-5-5"],
                       "usage": {"counts": counts, "observedModels": [],
                                  "accountingModels": ["claude-sonnet-5-5"]},
                       "rootWireUsage": {"counts": counts},
                       "quotaBefore": {"allowed": True}, "quotaAfter": {"allowed": True},
                       "archivedSession": {"status": "observed", "total": counts,
                                            "rootSession": session,
                                            "archivedRoot": str(archived_root),
                                            "archive": {"root.jsonl": bench.base.digest(archived_root)}}}
            manifest = {"schema": "native-methodology-manifest/1", "workflows": [workflow],
                        "inputs": {"spec-kit": {"publicTree": bench._public_tree(project)}},
                        "models": {"claude": "claude-sonnet-5-5"},
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
            receipt["archivedSession"]["rootSession"] = "22222222-2222-4222-8222-222222222222"
            (destination / "phase-receipt.json").write_text(json.dumps(receipt), encoding="utf-8")
            swapped_root = bench.report_manifest(manifest_path, output)
            self.assertEqual(swapped_root["attempts"][0]["status"], "incomplete")
            self.assertIsNone(swapped_root["usageAggregate"])
            receipt["archivedSession"]["rootSession"] = session
            unrelated = archive_root / "unrelated.txt"
            unrelated.write_text("unrelated evidence", encoding="utf-8")
            receipt["archivedSession"]["archive"] = {"unrelated.txt": bench.base.digest(unrelated)}
            (destination / "phase-receipt.json").write_text(json.dumps(receipt), encoding="utf-8")
            unhashed_root = bench.report_manifest(manifest_path, output)
            self.assertEqual(unhashed_root["attempts"][0]["status"], "incomplete")
            self.assertIsNone(unhashed_root["usageAggregate"])
            receipt["archivedSession"]["archive"] = {"root.jsonl": bench.base.digest(archived_root)}
            receipt["usage"]["counts"]["output"] += 1
            (destination / "phase-receipt.json").write_text(json.dumps(receipt), encoding="utf-8")
            forged_count = bench.report_manifest(manifest_path, output)
            self.assertEqual(forged_count["attempts"][0]["status"], "incomplete")
            self.assertIsNone(forged_count["usageAggregate"])
            receipt["usage"]["counts"]["output"] -= 1
            phase_spec = workflow["phases"][0]
            phase_spec["requiredOperations"] = [{"name": "setup", "commandPattern": "missing-native-command"}]
            receipt["attempt"]["planSha256"] = bench.phase_plan(
                workflow, {**workflow["taskContext"], "phase": "setup"},
                plan_id="spec-kit-frozen")["planSha256"]
            receipt["operations"] = {"status": "observed", "sha256": "forged"}
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            receipt["manifestSha256"] = bench._json_hash(manifest)
            (destination / "phase-receipt.json").write_text(json.dumps(receipt), encoding="utf-8")
            operation_forgery = bench.report_manifest(manifest_path, output)
            self.assertEqual(operation_forgery["attempts"][0]["status"], "incomplete")
            phase_spec["requiredOperations"] = []
            receipt["attempt"]["planSha256"] = plan["planSha256"]
            receipt.pop("operations")
            manifest["models"]["claude"] = "different-frozen-model"
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            receipt["manifestSha256"] = bench._json_hash(manifest)
            (destination / "phase-receipt.json").write_text(json.dumps(receipt), encoding="utf-8")
            model_forgery = bench.report_manifest(manifest_path, output)
            self.assertEqual(model_forgery["attempts"][0]["status"], "incomplete")
            self.assertIsNone(model_forgery["usageGroups"])
            receipt["requestedModel"] = "different-frozen-model"
            receipt["servedModels"] = ["different-frozen-model"]
            receipt["usage"]["accountingModels"] = ["different-frozen-model"]
            (destination / "phase-receipt.json").write_text(json.dumps(receipt), encoding="utf-8")
            model_wire_forgery = bench.report_manifest(manifest_path, output)
            self.assertEqual(model_wire_forgery["attempts"][0]["status"], "incomplete")
            receipt["requestedModel"] = "claude-sonnet-5-5"
            receipt["servedModels"] = ["claude-sonnet-5-5"]
            receipt["usage"]["accountingModels"] = ["claude-sonnet-5-5"]
            manifest["models"]["claude"] = "claude-sonnet-5-5"
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            receipt["manifestSha256"] = bench._json_hash(manifest)
            (destination / "phase-receipt.json").write_text(json.dumps(receipt), encoding="utf-8")
            skill = root / "native-skill.md"
            skill.write_text("pinned native skill", encoding="utf-8")
            phase_spec["requiredWitnessKinds"] = ["native_stream", "native_skill"]
            phase_spec["requiredWitnesses"] = [{"kind": "native_skill", "path": str(skill),
                                                  "sha256": bench.base.digest(skill)}]
            receipt["attempt"]["planSha256"] = bench.phase_plan(
                workflow, {**workflow["taskContext"], "phase": "setup"},
                plan_id="spec-kit-frozen")["planSha256"]
            receipt["skillLoad"] = {"status": "observed", "loads": [
                {"status": "observed", "skillSha256": bench.base.digest(skill)}]}
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            receipt["manifestSha256"] = bench._json_hash(manifest)
            (destination / "phase-receipt.json").write_text(json.dumps(receipt), encoding="utf-8")
            skill_forgery = bench.report_manifest(manifest_path, output)
            self.assertEqual(skill_forgery["attempts"][0]["status"], "incomplete")
            phase_spec["requiredWitnessKinds"] = ["native_stream"]
            phase_spec["requiredWitnesses"] = []
            receipt["attempt"]["planSha256"] = plan["planSha256"]
            receipt["skillLoad"] = {"status": "observed"}
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            receipt["manifestSha256"] = bench._json_hash(manifest)
            (destination / "phase-receipt.json").write_text(json.dumps(receipt), encoding="utf-8")
            extra = output / "phase-setup-retry-1"
            extra.mkdir()
            retried = bench.report_manifest(manifest_path, output)
            self.assertEqual(retried["unplannedAttemptDirs"], [extra.name])
            self.assertIsNone(retried["usageAggregate"])
            self.assertFalse(retried["complete"])
            extra.rmdir()
            injected = project / "injected.txt"
            injected.write_text("unexpected new context\n", encoding="utf-8")
            added = bench.report_manifest(manifest_path, output)
            self.assertEqual(added["attempts"][0]["status"], "incomplete")
            injected.unlink()
            contract.write_text("changed context\n", encoding="utf-8")
            drifted = bench.report_manifest(manifest_path, output)
            self.assertEqual(drifted["attempts"][0]["status"], "incomplete")
            contract.write_text("frozen public context\n", encoding="utf-8")
            receipt["capture"]["timeout"] = True
            (destination / "phase-receipt.json").write_text(json.dumps(receipt), encoding="utf-8")
            forged = bench.report_manifest(manifest_path, output)
            self.assertEqual(forged["attempts"][0]["status"], "incomplete")
            self.assertIsNone(forged["usageAggregate"])


if __name__ == "__main__":
    unittest.main()
