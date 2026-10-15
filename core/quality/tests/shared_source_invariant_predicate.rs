//! The compile carrier and the oracle for the shared source-invariant predicate.
//!
//! The predicate lives in `graphhelm_test_shims::detect` (#1315; before, an `include!`d file
//! with no `Cargo.toml`, which this test existed to compile). What stays is the oracle below.
//!
//! It lives in `core/quality` for a reason that is now history: the gate-freeze rule (M06
//! binding decision 5) counted `core/quality/` as gate machinery, so the shared file, its
//! frozen path and this carrier could travel in one pull request. The freeze was retired
//! with the gate (#1225); the cell that checked the predicate's path against it went too.
//! What stays is the oracle below: what the predicate detects and what it exempts.

use graphhelm_test_shims::detect::*;

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
