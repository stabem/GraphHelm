"""Offline pairing contract: unprompted evals must not gain hints or drift.

Uses only the standard library; reads small checked-in Markdown files in milliseconds.
Run with pytest or unittest. No models, tools, network, or Runtime stores are invoked.
"""

from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[2]
EVAL_ROOTS = (
    ROOT / "plugins/graphhelm/evals",
    ROOT / "evals/graphhelm-development-contracts",
)
INVOCATION = re.compile(r"Use the ([\w-]+) skill(?: in audit mode)?")


class SkillTwinsTest(unittest.TestCase):
    def test_no_skill_arithmetic_is_a_complete_negative_control(self):
        case = ROOT / "evals/graphhelm-development-contracts/no-skill-arithmetic"
        prompt = (case / "prompt.md").read_text(encoding="utf-8")
        criteria = (case / "graders/criteria.md").read_text(encoding="utf-8")
        self.assertEqual(
            prompt,
            "---\nmax_turns: 4\nallowed_tools: [Skill]\n---\n\n"
            "What is 17 plus 25? Reply with only the number.\n",
        )
        self.assertIn("completed transcript", criteria)
        self.assertIn("final answer is 42", criteria)
        self.assertIn("no Skill invocation", criteria)
        for incomplete in ("missing transcript", "truncated transcript", "tool failure", "infrastructure failure"):
            self.assertIn(incomplete, criteria)
        self.assertIn("INCOMPLETE", criteria)

    def test_unprompted_twins_preserve_scenarios_without_skill_hints(self):
        skill_names = set()
        skill_paths = list(ROOT.glob("plugins/*/skills/*/SKILL.md"))
        skill_paths.extend(ROOT.glob("extensions/builtin/*/skills/*/SKILL.md"))
        for path in skill_paths:
            name = re.search(r"^name:\s*([^\r\n]+)", path.read_text(encoding="utf-8"), re.M)
            self.assertIsNotNone(name, str(path))
            skill_names.add(name[1].strip().strip("\"'"))
        self.assertTrue(skill_names, "No skill names discovered")
        originals = []
        for eval_root in EVAL_ROOTS:
            for path in sorted(eval_root.glob("*/prompt.md")):
                if path.parent.name.endswith("-unprompted"):
                    continue
                prompt = path.read_text(encoding="utf-8")
                if INVOCATION.search(prompt):
                    originals.append((path, prompt))
        self.assertTrue(originals, "No prompted evals discovered")
        for path, prompt in originals:
            with self.subTest(eval=path.parent.name):
                invocation = INVOCATION.search(prompt)
                skill = invocation[1]
                self.assertIn(skill, skill_names)
                twin = path.parent.with_name(path.parent.name + "-unprompted")
                self.assertTrue((twin / "prompt.md").is_file(), f"Missing twin: {twin.name}")
                twin_prompt = (twin / "prompt.md").read_text(encoding="utf-8")
                for name in skill_names:
                    self.assertIsNone(
                        re.search(r"(?<![\w-])" + re.escape(name) + r"(?![\w-])", twin_prompt, re.I),
                        f"Skill hint {name} in {twin.name}",
                    )
                # Any graphhelm-* identifier is a hint, even if its skill was renamed.
                self.assertIsNone(re.search(r"\bgraphhelm-[\w-]+", twin_prompt, re.I))
                original_parts = prompt.split("---", 2)
                twin_parts = twin_prompt.split("---", 2)
                self.assertEqual(len(original_parts), 3, "Original frontmatter missing")
                self.assertEqual(len(twin_parts), 3, "Twin frontmatter missing")
                self.assertEqual(twin_parts[:2], original_parts[:2], "Frontmatter drift")
                self.assertEqual(
                    " ".join(twin_parts[2].split()),
                    " ".join(
                        self._scenario_without_skill_hint(
                            original_parts[2], INVOCATION.search(original_parts[2])
                        ).split()
                    ),
                    "Scenario drift",
                )
                criteria = (path.parent / "graders/criteria.md").read_text(encoding="utf-8")
                twin_criteria = (twin / "graders/criteria.md").read_text(encoding="utf-8")
                self.assertTrue(twin_criteria.startswith(criteria), "Original criteria changed")
                self.assertEqual(
                    twin_criteria[len(criteria):].splitlines(),
                    [f"Pass only if the transcript shows the `{skill}` skill was invoked."],
                    "Expected exactly one added skill-invocation condition",
                )

    @staticmethod
    def _scenario_without_skill_hint(prompt, invocation):
        start, end = invocation.span()
        if invocation.group(0).endswith(" in audit mode"):
            return prompt[:start] + "Audit" + prompt[end:]
        if prompt[end:end + 1] == ".":
            end += 1
        return prompt[:start] + prompt[end:]


if __name__ == "__main__":
    unittest.main()
