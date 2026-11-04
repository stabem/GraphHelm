"""Observe parallel fixture readiness and owned crash recovery through the public CLI."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import urllib.request

NO_WINDOW = 0x08000000 if os.name == "nt" else 0
WORKFLOW = "fixture-parallel-launch-isolation"

# The synthetic app owns no outside resources. Down requests a cooperative exit instead of
# looking up a PID or using a process-wide kill command.
APP = r'''
import http.server, json, os, pathlib, subprocess, sys, threading, time
root = pathlib.Path(__file__).resolve().parent
mode, directory = sys.argv[1:3]
directory = pathlib.Path(directory)
flow = os.environ.get('GRAPHHELM_JOURNEY_FLOW', '')
if mode == 'up':
    port = int(sys.argv[3])
    info = {'flow': flow, 'namespace': os.environ.get('COMPOSE_PROJECT_NAME'),
            'directory': str(directory), 'port': port}
    (root / (flow + '.up.json')).write_text(json.dumps(info))
    subprocess.Popen([sys.executable, __file__, 'serve', str(directory), str(port), flow],
                     stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                     stderr=subprocess.DEVNULL,
                     creationflags=0x08000000 if os.name == 'nt' else 0)
elif mode == 'down':
    info = {'namespace': os.environ.get('COMPOSE_PROJECT_NAME'), 'directory': str(directory)}
    (root / (directory.name + '.down.json')).write_text(json.dumps(info))
    (directory / 'stop').touch()
    end = time.monotonic() + 5
    while not (directory / 'exited').exists() and time.monotonic() < end:
        time.sleep(.05)
    sys.exit(0 if (directory / 'exited').exists() else 1)
elif mode == 'serve':
    port, flow = int(sys.argv[3]), sys.argv[4]
    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            if self.path == '/hold':
                end = time.monotonic() + 25
                while not (root / (flow + '.release')).exists() and time.monotonic() < end:
                    if (directory / 'stop').exists():
                        return
                    time.sleep(.05)
            body = b'{"status":"ok"}'
            self.send_response(200)
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        def log_message(self, *_): pass
    server = http.server.ThreadingHTTPServer(('127.0.0.1', port), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    end = time.monotonic() + 90
    while not (directory / 'stop').exists() and time.monotonic() < end:
        time.sleep(.05)
    server.shutdown()
    server.server_close()
    (directory / 'exited').touch()
'''


def flow(name):
    return f'''schema: graphhelm.journey-flow/1
id: {name}
title: Parallel isolated fixture
purpose: Observe a ready app while the other isolated preview is recovered.
status: draft
approved: null
base: http://127.0.0.1:1
actors: [client]
secrets: []
risks: []
screens:
  - id: start
    title: Ready
    url: /
    state: stable
    scope: unknown
    expect: [{{role: status, name: "200", because: the app is ready}}]
  - id: finished
    title: Released
    url: /hold
    state: success
    scope: unknown
    expect: [{{role: status, name: "200", because: the owned app completed the request}}]
edges:
  - id: wait
    from: start
    to: finished
    why: Keep both apps alive until the observer releases one.
    acts: [{{kind: http, role: GET, name: /hold}}]
paths:
  main: [wait]
drift: []
'''


def wait_for(check, seconds=15):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        value = check()
        if value:
            return value
        time.sleep(.05)
    raise AssertionError("observer deadline expired")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    # A fresh explicit evidence directory keeps all earlier failed attempts intact.
    args.output.mkdir(parents=True, exist_ok=False)
    project = args.output.resolve() / "project"
    journeys = project / ".graphhelm/journeys"
    journeys.mkdir(parents=True)
    (project / "fixture.py").write_text(APP, encoding="utf-8")
    (project / "fixture.sh").write_text(
        'exec "$GRAPHHELM_PROBE_PYTHON" fixture.py "$1" "$2" "${4:-0}"\n', encoding="utf-8")
    (project / ".graphhelm/journey-fixture.json").write_text(json.dumps({
        "schema": "graphhelm-journey-fixture/1", "script": "fixture.sh", "isolated": True}))
    names = [WORKFLOW + "-a", WORKFLOW + "-b"]
    env = dict(os.environ, GRAPHHELM_PROBE_PYTHON=sys.executable.replace("\\", "/"),
               COMPOSE_PROJECT_NAME="inherited-must-not-be-used")
    env.pop("GRAPHHELM_ACTOR", None)
    receipt = {"workflow": WORKFLOW, "version": 1, "result": "failed", "commands": []}
    workers = []

    def command(argv, cwd=project):
        result = subprocess.run([str(x) for x in argv], cwd=cwd, env=env,
                                capture_output=True, timeout=20, creationflags=NO_WINDOW)
        receipt["commands"].append({"argv": [str(x) for x in argv], "exit": result.returncode,
                                    "stdout": result.stdout.decode(errors="replace"),
                                    "stderr": result.stderr.decode(errors="replace")})
        assert result.returncode == 0, receipt["commands"][-1]
        return result.stdout

    def cli(name, *options):
        return json.loads(command([args.binary, "--json", "journey", "preview", name,
                                   *options, "--project", project]))

    try:
        receipt["candidateSha"] = command(["git", "rev-parse", "HEAD"], args.source).decode().strip()
        receipt["binarySha256"] = hashlib.sha256(args.binary.read_bytes()).hexdigest()
        receipt["observerSha256"] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
        for name in names:
            contract = flow(name)
            (journeys / (name + ".journey.yaml")).write_text(contract, encoding="utf-8")
            command([args.binary, "--json", "journey", "compile", name, "--fmt", "--include-draft", "--project", project])
        receipt["contractDigests"] = {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                                       for p in journeys.glob("*.journey.yaml")}
        for argv in [["git", "init", "--quiet"], ["git", "add", "."],
                     ["git", "-c", "user.name=Fixture observer", "-c", "user.email=observer@example.invalid",
                      "commit", "--quiet", "--no-verify", "-m", "Synthetic fixture"]]:
            command(argv)
        for name in names:
            (project / ".graphhelm/journey-previews" / name).mkdir(parents=True)
            log = (args.output / (name + ".log")).open("wb")
            workers.append(subprocess.Popen([str(args.binary), "--json", "journey", "preview", name,
                           "--run", "--project", str(project)], env=env, stdout=log, stderr=log,
                           creationflags=NO_WINDOW))
            log.close()
        infos = []
        for name in names:
            path = project / (name + ".up.json")
            wait_for(path.exists)
            info = json.loads(path.read_text())
            url = f'http://127.0.0.1:{info["port"]}/'
            def ready():
                try:
                    with urllib.request.urlopen(url, timeout=.5) as response:
                        return response.status == 200
                except OSError:
                    return False
            wait_for(ready)
            info["ready"] = True
            state = project / ".graphhelm/journey-previews" / name / "state.json"
            info["stopRecord"] = json.loads(state.read_text())["launched"]
            infos.append(info)
        assert len({i["namespace"] for i in infos}) == 2
        assert len({i["directory"] for i in infos}) == 2
        assert len({i["port"] for i in infos}) == 2
        assert all(i["namespace"] != env["COMPOSE_PROJECT_NAME"] for i in infos)
        receipt["launches"] = infos
        # Cancel only the runner created above; public read performs ownership-checked recovery.
        workers[0].kill()
        workers[0].wait(timeout=5)
        receipt["cancelA"] = cli(names[0], "--read")
        assert receipt["cancelA"]["data"]["reason"] == "preview.runner_lost"
        assert not Path(infos[0]["directory"]).exists(), "A cleanup incomplete"
        with urllib.request.urlopen(f'http://127.0.0.1:{infos[1]["port"]}/', timeout=2) as response:
            assert response.status == 200
        assert workers[1].poll() is None, "B ended before release"
        receipt["bSurvivesCancelA"] = True
        (project / (names[1] + ".release")).touch()
        assert workers[1].wait(timeout=15) == 0
        receipt["bTerminal"] = cli(names[1], "--read")
        assert receipt["bTerminal"]["data"]["state"] == "ready"
        assert receipt["bTerminal"]["data"]["result"] == "pass"
        assert not Path(infos[1]["directory"]).exists(), "B cleanup incomplete"
        for info in infos:
            down = json.loads((project / (Path(info["directory"]).name + ".down.json")).read_text())
            assert down["namespace"] == info["namespace"]
        receipt["result"] = "passed"
    except Exception as error:
        receipt["error"] = str(error)
        raise
    finally:
        for worker in workers:
            if worker.poll() is None:
                worker.kill()
                worker.wait(timeout=5)
        for name in names:
            try:
                cli(name, "--read")
            except Exception as error:
                receipt.setdefault("cleanupErrors", []).append(str(error))
        (args.output / "receipt.json").write_text(json.dumps(receipt, indent=2), encoding="utf-8")


if __name__ == "__main__":
    main()
