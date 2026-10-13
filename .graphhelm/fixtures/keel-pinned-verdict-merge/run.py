#!/usr/bin/env python3
"""Observe head-pinned verdicts in an isolated two-commit Git fixture."""

from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


HERE = Path(__file__).resolve().parent


def run(argv: list[str], cwd: Path) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(
        argv, cwd=cwd, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, timeout=30, check=False,
        creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
    )


def git(repo: Path, *args: str) -> str:
    result = run(["git", "-c", "user.name=Verdict fixture", "-c",
                  "user.email=verdict@example.invalid", "-c", "commit.gpgsign=false",
                  *args], repo)
    if result.returncode:
        raise RuntimeError(result.stderr.decode("utf-8", errors="replace"))
    return result.stdout.decode("ascii").strip()


def main() -> int:
    if len(sys.argv) != 2 or sys.argv[1] not in {"a", "stale", "b", "block"}:
        print("usage: run.py a|stale|b|block", file=sys.stderr)
        return 64
    try:
        with tempfile.TemporaryDirectory(prefix="keel-pinned-verdict-") as temporary:
            root = Path(temporary)
            bare = root / "remote.git"
            bare.mkdir()
            git(bare, "init", "--quiet", "--bare", "--object-format=sha1")
            repo = root / "work"
            repo.mkdir()
            git(repo, "init", "--quiet", "--object-format=sha1")
            git(repo, "remote", "add", "origin", str(bare))
            source = repo / "change.txt"
            source.write_text("A\n", encoding="utf-8")
            git(repo, "add", "change.txt")
            git(repo, "commit", "--quiet", "-m", "head A")
            head_a = git(repo, "rev-parse", "HEAD")
            git(repo, "push", "--quiet", "origin", "HEAD:refs/heads/main")
            mode = sys.argv[1]
            head_b = None
            if mode != "a":
                source.write_text("B\n", encoding="utf-8")
                git(repo, "commit", "--quiet", "-am", "head B")
                head_b = git(repo, "rev-parse", "HEAD")
                git(repo, "push", "--quiet", "origin", "HEAD:refs/heads/main")
                if head_a == head_b:
                    raise RuntimeError("fixture heads did not change")
            template = (HERE / ("approval-a.json" if mode in {"a", "stale"} else "review-b.json")).read_text(encoding="utf-8")
            snapshot = root / "comments.json"
            snapshot_data = json.loads(template.replace("{A8}", head_a[:8]).replace("{B8}", (head_b or head_a)[:8]))
            if mode == "b":
                snapshot_data["comments"] = [comment for comment in snapshot_data["comments"] if comment["id"] != "block-b"]
            snapshot.write_text(json.dumps(snapshot_data), encoding="utf-8")
            cli = os.environ.get("GRAPHHELM_CLI") or shutil.which("graphhelm")
            if not cli:
                raise RuntimeError("GRAPHHELM_CLI or graphhelm on PATH is required")
            head = head_a if mode == "a" else head_b
            result = run([cli, "--json", "keel", "verdict", "--head", head,
                          "--reviewer", "codex-review", "--comments", str(snapshot)], repo)
            if result.returncode:
                sys.stderr.buffer.write(result.stderr)
                sys.stdout.buffer.write(result.stdout)
                return result.returncode
            data = json.loads(result.stdout)
            expected = {"a": (["approval-a"], []), "stale": ([], []),
                        "b": (["approval-b"], []),
                        "block": (["approval-b", "block-b"], ["block-b"])}[mode]
            ids = [verdict["commentId"] for verdict in data["data"]["verdicts"]]
            blocks = data["data"]["matchingBlockIds"]
            # Keep the real CLI result visible to the journey. The test also checks
            # literal IDs, so a fixture cannot pass by merely printing a claim.
            print(json.dumps({"headA": head_a, "headB": head_b, "mode": mode,
                              "verdicts": ids, "matchingBlockIds": blocks,
                              "matchesExpected": (ids, blocks) == expected}, separators=(",", ":")))
            return 0 if (ids, blocks) == expected else 1
    except (OSError, RuntimeError, ValueError, KeyError, subprocess.TimeoutExpired) as error:
        print(str(error), file=sys.stderr)
        return 70


if __name__ == "__main__":
    raise SystemExit(main())
