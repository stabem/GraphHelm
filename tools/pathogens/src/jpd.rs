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
        if !claims_success(document) {
            return false;
        }
        match self {
            Self::CapabilityMissingUnderClaimedSuccess => {
                document.pointer("/gate/status").and_then(Value::as_str)
                    == Some("capability_missing")
            }
            Self::FlakyClaimedAsProven => {
                document.get("proposedResultStatus").and_then(Value::as_str) == Some("proven")
                    && document
                        .pointer("/retry/outcomeClassification")
                        .and_then(Value::as_str)
                        == Some("flaky_pass")
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
        JpdFailureAxis::CapabilityMissingUnderClaimedSuccess => {
            "a result claiming success reports gate.status = capability_missing: the capability              that would have judged it never ran"
                .to_owned()
        }
        JpdFailureAxis::FlakyClaimedAsProven => {
            "a run classified flaky_pass is presented as proven; flaky success is not proven              success"
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
    ]
}
