"""Issue #86: real HTTP envelope shapes for explicit run membership and mailbox receipts.

The fixture persists events and sealed evidence independently of the adapter. It needs a
local loopback listener and no provider, browser, credentials, or external network.
"""

from http.server import ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch

from test_task_handoff import RuntimeHandler
import run_team
import task_handoff


class RunTeamTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server = ThreadingHTTPServer(("127.0.0.1", 0), RuntimeHandler)
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown(); cls.server.server_close(); cls.thread.join()

    def setUp(self):
        RuntimeHandler.events = []; RuntimeHandler.evidence = {}; RuntimeHandler.requests = []
        RuntimeHandler.next_sequence = 1; RuntimeHandler.bad_pages = False
        RuntimeHandler.post_mode = "normal"
        self.temp = tempfile.TemporaryDirectory()
        token = Path(self.temp.name) / "events.token"
        token.write_text("fixture-token", encoding="utf-8")
        self.env = {"GRAPHHELM_EXECUTION_ID": "run-1", "GRAPHHELM_TOKEN_FILE": str(token),
                    "GRAPHHELM_RUNTIME_URL": f"http://127.0.0.1:{self.server.server_port}"}

    def tearDown(self):
        self.temp.cleanup()

    def test_four_distinct_sessions_share_one_run_and_only_recipient_acks(self):
        """C1/C2: catches actor collapse and a sender-side or wrong-session receipt."""
        with patch.dict(os.environ, self.env, clear=False):
            people = [run_team.join("codex", f"thread-{i}") for i in range(3)]
            people.append(run_team.join("claude", "session-4"))
            self.assertEqual(len({person["actorId"] for person in people}), 4)
            self.assertEqual({person["executionId"] for person in people}, {"run-1"})
            posts = len([row for row in RuntimeHandler.requests if row[0] == "POST"])
            self.assertEqual(run_team.join("codex", "thread-0"), people[0])
            self.assertEqual(len([row for row in RuntimeHandler.requests if row[0] == "POST"]), posts)

            work = run_team.report("codex", "thread-0", "progress-1", "Review API",
                                   "Reading the public route contract", "working")
            self.assertEqual(work["acceptance"], "unobserved")
            recipient = people[3]["actorId"]
            sent = run_team.send("codex", "thread-0", "question-1", "Can you review this?", recipient)
            self.assertEqual(sent["recipientReceipt"], "not_observed")
            before_ack = run_team.inbox("claude", "session-4")
            self.assertEqual([item["messageId"] for item in before_ack["messages"]], [sent["messageId"]])
            self.assertEqual(before_ack["receipt"], "not_recorded_by_read")
            self.assertEqual([item["messageId"] for item in run_team.inbox("claude", "session-4", 9999)["messages"]],
                             [sent["messageId"]])  # An old unacknowledged direct message survives a room cursor.
            with self.assertRaisesRegex(ValueError, "addressed session"):
                run_team.acknowledge("codex", "thread-1", sent["messageId"])
            acknowledged = run_team.acknowledge("claude", "session-4", sent["messageId"])
            self.assertEqual(acknowledged["receipt"], "recorded_by_recipient")
            self.assertEqual(run_team.inbox("claude", "session-4")["messages"], [])
            reply = run_team.send("claude", "session-4", "reply-1", "Review started.",
                                  people[0]["actorId"], sent["messageId"])
            self.assertEqual(run_team.inbox("codex", "thread-0")["messages"][0]["replyTo"],
                             sent["messageId"])
            self.assertNotEqual(reply["messageId"], sent["messageId"])
            room = run_team.send("codex", "thread-2", "room-1", "The build passed.")
            self.assertIn(room["messageId"],
                          [item["messageId"] for item in run_team.inbox("claude", "session-4")["messages"]])

    def test_wrong_run_forged_journal_actor_and_unjoined_recipient_fail_closed(self):
        """C3: catches a receipt or target accepted from another run or forged event actor."""
        with patch.dict(os.environ, self.env, clear=False):
            sender = run_team.join("codex", "sender")
            recipient = run_team.join("claude", "receiver")
            sent = run_team.send("codex", "sender", "m1", "hello", recipient["actorId"])
            with self.assertRaisesRegex(ValueError, "not attached"):
                run_team.send("codex", "sender", "m2", "hello", "codex-session-stranger")
            with self.assertRaisesRegex(ValueError, "not recorded"):
                run_team.send("claude", "receiver", "bad-reply", "hello", sender["actorId"],
                              "run-team-not-a-message")
            msg_event = next(event for event in RuntimeHandler.events
                             if event["kind"]["data"]["signalId"] == sent["messageId"])
            msg_event["actor"]["id"] = "forged-agent"
            with self.assertRaisesRegex(ValueError, "actor"):
                run_team.acknowledge("claude", "receiver", sent["messageId"])
            msg_event["actor"]["id"] = sender["actorId"]
            wrong_run = dict(self.env, GRAPHHELM_EXECUTION_ID="other-run")
            with patch.dict(os.environ, wrong_run, clear=False), self.assertRaisesRegex(ValueError, "execution"):
                run_team.inbox("codex", "sender")

    def test_lost_write_reply_requires_exact_sealed_replay_and_no_second_post(self):
        """C4: catches a timeout treated as proof or a duplicate write after lost HTTP ack."""
        RuntimeHandler.post_mode = "drop"
        with patch.dict(os.environ, self.env, clear=False):
            first = run_team.join("codex", "sender")
            second = run_team.join("codex", "sender")
            self.assertEqual(first, second)
            self.assertEqual(len([row for row in RuntimeHandler.requests if row[0] == "POST"]), 1)
            RuntimeHandler.post_mode = "drop_without_record"
            with self.assertRaisesRegex(ValueError, "unobserved"):
                run_team.join("claude", "missing")
            self.assertEqual(len(RuntimeHandler.events), 1)

    def test_inbox_does_not_treat_a_mismatched_sealed_receipt_as_delivery(self):
        """C2: catches a forged receipt with valid storage hashes suppressing a pending message."""
        with patch.dict(os.environ, self.env, clear=False):
            sender = run_team.join("codex", "sender")["actorId"]
            recipient = run_team.join("claude", "receiver")["actorId"]
            message_id = run_team.send("codex", "sender", "m1", "Please respond", recipient)["messageId"]
            execution, token, url, origin, _ = run_team._context("claude", "receiver")
            forged = run_team._signal(execution, origin, recipient, run_team.ACK, message_id,
                                      {"protocol": run_team.PROTOCOL, "executionId": execution,
                                       "messageId": message_id, "recipient": recipient,
                                       "sender": "another-agent"}, sender, message_id)
            task_handoff._post_signal(url, token, execution, forged, forged["id"], "receiver")
            with self.assertRaisesRegex(ValueError, "does not match"):
                run_team.inbox("claude", "receiver")

    def test_mcp_tool_exposes_session_bound_team_call_without_identity_arguments(self):
        """C5: catches a tool argument replacing the native session identity."""
        with patch.dict(os.environ, self.env, clear=False):
            names = {tool["name"] for tool in task_handoff._mcp_tools()}
            self.assertTrue({"team_join", "team_report", "team_send", "team_inbox", "team_acknowledge"} <= names)
            accepted = task_handoff._mcp_tool_call("team_join", {}, "codex", "native-thread")
            self.assertFalse(accepted["isError"])
            self.assertEqual(json.loads(accepted["content"][0]["text"])["actorId"],
                             "codex-session-native-thread")
            before = len(RuntimeHandler.events)
            rejected = task_handoff._mcp_tool_call("team_join", {"sessionId": "someone-else"},
                                                    "codex", "native-thread")
            self.assertTrue(rejected["isError"])
            self.assertEqual(len(RuntimeHandler.events), before)

    def test_environment_pins_claude_cli_session_before_runtime_io(self):
        """C6: catches a Claude CLI caller substituting another native session id."""
        env = dict(os.environ, **self.env, GRAPHHELM_SESSION_ID="claude-native")
        script = Path(__file__).with_name("run_team.py")
        good = subprocess.run([sys.executable, str(script), "join", "--host", "claude"],
                              env=env, capture_output=True, text=True, timeout=10)
        self.assertEqual(good.returncode, 0, good.stderr)
        self.assertEqual(json.loads(good.stdout)["actorId"], "claude-session-claude-native")
        before = len(RuntimeHandler.requests)
        wrong = subprocess.run([sys.executable, str(script), "join", "--host", "claude",
                                "--session-id", "impostor"],
                               env=env, capture_output=True, text=True, timeout=10)
        self.assertEqual(wrong.returncode, 1)
        self.assertEqual(len(RuntimeHandler.requests), before)
