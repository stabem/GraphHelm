"""Observer for the install path: green on a build of the head, red when INSTALL.md or the CLI drifts,
unobserved when the CLI is not a build of the head or of an ancestor with only binary-neutral paths changed."""

import importlib.util
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
RUN = HERE / "run.py"
PAGE = (REPO / "INSTALL.md").read_text(encoding="utf-8")
GREEN = "version=ok init=ok runtime=ok validate=ok"
SPEC = importlib.util.spec_from_file_location("install_first_use_run", RUN)
fixture = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(fixture)


def observe(*args, cli=None):
    env = {**os.environ, "GRAPHHELM_CLI": cli} if cli else None
    return subprocess.run([sys.executable, "-I", str(RUN), *args], capture_output=True, text=True, env=env,
                          stdin=subprocess.DEVNULL, timeout=240,
                          creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))


def edit_line(page, starts_with, old, new):
    lines = page.splitlines()
    hits = [i for i, line in enumerate(lines) if line.strip().startswith(starts_with)]
    assert hits, f"INSTALL.md has no line starting {starts_with!r}"
    lines[hits[0]] = lines[hits[0]].replace(old, new)
    return "\n".join(lines)


def fake_cli(directory, version_output):
    """An executable that answers `--version` with `version_output` and nothing else."""
    if os.name == "nt":
        path = Path(directory) / "graphhelm.cmd"
        path.write_text("@echo off\r\n" + "".join(f"echo {line}\r\n" for line in version_output.splitlines()),
                        encoding="ascii")
    else:
        path = Path(directory) / "graphhelm"
        path.write_text("#!/bin/sh\n" + "".join(f"echo '{line}'\n" for line in version_output.splitlines()),
                        encoding="ascii")
        path.chmod(path.stat().st_mode | stat.S_IXUSR)
    return str(path)


class Provenance(unittest.TestCase):
    """Needs git and a script that can print; no real CLI, so it runs whatever CLI is installed."""

    def unobserved_with(self, version_output):
        with tempfile.TemporaryDirectory() as scratch:
            done = observe(cli=fake_cli(scratch, version_output))
        self.assertEqual(done.returncode, fixture.EXIT_UNOBSERVED, done.stderr)
        self.assertEqual(done.stdout, "")
        self.assertTrue(done.stderr.startswith("UNOBSERVED: cli not built from head:"), done.stderr)
        return done.stderr

    def test_a_cli_that_reports_no_commit_is_unobserved(self):
        self.assertIn("reports no build commit", self.unobserved_with("graphhelm 0.1.1"))

    def test_a_cli_built_outside_a_checkout_is_unobserved(self):
        self.assertIn("commit unknown", self.unobserved_with("graphhelm 0.1.1\ncommit unknown"))

    def test_a_cli_built_from_another_commit_is_unobserved_and_names_both(self):
        head = fixture.head_commit()
        other = "0" * 40 if head != "0" * 40 else "1" * 40
        message = self.unobserved_with(f"graphhelm 0.1.1\ncommit {other}")
        self.assertIn(f"built from {other[:8]}, the head is {head[:8]}", message)

    def test_a_cli_that_reports_the_head_goes_on_to_walk_the_page(self):
        with tempfile.TemporaryDirectory() as scratch:
            done = observe(cli=fake_cli(scratch, f"graphhelm 0.1.1\ncommit {fixture.head_commit()}"))
        self.assertEqual(done.returncode, 1, done.stderr)
        self.assertIn("FAIL init:", done.stderr)
        self.assertNotIn("UNOBSERVED", done.stderr)


NO_WINDOW = getattr(subprocess, "CREATE_NO_WINDOW", 0)


def sh(repo, *args):
    done = subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false",
                           *args], cwd=repo, capture_output=True, text=True, stdin=subprocess.DEVNULL,
                          timeout=60, creationflags=NO_WINDOW)
    assert done.returncode == 0, done.stderr
    return done.stdout.strip()


def commit(repo, files, remove=()):
    for name, text in files.items():
        path = Path(repo) / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
    for name in remove:
        (Path(repo) / name).unlink()
    sh(repo, "add", "-A")
    sh(repo, "commit", "-q", "-m", "c")
    return sh(repo, "rev-parse", "HEAD")


