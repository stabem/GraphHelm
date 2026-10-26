//! #1714 / #1721: the segmented journal's contracts, each against a single-file twin or the bytes
//! on disk. A handle's `rotate_at` makes a rotation cost a few batches instead of 100,000 events.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use chrono::{TimeZone, Utc};
use graphhelm_protocols::{
    ActorId, Clock, EventEnvelope, EventKind, EvidenceId, EvidenceReference, GraphImported,
    GraphSourceKind, IdGenerator, NewEvent, OpaqueId, PersistedActor, PersistedActorType,
    ProjectId, RawSha256, RepositoryScope, Sensitivity, WorkspaceId,
};
use sha2::{Digest as _, Sha256};

use super::super::{LocalEventRepository, SealedEvidence, TORN_TAIL_AWAITS_SITE, WrappedKey};
use super::{SEGMENTS_DIR, concatenated_journal, segment_file_name};
use crate::{EventRepositoryError, PreparedAppend};

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 26, 12, 0, 0).unwrap()
    }
}

/// One counter PER PREFIX, so a rotation's temp names do not shift the event ids of a segmented
/// store away from those of its single-file twin.
#[derive(Default)]
struct SequenceIds(Mutex<BTreeMap<&'static str, u64>>);

impl IdGenerator for SequenceIds {
    fn next_id(&self, prefix: &'static str) -> String {
        let mut counters = self.0.lock().unwrap();
        let counter = counters.entry(prefix).or_insert(0);
        *counter += 1;
        format!("{prefix}-{counter}")
    }
}

fn scope() -> RepositoryScope {
    RepositoryScope::new(
        WorkspaceId::parse("workspace-1").unwrap(),
        ProjectId::parse("project-1").unwrap(),
        Some(graphhelm_protocols::ExecutionId::parse("execution-1").unwrap()),
    )
}

fn open(path: &Path) -> LocalEventRepository {
    LocalEventRepository::open(path, Arc::new(FixedClock), Arc::new(SequenceIds::default()))
        .unwrap()
}

/// A handle that seals its active segment once it holds `events` events.
fn rotating(path: &Path, events: usize) -> LocalEventRepository {
    let mut repository = open(path);
    repository.rotate_at = Some((u64::MAX, events));
    repository
}

fn new_event(key: &str, digest: char, evidence: Option<EvidenceReference>) -> NewEvent {
    NewEvent::new(
        OpaqueId::parse(key).unwrap(),
        PersistedActor::new(
            PersistedActorType::System,
            ActorId::parse("system-1").unwrap(),
        ),
        Sensitivity::Internal,
        EventKind::GraphImported(GraphImported {
            source_sha256: RawSha256::parse(digest.to_string().repeat(64)).unwrap(),
            source_kind: GraphSourceKind::GraphDocument,
        }),
        evidence.into_iter().collect(),
        vec![],
    )
}

fn request(expected: u64, key: &str, digest: char) -> PreparedAppend {
    PreparedAppend::new(
        scope(),
        OpaqueId::parse("stream-1").unwrap(),
        expected,
        vec![new_event(key, digest, None)],
        vec![],
        vec![],
    )
    .unwrap()
}

/// Appends batches `1..=count`, one event each, keyed `request-<n>`.
fn append_batches(repository: &LocalEventRepository, from: u64, count: u64) {
    for sequence in from..from + count {
        repository
            .append_atomic(&request(sequence, &format!("request-{sequence}"), 'a'))
            .unwrap();
    }
}

fn replay(repository: &LocalEventRepository) -> Vec<EventEnvelope> {
    repository.read_replay_stream(&scope(), "stream-1").unwrap()
}

fn sealed_names(root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root.join(SEGMENTS_DIR)) else {
        return Vec::new();
    };
    let mut names = entries
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.ends_with(".seal.json"))
        .collect::<Vec<_>>();
    names.sort();
    names
}

