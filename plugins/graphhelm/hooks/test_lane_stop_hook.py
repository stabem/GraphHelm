"""Stop contract against a real Runtime with a disposable sealed event store.

Cost: a few seconds, installed graphhelm, one loopback port; no builds or real Runtime.
"""
import hashlib
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import os
from pathlib import Path
import re
import signal
import shutil
import subprocess
import sys
import tempfile
import time
import threading
import unittest
from unittest import mock
import urllib.error
import urllib.request

from plugins.graphhelm.hooks import tasks_reconcile
from plugins.graphhelm.hooks import session_hook

# Owner rule: children run hidden in the calling terminal, never in a new console window.
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
MANIFESTS = (HERE / "hooks.json", HERE / "codex-hooks.json",
             REPO / "plugins/graphhelm-codex-hooks/hooks/codex-hooks.json")


def time_scale() -> int:
    """#1199: `GRAPHHELM_TEST_TIME_SCALE` (LANES.md §3) multiplies this file's wall-clock ceilings
    (the 300-note reminder budget, the Stop hang catcher) when the slot's outer script declares a
    loaded machine: a whole number from 1 to 20. Any other value is a named error, never a silent
    1, the same rule as apps/cli/tests/support/time_scale.rs. Product budgets never read it."""
    raw = os.environ.get("GRAPHHELM_TEST_TIME_SCALE", "1")
    if not raw.isdigit() or not 1 <= int(raw) <= 20:
        raise ValueError(f"GRAPHHELM_TEST_TIME_SCALE={raw!r} is not a whole number from 1 to 20")
    return int(raw)


class TimeScaleTests(unittest.TestCase):
    """#1199: the shared-machine knob is honoured and a malformed one is a named red."""

    def test_time_scale_reads_the_declared_factor_and_refuses_anything_else(self):
        with mock.patch.dict(os.environ, {"GRAPHHELM_TEST_TIME_SCALE": "3"}):
            self.assertEqual(time_scale(), 3)
        with mock.patch.dict(os.environ, {}):
            os.environ.pop("GRAPHHELM_TEST_TIME_SCALE", None)
            self.assertEqual(time_scale(), 1)
        for bad in ("fast", "0", "21", "2.5", ""):
            with self.subTest(value=bad), mock.patch.dict(os.environ, {"GRAPHHELM_TEST_TIME_SCALE": bad}):
                with self.assertRaises(ValueError) as refused:
                    time_scale()
                self.assertIn("GRAPHHELM_TEST_TIME_SCALE", str(refused.exception))


