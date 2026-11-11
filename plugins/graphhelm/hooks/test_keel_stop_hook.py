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

    def test_block_names_the_work_tree_it_checked(self):
        """#2731: a lane told 'no card' could not tell which directory was checked."""
        self.code(20)
        (self.repo / "sub").mkdir()
        reason = self.run_hook(cwd=str(self.repo / "sub"))["reason"]
        top = self.repo.resolve().as_posix().lower()
        self.assertIn(top, reason.replace("\\", "/").lower())

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


class CardPath(unittest.TestCase):
    """#1548: the hooks read .graphhelm/cards/issue-<N>.json on an issue-<N> branch, and the legacy
    keel-card.json everywhere else. Defect: the hooks read only keel-card.json, so a PR with the
    right per-issue card was blocked."""

    def setUp(self):
        self.tmp = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.tmp, True)
        self.repo = Path(self.tmp)
        git(self.repo, "init", "-q")

    def on_branch(self, name):
        git(self.repo, "symbolic-ref", "HEAD", f"refs/heads/{name}")

    def card(self, relative):
        path = self.repo / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("{}")

    def test_issue_branch_uses_its_per_issue_card(self):
        self.on_branch("issue-1548-card-path")
        self.card(".graphhelm/cards/issue-1548.json")
        self.assertEqual(keel_stop_hook.card_path(self.repo).as_posix(),
                         ".graphhelm/cards/issue-1548.json")

    def test_issue_branch_without_per_issue_card_falls_back(self):
        self.on_branch("issue-1548-card-path")
        self.card(".graphhelm/keel-card.json")
        self.assertEqual(keel_stop_hook.card_path(self.repo).as_posix(), ".graphhelm/keel-card.json")

    def test_a_card_of_another_issue_is_not_used(self):
        self.on_branch("issue-1548-card-path")
        self.card(".graphhelm/cards/issue-9999.json")
        self.assertEqual(keel_stop_hook.card_path(self.repo).as_posix(), ".graphhelm/keel-card.json")

    def test_a_branch_that_is_not_an_issue_keeps_the_legacy_card(self):
        self.on_branch("feature-x")
        self.card(".graphhelm/cards/issue-1548.json")
        self.card(".graphhelm/keel-card.json")
        self.assertEqual(keel_stop_hook.card_path(self.repo).as_posix(), ".graphhelm/keel-card.json")

    def test_issue_number_must_end_the_name_or_be_followed_by_a_dash(self):
        self.on_branch("issue-15480-x")
        self.card(".graphhelm/cards/issue-1548.json")
        self.assertEqual(keel_stop_hook.card_path(self.repo).as_posix(), ".graphhelm/keel-card.json")


class CardPathChangedCard(unittest.TestCase):
    """#2735: a detached review checkout, or a card named issue-<N>-<what>.json, has a card the
    branch name does not find. The hooks use the one card the checkout adds since the merge base;
    cards already on main (every merged card is tracked) never count."""

    def setUp(self):
        self.tmp = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.tmp, True)
        self.repo = Path(self.tmp)
        git(self.repo, "init", "-q", "-b", "main")
        git(self.repo, "config", "user.email", "t@t")
        git(self.repo, "config", "user.name", "t")
        self.card(".graphhelm/cards/issue-1000.json")
        git(self.repo, "add", ".")
        git(self.repo, "commit", "-qm", "base")

    def card(self, relative):
        path = self.repo / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("{}")

    def commit_card(self, relative):
        self.card(relative)
        git(self.repo, "add", ".")
        git(self.repo, "commit", "-qm", relative)

    def path(self):
        return keel_stop_hook.card_path(self.repo).as_posix()

    def test_detached_checkout_uses_the_card_it_adds(self):
        git(self.repo, "checkout", "-qb", "work")
        self.commit_card(".graphhelm/cards/issue-2694-phases.json")
        git(self.repo, "checkout", "-q", "--detach")
        self.assertEqual(self.path(), ".graphhelm/cards/issue-2694-phases.json")

    def test_suffixed_card_on_its_issue_branch_is_found(self):
        git(self.repo, "checkout", "-qb", "issue-2694-append-phases-main")
        self.commit_card(".graphhelm/cards/issue-2694-phases.json")
        self.assertEqual(self.path(), ".graphhelm/cards/issue-2694-phases.json")

    def test_cards_already_on_main_are_not_the_checkouts_card(self):
        git(self.repo, "checkout", "-q", "--detach")
        self.assertEqual(self.path(), ".graphhelm/keel-card.json")

    def test_two_added_cards_stay_ambiguous(self):
        git(self.repo, "checkout", "-qb", "work")
        self.commit_card(".graphhelm/cards/issue-2001.json")
        self.commit_card(".graphhelm/cards/issue-2002.json")
        git(self.repo, "checkout", "-q", "--detach")
        self.assertEqual(self.path(), ".graphhelm/keel-card.json")

    def test_an_existing_legacy_card_still_wins(self):
        git(self.repo, "checkout", "-qb", "work")
        self.commit_card(".graphhelm/cards/issue-2001.json")
        self.card(".graphhelm/keel-card.json")
        self.assertEqual(self.path(), ".graphhelm/keel-card.json")

    def test_an_unrelated_card_is_not_the_issue_branchs_card(self):
        """#2735 review: on issue-9-z with no card of its own, a changed issue-3 card does not count."""
        git(self.repo, "checkout", "-qb", "issue-9-z")
        self.commit_card(".graphhelm/cards/issue-3-other.json")
        self.assertEqual(self.path(), ".graphhelm/keel-card.json")

    def test_an_untracked_card_on_a_detached_checkout_is_found(self):
        git(self.repo, "checkout", "-q", "--detach")
        self.card(".graphhelm/cards/issue-2694-phases.json")  # written, never added
        self.assertEqual(self.path(), ".graphhelm/cards/issue-2694-phases.json")

    def test_a_pretool_run_looks_for_the_card_once(self):
        """#2735 review: the lookup is one `git diff` per run, not one per card_path call."""
        import keel_pretool_hook
        git(self.repo, "checkout", "-qb", "issue-2694-append-phases-main")
        self.commit_card(".graphhelm/cards/issue-2694-phases.json")
        target = self.repo / "app.py"
        real = keel_stop_hook._git
        with mock.patch.object(keel_stop_hook, "_git", side_effect=real) as spy:
            keel_pretool_hook.decide(self.repo, target, {"content": "x = 1\n"})
        lookups = [c for c in spy.call_args_list if ".graphhelm/cards/" in c.args]
        self.assertEqual(len(lookups), 1, spy.call_args_list)