/// Every reader that walks history answers across sealed segments exactly as it answers over the
/// same history in one file, and the segments concatenate to that file byte for byte.
#[test]
fn history_reads_across_sealed_segments_equal_a_single_file_twin() {
    let segmented_dir = tempfile::tempdir().unwrap();
    let single_dir = tempfile::tempdir().unwrap();
    let segmented = rotating(segmented_dir.path(), 2);
    let single = open(single_dir.path());
    append_batches(&segmented, 1, 7);
    append_batches(&single, 1, 7);

    assert_eq!(
        sealed_names(segmented_dir.path()),
        [
            "00000000.seal.json",
            "00000001.seal.json",
            "00000002.seal.json"
        ],
        "seven one-event batches at two events per segment seal three segments"
    );
    assert!(sealed_names(single_dir.path()).is_empty());

    let expected = replay(&single);
    assert_eq!(expected.len(), 7);
    assert_eq!(replay(&segmented), expected);
    drop(segmented);
    let reopened = open(segmented_dir.path());
    assert_eq!(replay(&reopened), expected, "a fresh handle reads the same");

    // The open walked only the active segment: its state holds one batch, the rest is sealed.
    let state = reopened.load_state("test").unwrap();
    assert_eq!(state.batches.len(), 1);
    assert_eq!(state.sealed.len(), 3);

    let mut paged = Vec::new();
    let mut cursor = None;
    loop {
        let page = reopened
            .read_stream(&scope(), "stream-1", 2, cursor.as_deref())
            .unwrap();
        paged.extend(page.events);
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    assert_eq!(paged, expected, "pages cross segment boundaries in order");
    assert_eq!(
        reopened.read_stream_after(&scope(), "stream-1", 3).unwrap(),
        single.read_stream_after(&scope(), "stream-1", 3).unwrap()
    );
    assert_eq!(
        reopened.list_streams().unwrap(),
        single.list_streams().unwrap()
    );
    assert_eq!(
        reopened.read_unique_replay_stream().unwrap(),
        single.read_unique_replay_stream().unwrap()
    );
    assert_eq!(
        reopened.next_sequence(&scope(), "stream-1").unwrap(),
        8,
        "the head comes from the active segment seeded by the last seal"
    );

    // The lossless downgrade: the segments in order ARE the single-file journal.
    assert_eq!(
        concatenated_journal(segmented_dir.path()).unwrap(),
        std::fs::read(single_dir.path().join("journal.jsonl")).unwrap()
    );
}

/// An exact retry of a batch committed in a SEALED segment answers its stored events, and a
/// divergent reuse of that key is still a conflict: idempotency spans every segment.
#[test]
fn idempotency_spans_sealed_segments() {
    let directory = tempfile::tempdir().unwrap();
    let repository = rotating(directory.path(), 2);
    let first = repository
        .append_atomic(&request(1, "request-1", 'a'))
        .unwrap();
    append_batches(&repository, 2, 4);
    assert!(!sealed_names(directory.path()).is_empty());
    drop(repository);
    let repository = open(directory.path());
    assert!(
        repository.load_state("test").unwrap().batches.len() < 5,
        "request-1 is no longer in the active segment"
    );

    assert_eq!(
        repository
            .append_atomic(&request(1, "request-1", 'a'))
            .unwrap(),
        first
    );
    assert_eq!(
        repository
            .committed_events_for_idempotency_for_test("request-1")
            .unwrap(),
        Some(first)
    );
    assert!(matches!(
        repository.append_atomic(&request(6, "request-1", 'b')),
        Err(EventRepositoryError::IdempotencyConflict)
    ));
    assert_eq!(replay(&repository).len(), 5);
}

/// An existing single-file journal is not rewritten: the first rotation seals `journal.jsonl` in
/// place, byte for byte, and new batches go to the next segment.
#[test]
fn a_single_file_journal_becomes_segment_zero_in_place() {
    let directory = tempfile::tempdir().unwrap();
    let repository = open(directory.path());
    append_batches(&repository, 1, 5);
    drop(repository);
    let journal = directory.path().join("journal.jsonl");
    let before = std::fs::read(&journal).unwrap();
    assert!(!directory.path().join(SEGMENTS_DIR).exists());
    let history = replay(&open(directory.path()));

    let repository = rotating(directory.path(), 3);
    append_batches(&repository, 6, 1);
    drop(repository);

    assert_eq!(std::fs::read(&journal).unwrap(), before);
    assert_eq!(sealed_names(directory.path()), ["00000000.seal.json"]);
    let active = std::fs::read(
        directory
            .path()
            .join(SEGMENTS_DIR)
            .join(segment_file_name(1)),
    )
    .unwrap();
    assert_eq!(active.iter().filter(|byte| **byte == b'\n').count(), 1);

    let reopened = open(directory.path());
    let after = replay(&reopened);
    assert_eq!(after.len(), 6);
    assert_eq!(after[..5], history[..]);
    assert_eq!(
        reopened.inspect_layout_for_test(),
        crate::LocalRepositoryInspection::Recognized
    );
}

/// The two crash points of a rotation: before its seal (an unreferenced state file) and after it
/// (no next segment file). Both reopen to the same history and keep appending.
#[test]
fn an_interrupted_rotation_reopens_to_the_same_history() {
    let directory = tempfile::tempdir().unwrap();
    let repository = rotating(directory.path(), 100);
    append_batches(&repository, 1, 2);
    repository
        .with_exclusive_lock(|| {
            let state = repository.load_state("test")?;
            repository.rotate_segment(&state)
        })
        .unwrap();
    drop(repository);
    let segments = directory.path().join(SEGMENTS_DIR);
    // Crash after the seal and before the next segment existed.
    std::fs::remove_file(segments.join(segment_file_name(1))).unwrap();
    // Crash of a LATER rotation before its seal: its state file is published, nothing else.
    std::fs::write(
        segments.join(format!("00000001.{}.state.json", "0".repeat(64))),
        b"{}",
    )
    .unwrap();

    let reopened = open(directory.path());
    assert_eq!(replay(&reopened).len(), 2);
    assert!(segments.join(segment_file_name(1)).is_file());
    append_batches(&reopened, 3, 1);
    assert_eq!(replay(&reopened).len(), 3);
    drop(reopened);
    assert_eq!(replay(&open(directory.path())).len(), 3);
}

/// A sealed segment's bytes are checked by the reader that reads them: a flipped byte refuses a
/// replay that reaches it, a tail past it still answers, and a changed length refuses the open.
#[test]
fn a_tampered_sealed_segment_is_refused_where_it_is_read() {
    let directory = tempfile::tempdir().unwrap();
    let repository = rotating(directory.path(), 2);
    append_batches(&repository, 1, 4);
    drop(repository);
    let journal = directory.path().join("journal.jsonl");
    let mut bytes = std::fs::read(&journal).unwrap();
    let position = bytes.iter().position(|byte| *byte == b'a').unwrap();
    bytes[position] = b'b';
    std::fs::write(&journal, &bytes).unwrap();

    let repository = open(directory.path());
    assert!(matches!(
        repository.read_replay_stream(&scope(), "stream-1"),
        Err(EventRepositoryError::IntegrityAt("segments:sealed-digest"))
    ));
    assert_eq!(
        repository
            .read_stream_after(&scope(), "stream-1", 3)
            .unwrap()
            .0
            .len(),
        1,
        "the tail lies past the tampered segment and is read without it"
    );
    drop(repository);

    bytes.pop();
    std::fs::write(&journal, &bytes).unwrap();
    assert!(matches!(
        LocalEventRepository::open(
            directory.path(),
            Arc::new(FixedClock),
            Arc::new(SequenceIds::default())
        ),
        Err(EventRepositoryError::IntegrityAt("segments:sealed-length"))
    ));
}

/// ADR-046 still holds on the ACTIVE segment: its torn tail is refused by an ordinary open and
/// trimmed by the Runtime's, verified from the last seal rather than from genesis.
#[test]
fn a_torn_tail_on_the_active_segment_is_recovered_from_the_last_seal() {
    let directory = tempfile::tempdir().unwrap();
    let repository = rotating(directory.path(), 2);
    append_batches(&repository, 1, 3);
    drop(repository);
    let active = directory
        .path()
        .join(SEGMENTS_DIR)
        .join(segment_file_name(1));
    let mut bytes = std::fs::read(&active).unwrap();
    bytes.extend_from_slice(b"{\"torn\":");
    std::fs::write(&active, &bytes).unwrap();

    assert!(matches!(
        LocalEventRepository::open(
            directory.path(),
            Arc::new(FixedClock),
            Arc::new(SequenceIds::default())
        ),
        Err(EventRepositoryError::IntegrityAt(site)) if site == TORN_TAIL_AWAITS_SITE
    ));
    let recovered = LocalEventRepository::open_recovering_torn_tail(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(SequenceIds::default()),
    )
    .unwrap();
    assert!(recovered.recovered_torn_tail().is_some());
    assert_eq!(replay(&recovered).len(), 3);
}

/// Evidence registered in a sealed segment stays referenceable after it: the walk of the active
/// segment verifies the blob the seal says is reachable.
#[test]
fn evidence_registered_in_a_sealed_segment_is_referenced_after_it() {
    let directory = tempfile::tempdir().unwrap();
    let repository = rotating(directory.path(), 1);
    let evidence = sealed_evidence();
    let reference = evidence.reference().clone();
    repository
        .append_atomic(
            &PreparedAppend::new(
                scope(),
                OpaqueId::parse("stream-1").unwrap(),
                1,
                vec![new_event("request-1", 'a', Some(reference.clone()))],
                vec![evidence],
                vec![],
            )
            .unwrap(),
        )
        .unwrap();
    append_batches(&repository, 2, 1);
    repository
        .append_atomic(
            &PreparedAppend::new(
                scope(),
                OpaqueId::parse("stream-1").unwrap(),
                3,
                vec![new_event("request-3", 'a', Some(reference.clone()))],
                vec![],
                vec![],
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(sealed_names(directory.path()).len(), 2);
    drop(repository);

    let reopened = open(directory.path());
    assert!(
        reopened
            .evidence_exists(&scope(), reference.evidence_id())
            .unwrap()
    );
    assert_eq!(replay(&reopened).len(), 3);
}

fn sealed_evidence() -> SealedEvidence {
    fn push(output: &mut Vec<u8>, field: &[u8]) {
        output.extend_from_slice(&u32::try_from(field.len()).unwrap().to_be_bytes());
        output.extend_from_slice(field);
    }
    let ciphertext = vec![7_u8; 16];
    let reference = EvidenceReference::new(
        EvidenceId::parse("evidence-1").unwrap(),
        RawSha256::parse("c".repeat(64)).unwrap(),
        RawSha256::parse(hex::encode(Sha256::digest(&ciphertext))).unwrap(),
    );
    let mut aad = Vec::new();
    push(&mut aad, b"graphhelm-evidence-aad-v1");
    push(&mut aad, b"workspace-1");
    push(&mut aad, b"project-1");
    aad.push(1);
    push(&mut aad, b"execution-1");
    push(&mut aad, b"evidence-1");
    push(&mut aad, b"1.0.0");
    push(&mut aad, b"application/json");
    push(&mut aad, b"internal");
    push(&mut aad, b"standard");
    push(
        &mut aad,
        format!("{}", reference.content_sha256()).as_bytes(),
    );
    let wrapped = WrappedKey::new(
        "key-1",
        "evidence-1",
        "xchacha20poly1305",
        vec![1_u8; 24],
        vec![2_u8; 48],
        RawSha256::parse(hex::encode(Sha256::digest(&aad))).unwrap(),
    )
    .unwrap();
    SealedEvidence::new(
        reference,
        scope(),
        "application/json",
        Sensitivity::Internal,
        "standard",
        "xchacha20poly1305",
        vec![3_u8; 24],
        ciphertext,
        wrapped,
    )
    .unwrap()
}

impl LocalEventRepository {
    fn committed_events_for_idempotency_for_test(
        &self,
        key: &str,
    ) -> Result<Option<Vec<EventEnvelope>>, EventRepositoryError> {
        <Self as crate::EventRepository>::committed_events_for_idempotency(
            self,
            &scope(),
            "stream-1",
            &OpaqueId::parse(key).unwrap(),
        )
    }

    fn inspect_layout_for_test(&self) -> crate::LocalRepositoryInspection {
        Self::inspect_repository(self.root()).unwrap()
    }
}

fn checkpoint_names(root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root.join(SEGMENTS_DIR)) else {
        return Vec::new();
    };
    let mut names = entries
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.ends_with(".checkpoint.json"))
        .collect::<Vec<_>>();
    names.sort();
    names
}

/// #1721 step 2: the checkpointed projection of a segmented stream is the full replay's, and
/// once its checkpoint exists a read never opens the sealed segments behind it again. Segment 0
/// is overwritten at its sealed length below, so the open still passes and every reader that
/// walks from genesis is refused at its digest; only the checkpoint can answer.
#[test]
fn a_projection_resumes_from_its_seal_checkpoint_without_reading_older_segments() {
    let directory = tempfile::tempdir().unwrap();
    let repository = rotating(directory.path(), 2);
    append_batches(&repository, 1, 7);
    let expected = crate::replay(&scope(), "stream-1", &replay(&repository)).unwrap();

    assert_eq!(
        repository.project_stream(&scope(), "stream-1").unwrap(),
        (expected.clone(), 7),
        "the checkpointed fold equals the replay from genesis"
    );
    assert_eq!(
        checkpoint_names(directory.path()).len(),
        1,
        "one checkpoint per stream: each newer seal's checkpoint retires the older one"
    );
    drop(repository);

    let journal = directory.path().join("journal.jsonl");
    let length = std::fs::metadata(&journal).unwrap().len();
    std::fs::write(&journal, vec![b'x'; usize::try_from(length).unwrap()]).unwrap();

    let reopened = rotating(directory.path(), 2);
    assert!(
        reopened.read_replay_stream(&scope(), "stream-1").is_err(),
        "a reader from genesis meets the overwritten segment 0"
    );
    assert_eq!(
        reopened.project_stream(&scope(), "stream-1").unwrap(),
        (expected, 7),
        "the checkpoint answers without segment 0"
    );

    // The tail past the checkpoint is still folded, event by event.
    reopened
        .append_atomic(&request(8, "request-8", 'b'))
        .unwrap();
    let (projection, head) = reopened.project_stream(&scope(), "stream-1").unwrap();
    assert_eq!(head, 8);
    assert_eq!(projection.stream_id.as_deref(), Some("stream-1"));
}

/// Checkpoints are disposable: deleted, or corrupted on disk, the next read folds the sealed
/// segments from the events again and answers exactly what the replay answers.
#[test]
fn a_missing_or_corrupt_checkpoint_rebuilds_from_the_events() {
    let directory = tempfile::tempdir().unwrap();
    let repository = rotating(directory.path(), 2);
    append_batches(&repository, 1, 7);
    let expected = crate::replay(&scope(), "stream-1", &replay(&repository)).unwrap();
    repository.project_stream(&scope(), "stream-1").unwrap();
    let names = checkpoint_names(directory.path());
    assert_eq!(names.len(), 1);
    let path = directory.path().join(SEGMENTS_DIR).join(&names[0]);

    std::fs::write(&path, b"{}").unwrap();
    assert_eq!(
        repository.project_stream(&scope(), "stream-1").unwrap(),
        (expected.clone(), 7),
        "a corrupt checkpoint is ignored, never trusted"
    );

    std::fs::remove_file(&path).unwrap();
    let reopened = open(directory.path());
    assert_eq!(
        reopened.project_stream(&scope(), "stream-1").unwrap(),
        (expected, 7),
        "with no checkpoint the fold starts from the first event"
    );
    assert_eq!(
        checkpoint_names(directory.path()),
        names,
        "and writes the same checkpoint again"
    );
}

/// A handle opened BEFORE another handle rotated the journal follows the new active segment:
/// its next append lands after the seal, in order, and a fresh open reads one history.
#[test]
fn a_handle_opened_before_another_handles_rotation_appends_to_the_new_segment() {
    let directory = tempfile::tempdir().unwrap();
    let rotator = rotating(directory.path(), 2);
    let stale = open(directory.path());
    append_batches(&rotator, 1, 3);
    assert_eq!(
        sealed_names(directory.path()),
        ["00000000.seal.json"],
        "the other handle sealed segment 0 after this one was opened"
    );

    stale.append_atomic(&request(4, "request-4", 'a')).unwrap();
    drop((rotator, stale));

    let reopened = open(directory.path());
    let history = replay(&reopened);
    assert_eq!(
        history
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
    let segment_one = std::fs::read_to_string(
        directory
            .path()
            .join(SEGMENTS_DIR)
            .join(segment_file_name(1)),
    )
    .unwrap();
    assert!(
        segment_one.contains("request-4"),
        "the stale handle's append is in segment 1, not in the sealed segment 0"
    );
    assert!(
        !std::fs::read_to_string(directory.path().join("journal.jsonl"))
            .unwrap()
            .contains("request-4")
    );
}
