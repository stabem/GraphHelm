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

# Owner rule: children run hidden in the calling terminal, never in a new console window.
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0

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
    scripted_pages = None
    post_mode = "normal"
    briefing_data = None

    def do_GET(self):
        RuntimeHandler.requests.append(("GET", self.path, dict(self.headers)))
        if self.path.endswith("/briefing"):
            body = {"ok": True, "data": self.briefing_data or {"asOfSequence": len(self.events),
                    "nextStep": {"kind": "diagnose"}, "pending": [], "unevaluated": [],
                    "decisions": [], "workDone": []}}
        elif "/evidence/" in self.path:
            evidence_id = self.path.rsplit("/", 1)[-1]
            if evidence_id not in self.evidence:
                self.send_error(404)
                return
            content, digest = self.evidence[evidence_id]
            body = {"ok": True, "data": {"evidenceId": evidence_id, "mediaType": "application/json",
                    "contentSha256": digest, "content": content}}
        elif "/events?after=" in self.path:
            after = int(self.path.split("after=", 1)[1].split("&", 1)[0])
            if self.scripted_pages:
                body = {"ok": True, "data": self.scripted_pages.pop(0)}
                self._write(body)
                return
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
        RuntimeHandler.scripted_pages = None
        RuntimeHandler.post_mode = "normal"
        RuntimeHandler.briefing_data = None
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

    def test_offer_replay_rejects_invalid_sealed_metadata_without_write(self):
        with patch.dict(os.environ, self.env, clear=False):
            offered = MODULE.offer("claude", "sender", "codex", "receiver", "alpha")
            evidence_id = f"evidence-{offered['offerId']}"
            original = json.loads(RuntimeHandler.evidence[evidence_id][0])
            original_details = json.loads(original["description"])
            self.assertEqual(MODULE.offer("claude", "sender", "codex", "receiver", "alpha")["state"],
                             "already_recorded")
            cases = {
                "kind": ("type", MODULE.RECEIPT_KIND),
                "protocol": ("protocol", "wrong-protocol"),
                "execution": ("executionId", "other-execution"),
                "offer": ("offerId", "other-offer"),
                "handoff": ("handoffId", "other-handoff"),
                "missing_briefing": ("briefing", None),
                "malformed_briefing": ("briefing", {"asOfSequence": "bad"}),
            }
            for name, (field, value) in cases.items():
                with self.subTest(name=name):
                    signal = json.loads(json.dumps(original))
                    details = json.loads(json.dumps(original_details))
                    if field == "type":
                        signal["type"] = value
                    elif field == "briefing" and value is None:
                        details.pop("briefing")
                    else:
                        details[field] = value
                    signal["description"] = json.dumps(details)
                    content = json.dumps(signal, separators=(",", ":"))
                    digest = hashlib.sha256(content.encode()).hexdigest()
                    RuntimeHandler.evidence[evidence_id] = (content, digest)
                    event = RuntimeHandler.events[0]
                    event["evidenceRefs"][0]["contentSha256"] = digest
                    event["kind"]["data"]["envelopeSha256"] = digest
                    event["kind"]["data"]["kind"] = signal["type"]
                    RuntimeHandler.requests = []
                    with self.assertRaises(ValueError):
                        MODULE.offer("claude", "sender", "codex", "receiver", "alpha")
                    self.assertFalse(any(row[0] == "POST" or "/briefing" in row[1]
                                         for row in RuntimeHandler.requests))

    def test_receipt_briefing_preserves_first_digest_when_current_briefing_advances(self):
        offered_digest = {"asOfSequence": 10, "nextStep": {"kind": "diagnose"},
                          "pending": [{"id": "offer-pending"}], "unevaluated": [],
                          "decisions": [], "workDone": [{"id": "offer-done"}]}
        first_current = {"asOfSequence": 20, "nextStep": {"kind": "diagnose"},
                         "pending": [], "unevaluated": [{"id": "current-unevaluated"}],
                         "decisions": [], "workDone": []}
        later_current = {"asOfSequence": 30, "nextStep": {"kind": "diagnose"},
                         "pending": [{"id": "later-pending"}], "unevaluated": [],
                         "decisions": [], "workDone": []}
        offered_reference = {"asOfSequence": 10, "nextStep": {"kind": "diagnose"},
                             "counts": {"pendingCount": 1, "unevaluatedCount": 0,
                                        "decisionsCount": 0, "workDoneCount": 1}}
        first_reference = {"asOfSequence": 20, "nextStep": {"kind": "diagnose"},
                           "counts": {"pendingCount": 0, "unevaluatedCount": 1,
                                      "decisionsCount": 0, "workDoneCount": 0}}
        later_reference = {"asOfSequence": 30, "nextStep": {"kind": "diagnose"},
                           "counts": {"pendingCount": 1, "unevaluatedCount": 0,
                                      "decisionsCount": 0, "workDoneCount": 0}}
        with patch.dict(os.environ, self.env, clear=False):
            RuntimeHandler.briefing_data = offered_digest
            offered = MODULE.offer("claude", "sender", "codex", "receiver")
            RuntimeHandler.briefing_data = first_current
            received = MODULE.receive("codex", "receiver", offered["offerId"])
            RuntimeHandler.briefing_data = later_current
            replay = MODULE.receive("codex", "receiver", offered["offerId"])
        self.assertEqual(received["offeredBriefing"], offered_reference)
        self.assertEqual(received["receiptBriefing"], first_reference)
        self.assertEqual(received["briefing"], first_reference)
        self.assertEqual(replay["offeredBriefing"], offered_reference)
        self.assertEqual(replay["receiptBriefing"], first_reference)
        self.assertEqual(replay["briefing"], later_reference)
        self.assertEqual(replay["state"], "already_recorded")
        self.assertEqual(replay["receiptId"], received["receiptId"])
        self.assertEqual(len([row for row in RuntimeHandler.requests if row[0] == "POST"]), 2)

    def test_receipt_briefing_rejects_invalid_sealed_digest_without_write(self):
        with patch.dict(os.environ, self.env, clear=False):
            offered = MODULE.offer("claude", "sender", "codex", "receiver")
            received = MODULE.receive("codex", "receiver", offered["offerId"])
            evidence_id = f"evidence-{received['receiptId']}"
            content, _ = RuntimeHandler.evidence[evidence_id]
            signal = json.loads(content)
            details = json.loads(signal["description"])
            details["briefing"] = {"asOfSequence": "invalid"}
            signal["description"] = json.dumps(details)
            content = json.dumps(signal, separators=(",", ":"))
            digest = hashlib.sha256(content.encode()).hexdigest()
            RuntimeHandler.evidence[evidence_id] = (content, digest)
            receipt_event = RuntimeHandler.events[-1]
            receipt_event["evidenceRefs"][0]["contentSha256"] = digest
            receipt_event["kind"]["data"]["envelopeSha256"] = digest
            posts_before = len([row for row in RuntimeHandler.requests if row[0] == "POST"])
            with self.assertRaisesRegex(ValueError, "receipt briefing"):
                MODULE.receive("codex", "receiver", offered["offerId"])
            with self.assertRaisesRegex(ValueError, "receipt briefing"):
                MODULE.status("codex", "receiver", offered["offerId"])
        self.assertEqual(len([row for row in RuntimeHandler.requests if row[0] == "POST"]), posts_before)

    def test_status_receipt_briefings_remain_readable_without_current_briefing(self):
        offered_digest = {"asOfSequence": 10, "nextStep": {"kind": "diagnose"},
                          "pending": [{"id": "offered"}], "unevaluated": [],
                          "decisions": [], "workDone": []}
        receipt_digest = {"asOfSequence": 20, "nextStep": {"kind": "diagnose"},
                          "pending": [], "unevaluated": [{"id": "received"}],
                          "decisions": [], "workDone": []}
        offered_reference = {"asOfSequence": 10, "nextStep": {"kind": "diagnose"},
                             "counts": {"pendingCount": 1, "unevaluatedCount": 0,
                                        "decisionsCount": 0, "workDoneCount": 0}}
        receipt_reference = {"asOfSequence": 20, "nextStep": {"kind": "diagnose"},
                             "counts": {"pendingCount": 0, "unevaluatedCount": 1,
                                        "decisionsCount": 0, "workDoneCount": 0}}
        with patch.dict(os.environ, self.env, clear=False):
            RuntimeHandler.briefing_data = offered_digest
            offered = MODULE.offer("claude", "sender", "codex", "receiver")
            empty = MODULE.status("codex", "receiver", offered["offerId"])
            self.assertEqual(empty["offers"][0]["receiptBriefings"], {})
            RuntimeHandler.briefing_data = receipt_digest
            received = MODULE.receive("codex", "receiver", offered["offerId"])
            RuntimeHandler.requests = []
            with patch.object(MODULE, "_briefing", side_effect=AssertionError("current briefing unavailable")):
                report = MODULE.status("codex", "receiver", offered["offerId"])
        entry = report["offers"][0]
        self.assertEqual(entry["briefingReference"], offered_reference)
        self.assertEqual(entry["receipts"], [received["receiptId"]])
        self.assertEqual(entry["receiptBriefings"], {received["receiptId"]: receipt_reference})
        self.assertFalse(any("/briefing" in row[1] or row[0] == "POST"
                             for row in RuntimeHandler.requests))

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

    def test_untargeted_status_bounds_evidence_reads(self):
        with patch.dict(os.environ, self.env, clear=False):
            base = MODULE.offer("claude", "sender", "codex", "receiver")
            received = MODULE.receive("codex", "receiver", base["offerId"])
            template_event = RuntimeHandler.events[0]
            template_signal = json.loads(RuntimeHandler.evidence[f"evidence-{base['offerId']}"][0])
            own_actor = MODULE._actor("codex", "receiver")
            other_actor = MODULE._actor("codex", "other")
            for index in range(189):
                signal = json.loads(json.dumps(template_signal))
                signal_id = f"handoff-offer-fixture-{index}"
                signal["id"] = signal_id
                signal["to"] = other_actor if index % 4 == 0 else own_actor
                details = json.loads(signal["description"])
                details["offerId"] = signal_id
                details["recipient"] = signal["to"]
                signal["description"] = json.dumps(details)
                content = json.dumps(signal, separators=(",", ":"))
                digest = hashlib.sha256(content.encode()).hexdigest()
                event = json.loads(json.dumps(template_event))
                event["sequence"] = RuntimeHandler.next_sequence
                event["evidenceRefs"] = [{"evidenceId": f"evidence-{signal_id}", "contentSha256": digest}]
                event["kind"]["data"].update(signalId=signal_id, envelopeSha256=digest)
                RuntimeHandler.events.append(event)
                RuntimeHandler.evidence[f"evidence-{signal_id}"] = (content, digest)
                RuntimeHandler.next_sequence += 1
            RuntimeHandler.requests = []
            report = MODULE.status("codex", "receiver", None)
        evidence_reads = [row[1] for row in RuntimeHandler.requests if "/evidence/" in row[1]]
        self.assertTrue(report["truncated"])
        self.assertEqual(len(report["offers"]), MODULE.MAX_OFFERS)
        self.assertEqual(report["offers"][0]["receipts"], [received["receiptId"]])
        # Recipient is sealed, not journal metadata: inspect only the prefix needed
        # to discover the 129th matching offer, plus the caller's one receipt.
        self.assertEqual(len(evidence_reads), 173)
        self.assertFalse(any("evidence-handoff-offer-fixture-171" in path for path in evidence_reads))
        self.assertFalse(any(row[0] == "POST" or "/briefing" in row[1]
                             for row in RuntimeHandler.requests))

    def test_targeted_status_avoids_unrelated_offer_evidence_and_preserves_receipts(self):
        with patch.dict(os.environ, self.env, clear=False):
            selected = MODULE.offer("claude", "sender", "codex", "receiver", "selected")
            other = MODULE.offer("claude", "sender", "codex", "receiver", "other")
            selected_receipt = MODULE.receive("codex", "receiver", selected["offerId"])
            other_receipt = MODULE.receive("codex", "receiver", other["offerId"])
            del RuntimeHandler.evidence[f"evidence-{other['offerId']}"]
            RuntimeHandler.requests = []
            report = MODULE.status("codex", "receiver", selected["offerId"])
            self.assertEqual(report["offers"][0]["offerId"], selected["offerId"])
            self.assertEqual(report["offers"][0]["receipts"], [selected_receipt["receiptId"]])
            evidence_reads = [row[1].rsplit("/", 1)[-1] for row in RuntimeHandler.requests if "/evidence/" in row[1]]
            self.assertCountEqual(evidence_reads, [f"evidence-{selected['offerId']}",
                f"evidence-{selected_receipt['receiptId']}", f"evidence-{other_receipt['receiptId']}"])
            self.assertTrue(all(row[0] == "GET" for row in RuntimeHandler.requests))
            with self.assertRaises(MODULE.urllib.error.HTTPError):
                MODULE.status("codex", "receiver", None)

    def test_status_receipt_relation_refuses_contradictory_or_missing_links(self):
        with patch.dict(os.environ, self.env, clear=False):
            selected = MODULE.offer("claude", "sender", "codex", "receiver", "selected")
            other = MODULE.offer("claude", "sender", "codex", "receiver", "other")
            receipt = MODULE.receive("codex", "receiver", selected["offerId"])
            event = next(event for event in RuntimeHandler.events
                         if event["kind"]["data"]["signalId"] == receipt["receiptId"])
            evidence_id = f"evidence-{receipt['receiptId']}"
            original, _ = RuntimeHandler.evidence[evidence_id]
            original_signal = json.loads(original)
            original_details = json.loads(original_signal["description"])
            original_digest = hashlib.sha256(original.encode()).hexdigest()
            cases = {
                "description mismatch": (other["offerId"], selected["offerId"]),
                "missing description offer": (None, selected["offerId"]),
                "reply mismatch": (selected["offerId"], other["offerId"]),
            }
            for name, (description_offer, reply_to) in cases.items():
                with self.subTest(name=name):
                    signal = json.loads(json.dumps(original_signal))
                    details = json.loads(json.dumps(original_details))
                    if description_offer is None:
                        details.pop("offerId")
                    else:
                        details["offerId"] = description_offer
                    signal["description"] = json.dumps(details)
                    signal["replyTo"] = reply_to
                    content = json.dumps(signal, separators=(",", ":"))
                    digest = hashlib.sha256(content.encode()).hexdigest()
                    RuntimeHandler.evidence[evidence_id] = (content, digest)
                    event["evidenceRefs"][0]["contentSha256"] = digest
                    event["kind"]["data"]["envelopeSha256"] = digest
                    RuntimeHandler.requests = []

                    with self.assertRaises(ValueError):
                        MODULE.status("codex", "receiver", selected["offerId"])
                    self.assertFalse(any(row[0] == "POST" or "/briefing" in row[1]
                                         for row in RuntimeHandler.requests))

            # A consistently linked receipt for a different offer stays unrelated.
            RuntimeHandler.evidence[evidence_id] = (original, original_digest)
            event["evidenceRefs"][0]["contentSha256"] = original_digest
            event["kind"]["data"]["envelopeSha256"] = original_digest
            with self.subTest(name="valid unrelated receipt"):
                report = MODULE.status("codex", "receiver", other["offerId"])
                self.assertEqual(report["offers"][0]["receipts"], [])
                self.assertFalse(any(row[0] == "POST" or "/briefing" in row[1]
                                     for row in RuntimeHandler.requests))

            # The selected receipt's historical digest remains byte-for-byte stable.
            selected_report = MODULE.status("codex", "receiver", selected["offerId"])
            self.assertEqual(selected_report["offers"][0]["receiptBriefings"][receipt["receiptId"]],
                             receipt["receiptBriefing"])

    def test_status_empty_selection_does_not_read_unrelated_receipt(self):
        with patch.dict(os.environ, self.env, clear=False):
            selected = MODULE.offer("claude", "sender", "codex", "receiver")
            received = MODULE.receive("codex", "receiver", selected["offerId"])
            del RuntimeHandler.evidence[f"evidence-{received['receiptId']}"]
            RuntimeHandler.requests = []

            report = MODULE.status("codex", "receiver", "missing-offer")

        self.assertEqual(report, {"format": "graphhelm-portable-v1", "phase": "status",
                                  "executionId": "run-1", "offers": [], "truncated": False,
                                  "activation": "unobserved"})
        self.assertTrue(RuntimeHandler.requests)
        self.assertTrue(all(row[0] == "GET" and "/events?after=" in row[1]
                            for row in RuntimeHandler.requests))

    def test_status_empty_untargeted_selection_skips_own_receipt_after_foreign_offer(self):
        with patch.dict(os.environ, self.env, clear=False):
            selected = MODULE.offer("claude", "sender", "codex", "receiver")
            received = MODULE.receive("codex", "receiver", selected["offerId"])
            offer_event = next(event for event in RuntimeHandler.events
                               if event["kind"]["data"]["signalId"] == selected["offerId"])
            evidence_id = f"evidence-{selected['offerId']}"
            signal = json.loads(RuntimeHandler.evidence[evidence_id][0])
            signal["to"] = MODULE._actor("codex", "other")
            details = json.loads(signal["description"])
            details["recipient"] = signal["to"]
            signal["description"] = json.dumps(details)
            content = json.dumps(signal, separators=(",", ":"))
            digest = hashlib.sha256(content.encode()).hexdigest()
            RuntimeHandler.evidence[evidence_id] = (content, digest)
            offer_event["evidenceRefs"][0]["contentSha256"] = digest
            offer_event["kind"]["data"]["envelopeSha256"] = digest
            RuntimeHandler.requests = []

            report = MODULE.status("codex", "receiver", None)

        self.assertEqual(report["offers"], [])
        self.assertFalse(report["truncated"])
        evidence_reads = [row[1].rsplit("/", 1)[-1] for row in RuntimeHandler.requests
                          if "/evidence/" in row[1]]
        self.assertEqual(evidence_reads, [evidence_id])
        self.assertNotIn(f"evidence-{received['receiptId']}", evidence_reads)
        self.assertFalse(any(row[0] == "POST" or "/briefing" in row[1]
                             for row in RuntimeHandler.requests))

    def test_targeted_status_still_refuses_missing_or_corrupt_selected_evidence(self):
        with patch.dict(os.environ, self.env, clear=False):
            selected = MODULE.offer("claude", "sender", "codex", "receiver")
            evidence_id = f"evidence-{selected['offerId']}"
            content, digest = RuntimeHandler.evidence[evidence_id]
            RuntimeHandler.evidence[evidence_id] = (content + "tampered", digest)
            with self.assertRaises(ValueError):
                MODULE.status("codex", "receiver", selected["offerId"])
            del RuntimeHandler.evidence[evidence_id]
            with self.assertRaises(MODULE.urllib.error.HTTPError):
                MODULE.status("codex", "receiver", selected["offerId"])

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

    def test_event_page_bounds_refuse_invalid_history_before_other_io(self):
        invalid = {
            "oversized": [{"events": [{"sequence": i} for i in range(1, 258)], "head": 257}],
            "negative_head": [{"events": [], "head": -1}],
            "head_behind_cursor": [{"events": [{"sequence": 1}], "head": 2},
                                   {"events": [], "head": 0}],
            "sequence_beyond_head": [{"events": [{"sequence": 2}], "head": 1}],
            "regressing_head": [{"events": [{"sequence": 1}], "head": 3},
                                {"events": [{"sequence": 2}], "head": 2}],
        }
        with patch.dict(os.environ, self.env, clear=False):
            for name, pages in invalid.items():
                for operation in ("offer", "receive", "status"):
                    with self.subTest(case=name, operation=operation):
                        RuntimeHandler.scripted_pages = list(pages)
                        RuntimeHandler.requests = []
                        with self.assertRaisesRegex(ValueError, "event page"):
                            if operation == "offer":
                                MODULE.offer("claude", "sender", "codex", "receiver")
                            elif operation == "receive":
                                MODULE.receive("codex", "receiver", "offer-1")
                            else:
                                MODULE.status("codex", "receiver", None)
                        self.assertEqual(len(RuntimeHandler.requests), len(pages))
                        self.assertIn("/events?after=0", RuntimeHandler.requests[0][1])
                        self.assertTrue(all(request[0] == "GET" and "/events?after=" in request[1]
                                            for request in RuntimeHandler.requests))

    def test_event_page_bounds_allow_empty_and_advancing_heads(self):
        valid = [[{"events": [], "head": 0}],
                 [{"events": [{"sequence": 1}], "head": 2},
                  {"events": [{"sequence": 2}], "head": 3},
                  {"events": [{"sequence": 3}], "head": 3}]]
        with patch.dict(os.environ, self.env, clear=False):
            for pages in valid:
                RuntimeHandler.scripted_pages = list(pages)
                RuntimeHandler.requests = []
                result = MODULE.status("codex", "receiver", None)
                self.assertEqual(result["offers"], [])
                self.assertEqual(len(RuntimeHandler.requests), len(pages))

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
                                 "--session-id", "other-session"], env=env, capture_output=True, text=True, creationflags=NO_WINDOW)
        self.assertEqual(result.returncode, 1)
        self.assertIn("unobserved", result.stderr)
        self.assertEqual(len(RuntimeHandler.requests), before)
        env["GRAPHHELM_SESSION_ID"] = "configured-session"
        result = subprocess.run([sys.executable, str(SCRIPT), "status", "--host", "codex"], env=env, capture_output=True, text=True, creationflags=NO_WINDOW)
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
                                input=payload, capture_output=True, text=True, creationflags=NO_WINDOW)
        self.assertEqual(result.returncode, 0, result.stderr)
        replies = [json.loads(line) for line in result.stdout.splitlines()]
        manifest = json.loads(SCRIPT.parent.parent.joinpath("plugin.json").read_text(encoding="utf-8"))
        self.assertEqual(replies[0]["result"]["serverInfo"]["version"], manifest["version"])
        self.assertEqual([tool["name"] for tool in replies[1]["result"]["tools"]],
                         ["offer", "receive", "status", "team_join", "team_report",
                          "team_send", "team_inbox", "team_acknowledge"])
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
                                capture_output=True, text=True, creationflags=NO_WINDOW)
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
                                        capture_output=True, text=True, creationflags=NO_WINDOW)
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
                                capture_output=True, text=True, creationflags=NO_WINDOW)
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
                                capture_output=True, text=True, creationflags=NO_WINDOW)
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
                                input=payload, capture_output=True, text=True, creationflags=NO_WINDOW)
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
                                capture_output=True, text=True, timeout=10, creationflags=NO_WINDOW)
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
                                capture_output=True, text=True, timeout=10, creationflags=NO_WINDOW)
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
                                        capture_output=True, text=True, timeout=10, creationflags=NO_WINDOW)
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
                                capture_output=True, text=True, timeout=10, creationflags=NO_WINDOW)
        self.assertEqual(len(RuntimeHandler.requests), before)
        self.assertNotEqual(result.returncode, 0)
        replies = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertEqual(len(replies), 1)
        self.assertEqual(replies[0]["error"]["code"], -32600)


if __name__ == "__main__":
    unittest.main()
