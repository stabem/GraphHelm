"""Bounded, local-first SessionStart and SessionEnd hooks."""

from __future__ import annotations

# The first instruction takes the hook's first timestamp, before any other import. With
# GRAPHHELM_HOOK_FIRST_LINE set (a probe only), it goes to stderr as "graphhelm-probe-first <pid>
# <epoch seconds>", so the time from spawn to this line can be told apart from the hook's own time.
# #2270: stderr, never a file: a file write on a starved disk stalled and lost this mark.
import time as _first_line_clock
_FIRST_LINE_AT = _first_line_clock.time()
import os as _first_line_os
import sys as _first_line_sys
import threading as _first_line_threading

# Under disk pressure, file I/O outside the bounded state steps (imports, stdin, team files, the
# waiter) can still stall. One watchdog bounds all of it: HOOK_WATCHDOG seconds after this line, a
# start or end that has not answered yet answers UNOBSERVED, says so, and leaves at once.
_watched_phase = _first_line_sys.argv[1] if len(_first_line_sys.argv) > 1 else ""
_watched_host = _first_line_sys.argv[_first_line_sys.argv.index("--host") + 1] if "--host" in _first_line_sys.argv[:-1] else ""
# Codex clamps every SessionEnd hook to 3 s whatever it declares, so a Codex end answers 1 s after
# its first line; spawn and teardown take the rest of the 3 s.
HOOK_WATCHDOG = 1.0 if (_watched_host, _watched_phase) == ("codex", "end") else 6.0
_answer_started = False


class _ClosedStderr:
    """#2353: stderr when the host spawned the hook with it closed. Warnings go nowhere instead of
    `print(file=None)` falling through to stdout and corrupting the answer."""

    def write(self, text):
        return len(text)

    def flush(self):
        return None


# A host may spawn the hook with stderr closed (Python then sets sys.stderr to None). The hook still
# answers with the same exit shapes; only the stderr-bound backstop below is not armed.
_stderr_open = _first_line_sys.stderr is not None
if not _stderr_open:
    _first_line_sys.stderr = _ClosedStderr()


class _AnswerGuard:
    """stdout that remembers whether the hook began its answer, so the watchdog never adds a second."""

    def __init__(self, inner):
        self._inner = inner

    def write(self, text):
        global _answer_started
        _answer_started = True
        return self._inner.write(text)

    def __getattr__(self, name):
        return getattr(self._inner, name)


def _watchdog_quote(text: str) -> str:
    """A JSON string with no import: the watchdog must not wait on an import lock the stalled main
    thread may hold."""
    out = ['"']
    for char in text:
        if char in '"\\':
            out.append("\\" + char)
        elif ord(char) < 0x20 or ord(char) > 0x7E:
            out.append("\\u%04x" % ord(char) if ord(char) <= 0xFFFF else char)
        else:
            out.append(char)
    out.append('"')
    return "".join(out)


def _watchdog_answer() -> str:
    argv = _first_line_sys.argv
    host = argv[argv.index("--host") + 1] if "--host" in argv[:-1] else ""
    output_format = argv[argv.index("--format") + 1] if "--format" in argv[:-1] else ("native" if host in {"claude", "codex"} else "portable")
    context = _watchdog_quote(globals().get("KEEL_CONTEXT", "") + "\nGraphHelm briefing UNOBSERVED.")
    if _watched_phase == "start" and output_format != "portable":
        return '{"hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":' + context + "}}\n"
    if output_format == "portable":
        report = '{"format":"graphhelm-portable-v1","phase":' + _watchdog_quote(_watched_phase) + ',"host":' + _watchdog_quote(host)
        if _watched_phase == "start":
            report += ',"sessionId":null,"context":' + context
        elif _watched_phase == "end":
            # A portable end always says its delivery, as the normal end report does.
            report += ',"delivery":"unobserved"'
        return report + ',"activation":"unobserved"}\n'
    return ""


def _probe_mark(event: str) -> None:
    """Probe only (GRAPHHELM_HOOK_FIRST_LINE set): one stderr line "graphhelm-probe-exit <pid>
    <watchdog|normal> <epoch>" right before `os._exit`, so a probe can split time inside the hook
    from teardown after it. It goes to the stderr pipe, never to a file: a file write on a starved
    disk can hold the watchdog before its os._exit. The time is taken before the write."""
    if not _first_line_os.environ.get("GRAPHHELM_HOOK_FIRST_LINE"):
        return
    at = _first_line_clock.time()
    try:
        _first_line_sys.stderr.write(f"graphhelm-probe-exit {_first_line_os.getpid()} {event} {at:.3f}\n")
        _first_line_sys.stderr.flush()
    except (OSError, ValueError):
        # stderr itself failed: there is nowhere left to report this probe-only mark, and the exit
        # must not wait.
        return


def _watchdog() -> None:
    _first_line_clock.sleep(HOOK_WATCHDOG)
    try:
        if not _answer_started:
            _first_line_sys.stdout._inner.write(_watchdog_answer())
        _first_line_sys.stderr.write(f"graphhelm-hook: {_watched_phase} unobserved (no answer {HOOK_WATCHDOG:g} s after the first line)\n")
        _first_line_sys.stdout.flush()
        _first_line_sys.stderr.flush()
    finally:
        _probe_mark("watchdog")
        # A timeout path exits 0: its UNOBSERVED answer is the visible warning and still reaches the
        # context. Only the faulthandler backstop exits 1.
        _first_line_os._exit(0)


if __name__ == "__main__" and _watched_phase in {"start", "end"}:
    _first_line_sys.stdout = _AnswerGuard(_first_line_sys.stdout)
    _first_line_threading.Thread(target=_watchdog, name="graphhelm-hook-watchdog", daemon=True).start()
    # If the Python watchdog cannot run (its thread blocked, or a call holding the GIL), this C timer,
    # which needs no Python lock, ends the process a short while after the watchdog's moment: it
    # writes every thread's stack to stderr (where it hung) and exits with status 1.
    import faulthandler as _first_line_faulthandler
    if _stderr_open:
        try:
            _first_line_faulthandler.dump_traceback_later(HOOK_WATCHDOG + 1.5, exit=True)
        except (RuntimeError, ValueError, OSError, AttributeError):
            # stderr is open but has no usable file descriptor: the dump has nowhere to go, so the
            # backstop stays off and the Python watchdog alone bounds the hook (#2353).
            _stderr_open = False

if _first_line_os.environ.get("GRAPHHELM_HOOK_FIRST_LINE"):
    try:
        _first_line_sys.stderr.write(f"graphhelm-probe-first {_first_line_os.getpid()} {_FIRST_LINE_AT:.3f}" + chr(10))
        _first_line_sys.stderr.flush()
    except (OSError, ValueError):
        # keel-allow: keel.rel.no_swallow stderr itself failed, so there is nowhere left to report this probe-only mark and the hook must not wait; re-decide with the probe (#2270, #2344) until 2027-02-01
        pass

import argparse
import codecs
from datetime import datetime, timezone
import hashlib
import itertools
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import tempfile
import threading
import time
import urllib.parse
import urllib.request
import uuid

MAX_INPUT = 64 * 1024
MAX_REPLY = 256 * 1024
MAX_STATE = 32 * 1024
MAX_CONTEXT = 1600
BRIEFING_TIMEOUT = 5.0
ID = re.compile(r"[A-Za-z0-9][A-Za-z0-9._:-]{0,127}\Z")
PORTABLE_HOST = re.compile(r"[A-Za-z0-9][A-Za-z0-9._:-]{0,63}\Z")
NEXT_STEPS = {"resume_held", "answer", "diagnose", "dispatch", "finished", "nothing"}
REMEDIES = {"approve", "claim", "clear", "amend_budget"}
STATUSES = {"completed", "failed", "cancelled"}
KEEL_CONTEXT = ("Keel is guidance and measurement, not a pass/fail ritual. Start from the task's "
                "promise and named paths. For a bounded code change, record scope, promise, and "
                "the smallest command that observes it. Keep skipped and unobserved separate from "
                "passed. A session ending does not prove task completion. "
                "Reuse current task evidence before tools. This is the default; do not wait for "
                "an owner reminder. For a source gap, load the installed keel skill and use "
                "contained code-index build/query when useful, with a valid private task bundle "
                "and your own actor. Pass resolvable evidence and this policy to subagents. "
                "Fall back to focused source reads for stale, partial or unavailable evidence. "
                "Never start a memory server/watcher in hooks or retain a provider process.")


