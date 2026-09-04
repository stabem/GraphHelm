//! `events backup` / `events restore` must serve the LOCAL filesystem store, not only Postgres
//! (#336).
//!
//! The install story (#327) runs `graphhelm serve --events <dir>` — the local store — so an
//! operator who follows the documented install has no backup path at all. The dispatch already
//! exists and is already explicit: `single_selector` returns `Selector::Local` for `--repository`
//! and `Selector::Postgres` for `--config`, and `events verify` already uses it.
//!
//! **What the archive carries, and why it is smaller than it looks** (design amendment 2 on #336,
//! after L's review): `journal.jsonl` and `blobs/` — nothing that describes the root's own state.
//! `classify_layout` forgives only `.tmp` and `active` as missing; a root holding `format.json`
//! without `repository.lock` is a root claiming to be finished when it is not, and the store
//! answers `Err(Integrity)` on purpose. So a restore that wrote `format.json` would produce a
//! directory that never opens. Restore materialises the evidence; `open` initialises the rest.
//!
//! Hence the acceptance includes OPENING, not only comparing after an open that was assumed.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::{Value, json};
use tempfile::TempDir;

fn command() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
}

fn json_output(arguments: &[&str]) -> (i32, Value) {
    let output = command().args(arguments).output().unwrap();
    assert!(
        output.stderr.is_empty(),
        "stderr must stay empty: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value =
        serde_json::from_slice(&output.stdout).expect("every invocation prints one JSON envelope");
    (output.status.code().unwrap(), value)
}

/// The execution the m05 acceptance store holds, and the head it must replay to.
///
/// Named from `committed_stores.rs`, which opens the same archive and asserts the same identity —
/// so a fixture swapped for a different run fails here loudly instead of quietly measuring some
/// other history.
const EXECUTION: &str = "exec-m05-acceptance";
const HEAD_SEQUENCE: u64 = 12;

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("apps/cli lives two levels below the repository root")
        .to_path_buf()
}

/// A local store carrying REAL committed events, copied out of acceptance evidence.
///
/// `supported_format_repository` next door writes only `format.json`, which is right for the
/// argument-validation tests there and useless here: an EMPTY store round-trips trivially, so a
/// green over one proves nothing about restoring anything.
///
/// `repository.lock` is not copied. It belongs to a running process, not to the data.
fn populated_local_repository(directory: &TempDir) -> PathBuf {
    let source = repository_root().join("docs/acceptance/m05-run-2026-08-16/events");
    let root = directory.path().join("source");
    fs::create_dir_all(root.join("blobs")).unwrap();
    fs::copy(source.join("format.json"), root.join("format.json")).unwrap();
    fs::copy(source.join("journal.jsonl"), root.join("journal.jsonl")).unwrap();
    for entry in fs::read_dir(source.join("blobs")).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), root.join("blobs").join(entry.file_name())).unwrap();
    }
    root
}

