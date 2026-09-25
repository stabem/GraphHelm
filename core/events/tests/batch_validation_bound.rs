//! The schema validator's own work budget is a bound on a physical batch, and it used to be
//! applied on ONE side of the store only (#744).
//!
//! `MAX_BATCH_EVENTS` (10,000) and `MAX_BATCH_BYTES` are declared ceilings checked on the way in.
//! The physical-batch schema is validated on the way OUT, in `parse_physical_batch`, and that
//! validation is preceded by a deterministic work governor which can decline to run at all. A
//! batch that crossed the governor's budget without crossing either declared ceiling was
//! therefore accepted, fsynced, and then unreadable forever: the events were intact and
//! unreachable, and the store reported the loss as `GHE005_INTEGRITY_FAILURE`, the code for a
//! corrupt or tampered journal.
//!
//! These cells pin two properties. The store never accepts a batch it cannot read back, and a
//! batch refused for being too complex is refused as a LIMIT, distinguishably from corruption.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{TimeZone, Utc};
use graphhelm_events::{LocalEventRepository, PreparedAppend};
use graphhelm_protocols::{
    ActorId, Clock, EventKind, ExecutionId, ExecutionMode, ExecutionStarted, IdGenerator, NewEvent,
    OpaqueId, PersistedActor, PersistedActorType, ProjectId, RawSha256, RepositoryScope,
    Sensitivity, SignalRecorded, SignalSeverity, SignalSourceKind, WireHash, WorkspaceId,
};

const STREAM: &str = "stream-batch-bound";

struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 4, 12, 0, 0).unwrap()
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

/// One `ExecutionStarted` followed by `signals` `SignalRecorded` events -- the shape the issue's
/// repro used, and the shape `serve/wake.rs` produces when several timers fire in one sweep.
fn batch_of(signals: usize) -> Vec<NewEvent> {
    keyed_batch_of("first", signals)
}

