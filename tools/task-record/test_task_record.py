"""Observer for task_record.py (#439 review): run with `python -m unittest tools/task-record/test_task_record.py`.

A fake Runtime on loopback answers like the real one: a key already committed to a different body
is GHE003_IDEMPOTENCY_CONFLICT, the same key with the same body replays its first answer.
"""
import http.server
import io
import json
import sys
import tempfile
import threading
import unittest
from contextlib import redirect_stdout
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import task_record  # noqa: E402

HEAD_A = "a" * 40
HEAD_B = "b" * 40


class FakeRuntime(http.server.BaseHTTPRequestHandler):
    keys = {}
    seen = []

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        key = self.headers["Idempotency-Key"]
        FakeRuntime.seen.append((key, body))
        identity = json.dumps(body, sort_keys=True)  # a retry carries a new emittedAt: a different body
        if key in FakeRuntime.keys and FakeRuntime.keys[key] != identity:
            status, reply = 409, {"ok": False, "diagnostics": [{"code": "GHE003_IDEMPOTENCY_CONFLICT", "severity": "error",
                                                                  "path": "/idempotencyKey", "message": "already committed"}]}
        else:
            FakeRuntime.keys[key] = identity
            status, reply = 200, {"ok": True, "diagnostics": []}
        data = json.dumps(reply).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *args):
        pass


class TaskRecordTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), FakeRuntime)
        threading.Thread(target=cls.server.serve_forever, daemon=True).start()
        cls.url = f"http://127.0.0.1:{cls.server.server_address[1]}"
        cls.tmp = tempfile.TemporaryDirectory()
        cls.token = Path(cls.tmp.name) / "events.agent.token"
        cls.token.write_text("agent-token\n", encoding="utf-8")

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.tmp.cleanup()

    def setUp(self):
        FakeRuntime.keys.clear()
        FakeRuntime.seen.clear()

    def run_step(self, *argv, url=None):
        out = io.StringIO()
        with redirect_stdout(out):
            code = task_record.main(["--url", url or self.url, "--token-file", str(self.token), *argv])
        return code, out.getvalue()

    def test_the_fix_loop_records_a_second_pr_opened_and_verdict_on_the_new_head(self):
        steps = [
            ("--lane", "lane-a", "pr_opened", "--issue", "9", "--pr", "19", "--head", HEAD_A),
            ("--lane", "lane-b", "review_verdict", "--issue", "9", "--pr", "19", "--head", HEAD_A,
             "--verdict", "BLOCK", "--comment-url", "https://github.com/o/r/pull/19#c1"),
            ("--lane", "lane-a", "pr_opened", "--issue", "9", "--pr", "19", "--head", HEAD_B),
            ("--lane", "lane-b", "review_verdict", "--issue", "9", "--pr", "19", "--head", HEAD_B,
             "--verdict", "APPROVE", "--comment-url", "https://github.com/o/r/pull/19#c2"),
        ]
        for step in steps:
            code, out = self.run_step(*step)
            self.assertEqual(code, 0, out)
            self.assertTrue(out.startswith("recorded "), out)
        self.assertEqual(len({key for key, _ in FakeRuntime.seen}), 4)

    def test_a_retry_of_a_recorded_step_reads_as_already_recorded(self):
        step = ("--lane", "lane-a", "claimed", "--issue", "9", "--branch", "issue-9-x")
        self.assertEqual(self.run_step(*step)[0], 0)
        # The first call landed seconds earlier: its body carried another emittedAt.
        for key in FakeRuntime.keys:
            FakeRuntime.keys[key] = "an earlier body"
        code, out = self.run_step(*step)
        self.assertEqual(code, 0, out)
        self.assertTrue(out.startswith("already recorded "), out)
        self.assertNotIn("REFUSED", out)

    def test_a_second_reviewer_on_the_same_head_is_a_new_record(self):
        # D: a reassignment on an unchanged head must be recorded, not read as a retry.
        for reviewer in ("lane-b", "lane-c"):
            code, out = self.run_step("--lane", "lane-a", "review_assigned", "--issue", "9", "--pr", "19",
                                      "--head", HEAD_A, "--reviewer", reviewer)
            self.assertEqual((code, out.split()[0]), (0, "recorded"), out)

    def test_a_changed_verdict_on_the_same_head_is_a_new_record(self):
        # E: BLOCK, a body-only fix, then APPROVE by the same reviewer on the same head.
        for verdict, comment in (("BLOCK", "c1"), ("APPROVE", "c2")):
            code, out = self.run_step("--lane", "lane-b", "review_verdict", "--issue", "9", "--pr", "19",
                                      "--head", HEAD_A, "--verdict", verdict,
                                      "--comment-url", f"https://github.com/o/r/pull/19#{comment}")
            self.assertEqual((code, out.split()[0]), (0, "recorded"), out)

    def test_a_non_loopback_url_is_refused_before_the_token_is_read_or_sent(self):
        with self.assertRaises(SystemExit) as refused:
            self.run_step("--lane", "lane-a", "claimed", "--issue", "9", "--branch", "b", url="http://example.com:8793")
        self.assertIn("loopback", str(refused.exception.code))
        self.assertEqual(FakeRuntime.seen, [])


if __name__ == "__main__":
    unittest.main()
