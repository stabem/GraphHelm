//! Enforces a property of this crate's SOURCE that no runtime test can see.
//!
//! A documented control which no test enforces is not a control. Operator-facing strings are such
//! a control: nothing renders them except a human, so a defect in them leaves every test green --
//! confirmed when eight of them were corrected here and not one test noticed.
//!
//! The defect: a line continuation inside a string literal keeps the NEXT line's indentation
//! inside the literal, and `cargo fmt` joins the pieces into one line. The source reads plausibly
//! while the operator reads `a waiter waits on its OWN                  lease`.

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/source-invariants/detect.rs"
));

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

/// This crate's exemption, composed on top of the shared detection.
///
/// The detection lives in `tools/source-invariants/detect.rs` and is included by value; only
/// the EXEMPTION is a property of this crate, which is why the two are separate functions
/// there. Composing them here rather than importing a bundled predicate is what lets the
/// exemption be asserted to be doing work, instead of merely having a subject.
///
/// Adopting the shared file also replaces this crate's own copy, which split on every `"`
/// and therefore mis-read escapes: `\"` shifted the parity of everything after it, and the
/// obvious repair for that broke the mirror case. Those defects were fixed once, in one
/// place, and this crate inherits the fix rather than needing it applied a second time --
/// which is the entire argument for the shared file, and is exactly what the twin pointer
/// deleted here predicted would go wrong.
fn offends(line: &str) -> bool {
    !is_line_comment(line) && has_run_in_literal(line)
}

#[test]
fn operator_strings_carry_no_collapsed_indentation() {
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
        "these string literals carry runs of whitespace, which the operator reads verbatim. A continued literal keeps the next line indentation inside it: put the string on one line, or concatenate explicitly.\n{}",
        offenders.join("\n")
    );
}

/// The walk must actually reach the crate.
///
/// Without this, a `read_dir` that returned almost nothing would satisfy the assertion above while
/// scanning nothing -- the vacuous pass that the previous `include_str!` list was chosen to avoid.
/// The floor is the replacement for that property, and it is the REAL count rather than a number
/// chosen to be comfortably under it. A floor with slack tolerates exactly the silent shrinkage it
/// exists to catch: at `>= 40` against 57 files, sixteen could vanish without a word.
///
/// The cost is that a legitimate removal now edits this number, which is the plausible-looking edit
/// a floor is supposed to resist. So the rule beside it: **lower this only in the same commit as
/// the removal that caused it, and name the removed file.** The landmark below and this count then
/// fail on different work, which is the whole reason for keeping both -- lowering a threshold is a
/// plausible edit, deleting a named assertion is a visible one.
#[test]
fn the_scan_covers_the_whole_crate() {
    let found = sources();
    assert!(
        found.len() >= 57,
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
    // PRECONDITION for the two cases below, and it is not ceremony. Their fixture property
    // is "this line CARRIES a run of three or more spaces, outside any literal". Lose one
    // space to an edit and `!offends(..)` collapses to `!false` and passes having measured
    // nothing -- and it would keep passing with the even-segment bug back in place, which is
    // the exact defect these two cells exist to catch. The pair is tight in both directions:
    // if the run vanished the precondition fails, and if it moved INSIDE a literal `offends`
    // becomes true and the assertion fails.
    let aligned = r#"let s = "ok"; //   aligned trailing comment"#;
    assert!(
        aligned.contains("   "),
        "the aligned-comment fixture stopped carrying a run, so the assertion below measures nothing"
    );
    assert!(
        !offends(aligned),
        "a trailing aligned comment after a literal is ordinary Rust"
    );

    let between = r#"let a = "x";        let b = "y";"#;
    assert!(
        between.contains("   "),
        "the between-literals fixture stopped carrying a run, so the assertion below measures nothing"
    );
    assert!(
        !offends(between),
        "spacing between two literals is ordinary Rust"
    );
    // PRECONDITION for the comment case, and it guards a correction rather than a fixture this
    // branch wrote. The fixture below was already repaired once, after a sabotage showed the
    // exemption was never reached. Nothing asserted it stays repaired: tidy the run out of its
    // inner literal and `has_run_in_literal` answers false again, the exemption goes unconsulted,
    // and the assertion passes for exactly the reason the repair removed -- with the comment above
    // it still explaining why that cannot happen.
    //
    // It composes `has_run_in_literal` instead of re-splitting on quotes. A precondition that
    // re-implements the predicate it validates inherits that predicate's blind spots by
    // construction, and this one would have inherited the escaped-quote bug the shared version
    // was cured of: in `let s = "a \" b   c";` a naive split flips the parity and reads the run
    // as being outside the literal. A second opinion assembled from the first opinion's parts is
    // not a second opinion.
    let commented = r#"//   let s = "a          b";"#;
    assert!(
        has_run_in_literal(commented),
        "the comment fixture stopped carrying a run INSIDE a literal, so the exemption below is \
         never reached and the assertion passes whether the exemption works or not -- which is the \
         defect this fixture was already corrected for once"
    );
    assert!(
        // The comment exemption must be REACHED to be observed. This fixture was
        // `"///   a doc comment whose indent is an intentional list"` -- a line with no
        // string literal in it at all, so `has_run_in_literal` answered `false` before the
        // exemption was ever consulted and the assertion passed whether the exemption
        // worked or not. Found by sabotaging `is_line_comment` against the pathogens copy
        // of this same fixture and watching nothing go red.
        !offends(commented),
        "comments are excluded: their indentation is often deliberate"
    );
    assert!(
        offends(r#"    "a real defect with          collapsed indent","#),
        "the defect itself must still be caught"
    );
}
