# Milestone 04e - Pause, Resume, Cancel and Recovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give an execution the owner-sovereign lifecycle D-019 promises — pause, resume, cancel — and make an interrupted execution recover to a defined state that never resumes a node whose effects are unknown, with a full completed run proven to replay identically.

**Architecture:** Two new event kinds (`execution_paused`, `execution_resumed`) and three vocabulary widenings (`NodeOutcome::Paused`, `NodeOutcome::Interrupted`, `SimulationStatus::Cancelled`) carry the lifecycle onto the wire; cancel needs no new kind because §13 defines it as a *final status with partial effects recorded*, which `execution_completed` already carries. Three transition arms close gaps named since 04a: `Blocked` gets its owner resume path, `Paused` gets producers, and `Running` gets the crash exit. Recovery and resume-preconditions are pure functions, and a test-only driver composes every pure piece shipped since 04a into a full lifecycle — pause, resume, crash, recover, complete, replay — without shipping the production driver, which stays 04f.

**Tech Stack:** Rust 1.97.1, edition 2024. No new crate, no new dependency. PostgreSQL 16+ for the adapter matrix.

**Design source:** `docs/superpowers/specs/2026-08-13-graph-engine-governor-design.md` §7 (04e) and §8; `docs/operations/OBSERVABILITY_AND_RECOVERY.md` §11.2, §11.4, §12, §13; decisions D-019, D-020, 5.7.

---

## Decisions this plan enacts, stated up front

**A crash is not an outcome the executor reported, so it gets its own word.** `NodeOutcome::Interrupted` records that a node was `Running` when the execution stopped and its effects are unknown. `(Running, Interrupted) -> Blocked`: the node blocks for an owner decision, which is precisely the design's acceptance criterion — *never resume a node whose effects are unknown*. Mapping a crash onto `RetryableFailure` would silently authorize a retry nobody judged safe.

**Cancel is a final status, not a new event kind.** §13: cancel does not erase history and the final status records partial effects. `execution_completed { status: "cancelled" }` says exactly that; `SimulationStatus` gains `Cancelled` and the schema's `simulationStatus` enum widens in place per D-037. Node-level cancellation already exists (`NodeOutcome::Cancelled`, owner-sovereign from any non-terminal state).

**Pause holds work that has not started; it does not interrupt work in flight.** In the effect-free milestone a `Running` node completes instantly, so graceful pause (§12) is the only honest kind: `(Ready | Queued, Paused) -> Paused`. Immediate-stop and sandbox-kill semantics need a real runtime and are Milestone 05's; branch pause needs descendant computation and is deferred with them. A ghost cannot be paused — the ghost arm precedes the pause arm and still rejects everything but approval.

**`Blocked` resumes only through the owner.** `(Blocked, Approved) -> Ready`. Waive, skip and cancel already existed; what was missing since 04a was the owner saying "try again". Approval makes the node dispatchable on the next scheduler pass — it does not start it, which keeps D-020's spirit: nothing auto-starts out of a manual intervention; the scheduler's normal cycle does.

**The checkpoint is the projection generation, and this plan says so rather than inventing a second one.** §11.2 lists eleven checkpoint content items. The ones that exist in this milestone — graph version, node states, attempts, mode, counters, watermark — are exactly `ProjectionGeneration`, already durable, already atomically swapped, already rebuilt-equals-replay since 04b. The rest (leases, sandbox snapshots, tool/model session refs, pending timers, compensation state) name things Milestone 05 introduces; `resume_preconditions` validates the decidable subset and names the rest as not validated, the same honesty rule §15's no-progress conditions got in 04c.

**Resume preconditions are a pure gate.** §11.4's decidable subset: the execution exists, is `Paused`, the graph version to resume against matches the projection's, and no node is `Running` (a paused execution with a running node is a contradiction; an interrupted one must recover first). Lease renewal, route health, sandbox recreation and session invalidation are Milestone 05 items, named as such in the rustdoc.

## Hard-won process rules, binding on every task

- **Verify the digest method against the unmodified file before trusting it**; mirror the frozen schema copy byte-for-byte; recompute both catalog digests. `catalog_integrity.rs` compares raw bytes; never edit a release-gate test.
- **Revert sabotage from a `cp` backup, never `git checkout --`.**
- **Run the workspace clippy** (`cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings`); a per-crate pass proves nothing.
- **Write documentation from the code as built.** Four milestones running shipped a doc claim no code delivered; the last one was a stale bullet contradicting a section forty lines above it. After writing, re-read the whole file for contradictions, not just the new section.
- **Every new guard must be observed failing once, deliberately**, and the sabotage reverted.
- **The code wins over this plan.** Report every discrepancy rather than improvising.

