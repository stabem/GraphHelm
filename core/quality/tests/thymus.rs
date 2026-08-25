//! M06 Task 3, the certification half: the COMPOSED geometry evaluator earns the thymus
//! receipt by rejecting every pathogen in the suite — and the layout grammar ALONE is REFUSED,
//! which is the crate's second rule as a test: geometry cannot see a gutted assertion or
//! an empty diff, so geometry alone must never gate.

use std::collections::BTreeSet;

use graphhelm_quality::{
    ContentManifest, Delivered, DeliveredClaim, DiffShape, JourneyStep, LayoutBudget, TestCheck,
    check_layout, evaluate_geometry,
};
use pathogens::{CandidateGate, Verdict, certify, suite};

/// Maps a pathogen deliverable onto the evaluators' own input surface.
fn delivered_from(deliverable: &pathogens::Deliverable) -> Delivered {
    Delivered {
        claims: deliverable
            .claims
            .iter()
            .map(|claim| DeliveredClaim {
                feature: claim.feature.clone(),
                element_id: claim.element_id.clone(),
                artifact: claim.artifact.clone(),
            })
            .collect(),
        html: deliverable.html.clone(),
        reachable_ids: deliverable
            .reachable_ids
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>(),
        journey: deliverable
            .journey
            .iter()
            .map(|step| JourneyStep {
                action: step.action.clone(),
                assertion: step.assertion.clone(),
                exercises_error_path: step.exercises_error_path,
            })
            .collect(),
        tests: deliverable
            .tests
            .iter()
            .map(|test| TestCheck {
                name: test.name.clone(),
                passed: test.passed,
                assertions: test.assertions,
            })
            .collect(),
        diff: DiffShape {
            files_touched: deliverable.diff.files_touched,
            behavior_lines: deliverable.diff.behavior_lines,
        },
    }
}

struct GeometryGate;

impl CandidateGate for GeometryGate {
    fn id(&self) -> &str {
        "gate-geometry"
    }
    fn evaluate(&self, deliverable: &pathogens::Deliverable) -> Verdict {
        let delivered = delivered_from(deliverable);
        let manifest = ContentManifest::from_claims(&delivered.claims);
        let findings = evaluate_geometry(&delivered, &manifest, &LayoutBudget::default());
        Verdict {
            passed: findings.is_empty(),
            findings: findings.into_iter().map(|f| f.claim).collect(),
        }
    }
}

struct LayoutOnlyGate;

impl CandidateGate for LayoutOnlyGate {
    fn id(&self) -> &str {
        "gate-layout-only"
    }
    fn evaluate(&self, deliverable: &pathogens::Deliverable) -> Verdict {
        let findings = check_layout(&deliverable.html, &LayoutBudget::default());
        Verdict {
            passed: findings.is_empty(),
            findings: findings.into_iter().map(|f| f.claim).collect(),
        }
    }
}

#[test]
fn the_composed_geometry_evaluator_earns_its_certification() {
    let suite = suite();
    let certification =
        certify(&GeometryGate, &suite).expect("the composed evaluator must reject every pathogen");
    assert_eq!(certification.gate_id, "gate-geometry");
    // DELIBERATE (M08): the suite grew to twelve with the interaction-cost pair. The
    // composed geometry evaluator still rejects EVERY specimen — certification succeeded,
    // only this pinned count moved — so growing the suite did not weaken what this test
    // proves; it widened what the gate had to refuse to keep saying it.
    assert_eq!(certification.specimens, 12);
    assert!(certification.suite_digest.starts_with("sha256:"));
}

/// The second rule as a test: geometry alone CANNOT see every uselessness mode (a gutted
/// assertion has flawless geometry), so certification refuses it — and names who fooled it.
#[test]
fn the_layout_grammar_alone_is_refused_certification() {
    let refusal =
        certify(&LayoutOnlyGate, &suite()).expect_err("geometry alone must never certify");
    assert!(
        !refusal.fooled_by.is_empty(),
        "the refusal names the pathogens that got through"
    );
}
