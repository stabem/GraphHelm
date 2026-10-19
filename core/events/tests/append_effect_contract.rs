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
use sha2::Digest as _;

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
    request_at(operation, 1)
}

/// One `GraphImported` event for `operation`, appended where the stream's next sequence is `next`.
fn request_at(operation: &str, next: u64) -> PreparedAppend {
    PreparedAppend::new(
        scope(),
        OpaqueId::parse("stream-contract").unwrap(),
        next,
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

/// The Runtime's start-up open: the only one that may recover a torn tail (ADR-046).
fn open_recovering(root: &Path) -> LocalEventRepository {
    LocalEventRepository::open_recovering_torn_tail(
        root,
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    )
    .unwrap()
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

/// The entries of the store's sibling `<root>.quarantine` directory, sorted: (name, bytes).
fn quarantined(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut quarantine = root.as_os_str().to_owned();
    quarantine.push(".quarantine");
    let Ok(entries) = std::fs::read_dir(quarantine) else {
        return Vec::new();
    };
    let mut files = entries
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().to_string_lossy().into_owned(),
                std::fs::read(entry.path()).unwrap(),
            )
        })
        .collect::<Vec<_>>();
    files.sort();
    files
}

/// ADR-046 (#41): a crash mid-append leaves bytes after the journal's last newline that no writer
/// was ever acknowledged for (the append writes one newline-terminated line, syncs, THEN returns).
/// Open trims exactly those bytes, quarantines them first with a manifest, and serves the
/// committed history unchanged; the interrupted operation's retry appends exactly one event.
#[test]
fn a_crash_mid_write_is_trimmed_quarantined_and_retried_on_resume() {
    let directory = tempfile::tempdir().unwrap();
    let first = open(directory.path());
    first.append_atomic(&request_for("operation-1")).unwrap();
    drop(first);
    let journal = directory.path().join("journal.jsonl");
    let committed = std::fs::read(&journal).unwrap();

    let crashing = LocalEventRepository::open_with_failpoint(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
        LocalFailpoint::PhysicalBatchAppend,
    )
    .unwrap();
    assert!(
        crashing
            .append_atomic(&request_at("operation-2", 2))
            .is_err()
    );
    drop(crashing);
    let torn = std::fs::read(&journal).unwrap();
    assert!(torn.len() > committed.len() && torn.starts_with(&committed));
    let tail = torn[committed.len()..].to_vec();

    let resumed = open_recovering(directory.path());
    assert_eq!(
        resumed.recovered_torn_tail(),
        Some(graphhelm_events::RecoveredTail {
            trimmed_bytes: u64::try_from(tail.len()).unwrap(),
            sha256: hex::encode(sha2::Sha256::digest(&tail)),
            quarantine: quarantined(directory.path())[0].0.clone(),
        }),
        "the handle reports exactly what it trimmed"
    );
    assert_eq!(
        std::fs::read(&journal).unwrap(),
        committed,
        "only the tail was cut"
    );
    assert_eq!(
        stream(&resumed).len(),
        1,
        "committed history is served unchanged"
    );
    let files = quarantined(directory.path());
    assert_eq!(
        files.len(),
        2,
        "{:?}",
        files.iter().map(|f| &f.0).collect::<Vec<_>>()
    );
    let (bin, json) = (&files[0], &files[1]);
    assert!(bin.0.ends_with(".bin") && json.0.ends_with(".json"));
    assert_eq!(
        bin.1, tail,
        "the quarantine holds exactly the trimmed bytes"
    );
    let manifest: serde_json::Value = serde_json::from_slice(&json.1).unwrap();
    assert_eq!(manifest["schema"], "graphhelm.journal-tail-recovered/1");
    assert_eq!(manifest["trimmedBytes"], tail.len());
    assert_eq!(manifest["journalBytesAfter"], committed.len());
    assert_eq!(manifest["quarantine"], bin.0.as_str());

    let retried = resumed
        .append_atomic(&request_at("operation-2", 2))
        .unwrap();
    assert_eq!(retried.len(), 1);
    assert_eq!(
        stream(&resumed).len(),
        2,
        "the retry appends exactly one event"
    );
    drop(resumed);
    let again = open(directory.path());
    assert_eq!(stream(&again).len(), 2);
    assert_eq!(
        quarantined(directory.path()).len(),
        2,
        "a clean open trims nothing"
    );
}

