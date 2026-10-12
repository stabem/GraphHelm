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


class LaneStopTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = Path(self.tmp.name)
        self.env = {**os.environ, "GRAPHHELM_EVENTS_KEY": "12" * 32,
                    "GRAPHHELM_EXECUTION_ID": "lane-test", "GRAPHHELM_ACTOR": "codex-4"}
        self.cli = os.environ.get("GRAPHHELM_CLI") or shutil.which("graphhelm")
        self.assertIsNotNone(self.cli, "installed graphhelm is required")
        keyring = root / "keyring"
        keyring.mkdir()
        self.run_cli("gateway", "keyring", "init", "--keyring", str(keyring), "--key-id", "test")
        events = root / "events"
        self.env["GRAPHHELM_EVENTS_DIR"] = str(events)
        self.env["GRAPHHELM_CLI"] = self.cli
        self.env["GRAPHHELM_HOOK_STATE_DIR"] = str(root / "hook-state")
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
        self.env.pop("GRAPHHELM_SESSION_ID", None)

    def stop_server(self):
        self.server.terminate()
        self.server.wait(timeout=10)

    def stop_waiters(self):
        """Stop only waiter processes recorded in this test's disposable state directory."""
        state = Path(self.env["GRAPHHELM_HOOK_STATE_DIR"])
        for pid_file in state.glob("*.waiter.pid"):
            try:
                pid = int(pid_file.read_text())
                if session_hook._pid_alive(pid):
                    os.kill(pid, signal.SIGTERM)
                deadline = time.monotonic() + 2
                while session_hook._pid_alive(pid) and time.monotonic() < deadline:
                    time.sleep(0.05)
                pid_file.unlink(missing_ok=True)
            except (OSError, ValueError):
                pass

    def run_cli(self, *args):
        result = subprocess.run([self.cli, *args], env=self.env, capture_output=True, text=True, timeout=15, creationflags=NO_WINDOW)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

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
        token = Path(self.env["GRAPHHELM_TOKEN_FILE"]).read_text().strip()
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
        token = Path(args.token_file).read_text().strip()
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

    def test_released_claim_is_recorded_once(self):
        args = type("Args", (), {"execution": "lane-test", "url": self.url,
                    "token_file": self.env["GRAPHHELM_TOKEN_FILE"],
                    "repo": "stabem/GraphHelm", "lane": "codex-4"})()
        now = "2026-10-10T00:00:00Z"
        doc = {"schema": "graphhelm-task-event-v1", "taskId": "issue-766", "revision": 1,
               "at": now, "lane": "codex-4", "branch": "issue-766-test", "repo": "stabem/GraphHelm"}
        doc["issue"] = 766
        token = Path(args.token_file).read_text().strip()
        signal = {"id": "claim-766", "source": {"type": "user", "id": "codex-4"},
                  "type": "task.claimed", "severity": "low", "description": json.dumps(doc),
                  "evidence": ["https://github.com/stabem/GraphHelm/issues/766"], "emittedAt": now}
        req = urllib.request.Request(self.url + "/v1/executions/lane-test/signal",
            data=json.dumps({"signal": signal}).encode(), headers={"Authorization": "Bearer " + token,
            "Content-Type": "application/json", "Idempotency-Key": "claim-766",
            "X-GraphHelm-Actor": "codex-4", "X-GraphHelm-Actor-Type": "agent"})
        try:
            response = urllib.request.urlopen(req, timeout=5)
        except urllib.error.HTTPError as error:
            self.fail(f"claim request failed: HTTP {error.code}: {error.read().decode('utf-8', 'replace')}")
        with response:
            answer = json.load(response)
            self.assertTrue(answer["ok"], answer)
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

    def test_stop_invokes_configured_cli_reconciliation(self):
        root = Path(self.tmp.name)
        subprocess.run(["git", "init", "-q", str(root)], check=True, capture_output=True, creationflags=NO_WINDOW)
        subprocess.run(["git", "-C", str(root), "remote", "add", "origin",
                        "git@github.com:stabem/GraphHelm.git"], check=True, capture_output=True, creationflags=NO_WINDOW)
        output = root / "cli-args.txt"
        script = root / "record_cli.py"
        script.write_text("import sys\nfrom pathlib import Path\n"
                          f"Path({str(output)!r}).open('a').write(' '.join(sys.argv[1:]) + '\\n')\n")
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
                    text=True, capture_output=True, timeout=12, creationflags=NO_WINDOW)
                if name == "lane_stop_hook.py":
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
        self.env.pop("GRAPHHELM_HOOK_STATE_DIR", None)
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
        for mode in ("busy", "recent", "long", "invalid", "stall"):
            for manifest in MANIFESTS:
                with self.subTest(mode=mode, manifest=manifest):
                    if mode in ("busy", "recent"):
                        calls.clear()
                        decisions = self.stop(manifest)
                        elapsed = self.reminder_seconds
                        self.assertTrue(any(d.get("decision") == "block" for d in decisions), decisions)
                        if mode == "busy":
                            self.assertLess(elapsed, 2.0, f"300-note reminder took {elapsed:.3f}s")
                            self.assertFalse(any("/evidence/" in call for call in calls), calls)
                            print(f"300 notes: {manifest.name}: {elapsed:.3f}s, {len(calls)} event GETs", flush=True)
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
            try:
                os.kill(pid, signal.SIGTERM)
            except OSError:
                pass
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
        os.kill(pid, signal.SIGTERM)
        deadline = time.monotonic() + 3
        while session_hook._pid_alive(pid) and time.monotonic() < deadline:
            time.sleep(0.05)
        self.assertFalse(session_hook._pid_alive(pid))
        self.assertEqual(self.stop(MANIFESTS[1], session=session), [])
        replacement = int(pid_files[0].read_text())
        self.assertNotEqual(replacement, pid)
        self.addCleanup(lambda: os.kill(replacement, signal.SIGTERM) if session_hook._pid_alive(replacement) else None)
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

    def test_shell_launched_waiter_survives_parent_and_expires_without_heartbeat(self):
        """A dead launcher cannot end a live session; stale activity must end it."""
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
            self.addCleanup(lambda p=pid: os.kill(p, signal.SIGTERM) if session_hook._pid_alive(p) else None)
            self.assertTrue(session_hook._pid_alive(pid), "waiter died with hook shell")
            host.kill()
            host.wait(timeout=5)
            self.note("host-exit-wake", to="codex-5")
            time.sleep(1)
            self.assertTrue(session_hook._pid_alive(pid), "waiter died with launcher")
            heartbeat = pid_files[0].with_suffix(".alive")
            os.utime(heartbeat, (time.time() - 1900, time.time() - 1900))
            self.note("heartbeat-expired", to="codex-5")
            deadline = time.monotonic() + 65
            while session_hook._pid_alive(pid) and time.monotonic() < deadline:
                time.sleep(0.05)
            self.assertFalse(session_hook._pid_alive(pid), "waiter outlived heartbeat")
            pid_files[0].unlink(missing_ok=True)
            self.assertEqual(self.stop(MANIFESTS[1], session="shell-host"), [])
            replacement = int(pid_files[0].read_text())
            self.assertNotEqual(replacement, pid)
            self.addCleanup(lambda p=replacement: os.kill(p, signal.SIGTERM) if session_hook._pid_alive(p) else None)

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
