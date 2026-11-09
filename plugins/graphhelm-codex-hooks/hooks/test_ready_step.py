"""#2697: the Codex copy of lane_stop_hook keeps the ready-step path working (a name defined twice
once shadowed `_step_key` and crashed the Stop hook of any lane with an assigned review)."""
import ast
from pathlib import Path
import sys
import unittest

HERE = Path(__file__).resolve().parent
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))

import lane_stop_hook as hook  # noqa: E402


class CodexReadyStep(unittest.TestCase):
    def test_a_ready_step_gives_a_reason_once(self):
        state = hook.new_turn_state()
        ready = [{"issue": 2678, "step": "review", "head": "abc123", "reason": "assigned review of PR 2691"}]
        first = hook.ready_step_reason(state, ready)
        self.assertIn("assigned review of PR 2691", first)
        self.assertIsNone(hook.ready_step_reason(state, ready))

    def test_no_function_is_defined_twice_at_module_level(self):
        tree = ast.parse((HERE / "lane_stop_hook.py").read_text(encoding="utf-8"))
        names = [node.name for node in tree.body if isinstance(node, ast.FunctionDef)]
        self.assertEqual([name for name in set(names) if names.count(name) > 1], [])


if __name__ == "__main__":
    unittest.main()
