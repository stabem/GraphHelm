"""The selection runner turns one headless session into one observation, with no judge.

#184: whether an agent picked a skill on its own is a fact in its transcript (a `Skill` tool call),
not a rubric verdict. These cells pin that reading on fixture transcripts: a picked skill, a skill
named with its plugin prefix, no skill, and a run that failed or left no transcript (INCOMPLETE,
never a negative). Cost: in-memory fixtures, no model, no process.
"""
import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import run_selection as runner  # noqa: E402
import selection_report as report  # noqa: E402


def line(obj):
    return json.dumps(obj)


def assistant(*content):
    return line({"type": "assistant", "message": {"role": "assistant", "content": list(content)}})


SKILL_CALL = {"type": "tool_use", "name": "Skill", "id": "t1", "input": {"skill": "graphhelm-development-contracts:test-audit"}}
OTHER_CALL = {"type": "tool_use", "name": "Read", "id": "t2", "input": {"file_path": "x"}}
TEXT = {"type": "text", "text": "Use the test-audit skill"}


class CasePrompt(unittest.TestCase):
    # The dry run fed a prompt starting with `---` to the CLI as an argument and it was read as an
    # option (`unknown option '---`): the frontmatter is the case's config, the body is the prompt.
    def test_frontmatter_sets_turns_and_tools_and_is_not_part_of_the_prompt(self):
        body, turns, tools = runner.split_case("---\nmax_turns: 4\nallowed_tools: [Skill, Read]\n---\n\nWhat is 17 plus 25?\n")
        self.assertEqual((body, turns, tools), ("What is 17 plus 25?", 4, ["Skill", "Read"]))
        self.assertEqual(runner.split_case("Just a prompt.\n"), ("Just a prompt.", None, None))

    def test_every_checked_in_case_parses(self):
        for prompt in sorted(runner.CASES.glob("*/prompt.md")):
            body, _, _ = runner.split_case(prompt.read_text(encoding="utf-8"))
            self.assertTrue(body and not body.startswith("---"), prompt.parent.name)


class ObservationFromTranscript(unittest.TestCase):
    def test_a_skill_call_is_the_invoked_skill_without_its_plugin_prefix(self):
        lines = [line({"type": "user"}), assistant(OTHER_CALL), assistant(TEXT, SKILL_CALL)]
        obs = runner.observation("test-audit-junk-unprompted", 1, "s-1", True, lines, 0.31)
        self.assertEqual(obs["status"], "completed")
        self.assertEqual(obs["invoked_skills"], ["test-audit"])
        self.assertEqual(obs["completion"], "unjudged")

    def test_text_that_names_a_skill_is_not_a_selection(self):
        obs = runner.observation("no-skill-arithmetic", 1, "s-2", True, [assistant(TEXT, OTHER_CALL)], 0.1)
        self.assertEqual(obs["invoked_skills"], [])

    def test_a_failed_run_or_missing_transcript_is_incomplete_not_a_miss(self):
        self.assertEqual(runner.observation("keel-scope", 1, "s-3", False, [assistant(SKILL_CALL)], None)["status"], "infra_error")
        self.assertEqual(runner.observation("keel-scope", 1, "s-4", True, None, None)["status"], "infra_error")

    def test_the_report_counts_selection_and_leaves_unjudged_completion_incomplete(self):
        observations = [
            runner.observation("test-audit-junk-unprompted", 1, "a", True, [assistant(SKILL_CALL)], 0.2),
            runner.observation("no-skill-arithmetic", 1, "b", True, [assistant(OTHER_CALL)], 0.2),
            runner.observation("keel-scope-unprompted", 1, "c", False, None, None),
        ]
        catalog = report.case_catalog()
        for item in observations:
            self.assertIn(item["case_id"], catalog)
        built = report.build_report(observations, catalog)
        self.assertEqual(built["selection"]["unprompted"]["correct_trigger"], 1)
        self.assertEqual(built["selection"]["unprompted"]["incomplete"], 1)
        self.assertEqual(built["selection"]["no_skill"]["correct_abstain"], 1)
        self.assertEqual(built["completion"]["no_skill"], {"pass": 0, "fail": 0, "incomplete": 1})


