# Milestone 05g — The Wake Doorbell: Implementation Plan (draft, pre-kickoff)

> **For agentic workers:** REQUIRED SUB-SKILL: the two-agent pair loop (05e/05f protocol verbatim). **Do not start before 05f merges** — Task 0 reconciles against the post-05f main.

**Goal:** A chat session (or any API client) sleeps at zero cost and is woken by another actor's append — one bit, no payload, no polling anywhere.

**Architecture:** The sleeper arms its own wait: it deposits a `WakeLease` event naming its session, its last-seen cursor and a local rendezvous (a named pipe/Unix socket under the operator's own directory), then blocks a sidecar on that rendezvous. When `append_atomic` lands an event in the lease's scope past the cursor, the serve process writes ONE content-free byte to the rendezvous and marks the lease consumed. The woken session re-reads `events` from its own cursor — **the signal chooses WHEN, never WHAT**: content travels only through the log, so a wake can never carry a prompt injection.

**Tech Stack:** existing workspace; the one new surface is the platform rendezvous (Windows named pipe / Unix domain socket) — Task 0 spikes it before anything else is written.

**Design sources:** the 05f-era divergent pass (five frames; the attacker frame's traps are binding: no wake payload, no session-driving, no unbounded park inside a stdio tool call); D-036 (events wire-safe); D-037 (event-kind additions); ADR-026 (the MCP layer stays minimal — `wake` joins as tools, not push notifications: the SIDECAR wakes the host, the protocol stays pull-only).

**Security constraints (binding):**
1. **Content-free by construction:** the ring is exactly one byte; the test pins it. The woken side trusts only its own re-read of the log.
2. **Anti fork-bomb:** leases are rate-bound (one live lease per session; a consumed lease must be re-armed by the sleeper — the waker can never schedule the sleeper into a loop).
3. **The wake is an accelerator, never a correction:** every sleeper keeps its dead-man timer (the harness's own scheduled wake); a lost ring degrades to slow, never to wrong.
4. The rendezvous lives under the operator's own profile with owner-only permissions; the serve process only ever OPENS-and-WRITES-ONE-BYTE — it never reads, never executes, never creates.

**Tasks (sketch — the kickoff Task 0 reconciles and finalizes):**
- **Task 0:** cross-platform rendezvous spike (Rust: `\\.\pipe\graphhelm-wake-<id>` vs `$XDG_RUNTIME_DIR/graphhelm/wake-<id>.sock`), plus plan reconciliation against post-05f main (event-kind registry state, GHCLI registry, gate list at ten suites).
- **Task 1:** `WakeLease` event kind (D-037 process: schema, registry, fold — the lease and its consumption are ordinary replayable events; a replay NEVER rings).
- **Task 2:** the serve-side ring: `append_atomic` completion hook checks live leases for the stream, writes the byte, appends `WakeLeaseConsumed` in the same batch. Sabotage: ring before durability; the "a ring implies a durable event" test fails.
- **Task 3:** the MCP tools (`wake_arm`, `wake_status`) — the sleeper-only surface; NO tool exists to ring another session directly (omission is the enforcement, the 05e closed-list pattern).
- **Task 4:** the sidecar reference (`graphhelm wake-wait --rendezvous <path> --timeout <s>`): blocks, exits 0 on ring / 3 on timeout — hook-friendly for any host. Content never crosses it.
- **Task 5:** the choreography test: two MCP sessions, one serve; A arms and blocks on the sidecar; B appends; A wakes and reads B's event from the log — with an assertion that ZERO requests happened between arm and ring (no polling, measured, not claimed).
- **Task 6:** gate stage + docs + honest limits (single-machine only — the rendezvous is local; multi-host waits stay on the dead-man timer until a transport exists; the driver sleeping on leases for `WaitingInput` nodes is the named follow-on, deliberately NOT this slice).

**Refused scope (the traps, diff-visible):** wake payloads of any kind; a `wake_ring(peer)` tool; driving another session's input (tmux/resume-injection shapes); unbounded blocking inside an MCP tool call (the sidecar blocks OUTSIDE the protocol, between turns).
