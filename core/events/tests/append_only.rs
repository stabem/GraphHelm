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
struct Ids(AtomicU64);
impl IdGenerator for Ids {
    fn next_id(&self, prefix: &'static str) -> String {
        format!("{prefix}-{}", self.0.fetch_add(1, Ordering::SeqCst) + 1)
    }
}

#[test]
fn exact_retry_preserves_the_committed_physical_journal_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let repository = LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    )
    .unwrap();
    let scope = RepositoryScope::new(
        WorkspaceId::parse("workspace-test").unwrap(),
        ProjectId::parse("project-test").unwrap(),
        Some(ExecutionId::parse("execution-test").unwrap()),
    );
    let request = PreparedAppend::new(
        scope,
        OpaqueId::parse("stream-test").unwrap(),
        1,
        vec![NewEvent::new(
            OpaqueId::parse("request-test").unwrap(),
            PersistedActor::new(
                PersistedActorType::System,
                ActorId::parse("system-test").unwrap(),
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
    .unwrap();
    let first = repository.append_atomic(&request).unwrap();
    let bytes = std::fs::read(directory.path().join("journal.jsonl")).unwrap();
    assert_eq!(repository.append_atomic(&request).unwrap(), first);
    assert_eq!(
        std::fs::read(directory.path().join("journal.jsonl")).unwrap(),
        bytes
    );
    assert_eq!(bytes.iter().filter(|byte| **byte == b'\n').count(), 1);
}
