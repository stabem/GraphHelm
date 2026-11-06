"""#2573: a hook-launched waiter must not open a visible console window on Windows.

A DETACHED_PROCESS child has no console, so a console program it starts opens a new window.
The hooks spawn their waiter and helper with CREATE_NO_WINDOW (plus the job breakaway, #919).
The test counts new visible console windows while such a child tree runs, and subtracts the
windows of an ambient baseline (a child that starts no console program) so other programs'
windows do not count. Skipped off Windows.
"""
import ctypes
import ctypes.wintypes as wt
import subprocess
import sys
import time
import unittest

CREATE_NO_WINDOW = 0x08000000
DETACHED_PROCESS = 0x00000008
CREATE_BREAKAWAY_FROM_JOB = 0x01000000
CONSOLE_CLASSES = {"ConsoleWindowClass", "CASCADIA_HOSTING_WINDOW_CLASS", "PseudoConsoleWindow"}


def _visible_console_windows(user32):
    found = set()
    buf = ctypes.create_unicode_buffer(256)
    proc = ctypes.WINFUNCTYPE(ctypes.c_bool, wt.HWND, wt.LPARAM)

    def visit(hwnd, _):
        if user32.IsWindowVisible(hwnd):
            user32.GetClassNameW(hwnd, buf, 256)
            if buf.value in CONSOLE_CLASSES:
                found.add(hwnd)
        return True

    user32.EnumWindows(proc(visit), 0)
    return found


def _new_windows_during(user32, flags, parent_code):
    before = _visible_console_windows(user32)
    child = subprocess.Popen([sys.executable, "-c", parent_code], creationflags=flags,
                             stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                             stderr=subprocess.DEVNULL)
    seen = set()
    deadline = time.time() + 12
    while child.poll() is None or time.time() < deadline:
        seen |= _visible_console_windows(user32) - before
        if child.poll() is not None and time.time() >= deadline:
            break
        time.sleep(0.1)
    child.wait(timeout=30)
    return len(seen)


@unittest.skipUnless(sys.platform == "win32", "console windows exist only on Windows")
class NoConsoleWindowTest(unittest.TestCase):
    def test_hook_spawn_flags_open_no_console_window(self):
        user32 = ctypes.WinDLL("user32", use_last_error=True)
        console_tree = ("import subprocess;"
                        "subprocess.run(['cmd','/c','ping -n 4 127.0.0.1 >nul'])")
        ambient = _new_windows_during(user32, CREATE_NO_WINDOW, "import time;time.sleep(6)")
        control = _new_windows_during(user32, CREATE_NO_WINDOW | DETACHED_PROCESS, console_tree)
        self.assertGreater(control - ambient, 0,
                           "the detached control must open a window, or this test proves nothing")
        shipped = _new_windows_during(user32, CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB,
                                      console_tree)
        self.assertEqual(max(0, shipped - ambient), 0, "the shipped spawn flags open a window")


if __name__ == "__main__":
    unittest.main()
