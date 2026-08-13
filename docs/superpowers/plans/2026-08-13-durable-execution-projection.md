# Milestone 04b - Durable Execution Projection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make a running execution durable by recording it as append-only events and reconstructing its state as a disposable projection, so an interrupted execution can be rebuilt from history and a replay reproduces the identical state.

**Architecture:** No new crate and no new projection machinery. `ExecutionProjection`, `ProjectionWatermark`, `ProjectionGeneration`, `ProjectionRepository` and `ProjectionRebuilder` already exist in `core/events` and already ship generations, watermarks and atomic swap. This plan adds four execution event kinds to the shared vocabulary, extends `ExecutionProjection` with the execution fields those events imply, and folds them in. Progress counters are **derived** by folding outcomes, never stored on the wire, so history remains the single source of truth.

**Tech Stack:** Rust 1.97.1, edition 2024. `graphhelm-protocols` for wire types, `graphhelm-events` for the projection, `graphhelm-schema` for envelope validation, PostgreSQL 16+ for the adapter tests. No new dependency.

**Design source:** `docs/superpowers/specs/2026-08-13-graph-engine-governor-design.md`, decisions 5.3, 5.5, 5.6, 5.7.

---

## Two constraints that shape every task

**1. `core/events` may not depend on `core/execution`.** The design fixes the direction: `core/execution` depends on `protocols`, `graph`, `policy` and `events`. The projection therefore cannot import `NodeOutcome` from `core/execution`, and duplicating that enum would be exactly the vocabulary drift decision 5.3 exists to prevent. Task 1 moves `NodeOutcome` into `graphhelm-protocols` — where it belongs anyway, because it travels on the wire — and `core/execution` re-exports it so no consumer changes.

**2. Anything on the wire has a schema, and the schema has two copies and two digests.** Milestone 04a shipped a `NodeState` variant whose wire contract was never updated, and nothing detected it. Task 3 does that work deliberately and in one place, following the same in-place correction as 04a per D-037.

---

## File structure

| File | Responsibility |
|---|---|
| `core/protocols/src/simulation.rs` | `NodeOutcome` and `ExecutionMode` — the shared vocabularies |
| `core/protocols/src/event.rs` | The four execution event payloads and their `EventKind` variants |
| `core/execution/src/transition.rs` | Keeps the transition rules; `NodeOutcome` now re-exported, not defined |
| `schemas/event-envelope.schema.json` | The wire contract for the four kinds |
| `core/events/src/projection.rs` | `ExecutionProjection` execution fields and the fold |
| `core/events/tests/execution_projection.rs` | Replay identity, derived counters, bounds |

---

### Task 1: Move `NodeOutcome` into the shared vocabulary

**Files:**
- Modify: `core/protocols/src/simulation.rs`
- Modify: `core/execution/src/transition.rs`
- Modify: `core/execution/src/lib.rs`
- Test: `core/protocols/tests/wire_roundtrip.rs`

- [ ] **Step 1: Write the failing test**

Add to `core/protocols/tests/wire_roundtrip.rs`, inside `normative_states_statuses_and_event_kinds_have_exact_wire_names`, immediately after the existing `states` loop:

```rust
    let outcomes = [
        (NodeOutcome::Started, "started"),
        (NodeOutcome::RetryableFailure, "retryable_failure"),
        (NodeOutcome::TerminalFailure, "terminal_failure"),
        (NodeOutcome::NeedsCapacity, "needs_capacity"),
        (NodeOutcome::Invalidated, "invalidated"),
    ];
    for (outcome, expected) in outcomes {
        assert_eq!(serde_json::to_value(outcome).unwrap(), expected);
        assert_eq!(
            serde_json::from_value::<NodeOutcome>(serde_json::json!(expected)).unwrap(),
            outcome
        );
    }
```

Add `NodeOutcome` to that file's existing `use graphhelm_protocols::{...}` import list.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo +1.97.1 test -p graphhelm-protocols --test wire_roundtrip normative_states`
Expected: FAIL to compile, `no NodeOutcome in the root`

- [ ] **Step 3: Define it in protocols**

In `core/protocols/src/simulation.rs`, immediately after the `NodeState` enum, add:

```rust
/// What happened to a node, as reported by the executor or the owner.
///
/// This is a wire vocabulary because `node_outcome_recorded` carries it. `core/execution` holds the
/// rules that interpret it and re-exports this type rather than defining a second one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeOutcome {
    /// The scheduler dispatched the node.
    Started,
    Succeeded,
    /// Failed in a way a further attempt could resolve.
    RetryableFailure,
    /// Failed in a way no further attempt can resolve.
    TerminalFailure,
    /// The node is waiting on input that has not arrived.
    NeedsInput,
    /// The node is waiting on capacity, such as an exhausted subscription quota.
    NeedsCapacity,
    /// The owner approved a proposed expansion.
    Approved,
    /// The owner waived the obligation blocking this node.
    Waived,
    /// The owner or a dependency failure removed this node from the run.
    Skipped,
    Cancelled,
    /// An upstream change invalidated a completed node's output.
    Invalidated,
}
```

Confirm `core/protocols/src/lib.rs` re-exports it the same way it re-exports `NodeState`; if that module uses an explicit list rather than a glob, add `NodeOutcome` to it.

- [ ] **Step 4: Delete the duplicate and re-export instead**

In `core/execution/src/transition.rs`, delete the local `pub enum NodeOutcome { ... }` definition entirely, and change the import at the top of the file to:

```rust
use graphhelm_protocols::{NodeOutcome, NodeState};
```

In `core/execution/src/lib.rs`, change the transition re-export to keep `NodeOutcome` nameable from this crate:

```rust
pub use graphhelm_protocols::NodeOutcome;
pub use transition::{
    ExecutionError, NodeExecutor, TransitionRequest, apply_transition,
};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo +1.97.1 test -p graphhelm-protocols -p graphhelm-execution`
Expected: PASS. `core/execution`'s ten unit tests and three property tests are unchanged — they referenced `NodeOutcome` through the crate root, which still resolves.

- [ ] **Step 6: Commit**

```bash
git add core/protocols/src/simulation.rs core/protocols/src/lib.rs core/protocols/tests/wire_roundtrip.rs core/execution/src
git commit -m "refactor(protocols): move NodeOutcome into the shared wire vocabulary"
```

---

### Task 2: The execution mode vocabulary

**Files:**
- Modify: `core/protocols/src/simulation.rs`
- Test: `core/protocols/tests/wire_roundtrip.rs`

D-022 defines three modes, switchable mid-execution. Decision 5.5 fixes when a mode binds; this task only defines the vocabulary the projection must carry.

- [ ] **Step 1: Write the failing test**

Add to the same test in `core/protocols/tests/wire_roundtrip.rs`:

```rust
    for (mode, expected) in [
        (ExecutionMode::Autopilot, "autopilot"),
        (ExecutionMode::Supervised, "supervised"),
        (ExecutionMode::Manual, "manual"),
    ] {
        assert_eq!(serde_json::to_value(mode).unwrap(), expected);
        assert_eq!(
            serde_json::from_value::<ExecutionMode>(serde_json::json!(expected)).unwrap(),
            mode
        );
    }
    assert!(serde_json::from_value::<ExecutionMode>(serde_json::json!("god_mode")).is_err());
```

Add `ExecutionMode` to that file's import list.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo +1.97.1 test -p graphhelm-protocols --test wire_roundtrip normative_states`
Expected: FAIL to compile, `no ExecutionMode in the root`

- [ ] **Step 3: Define it**

In `core/protocols/src/simulation.rs`, after `NodeOutcome`:

```rust
/// How much autonomy the owner has granted this execution, per D-022.
///
/// The vocabulary is closed: an unrecognized mode must fail deserialization rather than default to
/// the permissive one, because defaulting would silently grant autonomy nobody approved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    /// The Governor may accept its own mutations.
    Autopilot,
    /// The Governor proposes; the owner approves before anything is accepted.
    Supervised,
    /// Only the owner changes the graph.
    Manual,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo +1.97.1 test -p graphhelm-protocols --test wire_roundtrip normative_states`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add core/protocols/src/simulation.rs core/protocols/tests/wire_roundtrip.rs
