"""#2607: a Runtime whose command line carries the Git Bash path of the fixture is owned by it.

Calls the fixture's own `fixture_listeners` (a function in fixture.sh) against a listener this test
starts, with `dir` set the way fixture.sh sets it (`cd ... && pwd`), and expects that PID back.
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
PROBES = [
    'taskkill //PID $p //F && "C:/Windows/System32/netstat.exe" -ano',
    'C:/Windows/System32/taskkill.exe //PID 1; taskkill //PID 2 //F',
    '"$taskkill_exe" //PID 1; taskkill //PID 2',
    '"$taskkill_exe" //PID 1 || true; taskkill //PID 3 || true',
]

NO_WINDOW = getattr(subprocess, "CREATE_NO_WINDOW", 0)


def function_source(name):
    text = open(SCRIPT, encoding="utf-8").read()
    start = text.index(f"{name}() {{\n")
    # The function ends with its own `}` line after the bare PS terminator of its heredoc.
    ps = text.index("\nPS\n", start) + len("\nPS\n")
    return text[start:text.index("\n}\n", ps) + 3]


def bash_pwd(windows_path):
    """The path the way bash's `pwd` names it, which is the form the launcher hands the Runtime."""
    out = subprocess.run(["bash", "-c", f'cd "{windows_path.replace(chr(92), "/")}" && pwd'],
                         capture_output=True, text=True, timeout=30, creationflags=NO_WINDOW)
    return out.stdout.strip()


def posix_path(windows_path):
    return "/" + windows_path[0].lower() + windows_path[2:].replace("\\", "/")


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
            # #2607: the launcher spawns the fixture with a PATH that lacks System32; netstat must
            # still resolve, so the check calls it by its full path.
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
        """#2607 (Shino): every taskkill call names the System32 path; a bare one fails under the
        launcher's PATH, as the down path did."""
        for number, line in enumerate(open(SCRIPT, encoding="utf-8"), 1):
            if "taskkill" in line and "System32" not in line and "taskkill_exe" not in line                     and not line.lstrip().startswith("#"):
                self.fail(f"fixture.sh:{number} calls a bare taskkill: {line.strip()}")


    def test_every_netstat_and_taskkill_call_is_by_full_path(self):
        """#2607 (Tsunade, Shino): each netstat and taskkill call names its own System32 path.

        Each call is judged by its own offset in the line: the text between the previous
        command separator and this call must name the System32 path (or taskkill_exe), so a
        full-path call earlier on the line cannot hide a bare one later.
        """
        call = re.compile(r"\b(netstat|taskkill)(\.exe)?\b", re.IGNORECASE)
        separator = re.compile(r";|\|\||\||&&|&")
        for number, line in enumerate(open(SCRIPT, encoding="utf-8"), 1):
            if line.lstrip().startswith("#"):
                continue
            code = line.split(" #", 1)[0]
            for match in call.finditer(code):
                start = max((m.end() for m in separator.finditer(code, 0, match.start())), default=0)
                own = code[start:match.start()]
                if "System32" in own or "taskkill_exe" in own:
                    continue
                self.fail(f"fixture.sh:{number} calls {match.group(0)} without its own System32 path: {line.strip()}")


    def test_regression_probes_are_caught(self):
        """#2607 (Shino): bypass lines must fail the rule (each probe is judged on its own)."""
        call = re.compile(r"\b(netstat|taskkill)(\.exe)?\b", re.IGNORECASE)
        separator = re.compile(r";|\|\||\||&&|&")
        for probe in PROBES:
            flagged = False
            for match in call.finditer(probe):
                start = max((m.end() for m in separator.finditer(probe, 0, match.start())), default=0)
                own = probe[start:match.start()]
                if not ("System32" in own or "taskkill_exe" in own):
                    flagged = True
            self.assertTrue(flagged, f"probe passed the rule: {probe}")


if __name__ == "__main__":
    unittest.main()
