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
import re
import shutil
import subprocess
import sys
from typing import Callable

# Owner rule: children run hidden in the calling terminal, never in a new console window.
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0

CARD = Path(".graphhelm") / "keel-card.json"
TRIVIAL_LINES = 5
DOC_SUFFIXES = (".md", ".txt", ".rst")
BASES = ("origin/main", "main")


def _git(repo: Path, *args: str) -> str:
    return subprocess.run(["git", "-C", str(repo), *args], capture_output=True, text=True,
                          check=True, timeout=10, creationflags=NO_WINDOW).stdout


SHA = re.compile(r"[0-9a-f]{40}(?:[0-9a-f]{24})?")
BASE_REFS = {"origin/main": "refs/remotes/origin/main", "main": "refs/heads/main"}
MERGE_BASE_CACHE = Path(".graphhelm") / "cache" / "merge-base.json"


def git_dirs(start: Path) -> tuple[Path, Path, Path] | None:
    """#1278: (work tree top, git dir, common dir) read from the `.git` file or directory, with no
    git process. None when the layout is not the plain one; the caller then asks git."""
    try:
        start = start.resolve()
        for top in (start, *start.parents):
            dot = top / ".git"
            if dot.is_dir():
                gitdir = dot
            elif dot.is_file():
                text = dot.read_text(encoding="utf-8").strip()
                if not text.startswith("gitdir:"):
                    return None
                gitdir = Path(text[len("gitdir:"):].strip())
                gitdir = gitdir if gitdir.is_absolute() else (top / gitdir).resolve()
            else:
                continue
            common = gitdir
            pointer = gitdir / "commondir"
            if pointer.is_file():
                common = (gitdir / pointer.read_text(encoding="utf-8").strip()).resolve()
            return (top, gitdir, common) if (gitdir / "HEAD").is_file() else None
    except (OSError, ValueError):
        return None
    return None


def _ref(gitdir: Path, common: Path, name: str, depth: int = 0) -> str | None:
    """The sha a ref names, from the loose ref files or `packed-refs`; None when unreadable."""
    if depth > 4:
        return None
    for root in (gitdir, common):
        path = root / name
        if path.is_file():
            value = path.read_text(encoding="utf-8").strip()
            if value.startswith("ref:"):
                return _ref(gitdir, common, value[4:].strip(), depth + 1)
            return value if SHA.fullmatch(value) else None
    packed = common / "packed-refs"
    if packed.is_file():
        for line in packed.read_text(encoding="utf-8").splitlines():
            sha, _, ref = line.partition(" ")
            if ref == name and SHA.fullmatch(sha):
                return sha
    return None


def read_head(dirs: tuple[Path, Path, Path]) -> str | None:
    """HEAD's sha read from the git dir, or None (unborn branch, unreadable layout)."""
    _top, gitdir, common = dirs
    try:
        value = (gitdir / "HEAD").read_text(encoding="utf-8").strip()
        if value.startswith("ref:"):
            return _ref(gitdir, common, value[4:].strip())
        return value if SHA.fullmatch(value) else None
    except (OSError, ValueError):
        return None


def _merge_base_by_git(repo: Path) -> str | None:
    for base in BASES:
        try:
            return _git(repo, "merge-base", base, "HEAD").strip() or None
        except (subprocess.SubprocessError, OSError):
            continue
    return None


def _merge_base(repo: Path) -> str | None:
    """The merge base of HEAD with the first base ref that exists. #1278: cached under
    `.graphhelm/cache/` keyed by HEAD and that ref's sha, both read from files, so an unchanged
    branch costs no git process; any doubt asks git and does not cache."""
    dirs = git_dirs(repo)
    head = read_head(dirs) if dirs else None
    key = None
    if dirs and head:
        for base in BASES:
            sha = _ref(dirs[1], dirs[2], BASE_REFS[base])
            if sha:
                key = {"head": head, "base": base, "baseSha": sha}
                break
    cache = repo / MERGE_BASE_CACHE
    if key:
        try:
            data = json.loads(cache.read_text(encoding="utf-8"))
            if {name: data.get(name) for name in key} == key and SHA.fullmatch(str(data.get("mergeBase"))):
                return data["mergeBase"]
        except (OSError, ValueError, AttributeError):
            pass
    found = _merge_base_by_git(repo)
    if key and found:
        try:
            cache.parent.mkdir(parents=True, exist_ok=True)
            ignore = cache.parent / ".gitignore"
            if not ignore.is_file():  # the cache never shows up as an untracked change
                ignore.write_text("*\n", encoding="utf-8")
            cache.write_text(json.dumps({**key, "mergeBase": found}), encoding="utf-8")
        except OSError:
            pass
    return found


def toplevel(cwd: Path) -> Path:
    """The work tree top: read from the `.git` layout, else `git rev-parse --show-toplevel`."""
    dirs = git_dirs(cwd)
    if dirs:
        return dirs[0]
    return Path(_git(cwd, "rev-parse", "--show-toplevel").strip())


def _is_doc(path: str) -> bool:
    return path.startswith("docs/") or path.lower().endswith(DOC_SUFFIXES)


