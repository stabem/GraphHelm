"""Keel Stop gate: no card, no finish; a card that the diff leaves, no finish.

Runs on the host's Stop event. It looks at the branch's change against its merge base with the
default branch. A change in the "summary only" row of the Keel table (docs, or a few changed
lines) passes. Anything larger needs a card at `.graphhelm/keel-card.json`
(`schemas/keel-card.schema.json`), and when the `graphhelm` CLI is on PATH the card must pass
`graphhelm keel check`. The hook blocks once; when the host reports `stop_hook_active` it lets
the turn end, so an agent can never loop on it. Any failure of the hook itself lets the turn end.

The check result is cached in the git dir under a fingerprint of the change (merge base, HEAD,
working-tree diff, untracked files, card). A Stop on an unchanged state reuses it: no second CLI
run and no second signal on the execution (#1228). The decision is the same either way.
"""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

# Owner rule: children run hidden in the calling terminal, never in a new console window.
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0

CARD = Path(".graphhelm") / "keel-card.json"
TRIVIAL_LINES = 5
DOC_SUFFIXES = (".md", ".txt", ".rst")
BASES = ("origin/main", "main")


def _git(repo: Path, *args: str) -> str:
    return subprocess.run(["git", "-C", str(repo), *args], capture_output=True, text=True,
                          check=True, timeout=10, creationflags=NO_WINDOW).stdout


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


CACHE = "graphhelm-keel-stop.json"
LAST_FROM_CACHE = False  # set by decide(): the verdict came from the cache, already recorded


def _tool_stamp(path: str) -> str:
    """Path, size and mtime of a file, so a new CLI or hook version misses the cache."""
    stat = os.stat(path)
    return f"{os.path.realpath(path)}:{stat.st_size}:{stat.st_mtime_ns}"


def _execution(payload: dict) -> str:
    """The bound execution, so a cached verdict never stands in for another execution's signal."""
    try:
        import session_hook
        bound = session_hook.binding(payload)
        return bound[0] if bound else ""
    except Exception:  # noqa: BLE001 - an unknown binding just means a narrower cache key
        return os.environ.get("GRAPHHELM_EXECUTION_ID", "")


def _fingerprint(repo: Path, base: str, card: Path, cli: str, payload: dict) -> str | None:
    """A digest of everything the check reads, or None when it cannot be computed."""
    try:
        digest = hashlib.sha256()
        digest.update(f"{_execution(payload)}\0".encode())
        digest.update(f"{_tool_stamp(cli)}\0{_tool_stamp(__file__)}\0".encode())
        for part in (base, _git(repo, "rev-parse", "HEAD"), _git(repo, "diff", "--binary", base),
                     _git(repo, "ls-files", "--others", "--exclude-standard")):
            digest.update(part.encode() + b"\0")
        for path in _git(repo, "ls-files", "--others", "--exclude-standard").splitlines():
            digest.update((repo / path).read_bytes() + b"\0")
        digest.update(card.read_bytes())
        return digest.hexdigest()
    except (subprocess.SubprocessError, OSError, ValueError):
        return None


def _cache_path(repo: Path) -> Path | None:
    try:
        path = Path(_git(repo, "rev-parse", "--absolute-git-dir").strip())
    except (subprocess.SubprocessError, OSError):
        return None
    return path / CACHE if path.is_dir() else None


def _cached(repo: Path, fingerprint: str | None) -> tuple[int, str] | None:
    path = _cache_path(repo) if fingerprint else None
    try:
        data = json.loads(path.read_text()) if path else None
        if data and data.get("fingerprint") == fingerprint:
            return int(data["returncode"]), str(data["stdout"])
    except (OSError, ValueError, KeyError, TypeError, AttributeError):
        pass
    return None


def _store(repo: Path, fingerprint: str | None, returncode: int, stdout: str) -> None:
    path = _cache_path(repo) if fingerprint else None
    try:
        if path:
            path.write_text(json.dumps({"fingerprint": fingerprint, "returncode": returncode,
                                        "stdout": stdout}))
    except OSError:
        pass


def decide(repo: Path, payload: dict | None = None) -> str | None:
    """The reason to block, or None to let the turn end."""
    global LAST_FROM_CACHE
    LAST_FROM_CACHE = False
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
    fingerprint = _fingerprint(repo, base, card, cli, payload or {})
    hit = _cached(repo, fingerprint)
    if hit is None:
        result = subprocess.run([cli, "--json", "keel", "check", "--diff", f"{base}..HEAD",
                                 "--card", str(card), "--repo", str(repo)],
                                capture_output=True, text=True, timeout=60, creationflags=NO_WINDOW)
        returncode, stdout = result.returncode, result.stdout
        if returncode in (0, 2):
            try:
                from keel_record import record_check
                record_check(json.loads(stdout), payload or {})
            except Exception:  # noqa: BLE001 - recording must not change the gate
                pass
            _store(repo, fingerprint, returncode, stdout)
    else:
        returncode, stdout = hit
        LAST_FROM_CACHE = True
    if returncode != 2:
        return None
    codes = []
    try:
        for finding in json.loads(stdout).get("data", {}).get("findings", []):
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
        reason = decide(Path(top), payload)
    except (subprocess.SubprocessError, OSError, ValueError):
        return 0
    if reason:
        print(json.dumps({"decision": "block", "reason": reason}))
        if LAST_FROM_CACHE:  # this exact refusal was already recorded on this execution (#1228)
            return 0
        # The decision is already printed: recording comes after it and can never undo it.
        try:
            from keel_record import record_block
            record_block("stop", reason, payload)
        except Exception:  # noqa: BLE001
            pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
