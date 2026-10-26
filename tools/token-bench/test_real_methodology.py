"""Focused offline contracts for the real-project benchmark boundary."""
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import real_methodology as bench


class RealMethodologyContracts(unittest.TestCase):
    def make_task(self, root: Path, *, fixed: bool = True, identifier: str = "task-one") -> dict:
        parent = root / "parent"
        known_fix = root / "fix"
        oracle = root / "oracle"
        for directory in (parent, known_fix, oracle):
            directory.mkdir(parents=True)
        source = "VALUE = 2\n" if fixed else "VALUE = 1\n"
        (parent / "subject.py").write_text("VALUE = 1\n", encoding="utf-8")
        (known_fix / "subject.py").write_text(source, encoding="utf-8")
        public = "from subject import VALUE\nassert VALUE in (1, 2)\n"
        for directory in (parent, known_fix):
            (directory / "test_public.py").write_text(public, encoding="utf-8")
        (oracle / "test_hidden.py").write_text("from subject import VALUE\nassert VALUE == 2\n", encoding="utf-8")
        return {"id": identifier, "parentSha": "a" * 40, "knownFixSha": "b" * 40,
                "parentSnapshot": str(parent), "fixSnapshot": str(known_fix),
                "scope": ["subject.py"], "productionImportClosure": ["subject.py"],
                "publicTests": ["test_public.py"], "request": "Set VALUE to 2.",
                "hiddenOracle": {"root": str(oracle), "file": "test_hidden.py",
                                 "command": [sys.executable, "test_hidden.py"]}}

    def registry(self, root: Path) -> Path:
        tasks = [self.make_task(root / f"task-{index}", identifier=f"task-{index}") for index in range(3)]
        profiles = [{"id": f"profile-{index}", "prompt": "Implement carefully."} for index in range(7)]
        path = root / "registry.json"
        path.write_text(json.dumps({"schema": "real-methodology-registry/1",
                                    "tasks": tasks, "profiles": profiles}), encoding="utf-8")
        return path

    def test_qualification_requires_parent_public_green_and_hidden_red(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            registry = self.registry(root)
            result = bench.qualify(bench.load_registry(registry), root / "qualification.json")
            self.assertTrue(result["passed"], result)
            self.assertTrue(all(row["parent"]["public"] == 0 and row["parent"]["hidden"] != 0
                                for row in result["tasks"]))

    def test_weak_hidden_oracle_cannot_qualify(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            registry = json.loads((self.registry(root)).read_text(encoding="utf-8"))
            for task in registry["tasks"]:
                oracle = Path(task["hiddenOracle"]["root"]) / "test_hidden.py"
                oracle.write_text("pass\n", encoding="utf-8")
            with self.assertRaises(AssertionError):
                self.assertTrue(bench.qualify(registry, root / "qualification.json")["passed"])

    def test_freeze_is_seeded_three_by_seven_by_two(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            registry = self.registry(root)
            args = type("Args", (), {"registry": registry, "codex": Path(sys.executable),
                                      "claude": Path(sys.executable), "codex_model": "codex",
                                      "claude_model": "claude"})()
            original = bench.verify_identity
            bench.verify_identity = lambda executable, cli, root: {"path": str(executable), "sha256": bench.digest(executable), "version": "test", "subscription": True}
            try:
                manifest = bench.freeze(args)
            finally:
                bench.verify_identity = original
            self.assertEqual(len(manifest["plannedAttempts"]), 42)
            self.assertEqual({row["cli"] for row in manifest["plannedAttempts"]}, {"codex", "claude"})
            self.assertEqual([row["id"] for row in manifest["plannedAttempts"]],
                             [f"{index:03d}" for index in range(42)])

    def test_evaluator_uses_parent_public_test_after_candidate_changes_it(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            task = self.make_task(root / "task")
            candidate = root / "candidate"
            candidate.mkdir()
            (candidate / "subject.py").write_text("VALUE = 2\n", encoding="utf-8")
            (candidate / "test_public.py").write_text("raise SystemExit(0)\n", encoding="utf-8")
            result = bench.evaluate(task, candidate, root / "evaluation")
            self.assertTrue(result["public"]["accepted"])
            self.assertTrue(result["hidden"]["accepted"])

    def test_report_keeps_reserved_cell_incomplete(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            registry = self.registry(root)
            args = type("Args", (), {"registry": registry, "codex": Path(sys.executable),
                                      "claude": Path(sys.executable), "codex_model": "codex",
                                      "claude_model": "claude"})()
            original = bench.verify_identity
            bench.verify_identity = lambda executable, cli, root: {"path": str(executable), "sha256": bench.digest(executable), "version": "test", "subscription": True}
            try:
                manifest = bench.freeze(args)
            finally:
                bench.verify_identity = original
            manifest_path = root / "manifest.json"
            bench.publish(manifest_path, manifest)
            output = root / "results"
            output.mkdir()
            bench.publish(output / "000.reserved", {"reserved": True})
            result = bench.report(type("Args", (), {"manifest": manifest_path, "output": output})())
            self.assertFalse(result["complete"])
            self.assertEqual(result["attempts"][0]["status"], "incomplete")

    def test_scope_rejects_parent_traversal(self):
        task = {"id": "bad", "scope": ["../secret.py"], "productionImportClosure": ["../secret.py"],
                "publicTests": ["test_public.py"]}
        with self.assertRaises(ValueError):
            bench.task_paths(task)


if __name__ == "__main__":
    unittest.main()
