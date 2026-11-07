"""#2573: the hook's detached waiter and helper must not open a visible console window on Windows.

Imports the shipped session_hook module (HOOK_PATH overrides the file for a red run) and
records the creationflags it passes to subprocess.Popen when it starts the waiter and the
helper. A DETACHED_PROCESS child has no console, so the console program it starts opens a
window; the shipped flags must carry CREATE_NO_WINDOW and must not carry DETACHED_PROCESS.
Skipped off Windows.
"""
# graphhelm:intentional-visible-window — the job-breakaway test spawns the parent that owns the job; its Popen has no creationflags by design.
import ctypes
import importlib.util
import time
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

DETACHED_PROCESS = 0x00000008
CREATE_NO_WINDOW = 0x08000000
CREATE_BREAKAWAY_FROM_JOB = 0x01000000

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
            try:
                payload = {"session_id": "s-2573", "cwd": tmp}
                bound = ("exec-2573", str(events / "events.json"), "lane-2573", "node", None)
                recorded = self._record_popen(
                    module, lambda: module.start_lane_waiter(payload, bound, "codex"))
            finally:
                os.environ.pop("GRAPHHELM_EVENTS_DIR", None)
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



JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x00002000
JOB_OBJECT_LIMIT_BREAKAWAY_OK = 0x00000800
JOB_OBJECT_EXTENDED_LIMIT_INFORMATION = 9


class _IoCounters(ctypes.Structure):
    _fields_ = [(n, ctypes.c_ulonglong) for n in (
        "ReadOperationCount", "WriteOperationCount", "OtherOperationCount",
        "ReadTransferCount", "WriteTransferCount", "OtherTransferCount")]


class _BasicLimit(ctypes.Structure):
    _fields_ = [
        ("PerProcessUserTimeLimit", ctypes.c_longlong),
        ("PerJobUserTimeLimit", ctypes.c_longlong),
        ("LimitFlags", ctypes.c_uint32),
        ("MinimumWorkingSetSize", ctypes.c_size_t),
        ("MaximumWorkingSetSize", ctypes.c_size_t),
        ("ActiveProcessLimit", ctypes.c_uint32),
        ("Affinity", ctypes.c_size_t),
        ("PriorityClass", ctypes.c_uint32),
        ("SchedulingClass", ctypes.c_uint32),
    ]


class _ExtendedLimit(ctypes.Structure):
    _fields_ = [("BasicLimitInformation", _BasicLimit), ("IoInfo", _IoCounters),
                ("ProcessMemoryLimit", ctypes.c_size_t), ("JobMemoryLimit", ctypes.c_size_t),
                ("PeakProcessMemoryUsed", ctypes.c_size_t), ("PeakJobMemoryUsed", ctypes.c_size_t)]


def _recorded_waiter_flags(module):
    """The creationflags the shipped waiter spawn passes to subprocess.Popen."""
    recorded = []

    class _Fake:
        pid = 4242

        def __init__(self, argv, **kwargs):
            recorded.append(kwargs)

        def __getattr__(self, name):
            return lambda *a, **k: None

    import tempfile
    real = module.subprocess.Popen
    module.subprocess.Popen = _Fake
    try:
        with tempfile.TemporaryDirectory() as tmp:
            events = Path(tmp) / "events"
            events.mkdir()
            os.environ["GRAPHHELM_EVENTS_DIR"] = str(events)
            try:
                bound = ("exec-2579", str(events / "events.json"), "lane-2579", "node", None)
                module.start_lane_waiter({"session_id": "s-2579", "cwd": tmp}, bound, "codex")
            finally:
                os.environ.pop("GRAPHHELM_EVENTS_DIR", None)
    finally:
        module.subprocess.Popen = real
    return recorded[-1].get("creationflags", 0) if recorded else None


@unittest.skipUnless(sys.platform == "win32", "job objects are a Windows concern")
class WaiterSurvivesItsJobTest(unittest.TestCase):
    """The hook runs in a job object that closes when the hook returns (#919). A waiter started
    with the shipped flags must break away from that job and survive the close."""

    def _run_under_job(self, flags, tmp):
        kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        job = kernel32.CreateJobObjectW(None, None)
        self.assertTrue(job, "CreateJobObjectW failed")
        info = _ExtendedLimit()
        info.BasicLimitInformation.LimitFlags = (
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK)
        kernel32.SetInformationJobObject(job, JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                                         ctypes.byref(info), ctypes.sizeof(info))
        marker = Path(tmp) / "waiter-survived.txt"
        parent_code = (
            "import subprocess,sys,time;time.sleep(1);"
            "subprocess.Popen([sys.executable,'-c'," + repr(
                f"import time,pathlib;time.sleep(2);pathlib.Path({str(marker)!r}).write_text('ok')")
            + "],creationflags=" + str(flags) + ",stdin=subprocess.DEVNULL,"
            "stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)")
        parent = subprocess.Popen([sys.executable, "-c", parent_code],
                                  stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                  stderr=subprocess.DEVNULL)
        kernel32.AssignProcessToJobObject(job, int(parent._handle))
        parent.wait(timeout=30)
        time.sleep(0.5)
        kernel32.CloseHandle(job)  # the hook's job closes: KILL_ON_JOB_CLOSE kills its members
        time.sleep(4)
        return marker.exists()

    def test_shipped_waiter_flags_survive_their_job_closing(self):
        module = _load_hook()
        flags = _recorded_waiter_flags(module)
        self.assertIsNotNone(flags, "start_lane_waiter did not reach subprocess.Popen")
        self.assertTrue(flags & CREATE_BREAKAWAY_FROM_JOB,
                        f"the shipped waiter has no job breakaway: 0x{flags:08x}")
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            self.assertTrue(self._run_under_job(flags, tmp),
                            "the shipped waiter died when its job closed")

    def test_without_breakaway_the_waiter_dies_with_its_job(self):
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            survived = self._run_under_job(CREATE_NO_WINDOW, tmp)
        self.assertFalse(survived, "control: a child without breakaway should die with the job")


class NoDetachedFlagGuardTest(unittest.TestCase):
    """Every production hook Python file: no DETACHED_PROCESS in code (comments and tests excluded)."""

    def test_no_hook_source_uses_detached_process(self):
        root = Path(os.environ.get("HOOK_TREE", HERE.parent.parent))
        offenders = []
        for path in sorted(root.rglob("*.py")):
            if "__pycache__" in path.parts or path.name.startswith("test_"):
                continue
            for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
                code = line.split("#", 1)[0]
                if "DETACHED_PROCESS" in code:
                    offenders.append(f"{path}:{number}")
        self.assertEqual(offenders, [], "DETACHED_PROCESS in hook code")


if __name__ == "__main__":
    unittest.main()
