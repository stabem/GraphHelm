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
