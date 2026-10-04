"""Keel Stop gate: no card, no finish; a card that the diff leaves, no finish.

Runs on the host's Stop event. It looks at the branch's change against its merge base with the
default branch. A change in the "summary only" row of the Keel table (docs, or a few changed
lines) passes. Anything larger needs a card at `.graphhelm/keel-card.json`
(`schemas/keel-card.schema.json`), and when the `graphhelm` CLI is on PATH the card must pass
`graphhelm keel check`. The hook blocks once; when the host reports `stop_hook_active` it lets
the turn end, so an agent can never loop on it. Any failure of the hook itself lets the turn end.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

CARD = Path(".graphhelm") / "keel-card.json"
TRIVIAL_LINES = 5
DOC_SUFFIXES = (".md", ".txt", ".rst")
BASES = ("origin/main", "main")


def _git(repo: Path, *args: str) -> str:
    return subprocess.run(["git", "-C", str(repo), *args], capture_output=True, text=True,
                          check=True, timeout=10).stdout


def _merge_base(repo: Path) -> str | None:
    for base in BASES:
        try:
            return _git(repo, "merge-base", base, "HEAD").strip() or None
        except (subprocess.SubprocessError, OSError):
            continue
    return None


def _is_doc(path: str) -> bool:
    return path.startswith("docs/") or path.lower().endswith(DOC_SUFFIXES)


def code_lines_changed(repo: Path, base: str) -> int:
    """Lines added plus removed outside docs since `base`: committed, uncommitted and untracked."""
    total = 0
    for line in _git(repo, "diff", "--numstat", base).splitlines():
        added, removed, path = line.split("\t", 2)
        if _is_doc(path):
            continue
        if added == "-":  # binary
            total += TRIVIAL_LINES + 1
            continue
        total += int(added) + int(removed)
    for path in _git(repo, "ls-files", "--others", "--exclude-standard").splitlines():
        if not _is_doc(path):
            try:
                total += len((repo / path).read_bytes().splitlines())
            except OSError:
                continue
    return total


def decide(repo: Path) -> str | None:
    """The reason to block, or None to let the turn end."""
    base = _merge_base(repo)
    if base is None or code_lines_changed(repo, base) <= TRIVIAL_LINES:
        return None
    card = repo / CARD
    if not card.is_file():
        return (f"Keel: this branch changes code past the summary-only row, and there is no card at "
                f"{CARD.as_posix()}. Write the card (promise, scopePaths, proof; shape in "
                f"docs/keel/KEEL_CHECK.md), run its proof, then finish.")
    cli = shutil.which("graphhelm")
    if cli is None:
        return None
    result = subprocess.run([cli, "--json", "keel", "check", "--diff", f"{base}..HEAD",
                             "--card", str(card), "--repo", str(repo)],
                            capture_output=True, text=True, timeout=60)
    if result.returncode != 2:
        return None
    codes = []
    try:
        for finding in json.loads(result.stdout).get("data", {}).get("findings", []):
            if not finding.get("blocking"):
                continue
            codes.append(f"{finding.get('rule', '?')} {finding.get('path') or ''}".strip())
    except (ValueError, AttributeError):
        pass
    detail = "; ".join(codes[:5]) or "see `graphhelm keel check` output"
    return (f"Keel: `graphhelm keel check` refuses this diff against {CARD.as_posix()}: {detail}. "
            f"Widen the card and say why, or remove the change.")


def main() -> int:
    try:
        payload = json.load(sys.stdin)
    except ValueError:
        payload = {}
    if payload.get("stop_hook_active"):
        return 0
    repo = Path(payload.get("cwd") or os.getcwd())
    try:
        top = _git(repo, "rev-parse", "--show-toplevel").strip()
        reason = decide(Path(top))
    except (subprocess.SubprocessError, OSError, ValueError):
        return 0
    if reason:
        print(json.dumps({"decision": "block", "reason": reason}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
