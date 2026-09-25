//! The compile carrier and the oracle for the shared source-invariant predicate.
//!
//! `tools/source-invariants/detect.rs` has no `Cargo.toml` -- that is the point of it, and
//! it is also a hazard: a file nothing compiles is a file whose syntax errors wait for the
//! next adopter to discover. This test is what compiles it, from the moment it is created
//! and independently of whether any crate has adopted it yet.
//!
//! It lives in `core/quality` because `core/quality/` is already GATE_MACHINERY, so the
//! commit that adds the shared file, freezes its path, and gives it a compile carrier
//! touches only gate paths -- which is what lets all three travel in one pull request.
//! Measured against `freeze_enforced` before it was written, not assumed.

use graphhelm_quality::freeze_violation;

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/source-invariants/detect.rs"
));

/// Builds one fixture source line WITHOUT writing a run of spaces into this file.
///
/// Every cell below feeds the predicate a line that CONTAINS the defect, so writing those
/// lines as literals put ten authored runs in this file. That is why it was exempt from the
/// workspace sweep -- and, because `core/quality` has no per-crate source guard, why nothing
/// scanned it at all. Composing the run instead lets this file be swept like any other, which
/// is the method the exemption entry itself named as the way to close its own gap.
///
/// `INDENT` is the simulated source indentation every fixture opens with. It is built rather
/// than written for the same reason: four spaces inside the raw string is itself a run above
/// the predicate's threshold of three, so leaving it authored would keep the file red.
fn source_line(before: &str, run: usize, after: &str) -> String {
    const INDENT: usize = 4;
    format!("{}{before}{}{after}", " ".repeat(INDENT), " ".repeat(run))
}

/// The defect the predicate exists to catch: a run of spaces inside a string literal, which a
/// reader gets verbatim.
///
/// It arrives when a `\` continuation is lost BEFORE the file is written -- a code generator
/// consuming the escape, which is MEASURED (#440); any other upstream rewriter is conjecture --
/// so what lands is one line with the indentation already inside it. **Neither `rustc` nor `cargo fmt` can produce it, and the sentence this replaces
/// named both**: the continuation escape consumes the next line's indentation, and
/// `format_strings` is absent from this repository's `rustfmt.toml` (default `false`), so rustfmt
/// never edits inside a literal. Both measured in #440. I wrote the wrong version here; it is
/// refuted by the lane that proved it.
#[test]
fn it_catches_a_collapsed_run_inside_a_literal() {
    assert!(has_run_in_literal(&source_line(
        r#""a waiter waits on its OWN"#,
        18,
        r#"lease","#
    )));
}

