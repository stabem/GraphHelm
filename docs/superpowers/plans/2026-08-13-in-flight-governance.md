# Milestone 04d - In-Flight Governance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the Governor change a *running* graph — signal intake, ghost proposal, approval, mutation acceptance under the mode in force, and owner override with the existing waiver — while nothing but the Governor can, and every decision is recorded as an event.

**Architecture:** Three new event kinds carry governance onto the wire (`signal_recorded`, `ghost_node_proposed`, `mutation_accepted`); the projection folds them into counters and ghost states without judging them; and pure decision functions in `core/governor` judge them against 04a's bounds and D-022's modes. Ghost approval needs no new machinery: `node_outcome_recorded` with `Approved -> Ready` already exists and `apply_transition` already accepts it. Bounded concurrency lands as a pure `dispatch_plan` in `core/execution`, giving `GraphBudgets.max_parallel_model_calls` its first reader.

**Tech Stack:** Rust 1.97.1, edition 2024. `graphhelm-protocols` for wire types, `graphhelm-events` for the projection, `core/governor` for decisions. No new dependency. PostgreSQL 16+ for the adapter matrix.

**Design source:** `docs/superpowers/specs/2026-08-13-graph-engine-governor-design.md`, decisions 5.1, 5.2, 5.4, 5.5, 5.7, 5.8.

---

## Decisions this plan enacts, stated up front

**A signal event carries no free-form content (D-036).** `signal_recorded` carries the typed fields — id, source, raw kind string, severity — plus a SHA-256 of the full raw envelope. The description and the raw JSON are externalized as encrypted Evidence by whatever drives intake (04f), through the existing `SealingGraphExternalizer` path; the event's evidence list references it. The pure intake function returns *what to record* and *what to externalize*; it performs neither.

**Mode binds at acceptance (5.5).** `decide_mutation` consults the projection's mode at decision time, and `mutation_accepted` records the mode it was accepted under. The fold rejects an event whose recorded mode disagrees with the projection as `Corrupt` — an acceptance under a mode the execution was not in is history that cannot have happened.

**Bounds block, never truncate (5.7).** The 10,001st signal does not get dropped: `admit_signal` refuses it with an error the caller must turn into a blocked execution. The 65th mutation acceptance is refused the same way. The fold counts without judging, exactly as 04b's counters do, because `core/events` cannot import `core/execution`'s bounds.

**The waiver is the M03 waiver (5.8).** An owner override during execution emits the existing `PolicyWaiverCreated` with `WaiverScope::Node` and `requirement` naming the obligation cleared. No second waiver type, no new event kind.

**Ghost approval is already on the wire.** `NodeOutcomeRecorded { outcome: Approved, next_state: Ready }` exists since 04b and `apply_transition` maps `(Ghost, Approved) -> Ready` since 04a. This plan adds only the ghost's *birth* (`ghost_node_proposed`); its approval reuses what exists.

**`SignalSeverity` and `SignalSourceKind` move to `graphhelm-protocols`.** Same reasoning and same pattern as 04b's `NodeOutcome` move: they now travel on the wire, and `core/events` cannot import them from `core/execution`. `core/execution` re-exports them so no consumer changes. `TypedSignal` and `SignalKind` stay in `core/execution` — they are interpretation, not wire vocabulary.

---

## File structure

| File | Responsibility |
|---|---|
| `core/protocols/src/simulation.rs` | `SignalSeverity`, `SignalSourceKind` — now wire vocabularies |
| `core/protocols/src/event.rs` | The three governance payloads and `EventKind` variants |
| `schemas/event-envelope.schema.json` | Wire contract for the three kinds (both copies, both digests) |
| `core/events/src/projection.rs` | `signals_recorded`, `accepted_mutations`, ghost fold, guards |
| `core/governor/src/inflight.rs` | `admit_signal`, `decide_mutation`, `override_with_waiver` — pure decisions |
| `core/execution/src/dispatch.rs` | `dispatch_plan` — bounded concurrency as a pure decision |
| `core/governor/tests/inflight_governance.rs` | Mode binding, bounds, unrecognized-never-mutates |

## Hard-won process rules, binding on every task

- **Verify the digest method against the unmodified file before trusting it**, and mirror the frozen schema copy byte-for-byte. `catalog_integrity.rs` compares raw bytes.
- **Revert sabotage from a `cp` backup, never `git checkout --`** — that discards all uncommitted work in the file.
- **Run the workspace clippy** (`cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings`); a per-crate pass proves nothing.
- **Write documentation from the code as built, not from this plan's intent.** Three milestones in a row shipped a doc claim no code delivered. Every sentence in Task 7's docs must cite a test or a line of source.
- **Every new guard must be observed failing once, deliberately**, and the sabotage reverted.

---

### Task 1: Move the signal wire vocabularies and add the three event kinds

**Files:**
- Modify: `core/protocols/src/simulation.rs`
- Modify: `core/protocols/src/event.rs`
- Modify: `core/execution/src/signal.rs`
- Modify: `core/execution/src/lib.rs`
- Test: `core/protocols/tests/persistence_wire.rs`, `core/protocols/tests/wire_roundtrip.rs`

- [ ] **Step 1: Write the failing tests**

Add to `core/protocols/tests/wire_roundtrip.rs`, in the normative wire-name test:

```rust
    for (severity, expected) in [
        (SignalSeverity::Low, "low"),
        (SignalSeverity::Medium, "medium"),
        (SignalSeverity::High, "high"),
        (SignalSeverity::Critical, "critical"),
    ] {
        assert_eq!(serde_json::to_value(severity).unwrap(), expected);
    }
    for (source, expected) in [
        (SignalSourceKind::Node, "node"),
        (SignalSourceKind::Runtime, "runtime"),
        (SignalSourceKind::Tool, "tool"),
        (SignalSourceKind::Test, "test"),
        (SignalSourceKind::User, "user"),
        (SignalSourceKind::Dream, "dream"),
        (SignalSourceKind::System, "system"),
    ] {
        assert_eq!(serde_json::to_value(source).unwrap(), expected);
        assert_eq!(
            serde_json::from_value::<SignalSourceKind>(serde_json::json!(expected)).unwrap(),
            source
        );
    }
```

Add to `core/protocols/tests/persistence_wire.rs`:

```rust
#[test]
fn governance_event_kinds_round_trip_with_exact_wire_names() {
    let digest = "a".repeat(64);
    let cases = [
        (
            "signal_recorded",
            json!({"executionId":"execution-1","signalId":"signal-1","sourceKind":"node","sourceId":"node-a","kind":"unexpected_dependency","severity":"high","envelopeSha256":digest}),
        ),
        (
            "ghost_node_proposed",
            json!({"executionId":"execution-1","nodeId":"ghost-a","draftId":"draft-1"}),
        ),
        (
            "mutation_accepted",
            json!({"executionId":"execution-1","draftId":"draft-1","mode":"autopilot","graphVersion":4}),
        ),
    ];
    for (name, data) in cases {
        let wire = json!({"type": name, "data": data});
        let kind: EventKind = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&kind).unwrap(), wire, "{name}");
    }
}

/// An acceptance that does not say which mode it was accepted under is not evidence of anything.
#[test]
fn a_mutation_acceptance_without_a_mode_is_rejected() {
    let wire = json!({
        "type": "mutation_accepted",
        "data": {"executionId":"execution-1","draftId":"draft-1","graphVersion":4}
    });
    assert!(serde_json::from_value::<EventKind>(wire).is_err());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo +1.97.1 test -p graphhelm-protocols --test persistence_wire governance_ --test wire_roundtrip`
Expected: FAIL to compile (`no SignalSeverity in the root`), then `unknown variant 'signal_recorded'` once the vocabularies exist.

- [ ] **Step 3: Move the vocabularies**

In `core/protocols/src/simulation.rs`, after `ExecutionMode`, add — copied **verbatim** from `core/execution/src/signal.rs`, then delete them there:

```rust
/// Signal severity, from `schemas/graph-signal.schema.json`. A wire vocabulary because
/// `signal_recorded` carries it; `core/execution` re-exports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SignalSeverity {
    Low,
    Medium,
    High,
    Critical,
}

/// The closed signal source vocabulary, from `schemas/graph-signal.schema.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
```

In `core/execution/src/signal.rs`, delete both local enums and import them: `use graphhelm_protocols::{SignalSeverity, SignalSourceKind};`. In `core/execution/src/lib.rs`, change the signal re-export so both names stay reachable from the crate root:

```rust
pub use graphhelm_protocols::{SignalSeverity, SignalSourceKind};
pub use signal::{SignalError, SignalKind, SignalSource, TypedSignal};
```

Diff the moved enums against the originals before deleting — a changed `rename_all` or variant order is a silent wire change. `SignalSource` (the struct) stays in `core/execution`; only the two field-less vocabularies move.

- [ ] **Step 4: Add the payloads and variants**

In `core/protocols/src/event.rs`, after `ExecutionCompleted`:

```rust
/// A Graph Signal was validated and recorded.
///
/// No free-form content, per D-036: the description and the raw envelope are externalized as
/// encrypted Evidence and referenced by this event's evidence list; `envelope_sha256` binds this
/// record to those exact bytes. `kind` is the raw type string, preserved even when unrecognized,
/// because the emitting agent is not authoritative and the record is the evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SignalRecorded {
    pub execution_id: OpaqueId,
    pub signal_id: OpaqueId,
    pub source_kind: SignalSourceKind,
    pub source_id: OpaqueId,
    pub kind: String,
    pub severity: SignalSeverity,
    pub envelope_sha256: RawSha256,
}

/// The Governor proposed an expansion. The node exists in state `Ghost` from this moment,
/// visible and never scheduled, per decision 5.2. Approval travels as the existing
/// `node_outcome_recorded` with `Approved -> Ready`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GhostNodeProposed {
    pub execution_id: OpaqueId,
    pub node_id: OpaqueId,
    pub draft_id: OpaqueId,
}

/// The Governor accepted a mutation, under the mode in force at acceptance (decision 5.5), and
/// published `graph_version` as the successor (decision 5.1). The version content travels in the
/// existing `graph_version_published` event; this records the governance act itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MutationAccepted {
    pub execution_id: OpaqueId,
    pub draft_id: OpaqueId,
    pub mode: ExecutionMode,
    pub graph_version: u64,
}
```

Add to `EventKind` after `ExecutionCompleted(ExecutionCompleted),`:

```rust
    SignalRecorded(SignalRecorded),
    GhostNodeProposed(GhostNodeProposed),
    MutationAccepted(MutationAccepted),
