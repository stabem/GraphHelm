//! Enforces a property of this crate's SOURCE that no runtime test can see.
//!
//! A documented control which no test enforces is not a control. Operator-facing strings are such
//! a control: nothing renders them except a human, so a defect in them leaves every test green --
//! confirmed when eight of them were corrected here and not one test noticed.
//!
//! The defect: a line continuation inside a string literal keeps the NEXT line's indentation
//! inside the literal, and `cargo fmt` joins the pieces into one line. The source reads plausibly
//! while the operator reads `a waiter waits on its OWN                  lease`.

use std::path::{Path, PathBuf};

/// Every `.rs` file under `src/`, discovered by WALKING the directory.
///
/// **The population is the directory, not a list.** An earlier version named three files with
/// `include_str!`, which bought one property -- a moved path breaks the BUILD instead of silently
/// shrinking the scan -- and quietly gave up a bigger one: a file ADDED to the crate was not
/// scanned, and nothing said so. Guarding against a moved file while blind to a new file is the
/// weaker half of the trade. (Found by L.)
///
/// The shrinkage risk that `include_str!` covered is handled by the floor in
/// `the_scan_covers_the_whole_crate`: a walk that returns almost nothing fails loudly.
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
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = Vec::new();
    walk(&root, &mut found);
    found.sort();
    found
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
            let shown = path
                .strip_prefix(Path::new(env!("CARGO_MANIFEST_DIR")))
                .unwrap_or(&path)
                .display()
                .to_string();
            (shown, text)
        })
        .collect()
}

/// Whether this line carries a run of three or more spaces inside a STRING LITERAL.
///
/// **Only the odd-index segments of `split('"')` are literal bodies.** The even ones are the code
/// between and after literals, and inspecting those made the first version fire on ordinary Rust:
/// a trailing aligned comment (`let s = "ok"; //   note`) and plain spacing between two literals
/// (`let a = "x";        let b = "y";`) both tripped it. A guard that fires on legitimate code is
/// the guard the next person relaxes -- which is the exact failure this file was written to avoid,
/// so it is now covered by its own tests below. (Found by L.)
///
/// Three spaces rather than two: two after a full stop is a writing convention.
///
/// **TWIN COPY: `tools/pathogens/tests/source_invariants.rs` holds a byte-identical predicate.**
/// It cannot be deduplicated: `tools/pathogens` is GATE_MACHINERY and `apps/cli` is not, so M06
/// forbids one branch touching both. Fix one, fix the other -- that is exactly how this
/// predicate's false-positive bug reached two crates: it was copied, and a duplicated ORACLE
/// diverges in silence rather than loudly. **If a THIRD crate needs this, extract a shared
/// dev-only helper instead of a third copy** -- twin pointers do not scale to N, and N copies of
/// one predicate drift.
fn offending_literal(line: &str) -> bool {
    let body = line.trim_start_matches(' ');
    if body.starts_with("//") {
        return false;
    }
    body.split('"')
        .enumerate()
        .any(|(index, part)| index % 2 == 1 && part.contains("   "))
}

#[test]
fn operator_strings_carry_no_collapsed_indentation() {
    let offenders: Vec<String> = sources()
        .iter()
        .flat_map(|(path, text)| {
            text.lines()
                .enumerate()
                .filter(|(_, line)| offending_literal(line))
                .map(move |(number, line)| format!("{path}:{}: {}", number + 1, line.trim_start()))
        })
        .collect();

    assert!(
        offenders.is_empty(),
        "these string literals carry runs of whitespace, which the operator reads verbatim. A continued literal keeps the next line indentation inside it: put the string on one line, or concatenate explicitly.\n{}",
        offenders.join("\n")
    );
}

/// The walk must actually reach the crate.
///
/// Without this, a `read_dir` that returned almost nothing would satisfy the assertion above while
/// scanning nothing -- the vacuous pass that the previous `include_str!` list was chosen to avoid.
/// The floor is the replacement for that property, and it is deliberately well below the current
/// count so ordinary growth does not trip it.
#[test]
fn the_scan_covers_the_whole_crate() {
    let found = sources();
    assert!(
        found.len() >= 40,
        "HARNESS-BROKE: the walk found only {} source files, so the scan above reads far less \
         than this crate",
        found.len()
    );
    assert!(
        found.iter().any(|(path, _)| path.contains("wake_wait")),
        "HARNESS-BROKE: a file known to exist is absent from the walk"
    );
}

/// The predicate itself, because it is the part that decides what everything else means.
///
/// The false-negative case matters as much as the false positives: a guard tuned until it stops
/// complaining is a guard that stops working.
#[test]
fn the_predicate_ignores_ordinary_rust_and_still_catches_the_defect() {
    assert!(
        !offending_literal(r#"let s = "ok"; //   aligned trailing comment"#),
        "a trailing aligned comment after a literal is ordinary Rust"
    );
    assert!(
        !offending_literal(r#"let a = "x";        let b = "y";"#),
        "spacing between two literals is ordinary Rust"
    );
    assert!(
        !offending_literal("///   a doc comment whose indent is an intentional list"),
        "comments are excluded: their indentation is often deliberate"
    );
    assert!(
        offending_literal(r#"    "a real defect with          collapsed indent","#),
        "the defect itself must still be caught"
    );
}
