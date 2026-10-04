"""Contract tests for the Keel Stop gate, against throwaway git repositories."""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("keel_stop_hook.py")


def git(repo, *args):
    subprocess.run(["git", "-C", str(repo), *args], check=True, capture_output=True)


class KeelStopHook(unittest.TestCase):
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

    def run_hook(self, **payload):
        payload.setdefault("cwd", str(self.repo))
        env = dict(os.environ, PATH=os.path.dirname(sys.executable) + os.pathsep + "/usr/bin:/bin")
        out = subprocess.run([sys.executable, str(SCRIPT)], input=json.dumps(payload),
                             capture_output=True, text=True, env=env, check=True).stdout
        return json.loads(out) if out.strip() else None

    def code(self, lines):
        (self.repo / "app.py").write_text("".join(f"x{i} = {i}\n" for i in range(lines)))

    def test_large_code_change_without_card_blocks(self):
        self.code(20)
        result = self.run_hook()
        self.assertEqual(result["decision"], "block")
        self.assertIn("keel-card.json", result["reason"])

    def test_committed_change_without_card_blocks(self):
        self.code(20)
        git(self.repo, "add", ".")
        git(self.repo, "commit", "-qm", "code")
        self.assertEqual(self.run_hook()["decision"], "block")

    def test_card_present_lets_turn_end(self):
        self.code(20)
        (self.repo / ".graphhelm").mkdir()
        (self.repo / ".graphhelm" / "keel-card.json").write_text(
            json.dumps({"promise": "p", "scopePaths": ["app.py"], "proof": "true"}))
        (self.repo / ".gitignore").write_text("/.graphhelm/\n")
        self.assertIsNone(self.run_hook())

    def test_docs_only_change_passes(self):
        (self.repo / "docs").mkdir()
        (self.repo / "docs" / "a.md").write_text("line\n" * 50)
        self.assertIsNone(self.run_hook())

    def test_tiny_code_change_passes(self):
        self.code(3)
        self.assertIsNone(self.run_hook())

    def test_second_stop_never_blocks(self):
        self.code(20)
        self.assertIsNone(self.run_hook(stop_hook_active=True))

    def test_outside_a_repository_passes(self):
        with tempfile.TemporaryDirectory() as other:
            self.assertIsNone(self.run_hook(cwd=other))


if __name__ == "__main__":
    unittest.main()
