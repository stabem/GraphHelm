"""Keel review model warning (#3000): a PreToolUse hook on the Skill tool.

KEEL.md says the strongest model checks. A main session loads the review skill on whatever model it
runs, and the agent file's model does not reach it. When the skill being loaded is a review skill
and the session's last assistant message came from a middle or cheap model, the hook adds a short
warning to the session context. Loading the old `blind-review` skill also points that session at
`keel:keel-review`, on any model.

How the model is known: the hook input carries no model field (observed on this host), so the hook
reads the model of the newest assistant (`message.model`) or api-request (`params.model`) entry in
the file named by `transcript_path`. A subagent (`agent_id` in the input) is left alone: its agent file pins the model and its
transcript_path is the parent's. Any doubt (no path, no assistant entry, a model id of another
family) says nothing. It only warns, never blocks; it spawns nothing (no console window) and exits 0
on every failure.

The plugin ships this file only in `keel`: a plugin cannot read a sibling plugin's files.
"""
import json
import os
import sys

REVIEW_SKILLS = {"keel-review", "keel:keel-review", "blind-review", "graphhelm-development-contracts:blind-review"}
OLD_SKILLS = {"blind-review", "graphhelm-development-contracts:blind-review"}
NOT_STRONGEST = ("sonnet", "haiku")
TAIL_BYTES = 1048576

WARNING = (
    "Keel review model: this session runs on `{model}`, not the strongest tier. KEEL.md says the "
    "strongest model checks: hand this review to `keel:keel-reviewer` (opus), or tell the owner. "
    "This is a warning; the review is not blocked."
)
OLD_SKILL_NOTE = (
    "`blind-review` is the GraphHelm-task flavour of the review (it adds the task records); "
    "`keel:keel-review` (keel/KEEL_REVIEW.md) rules wherever the two differ."
)


def last_model(transcript_path):
    """The model id of the newest main-chain assistant entry in the last TAIL_BYTES, or None.

    A sidechain (subagent) entry and a `<synthetic>` model are skipped. The assistant entry of the
    tool call that fires the hook is not written yet, so a headless session whose first turn loads
    the skill has none; then the newest api-request entry, which that host writes, covers it."""
    with open(transcript_path, "rb") as handle:
        handle.seek(0, os.SEEK_END)
        size = handle.tell()
        handle.seek(max(0, size - TAIL_BYTES))
        lines = handle.read().decode("utf-8", errors="replace").splitlines()
    request_model = None
    for line in reversed(lines):
        try:
            entry = json.loads(line)
        except ValueError:
            continue
        if not isinstance(entry, dict) or entry.get("isSidechain"):
            continue
        if entry.get("type") == "api-request" and request_model is None:
            request_model = (entry.get("params") or {}).get("model")
        elif entry.get("type") == "assistant":
            model = (entry.get("message") or {}).get("model")
            if isinstance(model, str) and model and not model.startswith("<"):
                return model
    return request_model if isinstance(request_model, str) and request_model else None


def advice(hook_input, model_of=last_model):
    """The context text to add, or None when the skill is not a review skill and nothing is to say."""
    if not isinstance(hook_input, dict) or hook_input.get("agent_id"):
        return None
    skill = (hook_input.get("tool_input") or {}).get("skill")
    if skill not in REVIEW_SKILLS:
        return None
    parts = []
    path = hook_input.get("transcript_path")
    model = model_of(path) if isinstance(path, str) and path else None
    if model and any(tier in model.lower() for tier in NOT_STRONGEST):
        parts.append(WARNING.format(model=model))
    if skill in OLD_SKILLS:
        parts.append(OLD_SKILL_NOTE)
    return " ".join(parts) or None


if __name__ == "__main__":
    try:
        text = advice(json.loads(sys.stdin.read()))
        if text:
            reply = json.dumps({"hookSpecificOutput": {"hookEventName": "PreToolUse", "additionalContext": text}})
            sys.stdout.buffer.write((reply + "\n").encode("ascii"))
            sys.stdout.flush()
    except BaseException:  # noqa: BLE001 - a warning that cannot be computed says nothing; exit 0 below either way
        os._exit(0)
    os._exit(0)