class Budget(unittest.TestCase):
    # #1587 review (gh-claude-8): a run with no reported cost (timeout, failure, unparsed JSON)
    # counted as $0, so a batch of them never stopped at --budget-usd. Unknown cost is charged.
    def run_batch(self, cost, budget, unknown=1.0):
        calls = []

        def one(case_id, run):
            calls.append((case_id, run))
            return {"case_id": case_id, "run": run, "status": "completed" if cost is not None else "infra_error",
                    "invoked_skills": [], "completion": "unjudged", "rubric": "selection-only",
                    **({"cost_usd": cost} if cost is not None else {})}
        observations, spent, stopped = runner.run_batch(["a", "b"], 3, one, budget, unknown)
        return calls, observations, spent, stopped

    def test_known_cost_stops_once_the_budget_is_reached(self):
        calls, observations, spent, stopped = self.run_batch(5.0, 12.0)
        self.assertEqual((len(calls), spent, stopped), (3, 15.0, True))
        self.assertEqual(len(observations), 3)

    def test_unknown_cost_is_charged_and_stops_the_batch(self):
        calls, _, spent, stopped = self.run_batch(None, 1.0, unknown=0.6)
        self.assertEqual((len(calls), stopped), (2, True))
        self.assertAlmostEqual(spent, 1.2)

    def test_unknown_cost_is_charged_at_least_the_highest_seen(self):
        costs = iter([2.0, None, None])

        def one(case_id, run):
            c = next(costs)
            return {"case_id": case_id, "run": run, "status": "completed", "invoked_skills": [],
                    "completion": "unjudged", "rubric": "selection-only", **({"cost_usd": c} if c is not None else {})}
        _, spent, stopped = runner.run_batch(["a"], 3, one, 5.0, 0.1)
        self.assertEqual((spent, stopped), (6.0, False))

    def test_a_batch_under_budget_runs_every_case(self):
        calls, _, spent, stopped = self.run_batch(0.25, 20.0)
        self.assertEqual((len(calls), stopped), (6, False))


class Command(unittest.TestCase):
    # #184 re-measure: a two-arm comparison loads one plugin copy per arm with --plugin-dir, the
    # installed plugins equal in both arms; the prompt never rides on the command line.
    def test_plugin_dirs_are_passed_and_the_prompt_is_not_an_argument(self):
        cmd = runner.build_command("claude", "sid", "sonnet", 4, ["Skill"], ["D:/a", "D:/b"])
        self.assertEqual(cmd[:2], ["claude", "-p"])
        self.assertEqual([cmd[i + 1] for i, x in enumerate(cmd) if x == "--plugin-dir"], ["D:/a", "D:/b"])
        self.assertIn("--allowedTools", cmd)
        self.assertNotIn("--plugin-dir", runner.build_command("claude", "sid", "sonnet", 4, None, []))


class PluginDirGuard(unittest.TestCase):
    # #1595 review (gh-claude-8): a missing --plugin-dir ran a paid case and exited 0, so that arm
    # silently measured the installed plugins. It is refused before any run.
    def test_a_missing_or_non_plugin_dir_is_refused_before_any_run(self):
        import tempfile
        calls = []
        saved = runner.run_case, runner.shutil.which
        runner.run_case = lambda *a, **k: calls.append(a) or runner.observation("keel-scope", 1, "s", True, [], 0.1)
        runner.shutil.which = lambda _: "claude"
        try:
            with tempfile.TemporaryDirectory() as tmp:
                out = Path(tmp) / "o.json"
                code = runner.main(["--workdir", tmp, "--out", str(out), "--repeats", "1", "--case", "keel-scope",
                                    "--plugin-dir", str(Path(tmp) / "does-not-exist")])
                self.assertEqual((code, calls), (2, []))
                code = runner.main(["--workdir", tmp, "--out", str(out), "--repeats", "1", "--case", "keel-scope",
                                    "--plugin-dir", tmp])
                self.assertEqual((code, calls), (2, []))
                (Path(tmp) / ".claude-plugin").mkdir()
                (Path(tmp) / ".claude-plugin" / "plugin.json").write_text("{}", encoding="utf-8")
                code = runner.main(["--workdir", tmp, "--out", str(out), "--repeats", "1", "--case", "keel-scope",
                                    "--plugin-dir", tmp])
                self.assertEqual((code, len(calls)), (0, 1))
        finally:
            runner.run_case, runner.shutil.which = saved


if __name__ == "__main__":
    unittest.main()
