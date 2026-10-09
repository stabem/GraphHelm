The task-handoff MCP stdio adapter (`plugins/graphhelm/hooks/task_handoff.py`, `--mcp-stdio`)
refuses any `tools/call` whose `params` has a field other than `name` and `arguments`. Native MCP
clients send the standard `_meta` object (for example `{"progressToken": ...}`), so every tool call
from them fails before dispatch. Accept `_meta` when it is an object; every other unknown params
field, the identity-override refusal and the oversized-frame refusal must keep working. The Codex
companion (`plugins/graphhelm-codex-hooks/hooks/task_handoff.py`) ships the same file and must stay
byte-identical. Cover the `_meta` call in `plugins/graphhelm/hooks/test_task_handoff.py`.