def untracked(repo: Path) -> str:
    """`git ls-files --others --exclude-standard`: read once per hook run and shared (#1278)."""
    return _git(repo, "ls-files", "--others", "--exclude-standard")


def code_lines_changed(repo: Path, base: str, others: str | None = None) -> int:
    """Lines added plus removed outside docs since `base`: committed, uncommitted and untracked.
    `others` is `untracked(repo)` when the caller already has it."""
    total = 0
    for line in _git(repo, "diff", "--numstat", base).splitlines():
        added, removed, path = line.split("\t", 2)
        if _is_doc(path):
            continue
        if added == "-":  # binary
            total += TRIVIAL_LINES + 1
            continue
        total += int(added) + int(removed)
    for path in (untracked(repo) if others is None else others).splitlines():
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


def _head(repo: Path) -> str:
    """HEAD's sha followed by a newline, as `git rev-parse HEAD` prints it; read from the git dir
    when the layout allows (#1278)."""
    dirs = git_dirs(repo)
    head = read_head(dirs) if dirs else None
    return f"{head}\n" if head else _git(repo, "rev-parse", "HEAD")


def _fingerprint(repo: Path, base: str, card: Path, cli: str, payload: dict,
                 others: str | None = None) -> str | None:
    """A digest of everything the check reads, or None when it cannot be computed."""
    try:
        others = untracked(repo) if others is None else others
        digest = hashlib.sha256()
        digest.update(f"{_execution(payload)}\0".encode())
        digest.update(f"{_tool_stamp(cli)}\0{_tool_stamp(__file__)}\0".encode())
        for part in (base, _head(repo), _git(repo, "diff", "--binary", base), others):
            digest.update(part.encode() + b"\0")
        for path in others.splitlines():
            digest.update((repo / path).read_bytes() + b"\0")
        digest.update(card.read_bytes())
        return digest.hexdigest()
    except (subprocess.SubprocessError, OSError, ValueError):
        return None


def _cache_path(repo: Path) -> Path | None:
    dirs = git_dirs(repo)
    if dirs:
        return dirs[1] / CACHE
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


def forget(repo: Path) -> None:
    """Drop the cached verdict, so the next Stop checks and records again."""
    path = _cache_path(repo)
    try:
        if path:
            path.unlink(missing_ok=True)
    except OSError:
        pass


def decide(repo: Path, payload: dict | None = None) -> str | None:
    """The reason to block, or None to let the turn end."""
    global LAST_FROM_CACHE
    LAST_FROM_CACHE = False
    base = _merge_base(repo)
    if base is None:
        return None
    others = untracked(repo)
    if code_lines_changed(repo, base, others) <= TRIVIAL_LINES:
        return None
    card = repo / CARD
    if not card.is_file():
        return (f"Keel: this branch changes code past the summary-only row, and there is no card at "
                f"{CARD.as_posix()}. Write the card (promise, scopePaths, proof; shape in "
                f"docs/keel/KEEL_CHECK.md), run its proof, then finish.")
    cli = shutil.which("graphhelm")
    if cli is None:
        return None
    fingerprint = _fingerprint(repo, base, card, cli, payload or {}, others)
    hit = _cached(repo, fingerprint)
    if hit is None:
        result = subprocess.run([cli, "--json", "keel", "check", "--diff", f"{base}..HEAD",
                                 "--card", str(card), "--repo", str(repo)],
                                capture_output=True, text=True, timeout=60, creationflags=NO_WINDOW)
        returncode, stdout = result.returncode, result.stdout
        if returncode in (0, 2):
            delivered = False
            try:
                from keel_record import record_check
                delivered = record_check(json.loads(stdout), payload or {})
            except Exception:  # noqa: BLE001 - recording must not change the gate
                pass
            # Cache only once the signal is on the execution (or there is none to send it to), so
            # a Runtime hiccup is retried on the next Stop instead of lost.
            if delivered or not _execution(payload or {}):
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


def evaluate(payload: dict) -> tuple[str | None, Callable[[], None]]:
    """#1278: the gate for one Stop payload, callable in-process by `stop_hook.py`. Returns the
    block reason (or None) and the recording step to run after the decision is printed."""
    if payload.get("stop_hook_active"):
        return None, lambda: None
    repo = Path(payload.get("cwd") or os.getcwd())
    try:
        top = toplevel(repo)
        reason = decide(top, payload)
    except (subprocess.SubprocessError, OSError, ValueError):
        return None, lambda: None
    if not reason or LAST_FROM_CACHE:  # a cached refusal was already recorded (#1228)
        return reason, lambda: None

    def record() -> None:
        delivered = False
        try:
            from keel_record import record_block
            delivered = record_block("stop", reason, payload)
        except Exception:  # noqa: BLE001
            pass
        if not delivered and _execution(payload):
            forget(top)  # the refusal never reached the execution: retry on the next Stop
    return reason, record


def main() -> int:
    try:
        payload = json.load(sys.stdin)
    except ValueError:
        payload = {}
    reason, record = evaluate(payload if isinstance(payload, dict) else {})
    if reason:
        print(json.dumps({"decision": "block", "reason": reason}))
        # The decision is already printed: recording comes after it and can never undo it.
        record()
    return 0


if __name__ == "__main__":
    sys.exit(main())
