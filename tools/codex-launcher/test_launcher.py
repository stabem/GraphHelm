"""The process wrapper keeps the Codex result even if recording fails."""
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

# Owner rule: children run hidden in the calling terminal, never in a new console window.
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0


@unittest.skipUnless(os.name == "nt", "Windows cmd launcher")
class LauncherTest(unittest.TestCase):
    def test_preserves_run_exit_and_posts_end(self):
        with tempfile.TemporaryDirectory(dir="C:/gh/codex-10") as directory:
            root = Path(directory)
            (root / "codex.cmd").write_text("@echo off\r\nexit /b 7\r\n", encoding="ascii")
            (root / "graphhelm.cmd").write_text(
                "@echo off\r\necho %* > \"%GH_TEST_CALL%\"\r\nexit /b 1\r\n", encoding="ascii")
            prompt = root / "prompt.txt"
            prompt.write_text("test", encoding="ascii")
            call = root / "call.txt"
            env = {**os.environ, "PATH": str(root) + os.pathsep + os.environ["PATH"], "GH_TEST_CALL": str(call)}
            wrapper = Path(__file__).with_name("run-codex.cmd")
            result = subprocess.run(["cmd", "/c", str(wrapper), "codex-10", "unique-tag", str(prompt)],
                                    env=env, capture_output=True, timeout=10, check=False, creationflags=NO_WINDOW)
            self.assertEqual(result.returncode, 7)
            args = call.read_text(encoding="ascii")
            self.assertIn("tasks run-ended --lane \"codex-10\" --exit 7", args)
            self.assertIn("--run-id \"unique-tag\"", args)
