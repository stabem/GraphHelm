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
/// The production change that would make this fail: folding the JPD axes into
/// `UselessnessMode` — which is the tempting repair, and would give every exhaustive match over
/// that enum arms it cannot mean, silently, because the enum still compiles everywhere.
#[test]
fn a_jpd_specimen_carries_a_jpd_axis() {
    let specimen: JpdSpecimen = JpdSpecimen {
        id: "verification/missing-observer".to_owned(),
        axis: JpdFailureAxis::MissingObserver,
        evidence: JpdEvidence::VerificationResult(json!({ "status": "passed", "observers": [] })),
    };

    assert_eq!(specimen.id, "verification/missing-observer");
}

/// Fixture integrity generalises WITH the harness, or it is dropped in silence.
///
/// This is the arm that N's finding demanded: a specimen claiming an axis it does not defeat
/// certifies a gate for catching nothing, and nothing else in the harness notices.
#[test]
fn a_jpd_axis_can_say_whether_its_own_specimen_is_genuinely_defeated() {
    let real = JpdSpecimen {
        id: "verification/missing-observer".to_owned(),
        axis: JpdFailureAxis::MissingObserver,
        evidence: JpdEvidence::VerificationResult(json!({ "status": "passed", "observers": [] })),
    };
    let fraudulent = JpdSpecimen {
        id: "verification/not-actually-missing".to_owned(),
        axis: JpdFailureAxis::MissingObserver,
        evidence: JpdEvidence::VerificationResult(json!({
            "status": "passed",
            "observers": [{ "capability": "first_party_deterministic" }]
        })),
    };

    assert!(
        is_defeated_on_its_axis(&real),
        "a result claiming success with no observer IS defeated on the missing-observer axis"
    );
    assert!(
        !is_defeated_on_its_axis(&fraudulent),
        "a specimen that names an axis it does not defeat must be caught here: the digest sees          CHANGE, never QUALITY, so growth alone would certify a gate for catching nothing"
    );
}

/// The gate earns its certification by rejecting every specimen in its own suite.
#[test]
fn the_verification_gate_rejects_every_specimen_in_its_suite() {
    let certification =
        certify(&VerificationResultGate, &jpd_suite()).expect("the gate rejects all specimens");

    assert_eq!(certification.gate_id, "gate/jpd-verification-result");
    assert!(
        certification.specimens >= 3,
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
    assert!(!suite.is_empty(), "HARNESS-BROKE: an empty suite certifies nothing and passes");
    for specimen in &suite {
        assert!(
            is_defeated_on_its_axis(specimen),
            "specimen {} names an axis it does not defeat",
            specimen.id
        );
    }
}

/// ADVISORY IN BOTH DIRECTIONS. Found by L, and the direction nobody guards is the one that
/// gets implemented.
///
/// The obvious arm proves a unanimous approval cannot rescue bad evidence. But a gate that
/// consulted the council only in order to REFUSE would pass that arm untouched — and refusing on
/// disagreement looks conservative, so it is exactly what a well-meaning implementer reaches for.
///
/// Same evidence, opposite council verdicts, IDENTICAL result. That is the only shape that pins
/// "the council has no code path" rather than "the council is used carefully".
#[test]
fn a_council_verdict_moves_nothing_in_either_direction() {
    let base = serde_json::json!({
        "status": "passed",
        "observers": [{ "capability": "first_party_deterministic" }]
    });

    let mut approving = base.clone();
    approving["council"] = json!({ "agreement": "unanimous", "verdict": "approve" });
    let mut rejecting = base.clone();
    rejecting["council"] = json!({ "agreement": "unanimous", "verdict": "reject" });

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
        "a unanimous rejection must not sink anything either: refusing on disagreement looks          conservative, which is precisely why it is the direction that gets implemented unguarded"
    );
    assert_eq!(
        with_approval.findings, with_rejection.findings,
        "identical evidence under opposite council verdicts must produce identical findings"
    );
}

// MUTATION MATRIX, run against a COMMITTED tree and reverted with `git checkout` against that
// commit, so no mutation could eat the work it was testing.
//
//   baseline                                  -> 5 passed
//   M1  MissingObserver never fires           -> 2 passed, 3 failed
//         a_jpd_axis_can_say_whether_its_own_specimen_is_genuinely_defeated
//         every_shipped_specimen_genuinely_defeats_its_axis
//         the_verification_gate_rejects_every_specimen_in_its_suite
//   M4  a rejecting council sinks the verdict -> 4 passed, 1 failed
//         a_council_verdict_moves_nothing_in_either_direction   (ONLY)
//   revert                                    -> 5 passed
//
// M1 reddening THREE arms is deliberate layering, not coarse assertions: the axis predicate, the
// shipped specimens' integrity, and the gate's certification are three layers over one property,
// and each fails for its own reason. A mutation reddening two arms that assert the SAME thing
// would mean they must be split; these assert different things about one predicate.
//
// M4 is the one that justifies L's finding. `council` appears nowhere else in this file's fixtures
// and nowhere in the shipped suite — measured, not assumed — so WITHOUT the both-directions cell,
// M4 would have reddened NOTHING. A gate reading the council purely to refuse would have shipped
// green. The direction nobody guards is the one that gets implemented, because refusing on
// disagreement looks conservative.
//
// A note on the harness itself: the first run of M1 was reported as a DEAD MUTATION that failed to
// compile. It had not. `cargo` prints `error: test failed` for a red test, and a `^error` match
// counts that as a build failure — so a genuine red was labelled unbuildable and nearly discarded.
// The build check now matches `^error[E` or `could not compile`, which is what a compile failure
// actually looks like. A sabotage harness needs its own control, exactly like everything else here.
