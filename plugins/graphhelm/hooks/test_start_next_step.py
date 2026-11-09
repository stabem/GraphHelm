import hashlib, sys, tempfile, unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import session_hook

STEP = {"issue": 9, "step": "review", "head": "abcdef0123456789", "reason": "assigned review of PR 5"}


class NextStepText(unittest.TestCase):
    def root(self, with_skill=True, with_table=True):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = Path(tmp.name)
        if with_skill:
            skill = root / session_hook.STEP_SKILLS["review"]
            skill.parent.mkdir(parents=True)
            skill.write_text("x", encoding="utf-8")
        if with_table:
            table = root / session_hook.TRANSITION_TABLE
            table.parent.mkdir(parents=True)
            table.write_bytes(b"table")
        return root

    def test_names_step_existing_skill_and_table_version(self):
        text = session_hook.next_step_text([STEP], self.root())
        self.assertIn("issue 9", text)
        self.assertIn("assigned review of PR 5", text)
        self.assertIn(session_hook.STEP_SKILLS["review"], text)
        self.assertNotIn("MISSING", text)
        self.assertIn("table " + hashlib.sha256(b"table").hexdigest()[:12], text)

    def test_a_skill_not_under_the_cwd_is_named_on_origin_main_not_missing(self):
        text = session_hook.next_step_text([STEP], self.root(with_skill=False))
        self.assertIn("origin/main:" + session_hook.STEP_SKILLS["review"], text)
        self.assertNotIn("MISSING", text)

    def test_missing_table_is_named_missing(self):
        self.assertIn("table missing", session_hook.next_step_text([STEP], self.root(with_table=False)))

    def test_no_ready_step_adds_nothing(self):
        self.assertEqual(session_hook.next_step_text([], self.root()), "")

    def test_text_stays_short(self):
        self.assertLessEqual(len(session_hook.next_step_text([STEP], self.root())), 400)

    def test_lookup_failure_adds_nothing(self):
        with mock.patch.object(session_hook, "request", side_effect=OSError("down")):
            self.assertEqual(session_hook.start_next_step({"cwd": "/x"}, ("e", "t", "http://127.0.0.1:1", "o", None)), "")


if __name__ == "__main__":
    unittest.main()