def hook_input() -> dict:
    decoder = codecs.getincrementaldecoder("utf-8-sig")()
    parser = json.JSONDecoder()
    text = ""
    size = 0
    while True:
        chunk = sys.stdin.buffer.read1(4096)
        if not chunk:
            try:
                text += decoder.decode(b"", final=True)
                start = len(text) - len(text.lstrip(" \t\r\n"))
                value, end = parser.raw_decode(text, start)
            except (UnicodeDecodeError, json.JSONDecodeError) as error:
                raise ValueError("invalid hook input") from error
            if text[end:].strip(" \t\r\n"):
                raise ValueError("multiple hook inputs")
            if not isinstance(value, dict):
                raise ValueError("hook input is not an object")
            return value
        size += len(chunk)
        if size > MAX_INPUT:
            raise ValueError("hook input too large")
        try:
            text += decoder.decode(chunk)
        except UnicodeDecodeError as error:
            raise ValueError("invalid hook input") from error
        start = len(text) - len(text.lstrip(" \t\r\n"))
        try:
            value, end = parser.raw_decode(text, start)
        except json.JSONDecodeError:
            continue
        if decoder.getstate()[0]:
            raise ValueError("invalid hook input")
        if text[end:].strip(" \t\r\n"):
            raise ValueError("multiple hook inputs")
        if not isinstance(value, dict):
            raise ValueError("hook input is not an object")
        return value


def _session(payload: dict) -> str:
    value = payload.get("session_id")
    configured = os.environ.get("GRAPHHELM_SESSION_ID")
    if configured and (not ID.fullmatch(configured) or value != configured):
        raise ValueError("session identity mismatch")
    if not isinstance(value, str) or not ID.fullmatch(value):
        raise ValueError("invalid session identity")
    return value


def _lane_sessions_root() -> Path:
    return Path.home() / ".graphhelm" / "lane-sessions"


def _remember_lane(payload: dict) -> None:
    session = payload.get("session_id")
    title = payload.get("session_title")
    actor = title.split(" - ", 1)[0] if isinstance(title, str) and " - " in title else title
    if not isinstance(actor, str) or not ID.fullmatch(actor):
        if isinstance(title, str):
            print("lane not bound: title has no lane id", file=sys.stderr)
        return
    if not isinstance(session, str) or not ID.fullmatch(session):
        return
    root = _lane_sessions_root()
    root.mkdir(mode=0o700, parents=True, exist_ok=True)
    now = datetime.now(timezone.utc)
    cutoff = now.timestamp() - 7 * 24 * 60 * 60
    for path in root.glob("*.json"):
        try:
            if path.stat().st_mtime < cutoff:
                path.unlink()
        except OSError:
            continue
    path = root / f"{session}.json"
    raw = json.dumps({"actor": actor, "at": now.isoformat(timespec="seconds")}, separators=(",", ":")).encode()
    fd, name = tempfile.mkstemp(prefix=f".{session}.", suffix=".tmp", dir=root)
    temporary = Path(name)
    try:
        with os.fdopen(fd, "wb") as output:
            output.write(raw)
            output.flush()
            os.fsync(output.fileno())
        if os.name != "nt":
            temporary.chmod(0o600)
        os.replace(temporary, path)
    finally:
        try:
            temporary.unlink()
        except FileNotFoundError:
            pass


def _forget_lane(payload: dict) -> None:
    session = payload.get("session_id")
    if isinstance(session, str) and ID.fullmatch(session):
        try:
            (_lane_sessions_root() / f"{session}.json").unlink()
        except FileNotFoundError:
            pass


def _lane_actor(payload: dict) -> str | None:
    session = payload.get("session_id")
    if not isinstance(session, str) or not ID.fullmatch(session):
        return None
    try:
        path = _lane_sessions_root() / f"{session}.json"
        if path.stat().st_mtime < datetime.now(timezone.utc).timestamp() - 7 * 24 * 60 * 60:
            return None
        raw = path.read_bytes()
        if len(raw) > 4096:
            return None
        value = json.loads(raw)
        actor = value.get("actor") if isinstance(value, dict) else None
        return actor if isinstance(actor, str) and ID.fullmatch(actor) else None
    except (OSError, ValueError, TypeError, json.JSONDecodeError, RecursionError):
        return None


def _host(value: str) -> str:
    if not isinstance(value, str) or not PORTABLE_HOST.fullmatch(value):
        raise ValueError("invalid host identity")
    return value


def _binding_values(execution: str, token_file: str, url: str, node_id: str | None) -> tuple[str, str, str, str, str | None] | None:
    if not execution:
        return None
    if not token_file or not ID.fullmatch(execution):
        raise ValueError("invalid execution binding")
    if node_id is not None and not ID.fullmatch(node_id):
        raise ValueError("invalid node binding")
    parsed = urllib.parse.urlparse(url)
    if parsed.username or parsed.password or parsed.query or parsed.fragment or parsed.path:
        raise ValueError("invalid Runtime URL")
    if parsed.scheme == "http" and parsed.hostname not in {"127.0.0.1", "localhost", "::1"}:
        raise ValueError("plain HTTP Runtime must be loopback")
    if parsed.scheme not in {"http", "https"} or not parsed.hostname:
        raise ValueError("invalid Runtime URL")
    hostname = parsed.hostname.lower()
    authority = f"[{hostname}]" if ":" in hostname else hostname
    origin = f"{parsed.scheme}://{authority}:{parsed.port or (443 if parsed.scheme == 'https' else 80)}"
    return execution, token_file, url, origin, node_id


def binding(payload: dict | None = None) -> tuple[str, str, str, str, str | None] | None:
    execution = os.environ.get("GRAPHHELM_EXECUTION_ID", "")
    token_file = os.environ.get("GRAPHHELM_TOKEN_FILE", "")
    url = os.environ.get("GRAPHHELM_RUNTIME_URL", "http://127.0.0.1:8791").rstrip("/")
    node_id = os.environ.get("GRAPHHELM_NODE_ID")
    if execution:
        return _binding_values(execution, token_file, url, node_id)
    if payload is None:
        return None
    session = payload.get("session_id")
    if not isinstance(session, str) or not ID.fullmatch(session):
        return None
    try:
        actor = _lane_actor(payload)
        if actor is None:
            return None
        repo = Path(payload.get("cwd", ""))
        if not repo.is_absolute():
            return None
        for directory in (repo, *repo.parents):
            team_path = directory / ".graphhelm" / "team.json"
            if not team_path.is_file():
                continue
            team_raw = team_path.read_bytes()
            if len(team_raw) > 4096:
                return None
            team = json.loads(team_raw)
            if not isinstance(team, dict):
                return None
            candidate = _binding_values(execution or team.get("executionId", ""),
                                        token_file if os.environ.get("GRAPHHELM_TOKEN_FILE") else team.get("tokenFile", ""),
                                        (url if os.environ.get("GRAPHHELM_RUNTIME_URL") else
                                         str(team.get("runtimeUrl", ""))).rstrip("/"),
                                        node_id)
            if candidate is None or not Path(candidate[1]).is_file():
                return None
            os.environ["GRAPHHELM_ACTOR"] = actor
            return candidate
    except (OSError, TypeError, json.JSONDecodeError, RecursionError):
        return None
    return None


def token_from_file(path: str) -> str:
    with open(path, "rb") as source:
        raw = source.read(4097)
    if not raw or len(raw) > 4096:
        raise ValueError("invalid token file")
    token = raw.decode("utf-8").strip()
    if not token or "\n" in token or "\r" in token:
        raise ValueError("invalid token file")
    return token


def arm_lane_wake(payload: dict, bound: tuple[str, str, str, str, str | None], matures_in: int = 60,
                  cursor: int | None = None) -> int:
    """Renew the lane's finite lease from the last observed event head."""
    execution, token_file, url, _origin, _node = bound
    actor = os.environ.get("GRAPHHELM_ACTOR") or _lane_actor(payload)
    session = _session(payload)
    if not actor or not ID.fullmatch(actor):
        return
    token = token_from_file(token_file)
    base = f"{url}/v1/executions/{urllib.parse.quote(execution, safe='')}"
    deadline = time.monotonic() + ARM_BUDGET_SECONDS
    head = request(f"{base}/events?after=0&limit=1", token, "GET", timeout=ARM_BUDGET_SECONDS)["data"]["head"]
    if type(head) is not int or head < 0:
        raise ValueError("invalid event head")
    if cursor is not None and (type(cursor) is not int or cursor < 0 or cursor > head):
        raise ValueError("invalid wake cursor")
    armed_cursor = head if cursor is None else cursor
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        raise TimeoutError("wake lease arm budget exceeded")
    reply = request(f"{base}/wake-lease", token, "POST",
                    {"sessionId": session, "rendezvousId": str(uuid.uuid4()),
                     "cursor": armed_cursor, "maturesInSeconds": matures_in},
                    {"X-GraphHelm-Actor": actor, "X-GraphHelm-Actor-Type": "agent",
                     "Idempotency-Key": str(uuid.uuid4())}, timeout=remaining)
    data = reply.get("data")
    if not isinstance(data, dict) or data.get("sessionId") != session or data.get("armedCursor") != armed_cursor:
        raise ValueError("wake lease acknowledgement mismatch")
    return head


