# Milestone 04a - Execution Contracts and State Machine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create `core/execution`, a pure crate defining the closed execution state machine, the closed Graph Signal typed subset, and the `NodeExecutor` seam, so later plans can schedule, persist and govern an execution without redefining any of it.

**Architecture:** One new crate with no I/O, no clock, no randomness and no adapter dependency. It reuses `graphhelm_protocols::NodeState` rather than inventing a vocabulary, adds the single `Ghost` variant required by decision 5.2, and expresses every rule as a total function over values so the whole crate is property-testable. Bounds are counters, never durations, so replay reproduces identical decisions.

**Tech Stack:** Rust 1.97.1, edition 2024. `graphhelm-protocols` for shared types, `serde`/`serde_json` for the signal subset, `proptest` for state-machine properties. No `tokio`, no `sqlx`, no filesystem.

**Design source:** `docs/superpowers/specs/2026-08-13-graph-engine-governor-design.md`, decisions 5.2, 5.3, 5.4, 5.7.

---

### Task 1: Create the crate and add the `Ghost` node state

**Files:**
- Modify: `Cargo.toml:3-15`
- Create: `core/execution/Cargo.toml`
- Create: `core/execution/src/lib.rs`
- Modify: `core/protocols/src/simulation.rs:6-22`
- Test: `core/protocols/src/simulation.rs` (inline `mod tests`)

- [ ] **Step 1: Write the failing test**

Append to `core/protocols/src/simulation.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::NodeState;

    /// A proposed expansion must be representable in the one shared vocabulary. A second
    /// execution-only enum would let simulation and execution drift apart permanently.
    #[test]
    fn ghost_is_part_of_the_shared_node_state_vocabulary() {
        let ghost = NodeState::Ghost;
        let encoded = serde_json::to_string(&ghost).unwrap();
        assert_eq!(encoded, "\"ghost\"");
        assert_eq!(serde_json::from_str::<NodeState>("\"ghost\"").unwrap(), ghost);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo +1.97.1 test -p graphhelm-protocols ghost_is_part_of`
Expected: FAIL, `no variant named 'Ghost' found for enum 'NodeState'`

- [ ] **Step 3: Add the variant**

In `core/protocols/src/simulation.rs`, add `Ghost` to `NodeState` immediately after `Draft`:

```rust
pub enum NodeState {
    Draft,
    /// A Governor-proposed expansion, visible before approval and never scheduled.
    Ghost,
    Linting,
    Ready,
    Queued,
    Running,
    WaitingInput,
    WaitingCapacity,
    Paused,
    Blocked,
    Succeeded,
    Failed,
    Waived,
    Skipped,
    Cancelled,
    Invalidated,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo +1.97.1 test -p graphhelm-protocols ghost_is_part_of`
Expected: PASS

- [ ] **Step 5: Create the crate manifest**

Create `core/execution/Cargo.toml`:

```toml
[package]
name = "graphhelm-execution"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
graphhelm-protocols = { path = "../protocols" }
serde.workspace = true
serde_json.workspace = true

[dev-dependencies]
proptest.workspace = true
```

- [ ] **Step 6: Register the crate**

In `Cargo.toml`, add `"core/execution",` to `members` immediately after `"core/simulation",`.

No dependency edit is needed: `proptest` is already pinned in `[workspace.dependencies]` and already consumed by `core/graph` and `core/schema-evolution`. Do not change that pin — a downgrade would silently affect two unrelated crates.

- [ ] **Step 7: Create the crate root**

Create `core/execution/src/lib.rs`:

