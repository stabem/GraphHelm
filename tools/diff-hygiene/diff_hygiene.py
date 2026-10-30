#!/usr/bin/env python3
"""Diff hygiene before review (#2251; rule in keel/KEEL_EXEC.md).

    python tools/diff-hygiene/diff_hygiene.py --card <pr-body-or-card.md> [--base origin/main] [--head HEAD]

Compares `git diff <base>...<head>` with the Keel card and prints one status line:
  1. line endings: `--numstat` differs from `--numstat --ignore-cr-at-eol` for a file;
  2. stale base: a branch path also changed on <base> after the merge-base;
  3. scope: a branch path outside the card's `Paths:` line plus `allow-paths.txt`
     (a card without a `Paths:` line is summary-only and skips this check).
When <base> cannot be fetched or the card cannot be read, the status is `unobserved`, never `passed`.
Exit code: 0 passed, 1 failed, 2 unobserved.
"""
from __future__ import annotations

import argparse
import fnmatch
import re
import subprocess
import sys
from pathlib import Path

ALLOW_FILE = Path(__file__).with_name("allow-paths.txt")
GIT_TIMEOUT = 120
PATHS_LINE = re.compile(r"^\s*(?:[-*]\s*)?(?:\*\*)?Paths(?:\*\*)?\s*:(?:\*\*)?\s*(.*)$", re.IGNORECASE | re.MULTILINE)


class Unobserved(Exception):
    pass


def git(repo: str, *args: str) -> str:
    try:
        done = subprocess.run(["git", "-C", repo, *args], capture_output=True, text=True,
                              encoding="utf-8", errors="replace", timeout=GIT_TIMEOUT)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise Unobserved(f"git {args[0]}: {error}") from error
    if done.returncode != 0:
        raise Unobserved(f"git {' '.join(args)}: {done.stderr.strip() or done.returncode}")
    return done.stdout


def card_paths(text: str) -> list[str] | None:
    """The card's paths, or None for a summary-only card (no `Paths:` line)."""
    match = PATHS_LINE.search(text)
    if not match:
        return None
    quoted = re.findall(r"`([^`]+)`", match.group(1))
    items = quoted or [part.strip() for part in match.group(1).split(",")]
    return [path.strip().rstrip("/") for item in items for path in expand_braces(item) if path.strip()]


def expand_braces(item: str) -> list[str]:
    """`a/{b,c/d}.rs` -> `a/b.rs`, `a/c/d.rs`, as cards write grouped paths."""
    match = re.search(r"\{([^{}]*)\}", item)
    if not match:
        return [item]
    head, tail = item[:match.start()], item[match.end():]
    return [path for part in match.group(1).split(",") for path in expand_braces(head + part.strip() + tail)]


def in_scope(path: str, patterns: list[str]) -> bool:
    return any(path == p or path.startswith(p + "/") or fnmatch.fnmatchcase(path, p) for p in patterns)


def numstat(repo: str, rng: str, *extra: str) -> dict[str, str]:
    rows = {}
    for line in git(repo, "diff", "--numstat", *extra, rng).splitlines():
        added, removed, path = line.split("\t", 2)
        rows[path] = f"{added}\t{removed}"
    return rows


def check(repo: str, base: str, head: str, card_text: str, allow: list[str]) -> list[str]:
    rng = f"{base}...{head}"
    problems = []
    plain, ignoring_cr = numstat(repo, rng), numstat(repo, rng, "--ignore-cr-at-eol")
    crlf = sorted(p for p in plain if plain[p] != ignoring_cr.get(p))
    if crlf:
        problems.append(f"line endings: {', '.join(crlf)}")
    changed = set(plain)
    merge_base = git(repo, "merge-base", base, head).strip()
    moved = set(git(repo, "diff", "--name-only", merge_base, base).splitlines())
    overlap = sorted(changed & moved)
    if overlap:
        problems.append(f"stale base: {', '.join(overlap)} also changed on {base}; rebase and rerun")
    paths = card_paths(card_text)
    if paths is not None:
        outside = sorted(p for p in changed if not in_scope(p, paths + allow))
        if outside:
            problems.append(f"scope: {', '.join(outside)} outside the card's paths")
    return problems


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--card", required=True, help="file holding the Keel card (the PR body works)")
    ap.add_argument("--base", default="origin/main")
    ap.add_argument("--head", default="HEAD")
    ap.add_argument("--repo", default=".")
    ap.add_argument("--no-fetch", action="store_true", help="use the local <base> ref as it is")
    args = ap.parse_args(argv)
    try:
        try:
            card_text = Path(args.card).read_text(encoding="utf-8-sig")
        except OSError as error:
            raise Unobserved(f"card unreadable: {error}") from error
        if not args.no_fetch and "/" in args.base:
            remote, branch = args.base.split("/", 1)
            git(args.repo, "fetch", "--quiet", remote, branch)
        allow = [line.strip() for line in ALLOW_FILE.read_text(encoding="utf-8").splitlines()
                 if line.strip() and not line.startswith("#")]
        problems = check(args.repo, args.base, args.head, card_text, allow)
    except Unobserved as reason:
        print(f"diff-hygiene: unobserved | {reason}")
        return 2
    head = git(args.repo, "rev-parse", "--short=8", args.head).strip()
    if problems:
        print(f"diff-hygiene: failed @ {head} | " + " | ".join(problems))
        return 1
    print(f"diff-hygiene: passed @ {head} | line endings, base and scope clean")
    return 0


if __name__ == "__main__":
    sys.exit(main())
