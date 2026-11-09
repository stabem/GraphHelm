"""#2697: a module-level function defined twice silently shadows the first (the second `_step_key`
crashed the Stop hook of a lane with an assigned review). Both hook sources are checked."""
import ast
from pathlib import Path
import unittest

HERE = Path(__file__).resolve().parent


class NoShadowedFunctions(unittest.TestCase):
    def test_no_hook_defines_a_function_twice(self):
        for path in (HERE / "lane_stop_hook.py", HERE / "session_hook.py"):
            tree = ast.parse(path.read_text(encoding="utf-8"))
            names = [node.name for node in tree.body if isinstance(node, ast.FunctionDef)]
            self.assertEqual([name for name in set(names) if names.count(name) > 1], [], path.name)


if __name__ == "__main__":
    unittest.main()
