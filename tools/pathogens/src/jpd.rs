//! JPD typed evidence, as a second instantiation of the certification harness.
//!
//! Evidence is carried as validated JSON rather than as re-declared Rust structs. The schemas in
//! `extensions/builtin/graphhelm-jpd/schemas/` are the authority, and a second Rust declaration of
//! the same closed shapes would be a second producer of one vocabulary — which drifts in silence,
//! because a rename preserves a count and everything still compiles.
//!
//! **The cost of that choice, stated rather than discovered:** a malformed document reaches the
//! gate. Validating against the schema before certifying is `graphhelm_schema`'s job and is not
//! done here; whoever wires a public surface must validate first, or the gate judges shapes nobody
//! checked.

use serde::Serialize;
use serde_json::Value;

use crate::{EvidenceGate, FailureAxis, Specimen, Verdict};

/// The validated contract, its observation obligations, and verification result, bound by the
/// contract's trusted digest.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JourneyContractEvidence {
    pub contract: Value,
    pub contract_digest: String,
    pub observation_obligations: Vec<Value>,
    pub verification_result: Value,
}

/// The five artifact kinds #211 names for runtime-side validation.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JpdEvidence {
    JourneyContract(JourneyContractEvidence),
    ObservationObligation(Value),
    RetryLineage(Value),
    CouncilResult(Value),
    VerificationResult(Value),
}

impl JpdEvidence {
    /// The document inside, whatever the kind.
    #[must_use]
    pub const fn document(&self) -> &Value {
        match self {
            Self::JourneyContract(evidence) => &evidence.contract,
            Self::ObservationObligation(v)
            | Self::RetryLineage(v)
            | Self::CouncilResult(v)
            | Self::VerificationResult(v) => v,
        }
    }
}

/// How a JPD certification can be fooled.
///
/// A CLOSED set, and deliberately NOT merged into `UselessnessMode`: those are failures of a
/// rendered surface, these are failures of evidence.
///
/// **Every name and every value below is read from the DECLARED schema**
/// (`extensions/builtin/graphhelm-jpd/schemas/journey-verification-result.schema.json`) and checked
/// against the repository's real fixtures. The first version of this module invented `status`,
/// `observers`, `attempts`, `producer` and `validator` — none of which exist — so the gate passed
/// every real document including the repository's own negative fixture. Inventing a field name is
/// not a typo here; it is a gate that certifies nothing while reporting that it certified.
///
/// **A third axis was removed rather than translated, and this is the record of why.**
/// `SelfValidation` — a producer certifying its own work — is **not expressible against this
/// schema.** Enumerating all 17 identity-bearing properties finds identities for the VALIDATOR
/// (`evaluatorId`, `observerId`) and for the WORK (`verificationId`, `journeyRunId`, `contractId`,
/// and the rest), and **no producer identity at the root**: the comparison has no left side. Any
/// implementation would have had to invent one — the exact defect described above, wearing the
/// clothes of thoroughness. **An axis with no basis in the evidence is worse than a missing axis,
/// because it reports that something was checked.** (Enumerated independently by L.)
///
/// **Condition of death for this removal — the axis is meant to COME BACK.** When the schema gains
/// a producer identity at the root comparable against `observers[].observerId` or
/// `evaluatorReceipt.evaluatorId`, `SelfValidation` becomes expressible and should be restored.
/// Recorded here so the next author reads a decision instead of re-deriving it, and so the removal
/// cannot quietly harden into "we decided self-validation does not matter".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JpdFailureAxis {
    /// A promise appoints the actor performing its step as that step's observer.
    ActorUsedAsOwnObserver,
    /// A capability that was required never ran, and the result claims success anyway.
    ///
    /// Discriminated on `gate.status`, which the schema fixes to `evaluated` in the branches where
    /// the gate ran and to `capability_missing` in the branch where it did not. It can discriminate
    /// precisely BECAUSE the schema branches on it.
    ///
    /// **Not** on `authority.validation.status`. That path is declared exactly once, as
    /// `"const": "capability_missing"` (in `$defs/candidateAuthority`, the sole `$ref` target of
    /// `authority`), so **no schema-valid document can carry any other value there** — a gate keyed
    /// to it would refuse every input, both positive fixtures included, and look strict while being
    /// broken. That the three shipped fixtures all read `capability_missing` is the weaker form of
    /// this fact: it holds only until someone adds a fixture. The constant is the reason that
    /// cannot age, and it is the one worth writing down. (Sharpened by L.)
    CapabilityMissingUnderClaimedSuccess,
    /// A run classified `flaky_pass` presented as `proven`.
    ///
    /// `recovered_success` under `accepted_with_waiver` is a legitimate outcome and is not this.
    FlakyClaimedAsProven,
    /// A waiver whose `coverage.verificationId` names a DIFFERENT verification than the result
    /// it is attached to: a waiver transplanted from elsewhere.
    ///
    /// This is the one wrong-but-LEGAL waiver left once the schema has done its work. Every
    /// waiver site binds `blockerKind` by `const`, the receipt and digests are typed, and the
    /// waiver schema id is a constant -- so a kind mismatch or a malformed receipt never reaches
    /// this gate. Identity across records is what JSON Schema cannot express, and the repository's
    /// own positive fixture pins the rule: its waivers all cover the verification they ride on.
    ///
    /// Walked at EVERY site (`retry`, `bindings.observers[]`, `obligations[]`, `disagreements[]`,
    /// `gate`), because a transplant at the obligation site is the same pathogen at a different
    /// address, and a check that reads only `/gate/waiver` passes it.
    ///
    /// Deliberately there is no `Refused` axis beside this one. Gate `rejected` under any success
    /// claim without a covering waiver is schema-invalid (`allOf[3]`, `allOf[8]` of the
    /// verification-result schema), so an axis keyed to it would refuse only documents the schema
    /// already refuses -- the trap `CapabilityMissingUnderClaimedSuccess` documents. The refused
    /// outcome is a result that ADMITS the rejection (`unresolved`), and the gate lets it through.
    WaiverIssuedForAnotherVerification,
}

