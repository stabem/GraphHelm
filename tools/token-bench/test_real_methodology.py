"""Focused contracts for real-project snapshot isolation and truthful outcomes."""
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import real_methodology as bench


class RealMethodologyContracts(unittest.TestCase):
    def make_task(self, root: Path, identifier: str = "task-one") -> dict:
        parent, fixed, oracle = root / "parent", root / "fix", root / "oracle"
        for directory in (parent, fixed, oracle):
            directory.mkdir(parents=True)
        for directory, value in ((parent, 1), (fixed, 2)):
            (directory / "subject.py").write_text(f"VALUE = {value}\n", encoding="utf-8")
            (directory / "tests").mkdir()
            (directory / "tests" / "test_actual.py").write_text(
                "from subject import VALUE\nassert VALUE in (1, 2)\n", encoding="utf-8")
            (directory / "test_public.py").write_text(
                "import runpy\nrunpy.run_path('tests/test_actual.py', run_name='__main__')\nprint('BENCHMARK_COMPLETE')\n",
                encoding="utf-8")
        (oracle / "test_hidden.py").write_text(
            "from subject import VALUE\ntry:\n    assert VALUE == 2\nexcept AssertionError:\n    print('BENCH_ASSERTION_FAILURE')\n    raise\nprint('BENCHMARK_COMPLETE')\n", encoding="utf-8")
        return {"id": identifier, "parentSha": "a" * 40, "knownFixSha": "b" * 40,
                "parentSnapshot": str(parent), "fixSnapshot": str(fixed),
                "scope": ["subject.py"], "productionImportClosure": ["subject.py"],
                "publicTests": ["tests/test_actual.py"], "supportFiles": ["test_public.py"],
                "request": "Set VALUE to 2.",
                "completion": {"public": {"stdoutMarker": "BENCHMARK_COMPLETE"},
                               "hidden": {"stdoutMarker": "BENCHMARK_COMPLETE",
                                          "assertionMarker": "BENCH_ASSERTION_FAILURE"}},
                "hiddenOracle": {"root": str(oracle), "files": ["test_hidden.py"],
                                 "command": [sys.executable, "test_hidden.py"]}}

    def registry(self, root: Path) -> Path:
        tasks = [self.make_task(root / f"task-{index}", f"task-{index}") for index in range(3)]
        profiles = [{"id": f"profile-{index}", "prompt": "Implement carefully."} for index in range(7)]
        path = root / "registry.json"
        path.write_text(json.dumps({"schema": "real-methodology-registry/1",
                                    "tasks": tasks, "profiles": profiles}), encoding="utf-8")
        return path

    def test_qualification_requires_intended_parent_assertion_red(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            result = bench.qualify(bench.load_registry(self.registry(root)), root / "qualification.json")
            self.assertTrue(result["passed"], result)
            self.assertEqual(result["tasks"][0]["parent"]["hiddenReason"], "intended assertion failure")

    def test_weak_oracle_or_infrastructure_red_cannot_qualify(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            registry_path = self.registry(root)
            registry = json.loads(registry_path.read_text(encoding="utf-8"))
            for task in registry["tasks"]:
                (Path(task["hiddenOracle"]["root"]) / "test_hidden.py").write_text("pass\n", encoding="utf-8")
            self.assertFalse(bench.qualify(registry, root / "qualification.json")["passed"])
            self.assertFalse(bench.classify_hidden_red({"exit": 2, "timeout": False,
                                                         "cleanupConfirmed": True, "stdout": "", "stderr": "import failed"})[0])

    def test_freeze_is_seeded_and_captures_all_registry_provenance(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            registry = self.registry(root)
            readable_python = Path(bench.base.python_identity()["path"])
            args = type("Args", (), {"registry": registry, "output": root / "manifest.json",
                                      "codex": readable_python, "claude": readable_python,
                                      "codex_model": "codex", "claude_model": "claude"})()
            original = bench.base.identity
            bench.base.identity = lambda executable, cli, cwd: {"path": str(executable), "sha256": bench.digest(executable), "version": "test", "subscription": True}
            try:
                manifest = bench.freeze(args)
            finally:
                bench.base.identity = original
            self.assertEqual(len(manifest["plannedAttempts"]), 42)
            self.assertIn("task-0:parent", manifest["registryFiles"])
            self.assertEqual(manifest["parserSha256"], bench.digest(bench.HOME / "methodology.py"))

    def test_evaluator_restores_original_nested_public_test(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            task = self.make_task(root / "task")
            candidate = root / "candidate"
            candidate.mkdir()
            (candidate / "subject.py").write_text("VALUE = 2\n", encoding="utf-8")
            (candidate / "tests").mkdir()
            (candidate / "tests" / "test_actual.py").write_text("raise SystemExit(0)\n", encoding="utf-8")
            result = bench.evaluator(task, candidate, root / "evaluation", bench.registry_hashes({"tasks": [task]}))
            self.assertEqual(result["public"]["exit"], 0)
            self.assertEqual(result["hidden"]["exit"], 0)

    def test_exit_zero_without_intended_hidden_result_is_not_red_or_pass(self):
        result = {"exit": 0, "timeout": False, "cleanupConfirmed": True, "stdout": "", "stderr": ""}
        self.assertEqual(bench.classify_hidden_red(result), (False, "oracle unexpectedly passed"))

    def test_scope_rejects_parent_traversal(self):
        with self.assertRaises(ValueError):
            bench.task_paths({"id": "bad", "scope": ["../secret.py"],
                              "productionImportClosure": ["../secret.py"],
                              "publicTests": ["test.py"], "supportFiles": ["test_public.py"]})

    def test_source_snapshot_rejects_oversized_file(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "large.py"
            path.write_bytes(b"x" * (bench.MAX_FILE + 1))
            with self.assertRaises(ValueError):
                bench.safe_file(Path(directory), "large.py")

    def test_external_directory_fingerprint_changes_when_dependency_changes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "node_modules"
            root.mkdir()
            (root / "package.json").write_text("{}", encoding="utf-8")
            task = {"externalDependencies": [{"path": str(root), "kind": "directory"}]}
            first = bench.external_dependency_hashes(task)
            (root / "package.json").write_text("{\"changed\":true}", encoding="utf-8")
            second = bench.external_dependency_hashes(task)
            self.assertNotEqual(first, second)

    def test_declared_dependency_mount_is_created_at_exact_target(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            dependency = root / "deps"
            dependency.mkdir()
            workspace = root / "workspace"
            workspace.mkdir()
            task = {"externalDependencies": [{"path": str(dependency), "kind": "directory",
                                                "mount": "apps/studio/node_modules"}],
                    "scope": ["app.py"], "productionImportClosure": ["app.py"],
                    "publicTests": ["test.py"], "supportFiles": ["test_public.py"]}
            for name in ("app.py", "test.py", "test_public.py"):
                (workspace / name).write_text("# fixture\n", encoding="utf-8")
            bench.prepare_mounts(task, workspace)
            target = workspace / "apps" / "studio" / "node_modules"
            self.assertTrue(target.exists())
            self.assertEqual(target.resolve(), dependency.resolve())
            before = bench.candidate_files(workspace)
            bench.enforce_scope(task, workspace, before)

    def test_declared_mount_rejects_redirected_parent_before_writing(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            dependency = root / "deps"
            dependency.mkdir()
            outside = root / "outside"
            outside.mkdir()
            workspace = root / "workspace"
            workspace.mkdir()
            redirected = workspace / "apps"
            if os.name == "nt":
                import _winapi
                _winapi.CreateJunction(str(outside), str(redirected))
            else:
                redirected.symlink_to(outside, target_is_directory=True)
            task = {"externalDependencies": [{"path": str(dependency), "kind": "directory",
                                                "mount": "apps/studio/node_modules"}]}
            with mock.patch.object(bench.os.path, "isjunction", None, create=True):
                with self.assertRaises(ValueError):
                    bench.prepare_mounts(task, workspace)
            self.assertFalse((outside / "studio" / "node_modules").exists())


if __name__ == "__main__":
    unittest.main()
