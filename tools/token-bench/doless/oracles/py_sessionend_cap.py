"""py-sessionend-cap: Codex's SessionEnd hook declares at most the three seconds Codex enforces, the
end acknowledgement's socket wait fits inside that cap, and Claude's own SessionEnd budget is left
as it was. The known fix's test file (98dbd590) runs green against the checkout."""
import json
import re

from _lib import fail, hidden_unittest, ok, read

FIX = "98dbd5904248c1983cedc270149cc6d07310116f"


def session_end_timeouts(path: str) -> list:
    hooks = json.loads(read(path))["hooks"]["SessionEnd"]
    return [handler["timeout"] for matcher in hooks for handler in matcher["hooks"]]


for path in ("plugins/graphhelm/hooks/codex-hooks.json", "plugins/graphhelm-codex-hooks/hooks/codex-hooks.json"):
    if any(t > 3 for t in session_end_timeouts(path)):
        fail(f"{path} still declares a SessionEnd timeout above Codex's three-second cap")
if session_end_timeouts("plugins/graphhelm/hooks/hooks.json") != [5]:
    fail("Claude's SessionEnd budget in hooks/hooks.json changed; only Codex enforces the cap")
for path in ("plugins/graphhelm/hooks/session_hook.py", "plugins/graphhelm-codex-hooks/hooks/session_hook.py"):
    body = read(path)
    end = body[body.find("def _end_impl"):]
    end = end[:end.find("\ndef ", 1)]
    waits = [float(v) for v in re.findall(r"/signal\".*?timeout=([0-9.]+)", end)]
    if not waits or max(waits) >= 3.0:
        fail(f"{path}: the end signal's socket wait {waits} does not fit inside three seconds")
hidden_unittest(FIX, "plugins/graphhelm/hooks/test_session_hook.py")
ok("Codex SessionEnd fits its cap; Claude's budget is unchanged")
