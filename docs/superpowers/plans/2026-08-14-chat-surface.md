# Milestone 05e — The Chat Surface Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** GraphHelm operable from inside a coding-agent chat (D-039): a stateless MCP server over stdio whose tools map 1:1 onto Runtime API requests, the serve layer's small gateway read-surface addition that keeps `routes`/`probe` off the second-path list, the §5 multi-agent choreography and §6 parity/credential constraints as named tests, and thin deletable packaging (a Claude Code plugin carrying the first two operator skills, a Codex configuration snippet).

**Architecture:** A new `graphhelm mcp` subcommand in `apps/cli` — no new crate: the server is an adapter, and adapters over the API live where `serve` lives. Hand-rolled minimal JSON-RPC 2.0 over newline-delimited stdio (ADR-026; the tools-only MCP surface is small and closed, and the official Rust SDK's footprint is the revisit trigger, not the default), reusing `HttpTransport`/`UreqTransport` from `adapters/model-gateway` for the API client — zero new third-party dependencies. Statelessness per runtime-design §6.4: no store handle, no driver; the only in-process state is the session nonce and the client config. Notifications are pull: the `events` tool is the tail; no SSE exists and none is pretended.

**Tech Stack:** Rust 1.97.1 (pinned), serde/serde_json, std stdio/threads, `ureq =3.4.0` via the existing transport (ADR-025). MCP protocol revision pinned in code (see Task 2). No new dependency (ADR-026 records why).

**Design sources:** `docs/superpowers/specs/2026-08-13-runtime-design.md` §3 (D-039), §6.4, §7 bullet 05e, §8–9; `docs/ux/CHAT_SURFACE_SPEC.md` §2 (layering), §3 (the tool vocabulary — this plan's scope), §4 (skill catalog — 4.3/4.4 in, the rest deferred with reasons), §5 (choreography → tests), §6 (constraints), §7 (acceptance); `docs/milestones/runtime.md` (the 05a endpoint contract, three mutation headers, digest idempotency, If-Match, parity precedent; the 05b gateway CLI surface); `docs/DECISION_REGISTER.md` D-039/D-016/D-019/D-020/D-036; `docs/architecture/SYSTEM_ARCHITECTURE.md` §3.1. Issue: #43.

---

## Binding process rules (every task, no exceptions)

1. **The code wins over the plan.** Written against main `ceefcdb` (05b merged). Everything that depended on unmerged 05c (#25) or 05d (#26) was tagged `[RECONCILE]` and burned down in Task 0. **Task 0 ran against main `a8d40ef` (05c+05d merged)**: every tag below is rewritten to the real form; the registry check confirmed GHCLI015 free (05d used GHCLI016 for the driver-failure code, respecting the reservation) and no #35 spec-debt trigger fires on 05e.
2. **This plan adds NO event kinds and touches NO schema.** The MCP layer appends nothing itself — every mutation is an HTTP request to the API, which owns idempotency and attribution. If a task believes otherwise, STOP and report NEEDS_CONTEXT citing D-037.
3. **Never a second path, in both directions** (D-039, CHAT_SURFACE_SPEC §2): every MCP tool maps to exactly one documented API request; and where the API lacks a surface the CLI has (`gateway routes`/`probe`), the API grows it (Task 4) rather than the MCP layer shelling out. Deleting the wrappers must lose nothing but convenience (§7 acceptance).
4. **Statelessness (§6.4):** the MCP process holds no store handle and no driver — its state is the session nonce, the parsed config, and nothing else. A closed conversation loses nothing the API cannot reconstruct (§5).
5. **No secret through chat (§6):** the server exposes NO credential-accepting tool (omission is the enforcement), refuses secret-shaped tool arguments by prefix without echoing them, and never prints the bearer token anywhere. Tests plant sentinels.
6. **Workspace clippy is the bar:** `cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings` clean at every commit.
7. **Every new guard observed failing once** — sabotage or bad input, watch the named test fail, restore from `cp` backup, never `git checkout --`.
8. **New CLI test binaries added to `ci/gate.ps1` by name** (`ci/gate.ps1:106`; confirmed on merged main exactly as expected: `('cli_smoke','schema_cli','event_store_cli','execution_cli','api_http','gateway_cli','tool_cli','runtime_http')`); this plan adds `mcp_stdio`.
9. **Run `cargo +1.97.1 fmt --all` before every commit.** Commit per task: `type(scope): description`, why in the body, trailer `Co-Authored-By: Claude Code <noreply@anthropic.com>`.

## Coordination note

Until 05c/05d implementation completes, this branch touches exactly one file: this document. Shared hotspots (workspace `Cargo.toml` — untouched by this plan entirely, no new crate — `ci/gate.ps1`, `docs/milestones/runtime.md`, `CHANGELOG.md`, `apps/cli/src/commands/mod.rs`) change only in the implementation phase. Announce any full gate on issue #43 first; one gate at a time on this machine.

## What already exists (do not rebuild)

- The Runtime API (05a, on main): loopback-only fail-closed serve, bearer token at the sibling path `<events-dir>.token`, `/health` unauthenticated, `ServeState` (`apps/cli/src/commands/serve/mod.rs:91`) holding config-not-handles, `require_token` middleware (`mod.rs:208`), `build_router` (`mod.rs:159`), `run_idempotent_mutation` in `serve/routes.rs` with the four-state content-aware retry (`{command-key}-{suffix}-{digest16}`), the three mutation headers (`Idempotency-Key` ≤64 chars, `X-GraphHelm-Actor`, `X-GraphHelm-Actor-Type: owner|agent`), optional `If-Match` with `currentHead` 409s, `headSequence` on mutation successes, the paged events tail (`after`/`limit`, max 1000). The CLI–API parity test with an empty exception list.
- The 05b gateway surface: `gateway routes|probe|credential` CLI commands and the command-layer functions they call (`apps/cli/src/commands/gateway/{routes,probe}.rs`) — Task 4 lifts routes/probe to HTTP by calling the SAME command layer; `HttpTransport`/`UreqTransport` (`adapters/model-gateway/src/transport.rs`, redirects hard-off, per-call timeout); the loopback-authority rule with userinfo stripping (`core/gateway/src/manifest.rs`, post-#36 form) to mirror for the MCP client's `--url`.
- The API test pattern: `apps/cli/tests/api_http.rs` — spawn the compiled binary, `TcpListener` fakes, token bootstrap, the storm/two-agent shapes. `assert_cmd::cargo::cargo_bin!("graphhelm")`.
- Failure-code registry, confirmed on merged main: GHCLI001–011 (through 05b), GHCLI012–014 (05c tool CLI), GHCLI016_DRIVER_FAILURE (05d serve driver). GHCLI015 is free exactly as reserved. This plan takes **GHCLI015_MCP_INVALID** (CLI-level argument/config failures of `graphhelm mcp` only — protocol-level errors are JSON-RPC error objects, never GHCLI envelopes).
- **Reconciled (Task 0, main `a8d40ef`)** 05d: `POST /v1/executions/{id}/pause` accepts `{"mode":"immediate"}` (graceful when absent) exactly as planned; serve DID gain the grouped runtime flags (`--manifest`/`--broker`/`--route`/`--staging` all-or-none; `--keyring`/`--key-id` all-or-none) plus `--tests-runner`/`--allow-program`/`--path-prepend`, and `ServeState` now carries `runtime`/`sealing`/`cancels`. The `pause` tool passes `mode` through to that merged shape.
- Spec-debt queue #35: Task 0 re-verifies the five entries' triggers against implemented main. As of this writing none fires on 05e (no drifting-tool cache, no simulator, no Governor-milestone entry, no claim lifecycle); Task 0 confirms and records that in its commit message.

## Scope decisions from CHAT_SURFACE_SPEC §3–§4, stated

**In (the §3 base):** tools `start`, `status`, `events`, `signal`, `approve`, `pause` (graceful + immediate passthrough), `resume`, `cancel`, `routes`, `probe`. **Deferred, stated:** `rules` and `document-impact` (§3 names them "when Living Documentation lands its API" — no such API exists; the milestone doc records the pointer). Skills: **4.3 `operate-execution` and 4.4 `observe-agents` ship in 05e** — both are pure choreography over surfaces that exist. Deferred with the missing dependency named: 4.1/4.2 onboarding (need the D-002 connect flow, template-assisted authoring guidance, and `docs/rules/` seeding — Living Docs adjacent), 4.5 `invoke-agent` (its non-overlapping half is D-020 approval nuance that becomes real with 05d's driver), 4.6 `share-context` (needs an Evidence-upload API surface; none exists), 4.7 `triage-and-approve` (needs a cross-execution listing endpoint the API does not have — recorded as the next API increment candidate), 4.8 `deploy-and-verify` (needs the QUALITY_GATES flow tooling). MCP capabilities beyond tools — resources, prompts, sampling, push notifications, `listChanged` — deferred; notifications are the `events` tool by design (pull, §6.4).

## File map

| Path | Responsibility |
|---|---|
| `apps/cli/src/commands/mcp/mod.rs` | `graphhelm mcp` entry: config parsing, GHCLI015, the serve loop |
| `apps/cli/src/commands/mcp/rpc.rs` | Newline-delimited JSON-RPC 2.0 framing, dispatch, error objects |
| `apps/cli/src/commands/mcp/session.rs` | Lifecycle: initialize/initialized, protocol version, capabilities, ping, EOF |
| `apps/cli/src/commands/mcp/client.rs` | The API client over `HttpTransport`: auth, headers, idempotency-key derivation |
| `apps/cli/src/commands/mcp/tools.rs` | The ten tool definitions: schemas, dispatch, secret-prefix guard |
| `apps/cli/src/commands/serve/routes.rs` (+ `mod.rs` router) | Task 4: `GET /v1/gateway/routes`, `GET /v1/gateway/probe` |
| `apps/cli/src/commands/mod.rs`, `src/args.rs` | Subcommand registration (hotspot — implementation phase) |
| `apps/cli/tests/mcp_stdio.rs` | The suite: conformance, lifecycle, tools, parity, choreography (new gate stage) |
| `apps/cli/tests/api_http.rs` | Task 4's gateway read-surface tests (05a file, Agente A-owned) |
| `examples/chat-surface/claude-code-plugin/…` | Plugin manifest, MCP registration, `skills/operate-execution/SKILL.md`, `skills/observe-agents/SKILL.md` |
| `examples/chat-surface/codex/config.toml` | Codex MCP registration snippet |
| `docs/reference/REFERENCE_STACK_AND_ADRS.md` | ADR-026 |
| `ci/gate.ps1` | `mcp_stdio` stage |
| `docs/milestones/runtime.md`, `CHANGELOG.md` | 05e record |

---

### Task 0: reconciliation — burn the `[RECONCILE]` tags, check the #35 queue ✅ (ran 2026-08-16 against main a8d40ef)

**Files:** this document only.

- [x] **Step 1:** With 05c and 05d on main, read the merged reality: the gate suite list in `ci/gate.ps1`, the GHCLI registry (05c's 012–014 as landed), 05d's pause-immediate request shape and any new serve flags, and `apps/cli/src/commands/gateway/{routes,probe}.rs` post-05d signatures. Rewrite every `[RECONCILE]` reference in this document to the real form; **check GHCLI015 nominally** (review finding): grep the merged tree for `GHCLI015` — 05d's resume cross-check may have taken it for its dedicated refusal code — and renumber this plan's code to the first free slot if so.
- [x] **Step 2:** Re-verify the five #35 spec-debt entries' triggers against implemented main; record in the commit message that none fires (or STOP and report if one now does — a drifting-tool cache arriving early would fire entries 1–2).
- [x] **Step 3: Commit** `docs(plan): reconcile the 05e plan against merged 05c and 05d`.

---

### Task 1: ADR-026 and the JSON-RPC layer

**Files:**
- Create: `apps/cli/src/commands/mcp/rpc.rs` (module registered in a minimal `mcp/mod.rs`)
- Test: `apps/cli/tests/mcp_stdio.rs` (unit-style tests may live in `rpc.rs`; the black-box conformance tests spawn the binary once Task 2 wires the subcommand — write them now against the module, promote in Task 2)

- [ ] **Step 1: ADR-026** in `docs/reference/REFERENCE_STACK_AND_ADRS.md`, following ADR-024/025's structure. Context: the chat surface needs an MCP server over stdio; the decision is the protocol stack. Decision: **hand-rolled minimal JSON-RPC 2.0 + MCP handshake, no SDK** — the surface this milestone needs is five methods and a closed tool list; the official Rust SDK (`rmcp`) brings an async-trait/tokio service stack whose value begins with resources, streaming HTTP transports, and sampling, none of which 05e ships (verify the current `rmcp` version and its dependency footprint with `cargo add rmcp --dry-run` + `cargo tree` on a scratch branch and RECORD the observed numbers in the ADR — the decision must cite the real footprint it declined, the ADR-024 method). Consequences: we own protocol-revision pinning (Task 2) and conformance tests; **the revisit trigger is named**: the first milestone needing MCP resources, push notifications, `structuredContent` tool results (reviewer note: hosts are moving toward schema-validated structured results — a second candidate the hand-rolled layer should not grow), or a non-stdio transport adopts the SDK instead of growing this layer.
- [ ] **Step 2: Failing conformance tests** (the wire contract, written against `rpc.rs` as a library first):

```rust
#[test]
fn a_request_round_trips_and_the_id_echoes_including_string_ids() {
    // {"jsonrpc":"2.0","id":"abc","method":"ping"} → reply id == json!("abc");
    // {"jsonrpc":"2.0","id":7,"method":"ping"} → reply id == json!(7).
}

#[test]
fn malformed_json_is_32700_and_unknown_method_is_32601_and_bad_params_is_32602() {
    // "{not json" → error.code -32700, id null;
    // known-shape request, method "no/such" → -32601, id echoed;
    // tools/call with params missing "name" → -32602.
}

#[test]
fn a_notification_never_gets_a_reply() {
    // {"jsonrpc":"2.0","method":"notifications/initialized"} (no id) → zero output lines.
}

#[test]
fn an_oversized_line_is_refused_without_reading_it_whole() {
    // A line > MAX_LINE_BYTES (1 MiB) → -32700-class error naming the bound, id null; the
    // reader must cap via take()-style bounded reads, not buffer the whole line first.
}
```

- [ ] **Step 3: Implement `rpc.rs`.** Newline-delimited JSON-RPC 2.0 over stdio (one message per line, UTF-8 — the MCP stdio transport's framing; no Content-Length headers). `RpcRequest { id: Option<serde_json::Value>, method: String, params: serde_json::Value }`, `RpcReply::{result, error}` builders, `const MAX_LINE_BYTES: usize = 1024 * 1024`, a bounded line reader, and a dispatch loop `run(reader, writer, handler)` where `handler: fn(&str, &Value, &mut SessionState) -> HandlerOutcome`. Error codes: -32700 parse, -32600 invalid request, -32601 method, -32602 params. Requests with `id` always get exactly one reply; notifications never do.
- [ ] **Step 4: green, fmt, workspace clippy. Sabotage:** reply to notifications too; the notification test fails; restore. **Commit** `feat(mcp): minimal JSON-RPC stdio layer (ADR-026)`.

---

### Task 2: the MCP lifecycle

**Files:**
- Create: `apps/cli/src/commands/mcp/session.rs`, extend `mcp/mod.rs` (subcommand entry), `apps/cli/src/commands/mod.rs` + `src/args.rs` (registration — hotspot, implementation phase is now)
- Test: `apps/cli/tests/mcp_stdio.rs` (black-box from here on: spawn `graphhelm mcp` with piped stdio, write request lines, read reply lines — build a small `McpHarness` helper in the test file)

- [ ] **Step 1: Failing tests:**

```rust
#[test]
fn initialize_negotiates_and_reports_tools_capability() {
    // initialize with protocolVersion "2025-06-18" and client capabilities {} →
    // result.protocolVersion == SUPPORTED_PROTOCOL_VERSION, result.capabilities.tools
    // exists (listChanged false/absent), result.serverInfo.name == "graphhelm".
    // A client requesting a DIFFERENT version gets our version back (the spec's rule:
    // the server answers with what it supports; the client decides). Never an error
    // for a mere version mismatch.
}

#[test]
fn tools_are_refused_before_initialize_and_work_after_initialized() {
    // tools/list before initialize → JSON-RPC error (the MCP lifecycle rule);
    // after initialize + notifications/initialized, tools/list returns the tool array.
}

#[test]
fn ping_pongs_and_eof_exits_cleanly() {
    // ping → {} result; closing stdin → process exits 0 promptly (< 5s), no panic output.
}
```

- [ ] **Step 2: Implement.** `SUPPORTED_PROTOCOL_VERSION: &str = "2025-06-18"` — a named constant with a doc comment saying it is the pinned MCP revision this server implements, verified against the official spec at implementation time (check modelcontextprotocol.io for the current revision and pin what Claude Code and Codex both accept; record the verification in the commit body — the code wins over this plan's constant). `SessionState { initialized: bool, nonce: String }` — the nonce is 8 bytes OS-random hex, generated at startup (the one impure input; the MCP layer is otherwise deterministic per request). `graphhelm mcp` subcommand entry parses config (Task 3), then `rpc::run` with the session handler.
- [ ] **Step 3: green, fmt, clippy. Sabotage:** allow tools/list pre-initialize; the lifecycle test fails; restore. **Commit** `feat(mcp): lifecycle, version pin and capabilities`.

---

### Task 3: the API client — config, auth, idempotency derivation

**Files:**
- Create: `apps/cli/src/commands/mcp/client.rs`, extend `mcp/mod.rs` (config)
- Test: `apps/cli/tests/mcp_stdio.rs`

Config: `graphhelm mcp --url <http://127.0.0.1:PORT> --token-file <path> --actor <id> [--actor-type agent]`. Token alternatively via env `GRAPHHELM_API_TOKEN` (flag wins; refuse BOTH absent with GHCLI015 naming the two options; the token value NEVER via argv). `--url` is loopback-only fail-closed — mirror the post-#36 `is_loopback_authority` rule INCLUDING userinfo stripping (write the check against the same bypass shapes; the 05b test list is the fixture source). `--actor` validated with the serve layer's actor rules; `--actor-type` defaults `agent` (the chat is an agent; `owner` allowed for an owner-driven chat).

- [ ] **Step 1: Failing tests:**

```rust
#[test]
fn a_non_loopback_url_is_refused_fail_closed_including_userinfo_shapes() {
    // --url http://api.example.com → exit nonzero, GHCLI015 before any request;
    // --url "http://[::1]@evil.com" and "http://localhost:tok@attacker.example" → refused
    // (the #36 blocker's shapes, applied to this surface).
}

#[test]
fn the_token_never_travels_via_argv_and_never_appears_in_output() {
    // Start with --token-file containing SENTINEL; drive one failing tool call (server
    // down) and one succeeding session; assert SENTINEL appears in NO stdout line and NO
    // stderr line of the MCP process, ever.
}

#[test]
fn idempotency_keys_are_stable_per_rpc_id_and_bounded() {
    // Unit-level: derive_key(nonce, id) for id json!(7), json!("abc"), a 32-char id (direct,
    // the boundary), a 33-char id (digested — the cutoff is exact), a 200-char string id
    // and a unicode id → each ≤ 64 chars, charset [a-z0-9-], stable across two calls,
    // distinct across distinct ids, and prefixed "mcp-{nonce}-". Long/unicode ids are
    // digested (16 hex of sha256), not truncated (truncation collides; digests do not).
}
```

- [ ] **Step 2: Implement `client.rs`.** `ApiClient { base_url, token: Zeroizing<String>, actor, actor_type, transport: UreqTransport }` (reuse `adapters/model-gateway`'s transport — redirects already hard-off, per-call timeout; set a 30s default). Methods per endpoint returning the API's envelope as `serde_json::Value` plus status. Mutations attach the three headers; `derive_key(nonce, rpc_id)`: `"mcp-{nonce}-{norm}"` where `norm` is the id itself when it is `[a-z0-9-]{1,32}` (a number's decimal form counts), else the first 16 hex of its SHA-256 (via `graphhelm_graph::raw_content_sha256` or `sha2` directly — match whichever `apps/cli` already links). The `32` is the proof-by-construction bound (review finding): `"mcp-" (4) + nonce (16 hex) + "-" (1) + 32 = 53 ≤ 64` in the direct arm and `21 + 16 = 37` in the digest arm — the ≤64 header cap can never be exceeded by any input, and the Task 3 test asserts at exactly 32 and 33 chars. §3's rule lands here: the key follows the LOGICAL act (the rpc id), so a client retry of the same id reuses the key and the API's content-aware machinery absorbs it — proven in Task 6.
- [ ] **Step 3: green, fmt, clippy. Sabotage:** derive the key with a fresh random per call; Task 6's retry test (write it now, let it fail here if convenient, or defer the observation to Task 6 and say so); restore. **Commit** `feat(mcp): loopback-only API client with per-act idempotency`.

---

### Task 4: the serve layer's gateway read surface — so routes/probe are not a second path

**Files:**
- Modify: `apps/cli/src/commands/serve/routes.rs`, `serve/mod.rs` (router entries)
- Test: `apps/cli/tests/api_http.rs` (new section; this is the 05a suite and Agente A's file — no 05c/05d conflict)

- [ ] **Step 1: Failing tests** (api_http patterns: live serve on a temp store, real token):

```rust
#[test]
fn gateway_routes_over_http_matches_the_cli_report_for_the_same_manifest() {
    // GET /v1/gateway/routes?manifest=<path> (auth required) → the same JSON data the CLI's
    // `gateway routes --manifest` prints for that manifest (spawn the CLI, compare the
    // envelope's data — the parity rule applied to the new surface). Invalid manifest →
    // 400 with the same redaction guarantees (a MARKER string in the file never appears).
}

#[test]
fn gateway_probe_over_http_is_quota_free_and_reports_the_cli_shape() {
    // GET /v1/gateway/probe?manifest=<path>&route=<id> [+ broker/keyring/keyId query params
    // for the credential check — passphrase STAYS an env var of the serve process, never a
    // query param] → checks/health identical to the CLI probe for the same inputs.
}
```

- [ ] **Step 1b: The auth assert, explicit** (review finding): both new endpoints answer **401 without a valid bearer token** — one test hitting each with no token and a wrong token. The 05a auth tests pinned only the routes that existed then; a router refactor that left these two outside `require_token` would pass every existing test today. This test closes that hole for the new surface.
- [ ] **Step 2: Implement.** Two GET routes behind `require_token`, calling the SAME command-layer functions the CLI subcommands call (extract the core of `gateway/routes.rs`/`probe.rs` into shared `pub(crate)` functions if they are currently command-shaped — report the extraction). Query-string inputs mirror the CLI flags; nothing mutates, so no idempotency headers. Reconciled decision (Task 0): 05d DID give serve `--manifest` — the HTTP gateway read surface prefers the server-configured manifest and treats the `manifest` query param as an explicit override; a fixture-only server (no `--manifest`) requires the query param or answers 400 naming it. **Reconciled again by #1083 (ADR-038): `routes` on a fixture-only server without the query answers `200` with `configured: false` and an empty list, because a listing has a true answer ("no routes"); `probe`, which needs a manifest to act on, keeps the `400`.**
- [ ] **Step 3: green (both new tests + the whole api_http suite), fmt, clippy. Sabotage:** route the HTTP handler through a hand-rolled second listing instead of the shared function; the parity-with-CLI test fails when a field is renamed in one place; restore. **Commit** `feat(serve): gateway routes and probe over HTTP`.

---

### Task 5: the ten tools

**Files:**
- Create: `apps/cli/src/commands/mcp/tools.rs`
- Test: `apps/cli/tests/mcp_stdio.rs`

- [ ] **Step 1: Failing tests** (harness: one live serve + one MCP process wired to it):

```rust
#[test]
fn tools_list_names_exactly_the_ten_tools_with_closed_schemas() {
    // start, status, events, signal, approve, pause, resume, cancel, routes, probe — and
    // NOTHING else: no credential tool exists by design (rule 5; §6 "no secret through
    // chat" — omission is the enforcement, this test is its guard). Every inputSchema has
    // "additionalProperties": false and every required field listed.
}

#[test]
fn each_tool_maps_to_exactly_one_api_request_and_returns_the_envelope() {
    // Per tool, against the real serve, assert on EFFECT + SHAPE (no counting middleware —
    // the store's head sequence is the observable): status returns the same data as a
    // direct GET; a signal via MCP appends exactly one decision event (the head advances
    // by the same delta the direct API call produces); the tool result content is
    // [{type:"text","text": <the API envelope JSON verbatim>}] with isError false on ok,
    // true on API failure (the envelope's error travels intact — codes like GHE003 reach
    // the chat, already redaction-safe by the API's own contract).
}

#[test]
fn pause_passes_mode_immediate_through() {
    // pause {executionId, mode:"immediate"} → the API receives the 05d body shape
    // (merged 05d shape, Task 0-confirmed); graceful default when mode absent.
}

#[test]
fn a_secret_shaped_argument_is_refused_and_never_echoed() {
    // signal with content containing "sk-ant-SENTINEL123" → JSON-RPC tool error naming the
    // rule ("secret-shaped value refused; use the broker's stdin path"), the SENTINEL
    // absent from every output line; same for "sk-proj-" and "-----BEGIN " prefixes. The
    // prefix list is a named const with a doc comment stating it is a deliberate, narrow
    // heuristic (CHAT_SURFACE_SPEC §6), not a scanner.
}
```

- [ ] **Step 2: Implement `tools.rs`.** A static tool table: name, description (each description names the API call it maps to — §6 parity in the tool's own metadata), JSON Schema (hand-written serde_json values, `additionalProperties: false`), and a dispatch fn onto `ApiClient`. Mutating tools take the rpc id through `derive_key`. `events` maps `after`/`limit` straight through (the API's bounds are the bounds; a bounds error comes back as the API's 400 envelope). SECRET_PREFIXES guard scans string arguments of every tool pre-dispatch.
- [ ] **Step 3: green, fmt, clippy. Sabotage:** add an eleventh tool ("credential-set") to the table; the closed-list test fails; restore. **Commit** `feat(mcp): the ten tools over the runtime API`.

---

### Task 6: parity, retry, and the §5 choreography

**Files:**
- Test: `apps/cli/tests/mcp_stdio.rs` (this task is tests-first by nature; fixes land where they fall)

- [ ] **Step 1: The parity test** (§6 of the spec, the 05a pattern):

```rust
#[test]
fn the_mcp_and_the_api_report_identical_status_for_the_same_story() {
    // Fresh store A: drive the 05a scripted story (start → signal → approve → pause →
    // resume → completion) entirely through MCP tools. Fresh store B: the same story
    // through direct HTTP. Compare final status envelopes' data with an EMPTY exception
    // list. This is D-039's sentence as a test, and the tripwire for every future serve
    // change under the MCP layer.
}
```

- [ ] **Step 2: The retry test:** same tools/call line (same rpc id, same arguments) written twice into the same session → the second reply is a success AND the store's head sequence is unchanged between them (the API's Complete state absorbed it); a second call with the SAME id but DIFFERENT arguments → the API's 409 Divergent travels back as a tool error naming the reused key. (The 05a machinery does the work; this proves the MCP derivation composes with it.)
- [ ] **Step 3: The choreography test** (§5 → §7 acceptance):

```rust
#[test]
fn two_chat_sessions_coordinate_through_events_alone_and_resolve_a_race() {
    // Two MCP processes, one serve, actors "scout" and "builder" (distinct nonces).
    // Scout signals; builder polls events until the signal appears (attributed to scout),
    // approves the blocked node; scout polls events and sees builder's approval.
    // Then the race: both sessions mutate with If-Match pinned to the same stale head —
    // exactly one 409s; the loser re-reads status and retries once, succeeding. Assert
    // full attribution on every mutation and zero non-events communication between the
    // two processes (the test itself is the only side channel, and it only passes ids).
}
```

- [ ] **Step 4: green, fmt, clippy. Sabotage (rule 7):** make `derive_key` ignore the rpc id (fresh key each call); the retry test's head-unchanged assertion fails; restore. **Commit** `test(mcp): parity, idempotent retry and two-session choreography`.

---

### Task 7: packaging — thin, deletable, validated

**Files:**
- Create: `examples/chat-surface/claude-code-plugin/.claude-plugin/plugin.json`, `.mcp.json` (server registration: command `graphhelm`, args `["mcp","--url","...","--token-file","..."]`), `skills/operate-execution/SKILL.md`, `skills/observe-agents/SKILL.md`, `README.md`
- Create: `examples/chat-surface/codex/config.toml` (the `[mcp_servers.graphhelm]` snippet), `examples/chat-surface/codex/README.md`
- Test: `apps/cli/tests/mcp_stdio.rs` (a validation section)

- [ ] **Step 1: Write the two skills** per CHAT_SURFACE_SPEC §4.3/§4.4, each stating what it reads, what it mutates, and **naming the exact tools/API calls it choreographs** (§6 parity rule). `operate-execution`: the start→watch→triage→act→report loop; immediate-stop explicit and confirmed; cancel states its §13 partial-effects consequence before acting; the credential-refusal instruction verbatim ("a pasted secret is refused, never echoed — use `graphhelm gateway credential set`'s stdin path"). `observe-agents`: read-only actor timeline from `events` attribution; flags 409/stale-If-Match conflicts; never acts as another actor.
- [ ] **Step 2: Validation tests:** parse `plugin.json`/`.mcp.json`/`config.toml` (serde_json + a minimal TOML check — `toml` is NOT a workspace dep; validate the TOML by line-shape assertions rather than adding a dependency, and say so in a comment); assert every tool name referenced in both SKILL.md files exists in the Task 5 tool table (grep-extract `` `tool:` ``-marked names — define the marker convention in the skills); assert the credential-refusal sentence is present in `operate-execution`; assert the plugin registers the server with `--token-file` and NOT an inline token.
- [ ] **Step 3: The deletability sentence** (§7) goes in both READMEs: everything the plugin does is `graphhelm mcp` + documented CLI calls; deleting it loses convenience only. **Commit** `feat(chat): claude code plugin and codex packaging with the first two skills`.

---

### Task 8: the gate stage and the CLI regression

**Files:**
- Modify: `ci/gate.ps1` (suite list + `'mcp_stdio'`)

- [ ] **Step 1:** Add `'mcp_stdio'` to the CLI suite list (the confirmed eight-suite form above). Prove the stage can go red: run the targeted cargo test with a misspelled suite name, observe the resolution failure, restore (the 05b pattern).
- [ ] **Step 2:** Full CLI regression: `cargo +1.97.1 test -p graphhelm-cli --locked` green across all suites (the api_http additions from Task 4 included), plus workspace fmt/clippy. **Commit** `chore(ci): mcp_stdio gate stage`.

---

### Task 9: documentation and the full gate

**Files:**
- Modify: `docs/milestones/runtime.md` (05e section), `CHANGELOG.md`, `docs/superpowers/specs/2026-08-13-runtime-design.md` status line if the 05c/05d records updated it (match their precedent), index docs only if they enumerate ADRs/crates (they did not as of 05b — re-check).

- [ ] **Step 1: Write the 05e section from the code as built** (open the sources; verify every claim; the plan is not the source). Cover: ADR-026 and what was declined with the recorded footprint; the stdio/JSON-RPC layer and the pinned protocol revision; statelessness as shipped (what the process actually holds); the ten tools and the 1:1 mapping; the serve gateway read surface and why it exists (rule 3); idempotency-per-logical-act and the retry proof; the parity and choreography tests by name; the packaging layout and the two shipped skills.
- [ ] **Step 2: Honest limits, named:** the eight deferred skills each with its missing dependency; `rules`/`document-impact` awaiting Living Docs; pull-only notifications (no SSE, no MCP push); no resources/prompts/sampling; the secret-prefix guard is a narrow deliberate heuristic, not a scanner; the protocol revision pin and the SDK revisit trigger; `probe` over HTTP inherits the CLI probe's quota-free scope (reachability, not model quota) **and its spawn surface** (reviewer note: a probe of a native route spawns the configured program with `--version` inside the serve process's context — the manifest is the trust boundary, and the honest-limits line must say the API now exercises it on request).
- [ ] **Step 3: Re-read the whole edited file top to bottom.** Every claim must match a test or line you can name.
- [ ] **Step 4: CHANGELOG entry** (mirror the 05b entry's shape). **Step 5: Full gate** — announce on #43 first (one gate per machine), `GRAPHHELM_PG_BIN` set, `./ci/gate.ps1` → GREEN including both PostgreSQL passes and `mcp_stdio`. Tracked flakes (#19) re-run once with a note; anything else is a defect. **Step 6: Commit** `docs(mcp): record milestone 05e as built`.

---

## Definition of done

- `tools/list` is exactly the ten §3 tools, schemas closed, no credential tool representable — guard-tested.
- Every tool call is one Runtime API request; the parity story via MCP and via HTTP yields identical status with an empty exception list; deleting the wrappers loses convenience only (§7).
- Idempotency keys follow the logical act: a same-id retry appends nothing; a same-id different-body call surfaces the API's 409 Divergent.
- Two MCP sessions coordinate through the events tail alone, fully attributed, and resolve an If-Match race by re-read + one retry (§5 as a test).
- A secret-shaped argument is refused without echo; the bearer token appears in no output line ever; the token never travels via argv.
- The MCP process holds no store handle and no driver; EOF exits 0 clean.
- `GET /v1/gateway/routes|probe` report exactly what the CLI reports for the same inputs, behind the same auth.
- The two shipped skills name their choreographed calls; validation tests pin skill-to-tool references and the credential-refusal text.
- `mcp_stdio` gates and was seen red once; the full gate is GREEN, PostgreSQL matrix included; zero new dependencies, zero event kinds, zero schema changes.

## What this plan deliberately excludes

The six deferred skills and the two Living-Docs tools (each named with its missing dependency in the milestone doc); MCP resources/prompts/sampling/push notifications and non-stdio transports (ADR-026's revisit trigger); SSE on the API; a cross-execution listing endpoint (named as the next API increment candidate for 4.7); any Studio surface (05f); hosted/remote MCP.

## Self-review notes (already applied)

- Spec coverage: CHAT_SURFACE_SPEC §2 layering ✔ (T5 tools / T7 wrappers), §3 vocabulary ✔ (T5, with the two Living-Docs tools deferred exactly as §3 conditions them), §4 catalog decision ✔ (4.3/4.4 in at T7; six deferred with named dependencies), §5 choreography ✔ (T6 test), §6 constraints ✔ (parity T6, statelessness rule 4/T2, no-secret T5+T7, D-016/D-019/D-020 live in skill text and the API they choreograph, D-036 untouched — the MCP layer appends nothing), §7 acceptance mapped: deletability (T7), two-session observation (T6), pasted-credential refusal (T5), skill-docs naming calls (T7 validation). Runtime-design §6.4 ✔ (rule 4). Onboarding-skill §7 rows are deferred WITH the catalog they belong to — recorded in honest limits, not silently dropped.
- Rule 3's both-directions reading is the one scope addition (Task 4) and it cites its authority (D-039 + §2's table naming tools as "mapping 1:1 onto Runtime API requests").
- Type consistency: `derive_key` defined once (T3), consumed by T5 dispatch and proven in T6; `SessionState` (T2) is the only state; the tool table (T5) is the single source the T7 validation greps against.
- Placeholder scan: the protocol-revision constant and the rmcp footprint are verify-and-record steps with defined defaults, not TBDs; Task 4's manifest-flag question was decided in Task 0 (server manifest preferred, query param overrides).