---

## File structure

| File | Responsibility |
|---|---|
| `core/protocols/src/simulation.rs` | `NodeOutcome::{Paused, Interrupted}`, `SimulationStatus::Cancelled` |
| `core/protocols/src/event.rs` | `ExecutionPaused`, `ExecutionResumed` payloads and variants |
| `schemas/event-envelope.schema.json` | Widened enums, two new kinds (both copies, both digests) |
| `core/execution/src/transition.rs` | The three new arms |
| `core/events/src/projection.rs` | Pause/resume fold with status guards |
| `core/execution/src/recovery.rs` | `recovery_plan`, `resume_preconditions` — pure |
| `core/execution/tests/execution_lifecycle.rs` | The composed lifecycle: pause, crash, recover, complete, replay |

---

### Task 1: Widen the vocabularies

**Files:**
- Modify: `core/protocols/src/simulation.rs`
- Test: `core/protocols/tests/wire_roundtrip.rs`

- [ ] **Step 1: Write the failing tests**

In `wire_roundtrip.rs`'s normative wire-name test, extend the existing `outcomes` array with:

```rust
        (NodeOutcome::Paused, "paused"),
        (NodeOutcome::Interrupted, "interrupted"),
```

and the `SimulationStatus` loop with `(SimulationStatus::Cancelled, "cancelled")`.

- [ ] **Step 2: Run to verify they fail** — `no variant named 'Paused' found` (there is a `NodeState::Paused`; the error will name `NodeOutcome`). Quote it.

- [ ] **Step 3: Add the variants**

To `NodeOutcome`, after `Cancelled` and before `Invalidated` would split the owner-action group — instead append both **after `Invalidated`**, at the end:

```rust
    /// The owner paused work that had not started. Only `Ready` and `Queued` nodes pause; a
    /// running node in this milestone completes instantly, and interrupting real work is
    /// Milestone 05's problem.
    Paused,
    /// The execution stopped while this node was running, so its effects are unknown. The only
    /// legal consequence is `Blocked`: nothing may resume a node whose effects are unknown.
    Interrupted,
```

To `SimulationStatus`, after `Blocked`:

```rust
    /// Cancelled by the owner. History is not erased and partial effects are recorded, per
    /// `OBSERVABILITY_AND_RECOVERY.md` §13.
    Cancelled,
```

Appending at the end is deliberate both times: these enums have no positional encoding, and end-append keeps the wire names of every existing variant untouched by construction.

- [ ] **Step 4: Run to verify they pass.** Then run `cargo +1.97.1 check --workspace --all-targets` and list every site the widening breaks. Expected: `core/execution/tests/transition_properties.rs` (`OUTCOMES: [NodeOutcome; 11]` becomes 13) and any exhaustive `SimulationStatus` match. Fix the property array here — it is an inventory, and widening it is this task's point — but for any *behavioural* site (a match arm deciding something), add the arm that preserves existing behaviour and report it.

- [ ] **Step 5: Commit**

```bash
git add core/protocols core/execution
git commit -m "feat(protocols): add the pause, interruption and cancellation vocabulary"
```

---

### Task 2: The two lifecycle event kinds

**Files:**
- Modify: `core/protocols/src/event.rs`
- Test: `core/protocols/tests/persistence_wire.rs`

- [ ] **Step 1: Write the failing test** — round-trip cases modelled on `governance_event_kinds_round_trip_with_exact_wire_names`:

```rust
        ("execution_paused", json!({"executionId":"execution-1"})),
        ("execution_resumed", json!({"executionId":"execution-1"})),
```

- [ ] **Step 2: Run to verify it fails** — `unknown variant 'execution_paused'`. Quote it.

- [ ] **Step 3: Add the payloads and variants**

```rust
/// The owner paused the execution, per D-019 and §12's graceful pause: nodes not yet started are
/// held; nothing in flight is interrupted in this milestone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionPaused {
    pub execution_id: OpaqueId,
}

/// The owner resumed a paused execution. The resume preconditions in `graphhelm_execution` gate
/// whether this may be appended; the fold only checks it is coherent history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionResumed {
    pub execution_id: OpaqueId,
}
```

