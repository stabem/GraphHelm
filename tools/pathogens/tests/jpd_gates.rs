//! #211 Task 2: JPD typed evidence as a second instantiation of the harness.
//!
//! Evidence is carried as validated JSON rather than re-declared Rust structs. The schemas in
//! `extensions/builtin/graphhelm-jpd/schemas/` are the authority, and a second Rust declaration of
//! the same closed shapes is a second producer of one vocabulary — which drifts in silence,
//! because a rename preserves a count and everything still compiles.

use pathogens::jpd::{
    JourneyContractEvidence, JourneyContractGate, JourneyDiagnosticSeverity, JpdEvidence,
    JpdFailureAxis, JpdSpecimen, VerificationResultGate, journey_contract_suite, jpd_suite,
};
use pathogens::{EvidenceGate, certify, is_defeated_on_its_axis};
use serde_json::{Value, json};

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
        "a specimen naming an axis it does not defeat must be caught: the digest sees CHANGE, never QUALITY, so growth alone would certify a gate for catching nothing"
    );
}

fn journey_contract(observer: &str) -> serde_json::Value {
    json!({
        "contractId": "journey/checkout-s1b",
        "actors": [{
            "actorId": "agent/checkout-driver",
            "name": "Checkout driver",
            "goal": "Submit the order"
        }],
        "steps": [{
            "stepId": "step/submit-order",
            "actorId": "agent/checkout-driver"
        }],
        "promises": [{
            "promiseId": "promise/receipt-durable",
            "stepId": "step/submit-order",
            "requiredObserverCapability": observer
        }]
    })
}

const CONTRACT_DIGEST: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";

fn journey_evidence(observer_id: &str) -> JourneyContractEvidence {
    JourneyContractEvidence {
        contract: journey_contract("browser.semantic-journey"),
        contract_digest: CONTRACT_DIGEST.to_owned(),
        observation_obligations: vec![observation_obligation(
            "promise/receipt-durable",
            "browser.semantic-journey",
            observer_id,
        )],
        verification_result: json!({
            "contractId": "journey/checkout-s1b",
            "contractDigest": CONTRACT_DIGEST,
            "bindings": {
                "observers": [{ "observerId": observer_id }]
            }
        }),
    }
}

fn observation_obligation(promise_id: &str, capability_id: &str, observer_id: &str) -> Value {
    json!({
        "contractId": "journey/checkout-s1b",
        "contractDigest": CONTRACT_DIGEST,
        "promiseId": promise_id,
        "observerRequirements": { "capability": capability_id },
        "resolution": {
            "status": "matched",
            "capabilityBinding": {
                "capabilityId": capability_id,
                "observerId": observer_id
            }
        }
    })
}

fn assert_journey_diagnostic(
    evidence: JpdEvidence,
    expected_code: &str,
    expected_path: &str,
    expected_message: &str,
    expected_source: &str,
) {
    let diagnostics = JourneyContractGate.diagnostics(&evidence);
    assert_eq!(diagnostics.len(), 1, "diagnostics were {diagnostics:?}");
    assert_eq!(diagnostics[0].code, expected_code);
    assert_eq!(diagnostics[0].path, expected_path);
    assert_eq!(diagnostics[0].severity, JourneyDiagnosticSeverity::Error);
    assert_eq!(diagnostics[0].source_file, expected_source);
    assert_eq!(diagnostics[0].message, expected_message);
}

fn executed_council_with_decision(blocked: bool) -> Value {
    let fixture = jpd_fixture("positive/journey-verification-accepted-with-waiver.json");
    let mut council = fixture["bindings"]["council"].clone();
    if blocked {
        council["result"]["status"] = json!("blocked");
        council["result"]["decision"]["status"] = json!("blocked");
    }
    council
}

#[test]
fn journey_contract_gate_refuses_an_actor_used_as_its_own_observer() {
    let verdict = JourneyContractGate.evaluate(&JpdEvidence::JourneyContract(journey_evidence(
        "agent/checkout-driver",
    )));

    assert!(!verdict.passed);
    assert_eq!(
        verdict.findings,
        [
            "promise promise/receipt-durable is bound to observer agent/checkout-driver, which is also the actor for step step/submit-order; the observer must be independent from the actor"
        ]
    );
}

#[test]
fn journey_contract_gate_accepts_an_independent_observer() {
    let verdict = JourneyContractGate.evaluate(&JpdEvidence::JourneyContract(journey_evidence(
        "graphhelm-browser-user-journey",
    )));

    assert!(verdict.passed, "findings were {:?}", verdict.findings);
    assert!(verdict.findings.is_empty());
}

#[test]
fn real_observer_identity_moves_the_verdict_while_capability_stays_constant() {
    let evidence = |observer_id: &str| JpdEvidence::JourneyContract(journey_evidence(observer_id));

    let self_observed = JourneyContractGate.evaluate(&evidence("agent/checkout-driver"));
    let independently_observed =
        JourneyContractGate.evaluate(&evidence("graphhelm-browser-user-journey"));

    assert!(
        !self_observed.passed,
        "the real observer identity is the actor"
    );
    assert!(
        independently_observed.passed,
        "the capability is unchanged, but the real observer is independent: {:?}",
        independently_observed.findings
    );
}

