"""Contract tests using the actual Runtime HTTP shapes for explicit handoff."""

from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import sys
import subprocess
import tempfile
import threading
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).with_name("task_handoff.py")
sys.path.insert(0, str(SCRIPT.parent))
SPEC = importlib.util.spec_from_file_location("task_handoff_under_test", SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MODULE)


class RuntimeHandler(BaseHTTPRequestHandler):
    events = []
    evidence = {}
    requests = []
    next_sequence = 1
    bad_pages = False

    def do_GET(self):
        RuntimeHandler.requests.append(("GET", self.path, dict(self.headers)))
        if self.path.endswith("/briefing"):
            body = {"ok": True, "data": {"asOfSequence": len(self.events), "nextStep": {"kind": "diagnose"},
                    "pending": [], "unevaluated": [], "decisions": [], "workDone": []}}
        elif "/evidence/" in self.path:
            evidence_id = self.path.rsplit("/", 1)[-1]
            content, digest = self.evidence[evidence_id]
            body = {"ok": True, "data": {"evidenceId": evidence_id, "mediaType": "application/json",
                    "contentSha256": digest, "content": content}}
        elif "/events?after=" in self.path:
            after = int(self.path.split("after=", 1)[1].split("&", 1)[0])
            page = [event for event in self.events if event["sequence"] > after]
            if self.bad_pages:
                page = [{"sequence": after}]
            body = {"ok": True, "data": {"events": page, "head": self.events[-1]["sequence"] if self.events else 0}}
        else:
            body = {"ok": False}
        self._write(body)

    def do_POST(self):
        raw = self.rfile.read(int(self.headers["Content-Length"]))
        payload = json.loads(raw)
        RuntimeHandler.requests.append(("POST", self.path, dict(self.headers), payload))
        signal = payload["signal"]
        content = json.dumps(signal, separators=(",", ":"))
        digest = hashlib.sha256(content.encode()).hexdigest()
        signal_id = signal["id"]
        if not any(event["kind"]["data"]["signalId"] == signal_id for event in self.events):
            RuntimeHandler.events.append({"sequence": RuntimeHandler.next_sequence, "actor": {"type": "agent", "id": signal["source"]["id"]},
                "scope": {"executionId": "run-1"}, "evidenceRefs": [{"evidenceId": f"evidence-{signal_id}", "contentSha256": digest}],
                "kind": {"type": "signal_recorded", "data": {"executionId": "run-1", "signalId": signal_id,
                "sourceKind": "tool", "sourceId": signal["source"]["id"], "kind": signal["type"],
                "severity": signal["severity"], "envelopeSha256": digest}}})
            self.evidence[f"evidence-{signal_id}"] = (content, digest)
            RuntimeHandler.next_sequence += 1
        self._write({"ok": True, "data": {"executionId": "run-1", "signalId": signal_id}})

    def _write(self, body):
        encoded = json.dumps(body).encode()
        self.send_response(200); self.send_header("Content-Type", "application/json"); self.end_headers()
        self.wfile.write(encoded)

    def log_message(self, *_):
        pass


class HandoffTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server = ThreadingHTTPServer(("127.0.0.1", 0), RuntimeHandler)
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True); cls.thread.start()

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown(); cls.server.server_close(); cls.thread.join()

    def setUp(self):
        RuntimeHandler.events = []; RuntimeHandler.evidence = {}; RuntimeHandler.requests = []
        RuntimeHandler.next_sequence = 1; RuntimeHandler.bad_pages = False
        self.temp = tempfile.TemporaryDirectory()
        token = Path(self.temp.name) / "events.token"; token.write_text("test-token", encoding="utf-8")
        self.env = {"GRAPHHELM_EXECUTION_ID": "run-1", "GRAPHHELM_TOKEN_FILE": str(token),
                    "GRAPHHELM_RUNTIME_URL": f"http://127.0.0.1:{self.server.server_port}"}

    def tearDown(self):
        self.temp.cleanup()

    def test_offer_receive_status_replay_journal_after_local_state_loss(self):
        with patch.dict(os.environ, self.env, clear=False):
            offered = MODULE.offer("claude", "sender-1", "codex", "receiver-1", "alpha")
            second = MODULE.offer("claude", "sender-1", "codex", "receiver-1", "beta")
            received = MODULE.receive("codex", "receiver-1", offered["offerId"])
            report = MODULE.status("codex", "receiver-1", offered["offerId"])
            repeated_offer = MODULE.offer("claude", "sender-1", "codex", "receiver-1", "alpha")
            repeated_receive = MODULE.receive("codex", "receiver-1", offered["offerId"])
        self.assertEqual(offered["state"], "recorded"); self.assertEqual(received["state"], "recorded")
        self.assertNotEqual(offered["offerId"], second["offerId"])
        self.assertEqual(report["offers"][0]["receipts"], [received["receiptId"]])
        self.assertEqual(repeated_offer["state"], "already_recorded"); self.assertEqual(repeated_receive["state"], "already_recorded")
        post = [item for item in RuntimeHandler.requests if item[0] == "POST"]
        self.assertEqual(post[0][2]["X-Graphhelm-Actor".title()], MODULE._actor("claude", "sender-1"))
        self.assertNotIn("objective", json.dumps(post))

    def test_wrong_recipient_and_non_advancing_page_are_unobserved(self):
        with patch.dict(os.environ, self.env, clear=False):
            offered = MODULE.offer("claude", "sender-1", "codex", "receiver-1")
            with self.assertRaisesRegex(ValueError, "recipient"):
                MODULE.receive("codex", "other-session", offered["offerId"])
            RuntimeHandler.bad_pages = True
            with self.assertRaisesRegex(ValueError, "did not advance"):
                MODULE.status("codex", "receiver-1", offered["offerId"])

    def test_colliding_host_session_pair_cannot_receive_the_offer(self):
        with patch.dict(os.environ, self.env, clear=False):
            offered = MODULE.offer("claude", "sender-1", "a-session-b", "c", "collision")
            before = len(RuntimeHandler.events)
            with self.assertRaisesRegex(ValueError, "recipient"):
                MODULE.receive("a", "b-session-c", offered["offerId"])
            self.assertEqual(len(RuntimeHandler.events), before)
            received = MODULE.receive("a-session-b", "c", offered["offerId"])
            self.assertEqual(received["state"], "recorded")

    def test_sealed_content_hash_and_journal_actor_are_checked(self):
        signal = {"id": "handoff-offer-1", "source": {"type": "tool", "id": "claude-session-a"}, "type": MODULE.OFFER_KIND,
                  "severity": "medium", "description": "{}", "evidence": ["run-1"], "emittedAt": "2026-09-28T00:00:00Z", "to": "codex-session-b"}
        content = json.dumps(signal, separators=(",", ":")); digest = hashlib.sha256(content.encode()).hexdigest()
        event = {"actor": {"type": "agent", "id": signal["source"]["id"]}, "scope": {"executionId": "run-1"},
                 "evidenceRefs": [{"evidenceId": "evidence-1", "contentSha256": digest}], "kind": {"type": "signal_recorded", "data": {
                 "executionId": "run-1", "signalId": signal["id"], "sourceKind": "tool", "sourceId": signal["source"]["id"], "kind": MODULE.OFFER_KIND,
                 "severity": "medium", "envelopeSha256": digest}}}
        old = MODULE.request
        try:
            MODULE.request = lambda *_args, **_kwargs: {"ok": True, "data": {"evidenceId": "evidence-1", "mediaType": "application/json", "content": content + "tampered", "contentSha256": digest}}
            with self.assertRaisesRegex(ValueError, "content hash"):
                MODULE._sealed_signal("http://runtime", "token", "run-1", event)
            event["actor"]["id"] = "other-agent"
            with self.assertRaisesRegex(ValueError, "journal actor"):
                MODULE._sealed_signal("http://runtime", "token", "run-1", event)
        finally:
            MODULE.request = old

    def test_invalid_briefing_and_receipt_address_stay_unobserved(self):
        old = MODULE.request
        try:
            MODULE.request = lambda *_args, **_kwargs: {"ok": True, "data": {
                "asOfSequence": 1, "nextStep": {"kind": "invented"}, "pending": []}}
            with self.assertRaisesRegex(ValueError, "invalid Runtime briefing"):
                MODULE._briefing("http://runtime", "token", "run-1")
        finally:
            MODULE.request = old
        receipt = {"type": MODULE.RECEIPT_KIND, "source": {"type": "tool", "id": "codex-session-b"},
                   "to": "wrong-sender", "replyTo": "offer-1", "description": json.dumps({
                   "protocol": MODULE.PROTOCOL, "executionId": "run-1", "offerId": "offer-1",
                   "recipient": "codex-session-b", "sender": "claude-session-a"})}
        with self.assertRaisesRegex(ValueError, "identity"):
            MODULE._validate_receipt(receipt, "run-1", "offer-1", "codex-session-b", "claude-session-a")

    def test_subprocess_honors_configured_session_identity(self):
        env = os.environ.copy(); env.update(self.env); env["GRAPHHELM_SESSION_ID"] = "configured-session"
        before = len(RuntimeHandler.requests)
        result = subprocess.run([sys.executable, str(SCRIPT), "status", "--host", "codex",
                                 "--session-id", "other-session"], env=env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 1)
        self.assertIn("unobserved", result.stderr)
        self.assertEqual(len(RuntimeHandler.requests), before)
        env["GRAPHHELM_SESSION_ID"] = "configured-session"
        result = subprocess.run([sys.executable, str(SCRIPT), "status", "--host", "codex"], env=env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_mcp_stdio_lists_closed_tools_and_reaches_real_status_adapter(self):
        env = os.environ.copy(); env.update(self.env)
        env["GRAPHHELM_MCP_HOST"] = "codex"
        env["GRAPHHELM_SESSION_ID"] = "receiver-1"
        payload = "\n".join([
            json.dumps({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
            json.dumps({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            json.dumps({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                        "params": {"name": "status", "arguments": {},
                                    "_meta": {"progressToken": "native-client-token"}}}),
        ]) + "\n"
        result = subprocess.run([sys.executable, str(SCRIPT), "--mcp-stdio"], env=env,
                                input=payload, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        replies = [json.loads(line) for line in result.stdout.splitlines()]
        manifest = json.loads(SCRIPT.parent.parent.joinpath("plugin.json").read_text(encoding="utf-8"))
        self.assertEqual(replies[0]["result"]["serverInfo"]["version"], manifest["version"])
        self.assertEqual([tool["name"] for tool in replies[1]["result"]["tools"]], ["offer", "receive", "status"])
        self.assertFalse(replies[2]["result"]["isError"])
        self.assertEqual(replies[2]["result"]["content"][0]["type"], "text")

    def test_mcp_rejects_identity_override_and_oversized_request_before_runtime_io(self):
        env = os.environ.copy(); env.update(self.env)
        env["GRAPHHELM_MCP_HOST"] = "codex"
        env["GRAPHHELM_SESSION_ID"] = "receiver-1"
        before = len(RuntimeHandler.requests)
        override = {"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                    "params": {"name": "status", "arguments": {"host": "claude"}}}
        oversized = b'{"jsonrpc":"2.0","id":2,"method":"ping","params":{}}' + b" " * (MODULE.MAX_MCP_MESSAGE + 1) + b"\n"
        result = subprocess.run([sys.executable, str(SCRIPT), "--mcp-stdio"], env=env,
                                input=json.dumps(override) + "\n" + oversized.decode(),
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 1, result.stderr)
        replies = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertTrue(replies[0]["result"]["isError"])
        self.assertEqual(replies[1]["error"]["code"], -32600)
        self.assertEqual(len(RuntimeHandler.requests), before)

    def test_oversized_frame_cannot_execute_a_json_suffix_as_another_request(self):
        env = os.environ.copy(); env.update(self.env)
        env["GRAPHHELM_MCP_HOST"] = "codex"
        env["GRAPHHELM_SESSION_ID"] = "receiver-1"
        suffix = json.dumps({"jsonrpc": "2.0", "id": 9, "method": "tools/call",
                             "params": {"name": "status", "arguments": {}}})
        before = len(RuntimeHandler.requests)
        result = subprocess.run([sys.executable, str(SCRIPT), "--mcp-stdio"], env=env,
                                input="x" * (MODULE.MAX_MCP_MESSAGE + 1) + suffix + "\n",
                                capture_output=True, text=True, timeout=10)
        self.assertEqual(len(RuntimeHandler.requests), before)
        self.assertNotEqual(result.returncode, 0)
        replies = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertEqual(len(replies), 1)
        self.assertEqual(replies[0]["error"]["code"], -32600)


if __name__ == "__main__":
    unittest.main()