Variants after `MutationAccepted`; the `EventKind` doc comment's count becomes 25.

- [ ] **Step 4: Run to verify it passes**, then `cargo +1.97.1 check --workspace --all-targets` — expect the one `E0004` at `apply_projection_event`; the fold is Task 4. Report any other site.

- [ ] **Step 5: Commit**

```bash
git add core/protocols
git commit -m "feat(protocols): add the pause and resume event kinds"
```

---

### Task 3: The wire contract

**Files:**
- Modify: `schemas/event-envelope.schema.json`, frozen copy, both catalogs, `schemas/CHANGELOG.md`
- Test: `core/protocols/tests/persistence_wire.rs`, `core/schema-evolution/tests/conformance.rs`

The procedure is exactly Task 2 of the 04d plan, which is exactly Task 4 of the 04b plan — it has now run three times without a defect. Differences only:

- Widen `$defs/nodeOutcome` with `"paused"`, `"interrupted"` (appended, matching the Rust order) and `$defs/simulationStatus` with `"cancelled"`.
- Add `$defs/executionPaused` and `$defs/executionResumed` (one required `executionId`, `additionalProperties: false`), two `oneOf` branches, two `scopeWithExecution` rules.
- Fixture inventories grow 23 → 25 in `persistence_wire.rs` and `conformance.rs`; rename the `all_twenty_three_*` tests to `all_twenty_five_*`.
- The failing envelope-validation test comes first, is observed failing (`oneOf` rejection), and the digest method is verified against the unmodified file before the new digest is written into **both** catalogs.

Because `graphhelm-cli` will not compile until Task 4 closes the fold, verify the release gate through the scratch `compare_catalogs`/`enforce_release` test (delete it before committing), expecting class `unchanged`, impact `none`, `release.ok` true.

- [ ] **Step 1: Failing test, observed**
- [ ] **Step 2: Schema edit, mirror, digests**
- [ ] **Step 3: Fixture inventories and rename**
- [ ] **Step 4: Changelog bullet (never `- BREAKING `)**
- [ ] **Step 5: Verify (protocols, schema-evolution, scratch release gate, fmt), commit**

```bash
git add schemas core/protocols/tests/persistence_wire.rs core/schema-evolution/tests/conformance.rs
git commit -m "feat(schemas): add the lifecycle vocabulary to the envelope contract"
```

---

### Task 4: Fold pause and resume with status guards

**Files:**
- Modify: `core/events/src/projection.rs`
- Test: `core/events/tests/execution_projection.rs`

- [ ] **Step 1: Write the failing tests**

```rust
/// Pausing sets the aggregate status; resuming restores it. Both are coherent-history guards,
/// not judgments — the resume preconditions live in graphhelm_execution.
#[test]
fn pause_and_resume_fold_into_the_aggregate_status() {
    let projection = replay(&scope(), STREAM, &paused_then_resumed()).unwrap();
    assert_eq!(projection.simulation_status, Some(SimulationStatus::Running));

    let paused = replay(&scope(), STREAM, &started_then_paused()).unwrap();
    assert_eq!(paused.simulation_status, Some(SimulationStatus::Paused));
}

/// Pausing an execution that is not running, or resuming one that is not paused, is history that
/// cannot have happened.
#[test]
fn an_incoherent_pause_or_resume_is_corrupt() {
    assert_eq!(
        replay(&scope(), STREAM, &paused_twice()).unwrap_err(),
        ReplayError::Corrupt
    );
    assert_eq!(
        replay(&scope(), STREAM, &resumed_without_pause()).unwrap_err(),
        ReplayError::Corrupt
    );
}
```

Fixture helpers follow the file's existing pattern. `execution_started` folds `simulation_status` as... **check what the `ExecutionStarted` arm actually sets**. If it leaves `simulation_status` `None` until completion, the pause guard below must accept `None | Some(Running)` — decide from the code, not this plan, and say which it was.

- [ ] **Step 2: Add the arms** (one fold; after `MutationAccepted`):

