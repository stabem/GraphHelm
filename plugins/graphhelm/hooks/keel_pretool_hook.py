"""Keel PreToolUse gate: no card, no code edit past the summary-only row.

Runs before Edit, Write and MultiEdit. When the edit targets a file outside docs, and the branch's
code change plus the lines this edit writes would pass the summary-only row of the Keel table, the
edit is denied until `.graphhelm/keel-card.json` exists. Writing the card itself is always allowed.
Any failure of the hook itself lets the edit through; the Stop gate still stands behind it.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys

from keel_stop_hook import CARD, TRIVIAL_LINES, _git, _is_doc, _merge_base, code_lines_changed


def _edit_lines(tool_input: dict) -> int:
    if "content" in tool_input:
        return len(str(tool_input["content"]).splitlines())
    edits = tool_input.get("edits") or [tool_input]
    return sum(max(len(str(e.get("new_string", "")).splitlines()),
                   len(str(e.get("old_string", "")).splitlines())) for e in edits)


def decide(repo: Path, target: Path, tool_input: dict) -> str | None:
    try:
        relative = target.resolve().relative_to(repo.resolve()).as_posix()
    except ValueError:
        return None
    if _is_doc(relative) or relative.startswith(".graphhelm/") or (repo / CARD).is_file():
        return None
    base = _merge_base(repo)
    if base is None:
        return None
    if code_lines_changed(repo, base) + _edit_lines(tool_input) <= TRIVIAL_LINES:
        return None
    return (f"Keel: this edit takes the branch past the summary-only row and there is no card at "
            f"{CARD.as_posix()}. Write the card first (promise, scopePaths, proof; shape in "
            f"docs/keel/KEEL_CHECK.md), then edit {relative}.")


def main() -> int:
    try:
        payload = json.load(sys.stdin)
        tool_input = payload.get("tool_input") or {}
        path = tool_input.get("file_path")
        if not path:
            return 0
        cwd = Path(payload.get("cwd") or os.getcwd())
        target = Path(path) if os.path.isabs(path) else cwd / path
        repo = Path(_git(cwd, "rev-parse", "--show-toplevel").strip())
        reason = decide(repo, target, tool_input)
    except (subprocess.SubprocessError, OSError, ValueError, AttributeError):
        return 0
    if reason:
        print(json.dumps({"hookSpecificOutput": {"hookEventName": "PreToolUse",
                                                 "permissionDecision": "deny",
                                                 "permissionDecisionReason": reason}}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
