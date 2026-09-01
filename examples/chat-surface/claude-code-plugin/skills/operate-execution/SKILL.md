---
name: operate-execution
description: The operator loop for one GraphHelm execution - start it, watch the tail, surface triage items, act on them, report the outcome with evidence references. Use when the user wants to run, supervise, unblock, pause, resume, or cancel an execution from chat.
---

# Operate an execution

The operator loop as one skill (CHAT_SURFACE_SPEC §4.3): **start → watch → triage → act →
report**. Every step below names the exact tool it choreographs — each tool's own description
names the API call it maps to, and nothing here does anything `graphhelm` CLI commands could
not do (§6 parity).

## What this skill reads

- `tool:status` — the execution's aggregate state, node-state counts, head sequence.
- `tool:events` — the attributed event tail (`after`/`limit` paged); triage items surface
  here: blocked nodes, pending approvals, capacity waits.

## What this skill mutates

- `tool:start` — load the graph file and begin, in the mode the user names.
- `tool:approve` — ready one blocked or ghost node after the user decides.
- `tool:signal` — record a signal envelope. `evidenceOut` is OPTIONAL: a Runtime started with
  a keyring seals the envelope itself, and a path is then a second copy of something already
  durable. Without a keyring the path is the only copy and the call is refused without it. For
  notes addressed to a person watching rather than signals about the run, see `leave-records`.
- `tool:pause` / `tool:resume` — hold and continue. **Immediate-stop is explicit and
  confirmed**: `pause` with `mode: "immediate"` interrupts in-flight work and blocks the
  interrupted node for triage — say so and get a confirmation before sending it; a plain
  `pause` is the graceful default.
- `tool:cancel` — terminal. **State the §13 partial-effects consequence before acting**:
  work already recorded stays recorded; compensation is recorded, not executed. Confirm
  with the user first.

## The loop

1. `tool:start` (or pick up a running execution via `tool:status`).
2. Watch: poll `tool:events` from the last seen head (`after`), never re-reading the whole
   stream. Pin mutations with `ifMatch` when acting on what was just read — a stale pin
   comes back as the API's own 409: re-read via `tool:status`, retry once with the fresh
   head.
3. Triage: report blocked nodes and approval waits with their evidence references, verbatim
   from the envelope — never invented.
4. Act: one tool per user decision, attributed to this chat's `--actor`.
5. Report: the outcome envelope's own data (status, head, evidence ids). Codes like
   `GHE003_IDEMPOTENCY_CONFLICT` reach this chat already redaction-safe; show them as-is.

## Credentials

A pasted secret is refused, never echoed — use `graphhelm gateway credential set`'s stdin
path. This surface has no credential tool by design; do not try to route a key through
`tool:signal` or any other argument (secret-shaped values are refused by the server).
