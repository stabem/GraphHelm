//! Enforces a property of this crate's SOURCE that no runtime test can see.
//!
//! A documented control which no test enforces is not a control. This crate's refusal findings are
//! such a control: nothing renders them except a human reading a failed gate, so a defect in them
//! leaves every test green.
//!
//! The defect: a line continuation inside a string literal keeps the NEXT line's indentation
//! inside the literal, and `cargo fmt` joins the pieces into one line. The source reads plausibly
//! while the reader gets `flaky success is not proven              success`.

/// Every `.rs` file under `src/`, discovered by WALKING the directory.
///
/// **The population is the directory, not a list.** A hand-written list guards the file that
/// MOVES and says nothing about the file that is ADDED -- a new source in this crate would not be
/// scanned and nothing would say so. (L's finding on the sibling guard; the same hand-list shape
/// had been used here.) The shrinkage a list protected against is covered by the floor in
/// `the_scan_covers_the_whole_crate`.
fn sources() -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
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
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = Vec::new();
    walk(&root, &mut found);
    found.sort();
    found
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
            let shown = path.file_name().map_or_else(
                || path.display().to_string(),
                |n| format!("src/{}", n.to_string_lossy()),
            );
            (shown, text)
        })
        .collect()
}

/// Whether this line carries a run of whitespace inside a string literal.
///
/// **`html:` lines are exempt, and the exemption is principled rather than convenient.** Those
/// literals are the rendered-surface FIXTURES of the geometry pathogens, and `suite_digest` is a
/// sha256 over the serialized suite -- so "tidying" their whitespace would change the digest and
/// void every recorded geometry certification with the corpus semantically unchanged. An operator
/// who watches certifications fall for a cosmetic reason learns the signal is noisy, and a
/// mechanism that cries wolf is ignored on the day it is right.
///
/// The exemption is by ROLE, not by file: everything else in `lib.rs` stays scanned, so a genuine
/// message defect there is still caught.
/// Whether this line carries a run of three or more spaces inside a STRING LITERAL.
///
/// Detection only -- no exemptions. Split from the role check so the exemption below can be
/// asserted to be doing work, rather than merely to have a subject.
///
/// Only the ODD-index segments of `split('"')` are literal BODIES; the even ones are the code
/// between and after literals. Inspecting those made the first version fire on ordinary Rust: a
/// trailing aligned comment, and plain spacing between two literals. (Found by L.)
///
/// **TWIN COPY: `apps/cli/tests/source_invariants.rs` holds a byte-identical predicate.** It
/// cannot be deduplicated here: `tools/pathogens` is GATE_MACHINERY and `apps/cli` is not, so M06
/// forbids one branch touching both. Fix one, fix the other -- that is exactly how this
/// predicate's false-positive bug reached two crates: it was copied, and a duplicated ORACLE
/// diverges in silence. **If a THIRD crate needs this, extract a shared dev-only helper instead of
/// a third copy** -- twin pointers do not scale to N, and N copies of one predicate drift.
fn has_run_in_literal(line: &str) -> bool {
    line.trim_start_matches(' ')
        .split('"')
        .enumerate()
        .any(|(index, part)| index % 2 == 1 && part.contains("   "))
}

/// Lines whose runs are deliberate and must not be reported.
///
/// Comments: their indentation is often an intentional list.
///
/// `html:` lines: the rendered-surface FIXTURES of the geometry pathogens. `suite_digest` is a
/// sha256 over the serialized suite, so "tidying" their whitespace changes the digest and voids
/// every recorded geometry certification with the corpus semantically unchanged. An operator who
/// watches certifications fall for a cosmetic reason learns the signal is noisy.
///
/// Exempt by ROLE, not by file: every other line of `lib.rs` stays scanned.
fn is_exempt(line: &str) -> bool {
    let body = line.trim_start_matches(' ');
    body.starts_with("//") || body.starts_with("html:")
}

fn offends(line: &str) -> bool {
    !is_exempt(line) && has_run_in_literal(line)
}

#[test]
fn refusal_strings_carry_no_collapsed_indentation() {
    // The scan must have something to scan: an `include_str!` that resolved to nothing would
    // satisfy the assertion below while reading no source at all.
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
        "these string literals carry runs of whitespace, which a reader gets verbatim. A continued literal keeps the next line indentation inside it: put the string on one line, or concatenate explicitly.\n{}",
        offenders.join("\n")
    );
}

/// The exemption must be CARRYING WEIGHT, not merely have a subject.
///
/// The first version asserted that `html:` lines exist. **Existing is not offending.** Someone
/// reformats the geometry fixtures -- they are HTML strings, entirely tidyable -- so that no
/// `html:` line carries a run any more, and the exemption suppresses nothing while this cell stays
/// GREEN. That is precisely the rot the exemption's own comment claims nobody would notice, and it
/// was reachable: verified by collapsing both fixtures, after which all four tests still passed.
/// (Found by L.)
///
/// The S2 sabotage proved the exemption load-bearing ONCE. This asserts it is load-bearing NOW --
/// evidence of the moment against evidence that keeps.
#[test]
fn the_html_exemption_actually_suppresses_something() {
    let suppressed: Vec<String> = sources()
        .iter()
        .flat_map(|(path, text)| {
            text.lines()
                .enumerate()
                .filter(|(_, line)| {
                    line.trim_start().starts_with("html:") && has_run_in_literal(line)
                })
                .map(move |(number, _)| format!("{path}:{}", number + 1))
        })
        .collect();

    assert!(
        !suppressed.is_empty(),
        "HARNESS-BROKE: no `html:` line carries a run, so the exemption in `is_exempt` suppresses          nothing. Either the fixtures were reformatted -- in which case remove the exemption          rather than leave it looking load-bearing -- or the scan stopped reaching lib.rs"
    );
}

/// The walk must actually reach the crate.
///
/// Without this, a `read_dir` returning almost nothing would satisfy the scan above while reading
/// no source at all -- the vacuous pass a hand-written list was originally chosen to avoid. The
/// floor is the replacement for that property.
#[test]
fn the_scan_covers_the_whole_crate() {
    let found = sources();
    assert!(
        found.len() >= 3,
        "HARNESS-BROKE: the walk found only {} source files in this crate",
        found.len()
    );
    for expected in ["src/lib.rs", "src/jpd.rs", "src/retry_lineage.rs"] {
        assert!(
            found.iter().any(|(path, _)| path == expected),
            "HARNESS-BROKE: {expected} is known to exist and is absent from the walk"
        );
    }
}

/// The predicate itself, because it decides what everything else in this file means.
#[test]
fn the_predicate_ignores_ordinary_rust_and_still_catches_the_defect() {
    assert!(
        !offends(r#"let s = "ok"; //   aligned trailing comment"#),
        "a trailing aligned comment after a literal is ordinary Rust"
    );
    assert!(
        !offends(r#"let a = "x";        let b = "y";"#),
        "spacing between two literals is ordinary Rust"
    );
    assert!(
        !offends("///   a doc comment whose indent is an intentional list"),
        "comments are excluded: their indentation is often deliberate"
    );
    assert!(
        !offends(r#"        html: "<main>x</a>                   <section>","#),
        "digest-bearing fixtures are exempt by role"
    );
    // The false-negative case: a guard tuned until it stops complaining stops working.
    assert!(
        offends(r#"    "a real defect with          collapsed indent","#),
        "the defect itself must still be caught"
    );
}
