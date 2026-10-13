#!/usr/bin/env python3
"""Exercise proportional Keel guidance against disposable Git histories."""

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


def commit(repo: Path, message: str) -> None:
    git(repo, "add", ".")
    git(repo, "commit", "--quiet", "-m", message)


def graphhelm() -> str:
    supplied = os.environ.get("GRAPHHELM_CLI")
    binary = supplied or shutil.which("graphhelm")
    if not binary:
        raise RuntimeError("GRAPHHELM_CLI or graphhelm on PATH is required")
    return binary


def main() -> int:
    if len(sys.argv) != 2 or sys.argv[1] not in {"docs", "bounded-code", "expanded"}:
        print("usage: run.py docs|bounded-code|expanded", file=sys.stderr)
        return 64

    mode = sys.argv[1]
    try:
        with tempfile.TemporaryDirectory(prefix="keel-proportional-rows-") as temporary:
            repo = Path(temporary)
            (repo / "src").mkdir()
            (repo / "src/lib.rs").write_text("pub fn existing() {}\n", encoding="utf-8")
            if mode == "expanded":
                (repo / "card.json").write_text(
                    json.dumps({
                        "promise": "Persist the journal entry",
                        "scopePaths": ["core/events/journal.rs", "keel.yaml"],
                        "proof": "fixture check",
                    }),
                    encoding="utf-8",
                )
            git(repo, "init", "--quiet", "--object-format=sha1")
            commit(repo, "base")

            (repo / "docs").mkdir()
            (repo / "docs/notes.md").write_text("Document the feature.\n", encoding="utf-8")
            commit(repo, "docs-only change")

            (repo / "src/lib.rs").write_text(
                "pub fn existing() {}\npub fn added() {}\n", encoding="utf-8"
            )
            commit(repo, "bounded code change")

            if mode == "expanded":
                (repo / "keel.yaml").write_text(
                    "version: 1\ninvariants: {}\nplan:\n  reviews: {docs: 1, code: 1, user_visible: 1, invariant: 1}\n  delegation: {docs: [small, low], code: [standard, medium], user_visible: [standard, medium], invariant: [large, high]}\n  jevThresholdPercent: 80\n",
                    encoding="utf-8",
                )
                commit(repo, "fixture policy baseline")

            (repo / "core/events").mkdir(parents=True)
            (repo / "core/events/journal.rs").write_text("pub fn x() {}\n", encoding="utf-8")
            commit(repo, "one-line persistence change")

            if mode == "expanded":
                # The source change reaches the shipped persistence invariant by itself.
                bounded = run(
                    [
                        graphhelm(), "--json", "keel", "check", "--diff", "HEAD~1..HEAD",
                        "--repo", str(repo), "--card", str(repo / "card.json"),
                    ],
                    repo,
                )
                if bounded.returncode:
                    sys.stdout.buffer.write(bounded.stdout)
                    return bounded.returncode
                bounded_reply = json.loads(bounded.stdout)
                bounded_rules = {
                    finding.get("rule")
                    for finding in bounded_reply.get("data", {}).get("findings", [])
                }
                if "keel.invariant.persistence" not in bounded_rules:
                    print(
                        "expected the source commit to report the shipped persistence hit, "
                        f"got {sorted(bounded_rules)!r}",
                        file=sys.stderr,
                    )
                    return 1

                # The next commit adds the invariant path in isolation.
                (repo / "keel.yaml").write_text(
                    "version: 1\ninvariants:\n  persistence: [core/events/]\nplan:\n  reviews: {docs: 1, code: 1, user_visible: 1, invariant: 1}\n  delegation: {docs: [small, low], code: [standard, medium], user_visible: [standard, medium], invariant: [large, high]}\n  jevThresholdPercent: 80\n",
                    encoding="utf-8",
                )
                commit(repo, "configure persistence invariant")

                configured = run(
                    [
                        graphhelm(), "--json", "keel", "check", "--diff", "HEAD~1..HEAD",
                        "--repo", str(repo), "--card", str(repo / "card.json"),
                    ],
                    repo,
                )
                if configured.returncode:
                    sys.stdout.buffer.write(configured.stdout)
                    return configured.returncode
                configured_reply = json.loads(configured.stdout)
                configured_rules = {
                    finding.get("rule")
                    for finding in configured_reply.get("data", {}).get("findings", [])
                    if str(finding.get("rule", "")).startswith("keel.invariant.")
                }
                if configured_rules:
                    print(
                        "expected the policy-only commit to have no invariant finding, "
                        f"got {sorted(configured_rules)!r}",
                        file=sys.stderr,
                    )
                    return 1

            result = run(
                [
                    graphhelm(), "--json", "keel", "check", "--diff",
                    {"docs": "HEAD~3..HEAD~2", "bounded-code": "HEAD~2..HEAD~1",
                     "expanded": "HEAD~2..HEAD~1"}[mode],
                    "--repo", str(repo),
                    *( ["--card", str(repo / "card.json")] if mode == "expanded" else [] ),
                ],
                repo,
            )
            sys.stdout.buffer.write(result.stdout)
            sys.stdout.buffer.flush()
            if result.returncode:
                return result.returncode
            reply = json.loads(result.stdout)
            return 0
    except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
        print(str(error), file=sys.stderr)
        return 70


if __name__ == "__main__":
    raise SystemExit(main())
