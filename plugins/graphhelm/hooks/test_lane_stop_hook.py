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
MANIFESTS = (HERE / "hooks.json", HERE / "codex-hooks.json",
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
                    body = json.dumps({"ok": True, "data": {"evidenceId": "recent-evidence",
                        "mediaType": "application/json", "content": content,
                        "contentSha256": digest}}).encode()
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
                                event.update(kind={"type": "signal_recorded", "data": {
                                    "kind": "operator_note", "executionId": "lane-test",
                                    "signalId": f"note-{n}", "to": "codex-4" if n == 4097 else "codex-5",
                                    "envelopeSha256": digest}}, evidenceRefs=[{
                                        "evidenceId": "recent-evidence", "contentSha256": digest}])
                    if mode == "recent" and events and events[-1]["sequence"] == 4097:
                        events[-1] = {"sequence": 4097, "kind": {"type": "signal_recorded",
                            "data": {"kind": "operator_note", "executionId": "lane-test",
                                     "signalId": "recent-note", "to": "codex-4", "envelopeSha256": digest}},
                            "evidenceRefs": [{"evidenceId": "recent-evidence", "contentSha256": digest}]}
                    body = json.dumps({"ok": True, "data": {"head": 4097, "events": events}}).encode()
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
        self.assertTrue(any(d.get("decision") == "block" for d in self.stop("hooks.json")))
        self.note("own-reply", reply="request-1", actor="codex-4")
        self.assertEqual(self.stop("hooks.json"), [])
        self.note("request-2", to="codex-4")
        self.assertEqual(self.stop("hooks.json")[-1]["decision"], "block")


if __name__ == "__main__":
    unittest.main()
