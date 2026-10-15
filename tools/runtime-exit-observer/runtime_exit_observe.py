"""Observe one Windows process exit without controlling the process."""

import argparse
import ctypes
import json
import os
import sys
import time
from datetime import datetime, timezone

ERRORS = {
    "unsupported": "unsupported_host",
    "invalid": "invalid_arguments",
    "refused": "attach_refused",
    "identity": "identity_mismatch",
    "timeout": "observation_timeout",
    "io": "receipt_unavailable",
    "failed": "observation_failed",
}


def utc_now():
    return datetime.now(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")


def parser():
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--pid", type=int, required=True)
    result.add_argument("--creation-filetime", type=int, required=True)
    result.add_argument("--timeout-seconds", type=float, required=True)
    result.add_argument("--output", required=True)
    return result


def main(argv=None):
    args = parser().parse_args(argv)
    if not (0 < args.pid <= 0xFFFFFFFF) or args.creation_filetime <= 0 or not (0 < args.timeout_seconds <= 86400):
        print(ERRORS["invalid"], file=sys.stderr)
        return 2
    if os.name != "nt":
        print(ERRORS["unsupported"], file=sys.stderr)
        return 2

    try:
        output = open(args.output, "x", encoding="utf-8", newline="\n")
    except OSError:
        print(ERRORS["io"], file=sys.stderr)
        return 2

    terminal_sync_failed = False

    def emit(record):
        nonlocal terminal_sync_failed
        output.write(json.dumps(record, separators=(",", ":"), sort_keys=True) + "\n")
        output.flush()
        try:
            os.fsync(output.fileno())
        except OSError:
            if record["state"] in ("exited", "timeout"):
                terminal_sync_failed = True
            raise

    def close_output():
        try:
            output.close()
        except OSError:
            return False
        return True

    close_output_safely = close_output

    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.OpenProcess.argtypes = (ctypes.c_uint32, ctypes.c_int, ctypes.c_uint32)
    kernel32.OpenProcess.restype = ctypes.c_void_p
    kernel32.GetProcessTimes.argtypes = (ctypes.c_void_p,) + (ctypes.POINTER(ctypes.c_uint64),) * 4
    kernel32.GetProcessTimes.restype = ctypes.c_int
    kernel32.WaitForSingleObject.argtypes = (ctypes.c_void_p, ctypes.c_uint32)
    kernel32.WaitForSingleObject.restype = ctypes.c_uint32
    kernel32.GetExitCodeProcess.argtypes = (ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint32))
    kernel32.GetExitCodeProcess.restype = ctypes.c_int
    kernel32.CloseHandle.argtypes = (ctypes.c_void_p,)
    kernel32.CloseHandle.restype = ctypes.c_int

    # PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE; deliberately no terminate rights.
    handle = kernel32.OpenProcess(0x1000 | 0x00100000, False, args.pid)
    if not handle:
        if not close_output():
            print(ERRORS["io"], file=sys.stderr)
            return 2
        print(ERRORS["refused"], file=sys.stderr)
        return 2
    result = 2
    timeout_observed = False
    try:
        created = ctypes.c_uint64()
        exited = ctypes.c_uint64()
        kernel = ctypes.c_uint64()
        user = ctypes.c_uint64()
        if not kernel32.GetProcessTimes(handle, ctypes.byref(created), ctypes.byref(exited), ctypes.byref(kernel), ctypes.byref(user)):
            print(ERRORS["failed"], file=sys.stderr)
            result = 2
        elif created.value != args.creation_filetime:
            print(ERRORS["identity"], file=sys.stderr)
            result = 2
        else:
            started = time.monotonic()
            try:
                emit({"schema": "graphhelm.runtime-process-exit/1", "state": "attached", "pid": args.pid,
                      "creationFiletime": created.value, "observedAt": utc_now()})
            except OSError:
                print(ERRORS["io"], file=sys.stderr)
                result = 2
            else:
                remaining = args.timeout_seconds
                while remaining > 0:
                    wait_ms = max(1, min(1000, int(remaining * 1000)))
                    status = kernel32.WaitForSingleObject(handle, wait_ms)
                    if status == 0:
                        code = ctypes.c_uint32()
                        if not kernel32.GetExitCodeProcess(handle, ctypes.byref(code)) or not kernel32.GetProcessTimes(handle, ctypes.byref(created), ctypes.byref(exited), ctypes.byref(kernel), ctypes.byref(user)):
                            print(ERRORS["failed"], file=sys.stderr)
                            result = 2
                        else:
                            try:
                                emit({"schema": "graphhelm.runtime-process-exit/1", "state": "exited", "pid": args.pid,
                                      "creationFiletime": created.value, "exitFiletime": exited.value,
                                      "observedAt": utc_now(), "exitCode": code.value})
                                result = 0
                            except OSError:
                                if not terminal_sync_failed:
                                    print(ERRORS["io"], file=sys.stderr)
                                result = 2
                        break
                    if status != 0x102:
                        print(ERRORS["failed"], file=sys.stderr)
                        result = 2
                        break
                    remaining = args.timeout_seconds - (time.monotonic() - started)
                else:
                    try:
                        emit({"schema": "graphhelm.runtime-process-exit/1", "state": "timeout", "pid": args.pid,
                              "creationFiletime": created.value, "observedAt": utc_now()})
                        timeout_observed = True
                        result = 3
                    except OSError:
                        if not terminal_sync_failed:
                            print(ERRORS["io"], file=sys.stderr)
                        result = 2
    finally:
        kernel32.CloseHandle(handle)
        close_failed = not close_output_safely()
    if close_failed or terminal_sync_failed:
        if result in (0, 3) or terminal_sync_failed:
            try:
                with open(args.output, "a", encoding="utf-8", newline="\n") as failure_output:
                    failure_output.write(json.dumps({"schema": "graphhelm.runtime-process-exit/1",
                                                     "state": "receipt_unavailable", "pid": args.pid,
                                                     "creationFiletime": args.creation_filetime},
                                                    separators=(",", ":"), sort_keys=True) + "\n")
                    failure_output.flush()
                    os.fsync(failure_output.fileno())
            except OSError:
                pass
        print(ERRORS["io"], file=sys.stderr)
        return 2
    if timeout_observed:
        print(ERRORS["timeout"], file=sys.stderr)
    return result


if __name__ == "__main__":
    raise SystemExit(main())
