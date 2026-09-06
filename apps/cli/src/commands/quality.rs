//! `graphhelm quality certify` (M06 Task 7): the thymus ritual as an operator command —
//! run a REGISTERED gate against the pathogen suite and, only on full rejection, stamp
//! `GateCertified` onto the stream the gated execution will run on. The stamp is what
//! Task 4's certified-or-not-at-all precondition reads; a refusal names who fooled the
//! candidate. The registry of runnable gates is CLOSED (the 05e omission pattern): the
//! composed geometry evaluator is the one entry, and the blind judge is deliberately NOT
//! certifiable here — its verdicts are model calls; its discipline is blindness, proven
//! by the runtime's own fences, not by pathogen rejection.

use std::collections::BTreeSet;
use std::path::Path;

use graphhelm_protocols::{
    Diagnostic, EventKind, GateCertified, GateFinding, NewEvent, OpaqueId, Sensitivity,
    SignalSeverity, WireHash,
};

use graphhelm_runtime::ports::GateEvaluation;

use crate::commands::event_store;
use crate::commands::execution::{
    append_event, finish, load_projection, owner_actor, repository_failure,
};
use crate::output::Outcome;

const COMMAND: &str = "quality.certify";

/// The closed registry: ONE array, each id paired with the certification that id runs.
///
/// **There is no second list.** The refusal message maps over these entries and the lookup
/// searches them, so *advertised* and *certifiable* are the SAME SET by construction rather than
/// two lists held equal by a guard. The mismatch a guard would have caught -- a name advertised
/// with no adapter, or an adapter absent from the message -- is not detected here, it is
/// **inexpressible**: there is nowhere to write it.
///
/// Admission is the lookup, so a name with no entry cannot be admitted.
///
/// Adding a gate means adding an entry, and the entry carries its own suite: there is no generic
/// "certify anything" door. `certify` over an EMPTY suite has no pathogen to be fooled by, so an
/// open door would stamp a gate nothing ever attacked.
///
/// The key is the CLI-facing id, deliberately NOT the gate's own `id()`. `RetryLineageGate::id()`
/// is `graphhelm-jpd/retry-lineage-validator` -- the evaluatorId the JPD policy declares -- and it
/// contains `/`, which `GateCertified.gate_id` REFUSES: the event schema types it as `opaqueId`,
/// whose pattern excludes `/`. Keying this registry on `id()` would stamp an event the schema
/// rejects. Measured, not assumed.
type Certifier = fn() -> Result<pathogens::Certification, pathogens::CertificationRefusal>;

/// Each certifier names its OWN gate and suite. Kept as separate functions rather than closures so
/// that cross-wiring one to its neighbour's suite is a compile error: the evidence types differ,
/// so `certify(&GeometryGate, &retry_lineage_suite())` is `error[E0308]`, measured.
fn certify_geometry() -> Result<pathogens::Certification, pathogens::CertificationRefusal> {
    pathogens::certify(&GeometryGate, &pathogens::suite())
}

fn certify_retry_lineage() -> Result<pathogens::Certification, pathogens::CertificationRefusal> {
    pathogens::certify(
        &pathogens::retry_lineage::RetryLineageGate,
        &pathogens::retry_lineage::retry_lineage_suite(),
    )
}

/// #211's journey-contract gate, reachable from the command for the first time here.
///
/// The gate and its suite already existed and already certified inside the pathogens crate; what
/// did not exist was a way for an operator to run it. Registering it is the whole of that step.
///
/// The CLI-facing id is `gate-journey-contract`, NOT the gate's own `id()`
/// (`gate/jpd-journey-contract`): that `/` is refused by `GateCertified.gate_id`, typed as
/// `opaqueId`. Same constraint the retry-lineage entry above records, inherited rather than
/// rediscovered at runtime.
fn certify_journey_contract() -> Result<pathogens::Certification, pathogens::CertificationRefusal> {
    pathogens::certify(
        &pathogens::jpd::JourneyContractGate,
        &pathogens::jpd::journey_contract_suite(),
    )
}

