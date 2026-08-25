//! The thymus harness proofs: the bred suite of green-but-useless specimens (the original set
//! plus the M08 interaction-cost pair), certification refused on
//! any pass, digest-voiding on suite growth, and the per-specimen fooling power that
//! makes each pathogen load-bearing.

use std::collections::BTreeSet;

use pathogens::{
    CandidateGate, Deliverable, DiffSummary, GeometrySpecimen, RefusalCause, Specimen,
    UselessnessMode, certification_is_current, certify, correctness_battery,
    is_defeated_on_its_axis, paired_trivial_gate, reject_everything_gate, suite, suite_digest,
};

fn all_modes() -> Vec<UselessnessMode> {
    vec![
        UselessnessMode::DeadFeature,
        UselessnessMode::UnreachableUi,
        UselessnessMode::TautologicalJourney,
        UselessnessMode::BlankScreen,
        UselessnessMode::OrphanView,
        UselessnessMode::GuttedAssertion,
        UselessnessMode::HappyPathOnly,
        UselessnessMode::SpecClaimWithoutArtifact,
        UselessnessMode::MinimalDiffNoBehavior,
        UselessnessMode::LabelSwappedUi,
    ]
}

#[test]
fn the_suite_holds_twelve_specimens_one_per_mode() {
    let bred = suite();
    assert_eq!(
        bred.len(),
        12,
        "the suite is TWELVE specimens: the original ten plus the interaction-cost pair \n(M08). The count is pinned so growing the suite is a deliberate edit, never a drift"
    );
    let modes: BTreeSet<_> = bred.iter().map(|specimen| specimen.axis).collect();
    assert_eq!(modes.len(), 12, "one specimen per uselessness mode");
    let ids: BTreeSet<_> = bred.iter().map(|specimen| specimen.id.clone()).collect();
    assert_eq!(ids.len(), 12, "specimen ids are distinct");
}

#[test]
fn every_specimen_is_green_by_the_correctness_battery() {
    let battery = correctness_battery();
    for specimen in suite() {
        let verdict = battery.evaluate(&specimen.evidence);
        assert!(
            verdict.passed,
            "specimen {} must look fine to naive correctness checks, findings: {:?}",
            specimen.id, verdict.findings
        );
    }
}

#[test]
fn every_specimen_is_defeated_on_its_axis() {
    for specimen in suite() {
        assert!(
            is_defeated_on_its_axis(&specimen),
            "specimen {} lost its uselessness — a weakened pathogen",
            specimen.id
        );
    }
}

#[test]
fn each_specimen_fools_its_paired_plausible_gate() {
    for specimen in suite() {
        let gate = paired_trivial_gate(specimen.axis);
        let verdict = gate.evaluate(&specimen.evidence);
        assert!(
            verdict.passed,
            "specimen {} no longer fools {} — its fooling power is gone",
            specimen.id,
            gate.id()
        );
    }
}

#[test]
fn every_paired_trivial_gate_fails_certification_citing_its_specimen() {
    let bred = suite();
    for mode in all_modes() {
        let gate = paired_trivial_gate(mode);
        let refusal = certify(gate.as_ref(), &bred)
            .expect_err("a plausible-but-blind gate must never be certified");
        let expected = bred
            .iter()
            .find(|specimen| specimen.axis == mode)
            .expect("one specimen per mode")
            .id
            .clone();
        assert!(
            refusal.fooled_by.contains(&expected),
            "{} was refused but the refusal does not cite {expected}: {:?}",
            gate.id(),
            refusal.fooled_by
        );
    }
}

#[test]
fn the_correctness_battery_itself_fails_certification_on_all_twelve() {
    let refusal = certify(correctness_battery().as_ref(), &suite())
        .expect_err("correctness alone certifies nothing");
    assert_eq!(
        refusal.fooled_by.len(),
        12,
        "every specimen is green by correctness measures, so all TWELVE fool the battery — \nthe two interaction-cost specimens included: one answers in six calls, the other answers nothing in one, and correctness cannot see either"
    );
}

#[test]
fn a_gate_that_rejects_every_specimen_is_certified_with_the_suite_digest() {
    let bred = suite();
    let certification = certify(reject_everything_gate().as_ref(), &bred)
        .expect("rejecting all twelve earns the receipt");
    assert_eq!(certification.gate_id, "reject-everything");
    assert_eq!(certification.specimens, 12);
    assert_eq!(certification.suite_digest, suite_digest(&bred));
}