#[test]
fn journey_contract_gate_refuses_a_mismatched_contract_id_binding() {
    let mut evidence = journey_evidence("graphhelm-browser-user-journey");
    evidence.verification_result["contractId"] = json!("journey/different-contract");

    let verdict = JourneyContractGate.evaluate(&JpdEvidence::JourneyContract(evidence));

    assert!(!verdict.passed);
    assert_eq!(
        verdict.findings,
        ["verification result contractId does not match the journey contract"]
    );
}

#[test]
fn journey_contract_gate_refuses_a_mismatched_contract_digest_binding() {
    let mut evidence = journey_evidence("graphhelm-browser-user-journey");
    evidence.verification_result["contractDigest"] =
        json!("sha256:2222222222222222222222222222222222222222222222222222222222222222");

    let verdict = JourneyContractGate.evaluate(&JpdEvidence::JourneyContract(evidence));

    assert!(!verdict.passed);
    assert_eq!(
        verdict.findings,
        ["verification result contractDigest does not match the supplied journey contract digest"]
    );
}

#[test]
fn a_binding_failure_suppresses_identity_claims() {
    let mut evidence = journey_evidence("agent/checkout-driver");
    evidence.verification_result["contractId"] = json!("journey/different-contract");

    let verdict = JourneyContractGate.evaluate(&JpdEvidence::JourneyContract(evidence));

    assert_eq!(
        verdict.findings,
        ["verification result contractId does not match the journey contract"],
        "an unbound result cannot truthfully support an observer-identity finding"
    );
}

#[test]
fn renamed_actor_and_observer_still_defeat_the_axis() {
    let mut evidence = journey_evidence("agent/renamed-driver");
    evidence.contract["actors"][0]["actorId"] = json!("agent/renamed-driver");
    evidence.contract["steps"][0]["actorId"] = json!("agent/renamed-driver");

    let verdict = JourneyContractGate.evaluate(&JpdEvidence::JourneyContract(evidence));

    assert!(!verdict.passed);
    assert_eq!(
        verdict.findings,
        [
            "promise promise/receipt-durable is bound to observer agent/renamed-driver, which is also the actor for step step/submit-order; the observer must be independent from the actor"
        ]
    );
}

#[test]
fn duplicate_steps_and_observers_emit_one_bounded_finding_per_promise() {
    let mut evidence = journey_evidence("agent/checkout-driver");
    evidence.contract["steps"] = Value::Array(
        (0..128)
            .map(|_| {
                json!({
                    "stepId": "step/submit-order",
                    "actorId": "agent/checkout-driver"
                })
            })
            .collect(),
    );
    evidence.verification_result["bindings"]["observers"] = Value::Array(
        (0..64)
            .map(|_| json!({ "observerId": "agent/checkout-driver" }))
            .collect(),
    );

    let verdict = JourneyContractGate.evaluate(&JpdEvidence::JourneyContract(evidence));

    assert!(!verdict.passed);
    assert_eq!(
        verdict.findings.len(),
        1,
        "duplicate schema-bounded records must not amplify diagnostics"
    );
}

#[test]
fn duplicate_promise_ids_are_refused_as_one_ambiguous_binding() {
    let mut evidence = journey_evidence("graphhelm-browser-user-journey");
    evidence.contract["steps"] = json!([
        { "stepId": "step/submit-order", "actorId": "agent/checkout-driver" },
        { "stepId": "step/persist-order", "actorId": "agent/order-writer" }
    ]);
    evidence.contract["promises"] = json!([
        {
            "promiseId": "promise/receipt-durable",
            "stepId": "step/submit-order",
            "requiredObserverCapability": "browser.semantic-journey"
        },
        {
            "promiseId": "promise/receipt-durable",
            "stepId": "step/persist-order",
            "requiredObserverCapability": "browser.semantic-journey"
        }
    ]);

    let verdict = JourneyContractGate.evaluate(&JpdEvidence::JourneyContract(evidence));

    assert_eq!(
        verdict.findings,
        [
            "journey contract contains ambiguous promiseId promise/receipt-durable; exactly one promise is required"
        ],
        "one obligation cannot be attributed to two promises sharing an identity"
    );
}

