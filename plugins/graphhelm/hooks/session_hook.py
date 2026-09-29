"""Bounded, local-first SessionStart and SessionEnd hooks."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import itertools
import json
import os
from pathlib import Path
import re
import sys
import tempfile
import urllib.parse
import urllib.request

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
                "passed. A session ending does not prove task completion.")


def hook_input() -> dict:
    raw = sys.stdin.buffer.read(MAX_INPUT + 1)
    if len(raw) > MAX_INPUT:
        raise ValueError("hook input too large")
    value = json.loads(raw)
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


def _host(value: str) -> str:
    if not isinstance(value, str) or not PORTABLE_HOST.fullmatch(value):
        raise ValueError("invalid host identity")
    return value


def binding() -> tuple[str, str, str, str, str | None] | None:
    execution = os.environ.get("GRAPHHELM_EXECUTION_ID", "")
    token_file = os.environ.get("GRAPHHELM_TOKEN_FILE", "")
    url = os.environ.get("GRAPHHELM_RUNTIME_URL", "http://127.0.0.1:8791").rstrip("/")
    node_id = os.environ.get("GRAPHHELM_NODE_ID")
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


def token_from_file(path: str) -> str:
    with open(path, "rb") as source:
        raw = source.read(4097)
    if not raw or len(raw) > 4096:
        raise ValueError("invalid token file")
    token = raw.decode("utf-8").strip()
    if not token or "\n" in token or "\r" in token:
        raise ValueError("invalid token file")
    return token


def request(url: str, token: str, method: str, body: dict | None = None, headers: dict | None = None, timeout: float = 1.0) -> dict:
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
        raw = response.read(MAX_REPLY + 1)
    if len(raw) > MAX_REPLY:
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


def _write_state(path: Path, value: dict) -> None:
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
            os.fsync(output.fileno())
        if os.name != "nt":
            temporary.chmod(0o600)
        os.replace(temporary, path)
    finally:
        try:
            temporary.unlink()
        except FileNotFoundError:
            pass


def _read_state(path: Path, execution: str, session: str, host: str, origin: str) -> dict | None:
    try:
        with path.open("rb") as source:
            raw = source.read(MAX_STATE + 1)
    except FileNotFoundError:
        return None
    if len(raw) > MAX_STATE:
        raise ValueError("hook state too large")
    value = json.loads(raw)
    if not isinstance(value, dict) or value.get("version") != 2:
        raise ValueError("invalid hook state version")
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
    lock.close()


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


def _start_impl(payload: dict, host: str, output_format: str = "native") -> None:
    if payload.get("hook_event_name") != "SessionStart":
        raise ValueError("wrong hook event")
    session = _session(payload)
    bound = binding()
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
    state = _read_state(path, execution, session, host, origin)
    source = payload.get("source") or payload.get("session_start_source") or "startup"
    digest = state.get("briefing") if source in {"compact", "clear"} and state else None
    if source == "compact" and not isinstance(digest, dict):
        raise ValueError("compact briefing cache unavailable")
    freshness = "CACHED" if isinstance(digest, dict) else "FRESH"
    if not isinstance(digest, dict):
        reply = request(f"{url}/v1/executions/{urllib.parse.quote(execution, safe='')}/briefing", token_from_file(token_file), "GET", timeout=BRIEFING_TIMEOUT)
        data = reply.get("data")
        if not isinstance(data, dict):
            raise ValueError("invalid briefing")
        digest = _briefing_digest(data)
        state = state or {"version": 2, "executionId": execution, "sessionId": session, "host": host, "origin": origin}
        state["briefing"] = digest
        _write_state(path, state)
    context = KEEL_CONTEXT + "\n" + _context(digest, freshness, execution, session, node_id)
    if len(context) > MAX_CONTEXT:
        raise ValueError("hook context too large")
    if output_format == "portable":
        print(json.dumps({"format": "graphhelm-portable-v1", "phase": "start", "host": host, "sessionId": session, "activation": "unobserved", "context": context}, separators=(",", ":")))
    else:
        print(json.dumps({"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": context}}))


def start(payload: dict, host: str, output_format: str = "native") -> None:
    host = _host(host)
    bound = binding()
    if bound is None:
        _start_impl(payload, host, output_format)
        return
    execution, _, _, origin, _ = bound
    lock = _claim_lock(state_path(execution, _session(payload), host, origin))
    try:
        _start_impl(payload, host, output_format)
    finally:
        _release_lock(lock)


def _signal(execution: str, session: str, host: str, emitted_at: str, node_id: str | None, origin: str) -> tuple[str, dict]:
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
    key, expected = _signal(execution, session, host, timestamp, node, origin)
    if value.get("signalId") != key or signal != expected:
        raise ValueError("invalid stored signal")


def _end_impl(payload: dict, host: str) -> None:
    if payload.get("hook_event_name") != "SessionEnd":
        raise ValueError("wrong hook event")
    bound = binding()
    if bound is None:
        return
    execution, token_file, url, origin, node_id = bound
    session = _session(payload)
    path = state_path(execution, session, host, origin)
    state = _read_state(path, execution, session, host, origin)
    if state is None and path.exists():
        raise ValueError("invalid hook state")
    state = state or {"version": 2, "executionId": execution, "sessionId": session, "host": host, "origin": origin}
    delivery = state.get("delivery")
    if delivery is None:
        emitted_at = datetime.now(timezone.utc).isoformat(timespec="seconds")
        signal_id, signal = _signal(execution, session, host, emitted_at, node_id, origin)
        delivery = {"executionId": execution, "sessionId": session, "signalId": signal_id, "signal": signal, "declaredNodeId": node_id, "delivered": False}
        state["delivery"] = delivery
        _write_state(path, state)
    if delivery.get("delivered") is True:
        return
    signal = delivery.get("signal")
    signal_id = delivery.get("signalId")
    if not isinstance(signal, dict) or not isinstance(signal_id, str):
        raise ValueError("invalid delivery state")
    reply = request(f"{url}/v1/executions/{urllib.parse.quote(execution, safe='')}/signal", token_from_file(token_file), "POST", {"signal": signal}, {"X-GraphHelm-Actor": signal["source"]["id"], "X-GraphHelm-Actor-Type": "agent", "X-GraphHelm-Actor-Session": session, "Idempotency-Key": signal_id}, timeout=2.5)
    data = reply.get("data")
    if not isinstance(data, dict) or data.get("executionId") != execution or data.get("signalId") != signal_id:
        raise ValueError("signal acknowledgment did not match")
    delivery["delivered"] = True
    _write_state(path, state)


def end(payload: dict, host: str, output_format: str = "native") -> bool:
    host = _host(host)
    if payload.get("hook_event_name") != "SessionEnd":
        raise ValueError("wrong hook event")
    bound = binding()
    if bound is None:
        return False
    execution, _, _, origin, _ = bound
    session = _session(payload)
    lock = _claim_lock(state_path(execution, session, host, origin))
    try:
        _end_impl(payload, host)
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
    parser.add_argument("phase", choices=("start", "end", "inspect"))
    parser.add_argument("--host", type=_host, required=True)
    parser.add_argument("--format", choices=("native", "portable"), default=None)
    parser.add_argument("--session-id")
    args = parser.parse_args()
    host = args.host
    output_format = args.format or ("native" if host in {"claude", "codex"} else "portable")
    try:
        if args.phase == "inspect":
            inspect(host, args.session_id, output_format)
        else:
            payload = hook_input()
            if args.phase == "start":
                start(payload, host, output_format)
            else:
                delivered = end(payload, host, output_format)
                if output_format == "portable":
                    print(json.dumps({"format": "graphhelm-portable-v1", "phase": "end", "host": host, "sessionId": _session(payload), "delivery": "acknowledged" if delivered else "unobserved", "activation": "unobserved"}, separators=(",", ":")))
    except (OSError, ValueError, KeyError, RecursionError) as error:
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
            print(json.dumps({"format": "graphhelm-portable-v1", "phase": args.phase, "host": host, "activation": "unobserved"}, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
