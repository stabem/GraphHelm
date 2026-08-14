# Milestone 05a - Public Runtime API Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `graphhelm serve` — the Public Runtime API over the existing execution command layer — so that **many agents can work the same project concurrently**: every mutation carries an idempotency key and an expected version, conflicts surface as structured 409s instead of torn state, every event is attributed to the agent that caused it, and each agent sees the others' work through the events tail. This is the substrate the MCP chat surface (05e) and the monitor (05f) stand on.

**Architecture:** A new `serve` subcommand inside `apps/cli` (the crate is bin-only; 05d later lifts the driver into `core/runtime`, and this server rides along unchanged because it only speaks to the command layer). Endpoints map one-to-one onto the seven `execution` commands plus status, events and health — same JSON envelope, same redaction-safe failure codes — bound to `127.0.0.1` with a bearer token. The store's own append lock and per-event idempotency keys are what make concurrent writers safe; the API's job is to *surface* those guarantees as HTTP semantics rather than reinvent them.

**Tech Stack:** Rust 1.97.1, edition 2024. `axum` (new, exact-pinned — Task 1 records the ADR), `tokio` (already in the workspace). No other new dependency.

**Design source:** `docs/superpowers/specs/2026-08-13-runtime-design.md` §5, §6.4, §7 (05a); `docs/architecture/SYSTEM_ARCHITECTURE.md` §3.1; D-039's "never a second path" rule.

---

## The multi-agent contract this plan must get right

The owner's articulated vision: several agents on one project, operating GraphHelm through MCP/CLI, checking and writing documentation, sharing information, the harness in sync. 05a delivers the *coordination substrate* for that vision and is honest about what it does not:

- **Agents share state through the event log, not through files.** One agent signals, approves, pauses; the others see it in their next `status`/`events` read. Signals are the sharing mechanism the harness already defines — an agent that discovers something emits a signal, and it is recorded, attributed and visible.
- **Every mutation is attributed.** The caller declares who it is; agents write as `PersistedActorType::Agent` with their own id, the owner as `Owner`. The 04f actor split (sovereignty vs machinery) extends naturally: agent vs owner vs the driver's own hops.
- **Conflicts are explicit.** Two agents mutating the same execution race on the store's head sequence; the loser gets a 409 carrying the current head and re-reads — never a torn write (the store's exclusive append lock already guarantees that part; M03 proved it).
- **A documentation surface is NOT in this plan.** "Verificar doc / escrever doc" as first-class Living Documentation endpoints belongs to the Living Docs subsystem, which does not exist yet; §3.1 reserves `documents/claims/artifacts` in the API surface for it. Pretending a doc store into 05a would fork that design. What agents get today is the event/signal/status substrate; the doc surface arrives as its own plan and slots into this same API. This is stated in the milestone doc, not silently dropped.

## Hard-won process rules, binding on every task

- **The code wins over this plan**; report every discrepancy.
- **Revert sabotage from a `cp` backup, never `git checkout --`.**
- **Run the workspace clippy** (`cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings`).
- **Write documentation from the code as built; re-read the whole milestone file afterwards.**
- **Every new guard observed failing once, deliberately.**
- **A new integration-test binary must be added to `ci/gate.ps1` explicitly** — the gate lists CLI test binaries by name, and a new one is silently excluded until added. This bit 04f; do not repeat it.
- No schema or wire-vocabulary changes are expected. If a task believes it needs one, stop and report NEEDS_CONTEXT.

## File structure

| File | Responsibility |
|---|---|
| `docs/reference/REFERENCE_STACK_AND_ADRS.md` | The HTTP-stack ADR (axum, exact-pinned, rationale) |
| `apps/cli/src/commands/serve/mod.rs` | Router, auth, envelope/error mapping, shared state |
| `apps/cli/src/commands/serve/routes.rs` | The endpoint handlers, each delegating to the execution command layer |
| `apps/cli/src/commands/execution/…` | Command `execute()` internals become callable with an explicit actor (small, mechanical widening) |
| `apps/cli/tests/api_http.rs` | The API suite, including the multi-agent concurrency storm |
| `ci/gate.ps1` | New `cli: api_http` stage |

## The endpoint contract (normative for every task)

