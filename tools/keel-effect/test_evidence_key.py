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
    "core/process/src/lib.rs": ('pub const SCHEMA: &str = include_str!(\n'
                                '    "../../../schemas/event-envelope.schema.json"\n);\n'),
    "core/events/tests/conformance.rs": 'const F: &[u8] = include_bytes!("../../../conformance/valid/event.json");\n',
    "schemas/event-envelope.schema.json": '{"type": "object"}\n',
    "conformance/valid/event.json": '{"id": "e1"}\n',
    "schemas/unrelated.schema.json": '{}\n',
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
        for path, text in (("core/process/src/lib.rs", FILES["core/process/src/lib.rs"] + "pub fn command() {}\n"),
                           ("core/testkit/src/lib.rs", "pub fn fixture() { }\n"),
                           ("Cargo.lock", "# lock 2\n")):
            with self.subTest(path=path):
                head = commit(self.repo, {path: text}, path)
                result = ek.decide(self.repo, self.record, head)
                self.assertEqual((result["decision"], result["changed"]), ("invalidate", [path]))
                self.record = ek.make_record(self.repo, head, "rerun ok")

    def test_a_schema_embedded_by_a_linked_crate_invalidates_on_a_one_space_edit(self):
        head = commit(self.repo, {"schemas/event-envelope.schema.json": '{"type":  "object"}\n'}, "schema")
        result = ek.decide(self.repo, self.record, head)
        self.assertEqual((result["decision"], result["changed"]),
                         ("invalidate", ["schemas/event-envelope.schema.json"]))

    def test_a_conformance_fixture_embedded_by_an_observer_test_invalidates(self):
        head = commit(self.repo, {"conformance/valid/event.json": '{"id": "e2"}\n'}, "fixture")
        result = ek.decide(self.repo, self.record, head)
        self.assertEqual((result["decision"], result["changed"]), ("invalidate", ["conformance/valid/event.json"]))

    def test_a_schema_no_closure_crate_embeds_reuses(self):
        head = commit(self.repo, {"schemas/unrelated.schema.json": '{"x": 1}\n'}, "unrelated")
        self.assertEqual(ek.decide(self.repo, self.record, head)["decision"], "reuse")

    def test_a_non_literal_include_fails_closed_and_cannot_be_recorded(self):
        for text in ('const S: &str = include_str!(concat!(env!("OUT_DIR"), "/s.json"));\n',
                     'const S: &str = include_str!(r"../../../schemas/event-envelope.schema.json");\n',
                     'const S: &str = include_str!("../../../schemas/absent.json");\n'):
            with self.subTest(text=text):
                head = commit(self.repo, {"core/events/src/gen.rs": text}, "non-literal")
                result = ek.decide(self.repo, self.record, head)
                self.assertEqual(result["decision"], "invalidate")
                self.assertEqual(len(result["unknown"]), 1)
                with self.assertRaises(ValueError):
                    ek.make_record(self.repo, head, "ok")

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


# One real call per effect kind; the keys must equal the detector's, so a kind removed from the
# detector, or one it stops matching, fails below.
SAMPLES = {
    "file_write": "std::fs::OpenOptions::new().append(true).open(p)",
    "fsync": "journal.sync_data()",
    "file_lock": "file.lock_exclusive()",
    "network": "std::net::TcpStream::connect(addr)",
    "process_spawn": 'graphhelm_process::command("git")',
    "ffi": "unsafe { LockFile(h) }",
}


def append_fn(body):
    return f"fn committed_events_for_idempotency() -> bool {{ let _ = {body}; true }}\n"


class EffectSet(unittest.TestCase):
    setUp = EvidenceKey.setUp
    tearDown = EvidenceKey.tearDown

    def test_adding_each_effect_kind_to_a_closure_file_reports_it_as_added(self):
        self.assertEqual(set(SAMPLES), set(ek.EFFECT_KINDS))
        for kind, call in SAMPLES.items():
            with self.subTest(kind=kind):
                head = commit(self.repo, {"core/events/src/local.rs": append_fn(call)}, kind)
                result = ek.decide(self.repo, self.record, head)
                self.assertEqual(result["decision"], "invalidate")
                self.assertEqual(result["effects_changed"]["summary"], f"effects changed: +{kind}")
                self.assertEqual(result["effects_changed"]["sites"], {kind: ["core/events/src/local.rs"]})
                commit(self.repo, {"core/events/src/local.rs": FILES["core/events/src/local.rs"]}, "back")

    def test_a_spawn_in_a_linked_dependency_is_an_added_effect_of_the_append_path(self):
        head = commit(self.repo, {"core/process/src/lib.rs": FILES["core/process/src/lib.rs"]
                                  + 'pub fn run() { std::process::Command::new("x"); }\n'}, "spawn")
        self.assertEqual(ek.decide(self.repo, self.record, head)["effects_changed"]["added"], ["process_spawn"])

    def test_a_pure_refactor_changes_files_but_not_the_effect_set(self):
        base = commit(self.repo, {"core/events/src/local.rs": "fn append(f: &File) { f.sync_data(); }\n"}, "fsync")
        record = ek.make_record(self.repo, base, "ok")
        self.assertEqual(record["effects"], {"fsync": ["core/events/src/local.rs"]})
        head = commit(self.repo, {"core/events/src/local.rs": "fn persist(journal: &File) {\n    flush(journal);\n}\n",
                                  "core/events/src/flush.rs": "pub fn flush(j: &File) { j.sync_data(); }\n"},
                      "refactor")
        result = ek.decide(self.repo, record, head)
        self.assertEqual(result["decision"], "invalidate")
        self.assertEqual(result["changed"], ["core/events/src/flush.rs", "core/events/src/local.rs"])
        self.assertEqual(result["effects_changed"]["summary"], "effects unchanged")

    def test_a_removed_effect_is_reported_as_removed(self):
        base = commit(self.repo, {"core/events/src/local.rs": append_fn("file.lock_exclusive()")}, "lock")
        record = ek.make_record(self.repo, base, "ok")
        head = commit(self.repo, {"core/events/src/local.rs": FILES["core/events/src/local.rs"]}, "unlock")
        self.assertEqual(ek.decide(self.repo, record, head)["effects_changed"]["summary"],
                         "effects changed: -file_lock")

    def test_comments_a_mutex_lock_and_test_sources_are_not_effects_of_the_path(self):
        head = commit(self.repo, {
            "core/events/src/local.rs": ("// a TcpStream would be wrong here; so would Command::new\n"
                                         "/* unsafe { } */ fn f(m: &Mutex<u8>) { m.lock(); }\n"
                                         'const DENY: &[&str] = &["reqwest::get(", "Command::new"];\n'),
            "core/events/tests/append_effect_contract.rs": append_fn("std::net::TcpStream::connect(a)")}, "noise")
        self.assertEqual(ek.decide(self.repo, self.record, head)["effects_changed"]["summary"], "effects unchanged")

    def test_a_record_without_an_effect_set_reports_unknown_not_unchanged(self):
        head = commit(self.repo, {"core/events/src/local.rs": append_fn("f.sync_all()")}, "fsync")
        legacy = {k: v for k, v in self.record.items() if k != "effects"}
        changed = ek.decide(self.repo, legacy, head)["effects_changed"]
        self.assertEqual((changed["unknown"], changed["added"]), (["fsync"], []))


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
        now = ek.state(REPO, record["commit"])
        self.assertEqual((now["key"], now["effects"]), (record["key"], record["effects"]))
        self.assertTrue({"file_write", "fsync", "file_lock"} <= record["effects"].keys())


if __name__ == "__main__":
    unittest.main()