/// Whether a document claims success at all. `proven` and `accepted_with_waiver` both do;
/// `unresolved` does not, and a document admitting failure has no false certification to earn.
fn claims_success(document: &Value) -> bool {
    matches!(
        document.get("proposedResultStatus").and_then(Value::as_str),
        Some("proven" | "accepted_with_waiver")
    )
}

impl FailureAxis<JpdEvidence> for JpdFailureAxis {
    fn is_defeated_by(&self, evidence: &JpdEvidence) -> bool {
        let document = evidence.document();
        match self {
            Self::ActorUsedAsOwnObserver => {
                let JpdEvidence::JourneyContract(evidence) = evidence else {
                    return false;
                };
                resolved_observer_bindings(evidence)
                    .is_ok_and(|bindings| !actor_observer_collisions(&bindings).is_empty())
            }
            Self::CapabilityMissingUnderClaimedSuccess => {
                claims_success(document)
                    && document.pointer("/gate/status").and_then(Value::as_str)
                        == Some("capability_missing")
            }
            Self::FlakyClaimedAsProven => {
                claims_success(document)
                    && document.get("proposedResultStatus").and_then(Value::as_str)
                        == Some("proven")
                    && document
                        .pointer("/retry/outcomeClassification")
                        .and_then(Value::as_str)
                        == Some("flaky_pass")
            }
            Self::WaiverIssuedForAnotherVerification => {
                claims_success(document) && !foreign_waiver_sites(document).is_empty()
            }
        }
    }
}

/// Every waiver whose coverage names a verification other than the document's own, by site.
///
/// Absent `verificationId` on the document means no identity to compare against, and the schema
/// requires the field -- so a document without it is not a legal input and this reports nothing
/// rather than inventing a mismatch.
fn foreign_waiver_sites(document: &Value) -> Vec<String> {
    let Some(own) = document.get("verificationId").and_then(Value::as_str) else {
        return Vec::new();
    };
    let mut sites: Vec<(String, &Value)> = Vec::new();
    if let Some(w) = document.pointer("/retry/waiver") {
        sites.push(("retry".to_owned(), w));
    }
    if let Some(w) = document.pointer("/gate/waiver") {
        sites.push(("gate".to_owned(), w));
    }
    for (name, path) in [
        ("observer", "/bindings/observers"),
        ("obligation", "/obligations"),
        ("disagreement", "/disagreements"),
    ] {
        if let Some(items) = document.pointer(path).and_then(Value::as_array) {
            for (index, item) in items.iter().enumerate() {
                if let Some(w) = item.get("waiver") {
                    sites.push((format!("{name}[{index}]"), w));
                }
            }
        }
    }
    sites
        .into_iter()
        .filter(|(_, waiver)| {
            waiver
                .pointer("/coverage/verificationId")
                .and_then(Value::as_str)
                .is_some_and(|covered| covered != own)
        })
        .map(|(site, _)| site)
        .collect()
}

