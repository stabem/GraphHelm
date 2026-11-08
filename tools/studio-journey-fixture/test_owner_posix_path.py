"""#2607: the Runtime ownership check, and the rule that every netstat and taskkill call in
fixture.sh names its System32 path. One rule (bare_calls) is used by the fixture scan and by the
regression probes, so a probe that passes the rule proves the fixture scan does too.

Needs bash and PowerShell, not Node or Docker.
"""
import os
import re
import socket
import subprocess
import sys
import tempfile
import time
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(HERE, "fixture.sh")
NO_WINDOW = getattr(subprocess, "CREATE_NO_WINDOW", 0)

PROBES = [
    'taskkill //PID $p //F && "C:/Windows/System32/netstat.exe" -ano',
    'C:/Windows/System32/taskkill.exe //PID 1; taskkill //PID 2 //F',
    '"$taskkill_exe" //PID 1; taskkill //PID 2',
    '"$taskkill_exe" //PID 1 || true; taskkill //PID 3 || true',
    'echo System32 $(netstat -ano)',
    'grep System32 <(netstat -ano)',
    'echo System32 taskkill //PID 1',
    'echo C:/Windows/System32/ netstat -ano',
]

CALL = re.compile(r"\b(netstat|taskkill)(\.exe)?\b", re.IGNORECASE)
SEPARATOR = re.compile(r";|\|\||\||&&|&|\$\(|<\(|\(")
FULL_PATH_END = re.compile(r'(System32[/\\]+[A-Za-z.]*[/\\]?|taskkill_exe)$')


def bare_calls(text):
    """The netstat or taskkill calls in `text` not named by their full path.

    A call passes only when the text between the previous separator (or line start) and the call
    ends directly in the full-path form: a System32 path (or taskkill_exe), with nothing between it
    and the command word.
    """
    found = []
    for match in CALL.finditer(text):
        start = max((m.end() for m in SEPARATOR.finditer(text, 0, match.start())), default=0)
        own = text[start:match.start()]
        if not FULL_PATH_END.search(own):
            found.append(match.group(0))
    return found


def function_source(name):
    text = open(SCRIPT, encoding="utf-8").read()
    start = text.index(f"{name}() {{\n")
    ps = text.index("\nPS\n", start) + len("\nPS\n")
    return text[start:text.index("\n}\n", ps) + 3]


def bash_pwd(windows_path):
    """The path the way bash's `pwd` names it, which is the form the launcher hands the Runtime."""
    out = subprocess.run(["bash", "-c", f'cd "{windows_path.replace(chr(92), "/")}" && pwd'],
                         capture_output=True, text=True, timeout=30, creationflags=NO_WINDOW)
    return out.stdout.strip()


class OwnerPosixPath(unittest.TestCase):
    def test_a_listener_started_with_the_posix_path_is_owned(self):
        fixture = tempfile.mkdtemp(prefix="fx-")
        project = os.path.join(fixture, "project")
        os.makedirs(project)
        sock = socket.socket()
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
        sock.close()
        child = subprocess.Popen(
            [sys.executable, "-c",
             "import http.server,socketserver\n"
             "class H(http.server.BaseHTTPRequestHandler):\n"
             " def do_GET(self):\n  self.send_response(200);self.end_headers()\n"
             f"socketserver.TCPServer(('127.0.0.1',{port}),H).serve_forever()",
             bash_pwd(os.path.abspath(project))],
            creationflags=NO_WINDOW,
        )
        try:
            time.sleep(2)
            win_fixture = os.path.abspath(fixture).replace("\\", "/")
            win_repo = os.path.abspath(os.path.join(HERE, "..", "..")).replace("\\", "/")
            script = (
                function_source("fixture_listeners")
                + f'\ndir=$(cd "{win_fixture}" && pwd)\nrepo="{win_repo}"\n'
                + f"fixture_listeners record {port}\n"
            )
            env = dict(os.environ)
            env["PATH"] = "/usr/bin:/bin:/c/Windows/System32/WindowsPowerShell/v1.0"
            env["PATHEXT"] = ".CPL"  # the spawned environment carries only this (#2607 live log)
            run = subprocess.run(["bash", "-c", script], capture_output=True, text=True,
                                 timeout=90, creationflags=NO_WINDOW, env=env)
            out = run.stdout + run.stderr
            self.assertIn(str(child.pid), out, f"owner not found for pid {child.pid}: {out!r}")
        finally:
            child.kill()
            child.wait()

    def test_no_bare_taskkill_in_the_fixture(self):
        """#2607 (Shino): every taskkill call names the System32 path."""
        for number, line in enumerate(open(SCRIPT, encoding="utf-8"), 1):
            if line.lstrip().startswith("#"):
                continue
            found = [c for c in bare_calls(line.split(" #", 1)[0]) if c.lower().startswith("taskkill")]
            self.assertFalse(found, f"fixture.sh:{number} calls taskkill by bare name: {line.strip()}")

    def test_every_netstat_and_taskkill_call_is_by_full_path(self):
        """#2607 (Tsunade, Shino): the shared rule passes every netstat and taskkill call in the fixture."""
        for number, line in enumerate(open(SCRIPT, encoding="utf-8"), 1):
            if line.lstrip().startswith("#"):
                continue
            found = bare_calls(line.split(" #", 1)[0])
            self.assertFalse(found, f"fixture.sh:{number} has a bare call {found}: {line.strip()}")

    def test_regression_probes_are_caught(self):
        """#2607: each bypass probe is caught by the same shared rule the fixture scan uses."""
        for probe in PROBES:
            self.assertTrue(bare_calls(probe), f"probe passed the rule: {probe}")


if __name__ == "__main__":
    unittest.main()
