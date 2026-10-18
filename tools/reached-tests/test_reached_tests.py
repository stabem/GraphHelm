"""Observer for reached_tests.py (#361): `python -m unittest tools/reached-tests/test_reached_tests.py`.

Most cells use a small fake workspace, no cargo and no git: policy <- execution <- cli, an adapter
nobody depends on, a cli test bundle, and one embedded schema. The repository-bundle cell reads
offline Cargo metadata. Each case is a real diff shape from this repository.
"""
import json
import subprocess
import io
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
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
    {"name": "graphhelm-protocols", "dir": "core/protocols", "deps": [],
     "tests": [{"name": "canonical_golden", "src": "core/protocols/tests/canonical_golden.rs"}], "bundles": {}},
]
EMBEDDED = {"extensions/builtin/graphhelm-jpd/schemas/journey-contract.schema.json": {"graphhelm-cli"}}


def reach(*changed, repo=None):
    return rt.reach(list(changed), PACKAGES, EMBEDDED, repo)


class Reach(unittest.TestCase):

    def test_proportional_rows_fixture_selects_its_offline_test_once(self):
        """Fixture edits must execute the guidance regression, not only the Python guard.

        Cost: temporary files and direct plan construction; milliseconds, no Cargo, network,
        browser or Runtime.
        """
        fixture = ".graphhelm/fixtures/keel-proportional-rows"
        script = f"{fixture}/run.py"
        observer = f"{fixture}/test_run.py"
        expected = f"python {observer}"
        with tempfile.TemporaryDirectory() as repo:
            for relative in (script, observer):
                path = Path(repo, relative)
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("import unittest\n", encoding="utf-8")

            for changed in (script, observer):
                *_, tools, _, other = reach(changed, repo=repo)
                self.assertEqual(tools.count(expected), 1)
                self.assertEqual(other, [])
                self.assertIn({"argv": ["python", observer], "cwd": ".", "slot": False},
                              rt.steps(set(), set(), [], tools, repo=Path(repo)))

            *_, combined_tools, _, combined_other = reach(script, observer, repo=repo)
            self.assertEqual(combined_tools.count(expected), 1)
            self.assertEqual(combined_other, [])
            self.assertEqual(rt.steps(set(), set(), [], combined_tools, repo=Path(repo)).count(
                {"argv": ["python", observer], "cwd": ".", "slot": False}), 1)

            Path(repo, observer).unlink()
            *_, missing_tools, _, missing = reach(script, repo=repo)
            self.assertNotIn(expected, missing_tools)
            self.assertIn({"path": observer, "reason": "proportional-rows fixture observer is missing"}, missing)

    def test_any_changed_python_file_selects_no_console_window_guard(self):
        """Run the repository-wide Python guard for Python edits outside tools/.

        Cost: selector fixture only; milliseconds, no Cargo, network, browser or Runtime.
        """
        observer = "tools/reached-tests/test_no_console_window.py"
        expected = "python -m unittest tools/reached-tests/test_no_console_window.py"
        with tempfile.TemporaryDirectory() as repo:
            Path(repo, observer).parent.mkdir(parents=True)
            Path(repo, observer).write_text("import unittest\n", encoding="utf-8")
            changed = ".graphhelm/fixtures/example/scenario.py"
            *_, tools, _, other = reach(changed, repo=repo)
        self.assertEqual(tools, [expected])
        self.assertEqual(other, [])

    def test_no_window_observer_paths_select_one_fixed_powershell_step(self):
        observer = "tools/no-window/test_observe.ps1"
        expected = ["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass",
                    "-File", observer]
        with tempfile.TemporaryDirectory() as repo:
            paths = ["tools/no-window/observe.ps1", observer]
            for relative in paths:
                path = Path(repo, relative)
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("", encoding="utf-8")

            for changed in paths:
                *_, tools, _, other = reach(changed, repo=repo)
                self.assertEqual(tools, ["powershell.exe -NoProfile -ExecutionPolicy Bypass -File " + observer])
                self.assertEqual(rt.steps(set(), set(), [], tools, repo=Path(repo)), [
                    {"argv": expected, "cwd": ".", "slot": False}])
                self.assertEqual(other, [])

            *_, tools, _, other = reach(*paths, repo=repo)
            self.assertEqual(rt.steps(set(), set(), [], tools, repo=Path(repo)), [
                {"argv": expected, "cwd": ".", "slot": False}])
            self.assertEqual(other, [])

            *_, unrelated, _, other = reach("tools/no-window/README.md", repo=repo)
            self.assertEqual(unrelated, [])
            self.assertEqual(other, [])

            Path(repo, observer).unlink()
            *_, missing_tools, _, missing = reach(paths[0], repo=repo)
            self.assertEqual(missing_tools, [])
            self.assertIn({"path": observer, "reason": "no-window observer test is missing"}, missing)

    def test_no_window_launcher_fixture_paths_select_the_same_powershell_step_once(self):
        """A fixture-only change must not escape the native no-window launcher observation."""
        observer = "tools/no-window/test_observe.ps1"
        expected = ["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass",
                    "-File", observer]
        fixture = ["tools/no-window/fixture/fixture.sh", "tools/no-window/fixture/server.mjs"]
        with tempfile.TemporaryDirectory() as repo:
            for relative in (*fixture, observer):
                path = Path(repo, relative)
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("", encoding="utf-8")

            for changed in ([fixture[0]], [fixture[1]], fixture, [*fixture, observer]):
                *_, tools, _, other = reach(*changed, repo=repo)
                self.assertEqual(rt.steps(set(), set(), [], tools, repo=Path(repo)), [
                    {"argv": expected, "cwd": ".", "slot": False}], changed)
                self.assertEqual(other, [], changed)

            *_, unrelated, _, other = reach("tools/no-window/fixture/README.md", repo=repo)
            self.assertEqual(unrelated, [])

            Path(repo, observer).unlink()
            *_, missing_tools, _, missing = reach(fixture[0], repo=repo)
            self.assertEqual(missing_tools, [])
            self.assertIn({"path": observer, "reason": "no-window observer test is missing"}, missing)

    def test_task_handoff_fixture_selects_its_offline_scenario_once(self):
        """Reach the real isolated handoff observer for fixture-only diffs.

        Cost: temporary files only; milliseconds, no Cargo, network, browser or Runtime.
        """
        scenario = ".graphhelm/fixtures/keel-task-handoff/scenario.py"
        readme = ".graphhelm/fixtures/keel-task-handoff/README.md"
        observer = ".graphhelm/fixtures/keel-task-handoff/test_scenario.py"
        expected = f"python {scenario}"
        expected_test = f"python {observer}"
        with tempfile.TemporaryDirectory() as repo:
            scenario_path = Path(repo, scenario)
            scenario_path.parent.mkdir(parents=True)
            scenario_path.write_text("", encoding="utf-8")
            Path(repo, readme).write_text("", encoding="utf-8")
            Path(repo, observer).write_text("import unittest\n", encoding="utf-8")

            with patch.dict("os.environ", {"GRAPHHELM_HANDOFF_WRONG_STORE": ""}):
                *_, tools, _, other = reach(scenario, repo=repo)
            self.assertEqual(set(tools), {expected, expected_test})
            self.assertEqual(other, [])

            # Exercise the selector's executable plan as well as its legacy display mapping.
            self.assertEqual({tuple(step["argv"]) for step in rt.steps(set(), set(), [], tools, repo=Path(repo))}, {
                ("python", scenario), ("python", observer),
            })

            with patch.dict("os.environ", {"GRAPHHELM_HANDOFF_WRONG_STORE": "1"}):
                *_, wrong_store_tools, _, wrong_store_other = reach(scenario, repo=repo)
            self.assertEqual(set(wrong_store_tools), {expected + " --wrong-store", expected_test})
            self.assertEqual(wrong_store_other, [])
            self.assertEqual({tuple(step["argv"]) for step in rt.steps(set(), set(), [], wrong_store_tools, repo=Path(repo))}, {
                ("python", scenario, "--wrong-store"), ("python", observer),
            })

            *_, readme_tools, _, readme_other = reach(readme, repo=repo)
            self.assertEqual(set(readme_tools), {expected, expected_test})
            self.assertEqual(readme_other, [])

            with patch.dict("os.environ", {"GRAPHHELM_HANDOFF_WRONG_STORE": ""}):
                *_, missing_root_tools, _, missing_root_other = reach(scenario)
            self.assertEqual(missing_root_tools, [])
            self.assertEqual(missing_root_other, [scenario])

            *_, combined_tools, _, combined_other = reach(scenario, readme, repo=repo)
            self.assertEqual(set(combined_tools), {expected, expected_test})
            self.assertEqual(combined_other, [])

            scenario_path.unlink()
            *_, missing_tools, _, missing_other = reach(scenario, repo=repo)
            self.assertNotIn(expected, missing_tools)
            self.assertIn(expected_test, missing_tools)
            self.assertEqual(missing_other, [])

    def test_task_handoff_adapter_selection_test_is_executable_and_required(self):
        """Select the receiver-package regression once and refuse a missing observer.

        Cost: temporary files only; milliseconds, no Cargo, network, browser or Runtime.
        """
        fixture = ".graphhelm/fixtures/keel-task-handoff"
        scenario = f"{fixture}/scenario.py"
        observer = f"{fixture}/test_scenario.py"
        readme = f"{fixture}/README.md"
        expected = f"python {observer}"
        with tempfile.TemporaryDirectory() as repo:
            for relative in (scenario, observer, readme):
                path = Path(repo, relative)
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("import unittest\n", encoding="utf-8")

            *_, tools, _, other = reach(scenario, observer, observer, repo=repo)
            self.assertEqual(tools.count(expected), 1)
            self.assertIn({"argv": ["python", observer], "cwd": ".", "slot": False},
                          rt.steps(set(), set(), [], tools, repo=Path(repo)))
            self.assertEqual(other, [])

            Path(repo, observer).unlink()
            *_, missing_tools, _, missing = reach(scenario, repo=repo)
            self.assertNotIn(expected, missing_tools)
            self.assertIn({"path": observer, "reason": "handoff adapter-selection observer is missing"}, missing)

    def test_task_handoff_scenario_test_selects_full_offline_observers(self):
        """Reach the fixture regression and positive continuity scenario together.

        Cost: selector fixture only; milliseconds, no Cargo, network, browser or Runtime.
        """
        scenario = ".graphhelm/fixtures/keel-task-handoff/scenario.py"
        readme = ".graphhelm/fixtures/keel-task-handoff/README.md"
        test = ".graphhelm/fixtures/keel-task-handoff/test_scenario.py"
        positive = f"python {scenario}"
        unit = f"python {test}"
        with tempfile.TemporaryDirectory() as repo:
            for path in (scenario, readme, test):
                target = Path(repo, path)
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text("", encoding="utf-8")

            *_, tools, _, other = reach(scenario, test, repo=repo)
            self.assertEqual(tools, [positive, unit])
            self.assertEqual(other, [])
            self.assertEqual(rt.steps(set(), set(), [], tools, repo=Path(repo)), [
                {"argv": ["python", scenario], "cwd": ".", "slot": False},
                {"argv": ["python", test], "cwd": ".", "slot": False},
            ])

            *_, combined_tools, _, combined_other = reach(scenario, readme, test, repo=repo)
            self.assertEqual(combined_tools, [positive, unit])
            self.assertEqual(combined_other, [])

            *_, missing_tools, _, missing_other = reach(test)
            self.assertEqual(missing_tools, [])
            self.assertEqual(missing_other, [test])

    def test_shipped_plugin_diff_selects_executable_version_guard(self):
        """A shipped plugin edit must run the version guard for the selected refs.

        Cost: selector fixture only; milliseconds, no Cargo, network, browser, or Runtime.
        """
        with tempfile.TemporaryDirectory() as repo, \
                patch.object(rt, "run", return_value="plugins/graphhelm/skills/graphhelm-guide/SKILL.md"), \
                patch.object(rt, "workspace", return_value=[]), \
                patch.object(rt, "embeds", return_value={}), \
                patch.object(rt.shutil, "which", return_value=None):
            output = io.StringIO()
            with redirect_stdout(output):
                self.assertEqual(rt.main(["--repo", repo, "--base", "selected-base", "--head", "selected-head", "--json"]), 0)
        plan = json.loads(output.getvalue())
        guard = [step for step in plan["steps"] if step["argv"][:3] == ["python", "tools/reached-tests/plugin_version_guard.py", "--base"]]
        self.assertEqual(len(guard), 1)
        self.assertEqual(guard[0]["argv"], ["python", "tools/reached-tests/plugin_version_guard.py",
                                             "--base", "selected-base", "--head", "selected-head"])
        self.assertFalse(guard[0]["slot"])
        self.assertIn("python tools/reached-tests/plugin_version_guard.py --base selected-base --head selected-head",
                      plan["commands"])

    def test_selection_report_test_selects_offline_report_observer(self):
        """Map the report CLI regression to its unittest target.

        Cost: temporary files only, milliseconds; no Cargo or external services.
        """
        observer = "tools/evals/test_selection_report.py"
        expected = "python -m unittest tools/evals/test_selection_report.py"
        with tempfile.TemporaryDirectory() as repo:
            path = Path(repo, observer)
            path.parent.mkdir(parents=True)
            path.write_text("import unittest\n", encoding="utf-8")
            *_, tools, _, other = reach(observer, repo=repo)
            self.assertEqual(tools, [expected])
            self.assertEqual(other, [])

    def test_qualification_packet_selects_only_its_integrity_observer(self):
        """Keep the judge packet on its offline integrity path, not as unmapped JSON.

        Cost: tiny temporary files; milliseconds, no Cargo, network, model or Runtime.
        """
        packet = "evals/graphhelm-development-contracts/judge-qualification.json"
        observer = "tools/evals/test_skill_twins.py"
        expected = "python -m unittest tools/evals/test_skill_twins.py"
        with tempfile.TemporaryDirectory() as repo:
            observer_path = Path(repo, observer)
            observer_path.parent.mkdir(parents=True)
            observer_path.write_text("import unittest\n", encoding="utf-8")
            *_, tools, _, other = reach(packet, repo=repo)
            self.assertEqual(tools, [expected])
            self.assertEqual(other, [])

            *_, combined_tools, _, combined_other = reach(packet, observer, repo=repo)
            self.assertEqual(combined_tools, [expected])
            self.assertEqual(combined_other, [])

            observer_path.unlink()
            *_, missing_tools, _, missing_other = reach(packet, repo=repo)
            self.assertEqual(missing_tools, [])
            self.assertEqual(missing_other, [packet])

            unrelated = "evals/graphhelm-development-contracts/other.json"
            *_, unrelated_tools, _, unrelated_other = reach(unrelated, repo=repo)
            self.assertEqual(unrelated_tools, [])
            self.assertEqual(unrelated_other, [unrelated])

    def test_keel_plan_uses_configured_cli_without_path_fallback(self):
        with tempfile.TemporaryDirectory() as temp:
            exe = Path(temp) / "selected-graphhelm"
            exe.write_text("fixture", encoding="utf-8")
            reply = json.dumps({"data": {"plan": {"classes": [], "proof": [], "decidedBy": "fixture"}}})
            with patch.dict(rt.os.environ, {"GRAPHHELM_CLI": str(exe)}), \
                    patch.object(rt.shutil, "which", side_effect=AssertionError("no fallback")), \
                    patch.object(rt, "run", return_value=reply) as invoke:
                self.assertIsNotNone(rt.keel_plan(temp, ["example.rs"]))
                self.assertEqual(invoke.call_args.args[0][0], str(exe.resolve()))
                exe.unlink()
                with self.assertRaisesRegex(ValueError, "GRAPHHELM_CLI"):
                    rt.keel_plan(temp, ["example.rs"])
                self.assertEqual(invoke.call_count, 1)
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
            self.assertIn(f"cargo +1.97.1 test --locked -p graphhelm-cli --test cli -- --test-threads=2 {source.stem}::",
                          rt.commands(whole, single, []))

    def test_repository_cli_bundle_owns_registration_guard(self):
        # Cost: reads one Rust source. Package-level Cargo tests must catch an unregistered file.
        repo = Path(__file__).resolve().parents[2]
        bundle = (repo / "apps/cli/tests/cli.rs").read_text(encoding="utf-8")
        self.assertIn("fn every_top_level_test_file_is_registered()", bundle)

    def test_registration_bundle_change_uses_partitioned_cli_observer(self):
        repo = Path(__file__).resolve().parents[2]
        packages = rt.workspace(repo)
        whole, single, *_ = rt.reach(["apps/cli/tests/cli.rs"], packages, {}, repo)
        self.assertEqual(whole, set())
        self.assertEqual(single, {("graphhelm-cli", "cli", None)})
        commands = rt.commands(whole, single, [], package_metadata=packages)
        self.assertNotIn("cargo +1.97.1 test --locked -p graphhelm-cli --test cli -- --test-threads=2", commands)
        self.assertTrue(any("--test cli -- --test-threads=2 " in command and "--skip" not in command
                            for command in commands))
        self.assertTrue(any("--skip" in command for command in commands))

    def test_a_journey_flow_file_reaches_the_scope_guard_and_journey_validate(self):
        # #504 review: a flows-only diff (#430-style) printed no test, only "unmapped".
        whole, single, studio, tools, validate, other = reach(".graphhelm/journeys/checkout.journey.yaml")
        self.assertEqual(other, [])
        self.assertIn(("graphhelm-cli", "journeys", "journey_scope_guard"), single)
        self.assertIn("graphhelm --json journey validate --all", tools)

    def test_the_mcp_tool_catalog_reaches_the_stdio_tests_and_the_studio_adapter_test(self):
        # #1449: schemas/ mapped to nothing, so a catalog change stopped run_reached as unmapped.
        packages = [dict(PACKAGES[2], bundles={"mcp_stdio": "cli"})]
        with tempfile.TemporaryDirectory() as repo:
            studio_test = Path(repo, rt.MCP_CATALOG_STUDIO_TEST)
            studio_test.parent.mkdir(parents=True)
            studio_test.write_text("", encoding="utf-8")
            whole, single, studio, tools, validate, other = rt.reach(
                ["schemas/mcp-tools.json"], packages, {}, repo)
        self.assertEqual(other, [])
        self.assertIn(("graphhelm-cli", "cli", "mcp_stdio"), single)
        self.assertEqual(studio, [rt.MCP_CATALOG_STUDIO_TEST])

    def test_deleted_non_rust_include_reaches_embedding_crate_compile(self):
        """Compile crates whose Rust source embeds a changed non-Rust path, including deletions.

        Cost: temporary fixture files and selector work; milliseconds, no Cargo or external services.
        """
        fixture = "fixtures/studio-answer-node.journey.yaml"
        with tempfile.TemporaryDirectory() as repo:
            source = Path(repo, "crates/fixture/src/lib.rs")
            source.parent.mkdir(parents=True)
            source.write_text('const FLOW: &[u8] = include_bytes!("../../../fixtures/studio-answer-node.journey.yaml");', encoding="utf-8")
            Path(repo, fixture).parent.mkdir(parents=True)
            whole, single, *_ = rt.reach([fixture], [{"name": "graphhelm-fixture", "dir": "crates/fixture"}], {}, repo)
            self.assertEqual(whole, {"graphhelm-fixture"})
            self.assertEqual(single, set())

    def test_deleted_journey_embedded_by_cli_source_selects_only_binary(self):
        """#1075: a deleted flow must compile its Rust owner without planning every CLI test.

        Cost: local temporary files and selector work; milliseconds, no Cargo or services.
        """
        flow = ".graphhelm/journeys/studio-answer-node.journey.yaml"
        with tempfile.TemporaryDirectory() as repo:
            source = Path(repo, "apps/cli/src/commands/journey_preview.rs")
            source.parent.mkdir(parents=True)
            source.write_text('std::fs::read(root().join(".graphhelm/journeys/studio-answer-node.journey.yaml"));',
                              encoding="utf-8")
            packages = [dict(PACKAGES[2], targets=[{"name": "graphhelm", "kind": "bin",
                                                   "src": "apps/cli/src/main.rs"}])]
            whole, single, *_ = rt.reach([flow], packages, {}, repo)
            self.assertEqual(whole, set())
            self.assertEqual(single, {("graphhelm-cli", "bin:graphhelm", None),
                                      ("graphhelm-cli", "journeys", "journey_scope_guard")})
            self.assertEqual([step["argv"] for step in rt.steps(whole, single, [], lint_packages=[])][:1], [
                ["cargo", "+1.97.1", "test", "--locked", "-p", "graphhelm-cli", "--bin", "graphhelm",
                 "--", "--test-threads=2"]])

    def test_embedded_file_in_cli_test_module_selects_only_that_module(self):
        """A test fixture must not select unrelated CLI modules; pure local selector cost."""
        fixture = "fixtures/changed.yaml"
        with tempfile.TemporaryDirectory() as repo:
            source = Path(repo, "apps/cli/tests/journey_preview.rs")
            source.parent.mkdir(parents=True)
            source.write_text('include_str!("../../../fixtures/changed.yaml");', encoding="utf-8")
            packages = [dict(PACKAGES[2], bundles={"journey_preview": "cli"},
                             tests=[{"name": "cli", "src": "apps/cli/tests/cli.rs"}])]
            whole, single, *_ = rt.reach([fixture], packages, {}, repo)
            self.assertEqual(whole, set())
            self.assertEqual(single, {("graphhelm-cli", "cli", "journey_preview")})

    def test_deleted_non_rust_literal_root_join_reaches_embedding_crate_compile(self):
        """Resolve a repository-root literal joined at a Rust file read site.

        Cost: temporary fixture files and selector work; milliseconds, no Cargo or external services.
        """
        fixture = "fixtures/changed.yaml"
        with tempfile.TemporaryDirectory() as repo:
            source = Path(repo, "crates/fixture/src/lib.rs")
            source.parent.mkdir(parents=True)
            source.write_text('std::fs::read_to_string(root().join("fixtures/changed.yaml"));', encoding="utf-8")
            whole, single, *_ = rt.reach([fixture], [{"name": "graphhelm-fixture", "dir": "crates/fixture"}], {}, repo)
            self.assertEqual(whole, {"graphhelm-fixture"})
            self.assertEqual(single, set())

    def test_manifest_root_join_reaches_owning_crate(self):
        """A manifest-rooted literal resolves from the crate, not the Rust source.

        Cost: temporary fixture files and selector work; milliseconds, no Cargo or services.
        """
        with tempfile.TemporaryDirectory() as repo:
            source = Path(repo, "crates/fixture/src/lib.rs")
            source.parent.mkdir(parents=True)
            source.write_text('std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/changed.yaml"));', encoding="utf-8")
            packages = [{"name": "graphhelm-fixture", "dir": "crates/fixture", "tests": [], "deps": []}]
            whole, single, *_ = rt.reach(["crates/fixture/fixtures/changed.yaml"], packages, {}, repo)
            self.assertEqual(whole, {"graphhelm-fixture"})
            self.assertEqual(single, set())
            self.assertIn("crates/fixture/fixtures/changed.yaml", rt.rust_path_readers(repo, packages))

    def test_changed_non_rust_path_without_rust_reference_adds_no_compile(self):
        """Do not broaden compile plans for non-Rust files without a source reference.

        Cost: temporary fixture files and selector work; milliseconds, no Cargo or external services.
        """
        fixture = "fixtures/unreferenced.journey.yaml"
        with tempfile.TemporaryDirectory() as repo:
            Path(repo, "crates/fixture/src/lib.rs").parent.mkdir(parents=True)
            Path(repo, "crates/fixture/src/lib.rs").write_text("pub fn fixture() {}", encoding="utf-8")
            whole, single, *_ = rt.reach([fixture], [{"name": "graphhelm-fixture", "dir": "crates/fixture"}], {}, repo)
            self.assertEqual(whole, set())
            self.assertEqual(single, set())

    def test_a_journey_runtime_change_prints_the_ignored_browser_observers(self):
        # #504 review: the #[ignore] browser cells are the real proof of a driver or replay change.
        for path in ("tools/journey-driver/driver.mjs", "apps/cli/src/commands/journey_replay.rs",
                     "apps/cli/tests/journey_live_browser.rs"):
            *_, tools, _, _ = reach(path)
            self.assertIn(rt.BROWSER_OBSERVERS, tools, path)
        *_, tools, _, _ = reach("core/policy/src/lib.rs")
        self.assertNotIn(rt.BROWSER_OBSERVERS, tools)
        self.assertIn("-- --ignored journey_replay_browser::", rt.BROWSER_OBSERVERS)

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
        packages[0]["bundles"] = {"alpha": "cli", "beta": "cli"}
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
        cli_steps = [step["argv"] for step in plans["graphhelm-cli"] if "--test" in step["argv"]]
        self.assertEqual(cli_steps, [
            ["cargo", "+1.97.1", "test", "--locked", "-p", "graphhelm-cli", "--test", "cli", "--", "--test-threads=2", "alpha::"],
            ["cargo", "+1.97.1", "test", "--locked", "-p", "graphhelm-cli", "--test", "cli", "--", "--test-threads=2", "--skip", "alpha::", "beta::"],
            ["cargo", "+1.97.1", "test", "--locked", "-p", "graphhelm-cli", "--test", "cli", "--", "--test-threads=2", "--skip", "alpha::", "--skip", "beta::"],
        ])

    def test_cli_bundle_metadata_is_required_for_whole_package_partition(self):
        plans, unsupported = rt.expand_whole_packages({"graphhelm-cli"}, [{"name": "graphhelm-cli", "targets": [
            {"name": "cli", "kind": "test", "test": True, "doctest": False},
        ], "bundles": {}}])
        self.assertEqual(plans["graphhelm-cli"], [])
        self.assertIn("bundle metadata", unsupported[0]["reason"])

    def test_whole_package_required_features_run_with_those_features(self):
        # #1280: a gated target is planned with its features, not reported unsupported (which made
        # run_reached refuse the whole plan). The features go before libtest's `--`.
        plans, unsupported = rt.expand_whole_packages({"graphhelm-cli"}, [{"name": "graphhelm-cli", "targets": [
            {"name": "feature_test", "kind": "test", "test": True, "doctest": False,
             "requiredFeatures": ["special", "other"]},
        ]}])
        self.assertEqual(unsupported, [])
        self.assertEqual([step["argv"] for step in plans["graphhelm-cli"]], [[
            "cargo", rt.TOOLCHAIN, "test", "--locked", "-p", "graphhelm-cli", "--test", "feature_test",
            "--features", "special,other", "--", "--test-threads=2",
        ]])

    def test_a_single_gated_target_runs_with_its_features(self):
        # #1280: a touched gated test file is a single-target step; it needs the same features
        # the whole-package path adds, in both the executable step and the printed command.
        metadata = [{"name": "graphhelm-postgres-event-store", "targets": [
            {"name": "concurrency", "kind": "test", "test": True, "doctest": False,
             "requiredFeatures": ["test-support"]},
            {"name": "plain", "kind": "test", "test": True, "doctest": False, "requiredFeatures": []},
        ]}]
        single = {("graphhelm-postgres-event-store", "concurrency", None),
                  ("graphhelm-postgres-event-store", "plain", None)}
        argvs = [step["argv"] for step in rt.steps(set(), single, [], package_metadata=metadata)
                 if "graphhelm-postgres-event-store" in step["argv"] and "--test" in step["argv"]]
        gated = next(argv for argv in argvs if "concurrency" in argv)
        plain = next(argv for argv in argvs if "plain" in argv)
        self.assertEqual(gated[gated.index("--features") + 1], "test-support")
        self.assertLess(gated.index("--features"), gated.index("--"))
        self.assertNotIn("--features", plain)
        printed = rt.commands(set(), single, [], package_metadata=metadata)
        self.assertTrue(any("--test concurrency --features test-support --" in line for line in printed), printed)

    def test_the_postgres_event_store_plans_its_gated_targets_on_the_real_workspace(self):
        # #1280, the case seen live on #1279: graphhelm-postgres-event-store declares two test
        # targets with required-features = ["test-support"].
        packages = rt.workspace(Path(__file__).resolve().parents[2])
        plans, unsupported = rt.expand_whole_packages({"graphhelm-postgres-event-store"}, packages)
        self.assertEqual(unsupported, [])
        gated = [step["argv"] for step in plans["graphhelm-postgres-event-store"] if "--features" in step["argv"]]
        self.assertEqual(len(gated), 2, plans["graphhelm-postgres-event-store"])
        for argv in gated:
            self.assertEqual(argv[argv.index("--features") + 1], "test-support")
            self.assertLess(argv.index("--features"), argv.index("--"))

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

    def test_registered_bundle_paths_select_declared_modules(self):
        """An alias or nested registered file reaches its declared module, not its stem."""
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / "fixture"
            root.mkdir()
            manifest = root / "Cargo.toml"
            bundle = root / "tests" / "bundle.rs"
            bundle.parent.mkdir()
            manifest.write_text('[package]\nname = "fixture"\nversion = "0.0.0"\n', encoding="utf-8")
            bundle.write_text('#[path = "first.rs"] mod second;\n'
                              '#[path = "second.rs"] mod first;\n'
                              '#[path = "cases/renamed.rs"] mod nested;\n', encoding="utf-8")
            for name in ("first.rs", "second.rs", "cases/renamed.rs"):
                source = root / "tests" / name
                source.parent.mkdir(parents=True, exist_ok=True)
                source.write_text("", encoding="utf-8")
            metadata = {"workspace_root": str(root.parent), "packages": [{
                "name": "fixture", "manifest_path": str(manifest), "dependencies": [],
                "targets": [{"name": "bundle", "kind": ["test"], "src_path": str(bundle),
                             "test": True, "doctest": False, "required-features": []}],
            }]}
            with patch.object(rt, "run", return_value=json.dumps(metadata)):
                packages = rt.workspace(root)
            for changed, expected in (("fixture/tests/first.rs", "second"),
                                      ("fixture/tests/second.rs", "first"),
                                      ("fixture/tests/cases/renamed.rs", "nested")):
                with self.subTest(changed=changed):
                    whole, single, studio, tools, validate, other = rt.reach(
                        [changed], packages, {}, root.parent)
                    self.assertEqual((whole, single), (set(), {("fixture", "bundle", expected)}))
                    selected = rt.steps(whole, single, studio, tools, validate, repo=root.parent)
                    self.assertTrue(any(step["argv"][-1] == expected + "::" for step in selected))

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
        self.assertIn("cargo +1.97.1 test --locked -p graphhelm-cli --test development -- --test-threads=2 keel_check::", rt.commands(whole, single, []))

    def test_single_modules_group_by_executable_without_crossing_targets(self):
        single = {
            ("graphhelm-cli", "bundle", "alpha"),
            ("graphhelm-cli", "bundle", "beta"),
            ("graphhelm-cli", "other", "gamma"),
            ("other-package", "bundle", "delta"),
        }
        expected = [
            "cargo +1.97.1 test --locked -p graphhelm-cli --test bundle -- --test-threads=2 alpha:: beta::",
            "cargo +1.97.1 test --locked -p graphhelm-cli --test other -- --test-threads=2 gamma::",
            "cargo +1.97.1 test --locked -p other-package --test bundle -- --test-threads=2 delta::",
        ]
        command_targets = [command for command in rt.commands(set(), single, [])
                           if command.startswith("cargo +1.97.1 test --locked -p ")
                           and " authored_strings_across_the_workspace " not in command]
        self.assertEqual(command_targets, expected)
        step_targets = [step["argv"] for step in rt.steps(set(), single, [])
                        if step["argv"][:3] == ["cargo", "+1.97.1", "test"]
                        and step["argv"][5] in {"graphhelm-cli", "other-package"}]
        self.assertEqual([" ".join(argv) for argv in step_targets], expected)

    def test_unfiltered_selection_dominates_module_filters(self):
        single = {
            ("graphhelm-cli", "bundle", None),
            ("graphhelm-cli", "bundle", "alpha"),
        }
        expected = "cargo +1.97.1 test --locked -p graphhelm-cli --test bundle -- --test-threads=2"
        command_targets = [command for command in rt.commands(set(), single, [])
                           if command.startswith("cargo +1.97.1 test --locked -p ")
                           and " authored_strings_across_the_workspace " not in command]
        self.assertEqual(command_targets, [expected])
        step_targets = [step["argv"] for step in rt.steps(set(), single, [])
                        if step["argv"][:3] == ["cargo", "+1.97.1", "test"]
                        and step["argv"][5] == "graphhelm-cli"]
        self.assertEqual([" ".join(argv) for argv in step_targets], [expected])

    def test_cli_unit_and_integration_filters_coalesce_and_union(self):
        single = {
            ("graphhelm-cli", "bin:graphhelm", "commands::workspace_slot"),
            ("graphhelm-cli", "cli", "workspace_cli"),
        }
        expected = "cargo +1.97.1 test --locked -p graphhelm-cli --bin graphhelm --test cli -- --test-threads=2 commands::workspace_slot:: workspace_cli::"
        commands = [command for command in rt.commands(set(), single, [])
                    if command.startswith("cargo +1.97.1 test --locked -p graphhelm-cli")]
        self.assertEqual(commands, [expected])
        steps = [step["argv"] for step in rt.steps(set(), single, [])
                 if step["argv"][:3] == ["cargo", "+1.97.1", "test"]
                 and step["argv"][5] == "graphhelm-cli"]
        self.assertEqual([" ".join(argv) for argv in steps], [expected])

    def test_coalescing_requires_both_filtered_targets(self):
        single = {
            ("graphhelm-cli", "bin:graphhelm", None),
            ("graphhelm-cli", "cli", "workspace_cli"),
        }
        commands = [command for command in rt.commands(set(), single, [])
                    if command.startswith("cargo +1.97.1 test --locked -p graphhelm-cli")]
        self.assertEqual(len(commands), 2)

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

    def test_self_include_stays_narrow_while_cross_file_include_stays_broad(self):
        # Contract: a source file embedding itself remains a narrow test selection, while a
        # changed file embedded by another source still reaches that package. The regression is
        # self-include promotion to package-wide coverage; existing fixture mappings do not cover
        # self-resolved paths. Cost: temporary files and pure Python reach calculation.
        with tempfile.TemporaryDirectory() as repo_name:
            repo = Path(repo_name)
            source_dir = repo / "apps/cli/tests"
            source_dir.mkdir(parents=True)
            (source_dir / "api_http.rs").write_text(
                'include_str!("api_http.rs");', encoding="utf-8")
            (source_dir / "fixture.txt").write_text("fixture", encoding="utf-8")
            (source_dir / "cross_file.rs").write_text(
                'include_str!("fixture.txt");', encoding="utf-8")
            packages = [dict(PACKAGES[2],
                              tests=[{"name": "cli", "src": "apps/cli/tests/cli.rs"}],
                              bundles={"api_http": "cli", "cross_file": "cli"})]
            embedded = rt.embeds(repo, packages)

            self.assertNotIn("apps/cli/tests/api_http.rs", embedded)
            whole, single, *_ = rt.reach(
                ["apps/cli/tests/api_http.rs"], packages, embedded, repo)
            self.assertEqual(whole, set())
            self.assertEqual(single, {("graphhelm-cli", "cli", "api_http")})
            self.assertTrue(any("api_http::" in command for command in rt.commands(whole, single, [])))

            self.assertEqual(embedded["apps/cli/tests/fixture.txt"], {"graphhelm-cli"})
            whole, *_ = rt.reach(["apps/cli/tests/fixture.txt"], packages, embedded, repo)
            self.assertEqual(whole, {"graphhelm-cli"})

    def test_an_extension_change_reaches_its_runtime_readers_and_validate(self):
        whole, single, _, _, validate, other = reach("extensions/builtin/graphhelm-development-contracts/skills/merge/SKILL.md")
        self.assertEqual(whole, set())
        self.assertIn(("graphhelm-host-adoption", "release_packages", None), single)
        self.assertIn(("graphhelm-cli", "development", "development_plugin"), single)
        self.assertEqual(validate, ["extensions/builtin/graphhelm-development-contracts"])
        self.assertEqual(other, [])

    def test_an_extension_manifest_change_reaches_the_canonical_golden(self):
        """#1425: a re-pinned extension.json digest turned main red on canonical_golden unseen."""
        _, single, *_ = reach("extensions/builtin/graphhelm-development-contracts/extension.json")
        self.assertIn(("graphhelm-protocols", "canonical_golden", None), single)

    def test_studio_files_reach_vitest_related_and_nothing_in_rust(self):
        whole, single, studio, *_ = reach("apps/studio/src/runtime/team-tasks.ts")
        self.assertEqual((whole, single), (set(), set()))
        commands = rt.commands(whole, single, studio)
        self.assertEqual(commands, ["(cd apps/studio && npx vitest related --run --maxWorkers=1 src/runtime/team-tasks.ts && npx tsc -b)"])

    def test_studio_launcher_scripts_reach_the_cli_launcher_tests_not_vitest(self):
        """#1571: apps/studio/tools/* are exercised by `the_launcher_*` in graphhelm-cli."""
        for script in ("apps/studio/tools/studio-up.ps1", "apps/studio/tools/studio-up.sh"):
            whole, single, studio, tools, *_ = reach(script)
            self.assertEqual((whole, single, studio), (set(), set(), []))
            self.assertEqual(tools, [rt.LAUNCHER_TESTS])
            step = rt.steps(whole, single, studio, tools)[0]
            self.assertEqual(step["argv"][-1], "commands::studio::tests::the_launcher_")
            self.assertIn("--bin", step["argv"])

    def test_studio_config_reaches_full_suite_in_both_plans(self):
        for paths in (("apps/studio/vite.config.ts",),
                      ("apps/studio/vite.config.ts", "apps/studio/src/runtime/team-tasks.ts")):
            with self.subTest(paths=paths):
                whole, single, studio, *_ = reach(*paths)
                self.assertEqual(rt.commands(whole, single, studio),
                                 ["(cd apps/studio && npx vitest run --maxWorkers=1 && npx tsc -b)"])
                self.assertEqual(rt.steps(whole, single, studio)[-2:], [
                    {"argv": ["npx", "vitest", "run", "--maxWorkers=1"],
                     "cwd": "apps/studio", "slot": False},
                    {"argv": ["npx", "tsc", "-b"], "cwd": "apps/studio", "slot": False},
                ])

    def test_docs_reach_nothing_and_are_not_reported_unmapped(self):
        whole, single, studio, tools, validate, other = reach("docs/process/DELIVERY.md", "docs/journeys/x/shot.jpg")
        self.assertEqual((whole, single, studio, tools, validate, other), (set(), set(), [], [], [], []))
        self.assertEqual(rt.commands(whole, single, studio), [])

    def test_a_keel_rule_or_keel_plugin_change_reaches_the_keel_twin_guard(self):
        # The standalone keel plugin's skills carry the KEEL files' text: an edit on either side
        # must run the guard that compares them, or the two drift with every check green.
        with tempfile.TemporaryDirectory() as repo:
            guard = Path(repo, "tools", "keel-plugin")
            guard.mkdir(parents=True)
            (guard / "test_keel_plugin.py").write_text("", encoding="utf-8")
            for changed in ("KEEL.md", "keel/KEEL_EXEC.md", "plugins/keel/skills/keel-exec/SKILL.md",
                            ".claude-plugin/marketplace.json", ".agents/plugins/marketplace.json"):
                *_, tools, _, other = reach(changed, repo=repo)
                self.assertIn("python -m unittest tools/keel-plugin/test_keel_plugin.py", tools, changed)
                self.assertEqual(other, [], changed)

    def test_an_event_store_append_change_reaches_the_planted_defect_anchors(self):
        # #1476: the planted defects are exact-text anchors in local.rs. An edit there must run the
        # anchor check, or a moved anchor silently turns a defect from caught into unobserved.
        with tempfile.TemporaryDirectory() as repo:
            guard = Path(repo, "tools", "keel-effect")
            guard.mkdir(parents=True)
            (guard / "test_plant_defects.py").write_text("", encoding="utf-8")
            *_, tools, _, _ = reach("core/events/src/local.rs", repo=repo)
            self.assertIn("python -m unittest tools/keel-effect/test_plant_defects.py", tools)
            *_, tools, _, _ = reach("core/events/src/jsonl.rs", repo=repo)
            self.assertNotIn("python -m unittest tools/keel-effect/test_plant_defects.py", tools)

    def test_shipped_keel_plugin_content_needs_a_release_bump(self):
        self.assertTrue(rt._plugin_release_content("plugins/keel/skills/keel/SKILL.md"))
        self.assertTrue(rt._plugin_release_content("plugins/keel/.claude-plugin/plugin.json"))
        self.assertFalse(rt._plugin_release_content("plugins/keel/docs/notes.md"))

    def test_a_tool_change_reaches_that_tools_own_tests(self):
        with tempfile.TemporaryDirectory() as repo:
            tool = Path(repo, "tools", "task-record")
            tool.mkdir(parents=True)
            (tool / "task_record.py").write_text("", encoding="utf-8")
            (tool / "test_task_record.py").write_text("", encoding="utf-8")
            *_, tools, _, other = reach("tools/task-record/task_record.py", repo=repo)
        self.assertEqual(tools, ["python -m unittest tools/task-record/test_task_record.py"])
        self.assertEqual(other, [])

    def test_a_hook_change_reaches_its_test_and_hook_tests_map_to_themselves(self):
        with tempfile.TemporaryDirectory() as repo:
            hooks = Path(repo, "plugins", "graphhelm", "hooks")
            hooks.mkdir(parents=True)
            (hooks / "lane_stop_hook.py").write_text("", encoding="utf-8")
            (hooks / "test_lane_stop_hook.py").write_text("", encoding="utf-8")
            (hooks / "test_other_hook.py").write_text("", encoding="utf-8")
            (hooks / "test_session_hook.py").write_text("", encoding="utf-8")

            # A companion edit must reach behavior and the package-mirror contract.
            *_, tools, _, other = reach("plugins/graphhelm-codex-hooks/hooks/lane_stop_hook.py", repo=repo)
            self.assertEqual(tools, ["python -m unittest plugins/graphhelm/hooks/test_lane_stop_hook.py",
                                     "python -m unittest plugins/graphhelm/hooks/test_session_hook.py"])
            self.assertEqual(other, [])

            *_, tools, _, other = reach("plugins/graphhelm/hooks/lane_stop_hook.py", repo=repo)
            self.assertEqual(tools, ["python -m unittest plugins/graphhelm/hooks/test_lane_stop_hook.py"])
            self.assertEqual(other, [])

            *_, tools, _, other = reach("plugins/graphhelm/hooks/test_lane_stop_hook.py", repo=repo)
            self.assertEqual(tools, ["python -m unittest plugins/graphhelm/hooks/test_lane_stop_hook.py"])
            self.assertEqual(other, [])

    def test_a_hook_without_a_matching_test_reaches_all_hook_tests(self):
        with tempfile.TemporaryDirectory() as repo:
            hooks = Path(repo, "plugins", "graphhelm", "hooks")
            hooks.mkdir(parents=True)
            (hooks / "another_hook.py").write_text("", encoding="utf-8")
            (hooks / "test_lane_stop_hook.py").write_text("", encoding="utf-8")
            (hooks / "test_other_hook.py").write_text("", encoding="utf-8")

            *_, tools, _, other = reach("plugins/graphhelm/hooks/another_hook.py", repo=repo)
        self.assertEqual(tools, ["python -m unittest plugins/graphhelm/hooks/test_lane_stop_hook.py",
                                 "python -m unittest plugins/graphhelm/hooks/test_other_hook.py"])
        self.assertEqual(other, [])

    def test_every_plugins_path_selects_its_observer_or_is_docs_like(self):
        """Keep plugin manifests, markdown, and all hook extensions mapped.

        Cost: temporary filesystem fixtures only; milliseconds, no Cargo or network.
        """
        with tempfile.TemporaryDirectory() as repo:
            hooks = Path(repo, "plugins", "graphhelm", "hooks")
            hooks.mkdir(parents=True)
            (hooks / "session_hook.py").write_text("", encoding="utf-8")
            (hooks / "test_session_hook.py").write_text("import unittest\n", encoding="utf-8")
            companion_hooks = Path(repo, "plugins", "graphhelm-codex-hooks", "hooks")
            companion_hooks.mkdir(parents=True)
            (companion_hooks / "session_hook.py").write_text("", encoding="utf-8")
            (companion_hooks / "test_session_hook.py").write_text("import unittest\n", encoding="utf-8")
            other_hooks = Path(repo, "plugins", "example-plugin", "hooks")
            other_hooks.mkdir(parents=True)
            (other_hooks / "worker.mjs").write_text("", encoding="utf-8")
            (other_hooks / "test_worker.py").write_text("import unittest\n", encoding="utf-8")
            plugin = Path(repo, "plugins", "graphhelm", ".claude-plugin")
            plugin.mkdir(parents=True)
            (plugin / "plugin.json").write_text("{}", encoding="utf-8")
            (plugin / "marketplace.json").write_text("{}", encoding="utf-8")
            (Path(repo, "plugins", "graphhelm", "plugin.json")).write_text("{}", encoding="utf-8")
            (Path(repo, "plugins", "graphhelm", ".mcp.json")).write_text("{}", encoding="utf-8")
            (Path(repo, "plugins", "graphhelm", "SKILL.md")).write_text("# skill\n", encoding="utf-8")
            skill = Path(repo, "plugins", "graphhelm", "skills", "graphhelm-setup", "SKILL.md")
            skill.parent.mkdir(parents=True)
            skill.write_text("# skill\n", encoding="utf-8")
            agent = Path(repo, "plugins", "graphhelm", "agents", "worker.md")
            agent.parent.mkdir(parents=True)
            agent.write_text("# agent\n", encoding="utf-8")
            (Path(repo, "plugins", "graphhelm", "README.md")).write_text("# docs\n", encoding="utf-8")
            eval_test = Path(repo, "tools", "evals", "test_skill_twins.py")
            eval_test.parent.mkdir(parents=True)
            eval_test.write_text("import unittest\n", encoding="utf-8")

            changed = [
                # The real PR #841 path classes: plugin manifest/version, MCP manifest,
                # skill and agent markdown, and a hook source.
                "plugins/graphhelm-codex-hooks/.codex-plugin/plugin.json",
                "plugins/graphhelm-codex-hooks/hooks/session_hook.py",
                "plugins/graphhelm/.claude-plugin/plugin.json",
                "plugins/graphhelm/.codex-plugin/plugin.json",
                "plugins/graphhelm/.claude-plugin/marketplace.json",
                "plugins/graphhelm/plugin.json",
                "plugins/graphhelm/.mcp.json",
                "plugins/graphhelm/SKILL.md",
                "plugins/graphhelm/skills/graphhelm-setup/SKILL.md",
                "plugins/graphhelm/agents/worker.md",
                "plugins/graphhelm/hooks/session_hook.py",
                "plugins/graphhelm/README.md",
                "plugins/example-plugin/hooks/worker.mjs",
            ]
            *_, tools, validate, other = reach(*changed, repo=repo)

        self.assertEqual(validate, [])
        self.assertEqual(set(tools), {
            "python -m unittest plugins/graphhelm/hooks/test_session_hook.py",
            "python -m unittest plugins/example-plugin/hooks/test_worker.py",
            "python -m unittest tools/evals/test_skill_twins.py",
        })
        self.assertEqual(other, [])

    def test_shared_hook_imports_reach_consumer_tests_and_parity_reader(self):
        """Protect real imports/readers; existing filename-only fixtures miss shared helpers.

        Cost: filesystem fixtures only, milliseconds; no network or Cargo.
        """
        with tempfile.TemporaryDirectory() as repo:
            hooks = Path(repo, "plugins", "graphhelm", "hooks")
            hooks.mkdir(parents=True)
            files = {
                "session_hook.py": "",
                "subagent_hook.py": "from session_hook import request\n",
                "task_handoff.py": "from session_hook import request\n",
                "run_team.py": "from task_handoff import request\nfrom session_hook import ID\n",
                "keel_stop_hook.py": "",
                "keel_pretool_hook.py": "from keel_stop_hook import CARD\n",
                "test_session_hook.py": (
                    'SCRIPT = Path(__file__).with_name("session_hook.py")\n'
                    'SCRIPT.read_bytes()\n'
                    '(SCRIPT.parent / "subagent_hook.py").read_bytes()\n'
                    '(SCRIPT.parent / "task_handoff.py").read_bytes()\n'
                    '(SCRIPT.parent / "run_team.py").read_bytes()\n'
                ),
                "test_subagent_hook.py": "",
                "test_task_handoff.py": "",
                "test_run_team.py": "",
                "test_keel_stop_hook.py": "",
                "test_keel_pretool_hook.py": "",
            }
            for name, content in files.items():
                (hooks / name).write_text(content, encoding="utf-8")

            *_, tools, _, other = reach("plugins/graphhelm/hooks/session_hook.py", repo=repo)
            self.assertEqual(set(tools), {
                "python -m unittest plugins/graphhelm/hooks/test_session_hook.py",
                "python -m unittest plugins/graphhelm/hooks/test_subagent_hook.py",
                "python -m unittest plugins/graphhelm/hooks/test_task_handoff.py",
                "python -m unittest plugins/graphhelm/hooks/test_run_team.py",
            })
            self.assertEqual(other, [])

            *_, tools, _, other = reach("plugins/graphhelm/hooks/keel_stop_hook.py", repo=repo)
            self.assertEqual(set(tools), {
                "python -m unittest plugins/graphhelm/hooks/test_keel_stop_hook.py",
                "python -m unittest plugins/graphhelm/hooks/test_keel_pretool_hook.py",
            })
            self.assertEqual(other, [])

            *_, tools, _, other = reach("plugins/graphhelm/hooks/subagent_hook.py", repo=repo)
            self.assertIn("python -m unittest plugins/graphhelm/hooks/test_session_hook.py", tools)
            self.assertIn("python -m unittest plugins/graphhelm/hooks/test_subagent_hook.py", tools)
            self.assertEqual(other, [])

    def test_deleted_or_renamed_hook_test_paths_preserve_surviving_observers(self):
        """A missing path must not be read or executed; surviving tests remain selected.

        Cost: filesystem fixtures only, milliseconds; no network or Cargo.
        """
        with tempfile.TemporaryDirectory() as repo:
            hooks = Path(repo, "plugins", "graphhelm", "hooks")
            hooks.mkdir(parents=True)
            (hooks / "test_session_hook.py").write_text("import unittest\n", encoding="utf-8")
            (hooks / "test_lane_stop_hook.py").write_text("import unittest\n", encoding="utf-8")
            (hooks / "test_lane_exit_hook.py").write_text("import unittest\n", encoding="utf-8")

            # Deleted path, and the old side of a rename: neither may become a command.
            for old_path in ("test_retired_hook.py", "test_old_lane_hook.py"):
                *_, tools, _, other = reach("plugins/graphhelm/hooks/" + old_path, repo=repo)
                self.assertEqual(set(tools), {
                    "python -m unittest plugins/graphhelm/hooks/test_session_hook.py",
                    "python -m unittest plugins/graphhelm/hooks/test_lane_exit_hook.py",
                    "python -m unittest plugins/graphhelm/hooks/test_lane_stop_hook.py",
                })
                self.assertEqual(other, [])
            *_, tools, _, other = reach("plugins/graphhelm/hooks/test_lane_exit_hook.py", repo=repo)
            self.assertIn("python -m unittest plugins/graphhelm/hooks/test_lane_exit_hook.py", tools)
            self.assertEqual(other, [])

    def test_deleted_hook_test_without_surviving_observer_is_explicitly_unsupported(self):
        """Do not silently pass a deleted observer when no replacement remains.

        Cost: filesystem fixture only, milliseconds; no network or Cargo.
        """
        with tempfile.TemporaryDirectory() as repo:
            hooks = Path(repo, "plugins", "graphhelm", "hooks")
            hooks.mkdir(parents=True)
            with patch.object(rt, "run", return_value="plugins/graphhelm/hooks/test_removed.py"), \
                    patch.object(rt, "workspace", return_value=[]), patch.object(rt, "embeds", return_value={}), \
                    patch.object(rt.shutil, "which", return_value=None):
                output = io.StringIO()
                with redirect_stdout(output):
                    self.assertEqual(rt.main(["--repo", repo, "--base", "base", "--head", "head", "--json"]), 0)
        plan = json.loads(output.getvalue())
        self.assertEqual(plan["tools"], [])
        self.assertEqual(plan["unmapped"], [])
        self.assertEqual(plan["unsupported"], [{"path": "plugins/graphhelm/hooks/test_removed.py",
                                                "reason": "deleted hook test has no surviving observer"}])

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

    def test_a_markdown_change_under_docs_or_an_entry_point_runs_the_docs_guard(self):
        # #1248: before this rule a `.md` change mapped nothing, so an orphan or a dead link
        # introduced by a docs-only PR was never observed.
        repo = Path(__file__).resolve().parents[2]
        guard = "python -m unittest tools/docs-guard/test_docs_reachable.py"
        for path in ("docs/process/DELIVERY.md", "README.md", "AGENTS.md"):
            *_, tools, _, other = reach(path, repo=repo)
            self.assertIn(guard, tools, path)
            self.assertEqual(other, [], path)
        *_, tools, _, _ = reach("plugins/graphhelm/README.md", repo=repo)
        self.assertNotIn(guard, tools, "a plugin README is not a docs/ document")

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
        self.assertIn("cargo +1.97.1 test --locked -p graphhelm-cli --bin graphhelm -- --test-threads=2 commands::journey_explore::",
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


class RootFiles(unittest.TestCase):

    def test_every_tracked_root_file_maps_to_a_test_or_an_explicit_no_test_entry(self):
        # #1459 item 19: a root Dockerfile landed with no rule and stopped the post-merge main run.
        repo = Path(__file__).resolve().parents[2]
        listed = subprocess.run(["git", "ls-files", "-z"], cwd=repo, capture_output=True, text=True,
                                check=True, creationflags=rt.NO_WINDOW).stdout.split(chr(0))
        roots = sorted(path for path in listed if path and "/" not in path)
        self.assertIn("Dockerfile", roots)
        unmapped = [path for path in roots if rt.reach([path], PACKAGES, EMBEDDED, repo)[5]]
        self.assertEqual([], unmapped)


if __name__ == "__main__":
    unittest.main()
