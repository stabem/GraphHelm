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

/// The five artifact kinds #211 names for runtime-side validation.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JpdEvidence {
    JourneyContract(Value),
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
            Self::JourneyContract(v)
            | Self::ObservationObligation(v)
            | Self::RetryLineage(v)
            | Self::CouncilResult(v)
            | Self::VerificationResult(v) => v,
        }
    }
}

/// How a JPD certification can be fooled.
///
/// A CLOSED set, and deliberately NOT merged into `UselessnessMode`: those are failures of a
/// rendered surface, these are failures of evidence. One enum covering both would give every
/// exhaustive match over it arms it cannot mean, and that failure is silent because the enum
/// still compiles everywhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JpdFailureAxis {
    /// A required observer capability is absent and the result claims success anyway.
    MissingObserver,
    /// A run that only passed on retry, presented as proven.
    FlakyCountedAsProven,
    /// The producer of the work is also its validator.
    SelfValidation,
}

impl FailureAxis<JpdEvidence> for JpdFailureAxis {
    fn is_defeated_by(&self, evidence: &JpdEvidence) -> bool {
        let document = evidence.document();
        // Every axis below is only defeated by a document that CLAIMS success. A document
        // admitting failure defeats nothing: there is no false certification to earn.
        if document.get("status").and_then(Value::as_str) != Some("passed") {
            return false;
        }
        match self {
            Self::MissingObserver => document
                .get("observers")
                .and_then(Value::as_array)
                .is_none_or(|observers| observers.is_empty()),
            Self::FlakyCountedAsProven => {
                document
                    .get("attempts")
                    .and_then(Value::as_u64)
                    .unwrap_or(1)
                    > 1
            }
            Self::SelfValidation => {
                let producer = document.get("producer");
                producer.is_some() && producer == document.get("validator")
            }
        }
    }
}

/// A JPD specimen: typed evidence plus the axis it defeats.
pub type JpdSpecimen = Specimen<JpdEvidence, JpdFailureAxis>;

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
        if document.get("status").and_then(Value::as_str) != Some("passed") {
            return Verdict {
                passed: true,
                findings: Vec::new(),
            };
        }

        let mut findings = Vec::new();
        for axis in [
            JpdFailureAxis::MissingObserver,
            JpdFailureAxis::FlakyCountedAsProven,
            JpdFailureAxis::SelfValidation,
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
        JpdFailureAxis::MissingObserver => {
            "a result claiming success carries no observer capability".to_owned()
        }
        JpdFailureAxis::FlakyCountedAsProven => {
            "success reached only on retry is not proven success".to_owned()
        }
        JpdFailureAxis::SelfValidation => {
            "the producer of the work is also its validator".to_owned()
        }
    }
}

/// The JPD pathogen suite.
///
/// Three specimens is a FLOOR, not a coverage claim: it is the smallest set that exercises more
/// than one axis. Growing it changes `suite_digest`, which voids every stale certification by
/// comparison rather than by cleanup — but growth alone proves nothing, which is why every
/// specimen must also defeat the axis it names.
#[must_use]
pub fn jpd_suite() -> Vec<JpdSpecimen> {
    vec![
        JpdSpecimen {
            id: "verification/missing-observer".to_owned(),
            axis: JpdFailureAxis::MissingObserver,
            evidence: JpdEvidence::VerificationResult(serde_json::json!({
                "status": "passed",
                "observers": []
            })),
        },
        JpdSpecimen {
            id: "verification/flaky-as-proven".to_owned(),
            axis: JpdFailureAxis::FlakyCountedAsProven,
            evidence: JpdEvidence::VerificationResult(serde_json::json!({
                "status": "passed",
                "observers": [{ "capability": "first_party_deterministic" }],
                "attempts": 3
            })),
        },
        JpdSpecimen {
            id: "verification/self-validated".to_owned(),
            axis: JpdFailureAxis::SelfValidation,
            evidence: JpdEvidence::VerificationResult(serde_json::json!({
                "status": "passed",
                "observers": [{ "capability": "first_party_deterministic" }],
                "producer": "agent-a",
                "validator": "agent-a"
            })),
        },
    ]
}
