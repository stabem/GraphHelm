"""The standalone keel plugin carries the repository's Keel rules verbatim, and nothing else holds them.

Each skill is frontmatter, a short preamble, a source marker naming one KEEL file, then that file's
text byte for byte. A difference on either side is a rule written in two places: this guard is red
until `python tools/keel-plugin/sync_keel_plugin.py` copies the KEEL file into its skill again.
Cost: reads a handful of files; offline.
"""
import contextlib
import io
import json
import re
import sys
import tempfile
import unittest
from unittest import mock
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
                carried = sync.github_links(text(ROOT / source), source)
                self.assertEqual(body.split(marker, 1)[1], carried + sync.tail(skill),
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
        known = json.loads(keel_setup.DIGESTS.read_bytes())
        self.assertEqual(sorted(known), sorted(sync.SKILLS.values()))
        for source in sync.SKILLS.values():
            with self.subTest(source=source):
                self.assertIn(sync.bundled_digest(source), known[source],
                              f"{source}: run python tools/keel-plugin/sync_keel_plugin.py")
        # An earlier release's KEEL.md (keel 1.1.0) stays recognisable.
        self.assertIn("5ad3cf94b0a8512caa8f7b12053020df30382a17f3cc9e5cb92ebfe1dd360815", known["KEEL.md"])
        grown = sync.with_current_digests({"KEEL.md": ["a" * 64]})
        self.assertEqual(grown["KEEL.md"], ["a" * 64, sync.bundled_digest("KEEL.md")])
        self.assertEqual(sync.with_current_digests(grown), grown)

    def test_setup_installs_keel_once_and_keeps_the_projects_own_copy(self):
        def snapshot(root):
            return {p.relative_to(root).as_posix(): p.read_bytes() for p in root.rglob("*") if p.is_file()}

        with tempfile.TemporaryDirectory() as empty, tempfile.TemporaryDirectory() as own:
            root = Path(empty)
            keel_setup.main(["keel_setup", str(root)])
            first = snapshot(root)
            self.assertEqual(sorted(first), sorted([*keel_setup.FILES, "AGENTS.md", "CLAUDE.md", keel_setup.SETTINGS]))
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

    def run_setup(self, root, *flags):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = keel_setup.main(["keel_setup", str(root), *flags])
        return code, out.getvalue()

    def test_setup_advisor_step_keeps_what_the_project_decided_and_reports_what_it_cannot_use(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            settings = root / ".claude" / "settings.json"
            self.assertEqual(self.run_setup(root)[0], 0)
            self.assertEqual(json.loads(settings.read_bytes()), {"advisorModel": "fable"})
            # Already installed: the step still runs, and a settings file made since is completed.
            settings.write_bytes(b'{"model": "sonnet", "permissions": {"allow": ["Read"]}}\r\n')
            self.run_setup(root)
            raw = settings.read_bytes()
            self.assertEqual(json.loads(raw), {"model": "sonnet", "permissions": {"allow": ["Read"]},
                                               "advisorModel": "fable"})
            self.assertIn(b"\r\n", raw)
            self.assertNotIn(b"\n", raw.replace(b"\r\n", b""))
            self.run_setup(root)
            self.assertEqual(settings.read_bytes(), raw, "a second run changed bytes")
            settings.write_bytes(b'{"advisorModel": "opus"}')
            code, said = self.run_setup(root)
            self.assertEqual((code, settings.read_bytes()), (0, b'{"advisorModel": "opus"}'))
            self.assertIn("already sets it", said)

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self.run_setup(root, "--no-advisor")
            self.assertFalse((root / ".claude").exists())
            self.run_setup(root, "--advisor", "opus")
            self.assertEqual(json.loads((root / ".claude" / "settings.json").read_bytes()), {"advisorModel": "opus"})

        for content in (b"{not json", b"[1, 2]", b"3", b""):
            with self.subTest(content=content), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                (root / ".claude").mkdir()
                (root / ".claude" / "settings.json").write_bytes(content)
                code, said = self.run_setup(root)
                self.assertEqual(code, 1)
                self.assertEqual((root / ".claude" / "settings.json").read_bytes(), content)
                self.assertTrue((root / "KEEL.md").is_file(), "the other steps must still run")
                self.assertIn("left .claude/settings.json alone", said)

    def test_a_refused_settings_write_leaves_no_temporary_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / ".claude").mkdir()
            settings = root / ".claude" / "settings.json"
            settings.write_bytes(b'{"model": "sonnet"}')
            with mock.patch.object(keel_setup.os, "replace", side_effect=PermissionError("read-only")):
                code, said = self.run_setup(root)
            self.assertEqual(code, 1)
            self.assertIn("left .claude/settings.json alone", said)
            self.assertEqual(settings.read_bytes(), b'{"model": "sonnet"}')
            self.assertEqual(sorted(p.name for p in (root / ".claude").iterdir()), ["settings.json"])

    def test_every_skill_says_the_installer_writes_the_advisor_setting_and_its_two_flags(self):
        for name in sync.SKILLS:
            skill = sync.SKILLS_DIR / name / "SKILL.md"
            text = skill.read_text(encoding="utf-8")
            for needle in (".claude/settings.json", "advisorModel", "--no-advisor", "--advisor <model>"):
                self.assertIn(needle, text, f"{skill.parent.name} lacks {needle}")

    def test_setup_reports_a_real_difference_but_not_a_line_ending_difference(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self.run_setup(root, "--no-advisor")
            keel = root / "KEEL.md"
            bundled = keel.read_bytes()
            keel.write_bytes(bundled.replace(b"\n", b"\r\n"))
            code, said = self.run_setup(root, "--no-advisor")
            self.assertNotIn("differs", said)
            changed = bundled.replace(b"Keel", b"Kiel", 1)
            keel.write_bytes(changed)
            code, said = self.run_setup(root, "--no-advisor")
            self.assertIn("kept the project's KEEL.md: it differs", said)
            self.assertEqual(keel.read_bytes(), changed)

    def install_with_old_copies(self, root, old_texts, unlisted=None):
        # Seeds the project with older bundled versions and a digest list that knows them.
        self.run_setup(root, "--no-advisor")
        known = json.loads(keel_setup.DIGESTS.read_bytes())
        for name, old in old_texts.items():
            (root / name).write_bytes(old)
            known[name] = [*known[name], keel_setup.digest(old)]
        for name, text_ in (unlisted or {}).items():  # written to the project, never listed
            (root / name).write_bytes(text_)
        listing = root.parent / (root.name + "-known.json")
        listing.write_bytes(json.dumps(known).encode("utf-8"))
        return mock.patch.object(keel_setup, "DIGESTS", listing)

    def test_setup_upgrades_an_unedited_old_copy_and_only_that(self):
        old = b"old keel text\nsecond line\n"
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "project"
            root.mkdir()
            bundled = (keel_setup.BUNDLED / "KEEL.md").read_bytes()
            edited = old.replace(b"old", b"older")  # one word differs from a listed version
            newer = b"a copy newer than this plugin knows\n"
            with self.install_with_old_copies(root, {"KEEL.md": old, "keel/KEEL_REVIEW.md": old.replace(b"\n", b"\r\n")},
                                               unlisted={"keel/KEEL_PLAN.md": edited, "keel/KEEL_EXEC.md": newer}):
                code, said = self.run_setup(root, "--no-advisor")
                self.assertEqual(code, 0)
                self.assertEqual((root / "KEEL.md").read_bytes(), bundled)
                self.assertIn("updated KEEL.md", said)
                self.assertEqual((root / "keel/KEEL_PLAN.md").read_bytes(), edited)
                self.assertIn("kept the project's keel/KEEL_PLAN.md", said)
                self.assertEqual((root / "keel/KEEL_EXEC.md").read_bytes(), newer)
                self.assertIn("kept the project's keel/KEEL_EXEC.md", said)
                crlf = (root / "keel/KEEL_REVIEW.md").read_bytes()
                self.assertIn("updated keel/KEEL_REVIEW.md", said)
                self.assertEqual(crlf, (keel_setup.BUNDLED / "keel/KEEL_REVIEW.md").read_bytes()
                                 .replace(b"\r\n", b"\n").replace(b"\n", b"\r\n"))
                before = {p.name: p.read_bytes() for p in root.rglob("*") if p.is_file()}
                code, said = self.run_setup(root, "--no-advisor")
                self.assertNotIn("updated", said)
                self.assertEqual({p.name: p.read_bytes() for p in root.rglob("*") if p.is_file()}, before,
                                 "a second run changed bytes")

    def test_a_refused_replacement_leaves_the_old_copy_and_no_temporary_file(self):
        old = b"old keel text\n"
        real_replace = keel_setup.os.replace

        def refuse_keel_md(source, target):
            if Path(target).name == "KEEL.md":
                raise PermissionError("read-only")
            return real_replace(source, target)

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "project"
            root.mkdir()
            old_exec = b"old exec\n"
            with self.install_with_old_copies(root, {"KEEL.md": old, "keel/KEEL_EXEC.md": old_exec}):
                with mock.patch.object(keel_setup.os, "replace", side_effect=refuse_keel_md):
                    code, said = self.run_setup(root, "--no-advisor")
                self.assertEqual(code, 1)
                self.assertEqual((root / "KEEL.md").read_bytes(), old)
                self.assertIn("left KEEL.md alone", said)
                self.assertEqual((root / "keel/KEEL_EXEC.md").read_bytes(),
                                 (keel_setup.BUNDLED / "keel/KEEL_EXEC.md").read_bytes(),
                                 "the other files are still brought up to date")
                self.assertEqual([p.name for p in root.rglob("*.keel-tmp")], [])

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
