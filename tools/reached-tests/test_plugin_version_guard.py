"""Git-blob contract tests for plugin release versioning.

Cost: tiny temporary Git repositories; offline and no Cargo, browser, model, or Runtime.
"""
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import plugin_version_guard as guard  # noqa: E402

NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0


class PluginVersionGuard(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.repo = Path(self.temp.name)
        self.addCleanup(self.temp.cleanup)
        self.git("init", "-q")
        self.git("config", "user.email", "test@example.invalid")
        self.git("config", "user.name", "Test")
        self.write_manifests("0.1.28")
        self.write("plugins/graphhelm/skills/guide/SKILL.md", "# Guide\n")
        self.base = self.commit("base")

    def git(self, *args):
        return subprocess.run(["git", *args], cwd=self.repo, check=True, capture_output=True,
                              text=True, encoding="utf-8", creationflags=NO_WINDOW).stdout.strip()

    def write(self, path, content):
        target = self.repo / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content, encoding="utf-8")

    def write_manifests(self, version):
        for path in guard.MANIFESTS:
            self.write(path, json.dumps({"name": Path(path).parents[1].name, "version": version}) + "\n")

    def commit(self, message):
        self.git("add", "-A")
        self.git("commit", "-qm", message)
        return self.git("rev-parse", "HEAD")

    def check(self, head):
        return guard.check(self.repo, self.base, head)

    def test_unchanged_version_fails_with_bump_instruction_and_manifest(self):
        self.write("plugins/graphhelm/skills/guide/SKILL.md", "# Changed guide\n")
        passed, message = self.check(self.commit("ship content without bump"))
        self.assertFalse(passed)
        self.assertIn("bump the plugin version", message)
        self.assertIn(guard.MANIFESTS[0], message)

    def test_coherent_version_increase_passes_for_explicit_refs_not_working_tree(self):
        self.write("plugins/graphhelm/skills/guide/SKILL.md", "# Changed guide\n")
        self.write_manifests("0.1.29")
        head = self.commit("ship content with bump")
        self.write("plugins/graphhelm/docs/main-only.md", "main has moved\n")
        origin_main = self.commit("unrelated origin main head")
        self.git("update-ref", "refs/remotes/origin/main", origin_main)
        self.write_manifests("9.9.9")
        passed, message = guard.check(self.repo, self.base, head)
        self.assertTrue(passed, message)
        self.assertIn("0.1.29", message)

    def test_prerelease_precedence_allows_release_and_rejects_earlier_versions(self):
        self.write("plugins/graphhelm/skills/guide/SKILL.md", "# Changed guide\n")
        self.write_manifests("0.1.29-rc.1")
        base = self.commit("ship prerelease")
        self.base = base
        self.write_manifests("0.1.29")
        passed, message = self.check(self.commit("ship final release"))
        self.assertTrue(passed, message)

        self.base = self.git("rev-parse", "HEAD~1")
        self.write_manifests("0.1.29-beta.2")
        passed, message = self.check(self.commit("regress prerelease"))
        self.assertFalse(passed)
        self.assertIn("bump the plugin version", message)

    def test_longer_prerelease_with_same_prefix_is_higher(self):
        for before, after in (("0.1.29-rc", "0.1.29-rc.1"),
                              ("0.1.29-rc", "0.1.29-rc.next")):
            with self.subTest(before=before, after=after):
                self.write_manifests(before)
                self.base = self.commit("base prerelease")
                self.write("plugins/graphhelm/skills/guide/SKILL.md", f"# {after}\n")
                self.write_manifests(after)
                passed, message = self.check(self.commit("longer prerelease"))
                self.assertTrue(passed, message)

    def test_shorter_prerelease_with_same_prefix_is_lower(self):
        for before, after in (("0.1.29-rc.1", "0.1.29-rc"),
                              ("0.1.29-rc.next", "0.1.29-rc")):
            with self.subTest(before=before, after=after):
                self.write_manifests(before)
                self.base = self.commit("base prerelease")
                self.write("plugins/graphhelm/skills/guide/SKILL.md", f"# {after}\n")
                self.write_manifests(after)
                passed, message = self.check(self.commit("shorter prerelease"))
                self.assertFalse(passed)
                self.assertIn("bump the plugin version", message)

    def test_invalid_numeric_prerelease_identifier_is_rejected(self):
        self.write("plugins/graphhelm/skills/guide/SKILL.md", "# Changed guide\n")
        self.write_manifests("0.1.29-rc.01")
        passed, message = self.check(self.commit("invalid prerelease"))
        self.assertFalse(passed)
        self.assertIn("invalid release version", message)

    def test_mismatched_head_manifests_fail_with_the_path(self):
        self.write("plugins/graphhelm/hooks/session_hook.py", "# changed\n")
        self.write_manifests("0.1.29")
        self.write(guard.MANIFESTS[-1], json.dumps({"version": "0.1.30"}))
        passed, message = self.check(self.commit("mismatched release"))
        self.assertFalse(passed)
        self.assertIn("mismatched release manifests", message)
        self.assertIn(guard.MANIFESTS[-1], message)

    def test_docs_and_test_only_edits_do_not_require_bump(self):
        self.write("plugins/graphhelm/docs/release-notes.md", "Documentation\n")
        self.write("plugins/graphhelm/hooks/test_session_hook.py", "# test-only\n")
        passed, message = self.check(self.commit("docs and tests"))
        self.assertTrue(passed, message)
        self.assertIn("version bump not required", message)

    def test_shipped_markdown_requires_a_bump(self):
        self.write("plugins/graphhelm/skills/guide/SKILL.md", "# New instructions\n")
        passed, message = self.check(self.commit("change shipped markdown"))
        self.assertFalse(passed)
        self.assertIn("bump the plugin version", message)

    def test_move_from_shipped_content_to_plugin_docs_needs_no_bump(self):
        source = self.repo / "plugins/graphhelm/skills/guide/SKILL.md"
        destination = self.repo / "plugins/graphhelm/docs/SKILL.md"
        destination.parent.mkdir(parents=True, exist_ok=True)
        source.rename(destination)
        passed, message = self.check(self.commit("move guide into docs"))
        self.assertTrue(passed, message)
        self.assertIn("version bump not required", message)

    def test_deleted_or_renamed_shipped_content_cannot_evade_guard(self):
        (self.repo / "plugins/graphhelm/skills/guide/SKILL.md").unlink()
        self.write("moved-outside-plugin.md", "# Guide\n")
        passed, message = self.check(self.commit("rename shipped content"))
        self.assertFalse(passed)
        self.assertIn("bump the plugin version", message)

    def test_missing_and_invalid_release_manifests_fail_explicitly(self):
        (self.repo / guard.MANIFESTS[-1]).unlink()
        self.write("plugins/graphhelm/hooks/session_hook.py", "# changed\n")
        passed, message = self.check(self.commit("remove release manifest"))
        self.assertFalse(passed)
        self.assertIn("missing release manifest", message)
        self.assertIn(guard.MANIFESTS[-1], message)

        self.write(guard.MANIFESTS[-1], json.dumps({"version": "not-a-version"}))
        invalid_head = self.commit("restore invalid manifest")
        passed, message = self.check(invalid_head)
        self.assertFalse(passed)
        self.assertIn("invalid release version", message)
        self.assertIn(guard.MANIFESTS[-1], message)


