"""Contracts for main_whole_run.py: when a run is due and what a red receipt says; no git, Runtime or cargo."""
import json
import http.server
import threading
import os
import tempfile
import sys
import unittest
from types import SimpleNamespace
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import main_whole_run as mw
import run_reached as runner


class WholeRun(unittest.TestCase):
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
            self.assertEqual(mw.reexec(Path("c.py"), ["--x"], {}), 7)
        finally:
            mw.subprocess.run = real
        self.assertEqual(seen["env"]["MAIN_WHOLE_RUN_REEXEC"], "1")
        self.assertEqual(seen["creationflags"], mw.NO_WINDOW)
        self.assertEqual(seen["cmd"][1:], ["c.py", "--x"])


if __name__ == "__main__":
    unittest.main()