git commit -m "feat(protocols): add the closed execution mode vocabulary"
```

---

### Task 3: The four execution event kinds

**Files:**
- Modify: `core/protocols/src/event.rs`
- Test: `core/protocols/tests/persistence_wire.rs`

Counters are deliberately absent from these payloads. An attempt number written by a producer would be a second source of truth that history could contradict; the projection derives it in Task 6 instead.

- [ ] **Step 1: Write the failing test**

Add to `core/protocols/tests/persistence_wire.rs`:

```rust
#[test]
fn execution_event_kinds_round_trip_with_exact_wire_names() {
    let hash = format!("sha256:{}", "a".repeat(64));
    let cases = [
        (
            "execution_started",
            json!({"executionId":"execution-1","graphVersion":3,"graphHash":hash,"mode":"supervised"}),
        ),
        (
            "execution_mode_changed",
            json!({"executionId":"execution-1","previousMode":"supervised","mode":"manual"}),
        ),
        (
            "node_outcome_recorded",
            json!({"executionId":"execution-1","nodeId":"start","outcome":"retryable_failure","nextState":"queued"}),
        ),
        (
            "execution_completed",
            json!({"executionId":"execution-1","status":"failed"}),
        ),
    ];
    for (name, data) in cases {
        let wire = json!({"type": name, "data": data});
        let kind: EventKind = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&kind).unwrap(), wire, "{name}");
    }
}

/// An unknown mode must not silently become the permissive one.
#[test]
fn an_execution_cannot_start_in_an_unknown_mode() {
    let hash = format!("sha256:{}", "a".repeat(64));
    let wire = json!({
        "type": "execution_started",
        "data": {"executionId":"execution-1","graphVersion":3,"graphHash":hash,"mode":"god_mode"}
    });
    assert!(serde_json::from_value::<EventKind>(wire).is_err());
}
```

Use whatever `json!`/`EventKind` imports that file already has; do not add a second import block.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo +1.97.1 test -p graphhelm-protocols --test persistence_wire execution_`
Expected: FAIL, `unknown variant 'execution_started'`

- [ ] **Step 3: Add the payloads**