#[test]
fn the_suite_digest_is_stable_and_wire_hash_shaped() {
    let first = suite_digest(&suite());
    let second = suite_digest(&suite());
    assert_eq!(first, second, "the digest is deterministic");
    let hex = first
        .strip_prefix("sha256:")
        .expect("the digest is a WireHash: sha256:<64 hex>");
    assert_eq!(hex.len(), 64);
    assert!(
        hex.bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );
}

#[test]
fn growing_the_suite_voids_old_certifications() {
    let bred = suite();
    let certification =
        certify(reject_everything_gate().as_ref(), &bred).expect("certified against twelve");
    assert!(
        certification_is_current(&certification.suite_digest, &bred),
        "the receipt binds while the suite stands still"
    );

    let mut grown = bred.clone();
    let eleventh = Specimen {
        id: "eleventh-escape".to_owned(),
        axis: UselessnessMode::BlankScreen,
        evidence: Deliverable {
            claims: Vec::new(),
            html: String::new(),
            reachable_ids: BTreeSet::new(),
            journey: Vec::new(),
            tests: Vec::new(),
            diff: DiffSummary {
                files_touched: 1,
                behavior_lines: 0,
            },
            // Makes no claim about interaction — this fixture is about growing the suite.
            interaction: None,
        },
    };
    grown.push(eleventh);
    assert!(
        !certification_is_current(&certification.suite_digest, &grown),
        "growing the suite changes the digest and voids old immunity — recertify or stand down"
    );
}

#[test]
fn certification_refusal_names_the_gate() {
    struct HalfBlind;
    impl CandidateGate for HalfBlind {
        fn id(&self) -> &str {
            "half-blind"
        }
        fn evaluate(&self, deliverable: &Deliverable) -> pathogens::Verdict {
            pathogens::Verdict {
                passed: deliverable.html.is_empty(),
                findings: vec!["content looks present".to_owned()],
            }
        }
    }
    let refusal = certify(&HalfBlind, &suite()).expect_err("passing the blank screen refuses");
    assert_eq!(refusal.gate_id, "half-blind");
    assert!(
        refusal
            .fooled_by
            .iter()
            .any(|id| id.contains("blank-screen")),
        "the blank screen fooled it: {:?}",
        refusal.fooled_by
    );
}

// ---------------------------------------------------------------------------------------------
// The floor: a suite with no specimens must never certify.
// ---------------------------------------------------------------------------------------------

/// An empty suite REFUSES, and says why.
///
/// The production change that would make this fail: removing the emptiness check from `certify`.
/// Before it existed, an empty suite made `fooled_by` empty, so the function fell through to
/// `Ok(Certification { specimens: 0 })` — a gate certified against no pathogens at all, which is
/// the certification-that-certifies-nothing of #294 promoted to a feature.
///
/// **Why the cause is a field rather than `fooled_by.is_empty()`.** Emptiness would technically
/// discriminate, because a real refusal always names at least one specimen. But the CLI renders a
/// refusal as *"the candidate passed pathogens {fooled_by:?}"*, so the empty case would print
/// **"the candidate passed pathogens []"** — actively false, since the gate passed nothing and
/// there was nothing to pass. Two distinct causes flattened into one legal value is how a boundary
/// loses the distinction that mattered.
#[test]
fn an_empty_suite_refuses_instead_of_certifying_vacuously() {
    let empty: Vec<GeometrySpecimen> = Vec::new();
    let refusal = certify(reject_everything_gate().as_ref(), &empty)
        .expect_err("an empty suite must not certify");

    assert_eq!(refusal.gate_id, "reject-everything");
    assert_eq!(
        refusal.cause,
        RefusalCause::EmptySuite,
        "the refusal must name the empty suite as the cause, not imply the gate was fooled"
    );
    assert!(
        refusal.fooled_by.is_empty(),
        "nothing fooled the gate; `fooled_by` keeps its exact meaning"
    );
}

/// The floor must NOT fire on a real suite — otherwise `certify` refuses everything and the test
/// above passes for a reason that has nothing to do with emptiness.
#[test]
fn a_real_suite_still_certifies() {
    certify(reject_everything_gate().as_ref(), &suite())
        .expect("the bred suite must still certify a gate that rejects everything");
}

/// And a genuine refusal must still report the gate being fooled, not the new cause.
///
/// Without this, `cause: EmptySuite` on every refusal would satisfy the first test while
/// destroying the distinction it was added to make.
#[test]
fn a_gate_that_is_fooled_reports_a_different_cause_than_an_empty_suite() {
    let refusal = certify(correctness_battery().as_ref(), &suite())
        .expect_err("the correctness battery is fooled by the bred suite");
    assert_eq!(refusal.cause, RefusalCause::GatePassedSpecimens);
    assert!(
        !refusal.fooled_by.is_empty(),
        "a gate-passed refusal must name what fooled it"
    );
}