```

Add `SignalSeverity` and `SignalSourceKind` to this file's `use crate::{...}` list. Confirm `RawSha256` is the digest newtype the file already uses; if the existing convention for digests in payloads differs, follow the convention and report it.

- [ ] **Step 5: Run tests, then find the exhaustive matches**

Run: `cargo +1.97.1 test -p graphhelm-protocols --locked` — the new tests pass. The `EventKind` doc comment says "closed set of 20"; it becomes 23 — update it.

Run: `cargo +1.97.1 check --workspace --all-targets` — expect `E0004` in `core/events/src/projection.rs` (one site: `apply_projection_event`; `replay` now delegates to it). **Do not fix it here** — the fold is Task 3, and the workspace stays broken until then. Any *other* non-exhaustive site: add an arm preserving that site's existing behaviour and report it.

- [ ] **Step 6: Commit**

```bash
git add core/protocols core/execution
git commit -m "feat(protocols): add the three governance event kinds"
```

---

### Task 2: The wire contract for the three kinds

**Files:**
- Modify: `schemas/event-envelope.schema.json`, `schemas/releases/1.0.0/event-envelope.schema.json`
- Modify: `schemas/catalog.json`, `schemas/releases/1.0.0/catalog.json`, `schemas/CHANGELOG.md`
- Test: `core/protocols/tests/persistence_wire.rs`, `core/schema-evolution/tests/conformance.rs`

Per D-037 the `1.0.0` baseline is corrected in place; never edit a release-gate test to make this pass.

- [ ] **Step 1: Write the failing test** — model on `the_envelope_schema_accepts_every_execution_event_kind` in `persistence_wire.rs`, one envelope per new kind, using the existing `assert_envelope_valid` helper. Every governance event is execution-scoped (`scopeWithExecution`).

- [ ] **Step 2: Run it to verify it fails** — `is not valid under any of the schemas listed in the 'oneOf' keyword`.

- [ ] **Step 3: Add the `$defs`.** `signalSeverity` (`["low","medium","high","critical"]`), `signalSourceKind` (`["node","runtime","tool","test","user","dream","system"]` — derive from the Rust enums, do not retype from memory), and the three payload definitions modelled on `nodeStateChanged`: explicit `required`, `"additionalProperties": false`, `$ref` to existing `$defs` for ids and digests. Add three `oneOf` branches and three `scopeWithExecution` scope rules.

- [ ] **Step 4: Mirror, digest, verify.** Copy byte-for-byte over the frozen file. Recompute the canonical digest with the Python one-liner from the 04b plan, **after** reproducing the current stored digest from the unmodified file. Write the new digest into both catalogs and nothing else.

Run: `cargo +1.97.1 run --locked -q -p graphhelm-cli -- schema check --baseline schemas/releases/1.0.0/catalog.json --candidate schemas/catalog.json`
Expected: class `unchanged`, impact `none`, `release.ok` true. (If `graphhelm-cli` does not compile because of the Task 3 gap, verify through `graphhelm_schema_evolution::{compare_catalogs, enforce_release}` in a scratch test, delete it afterwards, and quote the output — the 04b Task 4 implementer did exactly this.)

- [ ] **Step 5: Grow the fixture inventories.** Add one fixture per kind to the schema-validated `variants` lists in `persistence_wire.rs` and `core/schema-evolution/tests/conformance.rs`, raising both counts from 20 to 23 and renaming the `all_twenty_*` tests to `all_twenty_three_*`.

- [ ] **Step 6: Changelog.** Plain `- ` bullet under `## [1.0.0]` in `schemas/CHANGELOG.md`, never the `- BREAKING ` prefix.

- [ ] **Step 7: Commit**

```bash
git add schemas core/protocols/tests/persistence_wire.rs core/schema-evolution/tests/conformance.rs
git commit -m "feat(schemas): add the governance event kinds to the envelope contract"
```

---

### Task 3: Fold governance events into the projection

**Files:**
- Modify: `core/events/src/projection.rs`
- Test: `core/events/tests/execution_projection.rs`

The fold counts and records; it never judges against bounds, because `core/events` cannot import `core/execution`. There is now **one** fold — `replay` delegates to `apply_projection_event` — so each arm is written once.

- [ ] **Step 1: Write the failing tests**

Add to `core/events/tests/execution_projection.rs`, reusing its existing envelope builders:

```rust
/// Signals are counted, not judged. The bound that blocks at MAX_SIGNALS_PER_EXECUTION lives in
/// the governor; the projection just makes the count replayable.
#[test]
fn signals_are_counted_by_folding() {
    let projection = replay(&scope(), STREAM, &signal_events(3)).unwrap();
    assert_eq!(projection.signals_recorded, 3);
}

/// A ghost is born in state Ghost, visible and never scheduled, per decision 5.2.
#[test]
fn a_proposed_ghost_appears_in_ghost_state() {
    let projection = replay(&scope(), STREAM, &ghost_proposal_events()).unwrap();
    assert_eq!(projection.node_states.get("ghost-a"), Some(&NodeState::Ghost));
}

/// A ghost proposal for a node that already has a state is history that cannot have happened.
#[test]
fn a_ghost_proposal_for_an_existing_node_is_corrupt() {
    assert_eq!(
        replay(&scope(), STREAM, &ghost_proposal_over_existing_node()).unwrap_err(),
        ReplayError::Corrupt
    );
}

/// Mode binds at acceptance, per decision 5.5. An acceptance recorded under a mode the execution
/// was not in is corrupt, not merely surprising.
#[test]
fn an_acceptance_under_the_wrong_mode_is_corrupt() {
    // started in Supervised, event claims acceptance under Autopilot
    assert_eq!(
        replay(&scope(), STREAM, &acceptance_with_mismatched_mode()).unwrap_err(),
        ReplayError::Corrupt
    );
}

#[test]
fn accepted_mutations_are_counted_by_folding() {
    let projection = replay(&scope(), STREAM, &acceptance_events(2)).unwrap();
    assert_eq!(projection.accepted_mutations, 2);
}
```

Write the fixture helpers concretely in the file, following its existing pattern (real envelopes through the real repository, chained hashes). `signal_events(n)` emits one `execution_started` then `n` `signal_recorded`; `ghost_proposal_events` emits a start then one `ghost_node_proposed` for `"ghost-a"`; `ghost_proposal_over_existing_node` records an outcome for `"ghost-a"` first; `acceptance_with_mismatched_mode` starts in `Supervised` and emits `mutation_accepted` with `mode: Autopilot`; `acceptance_events(n)` starts in `Autopilot` and emits `n` acceptances with `mode: Autopilot` and increasing `graph_version`.

- [ ] **Step 2: Run to verify they fail** — the fold has no arms, so `E0004` first; after adding empty-behaviour stubs is not allowed — go straight to Step 3 and let the assertions themselves be the RED where compilation permits.

- [ ] **Step 3: Extend the projection and the fold**

Add to `ExecutionProjection` after `identical_outcomes` (both with `#[serde(default)]`, same one-directional compatibility rule as 04b — and extend the `a_generation_missing_a_pre_existing_field_fails` loop is **not** needed, these are new fields):

```rust
    /// Signals recorded for this execution. Counted here, judged by the governor.
    #[serde(default)]
    pub signals_recorded: u32,
    /// Governor mutations accepted, per decision 5.1. Counted here, judged by the governor.
    #[serde(default)]
    pub accepted_mutations: u32,
```

Add the arms to `apply_projection_event`, after `ExecutionCompleted`:

```rust
        EventKind::SignalRecorded(payload) => {
            if projection.execution_id.as_deref() != Some(payload.execution_id.as_str()) {
                return Err(ReplayError::Corrupt);
            }
            projection.signals_recorded = projection
                .signals_recorded
                .checked_add(1)
                .ok_or(ReplayError::LimitExceeded)?;
        }
        EventKind::GhostNodeProposed(payload) => {
            if projection.execution_id.as_deref() != Some(payload.execution_id.as_str()) {
                return Err(ReplayError::Corrupt);
            }
            let node = payload.node_id.to_string();
            // A ghost is born, not transitioned into. A node that already has any state cannot
            // be proposed again; that history cannot have happened.
            if projection.node_states.contains_key(&node) {
                return Err(ReplayError::Corrupt);
            }
            if projection.node_states.len() >= MAX_PROJECTION_NODES {
                return Err(ReplayError::LimitExceeded);
            }
            projection.node_states.insert(node, NodeState::Ghost);
        }
        EventKind::MutationAccepted(payload) => {
            if projection.execution_id.as_deref() != Some(payload.execution_id.as_str())
                || projection.mode != Some(payload.mode)
            {
                return Err(ReplayError::Corrupt);
            }
            projection.accepted_mutations = projection
                .accepted_mutations
                .checked_add(1)
                .ok_or(ReplayError::LimitExceeded)?;
        }
```

- [ ] **Step 4: Run tests to verify they pass**, then prove two can fail: temporarily drop the mode comparison from the `MutationAccepted` arm and confirm `an_acceptance_under_the_wrong_mode_is_corrupt` fails; temporarily drop the `contains_key` guard and confirm `a_ghost_proposal_for_an_existing_node_is_corrupt` fails. Revert each from a `cp` backup and re-confirm green. Quote all outputs.

- [ ] **Step 5: Commit**

```bash
git add core/events
git commit -m "feat(events): fold governance events into the projection"
```

---

### Task 4: The pure governance decisions

