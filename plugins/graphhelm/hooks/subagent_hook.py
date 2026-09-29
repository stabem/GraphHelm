"""Provider-neutral SubagentStart and SubagentStop telemetry hooks."""

from __future__ import annotations

from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import sys
import urllib.error
import urllib.parse

from session_hook import (
    ID,
    PORTABLE_HOST,
    MAX_STATE,
    _claim_lock,
    _release_lock,
    _state_root,
    binding,
    hook_input,
    request,
    token_from_file,
)

PROTOCOL = "graphhelm-subagent-v1"
PHASES = {"SubagentStart": "started", "SubagentStop": "stopped"}


def _host(value: str) -> str:
    if not isinstance(value, str) or not PORTABLE_HOST.fullmatch(value):
        raise ValueError("invalid host identity")
    return value


def _identity(payload: dict) -> tuple[str, str, str]:
    parent = payload.get("session_id")
    child = payload.get("agent_id")
    agent_type = payload.get("agent_type")
    if not isinstance(parent, str) or not ID.fullmatch(parent):
        raise ValueError("invalid parent session identity")
    if not isinstance(child, str) or not ID.fullmatch(child):
        raise ValueError("invalid child agent identity")
    if not isinstance(agent_type, str) or not ID.fullmatch(agent_type):
        raise ValueError("invalid agent type")
    configured = os.environ.get("GRAPHHELM_SESSION_ID")
    if configured and (not ID.fullmatch(configured) or configured != parent):
        raise ValueError("session identity mismatch")
    return parent, child, agent_type


def _key(execution: str, origin: str, host: str, parent: str, child: str,
         agent_type: str, phase: str) -> str:
    digest = hashlib.sha256(
        f"{PROTOCOL}\0{execution}\0{origin}\0{host}\0{parent}\0{child}\0{agent_type}\0{phase}".encode()
    ).hexdigest()[:32]
    return f"subagent-{phase}-v1-{digest}"


def _state_path(execution: str, origin: str, host: str, parent: str, child: str,
                agent_type: str, phase: str) -> Path:
    digest = hashlib.sha256(
        f"{PROTOCOL}\0{execution}\0{origin}\0{host}\0{parent}\0{child}\0{agent_type}\0{phase}".encode()
    ).hexdigest()
    return _state_root() / f"subagent-{digest}.json"


def _write(path: Path, value: dict) -> None:
    raw = json.dumps(value, separators=(",", ":"), sort_keys=True).encode()
    if len(raw) > MAX_STATE:
        raise ValueError("subagent state too large")
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


def _read(path: Path, identity: tuple[str, str, str, str, str, str, str]) -> dict | None:
    try:
        raw = path.read_bytes()
    except FileNotFoundError:
        return None
    if len(raw) > MAX_STATE:
        raise ValueError("subagent state too large")
    value = json.loads(raw)
    if not isinstance(value, dict) or value.get("version") != 1:
        raise ValueError("invalid subagent state")
    keys = ("executionId", "runtimeOrigin", "host", "parentSessionId", "childAgentId", "agentType", "phase")
    if tuple(value.get(key) for key in keys) != identity:
        raise ValueError("subagent state identity mismatch")
    if type(value.get("delivered")) is not bool or not isinstance(value.get("signal"), dict):
        raise ValueError("invalid subagent delivery")
    node = value.get("declaredNodeId")
    if node is not None and (not isinstance(node, str) or not ID.fullmatch(node)):
        raise ValueError("invalid subagent node")
    signal = value["signal"]
    emitted_at = signal.get("emittedAt")
    if not isinstance(emitted_at, str) or len(emitted_at) > 40:
        raise ValueError("invalid subagent timestamp")
    try:
        if datetime.fromisoformat(emitted_at).tzinfo is None:
            raise ValueError("invalid subagent timestamp")
    except ValueError as error:
        raise ValueError("invalid subagent timestamp") from error
    signal_id, expected = _signal(
        identity[0], identity[1], identity[2], identity[3], identity[4], identity[5], identity[6], node, emitted_at,
    )
    if value.get("signalId") != signal_id or signal != expected:
        raise ValueError("invalid subagent delivery")
    return value


def _description(execution: str, host: str, parent: str, child: str, agent_type: str,
                 phase: str, node: str | None) -> str:
    details = {
        "protocol": PROTOCOL,
        "executionId": execution,
        "host": host,
        "parentSessionId": parent,
        "childAgentId": child,
        "agentType": agent_type,
        "phase": phase,
        "declaredNodeId": node,
    }
    return json.dumps(details, separators=(",", ":"), sort_keys=True)


def _actor_id(host: str, parent: str) -> str:
    actor = f"{host}-session-{parent}"
    if len(actor) <= 128:
        return actor
    identity = f"{host}\0{parent}"
    return f"agent-session-{hashlib.sha256(identity.encode()).hexdigest()[:48]}"


def _signal(execution: str, origin: str, host: str, parent: str, child: str,
            agent_type: str, phase: str, node: str | None, emitted_at: str) -> tuple[str, dict]:
    signal_id = _key(execution, origin, host, parent, child, agent_type, phase)
    actor = _actor_id(host, parent)
    signal = {
        "id": signal_id,
        "source": {"type": "tool", "id": actor},
        "type": f"agent_subagent_{phase}",
        "severity": "low",
        "description": _description(execution, host, parent, child, agent_type, phase, node),
        "evidence": [execution],
        "emittedAt": emitted_at,
    }
    return signal_id, signal


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


def _run(payload: dict, host: str, event: str) -> None:
    if payload.get("hook_event_name") != event:
        raise ValueError("wrong hook event")
    parent, child, agent_type = _identity(payload)
    bound = binding()
    if bound is None:
        return
    execution, token_file, url, origin, node = bound
    phase = PHASES[event]
    path = _state_path(execution, origin, host, parent, child, agent_type, phase)
    identity = (execution, origin, host, parent, child, agent_type, phase)
    state = _read(path, identity)
    if state is None:
        emitted_at = datetime.now(timezone.utc).isoformat(timespec="seconds")
        signal_id, signal = _signal(execution, origin, host, parent, child, agent_type, phase, node, emitted_at)
        state = {"version": 1, "executionId": execution, "runtimeOrigin": origin, "host": host,
                 "parentSessionId": parent, "childAgentId": child, "agentType": agent_type,
                 "phase": phase, "declaredNodeId": node, "signalId": signal_id, "signal": signal, "delivered": False}
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


def main() -> int:
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument("phase", choices=("start", "stop"))
    parser.add_argument("--host", type=_host, required=True)
    args = parser.parse_args()
    event = "SubagentStart" if args.phase == "start" else "SubagentStop"
    try:
        _run(hook_input(), args.host, event)
    except (OSError, ValueError, KeyError, RecursionError) as error:
        print(f"graphhelm-subagent-hook: {args.phase} unobserved ({type(error).__name__})", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