/// **This array is why the registry cannot disagree with itself, and the history is worth keeping.**
///
/// #313 split the refusal below into two, because two states existed: an operator mistyping an id,
/// and the registry disagreeing with the dispatch -- a defect the operator could not fix. The
/// second produced a refusal that CONTRADICTED ITSELF, naming the refused id inside the list of
/// what is registered. Not hypothetical: a half-applied edit produced exactly that state, and a
/// guard caught it. (Found by L.)
///
/// Pairing the id WITH its certifier in one tuple retired that whole class. An id without a
/// certifier is not caught here -- it cannot be WRITTEN: there is no field for it.
/// `certify_registered` searches these entries and `registered_ids` maps over them, so the two
/// predicates are exact complements and "registered but no adapter" has no state to occupy.
///
/// The reasoning lives HERE, on the thing that creates the property, rather than on the refusal
/// that merely benefits from it. A defensive arm for a dead state rots: it carries a string
/// nothing can render, so nothing goes red when someone edits it -- which nearly happened to that
/// very string, via a `\` continuation lost before the file was written. **The formatter did not
/// collapse it and cannot**: `format_strings` is absent from this repository's `rustfmt.toml`, so
/// rustfmt never edits inside a literal (#440). The refinement this once credited to L is refuted
/// by L; the finding it supports — that a defensive arm for a dead state rots unread — stands.
///
/// **If id and certifier are ever separated again, the split must come back.** L's finding holds
/// for that shape; the shape is what changed, not the finding.
/// **The consumer path reads this same array (#668), which is why it is a struct now.**
///
/// A registered gate is three inseparable things: the id an operator types, the certification
/// that id runs, and the evaluator that judges a NODE's evidence under it. Until #668 the third
/// column did not exist anywhere -- the runtime evaluated every dispatched gate as geometry and
/// compared every receipt against geometry's digest, so a gate could be certified here and was
/// still refused as uncertified at dispatch. Putting the evaluator in the SAME entry as the
/// certifier means a gate that certifies but cannot run has nowhere to be written, exactly as an
/// id without a certifier has nowhere to be written.
///
/// **There is no digest column, deliberately.** The digest a node's receipt is compared against
/// is read from the certification this entry's own certifier produces, so "the suite this gate
/// was certified against" and "the suite its receipt is checked against" are one value from one
/// call rather than two expressions held equal by hope.
struct RegisteredGate {
    id: &'static str,
    certify: Certifier,
    evaluate: Evaluator,
}

/// A node's evidence, judged by ONE registered gate.
///
/// The return type carries the distinction that matters to an append-only store: a VERDICT is a
/// permanent claim about a delivered surface, while UNREADABLE evidence is an authoring fault in
/// the node's contract and must produce no verdict at all.
type Evaluator = fn(&serde_json::Value) -> GateEvaluation;

const REGISTRY: [RegisteredGate; 3] = [
    RegisteredGate {
        id: "gate-geometry",
        certify: certify_geometry,
        evaluate: evaluate_geometry_evidence,
    },
    RegisteredGate {
        id: "gate-retry-lineage",
        certify: certify_retry_lineage,
        evaluate: evaluate_retry_lineage_evidence,
    },
    RegisteredGate {
        id: "gate-journey-contract",
        certify: certify_journey_contract,
        evaluate: evaluate_journey_contract_evidence,
    },
];

/// The registered ids, in registry order.
fn registered_ids() -> Vec<&'static str> {
    REGISTRY.iter().map(|entry| entry.id).collect()
}

/// Look the gate up and run its certification. `None` means no entry -- which is also what makes
/// admission and dispatch the same operation.
fn certify_registered(
    gate: &str,
) -> Option<Result<pathogens::Certification, pathogens::CertificationRefusal>> {
    REGISTRY
        .iter()
        .find(|entry| entry.id == gate)
        .map(|entry| (entry.certify)())
}

/// The refusal, naming what IS registered.
fn registry_refusal() -> String {
    format!(
        "no runnable gate by that id is registered (the registry is closed: {})",
        registered_ids().join(", ")
    )
}
const GATE_INVALID: &str = crate::error_codes::GHCLI018_GATE_INVALID;

fn refuse(message: &str, pointer: &str) -> Outcome {
    Outcome::domain(
        COMMAND,
        vec![Diagnostic::error(GATE_INVALID, message, pointer, COMMAND)],
    )
}

/// The composed geometry evaluator as a thymus candidate — the SAME composition
/// `core/quality`'s own certification tests run, adapted over the pathogen surface.
struct GeometryGate;

