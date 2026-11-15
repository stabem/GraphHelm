"""The GraphHelm plugin ships three subagent definitions (#2956); this pins their shape.

An agent file that loses its `Skill` tool, drifts from its file name, or tells the agent to load a
skill that no plugin here ships fails here. Cost: reads three small files and a directory listing;
offline.
"""
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
AGENTS = ROOT / "plugins" / "graphhelm" / "agents"
EXPECTED = {"keel-author", "keel-reviewer", "keel-journey-qa"}
# Plugin name as a session invokes it -> where that plugin's skills live in this repository.
SKILL_TREES = {
    "graphhelm": "plugins/graphhelm/skills",
    "keel": "plugins/keel/skills",
    "graphhelm-development-contracts": "extensions/builtin/graphhelm-development-contracts/skills",
    "graphhelm-jpd": "extensions/builtin/graphhelm-jpd/skills",
}
# A skill a body names for loading: a backticked `plugin:skill`.
LOADED_SKILL = re.compile(r"`([a-z][a-z0-9-]*):([a-z][a-z0-9-]*)`")


def split(path):
    text = path.read_bytes().decode("utf-8").replace("\r\n", "\n")
    match = re.match(r"---\n(.*?)\n---\n(.*)", text, re.S)
    if match is None:
        raise AssertionError(f"{path.name}: no frontmatter")
    fields = {}
    for line in match.group(1).split("\n"):
        key, sep, value = line.partition(":")
        if sep:
            fields[key.strip()] = value.strip()
    return fields, match.group(2)


class PluginAgents(unittest.TestCase):
    def test_exactly_the_three_agents_ship(self):
        self.assertEqual({p.stem for p in AGENTS.glob("*.md")}, EXPECTED)

    def test_each_agent_is_well_formed(self):
        for path in sorted(AGENTS.glob("*.md")):
            fields, body = split(path)
            self.assertEqual(fields.get("name"), path.stem, f"{path.name}: name must equal the file stem")
            self.assertTrue(fields.get("description"), f"{path.name}: description is empty")
            tools = [t.strip() for t in fields.get("tools", "").split(",") if t.strip()]
            self.assertIn("Skill", tools, f"{path.name}: tools must list Skill or the agent cannot load its step skill")
            self.assertTrue(body.strip(), f"{path.name}: empty body")

    def test_every_skill_a_body_loads_exists(self):
        for path in sorted(AGENTS.glob("*.md")):
            _, body = split(path)
            loaded = LOADED_SKILL.findall(body)
            self.assertTrue(loaded, f"{path.name}: names no plugin-prefixed skill to load")
            for plugin, skill in loaded:
                tree = SKILL_TREES.get(plugin)
                self.assertIsNotNone(tree, f"{path.name}: unknown plugin {plugin!r}")
                self.assertTrue((ROOT / tree / skill / "SKILL.md").is_file(),
                                f"{path.name}: skill {plugin}:{skill} does not exist under {tree}")


KEEL_AGENTS = ROOT / "plugins" / "keel" / "agents"


class KeelPluginAgents(unittest.TestCase):
    def test_models_and_tools_follow_the_roles(self):
        self.assertEqual({p.stem for p in KEEL_AGENTS.glob("*.md")}, {"keel-author", "keel-reviewer", "keel-scout"})
        roles = {"keel-author": "sonnet", "keel-reviewer": "opus", "keel-scout": "haiku"}
        for name, model in roles.items():
            fields, _ = split(KEEL_AGENTS / f"{name}.md")
            self.assertEqual(fields.get("name"), name)
            self.assertEqual(fields.get("model"), model, name)
        fields, _ = split(KEEL_AGENTS / "keel-scout.md")
        self.assertEqual(fields.get("tools"), "Read, Glob, Grep", "the scout must have no write or shell tool")

    def test_keel_agents_load_only_keel_plugin_skills(self):
        for name in ("keel-author", "keel-reviewer"):
            _, body = split(KEEL_AGENTS / f"{name}.md")
            loaded = LOADED_SKILL.findall(body)
            self.assertTrue(loaded, name)
            for plugin, skill in loaded:
                self.assertEqual(plugin, "keel", f"{name}: names {plugin}:{skill}, which the keel plugin does not ship")
                self.assertTrue((ROOT / SKILL_TREES["keel"] / skill / "SKILL.md").is_file(), f"{name}: {skill}")


ADVISOR_VALUES = ("Advisor: consulted (<moments>)", "Advisor: not available",
                  "Advisor: available, not consulted (<reason>)")

MOMENTS = {
    "keel-author": ("before the plan is fixed", "same error comes back a second time", "before you report done"),
    "keel-reviewer": ("before you post the verdict",),
}


class AdvisorLine(unittest.TestCase):
    """The reviewer reads the author's `Advisor:` line (#2975), so each agent must state all three values."""

    def test_author_and_reviewer_state_the_three_values_in_both_plugins(self):
        for tree in (AGENTS, KEEL_AGENTS):
            for name in ("keel-author", "keel-reviewer"):
                _, body = split(tree / f"{name}.md")
                flat = " ".join(body.split())
                for value in ADVISOR_VALUES:
                    self.assertIn(value, flat, f"{tree.parent.name}/{name}: missing {value!r}")
                for moment in MOMENTS[name]:
                    self.assertIn(moment, flat, f"{tree.parent.name}/{name}: missing moment {moment!r}")


class SkillsLineAndLongCommands(unittest.TestCase):
    """The reviewer checks the author's `Skills:` line (#2978); each agent must state the rules it reads."""

    def test_author_and_reviewer_state_the_skills_line_and_no_sleep_in_both_plugins(self):
        for tree in (AGENTS, KEEL_AGENTS):
            for name in ("keel-author", "keel-reviewer"):
                _, body = split(tree / f"{name}.md")
                flat = " ".join(body.split())
                where = f"{tree.parent.name}/{name}"
                self.assertIn("`Skills: <the skills loaded, in order>`", flat, f"{where}: missing Skills line")
                self.assertIn("never wait on it with `sleep`", flat, f"{where}: missing sleep rule")
                self.assertIn("started in the background", flat, f"{where}: missing background rule")
            _, reviewer = split(tree / "keel-reviewer.md")
            self.assertIn("author's step skill is among the skills it names", " ".join(reviewer.split()),
                          f"{tree.parent.name}/keel-reviewer: missing step-skill check")
            _, author = split(tree / "keel-author.md")
            self.assertIn("never through a shell heredoc", " ".join(author.split()),
                          f"{tree.parent.name}/keel-author: missing heredoc rule")
            self.assertIn("Never run `git stash` (the stash list is shared across worktrees)",
                          " ".join(author.split()), f"{tree.parent.name}/keel-author: missing stash rule")


if __name__ == "__main__":
    unittest.main()