class UntrackedReadLimit(unittest.TestCase):
    """#2807: an untracked file above UNTRACKED_READ_LIMIT is never read whole."""

    LIMIT = 100

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.repo = Path(self.tmp.name) / "repo"
        self.repo.mkdir()
        git(self.repo, "init", "-q", "-b", "main")
        git(self.repo, "config", "user.email", "t@t")
        git(self.repo, "config", "user.name", "t")
        (self.repo / "README.md").write_text("x\n")
        git(self.repo, "add", ".")
        git(self.repo, "commit", "-qm", "base")
        self.cli = Path(self.tmp.name) / "graphhelm"
        self.cli.write_text("cli")
        self.card = Path(self.tmp.name) / "card.json"
        self.card.write_text("{}")
        patch = mock.patch.object(keel_stop_hook, "UNTRACKED_READ_LIMIT", self.LIMIT, create=True)
        patch.start()
        self.addCleanup(patch.stop)
        self.addCleanup(self.tmp.cleanup)

    def fingerprint(self):
        return keel_stop_hook._fingerprint(self.repo, "main", self.card, str(self.cli), {})

    def test_over_limit_file_without_newline_is_a_non_trivial_change(self):
        (self.repo / "big.bin").write_bytes(b"a" * (self.LIMIT + 1))
        self.assertGreater(keel_stop_hook.code_lines_changed(self.repo, "main"), keel_stop_hook.TRIVIAL_LINES)

    def test_file_at_the_limit_still_counts_its_real_lines(self):
        (self.repo / "edge.txt.py").write_bytes(b"a\n" * (self.LIMIT // 2))
        self.assertEqual(keel_stop_hook.code_lines_changed(self.repo, "main"), self.LIMIT // 2)
        (self.repo / "edge.txt.py").write_bytes(b"a\n" * 3)
        self.assertEqual(keel_stop_hook.code_lines_changed(self.repo, "main"), 3)

    def test_fingerprint_does_not_read_an_over_limit_file(self):
        big = self.repo / "big.bin"
        big.write_bytes(b"a" * (self.LIMIT + 1))
        read, real = [], Path.read_bytes

        def spy(path):
            read.append(path.name)
            return real(path)

        with mock.patch.object(Path, "read_bytes", spy):
            first = self.fingerprint()
            second = self.fingerprint()
        self.assertNotIn("big.bin", read)
        self.assertIsInstance(first, str)
        self.assertEqual(first, second)
        big.write_bytes(b"a" * (self.LIMIT + 50))
        self.assertNotEqual(self.fingerprint(), first)

    def test_fingerprint_of_a_small_file_follows_its_bytes(self):
        small = self.repo / "small.py"
        small.write_bytes(b"abc\n")
        stamp = os.stat(small).st_mtime_ns
        first = self.fingerprint()
        small.write_bytes(b"abd\n")  # same size; the bytes are what changed
        os.utime(small, ns=(stamp, stamp))
        self.assertNotEqual(self.fingerprint(), first)


if __name__ == "__main__":
    unittest.main()
