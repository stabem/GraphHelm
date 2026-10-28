"""Contracts of the #1476 cost harness on a throwaway git repository: the reuse fraction counts a
closure-moving commit as invalidate and a UI commit as reuse, and `spread` reports median and range."""
import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import test_evidence_key as base  # noqa: E402

SPEC = importlib.util.spec_from_file_location("measure_cost", HERE / "measure_cost.py")
mc = importlib.util.module_from_spec(SPEC)
sys.modules["measure_cost"] = mc
SPEC.loader.exec_module(mc)


class MeasureCost(unittest.TestCase):
    def test_spread_is_median_and_range(self):
        self.assertEqual(mc.spread([3.0, 1.0, 2.0]), {"n": 3, "median_s": 2.0, "min_s": 1.0, "max_s": 3.0})
        self.assertEqual(mc.spread([]), {"n": 0})

    def test_the_fraction_calls_a_closure_commit_invalidate_and_a_ui_commit_reuse(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            base.sh(repo, "init", "-q")
            base.sh(repo, "config", "core.autocrlf", "false")
            base.commit(repo, base.FILES, "base")
            base.commit(repo, {"studio/src/App.tsx": "export const App = () => 1;\n"}, "ui")
            base.commit(repo, {"core/events/src/local.rs": "fn committed_events_for_idempotency() -> bool { false }\n"}, "dedup")
            report = mc.reuse_fraction(repo, "HEAD")
        decisions = [row["decision"] for row in report["prs"]]
        self.assertEqual(decisions[:2], ["invalidate", "reuse"])
        self.assertEqual(report["counts"]["reuse"], 1)


if __name__ == "__main__":
    unittest.main()
