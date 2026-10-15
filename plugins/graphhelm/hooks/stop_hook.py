"""One Stop hook process for the Claude host (#1278, plan D5).

The Keel gate (`keel_stop_hook`), the lane's reconcile and note reminder (`lane_stop_hook`) and
the verification handoff gate (`handoff_stop_hook`) used to be three Stop entries: three Python
interpreters per turn, and the lane hook started a fourth to scan notes. Here they run in one
process, in the same order, on the one payload read from stdin. Each keeps its own rules: a
gate that fails lets the turn end, `stop_hook_active` is honoured by each, and the Keel refusal
is recorded only after the decision is printed. When more than one gate blocks, the host gets one
block whose reason carries each gate's reason, in order.
"""

from __future__ import annotations

import json
import sys

import handoff_stop_hook
import keel_stop_hook
import lane_stop_hook
from session_hook import hook_input


def main() -> int:
    try:
        payload = hook_input()
    except (ValueError, OSError):
        payload = None
    reasons: list[str] = []
    after = []
    try:
        reason, record = keel_stop_hook.evaluate(payload or {})
        if reason:
            reasons.append(reason)
            after.append(record)
    except Exception:  # noqa: BLE001 - a broken gate never traps the turn
        pass
    if payload is not None:
        for gate in (lambda: lane_stop_hook.evaluate(payload, "claude"),
                     lambda: handoff_stop_hook.evaluate(payload)):
            try:
                reason = gate()
            except Exception:  # noqa: BLE001
                reason = None
            if reason:
                reasons.append(reason)
    if reasons:
        print(json.dumps({"decision": "block", "reason": "\n\n".join(reasons)}))
        sys.stdout.flush()
        # The decision is already printed: recording comes after it and can never undo it.
        for record in after:
            try:
                record()
            except Exception:  # noqa: BLE001
                pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
