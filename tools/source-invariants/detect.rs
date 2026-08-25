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

/// Whether this line carries a run of three or more spaces inside a STRING LITERAL.
///
/// Detection only, with no exemptions of any kind. The exemptions are per-crate and by
/// ROLE, so they compose on top of this rather than hiding inside it -- a predicate that
/// bundles its own exemptions cannot be asserted to be doing work separately from them.
///
/// Only the ODD-index segments of `split('"')` are literal BODIES; the even ones are the
/// code between and after literals. Inspecting those made the first version of this fire
/// on ordinary Rust -- a trailing aligned comment, and plain spacing between two adjacent
/// literals. (Found by L.) That bug reached two crates because the predicate had been
/// COPIED, which is the whole argument for this file.
fn has_run_in_literal(line: &str) -> bool {
    line.trim_start_matches(' ')
        .split('"')
        .enumerate()
        .any(|(index, part)| index % 2 == 1 && part.contains("   "))
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
