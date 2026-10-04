import json
import os
import threading
import unittest
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
import tempfile
from unittest import mock

import keel_record


class Handler(BaseHTTPRequestHandler):
    seen: list = []

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        Handler.seen.append((self.path, body))
        reply = json.dumps({"ok": True, "data": {}}).encode()
        self.send_response(200)
        self.send_header("Content-Length", str(len(reply)))
        self.end_headers()
        self.wfile.write(reply)

    def log_message(self, *_):
        pass


class RecordBlockTest(unittest.TestCase):
    def test_outside_an_execution_nothing_is_sent(self):
        with mock.patch.dict(os.environ, {"GRAPHHELM_EXECUTION_ID": ""}):
            self.assertFalse(keel_record.record_block("stop", "no card", {}))

    def test_inside_an_execution_a_keel_blocked_signal_is_posted_on_the_node(self):
        server = HTTPServer(("127.0.0.1", 0), Handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        with tempfile.TemporaryDirectory() as tmp:
            token = Path(tmp) / "t"
            token.write_text("tok")
            env = {"GRAPHHELM_EXECUTION_ID": "demo", "GRAPHHELM_TOKEN_FILE": str(token),
                   "GRAPHHELM_RUNTIME_URL": f"http://127.0.0.1:{server.server_port}",
                   "GRAPHHELM_NODE_ID": "implementation"}
            with mock.patch.dict(os.environ, env):
                self.assertTrue(keel_record.record_block("pretool", "no card", {"session_id": "s1"}))
        server.shutdown()
        path, body = Handler.seen[-1]
        self.assertEqual(path, "/v1/executions/demo/signal")
        self.assertEqual(body["signal"]["type"], "keel.blocked")
        self.assertEqual(body["signal"]["source"], {"type": "node", "id": "implementation"})

    def test_an_unreachable_runtime_never_raises(self):
        with tempfile.TemporaryDirectory() as tmp:
            token = Path(tmp) / "t"
            token.write_text("tok")
            env = {"GRAPHHELM_EXECUTION_ID": "demo", "GRAPHHELM_TOKEN_FILE": str(token),
                   "GRAPHHELM_RUNTIME_URL": "http://127.0.0.1:9"}
            with mock.patch.dict(os.environ, env):
                self.assertFalse(keel_record.record_block("stop", "no card", {}))


if __name__ == "__main__":
    unittest.main()


class LockFailsClosedTest(unittest.TestCase):
    """A broken recorder must never turn a lock's refusal into a silent pass."""

    def _run(self, hook, payload):
        import subprocess, sys
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            git = lambda *a: subprocess.run(["git", "-C", tmp, "-c", "user.email=a@b", "-c", "user.name=a", *a], check=True, capture_output=True)
            git("init", "-q", "-b", "main"); git("commit", "-q", "--allow-empty", "-m", "base"); git("checkout", "-q", "-b", "feat")
            (repo / "src").mkdir(); (repo / "src" / "a.py").write_text("\n".join(f"x={i}" for i in range(40)))
            shadow = repo / "shadow"; shadow.mkdir()
            (shadow / "keel_record.py").write_text("raise ImportError('broken recorder')\n")
            here = Path(__file__).resolve().parent
            env = {**os.environ, "PYTHONPATH": str(shadow), "GRAPHHELM_EXECUTION_ID": ""}
            # Copy the hook beside the broken recorder so the script's own dir resolves it first.
            for name in ("keel_stop_hook.py", "keel_pretool_hook.py"):
                (shadow / name).write_text((here / name).read_text())
            payload = {**payload, "cwd": tmp}
            return subprocess.run([sys.executable, str(shadow / hook)], input=json.dumps(payload),
                                  capture_output=True, text=True, env=env, cwd=tmp)

    def test_stop_still_blocks_when_the_recorder_cannot_import(self):
        result = self._run("keel_stop_hook.py", {"stop_hook_active": False})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["decision"], "block")

    def test_pretool_still_denies_when_the_recorder_cannot_import(self):
        result = self._run("keel_pretool_hook.py", {"tool_name": "Write", "tool_input": {"file_path": "src/b.py", "content": "y\n" * 10}})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["hookSpecificOutput"]["permissionDecision"], "deny")
