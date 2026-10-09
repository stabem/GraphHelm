**Handoff tools fail from native MCP clients**

Calling the GraphHelm task-handoff tools (`offer`, `receive`, `status`) through the plugin's MCP
stdio server works from our own test client, but from a real MCP client every `tools/call` comes
back as an error before anything happens. The client sends the usual `params._meta` (a progress
token). Expected: tool calls with `_meta` work, for both the Claude plugin and the Codex companion.
