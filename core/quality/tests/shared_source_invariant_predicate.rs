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

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/source-invariants/detect.rs"
));

/// The defect the predicate exists to catch: a line continuation inside a string literal
/// keeps the next line's indentation INSIDE the literal, and `cargo fmt` then joins the
/// pieces into one plausible-looking line.
#[test]
fn it_catches_a_collapsed_run_inside_a_literal() {
    assert!(has_run_in_literal(
        r#"    "a waiter waits on its OWN                  lease","#
    ));
}

/// Both halves of the false positive that reached two crates by being copied.
///
/// Neither is hypothetical: inspecting the EVEN segments of `split('"')` -- the code
/// between and after literals rather than the literal bodies -- made an earlier version
/// fire on each of these.
#[test]
fn it_ignores_ordinary_rust_that_merely_looks_spaced() {
    assert!(
        !has_run_in_literal(r#"    let x = "short";        // an aligned trailing comment"#),
        "spacing before a trailing comment is code, not a literal body"
    );
    assert!(
        !has_run_in_literal(r#"    f("first",        "second");"#),
        "spacing BETWEEN two literals is code, not a literal body"
    );
}

/// Detection and exemption are separate functions so that each can be asserted to be
/// doing work. A predicate that bundled the comment exemption would pass this file's
/// first test and still be unable to show that the exemption ever fires.
#[test]
fn detection_and_exemption_are_independently_observable() {
    let commented = r#"    // "a comment whose run          is deliberate""#;
    assert!(
        has_run_in_literal(commented),
        "the DETECTOR must still see the run -- if it did not, the exemption below would \
         be exempting nothing and this file could not tell the difference"
    );
    assert!(
        is_line_comment(commented),
        "and the EXEMPTION is what removes it"
    );

    let code = r#"    let s = "a run          that is not in a comment";"#;
    assert!(has_run_in_literal(code));
    assert!(!is_line_comment(code), "an ordinary line is not exempted");
}

/// An escaped quote inside a literal must not end the literal.
///
/// Without pair consumption the run below lands on an EVEN index -- read as code -- and
/// the predicate misses a defect of exactly the kind it exists to catch. Refusal and
/// operator messages quote things, so this is the common case rather than a corner.
#[test]
fn a_literal_holding_an_escaped_quote_is_still_scanned() {
    assert!(
        has_run_in_literal(r#"    let s = "he said \" and then          waited";"#),
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
        !has_run_in_literal(r#"    let s = "x\"";          let t = 1;"#),
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
        !has_run_in_literal(r#"    let root = "C:\\";          let n = 1;"#),
        "the quote after an escaped backslash CLOSES the literal, so the spacing that \
         follows is code"
    );
}

/// The same shape, in the direction where the mistake HIDES a defect rather than inventing
/// one: after an escaped backslash, a later literal's run must still be found.
#[test]
fn an_escaped_backslash_does_not_hide_a_later_literals_run() {
    assert!(
        has_run_in_literal(r#"    let a = "C:\\"; let b = "x          y";"#),
        "a run in a literal AFTER an escaped backslash must still be caught"
    );
}
