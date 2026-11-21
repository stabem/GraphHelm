"""Table test for the Keel review model warning (#3000).

Cost: pure functions, a temp transcript and one hook-command subprocess; offline, no host needed.
"""
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import keel_review_model as review_model  # noqa: E402

NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
NEW = "graphhelm-development-contracts:blind-review"


def skill_input(skill, **extra):
    return {"hook_event_name": "PreToolUse", "tool_name": "Skill", "tool_input": {"skill": skill}, **extra}


def transcript(directory, *models):
    path = Path(directory) / "t.jsonl"
    lines = [json.dumps({"type": "user", "message": {"content": "x"}})]
    lines += [json.dumps({"type": "assistant", "message": {"model": m, "content": []}}) for m in models]
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return str(path)


class ReviewModelWarning(unittest.TestCase):
    def test_middle_and_cheap_models_are_warned_on_every_review_skill(self):
        for skill in ("keel:keel-review", "keel-review", NEW, "blind-review"):
            for model in ("claude-sonnet-5-5", "claude-haiku-5"):
                text = review_model.advice(skill_input(skill, transcript_path="p"), model_of=lambda _p, m=model: m)
                self.assertIn(model, text, (skill, model))
                self.assertIn("keel:keel-reviewer", text)

    def test_strongest_or_unknown_model_is_silent(self):
        for model in ("claude-opus-5-5", "claude-fable-5-2", "something-new", None):
            self.assertIsNone(review_model.advice(skill_input("keel:keel-review", transcript_path="p"),
                                                  model_of=lambda _p, m=model: m), model)

    def test_other_skills_and_subagents_are_silent(self):
        weak = lambda _p: "claude-sonnet-5-5"  # noqa: E731
        self.assertIsNone(review_model.advice(skill_input("keel:keel-exec", transcript_path="p"), model_of=weak))
        self.assertIsNone(review_model.advice(skill_input("keel:keel-review", transcript_path="p", agent_id="a1"),
                                              model_of=weak))
        self.assertIsNone(review_model.advice("not a dict", model_of=weak))

    def test_old_skill_is_pointed_at_the_current_one_on_any_model(self):
        for model in ("claude-opus-5-5", "claude-sonnet-5-5"):
            text = review_model.advice(skill_input(NEW, transcript_path="p"), model_of=lambda _p, m=model: m)
            self.assertIn("`keel:keel-review` (keel/KEEL_REVIEW.md) rules", text)
        current = review_model.advice(skill_input("keel:keel-review", transcript_path="p"),
                                      model_of=lambda _p: "claude-opus-5-5")
        self.assertIsNone(current)

    def test_model_is_read_from_the_newest_assistant_entry(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = transcript(tmp, "claude-opus-5-5", "claude-sonnet-5-5")
            self.assertEqual(review_model.last_model(path), "claude-sonnet-5-5")
            with open(path, "a", encoding="utf-8") as handle:
                handle.write("not json\n" + json.dumps({"type": "user"}) + "\n")
            self.assertEqual(review_model.last_model(path), "claude-sonnet-5-5")
            with open(path, "a", encoding="utf-8") as handle:
                handle.write(json.dumps({"type": "api-request", "params": {"model": "claude-haiku-5"}}) + "\n")
                handle.write(json.dumps({"type": "assistant", "isSidechain": True,
                                         "message": {"model": "claude-opus-5-5"}}) + "\n")
                handle.write(json.dumps({"type": "assistant", "message": {"model": "<synthetic>"}}) + "\n")
            self.assertEqual(review_model.last_model(path), "claude-sonnet-5-5")

    def test_first_turn_session_with_only_an_api_request_entry_is_read(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "t.jsonl"
            path.write_text(json.dumps({"type": "api-request", "params": {"model": "claude-sonnet-5-5"}}) + "\n",
                            encoding="utf-8")
            self.assertEqual(review_model.last_model(str(path)), "claude-sonnet-5-5")

    def test_hook_process_prints_context_for_a_weak_session_and_nothing_otherwise(self):
        with tempfile.TemporaryDirectory() as tmp:
            for model, expect_warning in (("claude-sonnet-5-5", True), ("claude-opus-5-5", False)):
                payload = json.dumps(skill_input("keel:keel-review", transcript_path=transcript(tmp, model)))
                done = subprocess.run([sys.executable, str(HERE / "keel_review_model.py")], input=payload,
                                      capture_output=True, text=True, timeout=30, creationflags=NO_WINDOW)
                self.assertEqual(done.returncode, 0)
                if expect_warning:
                    out = json.loads(done.stdout)["hookSpecificOutput"]
                    self.assertEqual(out["hookEventName"], "PreToolUse")
                    self.assertIn(model, out["additionalContext"])
                else:
                    self.assertEqual(done.stdout, "")
            garbage = subprocess.run([sys.executable, str(HERE / "keel_review_model.py")], input="{",
                                     capture_output=True, text=True, timeout=30, creationflags=NO_WINDOW)
            self.assertEqual((garbage.returncode, garbage.stdout), (0, ""))

    def test_hooks_json_runs_it_on_the_skill_tool(self):
        hooks = json.loads((HERE / "hooks.json").read_text(encoding="utf-8"))["hooks"]["PreToolUse"]
        entry = [h for h in hooks if h["matcher"] == "Skill"]
        self.assertEqual(len(entry), 1)
        self.assertTrue(entry[0]["hooks"][0]["args"][-1].endswith("/hooks/keel_review_model.py"))


if __name__ == "__main__":
    unittest.main()
