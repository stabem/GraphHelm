"""Observer for task_record.py (#439 review): run with `python -m unittest tools/task-record/test_task_record.py`.

A fake Runtime on loopback answers like the real one: a key already committed to a different body
is GHE003_IDEMPOTENCY_CONFLICT, the same key with the same body replays its first answer.
"""
import hashlib
import http.server
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch
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
        if len(key) > 64:
            status, reply = 400, {"ok": False, "diagnostics": [{"code": "GHCLI001_ARGUMENT_INVALID", "severity": "error",
                                                                  "path": "/idempotencyKey", "message": "Idempotency-Key must be at most 64 characters"}]}
        elif FakeRuntime.old and ("title" in described or "summary" in described):
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
    def test_claim_transports_reported_assigner(self):
        # Wire gap: no existing test carries assignment provenance. Local HTTP only.
        code, out = self.run_step("--lane", "lane-a", "claimed", "--issue", "86",
                                  "--branch", "issue-86-tree", "--assigned-by", "coord")
        self.assertEqual(code, 0, out)
        self.assertEqual(json.loads(FakeRuntime.seen[-1][1]["signal"]["description"])["assignedBy"], "coord")

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

    def test_long_lane_keys_fit_the_runtime_without_losing_retry_identity(self):
        # #623: long lane/review keys were refused. Cost: bounded local HTTP calls, no real Runtime.
        keys = set()
        cases = [
            ("gh-claude-orquestrador", "623", "review_assigned", "lane-b"),
            ("gh-claude-orquestrador", "623", "review_assigned", "lane-c"),
            ("gh-claude-orquestrador", "624", "review_assigned", "lane-b"),
            ("gh-claude-orquestrador", "625", "pr_opened", "lane-b"),
            ("lane-" + "x" * 100, "623", "review_assigned", "lane-b"),
            ("lane-" + "x" * 100 + "y", "623", "review_assigned", "lane-b"),
            ("x" * 21, "623", "review_assigned", "lane-b"),  # legacy key exactly 64
            ("lane-a", "623", "review_assigned", "lane-b"),
        ]
        for lane, issue, kind, reviewer in cases:
            with self.subTest(lane=lane, issue=issue, kind=kind, reviewer=reviewer):
                step = ("--lane", lane, kind, "--issue", issue, "--pr", "19",
                        "--head", HEAD_A, "--reviewer", reviewer, "--no-github")
                start = len(FakeRuntime.seen)
                code, out = self.run_step(*step)
                self.assertEqual(code, 0, out)
                sent = FakeRuntime.seen[start:]
                for key, body in sent:
                    self.assertLessEqual(len(key), 64)
                    self.assertNotIn(key, keys)
                    keys.add(key)
                    self.assertEqual(key, body["signal"]["id"])
                    doc = json.loads(body["signal"]["description"])
                    record_kind = body["signal"]["type"].removeprefix("task.")
                    content = json.dumps({k: v for k, v in doc.items() if k != "at"}, sort_keys=True, separators=(",", ":"))
                    digest = hashlib.sha256("\n".join((lane, record_kind, content)).encode()).hexdigest()[:16]
                    legacy = f"{lane}-issue-{issue}-{record_kind}-{digest}"
                    if len(legacy) <= 64:
                        self.assertEqual(key, legacy)
                    FakeRuntime.keys[key] = "an earlier body"
                code, out = self.run_step(*step)
                self.assertEqual(code, 0, out)
                self.assertEqual(out.count("already recorded "), len(sent), out)
                self.assertEqual([key for key, _ in FakeRuntime.seen[start + len(sent):]],
                                 [key for key, _ in sent])

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

    def journeys_dir(self):
        folder = Path(self.tmp.name) / "journeys"
        folder.mkdir(exist_ok=True)
        for stem in ("studio-graph-tab", "studio-connect"):
            (folder / f"{stem}.journey.yaml").write_text(f"id: {stem}\n", encoding="utf-8")
        return str(folder)

    def test_a_claim_names_its_journey(self):
        # #577: the Studio links the task to its journey from the claim on.
        code, out = self.run_step("--lane", "lane-a", "claimed", "--issue", "9", "--branch", "b", "--no-github",
                                  "--journeys", "studio-graph-tab", "--journeys-dir", self.journeys_dir())
        self.assertEqual(code, 0, out)
        described = json.loads(FakeRuntime.seen[0][1]["signal"]["description"])
        self.assertEqual(described["journeys"], ["studio-graph-tab"])

    def test_a_claim_without_journeys_carries_no_key(self):
        code, out = self.run_step("--lane", "lane-a", "claimed", "--issue", "9", "--branch", "b", "--no-github")
        self.assertEqual(code, 0, out)
        self.assertNotIn("journeys", json.loads(FakeRuntime.seen[0][1]["signal"]["description"]))

    def test_an_unknown_journey_is_refused_before_anything_is_sent(self):
        with self.assertRaises(SystemExit) as refused:
            self.run_step("--lane", "lane-a", "claimed", "--issue", "9", "--branch", "b", "--no-github",
                          "--journeys", "studio-nope", "--journeys-dir", self.journeys_dir())
        message = str(refused.exception.code)
        self.assertIn("studio-nope", message)
        self.assertIn("studio-connect, studio-graph-tab", message)
        self.assertEqual(FakeRuntime.seen, [])

    def test_pr_opened_still_carries_its_journeys(self):
        code, out = self.run_step("--lane", "lane-a", "pr_opened", "--issue", "9", "--pr", "19", "--head", HEAD_A,
                                  "--reviewer", "lane-b", "--no-github", "--journeys", "studio-connect",
                                  "--journeys-dir", self.journeys_dir())
        self.assertEqual(code, 0, out)
        self.assertEqual(json.loads(FakeRuntime.seen[0][1]["signal"]["description"])["journeys"], ["studio-connect"])

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

    def critic(self, *extra):
        args = task_record.parse(["--lane", "gh-claude-3", "critic_verdict", "--issue", "467",
                                  "--design-ref", "docs/specs/critic-loop.md", "--reason", "names the promise", *extra])
        return task_record.document(args, "2026-10-08T00:00:00Z")

    def test_a_critic_verdict_follows_from_the_score_and_the_round(self):
        # #467: the Runtime refuses a verdict that disagrees with its score and round; the recipe
        # derives it, so the last round below the pass score is `exhausted`, never a pass.
        self.assertEqual(self.critic("--round", "2", "--score", "9")["verdict"], "pass")
        self.assertEqual(self.critic("--round", "1", "--score", "7")["verdict"], "revise")
        self.assertEqual(self.critic("--round", "3", "--score", "7")["verdict"], "exhausted")
        self.assertEqual(self.critic("--round", "2", "--score", "6", "--pass-score", "6", "--max-rounds", "2")["verdict"], "pass")
        doc = self.critic("--round", "1", "--score", "0", "--reason", "no proof named")
        self.assertEqual((doc["lane"], doc["passScore"], doc["maxRounds"], doc["reasons"]),
                         ("gh-claude-3", 8, 3, ["names the promise", "no proof named"]))
        self.assertEqual(doc["verdict"], "revise")  # a score of 0 is a score, not a missing argument

    def test_a_critic_verdict_without_a_reason_is_stopped_by_the_recipe(self):
        # #547 review: an omitted --reason is an empty list, which the recipe let through, so the
        # lane met the Runtime's refusal instead of the recipe's own line naming the argument.
        args = task_record.parse(["--lane", "gh-claude-3", "critic_verdict", "--issue", "467",
                                  "--round", "1", "--score", "5", "--design-ref", "d.md"])
        with self.assertRaises(SystemExit) as stopped:
            task_record.document(args, "2026-10-08T00:00:00Z")
        self.assertIn("--reason", str(stopped.exception))

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