```rust
        EventKind::ExecutionPaused(payload) => {
            if projection.execution_id.as_deref() != Some(payload.execution_id.as_str())
                || !matches!(
                    projection.simulation_status,
                    None | Some(SimulationStatus::Running)
                )
            {
                return Err(ReplayError::Corrupt);
            }
            projection.simulation_status = Some(SimulationStatus::Paused);
        }
        EventKind::ExecutionResumed(payload) => {
            if projection.execution_id.as_deref() != Some(payload.execution_id.as_str())
                || projection.simulation_status != Some(SimulationStatus::Paused)
            {
                return Err(ReplayError::Corrupt);
            }
            projection.simulation_status = Some(SimulationStatus::Running);
        }
```

Adjust the pause guard per the Step 1 finding, and mirror the reasoning in a comment.

- [ ] **Step 3: Run, then sabotage** — drop the resume guard, confirm `an_incoherent_pause_or_resume_is_corrupt` fails, revert from a backup, re-confirm. Quote all outputs.

- [ ] **Step 4: Commit**

```bash
git add core/events
git commit -m "feat(events): fold pause and resume with coherent-history guards"
```

---

### Task 5: The three transition arms

**Files:**
- Modify: `core/execution/src/transition.rs`
- Test: `core/execution/src/transition.rs` (inline `mod tests`)

- [ ] **Step 1: Write the failing tests**

```rust
    /// The owner's resume path out of Blocked, missing since 04a. Approval makes the node
    /// dispatchable on the next scheduler pass; per D-020's spirit nothing auto-starts out of a
    /// manual intervention — the scheduler's normal cycle does.
    #[test]
    fn an_owner_approval_readies_a_blocked_node() {
        assert_eq!(
            apply_transition(&request(NodeState::Blocked, NodeOutcome::Approved, 3)).unwrap(),
            NodeState::Ready
        );
    }

    /// Graceful pause holds work that has not started. A running node is not pausable in this
    /// milestone, and a ghost is not pausable in any.
    #[test]
    fn pause_holds_ready_and_queued_work_only() {
        for from in [NodeState::Ready, NodeState::Queued] {
            assert_eq!(
                apply_transition(&request(from, NodeOutcome::Paused, 0)).unwrap(),
                NodeState::Paused
            );
        }
        for from in [NodeState::Running, NodeState::Ghost, NodeState::Succeeded] {
            assert!(
                apply_transition(&request(from, NodeOutcome::Paused, 0)).is_err(),
                "{from:?} must not pause"
            );
        }
    }

    /// A node running when the execution stopped has unknown effects. Blocked is the only legal
    /// consequence; anything else would resume work nobody judged safe.
    #[test]
    fn an_interrupted_running_node_blocks() {
        assert_eq!(
            apply_transition(&request(NodeState::Running, NodeOutcome::Interrupted, 1)).unwrap(),
            NodeState::Blocked
        );
        for from in [NodeState::Ready, NodeState::Queued, NodeState::Paused] {
            assert!(
                apply_transition(&request(from, NodeOutcome::Interrupted, 0)).is_err(),
                "{from:?} was not running; interruption does not apply"
            );
        }
    }
```

- [ ] **Step 2: Run to verify they fail**, then add the arms in `apply_transition`, placed with their kin:

```rust
        (S::Blocked, O::Approved) => Ok(S::Ready),
        (S::Ready | S::Queued, O::Paused) => Ok(S::Paused),
        (S::Running, O::Interrupted) => Ok(S::Blocked),
```

The ghost arm and the terminal check precede these, so `Ghost + Paused` and `Succeeded + Interrupted` stay illegal by existing structure — the tests above pin it anyway.

- [ ] **Step 3: Check the property tests still hold.** `transition_properties.rs`'s totality and determinism cover the new outcomes once the `OUTCOMES` array grew in Task 1. `a_ghost_never_becomes_runnable` must still pass — a ghost cannot reach `Paused` either; extend that property's assertion to exclude `NodeState::Paused` as a ghost destination, and re-run.

- [ ] **Step 4: Sabotage** — make `(S::Running, O::Interrupted)` return `Ok(S::Queued)` (the silent-retry bug this design forbids), confirm `an_interrupted_running_node_blocks` fails, revert from a backup, re-confirm. Quote both.

- [ ] **Step 5: Commit**

```bash
git add core/execution
git commit -m "feat(execution): add the blocked-resume, pause and interruption transitions"
```

---

### Task 6: Recovery and resume preconditions, pure

