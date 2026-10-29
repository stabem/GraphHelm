"""Contract tests for the Keel PreToolUse gate, against throwaway git repositories."""

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import os

# Owner rule: children run hidden in the calling terminal, never in a new console window.
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0

SCRIPT = Path(__file__).with_name("keel_pretool_hook.py")


def git(repo, *args):
    subprocess.run(["git", "-C", str(repo), *args], check=True, capture_output=True, creationflags=NO_WINDOW)


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
                             capture_output=True, text=True, check=True, creationflags=NO_WINDOW).stdout
        return json.loads(out)["hookSpecificOutput"] if out.strip() else None

    def test_large_code_write_without_card_is_denied(self):
        result = self.run_hook("app.py", content="x = 1\n" * 20)
        self.assertEqual(result["permissionDecision"], "deny")
        self.assertIn("keel-card.json", result["permissionDecisionReason"])

    def test_master_only_repository_still_gates(self):
        # #2180: a repo whose only base is master once let every edit through without a card.
        with tempfile.TemporaryDirectory() as other:
            repo = Path(other)
            git(repo, "init", "-q", "-b", "master")
            git(repo, "config", "user.email", "t@t")
            git(repo, "config", "user.name", "t")
            (repo / "README.md").write_text("x\n")
            git(repo, "add", ".")
            git(repo, "commit", "-qm", "base")
            payload = {"cwd": str(repo), "tool_name": "Write",
                       "tool_input": {"file_path": str(repo / "app.py"), "content": "x = 1\n" * 20}}
            out = subprocess.run([sys.executable, str(SCRIPT)], input=json.dumps(payload),
                                 capture_output=True, text=True, check=True, creationflags=NO_WINDOW).stdout
        self.assertEqual(json.loads(out)["hookSpecificOutput"]["permissionDecision"], "deny")

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

    def write_card(self, **card):
        (self.repo / ".graphhelm").mkdir(exist_ok=True)
        (self.repo / ".graphhelm" / "keel-card.json").write_text(json.dumps(card))
        (self.repo / ".gitignore").write_text("/.graphhelm/\n")

    def test_source_edit_in_scope_before_any_test_is_denied(self):
        self.write_card(promise="p", scopePaths=["src/"], proof="python -m unittest")
        result = self.run_hook("src/app.py", content="x = 1\n")
        self.assertEqual(result["permissionDecision"], "deny")
        self.assertIn("test first", result["permissionDecisionReason"])

    def test_writing_the_test_in_scope_passes(self):
        self.write_card(promise="p", scopePaths=["src/"], proof="python -m unittest")
        self.assertIsNone(self.run_hook("src/test_app.py", content="x = 1\n"))

    def test_source_edit_after_test_in_scope_changed_passes(self):
        self.write_card(promise="p", scopePaths=["src/app.py", "src/test_app.py"], proof="p")
        (self.repo / "src").mkdir()
        (self.repo / "src" / "test_app.py").write_text("def test(): assert False\n")
        self.assertIsNone(self.run_hook("src/app.py", content="x = 1\n"))

    def test_a_test_outside_scope_does_not_unlock_the_edit(self):
        self.write_card(promise="p", scopePaths=["src/"], proof="p")
        (self.repo / "tests").mkdir()
        (self.repo / "tests" / "test_other.py").write_text("x\n")
        self.assertEqual(self.run_hook("src/app.py", content="x\n")["permissionDecision"], "deny")

    def test_waiver_needs_a_reason(self):
        self.write_card(promise="p", scopePaths=["src/"], proof="p", testFirst=False)
        self.assertEqual(self.run_hook("src/app.py", content="x\n")["permissionDecision"], "deny")
        self.write_card(promise="p", scopePaths=["src/"], proof="p", testFirst=False,
                        testFirstReason="config only")
        self.assertIsNone(self.run_hook("src/app.py", content="x\n"))

    def test_file_outside_repository_passes(self):
        with tempfile.TemporaryDirectory() as other:
            payload = {"cwd": str(self.repo), "tool_input": {"file_path": other + "/x.py",
                                                              "content": "x\n" * 50}}
            out = subprocess.run([sys.executable, str(SCRIPT)], input=json.dumps(payload),
                                 capture_output=True, text=True, check=True, creationflags=NO_WINDOW).stdout
            self.assertEqual(out.strip(), "")


if __name__ == "__main__":
    unittest.main()
