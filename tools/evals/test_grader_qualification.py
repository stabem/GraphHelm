"""The selection grader is qualified on recorded transcripts, and broken graders are caught.

#184: a grader is trusted only after it passes a known positive and fails known negatives.
Fixtures are synthetic recorded transcripts (tools/evals/fixtures/selection). Sabotaged graders
stand in for the plausible mistakes: reading text instead of tool calls, ignoring which tool
carried the field, keeping the plugin prefix, and constant verdicts. Cost: in-memory, no model.
"""
import json
import re
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import grader_qualification as gq  # noqa: E402
import run_selection as runner  # noqa: E402

PLUGIN_EVALS = Path(__file__).resolve().parents[2] / "plugins/graphhelm/evals"


def blocks(lines):
    for raw in lines:
        try:
            entry = json.loads(raw)
        except ValueError:
            continue
        content = (entry.get("message") or {}).get("content") if isinstance(entry, dict) else None
        for block in content if isinstance(content, list) else []:
            if isinstance(block, dict):
                yield block


def reads_text(lines):
    return re.findall(r"graphhelm-[a-z]+", "\n".join(lines))


def ignores_tool_name(lines):
    return [b["input"]["skill"].rsplit(":", 1)[-1] for b in blocks(lines)
            if b.get("type") == "tool_use" and (b.get("input") or {}).get("skill")]


def substring_match(lines):
    return [b["input"]["skill"] for b in blocks(lines) if b.get("type") == "tool_use" and b.get("name") == "Skill"
            and "graphhelm-resume" in b["input"]["skill"]] and ["graphhelm-resume"] or []


def keeps_plugin_prefix(lines):
    return [b["input"]["skill"] for b in blocks(lines)
            if b.get("type") == "tool_use" and b.get("name") == "Skill"]


class GraderQualification(unittest.TestCase):
    def setUp(self):
        self.expected, self.rows = gq.load_fixtures()

    def test_the_real_grader_has_no_false_pass_or_false_fail_on_recorded_transcripts(self):
        result = gq.qualify(runner.invoked_skills, self.expected, self.rows)
        self.assertEqual(result["false_pass"], [])
        self.assertEqual(result["false_fail"], [])
        self.assertTrue(result["qualified"])

    def test_the_corpus_holds_known_positives_and_several_known_negatives(self):
        labels = [label for _, label, _ in self.rows]
        self.assertGreaterEqual(labels.count("selected"), 2)
        self.assertGreaterEqual(labels.count("not_selected"), 4)

    def test_a_grader_that_reads_text_is_caught_as_a_false_pass(self):
        result = gq.qualify(reads_text, self.expected, self.rows)
        self.assertIn("neg-text-mention-only", result["false_pass"])
        self.assertIn("neg-tool-result-mention", result["false_pass"])
        self.assertFalse(result["qualified"])

    def test_a_grader_that_ignores_which_tool_carried_the_field_is_caught(self):
        result = gq.qualify(ignores_tool_name, self.expected, self.rows)
        self.assertEqual(result["false_pass"], ["neg-other-tool-with-skill-field"])
        self.assertFalse(result["qualified"])

    def test_a_grader_that_matches_substrings_is_caught(self):
        result = gq.qualify(substring_match, self.expected, self.rows)
        self.assertEqual(result["false_pass"], ["neg-longer-skill-name"])

    def test_a_grader_that_keeps_the_plugin_prefix_is_caught_as_a_false_fail(self):
        result = gq.qualify(keeps_plugin_prefix, self.expected, self.rows)
        self.assertIn("pos-plugin-prefixed", result["false_fail"])
        self.assertFalse(result["qualified"])

    def test_constant_graders_are_caught_on_the_side_they_get_wrong(self):
        always = gq.qualify(lambda lines: [self.expected], self.expected, self.rows)
        never = gq.qualify(lambda lines: [], self.expected, self.rows)
        self.assertTrue(always["false_pass"] and not always["false_fail"])
        self.assertTrue(never["false_fail"] and not never["false_pass"])
        self.assertFalse(always["qualified"] or never["qualified"])

    def test_a_corpus_missing_a_positive_or_a_negative_qualifies_nothing(self):
        positives = [r for r in self.rows if r[1] == "selected"]
        negatives = [r for r in self.rows if r[1] == "not_selected"]
        self.assertFalse(gq.qualify(runner.invoked_skills, self.expected, positives)["qualified"])
        self.assertFalse(gq.qualify(runner.invoked_skills, self.expected, negatives)["qualified"])


class PluginCorpus(unittest.TestCase):
    """Plugin cases that do not invoke by name must not name any skill; coverage is explicit."""

    SKILLS = {"graphhelm-guide", "graphhelm-kickoff", "graphhelm-resume", "graphhelm-setup", "graphhelm-team"}
    INVOKE = re.compile(r"Use the ([\w-]+) skill")
    # Skills with no eval case yet; shrinking this set is the work, growing it needs a reason.
    UNCOVERED = {"graphhelm-team"}

    def prompts(self):
        return {p.parent.name: p.read_text(encoding="utf-8") for p in sorted(PLUGIN_EVALS.glob("*/prompt.md"))}

    def test_shipped_skill_names_match_this_test(self):
        shipped = {p.parent.name for p in PLUGIN_EVALS.parent.glob("skills/*/SKILL.md")}
        self.assertEqual(shipped, self.SKILLS)

    def test_a_case_that_does_not_invoke_by_name_never_names_a_skill(self):
        for case, text in self.prompts().items():
            if self.INVOKE.search(text):
                continue
            for name in self.SKILLS:
                self.assertNotIn(name, text, f"{case} names {name}")

    def test_every_covered_skill_has_a_prompted_case_and_an_unprompted_twin(self):
        prompts = self.prompts()
        covered = set()
        for case, text in prompts.items():
            match = self.INVOKE.search(text)
            if match:
                covered.add(match[1])
                self.assertIn(case + "-unprompted", prompts)
        self.assertEqual(covered | self.UNCOVERED, self.SKILLS)
        self.assertTrue(covered.isdisjoint(self.UNCOVERED))

    def test_no_skill_cases_keep_skill_available_and_mark_failed_runs_incomplete(self):
        cases = [c for c in self.prompts() if c.startswith("no-skill-")]
        self.assertGreaterEqual(len(cases), 2)
        for case in cases:
            prompt = (PLUGIN_EVALS / case / "prompt.md").read_text(encoding="utf-8")
            self.assertIn("allowed_tools: [Skill]", prompt)
            criteria = (PLUGIN_EVALS / case / "graders/criteria.md").read_text(encoding="utf-8")
            self.assertIn("no Skill invocation", criteria)
            self.assertIn("INCOMPLETE, never a pass", criteria)


if __name__ == "__main__":
    unittest.main()
