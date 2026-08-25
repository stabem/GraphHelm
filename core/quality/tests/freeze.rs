//! M06 Task 7, binding decision 5: gate definitions freeze before implementation — a
//! synthetic PR diff touching both a gate definition and gated code is REFUSED, and
//! either side alone is clean.

use graphhelm_quality::freeze_violation;

#[test]
fn a_diff_touching_gate_and_gated_together_is_refused() {
    let mixed = [
        "core/quality/src/lib.rs",
        "apps/cli/src/commands/serve/monitor.rs",
    ];
    let violation = freeze_violation(&mixed).expect("the mixed diff must be refused");
    assert!(violation.0.starts_with("core/quality/"));
    assert!(violation.1.starts_with("apps/cli/"));

    let gates_only = ["tools/pathogens/src/lib.rs", "core/quality/tests/thymus.rs"];
    assert!(
        freeze_violation(&gates_only).is_none(),
        "evolving the gates alone is legitimate"
    );

    let code_only = ["apps/cli/src/commands/serve/monitor.rs", "CHANGELOG.md"];
    assert!(
        freeze_violation(&code_only).is_none(),
        "evolving the code alone is legitimate"
    );
}

/// Whether the freeze covers this prefix, asked of the rule rather than of a copied list.
///
/// Pairing a path under `prefix` with a landmark that is certainly NOT gate machinery makes
/// `freeze_violation` answer exactly one question: is `prefix` frozen? A second copy of the
/// list here would be a second oracle, and duplicated oracles diverge in silence -- the
/// defect this repository spent #323 removing.
fn frozen(prefix: &str) -> bool {
    let under_the_prefix = format!("{prefix}any/file.rs");
    freeze_violation(&[under_the_prefix.as_str(), NOT_GATE_MACHINERY]).is_some()
}

/// A path no reasonable reading of the freeze covers.
const NOT_GATE_MACHINERY: &str = "README.md";

/// Every frozen prefix, named one assertion at a time, on purpose.
///
/// **The list is a `const` inside `freeze_violation` with exactly one reader** -- measured, not
/// assumed. A pull request that only removes a prefix touches `core/quality/` alone, which is a
/// clean gate-only diff.
///
/// **What a removal is NOT is silent, and the "before" measurement said so rather than confirming
/// what was expected.** Every one of the four is caught today. But the coverage is INCIDENTAL, and
/// that is what this test replaces:
///
/// | removing | caught today by | because |
/// |---|---|---|
/// | `core/quality/`, `tools/pathogens/` | the test above | it uses them as example paths |
/// | `tools/source-invariants/` | `shared_source_invariant_predicate.rs` | by design (#361) |
/// | `docs/gates/` | that same test's CONTROL | it was picked as a convenient known-gate landmark |
///
/// The last row is the fragile one: `docs/gates/` is protected only because someone needed a gate
/// path for an unrelated control. Change that landmark and the protection leaves with it, and
/// nothing anywhere records that it was load-bearing for a second purpose.
///
/// **And the message the incidental catch produces points at the wrong thing.** Removing
/// `docs/gates/` today reads `CONTROL FAILED: freeze_violation did not recognise a known gate
/// path` -- which sends the reader to debug the oracle, not to look at their own deletion. A guard
/// that fires for the right reason with the wrong name costs a debugging session before it helps.
///
/// This test cannot PREVENT that, and no in-repository check can: whoever may edit the rule may
/// edit the rule's test. What it changes is what the act LOOKS like. Removing a prefix now means
/// deleting a named assertion with its reason written beside it, rather than a comma in a list.
/// **Lowering a threshold is a plausible edit; deleting a named assertion is a visible one** --
/// the same discipline the scan floors in this repository already use, applied to the frozen set.
///
/// One assertion per prefix rather than a loop over an array, deliberately. A loop would put the
/// prefixes back into a list, and a list element is exactly what deletes without comment.
///
/// This is not hypothetical. `.factory/h-agent-211-surface-blueprint.md` records a sealed claim
/// whose death condition is *"`GATE_MACHINERY` stops listing `tools/pathogens/`"* -- another
/// lane's prediction depends on this set not shrinking, and before this test nothing would have
/// told them it had. (#342)
#[test]
fn every_frozen_prefix_is_named_here_so_removing_one_deletes_an_assertion() {
    // CONTROL FIRST. `frozen` returning true for everything would satisfy every assertion below
    // while observing nothing.
    assert!(
        !frozen("apps/cli/"),
        "CONTROL FAILED: ordinary code reads as frozen, so the verdicts below mean nothing"
    );

    assert!(
        frozen("core/quality/"),
        "core/quality/ left the freeze. It holds the rule itself, its enforcement, and the \
         thymus suite -- unfrozen, a branch may rewrite the judge in the same breath as the code \
         the judge is judging, which is the whole of M06 binding decision 5"
    );

    assert!(
        frozen("tools/pathogens/"),
        "tools/pathogens/ left the freeze. It is the pathogen suite the gate certifies against, \
         and a branch that edits a pathogen alongside the code it detects has moved the target \
         and the shot together"
    );

    assert!(
        frozen("docs/gates/"),
        "docs/gates/ left the freeze. It holds the gate stamps a stream is certified by, so a \
         branch that restamps while changing what the stamp attests has certified itself"
    );

    assert!(
        frozen("tools/source-invariants/"),
        "tools/source-invariants/ left the freeze. It holds the shared detection predicate, and \
         a predicate is not an input to a gate -- it IS the gate. Outside the freeze it can be \
         edited in the same pull request as the code it judges (#323)"
    );
}
