#!/usr/bin/env python3
"""Walk the INSTALL.md path to the first useful commands in a throwaway project and a private port.

The commands come from INSTALL.md itself (`init`, `serve`), so a flag the page still shows but the
CLI no longer accepts turns this run red. Prints `version=ok init=ok runtime=ok validate=ok` on success.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shlex
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

NO_WINDOW = getattr(subprocess, "CREATE_NO_WINDOW", 0)
STEP_SECONDS = 60
SERVE_SECONDS = 30
REPO = Path(__file__).resolve().parents[3]


class StepFailed(Exception):
    """A documented step did not do what INSTALL.md says it does."""


class ObserverMissing(Exception):
    """Something this run needs is absent, so nothing may be claimed."""


def graphhelm() -> str:
    binary = os.environ.get("GRAPHHELM_CLI") or shutil.which("graphhelm")
    if not binary:
        raise ObserverMissing("GRAPHHELM_CLI or graphhelm on PATH is required")
    return binary


def documented(page: str, verb: str, must_have: str, must_lack: str = "") -> list[str]:
    """The first INSTALL.md line that runs `graphhelm <verb>` with `must_have`, as an argv."""
    for line in page.splitlines():
        text = line.strip()
        if text.startswith(f"graphhelm {verb} ") and must_have in text and not (must_lack and must_lack in text):
            return shlex.split(text)[1:]
    raise StepFailed(f"INSTALL.md no longer shows a `graphhelm {verb}` line with `{must_have}`; "
                     "update this fixture with the page")


def fill(argv: list[str], project: Path, port: int) -> list[str]:
    out = []
    for item in argv:
        item = item.replace("<project>", project.as_posix())
        out.append("--json" if item == "--pretty" else item.replace("127.0.0.1:8791", f"127.0.0.1:{port}"))
    return out


def envelope(argv: list[str], cwd: Path, env: dict[str, str], step: str, command: str) -> dict:
    try:
        done = subprocess.run([graphhelm(), *argv], cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                              capture_output=True, timeout=STEP_SECONDS, creationflags=NO_WINDOW)
    except subprocess.TimeoutExpired as error:
        raise StepFailed(f"{step}: no answer in {STEP_SECONDS}s: {shlex.join(argv)}") from error
    text = done.stdout.decode("utf-8", errors="replace")
    try:
        reply = json.loads(text)
    except ValueError as error:
        raise StepFailed(f"{step}: exit {done.returncode}, not a JSON reply: "
                         f"{(text or done.stderr.decode('utf-8', errors='replace'))[:300]}") from error
    if done.returncode or reply.get("ok") is not True or reply.get("command") != command:
        raise StepFailed(f"{step}: exit {done.returncode}, wanted ok:true command:{command}, "
                         f"got {json.dumps(reply)[:300]}")
    return reply


def free_port() -> int:
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


def get(url: str, headers: dict[str, str] | None = None) -> dict:
    request = urllib.request.Request(url, headers=headers or {})
    with urllib.request.urlopen(request, timeout=5) as reply:
        return json.loads(reply.read())


def first_line(path: Path, server: subprocess.Popen, seconds: int) -> str:
    """The first line the server wrote, or what it wrote before exiting, or "" after `seconds`."""
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        exited = server.poll() is not None
        text = path.read_bytes() if path.exists() else b""
        if b"\n" in text or (exited and text):
            return text.split(b"\n", 1)[0].decode("utf-8", errors="replace")
        if exited:
            return ""
        time.sleep(0.2)
    return ""


def runtime(argv: list[str], project: Path, port: int, env: dict[str, str]) -> None:
    key = (project / ".graphhelm" / "serve.key").read_text(encoding="utf-8").strip()
    token = (project / ".graphhelm" / "events.token").read_text(encoding="utf-8").strip()
    log = project.parent / "serve.out"
    server = subprocess.Popen([graphhelm(), *argv], cwd=project, env={**env, "GRAPHHELM_EVENTS_KEY": key},
                              stdin=subprocess.DEVNULL, stdout=log.open("wb"), stderr=subprocess.STDOUT,
                              creationflags=NO_WINDOW)
    try:
        started = first_line(log, server, SERVE_SECONDS)
        try:
            reply = json.loads(started)
        except ValueError as error:
            raise StepFailed(f"serve: no serve.started line (exit {server.poll()}): {started[:300]!r}") from error
        warnings = [d for d in reply.get("diagnostics", []) if d.get("severity") != "info"]
        if reply.get("command") != "serve.started" or reply.get("ok") is not True or warnings:
            raise StepFailed(f"serve: wanted a clean serve.started, got {started[:300]}")
        base = f"http://127.0.0.1:{port}"
        deadline = time.monotonic() + SERVE_SECONDS
        while True:
            try:
                health = get(f"{base}/health")
                break
            except (OSError, ValueError) as error:
                if time.monotonic() > deadline:
                    raise StepFailed(f"serve: /health did not answer in {SERVE_SECONDS}s: {error}") from error
                time.sleep(0.3)
        data = health.get("data") or {}
        if health.get("command") != "serve.health" or not data.get("projectId") or not data.get("instance"):
            raise StepFailed(f"serve: /health lacks projectId and instance: {json.dumps(health)[:300]}")
        try:
            listed = get(f"{base}/v1/executions?limit=5", {"Authorization": f"Bearer {token}"})
        except (OSError, ValueError) as error:
            raise StepFailed(f"serve: the init token was refused on /v1/executions: {error}") from error
        if listed.get("command") != "execution.list":
            raise StepFailed(f"serve: /v1/executions did not answer execution.list: {json.dumps(listed)[:300]}")
    finally:
        server.terminate()
        try:
            server.wait(timeout=10)
        except subprocess.TimeoutExpired:
            server.kill()
            server.wait(timeout=10)


def walk(page: str, graph: Path) -> list[str]:
    init_argv = documented(page, "init", "--harness claude-code", "codex")
    serve_argv = documented(page, "serve", "--bind")
    done = []
    with tempfile.TemporaryDirectory(prefix="install-first-use-") as scratch:
        project = Path(scratch) / "project"
        project.mkdir()
        port = free_port()
        env = {**os.environ, "GRAPHHELM_RUNTIME_DIR": str(Path(scratch) / "runtime")}
        env.pop("GRAPHHELM_EVENTS_KEY", None)
        version = subprocess.run([graphhelm(), "--version"], capture_output=True, timeout=STEP_SECONDS,
                                 stdin=subprocess.DEVNULL, creationflags=NO_WINDOW)
        if version.returncode or not version.stdout.startswith(b"graphhelm "):
            raise StepFailed(f"version: wanted `graphhelm <version>`, got {version.stdout[:100]!r}")
        done.append("version=ok")
        envelope(fill(init_argv, project, port), project, env, "init", "init")
        missing = [p for p in ("events", "events.token", "serve.key", "keyring")
                   if not (project / ".graphhelm" / p).exists()]
        if missing:
            raise StepFailed(f"init: INSTALL.md promises {missing} under .graphhelm/ and they are absent")
        done.append("init=ok")
        runtime(fill(serve_argv, project, port), project, port, env)
        done.append("runtime=ok")
        envelope(["--json", "graph", "validate", str(graph)], REPO, env, "validate", "graph.validate")
        done.append("validate=ok")
    return done


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--install-md", type=Path, default=REPO / "INSTALL.md")
    parser.add_argument("--graph", type=Path, default=REPO / "examples" / "graphs" / "software-feature.yaml")
    args = parser.parse_args()
    try:
        steps = walk(args.install_md.read_text(encoding="utf-8"), args.graph)
    except ObserverMissing as error:
        print(f"OBSERVER_MISSING: {error}", file=sys.stderr)
        return 70
    except StepFailed as error:
        print(f"FAIL {error}", file=sys.stderr)
        return 1
    except (OSError, subprocess.SubprocessError) as error:
        print(f"FAIL fixture: {error}", file=sys.stderr)
        return 1
    print(" ".join(steps))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
