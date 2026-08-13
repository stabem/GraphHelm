# Milestone 04c - Scheduler and Effect-Free Executor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Decide *what runs next* and *when to stop trying*, so a published graph can be driven to completion by an injected executor whose only milestone-04 implementation is effect-free.

**Architecture:** Two pure functions added to `core/execution` — `ready_set` over a graph spec and observed node states, and `classify_progress` over the counters 04b derives — plus the first `NodeExecutor` implementation, supplied by `core/simulation` so that simulation becomes a consumer of the execution contract instead of a parallel one. Neither function schedules anything itself; both are total functions over values, so the same history always yields the same decision.

**Tech Stack:** Rust 1.97.1, edition 2024. `graphhelm-protocols` for `GraphSpec` and the vocabularies, `graphhelm-events` for `ExecutionProjection`, `proptest` for the scheduling properties. No `tokio`, no adapter, no clock.

**Design source:** `docs/superpowers/specs/2026-08-13-graph-engine-governor-design.md`, decisions 5.2 and 5.7. No-progress conditions from `docs/operations/OBSERVABILITY_AND_RECOVERY.md` §15.

---

## What this plan must get right

**Bounds block, they never truncate.** Decision 5.7 is explicit: exceeding a bound blocks the execution for an owner decision, and nothing silently drops work. A `ready_set` that returned its first 1024 entries would satisfy the type signature and violate the design.

**A ghost is never scheduled.** Decision 5.2 says "consumes no tokens" is enforced structurally by the scheduler, not by convention. That means `ready_set` excludes `NodeState::Ghost` by construction, and a property test proves it at every counter value.

**04a's purity invariant is currently wrong and this plan is where it gets fixed.** `core/execution/tests/source_invariants.rs` forbids `graphhelm-events`, `graphhelm-graph` and `graphhelm-policy` in the manifest. But the design's §6 says `core/execution` *depends on* protocols, graph, policy and events — the invariant's real purpose is to keep out adapters, clocks and randomness. Task 1 narrows it deliberately rather than deleting the entries that get in the way.

---

## File structure

| File | Responsibility |
|---|---|
| `core/execution/tests/source_invariants.rs` | The purity boundary, narrowed to what it actually protects |
| `core/execution/src/ready.rs` | `ready_set`: which nodes may be dispatched now |
| `core/execution/src/progress.rs` | `classify_progress`: whether a node may be attempted again |
| `core/simulation/src/executor.rs` | The effect-free `NodeExecutor` |
| `core/execution/tests/scheduling_properties.rs` | Determinism, ghost-safety, boundedness |

---

### Task 1: Narrow the purity invariant and take the events dependency

**Files:**
- Modify: `core/execution/tests/source_invariants.rs`
- Modify: `core/execution/Cargo.toml`

`GraphSpec` lives in `graphhelm-protocols`, so the ready set needs no graph dependency. Only `graphhelm-events` is required, for `ExecutionProjection`. Add nothing else — an unused dependency in a crate whose whole claim is a narrow boundary is worse than no dependency.

- [ ] **Step 1: Write the failing test**

Replace the body of `the_execution_crate_has_no_impure_dependency` in `core/execution/tests/source_invariants.rs`:

```rust
/// This crate must stay pure. A clock, a random source or an *adapter* dependency would make replay
/// reproduce a different decision from the same history, which is exactly the defect class the
/// milestone-03 review had to correct twice.
///
/// Depending on another `core` crate is not impurity and never was. The original list forbade
/// `graphhelm-events`, `graphhelm-graph` and `graphhelm-policy`, which the design explicitly says
/// this crate depends on — the invariant was over-broad, not the design.
#[test]
fn the_execution_crate_has_no_impure_dependency() {
    for forbidden in [
        "tokio",
        "sqlx",
        "chrono",
        "getrandom",
        "rand",
        "reqwest",
        "graphhelm-postgres",
        "graphhelm-sealed",
        "adapters/",
    ] {
        assert!(
            !MANIFEST.contains(forbidden),
            "core/execution must not depend on {forbidden}"
        );
    }
}

/// The narrowing above must not become a licence to depend on anything. This pins the exact set,
/// so adding a dependency is a deliberate edit to a test rather than a silent manifest change.
#[test]
fn the_execution_crate_depends_on_exactly_the_declared_crates() {
    let declared: Vec<&str> = MANIFEST
        .lines()
        .filter(|line| line.starts_with("graphhelm-"))
        .map(|line| line.split_whitespace().next().unwrap_or_default())
        .collect();
    assert_eq!(declared, ["graphhelm-protocols", "graphhelm-events"]);
}
```

