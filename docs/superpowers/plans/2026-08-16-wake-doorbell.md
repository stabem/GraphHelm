# Milestone 05g — The Wake Doorbell: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: the two-agent pair loop (05e/05f protocol verbatim). Task 0 reconciles against post-05f main (`ee3f128`).

**Goal:** A chat session (or any API client) sleeps at zero cost and is woken by another actor's append — one bit, no payload, no polling anywhere.

**Architecture:** The sleeper arms its own wait: it deposits a `WakeLease` event naming its session, its last-seen cursor and a local rendezvous (a named pipe / Unix socket under the operator's own directory), then blocks a sidecar on that rendezvous. When `append_atomic` lands an event in the lease's scope past the cursor, the serve process writes ONE content-free byte to the rendezvous and appends `WakeLeaseConsumed` in the same durable batch. The woken session re-reads `events` from its own cursor — **the signal chooses WHEN, never WHAT**: content travels only through the log, so a wake can never carry a prompt injection.

**Design sources:** the 05f-era divergent pass (the attacker frame's traps are binding); D-036 (events wire-safe); D-037 (event-kind ritual); ADR-026 (MCP stays minimal — `wake` joins as tools, not push notifications: the SIDECAR wakes the host, the protocol stays pull-only).

**Security constraints (binding):**
1. **Content-free by construction:** the ring is exactly one byte; a test pins the byte count. The woken side trusts only its own re-read of the log.
2. **Anti fork-bomb:** one live lease per session; a consumed lease must be re-armed by the sleeper — the waker can never schedule the sleeper into a loop.
3. **The wake is an accelerator, never a correction:** every sleeper keeps its dead-man timer; a lost ring degrades to slow, never to wrong.
4. The rendezvous lives under the operator's own profile with owner-only permissions; the serve process only ever OPENS-and-WRITES-ONE-BYTE — never reads, never executes, never creates the rendezvous.

**Rules of engagement (the 05f set, plus):**
1. The code wins over the plan; discrepancies reported in the handoff.
2. TDD with observed red; sabotage per guard with the failing test cited; restore from `cp` backup.
3. `cargo +1.97.1 fmt --all` + workspace clippy `-D warnings` before every commit; commit trailer `Co-Authored-By: Claude Code <noreply@anthropic.com>`.
4. One writer at a time; SPEC-then-QUALITY review by the non-implementer; **per-file `git add` always** (the 05f slip lesson).
5. Failure-code registry: GHCLI001–016 taken. This plan takes **GHCLI017_WAKE_INVALID** (CLI/config failures of `wake-wait` and the wake tools' argument refusals that are not JSON-RPC-shaped).
6. New CLI test binary `wake_http` joins the gate (eleventh suite; `monitor_http` precedent).
7. Evidence-with-checksum artifacts, if any, hash the bytes git stores (`git show HEAD:path`), `.gitattributes -text` first (the 05f CRLF lesson).

**Files (map):**
- Create: `apps/cli/src/commands/wake.rs` (the `wake-wait` sidecar command), `apps/cli/src/commands/serve/wake.rs` (lease registry + ring), `apps/cli/tests/wake_http.rs`
- Modify: `core/protocols/src/event.rs` + schemas (D-037: `WakeLease`, `WakeLeaseConsumed`), `core/events` fold (explicit arms), `apps/cli/src/commands/serve/mod.rs` (ring hook), `apps/cli/src/commands/mcp/{tools,session}.rs` (`wake_arm`, `wake_status`), `args.rs`, `ci/gate.ps1`
- Hotspots (single-writer coordination): `args.rs`, `commands/mod.rs`, `mcp/tools.rs`, `serve/mod.rs`

### Task 0: rendezvous spike + reconciliation ✅ (ran 2026-08-16 against main 0c46f0a)

- [x] **Step 1 (spike, run live on Windows):** tokio =1.53.1 (the workspace pin) with the `net` feature: sleeper = `ServerOptions::new().first_pipe_instance(true).max_instances(1).create(r"\.\pipe\graphhelm-wake-<id>")` then `connect().await` + a blocked `read()`; ringer = `ClientOptions::new().open(name)` + `write_all(&[1])`. **Proven:** exactly one byte crossed the blocked read (`RING-OK read=1`); ringing a missing rendezvous fails `io::ErrorKind::NotFound` (the stale-lease shape — consumed-and-logged, never a serve error); a squatter pre-creating the name fails `PermissionDenied` against `first_pipe_instance(true)` (the anti-squat defense is the API's own). Windows pipe ACLs default to creator-owner — owner-only comes free; the Unix side (`UnixListener::bind` under `$XDG_RUNTIME_DIR/graphhelm/` 0o700; `NotFound`/`AddrInUse` shapes) is compile-shaped design, runtime-proven when a Linux gate exists — honest limit for Task 6's docs.
- [x] **Step 2 (reconciled):** GHCLI017 free (grep: zero hits; registry ends at 016); gate list exactly the ten suites (`ci/gate.ps1:106`); event-kind registry ends at `ReuseDecision` (`event.rs:178`) with 24 explicit fold arms. **The hook-point finding that reshapes Task 2:** `append_atomic` is per-process — a ring hook in the SERVE layer (wrapping serve's own append call sites) fires only for appends the serve process makes (API and MCP paths). A CLI-direct append in another process cannot ring without putting transport into `core/events` (refused). Resolution: the ring hook is serve-side; **CLI-direct appends do not ring in this slice** — the dead-man timer covers them (accelerator-never-correction, constraint 3), recorded as a Task 6 honest limit with the follow-on named (a store-level notification file the serve tails, or the lease checker moving into the driver era).
- [x] **Step 3: Commit** `docs(plans): reconcile 05g plan and record the rendezvous spike (Task 0)`.
### Task 1: the `WakeLease` / `WakeLeaseConsumed` kinds (D-037)

- [ ] **Failing tests:** schema round-trip for both kinds; the fold holds at most ONE live lease per session (arming twice replaces, never stacks — anti fork-bomb rule 2 as a fold invariant); a replay NEVER rings (the fold is pure — pinned by the purity source-invariant pattern); `WakeLeaseConsumed` without a matching live lease is a fold integrity refusal.
- [ ] **Implement:** the D-037 ritual complete — envelope schema in place, `schemas/releases/1.0.0/` byte-identical mirror, both catalog digests recomputed via `schema_digest` (canonical, not raw bytes), explicit fold arms, `checked_in_1_0_0_release...` green unmodified.
- [ ] **Sabotage:** stack two live leases in the fold; the one-lease invariant test fails; restore. **Commit** `feat(events): wake lease kinds with the one-live-lease fold invariant`.

### Task 2: the serve-side ring

- [ ] **Failing tests (wake_http.rs, live serve):** an append past the lease cursor rings exactly one byte at the rendezvous AND appends `WakeLeaseConsumed` in the same durable batch (read the stream: consumed follows the triggering event); an append at-or-before the cursor does NOT ring; a second append after consumption does NOT ring (the lease burned); a rendezvous that no longer exists marks the lease consumed WITHOUT error (stale sleeper, honest cleanup).
- [ ] **Implement:** serve registers a post-append callback with the store handle it already opens per request (the callback lives in `serve/wake.rs`; `core/events` stays transport-pure — it only exposes the hook point found in Task 0). Ring = open, write 1 byte, close; every failure shape maps to consumed-and-logged, never to a serve error (a wake failure must never fail the append that caused it — the append already happened).
- [ ] **Sabotage:** ring BEFORE `append_atomic` returns; the "a ring implies a durable event" test fails; restore. **Commit** `feat(serve): the one-byte ring on durable append`.

### Task 3: the MCP tools (`wake_arm`, `wake_status`)

- [ ] **Failing tests (mcp_stdio.rs):** the tool list grows to exactly TWELVE (closed-list test updated — the sabotage adds `wake_ring` and the list test must fail); `wake_arm(executionId, rendezvous)` appends the lease via the API and returns the armed cursor; `wake_status` reads the live lease; **NO tool exists to ring another session** (omission is the enforcement); a secret-shaped rendezvous path is refused by the existing guard.
- [ ] **Implement:** two tools over the existing `ApiClient`; serve gains `POST /v1/executions/{id}/wake-lease` (idempotent, the three headers) and the MCP tools map 1:1 (rule: never a second path).
- [ ] **Sabotage:** add `wake_ring` to the table; the closed-list test fails; restore. **Commit** `feat(mcp): wake_arm and wake_status — the sleeper-only surface`.

### Task 4: the sidecar (`graphhelm wake-wait`)

- [ ] **Failing tests (wake_http.rs, black-box):** `wake-wait --rendezvous <path> --timeout <s>` creates the rendezvous owner-only, blocks, exits 0 within ~1s of a ring, exits 3 on timeout, exits 2 (GHCLI017) on an unusable path; the process consumes ~zero CPU while blocked (no spin — assert via a platform counter or document the manual observation); **content never crosses**: whatever bytes a hostile ringer writes, the sidecar's stdout carries none of them (sentinel test).
- [ ] **Implement:** thin std blocking read; hook-friendly exit codes documented in `--help`.
- [ ] **Sabotage:** echo the received byte(s) to stdout; the content-never-crosses test fails; restore. **Commit** `feat(cli): wake-wait — the blocking sidecar`.

### Task 5: the choreography proof

- [ ] **The test:** two MCP sessions against one serve; A `wake_arm`s and blocks a real `wake-wait` child; B appends (a signal); A's sidecar exits 0, A re-reads `events` from its cursor and sees B's event; **the zero-polling assertion**: the serve's request log (count requests between arm and ring) shows ZERO reads by A in the window — measured, not claimed. Plus the degradation path: kill the serve before B appends; A's `--timeout` fires; A falls back to a plain read (slow, never wrong).
- [ ] **Sabotage:** make the sidecar poll the API in its wait loop; the zero-request assertion fails; restore. **Commit** `test(wake): two sessions, one ring, zero polls`.

### Task 6: gate stage + docs + close

- [ ] `'wake_http'` eleventh suite, red-proven (misspell pattern). Docs: the 05g section from the code as built; honest limits (single-machine rendezvous — multi-host stays on the dead-man timer until a transport exists; the driver sleeping on leases for `WaitingInput` nodes is the named follow-on, deliberately not this slice; ring-loss probability and the accelerator-never-correction rule). CHANGELOG; full gate announced then run. **Commit** `docs(wake): record milestone 05g`. PR.

**Refused scope (diff-visible):** wake payloads of any kind; a `wake_ring(peer)` tool; driving another session's input; unbounded blocking inside an MCP tool call (the sidecar blocks OUTSIDE the protocol, between turns); the driver integration (named follow-on).

**Suggested split (the alternating pattern):** A: 0, 2, 4, 6 + PR · B: 1, 3, 5 + final PR review — or inverted at kickoff; Task 0's spike owner decides at kickoff.
