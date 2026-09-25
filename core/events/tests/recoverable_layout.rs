//! `.tmp/` and `active/` are recoverable on open; `blobs/` is not.
//!
//! A store used to refuse to open when its root entry set was incomplete, even though the same
//! function creates those directories routinely for a partial layout. Empty directories cannot
//! be carried by git, zip or rsync, so two committed acceptance stores answered
//! `GHE005_INTEGRITY_FAILURE` for their whole committed life while every checksum stayed green —
//! `SHA256SUMS` hashes files, and an empty directory has no file to hash (#75, #76).
//!
//! Three tests, and which one measures what is not obvious — it took two wrong claims to find
//! out. The first says the transient directories may be restored. The second says a store whose
//! sealed Evidence is missing refuses, but it survives every sabotage of the layout rule, because
//! the load resolves evidence references on its own. Only the THIRD pins the `blobs/`-strict
//! layout decision, and only because the fixture journey seals no Evidence at all — which leaves
//! the layout rule as the sole thing that can refuse.
//!
//! Each test names the sabotage that fells it, and every one of those was RUN, not reasoned.
//!
//! All three use REAL committed archives, copied to a temporary directory. A synthetic store
//! would prove the branch; only the archives prove the case that shipped broken.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{TimeZone, Utc};
use graphhelm_events::{EventRepositoryError, LocalEventRepository};
use graphhelm_protocols::{Clock, IdGenerator};

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 16, 12, 0, 0).unwrap()
    }
}

#[derive(Default)]
struct SequenceIds(AtomicU64);

impl IdGenerator for SequenceIds {
    fn next_id(&self, prefix: &'static str) -> String {
        format!("{prefix}-{}", self.0.fetch_add(1, Ordering::SeqCst) + 1)
    }
}

/// The M05 acceptance run: a committed store whose history references sealed Evidence.
fn committed_archive() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/acceptance/m05-run-2026-08-16/events")
}

/// The M06 fixture journey: a committed store that seals NO Evidence. That is what makes it the
/// only archive able to test the LAYOUT decision about `blobs/` — see the third test.
fn evidence_free_archive() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/acceptance/demos/m06-fixture-journey/events")
}

fn copy_tree(source: &Path, destination: &Path) {
    std::fs::create_dir_all(destination).expect("the destination directory is creatable");
    for entry in std::fs::read_dir(source).expect("the archive is readable") {
        let entry = entry.expect("a directory entry reads");
        let target = destination.join(entry.file_name());
        if entry.file_type().expect("a file type reads").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("an archive file copies");
        }
    }
}

/// Copies the archive and normalizes it to a COMPLETE layout before the test removes anything.
///
/// Both the presence and the absence of these directories are made explicit on purpose. A
/// developer machine that has run the acceptance suite carries them untracked while a fresh
/// clone does not, so a fixture that inherited whichever state happened to be on disk would
/// decide by accident of local history what each test is actually measuring.
fn complete_archive_copy(directory: &Path) -> PathBuf {
    complete_copy_of(&committed_archive(), directory)
}

fn complete_copy_of(archive: &Path, directory: &Path) -> PathBuf {
    let events = directory.join("events");
    copy_tree(archive, &events);
    for name in ["blobs", ".tmp", "active"] {
        std::fs::create_dir_all(events.join(name)).expect("the complete shape is constructible");
    }
    events
}

fn remove_directories(events: &Path, names: &[&str]) {
    for name in names {
        let path = events.join(name);
        assert!(
            path.is_dir(),
            "the fixture removes {name}, so it must exist"
        );
        std::fs::remove_dir_all(&path).expect("a fixture directory is removable");
    }
}

fn open(events: &Path) -> Result<LocalEventRepository, EventRepositoryError> {
    LocalEventRepository::open(
        events,
        Arc::new(FixedClock),
        Arc::new(SequenceIds::default()),
    )
}

/// Half one: the transient directories are workspace, not evidence, so their absence is
/// recoverable and the recorded history still replays unchanged.
///
/// Red before the fix: `GHE005_INTEGRITY_FAILURE`, which is what `main` answered for every
/// committed archive. Sabotage after it: delete the `RecoverableDirs` arm and this returns to
/// that refusal.
#[test]
fn a_store_missing_its_transient_directories_opens_and_still_replays_its_history() {
    let directory = tempfile::tempdir().unwrap();
    let events = complete_archive_copy(directory.path());
    remove_directories(&events, &[".tmp", "active"]);

    let store = open(&events).expect("a store missing only .tmp and active opens");

    let streams = store.list_streams().expect("the store lists its streams");
    let stream = streams
        .into_iter()
        .find(|stream| stream.stream_id == "exec-m05-acceptance")
        .expect("the recorded stream is present");
    let history = store
        .read_replay_stream(&stream.scope, &stream.stream_id)
        .expect("the recorded stream reads");
    assert_eq!(
        history.len(),
        12,
        "recovering the shape must not change the history"
    );

    assert!(
        events.join(".tmp").is_dir() && events.join("active").is_dir(),
        "the missing directories are restored on open"
    );
    assert_eq!(
        std::fs::read(events.join("format.json")).expect("format.json reads"),
        std::fs::read(committed_archive().join("format.json")).expect("the archive's reads"),
        "recovery writes no file bytes — only the two empty directories"
    );
}

