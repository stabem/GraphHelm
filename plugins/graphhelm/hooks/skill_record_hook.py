"""Record which skill a session loaded, as a `skill.loaded` signal on its bound execution.

Runs after the Skill tool. Best effort and silent: outside a bound execution, on a payload that is
not a Skill call with a non-empty string name, or on any failure, nothing happens. It records the
skill name, the session and (for a subagent) the agent id, and never `args`, any other tool input
or any tool output. It prints nothing, always exits 0 and never changes the tool call.
"""

from __future__ import annotations

import json
import sys

NAME_LIMIT = 128


def record(payload: object) -> bool:
    if not isinstance(payload, dict) or payload.get("tool_name") != "Skill":
        return False
    tool_input = payload.get("tool_input")
    name = tool_input.get("skill") if isinstance(tool_input, dict) else None
    if not isinstance(name, str) or not name.strip():
        return False
    name = name.strip()[:NAME_LIMIT]
    agent = payload.get("agent_id")
    agent = agent[:NAME_LIMIT] if isinstance(agent, str) and agent else None
    from keel_record import post_signal
    return post_signal(payload, id_prefix="skill-loaded", digest_text=f"skill.loaded\0{name}\0{agent}",
                       kind="skill.loaded", severity="low", description=f"Skill loaded: {name}",
                       evidence_for=lambda session: [f"skill: {name}", f"session: {session}",
                                                     *([f"agent: {agent}"] if agent else [])],
                       tool_id="skill-record")


def main() -> int:
    try:
        record(json.load(sys.stdin))
    except Exception:  # noqa: BLE001 - recording must never change, block or annotate the tool call
        return 0  # deliberate: nothing is printed and the exit stays 0 whatever went wrong
    return 0


if __name__ == "__main__":
    sys.exit(main())
