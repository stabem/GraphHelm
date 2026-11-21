"""Keel route card (#2995): a SessionStart hook that puts a fixed routing card in the session context.

The orchestrator routes each step to an agent from memory, and a restart or a compaction drops that
memory. The card is static text: for each step, the agent to brief, its model and the skill that agent
loads. It reads no stdin, calls no network, spawns nothing (no `subprocess`, so no console window) and
never fails loudly: on any error it prints nothing and exits 0.

Context reaches the session as `hookSpecificOutput.additionalContext` with
`hookEventName: SessionStart` (host hooks documentation, SessionStart decision control).

The plugin ships this file and the graphhelm plugin ships a byte-identical copy (a plugin cannot read
a sibling plugin's files); the test beside it keeps the two equal.
"""
import json
import os
import sys

CARD = """Keel route card. Brief the agent in the row for the step; the default family is `keel:`.
Each brief carries one promise and `Session: <your session name> [ref] (<agent> subagent)`, filled in.
step | agent to brief | model | the agent loads
read-only question (audit, census, where is X) | keel:keel-scout | haiku | no skill
write | keel:keel-author | sonnet | keel:keel, keel:keel-exec
mechanical write (no judgement) | keel:keel-author | haiku (per-call override) | keel:keel, keel:keel-exec
review, then merge | keel:keel-reviewer | opus | keel:keel-review, keel:keel-merge
journey QA | graphhelm:keel-journey-qa | opus | keel:keel-journey-qa
Why `keel:`: it is standalone, carries the scout and the current step skills; `graphhelm:keel-author`
and `graphhelm:keel-reviewer` are twins that need other plugins' skills and can be older.
Journey QA exists only as a `graphhelm:` agent; the `keel:` plugin ships its skill only."""


if __name__ == "__main__":
    try:
        reply = json.dumps({"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": CARD}})
        sys.stdout.buffer.write((reply + "\n").encode("ascii"))
        sys.stdout.flush()
    except BaseException:  # noqa: BLE001 - a card that cannot print says nothing; exit 0 below either way
        os._exit(0)
    os._exit(0)
