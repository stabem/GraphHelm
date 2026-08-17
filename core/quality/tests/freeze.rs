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
