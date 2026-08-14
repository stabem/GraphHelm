# Milestone 05d — The Real Executor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the fixture behind the executor seam with real work: an async `core/runtime` crate whose driver — generalized from the 04f CLI loop — dispatches model calls through the 05b gateway and tool calls through the 05c broker, externalizes every reply and stream as encrypted Evidence in the same atomic append as the outcome event, seals signal envelopes, supports immediate-stop, and closes the three M04 ledger items: resume graph cross-check, dispatch fairness, and per-edge-type readiness for the decidable condition subset.

**Architecture:** One new crate, `core/runtime` (`graphhelm-runtime`): the `AsyncNodeExecutor` seam, the async `drive_to_quiescence` reproducing the 04f sequencing exactly (approve → ready ∪ retry-pending → advisory → hops → execute → record, every `next_state` from `apply_transition`), deterministic prompt assembly, and two **ports** — `ModelPort`, `ToolPort` — that invert the dependency so `core/runtime` never imports an adapter crate; `apps/cli` wires the 05b/05c implementations into the ports and swaps the API server's drive path onto the async driver while the CLI keeps its synchronous 04f path byte-for-byte (the 05a parity test is the tripwire). Evidence rides the store's existing atomic channel: `NewEvent` already carries `evidence_refs` and `PreparedAppend` already carries `Vec<SealedEvidence>` — **zero new event kinds, zero schema changes**. Two small, surgical changes land in `core/execution` (fairness ordering in `dispatch_plan`, edge-aware `ready_set`), both pure, both property-tested.

**Tech Stack:** Rust 1.97.1 (pinned). `tokio` (workspace-pinned, already present), serde/serde_json, sha2/hex, thiserror — no new third-party dependency. Cancellation is a `tokio::sync::watch` channel, not a new crate.

**Design sources:** `docs/superpowers/specs/2026-08-13-runtime-design.md` (§5 boundary, §6.1 async seam, §6.2 outcomes carry more, §7 bullet 05d, §8 acceptance, §9 risks); `docs/harness/HARNESS_SPEC.md` §18 (runtime envelope, output); `docs/operations/OBSERVABILITY_AND_RECOVERY.md` §11.4 (resume), §12 (immediate stop), §13 (cancel), §14 (retry categories); `docs/milestones/runtime.md` and `docs/milestones/graph-engine-governor.md` (the named 04f seams this plan closes). Issue: #26.

---

## Binding process rules (every task, no exceptions)

