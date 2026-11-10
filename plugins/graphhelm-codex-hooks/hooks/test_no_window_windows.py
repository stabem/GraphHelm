"""#2573: the hook's detached waiter and helper must not open a visible console window on Windows.

Imports the shipped session_hook module (HOOK_PATH overrides the file for a red run) and
records the creationflags it passes to subprocess.Popen when it starts the waiter and the
helper. A DETACHED_PROCESS child has no console, so the console program it starts opens a
window; the shipped flags must carry CREATE_NO_WINDOW and must not carry DETACHED_PROCESS.
Skipped off Windows.
"""
import importlib.util
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

DETACHED_PROCESS = 0x00000008
CREATE_NO_WINDOW = 0x08000000

HERE = Path(__file__).resolve().parent


def _load_hook():
    path = Path(os.environ.get("HOOK_PATH", HERE / "session_hook.py"))
    spec = importlib.util.spec_from_file_location("session_hook_under_test", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


@unittest.skipUnless(sys.platform == "win32", "console flags are a Windows concern")
class ShippedSpawnFlagsTest(unittest.TestCase):
    def _record_popen(self, module, call):
        recorded = []

        class _Fake:
            pid = 4242

            def __init__(self, argv, **kwargs):
                recorded.append(kwargs)

            def __getattr__(self, name):
                return lambda *a, **k: None

        real = module.subprocess.Popen
        module.subprocess.Popen = _Fake
        module.arm_lane_wake = lambda *a, **k: 0  # the spawn flags are under test; arming needs a live runtime
        try:
            call()
        finally:
            module.subprocess.Popen = real
        return recorded

    def test_waiter_spawn_flags_hide_the_console_and_do_not_detach(self):
        module = _load_hook()
        with tempfile.TemporaryDirectory() as tmp:
            events = Path(tmp) / "events"
            events.mkdir()
            os.environ["GRAPHHELM_EVENTS_DIR"] = str(events)
            ambient_actor = os.environ.get("GRAPHHELM_ACTOR")
            # #2714: an openbot- lane starts no waiter; these tests are about the waiter that does start.
            os.environ["GRAPHHELM_ACTOR"] = "gh-lane-test"
            try:
                payload = {"session_id": "s-2573", "cwd": tmp}
                bound = ("exec-2573", str(events / "events.json"), "lane-2573", "node", None)
                recorded = self._record_popen(
                    module, lambda: module.start_lane_waiter(payload, bound, "codex"))
            finally:
                os.environ.pop("GRAPHHELM_EVENTS_DIR", None)
                if ambient_actor is None:
                    os.environ.pop("GRAPHHELM_ACTOR", None)
                else:
                    os.environ["GRAPHHELM_ACTOR"] = ambient_actor
        self.assertTrue(recorded, "start_lane_waiter did not reach subprocess.Popen")
        flags = recorded[-1].get("creationflags", 0)
        self.assertFalse(flags & DETACHED_PROCESS, f"waiter detached: flags=0x{flags:08x}")
        self.assertTrue(flags & CREATE_NO_WINDOW, f"waiter has no CREATE_NO_WINDOW: 0x{flags:08x}")

    def test_helper_spawn_flags_hide_the_console_and_do_not_detach(self):
        module = _load_hook()
        helper = module._HelperClient.__new__(module._HelperClient)
        helper.host = "codex"
        helper.process = None
        helper.failure = None
        helper._started = __import__("threading").Event()
        recorded = self._record_popen(module, helper._spawn)
        self.assertTrue(recorded, "_HelperClient._spawn did not reach subprocess.Popen")
        flags = recorded[-1].get("creationflags", 0)
        self.assertFalse(flags & DETACHED_PROCESS, f"helper detached: flags=0x{flags:08x}")
        self.assertTrue(flags & CREATE_NO_WINDOW, f"helper has no CREATE_NO_WINDOW: 0x{flags:08x}")


if __name__ == "__main__":
    unittest.main()