**Files:**
- Create: `core/execution/src/recovery.rs`
- Modify: `core/execution/src/lib.rs`
- Modify: `core/execution/tests/source_invariants.rs` (add `recovery.rs` to the scan)
- Test: `core/execution/src/recovery.rs` (inline `mod tests`)

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use graphhelm_events::ExecutionProjection;
    use graphhelm_protocols::{NodeState, SimulationStatus};

    fn projection(nodes: &[(&str, NodeState)], status: Option<SimulationStatus>) -> ExecutionProjection {
        ExecutionProjection {
            execution_id: Some("execution-1".to_owned()),
            node_states: nodes
                .iter()
                .map(|(node, state)| ((*node).to_owned(), *state))
                .collect(),
            simulation_status: status,
            ..ExecutionProjection::default()
        }
    }

    /// Every running node was interrupted; nothing else was. The plan is deterministic because the
    /// map is a BTreeMap.
    #[test]
    fn recovery_interrupts_exactly_the_running_nodes() {
        let projection = projection(
            &[
                ("a", NodeState::Running),
                ("b", NodeState::Queued),
                ("c", NodeState::Succeeded),
                ("d", NodeState::Running),
            ],
            Some(SimulationStatus::Running),
        );
        let plan = recovery_plan(&projection);
        assert_eq!(plan, ["a".to_owned(), "d".to_owned()]);
    }

    #[test]
    fn a_clean_projection_needs_no_recovery() {
        let projection = projection(&[("a", NodeState::Succeeded)], Some(SimulationStatus::Running));
        assert!(recovery_plan(&projection).is_empty());
    }

    /// §11.4's decidable subset. Each rejection names its reason, because the driver turns these
    /// into operator messages.
    #[test]
    fn resume_requires_a_paused_execution_with_matching_version_and_no_running_node() {
        let good = projection(&[("a", NodeState::Paused)], Some(SimulationStatus::Paused));
        assert_eq!(resume_preconditions(&good, None), Ok(()));

        let not_paused = projection(&[], Some(SimulationStatus::Running));
        assert_eq!(
            resume_preconditions(&not_paused, None),
            Err(ResumeError::NotPaused)
        );

        let still_running = projection(
            &[("a", NodeState::Running)],
            Some(SimulationStatus::Paused),
        );
        assert_eq!(
            resume_preconditions(&still_running, None),
            Err(ResumeError::UnrecoveredInterruption)
        );

        let mut unstarted = projection(&[], Some(SimulationStatus::Paused));
        unstarted.execution_id = None;
        assert_eq!(
            resume_preconditions(&unstarted, None),
            Err(ResumeError::NotStarted)
        );
    }

    /// Resuming against a different graph version than the projection recorded is refused: the
    /// plan the nodes were scheduled under no longer describes the graph.
    #[test]
    fn resume_refuses_a_version_mismatch() {
        let mut projection = projection(&[], Some(SimulationStatus::Paused));
        // Wire a current_graph into the projection the same way execution_projection.rs test
        // fixtures do, then ask to resume against a different version number.
        attach_current_graph(&mut projection, 3);
        assert_eq!(
            resume_preconditions(&projection, Some(4)),
            Err(ResumeError::VersionMismatch)
        );
        assert_eq!(resume_preconditions(&projection, Some(3)), Ok(()));
    }
}
```

Write `attach_current_graph` concretely from how existing tests build a `PersistedGraphVersion` (the 04d waiver test loads `conformance/schemas/valid/persisted-graph-version.json`; reuse that approach, adjusting the version number field through the type's real API — if the fixture's version cannot be changed through the API, load it and test against the fixture's own number instead, and say so).

- [ ] **Step 2: Implement**

```rust
//! Crash recovery and resume preconditions, as pure decisions.
//!
//! `OBSERVABILITY_AND_RECOVERY.md` §11.4 lists eight resume steps. The decidable ones — a paused
//! execution, a matching graph version, no node still marked running — are validated here. Lease
//! renewal, route health, sandbox recreation and session invalidation need a runtime and are
//! Milestone 05's; naming them here is deliberate, so nothing pretends to validate them.

use graphhelm_events::ExecutionProjection;
use graphhelm_protocols::{NodeState, SimulationStatus};

