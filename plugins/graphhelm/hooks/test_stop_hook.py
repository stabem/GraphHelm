"""#1278 (plan D5): how many processes one Stop and one edit check start, against a throwaway repo.

Every `subprocess.run` and `subprocess.Popen` made while a hook runs in-process is counted (and
still executed). The counts are the promise: the Claude Stop is one interpreter with no child
interpreter, the merge base comes from `.graphhelm/cache/` while HEAD and the base ref stand
still, and HEAD and the git dir are read from files. Cost: one temp repo, a few git commands.
"""

import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import handoff_stop_hook  # noqa: E402
import keel_pretool_hook  # noqa: E402
import keel_stop_hook  # noqa: E402
import lane_stop_hook  # noqa: E402
import stop_hook  # noqa: E402

NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
HERE = Path(__file__).resolve().parent


def git(repo, *args):
    subprocess.run(["git", "-C", str(repo), *args], check=True, capture_output=True, creationflags=NO_WINDOW)


class Counted:
    """Counts the processes started while active; each one still runs."""

    def __init__(self):
        self.calls: list[list[str]] = []
        self._run, self._popen = subprocess.run, subprocess.Popen

    def __enter__(self):
        inside = []  # `subprocess.run` starts its process through `Popen`: count it once

        def run(args, *a, **k):
            self.calls.append([str(x) for x in args])
            inside.append(True)
            try:
                return self._run(args, *a, **k)
            finally:
                inside.pop()

        def popen(args, *a, **k):
            if not inside:
                self.calls.append([str(x) for x in args])
            return self._popen(args, *a, **k)
        self._patches = [mock.patch.object(subprocess, "run", run), mock.patch.object(subprocess, "Popen", popen)]
        for patch in self._patches:
            patch.start()
        return self

    def __exit__(self, *exc):
        for patch in self._patches:
            patch.stop()

    def interpreters(self):
        return [call for call in self.calls if Path(call[0]).name.lower().startswith("python")]


