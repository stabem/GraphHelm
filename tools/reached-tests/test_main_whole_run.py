"""Contracts for main_whole_run.py: when a run is due and what a red receipt says; no git, Runtime or cargo."""
import json
import http.server
import threading
import os
import tempfile
import sys
import subprocess
import datetime
import unittest
from types import SimpleNamespace
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import main_whole_run as mw
import run_reached as runner


class WholeRun(unittest.TestCase):
    def test_nightly_key_is_the_utc_calendar_date(self):
        moment = datetime.datetime(2026, 11, 4, 22, 15, tzinfo=datetime.timezone.utc)
        self.assertEqual(mw.night_date(moment), "2026-11-04")

    def test_nightly_lock_refuses_a_second_process_lock(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "nightly.lock"
            with mw.NightlyLock(path):
                source = ("import sys; sys.path.insert(0, " + repr(str(Path(mw.__file__).parent)) + "); "
                          "import main_whole_run; "
                          "lock = main_whole_run.NightlyLock(sys.argv[1]); "
                          "\ntry:\n lock.__enter__()\nexcept RuntimeError:\n raise SystemExit(2)\n"
                          "raise SystemExit(0)")
                child = subprocess.run([sys.executable, "-c", source, str(path)],
                                       capture_output=True, text=True, timeout=10,
                                       stdin=subprocess.DEVNULL, creationflags=mw.NO_WINDOW)
                self.assertEqual(child.returncode, 2, child.stderr)

    def test_quiet_preflight_requires_the_reserved_window_and_empty_slot(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "IDLE-WINDOWS.md"
            why = "#2510: single hidden latest-main full reached run at 22:15Z; 60-minute timeout and 30-minute margin"
            path.write_text(f"| 2026-11-04 22:00Z | 2026-11-04 23:30Z | {why} | openbot-shikaku |\n",
                            encoding="utf-8")
            args = SimpleNamespace(night_date="2026-11-04", idle_windows_file=str(path))
            moment = datetime.datetime(2026, 11, 4, 22, 15, tzinfo=datetime.timezone.utc)

            def empty_processes(command, **kwargs):
                self.assertEqual(command[0], "powershell.exe")
                return SimpleNamespace(returncode=0, stdout="[]")

            path.write_text(f"| 2026-11-04 22:00Z | 2026-11-04 23:30Z | {why} | another-lane |\n",
                            encoding="utf-8")
            allowed, _ = mw.quiet_preflight(args, moment, lambda *_a, **_k: self.fail("wrong owner must stop first"))
            self.assertFalse(allowed)
            path.write_text(f"| 2026-11-04 22:00Z | 2026-11-04 23:30Z | {why} | openbot-shikaku |\n",
                            encoding="utf-8")
            allowed, _ = mw.quiet_preflight(args, moment, empty_processes)
            self.assertTrue(allowed)
            busy_args = SimpleNamespace(**vars(args))
            busy = lambda *_a, **_k: SimpleNamespace(returncode=0, stdout=json.dumps([
                {"ProcessId": 12, "Name": "cargo.exe", "CommandLine": "cargo test -p graphhelm-cli"}]))
            allowed, reason = mw.quiet_preflight(busy_args, moment, busy)
            self.assertFalse(allowed)
            self.assertIn("active heavy processes", reason["reason"])
            self.assertEqual(reason["processes"], [{"pid": 12, "name": "cargo.exe"}])

    def test_busy_slot_writes_blocked_receipt_without_running_or_opening_p0(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            idle = root / "IDLE-WINDOWS.md"
            why = "#2510: single hidden latest-main full reached run at 22:15Z; 60-minute timeout and 30-minute margin"
            idle.write_text(f"| 2026-11-04 22:00Z | 2026-11-04 23:30Z | {why} | openbot-shikaku |\n",
                            encoding="utf-8")
            args = SimpleNamespace(receipt_root=str(root / "nightly"), night_date="2026-11-04",
                                   idle_windows_file=str(idle))
            calls = []

            def busy_process(command, **kwargs):
                calls.append(command)
                return SimpleNamespace(returncode=0, stdout=json.dumps([
                    {"ProcessId": 12, "Name": "cargo.exe", "CommandLine": "cargo test -p graphhelm-cli"}]))

            result = mw.run_nightly(args, ["--nightly"], run=busy_process,
                                    now=datetime.datetime(2026, 11, 4, 22, 15, tzinfo=datetime.timezone.utc))
            receipt = json.loads((root / "nightly" / "2026-11-04" / "receipt.json").read_text(encoding="utf-8"))
            self.assertEqual(result, 2)
            self.assertEqual(receipt["status"], "blocked")
            self.assertIsNone(receipt["p0Issue"])
            self.assertEqual(len(calls), 1)

    def test_main_copy_timeout_saves_logs_and_opens_one_p0(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            idle = root / "IDLE-WINDOWS.md"
            why = "#2510: single hidden latest-main full reached run at 22:15Z; 60-minute timeout and 30-minute margin"
            idle.write_text(f"| 2026-11-04 22:00Z | 2026-11-04 23:30Z | {why} | openbot-shikaku |\n",
                            encoding="utf-8")
            latest_script = root / "latest_main_whole_run.py"
            latest_script.write_text("--nightly", encoding="utf-8")
            args = SimpleNamespace(receipt_root=str(root / "nightly"), night_date="2026-11-04",
                                   idle_windows_file=str(idle), root=str(root / "build"), repo="repo",
                                   github="stabem/GraphHelm", timeout=1)
            old_git, old_prepare, old_newer, old_reexec = mw.git, mw.prepare_tree, mw.newer_copy, mw.reexec
            mw.git = lambda _repo, *parts: "a" * 40 if parts == ("rev-parse", "origin/main") else ""
            mw.prepare_tree = lambda _repo, tree, _sha: Path(tree).mkdir(parents=True, exist_ok=True)
            mw.newer_copy = lambda *_: latest_script
            mw.reexec = lambda *_a, **_k: (_ for _ in ()).throw(subprocess.TimeoutExpired("runner", 1, output=b"partial out", stderr=b"partial err"))

            def fake_run(command, **kwargs):
                if command[0] == "powershell.exe":
                    return SimpleNamespace(returncode=0, stdout="[]", stderr="")
                if command[:3] == ["gh", "issue", "list"]:
                    return SimpleNamespace(returncode=0, stdout="[]", stderr="")
                if command[:3] == ["gh", "issue", "create"]:
                    return SimpleNamespace(returncode=0, stdout="https://github.com/stabem/GraphHelm/issues/2521\n", stderr="")
                self.fail(f"unexpected command: {command}")

            try:
                result = mw.run_nightly(args, ["--nightly"], run=fake_run,
                                        now=datetime.datetime(2026, 11, 4, 22, 15, tzinfo=datetime.timezone.utc))
            finally:
                mw.git, mw.prepare_tree, mw.newer_copy, mw.reexec = old_git, old_prepare, old_newer, old_reexec
            receipt_dir = root / "nightly" / "2026-11-04"
            saved = json.loads((receipt_dir / "receipt.json").read_text(encoding="utf-8"))
            self.assertEqual(result, 1)
            self.assertEqual(saved["status"], "incomplete")
            self.assertEqual(saved["p0Issue"]["url"], "https://github.com/stabem/GraphHelm/issues/2521")
            self.assertEqual((receipt_dir / "runner.stdout.log").read_text(encoding="utf-8"), "partial out")
            self.assertEqual((receipt_dir / "runner.stderr.log").read_text(encoding="utf-8"), "partial err")
            self.assertIn("pinned copy exceeded the deadline", saved["redSteps"][0])

    def test_merge_list_names_only_main_commits_since_last_green(self):
        old_commits = mw.first_parent_commits
        mw.first_parent_commits = lambda *_: {"merge-sha"}
        calls = []

        def fake_run(command, **kwargs):
            calls.append(command)
            if command[0] == "git":
                return SimpleNamespace(returncode=0, stdout="", stderr="")
            return SimpleNamespace(returncode=0, stdout=json.dumps([
                {"number": 2511, "title": "Merged after green", "mergeCommit": {"oid": "merge-sha"}},
                {"number": 2512, "title": "Not on this main path", "mergeCommit": {"oid": "other-sha"}},
            ]), stderr="")

        try:
            result = mw.merges_since_green("repo", "stabem/GraphHelm", "green-sha", "head-sha", fake_run)
        finally:
            mw.first_parent_commits = old_commits
        self.assertEqual(result["status"], "observed")
        self.assertEqual([item["number"] for item in result["merges"]], [2511])
        self.assertEqual(len(calls), 2)

    def test_failure_issue_body_marks_missing_green_as_unobserved(self):
        body, marker = mw.failure_issue_body("2026-11-04", "head-sha", None,
                                              {"status": "UNOBSERVED", "merges": []},
                                              "nightly/reached.json", ["cargo test failed"])
        self.assertEqual(marker, "nightly-main-2026-11-04")
        self.assertIn("Last green SHA: `UNOBSERVED`", body)
        self.assertIn("Merges since last green: UNOBSERVED", body)
        self.assertIn("cargo test failed", body)
        self.assertIn("does not claim that any merge caused", body)

    def test_p0_search_failure_fails_closed_without_creating_a_duplicate(self):
        with tempfile.TemporaryDirectory() as temp:
            calls = []
            def search_fails(command, **kwargs):
                calls.append(command)
                return SimpleNamespace(returncode=1, stdout="", stderr="unavailable")
            args = SimpleNamespace(github="stabem/GraphHelm")
            result = mw.open_failure_issue(args, "2026-11-04", "head", None,
                                           {"status": "UNOBSERVED", "merges": []},
                                           Path(temp) / "reached.json", ["failed step"], run=search_fails)
            self.assertEqual(result, {"status": "failed"})
            self.assertEqual(len(calls), 1)
            self.assertEqual(calls[0][:2], ["gh", "issue"])
            self.assertIn("list", calls[0])

    def test_existing_nightly_receipt_prevents_a_second_run(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            receipt_dir = root / "2026-11-04"
            receipt_dir.mkdir()
            receipt = receipt_dir / "receipt.json"
            original = b'{"nightDate":"2026-11-04","status":"incomplete"}\n'
            receipt.write_bytes(original)
            args = SimpleNamespace(receipt_root=str(root), night_date="2026-11-04")
            result = mw.run_nightly(args, [], run=lambda *a, **k: self.fail("must not rerun"),
                                    now=datetime.datetime(2026, 11, 4, tzinfo=datetime.timezone.utc))
            self.assertEqual(result, 1)
            self.assertEqual(receipt.read_bytes(), original)

    def test_red_nightly_run_keeps_logs_receipt_and_opens_one_p0(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            args = SimpleNamespace(receipt_root=str(root / "nightly"), night_date="2026-11-04",
                                   root=str(root / "build"), repo="repo", github="stabem/GraphHelm",
                                   timeout=60, idle_windows_file=str(root / "IDLE-WINDOWS.md"))
            why = "#2510: single hidden latest-main full reached run at 22:15Z; 60-minute timeout and 30-minute margin"
            Path(args.idle_windows_file).write_text(
                f"| 2026-11-04 22:00Z | 2026-11-04 23:30Z | {why} | openbot-shikaku |\n", encoding="utf-8")
            head, base = "a" * 40, "b" * 40
            old_git, old_prepare, old_newer = mw.git, mw.prepare_tree, mw.newer_copy

            def fake_git(_repo, *parts):
                if parts == ("rev-parse", "origin/main"):
                    return head
                if parts == ("rev-parse", "origin/main^"):
                    return base
                return ""

            def fake_prepare(_repo, tree, _sha):
                Path(tree).mkdir(parents=True, exist_ok=True)

            mw.git, mw.prepare_tree, mw.newer_copy = fake_git, fake_prepare, lambda *_: None

            def fake_run(command, **kwargs):
                if command[0] == "powershell.exe":
                    return SimpleNamespace(returncode=0, stdout="[]", stderr="")
                if "run_reached.py" in str(command[1]):
                    report = Path(command[command.index("--output") + 1])
                    report.parent.mkdir(parents=True, exist_ok=True)
                    report.write_text(json.dumps({"status": "failed", "completed": [
                        {"name": "cargo test -p graphhelm-cli", "returncode": 101}]}), encoding="utf-8")
                    return SimpleNamespace(returncode=1, stdout="test output", stderr="test error")
                if command[:3] == ["gh", "issue", "list"]:
                    return SimpleNamespace(returncode=0, stdout="[]", stderr="")
                if command[:3] == ["gh", "issue", "create"]:
                    return SimpleNamespace(returncode=0, stdout="https://github.com/stabem/GraphHelm/issues/2520\n", stderr="")
                self.fail(f"unexpected command: {command}")

            try:
                result = mw.run_nightly(args, ["--nightly"], run=fake_run,
                                        now=datetime.datetime(2026, 11, 4, 22, 15, tzinfo=datetime.timezone.utc))
            finally:
                mw.git, mw.prepare_tree, mw.newer_copy = old_git, old_prepare, old_newer

            receipt_dir = root / "nightly" / "2026-11-04"
            saved = json.loads((receipt_dir / "receipt.json").read_text(encoding="utf-8"))
            issue = (receipt_dir / "p0-issue.md").read_text(encoding="utf-8")
            self.assertEqual(result, 1)
            self.assertEqual(saved["status"], "failed")
            self.assertEqual(saved["headSha"], head)
            self.assertEqual(saved["baseSha"], base)
            self.assertEqual(saved["baseline"], "UNOBSERVED")
            self.assertEqual(saved["p0Issue"]["url"], "https://github.com/stabem/GraphHelm/issues/2520")
            self.assertTrue((receipt_dir / "runner.stdout.log").is_file())
            self.assertTrue((receipt_dir / "runner.stderr.log").is_file())
            self.assertIn("Merges since last green: UNOBSERVED", issue)
            self.assertIn("cargo test -p graphhelm-cli", issue)

    def test_green_nightly_run_keeps_passed_receipt_and_does_not_open_p0(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            idle = root / "IDLE-WINDOWS.md"
            why = "#2510: single hidden latest-main full reached run at 22:15Z; 60-minute timeout and 30-minute margin"
            idle.write_text(f"| 2026-11-04 22:00Z | 2026-11-04 23:30Z | {why} | openbot-shikaku |\n",
                            encoding="utf-8")
            args = SimpleNamespace(receipt_root=str(root / "nightly"), night_date="2026-11-04",
                                   root=str(root / "build"), repo="repo", github="stabem/GraphHelm",
                                   timeout=60, idle_windows_file=str(idle))
            head, base = "a" * 40, "b" * 40
            old_git, old_prepare, old_newer = mw.git, mw.prepare_tree, mw.newer_copy
            calls = []

            def fake_git(_repo, *parts):
                if parts == ("rev-parse", "origin/main"):
                    return head
                if parts == ("rev-parse", "origin/main^"):
                    return base
                return ""

            def fake_prepare(_repo, tree, _sha):
                Path(tree).mkdir(parents=True, exist_ok=True)

            def fake_run(command, **kwargs):
                calls.append(command)
                if command[0] == "powershell.exe":
                    return SimpleNamespace(returncode=0, stdout="[]", stderr="")
                if "run_reached.py" in str(command[1]):
                    report = Path(command[command.index("--output") + 1])
                    report.parent.mkdir(parents=True, exist_ok=True)
                    report.write_text(json.dumps({"status": "passed", "completed": []}), encoding="utf-8")
                    return SimpleNamespace(returncode=0, stdout="green", stderr="")
                self.fail(f"unexpected command: {command}")

            mw.git, mw.prepare_tree, mw.newer_copy = fake_git, fake_prepare, lambda *_: None
            try:
                result = mw.run_nightly(args, ["--nightly"], run=fake_run,
                                        now=datetime.datetime(2026, 11, 4, 22, 15, tzinfo=datetime.timezone.utc))
            finally:
                mw.git, mw.prepare_tree, mw.newer_copy = old_git, old_prepare, old_newer

            receipt_dir = root / "nightly" / "2026-11-04"
            saved = json.loads((receipt_dir / "receipt.json").read_text(encoding="utf-8"))
            green = json.loads((root / "nightly" / "last-green.json").read_text(encoding="utf-8"))
            self.assertEqual(result, 0)
            self.assertEqual(saved["status"], "passed")
            self.assertIsNone(saved["p0Issue"])
            self.assertEqual(green["sha"], head)
            self.assertEqual(green["receipt"], str(receipt_dir / "receipt.json"))
            self.assertFalse(any(command[:3] == ["gh", "issue", "create"] for command in calls))

    def test_legacy_main_returns_the_reexec_child_exit_code(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            repo = root / "repo"
            repo.mkdir()
            copy = root / "main.py"
            copy.write_text("child", encoding="utf-8")
            old_resolve, old_git = mw.resolve_execution, mw.git
            old_prepare, old_newer, old_reexec = mw.prepare_tree, mw.newer_copy, mw.reexec
            mw.resolve_execution = lambda _args: "execution"
            mw.git = lambda _repo, *parts: "a" * 40 if parts == ("rev-parse", "origin/main") else ""
            mw.prepare_tree = lambda _repo, tree, _sha: Path(tree).mkdir(parents=True, exist_ok=True)
            mw.newer_copy = lambda *_: copy
            mw.reexec = lambda *_args, **_kwargs: SimpleNamespace(returncode=0, stdout="", stderr="")
            try:
                result = mw.main(["--repo", str(repo), "--root", str(root / "build")])
            finally:
                mw.resolve_execution, mw.git = old_resolve, old_git
                mw.prepare_tree, mw.newer_copy, mw.reexec = old_prepare, old_newer, old_reexec
            self.assertIs(type(result), int)
            self.assertEqual(result, 0)

    def test_main_posts_to_bound_execution_before_state_completion(self):
        seen = []

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_POST(self):
                seen.append(self.path)
                self.send_response(200)
                self.send_header('Content-Type', 'application/json')
                self.end_headers()
                self.wfile.write(b'{"ok":true}')

            def log_message(self, *args):
                pass

        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            repo = root / 'caller' / 'repo'
            (repo / '.graphhelm').mkdir(parents=True)
            (repo / '.graphhelm' / 'team.json').write_text(
                json.dumps({'executionId': 'studio#v2'}), encoding='utf-8')
            token = repo / '.graphhelm' / 'events.token'
            token.write_text('agent-token\n', encoding='utf-8')
            other = root / 'other-cwd'
            (other / '.graphhelm').mkdir(parents=True)
            (other / '.graphhelm' / 'team.json').write_text(
                json.dumps({'executionId': 'wrong-cwd'}), encoding='utf-8')
            state = root / 'state.json'
            work = root / 'work'
            sha, base = 'a' * 40, 'b' * 40

            def fake_git(_repo, *args):
                return sha if args == ('rev-parse', 'origin/main') else base

            def fake_prepare(_repo, tree, _sha):
                Path(tree).mkdir(parents=True, exist_ok=True)

            def fake_run(command, **kwargs):
                output = Path(command[command.index('--output') + 1])
                output.parent.mkdir(parents=True, exist_ok=True)
                output.write_text(json.dumps({'status': 'passed'}), encoding='utf-8')
                return SimpleNamespace(returncode=0)

            old_git, old_prepare, old_run, old_cwd = mw.git, mw.prepare_tree, mw.subprocess.run, os.getcwd()
            mw.git, mw.prepare_tree, mw.subprocess.run = fake_git, fake_prepare, fake_run
            try:
                os.chdir(other)
                code = mw.main(['--repo', str(repo), '--root', str(work), '--state', str(state),
                                '--url', 'http://127.0.0.1:%d' % server.server_address[1],
                                '--allow-shared-token', '--min-interval', '0'])
            finally:
                os.chdir(old_cwd)
                mw.git, mw.prepare_tree, mw.subprocess.run = old_git, old_prepare, old_run
            self.assertEqual(code, 0)
            self.assertEqual(seen, ['/v1/executions/studio%23v2/signal'])
            self.assertTrue(json.loads(state.read_text())['recorded'])

    def test_a_redirect_is_refused_and_the_token_goes_nowhere_else(self):
        """The receipt used to follow a redirect with the bearer token still attached."""
        seen = []

        class Sink(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                seen.append(self.headers.get('Authorization'))
                raw = b'{"ok": true}'
                self.send_response(200)
                self.send_header('Content-Length', str(len(raw)))
                self.end_headers()
                self.wfile.write(raw)

            do_POST = do_GET  # urllib turns a redirected POST into a GET

            def log_message(self, *_):
                pass

        sink = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Sink)

        class Front(http.server.BaseHTTPRequestHandler):
            def do_POST(self):
                self.send_response(302)
                self.send_header('Location', 'http://127.0.0.1:%d%s' % (sink.server_port, self.path))
                self.send_header('Content-Length', '0')
                self.end_headers()

            def log_message(self, *_):
                pass

        front = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Front)
        for server in (sink, front):
            threading.Thread(target=server.serve_forever, daemon=True).start()
            self.addCleanup(server.server_close)
            self.addCleanup(server.shutdown)
        with tempfile.TemporaryDirectory() as temp:
            token = Path(temp) / 'events.token'
            token.write_text('agent-token\n', encoding='utf-8')
            args = SimpleNamespace(url='http://127.0.0.1:%d' % front.server_port, execution='e',
                                   token_file=str(token), allow_shared_token=True)
            result = mw.post(args, {'signal': {'id': 'receipt-1'}})
        self.assertIs(result, False)
        self.assertEqual(seen, [])

    def test_scheduled_repo_binding_is_resolved_before_work(self):
        with tempfile.TemporaryDirectory() as temp:
            repo = Path(temp) / 'scheduled-repo'
            (repo / '.graphhelm').mkdir(parents=True)
            (repo / '.graphhelm' / 'team.json').write_text(
                json.dumps({'executionId': 'studio-v2'}), encoding='utf-8')
            args = SimpleNamespace(execution=None, token_file=str(Path(temp) / 'events.token'), repo=str(repo))
            self.assertEqual(mw.resolve_execution(args), 'studio-v2')

    def test_invalid_binding_refuses_before_git_or_state(self):
        with tempfile.TemporaryDirectory() as temp:
            repo = Path(temp) / 'repo'
            (repo / '.graphhelm').mkdir(parents=True)
            (repo / '.graphhelm' / 'team.json').write_text('{"executionId":"bad/id"}', encoding='utf-8')
            state = Path(temp) / 'state.json'
            old_git = mw.git
            mw.git = lambda *args: self.fail('git must not run before binding refusal')
            try:
                with self.assertRaises(SystemExit):
                    mw.main(['--repo', str(repo), '--state', str(state), '--root', str(Path(temp) / 'work')])
            finally:
                mw.git = old_git
            self.assertFalse(state.exists())

    def test_omitted_token_uses_repo_and_refuses_when_repo_binding_is_missing(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            caller = root / 'caller'
            repo = root / 'requested-repo'
            (caller / '.graphhelm').mkdir(parents=True)
            (caller / '.graphhelm' / 'team.json').write_text(
                json.dumps({'executionId': 'wrong-cwd'}), encoding='utf-8')
            (repo / '.graphhelm').mkdir(parents=True)
            (repo / '.git').mkdir()  # the walk stops at a .git entry, so an ancestor's team.json is out of reach
            state = root / 'state.json'
            old_git, old_cwd = mw.git, os.getcwd()
            mw.git = lambda *args: self.fail('git must not run before binding refusal')
            try:
                os.chdir(caller)
                with self.assertRaises(SystemExit):
                    mw.main(['--repo', str(repo), '--root', str(root / 'work'), '--state', str(state)])
            finally:
                os.chdir(old_cwd)
                mw.git = old_git
            self.assertFalse(state.exists())

    def test_explicit_relative_token_keeps_caller_cwd_binding(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            caller = root / 'caller'
            repo = root / 'requested-repo'
            (caller / '.graphhelm').mkdir(parents=True)
            (caller / '.graphhelm' / 'team.json').write_text(
                json.dumps({'executionId': 'caller-run'}), encoding='utf-8')
            (caller / '.graphhelm' / 'events.token').write_text('token\n', encoding='utf-8')
            (repo / '.graphhelm').mkdir(parents=True)
            (repo / '.graphhelm' / 'team.json').write_text(
                json.dumps({'executionId': 'repo-run'}), encoding='utf-8')
            old_git, old_cwd = mw.git, os.getcwd()
            seen = {}
            real_resolve = mw.resolve_execution
            mw.git = lambda *args: 'a' * 40
            mw.resolve_execution = lambda args: seen.setdefault('execution', real_resolve(args))
            try:
                os.chdir(caller)
                mw.main(['--repo', str(repo), '--root', str(root / 'work'), '--dry-run',
                         '--token-file', '.graphhelm/events.token'])
            finally:
                os.chdir(old_cwd)
                mw.git, mw.resolve_execution = old_git, real_resolve
            self.assertEqual(seen['execution'], 'caller-run')

    def test_due_only_when_main_moved_and_the_hour_passed(self):
        self.assertEqual(mw.due({}, "b", 1000, 3600), (True, "no earlier whole run"))
        self.assertFalse(mw.due({"sha": "a", "startedAt": 0}, "a", 99999, 3600)[0])
        self.assertFalse(mw.due({"sha": "a", "startedAt": 900}, "b", 1000, 3600)[0])
        self.assertTrue(mw.due({"sha": "a", "startedAt": 0}, "b", 3600, 3600)[0])

    def test_red_receipt_is_high_severity_and_names_the_red_step(self):
        report = {"status": "failed", "completed": [{"name": "cargo test -p x", "returncode": 101},
                                                    {"name": "fmt", "returncode": 0}]}
        signal = mw.build_signal("a" * 40, "b" * 40, report, Path("r.json"), "o/r", "2026-01-01T00:00:00Z")["signal"]
        self.assertEqual(signal["type"], "main.whole_run")
        self.assertEqual(signal["severity"], "high")
        self.assertEqual(json.loads(signal["description"])["red"], ["cargo test -p x"])
        green = mw.build_signal("a" * 40, "b" * 40, {"status": "passed"}, Path("r.json"), "o/r", "t")["signal"]
        self.assertEqual(green["severity"], "low")

    def test_child_budget_follows_the_timeout_and_an_incomplete_run_is_high(self):
        cmd = mw.child_command("t", "b", "s", "r", "o.json", 3600)
        self.assertEqual(float(cmd[cmd.index("--budget-seconds") + 1]), 3300.0)
        self.assertLess(mw.child_budget(100), 100)
        seen, real_run = [], runner.run
        runner.run = lambda args: seen.append(args) or 0
        try:
            runner.main(cmd[2:])
            runner.main(['--repo', 't', '--output', 'ordinary.json'])
        finally:
            runner.run = real_run
        broad = {'packages': ['graphhelm-cli']}
        self.assertIsNone(runner._whole_reason(broad, seen[0].allow_whole_package))
        self.assertIsNotNone(runner._whole_reason(broad, seen[1].allow_whole_package))
        self.assertTrue(seen[0].keep_going)
        self.assertFalse(seen[0].resume)
        self.assertEqual(seen[0].budget_seconds, 3300.0)
        for status in ("budgetExceeded", "incomplete"):
            signal = mw.build_signal("a" * 40, "b" * 40, {"status": status}, Path("r.json"), "o/r", "2026-01-01T00:00:00Z")["signal"]
            self.assertEqual(signal["severity"], "high")

    def test_state_is_saved_before_the_post_so_a_failed_post_cannot_rerun_the_suite(self):
        def refusing_poster(args, body):
            self.assertEqual(json.loads(state.read_text())["sha"], "a" * 40)
            raise SystemExit("no credential")

        with tempfile.TemporaryDirectory() as temp:
            state = Path(temp) / "state.json"
            with self.assertRaises(SystemExit):
                mw.save_then_post(None, state, "a" * 40, 5.0, {"status": "failed"}, {}, refusing_poster)
            saved = json.loads(state.read_text())
            self.assertEqual((saved["sha"], saved["recorded"]), ("a" * 40, False))
            self.assertFalse(mw.due(saved, "a" * 40, 99999, 3600)[0])

    def test_a_stale_copy_hands_over_to_mains_copy_and_a_dirty_tree_is_refused(self):
        with tempfile.TemporaryDirectory() as temp:
            tree = Path(temp) / "tree"
            (tree / "tools" / "reached-tests").mkdir(parents=True)
            old = Path(temp) / "main_whole_run.py"
            old.write_text("old", encoding="utf-8")
            new = tree / "tools" / "reached-tests" / "main_whole_run.py"
            new.write_text("new", encoding="utf-8")
            self.assertEqual(mw.newer_copy(tree, old), new)
            old.write_text("new", encoding="utf-8")
            self.assertIsNone(mw.newer_copy(tree, old))
            calls = []
            mw.prepare_tree("repo", tree, "s", lambda repo, *a: calls.append(a) or "")
            self.assertEqual(calls[-1], ("checkout", "--detach", "s"))
            with self.assertRaises(RuntimeError):
                mw.prepare_tree("repo", tree, "s", lambda repo, *a: "M file" if a[0] == "status" else "")
            self.assertNotIn(("checkout", "--detach", "--force", "s"), calls)

    def test_reexec_runs_the_copy_hidden_with_the_guard_set(self):
        seen = {}

        def fake(cmd, **kw):
            seen.update(cmd=cmd, **kw)
            return type("R", (), {"returncode": 7})()
        real = mw.subprocess.run
        mw.subprocess.run = fake
        try:
            self.assertEqual(mw.reexec(Path("c.py"), ["--x"], {}).returncode, 7)
        finally:
            mw.subprocess.run = real
        self.assertEqual(seen["env"]["MAIN_WHOLE_RUN_REEXEC"], "1")
        self.assertEqual(seen["creationflags"], mw.NO_WINDOW)
        self.assertIs(seen["stdin"], mw.subprocess.DEVNULL)
        self.assertTrue(seen["capture_output"])
        self.assertEqual(seen["cmd"][1:], ["c.py", "--x"])


if __name__ == "__main__":
    unittest.main()
