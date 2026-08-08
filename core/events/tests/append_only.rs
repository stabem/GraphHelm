use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{TimeZone, Utc};
use graphhelm_events::{EventStore, JsonlEventStore};
use graphhelm_protocols::{Clock, EventKind, IdGenerator, NewEvent};

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 8, 12, 0, 0).unwrap()
    }
}

#[derive(Default)]
struct SequenceIds(AtomicU64);

impl IdGenerator for SequenceIds {
    fn next_id(&self, prefix: &'static str) -> String {
        format!("{prefix}-{}", self.0.fetch_add(1, Ordering::SeqCst) + 1)
    }
}

fn store(path: &std::path::Path) -> JsonlEventStore {
    JsonlEventStore::new(path, Arc::new(FixedClock), Arc::new(SequenceIds::default()))
}

fn events() -> Vec<NewEvent> {
    vec![
        NewEvent {
            idempotency_key: "import-1".into(),
            kind: EventKind::graph_imported("graph.yaml"),
        },
        NewEvent {
            idempotency_key: "simulation-1".into(),
            kind: EventKind::simulation_started(),
        },
    ]
}

#[test]
fn append_batch_preserves_order_and_rejects_stale_sequence() {
    let directory = tempfile::tempdir().unwrap();
    let store = store(&directory.path().join("events.jsonl"));
    let written = store.append_batch("exec-1", 1, &events()).unwrap();
    assert_eq!(
        written
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    let error = store.append_batch("exec-1", 1, &events()[..1]).unwrap_err();
    assert_eq!(error.code(), "GHE001_SEQUENCE_CONFLICT");
}

#[test]
fn fresh_handle_reads_committed_batch_and_ignores_uncommitted_tail() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("events.jsonl");
    store(&path).append_batch("exec-1", 1, &events()).unwrap();
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    file.write_all(b"{interrupted").unwrap();
    drop(file);

    let read = store(&path).read_stream("exec-1").unwrap();
    assert_eq!(read.len(), 2);
    assert_eq!(read[0].idempotency_key, "import-1");
}

#[test]
fn committed_checksum_corruption_never_returns_partial_success() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("events.jsonl");
    store(&path).append_batch("exec-1", 1, &events()).unwrap();
    let mut text = std::fs::read_to_string(&path).unwrap();
    text = text.replace("sha256:", "sha256:tampered-");
    std::fs::write(&path, text).unwrap();

    let error = store(&path).read_stream("exec-1").unwrap_err();
    assert_eq!(error.code(), "GHE002_CORRUPT_BATCH");
}

#[test]
fn exact_retry_with_original_sequence_returns_committed_envelopes() {
    let directory = tempfile::tempdir().unwrap();
    let store = store(&directory.path().join("events.jsonl"));
    let first = store.append_batch("exec-1", 1, &events()).unwrap();
    let retried = store.append_batch("exec-1", 1, &events()).unwrap();

    assert_eq!(retried, first);
    assert_eq!(store.read_stream("exec-1").unwrap().len(), 2);
}

#[test]
fn oversized_event_is_rejected_before_persistence() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("events.jsonl");
    let store = store(&path);
    let event = NewEvent {
        idempotency_key: "x".repeat(1_048_577),
        kind: EventKind::simulation_started(),
    };

    let error = store.append_batch("exec-1", 1, &[event]).unwrap_err();
    assert_eq!(error.code(), "GHE002_CORRUPT_BATCH");
    assert!(!path.exists());
}

#[test]
fn duplicate_keys_and_oversized_aggregate_batches_are_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let store = store(&directory.path().join("events.jsonl"));
    let duplicate = NewEvent {
        idempotency_key: "same".into(),
        kind: EventKind::simulation_started(),
    };
    assert_eq!(
        store
            .append_batch("exec-1", 1, &[duplicate.clone(), duplicate])
            .unwrap_err()
            .code(),
        "GHE002_CORRUPT_BATCH"
    );

    let aggregate: Vec<_> = (0..18)
        .map(|index| NewEvent {
            idempotency_key: format!("{index}-{}", "x".repeat(950_000)),
            kind: EventKind::simulation_started(),
        })
        .collect();
    assert_eq!(
        store
            .append_batch("exec-1", 1, &aggregate)
            .unwrap_err()
            .code(),
        "GHE002_CORRUPT_BATCH"
    );
    assert!(store.read_stream("exec-1").unwrap().is_empty());
}
