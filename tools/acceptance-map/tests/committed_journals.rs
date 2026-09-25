//! Every committed journal under `docs/acceptance/` must be ACCOUNTED FOR by a test — either by
//! the store-layout suite next door, or by this file.
//!
//! #181: eleven journals are committed; three sit inside `events/` store layouts and are opened by
//! `committed_stores.rs`. The other eight are bare `journal.jsonl` bundles with no `events/` and no
//! `blobs/`, so `LocalEventRepository::open` does not apply to them — and **nothing parsed them at
//! all**. Their bytes are pinned by each bundle's `SHA256SUMS`, which proves the bytes did not move
//! and says nothing about whether they still mean anything.
//!
//! That is the exact blind spot `committed_stores.rs`'s own module doc was written to warn about:
//! *the artifact binding "cannot prove the bytes still mean anything, because it never opens the
//! store"* — and then it covers three archives from a hand-written list. Eight files sat inside the
//! described gap for the whole time the warning was in the file.
//!
//! **The accounting is the point, not the parse.** A hand-maintained list under-covers by DEFAULT
//! and in SILENCE: the eight were missed exactly by adding evidence without adding coverage, eight
//! times, with nothing failing. So the first thing this file does is walk the directory and refuse
//! to pass while any journal on disk is claimed by neither list.

use std::path::PathBuf;

use acceptance_map::repo_root;

/// Journals that live inside a store layout (`events/` with `blobs/`) and are opened and replayed
/// by `committed_stores.rs`. Listed here so this file can account for them without re-testing them:
/// duplicating that oracle would be worse than leaving the gap, because two oracles diverge in
/// silence.
const COVERED_BY_STORE_SUITE: &[&str] = &[
    "docs/acceptance/m05-run-2026-08-16",
    "docs/acceptance/m06-run-2026-08-17",
    "docs/acceptance/demos/m06-fixture-journey",
];

/// Bare-journal bundles this file parses itself.
///
/// This list does NOT buy coverage on its own — `every_journal_this_file_claims_is_actually_parsed`
/// reads every line of every entry, so a name added here without a readable journal behind it fails
/// rather than passing quietly. That is the whole difference between this list and the one that
/// produced the gap: adding a name here creates an obligation instead of discharging one.
const PARSED_HERE: &[&str] = &[
    "docs/acceptance/m07-run-2026-08-17",
    "docs/acceptance/m08-run-2026-08-18",
    "docs/acceptance/m08-rejudge-2026-08-18",
    "docs/acceptance/m08-rejudge5-2026-08-18",
    "docs/acceptance/m08-rejudge6-2026-08-18",
    "docs/acceptance/m08-rejudge7-2026-08-18",
    "docs/acceptance/m08-rejudge8-2026-08-18",
    "docs/acceptance/m08-rejudge9-2026-08-18",
];

/// Every directory under `docs/acceptance/` that carries a committed journal, found by WALKING
/// rather than by being told.
///
/// Both layouts are collected by the same walk — `<dir>/journal.jsonl` and
/// `<dir>/events/journal.jsonl` — and normalised to the bundle directory, so a bundle cannot hide
/// from this list by choosing the other shape.
fn committed_journal_dirs() -> Vec<String> {
    let root = repo_root();
    let acceptance = root.join("docs/acceptance");
    let mut found = Vec::new();
    collect(&acceptance, &root, &mut found);
    found.sort();
    found.dedup();
    found
}

fn collect(dir: &std::path::Path, root: &std::path::Path, found: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, root, found);
        } else if path.file_name().and_then(|name| name.to_str()) == Some("journal.jsonl") {
            // Normalise `<bundle>/events/journal.jsonl` and `<bundle>/journal.jsonl` to `<bundle>`.
            let mut bundle: PathBuf = path
                .parent()
                .expect("a journal has a parent directory")
                .to_path_buf();
            if bundle.file_name().and_then(|name| name.to_str()) == Some("events") {
                bundle = bundle
                    .parent()
                    .expect("an events directory has a parent")
                    .to_path_buf();
            }
            let relative = bundle
                .strip_prefix(root)
                .expect("every journal found lives under the repository root")
                .to_string_lossy()
                .replace('\\', "/");
            found.push(relative);
        }
    }
}

