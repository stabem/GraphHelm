"""Keel PreToolUse gate: no card, no code edit past the summary-only row.

Runs before Edit, Write and MultiEdit. When the edit targets a file outside docs, and the branch's
code change plus the lines this edit writes would pass the summary-only row of the Keel table, the
edit is denied until `.graphhelm/keel-card.json` exists. Writing the card itself is always allowed.
Once the card exists, an edit to a non-test file under the card's scopePaths is denied until the
branch has a new or changed test file under those paths (test first). A card with
`"testFirst": false` and a non-empty `testFirstReason` waives that check.
Any failure of the hook itself lets the edit through; the Stop gate still stands behind it.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys

import re

from keel_stop_hook import (CARD, TRIVIAL_LINES, _git, _is_doc, _merge_base, code_lines_changed,
                            toplevel, untracked)

TEST_NAME = re.compile(r"(^|/)(tests?/|test_[^/]*$|[^/]*_test\.[^/]+$|[^/]*\.(test|spec)\.[^/]+$)")


def is_test(path: str) -> bool:
    return bool(TEST_NAME.search(path))


def _in_scope(path: str, scope: list[str]) -> bool:
    return any(path == s or path.startswith(s.rstrip("/") + "/") for s in scope)


def _changed(repo: Path, base: str) -> list[str]:
    tracked = _git(repo, "diff", "--name-only", base).splitlines()
    return tracked + untracked(repo).splitlines()


def test_first(repo: Path, relative: str) -> str | None:
    """Deny a source edit in the card's scope while no test in that scope has changed."""
    if is_test(relative):
        return None
    try:
        card = json.loads((repo / CARD).read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None
    if not isinstance(card, dict):
        return None
    if card.get("testFirst") is False and str(card.get("testFirstReason") or "").strip():
        return None
    scope = [str(s).replace("\\", "/").removeprefix("./").rstrip("/")
             for s in card.get("scopePaths") or [] if str(s).strip()]
    if not scope or not _in_scope(relative, scope):
        return None
    base = _merge_base(repo)
    if base is None:
        return None
    if any(is_test(p) and _in_scope(p, scope) for p in _changed(repo, base)):
        return None
    return (f"Keel test first: no test under the card's scopePaths has changed yet. Write the failing "
            f"test first, run it from the repository root and see it fail, then edit {relative}. "
            f"If this change has no testable behavior, set \"testFirst\": false and a "
            f"\"testFirstReason\" in {CARD.as_posix()}.")


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
    if _is_doc(relative) or relative.startswith(".graphhelm/"):
        return None
    if (repo / CARD).is_file():
        return test_first(repo, relative)
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
        repo = toplevel(cwd)
        reason = decide(repo, target, tool_input)
    except (subprocess.SubprocessError, OSError, ValueError, AttributeError):
        return 0
    if reason:
        print(json.dumps({"hookSpecificOutput": {"hookEventName": "PreToolUse",
                                                 "permissionDecision": "deny",
                                                 "permissionDecisionReason": reason}}))
        # The decision is already printed: recording comes after it and can never undo it.
        try:
            from keel_record import record_block
            record_block("pretool", reason, payload)
        except Exception:  # noqa: BLE001
            pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
