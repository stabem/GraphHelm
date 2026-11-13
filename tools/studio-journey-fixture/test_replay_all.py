"""replay-all.sh gives every viewport of a multi-viewport flow a fresh fixture (#2903)."""

import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest

# Owner rule: children run hidden in the calling terminal, never in a new console window.
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
HERE = pathlib.Path(__file__).resolve().parent
SHELL = r"C:\Program Files\Git\bin\bash.exe" if os.name == "nt" else "bash"

STUB_FIXTURE = """#!/usr/bin/env bash
# up <dir> <runtime port> <studio port> | down <dir>: logs the call, starts nothing.
echo "fixture $1 $2 ${3:-} ${4:-} flow=${GRAPHHELM_JOURNEY_FLOW:-}" >> "$CALLS"
if [ "$1" = up ]; then
  mkdir -p "$2/.graphhelm"; printf 'GRAPHHELM_SECRET_STUDIO_TOKEN=t\n' > "$2/.graphhelm/secrets.env"
fi
"""

STUB_BIN = """#!/usr/bin/env bash
# Logs its arguments; a replay of the viewport named in FAIL_VIEW fails.
echo "replay $*" >> "$CALLS"
case " $* " in *" --viewport $FAIL_VIEW "*) echo '{"ok":false,"data":{"paths":[]},"diagnostics":[]}'; exit 1;; esac
echo '{"ok":true,"data":{"paths":[{"name":"main","outcome":"passed"}]},"diagnostics":[]}'
"""


class ReplayAllTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = pathlib.Path(self.tmp.name)
        tools = root / "repo/tools/studio-journey-fixture"
        tools.mkdir(parents=True)
        shutil.copy(HERE / "replay-all.sh", tools / "replay-all.sh")
        (tools / "fixture.sh").write_text(STUB_FIXTURE, newline="\n")
        journeys = root / "repo/.graphhelm/journeys"
        journeys.mkdir(parents=True)
        (journeys / "both.journey.yaml").write_text("id: both\nviewports: [web, mobile]\n", newline="\n")
        (journeys / "solo.journey.yaml").write_text("id: solo\nviewports: [web]\n", newline="\n")
        (root / "bin").write_text(STUB_BIN, newline="\n")
        given = root / "given"
        (given / "project/.graphhelm").mkdir(parents=True)
        (given / "project/.graphhelm/serve.key").write_text("k\n")
        (given / ".graphhelm").mkdir()
        (given / ".graphhelm/secrets.env").write_text("GRAPHHELM_SECRET_STUDIO_TOKEN=t\n")
        self.root, self.script = root, tools / "replay-all.sh"
        self.calls = root / "calls.log"

    def tearDown(self):
        self.tmp.cleanup()

    def run_script(self, *args, fail_view="", port=None):
        env = dict(os.environ, CALLS=str(self.calls), FAIL_VIEW=fail_view,
                   GRAPHHELM_BIN=f"{SHELL} {(self.root / 'bin').as_posix()}")
        env.pop("REPLAY_RUNTIME_PORT", None)
        if port:
            env["REPLAY_RUNTIME_PORT"] = port
        # GRAPHHELM_BIN is one word in the script, so point it at an executable wrapper.
        wrapper = self.root / "graphhelm"
        wrapper.write_text(f'#!/usr/bin/env bash\nexec bash "{(self.root / "bin").as_posix()}" "$@"\n', newline="\n")
        env["GRAPHHELM_BIN"] = wrapper.as_posix()
        return subprocess.run(
            [SHELL, str(self.script), str(self.root / "given"), str(self.root / "out"), *args],
            capture_output=True, text=True, timeout=60, check=False, env=env, creationflags=NO_WINDOW,
        )

    def calls_made(self):
        # Git Bash may show the temp directory as /tmp/<name>; spacing of empty ports is not asserted.
        shown = (self.root.as_posix(), f"/tmp/{self.root.name}")
        lines = []
        for line in self.calls.read_text().splitlines():
            for prefix in shown:
                line = line.replace(prefix, "<tmp>")
            lines.append(" ".join(line.split()))
        return lines

    def test_each_viewport_replays_alone_on_a_fresh_fixture_mobile_first_and_web_last(self):
        result = self.run_script("both")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("green=1 red=0", result.stdout)
        calls = self.calls_made()
        self.assertEqual(calls[0], "fixture down <tmp>/given flow=")
        self.assertEqual(
            calls[1:],
            [
                "fixture up <tmp>/out/fixtures/both-mobile 8811 5184 flow=both",
                "replay --json journey replay both --project <tmp>/repo --viewport mobile",
                "fixture down <tmp>/out/fixtures/both-mobile flow=",
                "fixture up <tmp>/out/fixtures/both-web 8811 5184 flow=both",
                "replay --json journey replay both --project <tmp>/repo --viewport web",
                "fixture down <tmp>/out/fixtures/both-web flow=",
            ],
        )

    def test_a_red_viewport_makes_the_flow_red_and_the_later_ones_are_not_run(self):
        result = self.run_script("both", fail_view="mobile")
        self.assertIn("green=0 red=1", result.stdout)
        self.assertIn("not run, mobile was red: web", result.stdout)
        replays = [c for c in self.calls_made() if c.startswith("replay ")]
        self.assertEqual(len(replays), 1, "a red replay voids the approval; web would only be refused")
        self.assertEqual((self.root / "out/both.json").read_text(), (self.root / "out/both.mobile.json").read_text())

    def test_the_last_viewport_red_makes_the_flow_red_too(self):
        result = self.run_script("both", fail_view="web")
        self.assertIn("green=0 red=1", result.stdout)
        self.assertEqual(sum(1 for c in self.calls_made() if c.startswith("replay ")), 2)
        self.assertEqual((self.root / "out/both.json").read_text(), (self.root / "out/both.web.json").read_text())

    def test_a_flow_of_one_viewport_replays_on_the_given_fixture_and_records(self):
        result = self.run_script("solo")
        self.assertIn("green=1 red=0", result.stdout)
        [call] = self.calls_made()
        self.assertIn("--events <tmp>/given/project/.graphhelm/events", call)
        self.assertIn("--key-id studio", call)
        self.assertNotIn("--viewport", call)

    def test_a_port_that_belongs_to_another_fixture_is_refused_before_anything_starts(self):
        for port in ("8793", "8795", "8797", "5183", "5184"):
            result = self.run_script("both", port=port)
            self.assertEqual(result.returncode, 2, f"{port}: {result.stderr}")
            self.assertFalse(self.calls.exists(), f"{port}: nothing may start")


if __name__ == "__main__":
    unittest.main()
