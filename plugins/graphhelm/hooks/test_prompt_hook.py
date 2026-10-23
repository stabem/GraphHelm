"""Observer for prompt_hook.py (#1921): an owner prompt becomes a candidate `request_recorded`.

Run with `python -m pytest plugins/graphhelm/hooks/test_prompt_hook.py`. The Runtime is replaced by
a capture of `session_hook.request`; the hook's own process is run once to prove it never blocks.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import prompt_hook  # noqa: E402
import session_hook  # noqa: E402

NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
HOOK = Path(__file__).resolve().parent / "prompt_hook.py"


class PromptHook(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        (self.root / ".graphhelm").mkdir()
        (self.root / ".graphhelm" / "events.agent.token").write_text("shared-token")
        (self.root / ".graphhelm" / "events.agent.lane-a.token").write_text("lane-a-token")
        self.team({"recordOwnerPrompts": True})
        self.sent = []
        self.patches = [patch.object(session_hook, "_lane_actor", return_value="lane-a"),
                        patch.object(session_hook, "request", side_effect=lambda *a, **k: self.sent.append(a) or {"ok": True})]
        for item in self.patches:
            item.start()

    def tearDown(self):
        for item in self.patches:
            item.stop()
        self.tmp.cleanup()

    def team(self, extra):
        value = {"executionId": "studio-v2", "runtimeUrl": "http://127.0.0.1:8793",
                 "tokenFile": str(self.root / ".graphhelm" / "events.agent.token"), **extra}
        (self.root / ".graphhelm" / "team.json").write_text(json.dumps(value))

    def submit(self, prompt):
        return prompt_hook.record({"session_id": "s-1", "cwd": str(self.root / "sub"), "prompt": prompt,
                                   "hook_event_name": "UserPromptSubmit"})

    def test_a_prompt_becomes_a_candidate_request_with_the_lane_credential(self):
        body = self.submit("Make the Graph show every issue title.\n\nThanks")
        self.assertIsNotNone(body)
        url, token, method, sent = self.sent[0][:4]
        self.assertEqual((url, token, method), ("http://127.0.0.1:8793/v1/executions/studio-v2/signal", "lane-a-token", "POST"))
        signal = sent["signal"]
        self.assertEqual(signal["type"], "task.request_recorded")
        doc = json.loads(signal["description"])
        self.assertTrue(doc["requestId"].startswith("candidate-"))
        self.assertEqual(doc["taskId"], f"request-{doc['requestId']}")
        self.assertEqual(doc["ask"], "Make the Graph show every issue title. Thanks")
        self.assertTrue(doc["title"].startswith("candidate: "))
        self.assertEqual(doc["recordedBy"], "lane-a")
        self.assertEqual(set(doc), {"schema", "taskId", "revision", "at", "requestId", "ask", "title", "recordedBy"})

    def test_off_by_default(self):
        self.team({"recordOwnerPrompts": False})
        self.assertIsNone(self.submit("hello"))
        (self.root / ".graphhelm" / "team.json").write_text(json.dumps({"executionId": "studio-v2"}))
        self.assertIsNone(self.submit("hello"))
        self.assertEqual(self.sent, [])

    def test_secret_looking_prompts_are_skipped_and_text_is_capped(self):
        for prompt in ("use ghp_" + "a" * 36, "token=abcdef123456", "x" * 64, "-----BEGIN RSA PRIVATE KEY-----", "/clear", "  "):
            self.assertIsNone(self.submit(prompt), prompt)
        self.assertEqual(self.sent, [])
        body = self.submit("word " * 400)
        self.assertLessEqual(len(json.loads(body["signal"]["description"])["ask"]), prompt_hook.MAX_ASK)

    def test_never_the_shared_token_and_never_a_remote_runtime(self):
        (self.root / ".graphhelm" / "events.agent.lane-a.token").unlink()
        self.assertIsNone(self.submit("hello"))
        (self.root / ".graphhelm" / "events.agent.lane-a.token").write_text("lane-a-token")
        self.team({"recordOwnerPrompts": True, "runtimeUrl": "http://example.com:8793"})
        self.assertIsNone(self.submit("hello"))
        self.assertEqual(self.sent, [])

    def test_the_hook_process_never_blocks_or_prints(self):
        for stdin in (b"not json", json.dumps({"session_id": "s", "cwd": str(self.root), "prompt": "hi"}).encode()):
            done = subprocess.run([sys.executable, str(HOOK)], input=stdin, capture_output=True, timeout=30,
                                  creationflags=NO_WINDOW)
            self.assertEqual((done.returncode, done.stdout, done.stderr), (0, b"", b""))


if __name__ == "__main__":
    unittest.main()
