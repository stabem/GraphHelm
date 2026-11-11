"""#2643 slice 1: the Stop hook blocks on a ready step, and the guard keeps it to once per step.

Pure: the step lookup is injected, so no Runtime is needed.
"""
import unittest

import lane_stop_hook as hook
import lane_step_guard as guard


class ReadyStepWiring(unittest.TestCase):
    def test_a_ready_step_blocks_once_then_stops(self):
        state = guard.new_state()
        ready = [{"issue": 2643, "step": "review", "head": "abc123", "reason": "assigned review"}]
        first = hook.ready_step_reason(state, ready)
        self.assertIsNotNone(first, "the first stop with a ready step blocks")
        self.assertIn("assigned review", first)
        self.assertIsNone(hook.ready_step_reason(state, ready), "the same step does not block twice")

    def test_no_ready_step_never_blocks(self):
        self.assertIsNone(hook.ready_step_reason(guard.new_state(), []))


class EvaluateDrivesTheReadyStep(unittest.TestCase):
    """#2643: evaluate() is the stop path; a ready step reaches it, once per turn state."""

    def test_evaluate_returns_the_ready_step_once_per_turn_state(self):
        turn = hook.new_turn_state()
        ready = [{"issue": 2643, "step": "review", "head": "abc123", "reason": "assigned review"}]
        payload = {"cwd": "", "stop_hook_active": False}
        first = hook.evaluate(payload, ready=ready, turn_state=turn)
        self.assertIsNotNone(first, "the stop path returns the ready step")
        self.assertIn("assigned review", first)
        second = hook.evaluate(payload, ready=ready, turn_state=turn)
        self.assertNotIn("assigned review", second or "", "the same step does not block again in this turn")

    def test_a_fresh_turn_state_allows_the_step_again(self):
        ready = [{"issue": 2643, "step": "review", "head": "abc123", "reason": "assigned review"}]
        payload = {"cwd": "", "stop_hook_active": False}
        self.assertIn("assigned review", hook.evaluate(payload, ready=ready, turn_state=hook.new_turn_state()))
        self.assertIn("assigned review", hook.evaluate(payload, ready=ready, turn_state=hook.new_turn_state()),
                      "a new turn starts with no injected steps")


if __name__ == "__main__":
    unittest.main()
