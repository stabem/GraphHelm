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


if __name__ == "__main__":
    unittest.main()
