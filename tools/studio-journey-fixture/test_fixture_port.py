"""The journey fixture must refuse the protected Runtime before any side effect."""

import json
import os
import pathlib
import subprocess
import tempfile
import time
import unittest

# Owner rule: children run hidden in the calling terminal, never in a new console window.
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0


class FixturePortTest(unittest.TestCase):
    def test_protected_runtime_port_is_refused_before_fixture_creation(self):
        root = pathlib.Path(__file__).resolve().parents[2]
        shell = r"C:\Program Files\Git\bin\bash.exe" if os.name == "nt" else "bash"
        with tempfile.TemporaryDirectory() as parent:
            fixture = pathlib.Path(parent) / "fixture"
            result = subprocess.run(
                [shell, str(root / "tools/studio-journey-fixture/fixture.sh"),
                 "up", str(fixture), "8793", "5184"],
                cwd=root, capture_output=True, text=True, timeout=10, check=False, creationflags=NO_WINDOW
            )
            self.assertEqual(result.returncode, 2, result.stderr)
            self.assertIn("unsafe Runtime port", result.stderr)
            self.assertFalse(fixture.exists(), "refusal must precede fixture creation")


    def test_wait_gives_up_at_once_when_the_studio_process_is_gone(self):
        # #1448: a Studio that died at start (a broken vite install) never answers; the launcher
        # waited out its 120 s deadline and reported only a git warning. The wait returns 2 as
        # soon as the process is gone, so fixture.sh can print the Studio's own stderr tail.
        root = pathlib.Path(__file__).resolve().parents[2]
        shell = r"C:\Program Files\Git\bin\bash.exe" if os.name == "nt" else "bash"
        script = (
            f'. "{(root / "tools/studio-journey-fixture/wait.sh").as_posix()}"; '
            "sleep 0 & pid=$!; wait $pid; "
            "wait_answers http://127.0.0.1:9/never $pid 30; echo $?"
        )
        started = time.monotonic()
        result = subprocess.run(
            [shell, "-c", script], capture_output=True, text=True, timeout=25, check=False,
            creationflags=NO_WINDOW,
        )
        self.assertEqual(result.stdout.strip(), "2", result.stderr)
        self.assertLess(time.monotonic() - started, 10)


    def test_the_fixture_project_carries_every_file_its_flows_scope(self):
        # #1041: fixture.sh copies the journeys into a disposable project (#585) and the Runtime
        # resolves each screen `scope` path against that project, so without the scoped files
        # every fixture preview was refused flow.scope_path_missing before it ran. The control
        # first shows the refusal on the bare copy, so a green result cannot be vacuous; a
        # planted `..` entry shows the copy never writes outside the project.
        import shutil

        root = pathlib.Path(__file__).resolve().parents[2]
        cli = os.environ.get("GRAPHHELM_CLI") or shutil.which("graphhelm")
        if not cli:
            self.skipTest("OBSERVER_MISSING: no graphhelm CLI (set GRAPHHELM_CLI)")
        node = shutil.which("node")
        if not node:
            self.skipTest("OBSERVER_MISSING: node is not on PATH")

        def missing(project):
            reply = subprocess.run(
                [cli, "--json", "journey", "flows", "--project", str(project)],
                capture_output=True, text=True, timeout=120, check=False, creationflags=NO_WINDOW,
            )
            return {
                flow["id"]
                for flow in json.loads(reply.stdout)["data"]["flows"]
                if flow["id"].startswith("studio2-")
                and any(f.get("code") == "flow.scope_path_missing" for f in flow.get("findings", []))
            }

        with tempfile.TemporaryDirectory() as parent:
            project = pathlib.Path(parent) / "project"
            journeys = project / ".graphhelm" / "journeys"
            shutil.copytree(root / ".graphhelm" / "journeys", journeys)
            subprocess.run(["git", "-C", str(project), "init", "-q"], check=True, creationflags=NO_WINDOW)
            (journeys / "zz-planted.journey.yaml").write_text("scope: [../escape.txt]\n", encoding="utf-8")

            before = missing(project)
            self.assertTrue(before, "control: the bare copy must show flow.scope_path_missing")

            copied = subprocess.run(
                [node, str(root / "tools/studio-journey-fixture/copy_scope.mjs"), str(root), str(project)],
                capture_output=True, text=True, timeout=60, check=False, creationflags=NO_WINDOW,
            )
            self.assertEqual(copied.returncode, 0, copied.stderr)
            self.assertIn("skipped (outside the project): ../escape.txt", copied.stderr)
            self.assertFalse((pathlib.Path(parent) / "escape.txt").exists())

            self.assertEqual(missing(project), set(), "every studio flow's scope must resolve")

    def test_the_fixture_project_can_run_the_browser_observer(self):
        # #1448: a preview the fixture's Runtime starts (the Studio's Run again) runs in the fixture
        # project, so the observer must be there: the driver and judge as `setup --install-observer`
        # writes them, and @playwright/test resolvable from the project (the runner clears NODE_PATH).
        # Without them every fixture preview ended replay.observer_missing / driver.observer_missing.
        import shutil
        import socket

        root = pathlib.Path(__file__).resolve().parents[2]
        cli = os.environ.get("GRAPHHELM_CLI") or shutil.which("graphhelm")
        node, npm = shutil.which("node"), shutil.which("npm")
        if not cli or not node or not npm or not (root / "apps/studio/node_modules").is_dir():
            self.skipTest("OBSERVER_MISSING: needs GRAPHHELM_CLI, node, npm and apps/studio/node_modules")
        shell = r"C:\Program Files\Git\bin\bash.exe" if os.name == "nt" else "bash"
        script = str(root / "tools/studio-journey-fixture/fixture.sh")

        def free_port():
            with socket.socket() as probe:
                probe.bind(("127.0.0.1", 0))
                return probe.getsockname()[1]

        env = {**os.environ, "GRAPHHELM_BIN": cli}
        with tempfile.TemporaryDirectory() as parent:
            fixture = pathlib.Path(parent) / "fixture"
            try:
                up = subprocess.run([shell, script, "up", str(fixture), str(free_port()), str(free_port())],
                                    cwd=root, env=env, capture_output=True, text=True, timeout=600,
                                    check=False, creationflags=NO_WINDOW)
                self.assertEqual(up.returncode, 0, up.stdout[-2000:] + up.stderr[-2000:])
                project = fixture / "project"
                observers = project / ".graphhelm" / "observers"
                for installed, source in (("journey_driver.mjs", "driver.mjs"), ("judge.mjs", "judge.mjs")):
                    self.assertEqual((observers / installed).read_bytes(),
                                     (root / "tools/journey-driver" / source).read_bytes(), installed)
                resolved = subprocess.run(
                    [node, "-e", "console.log(require.resolve('@playwright/test', {paths: [process.argv[1]]}))",
                     str(project)],
                    capture_output=True, text=True, timeout=60, check=False, creationflags=NO_WINDOW)
                self.assertEqual(resolved.returncode, 0, resolved.stderr)
                self.assertIn(str(project.resolve()).lower(), resolved.stdout.strip().lower(),
                              "@playwright/test must resolve inside the fixture project, not from a parent")
            finally:
                subprocess.run([shell, script, "down", str(fixture)], cwd=root, env=env, capture_output=True,
                               text=True, timeout=120, check=False, creationflags=NO_WINDOW)


if __name__ == "__main__":
    unittest.main()