class Planned(unittest.TestCase):
    """#480: `planned` records the lane's keel plan so the Studio lights Plan (and Critic for a
    `design` plan). Its fields come from the plan itself, never retyped, and the older steps keep
    their default revisions (their records' keys)."""

    PLAN = {"schema": "graphhelm-task-plan-v1", "taskId": "issue-480", "classes": ["user_visible", "code"],
            "invariantClasses": [], "journeys": [], "proof": "both", "reviews": 1, "skills": [], "tools": [],
            "delegation": {}, "path": ["card", "design"], "decidedBy": "rules", "jev": None,
            "critic": {"mode": "design", "passScore": 8, "maxRounds": 3}}

    def planned(self, *extra):
        args = task_record.parse(["--lane", "gh-claude-1", "planned", "--issue", "480", *extra])
        return task_record.document(args, "2026-10-08T00:00:00Z")

    def plan_file(self, plan):
        handle = tempfile.NamedTemporaryFile("w", suffix=".json", delete=False, encoding="utf-8")
        with handle:
            json.dump({"ok": True, "command": "keel", "data": {"plan": plan}}, handle)
        self.addCleanup(Path(handle.name).unlink)
        return handle.name

    def test_the_fields_are_copied_from_a_keel_plan_reply(self):
        doc = self.planned("--plan-file", self.plan_file(self.PLAN), "--summary", "Plan node")
        self.assertEqual({k: doc[k] for k in ("lane", "classes", "reviews", "proof", "critic", "summary")},
                         {"lane": "gh-claude-1", "classes": ["user_visible", "code"], "reviews": 1,
                          "proof": "both", "critic": {"mode": "design", "passScore": 8, "maxRounds": 3},
                          "summary": "Plan node"})

    def test_a_plan_recorded_before_the_critic_asks_for_none(self):
        plan = {k: v for k, v in self.PLAN.items() if k != "critic"}
        doc = self.planned("--plan-file", self.plan_file(plan), "--summary", "old plan")
        self.assertEqual(doc["critic"], {"mode": "none", "passScore": 8, "maxRounds": 3})

    def test_explicit_fields_stand_in_when_there_is_no_plan(self):
        doc = self.planned("--classes", "docs", "docs", "--reviews", "1", "--proof", "none",
                           "--critic-mode", "none", "--summary", "docs only")
        self.assertEqual(doc["classes"], ["docs"])
        self.assertEqual(doc["critic"]["mode"], "none")

    def offline(self):
        """A repository git cannot read and a graphhelm that does not exist: no network, no build."""
        empty = tempfile.mkdtemp()
        self.addCleanup(lambda: __import__("shutil").rmtree(empty, ignore_errors=True))
        return ["--plan-repo", empty, "--graphhelm", str(Path(empty) / "no-graphhelm.exe")]

    def test_summary_alone_records_the_stated_default_plan(self):
        # #602: lanes skipped `planned` when it demanded every field by hand. With nothing to plan
        # from (no paths, git cannot answer), it records the stated default and says so on stderr.
        err = io.StringIO()
        with redirect_stderr(err):
            doc = self.planned("--summary", "just claimed", *self.offline())
        self.assertEqual({k: doc[k] for k in ("classes", "reviews", "proof", "critic")}, task_record.DEFAULT_PLAN)
        self.assertIn("default plan", err.getvalue())

    def test_a_keel_plan_that_cannot_run_falls_back_to_the_default(self):
        err = io.StringIO()
        with redirect_stderr(err):
            doc = self.planned("--summary", "s", "--paths", "src/x.rs", *self.offline())
        self.assertEqual(doc["classes"], ["code"])
        self.assertIn("did not answer", err.getvalue())

    def test_explicit_fields_win_over_the_default(self):
        doc = self.planned("--summary", "s", "--proof", "both", "--critic-mode", "design", *self.offline())
        self.assertEqual((doc["proof"], doc["critic"]["mode"], doc["classes"]), ("both", "design", ["code"]))

    def test_a_partial_explicit_set_still_plans_the_rest_and_says_so(self):
        # #604 review: `--reviews 2` alone skipped the plan and defaulted the other fields silently.
        err = io.StringIO()
        with redirect_stderr(err):
            doc = self.planned("--summary", "s", "--reviews", "2", *self.offline())
        self.assertEqual(doc["reviews"], 2)
        self.assertIn("default plan", err.getvalue())
        doc = self.planned("--summary", "s", "--reviews", "2", "--plan-file", self.plan_file(self.PLAN))
        self.assertEqual((doc["reviews"], doc["proof"], doc["critic"]["mode"]), (2, "both", "design"))

    def test_the_paths_come_from_git_when_none_are_given(self):
        repo = tempfile.mkdtemp()
        self.addCleanup(lambda: __import__("shutil").rmtree(repo, ignore_errors=True))
        run = lambda *a: __import__("subprocess").run(["git", "-C", repo, *a], capture_output=True, check=True)
        run("init", "-q"); run("-c", "user.email=t@t", "-c", "user.name=t", "commit", "-q", "--allow-empty", "-m", "base")
        (Path(repo) / "changed.rs").write_text("x", encoding="utf-8")
        self.assertEqual(task_record.git_paths(repo), ["changed.rs"])

    def test_automatic_plan_passes_exact_git_paths_to_keel(self):
        # #702: untracked directories must expand to files and Git's quoted UTF-8 paths
        # must survive decoding. The sibling test covers rename parsing.
        # Existing coverage has only one ASCII root file. This uses a real temporary Git repo
        # and mocks only the planner process response. Cost: local Git, seconds, no network.
        repo = tempfile.mkdtemp()
        self.addCleanup(lambda: shutil.rmtree(repo, ignore_errors=True))
        run = lambda *a: subprocess.run(["git", "-C", repo, *a], capture_output=True, check=True)
        run("init", "-q")
        run("-c", "user.email=t@t", "-c", "user.name=t", "commit", "-q", "--allow-empty", "-m", "base")
        run("update-ref", "refs/remotes/origin/main", "HEAD")
        run("config", "core.quotePath", "true")

        tracked_name = "core/policy/移動.rs"
        tracked_path = Path(repo, tracked_name)
        tracked_path.parent.mkdir(parents=True)
        tracked_path.write_text("tracked", encoding="utf-8")
        run("add", tracked_name)
        run("-c", "user.email=t@t", "-c", "user.name=t", "commit", "-q", "-m", "tracked")
        tracked_path.write_text("changed", encoding="utf-8")

        nested = Path(repo, "core/policy/new/nested.rs")
        nested.parent.mkdir(parents=True)
        nested.write_text("untracked", encoding="utf-8")
        expected = ["core/policy/new/nested.rs", tracked_name]

        args = task_record.parse(["--lane", "lane-a", "planned", "--issue", "702", "--summary", "s",
                                  "--plan-repo", repo, "--graphhelm", "graphhelm"])
        plan = {"schema": "graphhelm-task-plan-v1", "classes": ["code"], "reviews": 1,
                "proof": "tests", "critic": {"mode": "none", "passScore": 8, "maxRounds": 3}}
        calls = []
        real_run = subprocess.run

        def observe(command, **options):
            if command[0] == "graphhelm":
                calls.append(command)
                return subprocess.CompletedProcess(command, 0, json.dumps({"ok": True, "data": {"plan": plan}}), "")
            return real_run(command, **options)

        with patch.object(task_record.subprocess, "run", side_effect=observe):
            task_record.keel_plan(args)

        self.assertEqual(calls[0][calls[0].index("--paths") + 1:], expected)

    def test_automatic_plan_keeps_literal_arrow_in_rename_path(self):
        # #702: Git's human rename separator can also occur in the destination filename.
        # Windows forbids `>` in real filenames, so supply Git's status output at the I/O edge.
        # This guards the planner argv; only subprocess I/O is replaced. Cost: milliseconds.
        args = task_record.parse(["--lane", "lane-a", "planned", "--issue", "702", "--summary", "s",
                                  "--plan-repo", ".", "--graphhelm", "graphhelm"])
        plan = {"schema": "graphhelm-task-plan-v1", "classes": ["code"], "reviews": 1,
                "proof": "tests", "critic": {"mode": "none", "passScore": 8, "maxRounds": 3}}
        calls = []

        def observe(command, **options):
            if command[0] == "git":
                if "diff" in command:
                    output = b""
                elif "-z" in command:
                    output = b"R  new -> label.rs\0old.rs\0"
                else:
                    output = "R  old.rs -> new -> label.rs\n"
                return subprocess.CompletedProcess(command, 0, output, b"")
            calls.append(command)
            return subprocess.CompletedProcess(command, 0, json.dumps({"ok": True, "data": {"plan": plan}}), "")

        with patch.object(task_record.subprocess, "run", side_effect=observe):
            task_record.keel_plan(args)

        self.assertEqual(calls[0][calls[0].index("--paths") + 1:], ["new -> label.rs"])

    def test_no_summary_is_refused(self):
        with self.assertRaises(SystemExit):
            self.planned("--plan-file", self.plan_file(self.PLAN))
        with self.assertRaises(SystemExit):
            self.planned("--plan-file", self.plan_file(self.PLAN), "--summary", "x" * 301)

    def test_the_older_steps_keep_their_default_revisions(self):
        for kind, extra, revision in (("claimed", ["--branch", "b"], 1),
                                      ("pr_opened", ["--pr", "1", "--head", HEAD_A, "--reviewer", "r"], 2),
                                      ("merged", ["--pr", "1", "--merge-sha", HEAD_A], 5)):
            args = task_record.parse(["--lane", "l", kind, "--issue", "9", *extra])
            self.assertEqual(task_record.document(args, "t")["revision"], revision, kind)


