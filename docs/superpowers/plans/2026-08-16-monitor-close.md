# Milestone 05f — The Monitor (D-040) and the Milestone 05 Close: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: executed via the two-agent pair loop (one writer at a time, TDD with observed reds, sabotage per guard, SPEC+QUALITY cross-review on the milestone issue). Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the read-only local monitor page (D-040) served from the Runtime binary, and close Milestone 05 with a self-verifying acceptance map plus the documented real-work acceptance run.

**Architecture:** The monitor is server-side-rendered zero-JavaScript HTML — a pure Rust function over the SAME status/events projection the CLI and the API already serve, refreshed by `<meta http-equiv="refresh">` with a `since` cursor in the refresh URL (stateless delta computation, no script for scope gravity to hook into). Auth is a one-hop bootstrap: `?token=` verified → HttpOnly cookie (the cookie IS the bearer token, re-verified per request) → 303 to the clean URL. The closure is a generated acceptance map: a gate-listed test walks the §8 clauses from a checked-in manifest, verifies each named prover test exists and its suite is in the gate, and diff-checks the emitted markdown.

**Tech Stack:** existing workspace only — axum (ADR-024), the serve command layer, no new dependencies, no JS toolchain (the page carries zero `<script>`).

**Design sources:** `docs/superpowers/specs/2026-08-13-runtime-design.md` §6.5 (D-040), §7 bullet 05f, §8 (acceptance), §9 (scope gravity risk); `docs/milestones/runtime.md` (05a–05e as built); `docs/ux/STUDIO_SPEC.md` (what the monitor must NOT become); D-036 (events are wire-safe by construction); the 05e plan's process rules. Issue: TBD at kickoff.

**Rules (the 05e set, restated):**
1. **The code wins over the plan.** Written against main `b602434` (05e merged). Task 0 burns any drift.
2. TDD per task: failing test observed and cited, then implementation, then sabotage per guard (red cited, restore from `cp` backup, never `git checkout --`).
3. `cargo +1.97.1 fmt --all` + workspace clippy `-D warnings` before every commit.
4. One writer at a time; SPEC-then-QUALITY review by the non-implementer; nothing starts without [APPROVED].
5. **D-040 is a refusal, not a feature:** any mutating affordance in the monitor is Studio's and is rejected in review by citing this line. The "refused scope" list (Task 6) makes the bans diff-visible.
6. Failure-code registry: GHCLI001–016 taken. The monitor introduces **no new GHCLI code**: its HTTP refusals reuse the serve vocabulary (401 unauthorized, 404 not found, 405 method not allowed), and `graphhelm status --html` failures are the existing `execution.status` family.
7. New CLI test binary `monitor_http` joins `ci/gate.ps1`'s suite list (making ten); `mcp_stdio` precedent.

**Files (map):**
- Create: `apps/cli/src/commands/serve/monitor.rs` (pure render + delta/staleness/blast-radius + the GET-only sub-router)
- Create: `apps/cli/src/commands/remediation.rs` (shared command-string renderer, CLI-owned so page and CLI cannot drift)
- Modify: `apps/cli/src/commands/serve/mod.rs` (router mount + cookie verifier beside `require_token`), `apps/cli/src/args.rs` + `commands/execution/status.rs` (`--html` dump), `ci/gate.ps1`
- Create: `apps/cli/tests/monitor_http.rs`, `docs/acceptance/m05-clauses.toml`, `docs/acceptance/M05_ACCEPTANCE_MAP.md` (generated), `docs/acceptance/m05-run-<date>/` (manual-run artifacts), `tools/acceptance-map/` (generator, dev-only workspace member)
- Modify (docs): `docs/milestones/runtime.md`, `CHANGELOG.md`, `docs/superpowers/specs/2026-08-13-runtime-design.md` status line, `docs/DECISION_REGISTER.md` only if D-040's text needs its "as implemented" note (match D-039's 05e precedent)

---

### Task 0: reconciliation

