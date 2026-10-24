"""Observer for task_record.py (#439 review): run with `python -m unittest tools/task-record/test_task_record.py`.

A fake Runtime on loopback answers like the real one: a key already committed to a different body
is GHE003_IDEMPOTENCY_CONFLICT, the same key with the same body replays its first answer.
"""
import datetime as datetime_module
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
import urllib.parse
from types import SimpleNamespace
from unittest.mock import patch
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import task_record  # noqa: E402

# Owner rule: children run hidden in the calling terminal, never in a new console window.
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0

HEAD_A = "a" * 40
HEAD_B = "b" * 40


class FakeRuntime(http.server.BaseHTTPRequestHandler):
    keys = {}
    seen = []
    paths = []
    # #498: a Runtime built before #486 refuses the opening records' title/summary keys.
    old = False
    no_lease = False  # #1223: a Runtime built before #1194 refuses `leaseSeconds` (GHCLI003)
    held_by = None  # #1220: a Runtime that refuses a claim because another lane holds the task
    reject_heartbeat = False
    history = []
    history_head = None
    evidence = {}
    reads = []
    auth = []
    deny_reads = False

    def do_GET(self):
        FakeRuntime.reads.append(self.path)
        if FakeRuntime.deny_reads:
            # #1556: the live Runtime answers 401 to a lane credential's history read.
            self.send_error(401)
            return
        parsed = urllib.parse.urlsplit(self.path)
        if parsed.path.endswith("/events"):
            reply = {"ok": True, "data": {"head": FakeRuntime.history_head if FakeRuntime.history_head is not None else len(FakeRuntime.history),
                                              "events": FakeRuntime.history}}
        elif "/evidence/" in parsed.path:
            evidence_id = parsed.path.rsplit("/", 1)[-1]
            content = FakeRuntime.evidence.get(evidence_id)
            reply = {"ok": content is not None, "data": {"content": json.dumps(content)} if content else {}}
        else:
            self.send_error(404)
            return
        data = json.dumps(reply).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_POST(self):
        FakeRuntime.paths.append(self.path)
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        key = self.headers["Idempotency-Key"]
        FakeRuntime.seen.append((key, body))
        FakeRuntime.auth.append(self.headers.get("Authorization"))
        identity = json.dumps(body, sort_keys=True)  # a retry carries a new emittedAt: a different body
        described = json.loads(body["signal"]["description"])
        if len(key) > 64:
            status, reply = 400, {"ok": False, "diagnostics": [{"code": "GHCLI001_ARGUMENT_INVALID", "severity": "error",
                                                                  "path": "/idempotencyKey", "message": "Idempotency-Key must be at most 64 characters"}]}
        elif FakeRuntime.held_by and body["signal"]["type"] == "task.claimed":
            status, reply = 400, {"ok": False, "diagnostics": [{"code": "GHCLI003_SIGNAL_INVALID", "severity": "error",
                                                                  "path": "/signal/description/taskId",
                                                                  "message": f"issue-9 is held by lane {FakeRuntime.held_by} (claim 7)"}]}
        elif (FakeRuntime.old and ("title" in described or "summary" in described)) or \
                (FakeRuntime.no_lease and "leaseSeconds" in described):
            status, reply = 400, {"ok": False, "diagnostics": [{"code": "GHCLI003_SIGNAL_INVALID", "severity": "error",
                                                                  "path": "/signal/description", "message": "one document for its kind"}]}
        elif FakeRuntime.reject_heartbeat and body["signal"]["type"] == "task.heartbeat":
            status, reply = 400, {"ok": False, "diagnostics": [{"code": "GHCLI003_SIGNAL_INVALID", "severity": "error",
                                                                    "path": "/signal/description", "message": "heartbeat refused"}]}
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
    FIXED_NOW = datetime_module.datetime(2026, 10, 13, 12, 0, tzinfo=datetime_module.timezone.utc)

    @classmethod
    def fixed_clock(cls):
        class FixedDateTime(datetime_module.datetime):
            @classmethod
            def now(inner, tz=None):
                return cls.FIXED_NOW if tz else cls.FIXED_NOW.replace(tzinfo=None)
        return SimpleNamespace(datetime=FixedDateTime, timezone=datetime_module.timezone,
                               timedelta=datetime_module.timedelta)

    def history_event(self, kind, sequence, *, lane="lane-a", task_id="issue-1024", occurred="2026-10-10T12:00:00Z",
                      claim_sequence=None, pr=44, at="2026-10-10T12:00:00Z"):
        doc = {"schema": "graphhelm-task-event-v1", "taskId": task_id, "revision": sequence,
               "at": at, "lane": lane}
        if kind == "task.claimed":
            doc.update(issue=int(task_id.removeprefix("issue-")) if task_id.startswith("issue-") else 1,
                       branch=f"{task_id}-branch")
        elif kind == "task.pr_opened":
            doc.update(pr=pr, headSha=HEAD_A, journeys=[])
        elif kind == "task.released":
            doc.update(claimSequence=claim_sequence or 1, reason="reassigned")
        elif kind == "task.closed":
            doc.update(pr=pr, reason="abandoned", by=1)
        elif kind == "task.merged":
            doc.update(pr=pr, mergeSha=HEAD_A, closes=[], merger=lane)
        evidence_id = f"evidence-{sequence}"
        FakeRuntime.evidence[evidence_id] = {"type": kind, "source": {"id": lane},
                                             "description": json.dumps(doc)}
        return {"sequence": sequence, "occurredAt": occurred, "actor": {"id": lane},
                "kind": {"type": "signal_recorded", "data": {"kind": kind}},
                "evidenceRefs": [{"evidenceId": evidence_id}]}

    def run_heartbeat(self, *extra):
        with patch.object(task_record, "datetime", self.fixed_clock()):
            return self.run_step("--lane", "lane-a", "heartbeat", "--issue", "1024", "--no-github", *extra)

    def assert_refused_without_post(self, *extra):
        before = len(FakeRuntime.seen)
        with self.assertRaises(SystemExit):
            self.run_heartbeat(*extra)
        self.assertEqual(len(FakeRuntime.seen), before)

    def window(self, moment):
        return int(moment.timestamp()) // 300

    def test_open_claim_heartbeat_posts_the_canonical_activity_record(self):
        # #1024: an eligible heartbeat must be emitted by the real recorder. #1556: it is sent
        # without reading history (a lane credential cannot read it), the open-claim and cooldown
        # rules are the Runtime's, and the revision is the 300-second window of `at`.
        with patch.object(task_record.subprocess, "run", side_effect=AssertionError("heartbeat called GitHub")):
            code, out = self.run_heartbeat()
        self.assertEqual(code, 0, out)
        self.assertTrue(out.startswith("recorded "), out)
        signal = FakeRuntime.seen[-1][1]["signal"]
        self.assertEqual(signal["type"], "task.heartbeat")
        document = json.loads(signal["description"])
        self.assertEqual(document, {"schema": "graphhelm-task-event-v1", "taskId": "issue-1024",
                                    "revision": self.window(self.FIXED_NOW), "at": "2026-10-13T12:00:00Z",
                                    "lane": "lane-a"})
        self.assertEqual(FakeRuntime.reads, [], "a heartbeat reads no history")

    def test_a_claim_with_a_request_id_also_links_the_request(self):
        # #1921: the issue-to-ask link rides on the claim; no separate request_linked call.
        code, out = self.run_step("--lane", "lane-a", "claimed", "--issue", "1921", "--branch", "b", "--title", "t",
                                  "--request-id", "auto-record")
        self.assertEqual(code, 0, out)
        kinds = [entry[1]["signal"]["type"] for entry in FakeRuntime.seen]
        self.assertEqual(kinds, ["task.claimed", "task.request_linked"])
        linked = json.loads(FakeRuntime.seen[-1][1]["signal"]["description"])
        self.assertEqual((linked["taskId"], linked["requestId"], linked["issue"], linked["linkedBy"]),
                         ("request-auto-record", "auto-record", 1921, "lane-a"))
        claim = json.loads(FakeRuntime.seen[0][1]["signal"]["description"])
        self.assertNotIn("requestId", claim)

    def test_a_lane_credential_that_cannot_read_history_still_records_its_heartbeat(self):
        # #1556 RED on main: the live Runtime answers 401 to a lane credential's history read, so
        # the recorder exited "heartbeat history is unavailable" and every waiting lane read Silent.
        FakeRuntime.deny_reads = True
        code, out = self.run_heartbeat()
        self.assertEqual((code, out.split()[0]), (0, "recorded"), out)
        self.assertEqual(json.loads(FakeRuntime.seen[-1][1]["signal"]["description"])["lane"], "lane-a")

    def test_heartbeat_open_claim_and_cooldown_are_the_runtimes_answer(self):
        # #1147/#1556: the Runtime refuses a heartbeat without an open pre-PR claim, or within 300 s
        # of the last one, over trusted history; the recorder reports that refusal as REFUSED.
        # The Runtime side is covered by task_event_cli::heartbeat_needs_an_open_claim_and_a_pr_ends_it.
        FakeRuntime.reject_heartbeat = True
        code, out = self.run_heartbeat()
        self.assertEqual((code, out.split()[0]), (1, "REFUSED"), out)
        self.assertEqual(len(FakeRuntime.seen), 1)

    def test_heartbeat_retry_in_one_window_is_the_same_record_and_the_next_window_is_new(self):
        first = self.run_heartbeat()
        second = self.run_heartbeat()
        self.assertEqual((first[0], second[0]), (0, 0), (first, second))
        self.assertEqual(FakeRuntime.seen[0][0], FakeRuntime.seen[1][0], "a retry reuses its key")
        later = self.FIXED_NOW + datetime_module.timedelta(seconds=300)
        with patch.object(type(self), "FIXED_NOW", later):
            third = self.run_heartbeat()
        self.assertEqual(third[0], 0, third)
        self.assertNotEqual(FakeRuntime.seen[-1][0], FakeRuntime.seen[0][0])
        self.assertEqual(json.loads(FakeRuntime.seen[-1][1]["signal"]["description"])["revision"],
                         self.window(later))
        self.assertEqual(self.window(later), self.window(self.FIXED_NOW) + 1)

    def test_heartbeat_reason_is_carried_and_a_bad_one_is_refused_before_any_request(self):
        # #1556: a waiting lane says why. The reason rides in the record unchanged; an empty,
        # repeated, over-long or non-printable one is refused locally, with nothing sent.
        code, out = self.run_heartbeat("--reason", "waiting on #1670 to land")
        self.assertEqual(code, 0, out)
        document = json.loads(FakeRuntime.seen[-1][1]["signal"]["description"])
        self.assertEqual(document["reason"], "waiting on #1670 to land")
        for flags in (("--reason", " "), ("--reason", "a", "--reason", "b"), ("--reason", "x" * 501),
                      ("--reason", "bell\a")):
            with self.subTest(flags=flags):
                self.assert_refused_without_post(*flags)

    def test_forbidden_overrides_refuse_without_post_or_reads(self):
        for flags in (("--pr", "44"), ("--head", HEAD_A), ("--revision", "9"), ("--title", "forged")):
            with self.subTest(flags=flags):
                FakeRuntime.reads.clear()
                self.assert_refused_without_post(*flags)
                self.assertEqual(FakeRuntime.reads, [])

    def test_authoritative_refusal_and_dry_run_never_report_recorded(self):
        FakeRuntime.reject_heartbeat = True
        code, out = self.run_heartbeat()
        self.assertEqual(code, 1)
        self.assertTrue(out.startswith("REFUSED "), out)
        self.assertNotIn("recorded ", out)
        self.assertEqual(len(FakeRuntime.seen), 1)

        FakeRuntime.seen.clear()
        stderr = io.StringIO()
        with redirect_stderr(stderr):
            code, out = self.run_heartbeat("--dry-run")
        self.assertEqual(code, 0)
        self.assertEqual(FakeRuntime.seen, [])
        self.assertIn("dry-run only", stderr.getvalue())
        self.assertIn('"type": "task.heartbeat"', out)

    def test_journey_frames_send_the_newest_finished_runs_pngs_four_per_record(self):
        # #1839: the run picked is the newest finished one; an in-flight current run is skipped.
        project = Path(self.tmp.name) / "wt"
        base = project / ".graphhelm" / "journey-previews" / "checkout"
        run = base / "history" / "00000001792058400000"
        (run / "frames").mkdir(parents=True)
        screens = {f"s{n}": {"result": "pass", "frame": True, "file": f"s{n}.png"} for n in range(5)}
        screens["skipped"] = {"frame": False}
        (run / "state.json").write_text(json.dumps({"state": "ready", "result": "pass",
                                                    "ranAt": "2026-10-22T10:00:00Z", "screens": screens}))
        for n in range(5):
            (run / "frames" / f"s{n}.png").write_bytes(task_record.PNG_MAGIC + f"frame-{n}".encode())
        (base / "state.json").write_text(json.dumps({"state": "running", "screens": {}}))
        code, out = self.run_step("--lane", "lane-a", "journey_frames", "--issue", "9", "--pr", "90",
                                  "--head", HEAD_A, "--journey", "checkout", "--preview-project", str(project))
        self.assertEqual(code, 0, out)
        bodies = [body for _, body in FakeRuntime.seen]
        self.assertEqual([json.loads(b["signal"]["description"])["screens"] for b in bodies],
                         [["s0", "s1", "s2", "s3"], ["s4"]])
        first = json.loads(bodies[0]["signal"]["description"])
        self.assertEqual((bodies[0]["signal"]["type"], first["headSha"], first["journey"], first["result"], first["lane"]),
                         ("task.journey_frames", HEAD_A, "checkout", "pass", "lane-a"))
        self.assertEqual([a["mediaType"] for a in bodies[0]["attachments"]], ["image/png"] * 4)
        import base64
        self.assertEqual(base64.b64decode(bodies[1]["attachments"][0]["base64"]),
                         task_record.PNG_MAGIC + b"frame-4")

    def test_release_records_the_exact_claim_sequence_and_reason(self):
        code, out = self.run_step("--lane", "lane-a", "released", "--issue", "86",
                                  "--claim-sequence", "12", "--release-reason", "Work moved elsewhere")
        self.assertEqual(code, 0, out)
        signal = FakeRuntime.seen[-1][1]["signal"]
        self.assertEqual(signal["type"], "task.released")
        document = json.loads(signal["description"])
        self.assertEqual(document["claimSequence"], 12)
        self.assertEqual(document["lane"], "lane-a")
        self.assertEqual(document["reason"], "Work moved elsewhere")

    def test_lane_retired_is_signed_by_the_owner_and_needs_a_reason(self):
        owner = Path(self.tmp.name) / "events.token"
        owner.write_text("owner-token\n", encoding="utf-8")
        argv = ["--token-file", str(owner), "--execution", "gh-team", "--lane", "orchestrator", "lane_retired",
                "--lanes", "lane-r,lane-s", "--retired", "true"]
        out = io.StringIO()
        with redirect_stdout(out):
            code = task_record.main(["--url", self.url, *argv, "--reason", "fleet stopped"])
        self.assertEqual(code, 0, out.getvalue())
        self.assertEqual(FakeRuntime.auth[-1], "Bearer owner-token")
        signal = FakeRuntime.seen[-1][1]["signal"]
        self.assertEqual(signal["type"], "task.lane_retired")
        document = json.loads(signal["description"])
        self.assertEqual({k: v for k, v in document.items() if k != "at"},
                         {"schema": "graphhelm-task-event-v1", "taskId": "lanes", "revision": 1,
                          "lanes": ["lane-r", "lane-s"], "retired": True, "reason": "fleet stopped"})
        seen = len(FakeRuntime.seen)
        with self.assertRaises(SystemExit):
            task_record.main(["--url", self.url, *argv])
        with self.assertRaises(SystemExit) as agent:  # an agent token is not the owner's
            task_record.main(["--url", self.url, "--token-file", str(self.token), *argv[2:], "--reason", "x"])
        self.assertIn("owner-only", str(agent.exception.code))
        self.assertEqual(len(FakeRuntime.seen), seen, "nothing is sent")

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
        # #1148: each lane these cells record as holds its own credential beside the shared token.
        for lane in ("lane-a", "lane-b", "test", "l", "gh-claude-1", "gh-claude-2", "gh-claude-3", "gh-claude-6"):
            task_record.credential_file(cls.token, lane).write_text(f"credential-{lane}\n", encoding="utf-8")

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.tmp.cleanup()

    def setUp(self):
        FakeRuntime.keys.clear()
        FakeRuntime.seen.clear()
        FakeRuntime.old = False
        FakeRuntime.no_lease = False
        FakeRuntime.held_by = None
        FakeRuntime.reject_heartbeat = False
        FakeRuntime.history = []
        FakeRuntime.history_head = None
        FakeRuntime.deny_reads = False
        FakeRuntime.evidence = {}
        FakeRuntime.reads = []
        FakeRuntime.paths = []
        # #477: opening records read their title from GitHub; these cells are about the wire.
        self.github = task_record.github_words
        task_record.github_words = lambda *_: (None, None)

    def tearDown(self):
        task_record.github_words = self.github

    def run_step(self, *argv, url=None, execution="gh-team"):
        out = io.StringIO()
        binding = [] if execution is None else ["--execution", execution]
        with redirect_stdout(out):
            code = task_record.main(["--url", url or self.url, "--token-file", str(self.token), *binding, *argv])
        return code, out.getvalue()

    def test_team_binding_selects_execution_and_rejects_broken_binding_without_send(self):
        # Contract: an existing project binding selects its execution, while malformed bindings
        # refuse before HTTP. Regression: the old implicit gh-team default ignored project scope.
        # Existing tests observe payloads and retries but never the execution path or config refusal.
        # No production seam: this drives the real recorder into the existing loopback observer.
        project = Path(self.tmp.name) / "binding-project"
        nested = project / "src" / "deep"
        (project / ".graphhelm").mkdir(parents=True)
        nested.mkdir(parents=True)
        (project / ".graphhelm" / "team.json").write_text(
            json.dumps({"executionId": "studio-v2", "harmless": True}), encoding="utf-8")
        token = project / ".graphhelm" / "events.agent.token"
        token.write_text("agent-token\n", encoding="utf-8")
        task_record.credential_file(token, "lane-a").write_text("credential-lane-a\n", encoding="utf-8")
        old = Path.cwd()
        try:
            os.chdir(nested)
            code, output = self.run_step("--lane", "lane-a", "heartbeat", "--issue", "1024",
                                         "--no-github", "--token-file", str(token), execution=None)
        finally:
            os.chdir(old)
        self.assertEqual(code, 0, output)
        self.assertEqual(FakeRuntime.paths, ["/v1/executions/studio-v2/signal"])

        ancestor = Path(self.tmp.name) / "ancestor-binding"
        ancestor_nested = ancestor / "src" / "deep"
        (ancestor / ".graphhelm").mkdir(parents=True)
        ancestor_nested.mkdir(parents=True)
        (ancestor / ".graphhelm" / "team.json").write_text(
            json.dumps({"executionId": "ancestor-run"}), encoding="utf-8")
        remote_token = Path(self.tmp.name) / "runtime-only" / "events.agent.token"
        remote_token.parent.mkdir()
        remote_token.write_text("agent-token\n", encoding="utf-8")
        task_record.credential_file(remote_token, "lane-a").write_text("credential-lane-a\n", encoding="utf-8")
        old = Path.cwd()
        try:
            os.chdir(ancestor_nested)
            code, output = self.run_step("--lane", "lane-a", "heartbeat", "--issue", "1024",
                                         "--no-github", "--token-file", str(remote_token), execution=None)
        finally:
            os.chdir(old)
        self.assertEqual(code, 0, output)
        self.assertEqual(FakeRuntime.paths[-1], "/v1/executions/ancestor-run/signal")

        worktree = Path(self.tmp.name) / "worktree"
        worktree_nested = worktree / "src"
        (worktree / ".graphhelm").mkdir(parents=True)
        worktree_nested.mkdir(parents=True)
        (worktree / ".git").write_text("gitdir: elsewhere", encoding="utf-8")
        (worktree / ".graphhelm" / "team.json").write_text(
            json.dumps({"executionId": "wrong-local"}), encoding="utf-8")
        worktree_token = Path(self.tmp.name) / "worktree-runtime" / "events.agent.token"
        worktree_token.parent.mkdir()
        worktree_token.write_text("agent-token\n", encoding="utf-8")
        task_record.credential_file(worktree_token, "lane-a").write_text("credential-lane-a\n", encoding="utf-8")
        (worktree_token.parent / "team.json").write_text(
            json.dumps({"executionId": "token-sibling"}), encoding="utf-8")
        old = Path.cwd()
        try:
            os.chdir(worktree_nested)
            code, output = self.run_step("--lane", "lane-a", "heartbeat", "--issue", "1024",
                                         "--no-github", "--token-file", str(worktree_token), execution=None)
        finally:
            os.chdir(old)
        self.assertEqual(code, 0, output)
        self.assertEqual(FakeRuntime.paths[-1], "/v1/executions/token-sibling/signal")

        before = len(FakeRuntime.paths)
        (project / ".graphhelm" / "team.json").write_text("not json", encoding="utf-8")
        old = Path.cwd()
        try:
            os.chdir(nested)
            with self.assertRaises(SystemExit):
                self.run_step("--lane", "lane-a", "heartbeat", "--issue", "1024",
                              "--no-github", "--token-file", str(token), execution=None)
        finally:
            os.chdir(old)
        self.assertEqual(len(FakeRuntime.paths), before)

        outer = Path(self.tmp.name) / "outer-project"
        repo = outer / "nested-repo"
        (outer / ".graphhelm").mkdir(parents=True)
        (outer / ".graphhelm" / "team.json").write_text(
            json.dumps({"executionId": "wrong-parent"}), encoding="utf-8")
        repo.mkdir(parents=True)
        (repo / ".git").write_text("gitdir: elsewhere", encoding="utf-8")
        work = repo / "src"
        work.mkdir(parents=True)
        old = Path.cwd()
        try:
            os.chdir(work)
            with self.assertRaises(SystemExit):
                self.run_step("--lane", "lane-a", "heartbeat", "--issue", "1024",
                              "--no-github", execution=None)
        finally:
            os.chdir(old)
        self.assertEqual(len(FakeRuntime.paths), before)

    def test_explicit_execution_bypasses_invalid_config(self):
        project = Path(self.tmp.name) / "explicit-binding"
        (project / ".graphhelm").mkdir(parents=True)
        (project / ".graphhelm" / "team.json").write_text("[]", encoding="utf-8")
        token = project / ".graphhelm" / "events.agent.token"
        token.write_text("agent-token\n", encoding="utf-8")
        task_record.credential_file(token, "lane-a").write_text("credential-lane-a\n", encoding="utf-8")
        old = Path.cwd()
        try:
            os.chdir(project)
            code, output = self.run_step("--lane", "lane-a", "heartbeat", "--issue", "1024",
                                         "--no-github", "--token-file", str(token),
                                         "--execution", "explicit-run")
        finally:
            os.chdir(old)
        self.assertEqual(code, 0, output)
        self.assertEqual(FakeRuntime.paths, ["/v1/executions/explicit-run/signal"])

    def test_invalid_team_bindings_are_bounded_and_never_sent(self):
        # Contract: malformed, non-object, missing and oversized bindings refuse before HTTP.
        # Regression: an invalid value could become a URL path or silently select a fallback.
        # Existing coverage had no config parser or no-send observer; this uses the real loopback.
        cases = [
            ("missing", {}), ("empty", {"executionId": ""}),
            ("path", {"executionId": "bad/id", "token": "sentinel-secret"}), ("query", {"executionId": "bad?query"}),
            ("fragment", {"executionId": "bad#fragment"}), ("percent", {"executionId": "bad%value"}),
            ("long", {"executionId": "a" * 129}),
            ("array", []), ("oversized", {"executionId": "ok", "padding": "x" * 65536}),
        ]
        for name, value in cases:
            with self.subTest(name=name):
                folder = Path(self.tmp.name) / f"invalid-{name}"
                folder.mkdir(parents=True)
                token = folder / "events.agent.token"
                token.write_text("agent-token\n", encoding="utf-8")
                task_record.credential_file(token, "lane-a").write_text("credential-lane-a\n", encoding="utf-8")
                (folder / "team.json").write_text(json.dumps(value), encoding="utf-8")
                before = len(FakeRuntime.paths)
                with self.assertRaises(SystemExit) as refusal:
                    self.run_step("--lane", "lane-a", "heartbeat", "--issue", "1024",
                                  "--no-github", "--token-file", str(token), execution=None)
                self.assertEqual(len(FakeRuntime.paths), before)
                self.assertNotIn("sentinel-secret", str(refusal.exception))

        folder = Path(self.tmp.name) / "invalid-utf8"
        folder.mkdir(parents=True)
        token = folder / "events.agent.token"
        token.write_text("agent-token\n", encoding="utf-8")
        task_record.credential_file(token, "lane-a").write_text("credential-lane-a\n", encoding="utf-8")
        (folder / "team.json").write_bytes(b"{\xff")
        before = len(FakeRuntime.paths)
        with self.assertRaises(SystemExit):
            self.run_step("--lane", "lane-a", "heartbeat", "--issue", "1024",
                          "--no-github", "--token-file", str(token), execution=None)
        self.assertEqual(len(FakeRuntime.paths), before)

        valid = Path(self.tmp.name) / "valid-colon"
        valid.mkdir(parents=True)
        token = valid / "events.agent.token"
        token.write_text("agent-token\n", encoding="utf-8")
        task_record.credential_file(token, "lane-a").write_text("credential-lane-a\n", encoding="utf-8")
        (valid / "team.json").write_text(json.dumps({"executionId": "run:blue"}), encoding="utf-8")
        code, output = self.run_step("--lane", "lane-a", "heartbeat", "--issue", "1024",
                                     "--no-github", "--token-file", str(token), execution=None)
        self.assertEqual(code, 0, output)
        self.assertEqual(FakeRuntime.paths[-1], "/v1/executions/run:blue/signal")

    def test_the_fix_loop_records_a_second_pr_opened_and_verdict_on_the_new_head(self):
        steps = [
            ("--lane", "lane-a", "pr_opened", "--issue", "9", "--pr", "19", "--head", HEAD_A, "--reviewer", "lane-b"),
            ("--lane", "lane-b", "review_verdict", "--issue", "9", "--pr", "19", "--head", HEAD_A,
             "--verdict", "BLOCK", "--comment-url", "https://github.com/o/r/pull/19#c1"),
            ("--lane", "lane-a", "pr_opened", "--issue", "9", "--pr", "19", "--head", HEAD_B, "--reviewer", "lane-b"),
            ("--lane", "lane-b", "review_verdict", "--issue", "9", "--pr", "19", "--head", HEAD_B,
             "--verdict", "APPROVE", "--comment-url", "https://github.com/o/r/pull/19#c2"),
        ]
        with patch.object(task_record.subprocess, "run", return_value=subprocess.CompletedProcess(
                ["gh"], 0, json.dumps({"state": "OPEN"}), "")):
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
                                      "--head", HEAD_A, "--reviewer", reviewer, "--no-github")
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
            task_record.credential_file(self.token, lane).write_text(f"credential-{lane}\n", encoding="utf-8")
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
                                  "--head", HEAD_A, "--reviewer", "lane-b", "--no-github")
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

    # #1148: the bearer is the lane's own credential beside --token-file, never the shared token.
    # #1193: a claim carries its lease when asked; absent, the field is not sent (a Runtime older
    # than #1193 refuses unknown keys, and the schema reads absence as 1800); an out-of-range lease
    # is refused before anything is sent (on the parent the flag did not exist).
    def test_a_claim_carries_its_lease_and_an_out_of_range_lease_is_refused(self):
        # #1223: the default claim states the fold's default lease, 1800 s (on the parent it sent none).
        code, out = self.run_step("--lane", "lane-a", "claimed", "--issue", "9", "--branch", "b", "--no-github")
        self.assertEqual(code, 0, out)
        self.assertEqual(json.loads(FakeRuntime.seen[-1][1]["signal"]["description"])["leaseSeconds"], 1800)
        code, out = self.run_step("--lane", "lane-a", "claimed", "--issue", "10", "--branch", "b", "--no-github",
                                  "--lease-seconds", "600")
        self.assertEqual(code, 0, out)
        self.assertEqual(json.loads(FakeRuntime.seen[-1][1]["signal"]["description"])["leaseSeconds"], 600)
        sent = len(FakeRuntime.seen)
        with self.assertRaises(SystemExit) as refused:
            self.run_step("--lane", "lane-a", "claimed", "--issue", "11", "--branch", "b", "--no-github",
                          "--lease-seconds", "299")
        self.assertIn("--lease-seconds", str(refused.exception.code))
        self.assertEqual(len(FakeRuntime.seen), sent)

    # #1851: a claim carries the lane's kind and model when given, nothing when not, and a value
    # outside the Runtime's rule is refused before anything is sent.
    def test_a_claim_carries_the_lanes_kind_and_model_and_a_bad_kind_is_refused(self):
        code, out = self.run_step("--lane", "lane-a", "claimed", "--issue", "12", "--branch", "b", "--no-github",
                                  "--agent-kind", "codex", "--agent-model", "gpt-5.1-codex")
        self.assertEqual(code, 0, out)
        doc = json.loads(FakeRuntime.seen[-1][1]["signal"]["description"])
        self.assertEqual((doc["agentKind"], doc["agentModel"]), ("codex", "gpt-5.1-codex"))
        code, out = self.run_step("--lane", "lane-a", "claimed", "--issue", "13", "--branch", "b", "--no-github")
        self.assertEqual(code, 0, out)
        doc = json.loads(FakeRuntime.seen[-1][1]["signal"]["description"])
        self.assertNotIn("agentKind", doc)
        self.assertNotIn("agentModel", doc)
        sent = len(FakeRuntime.seen)
        with self.assertRaises(SystemExit) as refused:
            self.run_step("--lane", "lane-a", "claimed", "--issue", "14", "--branch", "b", "--no-github",
                          "--agent-kind", "Claude")
        self.assertIn("--agent-kind", str(refused.exception.code))
        self.assertEqual(len(FakeRuntime.seen), sent)

    def test_a_runtime_older_than_leases_still_records_a_default_claim_but_not_a_given_lease(self):
        # #1223: a Runtime built before #1194 refuses `leaseSeconds` (GHCLI003). A claim that carried
        # only the DEFAULT lease is sent once more without it and lands; one whose lease the lane gave
        # is not retried, so a requested lease is never silently dropped. Cost: three local POSTs.
        FakeRuntime.no_lease = True
        err = io.StringIO()
        with redirect_stderr(err):
            code, out = self.run_step("--lane", "lane-a", "claimed", "--issue", "9", "--branch", "b", "--no-github")
        self.assertEqual(code, 0, out)
        self.assertEqual(len(FakeRuntime.seen), 2)
        self.assertEqual(json.loads(FakeRuntime.seen[0][1]["signal"]["description"])["leaseSeconds"], 1800)
        self.assertNotIn("leaseSeconds", json.loads(FakeRuntime.seen[1][1]["signal"]["description"]))
        self.assertIn("claim leases (#1194)", err.getvalue())
        FakeRuntime.seen.clear()
        with redirect_stderr(io.StringIO()):
            code, out = self.run_step("--lane", "lane-a", "claimed", "--issue", "10", "--branch", "b", "--no-github",
                                      "--lease-seconds", "600")
        self.assertEqual(code, 1, out)
        self.assertEqual(len(FakeRuntime.seen), 1, "a given lease is not dropped and resent")

    def test_a_claim_refused_as_held_is_sent_once_and_names_the_holder(self):
        # #1220: the Runtime refuses a claim another lane holds at `/signal/description/taskId`. That
        # is not an old Runtime refusing an unknown key, so the default-lease resend (#1223) must not
        # fire: one POST, no old-Runtime warning, the holder's sentence reported. On the parent the
        # resend fired on any GHCLI003 (two POSTs). Cost: one local POST.
        FakeRuntime.held_by = "lane-b"
        err = io.StringIO()
        with redirect_stderr(err):
            code, out = self.run_step("--lane", "lane-a", "claimed", "--issue", "9", "--branch", "b", "--no-github")
        self.assertEqual(code, 1, out)
        self.assertEqual(len(FakeRuntime.seen), 1, "a held claim is not resent")
        self.assertNotIn("claim leases (#1194)", err.getvalue())
        self.assertIn("held by lane lane-b", out + err.getvalue())

    def test_a_record_is_sent_with_the_lanes_credential_not_the_shared_token(self):
        code, out = self.run_step("--lane", "lane-a", "claimed", "--issue", "9", "--branch", "b", "--no-github")
        self.assertEqual(code, 0, out)
        self.assertEqual(FakeRuntime.auth[-1], "Bearer credential-lane-a")
        self.assertEqual(task_record.credential_file("x/events.token", "lane-a"), Path("x/events.agent.lane-a.token"))
        self.assertEqual(task_record.credential_file("x/events.agent.token", "lane-a"), Path("x/events.agent.lane-a.token"))
        # #1459: a --token-file that already names the lane's credential is used as it is.
        self.assertEqual(task_record.credential_file("x/events.agent.lane-a.token", "lane-a"), Path("x/events.agent.lane-a.token"))

    def test_a_lane_without_a_credential_is_refused_and_told_the_mint_command(self):
        # #1459: the silent fallback to the shared token recorded rev-1444's claim under it.
        seen = len(FakeRuntime.seen)
        with self.assertRaises(SystemExit) as refused:
            self.run_step("--lane", "lane-none", "claimed", "--issue", "9", "--branch", "b", "--no-github")
        self.assertEqual(len(FakeRuntime.seen), seen, "nothing is sent")
        self.assertIn("graphhelm tasks credential --lane lane-none --execution gh-team", str(refused.exception.code))
        self.assertIn("--allow-shared-token", str(refused.exception.code))

    def test_a_lane_without_a_credential_falls_back_to_the_shared_token_only_when_allowed(self):
        # #1148 rollout: a Runtime without --require-lane-credential still accepts the shared token,
        # so with --allow-shared-token the record is sent with it and the missing file is on stderr.
        err = io.StringIO()
        with redirect_stderr(err):
            code, out = self.run_step("--lane", "lane-none", "--allow-shared-token", "claimed", "--issue", "9",
                                      "--branch", "b", "--no-github")
        self.assertEqual(code, 0, out)
        self.assertEqual(FakeRuntime.auth[-1], "Bearer agent-token")
        self.assertIn("graphhelm tasks credential --lane lane-none --execution gh-team", err.getvalue())