class MergedRequiresGithub(unittest.TestCase):
    """#674: main must not send an unconfirmed merge. Existing document/wire tests never
    observe GitHub merge state. No production seam: fake gh is a child on PATH, send is I/O.
    Cost: small local Python children, no network; retry sleeps are replaced by a clock stub.
    """

    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.folder = Path(tmp.name)
        self.calls = self.folder / "calls.json"
        script = self.folder / "fake_gh.py"
        script.write_text(
            "import json, pathlib, sys\n"
            "root = pathlib.Path(__file__).parent\n"
            "calls = root / 'calls.json'\n"
            "seen = json.loads(calls.read_text()) if calls.exists() else []\n"
            "seen.append(sys.argv[1:])\n"
            "calls.write_text(json.dumps(seen))\n"
            "replies = json.loads((root / 'replies.json').read_text())\n"
            "reply = replies[min(len(seen) - 1, len(replies) - 1)]\n"
            "print(json.dumps(reply))\n"
            "sys.exit(1 if reply == 'failed' else 0)\n", encoding="utf-8")
        if os.name == "nt":
            (self.folder / "gh.cmd").write_text(
                f'@"{sys.executable}" "{script}" %*\n', encoding="utf-8")
        else:
            executable = self.folder / "gh"
            executable.write_text(f'#!{sys.executable}\n' + script.read_text(encoding="utf-8"), encoding="utf-8")
            executable.chmod(0o755)
        self.enterContext(patch.dict(os.environ, {"PATH": str(self.folder) + os.pathsep + os.environ["PATH"]}))
        if os.name == "nt":
            # CreateProcess searches .exe, not PATHEXT. Resolve the .cmd fake from PATH
            # at the process I/O boundary; keep the actual child execution and options.
            run = task_record.subprocess.run
            def run_fake(command, **options):
                executable = shutil.which(command[0])
                self.assertEqual(Path(executable), self.folder / "gh.cmd")
                return run([executable, *command[1:]], **options)
            self.enterContext(patch.object(task_record.subprocess, "run", side_effect=run_fake))
        self.sleep = self.enterContext(patch("time.sleep"))
        self.real_send = task_record.send
        self.sent = self.enterContext(patch.object(task_record, "send", return_value=(0, "record", {})))

    def run_merge(self, replies, sha="abcdef12", *extra):
        (self.folder / "replies.json").write_text(json.dumps(replies), encoding="utf-8")
        out = io.StringIO()
        with redirect_stdout(out):
            try:
                code = task_record.main(["--lane", "test", "--issue", "674", "merged",
                                         "--pr", "42", "--merge-sha", sha, *extra])
            except SystemExit as error:
                code = error.code
        return code, out.getvalue()

    def test_unconfirmed_merges_never_send_and_confirmed_prefixes_do(self):
        merged = {"state": "MERGED", "mergeCommit": {"oid": "abcdef12" + "a" * 32}}
        cases = [
            ([{"state": "OPEN", "mergeCommit": None}], "abcdef12", (), "OPEN", 3),
            ([merged], "bbbbbbbb", (), "does not match", 1),
            (["failed"], "abcdef12", (), "could not confirm the merge", 1),
            ([merged], "abcdef12", ("--no-github",), "--no-github", 0),
            ([merged], "abcdef", (), "at least 7 hex", 0),
            ([merged], "zzzzzzz", (), "at least 7 hex", 0),
            ([merged], "ABCDEF12", (), None, 1),
            ([merged], merged["mergeCommit"]["oid"], (), None, 1),
            ([{"state": "OPEN", "mergeCommit": None}, merged], "abcdef12", (), None, 2),
            ([{"state": "MERGED", "mergeCommit": None}, merged], "abcdef12", (), None, 2),
        ]
        for replies, sha, extra, refusal, count in cases:
            with self.subTest(replies=replies, sha=sha, extra=extra):
                self.calls.unlink(missing_ok=True)
                self.sent.reset_mock()
                self.sleep.reset_mock()
                code, _ = self.run_merge(replies, sha, *extra)
                if refusal:
                    self.assertNotEqual(code, 0)
                    self.assertIn(refusal, str(code))
                    self.sent.assert_not_called()
                else:
                    self.assertEqual(code, 0)
                    self.sent.assert_called_once()
                    self.assertEqual(self.sent.call_args.args[1]["mergeSha"], sha)
                calls = json.loads(self.calls.read_text()) if self.calls.exists() else []
                self.assertEqual(calls, [["pr", "view", "42", "--json", "state,mergeCommit",
                                          "--repo", "stabem/GraphHelm"]] * count)
                self.assertEqual(self.sleep.call_args_list, [((2,),)] * max(0, count - 1))

        # The real send formats the preview, but network I/O must remain unused.
        self.calls.unlink(missing_ok=True)
        with patch.object(task_record, "send", wraps=self.real_send), \
                patch.object(task_record.urllib.request, "urlopen") as post:
            code, out = self.run_merge([], "abcdef12", "--no-github", "--dry-run")
        self.assertEqual(code, 0)
        self.assertEqual(json.loads(out)["body"]["signal"]["type"], "task.merged")
        post.assert_not_called()
        self.assertFalse(self.calls.exists())


