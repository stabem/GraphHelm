//! Durable memory lifecycle events (#220).

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use chrono::{TimeZone, Utc};
use graphhelm_events::{
    LocalEventRepository, MemoryAdmissionRefusalAppend, MemoryPublicationTransitionAppend,
    MemoryRecordSupersededAppend, prepare_memory_admission_refusal,
    prepare_memory_publication_transition, prepare_memory_record_superseded, replay,
};
use graphhelm_protocols::{
    ActorId, Clock, IdGenerator, MemoryAdmissionLocal, MemoryAdmissionRefusalCode, OpaqueId,
    PersistedActor, PersistedActorType, PersistedMemoryPublicationState,
    PersistedMemoryPublicationTransition, PersistedMemorySemanticState,
    PersistedSupersessionReason, ProjectId, RepositoryScope, WorkspaceId,
};

const SENTINEL: &str = "ghp_G9SENTINELSECRETdoNotPersistMe0000000";
const STREAM: &str = "memory-admission";

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 28, 12, 0, 0).unwrap()
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
        WorkspaceId::parse("workspace-memory").unwrap(),
        ProjectId::parse("project-memory").unwrap(),
        None,
    )
}

fn actor() -> PersistedActor {
    PersistedActor::new(
        PersistedActorType::System,
        ActorId::parse("governor-memory").unwrap(),
    )
}

fn refusal(content: &str) -> MemoryAdmissionRefusalAppend {
    MemoryAdmissionRefusalAppend::new(
        scope(),
        OpaqueId::parse(STREAM).unwrap(),
        1,
        OpaqueId::parse("refusal-secret-detected").unwrap(),
        actor(),
        MemoryAdmissionRefusalCode::SecretDetected,
        MemoryAdmissionLocal::Content,
        u64::try_from(content.len()).unwrap(),
    )
}

#[test]
fn a_refusal_reopens_and_replays_with_only_code_local_and_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let content = format!("please remember my token {SENTINEL} for later");
    let prepared = prepare_memory_admission_refusal(refusal(&content)).unwrap();

    {
        let repository = LocalEventRepository::open(
            directory.path(),
            Arc::new(FixedClock),
            Arc::new(Ids::default()),
        )
        .unwrap();
        repository.append_atomic(&prepared).unwrap();
    }

    let journal = std::fs::read(directory.path().join("journal.jsonl")).unwrap();
    let journal_text = String::from_utf8(journal).unwrap();
    assert!(journal_text.contains("memory_admission_refused"));
    assert!(journal_text.contains("secret_detected"));
    assert!(journal_text.contains("content"));
    assert!(journal_text.contains(&format!("\"bytes\":{}", content.len())));
    assert!(!journal_text.contains(SENTINEL));
    assert!(!journal_text.contains("digest"));

    let reopened = LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    )
    .unwrap();
    let events = reopened.read_replay_stream(&scope(), STREAM).unwrap();
    assert_eq!(events.len(), 1);
    let projection = replay(&scope(), STREAM, &events).unwrap();
    assert_eq!(projection.memory_admission_refusal_count, 1);
    let receipt = projection
        .last_memory_admission_refusal
        .as_ref()
        .expect("replay lost the refusal receipt");
    assert_eq!(receipt.sequence, 1);
    assert_eq!(receipt.code, MemoryAdmissionRefusalCode::SecretDetected);
    assert_eq!(receipt.local, MemoryAdmissionLocal::Content);
    assert_eq!(receipt.bytes, content.len() as u64);

    let projected = serde_json::to_string(&projection).unwrap();
    assert!(!projected.contains(SENTINEL));
    assert!(!projected.contains("digest"));
}

#[test]
fn an_exact_retry_does_not_append_a_second_refusal() {
    let directory = tempfile::tempdir().unwrap();
    let content = format!("token {SENTINEL}");
    let prepared = prepare_memory_admission_refusal(refusal(&content)).unwrap();

    let first = {
        let repository = LocalEventRepository::open(
            directory.path(),
            Arc::new(FixedClock),
            Arc::new(Ids::default()),
        )
        .unwrap();
        repository.append_atomic(&prepared).unwrap()
    };
    let first_bytes = std::fs::read(directory.path().join("journal.jsonl")).unwrap();

    let reopened = LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    )
    .unwrap();
    let retry = reopened.append_atomic(&prepared).unwrap();
    let retry_bytes = std::fs::read(directory.path().join("journal.jsonl")).unwrap();

    assert_eq!(retry, first);
    assert_eq!(retry_bytes, first_bytes);
    assert_eq!(retry_bytes.iter().filter(|byte| **byte == b'\n').count(), 1);
    assert!(!String::from_utf8(retry_bytes).unwrap().contains(SENTINEL));
}