/// A JPD specimen: typed evidence plus the axis it defeats.
pub type JpdSpecimen = Specimen<JpdEvidence, JpdFailureAxis>;

/// Validates the independence obligation carried by a journey contract.
///
/// Inputs are expected to have passed the checked-in JSON Schema first. This gate performs only
/// the cross-record comparison that JSON Schema cannot express: the promise's observer identity
/// must differ from the actor identity on the referenced step.
pub struct JourneyContractGate;

/// Stable severity for a journey-contract refusal diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JourneyDiagnosticSeverity {
    Error,
}

/// Stable diagnostic emitted by the journey-contract gate.
///
/// `source_file` is a logical input filename. It deliberately never contains an operator's local
/// filesystem path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JourneyContractDiagnostic {
    pub code: &'static str,
    pub severity: JourneyDiagnosticSeverity,
    pub path: String,
    pub message: String,
    pub source_file: &'static str,
}

fn journey_diagnostic(
    code: &'static str,
    path: impl Into<String>,
    message: impl Into<String>,
    source_file: &'static str,
) -> JourneyContractDiagnostic {
    JourneyContractDiagnostic {
        code,
        severity: JourneyDiagnosticSeverity::Error,
        path: path.into(),
        message: message.into(),
        source_file,
    }
}

impl JourneyContractGate {
    /// Evaluate with stable diagnostics. The generic certification harness below projects only
    /// their messages into its legacy `Vec<String>` verdict.
    #[must_use]
    pub fn diagnostics(&self, evidence: &JpdEvidence) -> Vec<JourneyContractDiagnostic> {
        let JpdEvidence::JourneyContract(evidence) = evidence else {
            return vec![journey_diagnostic(
                "GHJPD000_WRONG_EVIDENCE_KIND",
                "/kind",
                "this gate judges journey contracts only",
                "jpd-evidence.json",
            )];
        };

        let bindings = match resolved_observer_bindings(evidence) {
            Ok(bindings) => bindings,
            Err(diagnostics) => return diagnostics,
        };

        actor_observer_collisions(&bindings)
            .into_iter()
            .map(|binding| {
                let PromiseObserverBinding {
                    promise_id,
                    step_id,
                    observer_id,
                    obligation_index,
                    ..
                } = binding;
                journey_diagnostic(
                    "GHJPD001_ACTOR_SELF_OBSERVATION",
                    format!(
                        "/observationObligations/{obligation_index}/resolution/capabilityBinding/observerId"
                    ),
                    format!(
                        "promise {promise_id} is bound to observer {observer_id}, which is also the actor for step {step_id}; the observer must be independent from the actor"
                    ),
                    "observation-obligations.json",
                )
            })
            .collect()
    }
}

impl EvidenceGate<JpdEvidence> for JourneyContractGate {
    fn id(&self) -> &str {
        "gate/jpd-journey-contract"
    }

    fn evaluate(&self, evidence: &JpdEvidence) -> Verdict {
        let findings = self
            .diagnostics(evidence)
            .into_iter()
            .map(|diagnostic| diagnostic.message)
            .collect::<Vec<_>>();

        Verdict {
            passed: findings.is_empty(),
            findings,
        }
    }
}

fn contract_result_binding_findings(
    evidence: &JourneyContractEvidence,
) -> Vec<JourneyContractDiagnostic> {
    let contract_id = evidence.contract.get("contractId").and_then(Value::as_str);
    let result_contract_id = evidence
        .verification_result
        .get("contractId")
        .and_then(Value::as_str);
    let result_digest = evidence
        .verification_result
        .get("contractDigest")
        .and_then(Value::as_str);
    let mut findings = Vec::new();

    if contract_id.is_none() || contract_id != result_contract_id {
        findings.push(journey_diagnostic(
            "GHJPD002_RESULT_CONTRACT_ID_MISMATCH",
            "/contractId",
            "verification result contractId does not match the journey contract",
            "journey-verification-result.json",
        ));
    }
    if result_digest != Some(evidence.contract_digest.as_str()) {
        findings.push(journey_diagnostic(
            "GHJPD003_RESULT_CONTRACT_DIGEST_MISMATCH",
            "/contractDigest",
            "verification result contractDigest does not match the supplied journey contract digest",
            "journey-verification-result.json",
        ));
    }

    findings
}