def _waiter_path(payload: dict, bound: tuple[str, str, str, str, str | None], host: str) -> Path:
    return state_path(bound[0], _session(payload), host, bound[3]).with_suffix(".waiter.pid")


HEARTBEAT_MAX_AGE = 30 * 60
CREATE_BREAKAWAY_FROM_JOB = 0x01000000
ARM_BUDGET_SECONDS = 5.0
# Consecutive unexpected wake-wait exits a waiter survives (#1118): one abort must not leave a lane
# deaf, but a CLI that fails every time must not spin. Back-off doubles from 1 s up to 60 s.
WAKE_WAIT_FAILURE_LIMIT = 5
# The tail of wake-wait's stderr kept in waiter.log, so an abort names its cause.
STDERR_TAIL = 400


def _heartbeat_path(payload: dict, bound: tuple[str, str, str, str, str | None], host: str) -> Path:
    return state_path(bound[0], _session(payload), host, bound[3]).with_suffix(".waiter.alive")


def _rang_path(payload: dict, bound: tuple[str, str, str, str, str | None], host: str) -> Path:
    """Touched on every ring, so an idle host's rewake (lane_stop_hook --rewake) wakes at once."""
    return state_path(bound[0], _session(payload), host, bound[3]).with_suffix(".waiter.rang")


def _waiter_log_path(session: str) -> Path:
    return _state_root() / f"{session}.waiter.log"


def _log_waiter_exit(session: str, reason: str) -> None:
    try:
        path = _waiter_log_path(session)
        path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        with path.open("a", encoding="utf-8") as output:
            output.write(f"{reason}\n")
    except OSError:
        pass


def touch_lane_heartbeat(payload: dict, bound: tuple[str, str, str, str, str | None], host: str) -> None:
    path = _heartbeat_path(payload, bound, host)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.touch()


def _heartbeat_fresh(path: Path) -> bool:
    try:
        return time.time() - path.stat().st_mtime < HEARTBEAT_MAX_AGE
    except OSError:
        return False


def _pid_alive(pid: int) -> bool:
    if pid <= 0:
        return False
    if os.name == "nt":
        import ctypes
        kernel = ctypes.windll.kernel32
        handle = kernel.OpenProcess(0x1000, False, pid)
        if not handle:
            return False
        try:
            code = ctypes.c_ulong()
            return bool(kernel.GetExitCodeProcess(handle, ctypes.byref(code))) and code.value == 259
        finally:
            kernel.CloseHandle(handle)
    try:
        os.kill(pid, 0)
        return True
    except OSError:
        return False


def start_lane_waiter(payload: dict, bound: tuple[str, str, str, str, str | None], host: str) -> None:
    """Start one local waiter for a bound session, without replacing a live owner."""
    configured = os.environ.get("GRAPHHELM_EVENTS_DIR")
    events = Path(configured) if configured else Path(bound[1]).parent / "events"
    if not events.is_dir():
        return
    touch_lane_heartbeat(payload, bound, host)
    pid_path = _waiter_path(payload, bound, host)
    try:
        pid = int(pid_path.read_text(encoding="ascii"))
        if _pid_alive(pid):
            return
    except (FileNotFoundError, ValueError, OSError):
        pass
    cursor = arm_lane_wake(payload, bound)
    pid_path.parent.mkdir(parents=True, exist_ok=True)
    argv = [sys.executable, str(Path(__file__).resolve()), "wait", "--host", host,
            "--session-id", _session(payload), "--events", str(events),
            "--heartbeat", str(_heartbeat_path(payload, bound, host)), "--cursor", str(cursor)]
    # #1058: a lane bound through team.json is re-resolved from the session's cwd; without it
    # the detached waiter found no binding and returned before ever parking on its lease.
    cwd = payload.get("cwd")
    if isinstance(cwd, str) and Path(cwd).is_absolute():
        argv += ["--cwd", cwd]
    flags = subprocess.CREATE_NO_WINDOW | subprocess.DETACHED_PROCESS if os.name == "nt" else 0
    # #919: the host runs hooks in a job object that is closed when the hook returns, which
    # killed every waiter before it could log an exit. Leave the job; a job that forbids
    # breakaway refuses the flag, and then the waiter starts inside it as before.
    attempts = (flags | CREATE_BREAKAWAY_FROM_JOB, flags) if os.name == "nt" else (flags,)
    for attempt, creationflags in enumerate(attempts):
        try:
            child = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                     stderr=subprocess.DEVNULL, close_fds=True,
                                     creationflags=creationflags, start_new_session=os.name != "nt")
            # #1118: a waiter killed with the hook's job leaves no exit line, so the spawn says
            # whether it left the job; a lane with "spawned:in_job" and no "started" was killed.
            _log_waiter_exit(_session(payload), f"spawned:{'in_job' if attempt else 'breakaway'}"
                             if os.name == "nt" else "spawned")
            break
        except OSError:
            if attempt == len(attempts) - 1:
                raise
    pid_path.write_text(str(child.pid), encoding="ascii")


def wait_for_lane(host: str, session: str, events: Path, heartbeat: Path,
                  cursor: int | None = None, cwd: str | None = None) -> None:
    payload = {"session_id": session}
    if cwd:
        payload["cwd"] = cwd
    bound = binding(payload)
    if bound is None:
        _log_waiter_exit(session, "unbound")
        return
    _log_waiter_exit(session, f"started:{os.getpid()}")
    cli = os.environ.get("GRAPHHELM_CLI", "graphhelm")
    pid_path = _waiter_path(payload, bound, host)
    exit_reason = "heartbeat_stale"
    failures = 0
    try:
      # SessionEnd removes this marker explicitly. Its age only says the host has been idle;
      # treating age as death drops an otherwise-live lane after a long quiet interval.
      while heartbeat.exists():
        result = subprocess.run([cli, "wake-wait", "--events", str(events),
                                 "--execution", bound[0], "--session-id", session],
                                stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, check=False,
                                text=True,
                                creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
        # An append can consume the lease before wake-wait parks its pipe. Its timeout
        # envelope carries the stale-burn receipt, even when an older CLI exits unusually.
        try:
            envelope = json.loads(getattr(result, "stdout", None) or "{}")
            data = envelope.get("data", {}) if isinstance(envelope, dict) else {}
            consumed = data.get("lastConsumed") if isinstance(data, dict) else None
            stale = (isinstance(consumed, dict) and
                     consumed.get("reason") == "stale_rendezvous" and
                     not data.get("misBurn"))
            # #2152: a ring aimed at this lane whose byte never reached the pipe is still a ring;
            # without this the rewake heard it only at the lease's maturity or its fallback.
            missed = data.get("missedRing") is True if isinstance(data, dict) else False
        except (TypeError, ValueError):
            stale = missed = False
        if result.returncode not in (0, 2, 3) and not stale:
            failures += 1
            tail = " ".join((getattr(result, "stderr", None) or "").split())[-STDERR_TAIL:]
            exit_reason = f"wake_wait_exit:{result.returncode}"
            if failures >= WAKE_WAIT_FAILURE_LIMIT:
                print(f"wake-wait exited {result.returncode} for {session} {failures} times; "
                      "stopping waiter", file=sys.stderr)
                exit_reason += f":{tail}"
                return
            _log_waiter_exit(session, f"{exit_reason}:{tail}")
            print(f"wake-wait exited {result.returncode} for {session}; re-arming waiter", file=sys.stderr)
            time.sleep(min(2 ** (failures - 1), 60))
        else:
            failures = 0
            exit_reason = "heartbeat_stale"
            if result.returncode == 0 or stale or missed:
                try:
                    _rang_path(payload, bound, host).touch()
                except OSError:
                    pass
        if not heartbeat.exists():
            return
        if stale:
            print(f"stale rendezvous consumed for {session}; re-arming waiter", file=sys.stderr)
        # #1058: a stale burn already lost the lease, so it re-arms at once; only a plain
        # timeout pauses, to keep an idle lane from spinning on its own lease.
        if result.returncode == 2 and not stale:
            time.sleep(1)
        try:
            cursor = arm_lane_wake(payload, bound, cursor=cursor)
        except (OSError, ValueError, KeyError):
            time.sleep(1)
    except Exception as error:
        exit_reason = f"exception:{type(error).__name__}"
        raise
    finally:
        if not _heartbeat_fresh(heartbeat):
            try:
                arm_lane_wake(payload, bound, matures_in=1)
            except (OSError, ValueError, KeyError):
                pass
        try:
            if int(pid_path.read_text(encoding="ascii")) == os.getpid():
                pid_path.unlink()
        except (FileNotFoundError, ValueError, OSError):
            pass
        _log_waiter_exit(session, exit_reason)


def stop_lane_waiter(payload: dict, bound: tuple[str, str, str, str, str | None], host: str) -> None:
    _heartbeat_path(payload, bound, host).unlink(missing_ok=True)
    pid_path = _waiter_path(payload, bound, host)
    try:
        pid = int(pid_path.read_text(encoding="ascii"))
        if _pid_alive(pid):
            os.kill(pid, signal.SIGTERM)
        pid_path.unlink(missing_ok=True)
    except (FileNotFoundError, ValueError, OSError):
        pass
    try:
        arm_lane_wake(payload, bound, matures_in=1)
    except (OSError, ValueError, KeyError):
        pass


def request(url: str, token: str, method: str, body: dict | None = None, headers: dict | None = None, timeout: float = 1.0, max_reply: int = MAX_REPLY) -> dict:
    data = None if body is None else json.dumps(body, separators=(",", ":")).encode("utf-8")
    request_headers = {"Authorization": f"Bearer {token}"}
    if data is not None:
        request_headers["Content-Type"] = "application/json"
    request_headers.update(headers or {})
    req = urllib.request.Request(url, data=data, headers=request_headers, method=method)

    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, *_):
            return None

    # This is socket inactivity, not a wall-time guarantee. The host hook timeout
    # bounds the whole process; SessionEnd allows one slower durable acknowledgement.
    with urllib.request.build_opener(NoRedirect).open(req, timeout=timeout) as response:
        raw = response.read(max_reply + 1)
    if len(raw) > max_reply:
        raise ValueError("Runtime reply too large")
    result = json.loads(raw)
    if not isinstance(result, dict) or result.get("ok") is not True:
        raise ValueError("Runtime refused hook operation")
    return result


