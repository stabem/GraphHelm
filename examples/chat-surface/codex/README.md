# GraphHelm MCP server for Codex

Merge `config.toml`'s `[mcp_servers.graphhelm]` table into your `~/.codex/config.toml`,
pointing `--token-file` at the token `graphhelm serve` writes beside its events directory.

## Picking up an execution

Call the `briefing` tool first (`GET /v1/executions/{id}/briefing`): it says what the run is
for, every decision with its actor, the work done, what is pending and the next step, derived
from the store alone — the same bytes Claude Code or the CLI would read. Act on `nextStep`.

## Deletability (CHAT_SURFACE_SPEC §7)

Everything this registration does is `graphhelm mcp` plus documented CLI calls; deleting it loses convenience only. No state lives here — executions, events, evidence, and credentials
all live in GraphHelm's own stores.

## Optional session hooks

The adjacent `claude-code-plugin` folder also has a portable `plugin.json`. Its Codex extension
loads `hooks/codex-hooks.json`, which runs the same Python 3 command hook on `SessionStart` and
`SessionEnd`. Point Codex at that local plugin folder and review/trust its hook definition when
Codex prompts. Codex skips non-managed plugin hooks until they are trusted. The MCP registration
above remains separate.

Set `GRAPHHELM_EXECUTION_ID`, `GRAPHHELM_TOKEN_FILE`, and optionally
`GRAPHHELM_RUNTIME_URL` in the environment that launches Codex. A session without an explicit
execution ID receives Keel guidance but posts nothing to GraphHelm. At start, the hook reads a
fresh briefing. At end, it records an attributed session-end signal, never a completion claim.
See the plugin README for the precise behavior and the local state directory.