In `core/protocols/src/event.rs`, immediately after the `SimulationCompleted` struct:

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionStarted {
    pub execution_id: OpaqueId,
    pub graph_version: u64,
    pub graph_hash: WireHash,
    pub mode: ExecutionMode,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionModeChanged {
    pub execution_id: OpaqueId,
    pub previous_mode: Option<ExecutionMode>,
    pub mode: ExecutionMode,
}

/// One reported outcome for one node.
///
/// `next_state` is the decision `graphhelm_execution::apply_transition` produced. It is recorded
/// rather than recomputed here because `core/events` must not depend on `core/execution`; the
/// dependency direction runs the other way.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeOutcomeRecorded {
    pub execution_id: OpaqueId,
    pub node_id: OpaqueId,
    pub outcome: NodeOutcome,
    pub next_state: NodeState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionCompleted {
    pub execution_id: OpaqueId,
    pub status: SimulationStatus,
}
```

Add `ExecutionMode` and `NodeOutcome` to this file's existing `use crate::{...}` import list beside `NodeState`.

- [ ] **Step 4: Add the variants**

In the `EventKind` enum, after `SimulationCompleted(SimulationCompleted),`:

```rust
    ExecutionStarted(ExecutionStarted),
    ExecutionModeChanged(ExecutionModeChanged),
    NodeOutcomeRecorded(NodeOutcomeRecorded),
    ExecutionCompleted(ExecutionCompleted),
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo +1.97.1 test -p graphhelm-protocols --test persistence_wire`
Expected: The two new tests PASS.

`persistence_wire.rs` asserts `variants.len() == 16` in two places. Sixteen becomes twenty. Update both, and **only** those two numbers — that assertion is an inventory of event kinds and adding four is exactly what this task does. If any other count in that file moves, stop and report it rather than editing it.

- [ ] **Step 6: Run the whole workspace to find exhaustive matches**

Run: `cargo +1.97.1 check --workspace --all-targets`
Expected: errors of the form `` non-exhaustive patterns: `EventKind::ExecutionStarted(_)` not covered ``. Note every location; Task 5 handles the projection fold. For any *other* site, add an arm that preserves that site's existing behaviour for kinds it does not model — do not invent new behaviour outside the fold.

- [ ] **Step 7: Commit**

```bash
git add core/protocols/src/event.rs core/protocols/tests/persistence_wire.rs
git commit -m "feat(protocols): add the four execution event kinds"
```

---

### Task 4: The wire contract for the four kinds

**Files:**
- Modify: `schemas/event-envelope.schema.json`
- Modify: `schemas/releases/1.0.0/event-envelope.schema.json`
- Modify: `schemas/catalog.json`, `schemas/releases/1.0.0/catalog.json`
- Modify: `schemas/CHANGELOG.md`
- Test: `core/protocols/tests/persistence_wire.rs`

Per D-037 (`docs/DECISION_REGISTER.md:43`) the unpublished `1.0.0` baseline is corrected in place; no `1.1.0` is cut. Both copies must move byte-identically, because `core/schema-evolution/tests/catalog_integrity.rs:303-308` compares raw bytes. **Never edit one of those tests to make this pass** — if one fails, the edit is wrong.

- [ ] **Step 1: Write the failing test**

Add to `core/protocols/tests/persistence_wire.rs`, modelled on the existing envelope-validation case in that file:

```rust
#[test]
fn the_envelope_schema_accepts_every_execution_event_kind() {
    let hash = format!("sha256:{}", "a".repeat(64));
    for data in [
        json!({"type":"execution_started","data":{"executionId":"execution-1","graphVersion":3,"graphHash":hash,"mode":"supervised"}}),
        json!({"type":"execution_mode_changed","data":{"executionId":"execution-1","previousMode":null,"mode":"manual"}}),
        json!({"type":"node_outcome_recorded","data":{"executionId":"execution-1","nodeId":"start","outcome":"succeeded","nextState":"succeeded"}}),
        json!({"type":"execution_completed","data":{"executionId":"execution-1","status":"completed"}}),
    ] {
        assert_envelope_valid(data);
    }
}
```

Reuse the helper that file already uses to build and validate a full envelope around a `kind`. If it has no such helper, extract one from the existing `node_state_changed` case rather than duplicating the envelope literal four times.

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo +1.97.1 test -p graphhelm-protocols --test persistence_wire the_envelope_schema_accepts`
Expected: FAIL, `is not valid under any of the schemas listed in the 'oneOf' keyword`

- [ ] **Step 3: Add the four `$defs`**

In `schemas/event-envelope.schema.json`, beside `$defs/nodeStateChanged`, add `executionMode` and the four payload definitions. Model each on `nodeStateChanged`: `"type": "object"`, an explicit `required` list, `"additionalProperties": false`, and `$ref` to the existing `$defs` for ids, hashes, `nodeState` and `simulationStatus` rather than restating their constraints.

```json
"executionMode": {"enum": ["autopilot", "supervised", "manual"]},
"nodeOutcome": {
  "enum": [
    "started", "succeeded", "retryable_failure", "terminal_failure",
    "needs_input", "needs_capacity", "approved", "waived",
    "skipped", "cancelled", "invalidated"
  ]
}
```

The `nodeOutcome` values must match the `#[serde(rename_all = "snake_case")]` output of the Rust enum exactly. Derive them from the enum, do not retype them from memory.

- [ ] **Step 4: Add the four `oneOf` branches and scope rules**

Add one branch per kind to the `kind` `oneOf` list, following the `node_state_changed` branch shape. Then add the matching entry to the scope rule list near the end of the file — every execution event is scoped to an execution, so each uses `{"$ref": "#/$defs/scopeWithExecution"}` exactly as `node_state_changed` does.

- [ ] **Step 5: Mirror the frozen copy**

```bash
cp schemas/event-envelope.schema.json schemas/releases/1.0.0/event-envelope.schema.json
```

Byte-for-byte. On Windows confirm the line endings did not change.

- [ ] **Step 6: Recompute the digest**

There is no tooling for this. The algorithm in `core/schema-evolution/src/canonical.rs:67-98` is: sort object keys byte-wise, preserve array order, serialize compactly, SHA-256, lowercase hex, `sha256:` prefix.

```bash
python -c "
import json,hashlib,sys
d=json.load(open(sys.argv[1],encoding='utf-8'))
b=json.dumps(d,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode('utf-8')
print('sha256:'+hashlib.sha256(b).hexdigest())
" schemas/event-envelope.schema.json
```

Run it against the **unmodified** file first and confirm it reproduces the digest currently in `schemas/catalog.json`. If it does not, stop and report BLOCKED. **Never commit a hash you could not verify.** Then write the new digest into both catalogs and change nothing else in them.

- [ ] **Step 7: Verify the release gate**

Run: `cargo +1.97.1 run --locked -q -p graphhelm-cli -- schema check --baseline schemas/releases/1.0.0/catalog.json --candidate schemas/catalog.json`
Expected: class `unchanged`, impact `none`, `release.ok` true — both sides moved together. An impact of `minor` means Step 5 did not land.

Run: `cargo +1.97.1 test -p graphhelm-schema-evolution --locked`
Expected: PASS, including `checked_in_1_0_0_release_is_complete_and_raw_byte_identical`.

- [ ] **Step 8: Record it**

Add a plain `- ` bullet under `## [1.0.0]` in `schemas/CHANGELOG.md` naming the four kinds. Do **not** start it with `- BREAKING `; that prefix is parsed as major-change evidence by `apps/cli/src/commands/schema/check.rs:202`.

- [ ] **Step 9: Commit**

```bash
git add schemas core/protocols/tests/persistence_wire.rs
git commit -m "feat(schemas): add the execution event kinds to the envelope contract"
```

---

### Task 5: Execution fields on the projection

**Files:**
- Modify: `core/events/src/projection.rs`
- Test: `core/events/src/projection.rs` (inline `mod tests`)

`ExecutionProjection` already carries `node_states` and `simulation_status`, and decision 5.3 says `SimulationStatus` is reused as the aggregate — so this task adds no status field and no ghost set. A ghost node is exactly a node whose state is `NodeState::Ghost`; a second collection would be a second source of truth.

- [ ] **Step 1: Write the failing test**

Add to `core/events/src/projection.rs`:

```rust
#[cfg(test)]
mod execution_fields_tests {
    use super::*;

    /// The execution fields must round-trip, because a projection generation is persisted as JSON
    /// and reloaded; a field that serializes but does not deserialize would silently reset on
    /// every rebuild.
    #[test]
    fn execution_fields_survive_a_generation_round_trip() {
        let mut projection = ExecutionProjection::default();
        projection.execution_id = Some("execution-1".to_owned());
        projection.mode = Some(graphhelm_protocols::ExecutionMode::Supervised);
        projection.node_attempts.insert("start".to_owned(), 3);
        projection
            .last_outcome
            .insert("start".to_owned(), graphhelm_protocols::NodeOutcome::RetryableFailure);
        projection.identical_outcomes.insert("start".to_owned(), 2);

        let encoded = serde_json::to_string(&projection).unwrap();
        let decoded: ExecutionProjection = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, projection);
    }

    /// An older generation predates these fields. It must load with them empty rather than fail,
    /// because generations are disposable and a rebuild will refill them.
    #[test]
    fn a_generation_written_before_these_fields_still_loads() {
        let decoded: ExecutionProjection = serde_json::from_str("{}").unwrap();
        assert_eq!(decoded.execution_id, None);
        assert_eq!(decoded.mode, None);
        assert!(decoded.node_attempts.is_empty());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo +1.97.1 test -p graphhelm-events execution_fields`
Expected: FAIL, `no field 'execution_id' on type 'ExecutionProjection'`

- [ ] **Step 3: Add the fields**

In `core/events/src/projection.rs`, add to `ExecutionProjection` after `simulation_status`:

```rust
    /// The execution this projection describes, once one has started.
    pub execution_id: Option<String>,
    /// Autonomy in force, per D-022. `None` until an execution starts.
    pub mode: Option<ExecutionMode>,
    /// Attempts observed per node. Derived by folding outcomes, never read from a payload, so
    /// history stays the single source of truth.
    pub node_attempts: BTreeMap<String, u32>,
    /// The most recent outcome per node, used to detect consecutive identical outcomes.
    pub last_outcome: BTreeMap<String, NodeOutcome>,
    /// Consecutive semantically identical outcomes per node, per decision 5.7.
    pub identical_outcomes: BTreeMap<String, u32>,
```

Add `ExecutionMode` and `NodeOutcome` to this file's existing `graphhelm_protocols::{...}` import.

Confirm the struct already carries `#[serde(default)]` behaviour for absent fields. If it does not, add `#[serde(default)]` to each new field rather than to the struct, so an older generation loads with them empty — the second test above is what proves this.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo +1.97.1 test -p graphhelm-events execution_fields`
Expected: PASS, 2 passed

- [ ] **Step 5: Commit**

```bash
git add core/events/src/projection.rs
git commit -m "feat(events): add execution state to the projection"
```

---

### Task 6: Fold the execution events

**Files:**
- Modify: `core/events/src/projection.rs`
- Test: `core/events/tests/execution_projection.rs`

- [ ] **Step 1: Write the failing test**

Create `core/events/tests/execution_projection.rs`. Build envelopes with the same helper the existing `core/events/tests/replay.rs` uses — read that file and reuse its constructor rather than writing a new one, so hash chaining and scope stay correct.

```rust
//! Execution state is a projection over execution events. These tests prove the counters are
//! derived from history rather than trusted from a payload, and that replay reproduces them.

// Reuse the envelope builder from replay.rs's pattern: same scope, same stream, chained hashes.

/// Attempts are counted, not read. Nothing in the payload says "attempt 3".
#[test]
fn attempts_are_derived_by_folding_outcomes() {
    let events = execution_events(&[
        Outcome::Started,
        Outcome::RetryableFailure,
        Outcome::Started,
        Outcome::RetryableFailure,
        Outcome::Started,
    ]);
    let projection = replay(&scope(), STREAM, &events).unwrap();
    assert_eq!(projection.node_attempts.get("start"), Some(&3));
}

/// Consecutive identical outcomes are what decision 5.7 bounds. A different outcome in between
/// resets the run, otherwise a node alternating between two failures would never look stalled.
#[test]
fn identical_outcomes_count_consecutively_and_reset() {
    let stalled = replay(&scope(), STREAM, &execution_events(&[
        Outcome::RetryableFailure,
        Outcome::RetryableFailure,
        Outcome::RetryableFailure,
    ])).unwrap();
    assert_eq!(stalled.identical_outcomes.get("start"), Some(&3));

    let interrupted = replay(&scope(), STREAM, &execution_events(&[
        Outcome::RetryableFailure,
        Outcome::RetryableFailure,
        Outcome::NeedsInput,
        Outcome::RetryableFailure,
    ])).unwrap();
    assert_eq!(interrupted.identical_outcomes.get("start"), Some(&1));
}

/// The state recorded on the wire is the decision apply_transition produced. The projection stores
/// it; it does not second-guess it, because core/events cannot depend on core/execution.
#[test]
fn the_recorded_next_state_becomes_the_node_state() {
    let projection = replay(&scope(), STREAM, &execution_events(&[Outcome::Succeeded])).unwrap();
    assert_eq!(projection.node_states.get("start"), Some(&NodeState::Succeeded));
}

/// Replaying the same history twice must produce byte-identical state. Any map iteration or
/// ordering dependence here breaks the guarantee this milestone exists to prove.
#[test]
fn replay_is_identical_across_runs() {
    let events = execution_events(&[
        Outcome::Started,
        Outcome::RetryableFailure,
        Outcome::Started,
        Outcome::Succeeded,
    ]);
    let first = replay(&scope(), STREAM, &events).unwrap();
    let second = replay(&scope(), STREAM, &events).unwrap();
    assert_eq!(
        serde_json::to_string(&first).unwrap(),
        serde_json::to_string(&second).unwrap()
    );
}

/// A mode change mid-execution is recorded, per D-022.
#[test]
fn a_mode_change_is_reflected_in_the_projection() {
    let projection = replay(&scope(), STREAM, &started_then_mode_changed()).unwrap();
    assert_eq!(projection.mode, Some(ExecutionMode::Manual));
}
```

Write the `execution_events`, `started_then_mode_changed`, `scope` and `Outcome` helpers concretely in this file. `execution_events` emits one `execution_started` followed by one `node_outcome_recorded` per outcome for node `"start"`, with `next_state` set to whatever `graphhelm_execution::apply_transition` would produce for that step — compute it in the helper by calling `apply_transition`, so the fixture cannot drift from the state machine. `core/events` may not depend on `core/execution`, but this is a **dev**-dependency of the test crate, which does not create a crate cycle; add `graphhelm-execution = { path = "../execution" }` under `[dev-dependencies]` in `core/events/Cargo.toml`. If cargo reports a cycle, stop and report it — do not hand-write the states instead, because a hand-written fixture is exactly the drift this guards against.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo +1.97.1 test -p graphhelm-events --test execution_projection`
Expected: FAIL. The fold has no arms for the new kinds yet, so counters stay empty and the first assertion fails with `left: None, right: Some(3)`.

- [ ] **Step 3: Write the fold**

In `apply_projection_event`, add arms after the `EventKind::NodeStateChanged` arm:

```rust
        EventKind::ExecutionStarted(payload) => {
            if projection.execution_id.is_some() {
                return Err(ReplayError::Corrupt);
            }
            projection.execution_id = Some(payload.execution_id.to_string());
            projection.mode = Some(payload.mode);
        }
        EventKind::ExecutionModeChanged(payload) => {
            if projection.mode != payload.previous_mode {
                return Err(ReplayError::Corrupt);
            }
            projection.mode = Some(payload.mode);
        }
        EventKind::NodeOutcomeRecorded(payload) => {
            let node = payload.node_id.to_string();
            if projection.node_attempts.len() >= MAX_PROJECTION_NODES
                && !projection.node_attempts.contains_key(&node)
            {
                return Err(ReplayError::LimitExceeded);
            }
            let attempts = projection.node_attempts.entry(node.clone()).or_insert(0);
            *attempts = attempts.checked_add(1).ok_or(ReplayError::LimitExceeded)?;

            let run = match projection.last_outcome.insert(node.clone(), payload.outcome) {
                Some(previous) if previous == payload.outcome => projection
                    .identical_outcomes
                    .get(&node)
                    .copied()
                    .unwrap_or(0)
                    .checked_add(1)
                    .ok_or(ReplayError::LimitExceeded)?,
                _ => 1,
            };
            projection.identical_outcomes.insert(node.clone(), run);
            projection.node_states.insert(node, payload.next_state);
        }
        EventKind::ExecutionCompleted(payload) => {
            if projection.execution_id.as_deref() != Some(payload.execution_id.as_str()) {
                return Err(ReplayError::Corrupt);
            }
            projection.simulation_status = Some(payload.status);
        }
```

Add the bound near the other projection constants in the same file:

```rust
// A projection is loaded whole and is size-checked on write and read. Bounding the node maps keeps
// a hostile history from growing it without limit before that check can reject it.
const MAX_PROJECTION_NODES: usize = 10_000;
```

If `payload.execution_id` does not expose `as_str`, compare through the same accessor the neighbouring arms use for `OpaqueId`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo +1.97.1 test -p graphhelm-events --test execution_projection`
Expected: PASS, 5 passed

- [ ] **Step 5: Prove the tests can fail**

A property that has never been observed failing proves nothing — milestone 03's review found that defect twice. Temporarily change the `_ => 1` arm to `_ => 0` and confirm `identical_outcomes_count_consecutively_and_reset` fails. Then revert completely and re-confirm green. Quote both outputs in your report. Do not commit the sabotage.

- [ ] **Step 6: Commit**

```bash
git add core/events/src/projection.rs core/events/tests/execution_projection.rs core/events/Cargo.toml
git commit -m "feat(events): fold execution events into the projection"
```

---

### Task 7: Rebuild, watermark and the durable path

**Files:**
- Modify: `core/events/tests/execution_projection.rs`
- Test: `adapters/postgres-event-store/tests/projection.rs`

The generation, watermark and swap machinery already exists and is tested. This task proves it carries the new state, and that a stale watermark is rejected rather than silently accepted.

- [ ] **Step 1: Write the failing test**

Add to `core/events/tests/execution_projection.rs`:

```rust
/// A projection generation is disposable: discarding it and rebuilding from history must land on
/// exactly the same state. This is what decision 5.6 rests on.
#[test]
fn a_discarded_generation_rebuilds_to_identical_state() {
    let events = execution_events(&[
        Outcome::Started,
        Outcome::RetryableFailure,
        Outcome::Started,
        Outcome::Succeeded,
    ]);
    let direct = replay(&scope(), STREAM, &events).unwrap();
    let (first_half, second_half) = events.split_at(3);
    let resumed = resume_via_generation(first_half, second_half);
    assert_eq!(
        serde_json::to_string(&direct).unwrap(),
        serde_json::to_string(&resumed).unwrap()
    );
}
```

`replay_from` above is shorthand. The real incremental entry point is
`ProjectionGeneration::apply_page(&mut self, events: &[EventEnvelope])`, built with
`ProjectionGeneration::new(scope, stream_id, projection_name, projection_version, generation)` and
read back through `generation.projection()`. Write `resume_via_generation` as:

```rust
fn resume_via_generation(first: &[EventEnvelope], second: &[EventEnvelope]) -> ExecutionProjection {
    let mut generation =
        ProjectionGeneration::new(scope(), STREAM.to_owned(), "execution".to_owned(), 1, 1).unwrap();
    generation.apply_page(first).unwrap();
    generation.apply_page(second).unwrap();
    generation.projection().clone()
}
```

Add no new public function to `core/events` for the test's convenience.

- [ ] **Step 2: Run it to verify it fails, then make it pass**

Run: `cargo +1.97.1 test -p graphhelm-events --test execution_projection a_discarded_generation`
If it passes on the first run, that is expected — the machinery already exists and this test is a guard on it. In that case prove it can fail: temporarily drop the `projection.node_attempts` update from the fold, confirm this test fails, revert, re-confirm. Quote both outputs.

- [ ] **Step 3: Prove the watermark still guards the swap**

In `adapters/postgres-event-store/tests/projection.rs`, find the existing test that exercises `GHPROJ001_WATERMARK_MISMATCH`. Extend its fixture so the generation being swapped carries execution state, and confirm the diagnostic is still produced when the source head has moved. The point is that adding fields did not make a stale generation look acceptable.

Run: `./ci/postgres.ps1 -TestArgs @('+1.97.1','test','-p','graphhelm-postgres-event-store','--test','projection','--all-features','--locked','--','--ignored','--test-threads=1')`
Expected: PASS.

If no such test exists, say so plainly in your report rather than inventing one — `GHPROJ001_WATERMARK_MISMATCH` was implemented in milestone 03 but recorded as unproven, and this is the moment to prove it. Write it in that case, and state that you did.

- [ ] **Step 4: Commit**

```bash
git add core/events/tests/execution_projection.rs adapters/postgres-event-store/tests/projection.rs
git commit -m "test(events): prove the execution projection rebuilds and the watermark still guards"
```

---

### Task 8: Documentation and the full gate

**Files:**
- Modify: `docs/architecture/SYSTEM_ARCHITECTURE.md`
- Modify: `CHANGELOG.md`
- Create: `docs/milestones/graph-engine-governor.md`

- [ ] **Step 1: Record what exists now**

Create `docs/milestones/graph-engine-governor.md` following the structure of `docs/milestones/production-event-evidence-store.md`. Cover only what 04a and 04b actually shipped: the execution contracts crate, the four event kinds, and the execution projection. State explicitly that scheduling, in-flight governance, pause/resume and the operator CLI are 04c through 04f and do not exist yet.

Documentation is always in English in this repository.

- [ ] **Step 2: Update the architecture and changelog**

In `docs/architecture/SYSTEM_ARCHITECTURE.md`, extend the event-kind list and the projection description. In `CHANGELOG.md`, add a section for this milestone in the voice of the existing entries.

Do not claim any property the tests do not demonstrate. Milestone 03's review found three documentation claims the code did not support; the fix is to describe what is proven.

- [ ] **Step 3: Run the full gate**

Run: `./ci/gate.ps1`
Expected: `[gate] GREEN - every stage passed.`

This plan changes the wire contract and the projection, so the PostgreSQL matrix is required — do not run with `-SkipPostgres`. It needs `GRAPHHELM_PG_BIN` pointing at a PostgreSQL 16+ `bin` directory.

- [ ] **Step 4: Commit**

```bash
git add docs CHANGELOG.md
git commit -m "docs(m04): record the execution contracts and durable projection"
```

---

## Definition of done

- `NodeOutcome` and `ExecutionMode` exist once each, in `graphhelm-protocols`, and no second copy exists anywhere.
- The four execution event kinds round-trip in Rust and validate against the envelope schema, with both schema copies byte-identical and both catalog digests recomputed and verified.
- `ExecutionProjection` carries execution state; a generation written before these fields still loads.
- `node_attempts` and `identical_outcomes` are derived by folding history; no payload carries a counter.
- Replaying the same history twice produces byte-identical state, and a discarded generation rebuilds to the same state as a direct replay.
- A stale watermark still yields `GHPROJ001_WATERMARK_MISMATCH` with execution state present.
- Every new property has been observed failing at least once, deliberately, and the sabotage reverted.
- `./ci/gate.ps1` is green, PostgreSQL matrix included.

## What this plan deliberately excludes

Ready-set computation, bounded concurrency and no-progress *decisions* (04c) — this plan derives the counters those decisions will read, but takes no decision from them. Signal intake, ghost approval, mutation publication and owner override (04d). Checkpoint content, resume preconditions and crash recovery (04e). The operator CLI (04f).

It also excludes any use of `MAX_NODE_ATTEMPTS` or `MAX_IDENTICAL_OUTCOMES`. Those bounds live in `core/execution` and `core/events` cannot import them; the projection counts without judging, and 04c does the judging.
