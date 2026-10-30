"""Observer for the install path: green on a build of the head, red when INSTALL.md or the CLI drifts,
unobserved when the CLI is not a build of the head."""

import importlib.util
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import unittest

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