#[derive(Clone, Copy, Debug)]
struct PromiseObserverBinding<'a> {
    promise_id: &'a str,
    step_id: &'a str,
    actor_id: &'a str,
    observer_id: &'a str,
    obligation_index: usize,
}

// The verification-result schema caps its obligation results at 128. Refuse the corresponding
// input collection before any scan or copy so schema-valid documents cannot be amplified by an
// unbounded envelope assembled by a caller.
const MAX_OBSERVATION_OBLIGATIONS: usize = 128;

fn resolved_observer_bindings(
    evidence: &JourneyContractEvidence,
) -> Result<Vec<PromiseObserverBinding<'_>>, Vec<JourneyContractDiagnostic>> {
    let mut findings = contract_result_binding_findings(evidence);
    if !findings.is_empty() {
        return Err(findings);
    }
    if evidence.observation_obligations.len() > MAX_OBSERVATION_OBLIGATIONS {
        return Err(vec![journey_diagnostic(
            "GHJPD018_OBSERVATION_OBLIGATION_LIMIT",
            "/observationObligations",
            format!(
                "journey evidence contains more than {MAX_OBSERVATION_OBLIGATIONS} observation obligations"
            ),
            "observation-obligations.json",
        )]);
    }

    let contract_id = evidence
        .contract
        .get("contractId")
        .and_then(Value::as_str)
        .expect("contract/result binding already established a contractId");
    let actors = evidence.contract.get("actors").and_then(Value::as_array);
    let steps = evidence.contract.get("steps").and_then(Value::as_array);
    let promises = evidence.contract.get("promises").and_then(Value::as_array);
    let observers = evidence
        .verification_result
        .pointer("/bindings/observers")
        .and_then(Value::as_array);
    let mut bindings = Vec::new();

    for (promise_index, promise) in promises.into_iter().flatten().enumerate() {
        let Some((promise_id, step_id, required_capability)) = promise
            .get("promiseId")
            .and_then(Value::as_str)
            .zip(promise.get("stepId").and_then(Value::as_str))
            .zip(
                promise
                    .get("requiredObserverCapability")
                    .and_then(Value::as_str),
            )
            .map(|((promise_id, step_id), required_capability)| {
                (promise_id, step_id, required_capability)
            })
        else {
            findings.push(journey_diagnostic(
                "GHJPD004_MALFORMED_PROMISE_BINDING",
                format!("/promises/{promise_index}"),
                "journey contract contains a malformed promise binding",
                "journey-contract.json",
            ));
            continue;
        };

        let promise_records = promises.expect("the loop only runs when promises is an array");
        let duplicate_count = promise_records
            .iter()
            .filter(|candidate| {
                candidate.get("promiseId").and_then(Value::as_str) == Some(promise_id)
            })
            .count();
        if duplicate_count > 1 {
            let already_reported = promise_records[..promise_index].iter().any(|candidate| {
                candidate.get("promiseId").and_then(Value::as_str) == Some(promise_id)
            });
            if !already_reported {
                findings.push(journey_diagnostic(
                    "GHJPD005_AMBIGUOUS_PROMISE_ID",
                    format!("/promises/{promise_index}/promiseId"),
                    format!(
                        "journey contract contains ambiguous promiseId {promise_id}; exactly one promise is required"
                    ),
                    "journey-contract.json",
                ));
            }
            continue;
        }

        let mut matching_steps = steps
            .into_iter()
            .flatten()
            .filter(|step| step.get("stepId").and_then(Value::as_str) == Some(step_id));
        let Some(step) = matching_steps.next() else {
            findings.push(journey_diagnostic(
                "GHJPD006_STEP_MISSING",
                format!("/promises/{promise_index}/stepId"),
                format!("promise {promise_id} references missing step {step_id}"),
                "journey-contract.json",
            ));
            continue;
        };
        if matching_steps.next().is_some() {
            findings.push(journey_diagnostic(
                "GHJPD007_STEP_AMBIGUOUS",
                format!("/promises/{promise_index}/stepId"),
                format!(
                    "promise {promise_id} references ambiguous step {step_id}; exactly one step actor is required"
                ),
                "journey-contract.json",
            ));
            continue;
        }
        let Some(actor_id) = step.get("actorId").and_then(Value::as_str) else {
            findings.push(journey_diagnostic(
                "GHJPD008_STEP_ACTOR_MISSING",
                format!("/promises/{promise_index}/stepId"),
                format!("promise {promise_id} references step {step_id} without an actor identity"),
                "journey-contract.json",
            ));
            continue;
        };
        let mut matching_actors = actors
            .into_iter()
            .flatten()
            .filter(|actor| actor.get("actorId").and_then(Value::as_str) == Some(actor_id));
        if matching_actors.next().is_none() {
            findings.push(journey_diagnostic(
                "GHJPD009_ACTOR_ROSTER_MISSING",
                format!("/promises/{promise_index}/stepId"),
                format!(
                    "promise {promise_id} references step {step_id} actor {actor_id}, which is absent from the journey contract actor roster"
                ),
                "journey-contract.json",
            ));
            continue;
        }
        if matching_actors.next().is_some() {
            findings.push(journey_diagnostic(
                "GHJPD010_ACTOR_ROSTER_AMBIGUOUS",
                format!("/promises/{promise_index}/stepId"),
                format!(
                    "promise {promise_id} references step {step_id} actor {actor_id}, which is ambiguous in the journey contract actor roster"
                ),
                "journey-contract.json",
            ));
            continue;
        }

        let matching = evidence
            .observation_obligations
            .iter()
            .enumerate()
            .filter(|(_, obligation)| {
                obligation.get("promiseId").and_then(Value::as_str) == Some(promise_id)
            })
            .collect::<Vec<_>>();
        if matching.is_empty() {
            findings.push(journey_diagnostic(
                "GHJPD011_OBSERVATION_OBLIGATION_MISSING",
                "/observationObligations",
                format!("promise {promise_id} has no observation obligation binding"),
                "observation-obligations.json",
            ));
            continue;
        }

        let promise_finding_start = findings.len();
        for (obligation_index, obligation) in matching {
            let obligation_path = format!("/observationObligations/{obligation_index}");
            let obligation_contract_id = obligation.get("contractId").and_then(Value::as_str);
            if obligation_contract_id != Some(contract_id) {
                findings.push(journey_diagnostic(
                    "GHJPD012_OBLIGATION_CONTRACT_ID_MISMATCH",
                    format!("{obligation_path}/contractId"),
                    format!(
                        "observation obligation for promise {promise_id} has a contractId that does not match the journey contract"
                    ),
                    "observation-obligations.json",
                ));
                continue;
            }
            let obligation_digest = obligation.get("contractDigest").and_then(Value::as_str);
            if obligation_digest != Some(evidence.contract_digest.as_str()) {
                findings.push(journey_diagnostic(
                    "GHJPD013_OBLIGATION_CONTRACT_DIGEST_MISMATCH",
                    format!("{obligation_path}/contractDigest"),
                    format!(
                        "observation obligation for promise {promise_id} has a contractDigest that does not match the supplied journey contract digest"
                    ),
                    "observation-obligations.json",
                ));
                continue;
            }
            let required_by_obligation = obligation
                .pointer("/observerRequirements/capability")
                .and_then(Value::as_str);
            if required_by_obligation != Some(required_capability) {
                findings.push(journey_diagnostic(
                    "GHJPD014_OBSERVER_CAPABILITY_MISMATCH",
                    format!("{obligation_path}/observerRequirements/capability"),
                    format!(
                        "observation obligation for promise {promise_id} does not bind required observer capability {required_capability}"
                    ),
                    "observation-obligations.json",
                ));
                continue;
            }
            if obligation
                .pointer("/resolution/status")
                .and_then(Value::as_str)
                != Some("matched")
            {
                findings.push(journey_diagnostic(
                    "GHJPD015_OBSERVER_BINDING_UNMATCHED",
                    format!("{obligation_path}/resolution/status"),
                    format!(
                        "observation obligation for promise {promise_id} has no matched observer binding"
                    ),
                    "observation-obligations.json",
                ));
                continue;
            }
            let bound_capability = obligation
                .pointer("/resolution/capabilityBinding/capabilityId")
                .and_then(Value::as_str);
            if bound_capability != Some(required_capability) {
                findings.push(journey_diagnostic(
                    "GHJPD014_OBSERVER_CAPABILITY_MISMATCH",
                    format!("{obligation_path}/resolution/capabilityBinding/capabilityId"),
                    format!(
                        "observation obligation for promise {promise_id} does not bind required observer capability {required_capability}"
                    ),
                    "observation-obligations.json",
                ));
                continue;
            }
            let Some(observer_id) = obligation
                .pointer("/resolution/capabilityBinding/observerId")
                .and_then(Value::as_str)
            else {
                findings.push(journey_diagnostic(
                    "GHJPD016_OBSERVER_ID_MISSING",
                    format!("{obligation_path}/resolution/capabilityBinding/observerId"),
                    format!(
                        "observation obligation for promise {promise_id} has no matched observer identity"
                    ),
                    "observation-obligations.json",
                ));
                continue;
            };
            let observer_is_rostered = observers.into_iter().flatten().any(|observer| {
                observer.get("observerId").and_then(Value::as_str) == Some(observer_id)
            });
            if !observer_is_rostered {
                findings.push(journey_diagnostic(
                    "GHJPD017_OBSERVER_ROSTER_MISSING",
                    format!("{obligation_path}/resolution/capabilityBinding/observerId"),
                    format!(
                        "observation obligation for promise {promise_id} binds observer {observer_id}, which is absent from the verification result observer roster"
                    ),
                    "observation-obligations.json",
                ));
                continue;
            }

            bindings.push(PromiseObserverBinding {
                promise_id,
                step_id,
                actor_id,
                observer_id,
                obligation_index,
            });
        }
        // Every obligation is inspected, but an untrusted collection cannot amplify refusal
        // output beyond the journey contract's schema-bounded promise count.
        findings.truncate(promise_finding_start.saturating_add(1));
    }

    if findings.is_empty() {
        Ok(bindings)
    } else {
        Err(findings)
    }
}

