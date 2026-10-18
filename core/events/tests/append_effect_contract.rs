//! Approved effect commitment for the local event store append, driven under its failure model.
//!
//! Commitment (`append_event`, effects `filesystem.write`, guarantees `idempotent` and
//! `atomic`): a repeated logical operation, meaning the same idempotency key with the same
//! events, never yields a second event, and the repeat returns the result the first attempt
//! committed. A write torn by a crash is never guessed into an event: the store refuses it.
//!
//! Failure model this file drives against the real adapter: retry after a lost acknowledgement
//! (the batch is durable but the caller saw an error), a crash in the middle of the journal
//! write, two writers racing the same operation on one root, and a resume by a second writer
//! that opened the store after the first one failed. Plain retry of an acknowledged append is
//! already proven by `append_only.rs` and `local_atomicity.rs`; writers racing DIFFERENT keys
//! by `concurrent_writers_serialize_and_only_one_claims_the_expected_sequence`.
//!
//! The reach of this evidence is the runs below on one host, not every interleaving.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};

use chrono::{TimeZone, Utc};
use graphhelm_events::{LocalEventRepository, LocalFailpoint, PreparedAppend};
use graphhelm_protocols::{
    ActorId, Clock, EventEnvelope, EventKind, ExecutionId, GraphImported, GraphSourceKind,
    IdGenerator, NewEvent, OpaqueId, PersistedActor, PersistedActorType, ProjectId, RawSha256,
    RepositoryScope, Sensitivity, WorkspaceId,
};

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
        WorkspaceId::parse("workspace-contract").unwrap(),
        ProjectId::parse("project-contract").unwrap(),
        Some(ExecutionId::parse("execution-contract").unwrap()),
    )
}

/// The request a caller rebuilds for one logical operation. The key is bound to the
/// operation, never to the attempt, so every attempt builds a fresh but equal request.
fn request_for(operation: &str) -> PreparedAppend {
    PreparedAppend::new(
        scope(),
        OpaqueId::parse("stream-contract").unwrap(),
        1,
        vec![NewEvent::new(
            OpaqueId::parse(operation).unwrap(),
            PersistedActor::new(
                PersistedActorType::System,
                ActorId::parse("system-contract").unwrap(),
            ),
            Sensitivity::Internal,
            EventKind::GraphImported(GraphImported {
                source_sha256: RawSha256::parse("a".repeat(64)).unwrap(),
                source_kind: GraphSourceKind::GraphDocument,
            }),
            vec![],
            vec![],
        )],
        vec![],
        vec![],
    )
    .unwrap()
}

fn open(root: &Path) -> LocalEventRepository {
    LocalEventRepository::open(root, Arc::new(FixedClock), Arc::new(Ids::default())).unwrap()
}

fn stream(repository: &LocalEventRepository) -> Vec<EventEnvelope> {
    repository
        .read_stream(&scope(), "stream-contract", 100, None)
        .unwrap()
        .events
}

#[test]
fn a_lost_ack_after_commit_is_recovered_by_a_second_writer_without_a_second_event() {
    for failpoint in [LocalFailpoint::JournalSync, LocalFailpoint::ActiveMarker] {
        let directory = tempfile::tempdir().unwrap();
        let first = LocalEventRepository::open_with_failpoint(
            directory.path(),
            Arc::new(FixedClock),
            Arc::new(Ids::default()),
            failpoint,
        )
        .unwrap();
        let error = first
            .append_atomic(&request_for("operation-1"))
            .unwrap_err();
        assert_eq!(error.code(), "GHE008_STORAGE_FAILURE", "{failpoint:?}");
        drop(first);

        let resumed = open(directory.path());
        let recovered = resumed
            .append_atomic(&request_for("operation-1"))
            .unwrap_or_else(|error| panic!("{failpoint:?}: resume refused: {}", error.code()));
        assert_eq!(stream(&resumed), recovered, "{failpoint:?}");
        assert_eq!(recovered.len(), 1, "{failpoint:?}");
        assert_eq!(
            recovered[0].idempotency_key.as_str(),
            "operation-1",
            "{failpoint:?}"
        );
        let again = resumed.append_atomic(&request_for("operation-1")).unwrap();
        assert_eq!(again, recovered, "{failpoint:?}");
        assert_eq!(stream(&resumed).len(), 1, "{failpoint:?}");
    }
}

#[test]
fn a_crash_mid_write_is_refused_on_resume_and_never_becomes_an_event() {
    let directory = tempfile::tempdir().unwrap();
    let first = LocalEventRepository::open_with_failpoint(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
        LocalFailpoint::PhysicalBatchAppend,
    )
    .unwrap();
    assert!(first.append_atomic(&request_for("operation-1")).is_err());
    drop(first);

    let error = LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    )
    .err()
    .expect("a torn journal tail must be refused, not repaired by guessing");
    assert_eq!(error.code(), "GHE005_INTEGRITY_FAILURE");
    let journal = std::fs::read(directory.path().join("journal.jsonl")).unwrap();
    assert!(
        !journal.contains(&b'\n'),
        "no complete batch line may exist"
    );
}

/// #1582: a crash after the last record's bytes but before its newline leaves a COMPLETE, valid
/// record with no terminator. Nothing else in the load can refuse it: the line parses and its hash
/// chain holds. So this is the one torn tail only the unterminated-tail check catches, and the
/// refusal must name that site. (`a_crash_mid_write_...` above tears mid-record, which the line
/// parser refuses first, so it stayed green with the check removed: #1580's planted defect.)
#[test]
fn a_complete_record_missing_only_its_newline_is_refused_as_an_unterminated_tail() {
    let directory = tempfile::tempdir().unwrap();
    let first = open(directory.path());
    first.append_atomic(&request_for("operation-1")).unwrap();
    drop(first);
    let journal = directory.path().join("journal.jsonl");
    let mut bytes = std::fs::read(&journal).unwrap();
    assert_eq!(
        bytes.pop(),
        Some(b'\n'),
        "a committed journal ends in a newline"
    );
    std::fs::write(&journal, &bytes).unwrap();

    match LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    ) {
        Err(graphhelm_events::EventRepositoryError::IntegrityAt(site)) => {
            assert_eq!(site, "load_state:journal-tail-unterminated");
        }
        Err(other) => panic!("refused for another reason: {}", other.code()),
        Ok(_) => panic!("an unterminated journal tail must be refused, not read on"),
    }
}

#[test]
fn two_writers_racing_one_operation_commit_exactly_one_event() {
    for round in 0..16 {
        let directory = tempfile::tempdir().unwrap();
        open(directory.path());
        let barrier = Arc::new(Barrier::new(2));
        let writers = (0..2)
            .map(|_| {
                let root = directory.path().to_path_buf();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    let repository = open(&root);
                    barrier.wait();
                    repository
                        .append_atomic(&request_for("operation-1"))
                        .map_err(|error| error.code())
                })
            })
            .collect::<Vec<_>>();
        let outcomes = writers
            .into_iter()
            .map(|writer| writer.join().unwrap())
            .collect::<Vec<_>>();
        let committed = stream(&open(directory.path()));
        assert_eq!(committed.len(), 1, "round {round}");
        for outcome in outcomes {
            assert_eq!(outcome, Ok(committed.clone()), "round {round}");
        }
    }
}
