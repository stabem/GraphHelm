"""The standalone keel plugin carries the repository's Keel rules verbatim, and nothing else holds them.

Each skill is frontmatter, a short preamble, a source marker naming one KEEL file, then that file's
text byte for byte. A difference on either side is a rule written in two places: this guard is red
until `python tools/keel-plugin/sync_keel_plugin.py` copies the KEEL file into its skill again.
Cost: reads a handful of files; offline.
"""
import json
import re
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(Path(__file__).resolve().parent))
import sync_keel_plugin as sync  # noqa: E402

sys.path.insert(0, str(ROOT / "plugins" / "keel" / "setup"))
import keel_setup  # noqa: E402

PLUGIN = ROOT / "plugins" / "keel"
FRONTMATTER_NAME = re.compile(r"\A---\nname: ([\w-]+)\n", re.M)


def text(path):
    return path.read_bytes().decode("utf-8").replace("\r\n", "\n")


class KeelPlugin(unittest.TestCase):
    def test_every_keel_file_has_exactly_one_skill_and_every_skill_one_keel_file(self):
        skills = sorted(path.parent.name for path in (PLUGIN / "skills").glob("*/SKILL.md"))
        self.assertEqual(skills, sorted([*sync.SKILLS, *sync.HAND_WRITTEN]),
                         "a KEEL file without a skill, or a skill that is neither generated nor hand-written")
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
                self.assertEqual(body.split(marker, 1)[1], sync.github_links(text(ROOT / source), source),
                                 f"{skill} differs from {source}: run python tools/keel-plugin/sync_keel_plugin.py")

    def test_each_skill_file_is_exactly_what_the_sync_writes(self):
        # The preamble and frontmatter have one source too: the sync script. A hand edit anywhere in
        # a skill, not only in its rule text, is a second copy that drifts.
        for skill in sync.SKILLS:
            with self.subTest(skill=skill):
                self.assertEqual(text(PLUGIN / "skills" / skill / "SKILL.md"), sync.render(skill),
                                 f"{skill} was edited by hand: run python tools/keel-plugin/sync_keel_plugin.py")

    def test_the_sync_leaves_a_crlf_checkout_of_the_same_text_untouched(self):
        # #1459 item 13: on a Windows checkout (core.autocrlf) every generated file is CRLF on disk.
        # Rewriting it as LF changes no content but makes the worktree dirty, and the reached plan
        # then refuses to run. A file whose text already matches keeps its bytes; a changed one is
        # still written.
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "SKILL.md"
            path.write_bytes(b"line one\r\nline two\r\n")
            self.assertFalse(sync.write_text(path, "line one\nline two\n"))
            self.assertEqual(path.read_bytes(), b"line one\r\nline two\r\n")
            self.assertTrue(sync.write_text(path, "line one\nline 2\n"))
            self.assertEqual(path.read_bytes(), b"line one\nline 2\n")
            fresh = Path(directory) / "new" / "KEEL.md"
            self.assertTrue(sync.write_text(fresh, "a\n"))
            self.assertEqual(fresh.read_bytes(), b"a\n")

    def test_the_graphhelm_keel_skill_carries_the_same_execute_rules(self):
        # The development-contracts `keel` skill must stay self-contained (no dependency on another
        # plugin), so it embeds the execute step's text: written by the same sync, from the same file.
        body = text(sync.GRAPHHELM_KEEL_SKILL)
        start, end = sync.embedded_markers(sync.GRAPHHELM_KEEL_SOURCE)
        self.assertEqual(body.count(start), 1, "the embedded block's start marker must appear once")
        self.assertEqual(body.count(end), 1, "the embedded block's end marker must appear once")
        embedded = body.split(start, 1)[1].split(end, 1)[0]
        self.assertEqual(embedded, text(ROOT / sync.GRAPHHELM_KEEL_SOURCE),
                         "the GraphHelm keel skill differs from its source: run python tools/keel-plugin/sync_keel_plugin.py")

    def test_no_relative_keel_link_is_left_in_the_plugin(self):
        # A plugin installed in another project has no `KEEL.md` or `keel/` beside it: a relative
        # link to either leads nowhere, so each must name the copy on GitHub.
        relative = re.compile(r"\]\((?!https?:)[^)]*(KEEL\.md|keel/)[^)]*\)")
        # The bundled setup files are copied into a project, where their relative links hold.
        for path in sorted(p for p in PLUGIN.rglob("*.md") if sync.SETUP_FILES not in p.parents):
            with self.subTest(path=path.relative_to(ROOT).as_posix()):
                self.assertEqual(relative.findall(text(path)), [], "relative KEEL link: run the sync")

    def test_the_setup_step_ships_every_keel_file_as_it_is(self):
        self.assertEqual(sorted(keel_setup.FILES), sorted(sync.SKILLS.values()))
        for source in sync.SKILLS.values():
            with self.subTest(source=source):
                self.assertEqual(text(sync.SETUP_FILES / source), text(ROOT / source),
                                 f"bundled {source} differs: run python tools/keel-plugin/sync_keel_plugin.py")

    def test_setup_installs_keel_once_and_keeps_the_projects_own_copy(self):
        def snapshot(root):
            return {p.relative_to(root).as_posix(): p.read_bytes() for p in root.rglob("*") if p.is_file()}

        with tempfile.TemporaryDirectory() as empty, tempfile.TemporaryDirectory() as own:
            root = Path(empty)
            keel_setup.main(["keel_setup", str(root)])
            first = snapshot(root)
            self.assertEqual(sorted(first), sorted([*keel_setup.FILES, "AGENTS.md", "CLAUDE.md"]))
            for name in keel_setup.POINTER_FILES:
                self.assertEqual(first[name].decode("utf-8").count(keel_setup.POINTER), 1, name)
            keel_setup.main(["keel_setup", str(root)])
            self.assertEqual(snapshot(root), first, "a second run changed bytes")

            root = Path(own)
            (root / "KEEL.md").write_bytes(b"project keel\n")
            (root / "AGENTS.md").write_bytes(b"# rules\n")
            keel_setup.main(["keel_setup", str(root)])
            self.assertEqual((root / "KEEL.md").read_bytes(), b"project keel\n")
            agents = (root / "AGENTS.md").read_bytes().decode("utf-8")
            self.assertTrue(agents.startswith("# rules\n"))
            self.assertEqual(agents.count(keel_setup.POINTER), 1)

    def test_each_skill_is_named_after_its_folder(self):
        for skill in [*sync.SKILLS, *sync.HAND_WRITTEN]:
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