class Ancestry(unittest.TestCase):
    """The build commit may be an ancestor of the head when only binary-neutral paths changed since.
    A real throwaway git repository stands in for the checkout; nothing is mocked but its location."""

    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        self.repo = Path(self.scratch.name) / "repo"
        self.repo.mkdir()
        sh(self.repo, "init", "-q")
        self.base = commit(self.repo, {"Cargo.toml": "a", "apps/cli/src/main.rs": "a", "docs/a.md": "a"})
        patch = mock.patch.object(fixture, "REPO", self.repo)
        patch.start()
        self.addCleanup(patch.stop)

    def gap(self, head):
        return fixture.ancestor_gap(self.base, head)

    def test_only_neutral_paths_changed_is_accepted(self):
        head = commit(self.repo, {"docs/new/b.md": "b", ".graphhelm/journeys/x.journey.yaml": "b",
                                  "apps/studio/src/App.tsx": "b", "tools/studio-journey-fixture/f.sh": "b"})
        self.assertIsNone(self.gap(head))

    def test_a_deleted_neutral_path_is_accepted(self):
        head = commit(self.repo, {}, remove=["docs/a.md"])
        self.assertIsNone(self.gap(head))

    def test_one_unlisted_path_is_refused_and_named(self):
        head = commit(self.repo, {"docs/b.md": "b", "apps/cli/src/main.rs": "changed"})
        self.assertIn("apps/cli/src/main.rs changed since", self.gap(head))

    def test_the_first_unlisted_path_is_the_one_named(self):
        head = commit(self.repo, {"Cargo.toml": "b", "INSTALL.md": "b"})
        self.assertIn("Cargo.toml changed since", self.gap(head))

    def test_a_sibling_directory_sharing_a_prefix_is_not_neutral(self):
        for path in ("docs-x/a.md", "docsx", "apps/studio-x/a.ts", "plugins/a", "INSTALL.md",
                     "examples/graphs/a.yaml", "schemas/a.json", ".graphhelm/fixtures/install-first-use/run.py"):
            head = commit(self.repo, {path: "b"})
            self.assertIn(f"{path} changed since", self.gap(head), path)
            sh(self.repo, "reset", "-q", "--hard", self.base)

    def test_a_rename_out_of_a_neutral_path_names_the_binary_side(self):
        sh(self.repo, "mv", "apps/cli/src/main.rs", "docs/main.rs")
        sh(self.repo, "commit", "-q", "-m", "mv")
        head = sh(self.repo, "rev-parse", "HEAD")
        self.assertIn("apps/cli/src/main.rs changed since", self.gap(head))

    def test_a_commit_that_is_not_an_ancestor_is_refused(self):
        side = commit(self.repo, {"docs/side.md": "s"})
        sh(self.repo, "reset", "-q", "--hard", self.base)
        head = commit(self.repo, {"docs/other.md": "o"})
        self.assertIn("not an ancestor", fixture.ancestor_gap(side, head))

    def test_an_unknown_commit_is_refused(self):
        self.assertIn("not in this checkout", fixture.ancestor_gap("0" * 40, self.base))

    def test_a_reported_value_that_is_not_a_hash_never_reaches_git(self):
        self.assertIn("not a 40-digit hash", fixture.ancestor_gap("--all", self.base))

    def test_a_checkout_git_cannot_read_is_refused(self):
        with mock.patch.object(fixture, "REPO", Path(self.scratch.name)):
            self.assertIsNotNone(self.gap(self.base))

    def test_a_git_that_times_out_is_refused(self):
        head = commit(self.repo, {"docs/b.md": "b"})
        with mock.patch.object(fixture, "git", side_effect=subprocess.TimeoutExpired("git", 1)):
            self.assertIn("git did not answer", self.gap(head))

    def test_the_whole_check_reads_the_cli_and_the_head(self):
        head = commit(self.repo, {"docs/b.md": "b"})
        with tempfile.TemporaryDirectory() as scratch, mock.patch.dict(
                os.environ, {"GRAPHHELM_CLI": fake_cli(scratch, f"graphhelm 0.1.1{chr(10)}commit {self.base}")}):
            self.assertIsNone(fixture.provenance_gap())
            commit(self.repo, {"core/graph/src/lib.rs": "x"})
            self.assertIn("core/graph/src/lib.rs changed since", fixture.provenance_gap())
        self.assertEqual(head, sh(self.repo, "rev-parse", "HEAD~1"))


def real_cli_gap():
    """None when the CLI the plan would drive is a build of the head, else why it is not."""
    try:
        return fixture.provenance_gap()
    except fixture.ObserverMissing as error:
        return str(error)


class InstallFirstUse(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        gap = real_cli_gap()
        if gap:
            raise unittest.SkipTest(f"UNOBSERVED: cli not built from head: {gap}")

    def observe_page(self, page):
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch) / "INSTALL.md"
            path.write_text(page, encoding="utf-8")
            return observe("--install-md", str(path))

    def test_documented_path_is_green_on_the_real_cli(self):
        done = observe()
        self.assertEqual((done.returncode, done.stdout.strip()), (0, GREEN), done.stderr)

    def test_a_flag_init_refuses_turns_the_run_red_at_init(self):
        done = self.observe_page(edit_line(PAGE, "graphhelm init ", "--harness claude-code",
                                           "--harness claude-code --no-such-flag"))
        self.assertEqual(done.returncode, 1)
        self.assertIn("FAIL init:", done.stderr)

    def test_a_flag_serve_refuses_turns_the_run_red_at_serve(self):
        done = self.observe_page(edit_line(PAGE, "graphhelm serve ", "--bind", "--bind-nowhere"))
        self.assertEqual(done.returncode, 1)
        self.assertIn("FAIL serve:", done.stderr)

    def test_a_page_without_the_init_command_names_what_is_missing(self):
        done = self.observe_page(PAGE.replace("graphhelm init ", "graphhelm initialise "))
        self.assertEqual(done.returncode, 1)
        self.assertIn("no longer shows a `graphhelm init` line", done.stderr)

    def test_a_graph_validate_rejects_turns_the_run_red_at_validate(self):
        with tempfile.TemporaryDirectory() as scratch:
            graph = Path(scratch) / "broken.yaml"
            graph.write_text("apiVersion: p50.dev/graph/v1\nkind: ExecutionGraph\n", encoding="utf-8")
            done = observe("--graph", str(graph))
        self.assertEqual(done.returncode, 1)
        self.assertIn("FAIL validate:", done.stderr)


def main():
    outcome = unittest.main(exit=False, argv=[sys.argv[0]]).result
    if not outcome.wasSuccessful():
        return 1
    gap = real_cli_gap()
    if gap:
        # The runner reads this line and reports the step unobserved, never passed.
        print(f"UNOBSERVED: cli not built from head: {gap}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
