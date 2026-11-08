"""#2617: `fixture.sh up` says what answered on an occupied port, so a launch failure is diagnosable.

Runs only the occupancy check (it exits before any setup), so it needs bash, not Node or Docker.
"""
import http.server
import os
import socket
import subprocess
import sys
import tempfile
import threading
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(HERE, "fixture.sh")
NO_WINDOW = getattr(subprocess, "CREATE_NO_WINDOW", 0)


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class Health(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.send_header("Content-Length", "2")
        self.end_headers()
        self.wfile.write(b"ok")

    def log_message(self, *args):
        pass


def run_up(rport, sport):
    with tempfile.TemporaryDirectory() as d:
        r = subprocess.run(
            ["bash", SCRIPT, "up", os.path.join(d, "fx"), str(rport), str(sport)],
            capture_output=True, text=True, timeout=60, creationflags=NO_WINDOW,
        )
    return r.returncode, r.stderr


class PortDetail(unittest.TestCase):
    def test_runtime_health_answer_is_named_with_its_status(self):
        server = http.server.HTTPServer(("127.0.0.1", 0), Health)
        port = server.server_address[1]
        threading.Thread(target=server.serve_forever, daemon=True).start()
        try:
            code, err = run_up(port, free_port())
        finally:
            server.shutdown()
        self.assertEqual(code, 2, err)
        self.assertIn("Runtime port already occupied", err)
        # #2617: the detail names what answered and its HTTP status, not just the words.
        self.assertIn(f"127.0.0.1:{port}", err, err)
        self.assertIn("200", err, err)

    def test_tcp_occupant_is_named_as_an_accepted_connection(self):
        listener = socket.socket()
        listener.bind(("127.0.0.1", 0))
        listener.listen(1)
        sport = listener.getsockname()[1]
        try:
            code, err = run_up(free_port(), sport)
        finally:
            listener.close()
        self.assertEqual(code, 2, err)
        self.assertIn(f"port {sport} already occupied", err)
        self.assertIn("connect accepted", err, err)


if __name__ == "__main__":
    unittest.main()