```rust
//! Pure execution contracts for the Graph Engine.
//!
//! This crate is total and side-effect free: no clock, no randomness, no filesystem, no network and
//! no adapter dependency. Every rule is a function over values so that replaying the same inputs
//! reproduces the same decision exactly.

mod bounds;
mod signal;
mod transition;

pub use bounds::{
    MAX_ACCEPTED_MUTATIONS, MAX_IDENTICAL_OUTCOMES, MAX_NODE_ATTEMPTS, MAX_READY_SET,
    MAX_SIGNALS_PER_EXECUTION,
};
pub use signal::{
    SignalError, SignalKind, SignalSeverity, SignalSource, SignalSourceKind, TypedSignal,
};
pub use transition::{ExecutionError, NodeExecutor, NodeOutcome, TransitionRequest, apply_transition};
```

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml core/execution core/protocols/src/simulation.rs
git commit -m "feat(execution): create the execution crate and add the Ghost node state"
```

---

### Task 2: Replay-stable bounds

**Files:**
- Create: `core/execution/src/bounds.rs`
- Test: `core/execution/src/bounds.rs` (inline `mod tests`)

- [ ] **Step 1: Write the failing test**

Create `core/execution/src/bounds.rs` containing only the test:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// Every bound is a count. A duration would make the same history replay to a different
    /// decision on a slower machine, which breaks the replay guarantee this milestone rests on.
    #[test]
    fn bounds_are_counts_and_are_ordered_sensibly() {
        assert_eq!(MAX_NODE_ATTEMPTS, 8);
        assert_eq!(MAX_IDENTICAL_OUTCOMES, 3);
        assert_eq!(MAX_ACCEPTED_MUTATIONS, 64);
        assert_eq!(MAX_READY_SET, 1024);
        assert_eq!(MAX_SIGNALS_PER_EXECUTION, 10_000);
        assert!(MAX_IDENTICAL_OUTCOMES < MAX_NODE_ATTEMPTS);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo +1.97.1 test -p graphhelm-execution bounds_are_counts`
Expected: FAIL, `cannot find value 'MAX_NODE_ATTEMPTS' in this scope`

- [ ] **Step 3: Write the constants**

Prepend to `core/execution/src/bounds.rs`:

```rust
//! Bounds for execution progress.
//!
//! Every value here is a count of attempts or events, never a wall-clock duration. Replay must
//! reproduce the identical decision, and a timing-dependent bound cannot.

/// Attempts of one node before the execution blocks for an owner decision.
pub const MAX_NODE_ATTEMPTS: u32 = 8;

/// Consecutive semantically identical outcomes treated as no progress.
pub const MAX_IDENTICAL_OUTCOMES: u32 = 3;

/// Governor mutations one execution may accept before blocking for an owner decision.
pub const MAX_ACCEPTED_MUTATIONS: u32 = 64;

/// Nodes that may be ready at once. Exceeding this blocks rather than truncating.
pub const MAX_READY_SET: usize = 1024;

/// Signals one execution may record. Exceeding this blocks rather than dropping evidence.
pub const MAX_SIGNALS_PER_EXECUTION: u32 = 10_000;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo +1.97.1 test -p graphhelm-execution bounds_are_counts`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add core/execution/src/bounds.rs
git commit -m "feat(execution): add replay-stable execution bounds"
```

---

### Task 3: The closed Graph Signal typed subset

**Files:**
- Create: `core/execution/src/signal.rs`
- Test: `core/execution/src/signal.rs` (inline `mod tests`)

Per decision 5.4, `schemas/graph-signal.schema.json` already closes `source.type` but leaves `type` an open string. This models the stable typed subset and preserves the raw type for forward compatibility. A signal is a proposal and never mutates anything.

- [ ] **Step 1: Write the failing test**

Create `core/execution/src/signal.rs` containing only the tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(kind: &str) -> serde_json::Value {
        serde_json::json!({
            "id": "signal-1",
            "source": {"type": "node", "id": "node-a"},
            "type": kind,
            "severity": "high",
            "description": "a dependency was discovered",
            "evidence": ["exec-1"],
            "emittedAt": "2026-08-13T00:00:00Z"
        })
    }

    #[test]
    fn a_recognized_signal_parses_into_the_typed_subset() {
        let signal = TypedSignal::parse(&envelope("unexpected_dependency")).unwrap();
        assert_eq!(signal.kind(), SignalKind::UnexpectedDependency);
        assert_eq!(signal.severity(), SignalSeverity::High);
        assert_eq!(signal.source().kind(), SignalSourceKind::Node);
        assert!(signal.can_propose_mutation());
    }

    /// An unrecognized type is kept as evidence, because the emitting agent is not authoritative
    /// and discarding the record would lose information. It can never produce a mutation.
    #[test]
    fn an_unrecognized_signal_is_recorded_but_can_never_propose_a_mutation() {
        let signal = TypedSignal::parse(&envelope("invented_by_an_agent")).unwrap();
        assert_eq!(signal.kind(), SignalKind::Unrecognized);
        assert_eq!(signal.raw_kind(), "invented_by_an_agent");
        assert!(!signal.can_propose_mutation());
    }

    #[test]
    fn a_signal_violating_the_schema_contract_is_rejected() {
        let mut missing_evidence = envelope("unexpected_dependency");
        missing_evidence["evidence"] = serde_json::json!([]);
        assert_eq!(
            TypedSignal::parse(&missing_evidence).unwrap_err(),
            SignalError::Invalid
        );

        let mut unknown_source = envelope("unexpected_dependency");
        unknown_source["source"]["type"] = serde_json::json!("oracle");
        assert_eq!(
            TypedSignal::parse(&unknown_source).unwrap_err(),
            SignalError::Invalid
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo +1.97.1 test -p graphhelm-execution --lib signal`
Expected: FAIL, `cannot find type 'TypedSignal' in this scope`