fn journal_lines(root: &Path) -> usize {
    fs::read_to_string(root.join("journal.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.is_empty())
        .count()
}

fn blob_names(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(root.join("blobs"))
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn back_up(source: &Path, archive: &Path) -> (i32, Value) {
    json_output(&[
        "events",
        "backup",
        "--repository",
        source.to_str().unwrap(),
        "--output",
        archive.to_str().unwrap(),
    ])
}

fn restore(into: &Path, archive: &Path) -> (i32, Value) {
    json_output(&[
        "events",
        "restore",
        "--repository",
        into.to_str().unwrap(),
        "--archive",
        archive.to_str().unwrap(),
    ])
}

fn assert_archive_version_rejected_without_target_mutation(
    directory: &TempDir,
    archive_version: Option<Value>,
) {
    let archive = directory.path().join("unsupported.ghbak");
    let mut contents = json!({
        "journal": "",
        "blobs": {},
    });
    if let Some(archive_version) = archive_version {
        contents["archiveVersion"] = archive_version;
    }
    fs::write(&archive, serde_json::to_vec(&contents).unwrap()).unwrap();

    let destination = directory.path().join("must-remain-absent");
    assert!(
        !destination.exists(),
        "HARNESS-BROKE: rejection target already exists"
    );

    let (code, value) = restore(&destination, &archive);

    assert_eq!(code, 2, "unsupported archive must be refused: {value}");
    assert_eq!(
        value["diagnostics"][0]["code"], "GHCLI002_CONFIG_INVALID",
        "archive version refusal must use a stable diagnostic code: {value}"
    );
    assert_eq!(
        value["diagnostics"][0]["path"], "/archiveVersion",
        "archive version refusal must identify the archive field: {value}"
    );
    assert!(
        !destination.exists(),
        "rejected archive mutated the destination at {}",
        destination.display()
    );
}

#[test]
fn restore_rejects_a_missing_archive_version_before_target_mutation() {
    let directory = TempDir::new().unwrap();
    assert_archive_version_rejected_without_target_mutation(&directory, None);
}

#[test]
fn restore_rejects_a_non_string_archive_version_before_target_mutation() {
    let directory = TempDir::new().unwrap();
    assert_archive_version_rejected_without_target_mutation(&directory, Some(json!(1)));
}

#[test]
fn restore_rejects_an_unsupported_archive_version_before_target_mutation() {
    let directory = TempDir::new().unwrap();
    assert_archive_version_rejected_without_target_mutation(&directory, Some(json!("999.0.0")));
}

/// The guard #336 exists for: the operator's data comes back.
///
/// Fails today at the first step — `backup` requires `--config` and knows no local store.
#[test]
fn a_local_store_round_trips_through_backup_and_restore() {
    let directory = TempDir::new().unwrap();
    let source = populated_local_repository(&directory);

    // Landmark. Everything below is a claim about carrying data across, and over an empty store
    // every one of those claims is true for free. Name the numbers so a shrinking fixture fails
    // loudly instead of quietly weakening the acceptance.
    let source_lines = journal_lines(&source);
    let source_blobs = blob_names(&source);
    assert!(
        source_lines >= 10,
        "HARNESS-BROKE: the source holds {source_lines} journal line(s); a round trip over an empty \
         store passes without restoring anything"
    );
    assert!(
        source_blobs.len() >= 4,
        "HARNESS-BROKE: the source holds {} blob(s); blobs are what a restore most plausibly drops",
        source_blobs.len()
    );

    let archive = directory.path().join("store.ghbak");
    let (code, value) = back_up(&source, &archive);
    assert_eq!(
        code, 0,
        "events backup must accept a local store via --repository; envelope: {value}"
    );
    assert!(
        archive.is_file(),
        "backup reported success without writing {}",
        archive.display()
    );

    let restored = directory.path().join("restored");
    let (code, value) = restore(&restored, &archive);
    assert_eq!(
        code, 0,
        "events restore must accept a local store via --repository; envelope: {value}"
    );

    assert_eq!(
        journal_lines(&restored),
        source_lines,
        "the restored store holds a different number of journal lines than the source"
    );
    assert_eq!(
        blob_names(&restored),
        source_blobs,
        "the restored store holds a different set of blobs than the source"
    );

    // Opening is part of the acceptance, and the verb has to be one that OPENS.
    //
    // `events verify --repository` does NOT: it calls `LocalEventRepository::inspect_format`, a
    // read-only format check that answers `ok: true, formatSupported: true, verified: false` for a
    // directory that DOES NOT EXIST — measured. An earlier version of this test asserted opening
    // through that verb and would have passed over an absent store, which is precisely the vacuous
    // green this file argues against everywhere else.
    //
    // `execution status --events` constructs the store through `event_store`, so a root that
    // cannot be opened fails here rather than being reported as fine.
    let (code, value) = json_output(&[
        "execution",
        "status",
        "--events",
        restored.to_str().unwrap(),
        "--execution",
        EXECUTION,
    ]);
    assert_eq!(code, 0, "the restored store must OPEN; envelope: {value}");
    assert_eq!(value["ok"], true, "restored store failed to open: {value}");
    assert_eq!(
        value["data"]["headSequence"], HEAD_SEQUENCE,
        "the restored store opened but replayed a different history than the source: {value}"
    );
}

/// The cell that decides whether `active/` may be left out — which the round trip above CANNOT.
///
/// `status` and an `events` read resolve from the journal, so a round trip that omitted `active/`
/// would go green because the instrument cannot see the difference, and that green would be read as
/// "derived, safe to omit": a true conclusion resting on a reason with no power to be false.
///
/// The source already answers it, on `LayoutState::RecoverableDirs`: *"an active marker is
/// republished from history on every open"*, and those directories vanishing is how two committed
/// acceptance stores "spent their whole life unopenable while every checksum stayed green". This
/// test is what makes that claim FAIL if it stops being true.
#[test]
fn a_restored_store_rebuilds_its_active_markers_on_open() {
    let directory = TempDir::new().unwrap();
    let source = populated_local_repository(&directory);
    let archive = directory.path().join("store.ghbak");
    assert_eq!(back_up(&source, &archive).0, 0, "backup must succeed first");

    let restored = directory.path().join("restored");
    assert_eq!(
        restore(&restored, &archive).0,
        0,
        "restore must succeed first"
    );

    // The restore lets the store build the layout, so the marker directory is there from the
    // start. That is not what this test is about: the question is whether `active/` is REBUILT
    // from history, which is the property that lets a backup leave it out of the archive.
    //
    // An earlier version asserted the opposite — that restore leaves `format.json` and `active/`
    // absent — and it was written against a design the first run refuted: a root carrying evidence
    // without a format marker is `GHE007_UNSUPPORTED_FORMAT`, never `RecognizedPartial`.
    assert!(
        restored.join("active").is_dir(),
        "HARNESS-BROKE: the restored root has no active/ at all, so removing it below would not be \
         removing anything and the rebuild could not be observed"
    );
    std::fs::remove_dir_all(restored.join("active")).unwrap();
    assert!(
        !restored.join("active").exists(),
        "HARNESS-BROKE: active/ survived its own removal, so the assertion below would pass on a \
         directory that was never gone"
    );

    // Again the verb must OPEN. `events verify` only inspects the format, so it would neither
    // rebuild anything nor notice that it had not.
    let (code, value) = json_output(&[
        "execution",
        "status",
        "--events",
        restored.to_str().unwrap(),
        "--execution",
        EXECUTION,
    ]);
    assert_eq!(
        code, 0,
        "opening a restored store must succeed; envelope: {value}"
    );

    assert!(
        restored.join("active").is_dir(),
        "open did not rebuild active/ after it was removed. The markers are NOT republished from \
         history, which is the assumption that lets a backup leave them out of the archive — and \
         `LayoutState::RecoverableDirs` states that assumption in the store's own source"
    );
}

/// #601 — THE ESCALATION CELL, and it cannot run on this house's gate.
///
/// `execute_local` enumerated `blobs/` and called `read_to_string` on every entry, following
/// symlinks. A service account that can write `blobs/` could therefore aim a root-run backup at a
/// file only root can read, and the target's bytes land in the portable archive under the link's
/// name. Found by the Codex reviewer on #595, where the scheduled backup runs as root.
///
/// `#[cfg(unix)]` is not a preference. Measured on this machine:
/// `std::os::windows::fs::symlink_file` fails with *"A required privilege is not held by the
/// client. (os error 1314)"*, so the escalation cannot be PLANTED here and this cell never runs on
/// the Windows gate. It carries the real threat wherever a unix runner exists; the cell below
/// carries the weaker half that this gate can actually observe. Which half is load-bearing where
/// is written down rather than left to whoever reads a green suite.
#[cfg(unix)]
#[test]
fn backup_refuses_a_symlinked_blob_instead_of_following_it() {
    let directory = TempDir::new().unwrap();
    let source = populated_local_repository(&directory);
    let secret = directory.path().join("root-only.txt");
    fs::write(&secret, "SECRET-BYTES").unwrap();
    std::os::unix::fs::symlink(&secret, source.join("blobs").join("planted")).unwrap();

    let archive = directory.path().join("archive.json");
    let (code, value) = back_up(&source, &archive);

    assert_ne!(
        code, 0,
        "the backup followed a symlink out of blobs/ and archived what it pointed at: {value}"
    );
    assert!(
        !archive.exists()
            || !fs::read_to_string(&archive)
                .unwrap()
                .contains("SECRET-BYTES"),
        "the linked file's bytes reached the archive"
    );
}

/// #601 — THE WEAKER HALF, and it is the only one this gate can run.
///
/// A directory is the other non-regular entry and needs no privilege to plant. Before the guard it
/// also failed the backup -- `read_to_string` on a directory errors -- so the observable change
/// here is the REASON, not the refusal: *"a repository blob could not be read"* is a true statement
/// at the wrong grain, since the entry was read fine and simply is not a blob.
///
/// Recorded plainly: this cell does NOT demonstrate the escalation. It demonstrates that the guard
/// exists and classifies. A reader who takes a green here as proof the symlink hole is closed has
/// read more than it says.
#[test]
fn backup_refuses_a_blob_entry_that_is_not_a_regular_file() {
    let directory = TempDir::new().unwrap();
    let source = populated_local_repository(&directory);
    fs::create_dir(source.join("blobs").join("planted")).unwrap();

    let archive = directory.path().join("archive.json");
    let (code, value) = back_up(&source, &archive);

    assert_ne!(
        code, 0,
        "a non-regular entry in blobs/ must refuse: {value}"
    );
    let message = value["diagnostics"][0]["message"]
        .as_str()
        .unwrap_or_default();
    assert!(
        message.contains("not a regular file"),
        "the refusal must name what is wrong with the entry, not report a read failure that did \
         not happen; got {message:?}"
    );
}

/// A backup whose store carries `repository.lock` takes that lock, SHARED, for the whole capture
/// (#664).
///
/// The command read `journal.jsonl` and then walked `blobs/` with nothing between them, so a
/// writer appending in that gap produced a bundle whose halves never coexisted -- and the command
/// reported success. A backup that is silently inconsistent fails toward false confidence: it is
/// found at restore time, which is when there is nothing left to fall back to.
///
/// This is observable rather than structural because the capture no longer opens the store: the
/// lock in the path is the SNAPSHOT's, so holding it exclusively is the whole difference between
/// a locked capture and an unlocked one. Removing the lock makes this cell go red, which a cell
/// that also had `open`'s acquisition in front of it could not do.
#[test]
fn a_backup_waits_for_a_writer_holding_the_store_lock() {
    use fs2::FileExt;

    let directory = TempDir::new().unwrap();
    let source = populated_local_repository(&directory);
    // The fixture copies a store WITHOUT its lock file; a real store has one, and this cell is
    // about the case where there is a lock to take.
    fs::copy(
        repository_root().join("docs/acceptance/m05-run-2026-08-16/events/repository.lock"),
        source.join("repository.lock"),
    )
    .unwrap();

    let held = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(source.join("repository.lock"))
        .unwrap();
    held.lock_exclusive().unwrap();

    // ARRANGEMENT, asserted rather than assumed: a second handle must fail to take the same lock,
    // or the wait measured below is not a wait for anything.
    let probe = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(source.join("repository.lock"))
        .unwrap();
    assert!(
        probe.try_lock_exclusive().is_err(),
        "ARRANGEMENT: the exclusive lock is not actually held, so nothing below is contended"
    );

    let archive = directory.path().join("under-lock.ghbak");
    let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args([
            "events",
            "backup",
            "--repository",
            source.to_str().unwrap(),
            "--output",
            archive.to_str().unwrap(),
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    // Deliberately generous, and the claim is coarse on purpose: not "it waited exactly this
    // long" but "it had not finished while a writer held the lock". An unlocked capture of this
    // fixture completes in single-digit milliseconds.
    std::thread::sleep(std::time::Duration::from_millis(700));
    assert!(
        child.try_wait().unwrap().is_none(),
        "the backup completed while a writer held the store's exclusive lock: its capture is not \
         taking the lock, so an append between its two reads can still tear the bundle"
    );

    FileExt::unlock(&held).unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "once the writer releases, the backup must complete: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        value["data"]["lockHeld"],
        json!(true),
        "a store carrying repository.lock must report a locked capture: {value}"
    );
}

/// The control the cell above needs, and the one that keeps `lockHeld` honest: with nothing
/// contending, the same command completes promptly and still reports that it held the lock.
/// Without this, a backup that always blocked -- or one that reported `lockHeld` unconditionally
/// -- would satisfy the cell above.
#[test]
fn an_uncontended_backup_completes_and_reports_the_lock_it_held() {
    let directory = TempDir::new().unwrap();
    let source = populated_local_repository(&directory);
    fs::copy(
        repository_root().join("docs/acceptance/m05-run-2026-08-16/events/repository.lock"),
        source.join("repository.lock"),
    )
    .unwrap();

    let archive = directory.path().join("uncontended.ghbak");
    let (code, value) = back_up(&source, &archive);

    assert_eq!(code, 0, "an uncontended backup must succeed: {value}");
    assert_eq!(
        value["data"]["lockHeld"],
        json!(true),
        "the capture held the store's lock and must say so: {value}"
    );
}

/// A root with no `repository.lock` is archived, and the envelope SAYS the capture held no lock.
///
/// That shape is a restored-but-never-opened directory, and `open` refuses it rather than
/// creating the lock file -- so nothing can attach a writer to it while the capture runs. The
/// absence is reported rather than assumed: a snapshot that silently could not lock is the same
/// false confidence one level down.
#[test]
fn a_root_without_a_lock_file_is_archived_and_says_it_held_no_lock() {
    let directory = TempDir::new().unwrap();
    let source = populated_local_repository(&directory);
    assert!(
        !source.join("repository.lock").exists(),
        "ARRANGEMENT: this fixture is the never-opened shape, which is what makes it the subject"
    );

    let archive = directory.path().join("unlocked.ghbak");
    let (code, value) = back_up(&source, &archive);

    assert_eq!(code, 0, "a never-opened root must still archive: {value}");
    assert_eq!(
        value["data"]["lockHeld"],
        json!(false),
        "with no lock file there is no lock to hold, and the envelope must not claim one: {value}"
    );
}

/// The snapshot lock is SHARED, so another reader does not block the backup (#664).
///
/// This is what separates "hold the lock" from "hold the lock the right way". An exclusive
/// snapshot lock would also keep writers out - the contended cell above would still pass - while
/// serialising the whole store against every concurrent reader, including a second backup and
/// every `execution status`. Nothing else in this file would notice, so the distinction gets its
/// own cell rather than a comment.
#[test]
fn a_backup_is_not_blocked_by_another_reader() {
    use fs2::FileExt;

    let directory = TempDir::new().unwrap();
    let source = populated_local_repository(&directory);
    fs::copy(
        repository_root().join("docs/acceptance/m05-run-2026-08-16/events/repository.lock"),
        source.join("repository.lock"),
    )
    .unwrap();

    let reader = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(source.join("repository.lock"))
        .unwrap();
    FileExt::lock_shared(&reader).unwrap();

    // ARRANGEMENT, asserted rather than assumed: this shared lock must genuinely exclude a
    // WRITER, or it is not the store's lock and the cell measures nothing.
    let probe = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(source.join("repository.lock"))
        .unwrap();
    assert!(
        probe.try_lock_exclusive().is_err(),
        "ARRANGEMENT: the shared lock is not held, so nothing below is contended"
    );

    // SPAWNED WITH A BOUNDED WAIT rather than called through `back_up`, and the reason is a
    // receipt rather than a preference: the first version of this cell blocked on that helper,
    // and when the guard was sabotaged to take the lock EXCLUSIVE the cell did not go red - it
    // HUNG, at zero CPU, indefinitely (measured; found by a peer reading the process table while
    // the sabotage was applied). `cargo test` has no per-test timeout, so that stalls the suite
    // and, inside a gate stage, the gate. A red is a verdict; a hang is the absence of one, and
    // the absence is the failure that never reports itself.
    let archive = directory.path().join("shared.ghbak");
    let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args([
            "events",
            "backup",
            "--repository",
            source.to_str().unwrap(),
            "--output",
            archive.to_str().unwrap(),
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    // Generous against a capture measured in single-digit milliseconds. The claim is coarse on
    // purpose: not "it was fast" but "a reader did not stop it at all".
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let finished = loop {
        match child.try_wait().unwrap() {
            Some(status) => break Some(status),
            None if std::time::Instant::now() >= deadline => break None,
            None => std::thread::sleep(std::time::Duration::from_millis(20)),
        }
    };
    if finished.is_none() {
        let _ = child.kill();
    }
    FileExt::unlock(&reader).unwrap();
    let status = finished.expect(
        "a backup must take its snapshot lock SHARED: another reader holding the same lock must \
         not block it, or every concurrent read of this store serialises behind a backup - and an \
         EXCLUSIVE snapshot lock makes this WAIT FOREVER rather than fail",
    );

    let output = child.wait_with_output().unwrap();
    assert_eq!(
        status.code(),
        Some(0),
        "the backup ran beside a reader and must succeed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["data"]["lockHeld"], json!(true), "{value}");
}
