//! Enforces a property of this crate's SOURCE that no runtime test can see.
//!
//! A documented control which no test enforces is not a control. Operator-facing strings are such
//! a control: nothing renders them except a human, so a defect in them leaves every test green.
//!
//! The defect: a line continuation inside a string literal keeps the NEXT line's indentation
//! inside the literal, and `cargo fmt` joins the pieces into one line. The source reads plausibly
//! while the reader of a failure gets `the assertion above is          satisfied by coincidence`.
//!
//! **This walk covers `tests/` as well as `src/`, and that is the point of adopting the guard
//! here.** A repository-wide census at the threshold that actually enforces found every `src/`
//! tree in the workspace already clean and every single instance of the class living in `tests/`.
//! A guard scoped to `src/` would therefore have been green on arrival and would have protected
//! nothing -- it would have been a control with no subject, which is the shape this file exists
//! to argue against.
//!
//! Scanning `tests/` means this file scans ITSELF, which is deliberate: a guard that exempts its
//! own file leaves a whole file unguarded, and the exemption is invisible from the failure
//! message. The cost is that every string written here must obey the rule it enforces, and the
//! `\` continuation idiom used below is what makes that possible -- Rust drops the newline and
//! the following indentation, so no run survives on any physical line.

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/source-invariants/detect.rs"
));

use std::path::{Path, PathBuf};

/// Every `.rs` file under `src/` AND `tests/`, discovered by WALKING the directories.
///
/// The population is the directory, not a list: a file ADDED to the crate must be scanned without
/// anyone remembering to name it. The risk that trades against -- a walk that silently returns
/// almost nothing -- is answered by the floor and the two landmarks below rather than by naming
/// files here.
fn sources() -> Vec<(String, String)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let entries =
            std::fs::read_dir(dir).unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut found = Vec::new();
    walk(&root.join("src"), &mut found);
    walk(&root.join("tests"), &mut found);
    found.sort();
    found
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
            let shown = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .display()
                .to_string();
            (shown, text)
        })
        .collect()
}

/// This crate's exemption, composed on top of the shared detection.
///
/// The detection lives in `tools/source-invariants/detect.rs` and is included by value; only the
/// EXEMPTION is a property of a crate, which is why the two are separate functions there.
/// Composing them here rather than importing a bundled predicate is what lets an exemption be
/// asserted to be doing work instead of merely having a subject.
///
/// This crate needs only the shared one. Every candidate the census turned up here is authored
/// prose in an assertion message; none of the DATA species that force a role exemption elsewhere
/// -- rendered `html:` surfaces, canonical-JSON samples whose formatting IS the test, a guard's
/// own detection fixtures, deliberate column alignment inside a `\n`-formatted report -- occurs in
/// this crate. That is a measurement rather than an expectation, and if one ever lands here the
/// exemption belongs beside it, by ROLE and never per-file.
fn offends(line: &str) -> bool {
    !is_line_comment(line) && has_run_in_literal(line)
}

#[test]
fn authored_strings_carry_no_collapsed_indentation() {
    let offenders: Vec<String> = sources()
        .iter()
        .flat_map(|(path, text)| {
            text.lines()
                .enumerate()
                .filter(|(_, line)| offends(line))
                .map(move |(number, line)| format!("{path}:{}: {}", number + 1, line.trim_start()))
        })
        .collect();

    assert!(
        offenders.is_empty(),
        "these string literals carry runs of whitespace, which a reader of the failure gets verbatim. A continued literal keeps the next line indentation inside it: end the line with a backslash so Rust drops the newline and the indentation, or put the string on one line.\n{}",
        offenders.join("\n")
    );
}

/// The walk must actually reach the crate, and it has TWO roots to reach.
///
/// Without this, a `read_dir` that returned almost nothing would satisfy the assertion above while
/// scanning nothing. The floor is the REAL count rather than a number chosen to sit comfortably
/// under it: a floor with slack tolerates exactly the silent shrinkage it exists to catch.
///
/// **One assertion per ROOT, because this walk makes two claims.** A check that only proved the
/// walk found files would be satisfied by a walk that never reached `tests/` at all -- and
/// `tests/` is where every instance of this class actually lives, so that failure would disable
/// the guard entirely while leaving it green.
///
/// **The root is asserted by PREFIX and not by naming a file in it, and the difference is not
/// style.** A named landmark only distinguishes the roots while that name happens to be unique
/// across them, which is an accident of the crate rather than a property of the guard: this crate
/// already has `memory.rs` and `retention.rs` under BOTH roots, so a later edit that picks one of
/// those as the landmark silently stops telling the roots apart while still passing. Elsewhere the
/// accident does not even hold -- `core/extension-host` mirrors all three of its test filenames in
/// `src/` -- so a copy of this file that carried the named form would arrive already broken. A
/// prefix asserts the thing actually meant.
///
/// The cost of the floor is that a legitimate removal now edits this number, which is the
/// plausible-looking edit a floor is supposed to resist. So the rule beside it: **lower this only
/// in the same commit as the removal that caused it, and name the removed file.** The landmarks
/// and the count then fail on different work, which is the reason for keeping both.
#[test]
fn the_scan_covers_both_roots_of_the_crate() {
    let found = sources();
    assert!(
        found.len() >= 32,
        "HARNESS-BROKE: the walk found only {} source files, so the scan above reads far less \
         than this crate",
        found.len()
    );
    assert!(
        found.iter().any(|(path, _)| path.starts_with("src")),
        "HARNESS-BROKE: the walk reached no file under src/ at all"
    );
    assert!(
        found.iter().any(|(path, _)| path.starts_with("tests")),
        "HARNESS-BROKE: the walk reached no file under tests/, so it is covering only one of its \
         two roots -- and tests/ is where every instance of this class lives, so the scan above \
         would be silently green"
    );
}
