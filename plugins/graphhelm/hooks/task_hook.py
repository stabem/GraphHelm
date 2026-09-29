"""Bounded Claude task lifecycle telemetry hooks."""

from __future__ import annotations

from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import sys
import time
import urllib.error
import urllib.parse

from session_hook import (
    ID,
    MAX_STATE,
    _claim_lock,
    _release_lock,
    _state_root,
    binding,
    hook_input,
    request,
    token_from_file,
)

PROTOCOL = "graphhelm-native-task-v1"
PHASES = {"TaskCreated": "created", "TaskCompleted": "completed"}
SECRET_ASSIGNMENT = re.compile(
    r"(?i)\b(?:password|passwd|token|secret|api[_-]?key|authorization)\b\s*[:=]\s*['\"]?[^\s'\";,]+"
)
SECRET_TOKEN = re.compile(
    r"(?i)\b(?:gh[pousr]_[a-z0-9]{20,}|sk-[a-z0-9_-]{20,}|sk_(?:live|test)_[a-z0-9]{12,}|xox[baprs]-[a-z0-9-]{20,}|akia[a-z0-9]{16})\b|\bbearer\s+[a-z0-9._~+/-]{12,}"
)


def _actor_id(host: str, parent: str) -> str:
    actor = f"{host}-session-{parent}"
    if len(actor) <= 128:
        return actor
    identity = f"{host}\0{parent}"
    return f"agent-session-{hashlib.sha256(identity.encode()).hexdigest()[:48]}"


def _safe_text(value: object, *, maximum: int, required: bool) -> str | None:
    if value is None and not required:
        return None
    if not isinstance(value, str) or not value.strip() or len(value) > maximum:
        raise ValueError("invalid task text")
    text = value.strip()
    if any(ord(character) < 32 or ord(character) == 127 for character in text):
        raise ValueError("invalid task text")
    if SECRET_ASSIGNMENT.search(text) or SECRET_TOKEN.search(text):
        raise ValueError("sensitive task text")
    return text


def _identity(payload: dict) -> tuple[str, str, str, str | None]:
    parent = payload.get("session_id")
    task_id = payload.get("task_id")
    if not isinstance(parent, str) or not ID.fullmatch(parent):
        raise ValueError("invalid parent session identity")
    if not isinstance(task_id, str) or not ID.fullmatch(task_id):
        raise ValueError("invalid native task identity")
    configured = os.environ.get("GRAPHHELM_SESSION_ID")
    if configured and (not ID.fullmatch(configured) or configured != parent):
        raise ValueError("session identity mismatch")
    title = _safe_text(payload.get("task_subject"), maximum=256, required=True)
    teammate = _safe_text(payload.get("teammate_name"), maximum=128, required=False)
    return parent, task_id, title or "", teammate


def _key(execution: str, origin: str, host: str, parent: str, task_id: str, phase: str) -> str:
    identity = f"{PROTOCOL}\0{execution}\0{origin}\0{host}\0{parent}\0{task_id}\0{phase}"
    return f"native-task-{phase}-v1-{hashlib.sha256(identity.encode()).hexdigest()[:32]}"


def _state_path(execution: str, origin: str, host: str, parent: str, task_id: str, phase: str) -> Path:
    identity = f"{PROTOCOL}\0{execution}\0{origin}\0{host}\0{parent}\0{task_id}\0{phase}"
    return _state_root() / f"native-task-{hashlib.sha256(identity.encode()).hexdigest()}.json"


def _description(execution: str, host: str, parent: str, task_id: str,
                 title: str, teammate: str | None, phase: str) -> str:
    details = {
        "protocol": PROTOCOL,
        "executionId": execution,
        "host": host,
        "parentSessionId": parent,
        "nativeTaskId": task_id,
        "taskSubject": title,
        "teammateName": teammate,
        "phase": phase,
    }
    return json.dumps(details, separators=(",", ":"), sort_keys=True)


def _signal(execution: str, origin: str, host: str, parent: str, task_id: str,
            title: str, teammate: str | None, phase: str, emitted_at: str) -> tuple[str, dict]:
    signal_id = _key(execution, origin, host, parent, task_id, phase)
    actor = _actor_id(host, parent)
    signal = {
        "id": signal_id,
        "source": {"type": "tool", "id": actor},
        "type": f"agent_task_{phase}",
        "severity": "low",
        "description": _description(execution, host, parent, task_id, title, teammate, phase),
        "evidence": [execution],
        "emittedAt": emitted_at,
    }
    return signal_id, signal


def _write(path: Path, value: dict) -> None:
    raw = json.dumps(value, separators=(",", ":"), sort_keys=True).encode()
    if len(raw) > MAX_STATE:
        raise ValueError("task state too large")
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp-{os.getpid()}")
    try:
        with temporary.open("wb") as output:
            output.write(raw)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
    finally:
        try:
            temporary.unlink()
        except FileNotFoundError:
            pass