def state_path(execution: str, session: str, host: str, origin: str = "") -> Path:
    digest = hashlib.sha256(f"v2\0{execution}\0{session}\0{host}\0{origin}".encode()).hexdigest()
    return _state_root() / f"{digest}.json"


def _state_root() -> Path:
    root = os.environ.get("GRAPHHELM_HOOK_STATE_DIR")
    if root:
        return Path(root)
    base = os.environ.get("LOCALAPPDATA") or os.environ.get("XDG_STATE_HOME")
    return Path(base) / "GraphHelm" / "session-hooks" if base else Path.home() / ".local" / "state" / "graphhelm" / "session-hooks"


def _write_state(path: Path, value: dict, durable: bool = True) -> None:
    """Atomic replace; `durable` also waits for fsync. #2295: SessionEnd passes False, because a
    host gives it 10 s and an fsync under load took longer. A lost write only re-sends the same
    idempotent signal, or a new one after a power loss."""
    raw = json.dumps(value, separators=(",", ":"), sort_keys=True).encode("utf-8")
    if len(raw) > MAX_STATE:
        raise ValueError("hook state too large")
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    fd, name = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    temporary = Path(name)
    try:
        with os.fdopen(fd, "wb") as output:
            output.write(raw)
            output.flush()
            if durable:
                os.fsync(output.fileno())
        if os.name != "nt":
            temporary.chmod(0o600)
        os.replace(temporary, path)
    finally:
        try:
            temporary.unlink()
        except FileNotFoundError:
            pass


# Under disk pressure the state write itself (mkstemp, write, replace), not only its fsync, can
# outlast the host's hook budget. Each piece of hook file I/O (lock, read, write, token) gets this
# long; past it the hook answers UNOBSERVED, records nothing more for that session
# and exits at once (`_StateTimeout`, `_answered_without_state`); an abandoned write may not land.
STATE_WRITE_BUDGET = 2.0
_answered_without_state = False


class _StateTimeout(BaseException):
    """A piece of hook file I/O did not answer within STATE_WRITE_BUDGET. Raised past every
    `except Exception` and OSError handler on purpose: the hook stops there, answers UNOBSERVED in
    `main`, records nothing more for the session and exits at once. Going on into more file I/O
    (the token, a second write) would only stall again on the same disk."""


def _bounded(what: str, call, budget: float | None = None):
    """Runs one piece of hook file I/O for at most STATE_WRITE_BUDGET (`budget` when given) and
    returns its result. When the file system has not answered by then, raises `_StateTimeout`. An
    error raised in time is raised here. In the helper process (#2344) there is no hook answer to
    protect, so the call runs directly."""
    global _answered_without_state
    if _IN_HELPER:
        return call()
    outcome: list = []

    def run() -> None:
        try:
            outcome.append((True, call()))
        except BaseException as error:  # handed to the caller below when it finishes in time
            outcome.append((False, error))

    worker = threading.Thread(target=run, name=f"graphhelm-state-{what}", daemon=True)
    worker.start()
    worker.join(STATE_WRITE_BUDGET if budget is None else budget)
    if not outcome:
        _answered_without_state = True
        print(f"graphhelm-session-hook: state {what} still pending; answering without it", file=sys.stderr)
        raise _StateTimeout(what)
    ok, value = outcome[0]
    if not ok:
        raise value
    return value


def _write_state_bounded(path: Path, value: dict, durable: bool = True) -> None:
    """`_write_state` that never holds the answer past STATE_WRITE_BUDGET. A write that is still
    pending is abandoned, never torn: the replace stays atomic. A lost write costs a retried,
    idempotent signal (the Runtime's 409 for an already committed key counts as delivered), or a
    later compact briefing that reads UNOBSERVED because its cache is missing."""
    snapshot = json.loads(json.dumps(value))
    _bounded("write", lambda: _write_state(path, snapshot, durable))


def _read_state_or_empty(path: Path, execution: str, session: str, host: str, origin: str) -> dict | None:
    """The state as `_read_state` validates it, or None (empty) when the file is unreadable:
    empty or half-written after a crash or a power loss (the end write is not fsync'd), not JSON, or
    not this format. One stderr warning says so; the hook carries on as for a missing file. A
    parseable state that fails validation (another identity, a forged receipt, an injected
    briefing) still fails closed. Another OSError (permission, sharing violation) is not "empty": it
    propagates and the hook answers UNOBSERVED. A read that does not answer in time raises
    `_StateTimeout`."""
    try:
        return _bounded("read", lambda: _read_state(path, execution, session, host, origin))
    except (FileNotFoundError, UnicodeDecodeError, json.JSONDecodeError, _UnreadableState) as error:
        # #2353: any other OSError (permission, sharing violation, a directory in the way) is a real
        # problem; it reaches main, which warns once and answers UNOBSERVED.
        print(f"graphhelm-session-hook: hook state unreadable ({type(error).__name__}); treated as empty", file=sys.stderr)
        return None


def _token_bounded(path: str) -> str:
    """The token read in start and end is file I/O too, bounded like the state."""
    return _bounded("token", lambda: token_from_file(path))


def _claim_lock_bounded(path: Path):
    """`_claim_lock`, bounded like every other piece of hook file I/O."""
    return _bounded("lock", lambda: _claim_lock(path))


class _UnreadableState(ValueError):
    """A state file that is not a whole hook state: too large, not an object, or another version."""


def _read_state(path: Path, execution: str, session: str, host: str, origin: str) -> dict | None:
    try:
        with path.open("rb") as source:
            raw = source.read(MAX_STATE + 1)
    except FileNotFoundError:
        return None
    if len(raw) > MAX_STATE:
        raise _UnreadableState("hook state too large")
    value = json.loads(raw)
    if not isinstance(value, dict) or value.get("version") != 2:
        raise _UnreadableState("invalid hook state version")
    if any(value.get(key) != expected for key, expected in (("executionId", execution), ("sessionId", session), ("host", host), ("origin", origin))):
        raise ValueError("hook state identity mismatch")
    if "briefing" in value:
        value["briefing"] = _validate_digest(value["briefing"])
    if "delivery" in value:
        _validate_delivery(value["delivery"], execution, session, host, origin)
    return value


