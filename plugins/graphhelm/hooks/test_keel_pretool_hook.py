"""Contract tests for the Keel PreToolUse gate, against throwaway git repositories."""

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("keel_pretool_hook.py")


def git(repo, *args):
    subprocess.run(["git", "-C", str(repo), *args], check=True, capture_output=True)


class KeelPreToolHook(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.repo = Path(self.tmp.name)
        git(self.repo, "init", "-q", "-b", "main")
        git(self.repo, "config", "user.email", "t@t")
        git(self.repo, "config", "user.name", "t")
        (self.repo / "README.md").write_text("x\n")
        git(self.repo, "add", ".")
        git(self.repo, "commit", "-qm", "base")
        git(self.repo, "checkout", "-qb", "work")

    def tearDown(self):
        self.tmp.cleanup()

    def run_hook(self, path, **tool_input):
        payload = {"cwd": str(self.repo), "tool_name": "Write",
                   "tool_input": {"file_path": str(self.repo / path), **tool_input}}
        out = subprocess.run([sys.executable, str(SCRIPT)], input=json.dumps(payload),
                             capture_output=True, text=True, check=True).stdout
        return json.loads(out)["hookSpecificOutput"] if out.strip() else None

    def test_large_code_write_without_card_is_denied(self):
        result = self.run_hook("app.py", content="x = 1\n" * 20)
        self.assertEqual(result["permissionDecision"], "deny")
        self.assertIn("keel-card.json", result["permissionDecisionReason"])

    def test_small_code_edit_passes(self):
        self.assertIsNone(self.run_hook("app.py", old_string="a", new_string="b"))

    def test_docs_write_passes(self):
        self.assertIsNone(self.run_hook("docs/a.md", content="x\n" * 50))

    def test_writing_the_card_passes(self):
        self.assertIsNone(self.run_hook(".graphhelm/keel-card.json", content="{}\n" * 20))

    def test_with_card_code_write_passes(self):
        (self.repo / ".graphhelm").mkdir()
        (self.repo / ".graphhelm" / "keel-card.json").write_text("{}")
        (self.repo / ".gitignore").write_text("/.graphhelm/\n")
        self.assertIsNone(self.run_hook("app.py", content="x = 1\n" * 20))

    def test_small_edit_on_top_of_large_branch_change_is_denied(self):
        (self.repo / "big.py").write_text("y = 2\n" * 20)
        result = self.run_hook("app.py", old_string="a", new_string="b")
        self.assertEqual(result["permissionDecision"], "deny")

    def test_file_outside_repository_passes(self):
        with tempfile.TemporaryDirectory() as other:
            payload = {"cwd": str(self.repo), "tool_input": {"file_path": other + "/x.py",
                                                              "content": "x\n" * 50}}
            out = subprocess.run([sys.executable, str(SCRIPT)], input=json.dumps(payload),
                                 capture_output=True, text=True, check=True).stdout
            self.assertEqual(out.strip(), "")


if __name__ == "__main__":
    unittest.main()