#[test]
fn cross_observation_between_two_actors_is_not_self_observation() {
    let mut evidence = journey_evidence("agent/actor-a");
    evidence.contract["actors"] = json!([
        { "actorId": "agent/actor-a", "name": "Actor A", "goal": "Perform A" },
        { "actorId": "agent/actor-b", "name": "Actor B", "goal": "Perform B" }
    ]);
    evidence.contract["steps"] = json!([
        { "stepId": "step/a", "actorId": "agent/actor-a" },
        { "stepId": "step/b", "actorId": "agent/actor-b" }
    ]);
    evidence.contract["promises"] = json!([
        {
            "promiseId": "promise/a",
            "stepId": "step/a",
            "requiredObserverCapability": "capability/a"
        },
        {
            "promiseId": "promise/b",
            "stepId": "step/b",
            "requiredObserverCapability": "capability/b"
        }
    ]);
    evidence.verification_result["bindings"]["observers"] = json!([
        { "observerId": "agent/actor-a" },
        { "observerId": "agent/actor-b" }
    ]);
    evidence.observation_obligations = vec![
        observation_obligation("promise/a", "capability/a", "agent/actor-b"),
        observation_obligation("promise/b", "capability/b", "agent/actor-a"),
    ];

    let verdict = JourneyContractGate.evaluate(&JpdEvidence::JourneyContract(evidence));

    assert!(
        verdict.passed,
        "cross-observation is independent: {:?}",
        verdict.findings
    );
}

#[test]
fn missing_observation_obligation_is_refused() {
    let mut evidence = journey_evidence("graphhelm-browser-user-journey");
    evidence.observation_obligations.clear();

    let verdict = JourneyContractGate.evaluate(&JpdEvidence::JourneyContract(evidence));

    assert!(!verdict.passed);
    assert_eq!(
        verdict.findings,
        ["promise promise/receipt-durable has no observation obligation binding"]
    );
}

#[test]
fn multiple_observation_obligations_for_one_promise_are_all_accepted_when_independent() {
    let mut evidence = journey_evidence("graphhelm-browser-user-journey");
    evidence.verification_result["bindings"]["observers"] = json!([
        { "observerId": "graphhelm-browser-user-journey" },
        { "observerId": "observer/second" }
    ]);
    evidence.observation_obligations = vec![
        observation_obligation(
            "promise/receipt-durable",
            "browser.semantic-journey",
            "graphhelm-browser-user-journey",
        ),
        observation_obligation(
            "promise/receipt-durable",
            "browser.semantic-journey",
            "observer/second",
        ),
    ];

    let verdict = JourneyContractGate.evaluate(&JpdEvidence::JourneyContract(evidence));

    assert!(verdict.passed, "findings were {:?}", verdict.findings);
    assert!(verdict.findings.is_empty());
}

#[test]
fn any_self_observing_obligation_refuses_a_multi_obligation_promise() {
    let mut evidence = journey_evidence("graphhelm-browser-user-journey");
    evidence.verification_result["bindings"]["observers"] = json!([
        { "observerId": "graphhelm-browser-user-journey" },
        { "observerId": "agent/checkout-driver" }
    ]);
    evidence.observation_obligations = vec![
        observation_obligation(
            "promise/receipt-durable",
            "browser.semantic-journey",
            "graphhelm-browser-user-journey",
        ),
        observation_obligation(
            "promise/receipt-durable",
            "browser.semantic-journey",
            "agent/checkout-driver",
        ),
    ];

    let evidence = JpdEvidence::JourneyContract(evidence);
    let diagnostics = JourneyContractGate.diagnostics(&evidence);
    let verdict = JourneyContractGate.evaluate(&evidence);

    assert!(!verdict.passed);
    assert_eq!(verdict.findings.len(), 1);
    assert_eq!(diagnostics[0].code, "GHJPD001_ACTOR_SELF_OBSERVATION");
    assert_eq!(
        diagnostics[0].path,
        "/observationObligations/1/resolution/capabilityBinding/observerId"
    );
}

#[test]
fn journey_contract_refusal_exposes_stable_code_path_severity_and_source() {
    let evidence = JpdEvidence::JourneyContract(journey_evidence("agent/checkout-driver"));

    let diagnostics = JourneyContractGate.diagnostics(&evidence);

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, "GHJPD001_ACTOR_SELF_OBSERVATION");
    assert_eq!(
        diagnostics[0].path,
        "/observationObligations/0/resolution/capabilityBinding/observerId"
    );
    assert_eq!(diagnostics[0].severity, JourneyDiagnosticSeverity::Error);
    assert_eq!(diagnostics[0].source_file, "observation-obligations.json");
}