class TimeoutAndFullHead(unittest.TestCase):
    """#1526: a read timeout after a committed write is a retry, and a full sha is the same head."""

    def send(self, replies):
        args = SimpleNamespace(lane="l", kind="claimed", repo="r", pr=None, issue=1, comment_url=None,
                               url="http://127.0.0.1:1", execution="e", dry_run=False, token_file="t")
        calls = []

        def fake(request, timeout):
            calls.append(request.get_header("Idempotency-key"))
            reply = replies[len(calls) - 1]
            if isinstance(reply, Exception):
                raise reply
            return io.BytesIO(json.dumps(reply).encode())

        with patch.object(task_record, "read_credential", return_value="x"), \
                patch.object(task_record.urllib.request, "urlopen", side_effect=fake):
            return task_record.send(args, {"taskId": "issue-1"}, "now"), calls

    def test_timeout_then_conflict_is_already_recorded(self):
        conflict = {"ok": False, "diagnostics": [{"code": "GHE003_IDEMPOTENCY_CONFLICT", "severity": "error"}]}
        (code, _, _), calls = self.send([TimeoutError("timed out"), conflict])
        self.assertEqual(code, 2)
        self.assertEqual(len(set(calls)), 1)

    def test_three_timeouts_exit_with_a_message(self):
        with self.assertRaises(SystemExit) as stop:
            self.send([TimeoutError()] * 3)
        self.assertIn("timed out three times", str(stop.exception))

    def test_full_and_short_head_both_parse_as_given(self):
        for head in ("abcdef0123456789abcdef0123456789abcdef01", "abcdef01"):
            args = task_record.parse(["pr_opened", "--lane", "l", "--execution", "gh-team", "--issue", "1", "--head", head])
            self.assertEqual(args.head, head)


