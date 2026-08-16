# GraphHelm chat plugin (Claude Code)

Operate GraphHelm executions from chat: this plugin registers the `graphhelm mcp` stdio
server and ships the first two operator skills (`operate-execution`, `observe-agents`).

## Setup

1. Run the Public Runtime API locally: `graphhelm serve --events <dir> --bind 127.0.0.1:8080`.
   The server writes its bearer token beside the events directory (`<dir>.token`).
2. Point `GRAPHHELM_TOKEN_FILE` at that token file (or edit `.mcp.json`'s `--token-file`
   argument directly). The token value never travels via argv or chat.
3. Install the plugin; the `graphhelm` MCP server appears with its ten tools.

## Deletability (CHAT_SURFACE_SPEC §7)

Everything this plugin does is `graphhelm mcp` plus documented CLI calls; deleting it loses convenience only. No state lives here: executions, events, evidence, and credentials all
live in GraphHelm's own stores, and every skill step names the tool (and through it the API
call) it choreographs — you can always do the same thing with `graphhelm` directly.
