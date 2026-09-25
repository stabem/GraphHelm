//! A file another process holds open is IN FLIGHT, not abandoned — and never corrupt.
//!
//! `plan_reconcile` runs at open time on the shared (read) fast path, and opens every file in
//! `blobs/` and `.tmp/` with `DELETE` access in order to decide whether the store is clean. On
//! Windows an open requesting `DELETE` is refused with `ERROR_SHARING_VIOLATION` whenever any
//! existing handle was opened without `FILE_SHARE_DELETE` — and the site renamed every
//! `io::Error` it saw into `EventRepositoryError::Integrity`, so ordinary contention read back
//! to the caller as `GHE005_INTEGRITY_FAILURE` and an HTTP 500 (#311, #326).
//!
//! Held open is not the same as damaged. Reconcile exists to remove ABANDONED files; deleting a
//! file another process is still writing would be the bug, not the fix. So contention here means
//! "not plannable this cycle", and that counts as clean.
//!
//! These tests live in their own file rather than joining `recoverable_layout.rs`: that file
//! measures which parts of a layout may be RECOVERED, and this one measures what happens while
//! another handle is open. The two share a fixture archive, not a subject.
//!
//! The defect needs a race. **These guards do not** — holding a handle open is deterministic, so
//! every one of them is arranged by hand and fails on demand.

#![cfg(windows)]

use std::fs::File;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{TimeZone, Utc};
use graphhelm_events::{EventRepositoryError, LocalEventRepository};
use graphhelm_protocols::{Clock, IdGenerator};
use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};

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

/// The M05 acceptance run: a committed store whose history references sealed Evidence, so its
/// `blobs/` actually has files to hold. A synthetic store would prove the branch; this proves the
/// case that shipped.
fn committed_archive() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/acceptance/m05-run-2026-08-16/events")
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

/// A fresh copy of the archive with the three transient directories present.
fn archive_copy(directory: &Path) -> PathBuf {
    let events = directory.join("events");
    copy_tree(&committed_archive(), &events);
    for name in ["blobs", ".tmp", "active"] {
        std::fs::create_dir_all(events.join(name)).expect("the complete shape is constructible");
    }
    events
}

fn open(events: &Path) -> Result<LocalEventRepository, EventRepositoryError> {
    LocalEventRepository::open(
        events,
        Arc::new(FixedClock),
        Arc::new(SequenceIds::default()),
    )
}

/// Opens a handle the way an unrelated process would: readable, shareable for read and write,
/// and deliberately WITHOUT `FILE_SHARE_DELETE`. That single omission is what makes another
/// process's `DELETE`-access open fail, and it is the ordinary default — `std::fs::File::open`
/// shares even less than this.
fn hold_without_share_delete(path: &Path) -> File {
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(path)
        .expect("a handle on an existing file opens")
}

/// Holds a file the way an exclusive reader does: no sharing at all. `open_child_file` asks only
/// for `GENERIC_READ` and never for `DELETE`, so the #340 mechanism cannot make it fail — but a
/// handle that shares NOTHING refuses it outright, and that is deterministic.
///
/// Measured before it was relied on: a `FileShare::None` open is itself refused while another
/// handle is open with sharing, which is why this must be taken before the store opens, and why
/// the same fixture cannot be built for `link_retained_file_between` (see #311).
fn hold_exclusively(path: &Path) -> File {
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(path)
        .expect("an exclusive handle on an existing file opens")
}

/// One real evidence blob from the copied archive, with the precondition asserted rather than
/// assumed: an empty `blobs/` would make every test below pass while holding nothing at all.
fn a_blob_in(events: &Path) -> PathBuf {
    let mut names: Vec<PathBuf> = std::fs::read_dir(events.join("blobs"))
        .expect("blobs/ reads")
        .map(|entry| entry.expect("a blob entry reads").path())
        .filter(|path| path.is_file())
        .collect();
    names.sort();
    assert!(
        !names.is_empty(),
        "PRECONDITION: the archive must carry at least one blob, or holding one measures nothing"
    );
    names.remove(0)
}

/// An orphan temp of the shape reconcile owns: `blob-<64 hex>.tmp`.
fn orphan_temp_in(events: &Path) -> PathBuf {
    let path = events
        .join(".tmp")
        .join(format!("blob-{}.tmp", "a".repeat(64)));
    std::fs::write(&path, b"orphan").expect("the orphan temp is writable");
    path
}

/// The subject. Red before the fix: `Err(Integrity)`, because the failed open was renamed into an
/// integrity verdict — no bytes having been read to disagree with anything.
#[test]
fn a_blob_held_by_another_handle_is_not_an_integrity_failure() {
    // CONTROL FIRST: the same archive, nothing held, must open. Without this, a red below would
    // not distinguish "holding the file broke the open" from "this archive never opened".
    let control = tempfile::tempdir().unwrap();
    let control_events = archive_copy(control.path());
    open(&control_events).expect("CONTROL: the archive opens when no handle is held");

    let directory = tempfile::tempdir().unwrap();
    let events = archive_copy(directory.path());
    let blob = a_blob_in(&events);
    let _held = hold_without_share_delete(&blob);

    match open(&events) {
        Ok(_) => {}
        Err(error) => panic!(
            "a blob another handle holds is in flight, not corrupt, and must not read back as \
             an integrity failure: {error} (held: {})",
            blob.display()
        ),
    }
}