class HookProcessCount(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.repo = Path(self.tmp.name) / "repo"
        self.repo.mkdir()
        git(self.repo, "init", "-q", "-b", "main")
        git(self.repo, "config", "user.email", "t@example.com")
        git(self.repo, "config", "user.name", "t")
        (self.repo / "README.md").write_text("x\n")
        git(self.repo, "add", ".")
        git(self.repo, "commit", "-q", "-m", "base")
        git(self.repo, "update-ref", "refs/remotes/origin/main", "HEAD")
        git(self.repo, "checkout", "-q", "-b", "work")
        (self.repo / "code.py").write_text("".join(f"line_{i} = {i}\n" for i in range(20)))
        git(self.repo, "add", ".")
        git(self.repo, "commit", "-q", "-m", "work")
        (self.repo / ".gitignore").write_text("/.graphhelm/*\n")
        git(self.repo, "add", ".gitignore")
        git(self.repo, "commit", "-q", "-m", "ignore")
        (self.repo / ".graphhelm").mkdir()
        (self.repo / ".graphhelm" / "keel-card.json").write_text("{}")
        self.cli = Path(self.tmp.name) / "graphhelm"
        self.cli.write_text("")
        clean = {k: v for k, v in os.environ.items() if not k.startswith("GRAPHHELM_")}
        patches = [mock.patch.dict(os.environ, clean, clear=True),
                   mock.patch.object(keel_stop_hook.shutil, "which", return_value=str(self.cli))]
        for patch in patches:
            patch.start()
            self.addCleanup(patch.stop)

    def payload(self, **extra):
        return {"cwd": str(self.repo), "session_id": "count-session", **extra}

    def stop(self) -> tuple[Counted, str]:
        """One registered Claude Stop, in-process. The `graphhelm keel check` call is answered by a
        stub (counted, not executed): it is the CLI's work, not the hook's."""
        real = subprocess.run

        def cli_stub(args, *a, **k):
            if str(args[0]) == str(self.cli):
                return subprocess.CompletedProcess(args, 0, json.dumps({"data": {"findings": []}}), "")
            return real(args, *a, **k)
        out = io.StringIO()
        with mock.patch.object(subprocess, "run", cli_stub), Counted() as counted, \
                mock.patch.object(sys, "stdin", io.TextIOWrapper(io.BytesIO(json.dumps(self.payload()).encode()))), \
                mock.patch.object(sys, "stdout", out):
            self.assertEqual(stop_hook.main(), 0)
        return counted, out.getvalue()

    def test_the_claude_stop_is_one_registered_process(self):
        stops = [group for group in json.loads((HERE / "hooks.json").read_text())["hooks"]["Stop"]
                 for _hook in group["hooks"]]
        self.assertEqual(len(stops), 1, "Claude registers one Stop process")
        self.assertIn("/hooks/stop_hook.py", stops[0]["hooks"][0]["command"])

    def test_a_stop_starts_no_interpreter_and_a_warm_stop_three_git_calls(self):
        first, _ = self.stop()
        self.assertEqual(first.interpreters(), [], "no child interpreter (no --scan re-exec)")
        warm, out = self.stop()
        self.assertEqual(out, "")
        self.assertEqual(warm.interpreters(), [])
        git_calls = [call[3:] for call in warm.calls if Path(call[0]).name.startswith("git")]
        # untracked list, numstat, binary diff for the fingerprint; no rev-parse, no merge-base
        self.assertEqual([call[0] for call in git_calls], ["ls-files", "diff", "diff"], warm.calls)
        self.assertEqual(len(warm.calls), 3, "the verdict is cached, so the CLI is not started again")

    def test_the_edit_check_reads_the_merge_base_from_the_cache(self):
        (self.repo / ".graphhelm" / "keel-card.json").unlink()
        tool = {"file_path": str(self.repo / "more.py"), "content": "x = 1\n"}
        with Counted() as cold:
            reason = keel_pretool_hook.decide(keel_stop_hook.toplevel(self.repo), self.repo / "more.py", tool)
        self.assertIsNotNone(reason)
        self.assertIn("merge-base", [call[3] for call in cold.calls])
        with Counted() as warm:
            again = keel_pretool_hook.decide(keel_stop_hook.toplevel(self.repo), self.repo / "more.py", tool)
        self.assertEqual(again, reason)
        self.assertEqual([call[3] for call in warm.calls], ["diff", "ls-files"], warm.calls)
        (self.repo / ".graphhelm" / "keel-card.json").write_text("{}")
        with Counted() as carded:
            self.assertIsNone(keel_pretool_hook.decide(keel_stop_hook.toplevel(self.repo), self.repo / "more.py", tool))
        self.assertEqual(carded.calls, [], "a branch with a card costs no process")

    def test_the_cache_follows_head_and_the_base_ref(self):
        first = keel_stop_hook._merge_base(self.repo)
        cache = self.repo / keel_stop_hook.MERGE_BASE_CACHE
        self.assertEqual(json.loads(cache.read_text())["mergeBase"], first)
        self.assertEqual(git_out(self.repo, "status", "--porcelain"), "", "the cache is never an untracked change")
        (self.repo / "more.py").write_text("y = 2\n")
        git(self.repo, "add", ".")
        git(self.repo, "commit", "-q", "-m", "more")
        with Counted() as moved:
            self.assertEqual(keel_stop_hook._merge_base(self.repo), first)
        self.assertIn("merge-base", [call[3] for call in moved.calls], "a new HEAD asks git again")
        git(self.repo, "update-ref", "refs/remotes/origin/main", "HEAD")
        head = git_out(self.repo, "rev-parse", "HEAD")
        self.assertEqual(keel_stop_hook._merge_base(self.repo), head, "a moved base ref is a new merge base")
        git(self.repo, "pack-refs", "--all")
        with Counted() as packed:
            self.assertEqual(keel_stop_hook._merge_base(self.repo), head)
        self.assertEqual(packed.calls, [], "packed refs are read from packed-refs")

    def test_the_lane_note_scan_runs_in_this_process_under_its_budget(self):
        with mock.patch.object(lane_stop_hook, "binding", return_value=None), \
                mock.patch.object(lane_stop_hook, "pending", return_value=True), Counted() as counted:
            self.assertEqual(lane_stop_hook.evaluate(self.payload()), lane_stop_hook.NOTES_REASON)
        self.assertEqual(counted.calls, [])
        with mock.patch.object(lane_stop_hook, "SCAN_CHILD_TIMEOUT_SECONDS", 0.2), \
                mock.patch.object(lane_stop_hook, "pending", side_effect=lambda _p: __import__("time").sleep(5)):
            self.assertEqual(lane_stop_hook.scan_status(self.payload()), 2, "a hung scan is unknown, not a block")

    def test_every_gate_still_speaks_through_the_one_process(self):
        (self.repo / ".graphhelm" / "keel-card.json").unlink()
        message = "Done. Please test it in the app and let me know if it works."
        out = io.StringIO()
        with mock.patch.object(lane_stop_hook, "binding", return_value=None), \
                mock.patch.object(lane_stop_hook, "pending", return_value=True), \
                mock.patch.object(sys, "stdin", io.TextIOWrapper(io.BytesIO(
                    json.dumps(self.payload(last_assistant_message=message)).encode()))), \
                mock.patch.object(sys, "stdout", out):
            stop_hook.main()
        reason = json.loads(out.getvalue())["reason"]
        self.assertIn("there is no card", reason)
        self.assertIn(lane_stop_hook.NOTES_REASON, reason)
        self.assertIn("hands verification to the user", reason)
        self.assertLess(reason.index("no card"), reason.index("operator_note"))
        self.assertIsNone(handoff_stop_hook.evaluate(self.payload(stop_hook_active=True,
                                                                  last_assistant_message=message)))


def git_out(repo, *args):
    return subprocess.run(["git", "-C", str(repo), *args], check=True, capture_output=True, text=True,
                          creationflags=NO_WINDOW).stdout.strip()


if __name__ == "__main__":
    unittest.main()