- [ ] **Step 3: Write the implementation**

Prepend to `core/execution/src/signal.rs`:

```rust
//! The stable typed subset of `schemas/graph-signal.schema.json`.
//!
//! The schema closes `source.type` but leaves `type` an open string for forward compatibility, so
//! this models the recognized kinds and preserves the raw value. A signal is always a proposal:
//! nothing here mutates a graph, and only a recognized kind may reach the Governor at all.

use serde::Deserialize;

/// Signal kinds this milestone recognizes, from `HARNESS_SPEC.md` §19.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalKind {
    UnexpectedDependency,
    AuthBoundaryDiscovered,
    ToolFailure,
    QuotaExhausted,
    StaleDraft,
    NoProgress,
    /// A type outside the recognized set. Recorded as evidence, never actionable.
    Unrecognized,
}

impl SignalKind {
    fn parse(value: &str) -> Self {
        match value {
            "unexpected_dependency" => Self::UnexpectedDependency,
            "auth_boundary_discovered" => Self::AuthBoundaryDiscovered,
            "tool_failure" => Self::ToolFailure,
            "quota_exhausted" => Self::QuotaExhausted,
            "stale_draft" => Self::StaleDraft,
            "no_progress" => Self::NoProgress,
            _ => Self::Unrecognized,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SignalSeverity {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SignalSourceKind {
    Node,
    Runtime,
    Tool,
    Test,
    User,
    Dream,
    System,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SignalSource {
    #[serde(rename = "type")]
    kind: SignalSourceKind,
    id: String,
}

impl SignalSource {
    #[must_use]
    pub const fn kind(&self) -> SignalSourceKind {
        self.kind
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalError {
    Invalid,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawSignal {
    id: String,
    source: SignalSource,
    #[serde(rename = "type")]
    kind: String,
    severity: SignalSeverity,
    description: String,
    evidence: Vec<String>,
    #[serde(default)]
    recommendations: Vec<String>,
    emitted_at: String,
}

/// A validated signal. Construction is the only way to obtain one, so an invalid signal cannot
/// reach the Governor.
#[derive(Clone, Debug)]
pub struct TypedSignal {
    raw: RawSignal,
    kind: SignalKind,
}

impl TypedSignal {
    /// Validates the envelope against the contract the schema fixes.
    ///
    /// # Errors
    /// Returns `SignalError::Invalid` when a required field is absent, empty, or outside the closed
    /// vocabularies the schema defines.
    pub fn parse(value: &serde_json::Value) -> Result<Self, SignalError> {
        let raw: RawSignal =
            serde_json::from_value(value.clone()).map_err(|_| SignalError::Invalid)?;
        if raw.id.is_empty()
            || raw.source.id.is_empty()
            || raw.kind.is_empty()
            || raw.description.is_empty()
            || raw.evidence.is_empty()
            || raw.evidence.iter().any(String::is_empty)
            || raw.emitted_at.is_empty()
        {
            return Err(SignalError::Invalid);
        }
        let kind = SignalKind::parse(&raw.kind);
        Ok(Self { raw, kind })
    }

    #[must_use]
    pub const fn kind(&self) -> SignalKind {
        self.kind
    }

    #[must_use]
    pub fn raw_kind(&self) -> &str {
        &self.raw.kind
    }

    #[must_use]
    pub const fn severity(&self) -> SignalSeverity {
        self.raw.severity
    }

    #[must_use]
    pub const fn source(&self) -> &SignalSource {
        &self.raw.source
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.raw.id
    }

    /// Whether this signal may be turned into a draft by the Governor.
    ///
    /// An unrecognized kind never can. This is the fail-closed boundary: the record is kept as
    /// evidence, but an agent cannot invent a signal type and thereby cause a mutation.
    #[must_use]
    pub const fn can_propose_mutation(&self) -> bool {
        !matches!(self.kind, SignalKind::Unrecognized)
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo +1.97.1 test -p graphhelm-execution --lib signal`
Expected: PASS, 3 passed

