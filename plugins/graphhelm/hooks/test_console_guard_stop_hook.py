"""Contract tests for the console-window guard Stop gate, against throwaway git repositories."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

# Owner rule: children run hidden in the calling terminal, never in a new console window.
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0

sys.path.insert(0, str(Path(__file__).resolve().parent))
import console_guard_stop_hook  # noqa: E402

STOP = Path(__file__).with_name("stop_hook.py")
# A stand-in guard with the real guard's contract: red when a .py spawns without NO_WINDOW.
GUARD_SOURCE = '''import pathlib, unittest
class Guard(unittest.TestCase):
    def test_spawns_are_hidden(self):
        root = pathlib.Path(__file__).resolve().parents[2]
        bad = [str(p) for p in root.rglob("*.py") if "subprocess.run(" in p.read_text() and "NO_WINDOW" not in p.read_text()
               and "reached-tests" not in str(p)]
        self.assertEqual(bad, [], "spawn without creationflags=NO_WINDOW")
'''


def git(repo, *args):
    subprocess.run(["git", "-C", str(repo), *args], check=True, capture_output=True, creationflags=NO_WINDOW)


class ConsoleGuardStopHook(unittest.TestCase):
    def setUp(self):
        self.repo = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.repo, ignore_errors=True)
        git(self.repo, "init", "-q")
        git(self.repo, "config", "user.email", "t@example.invalid")
        git(self.repo, "config", "user.name", "t")
        guard = self.repo / console_guard_stop_hook.GUARD
        guard.parent.mkdir(parents=True)
        guard.write_text(GUARD_SOURCE, encoding="utf-8")
        (self.repo / "app.py").write_text("x = 1\n", encoding="utf-8")
        git(self.repo, "add", ".")
        git(self.repo, "commit", "-q", "-m", "base")

    def stop(self, event=None):
        return console_guard_stop_hook.evaluate({"cwd": str(self.repo), **(event or {})})

    def test_a_changed_py_that_breaks_the_guard_blocks_with_the_guard_output(self):
        (self.repo / "app.py").write_text("import subprocess\nsubprocess.run(['x'])\n", encoding="utf-8")
        reason = self.stop()
        self.assertIsNotNone(reason)
        self.assertIn("test_no_console_window.py", reason)
        self.assertIn("spawn without creationflags=NO_WINDOW", reason)
        # A hook continuation never blocks, so the gate cannot loop.
        self.assertIsNone(self.stop({"stop_hook_active": True}))

    def test_a_session_started_in_a_subdirectory_is_still_checked(self):
        (self.repo / "app.py").write_text("import subprocess\nsubprocess.run(['x'])\n", encoding="utf-8")
        sub = self.repo / "docs"
        sub.mkdir()
        self.assertIsNotNone(console_guard_stop_hook.evaluate({"cwd": str(sub)}))

    def test_a_new_untracked_py_is_checked_too(self):
        (self.repo / "tools").mkdir(exist_ok=True)
        (self.repo / "tools/new_tool.py").write_text("import subprocess\nsubprocess.run(['x'])\n", encoding="utf-8")
        self.assertIsNotNone(self.stop())

    def test_a_green_guard_or_no_py_change_passes(self):
        (self.repo / "app.py").write_text(
            "import subprocess\nsubprocess.run(['x'], creationflags=NO_WINDOW)\n", encoding="utf-8")
        self.assertIsNone(self.stop())
        git(self.repo, "checkout", "--", "app.py")
        (self.repo / "notes.md").write_text("no python here\n", encoding="utf-8")
        self.assertIsNone(self.stop())

    def test_a_repository_without_the_guard_passes(self):
        (self.repo / console_guard_stop_hook.GUARD).unlink()
        (self.repo / "app.py").write_text("import subprocess\nsubprocess.run(['x'])\n", encoding="utf-8")
        self.assertIsNone(self.stop())

    def test_the_registered_stop_process_carries_the_gate(self):
        # The host runs stop_hook.py, and on Windows PowerShell pipes the event with a BOM.
        (self.repo / "app.py").write_text("import subprocess\nsubprocess.run(['x'])\n", encoding="utf-8")
        data = b"\xef\xbb\xbf" + json.dumps({"cwd": str(self.repo)}).encode("utf-8")
        env = {k: v for k, v in os.environ.items() if not k.startswith("GRAPHHELM_")}
        out = subprocess.run([sys.executable, str(STOP)], input=data, capture_output=True, check=True,
                             timeout=120, env=env, creationflags=NO_WINDOW).stdout
        decision = json.loads(out)
        self.assertEqual(decision["decision"], "block")
        self.assertIn("test_no_console_window.py", decision["reason"])


if __name__ == "__main__":
    unittest.main()