impl pathogens::CandidateGate for GeometryGate {
    fn id(&self) -> &str {
        "gate-geometry"
    }
    fn evaluate(&self, deliverable: &pathogens::Deliverable) -> pathogens::Verdict {
        let delivered = graphhelm_quality::Delivered {
            claims: deliverable
                .claims
                .iter()
                .map(|claim| graphhelm_quality::DeliveredClaim {
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
                .map(|step| graphhelm_quality::JourneyStep {
                    action: step.action.clone(),
                    assertion: step.assertion.clone(),
                    exercises_error_path: step.exercises_error_path,
                })
                .collect(),
            tests: deliverable
                .tests
                .iter()
                .map(|test| graphhelm_quality::TestCheck {
                    name: test.name.clone(),
                    passed: test.passed,
                    assertions: test.assertions,
                })
                .collect(),
            diff: graphhelm_quality::DiffShape {
                files_touched: deliverable.diff.files_touched,
                behavior_lines: deliverable.diff.behavior_lines,
            },
        };
        let manifest = graphhelm_quality::ContentManifest::from_claims(&delivered.claims);
        let findings = graphhelm_quality::evaluate_geometry(
            &delivered,
            &manifest,
            &graphhelm_quality::LayoutBudget::default(),
        );
        pathogens::Verdict {
            passed: findings.is_empty(),
            findings: findings.into_iter().map(|finding| finding.claim).collect(),
        }
    }
}

pub fn run(events: &Path, execution: Option<&str>, gate: &str) -> Outcome {
    // Admission IS dispatch: one lookup, so the two cannot disagree.
    //
    // The earlier shape asked `registered_ids().contains(&gate)` and then chose an adapter in a
    // separate `match`. Those are two statements of one fact, and the duplication they replaced
    // was LOAD-BEARING: when a single literal both admitted and dispatched, a name could not be
    // admitted without an adapter. Splitting them re-opened that gap one level up. (Found by L.)
    //
    // The key is the CLI-facing id, deliberately NOT the gate's own `id()`. They are different
    // namespaces and the difference is not cosmetic: `RetryLineageGate::id()` is
    // `graphhelm-jpd/retry-lineage-validator`, the evaluatorId the JPD policy declares -- and that
    // string contains `/`, which `GateCertified.gate_id` REFUSES, because the event schema types
    // it as `opaqueId` whose pattern excludes `/`. Keying this registry on `id()` would stamp an
    // event the schema rejects. Measured, not assumed.
    let Some(outcome) = certify_registered(gate) else {
        // One fact, and that is a property of REGISTRY rather than of this site: the
        // invariant is recorded above the array.
        return refuse(&registry_refusal(), "/gate");
    };
    let certification = match outcome {
        Ok(certification) => certification,
        Err(refusal) => {
            return refuse(
                &format!(
                    "certification refused: the candidate passed pathogens {:?}",
                    refusal.fooled_by
                ),
                "/gate",
            );
        }
    };

    if let Err(outcome) = stamp(events, execution, gate, &certification) {
        return outcome;
    }
    Outcome::success(
        COMMAND,
        serde_json::json!({
            "gateCertified": true,
            "gateId": gate,
            "suiteDigest": certification.suite_digest,
            "specimens": certification.specimens,
        }),
    )
}

/// Loads the stream, requires a started execution, and appends the receipt — Failure-based
/// so every store refusal keeps the house envelope.
fn stamp(
    events: &Path,
    execution: Option<&str>,
    gate: &str,
    certification: &pathogens::Certification,
) -> Result<(), Outcome> {
    let store = event_store(events).map_err(|error| {
        finish::<()>(COMMAND, Err(repository_failure(&error)), |_| {
            serde_json::Value::Null
        })
    })?;
    let (scope, stream, projection) = load_projection(&store, execution)
        .map_err(|failure| finish::<()>(COMMAND, Err(failure), |_| serde_json::Value::Null))?;
    let execution_id = projection
        .execution_id
        .clone()
        .ok_or_else(|| refuse("no execution has started on this stream", "/execution"))?;
    let stream_id = OpaqueId::parse(&stream)
        .map_err(|_| refuse("the stream identifier is not wire-safe", "/execution"))?;
    let execution_opaque = OpaqueId::parse(&execution_id)
        .map_err(|_| refuse("the execution identifier is not wire-safe", "/execution"))?;
    let gate_id =
        OpaqueId::parse(gate).map_err(|_| refuse("the gate id is not wire-safe", "/gate"))?;
    let digest = WireHash::parse(&certification.suite_digest)
        .map_err(|_| refuse("the suite digest is not a wire hash", "/gate"))?;
    append_event(
        &store,
        &scope,
        &stream_id,
        NewEvent::new(
            OpaqueId::parse(format!("gate-certified-{gate}-{}", certification.specimens))
                .expect("a derived key is wire-safe"),
            owner_actor(),
            Sensitivity::Internal,
            EventKind::GateCertified(GateCertified {
                execution_id: execution_opaque,
                gate_id,
                suite_digest: digest,
                specimens: certification.specimens,
            }),
            vec![],
            vec![],
        ),
    )
    .map_err(|failure| finish::<()>(COMMAND, Err(failure), |_| serde_json::Value::Null))?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// The consumer path (#668): what a REGISTERED gate does when a running node names it.
//
// Certification proves a gate cannot be fooled by its own suite. It says nothing about how that
// gate reads a node's evidence, and until this section existed there was nowhere to say it: the
// runtime hard-coded geometry's evaluator for every gate id and compared every fold receipt
// against geometry's suite digest. `quality certify` stamped `GateCertified` for
// `gate-retry-lineage` and `gate-journey-contract`; a node using either was refused as
// uncertified, so certification said yes and the consumer path could only say no.
//
// Each evaluator below builds the evidence ITS OWN gate judges, from the JSON the node's
// contract carries, and returns that gate's findings. A gate handed evidence of the wrong shape
// REFUSES WITH A FINDING rather than panicking or being skipped: "this is not the evidence I
// judge" is a verdict an operator can read, and it is the honest answer when a graph points a
// journey-contract gate at a rendered surface.
// ---------------------------------------------------------------------------------------------

/// Geometry's evidence, with the strictness that used to live on `GateCheckWork` itself.
///
/// `deny_unknown_fields` did not move OUT of the contract, it moved IN to the gate that cares.
/// A misspelled key is detected here and answered as `Unreadable`, which the executor turns into
/// the same `Unassemblable` refusal the driver used to give -- the node is not dispatched and no
/// verdict is appended.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GeometryEvidence {
    delivered: graphhelm_quality::Delivered,
    manifest: graphhelm_quality::ContentManifest,
    #[serde(default)]
    budget: graphhelm_quality::LayoutBudget,
}

/// One finding, in the shape the `GateVerdict` event carries.
fn gate_finding(claim: String, remediation: String) -> GateFinding {
    GateFinding {
        severity: SignalSeverity::High,
        claim,
        evidence: vec![],
        remediation,
    }
}

/// What a gate answers when the node's contract does not carry evidence it can read.
///
/// **Not a verdict, and the difference is the append-only store.** A `GateVerdict` is permanent
/// historical evidence; a failing one says a delivered surface was examined and refused. A node
/// whose `gate.check` block is misspelled was never examined by anything, and writing a
/// High-severity finding about it would leave a claim that fixing the typo cannot retract. The
/// executor turns this into the same `Unassemblable` refusal an unparseable contract always
/// produced -- the node is not dispatched, and nothing is appended.
///
/// (Found by L reviewing #771: the first version of this file returned findings here, which
/// silently converted a graph-authoring typo from a refusal into a permanent accusation.)
fn unreadable(gate: &str, detail: &str) -> GateEvaluation {
    GateEvaluation::Unreadable(format!(
        concat!(
            "{gate} cannot read this node's evidence: {detail}. ",
            "This is a fault in the node's gate contract, not a verdict about a delivered surface."
        ),
        gate = gate,
        detail = detail,
    ))
}

/// `gate-geometry`: the composed delivery-coherence and layout evaluator, over the delivered
/// surface the node's contract carries. Byte-identical scoring to what the runtime did before
/// this dispatch existed — the same `evaluate_geometry` call, reached by lookup instead of by
/// being the only thing the executor knew how to do.
fn evaluate_geometry_evidence(evidence: &serde_json::Value) -> GateEvaluation {
    match serde_json::from_value::<GeometryEvidence>(evidence.clone()) {
        Ok(evidence) => GateEvaluation::Verdict(graphhelm_quality::evaluate_geometry(
            &evidence.delivered,
            &evidence.manifest,
            &evidence.budget,
        )),
        // Including the misspelled-key case `deny_unknown_fields` catches: detected here, and
        // refused rather than verdicted.
        Err(error) => unreadable("gate-geometry", &error.to_string()),
    }
}

/// `gate-retry-lineage`: the nine declared structural checks over a retry-lineage document.
///
/// The document is the node's evidence under `document` when the contract names one, and the
/// whole evidence object otherwise — the second form is what a node written before this gate had
/// a consumer path carries, and the checks read it honestly either way (a document with no
/// lineage fails the lineage checks; it is not silently passed).
fn evaluate_retry_lineage_evidence(evidence: &serde_json::Value) -> GateEvaluation {
    let document = evidence.get("document").unwrap_or(evidence).clone();
    let verdict = pathogens::EvidenceGate::evaluate(
        &pathogens::retry_lineage::RetryLineageGate,
        &pathogens::jpd::JpdEvidence::RetryLineage(document),
    );
    GateEvaluation::Verdict(
        verdict
            .findings
            .into_iter()
            .map(|claim| {
                gate_finding(
                    claim,
                    "repair the retry lineage the document claims is complete".to_owned(),
                )
            })
            .collect(),
    )
}

/// `gate-journey-contract`: the cross-record independence obligation JSON Schema cannot express.
///
/// The gate's own diagnostics are kept (code, path and message), not the generic string
/// projection: a refusal that names `GHJPD000_WRONG_EVIDENCE_KIND` and the path it looked at is
/// the difference between "the gate refused" and "the gate was pointed at the wrong thing".
fn evaluate_journey_contract_evidence(evidence: &serde_json::Value) -> GateEvaluation {
    let jpd = journey_contract_evidence(evidence);
    // A VERDICT even when the gate rejects the evidence's kind: this gate parsed what the node
    // carried and judged it, which is what separates `GHJPD000_WRONG_EVIDENCE_KIND` from a
    // contract the evaluator could not read at all.
    GateEvaluation::Verdict(
        pathogens::jpd::JourneyContractGate
            .diagnostics(&jpd)
            .into_iter()
            .map(|diagnostic| {
                gate_finding(
                    format!(
                        "{} at {}: {}",
                        diagnostic.code, diagnostic.path, diagnostic.message
                    ),
                    "appoint an observer who is not the actor performing the step".to_owned(),
                )
            })
            .collect(),
    )
}

/// Builds the journey-contract evidence when the node carries all four parts, and something the
/// gate will refuse as the wrong kind when it does not. The refusal is the gate's own, so the
/// operator reads one vocabulary rather than this adapter's paraphrase of it.
fn journey_contract_evidence(evidence: &serde_json::Value) -> pathogens::jpd::JpdEvidence {
    let contract = evidence.get("contract");
    let digest = evidence.get("contractDigest").and_then(|v| v.as_str());
    let obligations = evidence
        .get("observationObligations")
        .and_then(|v| v.as_array());
    let verification = evidence.get("verificationResult");
    match (contract, digest, obligations, verification) {
        (Some(contract), Some(digest), Some(obligations), Some(verification)) => {
            pathogens::jpd::JpdEvidence::JourneyContract(pathogens::jpd::JourneyContractEvidence {
                contract: contract.clone(),
                contract_digest: digest.to_owned(),
                observation_obligations: obligations.clone(),
                verification_result: verification.clone(),
            })
        }
        _ => pathogens::jpd::JpdEvidence::VerificationResult(evidence.clone()),
    }
}

/// The runtime's view of this registry: which gates exist, and what each one says.
///
/// Both answers come from the SAME array, so the gate a node is certified against and the gate
/// that judges it are the same gate by construction. The digest is read from the certification
/// this build actually runs rather than recomputed from a suite named a second time.
pub struct RegisteredGates;

impl graphhelm_runtime::ports::GateRegistryPort for RegisteredGates {
    /// The digest of `gate_id`'s own suite, as this build certifies it.
    ///
    /// `None` for an unregistered id AND for a gate this build can no longer certify — a
    /// candidate its own pathogens now fool has no current immunity to compare a receipt
    /// against, and refusing to dispatch it is the same fail-closed rule as an absent registry.
    fn suite_digest(&self, gate_id: &str) -> Option<String> {
        certify_registered(gate_id)?
            .ok()
            .map(|certification| certification.suite_digest)
    }

    fn evaluate(&self, gate_id: &str, evidence: &serde_json::Value) -> Option<GateEvaluation> {
        REGISTRY
            .iter()
            .find(|entry| entry.id == gate_id)
            .map(|entry| (entry.evaluate)(evidence))
    }
}
