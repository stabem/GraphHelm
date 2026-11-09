"""#2643 slice 2: a new review or verdict for this lane touches the lane's waiter ring, which the
rewake reads by mtime. Pure file behaviour; no Runtime.
"""
import os
import tempfile
import time
import unittest
from pathlib import Path

import lane_stop_hook as hook


class RingWrite(unittest.TestCase):
    def test_ring_touch_moves_the_mtime_the_rewake_reads(self):
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp) / "state.json"
            rang = base.with_suffix(".waiter.rang")
            hook.ring_lane(base)
            first = rang.stat().st_mtime
            time.sleep(0.02)
            os.utime(rang, (first - 5, first - 5))
            hook.ring_lane(base)
            self.assertGreater(rang.stat().st_mtime, first - 5, "the ring moves on each ring")

    def test_ring_is_created_when_absent(self):
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp) / "state.json"
            hook.ring_lane(base)
            self.assertTrue(base.with_suffix(".waiter.rang").exists())


if __name__ == "__main__":
    unittest.main()
