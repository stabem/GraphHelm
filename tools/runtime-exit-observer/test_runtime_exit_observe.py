import json
import os
import ctypes
import importlib.util
import io
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import unittest.mock
from pathlib import Path

TOOL = Path(__file__).with_name("runtime_exit_observe.py")


@unittest.skipUnless(os.name == "nt", "requires native Windows process handles")
class WindowsProcessObserver(unittest.TestCase):
    def setUp(self):
        self.children = []
        self.observers = []

    def tearDown(self):
        for observer in self.observers:
            if observer.poll() is None:
                observer.kill()
                observer.wait(timeout=5)
        for process in self.children:
            if process.poll() is None:
                process.stdin.write(b"0\n")
                process.stdin.flush()
                process.wait(timeout=5)

    def child(self, code=None):
        script = "import sys; sys.stdin.readline(); sys.exit(int(sys.argv[1]))"
        process = subprocess.Popen([sys.executable, "-c", script, str(code or 0)], stdin=subprocess.PIPE,
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, creationflags=0x08000000)
        self.children.append(process)
        kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        handle = kernel32.OpenProcess(0x1000, False, process.pid)
        if not handle:
            self.fail("could not query test child creation time")
        class FILETIME(ctypes.Structure):
            _fields_ = [("low", ctypes.c_uint32), ("high", ctypes.c_uint32)]
        created, exited, kernel, user = FILETIME(), FILETIME(), FILETIME(), FILETIME()
        ok = kernel32.GetProcessTimes(ctypes.c_void_p(handle), *(ctypes.byref(item) for item in (created, exited, kernel, user)))
        kernel32.CloseHandle(ctypes.c_void_p(handle))
        if not ok:
            self.fail("could not read test child creation time")
        creation = (created.high << 32) | created.low
        return process, creation

    def start_observer(self, process, creation, timeout=5):
        directory = tempfile.TemporaryDirectory()
        receipt = Path(directory.name) / "receipt.jsonl"
        observer = subprocess.Popen([sys.executable, str(TOOL), "--pid", str(process.pid), "--creation-filetime", str(creation),
                                     "--timeout-seconds", str(timeout), "--output", str(receipt)],
                                    stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True, creationflags=0x08000000)
        deadline = time.monotonic() + 4
        while time.monotonic() < deadline:
            if receipt.exists() and receipt.stat().st_size:
                return observer, directory, receipt
            if observer.poll() is not None:
                self.fail("observer exited before attachment")
            time.sleep(0.01)
        observer.kill()
        observer.wait()
        directory.cleanup()
        self.fail("observer did not attach within bounded test wait")

    def test_known_exit_codes_and_signaled_259(self):
        for code in (0, 23, 259):
            process, creation = self.child(code)
            observer, directory, receipt = self.start_observer(process, creation)
            process.stdin.write(b"1\n")
            process.stdin.flush()
            _, stderr = observer.communicate(timeout=8)
            records = [json.loads(line) for line in receipt.read_text(encoding="utf-8").splitlines()]
            self.assertEqual(observer.returncode, 0, stderr)
            self.assertEqual([record["state"] for record in records], ["attached", "exited"])
            self.assertEqual(records[-1]["exitCode"], code)
            process.wait(timeout=2)
            directory.cleanup()

    def test_timeout_does_not_terminate_child_and_wrong_identity_refuses(self):
        process, creation = self.child()
        observer, directory, receipt = self.start_observer(process, creation, timeout=0.1)
        _, stderr = observer.communicate(timeout=3)
        records = [json.loads(line) for line in receipt.read_text(encoding="utf-8").splitlines()]
        self.assertEqual(observer.returncode, 3, stderr)
        self.assertEqual([record["state"] for record in records], ["attached", "timeout"])
        self.assertIsNone(process.poll())
        directory.cleanup()
        wrong, records = self.observe_wrong_identity(process, creation + 1)
        self.assertNotEqual(wrong.returncode, 0)
        self.assertEqual(records, [])
        self.assertIsNone(process.poll())

    def test_attachment_persistence_exhausts_budget_before_first_wait(self):
        spec = importlib.util.spec_from_file_location("runtime_exit_observe", TOOL)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        real_fsync = os.fsync
        for release_child in (True, False):
            with self.subTest(release_child=release_child), tempfile.TemporaryDirectory() as directory:
                process, creation = self.child()
                receipt = Path(directory) / "receipt.jsonl"
                args = ["--pid", str(process.pid), "--creation-filetime", str(creation),
                        "--timeout-seconds", "0.1", "--output", str(receipt)]
                clock = [100.0]
                syncs = []

                def sync(fd):
                    real_fsync(fd)
                    syncs.append(fd)
                    if len(syncs) == 1:
                        clock[0] = 100.2
                        if release_child:
                            process.stdin.write(b"0\n")
                            process.stdin.flush()
                            self.assertEqual(process.wait(timeout=2), 0)

                from contextlib import redirect_stderr
                error = io.StringIO()
                with unittest.mock.patch.object(module.time, "monotonic", side_effect=lambda: clock[0]), \
                     unittest.mock.patch.object(module.os, "fsync", side_effect=sync), redirect_stderr(error):
                    code = module.main(args)
                states = [json.loads(line)["state"] for line in receipt.read_text(encoding="utf-8").splitlines()]
                self.assertEqual(len(syncs), 2)
                self.assertEqual(code, 3)
                self.assertEqual(error.getvalue(), "observation_timeout\n")
                self.assertEqual(states, ["attached", "timeout"])
                if not release_child:
                    self.assertIsNone(process.poll())

    def observe_wrong_identity(self, process, creation):
        with tempfile.TemporaryDirectory() as directory:
            receipt = Path(directory) / "wrong.jsonl"
            result = subprocess.run([sys.executable, str(TOOL), "--pid", str(process.pid), "--creation-filetime", str(creation),
                                     "--timeout-seconds", "1", "--output", str(receipt)], capture_output=True, text=True,
                                    timeout=3, creationflags=0x08000000)
            return result, [json.loads(line) for line in receipt.read_text(encoding="utf-8").splitlines()] if receipt.exists() else []

    def test_create_new_output_refuses_existing_file(self):
        process, creation = self.child()
        with tempfile.TemporaryDirectory() as directory:
            receipt = Path(directory) / "existing.jsonl"
            receipt.write_text("keep", encoding="utf-8")
            result = subprocess.run([sys.executable, str(TOOL), "--pid", str(process.pid), "--creation-filetime", str(creation),
                                     "--timeout-seconds", "1", "--output", str(receipt)], capture_output=True, text=True,
                                    creationflags=0x08000000)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(receipt.read_text(encoding="utf-8"), "keep")

    def test_bad_pid_has_fixed_diagnostic(self):
        with tempfile.TemporaryDirectory() as directory:
            receipt = Path(directory) / "invalid.jsonl"
            result = subprocess.run([sys.executable, str(TOOL), "--pid", "0", "--creation-filetime", "1",
                                     "--timeout-seconds", "1", "--output", str(receipt)], capture_output=True, text=True,
                                    creationflags=0x08000000)
            self.assertFalse(receipt.exists())
        self.assertNotIn("Traceback", result.stderr)

    def test_out_of_range_pid_refuses_before_opening_receipt(self):
        process, creation = self.child()
        for pid in (process.pid + 2**32, 0, -1, 2**32):
            with self.subTest(pid=pid), tempfile.TemporaryDirectory() as directory:
                receipt = Path(directory) / "invalid.jsonl"
                result = subprocess.run([sys.executable, str(TOOL), "--pid", str(pid), "--creation-filetime", str(creation),
                                         "--timeout-seconds", "0.1", "--output", str(receipt)], capture_output=True,
                                        text=True, timeout=3, creationflags=0x08000000)
                self.assertEqual(result.returncode, 2)
                self.assertEqual(result.stderr, "invalid_arguments\n")
                self.assertFalse(receipt.exists())
                self.assertIsNone(process.poll())

    def test_receipt_io_errors_are_sanitized_and_release_process_handle(self):
        process, creation = self.child()
        spec = importlib.util.spec_from_file_location("runtime_exit_observe", TOOL)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        real_open = open
        real_fsync = os.fsync

        class FaultyOutput:
            def __init__(self, wrapped, failure):
                self.wrapped = wrapped
                self.failure = failure
                self.failed = False

            def write(self, value):
                if self.failure == "write":
                    self.failed = True
                    raise OSError("SENSITIVE_MARKER")
                return self.wrapped.write(value)

            def flush(self):
                if self.failure == "flush":
                    self.failed = True
                    raise OSError("SENSITIVE_MARKER")
                return self.wrapped.flush()

            def fileno(self):
                return self.wrapped.fileno()

            def close(self):
                if self.failure == "close" and not self.failed:
                    self.failed = True
                    raise OSError("SENSITIVE_MARKER")
                self.wrapped.close()

        for failure, stage in (("write", "attach"), ("flush", "attach"), ("sync", "attach")):
            with self.subTest(failure=failure, stage=stage), tempfile.TemporaryDirectory() as directory:
                receipt = Path(directory) / "receipt.jsonl"
                args = ["--pid", str(process.pid), "--creation-filetime", str(creation),
                        "--timeout-seconds", "0.1", "--output", str(receipt)]
                def injected_open(path, *open_args, **open_kwargs):
                    output = real_open(path, *open_args, **open_kwargs)
                    if Path(path) == receipt:
                        return FaultyOutput(output, failure if stage == "attach" else "none")
                    return output

                def run_with_fault():
                    from contextlib import redirect_stderr
                    import contextlib
                    error = io.StringIO()
                    with unittest.mock.patch.object(module, "open", injected_open, create=True), \
                         unittest.mock.patch.object(module.os, "fsync", side_effect=OSError("SENSITIVE_MARKER") if failure == "sync" else os.fsync), \
                         contextlib.redirect_stderr(error):
                        code = module.main(args)
                    return code, error.getvalue()

                code, error = run_with_fault()
                self.assertEqual(code, 2)
                self.assertEqual(error, "receipt_unavailable\n")
                self.assertNotIn("SENSITIVE_MARKER", error)
                self.assertNotIn("Traceback", error)
                self.assertIsNone(process.poll())
                states = [json.loads(line)["state"] for line in receipt.read_text(encoding="utf-8").splitlines()]
                self.assertNotIn("exited", states)

        for failure in ("write", "flush", "sync"):
            with self.subTest(failure=failure, close_failure=True), tempfile.TemporaryDirectory() as directory:
                receipt = Path(directory) / "receipt.jsonl"
                args = ["--pid", str(process.pid), "--creation-filetime", str(creation),
                        "--timeout-seconds", "0.1", "--output", str(receipt)]
                faults = []
                close_attempts = []
                append_opens = []

                class CombinedFaultOutput(FaultyOutput):
                    def write(self, value):
                        if failure == "write" and not self.failed:
                            self.failed = True
                            faults.append("write")
                            raise OSError("SENSITIVE_WRITE_MARKER")
                        return self.wrapped.write(value)

                    def flush(self):
                        if failure == "flush" and not self.failed:
                            self.failed = True
                            faults.append("flush")
                            raise OSError("SENSITIVE_FLUSH_MARKER")
                        return self.wrapped.flush()

                    def close(self):
                        close_attempts.append(True)
                        self.wrapped.close()
                        faults.append("close")
                        raise OSError("SENSITIVE_CLOSE_MARKER")

                def injected_open(path, *open_args, **open_kwargs):
                    if Path(path) == receipt and open_args and open_args[0] == "a":
                        append_opens.append(path)
                    output = real_open(path, *open_args, **open_kwargs)
                    if Path(path) == receipt and open_args and open_args[0] == "x":
                        return CombinedFaultOutput(output, failure)
                    return output

                def injected_fsync(fd):
                    if failure == "sync" and not faults:
                        faults.append("sync")
                        raise OSError("SENSITIVE_SYNC_MARKER")
                    return real_fsync(fd)

                from contextlib import redirect_stderr
                error = io.StringIO()
                with unittest.mock.patch.object(module, "open", injected_open, create=True), \
                     unittest.mock.patch.object(module.os, "fsync", side_effect=injected_fsync), \
                     redirect_stderr(error):
                    code = module.main(args)
                self.assertEqual(faults, [failure, "close"])
                self.assertEqual(len(close_attempts), 1)
                self.assertEqual(code, 2)
                self.assertEqual(error.getvalue(), "receipt_unavailable\n")
                for marker in ("SENSITIVE_WRITE_MARKER", "SENSITIVE_FLUSH_MARKER",
                               "SENSITIVE_SYNC_MARKER", "SENSITIVE_CLOSE_MARKER"):
                    self.assertNotIn(marker, error.getvalue())
                self.assertNotIn("Traceback", error.getvalue())
                self.assertEqual(append_opens, [])
                self.assertIsNone(process.poll())

    def test_terminal_sync_failure_appends_failure_marker(self):
        spec = importlib.util.spec_from_file_location("runtime_exit_observe", TOOL)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        real_fsync = os.fsync
        for terminal, marker_sync_fails in (("exited", False), ("timeout", False), ("exited", True)):
            with self.subTest(terminal=terminal, marker_sync_fails=marker_sync_fails), tempfile.TemporaryDirectory() as directory:
                process, creation = self.child()
                receipt = Path(directory) / "receipt.jsonl"
                args = ["--pid", str(process.pid), "--creation-filetime", str(creation),
                        "--timeout-seconds", "0.1" if terminal == "timeout" else "5", "--output", str(receipt)]
                attached = threading.Event()
                syncs = []

                def sync(fd):
                    syncs.append(fd)
                    if len(syncs) == 2 or (marker_sync_fails and len(syncs) == 3):
                        raise OSError("SENSITIVE_MARKER")
                    real_fsync(fd)
                    if len(syncs) == 1:
                        attached.set()

                def release_exit_child():
                    if attached.wait(3):
                        process.stdin.write(b"0\n")
                        process.stdin.flush()

                releaser = threading.Thread(target=release_exit_child) if terminal == "exited" else None
                if releaser:
                    releaser.start()
                from contextlib import redirect_stderr
                error = io.StringIO()
                try:
                    with unittest.mock.patch.object(module.os, "fsync", side_effect=sync), redirect_stderr(error):
                        code = module.main(args)
                finally:
                    if releaser:
                        releaser.join(timeout=4)
                lines = receipt.read_text(encoding="utf-8").splitlines()
                states = [json.loads(line)["state"] for line in lines[:2]]
                self.assertEqual(len(syncs), 3, "attachment, terminal, and marker must each sync once")
                self.assertEqual(code, 2)
                self.assertEqual(error.getvalue(), "receipt_unavailable\n")
                self.assertEqual(states, ["attached", terminal])
                if not marker_sync_fails:
                    self.assertEqual([json.loads(line)["state"] for line in lines],
                                     ["attached", terminal, "receipt_unavailable"])
                if terminal == "timeout":
                    self.assertIsNone(process.poll())
                else:
                    self.assertEqual(process.wait(timeout=2), 0)

    def test_terminal_flush_failure_appends_failure_marker(self):
        spec = importlib.util.spec_from_file_location("runtime_exit_observe", TOOL)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        real_open = open
        real_fsync = os.fsync
        for terminal in ("exited", "timeout"):
            with self.subTest(terminal=terminal), tempfile.TemporaryDirectory() as directory:
                process, creation = self.child()
                receipt = Path(directory) / "receipt.jsonl"
                args = ["--pid", str(process.pid), "--creation-filetime", str(creation),
                        "--timeout-seconds", "0.1" if terminal == "timeout" else "5", "--output", str(receipt)]
                attached = threading.Event()
                flush_failed = []
                initial_streams = []

                class ForwardingOutput:
                    def __init__(self, wrapped):
                        self.wrapped = wrapped
                        self.terminal_flush_failed = False

                    def write(self, value):
                        return self.wrapped.write(value)

                    def flush(self):
                        if attached.is_set() and not self.terminal_flush_failed:
                            self.terminal_flush_failed = True
                            flush_failed.append(True)
                            raise OSError("SENSITIVE_MARKER")
                        self.wrapped.flush()

                    def fileno(self):
                        return self.wrapped.fileno()

                    def close(self):
                        self.wrapped.close()

                def injected_open(path, *open_args, **open_kwargs):
                    stream = real_open(path, *open_args, **open_kwargs)
                    if Path(path) == receipt and open_args[0] == "x":
                        wrapped = ForwardingOutput(stream)
                        initial_streams.append(wrapped)
                        return wrapped
                    return stream

                sync_count = []

                def sync(fd):
                    real_fsync(fd)
                    sync_count.append(fd)
                    if len(sync_count) == 1:
                        attached.set()

                def release_exit_child():
                    if attached.wait(3):
                        process.stdin.write(b"0\n")
                        process.stdin.flush()

                releaser = threading.Thread(target=release_exit_child) if terminal == "exited" else None
                if releaser:
                    releaser.start()
                from contextlib import redirect_stderr
                error = io.StringIO()
                try:
                    with unittest.mock.patch.object(module, "open", injected_open, create=True), \
                         unittest.mock.patch.object(module.os, "fsync", side_effect=sync), \
                         redirect_stderr(error):
                        code = module.main(args)
                finally:
                    if releaser:
                        releaser.join(timeout=4)
                lines = receipt.read_text(encoding="utf-8").splitlines()
                states = [json.loads(line)["state"] for line in lines]
                self.assertEqual(flush_failed, [True], "terminal flush fault must be reached")
                self.assertEqual(len(initial_streams), 1)
                self.assertTrue(initial_streams[0].wrapped.closed, "close must flush buffered terminal bytes")
                self.assertEqual(code, 2)
                self.assertEqual(error.getvalue(), "receipt_unavailable\n")
                self.assertNotIn("SENSITIVE_MARKER", error.getvalue())
                self.assertEqual(states, ["attached", terminal, "receipt_unavailable"])
                if terminal == "timeout":
                    self.assertIsNone(process.poll())
                else:
                    self.assertEqual(process.wait(timeout=2), 0)

    def test_terminal_flush_failure_marker_write_failure_is_fixed_and_not_retried(self):
        spec = importlib.util.spec_from_file_location("runtime_exit_observe", TOOL)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        real_open = open
        real_fsync = os.fsync
        process, creation = self.child()
        with tempfile.TemporaryDirectory() as directory:
            receipt = Path(directory) / "receipt.jsonl"
            args = ["--pid", str(process.pid), "--creation-filetime", str(creation),
                    "--timeout-seconds", "5", "--output", str(receipt)]
            attached = threading.Event()
            initial = []
            append_attempts = []

            class FlushFailure:
                def __init__(self, wrapped):
                    self.wrapped = wrapped
                    self.failed = False

                def write(self, value):
                    return self.wrapped.write(value)

                def flush(self):
                    if attached.is_set() and not self.failed:
                        self.failed = True
                        raise OSError("SENSITIVE_MARKER")
                    self.wrapped.flush()

                def fileno(self):
                    return self.wrapped.fileno()

                def close(self):
                    self.wrapped.close()

            def injected_open(path, *open_args, **open_kwargs):
                if Path(path) == receipt and open_args[0] == "a":
                    append_attempts.append(True)
                    raise OSError("SENSITIVE_MARKER")
                stream = real_open(path, *open_args, **open_kwargs)
                if Path(path) == receipt and open_args[0] == "x":
                    wrapped = FlushFailure(stream)
                    initial.append(wrapped)
                    return wrapped
                return stream

            def sync(fd):
                real_fsync(fd)
                attached.set()

            def release_child():
                if attached.wait(3):
                    process.stdin.write(b"0\n")
                    process.stdin.flush()

            releaser = threading.Thread(target=release_child)
            releaser.start()
            from contextlib import redirect_stderr
            error = io.StringIO()
            try:
                with unittest.mock.patch.object(module, "open", injected_open, create=True), \
                     unittest.mock.patch.object(module.os, "fsync", side_effect=sync), \
                     redirect_stderr(error):
                    code = module.main(args)
            finally:
                releaser.join(timeout=4)
            states = [json.loads(line)["state"] for line in receipt.read_text(encoding="utf-8").splitlines()]
            self.assertEqual(code, 2)
            self.assertEqual(error.getvalue(), "receipt_unavailable\n")
            self.assertNotIn("SENSITIVE_MARKER", error.getvalue())
            self.assertEqual(append_attempts, [True])
            self.assertEqual(states, ["attached", "exited"])
            self.assertTrue(initial[0].wrapped.closed)
            self.assertEqual(process.wait(timeout=2), 0)

    def test_terminal_close_failure_reports_error_with_synced_exit_record(self):
        process, creation = self.child()
        spec = importlib.util.spec_from_file_location("runtime_exit_observe", TOOL)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        real_open = open

        class CloseFailure:
            def __init__(self, wrapped):
                self.wrapped = wrapped
                self.closed = False

            def write(self, value):
                return self.wrapped.write(value)

            def flush(self):
                return self.wrapped.flush()

            def fileno(self):
                return self.wrapped.fileno()

            def close(self):
                self.wrapped.close()
                if not self.closed:
                    self.closed = True
                    raise OSError("SENSITIVE_MARKER")

        with tempfile.TemporaryDirectory() as directory:
            receipt = Path(directory) / "receipt.jsonl"
            args = ["--pid", str(process.pid), "--creation-filetime", str(creation),
                    "--timeout-seconds", "5", "--output", str(receipt)]
            def injected_open(path, *open_args, **open_kwargs):
                output = real_open(path, *open_args, **open_kwargs)
                return CloseFailure(output) if Path(path) == receipt and open_args[0] == "x" else output

            process.stdin.write(b"1\n")
            process.stdin.flush()
            from contextlib import redirect_stderr
            error = io.StringIO()
            with unittest.mock.patch.object(module, "open", injected_open, create=True), redirect_stderr(error):
                code = module.main(args)
            states = [json.loads(line)["state"] for line in receipt.read_text(encoding="utf-8").splitlines()]
            self.assertEqual(code, 2)
            self.assertEqual(error.getvalue(), "receipt_unavailable\n")
            self.assertEqual(states, ["attached", "exited", "receipt_unavailable"])
            self.assertEqual(process.wait(timeout=2), 0)

class RuntimeExitObserverContract(unittest.TestCase):
    def test_help_exposes_bounded_identity_and_receipt_inputs(self):
        result = subprocess.run([sys.executable, str(TOOL), "--help"], capture_output=True, text=True,
                                creationflags=0x08000000)
        self.assertEqual(result.returncode, 0)
        for option in ("--pid", "--creation-filetime", "--timeout-seconds", "--output"):
            self.assertIn(option, result.stdout)

    def test_non_windows_host_refuses_without_creating_receipt(self):
        if sys.platform == "win32":
            self.skipTest("Windows observer behavior is covered by native child tests")
        with tempfile.TemporaryDirectory() as directory:
            receipt = Path(directory) / "receipt.jsonl"
            result = subprocess.run([sys.executable, str(TOOL), "--pid", "1", "--creation-filetime", "1", "--timeout-seconds", "1", "--output", str(receipt)], capture_output=True, text=True,
                                    creationflags=0x08000000)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(receipt.exists())
            self.assertNotIn("Traceback", result.stderr)

if __name__ == "__main__":
    unittest.main()
