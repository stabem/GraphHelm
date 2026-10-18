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


if __name__ == "__main__":
    unittest.main()