/// The nodes whose effects are unknown, in deterministic order.
///
/// The driver must record `NodeOutcome::Interrupted` for each before anything else happens to the
/// execution; `(Running, Interrupted) -> Blocked` is the only legal consequence, and the design's
/// acceptance criterion — never resume a node whose effects are unknown — rests on it.
#[must_use]
pub fn recovery_plan(projection: &ExecutionProjection) -> Vec<String> {
    projection
        .node_states
        .iter()
        .filter(|(_, state)| **state == NodeState::Running)
        .map(|(node, _)| node.clone())
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResumeError {
    /// No execution has started; there is nothing to resume.
    NotStarted,
    /// Only a paused execution resumes. A completed, failed or cancelled one is history.
    NotPaused,
    /// A node is still marked running: the interruption has not been recovered, and resuming
    /// would run work whose predecessor effects are unknown.
    UnrecoveredInterruption,
    /// The graph version to resume against is not the one the projection recorded.
    VersionMismatch,
}

/// §11.4's decidable subset, as a pure gate the driver must pass before appending
/// `execution_resumed`.
///
/// # Errors
/// Each variant names the operator-facing reason; see `ResumeError`.
pub fn resume_preconditions(
    projection: &ExecutionProjection,
    resume_against_version: Option<u64>,
) -> Result<(), ResumeError> {
    if projection.execution_id.is_none() {
        return Err(ResumeError::NotStarted);
    }
    if projection.simulation_status != Some(SimulationStatus::Paused) {
        return Err(ResumeError::NotPaused);
    }
    if projection
        .node_states
        .values()
        .any(|state| *state == NodeState::Running)
    {
        return Err(ResumeError::UnrecoveredInterruption);
    }
    match (resume_against_version, &projection.current_graph) {
        (Some(requested), Some(current)) if requested != current.number() => {
            Err(ResumeError::VersionMismatch)
        }
        _ => Ok(()),
    }
}
```

Export both plus `ResumeError` from `lib.rs`; add `recovery.rs` to the purity scan's `include_str!` list and loop.

- [ ] **Step 3: Sabotage** — make `recovery_plan` also return `Queued` nodes, confirm `recovery_interrupts_exactly_the_running_nodes` fails; separately make `resume_preconditions` skip the `UnrecoveredInterruption` check, confirm the precondition test fails. Revert each from a backup, re-confirm. Quote all outputs.

- [ ] **Step 4: Commit**

```bash
git add core/execution
git commit -m "feat(execution): add pure crash recovery and resume preconditions"
```

---

### Task 7: The composed lifecycle, replayed

**Files:**
- Create: `core/execution/tests/execution_lifecycle.rs`
- Modify: `core/execution/Cargo.toml` (dev-dependencies only)

This is the first time every pure piece shipped since 04a runs together: `ready_set` proposes, `dispatch_plan` bounds, `FixtureExecutor` executes, `apply_transition` decides, the fold records, `recovery_plan` and `resume_preconditions` gate the lifecycle. The driver here is **test-only** — a loop inside the test file — and shipping the production driver remains 04f. If this test cannot be written without new public API, that is a finding about 04f's real needs: report it, do not widen any crate for the test's convenience.

Dev-dependencies needed: `graphhelm-simulation` (for `FixtureExecutor`) — a dev-dep from `core/execution` to `core/simulation` reverses the normal edge, so **check for a cycle**: `simulation` depends on `execution` normally, and cargo permits dev-cycles only when the normal graph is acyclic — it is, but confirm `cargo +1.97.1 tree` accepts it. If cargo refuses, put the lifecycle test in `core/simulation/tests/` instead (it depends on everything needed) and say so.

- [ ] **Step 1: Write the test**

One test, four acts, asserted at each boundary:

1. **Run**: a three-node chain (`a -> b -> c`), all fixtures `Success`. Drive: fold `execution_started` (Autopilot); loop { mark untouched nodes `Ready` via the approval path their state machine requires — read `apply_transition` for the legal route from `Draft`, which is `Approved -> Ready`; compute `ready_set`; `dispatch_plan` with `max_parallel = 1`; for each dispatched node emit the two `Started` hops (`Ready -> Queued`, `Queued -> Running`) then the executor's outcome, each folded as `node_outcome_recorded` } until `a` succeeds.
2. **Pause**: after `a` succeeds and `b` is `Ready`, fold `execution_paused`; emit `Paused` outcomes for the held nodes per `(Ready | Queued, Paused) -> Paused`. Assert `ready_set` proposes nothing dispatchable that the plan would take (paused nodes are not dispatchable — 04c pinned it).
3. **Crash and recover**: simulate an interruption instead of a clean pause on a *fresh* history where `b` is `Running`: `recovery_plan` names exactly `b`; fold `node_outcome_recorded { Interrupted, Blocked }`; assert `resume_preconditions` refuses while anything is `Running` and accepts after recovery plus an owner `Approved` on the blocked node.
4. **Complete and replay**: resume (`execution_resumed` after preconditions pass, `Started` for paused nodes per `(Paused, Started) -> Queued`), drive to completion, fold `execution_completed { Completed }`. Then **replay the entire event list twice** and assert byte-identical serialized projections, and replay a prefix through `ProjectionGeneration::apply_page` and the remainder, asserting it equals the direct replay — the 04b guarantee, now over a history containing every lifecycle event kind.

Build envelopes with the same helpers `core/events/tests/execution_projection.rs` uses (real repository, chained hashes). Keep every act's event list appended to one `Vec` so the final replay covers the whole story.

- [ ] **Step 2: Run it, and prove it can fail** — flip one fixture to `Failure` and assert the run detours through `RetryableFailure -> Queued` and still completes (attempts derived, not trusted); then sabotage the replay comparison by mutating one serialized byte and confirm the assertion fails; revert. Quote outputs.

- [ ] **Step 3: Commit**

```bash
git add core/execution Cargo.lock
git commit -m "test(execution): prove the composed lifecycle pauses, recovers, completes and replays"
```

---

### Task 8: Documentation and the full gate

**Files:**
- Modify: `docs/milestones/graph-engine-governor.md`, `CHANGELOG.md`

- [ ] **Step 1: Write from the code as built.** A 04e section: the vocabulary widenings, the two event kinds, the three transition arms (naming `Blocked`'s resume as closing a gap open since 04a), the pure recovery/resume functions with §11.4's undecidable items named, and the composed lifecycle test as the first end-to-end composition. **Then re-read the entire file** — the stale-bullet defect happened in the out-of-scope section, not the new one. `Blocked` no longer lacks a resume path; `Paused` and `Linting` producer claims change (`Paused` now has one; `Linting` still does not — check the exact sentence and fix only what became false). Update the 04e bullet out of the "does not exist yet" list.

- [ ] **Step 2: CHANGELOG entry** in the established voice.

- [ ] **Step 3: Run the full gate** — `./ci/gate.ps1`, `GRAPHHELM_PG_BIN` set. The schema changed; the PostgreSQL matrix is required. Expected: `[gate] GREEN - every stage passed.`

- [ ] **Step 4: Commit**

```bash
git add docs CHANGELOG.md
git commit -m "docs(m04): record pause, resume, cancel and recovery"
```

---

## Definition of done

- `NodeOutcome::{Paused, Interrupted}` and `SimulationStatus::Cancelled` round-trip with exact wire names; both schema enums widened in place, both copies byte-identical, both digests recomputed; fixture inventories at 25.
- `(Blocked, Approved) -> Ready`, `(Ready | Queued, Paused) -> Paused`, `(Running, Interrupted) -> Blocked`; a ghost can still reach nothing but `Ready` and `Cancelled`, and a terminal node still transitions nowhere new.
- Pause and resume fold with coherent-history guards; an incoherent pause or resume is corrupt.
- `recovery_plan` names exactly the running nodes; `resume_preconditions` enforces §11.4's decidable subset and its rustdoc names what it does not validate.
- The composed lifecycle test drives pause, crash, recovery, owner resume and completion through every pure piece, and the full history replays byte-identically, including through `apply_page`.
- Every new guard observed failing once, deliberately, and reverted from a backup.
- `./ci/gate.ps1` green, PostgreSQL matrix included.

## What this plan deliberately excludes

The production driver and operator CLI (04f) — the lifecycle loop in Task 7 is a test fixture, not a shipped component. Immediate-stop, branch pause and sandbox kill (§12), which need a real runtime (Milestone 05). Compensation for external effects (§13) — nothing external exists yet. Lease renewal, route health, sandbox recreation and session invalidation from §11.4. The retry-category table of §14 — `classify_progress` judges counts, and mapping §14's categories onto outcomes needs the real executor. The deferred ledger from 04d stands: unbudgeted ghost births, `node_states` versus the resource guard, acceptance `graph_version` lineage, and the `simulate()`/`FixtureExecutor` divergence.