All under `/v1`, all JSON, all replying the standard four-key envelope (`ok`, `command`, `data`, `diagnostics`) exactly as the CLI prints it. `Authorization: Bearer <token>` required on everything but `/health`.

| Method and path | Command | Mutating |
|---|---|---|
| `GET /health` | — (`{"ok":true,"command":"serve.health",...}`) | no |
| `GET /v1/executions/{id}` | `execution.status` | no |
| `GET /v1/executions/{id}/events?after=N&limit=M` | events tail (raw envelopes, ordered by sequence, `after` exclusive) | no |
| `POST /v1/executions/{id}/start` | `execution.start` | yes |
| `POST /v1/executions/{id}/signal` | `execution.signal` | yes |
| `POST /v1/executions/{id}/approve` | `execution.approve` | yes |
| `POST /v1/executions/{id}/pause` | `execution.pause` | yes |
| `POST /v1/executions/{id}/resume` | `execution.resume` | yes |
| `POST /v1/executions/{id}/cancel` | `execution.cancel` | yes |

Mutation request headers, all validated before anything touches the store:

- `Idempotency-Key: <OpaqueId>` — **required**. Every event a command appends derives its event-level idempotency key deterministically from this command key plus a fixed per-event suffix, replacing the CLI's per-invocation UUIDs on the API path. A byte-identical retry therefore re-derives the same event keys, and the store's own duplicate-key enforcement makes the retry append nothing new.
- `X-GraphHelm-Actor: <ActorId>` — **required**. The actor recorded on every owner/agent-initiated event this command appends. Validated against the `ActorId` pattern; 400 on failure.
- `X-GraphHelm-Actor-Type: owner | agent` — **required**; maps to `PersistedActorType::{Owner, Agent}` (`Human` and `System` are not accepted from the wire: humans come later with Studio auth, and `System` is reserved for the driver's own hops, which keep their 04f attribution unchanged).
- `If-Match: <head-sequence>` — **optional**. When present, the command is refused with **409** unless the stream's current head sequence equals it, and the 409 body's `data` carries `{"currentHead": N}` so the caller re-reads and retries. When absent, the command still cannot tear (the store serializes appends); it merely accepts interleaving.

Failure mapping: argument/validation problems → 400 with the existing `GHCLI001` diagnostics; auth → 401 (constant-time token comparison); precondition refusals the commands already produce (`GHCLI005` etc.) → 409; store conflicts (`SequenceConflict`, duplicate idempotency key on a *partial* retry) → 409 with `currentHead`; everything else → 500 with the redaction-safe internal envelope. **No path, DSN or token ever appears in a response** — the 04f `Failure` discipline extends to HTTP bodies verbatim.

The request **bodies** reuse each command's existing argument shape (`file`, `fixtures`, `mode`, `node`, `signal` envelope inline, `evidenceOut` path) — read each command's `execute()` signature and mirror it as camelCase JSON; where a CLI argument is a filesystem path the operator supplied, the API accepts the same path string (the server is local by construction, D-040's premise).

---

### Task 1: The ADR, the skeleton and auth

**Files:**
- Modify: `docs/reference/REFERENCE_STACK_AND_ADRS.md`, `Cargo.toml` (workspace deps), `apps/cli/Cargo.toml`, `apps/cli/src/args.rs`, `apps/cli/src/commands/mod.rs`
- Create: `apps/cli/src/commands/serve/mod.rs`
- Test: `apps/cli/tests/api_http.rs`

- [ ] **Step 1: Write the failing test**

Create `apps/cli/tests/api_http.rs`. First helper and tests, modelled on `execution_cli.rs`'s process-spawning pattern:

```rust
/// Spawns `graphhelm serve` on an ephemeral port with a fresh events dir, waits for /health,
/// and returns (child-guard, base_url, token). The token comes from the file the server writes
/// into the events directory on first start; the guard kills the child on drop.
fn serve(events: &Path) -> (ServerGuard, String, String) {
    // Spawn the binary exactly as execution_cli.rs's command() helper does, with ["serve",
    // "--events", ..., "--bind", "127.0.0.1:0"]; parse the bound port from the server's own
    // startup envelope on stdout; read the token file from the events dir; poll /health until it
    // answers. ServerGuard wraps the Child and kills it on Drop.
}

#[test]
fn health_answers_without_auth_and_everything_else_refuses_without_the_token() {
    let directory = tempfile::tempdir().unwrap();
    let (_guard, base, token) = serve(&directory.path().join("events"));

    let health: serde_json::Value = get_json(&format!("{base}/health"), None);
    assert_eq!(health["ok"], true);
    assert_eq!(health["command"], "serve.health");

    let unauth = get_status(&format!("{base}/v1/executions/exec-a"), None);
    assert_eq!(unauth, 401);
    let wrong = get_status(&format!("{base}/v1/executions/exec-a"), Some("not-the-token"));
    assert_eq!(wrong, 401);
    let with = get_status(&format!("{base}/v1/executions/exec-a"), Some(&token));
    assert_ne!(with, 401, "the real token must pass auth (404/409 later is fine)");
}
```

Write `get_json`/`get_status` over `std::net::TcpStream` with hand-rolled minimal HTTP/1.1 — **no HTTP client dependency**; the requests this suite needs are small, and adding `reqwest` would drag TLS stacks the purity-conscious workspace does not want. If hand-rolling proves disproportionate mid-task, stop and report NEEDS_CONTEXT with the concrete pain rather than silently adding a client crate.

- [ ] **Step 2: Run to verify it fails** (`unrecognized subcommand 'serve'`), quoted.

- [ ] **Step 3: The ADR and the dependency**

Add `axum` to `[workspace.dependencies]`, exact-pinned to the newest version that builds on Rust 1.97.1 (verify by building; report the version you pinned — the proptest incident rule). Record the ADR in `docs/reference/REFERENCE_STACK_AND_ADRS.md` following that file's existing ADR format: chosen (axum over hand-rolled hyper and over actix), rationale (tower ecosystem, tokio already present, the register's replaceability rules — the server is an adapter and must stay swappable), and the constraint that no handler may hold state beyond the shared `ServeState`.

- [ ] **Step 4: The skeleton**

`apps/cli/src/commands/serve/mod.rs`: `pub fn run(events: &Path, bind: &str) -> Outcome` that

1. Creates or loads the bearer token: a `token` file inside the events directory, created with the same owner-restrictive file conventions the M03 events config enforces (read `apps/cli/src/commands/events/config.rs` and reuse its validation helpers; if its checks are read-side only, create the file and then validate it through the same path, and report what the convention actually was). The token is 32 bytes from the OS RNG, hex-encoded — the CLI binary may use OS randomness; the purity rules bind the core crates, not the operator binary. On every request but `/health`, compare in constant time (`subtle` is not in the workspace — a hand-written fold over bytes with `|=` is fine and matches the D-038 minimalism precedent).
2. Builds the axum router with `/health` and a catch-all 404 in the standard envelope, wraps it in the auth layer, binds to `bind` **refusing any host that is not a loopback address** (a non-loopback bind is a 04-style fail-closed refusal, `GHCLI006_SERVE_INVALID` — new code, declared in `serve/mod.rs` beside its consumers), and runs on the existing `runtime()` helper from `commands/events/mod.rs`.
3. Registers `serve` in `args.rs`/`commands/mod.rs` exactly as the other subcommands do.

- [ ] **Step 5: Green, then sabotage the auth** — make the token comparison accept any token transiently; the 401 assertions fail; revert from a backup; re-confirm. Quote both.

- [ ] **Step 6: Verify and commit**

`cargo +1.97.1 test -p graphhelm-cli --locked`, workspace clippy, fmt — quoted.

```bash
git add docs/reference/REFERENCE_STACK_AND_ADRS.md Cargo.toml Cargo.lock apps/cli
git commit -m "feat(cli): serve the runtime API skeleton with local auth"
```

---

### Task 2: The read surface — status and the events tail

**Files:**
- Create: `apps/cli/src/commands/serve/routes.rs`
- Modify: `apps/cli/src/commands/serve/mod.rs`, `apps/cli/src/commands/execution/mod.rs` (visibility only)
- Test: `apps/cli/tests/api_http.rs`

- [ ] **Step 1: Failing tests** — start an execution through the **CLI** against the same events dir the server is watching (the two share the store by design), then:

```rust
#[test]
fn status_over_http_matches_the_cli_and_the_events_tail_pages() {
    // CLI start against <events>, then:
    let status = get_json(&format!("{base}/v1/executions/{exec}"), Some(&token));
    assert_eq!(status["command"], "execution.status");
    assert_eq!(status["data"], cli_status_data, "one store, one truth");

    let first = get_json(&format!("{base}/v1/executions/{exec}/events?limit=3"), Some(&token));
    let events = first["data"]["events"].as_array().unwrap();
    assert_eq!(events.len(), 3);
    let last_seq = events[2]["sequence"].as_u64().unwrap();
    let rest = get_json(
        &format!("{base}/v1/executions/{exec}/events?after={last_seq}&limit=1000"),
        Some(&token),
    );
    let tail = rest["data"]["events"].as_array().unwrap();
    assert!(tail.first().unwrap()["sequence"].as_u64().unwrap() > last_seq);
    assert_eq!(rest["data"]["head"], status["data"]["headSequence"]);
}
```

- [ ] **Step 2: Implement.** `status` delegates to the same `load_projection` + `render` the CLI uses — widen `pub(super)` to `pub(crate)` on exactly the helpers the server needs, nothing more, and list each widening in your report. The events tail reads the store's replay stream (the same read the CLI's replay path uses), slices by `after`/`limit` (default 100, max 1000 — refuse larger with 400, never truncate silently: return `{"head": N}` so the caller knows there is more), and serializes the raw envelopes — their payloads are safe by construction (D-036: no free-form content on the wire), which is what makes serving them verbatim legitimate. Add `headSequence` to `render()`'s output if status does not already expose it — the `If-Match` workflow needs it, and the CLI gains it for free (report this as the one shared-surface change).

