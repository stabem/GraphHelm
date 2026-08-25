//! Enforces a property of this crate's SOURCE that no runtime test can see.
//!
//! A documented control which no test enforces is not a control. Operator-facing strings are such
//! a control: nothing renders them except a human, so a defect in them leaves every test green.
//!
//! The defect: a line continuation inside a string literal keeps the NEXT line's indentation
//! inside the literal, and `cargo fmt` joins the pieces into one line. The source reads plausibly
//! while the reader of a failure gets `the compiler produced {actual}: for a consumer that
//! loads this artifact`.
//!
//! **Why this file is not called `source_invariants.rs`.** This crate already has one, guarding
//! different properties entirely -- declared dependencies, that the runtime never names an
//! adapter, and that no source spawns a process or speaks HTTP. Adding these tests there would
//! make a file with two owners, and in a two-owner file a merge can drop one side's work without
//! ever raising a conflict.
//!
//! **It would also have broken that file's other guard, which is the sharper reason.** Its
//! `sources()` walks `src/` only and is consumed by `no_source_file_spawns_processes_or_speaks_http`.
//! Widening that walk to reach `tests/` -- the obvious way to adopt this class in place -- would
//! point the purity guard at test code, which spawns processes and speaks HTTP by profession. It
//! would go red on an unrelated test, and the tempting remedy would be to weaken the purity guard
//! to fit. A separate file with its own `sources()` removes that whole failure mode by
//! construction rather than by anyone remembering.
//!
//! **The walk covers `tests/` as well as `src/`.** Measured before writing, with the shipped
//! predicate: `src/` holds ZERO instances across 13 files and `tests/` holds seven across 11. A
//! guard scoped to `src/` would have been green on arrival and protected nothing.
//!
//! Scanning `tests/` means this file scans itself, which is deliberate: exempting its own file
//! would leave a whole file unguarded and the exemption would be invisible from the failure
//! message. The cost is that every string here obeys the rule it enforces.

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/source-invariants/detect.rs"
));

use std::path::{Path, PathBuf};

/// Every `.rs` file under `src/` AND `tests/`, discovered by WALKING the directories.
///
/// The population is the directory, not a list: a file ADDED to the crate must be scanned without
/// anyone remembering to name it. The risk that trades against, a walk that silently returns
/// almost nothing, is answered by the root assertions and the floor below.
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

/// Blank out literals whose ENTIRE content is spaces, leaving the quotes in place.
///
/// This is the crate's one role exemption, and it is a role rather than a location: a literal that
/// is nothing but spaces is not indentation that leaked into a message, it is a VALUE whose being
/// whitespace is the point. `prompt_assembly.rs` sets an objective to `"   "` to test what happens
/// when an objective is only blanks -- rewriting it to satisfy this guard would change what the
/// test measures, which is the one edit a guard must never provoke.
///
/// The quotes are KEPT (`"   "` becomes `""`) rather than the whole literal being deleted. The
/// shared predicate tracks whether it is inside a literal by counting quote transitions, so
/// removing a quote pair would shift the parity of everything after it on the line -- the guard
/// would then read code as string and string as code, which is the exact defect that file's own
/// doc comment says two earlier versions of the predicate had.
fn without_whitespace_valued_literals(line: &str) -> String {
    let characters: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut index = 0;
    while index < characters.len() {
        if characters[index] == '"' {
            let mut end = index + 1;
            while end < characters.len() && characters[end] == ' ' {
                end += 1;
            }
            if end > index + 1 && end < characters.len() && characters[end] == '"' {
                out.push_str("\"\"");
                index = end + 1;
                continue;
            }
        }
        out.push(characters[index]);
        index += 1;
    }
    out
}

