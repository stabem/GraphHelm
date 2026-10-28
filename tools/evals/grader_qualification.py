"""Check a deterministic selection grader against recorded transcripts with known labels (#184).

The grader is `run_selection.invoked_skills`. A grader is trusted only after it is shown to accept
a known positive and reject known negatives; a corpus with no positive or no negative cannot
qualify anything. Offline: reads checked-in fixtures, calls no model.

usage: python tools/evals/grader_qualification.py   (qualifies the real grader; exit 1 if not)
"""
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import run_selection  # noqa: E402

FIXTURES = Path(__file__).resolve().parent / "fixtures/selection"


def load_fixtures(directory=FIXTURES):
    """Return (expected_skill, [(id, label, transcript_lines)]) from labels.json and *.jsonl."""
    meta = json.loads((directory / "labels.json").read_text(encoding="utf-8"))
    rows = []
    for fixture_id, label in sorted(meta["labels"].items()):
        lines = (directory / f"{fixture_id}.jsonl").read_text(encoding="utf-8").splitlines()
        rows.append((fixture_id, label, lines))
    return meta["expected_skill"], rows


def qualify(grader, expected_skill, rows):
    """Count disagreements. `false_pass` = grader says selected where the label is not_selected."""
    false_pass, false_fail = [], []
    positives = sum(1 for _, label, _ in rows if label == "selected")
    negatives = sum(1 for _, label, _ in rows if label == "not_selected")
    for fixture_id, label, lines in rows:
        says_selected = expected_skill in grader(lines)
        if says_selected and label == "not_selected":
            false_pass.append(fixture_id)
        elif not says_selected and label == "selected":
            false_fail.append(fixture_id)
    qualified = positives > 0 and negatives > 0 and not false_pass and not false_fail
    return {"positives": positives, "negatives": negatives, "false_pass": false_pass,
            "false_fail": false_fail, "qualified": qualified}


def main():
    expected, rows = load_fixtures()
    result = qualify(run_selection.invoked_skills, expected, rows)
    print(json.dumps(result, indent=2))
    return 0 if result["qualified"] else 1


if __name__ == "__main__":
    sys.exit(main())
