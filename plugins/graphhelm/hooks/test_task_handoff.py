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
    post_mode = "normal"

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
        if self.post_mode == "refuse":
            self.send_error(403)
            return
        if self.post_mode == "drop_without_record":
            self.close_connection = True
            return
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
        if self.post_mode == "corrupt_then_drop":
            self.evidence[f"evidence-{signal_id}"] = (content + "tampered", digest)
        if self.post_mode in {"drop", "corrupt_then_drop"}:
            self.close_connection = True
            return
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
        RuntimeHandler.post_mode = "normal"
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

    def test_lost_http_ack_is_reconciled_from_sealed_records_without_post_retry(self):
        RuntimeHandler.post_mode = "drop"
        with patch.dict(os.environ, self.env, clear=False):
            offered = MODULE.offer("claude", "sender", "codex", "receiver")
            received = MODULE.receive("codex", "receiver", offered["offerId"])
            replay = MODULE.receive("codex", "receiver", offered["offerId"])
            report = MODULE.status("codex", "receiver", offered["offerId"])
        self.assertEqual(offered["state"], "recorded")
        self.assertEqual(received["state"], "recorded")
        self.assertEqual(offered["writeAcknowledgement"], "unobserved")
        self.assertEqual(received["writeAcknowledgement"], "unobserved")
        self.assertFalse(received["accepted"])
        self.assertEqual(replay["state"], "already_recorded")
        self.assertEqual(report["offers"][0]["receipts"], [received["receiptId"]])
        posts = [item for item in RuntimeHandler.requests if item[0] == "POST"]
        self.assertEqual(len(posts), 2)
        self.assertEqual([post[2]["X-Graphhelm-Actor-Session"] for post in posts], ["sender", "receiver"])

    def test_timeout_after_real_write_reconciles_offer_and_receive(self):
        original = MODULE.request

        def lost_ack(url, token, method, *args, **kwargs):
            response = original(url, token, method, *args, **kwargs)
            if method == "POST":
                raise TimeoutError("lost acknowledgement")
            return response

        with patch.dict(os.environ, self.env, clear=False), patch.object(MODULE, "request", side_effect=lost_ack):
            offered = MODULE.offer("claude", "sender", "codex", "receiver")
            received = MODULE.receive("codex", "receiver", offered["offerId"])
        self.assertEqual(received["writeAcknowledgement"], "unobserved")
        self.assertEqual([event["kind"]["data"]["kind"] for event in RuntimeHandler.events],
                         [MODULE.OFFER_KIND, MODULE.RECEIPT_KIND])
        self.assertEqual(len([item for item in RuntimeHandler.requests if item[0] == "POST"]), 2)

    def test_lost_ack_without_exact_valid_record_remains_unobserved(self):
        for mode in ("drop_without_record", "corrupt_then_drop"):
            with self.subTest(mode=mode), patch.dict(os.environ, self.env, clear=False):
                RuntimeHandler.post_mode = mode
                with self.assertRaises(ValueError):
                    MODULE.offer("claude", "sender", "codex", "receiver", mode)
                self.assertEqual(len([item for item in RuntimeHandler.requests if item[0] == "POST"]),
                                 1 if mode == "drop_without_record" else 2)
        for mode in ("drop_without_record", "corrupt_then_drop"):
            with self.subTest(receive_mode=mode), patch.dict(os.environ, self.env, clear=False):
                RuntimeHandler.post_mode = "normal"
                offered = MODULE.offer("claude", "sender", "codex", "receiver", "receive-" + mode)
                before = len([item for item in RuntimeHandler.requests if item[0] == "POST"])
                RuntimeHandler.post_mode = mode
                with self.assertRaises(ValueError):
                    MODULE.receive("codex", "receiver", offered["offerId"])
                self.assertEqual(len([item for item in RuntimeHandler.requests if item[0] == "POST"]), before + 1)

    def test_definite_http_refusal_is_not_reconciled_into_success(self):
        RuntimeHandler.post_mode = "refuse"
        with patch.dict(os.environ, self.env, clear=False), self.assertRaises(MODULE.urllib.error.HTTPError):
            MODULE.offer("claude", "sender", "codex", "receiver")
        self.assertEqual(RuntimeHandler.events, [])
        self.assertEqual([item[0] for item in RuntimeHandler.requests], ["GET", "GET", "POST"])

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

    def test_codex_metadata_is_per_call_and_missing_metadata_has_no_runtime_io(self):
        env = os.environ.copy(); env.update(self.env)
        env["GRAPHHELM_MCP_HOST"] = "codex"
        env["GRAPHHELM_MCP_SESSION_SOURCE"] = "codex_metadata"
        env.pop("GRAPHHELM_SESSION_ID", None)
        missing = {"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                   "params": {"name": "status", "arguments": {}}}
        missing_result = subprocess.run([sys.executable, str(SCRIPT), "--mcp-stdio"], env=env,
                                        input=json.dumps(missing) + "\n",
                                        capture_output=True, text=True)
        self.assertEqual(missing_result.returncode, 0, missing_result.stderr)
        self.assertTrue(json.loads(missing_result.stdout)["result"]["isError"])
        self.assertEqual(RuntimeHandler.requests, [])
        calls = [
            {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}},
            {"jsonrpc": "2.0", "id": 2, "method": "tools/list"},
            {"jsonrpc": "2.0", "id": 3, "method": "tools/call",
             "params": {"name": "status", "arguments": {}}},
            {"jsonrpc": "2.0", "id": 4, "method": "tools/call",
             "params": {"name": "offer", "arguments": {"recipientHost": "claude",
                         "recipientSessionId": "receiver-a"},
                        "_meta": {"threadId": "thread-a", "sessionId": "thread-a"}}},
            {"jsonrpc": "2.0", "id": 5, "method": "tools/call",
             "params": {"name": "offer", "arguments": {"recipientHost": "claude",
                         "recipientSessionId": "receiver-b"},
                        "_meta": {"threadId": "thread-b", "sessionId": "thread-b"}}},
            {"jsonrpc": "2.0", "id": 6, "method": "tools/call",
             "params": {"name": "status", "arguments": {}}},
        ]
        result = subprocess.run([sys.executable, str(SCRIPT), "--mcp-stdio"], env=env,
                                input="\n".join(json.dumps(call) for call in calls) + "\n",
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        replies = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertTrue(replies[2]["result"]["isError"])
        self.assertTrue(replies[-1]["result"]["isError"])
        posts = [item for item in RuntimeHandler.requests if item[0] == "POST"]
        self.assertEqual([post[2]["X-Graphhelm-Actor-Session"] for post in posts],
                         ["thread-a", "thread-b"])
        self.assertNotEqual(posts[0][3]["signal"]["id"], posts[1][3]["signal"]["id"])

    def test_codex_metadata_conflict_and_pin_fail_before_runtime_io(self):
        env = os.environ.copy(); env.update(self.env)
        env["GRAPHHELM_MCP_HOST"] = "codex"
        env["GRAPHHELM_MCP_SESSION_SOURCE"] = "codex_metadata"
        env["GRAPHHELM_SESSION_ID"] = "pinned-session"
        calls = [
            {"jsonrpc": "2.0", "id": 1, "method": "tools/call",
             "params": {"name": "status", "arguments": {},
                        "_meta": {"threadId": "one", "sessionId": "two"}}},
            {"jsonrpc": "2.0", "id": 2, "method": "tools/call",
             "params": {"name": "status", "arguments": {},
                        "_meta": {"threadId": "other", "sessionId": "other"}}},
        ]
        result = subprocess.run([sys.executable, str(SCRIPT), "--mcp-stdio"], env=env,
                                input="\n".join(json.dumps(call) for call in calls) + "\n",
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        replies = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertTrue(all(reply.get("result", {}).get("isError") or "error" in reply
                            for reply in replies))
        self.assertEqual(RuntimeHandler.requests, [])

    def test_codex_metadata_server_can_list_unbound(self):
        env = os.environ.copy()
        env.update({"GRAPHHELM_MCP_HOST": "codex", "GRAPHHELM_MCP_SESSION_SOURCE": "codex_metadata"})
        for key in ("GRAPHHELM_SESSION_ID", "GRAPHHELM_EXECUTION_ID", "GRAPHHELM_TOKEN_FILE"):
            env.pop(key, None)
        payload = "\n".join([
            json.dumps({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
            json.dumps({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        ]) + "\n"
        result = subprocess.run([sys.executable, str(SCRIPT), "--mcp-stdio"], env=env,
                                input=payload, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        replies = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertIn("tools", replies[1]["result"])
        self.assertEqual(RuntimeHandler.requests, [])

    def test_metadata_receive_records_only_the_current_recipient_and_replays(self):
        with patch.dict(os.environ, self.env, clear=False):
            a = MODULE.offer("claude", "sender", "codex", "receiver-a")
            b = MODULE.offer("claude", "sender", "codex", "receiver-b")
        RuntimeHandler.requests = []
        env = os.environ.copy(); env.update(self.env)
        env.update(GRAPHHELM_MCP_HOST="codex", GRAPHHELM_MCP_SESSION_SOURCE="codex_metadata")
        env.pop("GRAPHHELM_SESSION_ID", None)
        pairs = [("receiver-a", a), ("receiver-b", a), ("receiver-b", b), ("receiver-a", a)]
        calls = [{"jsonrpc": "2.0", "id": index, "method": "tools/call", "params": {
            "name": "receive", "arguments": {"offerId": offered["offerId"]},
            "_meta": {"threadId": session, "sessionId": session}}}
            for index, (session, offered) in enumerate(pairs, 1)]
        result = subprocess.run([sys.executable, str(SCRIPT), "--mcp-stdio"], env=env,
                                input="\n".join(json.dumps(call) for call in calls) + "\n",
                                capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        replies = [json.loads(line)["result"] for line in result.stdout.splitlines()]
        self.assertTrue(replies[1]["isError"])
        successful = [json.loads(reply["content"][0]["text"]) for reply in
                      [replies[0], replies[2], replies[3]]]
        self.assertEqual([value["state"] for value in successful],
                         ["recorded", "recorded", "already_recorded"])
        self.assertEqual(successful[0]["receiptId"], successful[2]["receiptId"])
        self.assertTrue(all(value["accepted"] is False
                            and value["activation"] == "unobserved" for value in successful))
        posts = [item for item in RuntimeHandler.requests if item[0] == "POST"]
        self.assertEqual([post[2]["X-Graphhelm-Actor-Session"] for post in posts],
                         ["receiver-a", "receiver-b"])
        self.assertEqual([post[3]["signal"]["replyTo"] for post in posts],
                         [a["offerId"], b["offerId"]])

    def test_metadata_invalid_ids_and_argument_overrides_do_not_contact_runtime(self):
        env = os.environ.copy(); env.update(self.env)
        env.update(GRAPHHELM_MCP_HOST="codex", GRAPHHELM_MCP_SESSION_SOURCE="codex_metadata")
        env.pop("GRAPHHELM_SESSION_ID", None)
        metadata_values = [{}, {"threadId": "a"}, {"threadId": "a", "sessionId": 1},
                           {"threadId": "bad/id", "sessionId": "bad/id"}]
        calls = [{"jsonrpc": "2.0", "id": i, "method": "tools/call", "params": {
            "name": "status", "arguments": {}, "_meta": metadata}}
            for i, metadata in enumerate(metadata_values, 1)]
        for field in ("sessionId", "host", "executionId", "tokenFile", "runtimeUrl", "_meta"):
            calls.append({"jsonrpc": "2.0", "id": len(calls)+1, "method": "tools/call", "params": {
                "name": "status", "arguments": {field: "override"},
                "_meta": {"threadId": "a", "sessionId": "a"}}})
        result = subprocess.run([sys.executable, str(SCRIPT), "--mcp-stdio"], env=env,
                                input="\n".join(json.dumps(call) for call in calls) + "\n",
                                capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        replies = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertEqual(len(replies), len(calls))
        self.assertTrue(all(reply["result"]["isError"] for reply in replies))
        self.assertEqual(RuntimeHandler.requests, [])

    def test_metadata_mode_rejects_wrong_host_and_legacy_still_requires_binding(self):
        for host, source, unbound in [("claude", "codex_metadata", False),
                                      ("codex", "unsupported", False),
                                      ("codex", "environment", True)]:
            with self.subTest(host=host, source=source):
                env = os.environ.copy(); env.update(self.env)
                env.update(GRAPHHELM_MCP_HOST=host, GRAPHHELM_MCP_SESSION_SOURCE=source,
                           GRAPHHELM_SESSION_ID="receiver")
                if unbound:
                    env.pop("GRAPHHELM_EXECUTION_ID", None)
                result = subprocess.run([sys.executable, str(SCRIPT), "--mcp-stdio"], env=env,
                                        input='{"jsonrpc":"2.0","id":1,"method":"initialize"}\n',
                                        capture_output=True, text=True, timeout=10)
                self.assertEqual(result.returncode, 1)
                self.assertEqual(result.stdout, "")
                self.assertEqual(RuntimeHandler.requests, [])

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
