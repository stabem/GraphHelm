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
use serde_json::Value;
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
    assert_eq!(
        code, 0,
        "the restored store must OPEN; envelope: {value}"
    );
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
    assert_eq!(restore(&restored, &archive).0, 0, "restore must succeed first");

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
