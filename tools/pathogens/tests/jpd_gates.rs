//! #211 Task 2: JPD typed evidence as a second instantiation of the harness.
//!
//! Evidence is carried as validated JSON rather than re-declared Rust structs. The schemas in
//! `extensions/builtin/graphhelm-jpd/schemas/` are the authority, and a second Rust declaration of
//! the same closed shapes is a second producer of one vocabulary — which drifts in silence,
//! because a rename preserves a count and everything still compiles.

use pathogens::jpd::{JpdEvidence, JpdFailureAxis, JpdSpecimen, VerificationResultGate, jpd_suite};
use pathogens::{EvidenceGate, certify, is_defeated_on_its_axis};
use serde_json::json;

/// The axes are different KINDS, and the compiler is what says so.
///
/// The production change that would make this fail: folding the JPD axes into `UselessnessMode`.
#[test]
fn a_jpd_specimen_carries_a_jpd_axis() {
    let specimen: JpdSpecimen = JpdSpecimen {
        id: "verification/capability-missing-claimed-proven".to_owned(),
        axis: JpdFailureAxis::CapabilityMissingUnderClaimedSuccess,
        evidence: JpdEvidence::VerificationResult(json!({
            "proposedResultStatus": "proven",
            "gate": { "status": "capability_missing" }
        })),
    };

    assert_eq!(
        specimen.id,
        "verification/capability-missing-claimed-proven"
    );
}

/// Fixture integrity generalises WITH the harness, or it is dropped in silence.
///
/// N's finding: a specimen claiming an axis it does not defeat certifies a gate for catching
/// nothing, and nothing else in the harness notices.
#[test]
fn a_jpd_axis_can_say_whether_its_own_specimen_is_genuinely_defeated() {
    let real = JpdSpecimen {
        id: "verification/capability-missing-claimed-proven".to_owned(),
        axis: JpdFailureAxis::CapabilityMissingUnderClaimedSuccess,
        evidence: JpdEvidence::VerificationResult(json!({
            "proposedResultStatus": "proven",
            "gate": { "status": "capability_missing" }
        })),
    };
    let fraudulent = JpdSpecimen {
        id: "verification/gate-actually-ran".to_owned(),
        axis: JpdFailureAxis::CapabilityMissingUnderClaimedSuccess,
        evidence: JpdEvidence::VerificationResult(json!({
            "proposedResultStatus": "proven",
            "gate": { "status": "evaluated" }
        })),
    };

    assert!(
        is_defeated_on_its_axis(&real),
        "a result claiming proven while its gate never ran IS defeated on this axis"
    );
    assert!(
        !is_defeated_on_its_axis(&fraudulent),
        "a specimen naming an axis it does not defeat must be caught: the digest sees CHANGE,          never QUALITY, so growth alone would certify a gate for catching nothing"
    );
}

/// The gate earns its certification by rejecting every specimen in its own suite.
#[test]
fn the_verification_gate_rejects_every_specimen_in_its_suite() {
    let certification =
        certify(&VerificationResultGate, &jpd_suite()).expect("the gate rejects all specimens");

    assert_eq!(certification.gate_id, "gate/jpd-verification-result");
    assert!(
        certification.specimens >= 2,
        "the suite must exercise more than one axis, or the certification says less than it looks"
    );
}

/// Every specimen in the shipped suite must genuinely defeat the axis it names.
///
/// Without this the suite could grow by worthless specimens: the digest would change, every
/// prior certification would stop binding, and the gate would be certified for catching nothing.
#[test]
fn every_shipped_specimen_genuinely_defeats_its_axis() {
    let suite = jpd_suite();
    // Guards the LOOP below, not `certify`: a `for` over an empty collection asserts nothing and
    // the test passes having checked no specimen at all. `certify`'s own empty-suite floor does
    // NOT make this redundant, because this test never calls `certify` -- the two guard different
    // things and the original message here conflated them, which is what nearly got this deleted
    // when the floor landed.
    assert!(
        !suite.is_empty(),
        "HARNESS-BROKE: the shipped suite is empty, so the loop below checks nothing"
    );
    for specimen in &suite {
        assert!(
            is_defeated_on_its_axis(specimen),
            "specimen {} names an axis it does not defeat",
            specimen.id
        );
    }
}

