"""The journey fixture must refuse the protected Runtime before any side effect."""

import os
import pathlib
import subprocess
import tempfile
import time
import unittest

# Owner rule: children run hidden in the calling terminal, never in a new console window.
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0


class FixturePortTest(unittest.TestCase):
    def test_protected_runtime_port_is_refused_before_fixture_creation(self):
        root = pathlib.Path(__file__).resolve().parents[2]
        shell = r"C:\Program Files\Git\bin\bash.exe" if os.name == "nt" else "bash"
        with tempfile.TemporaryDirectory() as parent:
            fixture = pathlib.Path(parent) / "fixture"
            result = subprocess.run(
                [shell, str(root / "tools/studio-journey-fixture/fixture.sh"),
                 "up", str(fixture), "8793", "5184"],
                cwd=root, capture_output=True, text=True, timeout=10, check=False, creationflags=NO_WINDOW
            )
            self.assertEqual(result.returncode, 2, result.stderr)
            self.assertIn("unsafe Runtime port", result.stderr)
            self.assertFalse(fixture.exists(), "refusal must precede fixture creation")


    def test_wait_gives_up_at_once_when_the_studio_process_is_gone(self):
        # #1448: a Studio that died at start (a broken vite install) never answers; the launcher
        # waited out its 120 s deadline and reported only a git warning. The wait returns 2 as
        # soon as the process is gone, so fixture.sh can print the Studio's own stderr tail.
        root = pathlib.Path(__file__).resolve().parents[2]
        shell = r"C:\Program Files\Git\bin\bash.exe" if os.name == "nt" else "bash"
        script = (
            f'. "{(root / "tools/studio-journey-fixture/wait.sh").as_posix()}"; '
            "sleep 0 & pid=$!; wait $pid; "
            "wait_answers http://127.0.0.1:9/never $pid 30; echo $?"
        )
        started = time.monotonic()
        result = subprocess.run(
            [shell, "-c", script], capture_output=True, text=True, timeout=25, check=False,
            creationflags=NO_WINDOW,
        )
        self.assertEqual(result.stdout.strip(), "2", result.stderr)
        self.assertLess(time.monotonic() - started, 10)


if __name__ == "__main__":
    unittest.main()