#[test]
fn conflicting_idempotency_reuse_leaves_the_original_refusal_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let original = prepare_memory_admission_refusal(refusal("secret")).unwrap();
    let conflicting = prepare_memory_admission_refusal(MemoryAdmissionRefusalAppend::new(
        scope(),
        OpaqueId::parse(STREAM).unwrap(),
        1,
        OpaqueId::parse("refusal-secret-detected").unwrap(),
        actor(),
        MemoryAdmissionRefusalCode::ScopeMismatch,
        MemoryAdmissionLocal::Scope,
        99,
    ))
    .unwrap();
    let repository = LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    )
    .unwrap();
    repository.append_atomic(&original).unwrap();
    let before = std::fs::read(directory.path().join("journal.jsonl")).unwrap();

    let error = repository
        .append_atomic(&conflicting)
        .expect_err("divergent input reused a committed idempotency key");
    let after = std::fs::read(directory.path().join("journal.jsonl")).unwrap();

    assert_eq!(error.code(), "GHE003_IDEMPOTENCY_CONFLICT");
    assert_eq!(after, before);
    assert_eq!(after.iter().filter(|byte| **byte == b'\n').count(), 1);
}

const LIFECYCLE_STREAM: &str = "memory-lifecycle";

fn publication_transition(record_id: &str) -> MemoryPublicationTransitionAppend {
    MemoryPublicationTransitionAppend::new(
        scope(),
        OpaqueId::parse(LIFECYCLE_STREAM).unwrap(),
        1,
        OpaqueId::parse("transition-publish").unwrap(),
        actor(),
        OpaqueId::parse(record_id).unwrap(),
        PersistedMemoryPublicationTransition::Publish,
        PersistedMemoryPublicationState::Published,
    )
}

#[test]
fn a_publication_transition_reopens_and_replays_into_the_keyed_projection() {
    let directory = tempfile::tempdir().unwrap();
    let prepared =
        prepare_memory_publication_transition(publication_transition("record-1")).unwrap();

    {
        let repository = LocalEventRepository::open(
            directory.path(),
            Arc::new(FixedClock),
            Arc::new(Ids::default()),
        )
        .unwrap();
        repository.append_atomic(&prepared).unwrap();
    }

    let journal = std::fs::read(directory.path().join("journal.jsonl")).unwrap();
    let journal_text = String::from_utf8(journal).unwrap();
    assert!(journal_text.contains("memory_publication_transitioned"));
    assert!(journal_text.contains("record-1"));
    assert!(journal_text.contains("publish"));
    assert!(journal_text.contains("published"));

    let reopened = LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    )
    .unwrap();
    let events = reopened
        .read_replay_stream(&scope(), LIFECYCLE_STREAM)
        .unwrap();
    assert_eq!(events.len(), 1);
    let projection = replay(&scope(), LIFECYCLE_STREAM, &events).unwrap();
    let record = projection
        .memory_records
        .get("record-1")
        .expect("replay lost the memory record's publication state");
    assert_eq!(
        record.publication,
        Some(PersistedMemoryPublicationState::Published)
    );
    assert_eq!(record.last_transition_sequence, Some(1));
}

#[test]
fn an_exact_retry_does_not_append_a_second_transition() {
    let directory = tempfile::tempdir().unwrap();
    let prepared =
        prepare_memory_publication_transition(publication_transition("record-1")).unwrap();

    let first = {
        let repository = LocalEventRepository::open(
            directory.path(),
            Arc::new(FixedClock),
            Arc::new(Ids::default()),
        )
        .unwrap();
        repository.append_atomic(&prepared).unwrap()
    };
    let first_bytes = std::fs::read(directory.path().join("journal.jsonl")).unwrap();

    let reopened = LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    )
    .unwrap();
    let retry = reopened.append_atomic(&prepared).unwrap();
    let retry_bytes = std::fs::read(directory.path().join("journal.jsonl")).unwrap();

    assert_eq!(retry, first);
    assert_eq!(retry_bytes, first_bytes);
    assert_eq!(retry_bytes.iter().filter(|byte| **byte == b'\n').count(), 1);
}

#[test]
fn conflicting_idempotency_reuse_leaves_the_original_transition_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let original =
        prepare_memory_publication_transition(publication_transition("record-1")).unwrap();
    let conflicting =
        prepare_memory_publication_transition(MemoryPublicationTransitionAppend::new(
            scope(),
            OpaqueId::parse(LIFECYCLE_STREAM).unwrap(),
            1,
            OpaqueId::parse("transition-publish").unwrap(),
            actor(),
            OpaqueId::parse("record-2").unwrap(),
            PersistedMemoryPublicationTransition::Withdraw,
            PersistedMemoryPublicationState::Withdrawn,
        ))
        .unwrap();
    let repository = LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    )
    .unwrap();
    repository.append_atomic(&original).unwrap();
    let before = std::fs::read(directory.path().join("journal.jsonl")).unwrap();

    let error = repository
        .append_atomic(&conflicting)
        .expect_err("divergent input reused a committed idempotency key");
    let after = std::fs::read(directory.path().join("journal.jsonl")).unwrap();

    assert_eq!(error.code(), "GHE003_IDEMPOTENCY_CONFLICT");
    assert_eq!(after, before);
    assert_eq!(after.iter().filter(|byte| **byte == b'\n').count(), 1);
}