/// The control that proves the walk looked at something.
///
/// Without it, a walker whose `read_dir` fails or whose path is wrong yields an empty list — and an
/// empty list is accounted for by every possible pair of lists, so the assertion below would pass
/// while measuring nothing. That is the same vacuous-green shape #181 reports, repeating inside
/// #181's own fix.
#[test]
fn the_walk_finds_the_journals_it_is_supposed_to_find() {
    let found = committed_journal_dirs();
    assert!(
        found.len() >= 10,
        "the walk over docs/acceptance/ found {} journal(s): {found:?}. Eleven were committed when \
         #181 was written, so a number this small means the walk is reading the wrong place, not \
         that the evidence was deleted.",
        found.len()
    );
    assert!(
        found
            .iter()
            .any(|dir| dir == "docs/acceptance/m05-run-2026-08-16"),
        "the walk did not find the m05 store journal, which is committed and has been for months: \
         the layout normalisation has stopped matching (found {found:?})"
    );
}

/// The second guard: the eight are actually PARSED, against the store's own canonical form.
///
/// Accounting alone would let a future author silence the test above by adding a name to a list —
/// green, and nothing read. This one reads every line of every journal in `PARSED_HERE` through
/// `graphhelm_events::journal_line_roundtrips`, which wraps the same parse, checksum and
/// canonical-byte comparison `load_state` performs. The failure it is built to catch: a
/// serialization change — a renamed serde field, a dropped `skip_serializing_if`, a changed enum
/// wire name — that leaves every other test green while these committed bytes stop meaning
/// anything.
///
/// It asserts per LINE and names the file and line number, because "some journal failed" sends the
/// next reader to grep eleven bundles.
#[test]
fn every_journal_this_file_claims_is_actually_parsed() {
    let root = repo_root();
    let mut silent: Vec<&str> = Vec::new();
    let mut lines_parsed = 0usize;

    for bundle in PARSED_HERE {
        let path = root.join(bundle).join("journal.jsonl");
        let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!("{bundle}: PARSED_HERE claims this journal, and it cannot be read: {error}")
        });

        let mut lines_here = 0usize;
        for (index, line) in text.lines().filter(|line| !line.is_empty()).enumerate() {
            graphhelm_events::journal_line_roundtrips(line).unwrap_or_else(|error| {
                panic!(
                    "{bundle}/journal.jsonl line {}: this committed line no longer round-trips \
                     through the store's canonical form ({error:?}). Its bytes did not move — \
                     SHA256SUMS still passes — so what changed is the code that reads them.",
                    index + 1
                )
            });
            lines_here += 1;
        }

        if lines_here == 0 {
            silent.push(bundle);
        }
        lines_parsed += lines_here;
    }

    // The control, counted in BUNDLES rather than in summed lines (found by L reviewing #256).
    //
    // The previous version asserted `checked >= 8` over lines summed across every bundle. That
    // answers "was anything read?", and the question is "was EVERY claimed bundle read?". With
    // `m07` alone holding 21 lines, the threshold was satisfied while the other seven journals sat
    // at zero bytes: `read_to_string` succeeds on an empty file, the inner loop runs zero times,
    // nothing panics, and the suite reported success having parsed one journal out of eight.
    //
    // Raising the number would not have fixed it. No threshold on a SUM can, because one large
    // bundle always covers for every silent one — the unit was wrong, not the value. That is the
    // defect #181 reports, reappearing inside the guard written to stop it: a green that cannot
    // tell "all eight read" from "one read, seven skipped".
    assert!(
        silent.is_empty(),
        "{} of {} bundle(s) in PARSED_HERE contributed no parsed line: {silent:?}\n\
         Each is claimed by this file and none of it was read. An empty or missing journal reads \
         as success under a summed line count, which is exactly what this assertion replaced. \
         ({lines_parsed} line(s) were parsed in total, all from the other bundles.)",
        silent.len(),
        PARSED_HERE.len()
    );
}

/// The guard #181 exists for.
///
/// Adding acceptance evidence without adding coverage must BREAK THE BUILD rather than pass
/// quietly. It passed quietly eight times.
#[test]
fn every_committed_journal_is_accounted_for_by_some_test() {
    let found = committed_journal_dirs();

    let unaccounted: Vec<&String> = found
        .iter()
        .filter(|dir| {
            !COVERED_BY_STORE_SUITE.contains(&dir.as_str()) && !PARSED_HERE.contains(&dir.as_str())
        })
        .collect();

    assert!(
        unaccounted.is_empty(),
        "{} committed journal(s) are parsed by NOTHING: {unaccounted:?}\n\
         Their bytes are pinned by SHA256SUMS, which proves they did not move and says nothing \
         about whether they still parse. Add each to PARSED_HERE and parse it, or to \
         COVERED_BY_STORE_SUITE if committed_stores.rs opens it.",
        unaccounted.len()
    );
}
