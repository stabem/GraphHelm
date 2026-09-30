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
        if node.id in env.get("__ambiguous__", ()):
            raise Unresolved(f"{node.id} is bound more than once or conditionally")
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
            if type(op) not in _CMP:
                raise Unresolved(ast.unparse(node))
            if not _CMP[type(op)](left, right):
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


def _assigned(node: ast.AST) -> list:
    """(name, straight_line) for every name a statement binds; straight_line is False for `+=`."""
    out = []
    def names(target):
        return [n.id for n in ast.walk(target) if isinstance(n, ast.Name)]
    if isinstance(node, ast.Assign):
        out += [(n, True) for t in node.targets for n in names(t)]
    elif isinstance(node, ast.AnnAssign) and node.value is not None:
        out += [(n, True) for n in names(node.target)]
    elif isinstance(node, ast.AugAssign):
        out += [(n, False) for n in names(node.target)]
    elif isinstance(node, (ast.For, ast.AsyncFor, ast.comprehension)):
        out += [(n, False) for n in names(node.target)]
    elif isinstance(node, ast.NamedExpr):
        out += [(node.target.id, False)]
    elif isinstance(node, (ast.With, ast.AsyncWith)):
        out += [(n, False) for item in node.items if item.optional_vars is not None for n in names(item.optional_vars)]
    elif isinstance(node, ast.ExceptHandler) and node.name:
        out += [(node.name, False)]
    return out


def _bind(statements: list, env: dict, ambiguous: set) -> None:
    """Bind `NAME = expr` statements that sit directly in `statements`. A name bound more than once
    in `statements` (walked in full), bound by `+=`, a loop, `with`, `except` or `:=`, or bound inside a branch is
    ambiguous: it is removed from `env` and any use of it is Unresolved (#139/#140: the last
    assignment walked must not win)."""
    top = {id(stmt) for stmt in statements}
    counts: dict = {}
    for node in (n for stmt in statements for n in ast.walk(stmt)):
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.Lambda, ast.ClassDef)):
            continue
        for name, straight in _assigned(node):
            counts[name] = counts.get(name, 0) + 1
            if not straight or id(node) not in top:
                ambiguous.add(name)
    ambiguous.update(name for name, n in counts.items() if n > 1)
    for name in ambiguous:
        env.pop(name, None)
    for stmt in statements:
        for name, _ in _assigned(stmt):
            if name in ambiguous:
                continue
            try:
                env[name] = evaluate(stmt.value, env)
            except (Unresolved, TypeError, ZeroDivisionError):
                ambiguous.add(name)
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
    module_ambiguous: set = set()
    env: dict = {}
    _bind([s for s in tree.body if not isinstance(s, (ast.FunctionDef, ast.ClassDef))], env, module_ambiguous)
    env["__ambiguous__"] = module_ambiguous
    defaults = _defaults(tree, env)
    end = next((n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == "_end_impl"), None)
    if end is None:
        raise Unresolved("_end_impl is missing")
    local_ambiguous = set(module_ambiguous)
    if any(name == "host" for stmt in end.body for node in ast.walk(stmt) for name, _ in _assigned(node)):
        raise Unresolved("_end_impl rebinds host")
    local = dict(env, __ambiguous__=local_ambiguous, host=host)
    _bind(end.body, local, local_ambiguous)
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
