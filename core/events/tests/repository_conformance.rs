use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use chrono::{TimeZone, Utc};
use graphhelm_events::{LocalEventRepository, PreparedAppend};
use graphhelm_protocols::{
    ActorId, Clock, EventKind, ExecutionId, GraphImported, GraphSourceKind, IdGenerator, NewEvent,
    OpaqueId, PersistedActor, PersistedActorType, ProjectId, RawSha256, RepositoryScope,
    Sensitivity, WorkspaceId,
};

struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 10, 12, 0, 0).unwrap()
    }
}

#[derive(Default)]
struct SequenceIds(AtomicU64);
impl IdGenerator for SequenceIds {
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

fn open(path: &std::path::Path) -> LocalEventRepository {
    LocalEventRepository::open(path, Arc::new(FixedClock), Arc::new(SequenceIds::default()))
        .unwrap()
}

fn event(key: &str, digest: char) -> NewEvent {
    NewEvent::new(
        OpaqueId::parse(key).unwrap(),
        PersistedActor::new(
            PersistedActorType::System,
            ActorId::parse("system-test").unwrap(),
        ),
        Sensitivity::Internal,
        EventKind::GraphImported(GraphImported {
            source_sha256: RawSha256::parse(digest.to_string().repeat(64)).unwrap(),
            source_kind: GraphSourceKind::GraphDocument,
        }),
        vec![],
        vec![],
    )
}

fn append(next: u64, events: Vec<NewEvent>) -> PreparedAppend {
    PreparedAppend::new(
        scope(),
        OpaqueId::parse("stream-test").unwrap(),
        next,
        events,
        vec![],
        vec![],
    )
    .unwrap()
}

#[test]
fn ordered_batches_reopen_page_and_reject_stale_sequence() {
    let directory = tempfile::tempdir().unwrap();
    let repository = open(directory.path());
    let first = repository
        .append_atomic(&append(
            1,
            vec![event("request-1", 'a'), event("request-2", 'b')],
        ))
        .unwrap();
    assert_eq!(
        first.iter().map(|item| item.sequence).collect::<Vec<_>>(),
        [1, 2]
    );

    let stale = repository
        .append_atomic(&append(1, vec![event("request-3", 'c')]))
        .unwrap_err();
    assert_eq!(stale.code(), "GHE001_SEQUENCE_CONFLICT");
    drop(repository);

    let reopened = open(directory.path());
    let first_page = reopened
        .read_stream(&scope(), "stream-test", 1, None)
        .unwrap();
    assert_eq!(first_page.events[0].sequence, 1);
    let second_page = reopened
        .read_stream(
            &scope(),
            "stream-test",
            1,
            first_page.next_cursor.as_deref(),
        )
        .unwrap();
    assert_eq!(second_page.events[0].sequence, 2);
    assert!(second_page.next_cursor.is_none());
}

#[test]
fn committed_journal_corruption_is_rejected_without_partial_success() {
    let directory = tempfile::tempdir().unwrap();
    open(directory.path())
        .append_atomic(&append(1, vec![event("request-1", 'a')]))
        .unwrap();
    let journal = directory.path().join("journal.jsonl");
    let mut bytes = std::fs::read(&journal).unwrap();
    let index = bytes.iter().position(|byte| *byte == b'a').unwrap();
    bytes[index] = b'f';
    std::fs::write(journal, bytes).unwrap();

    let error = match LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(SequenceIds::default()),
    ) {
        Ok(_) => panic!("corrupt journal unexpectedly opened"),
        Err(error) => error,
    };
    assert_eq!(error.code(), "GHE005_INTEGRITY_FAILURE");
}

#[test]
fn oversized_sparse_journal_is_rejected_from_metadata_before_read_allocation() {
    let directory = tempfile::tempdir().unwrap();
    drop(open(directory.path()));
    std::fs::OpenOptions::new()
        .write(true)
        .open(directory.path().join("journal.jsonl"))
        .unwrap()
        .set_len(64 * 1024 * 1024 + 1)
        .unwrap();
    let error = match LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(SequenceIds::default()),
    ) {
        Ok(_) => panic!("oversized journal unexpectedly opened"),
        Err(error) => error,
    };
    assert_eq!(error.code(), "GHE006_LIMIT_EXCEEDED");
}
