//! #1330: certification of the Keel scope gate (`graphhelm keel check`).
//!
//! Three things are asserted, each on its own so a failure names which one broke: every specimen is
//! really defeated on the axis it claims (judged without the gate), the gate rejects every
//! specimen for the scope rule and not for some other reason, and the same diffs with the stray
//! path listed on the card pass, so the gate is not "refuse everything".

use pathogens::keel_scope::{KeelScopeGate, keel_scope_suite};
use pathogens::{EvidenceGate, certify, is_defeated_on_its_axis};

#[test]
fn every_specimen_is_defeated_on_its_own_axis() {
    let suite = keel_scope_suite();
    assert_eq!(
        suite.len(),
        2,
        "one out-of-card edit, one sneaked-in test file"
    );
    for specimen in &suite {
        assert!(
            is_defeated_on_its_axis(specimen),
            "{} does not show the defect it names",
            specimen.id
        );
    }
}

#[test]
fn the_gate_certifies_and_refuses_each_specimen_for_scope_alone() {
    let gate = KeelScopeGate::shipped();
    for specimen in keel_scope_suite() {
        let verdict = gate.evaluate(&specimen.evidence);
        assert!(!verdict.passed, "{} passed the gate", specimen.id);
        assert!(
            verdict
                .findings
                .iter()
                .all(|finding| finding.starts_with("keel.scope.path_outside_card")),
            "{}: refused for another reason: {:?}",
            specimen.id,
            verdict.findings
        );
    }
    let certification = certify(&gate, &keel_scope_suite()).expect("the scope gate certifies");
    assert_eq!(certification.gate_id, "graphhelm-keel/check");
    assert_eq!(certification.specimens, 2);
}

#[test]
fn the_same_diffs_pass_once_the_card_lists_every_path() {
    let gate = KeelScopeGate::shipped();
    for mut specimen in keel_scope_suite() {
        specimen.evidence.card.scope_paths = vec!["src".to_owned(), "tests".to_owned()];
        let verdict = gate.evaluate(&specimen.evidence);
        assert!(
            verdict.passed,
            "{} with a card covering every path: {:?}",
            specimen.id, verdict.findings
        );
        assert!(!is_defeated_on_its_axis(&specimen));
    }
}
