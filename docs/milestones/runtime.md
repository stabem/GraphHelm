# Runtime

Status: 05a implemented — the Public Runtime API. Five plans remain: the gateway slice (05b), the
tool broker and tiers (05c), the real async executor (05d), the MCP chat surface (05e), and the
monitor with the milestone close (05f). Design: `docs/superpowers/specs/2026-08-13-runtime-design.md`;
decisions D-039 (chat-first via an official MCP server) and D-040 (the monitor precedes Studio).

Milestone 04 proved governance and durability with an effect-free executor. Milestone 05 makes it
real: model calls, tool calls, isolation — and the surfaces to operate it all. 05a shipped the
first surface, designed as the multi-agent concurrency contract: several agents can work one
project through the same API without torn state, with every act attributed, every conflict
explicit, and every observation flowing through the shared event log.

## What 05a shipped: `graphhelm serve`

### The server

A `serve` subcommand in `apps/cli`, on axum `=0.8.9` (ADR-024 in
`docs/reference/REFERENCE_STACK_AND_ADRS.md`: chosen over hand-rolled hyper and actix for the tower
ecosystem and the register's replaceability rules; no handler holds state beyond `ServeState`).
Binding is **loopback-only, fail-closed** — a non-loopback `--bind` is refused with
`GHCLI006_SERVE_INVALID` before anything opens, proven able to fail by a test that also caught its
own first version hanging on the regression it guards (`a_non_loopback_bind_is_refused_fail_closed_before_anything_is_opened`).

Auth is a bearer token: 32 OS-random bytes, hex, stored **beside** the events directory
(`<dir>.token`), never inside it — the store's layout allowlist rejects foreign entries with
`GHE007_UNSUPPORTED_FORMAT`, and the first draft that wrote the token inside the directory poisoned
the whole repository until moved. Comparison is constant-time by a hand-written fold, matching the
D-038 minimalism precedent. `/health` is the only unauthenticated route; unknown routes are 401
before they are 404. mTLS and non-local identity are deferred with the architecture's full §3.1
posture — recorded, not forgotten.

### One store, one truth

The server holds **no repository handle**: every request opens and drops the store exactly as every
CLI command does, because `LocalEventRepository::open` holds an OS-exclusive lock for the handle's
lifetime — a long-lived handle would lock out concurrent CLI processes and kill the multi-agent
premise. The per-request lock serialization *is* the concurrency model; the API surfaces it as HTTP
semantics rather than reinventing it.

### The endpoint contract

All under `/v1`, all replying the same four-key envelope the CLI prints. Reads: `status` (carrying
`headSequence` — the one shared-surface change to the command layer itself, the CLI gained it too)
and a paged events tail (`after` exclusive, `limit` default 100 max 1000, over-limit refused with
400 — never silent truncation) serving raw envelopes, legitimate because D-036 keeps free-form
content off the wire. A fresh mutation's own reply carries `headSequence` too, but that one is an
API-reply enrichment added in the serve layer alone (`run_idempotent_mutation`), not a change to
any command's return shape — the CLI's own mutation output is unchanged.

Mutations require three headers, validated before the store is touched: `Idempotency-Key` (an
`OpaqueId`, capped at 64 characters — see below), `X-GraphHelm-Actor`, and
`X-GraphHelm-Actor-Type: owner | agent` — `human` awaits Studio auth and `system` is reserved for
the driver's own hops, which keep their 04f attribution. `If-Match: <head-sequence>` is optional; a
stale head is refused with 409 carrying `data.currentHead`, checked after the idempotency
pre-flight (a completed command's retry succeeds even with a stale `If-Match`) and immediately
before the command body; the store remains the authority underneath. A fresh mutation success now
also carries `data.headSequence` (the same current head a following status read would report),
closing the gap that previously forced an extra GET per `If-Match`-chained write — CLI output is
unchanged, since this is a serve-layer enrichment of the API reply, not a change to any command's
own return shape.

### Idempotent retry, content-aware

The store **rejects** a duplicate idempotency key at a freshly-advanced sequence
(`GHE003_IDEMPOTENCY_CONFLICT`) — verified empirically before anything was built on it. Retry
semantics rest on a pre-flight: each mutation's *decision event* (the single always-present event
meaning "this command happened") derives its key from the caller's command key plus a fixed suffix
plus a 16-hex-character digest of the request's content (command name, execution id, canonical JSON
body — `{command-key}-{suffix}-{digest16}`), and `run_idempotent_mutation` classifies the decision
keys directly against the stream history `resolve_stream` already loads (no extra store open) as
one of four states:

- **Absent** — genuinely fresh; the command runs.
- **Complete** — a committed event's key matches exactly (suffix *and* digest): the command already
  happened. 200 with current state (`approve`'s own precondition would otherwise reject its own
  retry).
- **Partial** — some but not all of a command's expected keys are committed, possible only if a
  previous attempt died mid-command. 409 naming the stuck key.
- **Divergent** — a committed event's key shares this command's `{command-key}-{suffix}-` prefix but
  carries a *different* digest: the caller reused one `Idempotency-Key` for two different request
  bodies. 409 (`GHE003_IDEMPOTENCY_CONFLICT`) naming the reused key, refusing before the store is
  touched.

The digest is what makes `Complete` and `Divergent` the same check rather than two: presence alone
used to be the whole test, and a caller reusing a key across two different bodies (`approve
{"node":"a"}` then `approve {"node":"b"}` under the same key) was silently absorbed as a completed
retry of the first, the second request's real effect never applied — found in 05a's final review
and closed by folding the digest into the key itself, rather than adding a second check alongside
the old one. It is a divergence *detector*, not a security boundary: a caller can only ever collide
with its own past requests, made under its own bearer token. The header is capped at 64 characters
so the derived key (header + 2 dashes + longest suffix `"cancelled"` (9) + digest (16) = 91 at the
maximum) stays comfortably inside `OpaqueId`'s 128-character limit.

The fan-out a command appends beyond its decision event — pause's per-node holds, the drive loop's
hops — keeps fresh keys, because a variable count cannot be indexed by fixed suffixes; the
pre-flight keying on decision events alone is what makes that safe. Retrying with the same key and
the same body appends nothing, sabotage-proven twice: reverting key derivation to fresh UUIDs
doubles the head on a byte-identical retry, and dropping the digest back out of the derivation lets
a divergent-body reuse be silently absorbed instead of refused.

### Attribution

Owner and agent acts are recorded as the caller (`PersistedActorType::{Owner, Agent}` from the
headers, serialized as the actor's `"type"` field); the driver's hops stay `System`. On the API
path `execution_started` is attributed to the caller while `drive_to_quiescence` runs under a
fresh system actor — start's CLI behaviour is unchanged.

### The storm, and what it proved

`the_storm_holds_under_eight_concurrent_agents`: eight OS threads, per-thread actors, six rounds of
interleaved pause/resume/signal/status, retry-once on 409. Held across three consecutive runs:
never a 500 or hung connection, coherent replay, byte-identical double replay, every mutation
attributed to one of the eight or the system driver. The deliberate sabotage — fresh UUID keys plus
a dropped pause precondition — was caught **synchronously**: the fold's own guard rejected the
incoherent history inside the very request that caused it (`ReplayError::Corrupt` → 500), which is
the oracle working one step earlier than designed.

Two agents also close the information-sharing loop through the API alone: scout signals, builder
observes it in the tail and approves, scout observes the approval — nothing shared but the URL and
the token.

### Parity, pinned

`the_cli_and_the_api_report_identical_status_for_the_same_story` drives one scripted story through
both surfaces against fresh stores and asserts identical final `status` data with an **empty**
exception list — it passed on its first run, meaning D-039's "never a second path" already holds
rather than being aspired to. When 05d swaps the driver under the API, this test is what proves the
surfaces did not drift. The `api_http` suite is a named gate stage, proven able to go red.

## Honest limits, stated

- **No documentation surface yet.** Checking and writing docs as first-class endpoints is the
  Living Documentation subsystem, reserved in the architecture's API surface
  (`documents/claims/artifacts`, §3.1) and arriving as its own plan into this same API. Until then
  agents coordinate through events, signals and status — which is what the harness defines signals
  for. This is the next coordination increment of the multi-agent vision, not an absence.
- **Throughput.** The server runs the CLI's current-thread tokio runtime with synchronous
  per-request store I/O — measured ≈3 requests/second under eight-thread load, which is why the
  storm is six rounds. Real dispatch concurrency arrives with 05d's async driver; this number is
  recorded so 05d has a baseline to beat, not as an accepted end state.
- **An unknown execution id reads as empty, not 404** — the store returns an empty stream for a
  valid-but-absent id and the API mirrors CLI parity exactly. A distinct not-found signal would
  have to come from the store layer; deferred with a pointer here.
- **Bearer-on-loopback only.** mTLS and any non-local identity remain §3.1 work, refused rather
  than half-built: the server will not bind a non-loopback address at all.