- [ ] **Step 2: Run tests to verify the second one fails**

Run: `cargo +1.97.1 test -p graphhelm-execution --test source_invariants`
Expected: `the_execution_crate_depends_on_exactly_the_declared_crates` FAILS with `left: ["graphhelm-protocols"]`, because the dependency is not there yet. The first test should pass.

- [ ] **Step 3: Take the dependency**

In `core/execution/Cargo.toml`, under `[dependencies]`, after `graphhelm-protocols`:

```toml
graphhelm-events = { path = "../events" }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo +1.97.1 test -p graphhelm-execution --test source_invariants`
Expected: PASS.

Then confirm no cycle was created:

Run: `cargo +1.97.1 tree -p graphhelm-execution --depth 2`
Expected: `graphhelm-events` appears and cargo does not report a cyclic dependency. `core/events` depends on `protocols`, `graph`, `schema` and `policy`, never on `execution`, which is what keeps this legal.

- [ ] **Step 5: Commit**

```bash
git add core/execution/Cargo.toml core/execution/tests/source_invariants.rs Cargo.lock
git commit -m "refactor(execution): narrow the purity invariant to what it protects"
```

---

### Task 2: Ready-set computation

**Files:**
- Create: `core/execution/src/ready.rs`
- Modify: `core/execution/src/lib.rs`
- Test: `core/execution/src/ready.rs` (inline `mod tests`)

Readiness is fail-closed: **every** incoming edge is treated as a dependency, regardless of `EdgeType`. A node never runs before something it is connected downstream of. Refining that to per-edge-type semantics needs the condition evaluation that arrives with 04d, and guessing now would let a node run early.

- [ ] **Step 1: Write the failing test**

