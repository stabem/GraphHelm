"""Stop contract against a real Runtime with a disposable sealed event store.

Cost: a few seconds, installed graphhelm, one loopback port; no builds or real Runtime.
"""
import hashlib
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time
import threading
import unittest
import urllib.request

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
MANIFESTS = (HERE / "codex-hooks.json",
             REPO / "plugins/graphhelm-codex-hooks/hooks/codex-hooks.json")


class LaneStopTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = Path(self.tmp.name)
        self.env = {**os.environ, "GRAPHHELM_EVENTS_KEY": "12" * 32,
                    "GRAPHHELM_EXECUTION_ID": "lane-test", "GRAPHHELM_ACTOR": "codex-4"}
        self.cli = shutil.which("graphhelm")
        self.assertIsNotNone(self.cli, "installed graphhelm is required")
        keyring = root / "keyring"
        keyring.mkdir()
        self.run_cli("gateway", "keyring", "init", "--keyring", str(keyring), "--key-id", "test")
        events = root / "events"
        self.run_cli("execution", "start", "--file", str(REPO / "examples/graphs/software-feature.yaml"),
                     "--events", str(events), "--execution", "lane-test", "--mode", "manual", "--held")
        self.log = (root / "runtime.log").open("w+")
        self.addCleanup(self.log.close)
        self.server = subprocess.Popen([self.cli, "serve", "--events", str(events), "--bind", "127.0.0.1:0",
                                        "--keyring", str(keyring), "--key-id", "test"],
                                       env=self.env, stdout=self.log, stderr=subprocess.STDOUT)
        self.addCleanup(self.stop_server)
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

    def run_cli(self, *args):
        result = subprocess.run([self.cli, *args], env=self.env, capture_output=True, text=True, timeout=15)
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

    def stop(self, manifest, active=False):
        # Drive the registered Stop commands, not an unregistered helper. On the parent
        # the Keel hook allows this repeated stop and the addressed note is lost.
        results = []
        manifest = HERE / manifest
        for group in json.loads(manifest.read_text())["hooks"].get("Stop", []):
            for hook in group["hooks"]:
                name = re.search(r"/hooks/([a-z_]+\.py)", hook["command"]).group(1)
                started = time.monotonic()
                result = subprocess.run([sys.executable, str(manifest.parent / name)], env=self.env,
                    input=json.dumps({"cwd": self.tmp.name, "session_id": "test-session", "stop_hook_active": active}),
                    text=True, capture_output=True, timeout=12)
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
                 "session_title": "codex-4", "cwd": str(repo)}
        start_result = subprocess.run([sys.executable, str(HERE / "session_hook.py"),
            "start", "--host", "claude"], input=json.dumps(start), text=True,
            capture_output=True, env=self.env, timeout=12)
        self.assertEqual(start_result.returncode, 0, start_result.stderr)
        lane_file = home / ".graphhelm/lane-sessions/session-desktop.json"
        self.assertTrue(lane_file.exists())
        stop_result = subprocess.run([sys.executable, str(HERE / "lane_stop_hook.py")],
            input=json.dumps({"session_id": "session-desktop", "cwd": str(repo)}),
            text=True, capture_output=True, env=self.env, timeout=12)
        self.assertEqual(stop_result.returncode, 0, stop_result.stderr)
        self.assertEqual(json.loads(stop_result.stdout)["decision"], "block")

        # Session identity is scoped to the session ID even in the same checkout.
        self.note("request-other", to="codex-4")
        other_start = {**start, "session_id": "session-other", "session_title": "codex-5"}
        subprocess.run([sys.executable, str(HERE / "session_hook.py"), "start", "--host", "claude"],
            input=json.dumps(other_start), text=True, capture_output=True, env=self.env,
            timeout=12, check=True)
        other_stop = subprocess.run([sys.executable, str(HERE / "lane_stop_hook.py")],
            input=json.dumps({"session_id": "session-other", "cwd": str(repo)}),
            text=True, capture_output=True, env=self.env, timeout=12, check=True)
        self.assertEqual(other_stop.stdout, "")

        # Invalid titles write no lane identity; absent or incomplete team binding allows.
        invalid_start = {**start, "session_id": "invalid-title", "session_title": "free text title"}
        subprocess.run([sys.executable, str(HERE / "session_hook.py"), "start", "--host", "claude"],
            input=json.dumps(invalid_start), text=True, capture_output=True, env=self.env,
            timeout=12, check=True)
        self.assertFalse((home / ".graphhelm/lane-sessions/invalid-title.json").exists())
        no_identity = subprocess.run([sys.executable, str(HERE / "lane_stop_hook.py")],
            input=json.dumps({"session_id": "invalid-title", "cwd": str(repo)}),
            text=True, capture_output=True, env=self.env, timeout=12, check=True)
        self.assertEqual(no_identity.stdout, "")
        (home / ".graphhelm/lane-sessions/old-session.json").write_text(json.dumps({"actor": "codex-4"}))
        old_time = time.time() - 8 * 24 * 60 * 60
        os.utime(home / ".graphhelm/lane-sessions/old-session.json", (old_time, old_time))
        subprocess.run([sys.executable, str(HERE / "session_hook.py"), "start", "--host", "claude"],
            input=json.dumps({**start, "session_id": "prune-trigger", "session_title": "codex-4"}),
            text=True, capture_output=True, env=self.env, timeout=12, check=True)
        self.assertFalse((home / ".graphhelm/lane-sessions/old-session.json").exists())
        (repo / ".graphhelm/team.json").unlink()
        missing_team = subprocess.run([sys.executable, str(HERE / "lane_stop_hook.py")],
            input=json.dumps({"session_id": "session-desktop", "cwd": str(repo)}),
            text=True, capture_output=True, env=self.env, timeout=12, check=True)
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
            input=json.dumps(payload), text=True, capture_output=True, env=self.env, timeout=12)
        self.assertEqual(start.returncode, 0, start.stderr)
        lane_file = home / ".graphhelm/lane-sessions/ended-session.json"
        self.assertTrue(lane_file.exists())
        end = subprocess.run([sys.executable, str(HERE / "session_hook.py"), "end", "--host", "claude"],
            input=json.dumps({"hook_event_name": "SessionEnd", "session_id": "ended-session"}),
            text=True, capture_output=True, env=self.env, timeout=12)
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
            capture_output=True, env=self.env, timeout=12)
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
        for manifest in MANIFESTS:
            self.assertEqual(self.stop(manifest), [])

    def test_only_own_reply_clears_pending_note(self):
        self.note("request-1", to="codex-4")
        self.note("wrong-reply", reply="request-1", actor="codex-5")
        self.assertTrue(any(d.get("decision") == "block" for d in self.stop("codex-hooks.json")))
        self.note("own-reply", reply="request-1", actor="codex-4")
        self.assertEqual(self.stop("codex-hooks.json"), [])
        self.note("request-2", to="codex-4")
        self.assertEqual(self.stop("codex-hooks.json")[-1]["decision"], "block")


if __name__ == "__main__":
    unittest.main()
