"""Source guard: every child a Python tool starts runs hidden, never in a new console window.

Owner rule: on Windows each spawn passes ``creationflags=NO_WINDOW`` (``CREATE_NO_WINDOW`` there,
0 elsewhere), so agents running tools see no flashing consoles. A spawn site without it fails here.
"""
import ast
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parent.parent
SPAWN = {"Popen", "run", "call", "check_output", "check_call"}
FORBIDDEN = {("os", "system"), ("os", "popen"), ("os", "startfile"),
             ("asyncio", "create_subprocess_exec"), ("asyncio", "create_subprocess_shell")}


def offenders(root: Path) -> list[str]:
    found = []
    for path in sorted(root.rglob("*.py")):
        if "node_modules" in path.parts:
            continue
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        for node in ast.walk(tree):
            if not isinstance(node, ast.Call) or not isinstance(node.func, ast.Attribute):
                continue
            owner = node.func.value
            if not isinstance(owner, ast.Name):
                continue
            where = f"{path.relative_to(root)}:{node.lineno}"
            if (owner.id, node.func.attr) in FORBIDDEN:
                found.append(f"{where} {owner.id}.{node.func.attr} cannot hide its window")
            elif owner.id == "subprocess" and node.func.attr in SPAWN:
                if not any(keyword.arg == "creationflags" for keyword in node.keywords):
                    found.append(f"{where} subprocess.{node.func.attr} without creationflags=NO_WINDOW")
    return found


class NoConsoleWindow(unittest.TestCase):
    def test_every_tool_spawn_site_hides_its_console_window(self):
        self.assertEqual(offenders(TOOLS), [])

    def test_the_guard_catches_a_bare_spawn(self):
        import tempfile
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            (root / "bad.py").write_text(
                "import subprocess\nsubprocess.run(['git'])\n"
                "subprocess.run(['git'], creationflags=0)\n", encoding="utf-8")
            self.assertEqual(offenders(root), ["bad.py:2 subprocess.run without creationflags=NO_WINDOW"])


if __name__ == "__main__":
    unittest.main()
