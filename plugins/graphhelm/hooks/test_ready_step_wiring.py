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


if __name__ == "__main__":
    unittest.main()
