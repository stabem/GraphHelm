//! Execution state is a projection over execution events. These tests prove the counters are
//! derived from history rather than trusted from a payload, and that replay reproduces them.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use chrono::{TimeZone, Utc};
use graphhelm_events::{
    ExecutionProjection, LocalEventRepository, PreparedAppend, ProjectionGeneration, ReplayError,
    replay,
};
use graphhelm_execution::{TransitionRequest, apply_transition};
use graphhelm_protocols::{
    ActorId, Clock, EventEnvelope, EventKind, ExecutionId, ExecutionMode, ExecutionModeChanged,
    ExecutionPaused, ExecutionResumed, ExecutionStarted, GhostNodeProposed, IdGenerator,
    MutationAccepted, NewEvent, NodeOutcome, NodeOutcome as Outcome, NodeOutcomeRecorded,
    NodeState, OpaqueId, PersistedActor, PersistedActorType, ProjectId, RawSha256, RepositoryScope,
    Sensitivity, SignalRecorded as SignalRecordedPayload, SignalSeverity, SignalSourceKind,
    SimulationStatus, WireHash, WorkspaceId,
};

const STREAM: &str = "stream-execution-test";

struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 10, 12, 0, 0).unwrap()
    }
}

#[derive(Default)]
struct Ids(AtomicU64);
impl IdGenerator for Ids {
    fn next_id(&self, prefix: &'static str) -> String {
        format!("{prefix}-{}", self.0.fetch_add(1, Ordering::SeqCst) + 1)
    }
}

fn scope() -> RepositoryScope {
    RepositoryScope::new(
        WorkspaceId::parse("workspace-test").unwrap(),
        ProjectId::parse("project-test").unwrap(),
        Some(ExecutionId::parse("execution-test").unwrap()),
    )
}

fn actor() -> PersistedActor {
    PersistedActor::new(
        PersistedActorType::System,
        ActorId::parse("system-test").unwrap(),
    )
}

fn event(key: impl Into<String>, kind: EventKind) -> NewEvent {
    NewEvent::new(
        OpaqueId::parse(key.into()).unwrap(),
        actor(),
        Sensitivity::Internal,
        kind,
        vec![],
        vec![],
    )
}

/// Appends `events` in one batch through the real repository, exactly as `replay.rs` does, so
/// sequencing, idempotency and the hash chain are produced by the same code that runs in
/// production rather than hand-computed here.
fn append(events: Vec<NewEvent>) -> Vec<EventEnvelope> {
    let directory = tempfile::tempdir().unwrap();
    let repository = LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    )
    .unwrap();
    let request = PreparedAppend::new(
        scope(),
        OpaqueId::parse(STREAM).unwrap(),
        1,
        events,
        vec![],
        vec![],
    )
    .unwrap();
    repository.append_atomic(&request).unwrap()
}

/// The state each outcome is legal from, per `apply_transition`'s table. A real scheduler cycles a
/// retried node back out to the queue and redispatches it before the next failure, which is
/// scheduler behaviour these tests have no need to reproduce. Each outcome is still run through the
/// real transition function for its one hop, so the recorded `next_state` cannot drift from the
/// state machine that actually runs in production.
fn precondition(outcome: Outcome) -> NodeState {
    match outcome {
        Outcome::Started => NodeState::Queued,
        Outcome::Approved => NodeState::Ghost,
        Outcome::Invalidated => NodeState::Succeeded,
        Outcome::Succeeded
        | Outcome::TerminalFailure
        | Outcome::RetryableFailure
        | Outcome::NeedsInput
        | Outcome::NeedsCapacity
        | Outcome::Waived
        | Outcome::Skipped
        | Outcome::Cancelled
        | Outcome::Interrupted => NodeState::Running,
        // Only `Ready` and `Queued` nodes pause; `apply_transition` does not yet accept this
        // outcome from either (that arm ships in Task 5), so no fixture in this file drives it
        // through `execution_events` yet. This arm exists to keep `precondition` exhaustive.
        Outcome::Paused => NodeState::Ready,
    }
}