def _claim_lock(path: Path):
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    lock = path.with_suffix(".lock")
    handle = os.fdopen(os.open(lock, os.O_RDWR | os.O_CREAT, 0o600), "r+b")
    try:
        if os.name == "nt":
            import msvcrt
            msvcrt.locking(handle.fileno(), msvcrt.LK_NBLCK, 1)
        else:
            import fcntl
            fcntl.flock(handle.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        return handle
    except OSError:
        handle.close()
        raise ValueError("hook state busy") from None


def _release_lock(lock) -> None:
    # OS locks are released even when the host kills a timed-out hook. Keep the
    # inode: unlinking it could let a second process lock a different file.
    # The unlock and close can stall on a starved disk too. After a timeout the hook leaves at once
    # and the OS releases the lock with the process; otherwise the release is bounded like any
    # other piece of hook file I/O.
    global _answered_without_state
    if lock is None or _answered_without_state:
        return
    worker = threading.Thread(target=lock.close, name="graphhelm-state-unlock", daemon=True)
    worker.start()
    worker.join(STATE_WRITE_BUDGET)
    if worker.is_alive():
        # The answer and the state are already done: this is the normal path (exit 0), and start
        # and end leave with os._exit anyway, so the stuck release cannot hold the process.
        print("graphhelm-session-hook: state unlock still pending; leaving without it", file=sys.stderr)


# #2344: the process the host waits on does no file I/O after its first line. A detached helper
# process does the state, lock, token, binding and lane-file work; the hook asks it over the helper's
# own stdin/stdout, two anonymous pipes that only the hook and the helper hold, so there is no named
# endpoint for another user or process to connect to. A reply that does not come within the budget
# means the hook answers UNOBSERVED and leaves; it never falls back to doing the I/O itself.
HELPER_LIFETIME = 30.0  # a helper ends itself this long after it starts, whatever it is doing
HELPER_FIRST_BUDGET = 4.0  # the first request also waits for the helper to start
HELPER_REAP_GRACE = 10.0
_IN_HELPER = False


def _process_identity(pid: int) -> str | None:
    """A token that tells one process from a later one that reuses its PID: its creation time.
    None when it cannot be read, and then nothing is ever stopped on that PID's account."""
    try:
        if os.name == "nt":
            import ctypes
            from ctypes import wintypes
            kernel = ctypes.windll.kernel32
            kernel.OpenProcess.restype = wintypes.HANDLE
            handle = kernel.OpenProcess(0x1000, False, pid)
            if not handle:
                return None
            try:
                created, exited, kernel_time, user_time = (wintypes.FILETIME() for _ in range(4))
                if not kernel.GetProcessTimes(handle, ctypes.byref(created), ctypes.byref(exited),
                                              ctypes.byref(kernel_time), ctypes.byref(user_time)):
                    return None
                return str((created.dwHighDateTime << 32) | created.dwLowDateTime)
            finally:
                kernel.CloseHandle(handle)
        with open(f"/proc/{pid}/stat", "rb") as source:
            return source.read().decode("ascii", "replace").rsplit(")", 1)[1].split()[19]
    except (OSError, ValueError, IndexError, AttributeError):
        return None


def _terminate_process(pid: int) -> None:
    if os.name == "nt":
        import ctypes
        kernel = ctypes.windll.kernel32
        handle = kernel.OpenProcess(0x0001, False, pid)
        if handle:
            kernel.TerminateProcess(handle, 1)
            kernel.CloseHandle(handle)
        return
    os.kill(pid, signal.SIGKILL)


def _helpers_dir() -> Path:
    return _state_root() / "helpers"


def _reap_stale_helpers(directory: Path) -> None:
    """Stops a helper that outlived its own lifetime (stuck on a starved disk). A record names a
    PID and the process's creation time; only a process that still has that creation time is
    stopped, so a reused PID is never touched. A record whose process is gone, or whose identity
    cannot be read, is only removed."""
    now = time.time()
    for record in directory.glob("*.json"):
        try:
            data = json.loads(record.read_text(encoding="utf-8"))
            pid, started, at = int(data["pid"]), str(data["started"]), float(data["at"])
        except (OSError, ValueError, KeyError, TypeError):
            record.unlink(missing_ok=True)
            continue
        if pid == os.getpid() or now - at <= HELPER_LIFETIME + HELPER_REAP_GRACE:
            continue
        if started and _process_identity(pid) == started:
            _terminate_process(pid)
        record.unlink(missing_ok=True)


def _helper_housekeeping() -> None:
    """The helper's own pid record and the reaping of stale ones, off the request path: a starved
    disk must not hold the first reply."""
    directory = _helpers_dir()
    directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    _reap_stale_helpers(directory)
    record = {"pid": os.getpid(), "started": _process_identity(os.getpid()) or "", "at": time.time()}
    descriptor = os.open(directory / f"{os.getpid()}.json", os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(descriptor, "w", encoding="utf-8") as output:
        output.write(json.dumps(record, separators=(",", ":")))


_HELPER_LOCKS: dict[int, object] = {}


def _op_claim_lock(path: str) -> int:
    lock = _claim_lock(Path(path))
    _HELPER_LOCKS[id(lock)] = lock
    return id(lock)


def _op_release_lock(handle: int) -> None:
    lock = _HELPER_LOCKS.pop(handle, None)
    if lock is not None:
        lock.close()


_HELPER_OPS = {
    "binding": lambda payload: binding(payload),
    "lane_actor": lambda payload: _lane_actor(payload),
    "remember_lane": lambda payload: _remember_lane(payload),
    "forget_lane": lambda payload: _forget_lane(payload),
    "start_waiter": lambda payload, bound, host: start_lane_waiter(payload, tuple(bound), host),
    "stop_waiter": lambda payload, bound, host: stop_lane_waiter(payload, tuple(bound), host),
    "read_state": lambda path, execution, session, host, origin: _read_state_or_empty(Path(path), execution, session, host, origin),
    "write_state": lambda path, value, durable: _write_state(Path(path), value, durable),
    "token": lambda path: token_from_file(path),
    "claim_lock": _op_claim_lock,
    "release_lock": _op_release_lock,
}


def _helper_dispatch(line: bytes) -> bytes:
    import contextlib
    import io
    errors, printed = io.StringIO(), io.StringIO()
    actor_before = os.environ.get("GRAPHHELM_ACTOR")
    try:
        request_ = json.loads(line)
        with contextlib.redirect_stderr(errors), contextlib.redirect_stdout(printed):
            result = _HELPER_OPS[request_["op"]](**request_.get("args", {}))
        reply = {"ok": True, "result": list(result) if isinstance(result, tuple) else result}
    except Exception as error:  # every failure is handed to the hook, which raises the same type
        reply = {"ok": False, "error": {"type": type(error).__name__, "message": str(error)}}
    actor = os.environ.get("GRAPHHELM_ACTOR")
    if actor and actor != actor_before:
        reply["actor"] = actor
    reply["stderr"] = errors.getvalue()[:8192]
    return json.dumps(reply, separators=(",", ":")).encode("ascii") + b"\n"


def _helper_main() -> int:
    global _IN_HELPER
    _IN_HELPER = True
    # The lifetime can only be shortened (a test's hung helper must not outlive its temp dir).
    lifetime = min(HELPER_LIFETIME, float(os.environ.get("GRAPHHELM_HOOK_HELPER_LIFETIME") or HELPER_LIFETIME))
    threading.Thread(target=lambda: (time.sleep(lifetime), os._exit(0)), name="graphhelm-helper-lifetime", daemon=True).start()
    # A call that never releases the GIL (a runaway regex, a C loop) starves that thread, and a
    # helper once spun for hours that way. The C-level timer needs no GIL: it ends the process at the
    # lifetime whatever Python code is running (exit status 1; its stack dump goes to the null stderr).
    try:
        import faulthandler
        faulthandler.dump_traceback_later(lifetime, exit=True)
    except (RuntimeError, OSError, ValueError) as error:
        print(f"graphhelm-session-hook: helper backstop not armed ({type(error).__name__})", file=sys.stderr)
    threading.Thread(target=_helper_housekeeping, name="graphhelm-helper-housekeeping", daemon=True).start()
    source, sink = sys.stdin.buffer, sys.stdout.buffer
    while True:
        line = source.readline()
        if not line:
            break  # the hook left: its requests were all answered, one at a time
        try:
            sink.write(_helper_dispatch(line))
            sink.flush()
        except OSError:
            break  # the hook left without reading the reply
    try:
        (_helpers_dir() / f"{os.getpid()}.json").unlink(missing_ok=True)
    finally:
        os._exit(0)


def _remote_error(kind: str, message: str) -> BaseException:
    import builtins
    cls = _UnreadableState if kind == "_UnreadableState" else getattr(builtins, kind, None)
    if isinstance(cls, type) and issubclass(cls, Exception):
        try:
            return cls(message)
        except TypeError:
            return ValueError(message)
    return RuntimeError(f"{kind}: {message}")


class _HelperClient:
    def __init__(self, host: str):
        self.host = host
        self.process = None
        self.failure: BaseException | None = None
        self.used = False
        self._started = threading.Event()
        self._turn = threading.Lock()
        threading.Thread(target=self._spawn, name="graphhelm-helper-spawn", daemon=True).start()

    def _spawn(self) -> None:
        argv = [sys.executable, os.path.abspath(__file__), "helper", "--host", self.host]
        flags = subprocess.CREATE_NO_WINDOW | subprocess.DETACHED_PROCESS if os.name == "nt" else 0
        # As the waiter (#919): leave the host's job object when it allows, or the helper dies with
        # the hook's job; a job that forbids breakaway refuses the flag.
        attempts = (flags | CREATE_BREAKAWAY_FROM_JOB, flags) if os.name == "nt" else (flags,)
        for creationflags in attempts:
            try:
                # The helper's pipes are its own: nothing of the hook's stdout/stderr pipe reaches it.
                self.process = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                                stderr=subprocess.DEVNULL, close_fds=True,
                                                creationflags=creationflags, start_new_session=os.name != "nt")
                break
            except OSError as error:
                self.failure = error
        self._started.set()

    def call(self, op: str, args: dict):
        self._started.wait()
        if self.process is None:
            raise OSError(f"hook helper not started ({type(self.failure).__name__})")
        with self._turn:
            self.process.stdin.write(json.dumps({"op": op, "args": args}, separators=(",", ":")).encode("ascii") + b"\n")
            self.process.stdin.flush()
            raw = self.process.stdout.readline()
        if not raw:
            raise OSError("hook helper ended before it answered")
        reply = json.loads(raw)
        if reply.get("stderr"):
            sys.stderr.write(reply["stderr"])
        if reply.get("actor"):
            os.environ["GRAPHHELM_ACTOR"] = reply["actor"]
        if not reply["ok"]:
            raise _remote_error(reply["error"]["type"], reply["error"]["message"])
        return reply["result"]


class _RemoteLock:
    def __init__(self, handle: int):
        self.handle = handle


def _use_helper(host: str) -> None:
    """Hook mode: every piece of file I/O the hook used to do becomes a request to the helper, with
    the same bounds and the same errors, so `start` and `end` read as they always did."""
    client = _HelperClient(host)

    def remote(what: str, op: str, **args):
        budget = None if client.used else HELPER_FIRST_BUDGET
        client.used = True
        return _bounded(what, lambda: client.call(op, args), budget)

    real_binding = binding

    def binding_(payload: dict | None = None):
        if os.environ.get("GRAPHHELM_EXECUTION_ID") or payload is None:
            return real_binding(payload)  # from the environment alone: no file is read
        value = remote("binding", "binding", payload=payload)
        return tuple(value) if value is not None else None

    def release_(lock) -> None:
        if lock is None or _answered_without_state:
            return
        def unlock() -> None:
            try:
                client.call("release_lock", {"handle": lock.handle})
            except (OSError, ValueError) as error:
                print(f"graphhelm-session-hook: state unlock failed ({type(error).__name__})", file=sys.stderr)

        worker = threading.Thread(target=unlock, name="graphhelm-state-unlock", daemon=True)
        worker.start()
        worker.join(STATE_WRITE_BUDGET)
        if worker.is_alive():
            print("graphhelm-session-hook: state unlock still pending; leaving without it", file=sys.stderr)

    def read_(path, execution, session, host_, origin):
        return remote("read", "read_state", path=str(path), execution=execution, session=session, host=host_, origin=origin)

    def write_(path, value, durable=True):
        remote("write", "write_state", path=str(path), value=json.loads(json.dumps(value)), durable=durable)

    replacements = {
        "binding": binding_,
        "_lane_actor": lambda payload: remote("lane", "lane_actor", payload=payload),
        "_remember_lane": lambda payload: remote("lane", "remember_lane", payload=payload),
        "_forget_lane": lambda payload: remote("lane", "forget_lane", payload=payload),
        "start_lane_waiter": lambda payload, bound, host_: remote("waiter", "start_waiter", payload=payload, bound=list(bound), host=host_),
        "stop_lane_waiter": lambda payload, bound, host_: remote("waiter", "stop_waiter", payload=payload, bound=list(bound), host=host_),
        "_read_state_or_empty": read_,
        "_write_state_bounded": write_,
        "_token_bounded": lambda path: remote("token", "token", path=path),
        "_claim_lock_bounded": lambda path: _RemoteLock(remote("lock", "claim_lock", path=str(path))),
        "_release_lock": release_,
    }
    globals().update(replacements)


def _briefing_digest(data: dict) -> dict:
    sequence = data.get("asOfSequence")
    if type(sequence) is not int or not 0 <= sequence <= 2**64 - 1:
        raise ValueError("invalid briefing sequence")
    step = data.get("nextStep")
    if not isinstance(step, dict) or not isinstance(step.get("kind"), str) or step["kind"] not in NEXT_STEPS:
        raise ValueError("invalid next step")
    kind = step["kind"]
    next_step = {"kind": kind}
    for key in ("node", "remedy", "status"):
        if key in step:
            if not isinstance(step[key], str) or len(step[key]) > 128:
                raise ValueError("invalid next step field")
            if key == "node" and not ID.fullmatch(step[key]):
                raise ValueError("invalid next step node")
            if key == "remedy" and step[key] not in REMEDIES:
                raise ValueError("invalid next step remedy")
            if key == "status" and step[key] not in STATUSES:
                raise ValueError("invalid next step status")
            next_step[key] = step[key]
    if kind in {"dispatch", "resume_held"} and isinstance(step.get("nodes"), list):
        next_step["nodeCount"] = min(len(step["nodes"]), 10000)
    counts = {}
    for name in ("pending", "unevaluated", "decisions", "workDone"):
        value = data.get(name)
        if isinstance(value, list):
            counts[f"{name}Count"] = min(len(value), 10000)
    return {"asOfSequence": sequence, "nextStep": next_step, "counts": counts}


def _validate_digest(value: dict) -> dict:
    if not isinstance(value, dict):
        raise ValueError("invalid cached briefing")
    digest = _briefing_digest(value)
    counts = value.get("counts")
    if not isinstance(counts, dict):
        raise ValueError("invalid cached counts")
    for key in ("pendingCount", "unevaluatedCount", "decisionsCount", "workDoneCount"):
        if key in counts:
            if type(counts[key]) is not int or not 0 <= counts[key] <= 10000:
                raise ValueError("invalid cached count")
            digest["counts"][key] = counts[key]
    node_count = value["nextStep"].get("nodeCount")
    if node_count is not None:
        if type(node_count) is not int or not 0 <= node_count <= 10000:
            raise ValueError("invalid cached node count")
        digest["nextStep"]["nodeCount"] = node_count
    return digest


def _context(digest: dict, freshness: str, execution: str, session: str, node_id: str | None) -> str:
    identity = f"execution {execution}; session {session}"
    if node_id:
        identity += f"; configured node {node_id} (assignment unverified)"
    value = f"GraphHelm {freshness}; {identity}; sequence {digest['asOfSequence']}; nextStep {json.dumps(digest['nextStep'], separators=(',', ':'))}; counts {json.dumps(digest['counts'], separators=(',', ':'))}."
    if freshness == "CACHED":
        value += " Cached local briefing; not refreshed."
    if len(value) > MAX_CONTEXT:
        raise ValueError("briefing context too large")
    return value


SESSION_PROTOCOL = "graphhelm-session-v1"


def _lane(payload: dict) -> str | None:
    """#1184: the lane this session works as, when it is one: the env actor, or the lane name the
    SessionStart title / team.json binding recorded. None for a session that is not a lane."""
    lane = os.environ.get("GRAPHHELM_ACTOR") or _lane_actor(payload)
    return lane if isinstance(lane, str) and ID.fullmatch(lane) else None


def _session_details(phase: str, execution: str, session: str, host: str, lane: str, node_id: str | None) -> str:
    details = {"protocol": SESSION_PROTOCOL, "phase": phase, "executionId": execution, "host": host,
               "sessionId": session, "lane": lane, "declaredNodeId": node_id}
    if phase == "ended":
        details["verified"] = False
    return json.dumps(details, separators=(",", ":"), sort_keys=True)


def _started_signal(execution: str, session: str, host: str, origin: str, lane: str, node_id: str | None,
                    emitted_at: str) -> tuple[str, dict]:
    digest = hashlib.sha256(f"v1\0{execution}\0{session}\0{host}\0{origin}\0{lane}".encode()).hexdigest()[:24]
    key = f"session-start-v1-{digest}"
    signal = {"id": key, "source": {"type": "tool", "id": lane}, "type": "agent_session_started", "severity": "low",
              "description": _session_details("started", execution, session, host, lane, node_id),
              "evidence": [execution], "emittedAt": emitted_at}
    return key, signal


def _record_started(bound: tuple, host: str, session: str, lane: str, state: dict, path: Path) -> None:
    """#1184: one agent_session_started per lane session, signed as the lane. A resume, clear or
    compact of the same session reuses the key and posts nothing once the state says it landed.
    Failure is said on stderr and never costs the briefing."""
    if state.get("started") is True:
        return
    execution, token_file, url, origin, node_id = bound
    emitted_at = datetime.now(timezone.utc).isoformat(timespec="seconds")
    key, signal = _started_signal(execution, session, host, origin, lane, node_id, emitted_at)
    try:
        try:
            reply = request(f"{url}/v1/executions/{urllib.parse.quote(execution, safe='')}/signal",
                            _token_bounded(token_file), "POST", {"signal": signal},
                            {"X-GraphHelm-Actor": lane, "X-GraphHelm-Actor-Type": "agent",
                             "X-GraphHelm-Actor-Session": session, "Idempotency-Key": key}, timeout=2.0)
        except urllib.error.HTTPError as error:
            if error.code != 409:
                raise
            reply = {"data": {"executionId": execution, "signalId": key}}  # GHE003: already committed
        data = reply.get("data")
        if not isinstance(data, dict) or data.get("executionId") != execution or data.get("signalId") != key:
            raise ValueError("signal acknowledgment did not match")
        state["started"] = True
        _write_state_bounded(path, state)
    except (OSError, ValueError, KeyError) as error:
        print(f"graphhelm-session-hook: start signal unobserved ({type(error).__name__})", file=sys.stderr)


def _start_impl(payload: dict, host: str, output_format: str = "native") -> None:
    if payload.get("hook_event_name") != "SessionStart":
        raise ValueError("wrong hook event")
    session = _session(payload)
    bound = binding(payload)
    if bound is None:
        if os.environ.get("GRAPHHELM_KEEL_CONTEXT") != "1":
            if output_format == "portable":
                print(json.dumps({"format": "graphhelm-portable-v1", "phase": "start", "host": host, "sessionId": session, "activation": "unobserved", "context": ""}, separators=(",", ":")))
            return
        if output_format == "portable":
            print(json.dumps({"format": "graphhelm-portable-v1", "phase": "start", "host": host, "sessionId": session, "activation": "unobserved", "context": KEEL_CONTEXT}, separators=(",", ":")))
        else:
            print(json.dumps({"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": KEEL_CONTEXT}}))
        return
    execution, token_file, url, origin, node_id = bound
    path = state_path(execution, session, host, origin)
    state = _read_state_or_empty(path, execution, session, host, origin)
    source = payload.get("source") or payload.get("session_start_source") or "startup"
    digest = state.get("briefing") if source in {"compact", "clear"} and state else None
    if source == "compact" and not isinstance(digest, dict):
        raise ValueError("compact briefing cache unavailable")
    freshness = "CACHED" if isinstance(digest, dict) else "FRESH"
    if not isinstance(digest, dict):
        try:
            reply = request(f"{url}/v1/executions/{urllib.parse.quote(execution, safe='')}/briefing", _token_bounded(token_file), "GET", timeout=BRIEFING_TIMEOUT)
            data = reply.get("data")
            if not isinstance(data, dict):
                raise ValueError("invalid briefing")
        except Exception:
            # #919: a slow Runtime times the briefing out. The briefing stays UNOBSERVED (the
            # caller reports it), but the lane still gets its waiter: the waiter never needed the
            # briefing, and without it no lane held a wake lease.
            try:
                start_lane_waiter(payload, bound, host)
            except Exception as error:
                print(f"graphhelm-session-hook: waiter not started ({type(error).__name__})", file=sys.stderr)
            raise
        digest = _briefing_digest(data)
        state = state or {"version": 2, "executionId": execution, "sessionId": session, "host": host, "origin": origin}
        state["briefing"] = digest
        _write_state_bounded(path, state)
    # #1184: the started record is appended before the waiter arms its wake lease, so the append
    # cannot consume the lease the waiter is about to park on.
    lane = _lane(payload)
    if lane is not None:
        _record_started(bound, host, session, lane, state, path)
    start_lane_waiter(payload, bound, host)
    context = KEEL_CONTEXT + "\n" + _context(digest, freshness, execution, session, node_id)
    if len(context) > MAX_CONTEXT:
        raise ValueError("hook context too large")
    if output_format == "portable":
        print(json.dumps({"format": "graphhelm-portable-v1", "phase": "start", "host": host, "sessionId": session, "activation": "unobserved", "context": context}, separators=(",", ":")))
    else:
        print(json.dumps({"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": context}}))