/// This crate's exemption, composed on top of the shared detection.
///
/// The detection lives in `tools/source-invariants/detect.rs` and is included by value; only the
/// EXEMPTION is a property of a crate, which is why the two are separate functions there.
/// Composing them here rather than importing a bundled predicate is what lets the exemption be
/// asserted to be doing work instead of merely having a subject.
fn offends(line: &str) -> bool {
    !is_line_comment(line) && has_run_in_literal(&without_whitespace_valued_literals(line))
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

/// The exemption must SUPPRESS something, or it is decoration that survives review by looking
/// careful.
///
/// This is the falsifier for the paragraph above `without_whitespace_valued_literals`. Without the
/// exemption the scan finds a real line; with it, none. An exemption nobody can show doing work is
/// indistinguishable from one written against a case that never occurs -- and it is the exemption,
/// not the detection, that a future edit will widen when this guard becomes inconvenient.
///
/// It also pins WHICH line, so the exemption cannot quietly grow. If it ever starts suppressing a
/// second site, this fails and whoever widened it has to say why here.
#[test]
fn the_whitespace_value_exemption_actually_suppresses_something() {
    let unexempted: Vec<(String, String)> = sources()
        .iter()
        .flat_map(|(path, text)| {
            text.lines()
                .enumerate()
                .filter(|(_, line)| !is_line_comment(line) && has_run_in_literal(line))
                .map(move |(number, line)| {
                    (
                        format!("{path}:{}", number + 1),
                        line.trim_start().to_owned(),
                    )
                })
        })
        .collect();

    assert!(
        !unexempted.is_empty(),
        "removing the whitespace-value exemption changed nothing, so it suppresses no line in \
         this crate. Either the case it was written for is gone -- delete the exemption rather \
         than keeping a rule with no subject -- or the detection stopped working"
    );

    // PINNED BY THE LINE, not by the file, and the difference is a real hole rather than
    // pedantry. The first version asserted every suppressed hit `contains("prompt_assembly")`,
    // which is satisfied by ANY line of that file: a second collapsed run appearing there --
    // wrongly suppressed, in a message a reader would receive -- passes an assertion whose whole
    // job is to notice exactly that. The exemption could grow inside its own file unobserved.
    // (Found by N, reviewing #407.)
    //
    // The site is pinned by CONTENT rather than by line number, because a number moves whenever
    // anything above it is edited and would make this cell fail for the one reason that is not
    // interesting. The content identifies the site by what it IS: an objective assigned a value
    // that is nothing but spaces.
    let strays: Vec<&(String, String)> = unexempted
        .iter()
        .filter(|(_, text)| !(text.contains("objective") && text.contains('"')))
        .collect();
    assert!(
        strays.is_empty(),
        "the exemption suppresses a line that is not the whitespace-as-value objective it was \
         written for. Every additional site is a judgement someone has to defend by ROLE, so \
         name it here:\n{}",
        strays
            .iter()
            .map(|(where_, text)| format!("{where_}: {text}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert_eq!(
        unexempted.len(),
        1,
        "the exemption suppresses more than one line. Each is a separate role judgement, and a \
         count that grows silently is how a narrow exemption becomes a broad one:\n{}",
        unexempted
            .iter()
            .map(|(where_, text)| format!("{where_}: {text}"))
            .collect::<Vec<_>>()
            .join("\n")
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
/// separates the roots only while that name is unique across them, which is an accident of a crate
/// rather than a property of a guard. `tests/` is where every instance of this class lives in this
/// crate -- measured, `src/` had zero -- so a walk that lost that root would leave the scan above
/// silently green.
#[test]
fn the_scan_covers_both_roots_of_the_crate() {
    let found = sources();
    // ORDER IS LOAD-BEARING HERE, and it is the reverse of the obvious one -- so this note sits at
    // the site that ARMS it, because moving the cheap numeric check back to the top is exactly the
    // kind of tidying that looks like an improvement.
    //
    // The floor is this crate's EXACT file count, so losing either root drops the count below it.
    // With the floor first, it answers every lost-root case and these two assertions never run:
    // the file would credit a check that cannot fire, and nobody would learn whether it worked.
    // (Found by N, reviewing #391 and #392, and adopted as the corrected form in #403.)
    //
    // Roots first, so the specific diagnosis wins: a lost root says which root, instead of a count
    // the reader has to work backwards from. A walk that reached both roots but shrank still fails
    // the floor below, exactly as before.
    assert!(
        found.iter().any(|(path, _)| path.starts_with("src")),
        "HARNESS-BROKE: the walk reached no file under src/ at all"
    );
    assert!(
        found.iter().any(|(path, _)| path.starts_with("tests")),
        "HARNESS-BROKE: the walk reached no file under tests/, so it is covering only one of its \
         two roots -- and tests/ is where every instance of this class lives in this crate, so the \
         scan above would be silently green"
    );
    assert!(
        found.len() >= 24,
        "HARNESS-BROKE: the walk found only {} source files, so the scan above reads far less \
         than this crate",
        found.len()
    );
}