fn actor_observer_collisions<'a>(
    bindings: &[PromiseObserverBinding<'a>],
) -> Vec<PromiseObserverBinding<'a>> {
    let mut reported_promises = Vec::new();
    bindings
        .iter()
        .copied()
        .filter(|binding| {
            if binding.actor_id != binding.observer_id
                || reported_promises.contains(&binding.promise_id)
            {
                return false;
            }
            reported_promises.push(binding.promise_id);
            true
        })
        .collect()
}

/// Validates a Journey Verification Result. Deterministic: no provider, no network, no clock.
///
/// **The council is not read. Not carefully — at all.** Agent agreement is advisory, and the only
/// way to keep it advisory is to give it no code path. A gate that consulted the council merely to
/// REFUSE would still be letting agreement move the verdict, and that direction is the one a
/// well-meaning implementer reaches for, because refusing on disagreement looks conservative.
pub struct VerificationResultGate;

impl EvidenceGate<JpdEvidence> for VerificationResultGate {
    fn id(&self) -> &str {
        "gate/jpd-verification-result"
    }

    fn evaluate(&self, evidence: &JpdEvidence) -> Verdict {
        let JpdEvidence::VerificationResult(document) = evidence else {
            return Verdict {
                passed: false,
                findings: vec!["this gate judges verification results only".to_owned()],
            };
        };

        // A document that does not claim success has nothing to falsely certify.
        if !claims_success(document) {
            return Verdict {
                passed: true,
                findings: Vec::new(),
            };
        }

        let mut findings = Vec::new();
        for axis in [
            JpdFailureAxis::CapabilityMissingUnderClaimedSuccess,
            JpdFailureAxis::FlakyClaimedAsProven,
            JpdFailureAxis::WaiverIssuedForAnotherVerification,
        ] {
            if axis.is_defeated_by(evidence) {
                findings.push(finding_for(axis));
            }
        }

        Verdict {
            passed: findings.is_empty(),
            findings,
        }
    }
}