def start(payload: dict, host: str, output_format: str = "native") -> None:
    host = _host(host)
    _remember_lane(payload)
    bound = binding(payload)
    if bound is None:
        _start_impl(payload, host, output_format)
        return
    execution, _, _, origin, _ = bound
    lock = _claim_lock_bounded(state_path(execution, _session(payload), host, origin))
    try:
        _start_impl(payload, host, output_format)
    finally:
        _release_lock(lock)


def _signal(execution: str, session: str, host: str, emitted_at: str, node_id: str | None, origin: str,
            lane: str | None = None) -> tuple[str, dict]:
    if lane is not None:
        # #1184: a lane's end is signed as the lane, the name its task records and the Studio's
        # lanes carry; host and session id travel inside the description.
        digest = hashlib.sha256(f"v3\0{execution}\0{session}\0{host}\0{origin}\0{lane}".encode()).hexdigest()[:24]
        key = f"session-end-v3-{digest}"
        signal = {"id": key, "source": {"type": "tool", "id": lane}, "type": "agent_session_ended", "severity": "low",
                  "description": _session_details("ended", execution, session, host, lane, node_id),
                  "evidence": [execution], "emittedAt": emitted_at}
        return key, signal
    digest = hashlib.sha256(f"v2\0{execution}\0{session}\0{host}\0{origin}".encode()).hexdigest()[:24]
    key = f"session-end-v2-{digest}"
    actor = f"{host}-session-{session}"
    if len(actor) > 128:
        identity = f"{host}\0{session}"
        actor = f"agent-session-{hashlib.sha256(identity.encode()).hexdigest()[:48]}"
    description = f"{actor} session ended; task outcome was not verified by this hook."
    if node_id:
        description += f" Declared node binding label: {node_id}."
    signal = {"id": key, "source": {"type": "tool", "id": actor}, "type": "agent_session_ended", "severity": "low", "description": description, "evidence": [execution], "emittedAt": emitted_at}
    return key, signal


