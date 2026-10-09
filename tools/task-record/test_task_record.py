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
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import task_record  # noqa: E402

HEAD_A = "a" * 40
HEAD_B = "b" * 40


class FakeRuntime(http.server.BaseHTTPRequestHandler):
    keys = {}
    seen = []
    # #498: a Runtime built before #486 refuses the opening records' title/summary keys.
    old = False

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        key = self.headers["Idempotency-Key"]
        FakeRuntime.seen.append((key, body))
        identity = json.dumps(body, sort_keys=True)  # a retry carries a new emittedAt: a different body
        described = json.loads(body["signal"]["description"])
        if FakeRuntime.old and ("title" in described or "summary" in described):
            status, reply = 400, {"ok": False, "diagnostics": [{"code": "GHCLI003_SIGNAL_INVALID", "severity": "error",
                                                                  "path": "/signal/description", "message": "one document for its kind"}]}
        elif key in FakeRuntime.keys and FakeRuntime.keys[key] != identity:
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
        FakeRuntime.old = False
        # #477: opening records read their title from GitHub; these cells are about the wire.
        self.github = task_record.github_words
        task_record.github_words = lambda *_: (None, None)

    def tearDown(self):
        task_record.github_words = self.github

    def run_step(self, *argv, url=None):
        out = io.StringIO()
        with redirect_stdout(out):
            code = task_record.main(["--url", url or self.url, "--token-file", str(self.token), *argv])
        return code, out.getvalue()

    def test_the_fix_loop_records_a_second_pr_opened_and_verdict_on_the_new_head(self):
        steps = [
            ("--lane", "lane-a", "pr_opened", "--issue", "9", "--pr", "19", "--head", HEAD_A, "--reviewer", "lane-b"),
            ("--lane", "lane-b", "review_verdict", "--issue", "9", "--pr", "19", "--head", HEAD_A,
             "--verdict", "BLOCK", "--comment-url", "https://github.com/o/r/pull/19#c1"),
            ("--lane", "lane-a", "pr_opened", "--issue", "9", "--pr", "19", "--head", HEAD_B, "--reviewer", "lane-b"),
            ("--lane", "lane-b", "review_verdict", "--issue", "9", "--pr", "19", "--head", HEAD_B,
             "--verdict", "APPROVE", "--comment-url", "https://github.com/o/r/pull/19#c2"),
        ]
        for step in steps:
            code, out = self.run_step(*step)
            self.assertEqual(code, 0, out)
            self.assertTrue(out.startswith("recorded "), out)
        # Each pr_opened also records its review_assignment (#508): 4 steps, 6 records.
        self.assertEqual(len({key for key, _ in FakeRuntime.seen}), 6)

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

    def test_an_older_runtime_still_gets_the_opening_record_and_the_lane_is_told(self):
        # #498: on gh-team's 8793 (built before #486) every claimed/pr_opened was refused and lost.
        FakeRuntime.old = True
        err = io.StringIO()
        with redirect_stderr(err):
            code, out = self.run_step("--lane", "lane-a", "claimed", "--issue", "9", "--branch", "b",
                                      "--title", "Studio: x", "--summary", "The owner gets y.")
        self.assertEqual(code, 0, out)
        self.assertTrue(out.startswith("recorded "), out)
        self.assertIn("without title/summary", err.getvalue())
        self.assertIn("may predate", err.getvalue())
        sent = [json.loads(body["signal"]["description"]) for _, body in FakeRuntime.seen]
        self.assertEqual([("title" in doc) for doc in sent], [True, False])

    def test_a_kind_without_words_is_sent_once_to_an_older_runtime(self):
        FakeRuntime.old = True
        code, out = self.run_step("--lane", "lane-a", "review_assigned", "--issue", "9", "--pr", "19",
                                  "--head", HEAD_A, "--reviewer", "lane-b")
        self.assertEqual(code, 0, out)  # no title on this kind: the old Runtime accepts it as before
        self.assertEqual(len(FakeRuntime.seen), 1)

    def test_pr_opened_records_the_review_assignment_in_the_same_call(self):
        # #508 (owner): Review showed no name because review_assigned was a separate step to forget.
        code, out = self.run_step("--lane", "lane-a", "pr_opened", "--issue", "9", "--pr", "19", "--head", HEAD_A,
                                  "--reviewer", "lane-b", "--no-github")
        self.assertEqual(code, 0, out)
        kinds = [body["signal"]["type"] for _, body in FakeRuntime.seen]
        self.assertEqual(kinds, ["task.pr_opened", "task.review_assigned"])
        assigned = json.loads(FakeRuntime.seen[1][1]["signal"]["description"])
        self.assertEqual((assigned["reviewer"], assigned["headSha"], assigned["pr"]), ("lane-b", HEAD_A, 19))
        self.assertEqual(out.count("recorded "), 2, out)

    def test_pr_opened_without_a_reviewer_is_refused_before_anything_is_sent(self):
        with self.assertRaises(SystemExit) as refused:
            self.run_step("--lane", "lane-a", "pr_opened", "--issue", "9", "--pr", "19", "--head", HEAD_A, "--no-github")
        self.assertIn("--reviewer", str(refused.exception.code))
        self.assertEqual(FakeRuntime.seen, [])

    def test_a_non_loopback_url_is_refused_before_the_token_is_read_or_sent(self):
        with self.assertRaises(SystemExit) as refused:
            self.run_step("--lane", "lane-a", "claimed", "--issue", "9", "--branch", "b", url="http://example.com:8793")
        self.assertIn("loopback", str(refused.exception.code))
        self.assertEqual(FakeRuntime.seen, [])