class KeelPluginReleaseGroup(unittest.TestCase):
    """The standalone keel plugin is its own release: its version moves with its own content only."""

    setUp = PluginVersionGuard.setUp
    git = PluginVersionGuard.git
    write = PluginVersionGuard.write
    write_manifests = PluginVersionGuard.write_manifests
    commit = PluginVersionGuard.commit
    check = PluginVersionGuard.check

    def write_keel(self, version):
        for path in guard.KEEL_MANIFESTS:
            self.write(path, json.dumps({"name": "keel", "version": version}) + "\n")

    def test_a_new_plugin_group_with_coherent_head_manifests_passes(self):
        self.write_keel("0.1.0")
        self.write("plugins/keel/skills/keel/SKILL.md", "# Keel\n")
        passed, message = self.check(self.commit("add the keel plugin"))
        self.assertTrue(passed, message)

    def test_keel_content_without_a_keel_bump_fails_naming_its_manifest(self):
        self.write_keel("0.1.0")
        self.write("plugins/keel/skills/keel/SKILL.md", "# Keel\n")
        self.base = self.commit("add the keel plugin")
        self.write("plugins/keel/skills/keel/SKILL.md", "# Keel, changed\n")
        passed, message = self.check(self.commit("change keel without a bump"))
        self.assertFalse(passed)
        self.assertIn(guard.KEEL_MANIFESTS[0], message)

    def test_keel_and_graphhelm_releases_do_not_force_each_other(self):
        self.write_keel("0.1.0")
        self.write("plugins/keel/skills/keel/SKILL.md", "# Keel\n")
        self.base = self.commit("add the keel plugin")
        self.write("plugins/keel/skills/keel/SKILL.md", "# Keel, changed\n")
        self.write_keel("0.1.1")
        passed, message = self.check(self.commit("bump keel only"))
        self.assertTrue(passed, message)

        self.base = self.git("rev-parse", "HEAD")
        self.write("plugins/graphhelm/skills/guide/SKILL.md", "# Changed guide\n")
        self.write_manifests("0.1.29")
        passed, message = self.check(self.commit("bump graphhelm only"))
        self.assertTrue(passed, message)

    def test_keel_host_manifests_must_agree(self):
        self.write("plugins/keel/.claude-plugin/plugin.json", json.dumps({"name": "keel", "version": "0.1.0"}) + "\n")
        self.write("plugins/keel/.codex-plugin/plugin.json", json.dumps({"name": "keel", "version": "0.2.0"}) + "\n")
        self.write("plugins/keel/skills/keel/SKILL.md", "# Keel\n")
        passed, message = self.check(self.commit("mismatched keel manifests"))
        self.assertFalse(passed)
        self.assertIn("plugins/keel/.codex-plugin/plugin.json", message)