/// Both halves of the false positive that reached two crates by being copied.
///
/// Neither is hypothetical: inspecting the EVEN segments of `split('"')` -- the code
/// between and after literals rather than the literal bodies -- made an earlier version
/// fire on each of these.
#[test]
fn it_ignores_ordinary_rust_that_merely_looks_spaced() {
    assert!(
        !has_run_in_literal(&source_line(
            r#"let x = "short";"#,
            8,
            "// an aligned trailing comment"
        )),
        "spacing before a trailing comment is code, not a literal body"
    );
    assert!(
        !has_run_in_literal(&source_line(r#"f("first","#, 8, r#""second");"#)),
        "spacing BETWEEN two literals is code, not a literal body"
    );
}

/// Detection and exemption are separate functions so that each can be asserted to be
/// doing work. A predicate that bundled the comment exemption would pass this file's
/// first test and still be unable to show that the exemption ever fires.
#[test]
fn detection_and_exemption_are_independently_observable() {
    let commented = source_line(r#"// "a comment whose run"#, 10, r#"is deliberate""#);
    assert!(
        has_run_in_literal(&commented),
        "the DETECTOR must still see the run -- if it did not, the exemption below would \
         be exempting nothing and this file could not tell the difference"
    );
    assert!(
        is_line_comment(&commented),
        "and the EXEMPTION is what removes it"
    );

    let code = source_line(r#"let s = "a run"#, 10, r#"that is not in a comment";"#);
    assert!(has_run_in_literal(&code));
    assert!(!is_line_comment(&code), "an ordinary line is not exempted");
}

/// An escaped quote inside a literal must not end the literal.
///
/// Without pair consumption the run below lands on an EVEN index -- read as code -- and
/// the predicate misses a defect of exactly the kind it exists to catch. Refusal and
/// operator messages quote things, so this is the common case rather than a corner.
#[test]
fn a_literal_holding_an_escaped_quote_is_still_scanned() {
    assert!(
        has_run_in_literal(&source_line(
            r#"let s = "he said \" and then"#,
            10,
            r#"waited";"#
        )),
        "a run inside a literal must be caught even when the literal contains an escaped quote"
    );
}

/// The same parity shift in the other direction: code read as a literal body.
///
/// One escaped quote is enough to move ordinary code onto an odd index, where alignment
/// spacing is reported as a collapsed message. A guard that cries wolf is ignored on the
/// day it is right, which is why this direction gets its own cell.
#[test]
fn an_escaped_quote_does_not_make_following_code_look_like_a_literal() {
    assert!(
        !has_run_in_literal(&source_line(r#"let s = "x\"";"#, 10, "let t = 1;")),
        "spacing after a literal is code, and an escaped quote must not change that"
    );
}

/// An escaped BACKSLASH ends where it ends, and the quote after it is REAL.
///
/// This is the mirror of the two cells above and it exists because the obvious repair for
/// them -- strip the substring `\"` before splitting -- reintroduces the identical defect
/// backwards. In `"C:\\"` the bytes are `"`, `C`, `:`, `\`, `\`, `"`: the substring `\"`
/// is present, formed by the SECOND backslash and the genuine closing quote. Removing it
/// eats that quote, and the run in the code afterwards is reported as if it were inside a
/// literal.
///
/// **This cell does not depend on the repository containing the shape.** The substring
/// version and the pair-scanning version disagree here and agree on every line this
/// repository holds today, so nothing measured over the corpus can tell them apart.
/// (Found by L, on six live instances.)
#[test]
fn an_escaped_backslash_does_not_swallow_the_closing_quote() {
    assert!(
        !has_run_in_literal(&source_line(r#"let root = "C:\\";"#, 10, "let n = 1;")),
        "the quote after an escaped backslash CLOSES the literal, so the spacing that \
         follows is code"
    );
}

/// The same shape, in the direction where the mistake HIDES a defect rather than inventing
/// one: after an escaped backslash, a later literal's run must still be found.
#[test]
fn an_escaped_backslash_does_not_hide_a_later_literals_run() {
    assert!(
        has_run_in_literal(&source_line(r#"let a = "C:\\"; let b = "x"#, 10, r#"y";"#)),
        "a run in a literal AFTER an escaped backslash must still be caught"
    );
}

/// The frozen prefix and the file's real location must agree, and neither may be
/// re-spelled by hand to make them.
///
/// The path is written independently in three places: the `GATE_MACHINERY` entry, each
/// adopter's `include!`, and where the file actually sits. A rename fixes the include --
/// the compiler insists -- and can silently forget the constant, at which point the
/// predicate has left the freeze and nothing is red. A literal written here would be a
/// FOURTH spelling and would drift the same way.
///
/// So neither side is spelled here. The path comes from `file!()`, which inside an
/// included file expands to the INCLUDED file, built from the very string the `include!`
/// used -- measured, not assumed. And gate membership is not compared against a copied
/// list: `freeze_violation` is ASKED, so the authority is the same constant the gate
/// itself consults.
#[test]
fn the_shared_predicate_lives_where_the_freeze_says_it_does() {
    let me = repo_relative(shared_predicate_self_path());

    // A landmark that is certainly NOT gate machinery. Pairing it with `me` makes
    // `freeze_violation` answer the only question being asked: is `me` gate?
    const NON_GATE_LANDMARK: &str = "README.md";

    // CONTROL FIRST. If both paths were non-gate the call returns None, and a `None`
    // from a broken subject would be indistinguishable from a `None` from a broken
    // oracle. This proves the oracle discriminates before its verdict is trusted.
    assert_eq!(
        freeze_violation(&["docs/gates/whatever.md", NON_GATE_LANDMARK]).map(|(gate, _)| gate),
        Some("docs/gates/whatever.md".to_owned()),
        "CONTROL FAILED: freeze_violation did not recognise a known gate path, so its \
         verdict on the shared predicate below means nothing"
    );
    assert_eq!(
        freeze_violation(&[NON_GATE_LANDMARK, "src/other.rs"]),
        None,
        "CONTROL FAILED: the landmark must be non-gate, or the check below passes for \
         the wrong reason"
    );

    assert!(
        freeze_violation(&[me.as_str(), NON_GATE_LANDMARK]).is_some(),
        "the shared predicate is at {me}, which GATE_MACHINERY does not cover. Either \
         the file moved and the constant in core/quality/src/lib.rs was not updated, or \
         the constant changed and the file was not moved. A predicate outside the freeze \
         can be edited in the same pull request as the code it judges."
    );
}

/// `file!()` yields the include string appended to `CARGO_MANIFEST_DIR`, with its `..`
/// segments unresolved and its separators mixed. Resolve them and cut the repository
/// root off the front, so what is compared is the same shape `GATE_MACHINERY` holds.
fn repo_relative(raw: &str) -> String {
    use std::path::{Component, Path, PathBuf};

    let mut resolved = PathBuf::new();
    for part in Path::new(raw).components() {
        match part {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            other => resolved.push(other.as_os_str()),
        }
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("core/quality sits two levels below the repository root");

    resolved
        .strip_prefix(root)
        .expect("the shared predicate resolves to a path inside this repository")
        .to_string_lossy()
        .replace('\\', "/")
}

/// The escaped-backslash shape with an ALIGNED TRAILING COMMENT after it, rather than code.
///
/// Review named two `assert!` shapes. Only this one earns a cell: the other --
/// `assert!(!debug.contains("C:\\"));          let t = 1;` -- is the same shape the cell
/// above already covers, a literal ending in an escaped backslash followed by code with a
/// run, and a second copy of an oracle is worse than none. This one lands on a different
/// arm: alignment before a comment is the false positive the predicate was first written
/// to avoid, and an escaped backslash must not smuggle it back in through the other door.
#[test]
fn an_escaped_backslash_does_not_resurrect_the_aligned_comment_false_positive() {
    assert!(
        !has_run_in_literal(&source_line(
            r#"assert!(!debug.contains("C:\\"));"#,
            8,
            "// aligned"
        )),
        "spacing before a trailing comment is code, with or without an escaped backslash \
         earlier in the line"
    );
}
