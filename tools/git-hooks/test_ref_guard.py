import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).parent
INSTALL = HERE / "install.py"
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0


def git(repo, *args, env=None):
    return subprocess.run(["git", "-C", str(repo), *args], capture_output=True, text=True, timeout=120,
                          env={**os.environ, **(env or {})}, creationflags=NO_WINDOW)


class RefGuardTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.repo = Path(self.temp.name) / "repo"
        self.repo.mkdir()
        git(self.repo, "init", "-q", "-b", "main")
        git(self.repo, "config", "user.email", "t@example.com")
        git(self.repo, "config", "user.name", "t")
        git(self.repo, "commit", "-q", "--allow-empty", "-m", "base")
        self.git_dir = self.repo / ".git"

    def tearDown(self):
        self.temp.cleanup()

    def install(self, *extra):
        return subprocess.run([sys.executable, "-I", str(INSTALL), "--git-dir", str(self.git_dir), *extra],
                              capture_output=True, text=True, timeout=60, creationflags=NO_WINDOW)

    @property
    def hook(self):
        return self.git_dir / "hooks" / "reference-transaction"

    def test_deleting_a_watched_branch_warns_and_still_succeeds(self):
        self.assertEqual(self.install().returncode, 0)
        git(self.repo, "branch", "issue-1-x")
        result = git(self.repo, "branch", "-D", "issue-1-x")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("graphhelm-ref-guard: WARNING refs/heads/issue-1-x deleted", result.stderr)
        self.assertEqual(git(self.repo, "branch", "--list", "issue-1-x").stdout.strip(), "")

    def test_a_non_fast_forward_move_warns_and_a_fast_forward_does_not(self):
        self.install()
        git(self.repo, "branch", "issue-2-x")
        git(self.repo, "commit", "-q", "--allow-empty", "-m", "ahead")
        ahead = git(self.repo, "rev-parse", "HEAD").stdout.strip()
        forward = git(self.repo, "update-ref", "refs/heads/issue-2-x", ahead)
        self.assertEqual((forward.returncode, forward.stderr), (0, ""))
        back = git(self.repo, "update-ref", "refs/heads/issue-2-x", "HEAD~1")
        self.assertEqual(back.returncode, 0, back.stderr)
        self.assertIn("moved off its history", back.stderr)

    def test_an_unwatched_branch_prints_nothing(self):
        self.install()
        git(self.repo, "branch", "feature-x")
        result = git(self.repo, "branch", "-D", "feature-x")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("graphhelm-ref-guard", result.stderr)

    def test_an_internal_error_still_lets_the_update_through_with_exit_zero(self):
        self.install()
        git(self.repo, "branch", "issue-3-x")
        result = git(self.repo, "branch", "-D", "issue-3-x", env={"GRAPHHELM_REF_GUARD_FORCE_ERROR": "1"})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(git(self.repo, "branch", "--list", "issue-3-x").stdout.strip(), "")

    def test_install_twice_leaves_the_same_bytes(self):
        self.install()
        first = self.hook.read_bytes()
        again = self.install()
        self.assertEqual(again.stdout.strip(), "unchanged")
        self.assertEqual(self.hook.read_bytes(), first)

    def test_an_existing_hook_is_backed_up_and_uninstall_restores_it(self):
        self.hook.parent.mkdir(exist_ok=True)
        original = b"#!/bin/sh\n# someone else's hook\nexit 0\n"
        self.hook.write_bytes(original)
        self.install()
        backup = self.hook.with_name("reference-transaction.bak-graphhelm")
        self.assertEqual(backup.read_bytes(), original)
        self.install()  # a second install keeps the first backup
        self.assertEqual(backup.read_bytes(), original)
        self.assertEqual(self.install("--uninstall").returncode, 0)
        self.assertEqual(self.hook.read_bytes(), original)
        self.assertFalse(backup.exists())

    def test_uninstall_removes_what_the_installer_added_and_leaves_a_foreign_hook(self):
        self.install()
        self.install("--uninstall")
        self.assertFalse(self.hook.exists())
        self.hook.parent.mkdir(exist_ok=True)
        self.hook.write_bytes(b"#!/bin/sh\nexit 0\n")
        self.assertIn("left alone", self.install("--uninstall").stdout)
        self.assertTrue(self.hook.exists())


if __name__ == "__main__":
    unittest.main()