/// ADVISORY IN BOTH DIRECTIONS. Found by L, and the direction nobody guards is the one that gets
/// implemented.
///
/// A gate consulting the council only in order to REFUSE would pass a one-directional arm
/// untouched — and refusing on disagreement looks conservative, which is why it is the direction
/// reached for. Same evidence, opposite verdicts, IDENTICAL result.
///
/// The council is injected at `bindings.council`, which is where the schema actually puts it —
/// a top-level `council` key would test a shape no document has.
#[test]
fn a_council_verdict_moves_nothing_in_either_direction() {
    let base = json!({
        "proposedResultStatus": "proven",
        "gate": { "status": "evaluated" },
        "retry": { "outcomeClassification": "first_pass_success" },
        "bindings": {}
    });

    let mut approving = base.clone();
    approving["bindings"]["council"] = json!({ "status": "unanimous", "verdict": "approve" });
    let mut rejecting = base.clone();
    rejecting["bindings"]["council"] = json!({ "status": "unanimous", "verdict": "reject" });

    let bare = VerificationResultGate.evaluate(&JpdEvidence::VerificationResult(base));
    let with_approval =
        VerificationResultGate.evaluate(&JpdEvidence::VerificationResult(approving));
    let with_rejection =
        VerificationResultGate.evaluate(&JpdEvidence::VerificationResult(rejecting));

    assert_eq!(
        with_approval.passed, bare.passed,
        "a unanimous approval must not rescue anything"
    );
    assert_eq!(
        with_rejection.passed, bare.passed,
        "a unanimous rejection must not sink anything either"
    );
    assert_eq!(
        with_approval.findings, with_rejection.findings,
        "identical evidence under opposite council verdicts must produce identical findings"
    );
}

// ---------------------------------------------------------------------------------------------
// THE ARM THAT WOULD HAVE CAUGHT IT: the repository's own verification-result fixtures, driven
// through the real gate.
//
// The first version of this gate read `status`, `observers`, `attempts`, `producer` and
// `validator`. None of those exist in the declared schema, which requires `proposedResultStatus`
// and spells success as `proven`. So the gate's opening check found no `status`, concluded the
// document was not claiming success, and PASSED it — every schema-valid document, including the
// repository's own negative fixture, which exists precisely to be refused.
//
// Nothing caught it because MY SPECIMENS SHARED MY INVENTED VOCABULARY. The fixture and the defect
// had the same shape, so the suite agreed with the gate about a language neither the schema nor any
// real document speaks. Synthetic specimens can only disagree with a gate about logic; they cannot
// disagree with it about vocabulary. Real documents can, and that is the whole reason these arms
// exist. (Found by N wiring #226 against this gate.)

use std::path::{Path, PathBuf};

fn jpd_fixture(relative: &str) -> serde_json::Value {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../extensions/builtin/graphhelm-jpd/fixtures")
        .join(relative);
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("HARNESS-BROKE: cannot read {}: {e}", path.display()));
    serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("HARNESS-BROKE: {} is not valid JSON: {e}", path.display()))
}

/// The repository's own negative fixture must be REFUSED. It claims `proven` while its gate
/// reports `capability_missing` — a result claiming success over a capability that never ran.
#[test]
fn the_repositorys_negative_fixture_is_refused_by_the_gate() {
    let document = jpd_fixture("negative/journey-verification-missing-gate-claimed-success.json");

    assert_eq!(
        document
            .get("proposedResultStatus")
            .and_then(serde_json::Value::as_str),
        Some("proven"),
        "HARNESS-BROKE: this fixture is only meaningful while it CLAIMS success; if the vocabulary          moved again, this arm is comparing against something else entirely"
    );

    let verdict = VerificationResultGate.evaluate(&JpdEvidence::VerificationResult(document));

    assert!(
        !verdict.passed,
        "the repository's own missing-gate-claimed-success fixture must be refused: it exists to          be refused, and a gate that passes it certifies nothing"
    );
}

/// And the positive fixtures must still PASS, so the fix is not "refuse everything".
///
/// Without this pair the negative arm alone is satisfied by a gate that refuses unconditionally,
/// which is the cheapest possible false fix.
#[test]
fn the_repositorys_positive_fixtures_are_accepted_by_the_gate() {
    for name in [
        "positive/journey-verification-first-pass.json",
        "positive/journey-verification-accepted-with-waiver.json",
    ] {
        let document = jpd_fixture(name);
        let verdict = VerificationResultGate.evaluate(&JpdEvidence::VerificationResult(document));
        assert!(
            verdict.passed,
            "{name} is a well-formed accepted result and must pass; findings were {:?}",
            verdict.findings
        );
    }
}

// MUTATION EVIDENCE for the vocabulary fix.
//
//   baseline                                            -> 7 passed
//   M5  restore the original defect: read the invented
//       `status`/`passed` instead of `proposedResultStatus`/`proven`
//                                                       -> 3 passed, 4 failed, INCLUDING
//          the_repositorys_negative_fixture_is_refused_by_the_gate
//   revert                                              -> 7 passed
//
// The asymmetry is the whole point, and it is measurable:
//
//   BEFORE the fix: every synthetic arm PASSED and only the real-fixture arm failed.
//   AFTER  the fix: reverting the vocabulary reddens the synthetic arms too.
//
// Before the fix the synthetic specimens shared the gate's invented vocabulary, so they could not
// disagree with it — a suite written by the same hand as the gate agrees with it about language by
// construction, and language was the defect. Anchoring the suite to the schema's words is what
// gives those arms the power to fail at all; the real fixtures are what proved the words.
