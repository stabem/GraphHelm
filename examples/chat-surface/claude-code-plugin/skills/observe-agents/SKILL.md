---
name: observe-agents
description: Multi-agent visibility, read-only - who is acting on this execution right now and what did they do, derived entirely from the events tail's attribution. Use when the user asks what other agents or sessions are doing, or to review an interleaved actor timeline.
---

# Observe agents

Read-only multi-agent visibility (CHAT_SURFACE_SPEC §4.4): every mutation on the event log
carries its actor (05a's contract), so the whole picture derives from two read tools and
nothing else.

## What this skill reads

- `tool:events` — the attributed tail; every event names its actor id and type. Page with
  `after`/`limit` from the last seen head.
- `tool:status` — the aggregate frame (head sequence, node states) the timeline hangs on.

## What this skill mutates

Nothing. This skill is read-only by contract: it renders other actors' work, it **never acts
as them** and never impersonates — acting (approve, signal, pause) belongs to
`operate-execution` under this chat's own `--actor` identity.

## Rendering the timeline

1. Read `tool:status` for the current head, then `tool:events` for the window the user asks
   about.
2. Interleave by sequence, grouped by actor: which actor appended what, in order.
3. **Flag conflicts**: a 409 or a stale-`If-Match` loser shows up as a mutation attempt
   whose retry follows a re-read — call these out explicitly (who lost the race, at which
   head, and what their retry did).
4. Report evidence references by id, never contents: sealed evidence stays sealed; another
   session retrieves it by reference under its own scope.
