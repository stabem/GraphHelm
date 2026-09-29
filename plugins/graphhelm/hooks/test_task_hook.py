"""Offline contract tests for Claude native task lifecycle telemetry."""

from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
import unittest

SCRIPT = Path(__file__).with_name("task_hook.py")


class Handler(BaseHTTPRequestHandler):
    requests = []
    events = []
    ambiguous = False
    slow_post = False

    def do_GET(self):
        self.__class__.requests.append(("GET", self.path, None))
        body = {"ok": True, "data": {"events": self.__class__.events,
                                       "head": self.__class__.events[-1]["sequence"] if self.__class__.events else 0}}
        self._write(body)

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        self.__class__.requests.append(("POST", self.path, body))
        signal = body["signal"]
        actor_id = signal["source"]["id"]
        self.__class__.events.append({"sequence": len(self.__class__.events) + 1,
                                      "actor": {"type": "agent", "id": actor_id},
                                      "kind": {"type": "signal_recorded", "data": {
                                          "signalId": signal["id"], "executionId": "run-test",
                                          "sourceId": actor_id, "kind": signal["type"]}}})
        if self.__class__.slow_post:
            self.__class__.slow_post = False
            encoded = json.dumps({"ok": True, "data": {"executionId": "run-test", "signalId": signal["id"]}}).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(encoded)))
            self.end_headers()
            try:
                for byte in encoded:
                    self.wfile.write(bytes((byte,)))
                    self.wfile.flush()
                    time.sleep(0.15)
            except OSError:
                pass
            return
        if self.__class__.ambiguous:
            self.__class__.ambiguous = False
            self.close_connection = True
            return
        self._write({"ok": True, "data": {"executionId": "run-test", "signalId": signal["id"]}})

    def _write(self, body):
        encoded = json.dumps(body).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(encoded)

    def log_message(self, *_):
        pass


class ClaudeTaskHookTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server = type("DaemonHTTPServer", (ThreadingHTTPServer,), {"daemon_threads": True})(
            ("127.0.0.1", 0), Handler
        )
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
        Handler.slow_post = False
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

    def run_hook(self, phase, payload=None):
        event = "TaskCreated" if phase == "created" else "TaskCompleted"
        value = payload or {"hook_event_name": event, "session_id": "parent-1",
                            "task_id": "task-001", "task_subject": "Map the Runtime observer",
                            "task_description": "Must not be persisted", "teammate_name": "researcher",
                            "team_name": "legacy-name-must-not-be-persisted"}
        return subprocess.run([sys.executable, str(SCRIPT), phase], input=json.dumps(value),
                              text=True, capture_output=True, env=self.env, timeout=10)

    def test_records_only_bounded_title_teammate_and_phase(self):
        result = self.run_hook("created")
        self.assertEqual(result.stderr, "")
        signal = Handler.requests[0][2]["signal"]
        details = json.loads(signal["description"])
        self.assertEqual(signal["type"], "agent_task_created")
        self.assertEqual(details, {"executionId": "run-test", "host": "claude", "nativeTaskId": "task-001",
                                   "parentSessionId": "parent-1", "phase": "created",
                                   "protocol": "graphhelm-native-task-v1", "taskSubject": "Map the Runtime observer",
                                   "teammateName": "researcher"})
        self.assertNotIn("Must not be persisted", json.dumps(signal))
        self.assertNotIn("legacy-name", json.dumps(signal))

    def test_creation_and_completion_are_separate_observations(self):
        self.assertEqual(self.run_hook("created").stderr, "")
        self.assertEqual(self.run_hook("completed").stderr, "")
        signals = [item[2]["signal"] for item in Handler.requests if item[0] == "POST"]
        self.assertEqual([signal["type"] for signal in signals], ["agent_task_created", "agent_task_completed"])
        self.assertEqual([json.loads(signal["description"])["nativeTaskId"] for signal in signals],
                         ["task-001", "task-001"])
        self.assertNotEqual(signals[0]["id"], signals[1]["id"])

    def test_duplicate_phase_is_suppressed_with_stable_signal_id(self):
        self.assertEqual(self.run_hook("created").stderr, "")
        self.assertEqual(self.run_hook("created").stderr, "")
        self.assertEqual(len([item for item in Handler.requests if item[0] == "POST"]), 1)

    def test_ambiguous_post_reconciles_the_recorded_task(self):
        Handler.ambiguous = True
        result = self.run_hook("completed")
        self.assertEqual(result.stderr, "")
        self.assertEqual(len([item for item in Handler.requests if item[0] == "POST"]), 1)
        self.assertEqual(len([item for item in Handler.requests if item[0] == "GET"]), 1)
        self.assertEqual(self.run_hook("completed").stderr, "")
        self.assertEqual(len([item for item in Handler.requests if item[0] == "POST"]), 1)

    def test_slow_ack_obeys_hook_deadline_and_retry_recovers_same_signal(self):
        Handler.slow_post = True
        started = time.monotonic()
        first = self.run_hook("completed")
        elapsed = time.monotonic() - started
        self.assertLess(elapsed, 9.5)
        self.assertIn("deadline exceeded", first.stderr)
        posts = [item[2]["signal"] for item in Handler.requests if item[0] == "POST"]
        self.assertEqual(len(posts), 1)
        receipt = next((Path(self.temp.name) / "state").glob("native-task-*.json"))
        self.assertFalse(json.loads(receipt.read_text(encoding="utf-8"))["delivered"])

        retry = self.run_hook("completed")
        self.assertEqual(retry.stderr, "")
        retried_posts = [item[2]["signal"] for item in Handler.requests if item[0] == "POST"]
        self.assertEqual(len(retried_posts), 2)
        self.assertEqual(retried_posts[0], retried_posts[1])
        self.assertTrue(json.loads(receipt.read_text(encoding="utf-8"))["delivered"])

    def test_recognizable_credentials_in_title_or_teammate_are_not_sent(self):
        for field, value in (("task_subject", "Set token: ghp_abcdefghijklmnopqrstuvwxyz"),
                             ("task_subject", "Use Bearer abcdefghijklmnop123456"),
                             ("teammate_name", "sk_live_1234567890123")):
            payload = {"hook_event_name": "TaskCreated", "session_id": "parent-1", "task_id": "task-001",
                       "task_subject": "Safe title", "teammate_name": "worker"}
            payload[field] = value
            result = self.run_hook("created", payload)
            self.assertIn("unobserved", result.stderr)
        self.assertEqual(Handler.requests, [])

    def test_bad_identity_event_or_control_text_fails_without_http(self):
        bad_id = self.run_hook("created", {"hook_event_name": "TaskCreated", "session_id": "parent-1",
                                            "task_id": "", "task_subject": "Task"})
        wrong_event = self.run_hook("created", {"hook_event_name": "TaskCompleted", "session_id": "parent-1",
                                                 "task_id": "task-001", "task_subject": "Task"})
        control = self.run_hook("created", {"hook_event_name": "TaskCreated", "session_id": "parent-1",
                                             "task_id": "task-001", "task_subject": "Task\nwith control"})
        self.assertIn("unobserved", bad_id.stderr)
        self.assertIn("unobserved", wrong_event.stderr)
        self.assertIn("unobserved", control.stderr)
        self.assertEqual(Handler.requests, [])


if __name__ == "__main__":
    unittest.main()