/// The same shape under a caller-chosen idempotency-key prefix, so a cell can append twice to one
/// store without the second batch being answered from the first one's idempotency record.
fn keyed_batch_of(prefix: &str, signals: usize) -> Vec<NewEvent> {
    let execution_id = OpaqueId::parse("execution-test").unwrap();
    let mut events = vec![event(
        format!("{prefix}-execution-started"),
        EventKind::ExecutionStarted(ExecutionStarted {
            execution_id: execution_id.clone(),
            graph_version: 1,
            graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
            mode: ExecutionMode::Supervised,
        }),
    )];
    for index in 0..signals {
        events.push(event(
            format!("{prefix}-signal-{index}"),
            EventKind::SignalRecorded(SignalRecorded {
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
    events
}

/// What the store did with a batch of a given size: the append's verdict, and -- when the append
/// was accepted -- whether a SEPARATE open of the same directory can read the events back.
#[derive(Debug, PartialEq, Eq)]
enum Fate {
    /// Written, and a fresh reader recovered every event.
    WrittenAndReadable,
    /// Written, and a fresh reader refused the line. This is the defect.
    WrittenAndUnreadable(String),
    /// Refused at the door, with this code.
    Refused(String),
}

fn fate_of(signals: usize) -> Fate {
    let directory = tempfile::tempdir().unwrap();
    let expected = signals + 1;
    {
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
            batch_of(signals),
            vec![],
            vec![],
        )
        .unwrap();
        match repository.append_atomic(&request) {
            Ok(envelopes) => {
                assert_eq!(
                    envelopes.len(),
                    expected,
                    "the append reported a short batch"
                );
            }
            Err(error) => return Fate::Refused(error.code().to_owned()),
        }
    }

    // A SEPARATE open, because that is what the defect needs: the writer holds the batch in
    // memory and never re-parses it, so the failure only appears to a process that re-reads the
    // journal from disk. The issue found it with `graphhelm serve` in another process.
    //
    // The open is part of what is being measured, not setup. `open` runs `load_state`, which
    // parses EVERY journal line, so an unreadable line does not cost one stream -- it costs the
    // whole repository, including streams that have nothing to do with the batch that wrote it.
    // Unwrapping here would report that as a harness crash instead of the finding it is.
    let reader = match LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    ) {
        Ok(reader) => reader,
        Err(error) => {
            return Fate::WrittenAndUnreadable(format!("open: {}", error.code()));
        }
    };
    match reader.read_replay_stream(&scope(), STREAM) {
        Ok(events) => {
            assert_eq!(
                events.len(),
                expected,
                "the reader recovered a short stream"
            );
            Fate::WrittenAndReadable
        }
        Err(error) => Fate::WrittenAndUnreadable(error.code().to_owned()),
    }
}

/// Sizes straddling the validator's budget. The exact crossing point is a function of event count
/// AND event size, so it is deliberately not asserted as a number anywhere -- these cells assert
/// the RELATIONSHIP between the two sides of the store, which holds whatever the number is.
const SIZES: [usize; 8] = [0, 1, 8, 64, 140, 160, 400, 1_200];

#[test]
fn the_store_never_accepts_a_batch_it_cannot_read_back() {
    let fates: Vec<(usize, Fate)> = SIZES.into_iter().map(|n| (n, fate_of(n))).collect();

    let unreadable: Vec<&(usize, Fate)> = fates
        .iter()
        .filter(|(_, fate)| matches!(fate, Fate::WrittenAndUnreadable(_)))
        .collect();
    assert!(
        unreadable.is_empty(),
        "the store wrote batches a fresh reader then refused: {unreadable:?}"
    );

    // Both halves of the sweep must be populated, or this cell proves nothing. Without an
    // accepted size it would pass on a store that refuses everything; without a refused size it
    // would pass on a build whose governor never fires, which is the same as not running.
    assert!(
        fates
            .iter()
            .any(|(_, fate)| *fate == Fate::WrittenAndReadable),
        "no size was accepted, so the readable half of this cell is vacuous: {fates:?}"
    );
    assert!(
        fates
            .iter()
            .any(|(_, fate)| matches!(fate, Fate::Refused(_))),
        "no size was refused, so this sweep never reached the bound it is about: {fates:?}"
    );
}

#[test]
fn a_batch_too_complex_to_validate_is_refused_as_a_limit_not_as_corruption() {
    let refusals: Vec<(usize, String)> = SIZES
        .into_iter()
        .filter_map(|n| match fate_of(n) {
            Fate::Refused(code) => Some((n, code)),
            _ => None,
        })
        .collect();

    assert!(
        !refusals.is_empty(),
        "no size in {SIZES:?} was refused, so there is no refusal to classify"
    );
    for (n, code) in &refusals {
        assert_eq!(
            code, "GHE006_LIMIT_EXCEEDED",
            "a batch of {n} signals was refused as {code}; a bound the caller can act on must not \
             be reported with the code that means the journal is corrupt or tampered with"
        );
    }
}

#[test]
fn a_refused_batch_leaves_the_journal_exactly_as_it_was() {
    let directory = tempfile::tempdir().unwrap();
    let repository = LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    )
    .unwrap();

    let accepted = PreparedAppend::new(
        scope(),
        OpaqueId::parse(STREAM).unwrap(),
        1,
        batch_of(1),
        vec![],
        vec![],
    )
    .unwrap();
    repository.append_atomic(&accepted).unwrap();

    let journal = directory.path().join("journal.jsonl");
    let before = std::fs::read(&journal).unwrap();
    assert!(!before.is_empty(), "the arrangement wrote no journal line");

    let oversized = PreparedAppend::new(
        scope(),
        OpaqueId::parse(STREAM).unwrap(),
        3,
        keyed_batch_of("second", 1_200),
        vec![],
        vec![],
    )
    .unwrap();
    let error = repository.append_atomic(&oversized).unwrap_err();

    assert_eq!(error.code(), "GHE006_LIMIT_EXCEEDED");
    assert_eq!(
        std::fs::read(&journal).unwrap(),
        before,
        "a refused batch must not reach the journal: the whole point of refusing at the door is \
         that a half-written or unreadable line never exists"
    );
    assert_eq!(
        repository
            .read_replay_stream(&scope(), STREAM)
            .unwrap()
            .len(),
        2,
        "the stream that existed before the refusal must still read back"
    );
}