class DifferentPayloadIsNotAlreadyRecorded(unittest.TestCase):
    """#1548: GHE003 on a merged record is 'already recorded' only when the stored record is this one."""

    CONFLICT = {"ok": False, "diagnostics": [{"code": "GHE003_IDEMPOTENCY_CONFLICT", "severity": "error"}]}

    def send(self, stored_sha, sent_sha="c" * 40):
        args = SimpleNamespace(lane="l", kind="merged", repo="r", pr=7, issue=1, comment_url=None,
                               url="http://127.0.0.1:1", execution="e", dry_run=False, token_file="t")
        doc = {"taskId": "issue-1", "pr": 7, "mergeSha": sent_sha, "closes": [1], "merger": "l", "at": "now"}
        stored = {"description": json.dumps({**doc, "mergeSha": stored_sha, "at": "earlier"})}
        with patch.object(task_record, "read_credential", return_value="x"), \
                patch.object(task_record, "read_execution_history", return_value=[({}, "task.merged", stored)]), \
                patch.object(task_record.urllib.request, "urlopen",
                             side_effect=lambda request, timeout: io.BytesIO(json.dumps(self.CONFLICT).encode())):
            return task_record.send(args, doc, "now")

    def test_same_payload_is_already_recorded(self):
        self.assertEqual(self.send("c" * 40)[0], 2)

    def test_a_different_merge_sha_exits_naming_the_stored_one(self):
        with self.assertRaises(SystemExit) as stop:
            self.send("d" * 40)
        message = str(stop.exception)
        self.assertIn("differs from this one", message)
        self.assertIn("d" * 40, message)

    def test_a_lane_credential_that_cannot_read_history_leaves_the_comparison_unverified(self):
        # #1556: a lane credential gets 401 on the history read. The answer must say the
        # comparison is UNVERIFIED and nothing changed, never "already recorded", and never
        # blame a heartbeat (the old message said "heartbeat history is unavailable").
        args = SimpleNamespace(lane="l", kind="merged", repo="r", pr=7, issue=1, comment_url=None,
                               url="http://127.0.0.1:1", execution="e", dry_run=False, token_file="t")
        doc = {"taskId": "issue-1", "pr": 7, "mergeSha": "c" * 40, "closes": [1], "merger": "l", "at": "now"}

        def answer(request, timeout):
            if request.get_method() == "GET":
                raise task_record.urllib.error.HTTPError(request.full_url, 401, "Unauthorized", {}, None)
            return io.BytesIO(json.dumps(self.CONFLICT).encode())

        with patch.object(task_record, "read_credential", return_value="x"), \
                patch.object(task_record.urllib.request, "urlopen", side_effect=answer):
            with self.assertRaises(SystemExit) as stop:
                task_record.send(args, doc, "now")
        message = str(stop.exception)
        self.assertIn("UNVERIFIED", message)
        self.assertIn("401", message)
        self.assertNotIn("heartbeat", message)
        self.assertNotIn("already recorded", message)


