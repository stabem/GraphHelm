"""#1521: the Studio dev server the journey launcher starts must start.

`fixture.sh up` starts the Studio with `npm --prefix apps/studio run dev -- --port P --strictPort`.
When that process dies at start (2026-10-17: a half-reinstalled `node_modules` whose native
binding was missing), every journey preview ends `watch.launch_failed`, and no reached plan went
red because nothing in a plan starts Vite. This smoke starts it the same way, browser-free, and
requires an HTTP answer on `/`; it stops only the process tree it started.

`GRAPHHELM_STUDIO_SMOKE_DIR` points it at another Studio directory (a scratch copy), so the red
case can be shown without touching the shared install.
"""

import os
import pathlib
import shutil
import socket
import subprocess
import tempfile
import time
import unittest
import urllib.error
import urllib.request

# Owner rule: children run hidden in the calling terminal, never in a new console window.
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
ROOT = pathlib.Path(__file__).resolve().parents[2]
# A Studio that dies at start (the #1521 breakage) ends the wait at once; this bound only has to
# cover a slow, healthy first start on a loaded machine (97 s observed at 85% CPU).
STARTUP_SECONDS = 240


def free_port():
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


def answers(url):
    try:
        with urllib.request.urlopen(url, timeout=2) as response:
            return response.status
    except urllib.error.HTTPError as error:
        return error.code
    except OSError:
        return None


def stop_tree(process):
    if process.poll() is not None:
        return
    if os.name == "nt":
        subprocess.run(["taskkill", "/T", "/F", "/PID", str(process.pid)], capture_output=True,
                       check=False, creationflags=NO_WINDOW)
    else:
        os.killpg(process.pid, 15)
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        process.kill()


class StudioStartsTest(unittest.TestCase):
    def test_the_launchers_studio_dev_server_answers(self):
        studio = pathlib.Path(os.environ.get("GRAPHHELM_STUDIO_SMOKE_DIR") or ROOT / "apps/studio")
        self.assertTrue((studio / "node_modules").is_dir(),
                        f"{studio} has no node_modules: run `npm --prefix apps/studio ci` (the launcher needs it)")
        npm = shutil.which("npm")
        self.assertIsNotNone(npm, "npm is not on PATH (the launcher needs it)")
        port = free_port()
        with tempfile.TemporaryFile(mode="w+", encoding="utf-8", errors="replace") as log:
            process = subprocess.Popen(
                [npm, "--prefix", str(studio), "run", "dev", "--", "--port", str(port), "--strictPort"],
                cwd=studio, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT,
                creationflags=NO_WINDOW, start_new_session=os.name != "nt",
            )
            try:
                status = None
                deadline = time.monotonic() + STARTUP_SECONDS
                while time.monotonic() < deadline and process.poll() is None:
                    status = answers(f"http://127.0.0.1:{port}/")
                    if status is not None:
                        break
                    time.sleep(0.5)
            finally:
                stop_tree(process)
            log.seek(0)
            tail = "\n".join(log.read().splitlines()[-20:])
        self.assertEqual(status, 200, f"the Studio dev server did not answer on :{port} within "
                         f"{STARTUP_SECONDS}s (exit {process.returncode}); its last lines:\n{tail}")


if __name__ == "__main__":
    unittest.main()
