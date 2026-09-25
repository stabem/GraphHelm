//! `with_shared_lock` holds writers off for the whole closure (#664).
//!
//! The reader half of the store's lock became public so a snapshot spanning SEVERAL reads -
//! `events backup` reads `journal.jsonl` and then walks `blobs/` - cannot interleave with an
//! append. That guarantee is what the backup's consistency now rests on, and it had no cell:
//! `read_concurrency.rs` proves readers do not wait for READERS, which is the opposite question.
//!
//! Deterministic by a barrier, not by a sleep race: the reader signals that it is inside the
//! closure before the writer even tries, so the writer's wait is caused by the lock and by
//! nothing else. The one duration asserted is a floor far below the hold, for the reason
//! `read_concurrency.rs` gives about tight ratios being flakes wearing the clothes of a
//! guarantee.

use std::sync::{
    Arc, Barrier,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

use chrono::{TimeZone, Utc};
use graphhelm_events::{LocalEventRepository, PreparedAppend};
use graphhelm_protocols::{
    ActorId, Clock, EventKind, ExecutionId, ExecutionMode, ExecutionStarted, IdGenerator, NewEvent,
    OpaqueId, PersistedActor, PersistedActorType, ProjectId, RepositoryScope, Sensitivity,
    WireHash, WorkspaceId,
};

struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 3, 12, 0, 0).unwrap()
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
        Some(ExecutionId::parse("execution-lock").unwrap()),
    )
}

fn started(key: &str) -> NewEvent {
    NewEvent::new(
        OpaqueId::parse(key).unwrap(),
        PersistedActor::new(
            PersistedActorType::System,
            ActorId::parse("system-test").unwrap(),
        ),
        Sensitivity::Internal,
        EventKind::ExecutionStarted(ExecutionStarted {
            execution_id: OpaqueId::parse("execution-lock").unwrap(),
            graph_version: 1,
            graph_hash: WireHash::parse(format!("sha256:{}", "c".repeat(64))).unwrap(),
            mode: ExecutionMode::Supervised,
        }),
        vec![],
        vec![],
    )
}

/// How long the reader stays inside the closure. Long enough that a writer which did NOT wait
/// finishes far under it, short enough to stay a cheap cell.
const HOLD: Duration = Duration::from_millis(300);

/// The floor the writer's wait must clear. Half the hold: an append that waited is bounded below
/// by the remaining hold, and one that did not is bounded above by its own cost, measured in
/// single-digit milliseconds on this store.
const FLOOR: Duration = Duration::from_millis(150);

#[test]
fn an_append_waits_for_a_reader_holding_the_shared_lock() {
    let directory = tempfile::tempdir().unwrap();
    let repository = Arc::new(
        LocalEventRepository::open(
            directory.path(),
            Arc::new(FixedClock),
            Arc::new(Ids::default()),
        )
        .unwrap(),
    );

    // Two handles, not one: the in-process gate inside `with_lock` is per-INSTANCE, so a single
    // handle would serialise these two on a mutex and the cell would pass without the FILE lock
    // ever being the thing that blocked. The file lock is the subject.
    let writer = LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    )
    .unwrap();

    let inside = Arc::new(Barrier::new(2));
    let reader_repository = Arc::clone(&repository);
    let reader_inside = Arc::clone(&inside);

    let reader = std::thread::spawn(move || {
        reader_repository
            .with_shared_lock(|| {
                // The writer is released only once the lock is genuinely held, so what it waits
                // for is this closure and not the thread's own start-up.
                reader_inside.wait();
                std::thread::sleep(HOLD);
                Ok(())
            })
            .unwrap();
    });

    inside.wait();
    let request = PreparedAppend::new(
        scope(),
        OpaqueId::parse("stream-lock").unwrap(),
        1,
        vec![started("key-1")],
        vec![],
        vec![],
    )
    .unwrap();

    let started_at = Instant::now();
    writer.append_atomic(&request).unwrap();
    let waited = started_at.elapsed();

    reader.join().unwrap();

    assert!(
        waited >= FLOOR,
        "the append returned in {waited:?} while a reader held the shared lock for {HOLD:?}: \
         writers are not waiting for readers, so a snapshot taken under this lock could still be \
         torn by an append"
    );
}

/// The control the cell above needs. Without it, an append that is simply SLOW - on a loaded
/// machine, or on a filesystem having a bad day - would satisfy the floor with no lock involved
/// at all, and the guard would be measuring the machine rather than the lock.
#[test]
fn the_same_append_is_fast_when_no_reader_holds_the_lock() {
    let directory = tempfile::tempdir().unwrap();
    let repository = LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    )
    .unwrap();

    let request = PreparedAppend::new(
        scope(),
        OpaqueId::parse("stream-lock").unwrap(),
        1,
        vec![started("key-1")],
        vec![],
        vec![],
    )
    .unwrap();

    let started_at = Instant::now();
    repository.append_atomic(&request).unwrap();
    let waited = started_at.elapsed();

    assert!(
        waited < FLOOR,
        "an uncontended append took {waited:?}, at or past the {FLOOR:?} floor the contended cell \
         reads as evidence of waiting: that floor is measuring this machine, not the lock"
    );
}
