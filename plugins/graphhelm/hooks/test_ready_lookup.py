"""#2643 slice 3: the Stop hook reads this lane's ready step from the tasks projection.

The projection shape is the Runtime's GET /v1/executions/<id>/tasks `data`: each task carries
taskId, issue, step, lane, currentReviewer, assignedReviewers, headSha, mergeSha, blockedBy.
No Runtime is started: `request` and `binding` are replaced.
"""
import io
import json
import unittest
from contextlib import redirect_stdout
from unittest import mock

import lane_stop_hook as hook

LANE = "openbot-sasuke"


def task(**fields):
    base = {"taskId": "issue-1", "key": "issue-1#pr-2", "issue": 1, "pr": 2, "step": "review",
            "lane": "openbot-itachi", "currentReviewer": None, "assignedReviewers": [],
            "headSha": "a" * 40, "mergeSha": None, "blockedBy": None}
    base.update(fields)
    return base


class ReadySteps(unittest.TestCase):
    def test_an_assigned_review_is_a_ready_step(self):
        data = {"tasks": [task(issue=2655, assignedReviewers=[LANE], currentReviewer=LANE)]}
        steps = hook.ready_steps(data, LANE)
        self.assertEqual(len(steps), 1)
        self.assertEqual((steps[0]["issue"], steps[0]["step"], steps[0]["head"]), (2655, "review", "a" * 40))
        self.assertIn("assigned review", steps[0]["reason"])

    def test_a_review_assigned_to_someone_else_is_not_ready(self):
        data = {"tasks": [task(assignedReviewers=["openbot-neji"], currentReviewer="openbot-neji")]}
        self.assertEqual(hook.ready_steps(data, LANE), [])

    def test_a_reviewer_name_without_the_lane_prefix_still_matches(self):
        data = {"tasks": [task(assignedReviewers=["Sasuke"], currentReviewer="Sasuke")]}
        self.assertEqual(len(hook.ready_steps(data, LANE)), 1)

    def test_a_block_on_my_own_pr_is_a_fix_step(self):
        data = {"tasks": [task(lane=LANE, step="fix", blockedBy={"reviewer": "openbot-neji"})]}
        steps = hook.ready_steps(data, LANE)
        self.assertEqual([s["step"] for s in steps], ["fix"])
        self.assertIn("BLOCK", steps[0]["reason"])

    def test_a_merged_task_is_never_ready(self):
        data = {"tasks": [task(assignedReviewers=[LANE], mergeSha="b" * 40)]}
        self.assertEqual(hook.ready_steps(data, LANE), [])


class LookupReady(unittest.TestCase):
    bound = ("graphhelm-openbot-ops", "token.file", "http://127.0.0.1:8793", "origin", "node")

    def test_the_lookup_reads_the_tasks_endpoint_with_the_lanes_own_token(self):
        reply = {"data": {"tasks": [task(assignedReviewers=[LANE])]}}
        with mock.patch.object(hook, "binding", return_value=self.bound), \
                mock.patch.object(hook, "token_from_file", return_value="secret") as tok, \
                mock.patch.object(hook, "request", return_value=reply) as req, \
                mock.patch.dict("os.environ", {"GRAPHHELM_ACTOR": LANE}):
            steps = hook.lookup_ready({"cwd": ""})
        tok.assert_called_once_with("token.file")
        self.assertEqual(req.call_args.args[0], "http://127.0.0.1:8793/v1/executions/graphhelm-openbot-ops/tasks")
        self.assertEqual(req.call_args.args[1], "secret")
        self.assertEqual(len(steps), 1)

    def test_any_failure_allows_the_stop(self):
        with mock.patch.object(hook, "binding", return_value=self.bound), \
                mock.patch.object(hook, "token_from_file", side_effect=OSError("gone")), \
                mock.patch.dict("os.environ", {"GRAPHHELM_ACTOR": LANE}):
            self.assertEqual(hook.lookup_ready({"cwd": ""}), [])


class MainUsesTheLookup(unittest.TestCase):
    def test_main_blocks_the_stop_for_a_lane_with_an_assigned_review(self):
        ready = [{"issue": 2655, "step": "review", "head": "a" * 40, "reason": "assigned review"}]
        out = io.StringIO()
        with mock.patch.object(hook, "hook_input", return_value={"cwd": "", "stop_hook_active": False}), \
                mock.patch.object(hook, "binding", return_value=None), \
                mock.patch.object(hook, "lookup_ready", return_value=ready), \
                mock.patch("sys.argv", ["lane_stop_hook.py"]), redirect_stdout(out):
            code = hook.main()
        self.assertEqual(code, 0)
        decision = json.loads(out.getvalue())
        self.assertEqual(decision["decision"], "block")
        self.assertIn("assigned review", decision["reason"])


if __name__ == "__main__":
    unittest.main()
