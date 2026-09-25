//! Every committed event store under `docs/acceptance/` must still OPEN and REPLAY against the
//! current build — not merely re-hash.
//!
//! The artifact binding (`verify_artifacts`) proves the bytes did not move. It cannot prove the
//! bytes still mean anything, because it never opens the store. Two archives cited as acceptance
//! evidence — the M05 owner-subscription run and the M06 dogfood run — answered
//! `GHE005_INTEGRITY_FAILURE` on open for their whole committed life while every existing check
//! stayed green: `SHA256SUMS` hashes FILES, and what was missing was two EMPTY DIRECTORIES that
//! git cannot carry. The refusal happens in `classify_layout` before a single journal byte is
//! parsed, so nothing about the recorded history was ever wrong.
//!
//! This is the same defect as #58, and the restore below is the same move
//! `replay_demonstration_store` makes for the same stated reason (#59). That fix reached the
//! demonstration binding because the demonstration binding had a test that could fail. The
//! artifact binding had none, and the M06 run is bound by no clause at all — so the stores below
//! are named by DIRECTORY rather than by binding.
//!
//! **What this file covers, stated exactly, because the previous wording did not.** It opens the
//! three store-layout archives listed in `archives()`, by hand. It used to claim it covered "every
//! committed store by directory"; that was true when written and false by the time anyone read it.
//! Eight bare-journal bundles were committed afterwards and this file never noticed, because a
//! hand-written list under-covers in silence — the sentence asserted a property the code did not
//! have, which is worse than saying nothing, since a reader checking for coverage found a sentence
//! saying it existed (#181).
//!
//! **The enumeration now lives next door.** `committed_journals.rs` WALKS `docs/acceptance/`,
//! finds every committed journal in both layouts, and fails on any that neither this file's list
//! nor its own accounts for — so adding evidence without adding coverage breaks the build. It
//! deliberately does not re-open the three archives below: duplicating this oracle would be worse
//! than the gap it closes, because two oracles diverge in silence.
//!
//! **What is still not enforced, so nobody reads the above as more than it is:** the two lists are
//! separate. Deleting an entry from `archives()` here leaves that store named in
//! `COVERED_BY_STORE_SUITE` there, and the accounting guard would go on reporting it as covered
//! while nothing opened it. The walk closes the ADD path, not the REMOVE path.
//!
//! Restoring an empty directory adds no content and can hide no loss: a real evidence blob is a
//! tracked FILE and survives checkout.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use acceptance_map::{repo_root, verify_artifacts};
use graphhelm_events::LocalEventRepository;
use graphhelm_protocols::{Clock, IdGenerator};

/// A committed archive and the identity the current build must reproduce from it.
struct Archive {
    /// Repo-relative directory holding `events/`.
    directory: &'static str,
    stream_id: &'static str,
    execution_id: &'static str,
    /// The head sequence the run ended on — the count of events the replay must read.
    head_sequence: u64,
}

/// Fixed clock: opening a store must not depend on wall-clock time.
struct FrozenClock;

impl Clock for FrozenClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp(0, 0).expect("the epoch is a valid instant")
    }
}

/// Counting IDs: no randomness in a verification run.
#[derive(Default)]
struct CountingIds(AtomicU64);

impl IdGenerator for CountingIds {
    fn next_id(&self, prefix: &'static str) -> String {
        format!("{prefix}-verify-{}", self.0.fetch_add(1, Ordering::SeqCst))
    }
}

/// Restores the empty child directories git cannot track, then opens.
///
/// Removing this loop is the sabotage that proves the test measures something: without it the
/// first two archives fail with `GHE005_INTEGRITY_FAILURE`, which is the state `main` is in as
/// this test is written. The red is the defect itself, not a constructed one.
fn restore_shape_and_open(events: &Path) -> LocalEventRepository {
    for child in ["blobs", ".tmp", "active"] {
        std::fs::create_dir_all(events.join(child))
            .expect("the committed store's empty shape is restorable");
    }
    LocalEventRepository::open(
        events,
        Arc::new(FrozenClock),
        Arc::new(CountingIds::default()),
    )
    .expect("a committed acceptance store opens against the current build")
}

/// Every store committed under `docs/acceptance/`, with the identity its run recorded. Listed by
/// DIRECTORY rather than read from `m05-clauses.toml`, deliberately: the M06 dogfood run is
/// bound by no clause, and a store that no binding names is exactly the one that rots unseen.
fn archives() -> Vec<Archive> {
    vec![
        Archive {
            directory: "docs/acceptance/m05-run-2026-08-16",
            stream_id: "exec-m05-acceptance",
            execution_id: "exec-m05-acceptance",
            head_sequence: 12,
        },
        Archive {
            directory: "docs/acceptance/m06-run-2026-08-17",
            stream_id: "exec-m06-dogfood",
            execution_id: "exec-m06-dogfood",
            head_sequence: 70,
        },
        Archive {
            directory: "docs/acceptance/demos/m06-fixture-journey",
            stream_id: "demo-journey",
            execution_id: "demo-journey",
            head_sequence: 10,
        },
    ]
}

#[test]
fn every_committed_acceptance_store_opens_and_replays_its_recorded_identity() {
    let root: PathBuf = repo_root();
    for archive in archives() {
        let events = root.join(archive.directory).join("events");
        let store = restore_shape_and_open(&events);

        let streams = store.list_streams().expect("the store lists its streams");
        let stream = streams
            .into_iter()
            .find(|stream| stream.stream_id == archive.stream_id)
            .unwrap_or_else(|| {
                panic!(
                    "{}: stream {} is absent",
                    archive.directory, archive.stream_id
                )
            });

        let history = store
            .read_replay_stream(&stream.scope, &stream.stream_id)
            .expect("the recorded stream reads");
        assert_eq!(
            history.len() as u64,
            archive.head_sequence,
            "{}: the archive replays a different number of events than the run recorded",
            archive.directory
        );
        assert_eq!(
            history.last().map(|event| event.sequence),
            Some(archive.head_sequence),
            "{}: the last recorded sequence moved",
            archive.directory
        );

        let projection = graphhelm_events::replay(&stream.scope, &stream.stream_id, &history)
            .unwrap_or_else(|error| {
                panic!(
                    "{}: the current build refuses the recorded history: {error:?}",
                    archive.directory
                )
            });
        assert_eq!(
            projection.execution_id.as_deref(),
            Some(archive.execution_id),
            "{}: the replayed execution identity moved",
            archive.directory
        );

        // Opening must not touch the evidence. Re-hashed in the SAME run, so a store that
        // rewrites what it reads is caught here rather than by a later reviewer's `git status`.
        let problems = verify_artifacts(&root, archive.directory);
        assert!(
            problems.is_empty(),
            "{}: opening the archive changed the committed evidence: {problems:?}",
            archive.directory
        );
    }
}