class ReconciledDigestTests(unittest.TestCase):
    """#1195: an unchanged reconcile reply is not posted again. Offline: `request` is replaced."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        token = Path(self.tmp.name) / "events.agent.token"
        token.write_text("tok", encoding="utf-8")
        self.bound = ("lane-test", str(token), "http://127.0.0.1:1", "http://127.0.0.1:1", None)
        self.memory = Path(self.tmp.name) / "state" / "session.reconciled"

    def test_unchanged_reply_posts_once_per_session_and_a_changed_reply_posts_again(self):
        # On the parent every call POSTs (the second one earns a 409 the hook swallows): a refused
        # request on every quiet Stop. Cost: four in-process calls, no network.
        if str(HERE) not in sys.path:
            sys.path.insert(0, str(HERE))  # lane_stop_hook imports its siblings bare, as the hosts run it
        import lane_stop_hook
        quiet = {"merged": [], "closed": [], "released": [], "heartbeat": {"sent": [], "refused": []}, "inspected": 0}
        busy = {**quiet, "merged": [9001]}
        with mock.patch.object(lane_stop_hook, "request", return_value={"ok": True, "data": {}}) as post:
            lane_stop_hook.record_reconciled(self.bound, "codex-4", quiet, self.memory)
            lane_stop_hook.record_reconciled(self.bound, "codex-4", quiet, self.memory)
            self.assertEqual(post.call_count, 1, "an unchanged reply was posted again")
            lane_stop_hook.record_reconciled(self.bound, "codex-4", busy, self.memory)
            self.assertEqual(post.call_count, 2, "a changed reply must be posted")
            lane_stop_hook.record_reconciled(self.bound, "codex-4", busy, self.memory)
            self.assertEqual(post.call_count, 2)
        # A session with no memory of the reply (fresh state) posts it; the Runtime's 409 for a key
        # it already holds is "recorded", not an error, and is remembered so the next Stop is quiet.
        fresh = Path(self.tmp.name) / "state" / "other.reconciled"
        conflict = urllib.error.HTTPError("http://127.0.0.1:1", 409, "conflict", {}, None)
        with mock.patch.object(lane_stop_hook, "request", side_effect=conflict) as post:
            lane_stop_hook.record_reconciled(self.bound, "codex-4", busy, fresh)
            lane_stop_hook.record_reconciled(self.bound, "codex-4", busy, fresh)
            self.assertEqual(post.call_count, 1)


def stop_processes_naming(fragment):
    """#1459: the waiter deletes its own pid file when it exits, but the `wake-wait` child it parked
    in is then an orphan that no pid file names (and `taskkill /T` cannot reach it, its parent is
    gone). It still holds files in the temp dir. Stop every process whose command line names the
    dir, by that command line, and return how many were asked to stop."""
    if os.name == "nt":
        script = ("$f = $env:GRAPHHELM_SWEEP_FRAGMENT; $n = 0; Get-CimInstance Win32_Process | Where-Object { "
                  "$_.ProcessId -ne $PID -and $_.CommandLine -and $_.CommandLine.Contains($f) } | "
                  "ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue; $n++ }; $n")
        run = subprocess.run(["powershell", "-NoProfile", "-NonInteractive", "-Command", script],
                             env={**os.environ, "GRAPHHELM_SWEEP_FRAGMENT": fragment},
                             capture_output=True, text=True, timeout=30, creationflags=NO_WINDOW)
    else:
        run = subprocess.run(["pkill", "-9", "-f", "--", fragment], capture_output=True, text=True, timeout=30,
                             creationflags=NO_WINDOW)
        return 0 if run.returncode else 1
    try:
        return int(run.stdout.strip() or 0)
    except ValueError:
        return 0


def remove_busy_tmp(tmp, stop_writers, seconds=10.0):
    """#1459: a waiter (or its scan child) that wrote its pid file after the first stop_waiters can
    still be writing in the test's temp dir, and Windows refuses the delete (WinError 145/32). Stop
    the writers again and retry for a bounded time; if the directory still will not go, fail and
    name what is left instead of hiding it."""
    deadline = time.monotonic() + seconds
    while True:
        stop_writers()
        try:
            tmp.cleanup()
            return
        except OSError as error:
            if time.monotonic() >= deadline:
                left = sorted(str(p.relative_to(tmp.name)) for p in Path(tmp.name).rglob("*"))[:20]
                raise AssertionError(f"temp dir still busy after {seconds} s ({error}); left: {left}") from error
            time.sleep(0.2)


class BusyTempCleanupTests(unittest.TestCase):
    def test_an_orphan_that_names_the_dir_and_holds_a_file_is_stopped(self):
        tmp = tempfile.TemporaryDirectory()
        held = Path(tmp.name) / "orphan.log"
        code = "import sys,time; f=open(sys.argv[1],'w'); time.sleep(120)"
        orphan = subprocess.Popen([sys.executable, "-c", code, str(held)], creationflags=NO_WINDOW)
        try:
            deadline = time.monotonic() + 10
            while not held.exists() and time.monotonic() < deadline:
                time.sleep(0.05)
            remove_busy_tmp(tmp, lambda: stop_processes_naming(tmp.name), seconds=15)
            self.assertFalse(Path(tmp.name).exists())
            self.assertIsNotNone(orphan.wait(timeout=10))
        finally:
            orphan.kill()
            orphan.wait()

    def test_a_file_released_shortly_after_cleanup_starts_is_still_removed(self):
        tmp = tempfile.TemporaryDirectory()
        held = (Path(tmp.name) / "late-writer" / "waiter.log")
        held.parent.mkdir()
        handle = held.open("w")
        threading.Timer(0.5, handle.close).start()
        remove_busy_tmp(tmp, lambda: None, seconds=5)
        self.assertFalse(Path(tmp.name).exists())

    @unittest.skipUnless(os.name == "nt", "an open file blocks a delete only on Windows")
    def test_a_writer_that_never_stops_fails_and_names_the_file(self):
        tmp = tempfile.TemporaryDirectory()
        held = Path(tmp.name) / "stuck.log"
        handle = held.open("w")
        try:
            with self.assertRaises(AssertionError) as caught:
                remove_busy_tmp(tmp, lambda: None, seconds=0.5)
            self.assertIn("stuck.log", str(caught.exception))
        finally:
            handle.close()
            tmp.cleanup()


class LaneStopTests(unittest.TestCase):
    def setUp(self):
        self.real_state_root = session_hook._state_root()
        self.real_state_before = set(self.real_state_root.iterdir()) if self.real_state_root.exists() else set()
        self.addCleanup(self.assert_real_state_unchanged)
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.remove_tmp)
        root = Path(self.tmp.name)
        self.env = {**os.environ, "GRAPHHELM_EVENTS_KEY": "12" * 32,
                    "GRAPHHELM_EXECUTION_ID": "lane-test", "GRAPHHELM_ACTOR": "codex-4",
                    "LOCALAPPDATA": str(root / "local"),
                    "XDG_STATE_HOME": str(root / "xdg-state")}
        self.cli = os.environ.get("GRAPHHELM_CLI") or shutil.which("graphhelm")
        self.assertIsNotNone(self.cli, "installed graphhelm is required")
        keyring = root / "keyring"
        keyring.mkdir()
        self.run_cli("gateway", "keyring", "init", "--keyring", str(keyring), "--key-id", "test")
        events = root / "events"
        self.env["GRAPHHELM_EVENTS_DIR"] = str(events)
        self.env["GRAPHHELM_CLI"] = self.cli
        self.hook_state_dir = root / "hook-state"
        self.env["GRAPHHELM_HOOK_STATE_DIR"] = str(self.hook_state_dir)
        self.run_cli("execution", "start", "--file", str(REPO / "examples/graphs/software-feature.yaml"),
                     "--events", str(events), "--execution", "lane-test", "--mode", "manual", "--held")
        self.log = (root / "runtime.log").open("w+")
        self.addCleanup(self.log.close)
        self.server = subprocess.Popen([self.cli, "serve", "--events", str(events), "--bind", "127.0.0.1:0",
                                        "--keyring", str(keyring), "--key-id", "test"],
                                       env=self.env, stdout=self.log, stderr=subprocess.STDOUT, creationflags=NO_WINDOW)
        self.addCleanup(self.stop_server)
        self.addCleanup(self.stop_waiters)
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            self.log.seek(0)
            line = self.log.readline()
            if line:
                self.url = "http://" + json.loads(line)["data"]["address"]
                break
            time.sleep(0.05)
        else:
            self.fail("temporary Runtime did not start")
        self.assertNotEqual(self.url, "http://127.0.0.1:8793")
        self.env.update(GRAPHHELM_RUNTIME_URL=self.url,
                        GRAPHHELM_TOKEN_FILE=str(root / "events.agent.token"))
        self.owner_token = (root / "events.token").read_text().strip()
        # #1148: a task.* record is written only with the lane's credential (never the shared
        # agent token), minted by the owner once per lane; the reconciler writes `merged` as
        # `graphhelm` and `closed`/`released` as the lane. Requires an upgraded CLI.
        for lane in ("graphhelm", "codex-4"):
            self.run_cli("tasks", "credential", "--lane", lane, "--execution", "lane-test",
                         "--token-file", str(root / "events.token"))
        self.env.pop("GRAPHHELM_SESSION_ID", None)

    def lane_credential(self, lane):
        return (Path(self.env["GRAPHHELM_TOKEN_FILE"]).with_name(f"events.agent.{lane}.token")
                .read_text().strip())

    def stop_server(self):
        self.server.terminate()
        self.server.wait(timeout=10)

    def stop_waiters(self):
        """Stop only waiter processes recorded in this test's disposable state directory."""
        for pid_file in self.hook_state_dir.glob("*.waiter.pid"):
            try:
                pid = int(pid_file.read_text())
                self.stop_waiter_pid(pid)
                self.assertFalse(session_hook._pid_alive(pid), f"waiter {pid} survived cleanup")
                pid_file.unlink(missing_ok=True)
            except (OSError, ValueError):
                pass
        stop_processes_naming(self.tmp.name)

    def remove_tmp(self):
        remove_busy_tmp(self.tmp, self.stop_waiters if hasattr(self, "hook_state_dir") else (lambda: None))

    def assert_real_state_unchanged(self):
        after = set(self.real_state_root.iterdir()) if self.real_state_root.exists() else set()
        created = after - self.real_state_before
        own_state = {path.stem for path in created if path.suffix == ".json"
                     and self.url in path.read_text(encoding="utf-8", errors="replace")}
        self.assertFalse(own_state,
                         "hook test wrote into the real state directory")

    @staticmethod
    def stop_waiter_pid(pid):
        if os.name == "nt":
            subprocess.run(["taskkill", "/PID", str(pid), "/T", "/F"], capture_output=True,
                           timeout=5, creationflags=NO_WINDOW)
        elif session_hook._pid_alive(pid):
            os.kill(pid, signal.SIGTERM)
        deadline = time.monotonic() + 5
        while session_hook._pid_alive(pid) and time.monotonic() < deadline:
            time.sleep(0.05)

    def run_cli(self, *args):
        result = subprocess.run([self.cli, *args], env=self.env, capture_output=True, text=True, timeout=15, creationflags=NO_WINDOW)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_reconcile_reads_send_the_bound_lane_actor_header(self):
        self.claim(766)
        token = Path(self.env["GRAPHHELM_TOKEN_FILE"]).read_text(encoding="utf-8").strip()
        requests = []
        urlopen = tasks_reconcile.urllib.request.urlopen

        def capture(request, *args, **kwargs):
            requests.append(request)
            return urlopen(request, *args, **kwargs)

        with mock.patch.object(tasks_reconcile.urllib.request, "urlopen", side_effect=capture):
            tasks_reconcile.records(self.url, token, "lane-test", "codex-4")

        self.assertGreaterEqual(len(requests), 2, "reconcile should read events and their sealed evidence")
        self.assertTrue(all(request.get_header("X-graphhelm-actor") == "codex-4" for request in requests),
                        [request.headers for request in requests])

    def note(self, signal_id, to=None, reply=None, actor="owner"):
        signal = {"id": signal_id, "source": {"type": "user", "id": actor}, "type": "operator_note",
                  "severity": "low", "description": "Status please" if not reply else "Status recorded",
                  "evidence": ["lane-test"], "emittedAt": "2026-10-10T00:00:00Z"}
        if to:
            signal["to"] = to
        if reply:
            signal["replyTo"] = reply
        token = self.owner_token if actor == "owner" else Path(self.env["GRAPHHELM_TOKEN_FILE"]).read_text().strip()
        req = urllib.request.Request(self.url + "/v1/executions/lane-test/signal",
              data=json.dumps({"signal": signal}).encode(), headers={"Authorization": "Bearer " + token,
              "Content-Type": "application/json", "Idempotency-Key": signal_id,
              "X-GraphHelm-Actor": actor, "X-GraphHelm-Actor-Type": "owner" if actor == "owner" else "agent"})
        with urllib.request.urlopen(req, timeout=5) as response:
            self.assertTrue(json.load(response)["ok"])

    def test_reconcile_records_github_merge_once(self):
        """A fake gh MERGED reply closes one recorded PR in the disposable Runtime."""
        sha = "a" * 40
        reply = {"state": "MERGED", "mergeCommit": {"oid": sha},
                 "closingIssuesReferences": []}
        now = "2026-10-10T00:00:00Z"
        document = {"schema": "graphhelm-task-event-v1", "taskId": "issue-766",
                    "revision": 2, "at": now, "pr": 9001, "headSha": "b" * 40,
                    "journeys": [], "lane": "codex-4", "repo": "stabem/GraphHelm"}
        signal = {"id": "pr-9001-opened", "source": {"type": "user", "id": "codex-4"},
                  "type": "task.pr_opened", "severity": "low", "description": json.dumps(document),
                  "evidence": ["https://github.com/stabem/GraphHelm/pull/9001"], "emittedAt": now}
        token = self.lane_credential("codex-4")
        request = urllib.request.Request(self.url + "/v1/executions/lane-test/signal",
                  data=json.dumps({"signal": signal}).encode(), headers={"Authorization": "Bearer " + token,
                  "Content-Type": "application/json", "Idempotency-Key": "pr-9001-opened",
                  "X-GraphHelm-Actor": "codex-4", "X-GraphHelm-Actor-Type": "agent"})
        with urllib.request.urlopen(request, timeout=5) as response:
            self.assertTrue(json.load(response)["ok"])
        args = type("Args", (), {"execution": "lane-test", "url": self.url,
                    "token_file": self.env["GRAPHHELM_TOKEN_FILE"],
                    "repo": "stabem/GraphHelm", "lane": "codex-4"})()
        with mock.patch.object(tasks_reconcile, "github", return_value=reply) as github:
            self.assertEqual(tasks_reconcile.reconcile(args)["merged"], [9001])
            self.assertEqual(tasks_reconcile.reconcile(args)["merged"], [])
        github.assert_called_once_with(9001, "stabem/GraphHelm")

    def test_reconcile_finishes_a_pr_opened_by_another_lane(self):
        """#1520: a stopping lane records the merge or close of a PR some other lane opened."""
        args = self.reconcile_args()
        opened = ("task.pr_opened", {"taskId": "issue-766", "pr": 9001,
                  "lane": "codex-9", "repo": "stabem/GraphHelm"}, {})
        merged = {"state": "MERGED", "mergeCommit": {"oid": "a" * 40}, "closingIssuesReferences": []}
        with mock.patch.object(tasks_reconcile, "records", return_value=[opened]),              mock.patch.object(tasks_reconcile, "github", return_value=merged),              mock.patch.object(tasks_reconcile, "post") as post:
            self.assertEqual(tasks_reconcile.reconcile(args)["merged"], [9001])
        post.assert_called_once()

    def test_one_unreadable_pr_does_not_abort_the_reconcile_and_the_batch_is_capped(self):
        """#1520: a failing `gh pr view` is skipped; only the oldest MAX_PRS_PER_STOP are read."""
        args = self.reconcile_args()
        events = [("task.pr_opened", {"taskId": f"issue-{n}", "pr": n, "lane": "codex-9",
                   "repo": "stabem/GraphHelm"}, {}) for n in range(1, 31)]
        merged = {"state": "MERGED", "mergeCommit": {"oid": "a" * 40}, "closingIssuesReferences": []}

        def view(pr, repo):
            if pr == 1:
                raise subprocess.TimeoutExpired("gh", 3)
            return merged
        with mock.patch.object(tasks_reconcile, "records", return_value=events),              mock.patch.object(tasks_reconcile, "github", side_effect=view) as github,              mock.patch.object(tasks_reconcile, "post"):
            result = tasks_reconcile.reconcile(args)
        self.assertEqual(result["unreadable"], [1])
        self.assertEqual(result["merged"], list(range(2, 21)))
        self.assertEqual(github.call_count, 20)

    def test_reconcile_ignores_opened_pr_from_another_repository(self):
        args = type("Args", (), {"execution": "lane-test", "url": self.url,
                    "token_file": self.env["GRAPHHELM_TOKEN_FILE"],
                    "repo": "stabem/GraphHelm", "lane": "codex-4"})()
        opened = ("task.pr_opened", {"taskId": "issue-766", "pr": 9001,
                  "lane": "codex-4", "repo": "someone/else"}, {})
        with mock.patch.object(tasks_reconcile, "records", return_value=[opened]), \
             mock.patch.object(tasks_reconcile, "github") as github:
            self.assertEqual(tasks_reconcile.reconcile(args)["inspected"], 0)
        github.assert_not_called()

    def test_merged_pr_in_other_repository_does_not_suppress_opened_pr(self):
        args = type("Args", (), {"execution": "lane-test", "url": self.url,
                    "token_file": self.env["GRAPHHELM_TOKEN_FILE"],
                    "repo": "stabem/GraphHelm", "lane": "codex-4"})()
        events = [("task.merged", {"taskId": "issue-766", "pr": 9001,
                   "repo": "someone/else"}, {}),
                  ("task.pr_opened", {"taskId": "issue-766", "pr": 9001,
                   "lane": "codex-4", "repo": "stabem/GraphHelm"}, {})]
        reply = {"state": "OPEN", "mergeCommit": None, "closingIssuesReferences": []}
        with mock.patch.object(tasks_reconcile, "records", return_value=events), \
             mock.patch.object(tasks_reconcile, "github", return_value=reply) as github:
            self.assertEqual(tasks_reconcile.reconcile(args)["inspected"], 1)
        github.assert_called_once_with(9001, "stabem/GraphHelm")

    def test_backlog_over_the_cap_is_rotated_so_every_pr_is_checked_within_two_stops(self):
        args = type("Args", (), {"execution": "lane-test", "url": self.url,
                    "token_file": self.env["GRAPHHELM_TOKEN_FILE"],
                    "repo": "stabem/GraphHelm", "lane": "codex-rot"})()
        events = [("task.pr_opened", {"taskId": f"issue-{n}", "pr": n, "lane": "codex-4",
                   "repo": "stabem/GraphHelm"}, {}) for n in range(9001, 9026)]
        seen = []

        def still_open(pr, repo):
            seen.append(pr)
            return {"state": "OPEN", "mergedAt": None, "body": ""}
        for _ in range(2):
            with mock.patch.object(tasks_reconcile, "records", return_value=events),                  mock.patch.object(tasks_reconcile, "github", side_effect=still_open):
                tasks_reconcile.reconcile(args)
        self.assertLessEqual(len(seen) - len(set(seen)), tasks_reconcile.MAX_PRS_PER_STOP)
        self.assertEqual(set(seen), set(range(9001, 9026)))

    def test_closed_unmerged_pr_without_lane_does_not_post_or_crash(self):
        args = type("Args", (), {"execution": "lane-test", "url": self.url,
                    "token_file": self.env["GRAPHHELM_TOKEN_FILE"],
                    "repo": "stabem/GraphHelm", "lane": None})()
        opened = ("task.pr_opened", {"taskId": "issue-766", "pr": 9001,
                  "lane": "codex-4", "repo": "stabem/GraphHelm"}, {})
        with mock.patch.object(tasks_reconcile, "records", return_value=[opened]), \
             mock.patch.object(tasks_reconcile, "github", return_value={"state": "CLOSED", "mergedAt": None, "body": ""}), \
             mock.patch.object(tasks_reconcile, "post_end") as post_end:
            result = tasks_reconcile.reconcile(args)
        self.assertEqual(result["closed"], [])
        post_end.assert_not_called()
    def test_closed_unmerged_pr_is_recorded_once(self):
        args = type("Args", (), {"execution": "lane-test", "url": self.url,
                    "token_file": self.env["GRAPHHELM_TOKEN_FILE"],
                    "repo": "stabem/GraphHelm", "lane": "codex-4"})()
        now = "2026-10-10T00:00:00Z"
        document = {"schema": "graphhelm-task-event-v1", "taskId": "issue-766",
                    "revision": 2, "at": now, "pr": 9001, "headSha": "b" * 40,
                    "journeys": [], "lane": "codex-4", "repo": "stabem/GraphHelm"}
        signal = {"id": "pr-9001-opened", "source": {"type": "user", "id": "codex-4"},
                  "type": "task.pr_opened", "severity": "low", "description": json.dumps(document),
                  "evidence": ["https://github.com/stabem/GraphHelm/pull/9001"], "emittedAt": now}
        token = self.lane_credential("codex-4")
        request = urllib.request.Request(self.url + "/v1/executions/lane-test/signal",
                  data=json.dumps({"signal": signal}).encode(), headers={"Authorization": "Bearer " + token,
                  "Content-Type": "application/json", "Idempotency-Key": "pr-9001-opened",
                  "X-GraphHelm-Actor": "codex-4", "X-GraphHelm-Actor-Type": "agent"})
        with urllib.request.urlopen(request, timeout=5) as response:
            self.assertTrue(json.load(response)["ok"])
        with mock.patch.object(tasks_reconcile, "github", return_value={"state": "CLOSED", "mergedAt": None, "body": "Superseded by #9002"}):
            result = tasks_reconcile.reconcile(args)
        self.assertEqual(result["closed"], [9001])
        with mock.patch.object(tasks_reconcile, "github") as github:
            self.assertEqual(tasks_reconcile.reconcile(args)["closed"], [])
        github.assert_not_called()
        found = tasks_reconcile.records(args.url, Path(args.token_file).read_text().strip(), args.execution)
        self.assertEqual([(kind, doc["revision"], doc["reason"], doc["by"]) for kind, doc, _ in found if kind == "task.closed"],
                         [("task.closed", 6, "superseded", 9002)])

    def reconcile_args(self):
        return type("Args", (), {"execution": "lane-test", "url": self.url,
                    "token_file": self.env["GRAPHHELM_TOKEN_FILE"],
                    "repo": "stabem/GraphHelm", "lane": "codex-4"})()

    def claim(self, issue):
        """Record codex-4's `task.claimed` for the issue on the temporary Runtime."""
        now = "2026-10-10T00:00:00Z"
        doc = {"schema": "graphhelm-task-event-v1", "taskId": f"issue-{issue}", "revision": 1,
               "at": now, "lane": "codex-4", "branch": f"issue-{issue}-test", "repo": "stabem/GraphHelm",
               "issue": issue}
        token = self.lane_credential("codex-4")
        signal = {"id": f"claim-{issue}", "source": {"type": "user", "id": "codex-4"},
                  "type": "task.claimed", "severity": "low", "description": json.dumps(doc),
                  "evidence": [f"https://github.com/stabem/GraphHelm/issues/{issue}"], "emittedAt": now}
        req = urllib.request.Request(self.url + "/v1/executions/lane-test/signal",
            data=json.dumps({"signal": signal}).encode(), headers={"Authorization": "Bearer " + token,
            "Content-Type": "application/json", "Idempotency-Key": f"claim-{issue}",
            "X-GraphHelm-Actor": "codex-4", "X-GraphHelm-Actor-Type": "agent"})
        try:
            response = urllib.request.urlopen(req, timeout=5)
        except urllib.error.HTTPError as error:
            self.fail(f"claim request failed: HTTP {error.code}: {error.read().decode('utf-8', 'replace')}")
        with response:
            answer = json.load(response)
            self.assertTrue(answer["ok"], answer)

    def test_reconcile_releases_an_overdue_claim_as_graphhelm_and_skips_live_ones(self):
        # #1193: the tasks route reports each open pre-PR claim's lease; the reconciler releases
        # the overdue ones as graphhelm (releasedBy), with the lane's credential never involved,
        # and leaves live leases and malformed entries alone. Cost: no Runtime, one mocked call.
        view = {"executionId": "lane-test", "headSequence": 9, "tasks": [], "records": [], "rejected": 0,
                "leases": [
                    {"key": "issue-766#claim-1", "taskId": "issue-766", "lane": "codex-4", "claimSequence": 7,
                     "leaseSeconds": 1800, "expiresAt": "2026-10-14T03:30:00Z", "overdue": True},
                    {"key": "issue-767#claim-1", "taskId": "issue-767", "lane": "codex-5", "claimSequence": 8,
                     "leaseSeconds": 1800, "expiresAt": "2026-10-14T04:30:00Z", "overdue": False},
                    {"key": "bad", "taskId": "issue-768", "lane": "codex-6", "claimSequence": "9", "overdue": True}]}
        credentials = {}
        with mock.patch.object(tasks_reconcile, "post_end") as post_end:
            out = tasks_reconcile.release_overdue(view, "http://127.0.0.1:1", lambda actor: credentials.setdefault(actor, f"cred-{actor}"),
                                                  "lane-test", "stabem/GraphHelm")
        self.assertEqual(out, {"released": [{"taskId": "issue-766", "lane": "codex-4", "claimSequence": 7}], "refused": []})
        self.assertEqual(post_end.call_count, 1)
        args = post_end.call_args.args
        self.assertEqual(args[:2], ("http://127.0.0.1:1", "cred-graphhelm"))
        self.assertEqual(args[2:7], ("lane-test", "released", "issue-766", "stabem/GraphHelm", "graphhelm"))
        self.assertEqual(args[7]["lane"], "codex-4")
        self.assertEqual(args[7]["claimSequence"], 7)
        self.assertEqual(args[7]["releasedBy"], "graphhelm")
        self.assertIn("1800 s lapsed at 2026-10-14T03:30:00Z", args[7]["reason"])
        self.assertEqual(list(credentials), ["graphhelm"])
        # An older Runtime without leases, or an unreachable route (no view), releases nothing and raises nothing.
        for older in ({"tasks": []}, None):
            self.assertEqual(tasks_reconcile.release_overdue(older, "http://127.0.0.1:1", lambda a: "c", "lane-test", "r"),
                             {"released": [], "refused": []})

    def test_reconcile_reports_stalled_prs_with_their_owner(self):
        # #1235: the tasks route's `stalls` (an open PR idle past the budget, with the lane that
        # owes the next step) ride the reconcile reply, so `tasks.reconciled` records them; an
        # older Runtime without the field reports none. Cost: no Runtime, no calls.
        stall = {"key": "issue-1136#pr-1136", "taskId": "issue-1136", "pr": 1136, "reason": "block_unanswered",
                 "owner": "codex-1", "idleSince": "2026-10-13T14:00:00Z", "idleSeconds": 75600}
        self.assertEqual(tasks_reconcile.stalled({"stalls": [stall, "junk"]}),
                         [{key: stall[key] for key in ("taskId", "pr", "reason", "owner", "idleSince", "idleSeconds")}])
        self.assertEqual(tasks_reconcile.stalled({"tasks": []}), [])
        self.assertEqual(tasks_reconcile.stalled(None), [])

    def test_stop_reconcile_heartbeats_an_open_claim_and_the_runtime_owns_the_cooldown(self):
        # #1147: a live lane heartbeats without being told. The first Stop reconcile lands one
        # task.heartbeat for the open pre-PR claim; the second, inside 300 s, is refused by the
        # Runtime's own rule and reconcile still succeeds. On the parent, reconcile has no
        # `heartbeat` key and no heartbeat event exists. Cost: one claim, two reconciles and one
        # event read against the temporary Runtime, under a second.
        args = self.reconcile_args()
        self.claim(766)
        with mock.patch.object(tasks_reconcile, "github_issue", return_value={"state": "OPEN", "assignees": []}):
            first = tasks_reconcile.reconcile(args)
            second = tasks_reconcile.reconcile(args)
        self.assertEqual(first["heartbeat"], {"sent": [766], "refused": []})
        self.assertEqual(second["heartbeat"]["sent"], [])
        self.assertEqual([entry["issue"] for entry in second["heartbeat"]["refused"]], [766])
        self.assertIn("300 seconds", second["heartbeat"]["refused"][0]["message"])
        token = Path(args.token_file).read_text().strip()
        page = tasks_reconcile.get(self.url + "/v1/executions/lane-test/events?after=0&limit=512", token)
        self.assertLessEqual(page["head"], 512, "test execution outgrew one page")
        heartbeats = [event for event in page["events"]
                      if event["kind"]["type"] == "signal_recorded"
                      and event["kind"]["data"].get("kind") == "task.heartbeat"]
        self.assertEqual([event["actor"]["id"] for event in heartbeats], ["codex-4"])

    def test_released_claim_is_recorded_once(self):
        args = self.reconcile_args()
        self.claim(766)
        with mock.patch.object(tasks_reconcile, "github_issue", return_value={"state": "CLOSED", "assignees": []}):
            self.assertEqual(tasks_reconcile.reconcile(args)["released"], [766])
            self.assertEqual(tasks_reconcile.reconcile(args)["released"], [])

    def test_reassigned_issue_releases_only_an_open_claim(self):
        args = type("Args", (), {"execution": "lane-test", "url": self.url,
                    "token_file": self.env["GRAPHHELM_TOKEN_FILE"],
                    "repo": "stabem/GraphHelm", "lane": "codex-4"})()
        claim = ("task.claimed", {"taskId": "issue-766", "lane": "codex-4",
                 "repo": "stabem/GraphHelm"}, {"sequence": 12})
        with mock.patch.object(tasks_reconcile, "records", return_value=[claim]), \
             mock.patch.object(tasks_reconcile, "github_issue", return_value={"state": "OPEN", "assignees": [{"login": "other-lane"}]}), \
             mock.patch.object(tasks_reconcile, "post_end") as post:
            self.assertEqual(tasks_reconcile.reconcile(args)["released"], [766])
        self.assertEqual(post.call_args.args[-1], {"lane": "codex-4", "claimSequence": 12,
                                                   "reason": "Issue reassigned"})

    def test_latest_release_closes_all_earlier_claims_for_same_issue(self):
        args = type("Args", (), {"execution": "lane-test", "url": self.url,
                    "token_file": self.env["GRAPHHELM_TOKEN_FILE"],
                    "repo": "stabem/GraphHelm", "lane": "codex-4"})()
        records = [("task.claimed", {"taskId": "issue-766", "lane": "codex-4",
                    "repo": "stabem/GraphHelm"}, {"sequence": n}) for n in (12, 14)]
        records.append(("task.released", {"taskId": "issue-766", "lane": "codex-4",
                        "repo": "stabem/GraphHelm", "claimSequence": 14}, {"sequence": 15}))
        with mock.patch.object(tasks_reconcile, "records", return_value=records), \
             mock.patch.object(tasks_reconcile, "github_issue", return_value={"state": "CLOSED", "assignees": []}), \
             mock.patch.object(tasks_reconcile, "post_end") as post:
            self.assertEqual(tasks_reconcile.reconcile(args)["released"], [])
        post.assert_not_called()

    def test_release_from_another_issue_does_not_hide_claim(self):
        args = type("Args", (), {"execution": "lane-test", "url": self.url,
                    "token_file": self.env["GRAPHHELM_TOKEN_FILE"],
                    "repo": "stabem/GraphHelm", "lane": "codex-4"})()
        records = [("task.claimed", {"taskId": "issue-766", "lane": "codex-4",
                    "repo": "stabem/GraphHelm"}, {"sequence": 12}),
                   ("task.released", {"taskId": "issue-999", "lane": "codex-4",
                    "repo": "stabem/GraphHelm", "claimSequence": 12}, {"sequence": 13})]
        with mock.patch.object(tasks_reconcile, "records", return_value=records), \
             mock.patch.object(tasks_reconcile, "github_issue", return_value={"state": "CLOSED", "assignees": []}), \
             mock.patch.object(tasks_reconcile, "post_end") as post:
            self.assertEqual(tasks_reconcile.reconcile(args)["released"], [766])
        self.assertEqual(post.call_args.args[-1]["claimSequence"], 12)

    def test_codex_lane_stop_hook_is_byte_identical(self):
        # #1181: the Codex companion carried its own lane_stop_hook.py (hard-coded host), so a fix
        # in one copy did not reach the other. Cost: two file reads.
        claude_copy = (HERE / "lane_stop_hook.py").read_bytes()
        codex_copy = (REPO / "plugins/graphhelm-codex-hooks/hooks/lane_stop_hook.py").read_bytes()
        self.assertEqual(claude_copy, codex_copy, "plugins/graphhelm-codex-hooks/hooks/lane_stop_hook.py differs")

    def test_stop_invokes_configured_cli_reconciliation_and_records_its_reply(self):
        # #1181: the reconciler's JSON was discarded; now a Stop records it as a `tasks.reconciled`
        # signal by the lane. On the parent no such event exists. Cost: one Stop, one event read.
        root = Path(self.tmp.name)
        subprocess.run(["git", "init", "-q", str(root)], check=True, capture_output=True, creationflags=NO_WINDOW)
        subprocess.run(["git", "-C", str(root), "remote", "add", "origin",
                        "git@github.com:stabem/GraphHelm.git"], check=True, capture_output=True, creationflags=NO_WINDOW)
        output = root / "cli-args.txt"
        script = root / "record_cli.py"
        reply = {"ok": True, "command": "tasks.reconcile",
                 "data": {"merged": [9001], "closed": [], "released": [], "heartbeat": {"sent": [766], "refused": []}, "inspected": 1}}
        script.write_text("import sys\nfrom pathlib import Path\n"
                          f"Path({str(output)!r}).open('a').write(' '.join(sys.argv[1:]) + '\\n')\n"
                          f"print({json.dumps(reply)!r})\n")
        if os.name == "nt":
            cli = root / "configured-graphhelm.cmd"
            cli.write_text(f'@echo off\r\n"{sys.executable}" "{script}" %*\r\n')
        else:
            cli = root / "configured-graphhelm"
            cli.write_text(f'#!/bin/sh\nexec "{sys.executable}" "{script}" "$@"\n')
            cli.chmod(0o755)
        self.env["GRAPHHELM_CLI"] = str(cli)
        result = subprocess.run([sys.executable, str(HERE / "lane_stop_hook.py")],
            input=json.dumps({"cwd": str(root), "session_id": "test-session",
                              "stop_hook_active": True}), text=True, capture_output=True,
            env=self.env, timeout=12, creationflags=NO_WINDOW)
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = output.read_text().splitlines()
        reconcile = next((call for call in calls if call.startswith("tasks reconcile ")), "")
        self.assertIn("tasks reconcile --execution lane-test", reconcile)
        self.assertIn("--lane codex-4", reconcile)
        self.assertIn("--repo stabem/GraphHelm", reconcile)
        token = Path(self.env["GRAPHHELM_TOKEN_FILE"]).read_text().strip()
        page = tasks_reconcile.get(self.url + "/v1/executions/lane-test/events?after=0&limit=512", token)
        recorded = [event for event in page["events"]
                    if event["kind"]["type"] == "signal_recorded"
                    and event["kind"]["data"].get("kind") == "tasks.reconciled"]
        self.assertEqual([event["actor"]["id"] for event in recorded], ["codex-4"])
        evidence_id = recorded[0]["evidenceRefs"][0]["evidenceId"]
        content = tasks_reconcile.get(self.url + f"/v1/executions/lane-test/evidence/{evidence_id}", token)["content"]
        self.assertEqual(json.loads(json.loads(content)["description"]), reply["data"])

    def stop(self, manifest, active=False, session="test-session"):
        # Drive the registered Stop commands, not an unregistered helper. On the parent
        # the Keel hook allows this repeated stop and the addressed note is lost.
        results = []
        manifest = HERE / manifest
        for group in json.loads(manifest.read_text())["hooks"].get("Stop", []):
            for hook in group["hooks"]:
                name = re.search(r"/hooks/([a-z_]+\.py)", hook["command"]).group(1)
                started = time.monotonic()
                args = ["--host=codex"] if "--host=codex" in hook["command"] else []
                result = subprocess.run([sys.executable, str(manifest.parent / name), *args], env=self.env,
                    input=json.dumps({"cwd": self.tmp.name, "session_id": session, "stop_hook_active": active}),
                    text=True, capture_output=True, timeout=12 * time_scale(), creationflags=NO_WINDOW)
                if name in ("lane_stop_hook.py", "stop_hook.py"):  # #1278: one process on Claude
                    self.reminder_seconds = time.monotonic() - started
                self.assertEqual(result.returncode, 0, result.stderr)
                if result.stdout.strip():
                    results.append(json.loads(result.stdout))
        return results

    def test_desktop_session_identity_resolves_from_team_binding(self):
        self.env.pop("GRAPHHELM_EXECUTION_ID")
        self.env.pop("GRAPHHELM_RUNTIME_URL")
        self.env.pop("GRAPHHELM_TOKEN_FILE")
        self.env.pop("GRAPHHELM_ACTOR")
        self.env.pop("GRAPHHELM_EVENTS_KEY", None)
        root = Path(self.tmp.name)
        repo = root / "repo"
        (repo / ".graphhelm").mkdir(parents=True)
        (repo / ".graphhelm/team.json").write_text(json.dumps({
            "executionId": "lane-test", "runtimeUrl": self.url,
            "tokenFile": str(root / "events.agent.token")}))
        home = root / "home"
        self.env["HOME"] = str(home)
        self.env["USERPROFILE"] = str(home)
        self.env["GRAPHHELM_EVENTS_KEY"] = "12" * 32
        self.note("request-desktop", to="codex-4")
        start = {"hook_event_name": "SessionStart", "session_id": "session-desktop",
                 "session_title": "codex-4 - journeys", "cwd": str(repo)}
        start_result = subprocess.run([sys.executable, str(HERE / "session_hook.py"),
            "start", "--host", "claude"], input=json.dumps(start), text=True,
            capture_output=True, env=self.env, timeout=12, creationflags=NO_WINDOW)
        self.assertEqual(start_result.returncode, 0, start_result.stderr)
        lane_file = home / ".graphhelm/lane-sessions/session-desktop.json"
        self.assertTrue(lane_file.exists())
        self.assertEqual(json.loads(lane_file.read_text())["actor"], "codex-4")
        stop_result = subprocess.run([sys.executable, str(HERE / "lane_stop_hook.py")],
            input=json.dumps({"session_id": "session-desktop", "cwd": str(repo)}),
            text=True, capture_output=True, env=self.env, timeout=12, creationflags=NO_WINDOW)
        self.assertEqual(stop_result.returncode, 0, stop_result.stderr)
        self.assertEqual(json.loads(stop_result.stdout)["decision"], "block")

        # Session identity is scoped to the session ID even in the same checkout.
        self.note("request-other", to="codex-4")
        other_start = {**start, "session_id": "session-other", "session_title": "codex-5"}
        subprocess.run([sys.executable, str(HERE / "session_hook.py"), "start", "--host", "claude"],
            input=json.dumps(other_start), text=True, capture_output=True, env=self.env,
            timeout=12, check=True, creationflags=NO_WINDOW)
        other_stop = subprocess.run([sys.executable, str(HERE / "lane_stop_hook.py")],
            input=json.dumps({"session_id": "session-other", "cwd": str(repo)}),
            text=True, capture_output=True, env=self.env, timeout=12, check=True, creationflags=NO_WINDOW)
        self.assertEqual(other_stop.stdout, "")

        # Invalid titles write no lane identity; absent or incomplete team binding allows.
        invalid_start = {**start, "session_id": "invalid-title", "session_title": "free text title"}
        subprocess.run([sys.executable, str(HERE / "session_hook.py"), "start", "--host", "claude"],
            input=json.dumps(invalid_start), text=True, capture_output=True, env=self.env,
            timeout=12, check=True, creationflags=NO_WINDOW)
        self.assertFalse((home / ".graphhelm/lane-sessions/invalid-title.json").exists())
        no_identity = subprocess.run([sys.executable, str(HERE / "lane_stop_hook.py")],
            input=json.dumps({"session_id": "invalid-title", "cwd": str(repo)}),
            text=True, capture_output=True, env=self.env, timeout=12, check=True, creationflags=NO_WINDOW)
        self.assertEqual(no_identity.stdout, "")
        (home / ".graphhelm/lane-sessions/old-session.json").write_text(json.dumps({"actor": "codex-4"}))
        old_time = time.time() - 8 * 24 * 60 * 60
        os.utime(home / ".graphhelm/lane-sessions/old-session.json", (old_time, old_time))
        subprocess.run([sys.executable, str(HERE / "session_hook.py"), "start", "--host", "claude"],
            input=json.dumps({**start, "session_id": "prune-trigger", "session_title": "codex-4"}),
            text=True, capture_output=True, env=self.env, timeout=12, check=True, creationflags=NO_WINDOW)
        self.assertFalse((home / ".graphhelm/lane-sessions/old-session.json").exists())
        (repo / ".graphhelm/team.json").unlink()
        missing_team = subprocess.run([sys.executable, str(HERE / "lane_stop_hook.py")],
            input=json.dumps({"session_id": "session-desktop", "cwd": str(repo)}),
            text=True, capture_output=True, env=self.env, timeout=12, check=True, creationflags=NO_WINDOW)
        self.assertEqual(missing_team.stdout, "")

    def test_session_end_removes_desktop_lane_identity(self):
        root = Path(self.tmp.name)
        home = root / "home"
        self.env["HOME"] = str(home)
        self.env["USERPROFILE"] = str(home)
        self.env.pop("GRAPHHELM_ACTOR", None)
        self.env.pop("GRAPHHELM_EXECUTION_ID", None)
        self.env.pop("GRAPHHELM_RUNTIME_URL", None)
        self.env.pop("GRAPHHELM_TOKEN_FILE", None)
        self.env.pop("GRAPHHELM_EVENTS_KEY", None)
        payload = {"hook_event_name": "SessionStart", "session_id": "ended-session",
                   "session_title": "codex-4", "cwd": str(root)}
        start = subprocess.run([sys.executable, str(HERE / "session_hook.py"), "start", "--host", "claude"],
            input=json.dumps(payload), text=True, capture_output=True, env=self.env, timeout=12, creationflags=NO_WINDOW)
        self.assertEqual(start.returncode, 0, start.stderr)
        lane_file = home / ".graphhelm/lane-sessions/ended-session.json"
        self.assertTrue(lane_file.exists())
        end = subprocess.run([sys.executable, str(HERE / "session_hook.py"), "end", "--host", "claude"],
            input=json.dumps({"hook_event_name": "SessionEnd", "session_id": "ended-session"}),
            text=True, capture_output=True, env=self.env, timeout=12, creationflags=NO_WINDOW)
        self.assertEqual(end.returncode, 0, end.stderr)
        self.assertFalse(lane_file.exists())

    def test_env_binding_wins_over_desktop_session_identity(self):
        self.env.pop("GRAPHHELM_EVENTS_KEY", None)
        root = Path(self.tmp.name)
        home = root / "home"
        self.env["HOME"] = str(home)
        self.env["USERPROFILE"] = str(home)
        self.env.pop("GRAPHHELM_EXECUTION_ID", None)
        self.env.pop("GRAPHHELM_RUNTIME_URL", None)
        self.env.pop("GRAPHHELM_TOKEN_FILE", None)
        self.env["GRAPHHELM_ACTOR"] = "codex-5"
        (home / ".graphhelm/lane-sessions").mkdir(parents=True)
        (home / ".graphhelm/lane-sessions/env-wins.json").write_text(
            json.dumps({"actor": "codex-4", "at": "2026-10-10T00:00:00Z"}))
        self.env.update(GRAPHHELM_EXECUTION_ID="lane-test", GRAPHHELM_RUNTIME_URL=self.url,
                        GRAPHHELM_TOKEN_FILE=str(root / "events.agent.token"))
        self.note("env-wins-note", to="codex-5")
        result = subprocess.run([sys.executable, str(HERE / "lane_stop_hook.py")],
            input=json.dumps({"session_id": "env-wins", "cwd": str(root)}), text=True,
            capture_output=True, env=self.env, timeout=12, creationflags=NO_WINDOW)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["decision"], "block")

    def test_addressed_pending_note_blocks_once_then_allows_reentry(self):
        self.note("request-1", to="codex-4")
        for manifest in MANIFESTS:
            with self.subTest(manifest=manifest):
                decisions = self.stop(manifest)
                self.assertTrue(any(d.get("decision") == "block" for d in decisions), decisions)
                self.assertIn("read the notes addressed to you", decisions[-1]["reason"])
                self.assertEqual(self.stop(manifest, active=True), [])

    def test_slow_event_pages_still_remind_at_stop(self):
        """A 1.5 s head and page read must fit the Stop scan budget."""
        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                if "/events?" not in self.path:
                    self.send_error(404)
                    return
                time.sleep(1.5)
                events = [] if "limit=1" in self.path else [{
                    "sequence": 1, "actor": {"type": "owner", "id": "owner"},
                    "kind": {"type": "signal_recorded", "data": {
                        "kind": "operator_note", "executionId": "lane-test",
                        "signalId": "slow-request", "to": "codex-4"}}}]
                body = json.dumps({"ok": True, "data": {"head": 1, "events": events}}).encode()
                self.send_response(200)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, *_args):
                pass

        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        self.env["GRAPHHELM_RUNTIME_URL"] = f"http://127.0.0.1:{server.server_port}"
        for manifest in (HERE / "codex-hooks.json", MANIFESTS[2]):
            with self.subTest(manifest=manifest):
                decisions = self.stop(manifest)
                self.assertTrue(decisions, "slow addressed note was missed")
                self.assertEqual(decisions[-1]["decision"], "block")

    def test_non_github_origins_still_remind_about_unread_notes(self):
        self.note("origin-note", to="codex-4")
        root = Path(self.tmp.name)
        subprocess.run(["git", "init", "-q", str(root)], check=True, capture_output=True, creationflags=NO_WINDOW)
        for remote in ("ssh://git@github.com/stabem/GraphHelm.git", "https://gitlab.com/a/b.git"):
            with self.subTest(remote=remote):
                subprocess.run(["git", "-C", str(root), "remote", "remove", "origin"], capture_output=True, creationflags=NO_WINDOW)
                subprocess.run(["git", "-C", str(root), "remote", "add", "origin", remote], check=True, creationflags=NO_WINDOW)
                for manifest in MANIFESTS:
                    with self.subTest(manifest=manifest):
                        decisions = self.stop(manifest)
                        self.assertTrue(any(d.get("decision") == "block" for d in decisions), decisions)

    def test_runtime_down_allows_stop(self):
        self.stop_server()
        for manifest in MANIFESTS:
            with self.subTest(manifest=manifest):
                self.assertEqual(self.stop(manifest), [])

    def test_tail_window_and_unreadable_stream(self):
        # HTTP I/O fixture: an append-only execution past the scan cap, malformed
        # JSON, or a stalled peer. No bulk writes or load on any Runtime.
        mode = "long"
        calls = []
        content = json.dumps({"id": "recent-note", "type": "operator_note", "to": "codex-4"})
        digest = hashlib.sha256(content.encode()).hexdigest()

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                from urllib.parse import parse_qs, urlparse
                calls.append(self.path)
                if mode == "busy":
                    time.sleep(0.05)  # measured team HTTP latency, not CPU load
                if mode == "stall":
                    time.sleep(4)
                    return
                if mode == "invalid":
                    body = b"not json"
                elif "/evidence/" in self.path:
                    evidence_id = self.path.rsplit("/", 1)[-1]
                    envelope = content
                    if mode == "busy":
                        envelope = json.dumps({"id": evidence_id, "type": "operator_note",
                            "to": "codex-4" if evidence_id == "note-4097" else "codex-5"})
                    body = json.dumps({"ok": True, "data": {"evidenceId": evidence_id,
                        "mediaType": "application/json", "content": envelope,
                        "contentSha256": hashlib.sha256(envelope.encode()).hexdigest()}}).encode()
                else:
                    after = int(parse_qs(urlparse(self.path).query)["after"][0])
                    limit = int(parse_qs(urlparse(self.path).query)["limit"][0])
                    events = [
                        {"sequence": n, "kind": {"type": "execution_started"}}
                        for n in range(after + 1, min(after + limit, 4097) + 1)]
                    if mode == "busy":
                        for event in events:
                            n = event["sequence"]
                            if n > 3797:
                                to = "codex-4" if n == 4097 else "codex-5"
                                envelope = json.dumps({"id": f"note-{n}", "type": "operator_note", "to": to})
                                note_digest = hashlib.sha256(envelope.encode()).hexdigest()
                                event["scope"] = {"projectId": "test", "executionId": "lane-test"}
                                event["correlationId"] = "x" * 128
                                event["causationId"] = "y" * 128
                                event["idempotencyKey"] = "z" * 128
                                event.update(kind={"type": "signal_recorded", "data": {
                                    "kind": "operator_note", "executionId": "lane-test",
                                    "signalId": f"note-{n}", "to": to,
                                    "envelopeSha256": note_digest}}, evidenceRefs=[{
                                        "evidenceId": f"note-{n}", "contentSha256": note_digest}])
                    if mode == "recent" and events and events[-1]["sequence"] == 4097:
                        events[-1] = {"sequence": 4097, "kind": {"type": "signal_recorded",
                            "data": {"kind": "operator_note", "executionId": "lane-test",
                                     "signalId": "recent-note", "to": "codex-4", "envelopeSha256": digest}},
                            "evidenceRefs": [{"evidenceId": "recent-evidence", "contentSha256": digest}]}
                    body = json.dumps({"ok": True, "data": {"head": 4097, "events": events}}).encode()
                    if mode == "busy" and events and events[-1]["sequence"] == 4097:
                        assert len(body) > 256 * 1024  # realistic page exceeds the session-hook cap
                try:
                    self.send_response(200)
                    self.end_headers()
                    self.wfile.write(body)
                except ConnectionError:
                    pass  # expected when the bounded child times out

            def log_message(self, *_args):
                pass

        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        self.env["GRAPHHELM_RUNTIME_URL"] = f"http://127.0.0.1:{server.server_port}"
        # #1199: the span below is the Stop hook's reminder path (waiter arm + the scan child's
        # event pages at 50 ms each; the cwd has no git remote, so no reconcile runs). 2.0 s is
        # its budget on an idle machine; it is reported below, and the assertion is the GET count.
        budget = 2.0 * time_scale()
        for mode in ("busy", "recent", "long", "invalid", "stall"):
            for manifest in MANIFESTS:
                with self.subTest(mode=mode, manifest=manifest):
                    if mode in ("busy", "recent"):
                        calls.clear()
                        decisions = self.stop(manifest)
                        elapsed = self.reminder_seconds
                        self.assertTrue(any(d.get("decision") == "block" for d in decisions), decisions)
                        if mode == "busy":
                            # #1459: the promise is the tail window (a bounded number of event
                            # pages), which a count observes; the seconds are only reported, since
                            # a loaded host stretches them without the hook reading one page more.
                            self.assertLessEqual(len(calls), 12, calls)
                            self.assertFalse(any("/evidence/" in call for call in calls), calls)
                            print(f"300 notes: {manifest.name}: {elapsed:.3f}s of {budget:.1f}s, {len(calls)} event GETs", flush=True)
                        self.assertEqual(self.stop(manifest, active=True), [])
                    else:
                        self.assertEqual(self.stop(manifest), [])

    def test_no_addressed_note_allows_stop(self):
        self.note("other-1", to="codex-5")
        self.note("room-1")
        self.assertEqual(self.stop(MANIFESTS[0]), [])
        request = urllib.request.Request(self.url + "/v1/executions/lane-test",
                  headers={"Authorization": "Bearer " + self.owner_token})
        with urllib.request.urlopen(request, timeout=5) as response:
            self.assertIn("codex-4", json.load(response)["data"]["wakeListeners"])

    def test_session_waiter_survives_unrelated_append_and_catches_ask(self):
        session = "session-wake-test"
        script = HERE / "session_hook.py"
        payload = {"hook_event_name": "SessionStart", "session_id": session}
        result = subprocess.run([sys.executable, str(script), "start", "--host", "codex"],
                                input=json.dumps(payload), text=True, env=self.env,
                                capture_output=True, timeout=8, creationflags=NO_WINDOW)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("UNOBSERVED", result.stdout)
        pid_files = list(Path(self.env.get("GRAPHHELM_HOOK_STATE_DIR", "")).glob("*.waiter.pid"))
        self.assertEqual(len(pid_files), 1)
        pid = int(pid_files[0].read_text())
        def stop_waiter():
            self.stop_waiter_pid(pid)
        self.addCleanup(stop_waiter)
        self.assertTrue(session_hook._pid_alive(pid), "wake waiter exited before an append")
        again = subprocess.run([sys.executable, str(script), "start", "--host", "codex"],
                               input=json.dumps(payload), text=True, env=self.env,
                               capture_output=True, timeout=8, creationflags=NO_WINDOW)
        self.assertEqual(again.returncode, 0, again.stderr)
        self.assertEqual(int(pid_files[0].read_text()), pid)
        self.note("other-2", to="codex-5")
        self.assertTrue(session_hook._pid_alive(pid), "wake waiter exited after unrelated append")
        request = urllib.request.Request(self.url + "/v1/executions/lane-test",
                  headers={"Authorization": "Bearer " + self.owner_token})
        with urllib.request.urlopen(request, timeout=5) as response:
            self.assertIn("codex-4", json.load(response)["data"]["wakeListeners"])
        # Repeated unrelated rings must leave a listener, including after a
        # waiter exits and Stop has to replace it for this same session.
        self.stop_waiter_pid(pid)
        deadline = time.monotonic() + 3
        while session_hook._pid_alive(pid) and time.monotonic() < deadline:
            time.sleep(0.05)
        self.assertFalse(session_hook._pid_alive(pid))
        self.assertEqual(self.stop(MANIFESTS[1], session=session), [])
        replacement = int(pid_files[0].read_text())
        self.assertNotEqual(replacement, pid)
        self.addCleanup(lambda: self.stop_waiter_pid(replacement))
        for cycle in range(3):
            self.note(f"other-cycle-{cycle}", to="codex-5")
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                with urllib.request.urlopen(request, timeout=5) as response:
                    if "codex-4" in json.load(response)["data"]["wakeListeners"]:
                        break
                time.sleep(0.05)
            else:
                self.fail(f"lane lost its wake listener after cycle {cycle}")
        self.note("request-2", to="codex-4")
        self.assertTrue(any(d.get("decision") == "block" for d in self.stop(MANIFESTS[0])))

    def test_shell_launched_waiter_survives_parent_and_stale_heartbeat_until_session_end(self):
        """A quiet live session stays armed; SessionEnd, not heartbeat age, stops its waiter."""
        script = HERE / "session_hook.py"
        payload = json.dumps({"hook_event_name": "SessionStart", "session_id": "shell-host"})
        for shell in (["cmd", "/c"] if os.name == "nt" else ["bash", "-c"] for _ in (0,)):
            command = subprocess.list2cmdline([sys.executable, str(script), "start", "--host", "codex"])
            host_code = ("import subprocess,sys,time; "
                         "p=subprocess.run(sys.argv[1:3]+[sys.argv[3]],input=sys.argv[4],text=True,"
                         "capture_output=True,timeout=8); "
                         "print(p.returncode, flush=True); time.sleep(30)")
            host = subprocess.Popen([sys.executable, "-c", host_code, *shell, command, payload],
                                    env=self.env, stdout=subprocess.PIPE, text=True,
                                    creationflags=NO_WINDOW)
            self.addCleanup(lambda p=host: p.kill() if p.poll() is None else None)
            self.addCleanup(host.stdout.close)
            self.assertEqual(host.stdout.readline().strip(), "0")
            pid_files = list(Path(self.env["GRAPHHELM_HOOK_STATE_DIR"]).glob("*.waiter.pid"))
            self.assertEqual(len(pid_files), 1)
            pid = int(pid_files[0].read_text())
            self.addCleanup(lambda p=pid: self.stop_waiter_pid(p))
            self.assertTrue(session_hook._pid_alive(pid), "waiter died with hook shell")
            host.kill()
            host.wait(timeout=5)
            self.note("host-exit-wake", to="codex-5")
            time.sleep(1)
            self.assertTrue(session_hook._pid_alive(pid), "waiter died with launcher")
            heartbeat = pid_files[0].with_suffix(".alive")
            os.utime(heartbeat, (time.time() - 1900, time.time() - 1900))
            self.note("heartbeat-aged-wake", to="codex-5")
            request = urllib.request.Request(self.url + "/v1/executions/lane-test",
                      headers={"Authorization": "Bearer " + self.owner_token})
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                with urllib.request.urlopen(request, timeout=5) as response:
                    if "codex-4" in json.load(response)["data"]["wakeListeners"]:
                        break
                time.sleep(0.05)
            else:
                self.fail("live lane lost its wake listener after heartbeat age")
            self.assertTrue(session_hook._pid_alive(pid), "waiter stopped because heartbeat aged")

            ended = subprocess.run([sys.executable, str(script), "end", "--host", "codex"],
                                   input=json.dumps({"hook_event_name": "SessionEnd", "session_id": "shell-host"}),
                                   text=True, env=self.env, capture_output=True, timeout=8,
                                   creationflags=NO_WINDOW)
            self.assertEqual(ended.returncode, 0, ended.stderr)
            deadline = time.monotonic() + 5
            while session_hook._pid_alive(pid) and time.monotonic() < deadline:
                time.sleep(0.05)
            self.assertFalse(session_hook._pid_alive(pid), "SessionEnd did not stop waiter")

    def test_only_own_reply_clears_pending_note(self):
        self.note("request-1", to="codex-4")
        self.note("wrong-reply", reply="request-1", actor="codex-5")
        self.assertTrue(any(d.get("decision") == "block" for d in self.stop("hooks.json")))
        self.note("own-reply", reply="request-1", actor="codex-4")
        self.assertEqual(self.stop("hooks.json"), [])
        self.note("request-2", to="codex-4")
        self.assertEqual(self.stop("hooks.json")[-1]["decision"], "block")


if __name__ == "__main__":
    unittest.main()