**Files:**
- Create: `core/governor/src/inflight.rs`
- Modify: `core/governor/src/lib.rs`
- Modify: `core/governor/Cargo.toml` (only if `graphhelm-events` or `graphhelm-execution` is not already a dependency — check first, `apply.rs` likely already uses events)
- Test: `core/governor/src/inflight.rs` (inline `mod tests`)

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use graphhelm_events::ExecutionProjection;
    use graphhelm_execution::{MAX_ACCEPTED_MUTATIONS, MAX_SIGNALS_PER_EXECUTION};
    use graphhelm_protocols::ExecutionMode;

    fn projection(mode: ExecutionMode, accepted: u32, signals: u32) -> ExecutionProjection {
        ExecutionProjection {
            execution_id: Some("execution-1".to_owned()),
            mode: Some(mode),
            accepted_mutations: accepted,
            signals_recorded: signals,
            ..ExecutionProjection::default()
        }
    }

    fn signal(kind: &str) -> serde_json::Value {
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
    fn a_valid_signal_is_admitted_with_its_externalization_plan() {
        let admitted = admit_signal(&projection(ExecutionMode::Autopilot, 0, 0), &signal("no_progress"))
            .unwrap();
        assert_eq!(admitted.record.kind, "no_progress");
        assert!(!admitted.externalize.is_empty());
    }

    /// Decision 5.7: the signal budget blocks, it never drops evidence.
    #[test]
    fn the_signal_budget_blocks_rather_than_dropping() {
        let full = projection(ExecutionMode::Autopilot, 0, MAX_SIGNALS_PER_EXECUTION);
        assert_eq!(
            admit_signal(&full, &signal("no_progress")).unwrap_err(),
            GovernanceError::SignalBudgetExhausted
        );
    }

    /// Decision 5.4: an unrecognized kind is recorded but can never propose a mutation.
    #[test]
    fn an_unrecognized_signal_is_recorded_but_never_actionable() {
        let projection = projection(ExecutionMode::Autopilot, 0, 0);
        let admitted = admit_signal(&projection, &signal("invented_by_an_agent")).unwrap();
        assert!(!admitted.may_propose_mutation);
        assert_eq!(
            decide_mutation(&projection, &admitted),
            MutationDecision::Rejected(RejectionReason::SignalNotActionable)
        );
    }

    /// Decision 5.5 and D-022: the mode in force at the decision governs it.
    #[test]
    fn the_mode_in_force_governs_the_decision() {
        let admitted = |mode| {
            let projection = projection(mode, 0, 0);
            let admitted = admit_signal(&projection, &signal("no_progress")).unwrap();
            decide_mutation(&projection, &admitted)
        };
        assert_eq!(admitted(ExecutionMode::Autopilot), MutationDecision::Accept);
        assert_eq!(admitted(ExecutionMode::Supervised), MutationDecision::RequiresApproval);
        assert_eq!(
            admitted(ExecutionMode::Manual),
            MutationDecision::Rejected(RejectionReason::ManualMode)
        );
    }

    /// Decision 5.1/5.7: the 65th acceptance blocks for an owner decision, in every mode.
    #[test]
    fn the_mutation_budget_blocks_in_every_mode() {
        for mode in [ExecutionMode::Autopilot, ExecutionMode::Supervised] {
            let projection = projection(mode, MAX_ACCEPTED_MUTATIONS, 0);
            let admitted = admit_signal(&projection, &signal("no_progress")).unwrap();
            assert_eq!(
                decide_mutation(&projection, &admitted),
                MutationDecision::Blocked
            );
        }
    }

    /// Decision 5.8: an override is the M03 waiver, node-scoped, naming its obligation.
    #[test]
    fn an_override_is_the_existing_waiver_bound_to_node_and_obligation() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-08-13T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let waiver = override_with_waiver(
            &projection(ExecutionMode::Manual, 0, 0),
            "node-a",
            "quality.gate.tests",
            "owner@example",
            &["tests were reviewed manually"],
            "waiver-1".to_owned(),
            now,
        )
        .unwrap();
        assert_eq!(waiver.scope, graphhelm_protocols::WaiverScope::Node);
        assert_eq!(waiver.requirement, "quality.gate.tests");
        assert_eq!(waiver.execution_id, "execution-1");
        assert!(!waiver.acknowledged_risks.is_empty());
    }

    /// An override with no acknowledged risk is not an informed decision and must be refused.
    #[test]
    fn an_override_without_acknowledged_risks_is_refused() {
        assert_eq!(
            override_with_waiver(
                &projection(ExecutionMode::Manual, 0, 0),
                "node-a",
                "quality.gate.tests",
                "owner@example",
                &[],
                "waiver-1".to_owned(),
                chrono::DateTime::parse_from_rfc3339("2026-08-13T00:00:00Z")
                    .unwrap()
                    .with_timezone(&chrono::Utc),
            )
            .unwrap_err(),
            GovernanceError::UnacknowledgedRisk
        );
    }
}
```

- [ ] **Step 2: Run to verify they fail**, then implement:

```rust
//! In-flight governance decisions.
//!
//! Pure functions over the projection and 04a's bounds. Nothing here performs I/O, appends an
//! event, or externalizes Evidence: each function returns what should happen, and the driver (04f)
//! makes it happen. That split is what keeps every rule here property-testable offline.

use graphhelm_events::ExecutionProjection;
use graphhelm_execution::{MAX_ACCEPTED_MUTATIONS, MAX_SIGNALS_PER_EXECUTION, TypedSignal};
use graphhelm_protocols::{ExecutionMode, PolicyWaiver, SignalRecorded, WaiverScope};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GovernanceError {
    /// The signal failed validation against the contract the schema fixes.
    InvalidSignal,
    /// The execution has recorded `MAX_SIGNALS_PER_EXECUTION` signals. Per decision 5.7 this
    /// blocks the execution for an owner decision; the signal is refused, never dropped silently.
    SignalBudgetExhausted,
    /// No execution has started, so there is nothing to govern.
    NotStarted,
    /// An override must acknowledge at least one risk, or it is not an informed decision.
    UnacknowledgedRisk,
}

