"""py-sessionend-cap: Codex's SessionEnd hook declares at most the three seconds Codex enforces, the
end acknowledgement's socket wait on the Codex host fits inside that cap, and Claude's own
SessionEnd budget is left as it was. The known fix's test file (98dbd590) runs green against the
checkout.

The socket wait is read from the `/signal` call's `timeout` argument in `_end_impl` and evaluated
with `host = "codex"`: literals, module or local constants, conditionals on the host, arithmetic
and min/max resolve; anything else fails with the expression named. (#139: a literal-only regex
failed `timeout=CODEX_END_WAIT if host == "codex" else 3.0`.)"""
import ast
import json
import operator

from _lib import fail, hidden_unittest, ok, read

FIX = "98dbd5904248c1983cedc270149cc6d07310116f"
_BIN = {ast.Add: operator.add, ast.Sub: operator.sub, ast.Mult: operator.mul, ast.Div: operator.truediv}
_CMP = {ast.Eq: operator.eq, ast.NotEq: operator.ne, ast.Lt: operator.lt, ast.LtE: operator.le,
        ast.Gt: operator.gt, ast.GtE: operator.ge, ast.In: lambda a, b: a in b,
        ast.NotIn: lambda a, b: a not in b}


class Unresolved(Exception):
    pass


def evaluate(node: ast.AST, env: dict):
    """A small, side-effect-free evaluator for the expression forms a timeout is written in."""
    if isinstance(node, ast.Constant):
        return node.value
    if isinstance(node, ast.Name):
        if node.id in env:
            return env[node.id]
        raise Unresolved(node.id)
    if isinstance(node, (ast.Tuple, ast.List, ast.Set)):
        return [evaluate(e, env) for e in node.elts]
    if isinstance(node, ast.IfExp):
        return evaluate(node.body if evaluate(node.test, env) else node.orelse, env)
    if isinstance(node, ast.Compare):
        left = evaluate(node.left, env)
        for op, right_node in zip(node.ops, node.comparators):
            right = evaluate(right_node, env)
            if type(op) not in _CMP or not _CMP[type(op)](left, right):
                return False
            left = right
        return True
    if isinstance(node, ast.BoolOp):
        values = [evaluate(v, env) for v in node.values]
        return all(values) if isinstance(node.op, ast.And) else any(values)
    if isinstance(node, ast.UnaryOp) and isinstance(node.op, (ast.USub, ast.Not)):
        value = evaluate(node.operand, env)
        return -value if isinstance(node.op, ast.USub) else not value
    if isinstance(node, ast.BinOp) and type(node.op) in _BIN:
        return _BIN[type(node.op)](evaluate(node.left, env), evaluate(node.right, env))
    if (isinstance(node, ast.Call) and isinstance(node.func, ast.Name) and node.func.id in ("min", "max", "float")
            and not node.keywords):
        args = [evaluate(a, env) for a in node.args]
        return {"min": min, "max": max, "float": float}[node.func.id](*args)
    raise Unresolved(ast.unparse(node))


def _bind(statements, env: dict) -> None:
    """Simple `NAME = expr` / `NAME: T = expr` bindings, in order; unresolvable ones are skipped."""
    for stmt in statements:
        targets, value = [], None
        if isinstance(stmt, ast.Assign) and all(isinstance(t, ast.Name) for t in stmt.targets):
            targets, value = [t.id for t in stmt.targets], stmt.value
        elif isinstance(stmt, ast.AnnAssign) and isinstance(stmt.target, ast.Name) and stmt.value is not None:
            targets, value = [stmt.target.id], stmt.value
        for name in targets:
            try:
                env[name] = evaluate(value, env)
            except (Unresolved, TypeError, ZeroDivisionError):
                env.pop(name, None)


def _defaults(tree: ast.Module, env: dict) -> dict:
    """Each module function's resolvable `timeout` default, for a call that omits the argument."""
    found = {}
    for fn in tree.body:
        if isinstance(fn, ast.FunctionDef):
            args = fn.args.args[len(fn.args.args) - len(fn.args.defaults):]
            pairs = list(zip(args, fn.args.defaults)) + list(zip(fn.args.kwonlyargs, fn.args.kw_defaults))
            for arg, default in pairs:
                if arg.arg == "timeout" and default is not None:
                    try:
                        found[fn.name] = evaluate(default, env)
                    except Unresolved:
                        pass
    return found


def end_signal_waits(source: str, host: str = "codex") -> list:
    """The socket waits of every `/signal` call in `_end_impl`, evaluated for `host`."""
    tree = ast.parse(source)
    env: dict = {}
    _bind(tree.body, env)
    defaults = _defaults(tree, env)
    end = next((n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == "_end_impl"), None)
    if end is None:
        raise Unresolved("_end_impl is missing")
    local = dict(env, host=host)
    _bind([s for s in ast.walk(end) if isinstance(s, (ast.Assign, ast.AnnAssign))], local)
    local["host"] = host
    waits = []
    for call in (n for n in ast.walk(end) if isinstance(n, ast.Call)):
        if not any(isinstance(c, ast.Constant) and isinstance(c.value, str) and "/signal" in c.value
                   for arg in call.args for c in ast.walk(arg)):
            continue
        timeout = next((k.value for k in call.keywords if k.arg == "timeout"), None)
        if timeout is not None:
            waits.append(float(evaluate(timeout, local)))
        elif isinstance(call.func, ast.Name) and call.func.id in defaults:
            waits.append(float(defaults[call.func.id]))
        else:
            raise Unresolved(f"{ast.unparse(call.func)}(...) without a resolvable timeout")
    return waits


def session_end_timeouts(path: str) -> list:
    hooks = json.loads(read(path))["hooks"]["SessionEnd"]
    return [handler["timeout"] for matcher in hooks for handler in matcher["hooks"]]


def main() -> None:
    for path in ("plugins/graphhelm/hooks/codex-hooks.json", "plugins/graphhelm-codex-hooks/hooks/codex-hooks.json"):
        if any(t > 3 for t in session_end_timeouts(path)):
            fail(f"{path} still declares a SessionEnd timeout above Codex's three-second cap")
    if session_end_timeouts("plugins/graphhelm/hooks/hooks.json") != [5]:
        fail("Claude's SessionEnd budget in hooks/hooks.json changed; only Codex enforces the cap")
    for path in ("plugins/graphhelm/hooks/session_hook.py", "plugins/graphhelm-codex-hooks/hooks/session_hook.py"):
        try:
            waits = end_signal_waits(read(path))
        except (Unresolved, SyntaxError, TypeError, ValueError) as err:
            fail(f"{path}: the end signal's socket wait on Codex could not be resolved: {err}")
        if not waits or max(waits) >= 3.0:
            fail(f"{path}: the end signal's socket wait on Codex {waits} does not fit inside three seconds")
    hidden_unittest(FIX, "plugins/graphhelm/hooks/test_session_hook.py")
    ok("Codex SessionEnd fits its cap; Claude's budget is unchanged")


if __name__ == "__main__":
    main()
