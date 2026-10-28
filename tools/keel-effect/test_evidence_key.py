"""Contracts of the #1476 evidence key: a change outside the append path's closure reuses the
lost-ack evidence, a change inside it invalidates it. Driven on a throwaway git repository, plus
one read of this repository's own manifests; nothing is built.
"""
import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
SPEC = importlib.util.spec_from_file_location("evidence_key", HERE / "evidence_key.py")
ek = importlib.util.module_from_spec(SPEC)
sys.modules["evidence_key"] = ek
SPEC.loader.exec_module(ek)

FILES = {
    "Cargo.toml": '[workspace]\nmembers = ["core/events", "core/process", "core/other", "core/testkit"]\n',
    "Cargo.lock": "# lock\n",
    "core/events/Cargo.toml": ('[package]\nname = "graphhelm-events"\n[dependencies]\n'
                               'graphhelm-process = { path = "../process" }\nserde.workspace = true\n'
                               '[dev-dependencies]\ntestkit = { path = "../testkit" }\n'),
    "core/events/src/local.rs": "fn committed_events_for_idempotency() -> bool { true }\n",
    "core/events/tests/append_effect_contract.rs": "#[test] fn lost_ack() {}\n",
    "core/process/Cargo.toml": '[package]\nname = "graphhelm-process"\n',
    "core/process/src/lib.rs": "pub fn command() {}\n",
    "core/testkit/Cargo.toml": '[package]\nname = "testkit"\n',
    "core/testkit/src/lib.rs": "pub fn fixture() {}\n",
    "core/other/Cargo.toml": ('[package]\nname = "other"\n[dependencies]\n'
                              'graphhelm-events = { path = "../events" }\n'),
    "core/other/src/lib.rs": "pub fn uses_events() {}\n",
    "studio/src/App.tsx": "export const App = () => null;\n",
}


def sh(repo, *args):
    subprocess.run(["git", *args], cwd=repo, check=True, capture_output=True, creationflags=ek.NO_WINDOW)


def commit(repo, changes, message):
    for path, text in changes.items():
        target = repo / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text, encoding="utf-8", newline="\n")
    sh(repo, "add", "-A")
    sh(repo, "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", message)
    return ek.git(repo, "rev-parse", "HEAD").strip()


class EvidenceKey(unittest.TestCase):
    def setUp(self):
        self._dir = tempfile.TemporaryDirectory()
        self.repo = Path(self._dir.name)
        sh(self.repo, "init", "-q")
        sh(self.repo, "config", "core.autocrlf", "false")
        self.base = commit(self.repo, FILES, "base")
        self.record = ek.make_record(self.repo, self.base, "test result: ok. 1 passed")

    def tearDown(self):
        self._dir.cleanup()

    def test_the_closure_is_the_append_crate_its_path_deps_and_its_test_deps_not_its_dependents(self):
        self.assertEqual(self.record["closure"], ["core/events", "core/process", "core/testkit"])

    def test_a_ui_only_change_reuses_the_recorded_evidence(self):
        head = commit(self.repo, {"studio/src/App.tsx": "export const App = () => 1;\n"}, "ui")
        result = ek.decide(self.repo, self.record, head)
        self.assertEqual((result["decision"], result["changed"]), ("reuse", []))

    def test_a_dependent_crate_change_reuses_the_recorded_evidence(self):
        head = commit(self.repo, {"core/other/src/lib.rs": "pub fn uses_events() { }\n"}, "dependent")
        self.assertEqual(ek.decide(self.repo, self.record, head)["decision"], "reuse")

    def test_a_dedup_change_invalidates_the_evidence_and_names_the_file_and_the_observer(self):
        head = commit(self.repo, {"core/events/src/local.rs":
                                  "fn committed_events_for_idempotency() -> bool { false }\n"}, "dedup")
        result = ek.decide(self.repo, self.record, head)
        self.assertEqual(result["decision"], "invalidate")
        self.assertEqual(result["changed"], ["core/events/src/local.rs"])
        self.assertEqual(result["rerun"], ek.OBSERVER)

    def test_a_change_in_a_linked_dependency_or_the_lockfile_invalidates(self):
        for path, text in (("core/process/src/lib.rs", "pub fn command() { }\n"),
                           ("core/testkit/src/lib.rs", "pub fn fixture() { }\n"),
                           ("Cargo.lock", "# lock 2\n")):
            with self.subTest(path=path):
                head = commit(self.repo, {path: text}, path)
                result = ek.decide(self.repo, self.record, head)
                self.assertEqual((result["decision"], result["changed"]), ("invalidate", [path]))
                self.record = ek.make_record(self.repo, head, "rerun ok")

    def test_a_new_path_dependency_widens_the_closure_and_invalidates(self):
        manifest = FILES["core/events/Cargo.toml"].replace(
            "[dev-dependencies]", 'other = { path = "../other" }\n[dev-dependencies]')
        head = commit(self.repo, {"core/events/Cargo.toml": manifest}, "new dep")
        result = ek.decide(self.repo, self.record, head)
        self.assertEqual(result["decision"], "invalidate")
        self.assertIn("core/other/src/lib.rs", result["changed"])

    def test_the_cli_exits_0_on_reuse_and_1_on_invalidate(self):
        (self.repo / ek.RECORD).parent.mkdir(parents=True)
        self.assertEqual(ek.main(["record", "--repo", str(self.repo), "--rev", self.base, "--observed", "ok"]), 0)
        commit(self.repo, {"studio/src/App.tsx": "x\n"}, "ui")
        self.assertEqual(ek.main(["check", "--repo", str(self.repo)]), 0)
        commit(self.repo, {"core/events/src/local.rs": "y\n"}, "dedup")
        self.assertEqual(ek.main(["check", "--repo", str(self.repo)]), 1)


class ThisRepository(unittest.TestCase):
    def test_the_real_closure_holds_the_append_path_and_not_the_cli_or_studio(self):
        dirs = ek.closure(REPO, "HEAD")
        self.assertIn("core/events", dirs)
        self.assertIn("core/protocols", dirs)
        self.assertNotIn("apps/cli", dirs)
        self.assertFalse(any(d.startswith("studio") for d in dirs))

    def test_the_committed_record_names_this_guarantee_and_a_closure_it_can_recompute(self):
        import json
        record = json.loads((REPO / ek.RECORD).read_text(encoding="utf-8"))
        self.assertEqual((record["guarantee"], record["observer"]), (ek.GUARANTEE, ek.OBSERVER))
        self.assertEqual(ek.state(REPO, record["commit"])["key"], record["key"])


if __name__ == "__main__":
    unittest.main()