/// A signal the governor has admitted: what to record, what to externalize, and whether it may
/// ever become a mutation.
#[derive(Clone, Debug)]
pub struct AdmittedSignal {
    /// The event payload to append. Carries no free-form content, per D-036.
    pub record: SignalRecorded,
    /// The raw envelope bytes to externalize as encrypted Evidence, referenced by the event.
    pub externalize: Vec<u8>,
    /// Decision 5.4: false for an unrecognized kind, and nothing downstream may override it.
    pub may_propose_mutation: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectionReason {
    /// Manual mode: only the owner changes the graph (D-022).
    ManualMode,
    /// The signal's kind is unrecognized and can never propose a mutation (decision 5.4).
    SignalNotActionable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MutationDecision {
    /// Autopilot: the Governor may accept its own mutation.
    Accept,
    /// Supervised: the proposal stands until the owner approves it.
    RequiresApproval,
    /// The proposal is refused outright.
    Rejected(RejectionReason),
    /// `MAX_ACCEPTED_MUTATIONS` is spent. The execution blocks for an owner decision, in every
    /// mode, per decisions 5.1 and 5.7.
    Blocked,
}

/// Validates and admits one signal, or refuses it.
///
/// # Errors
/// `InvalidSignal` when the envelope violates the schema contract; `SignalBudgetExhausted` when
/// the execution has recorded its full signal budget; `NotStarted` when no execution exists.
pub fn admit_signal(
    projection: &ExecutionProjection,
    envelope: &serde_json::Value,
) -> Result<AdmittedSignal, GovernanceError> {
    let execution_id = projection
        .execution_id
        .as_deref()
        .ok_or(GovernanceError::NotStarted)?;
    if projection.signals_recorded >= MAX_SIGNALS_PER_EXECUTION {
        return Err(GovernanceError::SignalBudgetExhausted);
    }
    let signal = TypedSignal::parse(envelope).map_err(|_| GovernanceError::InvalidSignal)?;
    let externalize =
        serde_json::to_vec(envelope).map_err(|_| GovernanceError::InvalidSignal)?;
    let record = build_signal_record(execution_id, &signal, &externalize)?;
    Ok(AdmittedSignal {
        record,
        externalize,
        may_propose_mutation: signal.can_propose_mutation(),
    })
}

/// Decides one admitted proposal under the mode in force right now (decision 5.5).
#[must_use]
pub fn decide_mutation(
    projection: &ExecutionProjection,
    signal: &AdmittedSignal,
) -> MutationDecision {
    if !signal.may_propose_mutation {
        return MutationDecision::Rejected(RejectionReason::SignalNotActionable);
    }
    if projection.accepted_mutations >= MAX_ACCEPTED_MUTATIONS {
        return MutationDecision::Blocked;
    }
    match projection.mode {
        Some(ExecutionMode::Autopilot) => MutationDecision::Accept,
        Some(ExecutionMode::Supervised) => MutationDecision::RequiresApproval,
        // No mode and Manual read the same: the Governor does not act on its own.
        Some(ExecutionMode::Manual) | None => {
            MutationDecision::Rejected(RejectionReason::ManualMode)
        }
    }
}

/// Builds the M03 waiver for an owner override during execution, per decision 5.8: the same
/// contract, additionally bound to the node and the obligation it clears.
///
/// # Errors
/// `NotStarted` with no execution; `UnacknowledgedRisk` when no risk is acknowledged.
// override_with_waiver: full definition follows after the module sketch. It takes the waiver
// id and the timestamp as parameters, so this crate never reads a clock or generates an id —
// exactly as ApplyServices injects ids and clock in the M03 path.
```

The helpers, written against the real construction sites. `RawSha256` is a `validated_string!` parsing 64 lowercase hex characters; reuse the digest imports `core/governor` already has from its externalization path rather than adding new ones:

```rust
fn build_signal_record(
    execution_id: &str,
    signal: &TypedSignal,
    externalize: &[u8],
) -> Result<SignalRecorded, GovernanceError> {
    use sha2::{Digest, Sha256};
    let digest = hex::encode(Sha256::digest(externalize));
    Ok(SignalRecorded {
        execution_id: OpaqueId::parse(execution_id).map_err(|_| GovernanceError::InvalidSignal)?,
        signal_id: OpaqueId::parse(signal.id()).map_err(|_| GovernanceError::InvalidSignal)?,
        source_kind: signal.source().kind(),
        source_id: OpaqueId::parse(signal.source().id())
            .map_err(|_| GovernanceError::InvalidSignal)?,
        kind: signal.raw_kind().to_owned(),
        severity: signal.severity(),
        envelope_sha256: RawSha256::parse(digest).map_err(|_| GovernanceError::InvalidSignal)?,
    })
}
```

Note the consequence: a signal whose `id` or `source.id` is not an `OpaqueId` cannot be *recorded on the wire* even though the signal schema accepts it — the event contract is stricter than the signal contract. Surface that as `InvalidSignal` and say so in the doc comment rather than papering over it.

For the waiver, mirror `core/governor/src/apply.rs:323` exactly — same struct literal, same `graphhelm_schema::validate_waiver` check — with the id and timestamp injected so this function stays pure, exactly as `ApplyServices` injects `ids` and `clock`:

```rust
pub fn override_with_waiver(
    projection: &ExecutionProjection,
    node_id: &str,
    obligation: &str,
    actor: &str,
    acknowledged_risks: &[&str],
    waiver_id: String,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<PolicyWaiver, GovernanceError> {
    let execution_id = projection
        .execution_id
        .as_deref()
        .ok_or(GovernanceError::NotStarted)?;
    if acknowledged_risks.is_empty() {
        return Err(GovernanceError::UnacknowledgedRisk);
    }
    let waiver = PolicyWaiver {
        id: waiver_id,
        requirement: obligation.to_owned(),
        execution_id: execution_id.to_owned(),
        graph_version: projection
            .current_graph
            .as_ref()
            .map_or(0, |graph| graph.number()),
        actor: actor.to_owned(),
        reason: Some(format!("owner override on node {node_id}")),
        acknowledged_risks: acknowledged_risks
            .iter()
            .map(|risk| (*risk).to_owned())
            .collect(),
        scope: WaiverScope::Node,
        created_at: now,
        expires_at: None,
    };
    let raw = serde_json::to_value(&waiver).map_err(|_| GovernanceError::InvalidSignal)?;
    if !graphhelm_schema::validate_waiver(&raw, "override-waiver").is_empty() {
        return Err(GovernanceError::InvalidSignal);
    }
    Ok(waiver)
}
```

Two verification points before trusting this sketch. `PersistedGraphVersion`'s version accessor: `GraphVersion` exposes `number()` at `core/graph/src/version.rs:57` — find the equivalent on the persisted type and follow the type, not this plan, if it is spelled differently. And `apply.rs` serializes the waiver with `serde_json::to_value(&waiver)` before validating — mirror that call site. If `graphhelm_schema::validate_waiver` rejects the generated waiver, the waiver is wrong, not the validator — **stop and report NEEDS_CONTEXT** rather than improvising a second waiver shape; 5.8's whole point is that no second shape exists.

- [ ] **Step 3: Run tests to verify they pass**, prove `the_mode_in_force_governs_the_decision` can fail by temporarily making `Manual` return `Accept` (this is the security property — an autonomy grant nobody approved), revert from a backup, re-confirm.

- [ ] **Step 4: Export.** `pub mod inflight;` or itemized re-exports in `core/governor/src/lib.rs`, matching the file's convention.

- [ ] **Step 5: Commit**

```bash
git add core/governor Cargo.lock
git commit -m "feat(governor): add the pure in-flight governance decisions"
```

---

### Task 5: Bounded concurrency as a pure decision

**Files:**
- Create: `core/execution/src/dispatch.rs`
- Modify: `core/execution/src/lib.rs`
- Test: `core/execution/src/dispatch.rs` (inline `mod tests`)

The design's 04c line promised bounded concurrency; 04c recorded it as this milestone's work. `GraphBudgets.max_parallel_model_calls` gets its first reader.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn ready(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    /// At most `limit - in_flight` nodes are dispatched, in deterministic BTreeSet order, so a
    /// replay dispatches the identical prefix.
    #[test]
    fn dispatch_respects_the_parallel_limit_deterministically() {
        let plan = dispatch_plan(&ready(&["c", "a", "b"]), 1, 3).unwrap();
        assert_eq!(plan, ["a".to_owned(), "b".to_owned()]);
    }

    #[test]
    fn a_saturated_execution_dispatches_nothing() {
        assert!(dispatch_plan(&ready(&["a"]), 3, 3).unwrap().is_empty());
        assert!(dispatch_plan(&ready(&["a"]), 5, 3).unwrap().is_empty());
    }

    /// A zero limit would mean an execution that can never progress: that is a graph authoring
    /// error surfaced loudly, not an empty plan returned silently forever.
    #[test]
    fn a_zero_limit_is_an_error_not_a_silent_stall() {
        assert_eq!(
            dispatch_plan(&ready(&["a"]), 0, 0).unwrap_err(),
            DispatchError::ZeroParallelism
        );
    }

    #[test]
    fn an_empty_ready_set_is_an_empty_plan() {
        assert!(dispatch_plan(&BTreeSet::new(), 0, 3).unwrap().is_empty());
    }
}
```

- [ ] **Step 2: Run to verify they fail**, then implement:

```rust
//! Bounded concurrency as a pure decision.
//!
//! `ready_set` says what may run; this says what runs *now*, given how much is already in flight.
//! Selection order is the `BTreeSet`'s, so the same inputs always dispatch the identical prefix —
//! a replay must not dispatch a different subset than the original run.

use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispatchError {
    /// `max_parallel` of zero means the execution can never progress. Surfacing it beats returning
    /// an empty plan forever, which would look exactly like a healthy idle execution.
    ZeroParallelism,
}

/// Selects which ready nodes to dispatch now.
///
/// # Errors
/// `ZeroParallelism` when `max_parallel` is zero.
pub fn dispatch_plan(
    ready: &BTreeSet<String>,
    in_flight: usize,
    max_parallel: usize,
) -> Result<Vec<String>, DispatchError> {
    if max_parallel == 0 {
        return Err(DispatchError::ZeroParallelism);
    }
    let capacity = max_parallel.saturating_sub(in_flight);
    Ok(ready.iter().take(capacity).cloned().collect())
}
```

Check how `GraphBudgets.max_parallel_model_calls` is typed (likely an integer or `Option`); the caller converts it to `usize` — do not take `GraphBudgets` itself, keep the function's inputs minimal. Export `dispatch_plan` and `DispatchError` from `lib.rs`, and add `dispatch.rs` to the purity source scan in `core/execution/tests/source_invariants.rs` — 04c's review found that scan silently missing new files, and this plan does not repeat that.

- [ ] **Step 3: Run tests, prove `dispatch_respects_the_parallel_limit_deterministically` can fail** by taking from the set's iterator reversed, revert from a backup, re-confirm.

- [ ] **Step 4: Commit**

```bash
git add core/execution
git commit -m "feat(execution): add bounded concurrency as a pure dispatch plan"
```

---

### Task 6: Governance properties

**Files:**
- Create: `core/governor/tests/inflight_governance.rs`

- [ ] **Step 1: Write the properties** — using plain tests plus `proptest` if `core/governor` already has it as a dev-dependency (check; add pinned to the workspace version if not):

1. **Mode binding across a switch.** Build a projection started in `Autopilot`, admit a signal, then fold an `execution_mode_changed` to `Manual` and decide: the decision must be `Rejected(ManualMode)`. Then the reverse: proposed under `Manual`, switched to `Autopilot`, decided — `Accept`. The proposal's age never matters; only the mode in force at the decision (5.5).
2. **The unrecognized kind can never mutate, whatever the mode or counters** — for every mode and a sweep of counter values, `decide_mutation` on an unactionable signal is `Rejected(SignalNotActionable)`.
3. **The budget is a hard ceiling** — at exactly `MAX_ACCEPTED_MUTATIONS`, every mode yields `Blocked`; at `MAX_ACCEPTED_MUTATIONS - 1`, `Autopilot` yields `Accept`.
4. **Determinism** — the same projection and signal always produce the same decision (compare two calls).

- [ ] **Step 2: Prove property 1 can fail** by temporarily deciding from a mode captured before the switch (bind `projection.mode` to a local before folding the change and consult the local), confirm the property fails, revert from a backup, re-confirm.

- [ ] **Step 3: Commit**

```bash
git add core/governor/tests/inflight_governance.rs
git commit -m "test(governor): prove mode binding, budgets and the unrecognized-signal wall"
```

---

### Task 7: Documentation and the full gate

**Files:**
- Modify: `docs/milestones/graph-engine-governor.md`, `CHANGELOG.md`

- [ ] **Step 1: Write the docs from the code as built.** Extend the milestone document with a 04d section. Before writing any sentence, find the test or source line that makes it true, and cite it. State plainly what still does not exist: nothing appends these events in production, nothing externalizes the Evidence, nothing drives intake — the decisions are pure functions awaiting the 04f driver. The `simulate()`/`FixtureExecutor` divergence from 04c remains open and is not this plan's work.

- [ ] **Step 2: CHANGELOG entry** in the voice of the existing ones.

- [ ] **Step 3: Run the full gate.** `./ci/gate.ps1` with `GRAPHHELM_PG_BIN` set. Expected: `[gate] GREEN - every stage passed.` The schema changed, so the PostgreSQL matrix is required.

- [ ] **Step 4: Commit**

```bash
git add docs CHANGELOG.md
git commit -m "docs(m04): record in-flight governance"
```

---

## Definition of done

- Three governance event kinds round-trip in Rust and validate against the envelope, both schema copies byte-identical, both digests recomputed and verified, fixture inventories at 23.
- `signals_recorded` and `accepted_mutations` are derived by folding; no payload carries a counter.
- A ghost is born in state `Ghost` by `ghost_node_proposed`, a second proposal for the same node is corrupt, and approval reuses the existing `Approved -> Ready` path.
- An acceptance recorded under a mode the execution was not in is corrupt.
- `admit_signal` refuses the signal past `MAX_SIGNALS_PER_EXECUTION`; `decide_mutation` blocks at `MAX_ACCEPTED_MUTATIONS` in every mode; an unrecognized kind is `Rejected(SignalNotActionable)` under every mode and counter value.
- An owner override yields the existing `PolicyWaiver`, `WaiverScope::Node`, naming its obligation, refusing an empty risk acknowledgement.
- `dispatch_plan` bounds concurrency deterministically and `max_parallel_model_calls` has a reader; the purity scan covers `dispatch.rs`.
- Every new guard observed failing once, deliberately, and reverted from a backup.
- `./ci/gate.ps1` green, PostgreSQL matrix included.

## What this plan deliberately excludes

Appending events, externalizing Evidence, and driving intake — the decisions are pure and the 04f driver wires them. Likewise the acceptance-to-publication wiring: when `decide_mutation` says `Accept`, running `apply_draft` and `prepare_draft_publication` (both existing since M03) is the driver's act; recording it is `mutation_accepted`'s. Pause, resume, cancel and crash recovery (04e). The operator CLI (04f). Signal-to-draft *translation* — turning an admitted signal into concrete `DraftOperation`s is a Governor authoring capability that needs its own design pass; this plan governs whether a proposal may proceed, not what the proposal contains. Reconciling `simulate()` with `FixtureExecutor`, tracked since 04c. The five undetected no-progress conditions stay undetected until signals flow in production.
