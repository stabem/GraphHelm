"""Keel route card for Codex (#2997): a SessionStart hook that puts a fixed routing card in the Codex session context.

The same job as the Claude Code card in the keel plugin, written with Codex's own names: Codex has the
built-in subagents `default`, `worker` and `explorer`, and a subagent's model is asked for in the spawn
request (Codex subagents documentation, "Choosing models and reasoning" and "Custom agents").

The card is static text. It reads no stdin, calls no network, spawns nothing (no `subprocess`, so no console
window) and never fails loudly: on any error it prints nothing and exits 0. Context reaches the session as
`hookSpecificOutput.additionalContext` with `hookEventName: SessionStart` (Codex hooks documentation,
"SessionStart"); the card is far under the default `additionalContextLimit` of 2500 tokens.

No agent lock ships with it: the documented PreToolUse input has no `agent_id` or `agent_type` (only
SubagentStart, SubagentStop and the model-request hooks carry them), so a Codex hook cannot tell a Keel
agent from the main session, and a lock that cannot tell must never refuse anything.
"""
import json
import os
import sys

CARD = """Keel route card (Codex). Spawn the subagent in the row for the step, asking for its model in the spawn request.
Each brief carries one promise and `Session: <your session name> [ref] (<role> subagent)`, filled in; the session name is never a role name.
step | subagent | model | the subagent loads
read-only question (audit, census, where is X) | explorer | cheapest | no skill
write | worker | middle | keel, keel-exec
mechanical write (no judgement) | worker | cheapest | keel, keel-exec
review, then merge | worker, not the author's | strongest | keel-review, keel-merge
The skills are the Keel plugin's; name them in the brief. A review is by a session that did not write the change."""


if __name__ == "__main__":
    try:
        reply = json.dumps({"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": CARD}})
        sys.stdout.buffer.write((reply + "\n").encode("ascii"))
        sys.stdout.flush()
    except BaseException:  # noqa: BLE001 - a card that cannot print says nothing; exit 0 below either way
        os._exit(0)
    os._exit(0)