- [ ] **Step 1:** Read merged main: the gate suite list (nine suites expected), the serve router shape post-05e (`/v1/gateway/*` present), the GHCLI registry (016 last), `ServeState` fields, and the status projection structs the renderer will consume. Rewrite any stale reference in this document; confirm rule 6 (no new code needed) against the real refusal paths.
- [ ] **Step 2:** Re-check the #35 spec-debt triggers against 05f's scope (expected: none fire — the monitor adds no cache, simulator, governor, or claims surface).
- [ ] **Step 3: Commit** `docs(plans): reconcile 05f plan against merged main (Task 0)`.

### Task 1: the pure renderer — HTML as a third formatter over one truth

**Files:** create `apps/cli/src/commands/serve/monitor.rs`; test in-module + `apps/cli/tests/monitor_http.rs` (unit-style first; black-box promotes in Task 2).

- [ ] **Step 1: Failing tests:**

```rust
#[test]
fn the_monitor_page_contains_no_script_and_escapes_every_dynamic_string() {
    // render_monitor(&status_view_fixture_with_node_named("<img onerror=x>"), &events, now)
    // → output contains NO "<script" (case-insensitive), the node name appears only
    // HTML-escaped, and the <meta http-equiv="refresh"> tag is present with the since
    // cursor of the rendered head in its URL.
}

#[test]
fn the_monitor_renders_states_triage_and_tail_from_the_same_projection_fixture() {
    // The SAME StatusView fixture the status command tests use: every node state appears
    // exactly once, the triage list renders untriaged interruptions, the tail renders the
    // last N events with actor attribution ("ACTOR verb TARGET" line grammar).
}
```

- [ ] **Step 2: Implement** `fn render_monitor(view: &StatusView, events: &[EventEnvelope], since: u64, now: DateTime<Utc>) -> String`: inline `<style>`, no template engine, a local `fn escape(text: &str) -> String` for every dynamic string. Sections: header (execution id, aggregate status, head sequence), **delta strip** (transitions with sequence > `since` — computed from the tail, stateless because the refresh URL carries the cursor: `<meta http-equiv="refresh" content="2;url=/monitor/{id}?since={head}">`), node table, triage list, event tail.
- [ ] **Step 3:** green, fmt, clippy. **Sabotage:** drop the escape on node names; the no-script/escape test fails; restore. **Commit** `feat(monitor): pure zero-JS renderer over the status projection`.

### Task 2: the routes and the cookie bootstrap — read-only structurally

**Files:** modify `serve/mod.rs` (mount + verifier), `serve/monitor.rs` (handlers); test `apps/cli/tests/monitor_http.rs` (black-box: live serve, real browser-shaped requests over TcpStream).

- [ ] **Step 1: Failing tests:**

```rust
#[test]
fn the_bootstrap_url_sets_a_cookie_and_redirects_clean() {
    // GET /monitor/{id}?token=<real> → 303 Location /monitor/{id}, Set-Cookie HttpOnly;
    // SameSite=Strict; Path=/monitor. Following with the cookie → 200 text/html.
    // A WRONG token on bootstrap → 401 and NO cookie. The clean URL without cookie → 401.
    // The token never appears in the redirect Location nor in any page byte.
}

#[test]
fn the_monitor_surface_answers_405_to_every_mutating_verb() {
    // POST/PUT/DELETE/PATCH to /monitor and /monitor/{id} (with valid cookie) → 405,
    // and the CSP header "default-src 'none'; style-src 'unsafe-inline'" rides every 200.
}
```

- [ ] **Step 2: Implement.** A GET-only sub-router outside `/v1` (`/monitor`, `/monitor/{id}`): bootstrap branch verifies `?token=` against the same token bytes `require_token` uses (constant-time, same verifier function — ONE authority), sets the cookie whose value IS the token (re-verified per request against the file-loaded bytes; no session store, no second state), 303-redirects; steady-state branch reads the cookie, verifies, renders. `/monitor` lists executions (the store's stream ids) as links. CSP header on every response. Loopback-only is inherited from serve's bind rule.
- [ ] **Step 3:** green, fmt, clippy. **Sabotage:** register the sub-router with a POST route to a no-op handler; the 405 test fails; restore. **Commit** `feat(serve): read-only monitor routes with cookie bootstrap`.

### Task 3: silence, blast radius, and remediation-as-text

**Files:** create `apps/cli/src/commands/remediation.rs`; modify `serve/monitor.rs`; test `monitor_http.rs`.

- [ ] **Step 1: Failing tests:**

```rust
#[test]
fn staleness_blast_radius_and_remediation_render_for_a_stuck_story() {
    // A fixture story with one Failed node gating two downstream nodes and one node
    // Running with an old last-event timestamp:
    // - the Running node shows "last event {N}s ago" computed from the event tail;
    // - the Failed node shows blast radius "blocks 2 downstream; terminal unreachable
    //   through this path" (pure reachability over the projection's edges);
    // - beside the Blocked node: the EXACT string `graphhelm execution approve --events
    //   <dir> --execution <id> --node <name>` produced by remediation::render_invocation
    //   — asserted EQUAL to the string the function returns, not re-derived in the test.
}