1. **The code wins over the plan.** This plan was written against main at `0e1f095`. It additionally *names* interfaces from 05b (#24) and 05c (#25) that are not on main yet — every such reference is tagged `[RECONCILE]` and collected in Task 0, which runs first and rewrites the tags into real signatures. **Do not start implementation until 05b and 05c are merged**; if either landed differently than its plan, Task 0 absorbs the difference and reports it.
2. **This plan adds NO event kinds and touches NO schema.** `NodeOutcomeRecorded` and `signal_recorded` are carried unchanged; Evidence attaches through `NewEvent::new`'s existing `evidence_refs` parameter (`core/protocols/src/event.rs:78`) and `PreparedAppend`'s existing `evidence` vector. If a task believes it needs a new kind or a payload change, STOP and report NEEDS_CONTEXT citing D-037's ritual.
3. **`core/execution` stays pure and its invariants stay green.** The two edits this plan makes there (Tasks 2–3) are pure functions over values; `source_invariants.rs` must pass unmodified. `core/runtime` is a *different kind* of crate — async, I/O-adjacent — and must never appear in a pure crate's dependency table (the §9 risk, made a rule).
4. **Workspace clippy is the bar:** `cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings` clean at every commit.
5. **Every new guard must be observed failing once.** Sabotage, watch the named test fail, restore from a `cp` backup, never `git checkout --`.
6. **New CLI test binaries must be added to `ci/gate.ps1` by name** (after 05b/05c the suite list is expected as `('cli_smoke','schema_cli','event_store_cli','execution_cli','api_http','gateway_cli','tool_cli')`; this plan adds `runtime_http`).
7. **Free-form content never enters an event** (D-036). Model reply text, tool streams, prompt text, signal envelopes: Evidence or operator files, never payload fields. Every new payload-shaped struct in this plan carries digests and references only.
8. **The driver never invents an outcome** (the 04f rule). Every recorded outcome is what a port actually returned or a typed mapping of its error — no fabrication on predicted bounds, no synthesized success.
9. **Run `cargo +1.97.1 fmt --all` before every commit.** Commit per task: `type(scope): description`, body explains why, trailer `Co-Authored-By: Claude Code <noreply@anthropic.com>`.

## Coordination note

Until 05c's implementation completes, this branch touches exactly one file: this document. The workspace `Cargo.toml`, `ci/gate.ps1`, `docs/milestones/runtime.md` and `CHANGELOG.md` are shared surfaces; they change only during implementation. Before any full gate run, announce on issue #26 (one gate at a time on this machine).

## What already exists (do not rebuild)

- The synchronous seam and driver, which this plan generalizes but does not touch for the CLI path: `NodeExecutor` (`core/execution/src/transition.rs:29`), `drive_to_quiescence` (`apps/cli/src/commands/execution/driver.rs:32`) — its exact sequencing (approve untouched drafts; candidates = `ready_set` ∪ currently-`Queued`; `classify_progress` advisory before every dispatch, never skipping the executor; two `Started` hops; `record_outcome` rereading the projection and calling `apply_transition`; `complete_if_quiesced`).
- The atomic Evidence channel: `NewEvent::new(idempotency_key, actor, sensitivity, kind, evidence_refs, artifact_refs)` (`core/protocols/src/event.rs:78`); `PreparedAppend::new(scope, stream, next_sequence, events, evidence, artifacts)` with `MAX_EVIDENCE_ITEMS`/`MAX_EVIDENCE_BATCH_BYTES` bounds (`core/events/src/repository.rs`); `EvidenceSealer::seal(scope, EvidenceInput) -> RepositoryFuture<Result<SealedEvidence, EvidenceError>>` and `EvidenceInput::new(local_ref, media_type, sensitivity, retention_class, plaintext: SecretBytes)` (`core/events/src/evidence.rs:93-125, 242-248`); `EvidenceRepository::read` for reopening.
- `ExecutionProjection.current_graph: Option<PersistedGraphVersion>` (`core/events/src/projection.rs:149`) and `start.rs`'s hash derivation: `publish_loaded(&loaded, owner)` → `version.content_hash()` → `WireHash` (`apps/cli/src/commands/execution/start.rs:44,119`) — the resume cross-check reuses this exact path.
- `resume_preconditions(&projection, Some(version_number))` and `recovery_plan` (`core/execution/src/recovery.rs`); `(Running, Interrupted) → Blocked` and `UntriagedInterruption` refusal — immediate-stop composes these, it does not reinvent them.
- `dispatch_plan(&candidates, in_flight, max_parallel)` in `BTreeSet` order (`core/execution/src/dispatch.rs`) — Task 2 changes its ordering policy, nothing else.
- `ready_set(spec, states)` with fail-closed every-edge gating (`core/execution/src/ready.rs`) — Task 3 refines per edge type.
- The condition fixture subset the simulator already evaluates (`core/simulation/src/engine.rs:499-516`: `bool` literal, `"true"`/`"false"` strings, fixture lookup) — the runtime adopts the *literal* subset only.
- The 05a serve layer: `run_idempotent_mutation`, `ServeState`, per-request store open/drop, and the CLI–API parity test `the_cli_and_the_api_report_identical_status_for_the_same_story` — the regression net for the driver swap.
- `[RECONCILE]` 05b: `core/gateway`'s `ModelCall`/`ModelReply`/`Usage`, `GatewayError`, `outcome_for_error` (total, failure-shaped codomain), `ByokAdapter::call(&self, key, request)`, `RuntimeAdapter::call(&self, request)`, the credential broker's `lease`.
- `[RECONCILE]` 05c: `graphhelm-tool-broker`'s `ToolCall`/`ToolLease`/`ToolCallRecord`/`ToolDisposition`; `graphhelm-tool-host`'s `ToolHost::invoke(&self, call, lease, actor) -> (ToolCallRecord, CapturedStreams)`, `HostConfig`.

## The 04f seams this plan closes, named

| Seam (where recorded) | Closed by |
|---|---|
| Signal envelopes externalize to operator files, not encrypted Evidence (`docs/milestones/graph-engine-governor.md`, acceptance gaps) | Task 6 |
| `resume` re-takes `--file` and trusts it; no hash cross-check (`docs/milestones/graph-engine-governor.md`, 04f seams) | Task 7 |
| Dispatch is lexicographic-prefix fair only; a retrying node can starve a sibling (04f closing review) | Task 2 |
| Every incoming edge gates a node regardless of `EdgeType`; refinement awaited condition evaluation (04c ready-set rules) | Task 3 |
| A running node is not pausable; interrupting real work is Milestone 05's problem (`NodeOutcome::Paused` doc) | Task 8 |
| Throughput ≈3 req/s under the synchronous per-request drive (05a honest limits: "recorded so 05d has a baseline to beat") | Task 9 measures and records; no number is promised |

## File map

| Path | Responsibility |
|---|---|
| `core/runtime/Cargo.toml`, `src/lib.rs` | New crate `graphhelm-runtime` |
| `core/runtime/src/executor.rs` | `AsyncNodeExecutor`, `NodeWork`, `WorkOutcome` |
| `core/runtime/src/ports.rs` | `ModelPort`, `ToolPort` — the dependency-inverting seams |
| `core/runtime/src/prompt.rs` | Deterministic prompt assembly from the node contract |
| `core/runtime/src/evidence.rs` | Outcome→Evidence packaging (seal inputs, refs, digests) |
| `core/runtime/src/driver.rs` | Async `drive_to_quiescence` + cancellation watch |
| `core/runtime/src/classify.rs` | Node classification (cognitive/tool) and outcome mapping |
| `core/runtime/tests/prompt_assembly.rs` | Prompt determinism and content rules |
| `core/runtime/tests/driver_contract.rs` | Sequencing parity with 04f, fairness consumption, cancellation |
| `core/runtime/tests/source_invariants.rs` | Dependency pin (tokio allowed; adapters forbidden) |
| `core/execution/src/dispatch.rs` | Fairness ordering (Task 2) |
| `core/execution/src/ready.rs` | Per-edge-type readiness (Task 3) |
| `apps/cli/src/commands/execution/signal.rs` | Sealed signal envelope (Task 6) |
| `apps/cli/src/commands/execution/resume.rs` | Graph hash cross-check (Task 7) |
| `apps/cli/src/commands/serve/…` | Async drive path + immediate-stop wiring (Tasks 8–9) |
| `apps/cli/tests/runtime_http.rs` | The end-to-end suite (new gate stage) |
| `Cargo.toml` (workspace), `ci/gate.ps1` | Member + `runtime_http` stage *(implementation phase only)* |
| `docs/milestones/runtime.md`, `CHANGELOG.md` | 05d record *(implementation phase only)* |

**Deferred, stated (do not build):** the Context Compiler and Context Capsules (prompt assembly here is the node contract's own fields, nothing retrieved); the Harness Compiler pipeline; multi-node scheduling beyond `max_parallel_model_calls`; Tier 2/3; compensation execution for external effects (recorded, not compensated — §13); the five no-progress conditions needing signal intake; session management; SSE/streaming; artifact references on outcomes (`artifact_refs` stays empty until the artifact store design lands — evidence covers replies and streams); Studio/monitor/MCP (05e/05f); route scoring (05b deferred it; the runtime takes the first eligible route).

---

### Task 0: reconciliation — rewrite `[RECONCILE]` tags into merged reality

**Files:**
- Modify: this plan document only.

- [ ] **Step 1:** With 05b and 05c merged, read `core/gateway/src/{call,taxonomy,manifest}.rs`, `adapters/model-gateway/src/{byok,runtime,broker}.rs`, `core/tool-broker/src/{call,lease,record}.rs`, `adapters/tool-host/src/{host,workspace,process}.rs`. For every `[RECONCILE]` item in this document, replace the assumed signature with the real one (types, paths, async-ness, error shapes). Pay specific attention to: whether gateway adapters are sync (05b planned sync `ureq`/subprocess — the async driver then wraps them in `spawn_blocking`) and the exact shape of `ToolHost::invoke`'s return.
- [ ] **Step 2:** Commit the reconciled plan: `docs(plan): reconcile the 05d plan against merged 05b and 05c`. Report every signature that differed.

---

### Task 1: `core/runtime` skeleton — the async seam and ports

**Files:**
- Create: `core/runtime/Cargo.toml`, `src/lib.rs`, `src/executor.rs`, `src/ports.rs`
- Create: `core/runtime/tests/source_invariants.rs`
- Modify: `Cargo.toml` (workspace members — add `"core/runtime"` after `"core/gateway"`)

`[dependencies]`: `graphhelm-protocols`, `graphhelm-execution`, `graphhelm-events`, `graphhelm-gateway`, `graphhelm-tool-broker` (all path), `tokio` (workspace), `serde`, `serde_json`, `sha2`, `hex`, `thiserror`. **Not** `graphhelm-model-gateway`, **not** `graphhelm-tool-host` — the adapter crates implement this crate's ports in `apps/cli`'s wiring, never the reverse.

- [ ] **Step 1: Failing tests** in `core/runtime/tests/source_invariants.rs` — the `core/execution` pattern adapted: exact `[dependencies]` pin (the list above, nothing more); a source scan forbidding `std::process`, `ureq`, `axum` tokens under `core/runtime/src/` (tokio and async are this crate's point; adapters and HTTP are not); and one compile-shaped assertion: `core/execution/Cargo.toml`'s `[dependencies]` must NOT name `graphhelm-runtime` (the §9 risk pinned from the other side).

- [ ] **Step 2: The seam types**, `src/executor.rs`:

```rust
/// One dispatched unit of node work, everything the executor needs and nothing it may guess.
#[derive(Clone, Debug)]
pub struct NodeWork {
    pub execution_id: String,
    pub node_id: String,
    pub attempt: u32,
    pub prompt: crate::prompt::AssembledPrompt,      // Task 4
    pub kind: crate::classify::NodeWorkKind,          // Task 5: Cognitive | Tool
}

/// What real work produced: the outcome for `apply_transition`, plus the free-form material
/// to seal (D-036: it goes to Evidence, never into the event) and the safe summary that may
/// travel beside the outcome. `sealables` are (local_ref_suffix, media_type, bytes) triples;
/// the driver seals them and threads the resulting references into the SAME PreparedAppend
/// as the outcome event.
pub struct WorkOutcome {
    pub outcome: graphhelm_protocols::NodeOutcome,
    pub sealables: Vec<Sealable>,
    pub summary: WorkSummary,
}

pub struct Sealable {
    pub local_ref_suffix: &'static str,   // "reply", "stdout", "stderr", "record"
    pub media_type: &'static str,         // "application/json", "text/plain"
    pub bytes: Vec<u8>,
}

/// Digest-only, event-safe accounting (usage counts, exit dispositions). No free-form text.
#[derive(Clone, Debug, serde::Serialize)]
pub struct WorkSummary {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub exit_code: Option<i32>,
}

/// A refusal to attempt at all — the async mirror of `ExecutionError::IllegalTransition`'s
/// role on the sync seam. Work that ran and failed is never a refusal; it is a
/// failure-shaped `WorkOutcome`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, thiserror::Error)]
pub enum ExecutorRefusal {
    #[error("this node type is not executable in this milestone")] Unsupported,
    #[error("the node's contract cannot be assembled into a prompt")] Unassemblable,
}

pub trait AsyncNodeExecutor: Send + Sync {
    /// One attempt of one node. Errors are refusals to attempt (illegal dispatch), not work
    /// failures — a failed model call or tool run is a `WorkOutcome` with a failure-shaped
    /// outcome, exactly as §6.2 resolves it.
    fn execute<'a>(
        &'a self,
        work: &'a NodeWork,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<WorkOutcome, ExecutorRefusal>> + Send + 'a>>;
}
```

(Plain `async fn` in the trait is acceptable if object safety is not needed by the driver — the driver holds a concrete generic; prefer `async fn` and fall back to the boxed form only if the compiler objects. Report which form compiled.)

- [ ] **Step 3: The ports**, `src/ports.rs` — dependency inversion so the executor is testable without adapters:

```rust
/// [RECONCILE] Shapes mirror core/gateway's ModelCall/ModelReply; confirm in Task 0.
pub trait ModelPort: Send + Sync {
    fn call<'a>(
        &'a self,
        route_id: &'a str,
        call: &'a graphhelm_gateway::ModelCall,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<graphhelm_gateway::ModelReply, graphhelm_gateway::GatewayError>> + Send + 'a>>;
}

/// [RECONCILE] Mirrors graphhelm-tool-broker types; the host adapter implements it in apps/cli.
pub trait ToolPort: Send + Sync {
    fn invoke<'a>(
        &'a self,
        call: &'a graphhelm_tool_broker::ToolCall,
        lease: &'a graphhelm_tool_broker::ToolLease,
        actor: &'a str,
    ) -> std::pin::Pin<Box<dyn Future<Output = ToolPortResult> + Send + 'a>>;
}
// ToolPortResult = (ToolCallRecord, CapturedStreams-shaped bytes) [RECONCILE]
```

- [ ] **Step 4: Green, fmt, workspace clippy. Commit** `feat(runtime): async executor seam and adapter ports`.

---

### Task 2: dispatch fairness (M04 ledger)

**Files:**
- Modify: `core/execution/src/dispatch.rs`
- Test: extend `core/execution/tests/scheduling_properties.rs` (or the dispatch tests' home — read where `dispatch_plan`'s tests live and extend there; report the location)

The 04f finding: `BTreeSet` order is lexicographic, so with `max_parallel = 1` a persistently retrying, alphabetically earlier node starves a sibling's *first* attempt for up to the bound's worth of passes.

- [ ] **Step 1: Failing test:**

```rust
#[test]
fn a_retrying_node_cannot_starve_a_first_attempt() {
    // candidates {"aaa", "zzz"}; attempts: aaa → 3, zzz → 0; max_parallel 1, in_flight 0.
    // The plan must pick "zzz": fewer attempts dispatch first; the lexicographic order is
    // now only the tiebreak WITHIN an attempt count.
}

#[test]
fn fairness_is_deterministic_and_replay_indifferent() {
    // Same inputs → same plan (property, proptest over candidate sets and attempt maps).
    // Determinism is what replay needs from dispatch: the dispatch DECISION is never an
    // event input — replay folds recorded outcomes — so changing the policy is legal; being
    // nondeterministic is not. State this in the test's doc comment.
}
```

- [ ] **Step 2: Implement.** `dispatch_plan` gains an `attempts: &BTreeMap<String, u32>` parameter (callers pass `projection.node_attempts`); selection sorts by `(attempts.get(node).unwrap_or(0), node)` ascending and takes the prefix. Update the two call sites (`apps/cli` driver now; `core/runtime` driver in Task 5 consumes the same function). The existing determinism/sabotage tests keep passing — reversal of the iterator must still fail them.
- [ ] **Step 3: Sabotage:** drop the attempts key (pure lexicographic again); the starvation test fails; restore. **Step 4: fmt, workspace clippy, commit** `feat(execution): attempt-fair deterministic dispatch`.

---

### Task 3: per-edge-type readiness for the decidable subset (M04 ledger)

**Files:**
- Modify: `core/execution/src/ready.rs`
- Test: `core/execution/tests/scheduling_properties.rs`

Conservative by construction: **no node becomes ready that was not ready before, except through the two rules below; every other edge keeps the 04c fail-closed gate.**

- [ ] **Step 1: Failing tests:**

```rust
#[test]
fn a_literal_false_condition_ungates_its_edge() {
    // Edge a→b, type Control, condition: json!(false). 04c gated b on a unconditionally;
    // a literally-false condition is statically dead — the edge does not gate. Assert b is
    // ready while a is untouched. A json!(true) condition gates exactly as before. A
    // non-literal condition (string, object) stays FAIL-CLOSED: gate as if unconditioned —
    // the runtime evaluates only what the simulator's deterministic subset already defines
    // (engine.rs:499-516), minus fixtures, which execution does not have.
}

#[test]
fn a_failure_edge_releases_on_failed_and_blocks_otherwise() {
    // Edge a→handler, type Failure. handler must be ready when a is Failed — that is what a
    // failure route IS — and must NOT be ready when a Succeeded (the 04c release rule stays
    // for every other edge type; a failure handler after a success would be spurious work).
    // Waived/Skipped on a failure edge: NOT released (nothing failed).
}

#[test]
fn every_other_shape_is_exactly_the_04c_rule() {
    // Property: for graphs whose edges are all non-Failure with no literal-false conditions,
    // the new ready_set equals the old rule's output across randomized states (proptest) —
    // the refinement is additive on the two named cases only.
}
```

- [ ] **Step 2: Implement** in `ready.rs`: gate evaluation becomes per-edge — `edge_gates(edge, predecessor_state) -> bool` — with the two carve-outs and the property pinned. `Ghost` exclusions unchanged. **Step 3: Sabotage:** make the failure edge release on `Succeeded` too; the second test fails; restore. **Step 4: fmt, clippy, commit** `feat(execution): edge-aware readiness for the decidable subset`.

---

### Task 4: deterministic prompt assembly

**Files:**
- Create: `core/runtime/src/prompt.rs`
- Create: `core/runtime/tests/prompt_assembly.rs`

- [ ] **Step 1: Failing tests:**

```rust
#[test]
fn assembly_is_deterministic_and_complete() {
    // From a GraphNode (the software-feature example's `implement` node shape: objective,
    // agent.ephemeral.instructions, inputSchema/outputSchema refs), assemble twice; assert
    // byte-identical AssembledPrompt and that objective, instructions, and both schema refs
    // appear. Field order is fixed by the assembler, never by map iteration over
    // GraphNode.properties (BTreeMap already sorts; the assembler names fields explicitly).
}

#[test]
fn assembly_refuses_a_node_missing_its_contract() {
    // A Tool-type node has no prompt; an Agent node without objective is unassemblable —
    // typed refusal, never an empty prompt (execution must not invent, the FixtureExecutor
    // precedent).
}

#[test]
fn the_prompt_never_enters_an_event_payload() {
    // AssembledPrompt serializes for Evidence only; assert the type does NOT implement
    // the event-payload marker (compile-shaped: it lives outside protocols and nothing in
    // this crate constructs an EventKind from it — a source-scan test asserting no
    // `EventKind::` token in prompt.rs is the honest executable form).
}
```

- [ ] **Step 2: Implement** `AssembledPrompt { system: String, task: String, sha256: String }` built from named `GraphNode` fields (`objective`, `properties["agent"]["ephemeral"]["instructions"]`, schema refs; read the real property paths from `examples/graphs/software-feature.yaml` and `schemas/node.schema.json` — the code wins). The digest is computed at assembly so Evidence and record agree on identity. **Step 3: green, sabotage** (make assembly read a map in insertion order via a Vec shim — determinism test fails; restore), **fmt, clippy, commit** `feat(runtime): deterministic prompt assembly from the node contract`.

---

### Task 5: the real executor — classification, ports, outcome mapping

**Files:**
- Create: `core/runtime/src/classify.rs`, extend `src/executor.rs` with `PortExecutor`
- Create: `core/runtime/tests/driver_contract.rs` (first tests here)

- [ ] **Step 1: Failing tests** (fake ports — in-crate test doubles implementing `ModelPort`/`ToolPort` with scripted replies; no adapter crate anywhere near these tests):

```rust
#[test]
fn node_types_classify_cognitive_or_tool_and_nothing_else_dispatches() {
    // Agent/Planner/Classifier/Evaluator → Cognitive (Tier 0 work — the model call carries
    // no workspace); Tool → Tool. Every other NodeType (Gate, Fork, Join, HumanDecision,
    // Timer, Trigger, Subgraph, Materializer, Deploy, Rollback, ArtifactTransform) →
    // ExecutorRefusal::Unsupported. A refusal is a refusal — never laundered into an
    // outcome the fold would record: the driver simply does not dispatch what the executor
    // refuses, and the 05d acceptance graph uses agent+tool nodes only. Pin the refusal.
}

#[test]
fn a_model_reply_is_succeeded_with_sealed_reply_evidence() {
    // Fake ModelPort returns ModelReply { text: "REPLY-SENTINEL", usage 12/5 }.
    // PortExecutor.execute(work) → outcome Succeeded; sealables contain ("reply",
    // "application/json", bytes containing REPLY-SENTINEL); summary carries 12/5; and the
    // WorkOutcome's serialized summary NEVER contains REPLY-SENTINEL.
}

#[test]
fn gateway_errors_map_through_outcome_for_error_verbatim() {
    // Fake port returns Err(QuotaExhausted) → outcome NeedsCapacity (the §12 wait rule,
    // already pinned by 05b's outcome_for_error — assert PortExecutor delegates to it, not
    // to a second mapping: sabotage in Step 3 proves the delegation).
}

#[test]
fn a_tool_record_maps_by_disposition_and_seals_record_plus_streams() {
    // Fake ToolPort: disposition Completed{exit_code:0} → Succeeded; Completed{exit_code:101}
    // → RetryableFailure; TimedOut → RetryableFailure; Denied → TerminalFailure (a lease
    // refusal will not heal by retrying the same call); HostError → RetryableFailure.
    // Sealables: ("record", application/json, the ToolCallRecord JSON), ("stdout"/"stderr",
    // text/plain, stream bytes). [RECONCILE] against 05c's merged ToolDisposition variants.
}

#[test]
fn an_empty_reply_is_a_retryable_failure_never_a_success_and_never_a_park() {
    // ModelReply with empty text → RetryableFailure. Revised by the plan review: NeedsInput
    // parks the node waiting for input that nothing in 05d can deliver — an operational
    // dead end. An empty reply from a live provider is a provider defect: retryable, bounded
    // by MAX_NODE_ATTEMPTS/MAX_IDENTICAL_OUTCOMES, landing in Blocked for an owner decision
    // when persistent — every state on that path has an exit. Never Succeeded either way.
}
```

- [ ] **Step 2: Implement** `classify.rs` (`NodeWorkKind::{Cognitive, Tool}` from `NodeType` — cite `core/protocols/src/graph.rs:82`) and `PortExecutor { model: Arc<dyn ModelPort>, tools: Arc<dyn ToolPort>, route_id, lease, actor }` implementing `AsyncNodeExecutor`: assemble → dispatch by kind → map outcome → package sealables. Route selection: the configured route id (one route in 05d; scoring stayed deferred in 05b). **Step 3: Sabotage:** map `QuotaExhausted` to `RetryableFailure` locally (bypassing `outcome_for_error`); the mapping test fails; restore. **Step 4: fmt, clippy, commit** `feat(runtime): port executor with honest outcome mapping`.

---

### Task 6: Evidence-before-append — outcomes and signals sealed atomically

**Files:**
- Create: `core/runtime/src/evidence.rs`
- Create: `core/runtime/src/driver.rs` (append half; the loop arrives in Task 8)
- Modify: `apps/cli/src/commands/execution/signal.rs`
- Test: `core/runtime/tests/driver_contract.rs`, `apps/cli/tests/execution_cli.rs` (signal cases)

- [ ] **Step 1: Failing tests:**

```rust
// core/runtime/tests/driver_contract.rs — through a real LocalEventRepository (tempdir)
// and the real EvidenceProtector over a throwaway sealed-key provider (the M03 test
// bootstrap; read adapters/sealed-key-provider/tests for the pattern):

#[test]
fn an_outcome_and_its_evidence_land_in_one_atomic_append() {
    // record_outcome_with_evidence(...) for a WorkOutcome with two sealables:
    //   * the appended node_outcome_recorded event's evidence_refs name exactly the sealed
    //     references, deterministically derived: "exec-{id}-{node}-a{attempt}-{suffix}";
    //   * EvidenceRepository::read returns Available for each ref;
    //   * EvidenceOpener::open round-trips the original bytes;
    //   * the event payload and envelope contain no sealable byte (REPLY-SENTINEL scan).
}

#[test]
fn a_sealing_failure_appends_nothing() {
    // A key provider rigged to fail → the append never happens: head sequence unchanged,
    // no partial event, no orphan evidence. Evidence-before-append, fail-closed — the
    // signal command's 04f rule generalized to node work (§6.2).
}

// apps/cli/tests/execution_cli.rs:
#[test]
fn a_signal_envelope_is_sealed_beside_its_record() {
    // execution signal with --evidence-out (kept: operator copy) now ALSO seals the
    // envelope: the signal_recorded event carries an evidence_ref, the store's evidence
    // read returns Available, and envelopeSha256 equals the sealed plaintext's digest.
    // Requires the events config's keyring flags on the signal command — mirror the events
    // subcommands' provider construction (config.rs precedent), refusing to run without a
    // keyring rather than silently skipping the seal.
}
```

- [ ] **Step 2: Implement.** `evidence.rs`: `seal_work(sealer, scope, execution, node, attempt, sealables) -> Result<(Vec<SealedEvidence>, Vec<EvidenceReference>), _>` with the deterministic `local_ref` derivation (attempt-scoped so retries never collide) and per-item `Sensitivity::Confidential` for replies/streams (model output is user material) — verify `Sensitivity` variants in protocols and report the chosen one. `driver.rs`: `record_outcome_with_evidence` mirrors 04f's `record_outcome` (`driver.rs:178-228`: reread, `apply_transition`, `NewEvent`) but passes `evidence_refs` into `NewEvent::new` and the sealed items into `PreparedAppend::new`'s evidence vector — one `append_atomic`. `signal.rs`: seal before append, evidence ref on the record, `--evidence-out` unchanged.

- [ ] **Step 2c: Discharge the `ReuseDecision` obligation (review finding).** The 05c
amendment ships the kind with the executor named as its producer — this task is where that
promise lands. When the ToolPort's returned record carries `reused: true` (or `forced_fresh`
with its reason `[RECONCILE]` against the merged 05c shape), the writer appends a
`ReuseDecision` event alongside the outcome — same `PreparedAppend`, populated from the
record's decision, freshness class, key digest and evidence ref, `node_id: Some(...)`. The
fold arm is already on main as an explicit no-op ledger (05c Task 9b); test: a tool-node run
through the fake ToolPort scripted as a hit yields exactly one `ReuseDecision` in the stream,
replay-stable, and a fresh run yields a `Miss` decision — the ledger records both directions.
- [ ] **Step 3: Sabotage:** append first, seal after (reorder); the sealing-failure test fails (an event exists with no evidence); restore. **Step 4: fmt, clippy, commit** `feat(runtime): atomic evidence-before-append for outcomes and signals`.

---

### Task 7: the resume cross-check (M04 ledger)

**Files:**
- Modify: `apps/cli/src/commands/execution/resume.rs`
- Test: `apps/cli/tests/execution_cli.rs`

- [ ] **Step 1: Failing test:**

```rust
#[test]
fn resume_refuses_a_graph_file_that_does_not_match_the_started_hash() {
    // start a two-node graph; pause it; tamper one byte of a node's objective in a COPY of
    // the graph file; resume --file <copy> → refused with the new failure (GHCLI005 family
    // or a dedicated code — match the module's conventions), naming /execution/graph as the
    // pointer, BEFORE any recovery append. Resume with the pristine file still succeeds.
}
```

- [ ] **Step 2: Implement.** Resume already loads the file and reads the projection; add: derive the supplied file's content hash exactly as `start.rs` does (`publish_loaded` is start's path — extract the *hash-derivation* half into a shared helper so resume projects WITHOUT publishing; read `publish_loaded` and the governor's projection entry point, take the pure projection+hash path, and report the helper's final shape), compare against `projection.current_graph`'s `content_hash` (`projection.rs:149`; refuse `None` — an execution with no published graph cannot resume against any file). Mismatch → typed refusal before `recovery_plan` runs.
- [ ] **Step 3: Sabotage:** compare against the supplied file's own hash (always equal); the test fails; restore. **Step 4: fmt, clippy, commit** `feat(cli): resume cross-checks the supplied graph against the published hash`.

---

### Task 8: the async driver with immediate-stop

**Files:**
- Modify: `core/runtime/src/driver.rs` (the loop)
- Test: `core/runtime/tests/driver_contract.rs`

- [ ] **Step 1: Failing tests** (fake ports; real store; the lifecycle test's three-node chain as the fixture graph):

```rust
#[test]
fn the_async_driver_reproduces_the_04f_sequencing_on_a_happy_chain() {
    // Drive a two-node chain with a scripted ModelPort; assert the event stream shape the
    // 04f driver produces for the same story: approve hops, two Started hops per dispatch,
    // node_outcome_recorded with next_state from apply_transition, execution_completed —
    // and the full history replays byte-identically twice (the acceptance's replay clause).
}

#[test]
fn max_parallel_dispatches_concurrently_and_respects_the_bound() {
    // Fan-out graph (one root, three independent children), max_parallel_model_calls: 2,
    // a ModelPort whose call resolves only when the test releases it (a watch/oneshot per
    // node): assert at most 2 in flight at any instant, and 3 total completions. Wall-clock
    // free: gate on the port's own counters, never on sleeps.
}

#[test]
fn immediate_stop_interrupts_in_flight_work_and_blocks_it() {
    // Start a node whose fake port never resolves until cancelled; trigger the driver's
    // cancellation watch; assert: the port's future was dropped/aborted, the node's
    // recorded outcome is Interrupted with next_state Blocked ((Running, Interrupted) →
    // Blocked, the 04e arm), execution_paused follows (fold order: pause while Running is
    // legal — the 04e finding — and recovery is what resume does), and resume afterwards
    // refuses with UntriagedInterruption until the owner approves. §12 immediate stop,
    // composed from existing pieces.
}

#[test]
fn a_cancelled_tool_child_is_actually_dead() {
    // Tool-kind node with a fake ToolPort that spawns a real sleeping child process and
    // kills it on cancel (the port contract: cancellation propagates; the 05c host's
    // deadline kill is the mechanism [RECONCILE]) — assert the child pid is gone before
    // the outcome is recorded. If pid-liveness proves untestable portably, narrow to: the
    // port's cancel hook ran before record; report the narrowing.
}
```

- [ ] **Step 2: Implement — the single-writer design (revised by the plan review's critical
finding).** `LocalEventRepository::open` takes a **blocking** OS-exclusive lock
(`fs2::lock_exclusive`, `core/events/src/local.rs:503`); concurrent async appends would pile
blocking lock waits onto runtime threads and race `next_sequence` into `SequenceConflict`s.
The driver therefore separates concurrent WORK from serialized WRITES:

  - **Runtime flavor: `multi_thread`** — stated in code where the runtime is built; the
    current-thread runtime plus a blocking store call is a self-deadlock.
  - **Every store touch goes through `spawn_blocking`** — the blocking lock never runs on an
    async worker thread.
  - **One writer task owns the store sequence.** Executor futures (hops excepted — see below)
    run concurrently up to the bound and send completed `WorkOutcome`s over an `mpsc` channel
    to a single writer task, which per message: opens the store (spawn_blocking), rereads the
    projection, runs `apply_transition` against the FRESH state, seals evidence, appends
    atomically, drops the handle. Serialization through the writer is what makes
    `next_sequence` race-free and the projection reread always current; the OS lock underneath
    stays the cross-process authority (CLI concurrency is unaffected).
  - Dispatch hops (`Started` pairs) are writes too and flow through the same writer before the
    executor future starts — the writer emits a "dispatched" ack per node so sequencing
    matches 04f exactly.

  `drive_to_quiescence_async(store_open, scope, stream, spec, executor, actor, cancel:
  watch::Receiver<bool>)`: the 04f loop with `tokio::select!` between the dispatch batch and
  cancellation; on cancel: signal the ports' cancel hooks (Step 2b), abort in-flight futures,
  record `Interrupted` for each aborted node through the writer, append `execution_paused`,
  return.

- [ ] **Step 2b: Cancellation reaches blocking work (review finding).** Dropping a future
  does not stop a blocking `spawn_blocking` body (ureq call, subprocess wait). The port
  contract gains a cancel hook: `ModelPort::cancel_all(&self)` and `ToolPort::cancel_all(&self)`
  (or a per-call `CancelHandle` if Task 0 finds the adapters expose one — the code wins),
  with defined semantics per adapter: a subprocess-backed port kills its children (the 05c
  host's kill path is the mechanism `[RECONCILE]`); an HTTP-backed port CANNOT abort a request
  mid-flight — its bound is the transport timeout, and this is an honest limit named in
  Task 10, not hidden behind the hook. The immediate-stop test asserts the subprocess case
  kills for real; the HTTP case asserts the outcome is discarded-after-completion (recorded
  as `Interrupted`, never as the reply that arrived late).
- [ ] **Step 3: Sabotage:** on cancel, record `RetryableFailure` instead of `Interrupted` (the silent-retry bug 04e refused); the immediate-stop test fails; restore. **Step 4: fmt, clippy, commit** `feat(runtime): async drive-to-quiescence with immediate-stop`.

---

### Task 9: the API drives async — and the CLI does not change

**Files:**
- Modify: `apps/cli/src/commands/serve/…` (start/resume/pause routes wire `core/runtime`), `apps/cli/Cargo.toml` (add `graphhelm-runtime` and the two adapter crates for port wiring)
- Create: `apps/cli/tests/runtime_http.rs`
- Modify: `ci/gate.ps1` (add `runtime_http`)

- [ ] **Step 1: Failing tests** in `apps/cli/tests/runtime_http.rs` (the `api_http.rs` spawn/fake patterns; ports wired to the 05b fake-server + 05c fake-tool fixtures `[RECONCILE]`):
  - **the fixture bridge first (review finding):** the parity test drives a fixture story, and nothing async executes fixtures — add `FixtureAsyncExecutor` in `core/runtime` (a thin `AsyncNodeExecutor` over `graphhelm_simulation::FixtureExecutor`, same table, same absent-fixture-is-`NeedsInput` semantics, immediately-ready future) so the API's async driver can run the 05a scripted story without a gateway; it is a test-support type, `#[doc(hidden)]`, and the sync CLI path never touches it;
  - the 05a scripted story through the API now drives via the async driver (over `FixtureAsyncExecutor`) and the final `status` still matches the CLI's for the same story on a fresh store (**the parity test is the contract**: run the existing `the_cli_and_the_api_report_identical_status_for_the_same_story` unchanged — it must stay green through the swap);
  - a real end-to-end: agent node (fake Anthropic server) + tool node (fake tool) runs to completion over HTTP with sealed evidence readable from the store and byte-identical double replay — **the milestone's §8 acceptance sentence as one test**;
  - `POST /v1/executions/{id}/pause` with `{"mode":"immediate"}` interrupts an in-flight node (fake port held open), records `Interrupted → Blocked`, and a subsequent resume refuses with the untriaged-interruption failure until approve — the graceful default (`"mode"` absent) unchanged;
  - measure and print (eprintln, captured by the test log) the storm-shaped throughput number for the milestone doc — assert nothing about it beyond completion (the 05a baseline is recorded prose, not a gate).
- [ ] **Step 2: Implement.** Port wiring in serve: build `PortExecutor` from the gateway manifest/broker config flags (`serve` gains the manifest/keyring/staging flags, mirroring the 05b/05c CLI precedents `[RECONCILE]`); `start`/`resume` handlers call `drive_to_quiescence_async`; the CLI's `execution start/resume` keep calling the synchronous 04f driver **unchanged** (the design's §6.1 split, verbatim).
- [ ] **Step 3: Gate stage** `runtime_http` (rule 6), proven able to go red via the 05b misspell pattern. **Step 4: Sabotage:** point the API's resume at the sync driver transiently; the immediate-stop HTTP test fails (sync path cannot interrupt); restore. **Step 5: fmt, clippy, commit** `feat(cli): the API drives the async runtime executor`.

---

### Task 10: documentation and the full gate

**Files:**
- Modify: `docs/milestones/runtime.md` (05d section), `CHANGELOG.md`, `docs/superpowers/specs/2026-08-13-runtime-design.md` (status), index docs if they enumerate crates (check).

- [ ] **Step 1: Write the 05d section from the code as built.** Cover: `core/runtime` and the port inversion; the single-writer append design over the blocking store lock and why (the review's critical); the async/sync split and the parity proof through `FixtureAsyncExecutor`; evidence-before-append generalized (outcome + signal), with the deterministic ref derivation; the `ReuseDecision` producer discharging the 05c obligation; fairness and edge-readiness as shipped; the resume cross-check; immediate-stop's composition from 04e arms; the measured throughput against the 05a ≈3 req/s baseline. **Honest limits:** prompt assembly is the node contract only (no Context Compiler); one route, no scoring; artifact_refs still empty; compensation recorded-not-executed; the five signal-needing no-progress conditions still undetected; cancellation of a subscription-CLI child is process-kill, not protocol-level cancel; **an in-flight HTTP model call is not abortable — its bound is the transport timeout, and a reply arriving after cancel is discarded, recorded as `Interrupted`, never as the outcome**.
- [ ] **Step 2: Re-read the whole file top to bottom** — every claim matches a named test or line.
- [ ] **Step 3: CHANGELOG + spec status line.**
- [ ] **Step 4: Full gate** (announce on #26 first): `./ci/gate.ps1` with `GRAPHHELM_PG_BIN` → GREEN, both PostgreSQL passes. Tracked flakes (#19) re-run once with a note; anything else is a defect.
- [ ] **Step 5: Commit** `docs(runtime): record milestone 05d as built`.

---

## Definition of done

- A published graph with an agent node and a tool node runs to completion over the HTTP API doing real (faked-at-the-edge) work, every transition durable, the full history replaying byte-identically — the milestone §8 sentence as a named test.
- The CLI's synchronous path is byte-identical to 04f (its suites pass unmodified) and the CLI–API parity test survives the driver swap unchanged.
- Every model reply, tool record and tool stream is sealed Evidence referenced from its outcome event, appended atomically; a sealing failure appends nothing; signal envelopes are sealed beside their records.
- Immediate-stop interrupts in-flight work: aborted futures, dead children, `Interrupted → Blocked`, resume gated on triage — sabotage-proven against the silent-retry bug.
- Resume refuses a graph file whose derived hash disagrees with the published `current_graph`.
- Dispatch is attempt-fair and deterministic; readiness honors literal-false conditions and Failure edges, provably additive over the 04c rule everywhere else.
- Zero new event kinds, zero schema changes, zero new third-party dependencies; `core/execution` purity invariants and the no-reverse-dependency pin both green.
- `runtime_http` gates, proven able to go red; the full gate is green, PostgreSQL matrix included.

## What this plan deliberately excludes

The MCP surface (05e) and monitor (05f); Context Compiler/Capsules and the Harness Compiler; route scoring and multi-route failover; artifact references and the artifact store; compensation execution; session management; SSE; Tier 2/3; the signal-needing no-progress detectors; any Studio surface. Each is either a later plan's or named in the milestone doc's honest limits.

## Self-review notes (already applied)

- Spec coverage against `runtime-design.md` §7's 05d bullet: `core/runtime` with `AsyncNodeExecutor` ✔ (T1); prompt assembly ✔ (T4); outcomes with externalized evidence ✔ (T5–T6); encrypted signal envelopes ✔ (T6); immediate-stop ✔ (T8); resume cross-check ✔ (T7); dispatch fairness ✔ (T2); edge conditions ✔ (T3). §6.1 sync/async split ✔ (T9, parity pinned). §6.2 evidence-before-append for node work ✔ (T6). §8 acceptance sentence ✔ (T9's end-to-end). §9 purity-leak risk pinned from both sides ✔ (T1 invariants).
- Every 05b/05c-dependent signature is tagged `[RECONCILE]` and Task 0 exists solely to burn the tags down against merged reality before any code is written.
- Type consistency: `WorkOutcome`/`Sealable`/`WorkSummary` defined once (T1), produced by `PortExecutor` (T5), consumed by `record_outcome_with_evidence` (T6) and the driver (T8); `NodeWorkKind` (T5) is the only classification; the cancellation channel type (`watch::Receiver<bool>`) is named identically in T8's driver and T9's serve wiring.
- Placeholder scan: none of the forbidden shapes; the two open questions (async-fn-in-trait form, pid-liveness portability) are stated as report-back decisions with a defined fallback, not TBDs.
