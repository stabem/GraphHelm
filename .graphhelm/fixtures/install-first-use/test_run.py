"""Observer for the install path: green on the real CLI, red when INSTALL.md or the CLI drifts."""

import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
RUN = HERE / "run.py"
PAGE = (REPO / "INSTALL.md").read_text(encoding="utf-8")
GREEN = "version=ok init=ok runtime=ok validate=ok"


def observe(*args):
    return subprocess.run([sys.executable, "-I", str(RUN), *args], capture_output=True, text=True,
                          stdin=subprocess.DEVNULL, timeout=240,
                          creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))


def edit_line(page, starts_with, old, new):
    lines = page.splitlines()
    hits = [i for i, line in enumerate(lines) if line.strip().startswith(starts_with)]
    assert hits, f"INSTALL.md has no line starting {starts_with!r}"
    lines[hits[0]] = lines[hits[0]].replace(old, new)
    return "\n".join(lines)


class InstallFirstUse(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not (os.environ.get("GRAPHHELM_CLI") or shutil.which("graphhelm")):
            raise unittest.SkipTest("OBSERVER_MISSING: GRAPHHELM_CLI or graphhelm on PATH is required")

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


if __name__ == "__main__":
    unittest.main()