def _validate_delivery(value: dict, execution: str, session: str, host: str, origin: str) -> None:
    if not isinstance(value, dict) or value.get("executionId") != execution or value.get("sessionId") != session or type(value.get("delivered")) is not bool:
        raise ValueError("invalid delivery identity")
    node = value.get("declaredNodeId")
    if node is not None and (not isinstance(node, str) or not ID.fullmatch(node)):
        raise ValueError("invalid delivery node")
    signal = value.get("signal")
    if not isinstance(signal, dict) or not isinstance(signal.get("emittedAt"), str):
        raise ValueError("invalid stored signal")
    timestamp = signal["emittedAt"]
    if len(timestamp) > 40 or datetime.fromisoformat(timestamp).tzinfo is None:
        raise ValueError("invalid signal timestamp")
    lane = value.get("lane")
    if lane is not None and (not isinstance(lane, str) or not ID.fullmatch(lane)):
        raise ValueError("invalid delivery lane")
    key, expected = _signal(execution, session, host, timestamp, node, origin, lane)
    if value.get("signalId") != key or signal != expected:
        raise ValueError("invalid stored signal")


def _end_impl(payload: dict, host: str, bound: tuple | None = None, lane: str | None = None) -> None:
    if payload.get("hook_event_name") != "SessionEnd":
        raise ValueError("wrong hook event")
    if bound is None:
        bound = binding(payload)
    if bound is None:
        return
    execution, token_file, url, origin, node_id = bound
    session = _session(payload)
    path = state_path(execution, session, host, origin)
    state = _read_state_or_empty(path, execution, session, host, origin)
    state = state or {"version": 2, "executionId": execution, "sessionId": session, "host": host, "origin": origin}
    delivery = state.get("delivery")
    if delivery is None:
        emitted_at = datetime.now(timezone.utc).isoformat(timespec="seconds")
        signal_id, signal = _signal(execution, session, host, emitted_at, node_id, origin, lane)
        delivery = {"executionId": execution, "sessionId": session, "signalId": signal_id, "signal": signal, "declaredNodeId": node_id, "delivered": False}
        if lane is not None:
            delivery["lane"] = lane
        state["delivery"] = delivery
        _write_state_bounded(path, state, durable=False)
    if delivery.get("delivered") is True:
        return
    signal = delivery.get("signal")
    signal_id = delivery.get("signalId")
    if not isinstance(signal, dict) or not isinstance(signal_id, str):
        raise ValueError("invalid delivery state")
    try:
        reply = request(f"{url}/v1/executions/{urllib.parse.quote(execution, safe='')}/signal", _token_bounded(token_file), "POST", {"signal": signal}, {"X-GraphHelm-Actor": signal["source"]["id"], "X-GraphHelm-Actor-Type": "agent", "X-GraphHelm-Actor-Session": session, "Idempotency-Key": signal_id}, timeout=5.0)
    except urllib.error.HTTPError as error:
        # The key is the same on every retry; a 409 means an earlier end committed it and its state
        # write did not land (abandoned past STATE_WRITE_BUDGET), as `_record_started` reads it.
        if error.code != 409:
            raise ValueError(f"the Runtime refused the end signal (HTTP {error.code})") from error
        reply = {"data": {"executionId": execution, "signalId": signal_id}}
    data = reply.get("data")
    if not isinstance(data, dict) or data.get("executionId") != execution or data.get("signalId") != signal_id:
        raise ValueError("signal acknowledgment did not match")
    delivery["delivered"] = True
    _write_state_bounded(path, state, durable=False)


