"""Remind a bound lane once about confirmed unanswered addressed operator notes.

Read-only: a lane acknowledges handling a note by recording its own operator_note
with replyTo. Re-entry and unreadable state allow Stop without acknowledging notes.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys
from urllib.parse import quote

from session_hook import binding, hook_input, request, token_from_file, ID


def pending() -> bool:
    bound = binding()
    if bound is None:
        return False
    execution, token_file, url, _origin, _node = bound
    lane = os.environ.get("GRAPHHELM_ACTOR", "")
    if not ID.fullmatch(lane):
        raise ValueError("lane identity missing")
    token = token_from_file(token_file)
    base = f"{url}/v1/executions/{quote(execution, safe='')}"
    head = request(f"{base}/events?after=0&limit=1", token, "GET")["data"]["head"]
    if type(head) is not int or head < 0:
        raise ValueError("invalid event head")
    # Bound the window, not the execution's lifetime. A reply follows its note,
    # so replies to notes inside this window cannot precede the window.
    after = max(0, head - 4096)
    unanswered = set()
    # Size bound; the parent process imposes the wall-time bound even if a server
    # trickles bytes. request's timeout alone only bounds socket inactivity.
    for _ in range(8):
        data = request(f"{base}/events?after={after}&limit=512", token, "GET", max_reply=1024 * 1024)["data"]
        page, head = data["events"], data["head"]
        if not isinstance(page, list) or type(head) is not int or head < after or len(page) > 512:
            raise ValueError("invalid event page")
        if not page and head > after:
            raise ValueError("incomplete event page")
        for event in page:
            sequence = event["sequence"]
            if type(sequence) is not int or sequence <= after:
                raise ValueError("event cursor did not advance")
            after = sequence
            kind = event.get("kind", {})
            record = kind.get("data", {})
            if kind.get("type") != "signal_recorded" or record.get("kind") != "operator_note":
                continue
            if record.get("executionId") != execution:
                raise ValueError("wrong execution")
            # The Governor exposes only opaque routing hints. Legacy notes have no
            # hints and remain in the full inbox; never open N sealed envelopes here.
            signal_id = record["signalId"]
            to, reply_to = record.get("to"), record.get("replyTo")
            if not isinstance(signal_id, str) or not ID.fullmatch(signal_id):
                raise ValueError("invalid signal identity")
            if any(value is not None and (not isinstance(value, str) or not ID.fullmatch(value))
                   for value in (to, reply_to)):
                raise ValueError("invalid note routing")
            actor = event.get("actor", {})
            if actor.get("type") == "agent" and actor.get("id") == lane:
                unanswered.discard(reply_to)
            if to == lane:
                unanswered.add(signal_id)
        if after >= head:
            return bool(unanswered)
    raise ValueError("note scan exceeded bound")


def main() -> int:
    if sys.argv[1:] == ["--scan"]:
        try:
            return 1 if pending() else 0
        except Exception:
            return 2
    try:
        payload = hook_input()
    except (ValueError, OSError):
        return 0
    # The host sets this on the continuation caused by a Stop block. One reminder
    # per stop cycle is enough, even when the lane cannot send its reply.
    if payload.get("stop_hook_active") is True:
        return 0
    if not os.environ.get("GRAPHHELM_EXECUTION_ID"):
        return 0
    try:
        result = subprocess.run([sys.executable, str(Path(__file__).resolve()), "--scan"],
                                capture_output=True, timeout=2)
        status = result.returncode
    except (OSError, subprocess.SubprocessError):
        status = 2
    if status == 1:
        reason = ("GraphHelm: read the notes addressed to you on the team execution, act on them, "
                  "then record your operator_note reply with replyTo before stopping.")
        print(json.dumps({"decision": "block", "reason": reason}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
