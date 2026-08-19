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