- [ ] **Step 5: Commit**

```bash
git add core/execution/src/signal.rs
git commit -m "feat(execution): add the closed Graph Signal typed subset"
```

---

### Task 4: The node transition function

**Files:**
- Create: `core/execution/src/transition.rs`
- Test: `core/execution/src/transition.rs` (inline `mod tests`)

- [ ] **Step 1: Write the failing test**

Create `core/execution/src/transition.rs` containing only the tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use graphhelm_protocols::NodeState;

    fn request(from: NodeState, outcome: NodeOutcome, attempts: u32) -> TransitionRequest {
        TransitionRequest {
            current: from,
            outcome,
            attempts,
            identical_outcomes: 0,
        }
    }

    #[test]
    fn a_queued_node_starts_and_then_succeeds() {
        let started = apply_transition(&request(NodeState::Queued, NodeOutcome::Started, 0)).unwrap();
        assert_eq!(started, NodeState::Running);
        let done = apply_transition(&request(NodeState::Running, NodeOutcome::Succeeded, 1)).unwrap();
        assert_eq!(done, NodeState::Succeeded);
    }

    /// A ghost is a proposal. It never runs, which is how "consumes no tokens" is enforced
    /// structurally rather than by convention.
    #[test]
    fn a_ghost_node_can_never_start() {
        assert_eq!(
            apply_transition(&request(NodeState::Ghost, NodeOutcome::Started, 0)).unwrap_err(),
            ExecutionError::IllegalTransition
        );
        assert_eq!(
            apply_transition(&request(NodeState::Ghost, NodeOutcome::Approved, 0)).unwrap(),
            NodeState::Ready
        );
    }

    #[test]
    fn a_terminal_node_never_transitions_again() {
        for terminal in [
            NodeState::Succeeded,
            NodeState::Failed,
            NodeState::Cancelled,
            NodeState::Waived,
            NodeState::Skipped,
        ] {
            assert_eq!(
                apply_transition(&request(terminal, NodeOutcome::Started, 1)).unwrap_err(),
                ExecutionError::IllegalTransition,
                "{terminal:?} must be terminal"
            );
        }
    }

    /// Exhausting attempts blocks for an owner decision. It never silently gives up, and it never
    /// consults a clock.
    #[test]
    fn exhausting_attempts_blocks_rather_than_failing_silently() {
        let retryable = request(NodeState::Running, NodeOutcome::RetryableFailure, MAX_NODE_ATTEMPTS - 1);
        assert_eq!(apply_transition(&retryable).unwrap(), NodeState::Queued);

        let exhausted = request(NodeState::Running, NodeOutcome::RetryableFailure, MAX_NODE_ATTEMPTS);
        assert_eq!(apply_transition(&exhausted).unwrap(), NodeState::Blocked);
    }

    #[test]
    fn repeated_identical_outcomes_block_as_no_progress() {
        let stalled = TransitionRequest {
            current: NodeState::Running,
            outcome: NodeOutcome::RetryableFailure,
            attempts: 1,
            identical_outcomes: MAX_IDENTICAL_OUTCOMES,
        };
        assert_eq!(apply_transition(&stalled).unwrap(), NodeState::Blocked);
    }

    #[test]
    fn an_owner_waiver_clears_a_blocked_node() {
        assert_eq!(
            apply_transition(&request(NodeState::Blocked, NodeOutcome::Waived, 3)).unwrap(),
            NodeState::Waived
        );
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo +1.97.1 test -p graphhelm-execution --lib transition`
Expected: FAIL, `cannot find function 'apply_transition' in this scope`

- [ ] **Step 3: Write the implementation**

Prepend to `core/execution/src/transition.rs`:

```rust
//! The node transition function.
//!
//! `apply_transition` is total over its inputs and consults no clock, no randomness and no I/O, so
//! replaying the same request always yields the same state. Every bound it applies is a counter.

use graphhelm_protocols::NodeState;

use crate::bounds::{MAX_IDENTICAL_OUTCOMES, MAX_NODE_ATTEMPTS};

/// What happened to a node, as reported by the executor or the owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionError {
    /// The outcome is not legal from the current state.
    IllegalTransition,
}