#[test]
fn remediation_strings_are_the_cli_own_vocabulary() {
    // render_invocation round-trip: the emitted string, split into argv, PARSES under the
    // real clap Cli definition (Cli::try_parse_from succeeds) — the page and the CLI
    // compile from one source and cannot drift.
}
```

- [ ] **Step 2: Implement** `remediation::render_invocation(action, events_dir, execution, node) -> String` in the commands layer (owned by the CLI, consumed by the renderer), staleness from max event timestamp per node, blast radius as forward reachability O(V+E) with a terminal-reachability check. Node-type-aware staleness thresholds (agent nodes tolerate longer silence than tool nodes) using the node kind already in the projection.
- [ ] **Step 3:** green, fmt, clippy. **Sabotage:** hand-format the approve command in the renderer with a renamed flag; the parse-round-trip test fails; restore. **Commit** `feat(monitor): staleness, blast radius and remediation-as-text`.

### Task 4: the negative proof — the store cannot be moved through the monitor

**Files:** test `monitor_http.rs` (plus any fix it forces).

- [ ] **Step 1: Failing-first test** (expected green if Tasks 2–3 are honest — observe and record either way; the sabotage is the red proof):

```rust
#[test]
fn hammering_the_monitor_never_changes_a_byte_of_the_store() {
    // Seed a live story; hash every file under the events dir; fire at every monitor
    // route (enumerated by hitting both known paths AND asserting the router exposes
    // nothing else under /monitor via a 404 probe list): GET, HEAD, POST, PUT, DELETE,
    // PATCH, with and without cookie, with garbage bodies; re-hash; assert bit-identical
    // and the status JSON unchanged.
}
```

- [ ] **Step 2: Sabotage (the real red):** make one monitor handler append a marker event; the hash assertion fails; restore. **Commit** `test(monitor): negative read-only proof over store bytes`.

### Task 5: `graphhelm status --html` — the incident snapshot is the same code path

**Files:** modify `args.rs`, `commands/execution/status.rs`; test `execution_cli.rs`.

- [ ] **Step 1: Failing test:** `status --html` writes a self-contained page (no refresh tag — a frozen snapshot), byte-equal to `render_monitor` over the same projection with `since=0`, node names escaped; the normal JSON envelope still prints to stdout (the HTML goes to the `--html <path>` file).
- [ ] **Step 2: Implement** (thin: open store, build the same view, call the same renderer, `refresh: false` flag on render). **Step 3:** green, fmt, clippy, sabotage (snapshot diverges from the live renderer by a hand-edited literal; the byte-equal test fails; restore). **Commit** `feat(cli): status --html incident snapshot via the monitor renderer`.

### Task 6: the acceptance map generates itself

**Files:** create `tools/acceptance-map/` (dev-only workspace member), `docs/acceptance/m05-clauses.toml`, generated `docs/acceptance/M05_ACCEPTANCE_MAP.md`; test wired into the gate (workspace test, not CLI suite).

- [ ] **Step 1:** Hand-write `m05-clauses.toml`: the six §8 clauses, each with `tests = ["<fn name>", ...]`, `suite`, an `assert_fingerprint` (a distinctive assertion substring that must appear inside the test body — gutting the test breaks the map), and a human `rationale`. Include the **refused-scope table**: each banned monitor mutation (approve button, pause button, retry button, cancel button) citing the D-040 sentence, with a check that the quoted phrase still appears in the decision register.
- [ ] **Step 2: Failing test** `acceptance_map_is_grounded`: for every clause — the named `fn` exists exactly once in the tree, its suite appears in `ci/gate.ps1`'s invocation surface, the fingerprint substring appears in the test body, every D-citation resolves — and the committed `M05_ACCEPTANCE_MAP.md` is byte-identical to what the generator emits (the rustfmt-style diff check). Run it; let the failures produce the honest list of any §8 clause lacking a prover; fix by naming real tests (or writing the missing one — report which).
- [ ] **Step 3:** green, fmt, clippy. **Sabotage:** gut one named test's assertion to `assert!(true)` in a scratch copy; the fingerprint check fails; restore. **Commit** `feat(acceptance): self-verifying M05 acceptance map`.

### Task 7: the real-work acceptance run — once, documented, checksummed

**Files:** `docs/acceptance/m05-run-<date>/` (event-store export, replay hash pair, monitor page snapshot via `status --html`, CLI/HTTP/MCP status dumps, `SHA256SUMS`); manifest entry in `m05-clauses.toml`.

- [ ] **Step 1:** Announce on the issue (paid call, one machine). Run the §8 story against a REAL provider route once: publish → drive to completion (real model reply + real tool cycle) → replay twice (byte-identical, hashes recorded) → read the same story via CLI, HTTP and MCP (dumps captured) → capture `status --html`. **The gate never re-runs this**: the generator only validates the artifacts exist and match `SHA256SUMS`.
- [ ] **Step 2:** Commit artifacts + the clause entry pointing at them. **Commit** `docs(acceptance): M05 real-work acceptance run artifacts`.

### Task 8: the gate stage

- [ ] **Step 1:** Add `'monitor_http'` to the CLI suite list (ten suites). Red-prove with the misspelled-name resolution failure; restore. Full CLI regression `--locked`; workspace fmt/clippy. **Commit** `chore(ci): monitor_http gate stage`.

### Task 9: documentation, the Milestone 05 close, and the full gate

- [ ] **Step 1:** `docs/milestones/runtime.md`: the 05f section from the code as built, PLUS the Milestone 05 closing section (the acceptance map summarized, pointer to the generated map and the run artifacts). Honest limits, named: staleness is only as honest as event granularity (a streaming agent node emits nothing and looks stale; per-kind thresholds mitigate, heartbeat events deferred with the trigger named); the 2s meta-refresh is the update contract (no SSE/push — same ADR-026 revisit trigger); the cookie is a second door wearing the same lock (value IS the bearer token, one verifier — but it exists and is named); remediation strings don't carry If-Match (the API's optimistic concurrency is chat/HTTP's; the CLI has no flag — deferred until the CLI grows one); the monitor lists only what the store knows (no cross-store index).
- [ ] **Step 2:** CHANGELOG (house shape); spec status line → "implemented through 05f — Milestone 05 complete"; decision register D-040 "as implemented" note if the D-039 precedent applies.
- [ ] **Step 3:** Full gate announced then run (`GRAPHHELM_PG_BIN` set, orphan fake_* killed first) → GREEN including both PostgreSQL passes, the ten CLI suites and `acceptance_map_is_grounded`. **Commit** `docs(monitor): record milestone 05f and close Milestone 05`. Open the PR (Closes the milestone issue; validation evidence; security review: new auth path named + negative proof; rollback: revert — no store changes).

---

## Self-review notes

- **Spec coverage:** D-040 sentence ("renders the same JSON `status` produces, no mutating call") → Tasks 1–4; §7 bullet 05f ("read-only local page, acceptance run per §3.4's applicable steps, acceptance map, final review") → Tasks 5–9; §8 clauses → Task 6 manifest + Task 7 run; §9 scope-gravity risk → rule 5 + refused-scope table + zero-JS medium.
- **Placeholder scan:** the `<date>` in the run directory is fixed at Task 7 execution; every other artifact is named. No TBDs.
- **Type consistency:** `render_monitor(view, events, since, now)` is the one signature Tasks 1/3/5 share; `render_invocation` is defined in Task 3 and consumed nowhere else until Studio.
- **Pair split (proposal, invert of 05e):** A: 1, 3, 5, 7, 9 + opens PR · B: 0, 2, 4, 6, 8 + final PR review. Adjust at kickoff.
