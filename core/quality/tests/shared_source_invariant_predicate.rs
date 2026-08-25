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
