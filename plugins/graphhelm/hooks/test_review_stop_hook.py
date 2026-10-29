"""Contract tests for the self-review Stop gate, against throwaway git repositories."""

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
import review_stop_hook  # noqa: E402

STOP = Path(__file__).with_name("stop_hook.py")


def git(repo, *args):
    subprocess.run(["git", "-C", str(repo), *args], check=True, capture_output=True, creationflags=NO_WINDOW)


class ReviewStopHook(unittest.TestCase):
    def setUp(self):
        self.repo = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.repo, ignore_errors=True)
        git(self.repo, "init", "-q")
        git(self.repo, "config", "user.email", "t@example.invalid")
        git(self.repo, "config", "user.name", "t")
        (self.repo / "app.py").write_text("x = 1\n", encoding="utf-8")
        git(self.repo, "add", ".")
        git(self.repo, "commit", "-q", "-m", "base")
        (self.repo / ".graphhelm").mkdir()
        (self.repo / ".graphhelm/keel-card.json").write_text("{}", encoding="utf-8")

    def stop(self, event):
        return review_stop_hook.evaluate({"cwd": str(self.repo), **event})

    def test_the_registered_stop_process_carries_the_gate(self):
        # The host runs stop_hook.py, and on Windows PowerShell pipes the event with a BOM.
        (self.repo / "app.py").write_text("x = 2\n", encoding="utf-8")
        data = b"\xef\xbb\xbf" + json.dumps({"cwd": str(self.repo)}).encode("utf-8")
        out = subprocess.run([sys.executable, str(STOP)], input=data, capture_output=True, check=True,
                             timeout=60, creationflags=NO_WINDOW).stdout
        self.assertEqual(json.loads(out)["decision"], "block")
        self.assertIn("self-review.json", json.loads(out)["reason"])

    def test_carded_diff_without_record_blocks_once_then_passes(self):
        (self.repo / "app.py").write_text("x = 2\n", encoding="utf-8")
        self.assertIn("self-review.json", self.stop({}))
        # A Stop that is already a hook continuation never blocks, so the gate cannot loop.
        self.assertIsNone(self.stop({"stop_hook_active": True}))
        (self.repo / ".graphhelm/self-review.json").write_text('{"checked": []}', encoding="utf-8")
        self.assertIsNone(self.stop({}))

    def test_record_older_than_the_card_blocks_like_a_missing_one(self):
        # A reused checkout keeps the last task's record; a new card must not pass on it (#2239).
        card = self.repo / ".graphhelm/keel-card.json"
        record = self.repo / ".graphhelm/self-review.json"
        record.write_text('{"checked": []}', encoding="utf-8")
        os.utime(record, (1_000_000_000, 1_000_000_000))
        os.utime(card, (1_000_000_100, 1_000_000_100))
        (self.repo / "app.py").write_text("x = 2\n", encoding="utf-8")
        self.assertIn("self-review.json", self.stop({}))
        self.assertIsNone(self.stop({"stop_hook_active": True}))
        # A record written in the same instant as the card is not after it.
        os.utime(record, (1_000_000_100, 1_000_000_100))
        self.assertIn("self-review.json", self.stop({}))
        # The review written after the card passes.
        os.utime(record, (1_000_000_200, 1_000_000_200))
        self.assertIsNone(self.stop({}))

    def test_no_code_diff_or_no_card_passes(self):
        self.assertIsNone(self.stop({}))  # card only: .graphhelm changes are not a code diff
        (self.repo / "app.py").write_text("x = 2\n", encoding="utf-8")
        (self.repo / ".graphhelm/keel-card.json").unlink()
        self.assertIsNone(self.stop({}))


if __name__ == "__main__":
    unittest.main()
