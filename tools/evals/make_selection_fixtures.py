"""Regenerate the recorded selection-grader fixtures (synthetic transcripts, no model involved).

usage: python tools/evals/make_selection_fixtures.py
Writes tools/evals/fixtures/selection/*.jsonl and labels.json. The checked-in output is the
reviewed artifact; this script only documents how each transcript was shaped.
"""
import json
from pathlib import Path

OUT = Path(__file__).resolve().parent / "fixtures/selection"


def assistant(*content):
    return json.dumps({"type": "assistant", "message": {"role": "assistant", "content": list(content)}})


def user(*content):
    return json.dumps({"type": "user", "message": {"role": "user", "content": list(content)}})


def skill(name):
    return {"type": "tool_use", "id": "t1", "name": "Skill", "input": {"skill": name}}


def text(value):
    return {"type": "text", "text": value}


READ = {"type": "tool_use", "id": "t2", "name": "Read", "input": {"file_path": "x.md"}}
OTHER_TOOL_SKILL_FIELD = {"type": "tool_use", "id": "t3", "name": "Read", "input": {"skill": "graphhelm-resume"}}

FIXTURES = {
    "pos-plugin-prefixed": [user(text("q")), assistant(skill("graphhelm:graphhelm-resume")), assistant(text("done"))],
    "pos-bare-name": [assistant(skill("graphhelm-resume")), assistant(text("done"))],
    "pos-after-other-tool": [assistant(READ), assistant(text("reading"), skill("graphhelm:graphhelm-resume"))],
    "neg-text-mention-only": [assistant(text("I would use the graphhelm-resume skill here."))],
    "neg-tool-result-mention": [
        assistant(READ),
        user({"type": "tool_result", "tool_use_id": "t2", "content": "graphhelm-resume skill"}),
        assistant(text("ok")),
    ],
    "neg-other-skill": [assistant(skill("graphhelm:graphhelm-setup")), assistant(text("done"))],
    "neg-other-tool-with-skill-field": [assistant(OTHER_TOOL_SKILL_FIELD)],
    "neg-longer-skill-name": [assistant(skill("graphhelm:graphhelm-resume-extra")), assistant(text("done"))],
    "neg-no-calls": [assistant(text("Next: ask for the new head."))],
    "neg-malformed-line": ["not json {", assistant(text("plain answer"))],
}


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    labels = {}
    for name, lines in FIXTURES.items():
        (OUT / f"{name}.jsonl").write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
        labels[name] = "selected" if name.startswith("pos-") else "not_selected"
    meta = {"synthetic": True, "expected_skill": "graphhelm-resume", "labels": labels}
    (OUT / "labels.json").write_text(json.dumps(meta, indent=2) + "\n", encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
