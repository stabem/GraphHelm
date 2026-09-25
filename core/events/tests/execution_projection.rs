//! Execution state is a projection over execution events. These tests prove the counters are
//! derived from history rather than trusted from a payload, and that replay reproduces them.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use chrono::{TimeZone, Utc};
use graphhelm_events::{
    ClearanceOutcome, CustomsStage, ExecutionProjection, LocalEventRepository, PreparedAppend,
    ProjectionGeneration, ReplayError, replay,
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

/// A clock whose instant carries a FRACTION, which the canonical rendering keeps. Used by the
/// ordering guard: a horizon derived from it prints with `.500`, and `.` sorts before `Z`.
struct FractionalClock;
impl Clock for FractionalClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 10, 12, 0, 0).unwrap() + chrono::Duration::milliseconds(500)
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
    append_under(Arc::new(FixedClock), events)
}

fn append_under(clock: Arc<dyn Clock>, events: Vec<NewEvent>) -> Vec<EventEnvelope> {
    let directory = tempfile::tempdir().unwrap();
    let repository =
        LocalEventRepository::open(directory.path(), clock, Arc::new(Ids::default())).unwrap();
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
                reason: None,
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

/// A lease's `armed_at_sequence` is the ENVELOPE's sequence, and a re-arm moves it.
///
/// This is the property the wake sweep's discriminator rests on, asserted directly instead of
/// through a consumer. It exists because the guards that used to catch a broken fold caught it
/// BY ACCIDENT: their fixtures hand-wrote the number, so any wrong value disagreed with them and
/// they went red for a reason unrelated to what they claimed to test. Accidental coverage is a
/// coincidence wearing a guard's clothes; this is the same detection, by design and at the grain
/// of the property.
///
/// Both assertions compare against the envelope's OWN sequence rather than a literal. Against a
/// literal they would still pass for a fold that copied a cursor, or a constant that happened to
/// match, or an off-by-one that cancelled in this fixture.
#[test]
fn a_leases_armed_at_sequence_is_its_envelopes_and_a_re_arm_moves_it() {
    use graphhelm_protocols::WakeLease;

    let arm = |key: &str, rendezvous: &str| {
        NewEvent::new(
            OpaqueId::parse(key).unwrap(),
            actor(),
            Sensitivity::Internal,
            EventKind::WakeLease(WakeLease {
                execution_id: OpaqueId::parse("execution-test").unwrap(),
                session_id: OpaqueId::parse("session-fold").unwrap(),
                cursor: 0,
                rendezvous_id: OpaqueId::parse(rendezvous).unwrap(),
                matures_in_seconds: None,
            }),
            vec![],
            vec![],
        )
    };
    let committed = append(vec![arm("arm-first", "rdv-1"), arm("arm-second", "rdv-2")]);

    // The first arming alone: the lease carries THAT envelope's sequence.
    let first_only = replay(&scope(), STREAM, &committed[..1]).unwrap();
    assert_eq!(
        first_only
            .wake_leases
            .get("session-fold")
            .expect("the first arming is live")
            .armed_at_sequence,
        committed[0].sequence,
        "the fold must copy the arming event's own sequence, not a cursor or a constant"
    );

    // Both: the re-arm REPLACES, so the lease now carries the second envelope's sequence. That
    // replacement is what lets a stale capture be told from the lease that replaced it.
    let both = replay(&scope(), STREAM, &committed).unwrap();
    assert_eq!(
        both.wake_leases
            .get("session-fold")
            .expect("the re-armed lease is live")
            .armed_at_sequence,
        committed[1].sequence,
        "a re-arm must move it — if it did not, a capture from before the re-arm would still \
         match the lease that replaced it, which is the defect this discriminator exists for"
    );
}

/// A consumption naming an arming other than the one it burns is RECORDED, and replay SUCCEEDS.
///
/// This is the record-don't-refuse ruling as an executable statement. The fold refuses a
/// consumption with no live lease at all, because that log cannot be interpreted — replay
/// genuinely cannot build the next state from it. A consumption that burns the WRONG arming is a
/// different animal: the log is consistent and reconstructs exactly, it merely records a sweep
/// doing something bad. Refusing there would make history unreadable BECAUSE it recorded a
/// mistake, on a product whose thesis is that history reproduces — and it would take down the
/// glance at the moment an operator most needs it.
///
/// Built by hand-appending the mismatched history rather than by breaking the recorder. The
/// recorder will never produce this once the discriminator is in; the FOLD's contract is what is
/// under test, so the fixture states that contract directly and keeps working if the recorder is
/// rewritten.
#[test]
fn a_consumption_that_burns_the_wrong_arming_is_recorded_and_replay_still_succeeds() {
    use graphhelm_protocols::{WakeConsumeReason, WakeLease, WakeLeaseConsumed};

    let arm = |key: &str, rendezvous: &str| {
        NewEvent::new(
            OpaqueId::parse(key).unwrap(),
            actor(),
            Sensitivity::Internal,
            EventKind::WakeLease(WakeLease {
                execution_id: OpaqueId::parse("execution-test").unwrap(),
                session_id: OpaqueId::parse("session-mis").unwrap(),
                cursor: 0,
                rendezvous_id: OpaqueId::parse(rendezvous).unwrap(),
                matures_in_seconds: None,
            }),
            vec![],
            vec![],
        )
    };
    // Two armings, then a consumption naming the FIRST while the SECOND is live.
    let committed = append(vec![
        arm("mis-first", "rdv-a"),
        arm("mis-second", "rdv-b"),
        NewEvent::new(
            OpaqueId::parse("mis-consume").unwrap(),
            actor(),
            Sensitivity::Internal,
            EventKind::WakeLeaseConsumed(WakeLeaseConsumed {
                execution_id: OpaqueId::parse("execution-test").unwrap(),
                session_id: OpaqueId::parse("session-mis").unwrap(),
                reason: WakeConsumeReason::Rung,
                captured_arming: Some(1),
            }),
            vec![],
            vec![],
        ),
    ]);

    // REPLAY SUCCEEDS. If this ever refuses, the fold has started punishing a legal log.
    let projection = replay(&scope(), STREAM, &committed)
        .expect("burning the wrong arming is legal history — the fold must not refuse it");

    let mis = projection
        .wake_mis_burns
        .get("session-mis")
        .expect("the mismatch must be recorded where the attention predicate can reach it");
    assert_eq!(
        mis.captured_arming, committed[0].sequence,
        "the record names the arming the sweep CAPTURED"
    );
    assert_eq!(
        mis.live_arming, committed[1].sequence,
        "and the arming that was actually live and got burned — the PAIR is the diagnosis, \
         which is why a bare flag would not do"
    );
    assert_eq!(
        mis.at_sequence, committed[2].sequence,
        "and the consumption that did it"
    );

    // The lease is still gone: recording the mistake does not undo it. The sleeper is stranded,
    // which is the condition attention has to surface.
    assert!(
        !projection.wake_leases.contains_key("session-mis"),
        "the burn still happened — this records it, it does not prevent it"
    );
}

/// A consumption that names the arming it actually burns records NOTHING. Absent means absent.
#[test]
fn a_matching_consumption_and_a_pre_change_one_record_no_mis_burn() {
    use graphhelm_protocols::{WakeConsumeReason, WakeLease, WakeLeaseConsumed};

    let arm = |key: &str, session: &str| {
        NewEvent::new(
            OpaqueId::parse(key).unwrap(),
            actor(),
            Sensitivity::Internal,
            EventKind::WakeLease(WakeLease {
                execution_id: OpaqueId::parse("execution-test").unwrap(),
                session_id: OpaqueId::parse(session).unwrap(),
                cursor: 0,
                rendezvous_id: OpaqueId::parse("rdv-ok").unwrap(),
                matures_in_seconds: None,
            }),
            vec![],
            vec![],
        )
    };
    let consume = |key: &str, session: &str, captured: Option<u64>| {
        NewEvent::new(
            OpaqueId::parse(key).unwrap(),
            actor(),
            Sensitivity::Internal,
            EventKind::WakeLeaseConsumed(WakeLeaseConsumed {
                execution_id: OpaqueId::parse("execution-test").unwrap(),
                session_id: OpaqueId::parse(session).unwrap(),
                reason: WakeConsumeReason::Rung,
                captured_arming: captured,
            }),
            vec![],
            vec![],
        )
    };
    let committed = append(vec![
        arm("ok-arm", "session-ok"),
        arm("old-arm", "session-old"),
        // Names the arming it burns.
        consume("ok-consume", "session-ok", Some(1)),
        // Written before the field existed: absent, and absence is not a mismatch.
        consume("old-consume", "session-old", None),
    ]);
    let projection = replay(&scope(), STREAM, &committed).unwrap();

    assert!(
        projection.wake_mis_burns.is_empty(),
        "a matching consumption and a pre-change one are both clean — inventing a mismatch \
         from an absent field would make every committed consumption look like a defect: {:?}",
        projection.wake_mis_burns
    );

    // THE BRICK-GATE: an absent captured arming emits NO KEY, never a null.
    //
    // Every event is re-hashed on replay, so a consumption written before this field existed
    // must serialize to exactly the bytes it did then. A `"capturedArming": null` would change
    // those bytes and break the hash chain of every consumption already committed — the whole
    // stream unreadable, for a field nobody set.
    //
    // THIS ASSERTION IS REDUNDANT TODAY, and that is recorded rather than hidden. Measured:
    // removing `skip_serializing_if` fells four tests in this file, EVERY ONE AT ITS OWN
    // `append` (execution_projection.rs:96:40), because the schema types this field as an
    // integer and validation rejects a null before anything reaches here. The schema is the
    // live guard, and under that sabotage this line never executes.
    //
    // It is kept because redundant-today is not redundant-permanently: `{"type": "integer"}`
    // is one edit from being relaxed, and the day it is, this becomes a single-sabotage blade
    // with nothing else behind it. A guard whose redundancy is written down survives the change
    // that makes it necessary again; a removed one does not. It blinds nothing meanwhile — it
    // sits downstream of the schema and is simply unreached while the schema holds.
    //
    // ITS OWN SABOTAGE NEEDS TWO EDITS — loosen the schema AND remove the skip — and HAS NOT
    // BEEN RUN. Unobserved, and saying so is the point.
    let pre_change = serde_json::to_value(
        &committed
            .iter()
            .find(|event| event.idempotency_key.as_str() == "old-consume")
            .expect("the pre-change consumption is in the batch")
            .kind,
    )
    .unwrap();
    assert!(
        pre_change["data"].get("capturedArming").is_none(),
        "an absent captured arming must not appear on the wire at all: {pre_change}"
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
                reason: None,
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

/// Task 9b (05c): the `ReuseDecision` kind is ledger, not state — a stream carrying one must
/// replay to a projection byte-identical to the same stream without it, and replay twice must
/// stay byte-identical (replay stability for the new kind). The explicit no-op fold arm is
/// what this pins: a wildcard would pass this test too, but the arm's absence (a state change
/// smuggled in later) fails it loudly.
#[test]
fn a_reuse_decision_is_ledger_not_state_and_replays_stably() {
    use graphhelm_protocols::{
        ForcedFreshReason, FreshnessClass, ReuseDecision, ReuseKeyComponent, ReuseOutcome,
        ReusePlane, WireHash,
    };
    let base = vec![event(
        "start-1",
        EventKind::ExecutionStarted(ExecutionStarted {
            execution_id: OpaqueId::parse("execution-test").unwrap(),
            graph_version: 1,
            graph_hash: WireHash::parse(
                "sha256:0000000000000000000000000000000000000000000000000000000000000000",
            )
            .unwrap(),
            mode: ExecutionMode::Autopilot,
        }),
    )];
    let with_ledger = {
        let mut events = base.clone();
        events.push(event(
            "reuse-1",
            EventKind::ReuseDecision(ReuseDecision {
                execution_id: OpaqueId::parse("execution-test").unwrap(),
                node_id: None,
                plane: ReusePlane::ToolBroker,
                decision: ReuseOutcome::ForcedFresh,
                forced_reason: Some(ForcedFreshReason::DirtyTree),
                freshness_class: Some(FreshnessClass::SnapshotClosed),
                key_components: vec![
                    ReuseKeyComponent::ToolVersion,
                    ReuseKeyComponent::CanonicalInput,
                    ReuseKeyComponent::LeaseScope,
                    ReuseKeyComponent::SourceSnapshot,
                ],
                key_digest: WireHash::parse(
                    "sha256:1111111111111111111111111111111111111111111111111111111111111111",
                )
                .unwrap(),
                evidence_ref: None,
                provenance_erased: false,
            }),
        ));
        events
    };

    let plain = append(base);
    let ledgered = append(with_ledger);
    let projection_plain = replay(&scope(), STREAM, &plain).unwrap();
    let projection_ledgered = replay(&scope(), STREAM, &ledgered).unwrap();
    assert_eq!(
        serde_json::to_vec(&projection_plain).unwrap(),
        serde_json::to_vec(&projection_ledgered).unwrap(),
        "a reuse decision must change no projection state"
    );
    let replayed_again = replay(&scope(), STREAM, &ledgered).unwrap();
    assert_eq!(
        serde_json::to_vec(&projection_ledgered).unwrap(),
        serde_json::to_vec(&replayed_again).unwrap(),
        "replay of a ledgered stream must be byte-identical across runs"
    );

    // And the payload round-trips through the wire representation exactly.
    let envelope = ledgered.last().unwrap();
    let wire = serde_json::to_string(envelope).unwrap();
    let back: EventEnvelope = serde_json::from_str(&wire).unwrap();
    assert_eq!(
        *envelope, back,
        "ReuseDecision must round-trip byte-exactly"
    );
}

/// 05g Task 1: the one-live-lease invariant. Arming twice REPLACES (never stacks — the
/// anti-fork-bomb rule as a fold fact), consumption removes, and the whole lease ledger
/// round-trips the wire byte-exactly.
#[test]
fn arming_twice_replaces_the_lease_and_consumption_burns_it() {
    use graphhelm_protocols::{WakeConsumeReason, WakeLease, WakeLeaseConsumed};

    let mut events = vec![event(
        "start-1",
        EventKind::ExecutionStarted(ExecutionStarted {
            execution_id: OpaqueId::parse("execution-test").unwrap(),
            graph_version: 1,
            graph_hash: WireHash::parse(
                "sha256:0000000000000000000000000000000000000000000000000000000000000000",
            )
            .unwrap(),
            mode: ExecutionMode::Autopilot,
        }),
    )];
    let lease = |suffix: &str, cursor: u64, rendezvous: &str| {
        event(
            suffix,
            EventKind::WakeLease(WakeLease {
                execution_id: OpaqueId::parse("execution-test").unwrap(),
                session_id: OpaqueId::parse("session-a").unwrap(),
                cursor,
                rendezvous_id: OpaqueId::parse(rendezvous).unwrap(),
                matures_in_seconds: None,
            }),
        )
    };
    events.push(lease("lease-1", 1, "rdv-first"));
    events.push(lease("lease-2", 2, "rdv-second"));

    let armed = append(events.clone());
    let projection = replay(&scope(), STREAM, &armed).unwrap();
    assert_eq!(
        projection.wake_leases.len(),
        1,
        "one live lease per session, exactly — arming twice must replace, never stack"
    );
    let live = &projection.wake_leases["session-a"];
    assert_eq!(live.cursor, 2, "the replacement wins");
    assert_eq!(live.rendezvous_id, "rdv-second");

    events.push(event(
        "consume-1",
        EventKind::WakeLeaseConsumed(WakeLeaseConsumed {
            execution_id: OpaqueId::parse("execution-test").unwrap(),
            session_id: OpaqueId::parse("session-a").unwrap(),
            reason: WakeConsumeReason::Rung,
            captured_arming: None,
        }),
    ));
    let burned = append(events);
    let projection = replay(&scope(), STREAM, &burned).unwrap();
    assert!(
        projection.wake_leases.is_empty(),
        "consumption burns the lease"
    );

    // The payloads round-trip the wire byte-exactly.
    for envelope in burned.iter().rev().take(2) {
        let wire = serde_json::to_string(envelope).unwrap();
        let back: EventEnvelope = serde_json::from_str(&wire).unwrap();
        assert_eq!(*envelope, back, "wake kinds must round-trip byte-exactly");
    }
}

/// 05g Task 1: history cannot burn a lease that was never armed — a consumption with no
/// matching live lease is a replay integrity refusal, not a silent no-op.
#[test]
fn consuming_an_unarmed_lease_is_a_replay_integrity_refusal() {
    use graphhelm_protocols::{WakeConsumeReason, WakeLeaseConsumed};

    let events = append(vec![
        event(
            "start-1",
            EventKind::ExecutionStarted(ExecutionStarted {
                execution_id: OpaqueId::parse("execution-test").unwrap(),
                graph_version: 1,
                graph_hash: WireHash::parse(
                    "sha256:0000000000000000000000000000000000000000000000000000000000000000",
                )
                .unwrap(),
                mode: ExecutionMode::Autopilot,
            }),
        ),
        event(
            "consume-ghost",
            EventKind::WakeLeaseConsumed(WakeLeaseConsumed {
                execution_id: OpaqueId::parse("execution-test").unwrap(),
                session_id: OpaqueId::parse("session-ghost").unwrap(),
                reason: WakeConsumeReason::StaleRendezvous,
                captured_arming: None,
            }),
        ),
    ]);
    let refused = replay(&scope(), STREAM, &events);
    assert!(
        matches!(refused, Err(graphhelm_events::ReplayError::Corrupt)),
        "an unmatched consumption must refuse the replay: {refused:?}"
    );
}

/// 05g Task 1: a replay NEVER rings — pinned at the source: the fold crate speaks no pipe,
/// socket or async-net vocabulary at all. The ringer is the serve layer's post-append hook,
/// and the day this invariant breaks is the day a replay develops side effects.
#[test]
fn the_fold_crate_speaks_no_transport_vocabulary() {
    let source_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut checked = 0;
    for entry in std::fs::read_dir(&source_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|ext| ext != "rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for token in [
            "named_pipe",
            "UnixListener",
            "UnixStream",
            "tokio::net",
            "TcpStream",
        ] {
            assert!(
                !text.contains(token),
                "{} must not mention {token}: the fold is pure and a replay never rings",
                path.display()
            );
        }
        checked += 1;
    }
    assert!(checked > 3, "the invariant walked the real sources");
}

/// M06 Task 1: the verdict is ledger, not state; the certification is state the Task 4
/// precondition reads; both kinds round-trip byte-exactly through the REAL repository —
/// which also proves both envelope oneOf lists (the 05g lesson: a kind absent from the
/// root kind↔scope pairing dies at append with "exactly one required schema").
#[test]
fn gate_verdict_is_ledger_and_certification_is_replayable_state() {
    use graphhelm_protocols::{GateCertified, GateFinding, GateVerdict, SignalSeverity, WireHash};
    let started = event(
        "start-1",
        EventKind::ExecutionStarted(ExecutionStarted {
            execution_id: OpaqueId::parse("execution-test").unwrap(),
            graph_version: 1,
            graph_hash: WireHash::parse(
                "sha256:0000000000000000000000000000000000000000000000000000000000000000",
            )
            .unwrap(),
            mode: ExecutionMode::Autopilot,
        }),
    );
    let base = vec![started];
    let with_gates = {
        let mut events = base.clone();
        events.push(event(
            "certified-1",
            EventKind::GateCertified(GateCertified {
                execution_id: OpaqueId::parse("execution-test").unwrap(),
                gate_id: OpaqueId::parse("gate-layout").unwrap(),
                suite_digest: WireHash::parse(
                    "sha256:2222222222222222222222222222222222222222222222222222222222222222",
                )
                .unwrap(),
                specimens: 10,
            }),
        ));
        events.push(event(
            "verdict-1",
            EventKind::GateVerdict(GateVerdict {
                execution_id: OpaqueId::parse("execution-test").unwrap(),
                node_id: OpaqueId::parse("check-ui").unwrap(),
                gate_id: OpaqueId::parse("gate-layout").unwrap(),
                passed: false,
                findings: vec![GateFinding {
                    severity: SignalSeverity::High,
                    claim: "the triage section rendered empty on a populated projection".to_owned(),
                    evidence: vec![OpaqueId::parse("evidence-1").unwrap()],
                    remediation: "wire the untriaged list into the section renderer".to_owned(),
                }],
            }),
        ));
        events
    };

    let plain = append(base);
    let gated = append(with_gates);
    let projection_plain = replay(&scope(), STREAM, &plain).unwrap();
    let projection_gated = replay(&scope(), STREAM, &gated).unwrap();

    // The certification IS state, read by Task 4's precondition.
    assert_eq!(
        projection_gated.gate_certifications.get("gate-layout"),
        Some(&"sha256:2222222222222222222222222222222222222222222222222222222222222222".to_owned()),
        "the fold records the certification per gate"
    );

    // The verdict is ledger, not state: strip the certification difference and the node
    // world is untouched.
    assert_eq!(
        projection_plain.node_states, projection_gated.node_states,
        "a verdict changes no node state"
    );
    assert_eq!(
        projection_plain.signals_recorded,
        projection_gated.signals_recorded
    );

    // Replay-stable, byte-identical across runs.
    let again = replay(&scope(), STREAM, &gated).unwrap();
    assert_eq!(
        serde_json::to_vec(&projection_gated).unwrap(),
        serde_json::to_vec(&again).unwrap()
    );

    // Both payloads round-trip the wire exactly.
    for envelope in gated.iter().rev().take(2) {
        let wire = serde_json::to_string(envelope).unwrap();
        let back: EventEnvelope = serde_json::from_str(&wire).unwrap();
        assert_eq!(*envelope, back, "gate kinds must round-trip byte-exactly");
    }
}

/// M06 Task 1, binding decision 2 IN THE SCHEMA: a failing verdict with an empty findings
/// list is invalid ON THE WIRE — a bare fail cannot exist. A passing verdict with empty
/// findings stays valid (refusal-with-findings binds refusals).
#[test]
fn a_bare_failing_verdict_is_schema_invalid_by_construction() {
    let set = graphhelm_schema::repository_schema_set().expect("schema set loads");
    let envelope = |passed: bool, findings: serde_json::Value| {
        serde_json::json!({
            "schemaVersion": "1.0.0",
            "eventId": "event-x",
            "scope": {"workspaceId": "workspace-test", "projectId": "project-test",
                       "executionId": "execution-test"},
            "streamId": "execution-test", "sequence": 1,
            "occurredAt": "2026-08-16T12:00:00Z", "idempotencyKey": "verdict-schema-probe",
            "actor": {"type": "system", "id": "system-test"}, "sensitivity": "internal",
            "kind": {"type": "gate_verdict", "data": {
                "executionId": "execution-test", "nodeId": "check-ui",
                "gateId": "gate-layout", "passed": passed, "findings": findings}},
            "evidenceRefs": [], "artifactRefs": [],
            "previousHash":
                "sha256:35c8ab0717bef1684ad07efcf3bedd4648c778a2c944cbd2c7e6a4802e2237b3",
            "eventHash":
                "sha256:35c8ab0717bef1684ad07efcf3bedd4648c778a2c944cbd2c7e6a4802e2237b3"
        })
    };
    let finding = serde_json::json!([{ "severity": "high",
        "claim": "c", "evidence": [], "remediation": "r" }]);

    assert!(
        !set.validate_event(&envelope(false, serde_json::json!([])))
            .is_empty(),
        "a failing verdict with no findings must be refused by the schema itself"
    );
    assert!(
        set.validate_event(&envelope(false, finding.clone()))
            .is_empty(),
        "a failing verdict WITH findings is the valid shape"
    );
    assert!(
        set.validate_event(&envelope(true, serde_json::json!([])))
            .is_empty(),
        "a pass may carry no findings — the rule binds refusals"
    );
}

/// M07 F4: the alarm answers its OWN question. A burned lease used to vanish without a
/// trace, so `wake_status` could say "not live" but never "it rang at #N" — the judge's
/// exact complaint (an operator cannot tell a fired alarm from one that never armed). The
/// fold now keeps the LAST consumption per session, additively.
#[test]
fn a_burned_lease_leaves_its_receipt_behind() {
    use graphhelm_protocols::{WakeConsumeReason, WakeLease, WakeLeaseConsumed};

    let mut events = vec![event(
        "start-1",
        EventKind::ExecutionStarted(ExecutionStarted {
            execution_id: OpaqueId::parse("execution-test").unwrap(),
            graph_version: 1,
            graph_hash: WireHash::parse(
                "sha256:0000000000000000000000000000000000000000000000000000000000000000",
            )
            .unwrap(),
            mode: ExecutionMode::Autopilot,
        }),
    )];
    let lease = |suffix: &str, cursor: u64| {
        event(
            suffix,
            EventKind::WakeLease(WakeLease {
                execution_id: OpaqueId::parse("execution-test").unwrap(),
                session_id: OpaqueId::parse("session-a").unwrap(),
                cursor,
                rendezvous_id: OpaqueId::parse("rdv-one").unwrap(),
                matures_in_seconds: None,
            }),
        )
    };
    let consume = |suffix: &str, reason: WakeConsumeReason| {
        event(
            suffix,
            EventKind::WakeLeaseConsumed(WakeLeaseConsumed {
                execution_id: OpaqueId::parse("execution-test").unwrap(),
                session_id: OpaqueId::parse("session-a").unwrap(),
                reason,
                captured_arming: None,
            }),
        )
    };

    // Nothing consumed yet: no receipt to show.
    events.push(lease("lease-1", 1));
    let armed = replay(&scope(), STREAM, &append(events.clone())).unwrap();
    assert!(
        armed.wake_last_consumed.is_empty(),
        "an armed-but-never-burned lease has no receipt"
    );

    // Rung: the receipt names the reason AND the sequence the burn landed at, which is
    // what lets a woken sleeper say "my alarm rang at #N".
    events.push(consume("consume-1", WakeConsumeReason::Rung));
    let rung = replay(&scope(), STREAM, &append(events.clone())).unwrap();
    assert!(
        rung.wake_leases.is_empty(),
        "consumption still burns the lease"
    );
    let receipt = rung
        .wake_last_consumed
        .get("session-a")
        .expect("the burn leaves a receipt");
    assert_eq!(receipt.reason, WakeConsumeReason::Rung);
    assert_eq!(
        receipt.sequence, 3,
        "the receipt carries the sequence of the consumption event itself"
    );

    // Re-armed and burned as stale: the LAST consumption wins, so the answer is never a
    // stale one from an older cycle.
    events.push(lease("lease-2", 3));
    events.push(consume("consume-2", WakeConsumeReason::StaleRendezvous));
    let stale = replay(&scope(), STREAM, &append(events)).unwrap();
    let receipt = stale
        .wake_last_consumed
        .get("session-a")
        .expect("the second burn replaces the first");
    assert_eq!(receipt.reason, WakeConsumeReason::StaleRendezvous);
    assert_eq!(receipt.sequence, 5);
}

// -------------------------------------------------------------------------------------------
// M09 decision B, guard 1 — written BEFORE the field it needs, and before any schema.
//
// The alarm's horizon is an instant STORED at arming. The danger is not storing it; it is that
// the first convenient refactor caches whether it has PASSED, because `matured` is what every
// caller actually wants. A projection that answers that question is a function of the wall
// clock, and byte-identical replay dies silently — on the machines whose clock crossed the
// horizon mid-replay, and nowhere else.
//
// So the invariant is MECHANICAL, not a rule anyone has to remember: the horizon is stored and
// never evaluated below the surface boundary. Two logs identical except for a horizon far in
// the past and one far in the future must fold to projections that differ ONLY in that stored
// value. Anything clock-derived splits them apart, because one side is mature and the other is
// not.
// -------------------------------------------------------------------------------------------

use graphhelm_protocols::WakeLease;

/// The fixture clock stands at 2026-08-10T12:00:00Z, so one second is long past and the
/// schema's ten-year ceiling is far ahead. The two sides of the guard are a MATURED horizon
/// and an unmatured one, which is what a clock-reading fold would answer differently about.
const SHORT_BOUND: u64 = 1;
const LONG_BOUND: u64 = 315_576_000;

fn lease_with_horizon(seconds: u64) -> Vec<EventEnvelope> {
    append(lease_events(seconds))
}

fn lease_events(seconds: u64) -> Vec<NewEvent> {
    vec![
        event(
            "execution-started",
            EventKind::ExecutionStarted(ExecutionStarted {
                execution_id: OpaqueId::parse("execution-test").unwrap(),
                graph_version: 1,
                graph_hash: WireHash::parse(
                    "sha256:0000000000000000000000000000000000000000000000000000000000000000",
                )
                .unwrap(),
                mode: ExecutionMode::Autopilot,
            }),
        ),
        event(
            "lease-armed",
            EventKind::WakeLease(WakeLease {
                execution_id: OpaqueId::parse("execution-test").unwrap(),
                session_id: OpaqueId::parse("session-a").unwrap(),
                cursor: 1,
                rendezvous_id: OpaqueId::parse("rdv-one").unwrap(),
                matures_in_seconds: Some(seconds),
            }),
        ),
    ]
}

/// The horizon is DATA the fold carries, never a question the fold answers.
#[test]
fn a_horizon_in_the_past_folds_exactly_like_one_in_the_future() {
    let past = replay(&scope(), STREAM, &lease_with_horizon(SHORT_BOUND)).unwrap();
    let future = replay(&scope(), STREAM, &lease_with_horizon(LONG_BOUND)).unwrap();

    let past = serde_json::to_string(&past).unwrap();
    let future = serde_json::to_string(&future).unwrap();

    // Without this, the guard passes by COINCIDENCE: a projection that stores no horizon at
    // all makes both sides identical and the substitution below a no-op. The equality is only
    // evidence once there is something for it to be evidence ABOUT.
    assert!(
        past.contains("2026-08-10T12:00:01Z"),
        "the projection must carry the horizon the record implies: {past}"
    );

    // Substituting ONE known literal — not a normalising regex. The lesson of the day is that a
    // normalisation can merge what differs and split what matches; replacing an exact string
    // this test itself chose can do neither.
    assert_eq!(
        past.replace("2026-08-10T12:00:01Z", "<HORIZON>"),
        future.replace("2036-08-10T00:00:00Z", "<HORIZON>"),
        "the fold answered a question about the clock: the two projections differ by more than \
         the horizon each one stores"
    );
    assert!(
        !past.contains("matured"),
        "maturity is derived at the surface with an injected instant, never cached in the \
         projection: {past}"
    );
}

/// The stored horizon must ORDER chronologically, which the wire string does not.
///
/// The reviewer measured it: the canonical rendering carries 0, 3, 6 or 9 fractional digits
/// depending on the instant, so `...:01.500Z` sorts BEFORE `...:01Z` — `.` is 0x2E and `Z` is
/// 0x5A. Half a second later compares as earlier. The pair is not exotic: the horizon inherits
/// the fraction of the instant the arming event was stamped with, so any real clock produces
/// one side of it and a round one produces the other.
///
/// Sabotage: hold the wire string in `WakeLeaseState` again. This falls, because the assertion
/// is about ORDER and a string answers it backwards.
#[test]
fn a_horizon_half_a_second_later_is_stored_as_later() {
    let earlier = replay(&scope(), STREAM, &lease_with_horizon(1)).unwrap();
    let later = replay(
        &scope(),
        STREAM,
        &append_under(Arc::new(FractionalClock), lease_events(1)),
    )
    .unwrap();

    let earlier = earlier.wake_leases.get("session-a").unwrap();
    let later = later.wake_leases.get("session-a").unwrap();
    assert!(
        later.matures_at > earlier.matures_at,
        "half a second later must STORE as later: {:?} vs {:?}",
        later.matures_at,
        earlier.matures_at
    );
}

// -------------------------------------------------------------------------------------------
// M11 #160: the customs pipeline's fold-grain guards.
//
// EVERY fixture here appends ONE batch. That is not a style preference: `append()` opens a fresh
// repository and always asks for `expected_next_sequence = 1`, so two calls produce two events
// numbered 1 and a journal no real stream could contain. A guard whose arrangement cannot exist
// proves nothing about the product, and would break the moment anything validated sequence
// continuity — while being wrong about ITSELF, not about the code. (Found by J reading this file
// for #161, whose own cells are ORDERINGS and therefore could not be expressed at all under
// restarting sequences.)
//
// Sequences are PREDICTED from batch position and then CHECKED against the appended envelopes, so
// the prediction is an assertion rather than an assumption.
// -------------------------------------------------------------------------------------------

const CUSTOMS_NODE: &str = "implementation";

/// Minimal two-node chain: `implementation -> deploy`. Enough to ask readiness a real question.
fn customs_spec() -> graphhelm_protocols::GraphSpec {
    use graphhelm_protocols::{EdgeType, GraphEdge, GraphNode, GraphSpec, NodeType, Optionality};
    let node = || GraphNode {
        node_type: NodeType::Agent,
        name: "n".to_owned(),
        objective: "o".to_owned(),
        optionality: Optionality::Required,
        properties: std::collections::BTreeMap::new(),
    };
    let mut nodes = std::collections::BTreeMap::new();
    nodes.insert(CUSTOMS_NODE.to_owned(), node());
    nodes.insert("deploy".to_owned(), node());
    GraphSpec {
        entrypoints: vec![CUSTOMS_NODE.to_owned()],
        nodes,
        edges: vec![GraphEdge {
            id: "implementation-to-deploy".to_owned(),
            from: CUSTOMS_NODE.to_owned(),
            to: "deploy".to_owned(),
            edge_type: EdgeType::Control,
            payload_schema: None,
            condition: None,
            on_false: None,
            on_unknown: None,
            priority: None,
            bindings: std::collections::BTreeMap::new(),
        }],
        budgets: Default::default(),
        policies: Vec::new(),
        completion: serde_json::Value::Null,
    }
}

fn outcome_event(key: &str, outcome: Outcome, next_state: NodeState) -> NewEvent {
    event(
        key.to_owned(),
        EventKind::NodeOutcomeRecorded(NodeOutcomeRecorded {
            execution_id: OpaqueId::parse("execution-test").unwrap(),
            node_id: OpaqueId::parse(CUSTOMS_NODE).unwrap(),
            outcome,
            next_state,
            reason: None,
        }),
    )
}

fn claim_event(key: &str, completes_wait_seq: u64) -> NewEvent {
    event(
        key.to_owned(),
        EventKind::CompletionClaimed(graphhelm_protocols::CompletionClaimed {
            execution_id: OpaqueId::parse("execution-test").unwrap(),
            node: OpaqueId::parse(CUSTOMS_NODE).unwrap(),
            completes_wait_seq,
            evidence: vec![],
            attestation: graphhelm_protocols::ClaimAttestation {
                asserter: OpaqueId::parse("agent-claimer").unwrap(),
                mode: graphhelm_protocols::ClaimAttestationMode::OperatorAttested,
            },
        }),
    )
}

/// Drives `implementation` to a parked wait: execution start, dispatch, run, park.
///
/// Returns the batch so the caller can keep stacking events into the SAME append. The parking
/// event is the last entry, so its sequence is the batch's length — predicted here, checked by
/// [`sequence_of`] once the batch is appended.
fn parked_batch() -> Vec<NewEvent> {
    vec![
        event(
            "execution-started",
            EventKind::ExecutionStarted(ExecutionStarted {
                execution_id: OpaqueId::parse("execution-test").unwrap(),
                graph_version: 1,
                graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
                mode: ExecutionMode::Supervised,
            }),
        ),
        outcome_event("dispatch", Outcome::Started, NodeState::Queued),
        outcome_event("run", Outcome::Started, NodeState::Running),
        outcome_event("park", Outcome::NeedsInput, NodeState::WaitingInput),
    ]
}

/// The projection's node states with `deploy` set to `Ready`, so `ready_set` is asked a question
/// about the EDGE rather than about `deploy`'s own state.
///
/// This exists because the negative half of the guard below was passing for the wrong reason.
/// `is_dispatchable` is `Ready`-only by design, and a node with no recorded state folds to
/// `Draft` — so `deploy` was absent from the ready set no matter what `implementation` did, and
/// an assertion that would have held with the quarantine deleted is not a guard, it is decoration.
/// Marking `deploy` dispatchable removes its own state as an explanation and leaves exactly one:
/// the predecessor edge. The positive half then proves the same call releases it once cleared,
/// which is what makes the pair measure the transition instead of the default.
fn dispatchable_deploy(
    projection: &graphhelm_events::ExecutionProjection,
) -> std::collections::BTreeMap<String, NodeState> {
    let mut states = projection.node_states.clone();
    states.insert("deploy".to_owned(), NodeState::Ready);
    states
}

/// The sequence of the event at `index`, CHECKED rather than assumed: a batch appended from a
/// fresh repository numbers its events 1..n in order, and this asserts that held before any test
/// depends on it.
fn sequence_of(appended: &[EventEnvelope], index: usize) -> u64 {
    let sequence = appended[index].sequence;
    assert_eq!(
        sequence,
        index as u64 + 1,
        "batch position {index} did not become sequence {}: the fixture's whole identity model \
         rests on this",
        index + 1
    );
    sequence
}

/// THE TRAP GUARD (#159 sealed, #160 lane): a claim addressed to a SUPERSEDED wait answers
/// nothing and leaves the node parked.
///
/// The arrangement is reachable with today's events alone — `(WaitingInput, NeedsInput) ->
/// WaitingInput` is the state machine's own arm, so a node re-parks and its first wait is
/// superseded. That is what makes this fixture constructable BEFORE the fix, which is the
/// precondition the house rule demands of a trap guard.
#[test]
fn a_claim_naming_a_superseded_wait_answers_nothing_and_leaves_the_node_parked() {
    let mut batch = parked_batch();
    let first_wait_index = batch.len() - 1;
    // Re-park: the SECOND wait supersedes the first under the same node name.
    batch.push(outcome_event(
        "repark",
        Outcome::NeedsInput,
        NodeState::WaitingInput,
    ));
    let second_wait_index = batch.len() - 1;
    // The stale claim names the FIRST wait, which is no longer the open one.
    batch.push(claim_event("claim-stale", first_wait_index as u64 + 1));

    let appended = append(batch);
    let first_wait = sequence_of(&appended, first_wait_index);
    let second_wait = sequence_of(&appended, second_wait_index);
    assert_ne!(
        first_wait, second_wait,
        "the arrangement must produce two DISTINCT waits or it is not the trap"
    );

    let projection = replay(&scope(), STREAM, &appended).unwrap();
    assert!(
        projection.open_claims.is_empty(),
        "a claim against a superseded wait must mint NO open claim: {:?}",
        projection.open_claims
    );
    assert_eq!(
        projection.node_states.get(CUSTOMS_NODE),
        Some(&NodeState::WaitingInput),
        "the node must stay parked"
    );
    assert_eq!(
        projection
            .open_waits
            .get(CUSTOMS_NODE)
            .map(|wait| wait.at_sequence),
        Some(second_wait),
        "the OPEN wait is still the second one, untouched by the stale claim"
    );
}

/// THE FALSE-READY CELL (#159 sealed): a CLAIMED-not-cleared wait releases nothing downstream;
/// the clearance, and only the clearance, does.
///
/// Note what the assertion does NOT do: it never inspects a customs field to decide readiness. It
/// asks `ready_set` — the same derivation both drivers call, signature unchanged — so a green here
/// means both call sites inherit the behaviour rather than one of them being taught it.
#[test]
fn the_downstream_of_a_claimed_wait_is_not_ready_until_the_claim_clears() {
    let spec = customs_spec();

    // First batch: park, then claim the OPEN wait. Claimed-not-cleared is the quarantine.
    let mut batch = parked_batch();
    let wait_index = batch.len() - 1;
    batch.push(claim_event("claim-open", wait_index as u64 + 1));
    let claim_index = batch.len() - 1;

    let claimed_events = append(batch.clone());
    let _wait_seq = sequence_of(&claimed_events, wait_index);
    let claim_seq = sequence_of(&claimed_events, claim_index);

    let claimed = replay(&scope(), STREAM, &claimed_events).unwrap();
    assert_eq!(
        claimed.node_states.get(CUSTOMS_NODE),
        Some(&NodeState::WaitingInput),
        "a claim is testimony, not a transition"
    );
    assert!(
        !graphhelm_execution::ready_set(&spec, &dispatchable_deploy(&claimed))
            .unwrap()
            .contains("deploy"),
        "downstream of a CLAIMED-not-cleared wait must not be ready — this is the quarantine"
    );

    // Same arrangement, one event longer: the countersignature. Appended as ONE batch again, so
    // the clearance's `claim_seq` names a sequence that genuinely is the claim's.
    batch.push(event(
        "clearance",
        EventKind::CompletionCleared(graphhelm_protocols::CompletionCleared {
            execution_id: OpaqueId::parse("execution-test").unwrap(),
            claim_seq,
            verifier: graphhelm_protocols::ClearanceVerifier::MachineReplay {
                // #161: the digest of the EMPTY bundle, because `claim_event` journals
                // `evidence: vec![]`. Was an arbitrary literal while nothing read the field.
                manifest_hash: graphhelm_events::claim_evidence_digest(&[]),
            },
        }),
    ));
    let cleared_events = append(batch);
    assert_eq!(
        sequence_of(&cleared_events, claim_index),
        claim_seq,
        "the claim kept its sequence when the batch grew, or the clearance names the wrong event"
    );

    let cleared = replay(&scope(), STREAM, &cleared_events).unwrap();
    assert_eq!(
        cleared.node_states.get(CUSTOMS_NODE),
        Some(&NodeState::Succeeded),
        "clearance is the release"
    );
    assert!(
        cleared.open_claims.is_empty() && cleared.open_waits.is_empty(),
        "clearance spends the claim and closes the wait"
    );
    assert!(
        graphhelm_execution::ready_set(&spec, &dispatchable_deploy(&cleared))
            .unwrap()
            .contains("deploy"),
        "once cleared, the dependent is ready through the SAME derivation both drivers call"
    );
}

/// A clearance whose claim sequence names no claim is UNINTERPRETABLE — the countersignature has
/// no testimony under it, and no later reader can decide what was cleared. That is the one Corrupt
/// case in this family; a refused or rejected claim is merely a recorded mistake.
#[test]
fn a_clearance_naming_no_claim_is_corrupt_rather_than_silently_ignored() {
    let mut batch = parked_batch();
    let wait_index = batch.len() - 1;
    batch.push(event(
        "clearance-orphan",
        EventKind::CompletionCleared(graphhelm_protocols::CompletionCleared {
            execution_id: OpaqueId::parse("execution-test").unwrap(),
            // The PARKING event's sequence is a real sequence and not a claim: the sharpest
            // possible near-miss, derived from the arrangement rather than invented.
            claim_seq: wait_index as u64 + 1,
            verifier: graphhelm_protocols::ClearanceVerifier::MachineReplay {
                // #161: deliberately still arbitrary. This clearance names no claim, so the
                // fold returns Corrupt BEFORE any verifier is inspected - the hash is
                // unreachable on this path. Giving it a real digest would imply this test
                // exercises the verification, which it does not.
                manifest_hash: WireHash::parse(format!("sha256:{}", "c".repeat(64))).unwrap(),
            },
        }),
    ));
    let appended = append(batch);
    let _ = sequence_of(&appended, wait_index);
    assert!(matches!(
        replay(&scope(), STREAM, &appended),
        Err(ReplayError::Corrupt)
    ));
}

/// THE TIMELINE HAS AN ORACLE NOW (#160). `customs_scans` is written by FIVE arms of the fold and,
/// until this guard, was asserted by NONE — five writers and no reader.
///
/// That is worse than an unused field. An unused field is inert; a field written by five sites
/// with no oracle is five places where a wrong stage, a wrong sequence or a dropped entry survives
/// every green suite. The question the orchestrator asked was whether this is "an interface
/// awaiting a consumer" or dead state; measured, it was neither — it was UNGUARDED state, and the
/// answer to that is a guard rather than a deadline.
///
/// The consumer (#163 renders this timeline) is real and is someone else's lane. This guard does
/// not wait for it, because the defect it catches is mine and lands before theirs.
///
/// Asserts the whole story in LOG ORDER, which is the property the renderer depends on: a timeline
/// whose entries are individually right and collectively out of order tells the operator a false
/// sequence of events.
#[test]
fn the_customs_timeline_records_each_stage_once_in_log_order() {
    let mut batch = parked_batch();
    let wait_index = batch.len() - 1;
    batch.push(claim_event("claim", wait_index as u64 + 1));
    let claim_index = batch.len() - 1;
    let appended = append(batch.clone());
    let wait_seq = sequence_of(&appended, wait_index);
    let claim_seq = sequence_of(&appended, claim_index);

    batch.push(event(
        "clearance",
        EventKind::CompletionCleared(graphhelm_protocols::CompletionCleared {
            execution_id: OpaqueId::parse("execution-test").unwrap(),
            claim_seq,
            verifier: graphhelm_protocols::ClearanceVerifier::MachineReplay {
                // #161: the digest of the EMPTY bundle, because `claim_event` journals
                // `evidence: vec![]`. Was an arbitrary literal while nothing read the field.
                manifest_hash: graphhelm_events::claim_evidence_digest(&[]),
            },
        }),
    ));
    let cleared_index = batch.len() - 1;
    let appended = append(batch);
    let cleared_seq = sequence_of(&appended, cleared_index);

    let projection = replay(&scope(), STREAM, &appended).unwrap();
    let timeline = projection
        .customs_scans
        .get(CUSTOMS_NODE)
        .expect("the node that parked, claimed and cleared has a timeline");

    // The SHAPE is asserted as a whole rather than field-by-field, so an entry appearing twice or
    // in the wrong place fails here instead of passing three separate spot checks.
    let shape: Vec<(u64, graphhelm_events::CustomsStage, Option<u64>)> = timeline
        .iter()
        .map(|scan| (scan.at_sequence, scan.stage, scan.claim_seq))
        .collect();
    assert_eq!(
        shape,
        vec![
            (wait_seq, graphhelm_events::CustomsStage::Parked, None),
            (
                claim_seq,
                graphhelm_events::CustomsStage::Claimed,
                Some(claim_seq)
            ),
            (
                cleared_seq,
                graphhelm_events::CustomsStage::Cleared,
                Some(claim_seq)
            ),
        ],
        "the timeline is the story in log order: parked, claimed, cleared"
    );

    // The CLEARED entry points at the claim it spent, not at itself — the property that lets a
    // reader join an outcome back to the testimony it answered.
    assert_eq!(
        timeline[2].claim_seq,
        Some(claim_seq),
        "a clearance names the claim it cleared, never its own sequence"
    );
    assert_ne!(
        cleared_seq, claim_seq,
        "the arrangement must give the clearance its own sequence or the assertion above \
         is satisfied by coincidence"
    );
}

/// A REFUSAL reaches the timeline carrying its reason, and changes nothing else.
///
/// Separate from the story above because a refusal is the one stage with NO claim to point at —
/// which is exactly why it carries `reason_code` itself while a rejection points at its claim's
/// outcome record instead. Asserting both in one fixture would let either explain the other.
#[test]
fn a_refusal_reaches_the_timeline_with_its_reason_and_leaves_the_node_parked() {
    let mut batch = parked_batch();
    let wait_index = batch.len() - 1;
    batch.push(event(
        "refusal",
        EventKind::CompletionRefused(graphhelm_protocols::CompletionRefused {
            execution_id: OpaqueId::parse("execution-test").unwrap(),
            node: OpaqueId::parse(CUSTOMS_NODE).unwrap(),
            claimed_wait_seq: wait_index as u64 + 1,
            reason_code: graphhelm_protocols::SafeCode::parse("stale_rendezvous").unwrap(),
        }),
    ));
    let refusal_index = batch.len() - 1;
    let appended = append(batch);
    let refusal_seq = sequence_of(&appended, refusal_index);

    let projection = replay(&scope(), STREAM, &appended).unwrap();
    let timeline = projection
        .customs_scans
        .get(CUSTOMS_NODE)
        .expect("a refused node still has a timeline");

    assert_eq!(timeline.len(), 2, "parked, then refused: {timeline:?}");
    assert_eq!(timeline[1].at_sequence, refusal_seq);
    assert_eq!(timeline[1].stage, graphhelm_events::CustomsStage::Refused);
    assert_eq!(
        timeline[1].reason_code.as_deref(),
        Some("stale_rendezvous"),
        "a refusal has no claim to point at, so it must carry its own reason"
    );
    assert_eq!(
        timeline[1].claim_seq, None,
        "and it names no claim, because a refused claim never became one"
    );
    assert_eq!(
        projection.node_states.get(CUSTOMS_NODE),
        Some(&NodeState::WaitingInput),
        "a recorded refusal changes no state — that is its whole point"
    );
}

// =================================================================================================
// M11 #161 (lane 2) — clearance and the countersign identity registry.
//
// EVERY fixture below appends ONE batch. That is not style: `append()` opens a fresh store and
// always asks for first-sequence 1, so two calls produce two events numbered 1 and the combined
// vector carries restarting sequences. This family's whole thesis is SEQUENCE AS IDENTITY — which
// wait, which claim, which registration came first — so a fixture whose sequences restart cannot
// express the orderings these cells are about. Positions inside the single batch ARE the
// sequences, captured as the list is built.
// =================================================================================================

fn identity_registered(key: &str, identity: &str, fingerprint: &str) -> NewEvent {
    event(
        key,
        EventKind::ClearanceIdentityRegistered(graphhelm_protocols::ClearanceIdentityRegistered {
            execution_id: OpaqueId::parse("execution-test").unwrap(),
            identity: OpaqueId::parse(identity).unwrap(),
            key_fingerprint: WireHash::parse(format!("sha256:{}", fingerprint.repeat(64))).unwrap(),
        }),
    )
}

fn identity_revoked(key: &str, identity: &str) -> NewEvent {
    event(
        key,
        EventKind::ClearanceIdentityRevoked(graphhelm_protocols::ClearanceIdentityRevoked {
            execution_id: OpaqueId::parse("execution-test").unwrap(),
            identity: OpaqueId::parse(identity).unwrap(),
        }),
    )
}

fn countersigned(key: &str, claim_seq: u64, identity: &str, fingerprint: &str) -> NewEvent {
    event(
        key,
        EventKind::CompletionCleared(graphhelm_protocols::CompletionCleared {
            execution_id: OpaqueId::parse("execution-test").unwrap(),
            claim_seq,
            verifier: graphhelm_protocols::ClearanceVerifier::Countersign {
                identity: OpaqueId::parse(identity).unwrap(),
                key_fingerprint: WireHash::parse(format!("sha256:{}", fingerprint.repeat(64)))
                    .unwrap(),
            },
        }),
    )
}

/// R1 — THE SHARP `UnknownIdentity` CELL.
///
/// The signer IS in the registry at head and was NOT in it at the clearance's own sequence. This
/// is the case that discriminates a correct fold from every implementation validating against the
/// FINAL registry — the easy mistake, because that map is sitting right there when the fold ends.
/// A never-registered signer is caught by the wrong implementations too, so it is the companion
/// below and never the headline.
#[test]
fn a_clearance_by_an_identity_registered_after_it_is_refused() {
    let mut batch = parked_batch();
    let wait_index = batch.len() - 1;
    batch.push(claim_event("claim-1", wait_index as u64 + 1));
    let claim_index = batch.len() - 1;
    batch.push(countersigned(
        "clear-1",
        claim_index as u64 + 1,
        "auditor-late",
        "b",
    ));
    batch.push(identity_registered("reg-late", "auditor-late", "b"));

    let appended = append(batch);
    let claim_seq = sequence_of(&appended, claim_index);
    let projection = replay(&scope(), STREAM, &appended).expect("a legal log replays");

    assert!(
        projection.clearance_registry.contains_key("auditor-late"),
        "precondition: the signer IS in the registry at head, or this fixture is not measuring \
         the case it names: {:?}",
        projection.clearance_registry
    );
    assert_eq!(
        projection.clearances.get(&claim_seq),
        Some(&ClearanceOutcome::Refused {
            reason_code: graphhelm_protocols::SafeCode::parse("unknown_identity")
                .expect("a refusal code is a SafeCode")
        }),
        "a clearance is judged by the registry AS OF ITS OWN SEQUENCE — a registration landing \
         afterwards cannot reach back and validate it"
    );
    assert_eq!(
        projection.node_states.get(CUSTOMS_NODE),
        Some(&NodeState::WaitingInput),
        "and nothing was released: a refused clearance leaves the node parked"
    );
}

/// The cheap companion. Kept because it is free, NOT because it discriminates.
#[test]
fn a_clearance_by_an_identity_never_registered_is_refused() {
    let mut batch = parked_batch();
    let wait_index = batch.len() - 1;
    batch.push(claim_event("claim-1", wait_index as u64 + 1));
    let claim_index = batch.len() - 1;
    batch.push(countersigned(
        "clear-1",
        claim_index as u64 + 1,
        "auditor-ghost",
        "c",
    ));

    let appended = append(batch);
    let claim_seq = sequence_of(&appended, claim_index);
    let projection = replay(&scope(), STREAM, &appended).expect("a legal log replays");
    assert_eq!(
        projection.clearances.get(&claim_seq),
        Some(&ClearanceOutcome::Refused {
            reason_code: graphhelm_protocols::SafeCode::parse("unknown_identity")
                .expect("a refusal code is a SafeCode")
        }),
        "nobody registered this signer, ever"
    );
}

/// R2 — revocation is NOT retroactive: a clearance valid when it happened stays valid.
///
/// NAMED IN ADVANCE so this reads as half a pair: it stays GREEN under a registry that ignores
/// revocation entirely, because it asserts SURVIVAL. The member that makes revocation bite is
/// `a_revoked_identity_cannot_clear_a_later_claim`.
#[test]
fn a_clearance_survives_the_later_revocation_of_its_signer() {
    let mut batch = parked_batch();
    let wait_index = batch.len() - 1;
    batch.push(identity_registered("reg-a", "auditor-a", "b"));
    batch.push(claim_event("claim-1", wait_index as u64 + 1));
    let claim_index = batch.len() - 1;
    batch.push(countersigned(
        "clear-1",
        claim_index as u64 + 1,
        "auditor-a",
        "b",
    ));
    batch.push(identity_revoked("rev-a", "auditor-a"));

    let appended = append(batch);
    let claim_seq = sequence_of(&appended, claim_index);
    let projection = replay(&scope(), STREAM, &appended).expect("a legal log replays");

    assert!(
        !projection.clearance_registry.contains_key("auditor-a"),
        "precondition: the signer is revoked at head, or this measures nothing"
    );
    assert_eq!(
        projection.clearances.get(&claim_seq),
        Some(&ClearanceOutcome::Cleared),
        "the clearance was valid when it happened, and no later event rewrites that verdict"
    );
    assert_eq!(
        projection.node_states.get(CUSTOMS_NODE),
        Some(&NodeState::Succeeded),
        "the release it earned stands too"
    );
}

/// The member that makes the pair measure revocation.
#[test]
fn a_revoked_identity_cannot_clear_a_later_claim() {
    let mut batch = parked_batch();
    let wait_index = batch.len() - 1;
    batch.push(identity_registered("reg-a", "auditor-a", "b"));
    batch.push(identity_revoked("rev-a", "auditor-a"));
    batch.push(claim_event("claim-1", wait_index as u64 + 1));
    let claim_index = batch.len() - 1;
    batch.push(countersigned(
        "clear-1",
        claim_index as u64 + 1,
        "auditor-a",
        "b",
    ));

    let appended = append(batch);
    let claim_seq = sequence_of(&appended, claim_index);
    let projection = replay(&scope(), STREAM, &appended).expect("a legal log replays");
    assert_eq!(
        projection.clearances.get(&claim_seq),
        Some(&ClearanceOutcome::Refused {
            reason_code: graphhelm_protocols::SafeCode::parse("unknown_identity")
                .expect("a refusal code is a SafeCode")
        }),
        "revocation binds everything after it"
    );
}

/// R1's fingerprint half: the right name with foreign key material is a different signer.
#[test]
fn a_clearance_whose_fingerprint_does_not_match_the_registration_is_refused() {
    let mut batch = parked_batch();
    let wait_index = batch.len() - 1;
    batch.push(identity_registered("reg-a", "auditor-a", "b"));
    batch.push(claim_event("claim-1", wait_index as u64 + 1));
    let claim_index = batch.len() - 1;
    batch.push(countersigned(
        "clear-1",
        claim_index as u64 + 1,
        "auditor-a",
        "d",
    ));

    let appended = append(batch);
    let claim_seq = sequence_of(&appended, claim_index);
    let projection = replay(&scope(), STREAM, &appended).expect("a legal log replays");
    assert_eq!(
        projection.clearances.get(&claim_seq),
        Some(&ClearanceOutcome::Refused {
            reason_code: graphhelm_protocols::SafeCode::parse("unknown_identity")
                .expect("a refusal code is a SafeCode")
        }),
        "membership is name AND fingerprint: the fold compares both"
    );
}

/// R5 — the same journal folds to the same verdicts every time, and those verdicts are the ones
/// the ORDER dictates rather than the ones the end state suggests.
#[test]
fn interleaved_registrations_and_clearances_replay_identically() {
    let mut batch = parked_batch();
    let wait_index = batch.len() - 1;
    batch.push(identity_registered("reg-a", "auditor-a", "b"));
    batch.push(claim_event("claim-1", wait_index as u64 + 1));
    let claim_index = batch.len() - 1;
    batch.push(countersigned(
        "clear-1",
        claim_index as u64 + 1,
        "auditor-a",
        "b",
    ));
    batch.push(identity_revoked("rev-a", "auditor-a"));
    batch.push(identity_registered("reg-b", "auditor-b", "c"));

    let appended = append(batch);
    let claim_seq = sequence_of(&appended, claim_index);
    let first = replay(&scope(), STREAM, &appended).expect("a legal log replays");
    let second = replay(&scope(), STREAM, &appended).expect("a legal log replays");

    assert_eq!(
        first, second,
        "the same journal folds to the same state every time"
    );
    assert_eq!(
        first.clearances.get(&claim_seq),
        Some(&ClearanceOutcome::Cleared),
        "cleared while its signer was live — decided by ORDER, not by the end state, where \
         auditor-a is revoked and auditor-b appeared later"
    );
}

/// The node's own timeline entry for a REFUSED clearance, which nothing observed until now.
///
/// `customs_scans` is public, several fold arms write it, and a grep across the repository finds
/// **no test reading it at all**. Deleting the whole `record_scan` call on the refusal path left
/// the suite at 38 passed. That is how this cell was found, and reading the field's own doc while
/// writing it is how two defects in my own code were found with it.
///
/// The vocabulary settles both. `Cleared` is documented as "the only stage that releases a
/// dependent" and a refused clearance releases nothing; `Rejected` is documented as "clearance
/// withheld with a reason; the claim is spent, the node stays parked", which is verbatim what the
/// fold does. And `reason_code` is for stages where this timeline is the fact's SOLE owner: a
/// rejected claim has an outcome record keyed by `claim_seq` that owns its reason, so this entry
/// carries the POINTER and not a copy. Two structures recording one fact is duplicated state, and
/// a later replay can make the copies disagree.
#[test]
fn a_refused_clearance_leaves_a_pointer_in_the_nodes_timeline_not_a_copy() {
    let mut batch = parked_batch();
    let wait_index = batch.len() - 1;
    batch.push(claim_event("claim-1", wait_index as u64 + 1));
    let claim_index = batch.len() - 1;
    batch.push(countersigned(
        "clear-1",
        claim_index as u64 + 1,
        "auditor-ghost",
        "c",
    ));
    let clear_index = batch.len() - 1;

    let appended = append(batch);
    let claim_seq = sequence_of(&appended, claim_index);
    let clear_seq = sequence_of(&appended, clear_index);
    let projection = replay(&scope(), STREAM, &appended).expect("a recorded refusal replays");

    let history = projection
        .customs_scans
        .get(CUSTOMS_NODE)
        .expect("the node has a customs timeline");
    // Selected by the entry's OWN sequence, not by `claim_seq`: the Claimed entry and this one
    // both point at the same claim, so a `claim_seq` selector picks the first and silently
    // measures the wrong row. The first draft of this guard did exactly that and reported
    // `left: Claimed`.
    let entry = history
        .iter()
        .find(|s| s.at_sequence == clear_seq)
        .expect("the refused clearance left an entry at its own sequence");
    assert_eq!(
        entry.claim_seq,
        Some(claim_seq),
        "and that entry points at the claim it refused"
    );

    assert_eq!(
        entry.stage,
        CustomsStage::Rejected,
        "a withheld clearance is Rejected, never Cleared: Cleared is the only stage that \
         releases a dependent, and this one released nothing"
    );
    assert_eq!(
        entry.reason_code, None,
        "the timeline carries the POINTER (claim_seq) and not a copy of the reason: the \
         outcome record keyed by that claim owns it, and two structures holding one fact \
         can disagree after a replay"
    );
    assert_eq!(
        projection.clearances.get(&claim_seq),
        Some(&ClearanceOutcome::Refused {
            reason_code: graphhelm_protocols::SafeCode::parse("unknown_identity")
                .expect("a refusal code is a SafeCode")
        }),
        "and the record the pointer points AT is the one carrying the reason"
    );
}

/// The one evidence bundle the #161 fixtures claim, so the claim and any clearance for it cannot
/// drift apart by being written out twice.
fn evidence_fixture() -> Vec<graphhelm_protocols::ClaimEvidence> {
    vec![graphhelm_protocols::ClaimEvidence {
        kind: "transcript".to_owned(),
        content_hash: WireHash::parse(format!("sha256:{}", "1".repeat(64))).unwrap(),
        size: 42,
    }]
}

/// Builds a claim carrying REAL evidence, so a bundle digest is a digest of something.
///
/// `claim_event` journals `evidence: vec![]`, which makes every bundle in this suite the SAME
/// (empty) bundle. A hash guard measured only against that fixture would be measuring the digest
/// of nothing.
fn claim_event_with_evidence(key: &str, completes_wait_seq: u64) -> NewEvent {
    event(
        key.to_owned(),
        EventKind::CompletionClaimed(graphhelm_protocols::CompletionClaimed {
            execution_id: OpaqueId::parse("execution-test").unwrap(),
            node: OpaqueId::parse(CUSTOMS_NODE).unwrap(),
            completes_wait_seq,
            evidence: evidence_fixture(),
            attestation: graphhelm_protocols::ClaimAttestation {
                asserter: OpaqueId::parse("agent-claimer").unwrap(),
                mode: graphhelm_protocols::ClaimAttestationMode::MachineVerified,
            },
        }),
    )
}

/// #161 R1's CO-REQUIRED pair: a machine replay presenting the RIGHT digest still clears.
///
/// Without this, R1 alone is satisfied by a fold that refuses every machine replay — the opposite
/// defect, wearing the same colour. One member says a wrong hash is rejected; this one says a
/// right hash is not.
///
/// Both sides use `claim_evidence_digest`, and that is deliberate: this measures the FOLD's
/// decision, not the derivation. The derivation has its own guards below, which do not use it on
/// both sides of an equality.
#[test]
fn a_machine_replay_clearance_with_the_matching_bundle_hash_clears() {
    let mut batch = parked_batch();
    let wait_index = batch.len() - 1;
    batch.push(claim_event_with_evidence("claim-1", wait_index as u64 + 1));
    let claim_index = batch.len() - 1;
    batch.push(event(
        "clear-1",
        EventKind::CompletionCleared(graphhelm_protocols::CompletionCleared {
            execution_id: OpaqueId::parse("execution-test").unwrap(),
            claim_seq: claim_index as u64 + 1,
            verifier: graphhelm_protocols::ClearanceVerifier::MachineReplay {
                manifest_hash: graphhelm_events::claim_evidence_digest(&evidence_fixture()),
            },
        }),
    ));

    let appended = append(batch);
    let claim_seq = sequence_of(&appended, claim_index);
    let projection = replay(&scope(), STREAM, &appended).expect("a legal log replays");

    assert_eq!(
        projection.clearances.get(&claim_seq),
        Some(&ClearanceOutcome::Cleared),
        "the presented digest IS the claim's evidence digest, so the verification passes"
    );
    assert_eq!(
        projection.node_states.get(CUSTOMS_NODE),
        Some(&NodeState::Succeeded),
        "and a cleared machine replay releases downstream exactly as before #161"
    );
}

/// The derivation is INJECTIVE across ITEM boundaries, which is what the length prefixes buy.
///
/// My first fixture here was `["ab"]` against `["a"]` and it could not fail: `content_hash` and
/// `size` are fixed-width, so those two bundles differ in total length under ANY encoding. A
/// sabotage that removed the length prefixes left it green - the guard was decoration, and only
/// running that sabotage said so.
///
/// A real collision needs one bundle's `kind` to SWALLOW a whole item of the other. `size: 0`
/// makes the fixed-width size field eight NUL bytes, which are valid UTF-8 and so can live inside
/// a `String`. Without length prefixes both bundles encode to exactly the same bytes, and a
/// clearance for one would clear the other.
#[test]
fn evidence_digests_separate_a_bundle_from_one_whose_kind_swallows_an_item() {
    let first = WireHash::parse(format!("sha256:{}", "1".repeat(64))).unwrap();
    let second = WireHash::parse(format!("sha256:{}", "2".repeat(64))).unwrap();

    let split = vec![
        graphhelm_protocols::ClaimEvidence {
            kind: "a".to_owned(),
            content_hash: first.clone(),
            size: 0,
        },
        graphhelm_protocols::ClaimEvidence {
            kind: "b".to_owned(),
            content_hash: second.clone(),
            size: 0,
        },
    ];
    // `kind` here is exactly the bytes the unprefixed encoding would emit for the first item plus
    // the second item's kind: "a" + <hash> + eight NULs + "b".
    let swallowed = vec![graphhelm_protocols::ClaimEvidence {
        kind: format!("a{}{}b", first.as_str(), "\0".repeat(8)),
        content_hash: second,
        size: 0,
    }];

    assert_ne!(
        graphhelm_events::claim_evidence_digest(&split),
        graphhelm_events::claim_evidence_digest(&swallowed),
        "two bundles that concatenate to the same bytes must not share a digest: without length prefixes these collide, and a clearance for one would clear the other"
    );
}

/// Evidence ORDER is part of what was claimed, so reordering changes the digest.
///
/// Recorded as a decision rather than left to be discovered: the journal preserves order, so the
/// order is a fact about the testimony. If a later lane wants bundles to be order-insensitive,
/// that is a deliberate change with this guard as its landmark.
#[test]
fn evidence_digests_depend_on_the_order_the_bundle_was_journaled_in() {
    let first = graphhelm_protocols::ClaimEvidence {
        kind: "transcript".to_owned(),
        content_hash: WireHash::parse(format!("sha256:{}", "1".repeat(64))).unwrap(),
        size: 1,
    };
    let second = graphhelm_protocols::ClaimEvidence {
        kind: "diff".to_owned(),
        content_hash: WireHash::parse(format!("sha256:{}", "2".repeat(64))).unwrap(),
        size: 2,
    };
    let forward = vec![first.clone(), second.clone()];
    let reversed = vec![second, first];

    assert_ne!(
        graphhelm_events::claim_evidence_digest(&forward),
        graphhelm_events::claim_evidence_digest(&reversed),
        "the same items in a different order are different testimony"
    );
}

/// #161 R1, the sealed cell: a machine replay presenting a hash the evidence does NOT produce
/// must be REFUSED, not cleared.
///
/// The arm it guards read `MachineReplay { .. }` and cleared unconditionally — the `..` discarded
/// `manifest_hash` outright — while the comment directly above it said the arm "re-derives the
/// evidence against the node's declared manifest". The comment described a verification the code
/// did not perform, which is the likeliest reason the gap survived review: the arm reads as
/// correct if you read the sentence above it.
///
/// The presented hash here is arbitrary AND the claim carries real evidence, so the two cannot
/// coincide. Its companion `..._with_the_matching_digest_clears` is CO-REQUIRED: this test alone
/// is satisfied by a fold that refuses every machine replay, which is a different defect with the
/// same colour.
#[test]
fn a_machine_replay_clearance_with_a_mismatched_bundle_hash_is_refused() {
    let mut batch = parked_batch();
    let wait_index = batch.len() - 1;
    batch.push(claim_event_with_evidence("claim-1", wait_index as u64 + 1));
    let claim_index = batch.len() - 1;
    batch.push(event(
        "clear-1",
        EventKind::CompletionCleared(graphhelm_protocols::CompletionCleared {
            execution_id: OpaqueId::parse("execution-test").unwrap(),
            claim_seq: claim_index as u64 + 1,
            verifier: graphhelm_protocols::ClearanceVerifier::MachineReplay {
                manifest_hash: WireHash::parse(format!("sha256:{}", "e".repeat(64))).unwrap(),
            },
        }),
    ));

    let appended = append(batch);
    let claim_seq = sequence_of(&appended, claim_index);
    let projection = replay(&scope(), STREAM, &appended).expect(
        "a clearance naming a real claim is INTERPRETABLE even when refused - only a clearance naming a non-claim is Corrupt",
    );

    // Landmark, per this issue's second amendment: assert the precondition rather than assume it.
    // Without this the test also passes when the clearance never reached the decision at all.
    assert!(
        projection.clearances.contains_key(&claim_seq),
        "precondition: the clearance must have been DECIDED and recorded against the claim, or the verdict below measures an absence: {:?}",
        projection.clearances
    );

    assert_eq!(
        projection.clearances.get(&claim_seq),
        Some(&ClearanceOutcome::Refused {
            reason_code: graphhelm_protocols::SafeCode::parse("hash_mismatch").unwrap(),
        }),
        "a machine replay whose manifest hash is not the evidence bundle's digest must refuse with hash_mismatch - the registry's own words for this code are \"presented evidence does not hash to what it claims\""
    );
    assert_ne!(
        projection.node_states.get(CUSTOMS_NODE),
        Some(&NodeState::Succeeded),
        "a refused clearance releases NOTHING: the claim is spent and the node stays parked"
    );
}

/// The `MachineReplay` arm, which NOTHING exercised until this guard existed.
///
/// Found by sabotage, not by reading: inverting that arm from `Cleared` to `Refused` changed the
/// suite by exactly zero tests. The two existing fixtures that build a `MachineReplay` verifier
/// both fail to reach the decision - one takes the Corrupt path for a clearance naming no claim
/// and returns before the outcome is computed, the other is red for reasons in another lane's
/// base. A production branch that no guard can turn red is the mirror of a guard that has never
/// been red, and it deserves the same treatment.
///
/// The discriminating part is that the registry is EMPTY. A machine replay re-derives the evidence
/// against the node's declared manifest and asks no membership question, so it clears with nobody
/// registered at all. Should anyone later make this arm consult the registry, this is the cell
/// that falls - and it is deliberately NOT left to `the_downstream_of_a_claimed_wait`, which will
/// exercise this arm incidentally once it goes green. Coverage that a test provides by accident
/// disappears silently the day that test changes for its own reasons.
#[test]
fn a_machine_replay_clearance_needs_no_registered_identity() {
    let mut batch = parked_batch();
    let wait_index = batch.len() - 1;
    batch.push(claim_event("claim-1", wait_index as u64 + 1));
    let claim_index = batch.len() - 1;
    batch.push(event(
        "clear-1",
        EventKind::CompletionCleared(graphhelm_protocols::CompletionCleared {
            execution_id: OpaqueId::parse("execution-test").unwrap(),
            claim_seq: claim_index as u64 + 1,
            verifier: graphhelm_protocols::ClearanceVerifier::MachineReplay {
                // #161: this hash used to be arbitrary (`sha256:eee...`) and legal, because
                // NOTHING read the field. It is now load-bearing, so it must be the digest of
                // the bundle `claim_event` journals - which is the EMPTY bundle. The assertion
                // below is untouched: this fixture supplies a correct hash so the test can go on
                // measuring the thing it was written for, which is that MEMBERSHIP is not asked.
                manifest_hash: graphhelm_events::claim_evidence_digest(&[]),
            },
        }),
    ));

    let appended = append(batch);
    let claim_seq = sequence_of(&appended, claim_index);
    let projection = replay(&scope(), STREAM, &appended).expect("a legal log replays");

    assert!(
        projection.clearance_registry.is_empty(),
        "precondition: NOBODY is registered, or this measures nothing about membership \
         being irrelevant here: {:?}",
        projection.clearance_registry
    );
    assert_eq!(
        projection.clearances.get(&claim_seq),
        Some(&ClearanceOutcome::Cleared),
        "a machine replay carries no identity, so an empty registry cannot refuse it"
    );
    assert_eq!(
        projection.node_states.get(CUSTOMS_NODE),
        Some(&NodeState::Succeeded),
        "and it releases, exactly as a countersignature by a live identity would"
    );
}

/// R6, refused half: a refusal is journal data and the log still reads.
#[test]
fn a_refused_clearance_is_recorded_and_the_log_still_replays() {
    let mut batch = parked_batch();
    let wait_index = batch.len() - 1;
    batch.push(claim_event("claim-1", wait_index as u64 + 1));
    let claim_index = batch.len() - 1;
    batch.push(countersigned(
        "clear-1",
        claim_index as u64 + 1,
        "auditor-ghost",
        "c",
    ));

    let appended = append(batch);
    let claim_seq = sequence_of(&appended, claim_index);
    let projection =
        replay(&scope(), STREAM, &appended).expect("a recorded refusal is not an unreadable log");
    assert!(
        projection.clearances.contains_key(&claim_seq),
        "the mistake is ON RECORD, readable by anyone replaying: {:?}",
        projection.clearances
    );
}

/// The registry half, exercised WITHOUT the customs prelude — the part of this lane that is
/// testable today, found by H's review.
///
/// It matters that this one RUNS while the clearance cells cannot: `clearance_identity_*` are
/// declared in the schema, so a batch containing only them is appendable, while every cell about
/// a CLEARANCE drags `completion_claimed` + `completion_cleared`, which are not. So the lane's
/// blockage is PARTIAL, not total, and this is the piece on the free side of the line.
///
/// What it does NOT do, stated so nobody reads it as more: it exercises the register/revoke fold
/// arms and their determinism. It does NOT test membership-at-N, because that property only
/// becomes observable when a clearance is VALIDATED at N — and validating a clearance is exactly
/// what is not implemented yet. A green here is coverage of the accumulator, never of the rule.
#[test]
fn the_identity_registry_folds_deterministically_and_revocation_removes() {
    let mut batch = parked_batch();
    // `auditor-a` is the composite: registered, revoked, registered again with new key material.
    batch.push(identity_registered("reg-a", "auditor-a", "b"));
    // `auditor-b` is never touched again.
    batch.push(identity_registered("reg-b", "auditor-b", "c"));
    batch.push(identity_revoked("rev-a", "auditor-a"));
    batch.push(identity_registered("reg-a2", "auditor-a", "d"));
    // `auditor-c` is revoked and NEVER restored. Without this identity the fixture is blind to a
    // revoke that does nothing, because on `auditor-a` a no-op revoke is invisible: the later
    // registration overwrites to the same value whether or not the remove happened.
    batch.push(identity_registered("reg-c", "auditor-c", "e"));
    batch.push(identity_revoked("rev-c", "auditor-c"));
    // `auditor-d` rotates its key with NO revocation in between. Without this identity the fixture
    // is blind to first-write-wins, because every other overwrite here follows a `remove`, which
    // leaves an empty slot that a first-write-wins insert fills identically.
    batch.push(identity_registered("reg-d", "auditor-d", "f"));
    batch.push(identity_registered("reg-d2", "auditor-d", "0"));

    let appended = append(batch);
    let first = replay(&scope(), STREAM, &appended).expect("a legal log replays");
    let second = replay(&scope(), STREAM, &appended).expect("a legal log replays");

    assert_eq!(
        first, second,
        "the same journal folds to the same registry every time"
    );
    assert!(
        first.clearance_registry.contains_key("auditor-b"),
        "a registration that was never revoked survives: {:?}",
        first.clearance_registry
    );
    assert!(
        !first.clearance_registry.contains_key("auditor-c"),
        "a revocation with no later registration leaves NOTHING behind, which is the only \
         assertion here that a do-nothing revoke can fail: {:?}",
        first.clearance_registry
    );
    assert_eq!(
        first
            .clearance_registry
            .get("auditor-d")
            .map(|hash| hash.as_str().to_owned()),
        Some(format!("sha256:{}", "0".repeat(64))),
        "a rotation with no revocation between the two registrations keeps the LAST fingerprint, \
         which is the only assertion here that first-write-wins can fail: {:?}",
        first.clearance_registry
    );
    assert_eq!(
        first
            .clearance_registry
            .get("auditor-a")
            .map(|hash| hash.as_str().to_owned()),
        Some(format!("sha256:{}", "d".repeat(64))),
        "re-registering after a revocation restores the identity with the NEW fingerprint, \
         because a rotation is a register and the last one before the cursor is the one that \
         counts: {:?}",
        first.clearance_registry
    );
}
