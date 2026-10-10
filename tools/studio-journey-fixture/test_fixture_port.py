"""The journey fixture must refuse the protected Runtime before any side effect."""

import os
import pathlib
import subprocess
import tempfile
import unittest


class FixturePortTest(unittest.TestCase):
    def test_protected_runtime_port_is_refused_before_fixture_creation(self):
        root = pathlib.Path(__file__).resolve().parents[2]
        shell = r"C:\Program Files\Git\bin\bash.exe" if os.name == "nt" else "bash"
        with tempfile.TemporaryDirectory() as parent:
            fixture = pathlib.Path(parent) / "fixture"
            result = subprocess.run(
                [shell, str(root / "tools/studio-journey-fixture/fixture.sh"),
                 "up", str(fixture), "8793", "5184"],
                cwd=root, capture_output=True, text=True, timeout=10, check=False,
            )
            self.assertEqual(result.returncode, 2, result.stderr)
            self.assertIn("unsafe Runtime port", result.stderr)
            self.assertFalse(fixture.exists(), "refusal must precede fixture creation")


if __name__ == "__main__":
    unittest.main()
