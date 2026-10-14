# Claude Code talking to Codex over MCP

Codex CLI ships an MCP server: `codex mcp-server` (stdio). It exposes two tools:
`codex` (start a Codex session with a prompt) and `codex-reply` (continue it by
conversation id). Any MCP client, Claude Code included, can call them. This is the
official, cross-platform route (Windows, Linux, macOS). The ChatGPT Mac app's
computer-use server is a different, macOS-only surface and is not used here.

## Register it in Claude Code

One line, user scope:

```
claude mcp add --scope user codex -- codex mcp-server
```

Claude now sees `mcp__codex__codex` and `mcp__codex__codex-reply`.

## Keep the lane visible in GraphHelm

Codex reports to GraphHelm itself, so Claude never relays Codex's transcript
(that is where tokens would be wasted). Register the GraphHelm MCP server in Codex
with `../codex/config.toml`, then brief Codex with only:

- the execution id,
- "call `briefing` first, act on `nextStep`, record proof with `evidence`".

Progress and evidence then land in the same store Studio reads. Claude reads the
outcome with `status` / `briefing`, not by reading Codex's output.

## When to use which route

| Need | Route |
|---|---|
| Scheduled worker lane or review inside a run | the gateway's Codex runtime (`codex exec`, routed by tier) — already proven, receipts recorded |
| Ad-hoc: Claude asks Codex for a second opinion or a sub-task mid-conversation | `codex mcp-server` as above |

## Windows notes

- `codex mcp-server` is a stdio child of Claude Code: no extra terminal window opens.
- Keep briefs short; long command lines on Windows hit the 32 KB limit (error 206).
  Put long context in a file in the workspace and pass its path.

## Deletability (CHAT_SURFACE_SPEC §7)

`claude mcp remove codex` undoes it. No state lives here.
