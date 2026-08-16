# GraphHelm MCP server for Codex

Merge `config.toml`'s `[mcp_servers.graphhelm]` table into your `~/.codex/config.toml`,
pointing `--token-file` at the token `graphhelm serve` writes beside its events directory.

## Deletability (CHAT_SURFACE_SPEC §7)

Everything this registration does is `graphhelm mcp` plus documented CLI calls; deleting it loses convenience only. No state lives here — executions, events, evidence, and credentials
all live in GraphHelm's own stores.