- [ ] **Step 3: Green, verify, commit**

```bash
git add apps/cli
git commit -m "feat(cli): serve status and the paged events tail"
```

---

### Task 3: Actor-attributed mutations — signal, approve

**Files:**
- Modify: `apps/cli/src/commands/serve/routes.rs`, `apps/cli/src/commands/execution/{signal,approve}.rs` (and `mod.rs`)
- Test: `apps/cli/tests/api_http.rs`

The command internals currently hard-code `owner_actor()`. Widen each `execute()` with an explicit `actor: PersistedActor` parameter (the CLI's `run()` keeps passing `owner_actor()`, so CLI behaviour is byte-identical); the server builds the actor from the two headers. This is the mechanical widening the file table names — it must not change any command's logic.

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn a_signal_over_http_is_attributed_to_the_calling_agent() {
    // start via CLI; then POST /signal with X-GraphHelm-Actor: agent-planner, type agent,
    // Idempotency-Key: sig-cmd-1, body carrying the signal envelope inline and an evidenceOut
    // path inside the temp dir.
    assert_eq!(reply["data"]["decision"], "requires_approval");
    // The appended signal_recorded event's actor, read back through the events tail:
    let event = last_event_of_kind(&base, &token, &exec, "signal_recorded");
    assert_eq!(event["actor"]["actorType"], "agent");
    assert_eq!(event["actor"]["id"], "agent-planner");
}

#[test]
fn a_retried_mutation_with_the_same_idempotency_key_appends_nothing() {
    let before = head_sequence(&base, &token, &exec);
    post_signal(/* same Idempotency-Key, same body */);
    let first_head = head_sequence(&base, &token, &exec);
    assert!(first_head > before);
    let retry = post_signal(/* identical again */);
    assert_eq!(retry_status, 200, "a full retry is success, not conflict");
    assert_eq!(head_sequence(&base, &token, &exec), first_head, "and appends nothing");
}

#[test]
fn missing_or_invalid_actor_headers_are_400_before_the_store_is_touched() { /* three cases:
    no Idempotency-Key, no actor, actor type "system" — each 400, head unchanged */ }
```

Check the exact actor JSON field names against `PersistedActor`'s serde derives before asserting — the plan guesses `actorType`/`id`; the code wins.

- [ ] **Step 2: The idempotent-retry semantics, precisely.** Event keys derive as `{command-key}-{fixed-suffix}` per appended event (e.g. `sig-cmd-1-record`). On retry the derived keys already exist; **verify what the store actually does with a duplicate key** — the M03 suites treat it as a rejected append. Map that rejection to *idempotent success*: catch the duplicate-key error, re-read state, and reply 200 with current state, because the caller's command *has already happened*. A **partial** overlap (some keys exist, some do not — possible only if a previous attempt died mid-command) must NOT silently half-apply: detect it (first append rejected but state shows the command incomplete, or vice versa) and reply 409 with a diagnostic naming the stuck command key. Write this logic once in `serve/mod.rs` and route every mutation through it. Report the store's actual duplicate behaviour as a checkpoint before building on it.

- [ ] **Step 3: `approve` the same way** — attributed, idempotent, its existing `GHCLI005` refusals mapped to 409.

- [ ] **Step 4: Sabotage** — derive event keys with a fresh UUID (the CLI's old way) transiently; the retry test fails with a doubled head; revert from a backup; re-confirm. Quote both.

- [ ] **Step 5: Verify, commit**

```bash
git add apps/cli
git commit -m "feat(cli): attributed idempotent mutations over the API"
```

---

### Task 4: The lifecycle mutations and `If-Match`

**Files:**
- Modify: `apps/cli/src/commands/serve/routes.rs`, `apps/cli/src/commands/execution/{start,pause,resume,cancel}.rs`
- Test: `apps/cli/tests/api_http.rs`

- [ ] **Step 1: Failing tests** — `start` over HTTP (body: `file`, `fixtures`, `mode`; refuses a second start with 409); `pause`/`resume`/`cancel` attributed and idempotent like Task 3; and the version guard:

```rust
#[test]
fn if_match_refuses_a_stale_head_with_the_current_one() {
    let head = head_sequence(&base, &token, &exec);
    let stale = post(&format!(".../pause"), if_match(head - 1), ...);
    assert_eq!(stale.status, 409);
    assert_eq!(stale.body["data"]["currentHead"].as_u64().unwrap(), head);
    let fresh = post(&format!(".../pause"), if_match(head), ...);
    assert_eq!(fresh.status, 200);
}
```

- [ ] **Step 2: Implement.** `If-Match` is checked against the store head immediately before the command runs; the small race between check and append is closed by the store's own serialization plus the derived idempotency keys (state it in a comment — the check is advisory-fast-fail, the store is the authority). `resume` takes `file`/`fixtures` in the body exactly as the CLI takes them, same trust seam, same documentation pointer.

- [ ] **Step 3: Sabotage** — skip the `If-Match` comparison transiently; the stale-head test fails; revert; re-confirm. Quote both.

- [ ] **Step 4: Verify, commit**

```bash
git add apps/cli
git commit -m "feat(cli): lifecycle mutations with optimistic concurrency over the API"
```

---

### Task 5: The multi-agent storm

**Files:**
- Modify: `apps/cli/tests/api_http.rs`

The test this plan exists for. Two scenarios, both through real HTTP against one server:

- [ ] **Step 1: Information sharing.** Agent A (`agent-scout`) signals `unexpected_dependency`; agent B (`agent-builder`) polls the events tail and must observe A's `signal_recorded` with A's attribution, then B approves the blocked node (produced by a failing fixture) and A observes B's approval the same way. The assertion is the loop closing: **each agent learns of the other's act only through the API**, nothing shared but the URL and token.

- [ ] **Step 2: The storm.** From `N = 8` OS threads, hammer one execution concurrently for a fixed number of rounds: interleaved `pause`/`resume`/`signal`/`status` with per-thread actors, unique command keys per logical act, retries on 409 (re-read head, retry once). Then assert:
  1. every reply was 200, 400, or 409 — never 500, never a hung connection;
  2. the final projection is coherent (replay succeeds — the fold's own guards are the oracle: no corrupt history was ever appendable);
  3. `graph replay` twice over the stream is byte-identical (the milestone's replay guarantee survived concurrency);
  4. every mutation event's actor is one of the eight thread actors or the system driver — nothing unattributed.

The genuinely load-bearing assertion is 2: the fold rejecting incoherent histories at replay time means a raced append that would corrupt state **could not have been appended**. If the storm ever produces a corrupt replay, that is a real store-or-command bug, not test flakiness — report it as a finding, do not widen tolerances.

- [ ] **Step 3: Prove the storm can fail** — transiently disable the duplicate-key derivation (fresh UUIDs) *and* the pause-status precondition in one command; the storm must catch a double-apply or an incoherent history. Revert both from backups; re-confirm green three consecutive runs (concurrency tests earn their keep only when stable — three green runs is the bar; if it flakes, fix the test's own synchronization, never loosen assertion 2).

- [ ] **Step 4: Commit**

```bash
git add apps/cli
git commit -m "test(cli): prove the API holds under a multi-agent storm"
```

---

### Task 6: CLI-parity guard and the gate stage

**Files:**
- Modify: `apps/cli/tests/api_http.rs`, `ci/gate.ps1`

- [ ] **Step 1: The parity guard.** D-039's rule made testable: for one scripted story (start → signal → approve → pause → resume → status), drive it once through the CLI and once through the API against two fresh stores, and assert the final `status` `data` payloads are **identical** except for fields that legitimately differ (list them explicitly in the test if any exist; the goal is an empty list). This is the "never a second path" constraint as a regression test — when 05d swaps the driver, this test is what proves the surfaces did not drift.

- [ ] **Step 2: The gate stage.** Add `cli: api_http` to `ci/gate.ps1`'s CLI suite list, same invocation shape. Prove the stage can fail (transient assertion break, the stage's own cargo command goes red, revert from backup, re-confirm). Quote both.

- [ ] **Step 3: Verify, commit**

```bash
git add apps/cli ci/gate.ps1
git commit -m "test(cli): pin CLI-API parity and gate the API suite"
```

---

### Task 7: Documentation and the full gate

**Files:**
- Create: `docs/milestones/runtime.md` (Milestone 05 opens its own document; the 04 one is closed and stays untouched)
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Write from the code as built.** `docs/milestones/runtime.md` opens the Milestone 05 record: 05a's surface table, the header contract, the idempotent-retry and If-Match semantics as implemented, the storm's guarantees, and — explicitly — the honesty section: the documentation surface for agents ("verificar doc / escrever doc") is **not here**; agents coordinate through events, signals and status until the Living Docs plan lands, and that plan slots into this same API per §3.1. State the auth limit plainly too: bearer token on loopback, mTLS deferred (the architecture's full §3.1 posture), non-loopback binds refused.

- [ ] **Step 2: CHANGELOG** entry in the established voice.

- [ ] **Step 3: The full gate.** `./ci/gate.ps1` with `GRAPHHELM_PG_BIN` set — now including `cli: api_http`. Expected `[gate] GREEN - every stage passed.` A red on the tracked flakes (#19) is re-run once with the flake noted; any other red is a defect.

- [ ] **Step 4: Commit**

```bash
git add docs CHANGELOG.md
git commit -m "docs(m05): open the runtime milestone with the API surface"
```

---

## Definition of done

- `graphhelm serve` binds loopback-only with token auth (constant-time compare, sabotage-proven), refusing non-loopback binds fail-closed.
- Every CLI execution command has an API counterpart replying the identical envelope; the parity guard pins it.
- Every mutation is attributed (`Owner`/`Agent` from headers, `System` reserved for the driver), idempotent under full retry (same command key ⇒ nothing appended, 200 with current state), and explicit about partial retries (409 naming the stuck key).
- `If-Match` gives optimistic concurrency with the current head in every 409.
- The events tail pages deterministically and never truncates silently.
- The multi-agent storm holds: no 500s, coherent replay, byte-identical double replay, full attribution — stable across three consecutive runs, sabotage-proven capable of catching a double-apply.
- The two-agent information-sharing loop closes through the API alone.
- `cli: api_http` runs in the gate, proven able to fail; the full gate is green, PostgreSQL matrix included.
- `docs/milestones/runtime.md` opens Milestone 05 and names the doc-surface gap honestly.

## What this plan deliberately excludes

The MCP server (05e — it consumes this API), the monitor page (05f), the async driver (05d — this server rides the synchronous command layer until then), the gateway and tool broker (05b/05c), mTLS and non-local identity (deferred with the architecture's full posture, recorded), SSE/streaming events (polling suffices until the monitor needs push), and the Living Documentation surface — named in the milestone doc as the next coordination increment for the multi-agent vision, not silently absent.
