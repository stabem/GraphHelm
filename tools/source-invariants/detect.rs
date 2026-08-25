// The collapsed-run-in-literal predicate, shared by every crate that guards its own
// source. Included BY VALUE with `include!`; there is no `Cargo.toml` here, so adopting
// it adds no dependency edge and no `Cargo.lock` line.
//
// Why a shared file at all: `tools/pathogens/tests/source_invariants.rs` said it, before
// this file existed -- "if a THIRD crate needs this, extract a shared dev-only helper
// instead of a third copy: twin pointers do not scale to N, and N copies of one predicate
// drift." By the time the extraction was scheduled there were EIGHT guards, and the twin
// pointers each still claimed a byte-identical partner that had already diverged.
//
// Why this path is inside GATE_MACHINERY: a predicate is not an input to a gate, it IS
// the gate. Left outside the freeze, one pull request could edit what the pathogen guard
// DETECTS in the same breath as the code that guard judges, and `freeze_violation` would
// see an all-non-gate path set and wave it through.
//
// The comments here are `//` and NOT `//!` on purpose: an included file is spliced into
// the MIDDLE of the including file, so an inner doc comment is E0753 in every adopting
// crate at once. Measured -- it failed in two crates before this line was written.
//
// EVERY ITEM HERE MUST BE USED BY EVERY ADOPTER, and that is a hard constraint rather
// than tidiness. The gate runs `clippy -- -D warnings`, so an item an adopter does not
// consume is `dead_code` and fails the build for that crate. `include!` copies the whole
// file into each adopting crate, so there is no way to take half of it.
//
// The rule that follows, and it is a rule about what may be ADDED here: export only what
// every adopter consumes. Anything narrower than that -- a helper two crates want and the
// third does not -- belongs in those two crates, not here, however tempting the symmetry.
// If something must live here that not everyone uses, the cost is declared at the same
// time: which crates carry an `#[allow(dead_code)]` and why, in this comment, so the
// exception is visible to the next person rather than discovered by a red build.
//
// Today both items qualify: the compile carrier uses both, and every one of the eight
// guards exempts comments. (Constraint named by L.)

/// Whether this line carries a run of three or more spaces inside a STRING LITERAL.
///
/// Detection only, with no exemptions of any kind. The exemptions are per-crate and by
/// ROLE, so they compose on top of this rather than hiding inside it -- a predicate that
/// bundles its own exemptions cannot be asserted to be doing work separately from them.
///
/// An earlier version split on `"` and read the odd-index segments as literal bodies.
/// Inspecting the even ones made it fire on ordinary Rust -- a trailing aligned comment,
/// and plain spacing between two adjacent literals. (Found by L.) That bug reached two
/// crates because the predicate had been COPIED, which is the whole argument for this
/// file.
///
/// **It scans ESCAPE PAIRS rather than removing a substring, and that is the whole
/// correctness argument.** A backslash consumes whatever follows it, quote or backslash.
/// Two shortcuts both fail, in mirror images of each other:
///
/// - splitting on `"` alone treats an escaped quote as a boundary, so `\"` shifts every
///   later segment's parity by one -- a run inside such a literal is MISSED, and a run in
///   the code after it is REPORTED;
/// - stripping the substring `\"` first fixes that and breaks the mirror case: in `"C:\\"`
///   the bytes are `"`, `C`, `:`, `\`, `\`, `"`, so the substring `\"` exists formed by the
///   SECOND backslash and the REAL closing quote. Removing it eats a genuine quote and
///   shifts the parity by one -- the same defect, in the opposite direction. (Found by L,
///   on six live instances, after the substring version had already been written.)
///
/// Only left-to-right pair consumption gets both. Each case below has its own cell, and
/// the two mirrored ones are there because **a repository-wide differential cannot decide
/// this**: the substring and pair versions disagree on constructible input and agree on
/// every line this repository happens to contain, so a corpus difference of zero says what
/// the CORPUS holds, never what the PREDICATE does.
///
/// LIMIT, named rather than glossed: this is a line-level scan and not a Rust lexer. It
/// does not know about raw strings, so `r#"..."#` is read as ordinary quoting and a
/// backslash inside one is treated as an escape when it is literal. The version this
/// replaces did not know either; nothing here makes that better, and no cell claims it.
fn has_run_in_literal(line: &str) -> bool {
    let mut inside = false;
    let mut spaces = 0_usize;
    let mut chars = line.trim_start_matches(' ').chars();

    while let Some(character) = chars.next() {
        match character {
            // Consume the escaped character WHATEVER it is. Skipping only quotes is the
            // substring bug: it lets the second backslash of `\\` pair with the real
            // closing quote that follows.
            '\\' => {
                chars.next();
                spaces = 0;
            }
            '"' => {
                inside = !inside;
                spaces = 0;
            }
            ' ' if inside => {
                spaces += 1;
                if spaces >= 3 {
                    return true;
                }
            }
            _ => spaces = 0,
        }
    }

    false
}

/// The one exemption every adopting crate shares: a line comment.
///
/// A comment's indentation is frequently a deliberate list, and no operator, test reader
/// or refusal message ever renders it. Every OTHER exemption is a property of a particular
/// crate's fixtures -- `html:` rendered surfaces, canonical-JSON samples, the detection
/// fixtures of the guards themselves -- and belongs beside those fixtures, not here.
fn is_line_comment(line: &str) -> bool {
    line.trim_start_matches(' ').starts_with("//")
}
