"""Explicit, event-backed membership and mailbox for one bound execution.

The trusted MCP launch fixes the Runtime and execution. Codex supplies its native session
identity per call; an environment-bound host must pin GRAPHHELM_SESSION_ID. A recorded
message is not a delivery receipt. Only a separate call by the addressed session records
an acknowledgement. Neither act proves model comprehension or output acceptance.
"""

from __future__ import annotations

from datetime import datetime, timezone
import argparse
import hashlib
import json
import os
import sys

from session_hook import ID, PORTABLE_HOST
from task_handoff import _binding, _events, _post_signal, _sealed_signal, _session, _signal_event, _signal_record

PROTOCOL = "graphhelm-run-team-v1"
JOIN = "run_team_joined"
REPORT = "run_team_reported"
MESSAGE = "run_team_message"
ACK = "run_team_acknowledged"
KINDS = frozenset({JOIN, REPORT, MESSAGE, ACK})
MAX_TEXT = 2000
MAX_INBOX = 12
STATES = frozenset({"working", "waiting", "blocked", "completed"})


def _id(value: object, label: str) -> str:
    if not isinstance(value, str) or not ID.fullmatch(value):
        raise ValueError(f"invalid {label}")
    return value


def _actor(host: str, session: str) -> str:
    host = _id(host, "host")
    if not PORTABLE_HOST.fullmatch(host) or host not in {"codex", "claude"}:
        raise ValueError("invalid host")
    session = _id(session, "session")
    name = f"{host}-session-{session}"
    if len(name) <= 128:
        return name
    return "agent-session-" + hashlib.sha256(f"{host}\0{session}".encode()).hexdigest()[:48]


def _key(execution: str, origin: str, actor: str, kind: str, caller_id: str) -> str:
    material = "\0".join((PROTOCOL, execution, origin, actor, kind, caller_id))
    return "run-team-" + hashlib.sha256(material.encode()).hexdigest()[:48]


def _context(host: str, session: str) -> tuple[str, str, str, str, str]:
    session = _session(session)
    execution, token_file, url, origin, _ = _binding()
    from session_hook import token_from_file
    return execution, token_from_file(token_file), url, origin, _actor(host, session)


def _opened(url: str, token: str, execution: str, event: dict, kind: str) -> tuple[dict, dict]:
    record = _signal_record(event)
    if record.get("kind") != kind:
        raise ValueError("recorded team signal has the wrong kind")
    signal = _sealed_signal(url, token, execution, event, KINDS)
    if signal.get("type") != kind:
        raise ValueError("sealed team signal has the wrong kind")
    try:
        details = json.loads(signal.get("description", ""))
    except (TypeError, json.JSONDecodeError) as error:
        raise ValueError("team signal description is invalid") from error
    if not isinstance(details, dict) or details.get("protocol") != PROTOCOL or details.get("executionId") != execution:
        raise ValueError("team signal belongs to another protocol or execution")
    return signal, details


def _record(url: str, token: str, execution: str, session: str, events: list[dict],
            signal: dict, expected: dict) -> str:
    signal_id = signal["id"]
    existing = _signal_event(events, signal_id)
    if existing is None:
        _post_signal(url, token, execution, signal, signal_id, session)
        existing = _signal_event(_events(url, token, execution), signal_id)
    if existing is None:
        raise ValueError("team write is unobserved")
    sealed, details = _opened(url, token, execution, existing, signal["type"])
    if (sealed.get("source") != signal["source"] or sealed.get("to") != signal.get("to")
            or sealed.get("replyTo") != signal.get("replyTo") or details != expected):
        raise ValueError("recorded team signal conflicts with this request")
    return signal_id


def _signal(execution: str, origin: str, actor: str, kind: str, caller_id: str,
            details: dict, to: str | None = None, reply_to: str | None = None) -> dict:
    signal = {"id": _key(execution, origin, actor, kind, caller_id),
              "source": {"type": "tool", "id": actor}, "type": kind, "severity": "low",
              "description": json.dumps(details, sort_keys=True, separators=(",", ":")),
              "evidence": [execution], "emittedAt": datetime.now(timezone.utc).isoformat(timespec="seconds")}
    if to is not None:
        signal["to"] = to
    if reply_to is not None:
        signal["replyTo"] = reply_to
    return signal


