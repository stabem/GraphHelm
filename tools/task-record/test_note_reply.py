import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("note_reply.py")


class NoteReplyTests(unittest.TestCase):
    def run_tool(self, *extra):
        with tempfile.TemporaryDirectory() as temp:
            token = Path(temp) / "t.token"
            token.write_text("secret-token-value", encoding="utf-8")
            return subprocess.run(
                [sys.executable, "-I", str(SCRIPT), "--lane", "lane-a", "--execution", "exec-1",
                 "--token-file", str(token), "--reply-to", "note-1", "--to", "owner", "--text", "done", *extra],
                capture_output=True, text=True, timeout=60)

    def test_dry_run_builds_the_reply_and_never_prints_the_token(self):
        result = self.run_tool("--dry-run")
        self.assertEqual(result.returncode, 0, result.stderr)
        body = json.loads(result.stdout)
        self.assertEqual(body["signal"]["type"], "operator_note")
        self.assertEqual(body["signal"]["replyTo"], "note-1")
        self.assertEqual(body["signal"]["source"], {"type": "tool", "id": "lane-a"})
        self.assertNotIn("secret-token-value", result.stdout + result.stderr)

    def test_a_non_loopback_runtime_is_refused(self):
        result = self.run_tool("--url", "http://example.com:8793")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("loopback", result.stderr)


if __name__ == "__main__":
    unittest.main()
