"""Every Markdown document under docs/ is reachable from an entry point (#1248).

A document nobody links is a document nobody reads, until an agent greps it and treats a stale
claim as current. The optimisation plan's audit found 21 such orphans. This guard follows the
relative Markdown links from the three entry points (README.md, AGENTS.md, docs/INDEX.md),
transitively through every linked Markdown file, and fails naming each tracked `docs/**/*.md` the
walk never reached. It also fails on a link from a reached document to a Markdown file that does
not exist, which is what a deletion leaves behind.

`tools/reached-tests` routes any change to `docs/**/*.md`, README.md or AGENTS.md here, so the
guard runs where the change is reached rather than only when someone remembers it.
"""

from __future__ import annotations

import re
import subprocess
import tempfile
import unittest
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[2]
ENTRY_POINTS = ("README.md", "AGENTS.md", "docs/INDEX.md")

# [text](target) and [text]: target, skipping images. A target is cut at its anchor or title.
INLINE = re.compile(r"(?<!!)\[[^\]]*\]\(\s*<?([^)\s>]+)>?(?:\s+\"[^\"]*\")?\s*\)")
REFERENCE = re.compile(r"^\s*\[[^\]]+\]:\s*<?(\S+?)>?(?:\s+\"[^\"]*\")?\s*$", re.M)
FENCE = re.compile(r"^(```|~~~).*?^\1", re.M | re.S)


def tracked_markdown(root: Path) -> set[str]:
    listed = subprocess.run(
        ["git", "ls-files", "--", "docs/*.md", "docs/**/*.md"],
        cwd=root, capture_output=True, text=True, check=True,
    ).stdout.split("\n")
    return {path for path in listed if path}


def links(root: Path, document: str) -> tuple[set[str], set[str]]:
    """Repository-relative Markdown targets of `document`: (existing, missing)."""
    text = FENCE.sub("", (root / document).read_text(encoding="utf-8", errors="replace"))
    found, missing = set(), set()
    base = PurePosixPath(document).parent
    for target in INLINE.findall(text) + REFERENCE.findall(text):
        target = target.split("#", 1)[0].split("?", 1)[0]
        if not target or re.match(r"^[a-z][a-z0-9+.-]*:", target, re.I) or target.startswith("/"):
            continue
        if not target.lower().endswith(".md"):
            continue
        parts: list[str] = []
        for part in (base / target).parts:
            if part == "..":
                if not parts:
                    break
                parts.pop()
            elif part != ".":
                parts.append(part)
        else:
            resolved = "/".join(parts)
            (found if (root / resolved).is_file() else missing).add(resolved)
    return found, missing


def walk(root: Path) -> tuple[set[str], dict[str, set[str]]]:
    """Every Markdown file reachable from the entry points, and each reached file's dead links."""
    seen: set[str] = set()
    dead: dict[str, set[str]] = {}
    queue = [entry for entry in ENTRY_POINTS if (root / entry).is_file()]
    while queue:
        document = queue.pop()
        if document in seen:
            continue
        seen.add(document)
        found, missing = links(root, document)
        if missing:
            dead[document] = missing
        queue.extend(found - seen)
    return seen, dead


class DocsReachabilityTest(unittest.TestCase):
    def test_every_document_under_docs_is_reachable_from_an_entry_point(self):
        tracked = tracked_markdown(ROOT)
        # CONTROL: an empty listing (wrong cwd, wrong glob) would make the check below vacuous.
        self.assertGreater(len(tracked), 50, f"git ls-files found only {len(tracked)} docs")
        reached, _ = walk(ROOT)
        orphans = sorted(tracked - reached)
        self.assertEqual(
            orphans, [],
            "these documents are linked from nowhere reachable from README.md, AGENTS.md or "
            "docs/INDEX.md; link each from the document that owns its topic (or from docs/INDEX.md), "
            "or delete it:\n  " + "\n  ".join(orphans),
        )

    def test_no_reached_document_links_a_markdown_file_that_does_not_exist(self):
        _, dead = walk(ROOT)
        self.assertEqual(
            dead, {},
            "dead Markdown links (a deleted document is cited as `git show <sha>:<path>`, not "
            "linked):\n  " + "\n  ".join(f"{doc}: {sorted(targets)}" for doc, targets in sorted(dead.items())),
        )

    def test_the_walk_follows_links_transitively_and_reports_orphans_and_dead_links(self):
        # The instrument on a tree it can be wrong about: a chain, an orphan, a dead link, a link
        # inside a code fence (not a link), an anchor, and an image (not a document).
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "docs" / "a").mkdir(parents=True)
            (root / "README.md").write_text("[index](docs/INDEX.md)\n", encoding="utf-8")
            (root / "docs" / "INDEX.md").write_text(
                "[one](a/one.md#part)\n```\n[fenced](a/fenced.md)\n```\n![img](a/pic.md)\n",
                encoding="utf-8",
            )
            (root / "docs" / "a" / "one.md").write_text("[two](../two.md) [gone](gone.md)\n", encoding="utf-8")
            (root / "docs" / "two.md").write_text("no links\n", encoding="utf-8")
            (root / "docs" / "a" / "fenced.md").write_text("orphan\n", encoding="utf-8")
            (root / "docs" / "a" / "pic.md").write_text("orphan\n", encoding="utf-8")
            reached, dead = walk(root)
            self.assertEqual(
                reached, {"README.md", "docs/INDEX.md", "docs/a/one.md", "docs/two.md"},
                "the walk is transitive and ignores fenced code and images",
            )
            self.assertEqual(dead, {"docs/a/one.md": {"docs/a/gone.md"}})


if __name__ == "__main__":
    unittest.main()
