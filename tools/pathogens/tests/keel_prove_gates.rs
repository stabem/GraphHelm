//! #1333: certification of the Keel test-proof gate (`graphhelm keel check --prove-new-tests`).
//!
//! Each specimen is really run: a two-commit crate is built on the base and on the head. Three
//! things are asserted: every specimen is defeated on the axis it claims (judged without the
//! gate), the gate refuses each one as green on the parent and for no other reason, and a real
//! regression test beside the same fix passes, so the gate is not "refuse everything".

use pathogens::keel_prove::{KeelProveGate, keel_prove_suite, real_regression_test};
use pathogens::{EvidenceGate, certify, is_defeated_on_its_axis};

fn gate(name: &str) -> KeelProveGate {
    KeelProveGate::under(std::env::temp_dir().join(format!(
        "graphhelm-keel-prove-{name}-{}",
        std::process::id()
    )))
}

#[test]
fn every_specimen_is_defeated_on_its_own_axis() {
    let suite = keel_prove_suite();
    assert_eq!(
        suite.len(),
        2,
        "one tautology, one mock that asserts the mock"
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
fn the_gate_refuses_each_specimen_as_green_on_parent_and_passes_a_real_regression_test() {
    let gate = gate("gates");
    for specimen in keel_prove_suite() {
        let verdict = gate.evaluate(&specimen.evidence);
        assert!(!verdict.passed, "{} passed the gate", specimen.id);
        assert!(
            !verdict.findings.is_empty()
                && verdict
                    .findings
                    .iter()
                    .all(|finding| finding.starts_with("keel.test.green_on_parent")),
            "{}: refused for another reason: {:?}",
            specimen.id,
            verdict.findings
        );
    }
    let regression = gate.evaluate(&real_regression_test());
    assert!(
        regression.passed,
        "a test red on the base and green on the head was refused: {:?}",
        regression.findings
    );
    let certification = certify(&gate, &keel_prove_suite()).expect("the proof gate certifies");
    assert_eq!(certification.gate_id, "graphhelm-keel/prove-new-tests");
    assert_eq!(certification.specimens, 2);
}