/// ADR-046 (#41): the very first append torn leaves a journal with no newline at all, and the
/// recovery trims it to empty: nothing was ever committed.
#[test]
fn a_torn_first_append_is_trimmed_to_an_empty_journal() {
    let directory = tempfile::tempdir().unwrap();
    let crashing = LocalEventRepository::open_with_failpoint(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
        LocalFailpoint::PhysicalBatchAppend,
    )
    .unwrap();
    assert!(crashing.append_atomic(&request_for("operation-1")).is_err());
    drop(crashing);

    let resumed = open_recovering(directory.path());
    assert!(
        std::fs::read(directory.path().join("journal.jsonl"))
            .unwrap()
            .is_empty()
    );
    assert!(stream(&resumed).is_empty());
    assert_eq!(quarantined(directory.path()).len(), 2);
}

/// ADR-046 (#41) decision 2: only an UNTERMINATED suffix is recoverable. A terminated line that
/// fails verification is committed history gone bad, and open still refuses it; nothing is
/// trimmed or quarantined.
#[test]
fn a_corrupt_terminated_line_is_still_refused_and_nothing_is_trimmed() {
    let directory = tempfile::tempdir().unwrap();
    let first = open(directory.path());
    first.append_atomic(&request_for("operation-1")).unwrap();
    drop(first);
    let journal = directory.path().join("journal.jsonl");
    let mut bytes = std::fs::read(&journal).unwrap();
    let middle = bytes.len() / 2;
    bytes[middle] = if bytes[middle] == b'a' { b'b' } else { b'a' };
    bytes.extend_from_slice(b"{\"torn\":");
    std::fs::write(&journal, &bytes).unwrap();

    let error = LocalEventRepository::open_recovering_torn_tail(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    )
    .err()
    .expect("a corrupt committed line must be refused");
    // Which integrity refusal names the damage depends on the byte that broke (the batch checksum
    // or the line format); either refuses, and nothing below may be trimmed.
    assert!(
        matches!(
            error.code(),
            "GHE002_CORRUPT_BATCH" | "GHE005_INTEGRITY_FAILURE"
        ),
        "{}",
        error.code()
    );
    assert_eq!(
        std::fs::read(&journal).unwrap(),
        bytes,
        "nothing was trimmed"
    );
    assert!(quarantined(directory.path()).is_empty());
}

/// ADR-046 (#41) decision 5: a tail longer than one batch cannot be one interrupted append, so
/// something else wrote it; open refuses and trims nothing. (16 MiB is `MAX_BATCH_BYTES`.)
#[test]
fn a_tail_longer_than_one_batch_is_refused_and_nothing_is_trimmed() {
    let directory = tempfile::tempdir().unwrap();
    let first = open(directory.path());
    first.append_atomic(&request_for("operation-1")).unwrap();
    drop(first);
    let journal = directory.path().join("journal.jsonl");
    let mut bytes = std::fs::read(&journal).unwrap();
    bytes.extend(std::iter::repeat_n(b'x', 16 * 1024 * 1024 + 1));
    std::fs::write(&journal, &bytes).unwrap();

    match LocalEventRepository::open_recovering_torn_tail(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    ) {
        Err(graphhelm_events::EventRepositoryError::IntegrityAt(site)) => {
            assert_eq!(site, "load_state:journal-tail-oversized");
        }
        Err(other) => panic!("refused for another reason: {}", other.code()),
        Ok(_) => panic!("an oversized tail must be refused"),
    }
    assert_eq!(std::fs::read(&journal).unwrap().len(), bytes.len());
    assert!(quarantined(directory.path()).is_empty());
}