def _member(events: list[dict], url: str, token: str, execution: str, actor: str) -> dict:
    for event in events:
        record = _signal_record(event)
        if record.get("kind") != JOIN or record.get("sourceId") != actor:
            continue
        signal, details = _opened(url, token, execution, event, JOIN)
        if (signal.get("source", {}).get("id") == actor
                and details.get("actorId") == actor
                and details.get("host") in {"codex", "claude"}
                and isinstance(details.get("sessionId"), str)
                and _actor(details["host"], details["sessionId"]) == actor):
            return details
    raise ValueError("session is not attached to this execution")


def join(host: str, session: str) -> dict:
    execution, token, url, origin, actor = _context(host, session)
    events = _events(url, token, execution)
    details = {"protocol": PROTOCOL, "executionId": execution, "actorId": actor,
               "host": host, "sessionId": session}
    signal = _signal(execution, origin, actor, JOIN, "join", details)
    signal_id = _record(url, token, execution, session, events, signal, details)
    return {"executionId": execution, "actorId": actor, "joinId": signal_id,
            "membership": "recorded", "activation": "unobserved"}


def report(host: str, session: str, update_id: str, task: str, activity: str, state: str) -> dict:
    update_id = _id(update_id, "update id")
    if state not in STATES:
        raise ValueError("invalid reported state")
    if not isinstance(task, str) or not isinstance(activity, str) or not task.strip() or not activity.strip():
        raise ValueError("task and activity are required")
    if len(task) > 500 or len(activity) > MAX_TEXT:
        raise ValueError("team report too long")
    execution, token, url, origin, actor = _context(host, session)
    events = _events(url, token, execution)
    _member(events, url, token, execution, actor)
    details = {"protocol": PROTOCOL, "executionId": execution, "actorId": actor,
               "task": task, "activity": activity, "state": state}
    signal = _signal(execution, origin, actor, REPORT, update_id, details)
    return {"executionId": execution, "actorId": actor,
            "reportId": _record(url, token, execution, session, events, signal, details),
            "state": "recorded", "acceptance": "unobserved"}


def _message(events: list[dict], url: str, token: str, execution: str, message_id: str) -> tuple[dict, dict]:
    event = _signal_event(events, _id(message_id, "message id"))
    if event is None:
        raise ValueError("message is not recorded in this execution")
    signal, details = _opened(url, token, execution, event, MESSAGE)
    if (details.get("messageId") != message_id or details.get("sender") != signal["source"]["id"]
            or details.get("recipient") != signal.get("to")
            or not isinstance(details.get("text"), str)):
        raise ValueError("message metadata disagrees with the sealed signal")
    return signal, details


def send(host: str, session: str, message_id: str, text: str,
         to: str | None = None, reply_to: str | None = None) -> dict:
    message_id = _id(message_id, "caller message id")
    if not isinstance(text, str) or not text.strip() or len(text) > MAX_TEXT:
        raise ValueError("message text is empty or too long")
    if to is not None:
        to = _id(to, "recipient")
    if reply_to is not None:
        reply_to = _id(reply_to, "reply id")
    execution, token, url, origin, actor = _context(host, session)
    events = _events(url, token, execution)
    _member(events, url, token, execution, actor)
    if to is not None:
        _member(events, url, token, execution, to)
        if to == actor:
            raise ValueError("self-addressed team messages are not supported")
    if reply_to is not None:
        previous, _ = _message(events, url, token, execution, reply_to)
        if previous.get("to") != actor or to != previous["source"]["id"]:
            raise ValueError("reply must return an addressed message to its sender")
    signal_id = _key(execution, origin, actor, MESSAGE, message_id)
    details = {"protocol": PROTOCOL, "executionId": execution, "messageId": signal_id,
               "sender": actor, "recipient": to, "text": text}
    signal = _signal(execution, origin, actor, MESSAGE, message_id, details, to, reply_to)
    _record(url, token, execution, session, events, signal, details)
    return {"executionId": execution, "messageId": signal_id, "state": "recorded",
            "recipientReceipt": "not_observed"}


