import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("claude_plugin_check.py")
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0


class ClaudePluginCheck(unittest.TestCase):
    def run_script(self, path_dir):
        env = {**os.environ, "PATH": str(path_dir)}
        return subprocess.run([sys.executable, str(SCRIPT), "plugins/x"], env=env, capture_output=True,
                              text=True, creationflags=NO_WINDOW)

    def test_a_missing_claude_cli_is_unobserved_never_a_pass(self):
        with tempfile.TemporaryDirectory() as empty:
            done = self.run_script(empty)
        self.assertEqual(done.returncode, 0)
        self.assertTrue(done.stdout.strip().splitlines()[-1].startswith("UNOBSERVED:"), done.stdout)

    def test_a_failing_claude_cli_fails_the_step(self):
        with tempfile.TemporaryDirectory() as bin_dir:
            if os.name == "nt":
                Path(bin_dir, "claude.cmd").write_text("@exit /b 7\r\n")
            else:
                shim = Path(bin_dir, "claude")
                shim.write_text("#!/bin/sh\nexit 7\n")
                shim.chmod(0o755)
            done = self.run_script(bin_dir)
        self.assertEqual(done.returncode, 7, done.stdout + done.stderr)
        self.assertNotIn("UNOBSERVED", done.stdout)

    def test_a_plugin_without_hooks_skips_the_test_verb_and_a_failure_shows_its_text(self):
        with tempfile.TemporaryDirectory() as bin_dir, tempfile.TemporaryDirectory() as plugin:
            if os.name == "nt":
                Path(bin_dir, "claude.cmd").write_text("@echo %2 ran\r\n@exit /b 0\r\n")
            else:
                shim = Path(bin_dir, "claude")
                shim.write_text('#!/bin/sh\necho "$2 ran"\nexit 0\n')
                shim.chmod(0o755)
            env = {**os.environ, "PATH": bin_dir}
            done = subprocess.run([sys.executable, str(SCRIPT), plugin], env=env, capture_output=True,
                                  text=True, creationflags=NO_WINDOW)
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)
        self.assertIn("NOT APPLICABLE", done.stdout)
        self.assertIn("validate ran", done.stdout)
        self.assertNotIn("test ran", done.stdout)


if __name__ == "__main__":
    unittest.main()
