"""No rule sentence is stated in two instruction files (B3, the instruction diet).

Every Claude and Codex session loads AGENTS.md, and lanes load DELIVERY.md and LANES.md beside it.
A rule written in two of them costs tokens twice and, worse, drifts: one copy is edited and the
other keeps the old rule, so two sessions read two different rules. This guard splits the four
instruction files into sentences (code fences removed, whitespace and emphasis normalised) and
fails naming every sentence of eight words or more that appears in two of them. Move a rule to the
one file that owns it and point at it from the others. Cost: four file reads, no subprocess.
"""

from __future__ import annotations

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FILES = ("AGENTS.md", "CLAUDE.md", "docs/process/DELIVERY.md", "docs/process/LANES.md")
MIN_WORDS = 8

FENCE = re.compile(r"^(```|~~~).*?^\1", re.M | re.S)
INDENTED = re.compile(r"^(?: {4,}|\t)\S.*$", re.M)


def sentences(text: str) -> set[str]:
    """Normalised sentences of `text` with at least MIN_WORDS words."""
    text = INDENTED.sub("", FENCE.sub("", text))
    found = set()
    for part in re.split(r"(?<=[.!?])\s+|\n\s*\n|\n\s*(?:[-*]|\d+\.)\s+|\n#+\s", text):
        part = re.sub(r"^\s*(?:[-*]|\d+\.|#+|>)\s+", "", part)
        normal = re.sub(r"\s+", " ", re.sub(r"[*_]{1,2}", "", part)).strip().lower().rstrip(".")
        if len(normal.split(" ")) >= MIN_WORDS:
            found.add(normal)
    return found


def duplicates(texts: dict[str, str]) -> dict[str, list[str]]:
    """Each sentence found in two or more of `texts`, with the files that hold it."""
    seen: dict[str, list[str]] = {}
    for name, text in texts.items():
        for sentence in sentences(text):
            seen.setdefault(sentence, []).append(name)
    return {sentence: names for sentence, names in seen.items() if len(names) > 1}


class NoDuplicateRules(unittest.TestCase):
    def test_a_sentence_in_two_files_is_reported(self):
        rule = "Never write a closing keyword next to an issue you do not intend to close."
        found = duplicates({"a.md": f"# A\n\n- {rule}\n", "b.md": f"Intro line here.\n\n{rule} More.\n", "c.md": "Unrelated."})
        self.assertEqual(found, {rule.lower().rstrip("."): ["a.md", "b.md"]})

    def test_code_fences_and_short_sentences_are_not_rules(self):
        block = "```\ncargo +1.97.1 fmt --all -- --check and some more words here\n```\n"
        self.assertEqual(duplicates({"a.md": block + "See below.", "b.md": block + "See below."}), {})

    def test_no_rule_sentence_appears_in_two_instruction_files(self):
        texts = {name: (ROOT / name).read_text(encoding="utf-8") for name in FILES}
        found = duplicates(texts)
        self.assertEqual(found, {}, "\n".join(f"{names}: {sentence}" for sentence, names in sorted(found.items())))


if __name__ == "__main__":
    unittest.main()
