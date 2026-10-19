"""Hidden launcher-path probe for the no-window rule (#1290).

Builds a disposable project whose declared launcher is tools/no-window/fixture/fixture.sh, then runs
`journey preview --force` and reads it until it ends, RUNS times. Every child this script starts
runs with CREATE_NO_WINDOW, as a hidden host starts the CLI, so a console window seen while it runs
comes from GraphHelm's own tree (preview runner -> launcher shell -> fixture child), never from this
driver. Run it under the observer, which needs no opt-in:

    pwsh tools/no-window/observe.ps1 -Exe <python.exe> -Arguments @('tools/no-window/launcher_probe.py', '<graphhelm.exe>', '3')

Exits 0 only when every run ended `ready` and reached the launcher's `up` and `down`; anything else
is a probe failure, not a window verdict (the observer prints the window count either way).
"""

from __future__ import annotations

import json
import os
import pathlib
import shutil
import stat
import subprocess
import sys
import tempfile
import time

NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
REPO = pathlib.Path(__file__).resolve().parents[2]
FLOW = """schema: graphhelm.journey-flow/1
id: nowindow
title: Fixture service answers its health check
purpose: A client sees the fixture service report that it is healthy.
status: draft
approved: null
base: http://127.0.0.1:8765
actors: [client]
secrets: []
risks: []
screens:
  - id: start
    title: Before any request
    url: /
    state: stable
    expect: [{role: status, name: "404"}]
    scope: unknown
  - id: healthy
    title: The health answer
    url: /health.json
    state: success
    expect: [{role: status, name: "200"}, {role: json, name: status=ok}]
    scope: unknown
edges:
  - id: check
    from: start
    to: healthy
    acts:
      - {kind: http, role: GET, name: /health.json}
paths:
  main: [check]
drift: []
"""


def main(cli: str, runs: int) -> int:
    project = pathlib.Path(tempfile.mkdtemp(prefix="gh-nowindow-launch-"))
    marks = project / "marks.txt"
    env = {**os.environ, "NOWINDOW_MARKS": str(marks)}

    def run(*args: str, check: bool = True) -> subprocess.CompletedProcess:
        done = subprocess.run(list(args), cwd=project, env=env, capture_output=True, text=True,
                              timeout=300, check=False, creationflags=NO_WINDOW)
        if check and done.returncode:
            raise SystemExit(f"probe setup failed: {args[1:4]} exit {done.returncode}: {done.stdout[-400:]}")
        return done

    try:
        (project / ".graphhelm/journeys").mkdir(parents=True)
        for name in ("fixture.sh", "server.mjs"):
            shutil.copy(REPO / "tools/no-window/fixture" / name, project / name)
        (project / ".graphhelm/journey-fixture.json").write_text(
            '{"schema":"graphhelm-journey-fixture/1","script":"fixture.sh","isolated":true}', encoding="utf-8")
        (project / ".graphhelm/journeys/nowindow.journey.yaml").write_text(FLOW, encoding="utf-8", newline="\n")
        run("git", "init", "-q")
        run("git", "config", "user.email", "nowindow@example.invalid")
        run("git", "config", "user.name", "nowindow")
        run(cli, "--json", "init", "--harness", "claude-code", "--project", ".")
        run(cli, "--json", "journey", "compile", "nowindow", "--fmt", "--include-draft", "--project", ".")
        run("git", "add", "-A")
        run("git", "commit", "-q", "-m", "fixture")
        ended = []
        for attempt in range(runs):
            run(cli, "--json", "journey", "preview", "nowindow", "--force", "--project", ".", check=False)
            state = {}
            for _ in range(120):
                reply = run(cli, "--json", "journey", "preview", "nowindow", "--read", "--project", ".", check=False)
                state = json.loads(reply.stdout or "{}").get("data") or {}
                if state.get("state") != "running":
                    break
                time.sleep(1)
            ended.append(state.get("state"))
            print(f"run {attempt}: {state.get('state')} {state.get('result')} {state.get('reason')}", flush=True)
        steps = marks.read_text(encoding="utf-8").split() if marks.exists() else []
        ups, downs = steps.count("up"), steps.count("down")
        print(f"launcher steps: up {ups}, down {downs}", flush=True)
        return 0 if ended == ["ready"] * runs and ups == runs and downs == runs else 1
    finally:
        # Git writes its objects read-only, which Windows refuses to delete until the bit is cleared.
        def writable(remove, path, _error):
            os.chmod(path, stat.S_IWRITE)
            remove(path)

        shutil.rmtree(project, onexc=writable) if sys.version_info >= (3, 12) else shutil.rmtree(project, onerror=writable)


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit("usage: launcher_probe.py <graphhelm.exe> <runs>")
    sys.exit(main(sys.argv[1], int(sys.argv[2])))