/// One transition request. Counters are supplied by the caller so this function stays pure.
#[derive(Clone, Copy, Debug)]
pub struct TransitionRequest {
    pub current: NodeState,
    pub outcome: NodeOutcome,
    /// Attempts already made, not counting the one being reported.
    pub attempts: u32,
    /// Consecutive semantically identical outcomes already observed.
    pub identical_outcomes: u32,
}

/// Performs node work. The only milestone-04 implementation is effect-free; Milestone 05 supplies
/// one that calls real models and tools behind this same contract.
pub trait NodeExecutor {
    /// Executes one attempt of a node and reports its outcome.
    ///
    /// # Errors
    /// Returns `ExecutionError::IllegalTransition` when asked to execute a node that cannot run.
    fn execute(&self, node_id: &str, attempt: u32) -> Result<NodeOutcome, ExecutionError>;
}

const fn is_terminal(state: NodeState) -> bool {
    matches!(
        state,
        NodeState::Succeeded
            | NodeState::Failed
            | NodeState::Waived
            | NodeState::Skipped
            | NodeState::Cancelled
    )
}

/// Applies one outcome to one node.
///
/// # Errors
/// Returns `ExecutionError::IllegalTransition` when the outcome is not reachable from `current`.
pub fn apply_transition(request: &TransitionRequest) -> Result<NodeState, ExecutionError> {
    use NodeOutcome as O;
    use NodeState as S;

    // A terminal node is immutable. Only invalidation, which is an upstream event rather than an
    // outcome of this node, may reopen a success.
    if is_terminal(request.current) {
        return match (request.current, request.outcome) {
            (S::Succeeded, O::Invalidated) => Ok(S::Invalidated),
            _ => Err(ExecutionError::IllegalTransition),
        };
    }

    // Cancellation is owner sovereignty and applies from any non-terminal state.
    if request.outcome == O::Cancelled {
        return Ok(S::Cancelled);
    }

    match (request.current, request.outcome) {
        // A ghost is a proposal: approval readies it, nothing else may touch it.
        (S::Ghost, O::Approved) => Ok(S::Ready),
        (S::Ghost, _) => Err(ExecutionError::IllegalTransition),

        (S::Draft | S::Linting, O::Approved) => Ok(S::Ready),
        (S::Ready, O::Started) => Ok(S::Queued),
        (S::Queued, O::Started) => Ok(S::Running),

        (S::Running, O::Succeeded) => Ok(S::Succeeded),
        (S::Running, O::TerminalFailure) => Ok(S::Failed),
        (S::Running, O::NeedsInput) => Ok(S::WaitingInput),
        (S::Running, O::NeedsCapacity) => Ok(S::WaitingCapacity),

        // Retry is bounded by counters only. Exhaustion blocks for an owner decision rather than
        // failing silently, so no work is discarded without a record.
        (S::Running, O::RetryableFailure) => {
            if request.attempts >= MAX_NODE_ATTEMPTS
                || request.identical_outcomes >= MAX_IDENTICAL_OUTCOMES
            {
                Ok(S::Blocked)
            } else {
                Ok(S::Queued)
            }
        }

        (S::WaitingInput, O::NeedsInput) => Ok(S::WaitingInput),
        (S::WaitingCapacity, O::NeedsCapacity) => Ok(S::WaitingCapacity),
        (S::WaitingInput | S::WaitingCapacity | S::Paused, O::Started) => Ok(S::Queued),

        (_, O::Waived) => Ok(S::Waived),
        (_, O::Skipped) => Ok(S::Skipped),
        (S::Invalidated, O::Approved) => Ok(S::Ready),

        _ => Err(ExecutionError::IllegalTransition),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo +1.97.1 test -p graphhelm-execution --lib transition`
Expected: PASS, 6 passed

- [ ] **Step 5: Commit**

```bash
git add core/execution/src/transition.rs
git commit -m "feat(execution): add the total node transition function"
```

---

### Task 5: Property tests for totality and determinism

**Files:**
- Create: `core/execution/tests/transition_properties.rs`

- [ ] **Step 1: Write the failing test**

Create `core/execution/tests/transition_properties.rs`:

```rust
use graphhelm_execution::{ExecutionError, NodeOutcome, TransitionRequest, apply_transition};
use graphhelm_protocols::NodeState;
use proptest::prelude::*;

const STATES: [NodeState; 16] = [
    NodeState::Draft,
    NodeState::Ghost,
    NodeState::Linting,
    NodeState::Ready,
    NodeState::Queued,
    NodeState::Running,
    NodeState::WaitingInput,
    NodeState::WaitingCapacity,
    NodeState::Paused,
    NodeState::Blocked,
    NodeState::Succeeded,
    NodeState::Failed,
    NodeState::Waived,
    NodeState::Skipped,
    NodeState::Cancelled,
    NodeState::Invalidated,
];

const OUTCOMES: [NodeOutcome; 11] = [
    NodeOutcome::Started,
    NodeOutcome::Succeeded,
    NodeOutcome::RetryableFailure,
    NodeOutcome::TerminalFailure,
    NodeOutcome::NeedsInput,
    NodeOutcome::NeedsCapacity,
    NodeOutcome::Approved,
    NodeOutcome::Waived,
    NodeOutcome::Skipped,
    NodeOutcome::Cancelled,
    NodeOutcome::Invalidated,
];

fn request(state: usize, outcome: usize, attempts: u32, identical: u32) -> TransitionRequest {
    TransitionRequest {
        current: STATES[state % STATES.len()],
        outcome: OUTCOMES[outcome % OUTCOMES.len()],
        attempts,
        identical_outcomes: identical,
    }
}

proptest! {
    /// The function is total: every input either yields a state or a typed error, never a panic.
    #[test]
    fn every_input_is_total(state in 0usize..64, outcome in 0usize..64, attempts in 0u32..64, identical in 0u32..64) {
        let _ = apply_transition(&request(state, outcome, attempts, identical));
    }

    /// The same request always yields the same answer. Any clock or ordering dependence here would
    /// break replay, which is the defect class this milestone must not repeat.
    #[test]
    fn transitions_are_deterministic(state in 0usize..64, outcome in 0usize..64, attempts in 0u32..64, identical in 0u32..64) {
        let request = request(state, outcome, attempts, identical);
        prop_assert_eq!(apply_transition(&request), apply_transition(&request));
    }

    /// A ghost never reaches a running state by any path, at any counter value.
    #[test]
    fn a_ghost_never_becomes_runnable(outcome in 0usize..64, attempts in 0u32..64, identical in 0u32..64) {
        let mut request = request(0, outcome, attempts, identical);
        request.current = NodeState::Ghost;
        match apply_transition(&request) {
            Ok(next) => prop_assert!(
                !matches!(next, NodeState::Queued | NodeState::Running),
                "a ghost reached {next:?}"
            ),
            Err(ExecutionError::IllegalTransition) => {}
        }
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo +1.97.1 test -p graphhelm-execution --test transition_properties`
Expected: FAIL to compile, `unresolved import 'proptest'` until Task 1 Step 6 is present; if Task 1 was completed, expect PASS.

- [ ] **Step 3: Confirm the public surface the properties rely on**

Run: `cargo +1.97.1 doc -p graphhelm-execution --no-deps 2>&1 | grep -c warning`
Expected: `0`. A non-zero count means a public item references a type that is not itself public; re-export it from `core/execution/src/lib.rs` alongside the others rather than making the item private.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo +1.97.1 test -p graphhelm-execution --test transition_properties`
Expected: PASS, 3 passed

- [ ] **Step 5: Commit**

```bash
git add core/execution/tests/transition_properties.rs
git commit -m "test(execution): prove transitions are total, deterministic and ghost-safe"
```

---

### Task 6: Close the ghost wire drift

**Files:**
- Modify: `schemas/event-envelope.schema.json` (`$defs/nodeState`)
- Modify: `schemas/releases/1.0.0/event-envelope.schema.json`
- Modify: `schemas/catalog.json`, `schemas/releases/1.0.0/catalog.json`
- Test: `core/protocols/tests/persistence_wire.rs`

Task 1 put `Ghost` in the shared `NodeState` on the argument that one vocabulary cannot drift. But `NodeState` is on the wire through `NodeStateChanged`, and `$defs/nodeState` is a closed enum without `"ghost"`, so Task 1 created exactly the drift it was meant to prevent. Nothing in the repository asserts parity between the Rust enum and the schema enum, so the gap is silent.

Per D-037 (`docs/DECISION_REGISTER.md:43`) the unpublished `1.0.0` baseline is corrected in place; no `1.1.0` is cut. Both copies of the schema must move together and byte-identically, because `core/schema-evolution/tests/catalog_integrity.rs:303-308` compares raw bytes. Never edit one of those tests to make this pass.

- [ ] **Step 1: Write the failing test**

Add to `core/protocols/tests/persistence_wire.rs` a case asserting that an event envelope carrying `"nextState": "ghost"` validates. Model it on the existing `node_state_changed` envelope in that file.

- [ ] **Step 2: Run it to verify it fails**

Expected: FAIL, `is not valid under any of the schemas listed in the 'oneOf' keyword`.

- [ ] **Step 3: Add the value and mirror the frozen copy**

Insert `"ghost"` after `"draft"` in `$defs/nodeState`, mirroring the Rust variant order. Leave `x-graphhelm-schema-version` at `1.0.0`. Copy the file byte-for-byte over `schemas/releases/1.0.0/event-envelope.schema.json`.

- [ ] **Step 4: Recompute the canonical digest**

There is no tooling for this. The algorithm in `core/schema-evolution/src/canonical.rs:67-98` is: sort object keys byte-wise, preserve array order, serialize compactly, SHA-256, lowercase hex, `sha256:` prefix.

```bash
python -c "
import json,hashlib,sys
d=json.load(open(sys.argv[1],encoding='utf-8'))
b=json.dumps(d,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode('utf-8')
print('sha256:'+hashlib.sha256(b).hexdigest())
" schemas/event-envelope.schema.json
```

Run it against the *unmodified* file first and confirm it reproduces the digest already stored in `schemas/catalog.json`. If it does not, stop; never commit a hash you could not verify. Then write the new digest into both catalogs and change nothing else in them.

- [ ] **Step 5: Verify**

```
cargo +1.97.1 run --locked -q -p graphhelm-cli -- schema check --baseline schemas/releases/1.0.0/catalog.json --candidate schemas/catalog.json
```

Expected: class `unchanged`, impact `none`, `release.ok` true — both sides moved together. An impact of `minor` means the frozen copy is stale.

- [ ] **Step 6: Update the prose that enumerates node states**

`schemas/CHANGELOG.md` (a plain `- ` bullet under `## [1.0.0]`; the prefix `- BREAKING ` is parsed as major-change evidence by `apps/cli/src/commands/schema/check.rs:202`), `MASTER_PRD.md`, and `docs/architecture/SYSTEM_ARCHITECTURE.md`.

- [ ] **Step 7: Commit**

```bash
git add schemas core/protocols/tests/persistence_wire.rs MASTER_PRD.md docs/architecture/SYSTEM_ARCHITECTURE.md
git commit -m "fix(schemas): close the ghost wire drift in event-envelope"
```

---

### Task 7: Guard the crate's purity and run the gate

**Files:**
- Create: `core/execution/tests/source_invariants.rs`

The M03 review found that a documented control which no test enforces is not a control. This makes the purity boundary enforceable.

- [ ] **Step 1: Write the failing test**

Create `core/execution/tests/source_invariants.rs`:

```rust
const MANIFEST: &str = include_str!("../Cargo.toml");
const LIB: &str = include_str!("../src/lib.rs");
const TRANSITION: &str = include_str!("../src/transition.rs");
const SIGNAL: &str = include_str!("../src/signal.rs");

/// This crate must stay pure. A clock, a random source or an adapter dependency would make replay
/// reproduce a different decision from the same history, which is exactly the defect class the
/// milestone-03 review had to correct twice.
#[test]
fn the_execution_crate_has_no_impure_dependency() {
    for forbidden in ["tokio", "sqlx", "chrono", "getrandom", "graphhelm-postgres", "graphhelm-events"] {
        assert!(
            !MANIFEST.contains(forbidden),
            "core/execution must not depend on {forbidden}"
        );
    }
}

#[test]
fn no_source_file_reads_a_clock_or_randomness() {
    for (name, source) in [("lib.rs", LIB), ("transition.rs", TRANSITION), ("signal.rs", SIGNAL)] {
        for forbidden in ["SystemTime", "Instant", "Utc::now", "rand", "thread_rng", "std::fs", "std::net"] {
            assert!(
                !source.contains(forbidden),
                "{name} must not reference {forbidden}"
            );
        }
    }
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo +1.97.1 test -p graphhelm-execution --test source_invariants`
Expected: PASS, 2 passed. If either fails, remove the offending dependency or call rather than relaxing the assertion.

- [ ] **Step 3: Run the full local gate**

Run: `./ci/gate.ps1 -SkipPostgres`
Expected: `[gate] GREEN - every stage passed.` This plan touches no SQL, so the PostgreSQL matrix is not required; say so when reporting the result.

- [ ] **Step 4: Commit**

```bash
git add core/execution/tests/source_invariants.rs
git commit -m "test(execution): enforce the purity boundary with a source invariant"
```

---

## Definition of done

- `core/execution` exists, is a workspace member, and depends only on `graphhelm-protocols`, `serde` and `serde_json`.
- `NodeState::Ghost` exists in the one shared vocabulary; no second execution state enum was created.
- `apply_transition` is total, deterministic, and proven so by property tests.
- A ghost node is unreachable from any running state at any counter value.
- Every bound is a counter; no wall-clock value influences a transition.
- An unrecognized signal kind is recorded and can never propose a mutation.
- `NodeState::Ghost` is representable on the wire: `$defs/nodeState` carries `"ghost"` and both catalog digests agree.
- `./ci/gate.ps1 -SkipPostgres` is green.

## What this plan deliberately excludes

Scheduling and the ready set (04c), durable projection of execution state (04b), signal-to-draft translation and ghost approval by the Governor (04d), pause/resume and recovery (04e), and the operator CLI (04f). This plan defines the contracts those depend on and nothing more.