/// The skip is bounded to CONTENT: `is_digest_json_name` runs BEFORE the open, so contention can
/// never smuggle a foreign NAME past the scan.
///
/// **The held file is the foreign-named one, and that is the entire guard.** An earlier version
/// held a valid blob and left the foreign file unheld — the assertion was already sharp
/// (`UnsupportedFormat`, not `is_err()`), but the ARRANGEMENT never put the two rules in conflict:
///
/// ```text
///  name-first (today)   the name refuses                            -> UnsupportedFormat
///  open-first (hypot.)  the foreign file is NOT held, its open
///                       SUCCEEDS, and the name refuses next         -> UnsupportedFormat
/// ```
///
/// Same verdict either way, so the ORDER went untested. Holding the foreign file itself separates
/// them: under open-first its open would raise a sharing violation, return `Ok(None)`, and the
/// scan would skip it — name never examined, store opens **clean**. This is also the only shape in
/// which the deferral could actually smuggle something, which is why it is the shape worth pinning.
///
/// A guard can name its error precisely and still never put the subject in the state that would
/// break it. Found by K reviewing #340.
#[test]
fn a_foreign_name_refuses_even_when_that_very_file_is_held() {
    let directory = tempfile::tempdir().unwrap();
    let events = archive_copy(directory.path());

    let foreign = events.join("blobs").join("not-a-digest.txt");
    std::fs::write(&foreign, b"foreign").expect("the foreign file is writable");
    let _held = hold_without_share_delete(&foreign);

    assert!(
        matches!(open(&events), Err(EventRepositoryError::UnsupportedFormat)),
        "the name rule runs BEFORE the open, so holding the file cannot defer it"
    );
}

/// The skip did not disable reconcile: an orphan nobody holds is still swept.
#[test]
fn an_orphan_temp_nobody_holds_is_still_deleted() {
    let directory = tempfile::tempdir().unwrap();
    let events = archive_copy(directory.path());
    let orphan = orphan_temp_in(&events);

    open(&events).expect("the store opens");

    assert!(
        !orphan.exists(),
        "an abandoned temp is exactly what reconcile exists to remove"
    );
}

/// `open_child_file` is the sibling `local.rs:3203` had all along, thirty lines away and carrying
/// the same override: every failed open becomes an integrity verdict unless the caller was
/// creating. It named itself in #311's capture at run 63 of 300, and then a 300-run budget went
/// clean — so this fixture stops waiting for the race and makes the site fire on purpose.
///
/// Measured, not argued: with `journal.jsonl` held by a handle that shares nothing, the open is
/// refused with `SHARING_VIOLATION(32)` — no `DELETE` access requested anywhere, which is exactly
/// why #340's mechanism could not explain this site.
///
/// **A file another process holds is a statement about the machine, not about the bytes.** No
/// bytes were read, so nothing can disagree with what it should be — the crate's own
/// `From<io::Error>` already answers `Storage` for every `?`, and this site overrides that.
#[test]
fn a_file_held_without_sharing_is_a_storage_error_not_an_integrity_verdict() {
    let control = tempfile::tempdir().unwrap();
    let control_events = archive_copy(control.path());
    open(&control_events).expect("CONTROL: the archive opens when no handle is held");

    let directory = tempfile::tempdir().unwrap();
    let events = archive_copy(directory.path());
    let _held = hold_exclusively(&events.join("journal.jsonl"));

    match open(&events) {
        // The class is not enough. A nameless `Storage` cannot tell a sharing violation from a full
        // disk, and that is the whole of #824's failure #2: the provoked failure must NAME itself.
        Err(EventRepositoryError::StorageAt { site, os }) => {
            assert!(
                !site.is_empty() && site != "io",
                "a site this crate raised itself must name itself rather than fall back to the propagated label: site={site:?} os={os:?}"
            );
            assert!(
                os.is_some(),
                "a provoked OS failure must carry the OS code that caused it: site={site:?}"
            );
        }
        Err(EventRepositoryError::Storage) => panic!(
            "the held file still answers the NAMELESS variant, so the cause was discarded on the path this cell provokes"
        ),
        Err(other) => panic!(
            "a file another handle holds is contention, not corruption, and must not reach the caller as an integrity verdict: {other:?}"
        ),
        Ok(_) => panic!("the store opened, so the held file never reached the site under test"),
    }
}

/// The other half, and the one that must NOT move: a genuinely missing `journal.jsonl` **is** an
/// integrity signal, and the fix above must not buy its quiet by silencing this.
///
/// Green before the change and green after. It asserts the wire code rather than the variant on
/// purpose — the refusal is raised by a different site than the one under test, and pinning the
/// variant would pin which site answers instead of the property that matters.
#[test]
fn a_missing_journal_is_still_an_integrity_failure() {
    let directory = tempfile::tempdir().unwrap();
    let events = archive_copy(directory.path());
    std::fs::remove_file(events.join("journal.jsonl")).expect("the journal is removable");

    match open(&events) {
        Err(error) => assert_eq!(
            error.code(),
            "GHE005_INTEGRITY_FAILURE",
            "a missing journal is damage, and must keep saying so: {error:?}"
        ),
        Ok(_) => panic!("a store with no journal must refuse to open"),
    }
}

/// The semantics, asserted directly rather than described in a comment: a held orphan is DEFERRED,
/// not deleted. Deleting a file another process still has open would be the bug this skip avoids.
#[test]
fn an_orphan_temp_someone_holds_is_deferred_not_deleted() {
    let directory = tempfile::tempdir().unwrap();
    let events = archive_copy(directory.path());
    let orphan = orphan_temp_in(&events);
    let _held = hold_without_share_delete(&orphan);

    open(&events).expect("a held orphan does not fail the open");

    assert!(
        orphan.exists(),
        "in flight is not abandoned: a file another handle holds must survive this cycle"
    );
}
