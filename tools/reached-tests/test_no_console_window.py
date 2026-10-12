"""Source guard: every child a repository Python script (tools, plugin hooks, tests) starts runs hidden, never in a new console window.

Owner rule: on Windows each spawn passes ``creationflags=NO_WINDOW`` (``CREATE_NO_WINDOW`` there,
0 elsewhere), so agents running tools see no flashing consoles. A spawn site without it fails here.
"""
import ast
import unittest
import warnings
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
SKIP = {"node_modules", "target", ".git"}
SPAWN = {"Popen", "run", "call", "check_output", "check_call"}
# These cannot take creationflags, so they cannot hide a window: forbidden outright.
FORBIDDEN = {("subprocess", "getoutput"), ("subprocess", "getstatusoutput"), ("os", "system"),
             ("os", "popen"), ("os", "startfile"), ("asyncio", "create_subprocess_exec"),
             ("asyncio", "create_subprocess_shell")}
SPAWN_PREFIXES = {"os": ("spawn", "exec", "posix_spawn")}


def _aliases(tree: ast.AST) -> tuple[dict[str, str], dict[str, tuple[str, str]]]:
    """Module aliases (``import subprocess as sp``) and imported functions (``from subprocess
    import run as r``) for the modules the guard watches."""
    modules: dict[str, str] = {}
    functions: dict[str, tuple[str, str]] = {}
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                if alias.name in {"subprocess", "os", "asyncio"}:
                    modules[alias.asname or alias.name] = alias.name
        elif isinstance(node, ast.ImportFrom) and node.module in {"subprocess", "os", "asyncio"}:
            for alias in node.names:
                functions[alias.asname or alias.name] = (node.module, alias.name)
    return modules, functions


def _target(node: ast.Call, modules, functions) -> tuple[str, str] | None:
    func = node.func
    if isinstance(func, ast.Attribute) and isinstance(func.value, ast.Name):
        module = modules.get(func.value.id, func.value.id if func.value.id in {"subprocess", "os", "asyncio"} else None)
        return (module, func.attr) if module else None
    if isinstance(func, ast.Name) and func.id in functions:
        return functions[func.id]
    return None


def offenders(root: Path) -> list[str]:
    found = []
    for path in sorted(root.rglob("*.py")):
        if SKIP & set(path.relative_to(root).parts):
            continue
        with warnings.catch_warnings():
            warnings.simplefilter("ignore", SyntaxWarning)
            tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        modules, functions = _aliases(tree)
        for node in ast.walk(tree):
            if not isinstance(node, ast.Call):
                continue
            target = _target(node, modules, functions)
            if target is None:
                continue
            module, name = target
            where = f"{path.relative_to(root)}:{node.lineno}"
            if target in FORBIDDEN or name.startswith(SPAWN_PREFIXES.get(module, ("\0",))):
                found.append(f"{where} {module}.{name} cannot hide its window")
            elif module == "subprocess" and name in SPAWN:
                if not any(keyword.arg == "creationflags" for keyword in node.keywords):
                    found.append(f"{where} subprocess.{name} without creationflags=NO_WINDOW")
    return found


class NoConsoleWindow(unittest.TestCase):
    def test_every_tool_spawn_site_hides_its_console_window(self):
        self.assertEqual(offenders(REPO), [])

    def test_the_guard_catches_a_bare_spawn(self):
        import tempfile
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            (root / "bad.py").write_text(
                "import subprocess\nsubprocess.run(['git'])\n"
                "subprocess.run(['git'], creationflags=0)\n", encoding="utf-8")
            (root / "alias.py").write_text(
                "import subprocess as sp\nfrom subprocess import Popen as P, check_output\nimport os\n"
                "sp.run(['git'])\nP(['git'])\ncheck_output(['git'])\nos.spawnl(0, 'git')\n"
                "sp.getoutput('git')\nsp.run(['git'], creationflags=0)\n", encoding="utf-8")
            self.assertEqual(offenders(root), [
                "alias.py:4 subprocess.run without creationflags=NO_WINDOW",
                "alias.py:5 subprocess.Popen without creationflags=NO_WINDOW",
                "alias.py:6 subprocess.check_output without creationflags=NO_WINDOW",
                "alias.py:7 os.spawnl cannot hide its window",
                "alias.py:8 subprocess.getoutput cannot hide its window",
                "bad.py:2 subprocess.run without creationflags=NO_WINDOW",
            ])


if __name__ == "__main__":
    unittest.main()