#[test]
fn every_journey_contract_diagnostic_has_a_stable_code_path_and_source() {
    assert_journey_diagnostic(
        JpdEvidence::VerificationResult(json!({})),
        "GHJPD000_WRONG_EVIDENCE_KIND",
        "/kind",
        "this gate judges journey contracts only",
        "jpd-evidence.json",
    );
    assert_journey_diagnostic(
        JpdEvidence::JourneyContract(journey_evidence("agent/checkout-driver")),
        "GHJPD001_ACTOR_SELF_OBSERVATION",
        "/observationObligations/0/resolution/capabilityBinding/observerId",
        "promise promise/receipt-durable is bound to observer agent/checkout-driver, which is also the actor for step step/submit-order; the observer must be independent from the actor",
        "observation-obligations.json",
    );

    let mut evidence = journey_evidence("graphhelm-browser-user-journey");
    evidence.verification_result["contractId"] = json!("journey/different");
    assert_journey_diagnostic(
        JpdEvidence::JourneyContract(evidence),
        "GHJPD002_RESULT_CONTRACT_ID_MISMATCH",
        "/contractId",
        "verification result contractId does not match the journey contract",
        "journey-verification-result.json",
    );

    let mut evidence = journey_evidence("graphhelm-browser-user-journey");
    evidence.verification_result["contractDigest"] = json!("sha256:different");
    assert_journey_diagnostic(
        JpdEvidence::JourneyContract(evidence),
        "GHJPD003_RESULT_CONTRACT_DIGEST_MISMATCH",
        "/contractDigest",
        "verification result contractDigest does not match the supplied journey contract digest",
        "journey-verification-result.json",
    );

    let mut evidence = journey_evidence("graphhelm-browser-user-journey");
    evidence.contract["promises"][0]
        .as_object_mut()
        .unwrap()
        .remove("promiseId");
    assert_journey_diagnostic(
        JpdEvidence::JourneyContract(evidence),
        "GHJPD004_MALFORMED_PROMISE_BINDING",
        "/promises/0",
        "journey contract contains a malformed promise binding",
        "journey-contract.json",
    );

    let mut evidence = journey_evidence("graphhelm-browser-user-journey");
    let duplicate = evidence.contract["promises"][0].clone();
    evidence.contract["promises"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    assert_journey_diagnostic(
        JpdEvidence::JourneyContract(evidence),
        "GHJPD005_AMBIGUOUS_PROMISE_ID",
        "/promises/0/promiseId",
        "journey contract contains ambiguous promiseId promise/receipt-durable; exactly one promise is required",
        "journey-contract.json",
    );

    let mut evidence = journey_evidence("graphhelm-browser-user-journey");
    evidence.contract["promises"][0]["stepId"] = json!("step/missing");
    assert_journey_diagnostic(
        JpdEvidence::JourneyContract(evidence),
        "GHJPD006_STEP_MISSING",
        "/promises/0/stepId",
        "promise promise/receipt-durable references missing step step/missing",
        "journey-contract.json",
    );

    let mut evidence = journey_evidence("graphhelm-browser-user-journey");
    let duplicate = evidence.contract["steps"][0].clone();
    evidence.contract["steps"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    assert_journey_diagnostic(
        JpdEvidence::JourneyContract(evidence),
        "GHJPD007_STEP_AMBIGUOUS",
        "/promises/0/stepId",
        "promise promise/receipt-durable references ambiguous step step/submit-order; exactly one step actor is required",
        "journey-contract.json",
    );

    let mut evidence = journey_evidence("graphhelm-browser-user-journey");
    evidence.contract["steps"][0]
        .as_object_mut()
        .unwrap()
        .remove("actorId");
    assert_journey_diagnostic(
        JpdEvidence::JourneyContract(evidence),
        "GHJPD008_STEP_ACTOR_MISSING",
        "/promises/0/stepId",
        "promise promise/receipt-durable references step step/submit-order without an actor identity",
        "journey-contract.json",
    );

    let mut evidence = journey_evidence("graphhelm-browser-user-journey");
    evidence.contract["actors"] = json!([]);
    assert_journey_diagnostic(
        JpdEvidence::JourneyContract(evidence),
        "GHJPD009_ACTOR_ROSTER_MISSING",
        "/promises/0/stepId",
        "promise promise/receipt-durable references step step/submit-order actor agent/checkout-driver, which is absent from the journey contract actor roster",
        "journey-contract.json",
    );

    let mut evidence = journey_evidence("graphhelm-browser-user-journey");
    let duplicate = evidence.contract["actors"][0].clone();
    evidence.contract["actors"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    assert_journey_diagnostic(
        JpdEvidence::JourneyContract(evidence),
        "GHJPD010_ACTOR_ROSTER_AMBIGUOUS",
        "/promises/0/stepId",
        "promise promise/receipt-durable references step step/submit-order actor agent/checkout-driver, which is ambiguous in the journey contract actor roster",
        "journey-contract.json",
    );

    let mut evidence = journey_evidence("graphhelm-browser-user-journey");
    evidence.observation_obligations.clear();
    assert_journey_diagnostic(
        JpdEvidence::JourneyContract(evidence),
        "GHJPD011_OBSERVATION_OBLIGATION_MISSING",
        "/observationObligations",
        "promise promise/receipt-durable has no observation obligation binding",
        "observation-obligations.json",
    );

    let mut evidence = journey_evidence("graphhelm-browser-user-journey");
    evidence.observation_obligations[0]["contractId"] = json!("journey/different");
    assert_journey_diagnostic(
        JpdEvidence::JourneyContract(evidence),
        "GHJPD012_OBLIGATION_CONTRACT_ID_MISMATCH",
        "/observationObligations/0/contractId",
        "observation obligation for promise promise/receipt-durable has a contractId that does not match the journey contract",
        "observation-obligations.json",
    );

    let mut evidence = journey_evidence("graphhelm-browser-user-journey");
    evidence.observation_obligations[0]["contractDigest"] = json!("sha256:different");
    assert_journey_diagnostic(
        JpdEvidence::JourneyContract(evidence),
        "GHJPD013_OBLIGATION_CONTRACT_DIGEST_MISMATCH",
        "/observationObligations/0/contractDigest",
        "observation obligation for promise promise/receipt-durable has a contractDigest that does not match the supplied journey contract digest",
        "observation-obligations.json",
    );

    let mut evidence = journey_evidence("graphhelm-browser-user-journey");
    evidence.observation_obligations[0]["observerRequirements"]["capability"] =
        json!("browser.different");
    assert_journey_diagnostic(
        JpdEvidence::JourneyContract(evidence),
        "GHJPD014_OBSERVER_CAPABILITY_MISMATCH",
        "/observationObligations/0/observerRequirements/capability",
        "observation obligation for promise promise/receipt-durable does not bind required observer capability browser.semantic-journey",
        "observation-obligations.json",
    );

    let mut evidence = journey_evidence("graphhelm-browser-user-journey");
    evidence.observation_obligations[0]["resolution"]["capabilityBinding"]["capabilityId"] =
        json!("browser.different");
    assert_journey_diagnostic(
        JpdEvidence::JourneyContract(evidence),
        "GHJPD014_OBSERVER_CAPABILITY_MISMATCH",
        "/observationObligations/0/resolution/capabilityBinding/capabilityId",
        "observation obligation for promise promise/receipt-durable does not bind required observer capability browser.semantic-journey",
        "observation-obligations.json",
    );

    let mut evidence = journey_evidence("graphhelm-browser-user-journey");
    evidence.observation_obligations[0]["resolution"]["status"] = json!("capability_missing");
    assert_journey_diagnostic(
        JpdEvidence::JourneyContract(evidence),
        "GHJPD015_OBSERVER_BINDING_UNMATCHED",
        "/observationObligations/0/resolution/status",
        "observation obligation for promise promise/receipt-durable has no matched observer binding",
        "observation-obligations.json",
    );

    let mut evidence = journey_evidence("graphhelm-browser-user-journey");
    evidence.observation_obligations[0]["resolution"]["capabilityBinding"]
        .as_object_mut()
        .unwrap()
        .remove("observerId");
    assert_journey_diagnostic(
        JpdEvidence::JourneyContract(evidence),
        "GHJPD016_OBSERVER_ID_MISSING",
        "/observationObligations/0/resolution/capabilityBinding/observerId",
        "observation obligation for promise promise/receipt-durable has no matched observer identity",
        "observation-obligations.json",
    );

    let mut evidence = journey_evidence("graphhelm-browser-user-journey");
    evidence.verification_result["bindings"]["observers"] = json!([]);
    assert_journey_diagnostic(
        JpdEvidence::JourneyContract(evidence),
        "GHJPD017_OBSERVER_ROSTER_MISSING",
        "/observationObligations/0/resolution/capabilityBinding/observerId",
        "observation obligation for promise promise/receipt-durable binds observer graphhelm-browser-user-journey, which is absent from the verification result observer roster",
        "observation-obligations.json",
    );

    let mut evidence = journey_evidence("graphhelm-browser-user-journey");
    evidence.observation_obligations = vec![evidence.observation_obligations[0].clone(); 129];
    assert_journey_diagnostic(
        JpdEvidence::JourneyContract(evidence),
        "GHJPD018_OBSERVATION_OBLIGATION_LIMIT",
        "/observationObligations",
        "journey evidence contains more than 128 observation obligations",
        "observation-obligations.json",
    );
}

#[test]
fn observation_obligation_count_accepts_limit_and_refuses_limit_plus_one() {
    let mut at_limit = journey_evidence("graphhelm-browser-user-journey");
    at_limit.observation_obligations = vec![at_limit.observation_obligations[0].clone(); 128];
    let at_limit = JpdEvidence::JourneyContract(at_limit);
    assert!(JourneyContractGate.diagnostics(&at_limit).is_empty());

    let mut over_limit = journey_evidence("graphhelm-browser-user-journey");
    over_limit.observation_obligations = vec![over_limit.observation_obligations[0].clone(); 129];
    over_limit.observation_obligations[128]["contractId"] = json!("poison-tail");
    assert_journey_diagnostic(
        JpdEvidence::JourneyContract(over_limit),
        "GHJPD018_OBSERVATION_OBLIGATION_LIMIT",
        "/observationObligations",
        "journey evidence contains more than 128 observation obligations",
        "observation-obligations.json",
    );
}

#[test]
fn observation_obligation_must_bind_the_same_contract_digest() {
    let mut evidence = journey_evidence("graphhelm-browser-user-journey");
    evidence.observation_obligations[0]["contractDigest"] =
        json!("sha256:2222222222222222222222222222222222222222222222222222222222222222");

    let verdict = JourneyContractGate.evaluate(&JpdEvidence::JourneyContract(evidence));

    assert_eq!(
        verdict.findings,
        [
            "observation obligation for promise promise/receipt-durable has a contractDigest that does not match the supplied journey contract digest"
        ]
    );
}

#[test]
fn observation_obligation_must_bind_the_same_contract_id() {
    let mut evidence = journey_evidence("graphhelm-browser-user-journey");
    evidence.observation_obligations[0]["contractId"] = json!("journey/different-contract");

    let verdict = JourneyContractGate.evaluate(&JpdEvidence::JourneyContract(evidence));

    assert_eq!(
        verdict.findings,
        [
            "observation obligation for promise promise/receipt-durable has a contractId that does not match the journey contract"
        ]
    );
}

#[test]
fn observation_obligation_must_bind_the_promises_required_capability() {
    let mut evidence = journey_evidence("graphhelm-browser-user-journey");
    evidence.observation_obligations[0]["resolution"]["capabilityBinding"]["capabilityId"] =
        json!("browser.different-capability");

    let verdict = JourneyContractGate.evaluate(&JpdEvidence::JourneyContract(evidence));

    assert_eq!(
        verdict.findings,
        [
            "observation obligation for promise promise/receipt-durable does not bind required observer capability browser.semantic-journey"
        ]
    );
}

#[test]
fn a_step_actor_missing_from_the_actor_roster_is_refused_before_identity_comparison() {
    let mut evidence = journey_evidence("agent/checkout-driver");
    evidence.contract["steps"][0]["actorId"] = json!("agent/alias");

    let verdict = JourneyContractGate.evaluate(&JpdEvidence::JourneyContract(evidence));

    assert_eq!(
        verdict.findings,
        [
            "promise promise/receipt-durable references step step/submit-order actor agent/alias, which is absent from the journey contract actor roster"
        ],
        "a dangling actor identity cannot make self-observation look independent"
    );
}

#[test]
fn a_step_actor_duplicated_in_the_actor_roster_is_refused_as_ambiguous() {
    let mut evidence = journey_evidence("agent/checkout-driver");
    evidence.contract["actors"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "actorId": "agent/checkout-driver",
            "name": "Duplicate checkout driver",
            "goal": "Shadow the same actor identity"
        }));

    let verdict = JourneyContractGate.evaluate(&JpdEvidence::JourneyContract(evidence));

    assert_eq!(
        verdict.findings,
        [
            "promise promise/receipt-durable references step step/submit-order actor agent/checkout-driver, which is ambiguous in the journey contract actor roster"
        ]
    );
}

#[test]
fn capability_missing_is_reported_before_looking_for_a_matched_capability_binding() {
    let mut evidence = journey_evidence("graphhelm-browser-user-journey");
    evidence.observation_obligations[0]["resolution"] = json!({
        "status": "capability_missing",
        "authority": "advisory",
        "resultStatus": "unresolved",
        "refusal": {
            "code": "OBSERVER_MISSING",
            "missingCapability": "browser.semantic-journey",
            "reason": "No activated observer can inspect the promised fact.",
            "weakerProxiesRejected": ["process_exit"]
        }
    });

    let verdict = JourneyContractGate.evaluate(&JpdEvidence::JourneyContract(evidence));

    assert_eq!(
        verdict.findings,
        [
            "observation obligation for promise promise/receipt-durable has no matched observer binding"
        ],
        "a valid capability_missing resolution has no capabilityBinding to inspect"
    );
}

#[test]
fn observation_obligation_observer_must_exist_in_the_result_roster() {
    let mut evidence = journey_evidence("graphhelm-browser-user-journey");
    evidence.observation_obligations[0]["resolution"]["capabilityBinding"]["observerId"] =
        json!("observer/not-in-result");

    let verdict = JourneyContractGate.evaluate(&JpdEvidence::JourneyContract(evidence));

    assert_eq!(
        verdict.findings,
        [
            "observation obligation for promise promise/receipt-durable binds observer observer/not-in-result, which is absent from the verification result observer roster"
        ]
    );
}

#[test]
fn a_duplicate_step_id_with_conflicting_actors_is_refused_as_ambiguous() {
    let mut evidence = journey_evidence("agent/checkout-driver");
    evidence.contract["steps"] = json!([
        {
            "stepId": "step/submit-order",
            "actorId": "agent/different-driver"
        },
        {
            "stepId": "step/submit-order",
            "actorId": "agent/checkout-driver"
        }
    ]);

    let verdict = JourneyContractGate.evaluate(&JpdEvidence::JourneyContract(evidence));

    assert_eq!(
        verdict.findings,
        [
            "promise promise/receipt-durable references ambiguous step step/submit-order; exactly one step actor is required"
        ],
        "an ambiguous step binding cannot support an identity claim"
    );
}

#[test]
fn a_promise_referencing_no_step_is_refused_as_unbound() {
    let mut evidence = journey_evidence("graphhelm-browser-user-journey");
    evidence.contract["promises"][0]["stepId"] = json!("step/missing");

    let verdict = JourneyContractGate.evaluate(&JpdEvidence::JourneyContract(evidence));

    assert_eq!(
        verdict.findings,
        ["promise promise/receipt-durable references missing step step/missing"],
        "a missing step cannot silently erase the actor side of the identity comparison"
    );
}

#[test]
fn actor_as_observer_specimen_genuinely_defeats_its_axis() {
    let suite = journey_contract_suite();
    assert!(!suite.is_empty(), "the journey-contract suite is empty");
    for specimen in &suite {
        assert!(
            is_defeated_on_its_axis(specimen),
            "specimen {} names an axis it does not defeat",
            specimen.id
        );
    }
}

#[test]
fn actor_as_observer_axis_does_not_cross_evidence_kinds() {
    let specimen = JpdSpecimen {
        id: "verification/not-a-contract".to_owned(),
        axis: JpdFailureAxis::ActorUsedAsOwnObserver,
        evidence: JpdEvidence::VerificationResult(journey_contract("agent/checkout-driver")),
    };

    assert!(!is_defeated_on_its_axis(&specimen));
}

#[test]
fn actor_as_observer_axis_requires_a_bound_contract_and_result() {
    let mut evidence = journey_evidence("agent/checkout-driver");
    evidence.verification_result["contractId"] = json!("journey/different-contract");
    let specimen = JpdSpecimen {
        id: "contract/unbound-actor-as-observer".to_owned(),
        axis: JpdFailureAxis::ActorUsedAsOwnObserver,
        evidence: JpdEvidence::JourneyContract(evidence),
    };

    assert!(!is_defeated_on_its_axis(&specimen));
}

#[test]
fn journey_contract_gate_rejects_every_specimen_in_its_suite() {
    let certification = certify(&JourneyContractGate, &journey_contract_suite())
        .expect("the gate rejects every journey-contract specimen");

    assert_eq!(certification.gate_id, "gate/jpd-journey-contract");
    assert_eq!(certification.specimens, 1);
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
        "bindings": {
            "council": { "status": "not_applicable", "reason": "direct_tier" }
        }
    });

    let mut recommended = base.clone();
    recommended["bindings"]["council"] = executed_council_with_decision(false);
    let mut blocked = base.clone();
    blocked["bindings"]["council"] = executed_council_with_decision(true);

    let bare = VerificationResultGate.evaluate(&JpdEvidence::VerificationResult(base));
    let with_recommendation =
        VerificationResultGate.evaluate(&JpdEvidence::VerificationResult(recommended));
    let with_block = VerificationResultGate.evaluate(&JpdEvidence::VerificationResult(blocked));

    assert_eq!(
        with_recommendation.passed, bare.passed,
        "a council recommendation must not rescue anything"
    );
    assert_eq!(
        with_block.passed, bare.passed,
        "a blocked council result must not sink anything either"
    );
    assert_eq!(
        with_recommendation.findings, with_block.findings,
        "identical evidence under opposite council decisions must produce identical findings"
    );
}