ROOT = Path(__file__).resolve().parents[2]
JPD_SKILL = "extensions/builtin/graphhelm-jpd/skills/journey-map/SKILL.md"
CONTRACTS_SKILL = "extensions/builtin/graphhelm-development-contracts/skills/implement/SKILL.md"


class SkillChain(unittest.TestCase):
    """#1459 items 14 and 16: one command regenerates a skill change's chain, and two skill PRs in
    different packages merge without a conflict. Cost: one temp repository holding copies of the
    two real builtin packages, their goldens and the release file; a few git commands."""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name)
        self.git("init", "-q", "-b", "main")
        for key, value in (("user.email", "t@example.invalid"), ("user.name", "T"), ("core.autocrlf", "false")):
            self.git("config", key, value)
        for tree in ("extensions/builtin", "extensions/releases", guard.GOLDENS.rstrip("/")):
            for source in (ROOT / tree).rglob("*"):
                if source.is_file():
                    target = self.repo / source.relative_to(ROOT)
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_bytes(source.read_bytes())
        self.git("add", "-A")
        self.git("commit", "-qm", "base")

    def git(self, *args):
        return subprocess.run(["git", *args], cwd=self.repo, check=True, capture_output=True,
                              text=True, encoding="utf-8", creationflags=NO_WINDOW).stdout.strip()

    def edit_skill(self, branch, skill):
        self.git("checkout", "-q", "-b", branch, "main")
        path = self.repo / skill
        path.write_bytes(path.read_bytes() + f"\nEdited on {branch}.\n".encode("utf-8"))
        first = guard.write(self.repo, "main")
        second = guard.write(self.repo, "main")
        self.git("add", "-A")
        self.git("commit", "-qm", branch)
        return first, second

    def version(self, path):
        return guard._version_field(json.loads((self.repo / path).read_bytes()))

    def test_the_command_writes_the_whole_chain_once_and_nothing_the_second_time(self):
        package = "extensions/builtin/graphhelm-jpd/"
        before = self.version(package + "extension.json")
        first, second = self.edit_skill("pr-a", JPD_SKILL)
        self.assertEqual(second, [])
        self.assertEqual(sorted(first), sorted([
            package + "extension.json", package + ".claude-plugin/plugin.json", package + ".codex-plugin/plugin.json",
            guard.GOLDENS + "graphhelm-jpd.json"]))
        major, minor, patch = (int(part) for part in before.split("."))
        for manifest in ("extension.json", ".claude-plugin/plugin.json", ".codex-plugin/plugin.json"):
            self.assertEqual(self.version(package + manifest), f"{major}.{minor}.{patch + 1}")
        passed, message = guard.check(self.repo, "main", "pr-a")
        self.assertTrue(passed, message)

    def test_a_skill_change_without_the_chain_fails_the_check_naming_the_extension(self):
        self.git("checkout", "-q", "-b", "bare", "main")
        (self.repo / JPD_SKILL).write_bytes((self.repo / JPD_SKILL).read_bytes() + b"\nbare\n")
        self.git("commit", "-qam", "bare")
        passed, message = guard.check(self.repo, "main", "bare")
        self.assertFalse(passed)
        self.assertIn("extensions/builtin/graphhelm-jpd/extension.json", message)

    def test_two_skill_prs_in_different_packages_merge_without_a_conflict(self):
        self.edit_skill("pr-a", JPD_SKILL)
        self.edit_skill("pr-b", CONTRACTS_SKILL)
        self.git("checkout", "-q", "main")
        self.git("merge", "-q", "--no-edit", "pr-a")
        merged = subprocess.run(["git", "merge", "-q", "--no-edit", "pr-b"], cwd=self.repo, capture_output=True,
                                text=True, creationflags=NO_WINDOW)
        self.assertEqual(merged.returncode, 0, merged.stdout + merged.stderr)
        self.assertEqual(guard.write(self.repo, "main"), [], "the merged main carries a current chain")


if __name__ == "__main__":
    unittest.main()
