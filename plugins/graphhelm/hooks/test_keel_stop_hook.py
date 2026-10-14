"""Contract tests for the Keel Stop gate, against throwaway git repositories."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import keel_record
import keel_stop_hook

# Owner rule: children run hidden in the calling terminal, never in a new console window.
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0

SCRIPT = Path(__file__).with_name("keel_stop_hook.py")


def git(repo, *args):
    subprocess.run(["git", "-C", str(repo), *args], check=True, capture_output=True, creationflags=NO_WINDOW)


class KeelStopHook(unittest.TestCase):
    def test_bound_check_recommendation_reaches_recorder(self):
        self.code(20)
        (self.repo / ".graphhelm").mkdir()
        (self.repo / ".graphhelm" / "keel-card.json").write_text("{}")
        envelope = {"data": {"recommendation": {"step": "review", "head": "abc123",
                    "findingIds": [], "paths": [], "applied": False}}}
        result = mock.Mock(returncode=0, stdout=json.dumps(envelope))
        with mock.patch.object(keel_stop_hook, "_merge_base", return_value="base"), \
             mock.patch.object(keel_stop_hook, "code_lines_changed", return_value=20), \
             mock.patch.object(keel_stop_hook.shutil, "which", return_value="graphhelm"), \
             mock.patch.object(keel_stop_hook, "_fingerprint", return_value=None), \
             mock.patch.object(keel_stop_hook.subprocess, "run", return_value=result), \
             mock.patch.object(keel_record, "record_check") as record:
            self.assertIsNone(keel_stop_hook.decide(self.repo, {"session_id": "s1"}))
        record.assert_called_once_with(envelope, {"session_id": "s1"})

    def test_undelivered_signal_is_not_cached(self):
        """#1228 review: a Runtime failure on record_check retries on the next Stop."""
        self.code(20)
        (self.repo / ".graphhelm").mkdir()
        (self.repo / ".graphhelm" / "keel-card.json").write_text("{}")
        cli = self.repo.parent / f"{self.repo.name}-graphhelm"
        cli.write_text("v1")
        self.addCleanup(cli.unlink)
        real_run = subprocess.run
        calls = []

        def fake_run(cmd, *args, **kwargs):
            if cmd[0].endswith("-graphhelm"):
                calls.append(cmd)
                return mock.Mock(returncode=0, stdout="{}")
            return real_run(cmd, *args, **kwargs)

        with mock.patch.object(keel_stop_hook.shutil, "which", return_value=str(cli)), \
             mock.patch.object(keel_stop_hook.subprocess, "run", side_effect=fake_run), \
             mock.patch.object(keel_stop_hook, "_execution", return_value="run-1"), \
             mock.patch.object(keel_record, "record_check", side_effect=[False, True, True]):
            keel_stop_hook.decide(self.repo, {})
            keel_stop_hook.decide(self.repo, {})  # retried: first signal was lost
            keel_stop_hook.decide(self.repo, {})  # delivered last time: cached now
        self.assertEqual(len(calls), 2)

    def test_unchanged_state_reuses_check_and_records_once(self):
        """#1228: a second Stop on the same diff neither reruns the CLI nor re-posts the signal."""
        self.code(20)
        (self.repo / ".graphhelm").mkdir()
        (self.repo / ".graphhelm" / "keel-card.json").write_text("{}")
        envelope = {"data": {"findings": [{"blocking": True, "rule": "R1", "path": "a.py"}]}}
        real_run = subprocess.run
        calls = []

        def fake_run(cmd, *args, **kwargs):
            if cmd[0].endswith("-graphhelm"):
                calls.append(cmd)
                return mock.Mock(returncode=2, stdout=json.dumps(envelope))
            return real_run(cmd, *args, **kwargs)

        cli = self.repo.parent / f"{self.repo.name}-graphhelm"
        cli.write_text("v1")
        self.addCleanup(cli.unlink)
        with mock.patch.object(keel_stop_hook.shutil, "which", return_value=str(cli)), \
             mock.patch.object(keel_stop_hook.subprocess, "run", side_effect=fake_run), \
             mock.patch.object(keel_record, "record_check") as record:
            first = keel_stop_hook.decide(self.repo, {})
            self.assertFalse(keel_stop_hook.LAST_FROM_CACHE)
            second = keel_stop_hook.decide(self.repo, {})
            self.assertTrue(keel_stop_hook.LAST_FROM_CACHE)  # main() skips record_block on this
            self.assertEqual(first, second)
            self.assertIn("R1 a.py", first)
            self.assertEqual((len(calls), record.call_count), (1, 1))
            (self.repo / "a.py").write_text("changed\n")
            self.assertIn("R1 a.py", keel_stop_hook.decide(self.repo, {}))
            self.assertEqual((len(calls), record.call_count), (2, 2))
            with mock.patch.object(keel_stop_hook, "_execution", return_value="other-run"):
                keel_stop_hook.decide(self.repo, {})  # another execution records its own signal
            self.assertEqual((len(calls), record.call_count), (3, 3))
            cli.write_text("v2, new rules")  # `graphhelm update` must not reuse an old verdict
            keel_stop_hook.decide(self.repo, {})
            self.assertEqual((len(calls), record.call_count), (4, 4))

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.repo = Path(self.tmp.name)
        git(self.repo, "init", "-q", "-b", "main")
        git(self.repo, "config", "user.email", "t@t")
        git(self.repo, "config", "user.name", "t")
        (self.repo / "README.md").write_text("x\n")
        git(self.repo, "add", ".")
        git(self.repo, "commit", "-qm", "base")
        git(self.repo, "checkout", "-qb", "work")

    def tearDown(self):
        self.tmp.cleanup()

    def run_hook(self, **payload):
        payload.setdefault("cwd", str(self.repo))
        git_dir = os.path.dirname(shutil.which("git"))
        env = dict(os.environ, PATH=os.pathsep.join((os.path.dirname(sys.executable), git_dir, os.environ["PATH"])))
        out = subprocess.run([sys.executable, str(SCRIPT)], input=json.dumps(payload),
                             capture_output=True, text=True, env=env, check=True, creationflags=NO_WINDOW).stdout
        return json.loads(out) if out.strip() else None

    def code(self, lines):
        (self.repo / "app.py").write_text("".join(f"x{i} = {i}\n" for i in range(lines)))

    def test_large_code_change_without_card_blocks(self):
        self.code(20)
        result = self.run_hook()
        self.assertEqual(result["decision"], "block")
        self.assertIn("keel-card.json", result["reason"])

    def test_committed_change_without_card_blocks(self):
        self.code(20)
        git(self.repo, "add", ".")
        git(self.repo, "commit", "-qm", "code")
        self.assertEqual(self.run_hook()["decision"], "block")

    def test_card_present_lets_turn_end(self):
        self.code(20)
        (self.repo / ".graphhelm").mkdir()
        (self.repo / ".graphhelm" / "keel-card.json").write_text(
            json.dumps({"promise": "p", "scopePaths": ["app.py"], "proof": "true"}))
        (self.repo / ".gitignore").write_text("/.graphhelm/\n")
        self.assertIsNone(self.run_hook())

    def test_docs_only_change_passes(self):
        (self.repo / "docs").mkdir()
        (self.repo / "docs" / "a.md").write_text("line\n" * 50)
        self.assertIsNone(self.run_hook())

    def test_tiny_code_change_passes(self):
        self.code(3)
        self.assertIsNone(self.run_hook())

    def test_second_stop_never_blocks(self):
        self.code(20)
        self.assertIsNone(self.run_hook(stop_hook_active=True))

    def test_outside_a_repository_passes(self):
        with tempfile.TemporaryDirectory() as other:
            self.assertIsNone(self.run_hook(cwd=other))


if __name__ == "__main__":
    unittest.main()
