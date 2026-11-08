"""#2643 slice 1: the ready-step Stop block fires at most once per step key, and at most three
times per session turn. Pure logic; no Runtime, no subprocess.
"""
import unittest

import lane_step_guard as guard


class ReadyStepGuard(unittest.TestCase):
    def test_a_ready_step_blocks_once_then_is_recorded_as_injected(self):
        state = guard.new_state()
        key = guard.step_key(issue=2643, step="review", head="abc123")
        self.assertTrue(guard.should_block(state, key), "the first stop with a ready step blocks")
        guard.record_block(state, key)
        self.assertFalse(guard.should_block(state, key), "the same step on the next stop does not block again")

    def test_a_different_head_of_the_same_step_blocks_again(self):
        state = guard.new_state()
        guard.record_block(state, guard.step_key(issue=2643, step="review", head="abc123"))
        new = guard.step_key(issue=2643, step="review", head="def456")
        self.assertTrue(guard.should_block(state, new), "a new head is a new step")

    def test_three_blocks_per_turn_then_the_stop_is_allowed(self):
        state = guard.new_state()
        for n in range(3):
            key = guard.step_key(issue=2600 + n, step="review", head="h")
            self.assertTrue(guard.should_block(state, key), f"block {n + 1} of 3")
            guard.record_block(state, key)
        fourth = guard.step_key(issue=2699, step="review", head="h")
        self.assertFalse(guard.should_block(state, fourth), "a fourth block in one turn is refused")


if __name__ == "__main__":
    unittest.main()