#[test]
fn a_council_verdict_does_not_move_the_journey_contract_gate() {
    let mut base = journey_evidence("graphhelm-browser-user-journey");
    base.verification_result["bindings"]["council"] =
        json!({ "status": "not_applicable", "reason": "direct_tier" });
    let bare = JourneyContractGate.evaluate(&JpdEvidence::JourneyContract(base.clone()));
    let mut recommended = base.clone();
    recommended.verification_result["bindings"]["council"] = executed_council_with_decision(false);
    let mut blocked = base;
    blocked.verification_result["bindings"]["council"] = executed_council_with_decision(true);

    let with_recommendation =
        JourneyContractGate.evaluate(&JpdEvidence::JourneyContract(recommended));
    let with_block = JourneyContractGate.evaluate(&JpdEvidence::JourneyContract(blocked));

    assert!(bare.passed, "independent base evidence must pass");
    assert_eq!(with_recommendation.passed, bare.passed);
    assert_eq!(with_block.passed, bare.passed);
    assert_eq!(with_recommendation.findings, bare.findings);
    assert_eq!(
        with_block.findings, bare.findings,
        "opposite council decisions over the same bound journey evidence must be advisory"
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
        "HARNESS-BROKE: this fixture is only meaningful while it CLAIMS success; if the vocabulary moved again, this arm is comparing against something else entirely"
    );

    let verdict = VerificationResultGate.evaluate(&JpdEvidence::VerificationResult(document));

    assert!(
        !verdict.passed,
        "the repository's own missing-gate-claimed-success fixture must be refused: it exists to be refused, and a gate that passes it certifies nothing"
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

// ---------------------------------------------------------------------------------------------
// #211 B5: the WAIVED and REFUSED outcomes of the acceptance journey, as cells of their own.
//
// Waived is a pathogen axis: a waiver whose `coverage.verificationId` names a DIFFERENT
// verification than the result it is attached to. Every other property of a waiver the schema
// already pins (`blockerKind` is `const` per site; the receipt, the digests, the schema id are all
// typed), so the transplanted waiver is the one wrong-but-LEGAL shape left -- the exact
// cross-record comparison this gate exists for.
//
// Refused is NOT an axis, and saying so is part of the delivery: gate `rejected` under any success
// claim without a covering waiver is schema-INVALID (`allOf[3]` restricts the status to
// `accepted_with_waiver | unresolved`; `allOf[8]` requires the gate accepted or waived under
// `accepted_with_waiver`). An axis keyed to it would refuse only documents the schema already
// refuses -- the trap `CapabilityMissingUnderClaimedSuccess`'s doc records. The refused outcome is
// a document that ADMITS the rejection (`unresolved`), and the gate's obligation is to let it
// through: nothing there is falsely certified.

/// Every specimen is one field away from the repository's own positive waiver fixture, so its
/// schema-validity is inherited from a document the repository already accepts.
fn waived_fixture_with(mutate: impl FnOnce(&mut serde_json::Value)) -> serde_json::Value {
    let mut document = jpd_fixture("positive/journey-verification-accepted-with-waiver.json");
    assert_eq!(
        document.get("proposedResultStatus").and_then(Value::as_str),
        Some("accepted_with_waiver"),
        "HARNESS-BROKE: the waiver fixture no longer claims accepted_with_waiver; these arms would be comparing against something else"
    );
    assert_eq!(
        document
            .pointer("/gate/waiver/coverage/verificationId")
            .and_then(Value::as_str),
        document.get("verificationId").and_then(Value::as_str),
        "HARNESS-BROKE: the fixture's own gate waiver must cover THIS verification, or the mutation below is not the only difference"
    );
    mutate(&mut document);
    document
}

/// WAIVED, negative face: a gate waiver transplanted from another verification must be refused.
///
/// One field differs from the accepted fixture. Before this axis existed the gate PASSED this
/// document: the waiver is well-formed, its blockerKind is `gate`, and nothing compared its
/// identity to the result it rides on.
#[test]
fn a_gate_waiver_issued_for_another_verification_is_refused() {
    let document = waived_fixture_with(|d| {
        d["gate"]["waiver"]["coverage"]["verificationId"] = json!("verification/issue-210/other");
    });
    let verdict = VerificationResultGate.evaluate(&JpdEvidence::VerificationResult(document));
    assert!(
        !verdict.passed,
        "a waiver covering a different verification must not rescue this one; findings were {:?}",
        verdict.findings
    );
    assert!(
        verdict
            .findings
            .iter()
            .any(|f| f.contains("another verification")),
        "the refusal must name the transplant, not some neighbouring axis: {:?}",
        verdict.findings
    );
}

/// The walk covers EVERY waiver site, not only the gate's. An obligation waiver transplanted from
/// elsewhere is the same pathogen at a different address; a check that only reads `/gate/waiver`
/// would pass this document.
#[test]
fn an_obligation_waiver_issued_for_another_verification_is_refused() {
    let document = waived_fixture_with(|d| {
        d["obligations"][0]["waiver"]["coverage"]["verificationId"] =
            json!("verification/issue-210/other");
    });
    let verdict = VerificationResultGate.evaluate(&JpdEvidence::VerificationResult(document));
    assert!(
        !verdict.passed,
        "the transplant hides at the obligation site and must still be refused; findings were {:?}",
        verdict.findings
    );
}

/// CONTROL for the pair above: the same fixture with the field put BACK is accepted. Without this
/// the two refusals are satisfied by a gate that refuses every waiver, which is the cheapest false
/// fix and would kill the waived outcome the issue asks for.
#[test]
fn the_same_waiver_covering_this_verification_is_accepted() {
    let document = waived_fixture_with(|d| {
        let own = d["verificationId"].clone();
        d["gate"]["waiver"]["coverage"]["verificationId"] = own;
    });
    let verdict = VerificationResultGate.evaluate(&JpdEvidence::VerificationResult(document));
    assert!(
        verdict.passed,
        "a waiver that covers THIS verification is the legitimate waived outcome; findings were {:?}",
        verdict.findings
    );
}

/// REFUSED, as an outcome: a result whose gate was rejected and which ADMITS it (`unresolved`)
/// carries no false certification, and the gate must let it through. This is the fifth outcome
/// B5 names, and it is deliberately not an axis -- see the block comment above for the schema
/// coordinates that make "refused but claims success" unrepresentable.
#[test]
fn a_rejected_gate_admitted_as_unresolved_is_the_refused_outcome_and_passes() {
    let document = waived_fixture_with(|d| {
        d["proposedResultStatus"] = json!("unresolved");
    });
    assert_eq!(
        document.pointer("/gate/result").and_then(Value::as_str),
        Some("rejected"),
        "HARNESS-BROKE: the fixture's gate must be rejected for this to be the refused outcome"
    );
    let verdict = VerificationResultGate.evaluate(&JpdEvidence::VerificationResult(document));
    assert!(
        verdict.passed,
        "an admitted refusal has nothing to falsely certify; findings were {:?}",
        verdict.findings
    );
}

/// The axis knows its own specimen, like every other axis in the suite.
#[test]
fn the_waived_axis_can_say_whether_its_own_specimen_is_genuinely_defeated() {
    let specimen = jpd_suite()
        .into_iter()
        .find(|s| matches!(s.axis, JpdFailureAxis::WaiverIssuedForAnotherVerification))
        .expect("the suite carries the waived specimen");
    assert!(is_defeated_on_its_axis(&specimen));
}
