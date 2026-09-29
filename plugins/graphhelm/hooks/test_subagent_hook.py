"""Offline contract tests for provider-neutral child-agent telemetry."""

from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import unittest

SCRIPT = Path(__file__).with_name("subagent_hook.py")


class Handler(BaseHTTPRequestHandler):
    requests = []
    events = []
    ambiguous = False
    wrong_actor = False
    pause_post = False
    post_entered = threading.Event()
    duplicate_post_entered = threading.Event()
    release_post = threading.Event()

    def do_GET(self):
        self.__class__.requests.append(("GET", self.path, None))
        body = {"ok": True, "data": {"events": self.__class__.events, "head": len(self.__class__.events)}}
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(json.dumps(body).encode())

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        self.__class__.requests.append(("POST", self.path, body))
        if sum(item[0] == "POST" for item in self.__class__.requests) > 1:
            self.__class__.duplicate_post_entered.set()
        signal = body["signal"]
        actor_id = "other-agent" if self.__class__.wrong_actor else signal["source"]["id"]
        self.__class__.events.append({"sequence": len(self.__class__.events) + 1,
                                      "actor": {"type": "agent", "id": actor_id},
                                      "kind": {"type": "signal_recorded", "data": {
                                          "signalId": signal["id"], "executionId": "run-test",
                                          "sourceId": actor_id, "kind": signal["type"]}}})
        if self.__class__.pause_post:
            self.__class__.pause_post = False
            self.__class__.post_entered.set()
            self.__class__.release_post.wait(timeout=3)
        if self.__class__.ambiguous:
            self.__class__.ambiguous = False
            self.close_connection = True
            return
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(json.dumps({"ok": True, "data": {"executionId": "run-test", "signalId": signal["id"]}}).encode())

    def log_message(self, *_):
        pass


class SubagentHookTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join()

    def setUp(self):
        Handler.requests = []
        Handler.events = []
        Handler.ambiguous = False
        Handler.wrong_actor = False
        Handler.pause_post = False
        Handler.post_entered = threading.Event()
        Handler.duplicate_post_entered = threading.Event()
        Handler.release_post = threading.Event()
        self.temp = tempfile.TemporaryDirectory()
        token = Path(self.temp.name) / "token"
        token.write_text("test-token", encoding="utf-8")
        self.env = {**os.environ, "GRAPHHELM_EXECUTION_ID": "run-test",
                    "GRAPHHELM_RUNTIME_URL": f"http://127.0.0.1:{self.server.server_port}",
                    "GRAPHHELM_TOKEN_FILE": str(token),
                    "GRAPHHELM_HOOK_STATE_DIR": str(Path(self.temp.name) / "state")}
        self.env.pop("GRAPHHELM_SESSION_ID", None)

    def tearDown(self):
        self.temp.cleanup()

    def run_hook(self, phase, payload=None, host="codex"):
        value = payload or {"hook_event_name": f"Subagent{'Start' if phase == 'start' else 'Stop'}",
                            "session_id": "parent-1", "agent_id": "child-1", "agent_type": "worker"}
        return subprocess.run([sys.executable, str(SCRIPT), phase, "--host", host], input=json.dumps(value),
                              text=True, capture_output=True, env=self.env, timeout=8)

    def test_request_contains_protocol_identity_and_phase(self):
        result = self.run_hook("start")
        self.assertEqual(result.stderr, "")
        signal = Handler.requests[0][2]["signal"]
        details = json.loads(signal["description"])
        self.assertEqual(signal["type"], "agent_subagent_started")
        self.assertEqual(details, {"agentType": "worker", "childAgentId": "child-1", "declaredNodeId": None,
                                   "executionId": "run-test", "host": "codex", "parentSessionId": "parent-1",
                                   "phase": "started", "protocol": "graphhelm-subagent-v1"})
        self.assertEqual(Handler.requests[0][2]["signal"]["source"]["id"], "codex-session-parent-1")

    def test_long_parent_uses_same_bounded_actor_identity(self):
        parent = "p" * 128
        payload = {"hook_event_name": "SubagentStart", "session_id": parent,
                   "agent_id": "child-1", "agent_type": "worker"}
        result = self.run_hook("start", payload)
        self.assertEqual(result.stderr, "")
        actor = Handler.requests[0][2]["signal"]["source"]["id"]
        expected = "agent-session-" + hashlib.sha256(f"codex\0{parent}".encode()).hexdigest()[:48]
        self.assertEqual(actor, expected)

    def test_identity_and_wrong_event_fail_closed_without_http(self):
        bad = self.run_hook("start", {"hook_event_name": "SubagentStart", "session_id": "parent-1", "agent_id": "", "agent_type": "worker"})
        wrong = self.run_hook("start", {"hook_event_name": "SessionStart", "session_id": "parent-1", "agent_id": "child-1", "agent_type": "worker"})
        self.assertIn("unobserved", bad.stderr)
        self.assertIn("unobserved", wrong.stderr)
        self.assertEqual(Handler.requests, [])

    def test_duplicate_is_suppressed_with_stable_id(self):
        self.assertEqual(self.run_hook("stop").stderr, "")
        self.assertEqual(self.run_hook("stop").stderr, "")
        posts = [item for item in Handler.requests if item[0] == "POST"]
        self.assertEqual(len(posts), 1)

    def test_concurrent_duplicate_waits_for_delivery_and_posts_once(self):
        Handler.pause_post = True
        first = subprocess.Popen(
            [sys.executable, str(SCRIPT), "start", "--host", "codex"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, env=self.env,
        )
        payload = json.dumps({"hook_event_name": "SubagentStart", "session_id": "parent-1",
                              "agent_id": "child-1", "agent_type": "worker"})
        first.stdin.write(payload)
        first.stdin.close()
        self.assertTrue(Handler.post_entered.wait(timeout=1))
        second = subprocess.Popen(
            [sys.executable, str(SCRIPT), "start", "--host", "codex"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, env=self.env,
        )
        second.stdin.write(payload)
        second.stdin.close()
        self.assertFalse(Handler.duplicate_post_entered.wait(timeout=0.5))
        self.assertIsNone(second.poll(), "the duplicate callback should wait on the state lock")
        Handler.release_post.set()
        _, first_stderr = first.communicate(timeout=8)
        _, second_stderr = second.communicate(timeout=8)
        self.assertEqual(first_stderr, "")
        self.assertEqual(second_stderr, "")
        self.assertEqual(first.returncode, 0)
        self.assertEqual(second.returncode, 0)
        posts = [item for item in Handler.requests if item[0] == "POST"]
        self.assertEqual(len(posts), 1)

    def test_ambiguous_post_reconciles_recorded_signal(self):
        Handler.ambiguous = True
        result = self.run_hook("start")
        self.assertEqual(result.stderr, "")
        self.assertEqual(len([item for item in Handler.requests if item[0] == "POST"]), 1)
        self.assertEqual(len([item for item in Handler.requests if item[0] == "GET"]), 1)
        self.assertEqual(self.run_hook("start").stderr, "")
        self.assertEqual(len([item for item in Handler.requests if item[0] == "POST"]), 1)

    def test_tampered_receipt_fails_closed_without_post(self):
        self.assertEqual(self.run_hook("stop").stderr, "")
        state = next((Path(self.env["GRAPHHELM_HOOK_STATE_DIR"])).glob("*.json"))
        value = json.loads(state.read_text(encoding="utf-8"))
        value["signal"]["description"] = "{}"
        state.write_text(json.dumps(value), encoding="utf-8")
        result = self.run_hook("stop")
        self.assertIn("unobserved", result.stderr)
        self.assertEqual(len([item for item in Handler.requests if item[0] == "POST"]), 1)

    def test_reconcile_rejects_matching_id_with_wrong_actor(self):
        Handler.ambiguous = True
        Handler.wrong_actor = True
        result = self.run_hook("start")
        self.assertIn("unobserved", result.stderr)
        self.assertGreaterEqual(len([item for item in Handler.requests if item[0] == "POST"]), 1)
        self.assertEqual(len([item for item in Handler.requests if item[0] == "GET"]), 1)


if __name__ == "__main__":
    unittest.main()
