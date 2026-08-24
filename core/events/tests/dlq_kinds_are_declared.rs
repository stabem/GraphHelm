//! The five #162 event kinds must be APPENDABLE and REPLAYABLE, not merely typed.
//!
//! A new `EventKind` compiles, serialises and folds long before the store will accept it.
//! `validate_envelope` checks every envelope against `repository_schema_set()` and answers
//! `EventRepositoryError::Invalid` for anything the schema does not describe — with no
//! diagnostic, no mention of the schema and no mention of the kind. The cost of that silence
//! has been measured on this project at eleven commits of enum, payloads, fold and guards with
//! nothing whatsoever reaching a journal.
//!
//! So this test is not about the fold. It asserts the DECLARATION: that all four sites know the
//! five kinds. Remove any one of them from `schemas/event-envelope.schema.json` — the payload
//! `$defs`, the `eventKind` union branch, or the top-level scope pairing — and the append below
//! fails with a bare `Invalid`.
//!
//! It also reads the events back, deliberately. `validate_envelope` runs on the READ paths as
//! well as on append (`local.rs`, `projection.rs`), so an undeclared kind breaks REPLAY and not
//! only writing. A test that only appended would leave that half unmeasured.
//!
//! The batch is not arbitrary either: `sweep_performed` and the `overdue_exception` it mints are
//! appended TOGETHER, because adjacency is what links an exception to its sweep. No field
//! carries that link, so no state can exist in which one was recorded and the other was not.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use chrono::{TimeZone, Utc};
use graphhelm_events::{LocalEventRepository, PreparedAppend};
use graphhelm_protocols::{
    ActorId, Clock, CustomsStage, DlqRedrive, DlqReturned, DlqRouted, EventKind, ExecutionId,
    IdGenerator, NewEvent, OpaqueId, OverdueException, PersistedActor, PersistedActorType,
    PersistedTimestamp, ProjectId, RepositoryScope, SafeCode, Sensitivity, SweepCaller,
    SweepPerformed, WorkspaceId,
};

struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 20, 12, 0, 0).unwrap()
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

fn event(key: &str, kind: EventKind) -> NewEvent {
    NewEvent::new(
        OpaqueId::parse(key).unwrap(),
        actor(),
        Sensitivity::Internal,
        kind,
        vec![],
        vec![],
    )
}

fn execution() -> OpaqueId {
    OpaqueId::parse("execution-test").unwrap()
}

fn node() -> OpaqueId {
    OpaqueId::parse("node-under-customs").unwrap()
}

fn instant() -> PersistedTimestamp {
    PersistedTimestamp::from_datetime(Utc.with_ymd_and_hms(2026, 8, 20, 11, 0, 0).unwrap())
        .expect("a fixed instant is a valid timestamp")
}

#[test]
fn every_dlq_and_sweep_kind_appends_and_reads_back() {
    let directory = tempfile::tempdir().unwrap();
    let repository = LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    )
    .unwrap();

    let request = PreparedAppend::new(
        scope(),
        OpaqueId::parse("stream-test").unwrap(),
        1,
        vec![
            event(
                "routed",
                EventKind::DlqRouted(DlqRouted {
                    execution_id: execution(),
                    node_id: node(),
                    episode_sequence: 4,
                    reason: SafeCode::parse("clearance_never_granted").unwrap(),
                }),
            ),
            event(
                "redrive",
                EventKind::DlqRedrive(DlqRedrive {
                    execution_id: execution(),
                    node_id: node(),
                    dlq_episode_sequence: 5,
                }),
            ),
            // Returned WITHOUT a declared budget, deliberately: absence is absence, and the
            // schema key is omitted rather than sent as null. A present `null` would say a
            // budget WAS declared and is nothing, which is a claim no caller made.
            event(
                "returned-unbudgeted",
                EventKind::DlqReturned(DlqReturned {
                    execution_id: execution(),
                    node_id: node(),
                    dlq_episode_sequence: 5,
                    wait_within_seconds: None,
                }),
            ),
            event(
                "returned-budgeted",
                EventKind::DlqReturned(DlqReturned {
                    execution_id: execution(),
                    node_id: node(),
                    dlq_episode_sequence: 5,
                    wait_within_seconds: Some(900),
                }),
            ),
            event(
                "swept",
                EventKind::SweepPerformed(SweepPerformed {
                    execution_id: execution(),
                    as_of: instant(),
                    caller: SweepCaller::Tick,
                }),
            ),
            event(
                "overdue",
                EventKind::OverdueException(OverdueException {
                    execution_id: execution(),
                    node_id: node(),
                    episode_sequence: 8,
                    stage: CustomsStage::Claimed,
                    deadline: instant(),
                }),
            ),
        ],
        vec![],
        vec![],
    )
    .unwrap();

    // The append is the assertion. An undeclared kind fails here with a bare `Invalid`.
    let appended = repository
        .append_atomic(&request)
        .expect("the five #162 kinds are declared to the schema and therefore appendable");
    assert_eq!(appended.len(), 6, "every event in the batch was written");

    // And the read path validates too, so replay must accept what append accepted.
    let read_back = repository
        .read_replay_stream(&scope(), "stream-test")
        .expect("a journal carrying the five kinds replays");
    assert_eq!(
        read_back.len(),
        6,
        "the read path accepts every kind the append path accepted"
    );

    // Both optional spellings survive the round trip, which is the R7 guarantee on the wire:
    // the unbudgeted return must come back with no budget, not with zero.
    let returned: Vec<_> = read_back
        .iter()
        .filter_map(|envelope| match &envelope.kind {
            EventKind::DlqReturned(payload) => Some(payload.wait_within_seconds),
            _ => None,
        })
        .collect();
    assert_eq!(
        returned,
        vec![None, Some(900)],
        "an absent budget reads back ABSENT, never as zero or a default"
    );
}