/// Evidence that is gone stays gone: a store whose history references sealed Evidence and whose
/// `blobs/` is missing refuses, and it refuses for the DEEPER reason — `load_state` resolves every
/// evidence reference through `read_verified_blob`, so the missing blob file is caught during the
/// load whatever the layout policy says.
///
/// MEASURED, not reasoned: this test survives BOTH sabotages of the layout decision (predicate
/// only, and predicate plus creation loop). It therefore does NOT pin the `blobs/`-strict layout
/// rule — the test below it does. Two claims about this test have already been wrong, mine and
/// the reviewer's, and both were plausible: the refusal simply arrives from a second, independent
/// defence. It is kept because that defence is worth pinning, and renamed so nobody reads it as
/// the layout guard again.
#[test]
fn a_store_whose_sealed_evidence_is_missing_refuses_during_the_load() {
    // The fixture property this test rests on: the archive's history really does reference
    // sealed Evidence. Pinned here, because if the archive were ever replaced by one that seals
    // nothing, the assertion below would still pass while measuring nothing.
    let intact_directory = tempfile::tempdir().unwrap();
    let intact = complete_archive_copy(intact_directory.path());
    let store = open(&intact).expect("the complete archive opens");
    let streams = store.list_streams().expect("the store lists its streams");
    let stream = streams
        .into_iter()
        .find(|stream| stream.stream_id == "exec-m05-acceptance")
        .expect("the recorded stream is present");
    let history = store
        .read_replay_stream(&stream.scope, &stream.stream_id)
        .expect("the recorded stream reads");
    assert!(
        history.iter().any(|event| !event.evidence_refs.is_empty()),
        "the archive must reference sealed Evidence, or this test measures nothing"
    );

    let directory = tempfile::tempdir().unwrap();
    let events = complete_archive_copy(directory.path());
    remove_directories(&events, &["blobs"]);

    assert!(
        matches!(open(&events), Err(EventRepositoryError::Integrity)),
        "a missing evidence directory is a real signal, never recovered"
    );
}

/// The test that actually holds the `blobs/`-strict decision.
///
/// It uses the fixture journey, the one committed archive that seals NO Evidence, because that is
/// the only place where the LAYOUT rule is the sole thing standing between a missing `blobs/` and
/// a successful open. In an archive with sealed Evidence the load refuses on its own (see above),
/// which is exactly how two earlier sabotage claims about this pair came to be wrong.
///
/// SABOTAGE, RUN not asserted: add `"blobs"` to the recoverable predicate AND to the recovery
/// arm's creation loop in `local.rs`. The layout then reaches `Complete`, this archive has no
/// evidence to resolve, the store opens, and this test goes RED. Widening the predicate alone
/// leaves it GREEN, because the re-classification still sees `blobs` missing.
#[test]
fn an_evidence_free_store_missing_blobs_is_refused_by_the_layout_rule_itself() {
    let intact_directory = tempfile::tempdir().unwrap();
    let intact = complete_copy_of(&evidence_free_archive(), intact_directory.path());
    let store = open(&intact).expect("the complete fixture journey opens");
    let streams = store.list_streams().expect("the store lists its streams");
    let stream = streams
        .into_iter()
        .find(|stream| stream.stream_id == "demo-journey")
        .expect("the recorded stream is present");
    let history = store
        .read_replay_stream(&stream.scope, &stream.stream_id)
        .expect("the recorded stream reads");
    assert!(
        history.iter().all(|event| event.evidence_refs.is_empty()),
        "this archive must seal NO Evidence, or the layout rule is not what refuses below"
    );

    let directory = tempfile::tempdir().unwrap();
    let events = complete_copy_of(&evidence_free_archive(), directory.path());
    remove_directories(&events, &["blobs"]);

    assert!(
        matches!(open(&events), Err(EventRepositoryError::Integrity)),
        "a missing evidence directory is refused by the layout rule, never recovered"
    );
}