/// What each axis means when it fires, in the operator's words.
fn finding_for(axis: JpdFailureAxis) -> String {
    match axis {
        JpdFailureAxis::ActorUsedAsOwnObserver => {
            "a journey contract uses a step actor as that step's observer".to_owned()
        }
        JpdFailureAxis::CapabilityMissingUnderClaimedSuccess => {
            "a result claiming success reports gate.status = capability_missing: the capability that would have judged it never ran"
                .to_owned()
        }
        JpdFailureAxis::FlakyClaimedAsProven => {
            "a run classified flaky_pass is presented as proven; flaky success is not proven success"
                .to_owned()
        }
        JpdFailureAxis::WaiverIssuedForAnotherVerification => {
            "a waiver covers another verification than the result it is attached to; a transplanted waiver waives nothing here"
                .to_owned()
        }
    }
}

/// The JPD pathogen suite, written in the schema's vocabulary.
///
/// Two specimens is a FLOOR, not a coverage claim. Growing it changes `suite_digest`, which voids
/// stale certifications by comparison rather than by cleanup — but growth alone proves nothing,
/// which is why every specimen must also defeat the axis it names, and why the real repository
/// fixtures are driven through this gate in the tests: synthetic specimens can disagree with a
/// gate about LOGIC, never about VOCABULARY, because they were written by the same hand.
#[must_use]
pub fn jpd_suite() -> Vec<JpdSpecimen> {
    vec![
        JpdSpecimen {
            id: "verification/capability-missing-claimed-proven".to_owned(),
            axis: JpdFailureAxis::CapabilityMissingUnderClaimedSuccess,
            evidence: JpdEvidence::VerificationResult(serde_json::json!({
                "proposedResultStatus": "proven",
                "gate": { "status": "capability_missing" },
                "retry": { "outcomeClassification": "first_pass_success" }
            })),
        },
        JpdSpecimen {
            id: "verification/flaky-claimed-proven".to_owned(),
            axis: JpdFailureAxis::FlakyClaimedAsProven,
            evidence: JpdEvidence::VerificationResult(serde_json::json!({
                "proposedResultStatus": "proven",
                "gate": { "status": "evaluated" },
                "retry": { "outcomeClassification": "flaky_pass" }
            })),
        },
        JpdSpecimen {
            id: "verification/waiver-transplanted-from-another-verification".to_owned(),
            axis: JpdFailureAxis::WaiverIssuedForAnotherVerification,
            evidence: JpdEvidence::VerificationResult(serde_json::json!({
                "verificationId": "verification/this-run",
                "proposedResultStatus": "accepted_with_waiver",
                "gate": {
                    "status": "evaluated",
                    "result": "rejected",
                    "waiver": {
                        "coverage": {
                            "verificationId": "verification/some-other-run",
                            "blockerKind": "gate"
                        }
                    }
                },
                "retry": { "outcomeClassification": "recovered_success" }
            })),
        },
    ]
}