/// Emits one `execution_started` followed by one `node_outcome_recorded` per outcome, for node
/// `"start"`. `next_state` is computed by calling `apply_transition`, so the fixture cannot drift
/// from the state machine it is exercising.
fn execution_events(outcomes: &[Outcome]) -> Vec<EventEnvelope> {
    let node_id = OpaqueId::parse("start").unwrap();
    let execution_id = OpaqueId::parse("execution-test").unwrap();
    let mut new_events = vec![event(
        "execution-started",
        EventKind::ExecutionStarted(ExecutionStarted {
            execution_id: execution_id.clone(),
            graph_version: 1,
            graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
            mode: ExecutionMode::Supervised,
        }),
    )];

    let mut attempts = 0_u32;
    let mut identical = 0_u32;
    let mut last: Option<Outcome> = None;
    for (index, outcome) in outcomes.iter().copied().enumerate() {
        let identical_so_far = if last == Some(outcome) { identical } else { 0 };
        let next_state = apply_transition(&TransitionRequest {
            current: precondition(outcome),
            outcome,
            attempts,
            identical_outcomes: identical_so_far,
        })
        .unwrap();
        if outcome == Outcome::Started {
            attempts += 1;
        }
        identical = if last == Some(outcome) {
            identical + 1
        } else {
            1
        };
        last = Some(outcome);

        new_events.push(event(
            format!("outcome-{index}"),
            EventKind::NodeOutcomeRecorded(NodeOutcomeRecorded {
                execution_id: execution_id.clone(),
                node_id: node_id.clone(),
                outcome,
                next_state,
            }),
        ));
    }
    append(new_events)
}

/// An execution start followed by a mode change, per D-022.
fn started_then_mode_changed() -> Vec<EventEnvelope> {
    let execution_id = OpaqueId::parse("execution-test").unwrap();
    append(vec![
        event(
            "execution-started",
            EventKind::ExecutionStarted(ExecutionStarted {
                execution_id: execution_id.clone(),
                graph_version: 1,
                graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
                mode: ExecutionMode::Supervised,
            }),
        ),
        event(
            "mode-changed",
            EventKind::ExecutionModeChanged(ExecutionModeChanged {
                execution_id,
                previous_mode: Some(ExecutionMode::Supervised),
                mode: ExecutionMode::Manual,
            }),
        ),
    ])
}

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
    let stalled = replay(
        &scope(),
        STREAM,
        &execution_events(&[
            Outcome::RetryableFailure,
            Outcome::RetryableFailure,
            Outcome::RetryableFailure,
        ]),
    )
    .unwrap();
    assert_eq!(stalled.identical_outcomes.get("start"), Some(&3));

    let interrupted = replay(
        &scope(),
        STREAM,
        &execution_events(&[
            Outcome::RetryableFailure,
            Outcome::RetryableFailure,
            Outcome::NeedsInput,
            Outcome::RetryableFailure,
        ]),
    )
    .unwrap();
    assert_eq!(interrupted.identical_outcomes.get("start"), Some(&1));
}

/// `Started` is dispatch bookkeeping, not a semantic outcome of work. It must not break a run of
/// identical failures, or the no-progress bound can never fire for a retry loop — the state
/// machine forces a `Started` between any two failures of one node (04e finding 3).
#[test]
fn dispatch_hops_do_not_break_an_identical_outcome_run() {
    let events = execution_events(&[
        Outcome::RetryableFailure,
        Outcome::Started,
        Outcome::Started,
        Outcome::RetryableFailure,
    ]);
    let projection = replay(&scope(), STREAM, &events).unwrap();
    assert_eq!(projection.identical_outcomes.get("start"), Some(&2));
    assert_eq!(
        projection.identical_outcomes_for("start", NodeOutcome::RetryableFailure),
        2
    );
}