fn supersession(predecessor_id: &str, successor_id: &str) -> MemoryRecordSupersededAppend {
    MemoryRecordSupersededAppend::new(
        scope(),
        OpaqueId::parse(LIFECYCLE_STREAM).unwrap(),
        1,
        OpaqueId::parse("supersession-1").unwrap(),
        actor(),
        OpaqueId::parse(predecessor_id).unwrap(),
        OpaqueId::parse(successor_id).unwrap(),
        PersistedSupersessionReason::Contradicted,
        PersistedMemorySemanticState::Contradicted,
    )
}

#[test]
fn a_supersession_reopens_and_replays_onto_both_records_independently() {
    let directory = tempfile::tempdir().unwrap();
    let prepared = prepare_memory_record_superseded(supersession("record-1", "record-2")).unwrap();

    {
        let repository = LocalEventRepository::open(
            directory.path(),
            Arc::new(FixedClock),
            Arc::new(Ids::default()),
        )
        .unwrap();
        repository.append_atomic(&prepared).unwrap();
    }

    let journal = std::fs::read(directory.path().join("journal.jsonl")).unwrap();
    let journal_text = String::from_utf8(journal).unwrap();
    assert!(journal_text.contains("memory_record_superseded"));
    assert!(journal_text.contains("record-1"));
    assert!(journal_text.contains("record-2"));
    assert!(journal_text.contains("contradicted"));

    let reopened = LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    )
    .unwrap();
    let events = reopened
        .read_replay_stream(&scope(), LIFECYCLE_STREAM)
        .unwrap();
    assert_eq!(events.len(), 1);
    let projection = replay(&scope(), LIFECYCLE_STREAM, &events).unwrap();

    // The PREDECESSOR's semantic axis moved. Its publication axis is untouched -- arrangement
    // proof that this event carries no publication field to fabricate one from (ADR-032
    // decision 3), not just that nothing happened to set it.
    let predecessor = projection
        .memory_records
        .get("record-1")
        .expect("replay lost the predecessor's semantic state");
    assert_eq!(
        predecessor.semantic,
        Some(PersistedMemorySemanticState::Contradicted)
    );
    assert_eq!(predecessor.publication, None);
    assert_eq!(predecessor.supersedes, None);

    // The SUCCESSOR records the relationship. Its own semantic axis is untouched -- supersede
    // moves the PREDECESSOR's belief state, never the successor's.
    let successor = projection
        .memory_records
        .get("record-2")
        .expect("replay lost the successor's supersedes reference");
    assert_eq!(
        successor.supersedes.as_ref().map(OpaqueId::as_str),
        Some("record-1")
    );
    assert_eq!(successor.semantic, None);
    assert_eq!(successor.publication, None);
}

#[test]
fn an_exact_retry_does_not_append_a_second_supersession() {
    let directory = tempfile::tempdir().unwrap();
    let prepared = prepare_memory_record_superseded(supersession("record-1", "record-2")).unwrap();

    let first = {
        let repository = LocalEventRepository::open(
            directory.path(),
            Arc::new(FixedClock),
            Arc::new(Ids::default()),
        )
        .unwrap();
        repository.append_atomic(&prepared).unwrap()
    };
    let first_bytes = std::fs::read(directory.path().join("journal.jsonl")).unwrap();

    let reopened = LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    )
    .unwrap();
    let retry = reopened.append_atomic(&prepared).unwrap();
    let retry_bytes = std::fs::read(directory.path().join("journal.jsonl")).unwrap();

    assert_eq!(retry, first);
    assert_eq!(retry_bytes, first_bytes);
    assert_eq!(retry_bytes.iter().filter(|byte| **byte == b'\n').count(), 1);
}

#[test]
fn conflicting_idempotency_reuse_leaves_the_original_supersession_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let original = prepare_memory_record_superseded(supersession("record-1", "record-2")).unwrap();
    let conflicting = prepare_memory_record_superseded(MemoryRecordSupersededAppend::new(
        scope(),
        OpaqueId::parse(LIFECYCLE_STREAM).unwrap(),
        1,
        OpaqueId::parse("supersession-1").unwrap(),
        actor(),
        OpaqueId::parse("record-3").unwrap(),
        OpaqueId::parse("record-4").unwrap(),
        PersistedSupersessionReason::Deprecated,
        PersistedMemorySemanticState::Deprecated,
    ))
    .unwrap();
    let repository = LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    )
    .unwrap();
    repository.append_atomic(&original).unwrap();
    let before = std::fs::read(directory.path().join("journal.jsonl")).unwrap();

    let error = repository
        .append_atomic(&conflicting)
        .expect_err("divergent input reused a committed idempotency key");
    let after = std::fs::read(directory.path().join("journal.jsonl")).unwrap();

    assert_eq!(error.code(), "GHE003_IDEMPOTENCY_CONFLICT");
    assert_eq!(after, before);
    assert_eq!(after.iter().filter(|byte| **byte == b'\n').count(), 1);
}