class ClosesAsGiven(unittest.TestCase):
    """#464: `closes` is exactly the issues the merge closed. The default `[--issue]` recorded
    "closes #356" for PR #456, a `Refs #356` merge, and `--closes` with no numbers could not say none."""

    def closes(self, *extra):
        args = task_record.parse(["--lane", "gh-claude-6", "--execution", "gh-team", "merged", "--issue", "356", "--pr", "456",
                                  "--merge-sha", "c" * 40, *extra])
        return task_record.document(args, "2026-10-08T00:00:00Z")["closes"]

    def critic(self, *extra):
        args = task_record.parse(["--lane", "gh-claude-3", "--execution", "gh-team", "critic_verdict", "--issue", "467",
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
        args = task_record.parse(["--lane", "gh-claude-3", "--execution", "gh-team", "critic_verdict", "--issue", "467",
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
            document = task_record.document(task_record.parse(["--lane", "gh-claude-2", "--execution", "gh-team", *argv]), "2026-10-08T00:00:00Z")
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

    def test_an_omitted_title_without_gh_fails_loudly(self):
        # #1921: a claim without gh used to land with no title ("title not recorded" on the Graph).
        def unavailable(*_):
            raise task_record.GithubUnavailable("gh issue view 1921: FileNotFoundError")
        task_record.github_words = unavailable
        with self.assertRaises(SystemExit) as raised:
            self.doc("claimed", "--issue", "1921", "--branch", "b")
        self.assertIn("--title was omitted and gh could not read it", str(raised.exception))
        document, _ = self.doc("claimed", "--issue", "1921", "--branch", "b", "--title", "Mine")
        self.assertEqual(document["title"], "Mine")

    def test_a_missing_gh_binary_raises_unavailable(self):
        # #1921: the real reader, with gh absent from the host, names the failure instead of (None, None).
        task_record.github_words = self.github
        with patch.object(task_record.subprocess, "run", side_effect=FileNotFoundError("gh")):
            with self.assertRaises(task_record.GithubUnavailable):
                task_record.github_words("claimed", 1921, "stabem/GraphHelm")

class Planned(unittest.TestCase):
    """#480: `planned` records the lane's keel plan so the Studio lights Plan (and Critic for a
    `design` plan). Its fields come from the plan itself, never retyped, and the older steps keep
    their default revisions (their records' keys)."""

    PLAN = {"schema": "graphhelm-task-plan-v1", "taskId": "issue-480", "classes": ["user_visible", "code"],
            "invariantClasses": [], "journeys": [], "proof": "both", "reviews": 1, "skills": [], "tools": [],
            "delegation": {}, "path": ["card", "design"], "decidedBy": "rules", "jev": None,
            "critic": {"mode": "design", "passScore": 8, "maxRounds": 3}}

    def planned(self, *extra):
        args = task_record.parse(["--lane", "gh-claude-1", "--execution", "gh-team", "planned", "--issue", "480", *extra])
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
        run = lambda *a: subprocess.run(["git", "-C", repo, *a], capture_output=True, check=True, creationflags=NO_WINDOW)
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

        args = task_record.parse(["--lane", "lane-a", "--execution", "gh-team", "planned", "--issue", "702", "--summary", "s",
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
        args = task_record.parse(["--lane", "lane-a", "--execution", "gh-team", "planned", "--issue", "702", "--summary", "s",
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

    def test_the_record_carries_normalized_valid_paths_only(self):
        # #1825: the Runtime refuses a whole `task.planned` whose `paths` has one bad entry, so the
        # script sends only entries the door accepts; this is the door's rule, replicated.
        err = io.StringIO()
        with redirect_stderr(err):
            doc = self.planned("--summary", "s", "--paths", "b\\c.rs", "a/x.rs", "a/x.rs", "../up.rs",
                               "/abs.rs", "C:/win.rs", "a//b.rs", "./a.rs", "sp ace.rs", "back\\slash.rs", "",
                               "é.rs", *self.offline())
        self.assertEqual(doc["paths"], ["a/x.rs", "b/c.rs", "back/slash.rs"])
        self.assertIn("not recording 8 path(s)", err.getvalue())

    def test_the_record_caps_paths_at_the_runtime_limit_and_the_length(self):
        names = [f"d/f{n:03}.rs" for n in range(70)]
        err = io.StringIO()
        with redirect_stderr(err):
            doc = self.planned("--summary", "s", "--paths", *names, "x" * 257, "y" * 256, *self.offline())
        self.assertEqual(doc["paths"], sorted(names + ["y" * 256])[:64])
        self.assertIn("first 64 of 71", err.getvalue())
        self.assertNotIn("x" * 257, err.getvalue())

    def test_long_paths_shrink_to_fit_the_runtime_record_cap(self):
        # #1861: 64 paths of 200 characters pass the count cap but not the 8 KiB description cap
        # the Runtime enforces; the record keeps what fits, in order, and says how many it dropped.
        names = [f"d{n:02}/" + "p" * 190 + ".rs" for n in range(64)]
        err = io.StringIO()
        with redirect_stderr(err):
            doc = self.planned("--summary", "s", "--paths", *names, *self.offline())
        self.assertLessEqual(len(json.dumps(doc, separators=(",", ":")).encode()), 8000)
        self.assertTrue(0 < len(doc["paths"]) < 64)
        self.assertEqual(doc["paths"], sorted(names)[:len(doc["paths"])])
        self.assertIn(f"recording {len(doc['paths'])} of 64 paths", err.getvalue())

    def test_a_plan_without_paths_omits_the_field_and_git_paths_fill_it(self):
        with redirect_stderr(io.StringIO()):
            self.assertNotIn("paths", self.planned("--summary", "s", *self.offline()))
            self.assertNotIn("paths", self.planned("--summary", "s", "--paths", "../x", *self.offline()))
        with patch.object(task_record, "git_paths", return_value=["z.rs", "a.rs"]):
            doc = self.planned("--summary", "s", "--plan-file", self.plan_file(self.PLAN))
        self.assertEqual(doc["paths"], ["a.rs", "z.rs"])

    def test_no_summary_is_refused(self):
        with self.assertRaises(SystemExit):
            self.planned("--plan-file", self.plan_file(self.PLAN))
        with self.assertRaises(SystemExit):
            self.planned("--plan-file", self.plan_file(self.PLAN), "--summary", "x" * 301)

    def test_the_older_steps_keep_their_default_revisions(self):
        for kind, extra, revision in (("claimed", ["--branch", "b"], 1),
                                      ("pr_opened", ["--pr", "1", "--head", HEAD_A, "--reviewer", "r"], 2),
                                      ("merged", ["--pr", "1", "--merge-sha", HEAD_A], 5)):
            # #1921: --no-github; this cell is about revisions, and an omitted title without gh now fails.
            args = task_record.parse(["--lane", "l", "--execution", "gh-team", kind, "--issue", "9", "--no-github", *extra])
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
                code = task_record.main(["--lane", "test", "--execution", "gh-team", "--issue", "674", "merged",
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
                task_record.main(["--lane", "test", "--execution", "gh-team", "--issue", "747", "closed", "--pr", "685",
                                  "--reason", "superseded", "--by", "740"])
        self.assertIn("state=OPEN", str(error.exception))
        self.assertEqual(run.call_args.args[0], ["gh", "pr", "view", "685", "--json", "state,mergedAt",
                                                 "--repo", "stabem/GraphHelm"])
        send.assert_not_called()


class MergedPrIsTerminal(unittest.TestCase):
    """#860: no follow-up task record may reopen a merged PR. Local gh/send stubs; no network."""

    def test_pr_opened_refuses_a_merged_pr_without_sending(self):
        with patch.object(task_record.subprocess, "run", return_value=subprocess.CompletedProcess(
                ["gh"], 0, json.dumps({"state": "MERGED"}), "")) as run, \
                patch.object(task_record, "send") as send:
            with self.assertRaises(SystemExit) as error:
                task_record.main(["--lane", "test", "--execution", "gh-team", "--issue", "860", "pr_opened", "--pr", "840",
                                  "--head", HEAD_B, "--reviewer", "codex-review"])
        self.assertIn("PR #840 is already merged", str(error.exception))
        self.assertEqual(run.call_args.args[0], ["gh", "pr", "view", "840", "--json", "state",
                                                 "--repo", "stabem/GraphHelm"])
        send.assert_not_called()

    def test_review_assigned_refuses_a_merged_pr_without_sending(self):
        with patch.object(task_record.subprocess, "run", return_value=subprocess.CompletedProcess(
                ["gh"], 0, json.dumps({"state": "MERGED"}), "")) as run, \
                patch.object(task_record, "send", return_value=(0, "record", {})) as send:
            with self.assertRaises(SystemExit) as error:
                task_record.main(["--lane", "test", "--execution", "gh-team", "--issue", "860", "review_assigned", "--pr", "840",
                                  "--head", HEAD_B, "--reviewer", "codex-review"])
        self.assertIn("PR #840 is already merged", str(error.exception))
        self.assertEqual(run.call_args.args[0], ["gh", "pr", "view", "840", "--json", "state",
                                                 "--repo", "stabem/GraphHelm"])
        send.assert_not_called()

    def test_closed_unmerged_pr_sends_the_requested_close_record(self):
        with patch.object(task_record.subprocess, "run", return_value=subprocess.CompletedProcess(
                ["gh"], 0, json.dumps({"state": "CLOSED", "mergedAt": None}), "")), \
                patch.object(task_record, "send", return_value=(0, "record", {})) as send:
            self.assertEqual(task_record.main(["--lane", "test", "--execution", "gh-team", "--issue", "747", "closed", "--pr", "685",
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


class LaneDocsGuardTest(unittest.TestCase):
    """#1197 (A8): the two pages a lane reads name every record kind the recorder writes, and keep
    no sentence the code has outgrown. Cost: two file reads, no network."""

    REPO = Path(__file__).resolve().parents[2]

    def test_the_lane_docs_name_every_record_kind_the_recorder_writes(self):
        import re
        kinds = set(task_record.KINDS)
        lanes = (self.REPO / "docs/process/LANES.md").read_text(encoding="utf-8")
        section = lanes.split("## 4. Task records", 1)[1].split("\n## ", 1)[0]
        # LANES §4: every `$T --issue N <kind>` in the command table (`$T queued` takes no issue, #1469).
        in_lanes = set(re.findall(r"\$T (?:--issue N )?(\w+)", section))
        self.assertEqual(in_lanes, kinds, f"LANES.md §4 names {sorted(in_lanes)}, task_record.py writes {sorted(kinds)}")
        self.assertNotIn("Until the change that does so lands", section,
                         "#508 landed: pr_opened --reviewer records review_assigned")
        self.assertNotIn("the Runtime cannot stop", section,
                         "#1179 landed: a Runtime started with --require-lane-credential refuses another lane's record")
        delivery = (self.REPO / "docs/process/DELIVERY.md").read_text(encoding="utf-8")
        table = delivery.split("### Task records", 1)[1].split("`taskId` is `issue-<N>`", 1)[0]
        in_delivery = set(re.findall(r"\| `task\.(\w+)` \|", table))
        self.assertEqual(in_delivery, kinds, f"DELIVERY.md step table names {sorted(in_delivery)}, task_record.py writes {sorted(kinds)}")


class ReviewActivityCaller(unittest.TestCase):
    def request(self, *extra):
        output = io.StringIO()
        with redirect_stdout(output), redirect_stderr(io.StringIO()), patch.object(task_record.urllib.request, "urlopen", side_effect=AssertionError("dry-run must not contact Runtime")):
            code = task_record.main(["--lane", "reviewer", "--execution", "gh-team", "review_activity", "--issue", "1745",
                "--repo", "fixture/project", "--pr", "17", "--head", HEAD_A,
                "--review-context-sequence", "41", "--revision", "1", "--activity", "waiting",
                "--reason", "Waiting for measured dependency proof", "--no-github", "--dry-run", *extra])
        self.assertEqual(code, 0)
        return json.loads(output.getvalue())

    def test_exact_reviewer_binding_context_revision_and_retry_key(self):
        first = self.request()
        same = self.request()
        doc = json.loads(first["body"]["signal"]["description"])
        self.assertEqual(doc["reviewer"], "reviewer")
        self.assertEqual((doc["repo"], doc["pr"], doc["headSha"], doc["reviewContextSequence"], doc["revision"]),
                         ("fixture/project", 17, HEAD_A, 41, 1))
        self.assertEqual(first["headers"]["Idempotency-Key"], same["headers"]["Idempotency-Key"])
        self.assertNotEqual(first["headers"]["Idempotency-Key"], self.request("--review-context-sequence", "42")["headers"]["Idempotency-Key"])
        self.assertNotEqual(first["headers"]["Idempotency-Key"], self.request("--revision", "2")["headers"]["Idempotency-Key"])
        args = task_record.parse(["--lane", "reviewer", "--execution", "gh-team", "review_activity", "--issue", "1745", "--pr", "17", "--head", HEAD_A,
            "--revision", "2", "--review-context-sequence", "41", "--activity", "resumed", "--no-github"])
        resumed = task_record.document(args, "2026-01-01T00:00:00Z")
        self.assertEqual(resumed["activity"], "resumed")
        self.assertNotIn("reason", resumed)

    def test_invalid_or_implicit_activity_binding_is_refused_before_send(self):
        base = ["--lane", "reviewer", "review_activity", "--issue", "1745", "--pr", "17", "--head", HEAD_A,
                "--revision", "1", "--review-context-sequence", "41", "--activity", "waiting", "--reason", "A reason", "--no-github"]
        for key, value in [("--head", "abcdef0"), ("--pr", "0"), ("--revision", "0"), ("--review-context-sequence", "0"),
                           ("--reason", " "), ("--reason", "x" * 501), ("--reason", "line\ncontrol"), ("--activity", "resumed")]:
            candidate = base.copy()
            candidate[candidate.index(key) + 1] = value
            with self.subTest(key=key, value=value), self.assertRaises(SystemExit):
                task_record.document(task_record.parse(candidate), "2026-01-01T00:00:00Z")
        for key in ["--revision", "--review-context-sequence"]:
            candidate = base.copy()
            index = candidate.index(key)
            del candidate[index:index + 2]
            with self.subTest(missing=key), self.assertRaises(SystemExit):
                task_record.document(task_record.parse(candidate), "2026-01-01T00:00:00Z")


    def test_review_activity_refusals_and_conflicts_do_not_refresh_or_retry_without_bound(self):
        args = SimpleNamespace(lane="reviewer", kind="review_activity", repo="fixture/project", pr=17,
            issue=1745, comment_url=None, url="http://127.0.0.1:1", execution="e", dry_run=False, token_file="t")
        for error, maximum in [("GHE001_SEQUENCE_CONFLICT", 2), ("GHE003_IDEMPOTENCY_CONFLICT", 2),
                               ("GHCLI003_SIGNAL_INVALID", 1)]:
            with self.subTest(error=error), patch.object(task_record, "read_credential", return_value="fixture-only"), \
                    patch.object(task_record.urllib.request, "urlopen", side_effect=lambda *a, **k:
                        io.BytesIO(json.dumps({"ok":False,"diagnostics":[{"code":error,"severity":"error"}]}).encode())) as request:
                code, _, _ = task_record.send(args, {"taskId":"issue-1745"}, "now")
                self.assertEqual(code, 1)
                self.assertEqual(request.call_count, maximum)
                self.assertEqual(len({call.args[0].get_header("Idempotency-key") for call in request.call_args_list}), 1)
                self.assertEqual(len({call.args[0].data for call in request.call_args_list}), 1)
        # A conflict is never success: only the actual retried Runtime acknowledgement is.
        for final, expected in [(True, 0), (False, 1)]:
            replies = [io.BytesIO(json.dumps({"ok":False,"diagnostics":[{"code":"GHE003_IDEMPOTENCY_CONFLICT","severity":"error"}]}).encode()),
                       io.BytesIO(json.dumps({"ok":final,"diagnostics":[] if final else [{"code":"GHE003_IDEMPOTENCY_CONFLICT","severity":"error"}]}).encode())]
            with self.subTest(final=final), patch.object(task_record, "read_credential", return_value="fixture-only"), \
                    patch.object(task_record.urllib.request, "urlopen", side_effect=replies) as request:
                code, _, _ = task_record.send(args, {"taskId":"issue-1745"}, "now")
                self.assertEqual(code, expected)
                self.assertEqual(request.call_count, 2)
                self.assertIs(request.call_args_list[0].args[0], request.call_args_list[1].args[0])



if __name__ == "__main__":
    unittest.main()