/// #1582, flipped by ADR-046 (#41): a crash after the last record's bytes but before its newline
/// leaves a COMPLETE, valid record with no terminator. The append writes the newline in the same
/// buffer and acknowledges only after the sync, so that record was never acknowledged. The
/// unterminated-tail check is still the only thing that can tell it apart from a committed line
/// (it parses and its hash chain holds), and it now routes the record to recovery: open trims it,
/// quarantines exactly its bytes, and serves an empty stream.
#[test]
fn a_complete_record_missing_only_its_newline_is_trimmed_as_an_unacknowledged_tail() {
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

    let resumed = open_recovering(directory.path());
    assert!(std::fs::read(&journal).unwrap().is_empty());
    assert!(stream(&resumed).is_empty());
    let files = quarantined(directory.path());
    assert_eq!(files.len(), 2);
    assert_eq!(
        files[0].1, bytes,
        "the quarantine holds exactly the unterminated record"
    );
}

/// ADR-046, as decided on #1626: only the Runtime's start-up open recovers. Every other open (a
/// CLI one-shot, an MCP read, a lane waiter) refuses the torn tail, names who recovers it, and
/// leaves the journal and the quarantine untouched.
#[test]
fn a_plain_open_refuses_a_torn_tail_for_the_runtime_to_recover() {
    let directory = tempfile::tempdir().unwrap();
    let crashing = LocalEventRepository::open_with_failpoint(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
        LocalFailpoint::PhysicalBatchAppend,
    )
    .unwrap();
    assert!(crashing.append_atomic(&request_for("operation-1")).is_err());
    drop(crashing);
    let journal = directory.path().join("journal.jsonl");
    let torn = std::fs::read(&journal).unwrap();

    match LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    ) {
        Err(graphhelm_events::EventRepositoryError::IntegrityAt(site)) => {
            assert_eq!(site, "open:journal-tail-awaits-runtime-recovery");
        }
        Err(other) => panic!("refused for another reason: {}", other.code()),
        Ok(_) => panic!("only the Runtime's start-up open may recover a torn tail"),
    }
    assert_eq!(
        std::fs::read(&journal).unwrap(),
        torn,
        "nothing was trimmed"
    );
    assert!(quarantined(directory.path()).is_empty());
}

/// #1626 review: a crash while quarantining can leave a SHORT final file (an older, non-atomic
/// write) or a stray temp file. Neither may turn into a permanent refusal to boot: the content is
/// fixed by the tail it records, so the recovery rewrites it completely and continues.
#[test]
fn a_short_leftover_quarantine_file_does_not_block_the_next_boot() {
    let directory = tempfile::tempdir().unwrap();
    let first = open(directory.path());
    first.append_atomic(&request_for("operation-1")).unwrap();
    drop(first);
    let journal = directory.path().join("journal.jsonl");
    let mut bytes = std::fs::read(&journal).unwrap();
    let committed = bytes.clone();
    bytes.extend_from_slice(b"{\"torn\":true,\"half");
    std::fs::write(&journal, &bytes).unwrap();
    let tail = bytes[committed.len()..].to_vec();
    let digest = hex::encode(sha2::Sha256::digest(&tail));
    let mut quarantine = directory.path().as_os_str().to_owned();
    quarantine.push(".quarantine");
    let quarantine = std::path::PathBuf::from(quarantine);
    std::fs::create_dir_all(&quarantine).unwrap();
    let name = format!("torn-tail-{}-{digest}", bytes.len());
    std::fs::write(quarantine.join(format!("{name}.bin")), &tail[..3]).unwrap();
    std::fs::write(quarantine.join(format!("{name}.bin.tmp-1-1")), b"stray").unwrap();

    let resumed = open_recovering(directory.path());
    assert_eq!(std::fs::read(&journal).unwrap(), committed);
    assert_eq!(stream(&resumed).len(), 1);
    assert_eq!(
        std::fs::read(quarantine.join(format!("{name}.bin"))).unwrap(),
        tail,
        "the short leftover was replaced by the complete tail"
    );
    assert!(quarantine.join(format!("{name}.json")).is_file());
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
