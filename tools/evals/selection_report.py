"""Summarize recorded skill-selection and completion observations offline."""

import json
import re
import sys
from collections import defaultdict
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
CASES = ROOT / "evals/graphhelm-development-contracts"
SKILL_LINE = re.compile(r"Use the ([A-Za-z0-9_-]+) skill\b", re.IGNORECASE)
VERDICTS = {"pass", "fail"}
RESPONSE_SHA256 = re.compile(r"[0-9a-f]{64}\Z")


class InputError(ValueError):
    pass


def case_catalog():
    """Return case id -> (kind, expected skill) from the checked-in eval corpus."""
    catalog = {}
    for prompt in sorted(CASES.glob("*/prompt.md")):
        case_id = prompt.parent.name
        match = SKILL_LINE.search(prompt.read_text(encoding="utf-8"))
        if case_id.startswith("no-skill-"):
            kind, expected = "no_skill", None
        elif case_id == "audit-before-planning":
            kind, expected = "competing_intent", "test-audit"
        elif case_id.endswith("-unprompted"):
            kind = "unprompted"
            original = CASES / case_id.removesuffix("-unprompted") / "prompt.md"
            original_match = SKILL_LINE.search(original.read_text(encoding="utf-8")) if original.is_file() else None
            expected = original_match.group(1) if original_match else None
        else:
            kind, expected = "prompted", match.group(1) if match else None
        if kind != "no_skill" and expected is None:
            raise InputError(f"cannot determine expected skill for eval case {case_id}")
        catalog[case_id] = (kind, expected)
    return catalog


def load_observations(path):
    try:
        value = json.loads(Path(path).read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise InputError(f"cannot read observations JSON: {error}") from error
    if not isinstance(value, list):
        raise InputError("observations must be a JSON array")
    catalog = case_catalog()
    for index, item in enumerate(value):
        label = f"observation {index}"
        if not isinstance(item, dict):
            raise InputError(f"{label} must be an object")
        case_id = item.get("case_id")
        if not isinstance(case_id, str) or not case_id:
            raise InputError(f"{label} has no case_id")
        if case_id not in catalog:
            raise InputError(f"unknown eval case: {case_id}")
        status = item.get("status")
        if not isinstance(status, str) or not status:
            raise InputError(f"{label} ({case_id}) has no run status")
        skills = item.get("invoked_skills")
        if not isinstance(skills, list) or any(not isinstance(skill, str) or not skill for skill in skills):
            raise InputError(f"{label} ({case_id}) invoked_skills must be an array of names")
        if status == "completed" and item.get("completion") not in ("pass", "fail"):
            raise InputError(f"{label} ({case_id}) completed run needs completion pass or fail")
        if not isinstance(item.get("rubric"), str) or not item["rubric"]:
            raise InputError(f"{label} ({case_id}) needs rubric")
        for field in ("judge", "human_label"):
            if field in item and item[field] not in VERDICTS:
                raise InputError(f"{label} ({case_id}) {field} must be pass or fail")
        has_response = "response_sha256" in item
        has_judge_id = "judge_id" in item
        if has_response != has_judge_id:
            raise InputError(f"{label} ({case_id}) response_sha256 and judge_id must occur together")
        if has_response:
            if not isinstance(item["response_sha256"], str) or not RESPONSE_SHA256.fullmatch(item["response_sha256"]):
                raise InputError(f"{label} ({case_id}) response_sha256 must be 64 lowercase hexadecimal characters")
            judge_id = item["judge_id"]
            if not isinstance(judge_id, str) or not judge_id.strip() or len(judge_id) > 128:
                raise InputError(f"{label} ({case_id}) judge_id must be a nonempty identifier of at most 128 characters")
    return value, catalog


def empty_selection():
    return {"correct_trigger": 0, "miss": 0, "false_trigger": 0,
            "correct_abstain": 0, "incomplete": 0}


def empty_completion():
    return {"pass": 0, "fail": 0, "incomplete": 0}


def build_report(observations, catalog):
    selection = defaultdict(empty_selection)
    completion = defaultdict(empty_completion)
    judges = defaultdict(lambda: {"observed": 0, "pass": 0, "fail": 0,
                                  "human_labeled": 0, "compared": 0,
                                  "false_pass": 0, "false_fail": 0,
                                  "repeat_groups": 0, "repeat_disagreement": 0,
                                  "repeat_unidentified": 0})
    repeats = defaultdict(list)

    for item in observations:
        case_id = item["case_id"]
        kind, expected = catalog[case_id]
        if item["status"] != "completed":
            selection[kind]["incomplete"] += 1
            completion[kind]["incomplete"] += 1
            continue

        invoked = set(item["invoked_skills"])
        if expected is None:
            bucket = "false_trigger" if invoked else "correct_abstain"
        else:
            bucket = "correct_trigger" if expected in invoked else "miss"
        selection[kind][bucket] += 1
        completion[kind][item["completion"]] += 1

        rubric = item["rubric"]
        if "judge" in item:
            judges[rubric]["observed"] += 1
            judges[rubric][item["judge"]] += 1
            if "response_sha256" in item:
                repeats[(case_id, rubric, item["response_sha256"], item["judge_id"])].append(item["judge"])
            else:
                judges[rubric]["repeat_unidentified"] += 1
        if "human_label" in item:
            counts = judges[rubric]
            counts["human_labeled"] += 1
            if "judge" in item:
                counts["compared"] += 1
                if item["judge"] == "pass" and item["human_label"] == "fail":
                    counts["false_pass"] += 1
                if item["judge"] == "fail" and item["human_label"] == "pass":
                    counts["false_fail"] += 1

    for (_, rubric, _, _), verdicts in repeats.items():
        if len(verdicts) > 1:
            judges[rubric]["repeat_groups"] += 1
            if len(set(verdicts)) > 1:
                judges[rubric]["repeat_disagreement"] += 1

    judge_report = {}
    for rubric in sorted(set(judges) | {item["rubric"] for item in observations}):
        counts = judges[rubric]
        if counts["compared"]:
            judge_report[rubric] = {"status": "comparisons_available", **counts}
        else:
            judge_report[rubric] = {"status": "unqualified", "observed": counts["observed"],
                                    "pass": counts["pass"], "fail": counts["fail"],
                                    "human_labeled": counts["human_labeled"],
                                    "compared": 0,
                                    "false_pass": None, "false_fail": None,
                                    "repeat_groups": counts["repeat_groups"],
                                    "repeat_disagreement": counts["repeat_disagreement"],
                                    "repeat_unidentified": counts["repeat_unidentified"]}

    return {
        "selection": {kind: selection[kind] for kind in sorted(selection)},
        "completion": {kind: completion[kind] for kind in sorted(completion)},
        "judges": {"by_rubric": judge_report},
    }


def main(argv=None):
    args = sys.argv[1:] if argv is None else argv
    if len(args) != 1:
        print("usage: python tools/evals/selection_report.py <observations.json>", file=sys.stderr)
        return 2
    try:
        observations, catalog = load_observations(args[0])
        report = build_report(observations, catalog)
    except InputError as error:
        print(str(error), file=sys.stderr)
        return 2
    print(json.dumps(report, sort_keys=True, separators=(",", ":"), ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