Create `core/execution/src/ready.rs` containing only the tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use graphhelm_protocols::{
        EdgeType, GraphEdge, GraphNode, NodeState, NodeType, Optionality,
    };
    use std::collections::BTreeMap;

    fn agent_node() -> GraphNode {
        GraphNode {
            node_type: NodeType::Agent,
            name: "n".to_owned(),
            objective: "o".to_owned(),
            optionality: Optionality::Required,
            properties: BTreeMap::new(),
        }
    }

    fn spec(nodes: &[&str], edges: &[(&str, &str)]) -> GraphSpec {
        let mut spec = GraphSpec {
            entrypoints: vec![nodes[0].to_owned()],
            nodes: BTreeMap::new(),
            edges: Vec::new(),
            budgets: Default::default(),
            policies: Vec::new(),
            completion: serde_json::Value::Null,
        };
        for node in nodes {
            spec.nodes.insert((*node).to_owned(), agent_node());
        }
        for (from, to) in edges {
            spec.edges.push(GraphEdge {
                id: format!("{from}-to-{to}"),
                from: (*from).to_owned(),
                to: (*to).to_owned(),
                edge_type: EdgeType::Control,
                payload_schema: None,
                condition: None,
                on_false: None,
                on_unknown: None,
                bindings: BTreeMap::new(),
                priority: None,
            });
        }
        spec
    }

    fn states(pairs: &[(&str, NodeState)]) -> BTreeMap<String, NodeState> {
        pairs
            .iter()
            .map(|(node, state)| ((*node).to_owned(), *state))
            .collect()
    }

    #[test]
    fn an_untouched_entrypoint_is_ready_and_its_successor_is_not() {
        let spec = spec(&["a", "b"], &[("a", "b")]);
        let ready = ready_set(&spec, &BTreeMap::new()).unwrap();
        assert_eq!(ready, ["a".to_owned()].into_iter().collect());
    }

    #[test]
    fn a_successor_becomes_ready_once_every_predecessor_is_satisfied() {
        let spec = spec(&["a", "b", "c"], &[("a", "c"), ("b", "c")]);
        let half = ready_set(&spec, &states(&[("a", NodeState::Succeeded)])).unwrap();
        assert!(!half.contains("c"), "c ran with b unfinished");

        let full = ready_set(
            &spec,
            &states(&[("a", NodeState::Succeeded), ("b", NodeState::Waived)]),
        )
        .unwrap();
        assert!(full.contains("c"));
    }

    /// A ghost is a proposal. Excluding it here is what makes "consumes no tokens" structural
    /// rather than a convention someone has to remember.
    #[test]
    fn a_ghost_is_never_ready_and_never_satisfies_a_dependent() {
        let spec = spec(&["a", "b"], &[("a", "b")]);
        let ready = ready_set(&spec, &states(&[("a", NodeState::Ghost)])).unwrap();
        assert!(ready.is_empty(), "a ghost or its dependent was scheduled");
    }

    /// A node already in flight, or finished, or awaiting an owner, is not dispatchable.
    #[test]
    fn only_untouched_and_ready_nodes_are_dispatchable() {
        let spec = spec(&["a"], &[]);
        for state in [
            NodeState::Queued,
            NodeState::Running,
            NodeState::Blocked,
            NodeState::Paused,
            NodeState::Succeeded,
            NodeState::Failed,
            NodeState::Cancelled,
        ] {
            let ready = ready_set(&spec, &states(&[("a", state)])).unwrap();
            assert!(ready.is_empty(), "{state:?} was dispatched");
        }
        assert!(
            ready_set(&spec, &states(&[("a", NodeState::Ready)]))
                .unwrap()
                .contains("a")
        );
    }

    /// Decision 5.7: exceeding a bound blocks for an owner decision. It never truncates, because a
    /// truncated ready set looks exactly like a smaller graph and loses work silently.
    #[test]
    fn an_oversized_ready_set_blocks_rather_than_truncating() {
        let names: Vec<String> = (0..=MAX_READY_SET).map(|index| format!("n{index}")).collect();
        let borrowed: Vec<&str> = names.iter().map(String::as_str).collect();
        let spec = spec(&borrowed, &[]);
        assert_eq!(
            ready_set(&spec, &BTreeMap::new()).unwrap_err(),
            ScheduleError::ReadySetTooLarge
        );
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo +1.97.1 test -p graphhelm-execution --lib ready`
Expected: FAIL, `cannot find function 'ready_set' in this scope`

- [ ] **Step 3: Write the implementation**

Prepend to `core/execution/src/ready.rs`:

```rust
//! Which nodes may be dispatched right now.
//!
//! A total function over a graph spec and the observed node states. It consults no clock and holds
//! no state of its own, so the same inputs always yield the same set — which is what lets a replayed
//! execution schedule identically to the original.

use std::collections::{BTreeMap, BTreeSet};

use graphhelm_protocols::{GraphSpec, NodeState};

use crate::bounds::MAX_READY_SET;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduleError {
    /// More nodes are ready at once than the execution may dispatch.
    ///
    /// This blocks for an owner decision. Returning a truncated set instead would silently drop
    /// work and look identical to a smaller graph.
    ReadySetTooLarge,
}

/// A predecessor no longer holds its dependent back.
///
/// `Waived` and `Skipped` count: an owner waiving an obligation or skipping a phase is exercising
/// the sovereignty D-019 grants, and the run must proceed. `Failed`, `Cancelled` and `Blocked`
/// deliberately do not — a dependent of a failed node stays unready until someone intervenes.
const fn satisfies_dependents(state: NodeState) -> bool {
    matches!(
        state,
        NodeState::Succeeded | NodeState::Waived | NodeState::Skipped
    )
}

/// A node in this state may be dispatched.
///
/// `Ghost` is absent by construction, which is how decision 5.2's "consumes no tokens" is enforced
/// structurally. Resuming a `Paused`, `WaitingInput` or `WaitingCapacity` node is 04e's business,
/// not the scheduler's.
const fn is_dispatchable(state: NodeState) -> bool {
    matches!(state, NodeState::Draft | NodeState::Ready)
}

/// Computes the set of nodes that may be dispatched now.
///
/// # Errors
/// Returns `ScheduleError::ReadySetTooLarge` when more than `MAX_READY_SET` nodes are ready.
pub fn ready_set(
    spec: &GraphSpec,
    states: &BTreeMap<String, NodeState>,
) -> Result<BTreeSet<String>, ScheduleError> {
    let mut predecessors: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for edge in &spec.edges {
        predecessors
            .entry(edge.to.as_str())
            .or_default()
            .push(edge.from.as_str());
    }

    let mut ready = BTreeSet::new();
    for node_id in spec.nodes.keys() {
        // An untouched node has no recorded state yet and behaves as a draft.
        let state = states.get(node_id).copied().unwrap_or(NodeState::Draft);
        if !is_dispatchable(state) {
            continue;
        }
        let satisfied = predecessors
            .get(node_id.as_str())
            .is_none_or(|sources| {
                sources.iter().all(|source| {
                    satisfies_dependents(
                        states
                            .get(*source)
                            .copied()
                            .unwrap_or(NodeState::Draft),
                    )
                })
            });
        if satisfied {
            ready.insert(node_id.clone());
            if ready.len() > MAX_READY_SET {
                return Err(ScheduleError::ReadySetTooLarge);
            }
        }
    }
    Ok(ready)
}
```

- [ ] **Step 4: Export it**

In `core/execution/src/lib.rs`, add `mod ready;` beside the other modules and:

```rust
pub use ready::{ScheduleError, ready_set};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo +1.97.1 test -p graphhelm-execution --lib ready`
Expected: PASS, 5 passed

- [ ] **Step 6: Prove the bound test can fail**

Temporarily change the bound check to `>= MAX_READY_SET * 2` and confirm `an_oversized_ready_set_blocks_rather_than_truncating` fails. Revert completely and re-confirm. Quote both outputs. Do not commit the sabotage.

- [ ] **Step 7: Commit**

```bash
git add core/execution/src/ready.rs core/execution/src/lib.rs
git commit -m "feat(execution): add ready-set computation"
```

---

### Task 3: No-progress classification

**Files:**
- Create: `core/execution/src/progress.rs`
- Modify: `core/execution/src/lib.rs`
- Test: `core/execution/src/progress.rs` (inline `mod tests`)

`OBSERVABILITY_AND_RECOVERY.md` §15 lists seven no-progress conditions. Only two are decidable from what 04b records: *retries with no change* and *semantically identical outputs*. The other five — recurring remediation loops, alternating graph mutations, agent delegation chains, repeated tool failure, and budget consumed without evidence gain — need signal intake (04d) or real tool calls (Milestone 05). This task implements the two that are decidable and names the five that are not, rather than pretending to detect them.

- [ ] **Step 1: Write the failing test**

Create `core/execution/src/progress.rs` containing only the tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use graphhelm_events::ExecutionProjection;
    use graphhelm_protocols::NodeOutcome;

    fn projection(attempts: u32, run: u32, last: NodeOutcome) -> ExecutionProjection {
        let mut projection = ExecutionProjection::default();
        projection.node_attempts.insert("start".to_owned(), attempts);
        projection.last_outcome.insert("start".to_owned(), last);
        projection.identical_outcomes.insert("start".to_owned(), run);
        projection
    }

    #[test]
    fn a_node_with_attempts_left_may_continue() {
        let projection = projection(1, 1, NodeOutcome::RetryableFailure);
        assert_eq!(
            classify_progress(&projection, "start", NodeOutcome::RetryableFailure),
            Progress::Continue
        );
    }

    /// Exhausting attempts blocks for an owner decision. It never silently gives up, and it never
    /// consults a clock.
    #[test]
    fn exhausted_attempts_block() {
        let projection = projection(MAX_NODE_ATTEMPTS, 1, NodeOutcome::RetryableFailure);
        assert_eq!(
            classify_progress(&projection, "start", NodeOutcome::RetryableFailure),
            Progress::AttemptsExhausted
        );
    }

    #[test]
    fn a_repeating_outcome_blocks_as_no_progress() {
        let projection = projection(1, MAX_IDENTICAL_OUTCOMES, NodeOutcome::RetryableFailure);
        assert_eq!(
            classify_progress(&projection, "start", NodeOutcome::RetryableFailure),
            Progress::NoProgress
        );
    }

    /// The run length belongs to the outcome that produced it. A node whose *previous* run reached
    /// the bound with some other outcome has made no repeated failure of this kind, and blocking it
    /// on its first would be wrong.
    #[test]
    fn a_run_of_a_different_outcome_does_not_block_this_one() {
        let projection = projection(1, MAX_IDENTICAL_OUTCOMES, NodeOutcome::NeedsCapacity);
        assert_eq!(
            classify_progress(&projection, "start", NodeOutcome::RetryableFailure),
            Progress::Continue
        );
    }

    #[test]
    fn an_unknown_node_may_continue() {
        assert_eq!(
            classify_progress(&ExecutionProjection::default(), "absent", NodeOutcome::Started),
            Progress::Continue
        );
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo +1.97.1 test -p graphhelm-execution --lib progress`
Expected: FAIL, `cannot find function 'classify_progress' in this scope`

- [ ] **Step 3: Write the implementation**

Prepend to `core/execution/src/progress.rs`:

```rust
//! Whether a node may be attempted again.
//!
//! Every threshold here is a count, never a duration, so replaying the same history reaches the same
//! verdict on a slower machine. The counters themselves are derived by the projection in 04b; this
//! module only judges them.
//!
//! `OBSERVABILITY_AND_RECOVERY.md` §15 names seven no-progress conditions. Two are decidable from
//! recorded history and are implemented here: retries with no change, and semantically identical
//! outcomes. The remaining five — recurring remediation loops, alternating graph mutations, agent
//! delegation chains, repeated tool failure, and budget consumed without evidence gain — need signal
//! intake (04d) or real tool calls (Milestone 05). None of them is silently approximated.

use graphhelm_events::ExecutionProjection;
use graphhelm_protocols::NodeOutcome;

use crate::bounds::{MAX_IDENTICAL_OUTCOMES, MAX_NODE_ATTEMPTS};

/// The scheduler's verdict for one node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    /// The node may be dispatched again.
    Continue,
    /// Attempts are spent. Block for an owner decision rather than failing the node, so no work is
    /// discarded without a record.
    AttemptsExhausted,
    /// The same outcome keeps recurring. Block for an owner decision.
    NoProgress,
}

/// Classifies whether `node` may be attempted again, given the outcome about to be reported.
#[must_use]
pub fn classify_progress(
    projection: &ExecutionProjection,
    node: &str,
    outcome: NodeOutcome,
) -> Progress {
    if projection.node_attempts.get(node).copied().unwrap_or(0) >= MAX_NODE_ATTEMPTS {
        return Progress::AttemptsExhausted;
    }
    // `identical_outcomes_for` returns 0 when the last recorded outcome differs, which is why this
    // reads through it rather than the raw map: a run belonging to some other outcome must not
    // block this one on its first occurrence.
    if projection.identical_outcomes_for(node, outcome) >= MAX_IDENTICAL_OUTCOMES {
        return Progress::NoProgress;
    }
    Progress::Continue
}
```

- [ ] **Step 4: Export it**

In `core/execution/src/lib.rs`, add `mod progress;` and:

```rust
pub use progress::{Progress, classify_progress};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo +1.97.1 test -p graphhelm-execution --lib progress`
Expected: PASS, 5 passed

- [ ] **Step 6: Prove the outcome-pairing test can fail**

Change `identical_outcomes_for(node, outcome)` to read `projection.identical_outcomes.get(node).copied().unwrap_or(0)` and confirm `a_run_of_a_different_outcome_does_not_block_this_one` fails. Revert and re-confirm. Quote both outputs. This is the exact defect the accessor exists to prevent, so seeing it fail matters.

- [ ] **Step 7: Commit**

```bash
git add core/execution/src/progress.rs core/execution/src/lib.rs
git commit -m "feat(execution): classify retry exhaustion and no-progress"
```

---

### Task 4: The effect-free executor

**Files:**
- Create: `core/simulation/src/executor.rs`
- Modify: `core/simulation/src/lib.rs`
- Modify: `core/simulation/Cargo.toml`
- Test: `core/simulation/src/executor.rs` (inline `mod tests`)

This is the seam Milestone 05 replaces. Making `core/simulation` implement `NodeExecutor` is what turns simulation into a consumer of the execution contract rather than a second implementation of it.

- [ ] **Step 1: Write the failing test**

Create `core/simulation/src/executor.rs` containing only the tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use graphhelm_execution::NodeExecutor;
    use graphhelm_protocols::{FixtureOutcome, NodeOutcome};

    fn executor(pairs: &[(&str, FixtureOutcome)]) -> FixtureExecutor {
        let mut fixtures = SimulationFixtures::default();
        for (node, outcome) in pairs {
            fixtures
                .node_outcomes
                .insert((*node).to_owned(), *outcome);
        }
        FixtureExecutor::new(fixtures)
    }

    #[test]
    fn a_fixture_outcome_maps_to_a_node_outcome() {
        let executor = executor(&[
            ("ok", FixtureOutcome::Success),
            ("bad", FixtureOutcome::Failure),
        ]);
        assert_eq!(executor.execute("ok", 0).unwrap(), NodeOutcome::Succeeded);
        assert_eq!(
            executor.execute("bad", 0).unwrap(),
            NodeOutcome::RetryableFailure
        );
    }

    /// A node with no fixture has not been told what to do. It waits for input rather than being
    /// invented as a success, because inventing one would make a simulation pass for a node nobody
    /// specified.
    #[test]
    fn an_unspecified_node_waits_for_input() {
        let executor = executor(&[]);
        assert_eq!(
            executor.execute("absent", 0).unwrap(),
            NodeOutcome::NeedsInput
        );
        assert_eq!(
            executor(&[("u", FixtureOutcome::Unknown)])
                .execute("u", 0)
                .unwrap(),
            NodeOutcome::NeedsInput
        );
    }

    /// The executor is effect-free and its answer cannot depend on the attempt number, or replaying
    /// an execution would diverge from the original run.
    #[test]
    fn the_outcome_does_not_depend_on_the_attempt() {
        let executor = executor(&[("bad", FixtureOutcome::Failure)]);
        let first = executor.execute("bad", 0).unwrap();
        for attempt in [1, 7, u32::MAX] {
            assert_eq!(executor.execute("bad", attempt).unwrap(), first);
        }
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo +1.97.1 test -p graphhelm-simulation --lib executor`
Expected: FAIL, `failed to resolve: use of undeclared crate or module 'graphhelm_execution'`

- [ ] **Step 3: Take the dependency**

In `core/simulation/Cargo.toml`, under `[dependencies]`:

```toml
graphhelm-execution = { path = "../execution" }
```

`core/execution` depends on `protocols` and `events`; `core/simulation` depending on `execution` therefore creates no cycle. Confirm with `cargo +1.97.1 tree -p graphhelm-simulation --depth 2` and report if cargo disagrees.

- [ ] **Step 4: Write the implementation**

Prepend to `core/simulation/src/executor.rs`:

```rust
//! The effect-free `NodeExecutor`.
//!
//! This is the only milestone-04 implementation of the seam: it consults a fixture table and
//! nothing else — no model, no tool, no sandbox, no network. Milestone 05 supplies one that does
//! real work behind the same contract, which is what keeps every property in this milestone
//! testable offline.

use graphhelm_execution::{ExecutionError, NodeExecutor};
use graphhelm_protocols::{FixtureOutcome, NodeOutcome};

use crate::fixtures::SimulationFixtures;

/// Answers from a fixture table. Deterministic and side-effect free.
#[derive(Clone, Debug, Default)]
pub struct FixtureExecutor {
    fixtures: SimulationFixtures,
}

impl FixtureExecutor {
    #[must_use]
    pub const fn new(fixtures: SimulationFixtures) -> Self {
        Self { fixtures }
    }
}

impl NodeExecutor for FixtureExecutor {
    fn execute(&self, node_id: &str, _attempt: u32) -> Result<NodeOutcome, ExecutionError> {
        // The attempt number is deliberately unused. An executor whose answer changed with the
        // attempt would make a replay diverge from the run it replays.
        Ok(match self.fixtures.node_outcomes.get(node_id) {
            Some(FixtureOutcome::Success) => NodeOutcome::Succeeded,
            Some(FixtureOutcome::Failure) => NodeOutcome::RetryableFailure,
            // An absent fixture and an explicitly unknown one mean the same thing: nobody said what
            // this node does. Waiting is honest; inventing a success is not.
            Some(FixtureOutcome::Unknown) | None => NodeOutcome::NeedsInput,
        })
    }
}
```

- [ ] **Step 5: Export it**

In `core/simulation/src/lib.rs`, add `mod executor;` and `pub use executor::FixtureExecutor;`, matching how that file exposes its other modules.

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo +1.97.1 test -p graphhelm-simulation --lib executor`
Expected: PASS, 3 passed

- [ ] **Step 7: Commit**

```bash
git add core/simulation/src/executor.rs core/simulation/src/lib.rs core/simulation/Cargo.toml Cargo.lock
git commit -m "feat(simulation): supply the effect-free node executor"
```

---

### Task 5: Reconcile the projection's resource guard with the design's bounds

**Files:**
- Modify: `core/events/src/projection.rs`
- Test: `core/execution/src/ready.rs` (inline `mod tests`)

The 04b review found that `MAX_PROJECTION_NODES` returns `LimitExceeded`, which makes a projection permanently unrebuildable — while design 5.7 says exceeding a bound *blocks for an owner decision* and never truncates. Both are defensible individually and contradictory together.

The resolution is that they are different kinds of limit. `MAX_READY_SET` is a **domain bound**: a real execution can reach it, and reaching it must block. `MAX_PROJECTION_NODES` is a **resource guard** against a hostile or corrupt history growing a map without limit; a legitimate execution must never reach it. That is only true if the guard sits comfortably above every domain bound, and nothing currently checks that.

- [ ] **Step 1: Write the failing test**

Add to `core/execution/src/ready.rs`'s test module:

```rust
    /// A resource guard and a domain bound are different things. `MAX_READY_SET` is reachable by a
    /// legitimate execution and must block for an owner. The projection's node-map guard exists
    /// only to stop a corrupt history exhausting memory, so a legitimate execution must never reach
    /// it — which is only true while it stays well above every domain bound.
    #[test]
    fn the_projection_resource_guard_sits_above_every_domain_bound() {
        assert!(
            graphhelm_events::MAX_PROJECTION_NODES > MAX_READY_SET,
            "a legitimate execution can reach the projection guard"
        );
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo +1.97.1 test -p graphhelm-execution --lib the_projection_resource_guard`
Expected: FAIL, `MAX_PROJECTION_NODES` is private to `graphhelm_events`.

- [ ] **Step 3: Publish the guard and say what it is**

In `core/events/src/projection.rs`, change the constant to public and replace its comment:

```rust
/// Resource guard on the projection's per-node maps.
///
/// This is **not** one of decision 5.7's bounds. Those are domain limits a real execution can
/// reach, and reaching one blocks for an owner decision. This one exists so a corrupt or hostile
/// history cannot grow the maps without limit before the projection's size check can reject it, and
/// a legitimate execution must never reach it — which is why it sits an order of magnitude above
/// `MAX_READY_SET`. `graphhelm_execution` pins that relationship in a test.
pub const MAX_PROJECTION_NODES: usize = 10_000;
```

Re-export it from `core/events/src/lib.rs` alongside the other projection items.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo +1.97.1 test -p graphhelm-execution --lib`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add core/events/src/projection.rs core/events/src/lib.rs core/execution/src/ready.rs
git commit -m "fix(events): distinguish the projection resource guard from a design bound"
```

---

### Task 6: Scheduling properties

**Files:**
- Create: `core/execution/tests/scheduling_properties.rs`

- [ ] **Step 1: Write the failing test**

Create `core/execution/tests/scheduling_properties.rs`:

```rust
use std::collections::BTreeMap;

use graphhelm_execution::{ScheduleError, ready_set};
use graphhelm_protocols::{
    EdgeType, GraphEdge, GraphNode, GraphSpec, NodeState, NodeType, Optionality,
};
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

fn agent_node() -> GraphNode {
    GraphNode {
        node_type: NodeType::Agent,
        name: "n".to_owned(),
        objective: "o".to_owned(),
        optionality: Optionality::Required,
        properties: BTreeMap::new(),
    }
}

/// A chain of eight nodes, each depending on the one before it.
fn chain() -> GraphSpec {
    let mut spec = GraphSpec {
        entrypoints: vec!["n0".to_owned()],
        nodes: BTreeMap::new(),
        edges: Vec::new(),
        budgets: Default::default(),
        policies: Vec::new(),
        completion: serde_json::Value::Null,
    };
    for index in 0..8 {
        spec.nodes.insert(format!("n{index}"), agent_node());
        if index > 0 {
            spec.edges.push(GraphEdge {
                id: format!("e{index}"),
                from: format!("n{}", index - 1),
                to: format!("n{index}"),
                edge_type: EdgeType::Control,
                payload_schema: None,
                condition: None,
                on_false: None,
                on_unknown: None,
                bindings: BTreeMap::new(),
                priority: None,
            });
        }
    }
    spec
}

fn assignment(seeds: Vec<usize>) -> BTreeMap<String, NodeState> {
    seeds
        .into_iter()
        .enumerate()
        .map(|(index, seed)| (format!("n{index}"), STATES[seed % STATES.len()]))
        .collect()
}

proptest! {
    /// The same graph and the same states always yield the same set. Any ordering or iteration
    /// dependence here would make a replayed execution schedule differently from the original.
    #[test]
    fn scheduling_is_deterministic(seeds in prop::collection::vec(0usize..64, 0..8)) {
        let spec = chain();
        let states = assignment(seeds);
        prop_assert_eq!(ready_set(&spec, &states), ready_set(&spec, &states));
    }

    /// A ghost is never dispatched, at any assignment of every other node's state.
    #[test]
    fn a_ghost_is_never_scheduled(seeds in prop::collection::vec(0usize..64, 0..8)) {
        let spec = chain();
        let mut states = assignment(seeds);
        states.insert("n3".to_owned(), NodeState::Ghost);
        if let Ok(ready) = ready_set(&spec, &states) {
            prop_assert!(!ready.contains("n3"), "a ghost was scheduled");
        }
    }

    /// Never more than the bound, and never a truncated success.
    #[test]
    fn the_ready_set_is_bounded_or_blocks(seeds in prop::collection::vec(0usize..64, 0..8)) {
        let spec = chain();
        match ready_set(&spec, &assignment(seeds)) {
            Ok(ready) => prop_assert!(ready.len() <= graphhelm_execution::MAX_READY_SET),
            Err(ScheduleError::ReadySetTooLarge) => {}
        }
    }

    /// A node is only ever ready when every predecessor is satisfied. This is the safety property:
    /// nothing runs before what it depends on.
    #[test]
    fn nothing_is_ready_before_its_predecessor(seeds in prop::collection::vec(0usize..64, 0..8)) {
        let spec = chain();
        let states = assignment(seeds);
        if let Ok(ready) = ready_set(&spec, &states) {
            for node in &ready {
                let index: usize = node.trim_start_matches('n').parse().unwrap();
                if index > 0 {
                    let predecessor = states
                        .get(&format!("n{}", index - 1))
                        .copied()
                        .unwrap_or(NodeState::Draft);
                    prop_assert!(
                        matches!(
                            predecessor,
                            NodeState::Succeeded | NodeState::Waived | NodeState::Skipped
                        ),
                        "{node} was ready with predecessor {predecessor:?}"
                    );
                }
            }
        }
    }
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo +1.97.1 test -p graphhelm-execution --test scheduling_properties`
Expected: PASS, 4 passed. They should pass immediately, since Task 2 is already implemented.

- [ ] **Step 3: Prove them able to fail**

Temporarily change `is_dispatchable` to include `NodeState::Ghost` and confirm `a_ghost_is_never_scheduled` fails. Then revert, temporarily make `satisfies_dependents` also accept `NodeState::Failed`, and confirm `nothing_is_ready_before_its_predecessor` fails. Revert completely and re-confirm all four pass. Quote every output. Do not commit either sabotage.

A property that has never been observed failing proves nothing; milestone 03's review found that defect twice and milestone 04b found it again in a test asserting against a bare `{}`.

- [ ] **Step 4: Commit**

```bash
git add core/execution/tests/scheduling_properties.rs
git commit -m "test(execution): prove scheduling is deterministic, bounded and ghost-safe"
```

---

### Task 7: Documentation and the full gate

**Files:**
- Modify: `docs/milestones/graph-engine-governor.md`
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Record what exists now**

Extend `docs/milestones/graph-engine-governor.md` with a 04c section. Describe only what the tests demonstrate: ready-set computation with fail-closed dependencies, the ghost exclusion, the blocking bound, the two decidable no-progress conditions, and the effect-free executor.

State plainly that five of `OBSERVABILITY_AND_RECOVERY.md` §15's seven conditions are **not** detected, and why: they need signal intake (04d) or real tool calls (Milestone 05). Do not describe them as future work in a way that implies partial coverage today.

Also record the resource-guard-versus-domain-bound distinction from Task 5, since it resolves a contradiction the 04b review raised.

Documentation is always in English in this repository.

- [ ] **Step 2: Update the changelog**

Add a 04c entry to `CHANGELOG.md` in the voice of the existing entries.

- [ ] **Step 3: Run the full gate**

Run: `./ci/gate.ps1`
Expected: `[gate] GREEN - every stage passed.`

This plan changes no SQL and no schema, but it does change crate dependencies, so run the whole gate rather than `-SkipPostgres`. It needs `GRAPHHELM_PG_BIN` pointing at a PostgreSQL 16+ `bin` directory.

- [ ] **Step 4: Commit**

```bash
git add docs CHANGELOG.md
git commit -m "docs(m04): record the scheduler and the effect-free executor"
```

---

## Definition of done

- `core/execution` depends on `graphhelm-protocols` and `graphhelm-events` and nothing else, pinned by a test.
- The purity invariant forbids adapters, clocks and randomness — not sibling core crates.
- `ready_set` is total, deterministic, excludes ghosts by construction, and blocks rather than truncating at `MAX_READY_SET`.
- Nothing is ready before every one of its predecessors is `Succeeded`, `Waived` or `Skipped`.
- `classify_progress` reads the run length through `identical_outcomes_for`, so a run of a different outcome cannot block a node on its first failure.
- `core/simulation` implements `NodeExecutor`, and its answer does not depend on the attempt number.
- The projection's resource guard is documented as such and proven to sit above every domain bound.
- Every new property has been observed failing at least once, deliberately, and the sabotage reverted.
- `./ci/gate.ps1` is green, PostgreSQL matrix included.

## What this plan deliberately excludes

Actually dispatching work, and appending the resulting events — the scheduler decides, and 04d/04e drive. Signal intake, ghost approval, mutation publication and owner override (04d). Pause, resume, cancel and crash recovery (04e), which is why `Paused`, `WaitingInput` and `WaitingCapacity` are not dispatchable here. The operator CLI (04f).

It also excludes five of the seven no-progress conditions, named in Task 7, and any per-`EdgeType` refinement of readiness: every incoming edge gates, which is fail-closed and can only be relaxed once 04d evaluates edge conditions.
