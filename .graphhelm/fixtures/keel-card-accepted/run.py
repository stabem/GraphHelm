#!/usr/bin/env python3
"""Run a real Keel scope check in a disposable Git repository."""

from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


def run(args: list[str], cwd: Path) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(
        args,
        cwd=cwd,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=30,
        check=False,
        creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
    )


def git(repo: Path, *args: str) -> None:
    result = run(
        ["git", "-c", "user.name=Keel fixture", "-c", "user.email=keel@example.invalid", *args],
        repo,
    )
    if result.returncode:
        raise RuntimeError(result.stderr.decode("utf-8", errors="replace"))


def graphhelm() -> str:
    supplied = os.environ.get("GRAPHHELM_CLI")
    binary = supplied or shutil.which("graphhelm")
    if not binary:
        raise RuntimeError("GRAPHHELM_CLI or graphhelm on PATH is required")
    return binary


def main() -> int:
    if len(sys.argv) != 2 or sys.argv[1] not in {"inside", "outside"}:
        print("usage: run.py inside|outside", file=sys.stderr)
        return 64

    mode = sys.argv[1]
    try:
        with tempfile.TemporaryDirectory(prefix="keel-card-accepted-") as temporary:
            repo = Path(temporary)
            (repo / "src").mkdir()
            source = repo / "src" / "lib.rs"
            source.write_text("pub fn existing() {}\n", encoding="utf-8")
            git(repo, "init", "--quiet", "--object-format=sha1")
            git(repo, "add", ".")
            git(repo, "commit", "--quiet", "-m", "base")

            if mode == "inside":
                source.write_text("pub fn existing() {}\npub fn added() {}\n", encoding="utf-8")
            else:
                notes = repo / "docs" / "notes.md"
                notes.parent.mkdir(parents=True)
                notes.write_text("unplanned edit\n", encoding="utf-8")

            git(repo, "add", ".")
            git(repo, "commit", "--quiet", "-m", "contributor change")
            card = {
                "promise": "added() exists",
                "scopePaths": ["src/lib.rs"],
                "proof": "graphhelm keel check --diff HEAD~1..HEAD",
                "exportedSymbols": ["added"],
            }
            card_path = repo / "card.json"
            card_path.write_text(json.dumps(card), encoding="utf-8")
            result = run(
                [
                    graphhelm(),
                    "--json",
                    "keel",
                    "check",
                    "--diff",
                    "HEAD~1..HEAD",
                    "--repo",
                    str(repo),
                    "--card",
                    str(card_path),
                ],
                repo,
            )
            # Preserve the CLI's actual JSON and exit code for the journey observer.
            sys.stdout.buffer.write(result.stdout)
            sys.stdout.buffer.flush()
            return result.returncode
    except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
        print(str(error), file=sys.stderr)
        return 70


if __name__ == "__main__":
    raise SystemExit(main())