class ClosedRequiresGithub(unittest.TestCase):
    """#747: a closure must not end a task slice while its PR is still open or merged.
    The task-record tests otherwise observe only document shape, not GitHub state. Fake gh is a
    local child and send is the I/O boundary. Cost: one child process, no network.
    """

    def test_open_pr_is_refused_without_sending_a_closed_record(self):
        with patch.object(task_record.subprocess, "run", return_value=subprocess.CompletedProcess(
                ["gh"], 0, json.dumps({"state": "OPEN", "mergedAt": None}), "")) as run, \
                patch.object(task_record, "send") as send, patch("task_record.time.sleep"):
            with self.assertRaises(SystemExit) as error:
                task_record.main(["--lane", "test", "--issue", "747", "closed", "--pr", "685",
                                  "--reason", "superseded", "--by", "740"])
        self.assertIn("state=OPEN", str(error.exception))
        self.assertEqual(run.call_args.args[0], ["gh", "pr", "view", "685", "--json", "state,mergedAt",
                                                 "--repo", "stabem/GraphHelm"])
        send.assert_not_called()

    def test_closed_unmerged_pr_sends_the_requested_close_record(self):
        with patch.object(task_record.subprocess, "run", return_value=subprocess.CompletedProcess(
                ["gh"], 0, json.dumps({"state": "CLOSED", "mergedAt": None}), "")), \
                patch.object(task_record, "send", return_value=(0, "record", {})) as send:
            self.assertEqual(task_record.main(["--lane", "test", "--issue", "747", "closed", "--pr", "685",
                                               "--reason", "superseded", "--by", "740"]), 0)
        self.assertEqual(send.call_args.args[1]["pr"], 685)
        self.assertEqual(send.call_args.args[1]["reason"], "superseded")
        self.assertEqual(send.call_args.args[1]["by"], 740)


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