def _read(path: Path, identity: tuple[str, str, str, str, str, str, str | None, str]) -> dict | None:
    try:
        raw = path.read_bytes()
    except FileNotFoundError:
        return None
    if len(raw) > MAX_STATE:
        raise ValueError("task state too large")
    value = json.loads(raw)
    keys = ("executionId", "runtimeOrigin", "host", "parentSessionId", "nativeTaskId", "taskSubject", "teammateName", "phase")
    if not isinstance(value, dict) or value.get("version") != 1 or tuple(value.get(key) for key in keys) != identity:
        raise ValueError("invalid task state identity")
    if type(value.get("delivered")) is not bool or not isinstance(value.get("signal"), dict):
        raise ValueError("invalid task delivery")
    emitted_at = value["signal"].get("emittedAt")
    if not isinstance(emitted_at, str) or len(emitted_at) > 40:
        raise ValueError("invalid task timestamp")
    try:
        if datetime.fromisoformat(emitted_at).tzinfo is None:
            raise ValueError("invalid task timestamp")
    except ValueError as error:
        raise ValueError("invalid task timestamp") from error
    signal_id, expected = _signal(
        identity[0], identity[1], identity[2], identity[3], identity[4], identity[5], identity[6], identity[7], emitted_at,
    )
    if value.get("signalId") != signal_id or value["signal"] != expected:
        raise ValueError("invalid task delivery")
    return value


def _reconcile(url: str, token: str, execution: str, signal: dict) -> bool:
    signal_id = signal["id"]
    actor_id = signal["source"]["id"]
    after = 0
    for _ in range(16):
        try:
            data = request(
                f"{url}/v1/executions/{urllib.parse.quote(execution, safe='')}/events?after={after}&limit=128",
                token, "GET", timeout=1.0,
            ).get("data")
        except (OSError, ValueError, KeyError):
            return False
        if not isinstance(data, dict) or not isinstance(data.get("events"), list):
            return False
        events = data["events"]
        head = data.get("head")
        if type(head) is not int or head < after or len(events) > 128:
            return False
        for event in events:
            if not isinstance(event, dict) or type(event.get("sequence")) is not int:
                return False
            kind = event.get("kind")
            record = kind.get("data") if isinstance(kind, dict) and kind.get("type") == "signal_recorded" else None
            actor = event.get("actor")
            if (isinstance(record, dict) and record.get("signalId") == signal_id
                    and record.get("executionId") == execution
                    and record.get("sourceId") == actor_id
                    and record.get("kind") == signal["type"]
                    and isinstance(actor, dict) and actor.get("type") == "agent" and actor.get("id") == actor_id):
                return True
            after = max(after, event["sequence"])
        if after >= head or not events:
            return False
    return False


def _run(payload: dict, event: str) -> None:
    if payload.get("hook_event_name") != event:
        raise ValueError("wrong hook event")
    parent, task_id, title, teammate = _identity(payload)
    bound = binding()
    if bound is None:
        return
    execution, token_file, url, origin, _node = bound
    phase = PHASES[event]
    host = "claude"
    path = _state_path(execution, origin, host, parent, task_id, phase)
    identity = (execution, origin, host, parent, task_id, title, teammate, phase)
    deadline = time.monotonic() + 6.0
    while True:
        try:
            lock = _claim_lock(path)
            break
        except ValueError as error:
            if str(error) != "hook state busy" or time.monotonic() >= deadline:
                raise
            time.sleep(min(0.025, max(0.0, deadline - time.monotonic())))
    try:
        state = _read(path, identity)
        if state is None:
            emitted_at = datetime.now(timezone.utc).isoformat(timespec="seconds")
            signal_id, signal = _signal(execution, origin, host, parent, task_id, title, teammate, phase, emitted_at)
            state = {"version": 1, "executionId": execution, "runtimeOrigin": origin, "host": host,
                     "parentSessionId": parent, "nativeTaskId": task_id, "taskSubject": title,
                     "teammateName": teammate, "phase": phase, "signalId": signal_id,
                     "signal": signal, "delivered": False}
            _write(path, state)
        if state["delivered"]:
            return
        signal = state["signal"]
        signal_id = state["signalId"]
        try:
            reply = request(
                f"{url}/v1/executions/{urllib.parse.quote(execution, safe='')}/signal",
                token_from_file(token_file), "POST", {"signal": signal},
                {"X-GraphHelm-Actor": signal["source"]["id"], "X-GraphHelm-Actor-Type": "agent",
                 "X-GraphHelm-Actor-Session": parent, "Idempotency-Key": signal_id}, timeout=5.0,
            )
            data = reply.get("data")
            if not isinstance(data, dict) or data.get("executionId") != execution or data.get("signalId") != signal_id:
                raise ValueError("signal acknowledgment did not match")
        except urllib.error.HTTPError as error:
            if 400 <= error.code < 500:
                raise
            if not _reconcile(url, token_from_file(token_file), execution, signal):
                raise
        except (OSError, ValueError, KeyError):
            if not _reconcile(url, token_from_file(token_file), execution, signal):
                raise
        state["delivered"] = True
        _write(path, state)
    finally:
        _release_lock(lock)


def main() -> int:
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument("phase", choices=("created", "completed"))
    args = parser.parse_args()
    event = "TaskCreated" if args.phase == "created" else "TaskCompleted"
    try:
        _run(hook_input(), event)
    except (OSError, ValueError, KeyError, RecursionError) as error:
        print(f"graphhelm-task-hook: {args.phase} unobserved ({type(error).__name__})", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
