import json
import subprocess
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("note_reply.py")
NO_WINDOW = getattr(subprocess, "CREATE_NO_WINDOW", 0)


class NoteReplyTests(unittest.TestCase):
    def run_tool(self, *extra):
        with tempfile.TemporaryDirectory() as temp:
            token = Path(temp) / "t.token"
            token.write_text("secret-token-value", encoding="utf-8")
            return subprocess.run(
                [sys.executable, "-I", str(SCRIPT), "--lane", "lane-a", "--execution", "exec-1",
                 "--token-file", str(token), "--reply-to", "note-1", "--to", "owner", "--text", "done", *extra],
                capture_output=True, text=True, timeout=60, creationflags=NO_WINDOW)

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


class FakeRuntime(BaseHTTPRequestHandler):
    recorded = []
    store_reply = True
    conflict = False

    def _send(self, body):
        raw = json.dumps(body).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def do_POST(self):
        signal = json.loads(self.rfile.read(int(self.headers["Content-Length"])))["signal"]
        if self.conflict:
            self.send_response(409)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        if self.store_reply:
            self.recorded.append(signal)
        self._send({"ok": True, "data": {"decision": "rejected", "rejectionReason": "signal_not_actionable"}})

    def do_GET(self):
        events = [{"sequence": 7 + i, "actor": {"id": "lane-a"},
                   "kind": {"type": "signal_recorded", "data": {"signalId": s["id"], "replyTo": s["replyTo"]}}}
                  for i, s in enumerate(self.recorded)]
        self._send({"ok": True, "data": {"head": 7 + len(events), "events": events}})

    def log_message(self, *_):
        pass


class NoteReplyReadBackTests(unittest.TestCase):
    def setUp(self):
        FakeRuntime.recorded = []
        FakeRuntime.store_reply = True
        FakeRuntime.conflict = False
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), FakeRuntime)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.temp = tempfile.TemporaryDirectory()
        self.token = Path(self.temp.name) / "t.token"
        self.token.write_text("secret-token-value", encoding="utf-8")

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.temp.cleanup()

    def run_tool(self):
        return subprocess.run(
            [sys.executable, "-I", str(SCRIPT), "--lane", "lane-a", "--execution", "exec-1",
             "--url", f"http://127.0.0.1:{self.server.server_port}", "--token-file", str(self.token),
             "--reply-to", "note-1", "--text", "done"], capture_output=True, text=True, timeout=60,
            creationflags=NO_WINDOW)

    def test_the_reply_is_sent_and_read_back_with_its_sequence(self):
        result = self.run_tool()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("recorded at sequence 7 (replyTo note-1)", result.stdout)
        self.assertEqual(FakeRuntime.recorded[0]["replyTo"], "note-1")
        self.assertNotIn("secret-token-value", result.stdout + result.stderr)

    def test_a_reply_the_journal_does_not_show_fails_the_read_back(self):
        FakeRuntime.store_reply = False
        result = self.run_tool()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("not found on read-back", result.stderr)
        self.assertIn("decision=rejected reason=signal_not_actionable", result.stderr)  # what the Runtime said

    def test_a_409_with_no_journal_record_is_told_apart_from_a_read_miss(self):
        FakeRuntime.conflict = True
        result = self.run_tool()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("HTTP 409", result.stderr)
        self.assertIn("refused or lost earlier, not missed by the read", result.stderr)


if __name__ == "__main__":
    unittest.main()