/// The state recorded on the wire is the decision apply_transition produced. The projection stores
/// it; it does not second-guess it, because core/events cannot depend on core/execution.
#[test]
fn the_recorded_next_state_becomes_the_node_state() {
    let projection = replay(&scope(), STREAM, &execution_events(&[Outcome::Succeeded])).unwrap();
    assert_eq!(
        projection.node_states.get("start"),
        Some(&NodeState::Succeeded)
    );
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

/// An execution start followed by a pause. `simulation_status` is `None` right after
/// `execution_started` — nothing sets it until `simulation_started`, `simulation_completed` or
/// `execution_completed` folds — so this pins the `None -> Paused` half of the pause guard.
fn started_then_paused() -> Vec<EventEnvelope> {
    let execution_id = OpaqueId::parse("execution-test").unwrap();
    append(vec![
        event(
            "execution-started",
            EventKind::ExecutionStarted(ExecutionStarted {
                execution_id: execution_id.clone(),
                graph_version: 1,
                graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
                mode: ExecutionMode::Supervised,
            }),
        ),
        event(
            "execution-paused",
            EventKind::ExecutionPaused(ExecutionPaused { execution_id }),
        ),
    ])
}

/// A start, a pause, then a resume. Resuming restores `Running`.
fn paused_then_resumed() -> Vec<EventEnvelope> {
    let execution_id = OpaqueId::parse("execution-test").unwrap();
    append(vec![
        event(
            "execution-started",
            EventKind::ExecutionStarted(ExecutionStarted {
                execution_id: execution_id.clone(),
                graph_version: 1,
                graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
                mode: ExecutionMode::Supervised,
            }),
        ),
        event(
            "execution-paused",
            EventKind::ExecutionPaused(ExecutionPaused {
                execution_id: execution_id.clone(),
            }),
        ),
        event(
            "execution-resumed",
            EventKind::ExecutionResumed(ExecutionResumed { execution_id }),
        ),
    ])
}

/// A start followed by two pauses. The second pause finds `simulation_status` already `Paused`,
/// which is not `None | Some(Running)` — this history cannot have happened.
fn paused_twice() -> Vec<EventEnvelope> {
    let execution_id = OpaqueId::parse("execution-test").unwrap();
    append(vec![
        event(
            "execution-started",
            EventKind::ExecutionStarted(ExecutionStarted {
                execution_id: execution_id.clone(),
                graph_version: 1,
                graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
                mode: ExecutionMode::Supervised,
            }),
        ),
        event(
            "execution-paused-1",
            EventKind::ExecutionPaused(ExecutionPaused {
                execution_id: execution_id.clone(),
            }),
        ),
        event(
            "execution-paused-2",
            EventKind::ExecutionPaused(ExecutionPaused { execution_id }),
        ),
    ])
}

/// A start followed directly by a resume, with no pause in between. `simulation_status` is
/// `None`, not `Some(Paused)` — this history cannot have happened.
fn resumed_without_pause() -> Vec<EventEnvelope> {
    let execution_id = OpaqueId::parse("execution-test").unwrap();
    append(vec![
        event(
            "execution-started",
            EventKind::ExecutionStarted(ExecutionStarted {
                execution_id: execution_id.clone(),
                graph_version: 1,
                graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
                mode: ExecutionMode::Supervised,
            }),
        ),
        event(
            "execution-resumed",
            EventKind::ExecutionResumed(ExecutionResumed { execution_id }),
        ),
    ])
}

/// Pausing sets the aggregate status; resuming restores it. Both are coherent-history guards, not
/// judgments — the resume preconditions live in `graphhelm_execution`.
#[test]
fn pause_and_resume_fold_into_the_aggregate_status() {
    let projection = replay(&scope(), STREAM, &paused_then_resumed()).unwrap();
    assert_eq!(
        projection.simulation_status,
        Some(SimulationStatus::Running)
    );

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

fn resume_via_generation(first: &[EventEnvelope], second: &[EventEnvelope]) -> ExecutionProjection {
    let mut generation =
        ProjectionGeneration::new(scope(), STREAM.to_owned(), "execution".to_owned(), 1, 1)
            .unwrap();
    generation.apply_page(first).unwrap();
    generation.apply_page(second).unwrap();
    generation.projection().clone()
}

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

/// Emits `execution_started` followed by `n` `signal_recorded` events for distinct signals
/// sourced from node `"node-a"`.
fn signal_events(n: usize) -> Vec<EventEnvelope> {
    let execution_id = OpaqueId::parse("execution-test").unwrap();
    let mut new_events = vec![event(
        "execution-started",
        EventKind::ExecutionStarted(ExecutionStarted {
            execution_id: execution_id.clone(),
            graph_version: 1,
            graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
            mode: ExecutionMode::Supervised,
        }),
    )];
    for index in 0..n {
        new_events.push(event(
            format!("signal-{index}"),
            EventKind::SignalRecorded(SignalRecordedPayload {
                execution_id: execution_id.clone(),
                signal_id: OpaqueId::parse(format!("signal-{index}")).unwrap(),
                source_kind: SignalSourceKind::Node,
                source_id: OpaqueId::parse("node-a").unwrap(),
                kind: "no_progress".to_owned(),
                severity: SignalSeverity::Medium,
                envelope_sha256: RawSha256::parse("a".repeat(64)).unwrap(),
            }),
        ));
    }
    append(new_events)
}

/// A start followed by one `ghost_node_proposed` for `"ghost-a"`.
fn ghost_proposal_events() -> Vec<EventEnvelope> {
    let execution_id = OpaqueId::parse("execution-test").unwrap();
    append(vec![
        event(
            "execution-started",
            EventKind::ExecutionStarted(ExecutionStarted {
                execution_id: execution_id.clone(),
                graph_version: 1,
                graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
                mode: ExecutionMode::Supervised,
            }),
        ),
        event(
            "ghost-proposed",
            EventKind::GhostNodeProposed(GhostNodeProposed {
                execution_id,
                node_id: OpaqueId::parse("ghost-a").unwrap(),
                draft_id: OpaqueId::parse("draft-1").unwrap(),
            }),
        ),
    ])
}

/// A node that already carries a state, via a recorded outcome, before it is proposed as a
/// ghost. A ghost is born, not transitioned into, so this history cannot have happened.
fn ghost_proposal_over_existing_node() -> Vec<EventEnvelope> {
    let execution_id = OpaqueId::parse("execution-test").unwrap();
    let node_id = OpaqueId::parse("ghost-a").unwrap();
    let outcome = Outcome::Succeeded;
    let next_state = apply_transition(&TransitionRequest {
        current: precondition(outcome),
        outcome,
        attempts: 0,
        identical_outcomes: 0,
    })
    .unwrap();
    append(vec![
        event(
            "execution-started",
            EventKind::ExecutionStarted(ExecutionStarted {
                execution_id: execution_id.clone(),
                graph_version: 1,
                graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
                mode: ExecutionMode::Supervised,
            }),
        ),
        event(
            "outcome-ghost-a",
            EventKind::NodeOutcomeRecorded(NodeOutcomeRecorded {
                execution_id: execution_id.clone(),
                node_id: node_id.clone(),
                outcome,
                next_state,
            }),
        ),
        event(
            "ghost-proposed",
            EventKind::GhostNodeProposed(GhostNodeProposed {
                execution_id,
                node_id,
                draft_id: OpaqueId::parse("draft-1").unwrap(),
            }),
        ),
    ])
}

/// Started `Supervised`; the acceptance claims `Autopilot`. Mode binds at acceptance (5.5), so
/// this history cannot have happened.
fn acceptance_with_mismatched_mode() -> Vec<EventEnvelope> {
    let execution_id = OpaqueId::parse("execution-test").unwrap();
    append(vec![
        event(
            "execution-started",
            EventKind::ExecutionStarted(ExecutionStarted {
                execution_id: execution_id.clone(),
                graph_version: 1,
                graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
                mode: ExecutionMode::Supervised,
            }),
        ),
        event(
            "mutation-accepted",
            EventKind::MutationAccepted(MutationAccepted {
                execution_id,
                draft_id: OpaqueId::parse("draft-1").unwrap(),
                mode: ExecutionMode::Autopilot,
                graph_version: 2,
            }),
        ),
    ])
}

/// Started `Autopilot`; `n` acceptances under `Autopilot` with increasing `graph_version`.
fn acceptance_events(n: usize) -> Vec<EventEnvelope> {
    let execution_id = OpaqueId::parse("execution-test").unwrap();
    let mut new_events = vec![event(
        "execution-started",
        EventKind::ExecutionStarted(ExecutionStarted {
            execution_id: execution_id.clone(),
            graph_version: 1,
            graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
            mode: ExecutionMode::Autopilot,
        }),
    )];
    for index in 0..n {
        new_events.push(event(
            format!("mutation-accepted-{index}"),
            EventKind::MutationAccepted(MutationAccepted {
                execution_id: execution_id.clone(),
                draft_id: OpaqueId::parse(format!("draft-{index}")).unwrap(),
                mode: ExecutionMode::Autopilot,
                graph_version: index as u64 + 2,
            }),
        ));
    }
    append(new_events)
}

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
    assert_eq!(
        projection.node_states.get("ghost-a"),
        Some(&NodeState::Ghost)
    );
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
