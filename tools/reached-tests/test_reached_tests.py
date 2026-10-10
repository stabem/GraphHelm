"""Observer for reached_tests.py (#361): `python -m unittest tools/reached-tests/test_reached_tests.py`.

Most cells use a small fake workspace, no cargo and no git: policy <- execution <- cli, an adapter
nobody depends on, a cli test bundle, and one embedded schema. The repository-bundle cell reads
offline Cargo metadata. Each case is a real diff shape from this repository.
"""
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

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
    def test_repository_cli_files_select_bundled_modules(self):
        # Cost: offline cargo metadata and file reads. Fixture-only bundle coverage misses a
        # manifest that still auto-discovers individual targets or forgets a newly added file.
        repo = Path(__file__).resolve().parents[2]
        packages = rt.workspace(repo)
        cli = next(package for package in packages if package["name"] == "graphhelm-cli")
        self.assertEqual([target["name"] for target in cli["tests"]], ["cli"])
        for source in sorted((repo / "apps/cli/tests").glob("*.rs")):
            if source.stem == "cli":
                continue
            whole, single, *_ = rt.reach([source.relative_to(repo).as_posix()], packages, {}, repo)
            self.assertEqual(whole, set())
            self.assertEqual(single, {("graphhelm-cli", "cli", source.stem)}, source.name)
            self.assertIn(f"cargo +1.97.1 test --locked -p graphhelm-cli --test cli {source.stem}::",
                          rt.commands(whole, single, []))

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
        self.assertIn("-- --ignored --test-threads=2", rt.BROWSER_OBSERVERS)

    def test_rust_package_and_workspace_guard_commands_bound_test_parallelism(self):
        commands = rt.commands({"graphhelm-cli"}, set(), [])
        self.assertIn("cargo +1.97.1 test --locked -p graphhelm-cli -- --test-threads=2", commands)
        self.assertIn(rt.GUARD, commands)

    def test_steps_keep_argv_and_slot_metadata_without_shell_parsing(self):
        package_plans = {"graphhelm-cli": [{"argv": ["cargo", "+1.97.1", "test", "--locked", "-p", "graphhelm-cli",
                                                       "--", "--test-threads=2"],
                                             "cwd": ".", "slot": True}]}
        records = rt.steps({"graphhelm-cli"}, set(), ["apps/studio/src/runtime/team-tasks.ts"],
                           [rt.BROWSER_OBSERVERS], [], package_plans=package_plans)
        self.assertEqual(records[0], {"argv": rt.BROWSER_ARGV, "cwd": ".", "slot": True, "observer": "browser"})
        self.assertEqual(records[1]["argv"], ["cargo", "+1.97.1", "test", "--locked", "-p", "graphhelm-cli",
                                               "--", "--test-threads=2"])
        self.assertEqual(records[-2], {"argv": ["npx", "vitest", "related", "--run", "--maxWorkers=1",
                                                 "src/runtime/team-tasks.ts"],
                                       "cwd": "apps/studio", "slot": False})
        self.assertEqual(records[-1], {"argv": ["npx", "tsc", "-b"], "cwd": "apps/studio", "slot": False})

    def test_whole_package_steps_without_metadata_refuse_unscoped_cargo(self):
        with self.assertRaisesRegex(ValueError, "target metadata"):
            rt.steps({"graphhelm-cli"}, set(), [], package_plans=None)

    def test_lint_scope_does_not_follow_test_dependents(self):
        self.assertEqual(rt.lint_scope(["core/policy/src/keel_plan.rs"], PACKAGES, EMBEDDED), {"graphhelm-policy"})
        commands = rt.commands({"graphhelm-policy", "graphhelm-execution", "graphhelm-cli"}, set(), [],
                               lint_packages={"graphhelm-policy"})
        self.assertIn("cargo +1.97.1 clippy --locked -p graphhelm-policy --all-targets --all-features -- -D warnings", commands)
        self.assertNotIn("-p graphhelm-execution", next(c for c in commands if " clippy " in c))

    def test_whole_package_expands_to_explicit_targets_and_doc_examples(self):
        packages = [{"name": "graphhelm-cli", "targets": [
            {"name": "graphhelm_cli", "kind": "lib", "test": True, "doctest": True},
            {"name": "graphhelm", "kind": "bin", "test": True, "doctest": False},
            {"name": "cli", "kind": "test", "test": True, "doctest": False},
            {"name": "fixture", "kind": "example", "test": False, "doctest": False},
            {"name": "bench", "kind": "bench", "test": False, "doctest": False},
        ]}]
        plans, unsupported = rt.expand_whole_packages({"graphhelm-cli"}, packages)
        self.assertEqual(unsupported, [])
        argv = [step["argv"] for step in plans["graphhelm-cli"]]
        self.assertIn(["cargo", "+1.97.1", "test", "--locked", "-p", "graphhelm-cli",
                       "--lib", "--", "--test-threads=2"], argv)
        self.assertIn(["cargo", "+1.97.1", "test", "--locked", "-p", "graphhelm-cli",
                       "--doc", "--", "--test-threads=2"], argv)
        self.assertIn(["cargo", "+1.97.1", "test", "--locked", "-p", "graphhelm-cli",
                       "--example", "fixture", "--no-run", "--", "--test-threads=2"], argv)
        self.assertFalse(any(step["argv"] == ["cargo", "+1.97.1", "test", "--locked", "-p", "graphhelm-cli",
                                               "--", "--test-threads=2"] for step in plans["graphhelm-cli"]))
        commands = rt.commands({"graphhelm-cli"}, set(), [], package_plans=plans)
        self.assertNotIn("cargo +1.97.1 test --locked -p graphhelm-cli -- --test-threads=2", commands)

    def test_whole_package_required_features_are_explicitly_unsupported(self):
        plans, unsupported = rt.expand_whole_packages({"graphhelm-cli"}, [{"name": "graphhelm-cli", "targets": [
            {"name": "feature_test", "kind": "test", "test": True, "doctest": False,
             "requiredFeatures": ["special"]},
        ]}])
        self.assertEqual(plans["graphhelm-cli"], [])
        self.assertEqual(unsupported[0]["reason"], "target requires features: special")

    def test_missing_target_metadata_has_empty_plan_without_broad_fallback(self):
        plans, unsupported = rt.expand_whole_packages({"graphhelm-cli"}, [])
        self.assertEqual(plans, {"graphhelm-cli": []})
        self.assertIn("metadata unavailable", unsupported[0]["reason"])
        with self.assertRaisesRegex(ValueError, "target metadata"):
            rt.steps({"graphhelm-cli"}, set(), [], package_plans={})

    def test_custom_harness_and_test_enabled_examples_are_explicitly_unsupported(self):
        packages = [{"name": "graphhelm-cli", "targets": [
            {"name": "custom", "kind": "test", "test": True, "harness": False},
            {"name": "example_test", "kind": "example", "test": True, "harness": True},
        ]}]
        plans, unsupported = rt.expand_whole_packages({"graphhelm-cli"}, packages)
        self.assertEqual(plans["graphhelm-cli"], [])
        reasons = {item["target"]: item["reason"] for item in unsupported}
        self.assertIn("custom harness", reasons["custom"])
        self.assertIn("compile-only", reasons["example_test"])

    def test_workspace_reads_harness_from_manifest_fixture(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            manifest = root / "Cargo.toml"
            source = root / "src" / "lib.rs"
            source.parent.mkdir()
            source.write_text("", encoding="utf-8")
            manifest.write_text('[package]\nname = "fixture"\nversion = "0.0.0"\n\n[lib]\nharness = false\n',
                                encoding="utf-8")
            metadata = {"workspace_root": str(root), "packages": [{
                "name": "fixture", "manifest_path": str(manifest), "dependencies": [],
                "targets": [{"name": "fixture", "kind": ["lib"], "src_path": str(source),
                              "test": True, "doctest": False, "required-features": []}],
            }]}
            with patch.object(rt, "run", return_value=json.dumps(metadata)):
                packages = rt.workspace(root)
            self.assertFalse(packages[0]["targets"][0]["harness"])
            self.assertTrue(packages[0]["targets"][0]["harnessKnown"])

    def test_library_doctest_without_library_tests_emits_only_doc_step(self):
        packages = [{"name": "graphhelm-cli", "targets": [
            {"name": "graphhelm_cli", "kind": "lib", "test": False, "doctest": True,
             "harness": True, "harnessKnown": True},
        ]}]
        plans, unsupported = rt.expand_whole_packages({"graphhelm-cli"}, packages)
        self.assertEqual(unsupported, [])
        self.assertEqual([step["argv"][6] for step in plans["graphhelm-cli"]], ["--doc"])

    def test_node_test_path_with_space_is_recovered_from_filesystem(self):
        with tempfile.TemporaryDirectory() as repo:
            path = Path(repo, "tools", "x", "my test.test.mjs")
            path.parent.mkdir(parents=True)
            path.write_text("", encoding="utf-8")
            records = rt.steps(set(), set(), [], ["node --test tools/x/my test.test.mjs"], [], repo=Path(repo))
        self.assertEqual(records[0]["argv"], ["node", "--test", "tools/x/my test.test.mjs"])

    def test_a_test_file_in_a_bundle_reaches_only_its_module(self):
        whole, single, *_ = reach("apps/cli/tests/keel_check.rs")
        self.assertEqual(whole, set())
        self.assertEqual(single, {("graphhelm-cli", "development", "keel_check")})
        self.assertIn("cargo +1.97.1 test --locked -p graphhelm-cli --test development keel_check:: -- --test-threads=2", rt.commands(whole, single, []))

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
        self.assertEqual(commands, ["(cd apps/studio && npx vitest related --run --maxWorkers=1 src/runtime/team-tasks.ts && npx tsc -b)"])

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

    def test_pytest_function_module_is_selected_with_pytest_runner(self):
        with tempfile.TemporaryDirectory() as repo:
            tool = Path(repo, "tools", "token-bench")
            tool.mkdir(parents=True)
            (tool / "test_run.py").write_text("def test_reaches_pytest():\n    assert True\n", encoding="utf-8")
            *_, tools, _, other = reach("tools/token-bench/run.py", repo=repo)
        self.assertEqual(tools, ["python -m pytest tools/token-bench/test_run.py"])
        self.assertEqual(other, [])

    def test_structured_pytest_step_keeps_direct_argv(self):
        records = rt.steps(set(), set(), [], ["python -m pytest tools/token-bench/test_run.py"], [])
        self.assertEqual(records[0]["argv"], ["python", "-m", "pytest", "tools/token-bench/test_run.py"])

    def test_pytest_tool_contract_is_selected_explicitly_without_importing_tests(self):
        with tempfile.TemporaryDirectory() as repo:
            tool = Path(repo, "tools", "example")
            tool.mkdir(parents=True)
            (tool / "tool.py").write_text("", encoding="utf-8")
            (tool / "test_pytest_contract.py").write_text("def test_contract(): pass\n", encoding="utf-8")
            (tool / "test_unittest_contract.py").write_text("import unittest\nclass T(unittest.TestCase): pass\n", encoding="utf-8")
            *_, tools, _, other = reach("tools/example/tool.py", repo=repo)
            records = rt.steps(set(), set(), [], tools, repo=Path(repo))
        self.assertEqual(tools, ["python -m pytest tools/example/test_pytest_contract.py",
                                 "python -m unittest tools/example/test_unittest_contract.py"])
        self.assertEqual([step["argv"] for step in records], [
            ["python", "-m", "pytest", "tools/example/test_pytest_contract.py"],
            ["python", "-m", "unittest", "tools/example/test_unittest_contract.py"],
        ])
        self.assertEqual(other, [])

    def test_a_path_no_rule_maps_is_reported(self):
        *_, other = reach("ci/gate.ps1")
        self.assertEqual(other, ["ci/gate.ps1"])

    def test_dependents_is_transitive_and_includes_the_start(self):
        self.assertEqual(rt.dependents(PACKAGES, {"graphhelm-policy"}), {"graphhelm-policy", "graphhelm-execution", "graphhelm-cli"})


class CliModules(unittest.TestCase):
    """#361 (gh-claude-7's review): a change in one `apps/cli/src/commands` module used to reach the
    whole graphhelm-cli package, so every CLI review ran the full battery. Now it reaches that
    module's unit tests, the tests that read the crate's own source, and the integration tests that
    name its command; shared files and unmatched modules still reach the whole package."""

    def tree(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = Path(tmp.name)
        tests = root / "apps/cli/tests"
        tests.mkdir(parents=True)
        files = {
            "journey_explore_cli.rs": 'cmd.args(["--json", "journey", "explore", "--project"]);',
            "keel_check.rs": 'cmd.args(["keel", "check"]);',
            "surface_completeness.rs": 'let root = env!("CARGO_MANIFEST_DIR"); read(root.join("src/commands/serve/mod.rs"));',
            "extension_cli.rs": 'cmd.args(["extension", "validate"]);',
        }
        for name, body in files.items():
            (tests / name).write_text(body, encoding="utf-8")
        packages = [dict(PACKAGES[2], tests=PACKAGES[2]["tests"] + [
            {"name": n[:-3], "src": f"apps/cli/tests/{n}"} for n in ("journey_explore_cli.rs", "surface_completeness.rs")],
            bundles=dict(PACKAGES[2]["bundles"], keel_check="development"))]
        return root, packages

    def test_a_command_module_reaches_its_unit_tests_source_readers_and_command_tests(self):
        root, packages = self.tree()
        # #487's diff: one command module.
        whole, single, *_ = rt.reach(["apps/cli/src/commands/journey_explore.rs"], packages, {}, root)
        self.assertEqual(whole, set())
        self.assertEqual(single, {("graphhelm-cli", "bin:graphhelm", "commands::journey_explore"),
                                  ("graphhelm-cli", "journey_explore_cli", None)})
        self.assertIn("cargo +1.97.1 test --locked -p graphhelm-cli --bin graphhelm commands::journey_explore:: -- --test-threads=2",
                      rt.commands(whole, single, []))

    def test_fixture_source_strings_do_not_reach_as_readers(self):
        root, packages = self.tree()
        tests = root / "apps/cli/tests"
        (tests / "api_http.rs").write_text(
            'let _root = env!("CARGO_MANIFEST_DIR"); let _fixture = "src/lib.rs";', encoding="utf-8")
        (tests / "runtime_http.rs").write_text(
            'let _root = env!("CARGO_MANIFEST_DIR"); project.join("src");', encoding="utf-8")
        packages[0]["tests"] += [{"name": name, "src": f"apps/cli/tests/{name}.rs"}
                                  for name in ("api_http", "runtime_http")]
        whole, single, *_ = rt.reach(["apps/cli/src/commands/journey_explore.rs"], packages, {}, root)
        self.assertNotIn(("graphhelm-cli", "api_http", None), single)
        self.assertNotIn(("graphhelm-cli", "runtime_http", None), single)

    def test_unknown_source_reader_stays_conservative(self):
        root, packages = self.tree()
        tests = root / "apps/cli/tests"
        (tests / "future_reader.rs").write_text(
            'let _root = env!("CARGO_MANIFEST_DIR"); let _source = "src/unknown.rs";', encoding="utf-8")
        packages[0]["tests"].append({"name": "future_reader", "src": "apps/cli/tests/future_reader.rs"})
        whole, single, *_ = rt.reach(["apps/cli/src/commands/journey_explore.rs"], packages, {}, root)
        self.assertIn(("graphhelm-cli", "future_reader", None), single)

    def test_private_cfg_test_path_reaches_parent_module_and_readers(self):
        root, packages = self.tree()
        source = root / "apps/cli/src/commands"
        source.mkdir(parents=True, exist_ok=True)
        (source / "workspace_slot.rs").write_text(
            '#[cfg(test)]\n#[path = "workspace_slot_tests.rs"]\nmod workspace_slot_tests;\n', encoding="utf-8")
        (source / "workspace_slot_tests.rs").write_text("", encoding="utf-8")
        tests = root / "apps/cli/tests"
        (tests / "workspace_cli.rs").write_text(
            'cmd.args(["workspace", "slot"]);', encoding="utf-8")
        (tests / "source_invariants.rs").write_text(
            'env!("CARGO_MANIFEST_DIR"); "src/any.rs";', encoding="utf-8")
        packages[0]["tests"] += [
            {"name": "workspace_cli", "src": "apps/cli/tests/workspace_cli.rs"},
            {"name": "source_invariants", "src": "apps/cli/tests/source_invariants.rs"},
        ]
        packages[0]["targets"] = [{"name": "graphhelm", "kind": "bin"}]
        whole, single, *_ = rt.reach(
            ["apps/cli/src/commands/workspace_slot_tests.rs"], packages, {}, root)
        self.assertEqual(whole, set())
        self.assertIn(("graphhelm-cli", "bin:graphhelm", "commands::workspace_slot"), single)
        self.assertIn(("graphhelm-cli", "workspace_cli", None), single)
        self.assertIn(("graphhelm-cli", "source_invariants", None), single)

    def test_private_cfg_test_unknown_caller_falls_back_to_whole_package(self):
        root, packages = self.tree()
        source = root / "apps/cli/src/commands"
        source.mkdir(parents=True, exist_ok=True)
        (source / "workspace_slot.rs").write_text(
            '#[cfg(test)]\n#[path = "workspace_slot_tests.rs"]\nmod workspace_slot_tests;\n', encoding="utf-8")
        (source / "workspace_slot_tests.rs").write_text("", encoding="utf-8")
        (source / "unrelated.rs").write_text("use crate::workspace_slot;\n", encoding="utf-8")
        tests = root / "apps/cli/tests"
        (tests / "workspace_cli.rs").write_text('cmd.args(["workspace", "slot"]);', encoding="utf-8")
        packages[0]["tests"] += [{"name": "workspace_cli", "src": "apps/cli/tests/workspace_cli.rs"}]
        packages[0]["targets"] = [{"name": "graphhelm", "kind": "bin"}]
        whole, *_ = rt.reach(["apps/cli/src/commands/workspace_slot_tests.rs"], packages, {}, root)
        self.assertEqual(whole, {"graphhelm-cli"})

    def test_private_cfg_test_without_bin_or_workspace_target_falls_back(self):
        root, packages = self.tree()
        source = root / "apps/cli/src/commands"
        source.mkdir(parents=True, exist_ok=True)
        (source / "workspace_slot.rs").write_text(
            '#[cfg(test)]\n#[path = "workspace_slot_tests.rs"]\nmod workspace_slot_tests;\n', encoding="utf-8")
        (source / "workspace_slot_tests.rs").write_text("", encoding="utf-8")
        packages[0]["targets"] = []
        whole, *_ = rt.reach(["apps/cli/src/commands/workspace_slot_tests.rs"], packages, {}, root)
        self.assertEqual(whole, {"graphhelm-cli"})

    def test_workspace_production_change_keeps_normal_narrow_coverage(self):
        root, packages = self.tree()
        source = root / "apps/cli/src/commands"
        source.mkdir(parents=True, exist_ok=True)
        (source / "workspace.rs").write_text("", encoding="utf-8")
        tests = root / "apps/cli/tests"
        (tests / "workspace_cli.rs").write_text('cmd.args(["workspace"]);', encoding="utf-8")
        packages[0]["tests"] += [{"name": "workspace_cli", "src": "apps/cli/tests/workspace_cli.rs"}]
        packages[0]["targets"] = [{"name": "graphhelm", "kind": "bin"}]
        whole, single, *_ = rt.reach(["apps/cli/src/commands/workspace.rs"], packages, {}, root)
        self.assertEqual(whole, set())
        self.assertIn(("graphhelm-cli", "bin:graphhelm", "commands::workspace"), single)

    def test_workspace_slot_production_file_uses_audited_narrow_coverage(self):
        root, packages = self.tree()
        source = root / "apps/cli/src/commands"
        source.mkdir(parents=True, exist_ok=True)
        (source / "workspace_slot.rs").write_text(
            '#[cfg(test)]\n#[path = "workspace_slot_tests.rs"]\nmod workspace_slot_tests;\n', encoding="utf-8")
        (source / "workspace_slot_tests.rs").write_text("", encoding="utf-8")
        tests = root / "apps/cli/tests"
        (tests / "workspace_cli.rs").write_text('cmd.args(["workspace", "slot"]);', encoding="utf-8")
        packages[0]["tests"] += [{"name": "workspace_cli", "src": "apps/cli/tests/workspace_cli.rs"}]
        packages[0]["targets"] = [{"name": "graphhelm", "kind": "bin"}]
        whole, single, *_ = rt.reach(["apps/cli/src/commands/workspace_slot.rs"], packages, {}, root)
        self.assertEqual(whole, set())
        self.assertIn(("graphhelm-cli", "bin:graphhelm", "commands::workspace_slot"), single)
        self.assertIn(("graphhelm-cli", "workspace_cli", None), single)

    def test_other_private_cfg_test_parent_keeps_normal_module_selection(self):
        root, packages = self.tree()
        source = root / "apps/cli/src/commands"
        source.mkdir(parents=True, exist_ok=True)
        (source / "other_module.rs").write_text(
            '#[cfg(test)]\n#[path = "other_module_tests.rs"]\nmod other_module_tests;\n', encoding="utf-8")
        (source / "other_module_tests.rs").write_text("", encoding="utf-8")
        tests = root / "apps/cli/tests"
        (tests / "other_module_cli.rs").write_text('cmd.args(["other", "module"]);', encoding="utf-8")
        packages[0]["tests"] += [{"name": "other_module_cli", "src": "apps/cli/tests/other_module_cli.rs"}]
        whole, single, *_ = rt.reach(
            ["apps/cli/src/commands/other_module_tests.rs"], packages, {}, root)
        self.assertEqual(whole, set())
        self.assertIn(("graphhelm-cli", "bin:graphhelm", "commands::other_module"), single)

    def test_scoped_reader_reaches_when_its_real_source_is_touched(self):
        texts = {
            Path("surface_completeness.rs"): 'env!("CARGO_MANIFEST_DIR"); "src/commands/serve/mod.rs";',
        }
        packages = [dict(PACKAGES[2], tests=PACKAGES[2]["tests"] + [
            {"name": "surface_completeness", "src": "apps/cli/tests/surface_completeness.rs"}])]
        self.assertIn(("graphhelm-cli", "surface_completeness", None),
                      rt.source_reader_targets(texts, packages, "graphhelm-cli", "src/commands/serve/mod.rs"))
        self.assertNotIn(("graphhelm-cli", "surface_completeness", None),
                         rt.source_reader_targets(texts, packages, "graphhelm-cli", "src/commands/journey_explore.rs"))

    def test_audited_broad_readers_stay_broad(self):
        texts = {
            Path("source_invariants.rs"): 'env!("CARGO_MANIFEST_DIR"); "src/any.rs";',
            Path("attention_inputs_one_feed.rs"): 'env!("CARGO_MANIFEST_DIR"); join("src");',
        }
        packages = [dict(PACKAGES[2], tests=PACKAGES[2]["tests"] + [
            {"name": name, "src": f"apps/cli/tests/{name}.rs"}
            for name in ("source_invariants", "attention_inputs_one_feed")])]
        readers = rt.source_reader_targets(texts, packages, "graphhelm-cli", "src/commands/journey_explore.rs")
        self.assertEqual(readers, {("graphhelm-cli", "source_invariants", None),
                                   ("graphhelm-cli", "attention_inputs_one_feed", None)})

    def test_a_module_other_modules_import_reaches_their_tests_too(self):
        # #530 review (gh-claude-6): journey_live.rs imports Driver/preflight from journey_replay.rs,
        # so a journey_replay change must reach journey_live's tests as well.
        root, packages = self.tree()
        src = root / "apps/cli/src/commands"
        src.mkdir(parents=True)
        (src / "journey_replay.rs").write_text("pub struct Driver;", encoding="utf-8")
        (src / "journey_live.rs").write_text("use super::journey_replay::Driver;", encoding="utf-8")
        (src / "mod.rs").write_text("mod journey_live; mod journey_replay;", encoding="utf-8")
        (root / "apps/cli/tests/journey_live_cli.rs").write_text('cmd.args(["journey", "live"]);', encoding="utf-8")
        (root / "apps/cli/tests/journey_replay_cli.rs").write_text('cmd.args(["journey", "replay"]);', encoding="utf-8")
        packages[0]["tests"] += [{"name": n, "src": f"apps/cli/tests/{n}.rs"} for n in ("journey_live_cli", "journey_replay_cli")]
        whole, single, *_ = rt.reach(["apps/cli/src/commands/journey_replay.rs"], packages, {}, root)
        self.assertEqual(whole, set())
        self.assertTrue({("graphhelm-cli", "bin:graphhelm", "commands::journey_replay"), ("graphhelm-cli", "journey_replay_cli", None),
                         ("graphhelm-cli", "bin:graphhelm", "commands::journey_live"), ("graphhelm-cli", "journey_live_cli", None)} <= single, single)

    def test_a_module_used_outside_commands_reaches_the_whole_package(self):
        root, packages = self.tree()
        src = root / "apps/cli/src"
        (src / "commands").mkdir(parents=True)
        (src / "commands/keel.rs").write_text("pub fn check() {}", encoding="utf-8")
        (src / "output.rs").write_text("fn print() { commands::keel::check(); }", encoding="utf-8")
        whole, _, *_ = rt.reach(["apps/cli/src/commands/keel.rs"], packages, {}, root)
        self.assertEqual(whole, {"graphhelm-cli"})

    def test_the_dispatchers_do_not_widen_a_module_to_the_whole_package(self):
        # main.rs and commands/mod.rs call every module to run it; that is not building on it.
        root, packages = self.tree()
        src = root / "apps/cli/src"
        (src / "commands").mkdir(parents=True)
        (src / "commands/keel.rs").write_text("pub fn check() {}", encoding="utf-8")
        (src / "commands/mod.rs").write_text("mod keel; fn run() { keel::check(); }", encoding="utf-8")
        (src / "main.rs").write_text("fn main() { commands::keel::check(); }", encoding="utf-8")
        whole, single, *_ = rt.reach(["apps/cli/src/commands/keel.rs"], packages, {}, root)
        self.assertEqual(whole, set())
        self.assertIn(("graphhelm-cli", "bin:graphhelm", "commands::keel"), single)

    def test_shared_files_and_unmatched_modules_still_reach_the_whole_package(self):
        root, packages = self.tree()
        for path in ("apps/cli/src/main.rs", "apps/cli/src/commands/mod.rs", "apps/cli/src/commands/hash.rs"):
            whole, _, *_ = rt.reach([path], packages, {}, root)
            self.assertEqual(whole, {"graphhelm-cli"}, path)


if __name__ == "__main__":
    unittest.main()