def end(payload: dict, host: str, output_format: str = "native") -> bool:
    host = _host(host)
    if payload.get("hook_event_name") != "SessionEnd":
        raise ValueError("wrong hook event")
    # #1184: the binding and the lane are read from the payload before the lane file goes, so a
    # title- or team.json-bound lane ends under its own name like an env-bound one.
    bound = binding(payload)
    lane = _lane(payload)
    if bound is not None:
        stop_lane_waiter(payload, bound, host)
    _forget_lane(payload)
    if bound is None:
        return False
    execution, _, _, origin, _ = bound
    session = _session(payload)
    lock = _claim_lock_bounded(state_path(execution, session, host, origin))
    try:
        _end_impl(payload, host, bound, lane)
    finally:
        _release_lock(lock)
    return True


def inspect(host: str, session: str | None, output_format: str = "native") -> None:
    host = _host(host)
    configured_session = os.environ.get("GRAPHHELM_SESSION_ID")
    if session and not ID.fullmatch(session):
        raise ValueError("invalid session identity")
    if configured_session and session and configured_session != session:
        raise ValueError("session identity mismatch")
    if configured_session and not ID.fullmatch(configured_session):
        raise ValueError("invalid session identity")
    session = session or configured_session
    bound = binding()
    observed = []
    root = _state_root()
    paths, truncated, skipped = [], False, 0
    if bound and session:
        paths = [state_path(bound[0], session, host, bound[3])]
    elif root.is_dir():
        with os.scandir(root) as entries:
            batch = list(itertools.islice(entries, 129))
        truncated = len(batch) > 128
        paths = [Path(entry.path) for entry in batch[:128] if entry.name.endswith(".json") and not entry.is_symlink()]
    for path in paths:
        try:
            with path.open("rb") as source:
                raw = source.read(MAX_STATE + 1)
            if len(raw) > MAX_STATE:
                raise ValueError("state too large")
            value = json.loads(raw)
            if not isinstance(value, dict) or value.get("host") != host:
                raise ValueError("state host invalid")
            execution, saved_session, origin = (value.get(key) for key in ("executionId", "sessionId", "origin"))
            if not all(isinstance(item, str) and ID.fullmatch(item) for item in (execution, saved_session)) or not isinstance(origin, str) or len(origin) > 2048:
                raise ValueError("state identity invalid")
            if session and saved_session != session:
                continue
            if bound and (execution != bound[0] or origin != bound[3]):
                continue
            if path.name != state_path(execution, saved_session, host, origin).name:
                raise ValueError("state filename mismatch")
            value = _read_state(path, execution, saved_session, host, origin)
            if value is None:
                continue
            delivery = value.get("delivery", {})
            observed.append({"sessionId": saved_session, "executionId": execution, "runtimeOrigin": origin, "briefingCached": "briefing" in value, "deliveryDelivered": delivery.get("delivered") is True})
            if len(observed) == 32:
                truncated = True
                break
        except FileNotFoundError:
            continue
        except (OSError, ValueError, KeyError, RecursionError):
            skipped += 1
    report = {"phase": "inspect", "host": host, "sessionId": session, "configured": {"executionId": bound[0] if bound else None, "runtimeConfigured": bool(os.environ.get("GRAPHHELM_RUNTIME_URL")), "tokenConfigured": bool(os.environ.get("GRAPHHELM_TOKEN_FILE")), "declaredNodeId": bound[4] if bound else None, "keelContextOptIn": os.environ.get("GRAPHHELM_KEEL_CONTEXT") == "1"}, "localObservations": observed, "truncated": truncated, "skippedStateFiles": skipped, "activation": "unobserved"}
    if output_format == "portable":
        report["format"] = "graphhelm-portable-v1"
    print(json.dumps(report, separators=(",", ":")))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("phase", choices=("start", "end", "inspect", "wait", "touch", "helper"))
    parser.add_argument("--host", type=_host, required=True)
    parser.add_argument("--format", choices=("native", "portable"), default=None)
    parser.add_argument("--session-id")
    parser.add_argument("--events", type=Path)
    parser.add_argument("--heartbeat", type=Path)
    parser.add_argument("--cursor", type=int)
    parser.add_argument("--cwd")
    args = parser.parse_args()
    host = args.host
    output_format = args.format or ("native" if host in {"claude", "codex"} else "portable")
    if args.phase == "helper":
        return _helper_main()
    if args.phase in {"start", "end"}:
        _use_helper(host)
    try:
        if args.phase == "wait":
            if not args.session_id or args.events is None or args.heartbeat is None:
                raise ValueError("waiter binding missing")
            wait_for_lane(host, args.session_id, args.events, args.heartbeat, args.cursor, args.cwd)
        elif args.phase == "touch":
            payload = hook_input()
            bound = binding(payload)
            if bound is not None:
                touch_lane_heartbeat(payload, bound, host)
        elif args.phase == "inspect":
            inspect(host, args.session_id, output_format)
        else:
            payload = hook_input()
            if args.phase == "start":
                start(payload, host, output_format)
            else:
                delivered = end(payload, host, output_format)
                if output_format == "portable":
                    print(json.dumps({"format": "graphhelm-portable-v1", "phase": "end", "host": host, "sessionId": _session(payload), "delivery": "acknowledged" if delivered else "unobserved", "activation": "unobserved"}, separators=(",", ":")))
    except (OSError, ValueError, KeyError, RecursionError, _StateTimeout) as error:
        # `_StateTimeout` lands here too: the hook answers UNOBSERVED now, records nothing more for
        # the session, and `__main__` exits at once.
        print(f"graphhelm-hook: {args.phase} unobserved ({type(error).__name__})", file=sys.stderr)
        if args.phase == "inspect":
            print(json.dumps({"phase": "inspect", "configuration": "invalid", "activation": "unobserved"}))
            return 1
        if args.phase == "start":
            if output_format == "portable":
                print(json.dumps({"format": "graphhelm-portable-v1", "phase": "start", "host": host, "sessionId": None, "activation": "unobserved", "context": KEEL_CONTEXT + "\nGraphHelm briefing UNOBSERVED."}, separators=(",", ":")))
            else:
                print(json.dumps({"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": KEEL_CONTEXT + "\nGraphHelm briefing UNOBSERVED."}}))
        elif output_format == "portable":
            report = {"format": "graphhelm-portable-v1", "phase": args.phase, "host": host, "activation": "unobserved"}
            if args.phase == "end":
                # A portable end always says its delivery, as the normal end report does.
                report["delivery"] = "unobserved"
            print(json.dumps(report, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    code = main()
    if _answered_without_state or _watched_phase in {"start", "end"}:
        # The answer is out. Start and end never go through interpreter shutdown: it can wait on a
        # stuck file system, and once it starts the watchdog thread can no longer run. Flush what
        # was said and leave now.
        sys.stdout.flush()
        sys.stderr.flush()
        _probe_mark("normal")
        os._exit(code)
    raise SystemExit(code)
