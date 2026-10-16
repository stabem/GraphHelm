"""The standalone keel plugin carries the repository's Keel rules verbatim, and nothing else holds them.

Each skill is frontmatter, a short preamble, a source marker naming one KEEL file, then that file's
text byte for byte. A difference on either side is a rule written in two places: this guard is red
until `python tools/keel-plugin/sync_keel_plugin.py` copies the KEEL file into its skill again.
Cost: reads a handful of files; offline.
"""
import json
import re
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(Path(__file__).resolve().parent))
import sync_keel_plugin as sync  # noqa: E402

PLUGIN = ROOT / "plugins" / "keel"
FRONTMATTER_NAME = re.compile(r"\A---\nname: ([\w-]+)\n", re.M)


def text(path):
    return path.read_bytes().decode("utf-8").replace("\r\n", "\n")


class KeelPlugin(unittest.TestCase):
    def test_every_keel_file_has_exactly_one_skill_and_every_skill_one_keel_file(self):
        skills = sorted(path.parent.name for path in (PLUGIN / "skills").glob("*/SKILL.md"))
        self.assertEqual(skills, sorted(sync.SKILLS), "a KEEL file without a skill, or a skill without one")
        for source in sync.SKILLS.values():
            self.assertTrue((ROOT / source).is_file(), f"{source} is named by the plugin but missing")
        keel_files = {"KEEL.md", *(f"keel/{p.name}" for p in (ROOT / "keel").glob("KEEL_*.md"))}
        self.assertEqual(keel_files, set(sync.SKILLS.values()), "a KEEL file the plugin does not carry")

    def test_each_skill_carries_its_keel_file_byte_for_byte(self):
        for skill, source in sync.SKILLS.items():
            with self.subTest(skill=skill):
                body = text(PLUGIN / "skills" / skill / "SKILL.md")
                marker = sync.marker(source)
                self.assertEqual(body.count(marker), 1, f"{skill}: the source marker for {source} must appear once")
                self.assertEqual(body.split(marker, 1)[1], text(ROOT / source),
                                 f"{skill} differs from {source}: run python tools/keel-plugin/sync_keel_plugin.py")

    def test_each_skill_file_is_exactly_what_the_sync_writes(self):
        # The preamble and frontmatter have one source too: the sync script. A hand edit anywhere in
        # a skill, not only in its rule text, is a second copy that drifts.
        for skill in sync.SKILLS:
            with self.subTest(skill=skill):
                self.assertEqual(text(PLUGIN / "skills" / skill / "SKILL.md"), sync.render(skill),
                                 f"{skill} was edited by hand: run python tools/keel-plugin/sync_keel_plugin.py")

    def test_each_skill_is_named_after_its_folder(self):
        for skill in sync.SKILLS:
            with self.subTest(skill=skill):
                match = FRONTMATTER_NAME.match(text(PLUGIN / "skills" / skill / "SKILL.md"))
                self.assertIsNotNone(match, f"{skill}: SKILL.md must open with frontmatter naming the skill")
                self.assertEqual(match.group(1), skill)

    def test_claude_and_codex_manifests_name_one_release(self):
        claude = json.loads(text(PLUGIN / ".claude-plugin" / "plugin.json"))
        codex = json.loads(text(PLUGIN / ".codex-plugin" / "plugin.json"))
        self.assertEqual((claude["name"], codex["name"]), ("keel", "keel"))
        self.assertEqual(claude["version"], codex["version"])

    def test_both_marketplaces_offer_the_plugin(self):
        claude = json.loads(text(ROOT / ".claude-plugin" / "marketplace.json"))
        codex = json.loads(text(ROOT / ".agents" / "plugins" / "marketplace.json"))
        self.assertIn({"name": "keel", "source": "./plugins/keel"},
                      [{"name": p["name"], "source": p["source"]} for p in claude["plugins"]])
        self.assertIn("./plugins/keel", [p["source"]["path"] for p in codex["plugins"] if p["name"] == "keel"])


if __name__ == "__main__":
    unittest.main()