class ClosesAsGiven(unittest.TestCase):
    """#464: `closes` is exactly the issues the merge closed. The default `[--issue]` recorded
    "closes #356" for PR #456, a `Refs #356` merge, and `--closes` with no numbers could not say none."""

    def closes(self, *extra):
        args = task_record.parse(["--lane", "gh-claude-6", "merged", "--issue", "356", "--pr", "456",
                                  "--merge-sha", "c" * 40, *extra])
        return task_record.document(args, "2026-10-08T00:00:00Z")["closes"]

    def test_a_merge_without_closes_closes_nothing(self):
        self.assertEqual(self.closes(), [])
        self.assertEqual(self.closes("--closes"), [])

    def test_closes_lists_exactly_the_given_issues(self):
        self.assertEqual(self.closes("--closes", "356"), [356])
        self.assertEqual(self.closes("--closes", "457", "458"), [457, 458])


class TitleAndSummary(unittest.TestCase):
    """#477: an opening record carries the issue's or PR's title and its `Summary:` line for the
    Team tab. Flags win over GitHub; over the standard only warns; over what the Runtime accepts is
    clipped; nothing is added to the other kinds."""

    def setUp(self):
        self.github = task_record.github_words
        self.asked = []

        def fake(kind, number, repo):
            self.asked.append((kind, number, repo))
            return ("Studio: Team tab shows task titles", "The owner reads each task's title.")
        task_record.github_words = fake

    def tearDown(self):
        task_record.github_words = self.github

    def doc(self, *argv):
        err = io.StringIO()
        with redirect_stderr(err):
            document = task_record.document(task_record.parse(["--lane", "gh-claude-2", *argv]), "2026-10-08T00:00:00Z")
        return document, err.getvalue()

    def test_claimed_reads_the_issue_title_and_summary_from_github(self):
        document, err = self.doc("claimed", "--issue", "477", "--branch", "b")
        self.assertEqual((document["title"], document["summary"]), ("Studio: Team tab shows task titles", "The owner reads each task's title."))
        self.assertEqual(self.asked, [("claimed", 477, "stabem/GraphHelm")])
        self.assertEqual(err, "")

    def test_pr_opened_reads_the_pr_and_flags_win(self):
        document, _ = self.doc("pr_opened", "--issue", "477", "--pr", "484", "--head", "a" * 40, "--reviewer", "gh-claude-5", "--title", "fix(studio): mine")
        self.assertEqual(document["title"], "fix(studio): mine")
        self.assertEqual(document["summary"], "The owner reads each task's title.")
        self.assertEqual(self.asked, [("pr_opened", 484, "stabem/GraphHelm")])

    def test_over_the_standard_warns_and_over_the_runtime_limit_clips(self):
        document, err = self.doc("claimed", "--issue", "1", "--branch", "b", "--title", "t" * 250, "--summary", "line\nbreak")
        self.assertEqual(len(document["title"]), 200)
        self.assertEqual(document["summary"], "line break")
        self.assertIn("title is 250 characters; the standard asks for 50", err)

    def test_a_claim_names_its_parent_issue_only_when_given(self):
        # #514: a task spawned from another task's finding links back to it.
        document, _ = self.doc("claimed", "--issue", "514", "--branch", "b", "--parent", "356")
        self.assertEqual(document["parent"], 356)
        document, _ = self.doc("claimed", "--issue", "514", "--branch", "b")
        self.assertNotIn("parent", document)

    def test_no_github_and_other_kinds_add_nothing(self):
        document, _ = self.doc("claimed", "--issue", "1", "--branch", "b", "--no-github")
        self.assertNotIn("title", document)
        document, _ = self.doc("merged", "--issue", "1", "--pr", "2", "--merge-sha", "c" * 40)
        self.assertNotIn("title", document)
        self.assertEqual(self.asked, [])


class GithubWordsDecodeUtf8(unittest.TestCase):
    """#526: `gh` answers in UTF-8. Read with the platform default, a curly quote (byte 0x9d in
    UTF-8, unmapped in cp1252) killed the reader on Windows and the record was never sent. The fake
    `gh` is a real child process writing those bytes, so the decoding under test is the real one.
    On a host whose default is already UTF-8 this passes with or without the fix. Cost: one child
    Python process."""

    def test_a_pr_body_with_non_latin_characters_is_read(self):
        reply = json.dumps({"title": "feat: say \u201chello\u201d", "body": "Summary: The owner reads \u201cplain words\u201d \u2713\n"}, ensure_ascii=False)
        child = [sys.executable, "-c", "import sys; sys.stdout.buffer.write(bytes.fromhex(sys.argv[1]))", reply.encode("utf-8").hex()]
        real = task_record.subprocess.run
        task_record.subprocess.run = lambda _command, **options: real(child, **options)
        try:
            words = task_record.github_words("pr_opened", 526, "stabem/GraphHelm")
        finally:
            task_record.subprocess.run = real
        self.assertEqual(words, ("feat: say \u201chello\u201d", "The owner reads \u201cplain words\u201d \u2713"))


if __name__ == "__main__":
    unittest.main()
