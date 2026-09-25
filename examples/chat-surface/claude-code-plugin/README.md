# GraphHelm chat plugin (Claude Code)

Operate GraphHelm executions from chat: this plugin registers the `graphhelm mcp` stdio
server and ships three skills — `operate-execution` and `observe-agents` for driving and
watching a run from chat, and `leave-records` for the other direction: an agent writing into
the log for a person who is watching the Studio and is not at this terminal.

## Setup

1. Run the Public Runtime API locally: `graphhelm serve --events <dir> --bind 127.0.0.1:8080`.
   The server writes its bearer token beside the events directory (`<dir>.token`).
2. Point `GRAPHHELM_TOKEN_FILE` at that token file (or edit `.mcp.json`'s `--token-file`
   argument directly). The token value never travels via argv or chat.
3. Install the plugin; the `graphhelm` MCP server appears with its tools. (This line used to
   name a count. It said fourteen; the server exposed twenty-four when this was last checked,
   on 2026-08-28. A number here rots silently every time a tool is added, so the authority is
   `tools/list` — ask the server.)

## Deletability (CHAT_SURFACE_SPEC §7)

Everything this plugin does is `graphhelm mcp` plus documented CLI calls; deleting it loses convenience only. No state lives here: executions, events, evidence, and credentials all
live in GraphHelm's own stores, and every skill step names the tool (and through it the API
call) it choreographs — you can always do the same thing with `graphhelm` directly.