def inbox(host: str, session: str, after: int = 0) -> dict:
    if type(after) is not int or after < 0:
        raise ValueError("invalid inbox cursor")
    execution, token, url, _, actor = _context(host, session)
    events = _events(url, token, execution)
    _member(events, url, token, execution, actor)
    acknowledged = set()
    for event in events:
        record = _signal_record(event)
        if record.get("kind") == ACK and record.get("sourceId") == actor:
            receipt, details = _opened(url, token, execution, event, ACK)
            message_id = receipt.get("replyTo")
            if not isinstance(message_id, str):
                raise ValueError("recorded acknowledgement has no message")
            message, _ = _message(events, url, token, execution, message_id)
            if (message.get("to") != actor or receipt.get("to") != message["source"]["id"]
                    or details.get("messageId") != message_id or details.get("recipient") != actor
                    or details.get("sender") != message["source"]["id"]):
                raise ValueError("recorded acknowledgement does not match the addressed message")
            acknowledged.add(message_id)
    pending = []
    truncated = False
    for event in events:
        if type(event.get("sequence")) is not int:
            continue
        record = _signal_record(event)
        if record.get("kind") != MESSAGE:
            continue
        message, details = _opened(url, token, execution, event, MESSAGE)
        if message.get("to") not in {None, actor} or message["source"]["id"] == actor:
            continue
        if message.get("to") == actor and message["id"] in acknowledged:
            continue
        if message.get("to") is None and event["sequence"] <= after:
            continue
        if len(pending) >= MAX_INBOX:
            truncated = True
            break
        pending.append({"messageId": message["id"], "sender": details["sender"],
                        "to": message.get("to"), "replyTo": message.get("replyTo"),
                        "text": details["text"], "sequence": event["sequence"]})
    return {"executionId": execution, "recipient": actor, "messages": pending,
            "truncated": truncated, "receipt": "not_recorded_by_read"}


def acknowledge(host: str, session: str, message_id: str) -> dict:
    execution, token, url, origin, actor = _context(host, session)
    events = _events(url, token, execution)
    _member(events, url, token, execution, actor)
    message, _ = _message(events, url, token, execution, message_id)
    if message.get("to") != actor:
        raise ValueError("only the addressed session can acknowledge this message")
    sender = message["source"]["id"]
    details = {"protocol": PROTOCOL, "executionId": execution,
               "messageId": message_id, "recipient": actor, "sender": sender}
    receipt = _signal(execution, origin, actor, ACK, message_id, details, sender, message_id)
    receipt_id = _record(url, token, execution, session, events, receipt, details)
    return {"executionId": execution, "messageId": message_id, "receiptId": receipt_id,
            "receipt": "recorded_by_recipient", "comprehension": "unobserved"}


def main() -> int:
    parser = argparse.ArgumentParser(description="Explicit run-team mailbox for a bound native session")
    parser.add_argument("action", choices=("join", "report", "send", "inbox", "acknowledge"))
    parser.add_argument("--host", choices=("codex", "claude"), required=True)
    parser.add_argument("--session-id")
    parser.add_argument("--update-id")
    parser.add_argument("--task")
    parser.add_argument("--activity")
    parser.add_argument("--state")
    parser.add_argument("--message-id")
    parser.add_argument("--text")
    parser.add_argument("--to")
    parser.add_argument("--reply-to")
    parser.add_argument("--after", type=int, default=0)
    args = parser.parse_args()
    try:
        session = _id(args.session_id or os.environ.get("GRAPHHELM_SESSION_ID"), "session")
        if args.action == "join":
            value = join(args.host, session)
        elif args.action == "report":
            value = report(args.host, session, args.update_id, args.task, args.activity, args.state)
        elif args.action == "send":
            value = send(args.host, session, args.message_id, args.text, args.to, args.reply_to)
        elif args.action == "inbox":
            value = inbox(args.host, session, args.after)
        else:
            value = acknowledge(args.host, session, args.message_id)
        print(json.dumps(value, separators=(",", ":")))
        return 0
    except (OSError, ValueError, KeyError, TypeError, json.JSONDecodeError):
        print("run-team operation failed; result is unobserved", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