/// The journey-contract pathogen suite.
#[must_use]
pub fn journey_contract_suite() -> Vec<JpdSpecimen> {
    vec![JpdSpecimen {
        id: "contract/actor-as-own-observer".to_owned(),
        axis: JpdFailureAxis::ActorUsedAsOwnObserver,
        evidence: JpdEvidence::JourneyContract(JourneyContractEvidence {
            contract: serde_json::json!({
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
                    "requiredObserverCapability": "browser.semantic-journey"
                }]
            }),
            contract_digest:
                "sha256:1111111111111111111111111111111111111111111111111111111111111111".to_owned(),
            observation_obligations: vec![serde_json::json!({
                "contractId": "journey/checkout-s1b",
                "contractDigest": "sha256:1111111111111111111111111111111111111111111111111111111111111111",
                "promiseId": "promise/receipt-durable",
                "observerRequirements": {
                    "capability": "browser.semantic-journey"
                },
                "resolution": {
                    "status": "matched",
                    "capabilityBinding": {
                        "capabilityId": "browser.semantic-journey",
                        "observerId": "agent/checkout-driver"
                    }
                }
            })],
            verification_result: serde_json::json!({
                "contractId": "journey/checkout-s1b",
                "contractDigest": "sha256:1111111111111111111111111111111111111111111111111111111111111111",
                "bindings": {
                    "observers": [{ "observerId": "agent/checkout-driver" }]
                }
            }),
        }),
    }]
}
