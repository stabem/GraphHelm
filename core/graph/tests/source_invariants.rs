//! Enforces a property of this crate's SOURCE that no runtime test can see.
//!
//! A documented control which no test enforces is not a control. Operator-facing strings are such
//! a control: nothing renders them except a human, so a defect in them leaves every test green.
//!
//! The defect: a line continuation inside a string literal keeps the NEXT line's indentation
//! inside the literal, and `cargo fmt` joins the pieces into one line. The source reads plausibly
//! while the reader of a failure gets `must stop being named, or the rule is not          reading
//! the declaration`.
//!
//! **The walk covers `tests/` as well as `src/`.** A census at the threshold that actually
//! enforces reports every `src/` tree in the workspace already clean and every instance of this
//! class living in `tests/`. A guard scoped to `src/` would have been green on arrival and
//! protected nothing, which is a control with no subject.
//!
//! Scanning `tests/` means this file scans itself. That is deliberate: exempting its own file
//! would leave a whole file unguarded and the exemption would be invisible from the failure
//! message. The cost is that every string here obeys the rule it enforces, and the `\`
//! continuation idiom is what makes that possible.

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/source-invariants/detect.rs"
));

use std::path::{Path, PathBuf};

/// Every `.rs` file under `src/` AND `tests/`, discovered by WALKING the directories.
///
/// The population is the directory, not a list: a file ADDED to the crate must be scanned without
/// anyone remembering to name it. The risk that trades against, a walk that silently returns
/// almost nothing, is answered by the floor and the two root assertions below.
fn sources() -> Vec<(String, String)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        // A MISSING root is deferred to the root assertions below rather than reported here.
        // `read_dir` on an absent directory panics in the ARRANGEMENT, so a crate without a
        // `tests/` directory would fail with an io error from a helper -- while a missing root is
        // precisely the condition those assertions exist to report, in the vocabulary of coverage.
        // The wrong mechanism answering first also hides the right one: the assertion never runs,
        // so nobody learns whether it would have caught this. (Found by M, reviewing #390.)
        //
        // Only NotFound is deferred. Every other io error still panics here, because a root that
        // exists but cannot be read is NOT what the assertions below describe, and swallowing it
        // would let the walk shrink silently -- which is the failure the floor and the roots are
        // both built against.
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => panic!("cannot read {}: {e}", dir.display()),
        };
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
/// This crate needs only the shared one. Its single candidate is authored prose in an assertion
/// message; none of the DATA species that force a role exemption elsewhere occurs here -- rendered
/// `html:` surfaces, canonical-JSON samples whose formatting IS the test, a guard's own detection
/// fixtures, deliberate column alignment inside a `\n`-formatted report, or a value that IS
/// whitespace. That is a measurement, and if one ever lands here the exemption belongs beside it,
/// by ROLE and never per-file.
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
/// The floor is the REAL count rather than a number chosen to sit comfortably under it: a floor
/// with slack tolerates exactly the silent shrinkage it exists to catch. Its cost is that a
/// legitimate removal now edits this number, which is the plausible-looking edit a floor is
/// supposed to resist, so the rule beside it: **lower this only in the same commit as the removal
/// that caused it, and name the removed file.**
///
/// The roots are asserted by PREFIX rather than by naming a file in each. A named landmark
/// separates the roots only while that name is unique across them, which is an accident of a
/// crate rather than a property of a guard: in `core/extension-host` every test filename is
/// mirrored under `src/`, so the named form arrives already unable to make the distinction it
/// claims to make. `tests/` is where every instance of this class lives, so a walk that lost that
/// root would leave the scan above silently green -- disabled and passing.
#[test]
fn the_scan_covers_both_roots_of_the_crate() {
    let found = sources();
    assert!(
        found.len() >= 16,
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
