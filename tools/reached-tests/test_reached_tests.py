"""Observer for reached_tests.py (#361): `python -m unittest tools/reached-tests/test_reached_tests.py`.

A small fake workspace, no cargo and no git: policy <- execution <- cli, an adapter nobody depends
on, a cli test bundle, and one embedded schema. Each case is a real diff shape from this repository.
"""
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import reached_tests as rt  # noqa: E402

PACKAGES = [
    {"name": "graphhelm-policy", "dir": "core/policy", "deps": [], "tests": [{"name": "keel", "src": "core/policy/tests/keel.rs"}], "bundles": {}},
    {"name": "graphhelm-execution", "dir": "core/execution", "deps": ["graphhelm-policy"], "tests": [], "bundles": {}},
    {"name": "graphhelm-cli", "dir": "apps/cli", "deps": ["graphhelm-execution"],
     "tests": [{"name": "development", "src": "apps/cli/tests/bundle_development.rs"}, {"name": "extension_cli", "src": "apps/cli/tests/extension_cli.rs"},
               {"name": "jpd_plugin", "src": "apps/cli/tests/jpd_plugin.rs"}],
     "bundles": {"keel_check": "development", "development_plugin": "development", "journey_scope_guard": "journeys"}},
    {"name": "graphhelm-host-adoption", "dir": "adapters/host-adoption", "deps": [],
     "tests": [{"name": "release_packages", "src": "adapters/host-adoption/tests/release_packages.rs"}], "bundles": {}},
]
EMBEDDED = {"extensions/builtin/graphhelm-jpd/schemas/journey-contract.schema.json": {"graphhelm-cli"}}


def reach(*changed, repo=None):
    return rt.reach(list(changed), PACKAGES, EMBEDDED, repo)


class Reach(unittest.TestCase):
    def test_a_journey_flow_file_reaches_the_scope_guard_and_journey_validate(self):
        # #504 review: a flows-only diff (#430-style) printed no test, only "unmapped".
        whole, single, studio, tools, validate, other = reach(".graphhelm/journeys/checkout.journey.yaml")
        self.assertEqual(other, [])
        self.assertIn(("graphhelm-cli", "journeys", "journey_scope_guard"), single)
        self.assertIn("graphhelm --json journey validate --all", tools)

    def test_a_journey_runtime_change_prints_the_ignored_browser_observers(self):
        # #504 review: the #[ignore] browser cells are the real proof of a driver or replay change.
        for path in ("tools/journey-driver/driver.mjs", "apps/cli/src/commands/journey_replay.rs",
                     "apps/cli/tests/journey_live_browser.rs"):
            *_, tools, _, _ = reach(path)
            self.assertIn(rt.BROWSER_OBSERVERS, tools, path)
        *_, tools, _, _ = reach("core/policy/src/lib.rs")
        self.assertNotIn(rt.BROWSER_OBSERVERS, tools)

    def test_a_test_file_in_a_bundle_reaches_only_its_module(self):
        whole, single, *_ = reach("apps/cli/tests/keel_check.rs")
        self.assertEqual(whole, set())
        self.assertEqual(single, {("graphhelm-cli", "development", "keel_check")})
        self.assertIn("cargo +1.97.1 test --locked -p graphhelm-cli --test development keel_check::", rt.commands(whole, single, []))

    def test_a_test_file_of_its_own_reaches_its_target(self):
        _, single, *_ = reach("core/policy/tests/keel.rs")
        self.assertEqual(single, {("graphhelm-policy", "keel", None)})

    def test_a_crate_source_reaches_the_crate_and_every_dependent(self):
        whole, single, *_ = reach("core/policy/src/keel_plan.rs")
        self.assertEqual(whole, {"graphhelm-policy", "graphhelm-execution", "graphhelm-cli"})
        self.assertEqual(single, set())

    def test_a_leaf_source_reaches_only_its_own_crate(self):
        whole, *_ = reach("adapters/host-adoption/src/lib.rs")
        self.assertEqual(whole, {"graphhelm-host-adoption"})

    def test_support_and_fixtures_reach_their_package_tests(self):
        whole, *_ = reach("apps/cli/tests/support/mod.rs")
        self.assertEqual(whole, {"graphhelm-cli"})

    def test_an_embedded_file_reaches_the_crates_that_embed_it(self):
        whole, *_ = reach("extensions/builtin/graphhelm-jpd/schemas/journey-contract.schema.json")
        self.assertIn("graphhelm-cli", whole)

    def test_an_extension_change_reaches_its_runtime_readers_and_validate(self):
        whole, single, _, _, validate, other = reach("extensions/builtin/graphhelm-development-contracts/skills/merge/SKILL.md")
        self.assertEqual(whole, set())
        self.assertIn(("graphhelm-host-adoption", "release_packages", None), single)
        self.assertIn(("graphhelm-cli", "development", "development_plugin"), single)
        self.assertEqual(validate, ["extensions/builtin/graphhelm-development-contracts"])
        self.assertEqual(other, [])

    def test_studio_files_reach_vitest_related_and_nothing_in_rust(self):
        whole, single, studio, *_ = reach("apps/studio/src/runtime/team-tasks.ts")
        self.assertEqual((whole, single), (set(), set()))
        commands = rt.commands(whole, single, studio)
        self.assertEqual(commands, ["(cd apps/studio && npx vitest related --run src/runtime/team-tasks.ts && npx tsc -b)"])

    def test_docs_reach_nothing_and_are_not_reported_unmapped(self):
        whole, single, studio, tools, validate, other = reach("docs/process/DELIVERY.md", "docs/journeys/x/shot.jpg")
        self.assertEqual((whole, single, studio, tools, validate, other), (set(), set(), [], [], [], []))
        self.assertEqual(rt.commands(whole, single, studio), [])

    def test_a_tool_change_reaches_that_tools_own_tests(self):
        with tempfile.TemporaryDirectory() as repo:
            tool = Path(repo, "tools", "task-record")
            tool.mkdir(parents=True)
            (tool / "task_record.py").write_text("", encoding="utf-8")
            (tool / "test_task_record.py").write_text("", encoding="utf-8")
            *_, tools, _, other = reach("tools/task-record/task_record.py", repo=repo)
        self.assertEqual(tools, ["python -m unittest tools/task-record/test_task_record.py"])
        self.assertEqual(other, [])

    def test_a_path_no_rule_maps_is_reported(self):
        *_, other = reach("ci/gate.ps1")
        self.assertEqual(other, ["ci/gate.ps1"])

    def test_dependents_is_transitive_and_includes_the_start(self):
        self.assertEqual(rt.dependents(PACKAGES, {"graphhelm-policy"}), {"graphhelm-policy", "graphhelm-execution", "graphhelm-cli"})


if __name__ == "__main__":
    unittest.main()
