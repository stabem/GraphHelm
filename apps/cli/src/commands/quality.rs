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
    Diagnostic, EventKind, GateCertified, NewEvent, OpaqueId, Sensitivity, WireHash,
};

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
/// very string, via a line continuation the formatter collapsed. (L's refinement.)
///
/// **If id and certifier are ever separated again, the split must come back.** L's finding holds
/// for that shape; the shape is what changed, not the finding.
const REGISTRY: [(&str, Certifier); 2] = [
    ("gate-geometry", certify_geometry),
    ("gate-retry-lineage", certify_retry_lineage),
];

/// The registered ids, in registry order.
fn registered_ids() -> Vec<&'static str> {
    REGISTRY.iter().map(|(id, _)| *id).collect()
}

/// Look the gate up and run its certification. `None` means no entry -- which is also what makes
/// admission and dispatch the same operation.
fn certify_registered(
    gate: &str,
) -> Option<Result<pathogens::Certification, pathogens::CertificationRefusal>> {
    REGISTRY
        .iter()
        .find(|(id, _)| *id == gate)
        .map(|(_, run)| run())
}

/// The refusal, naming what IS registered.
fn registry_refusal() -> String {
    format!(
        "no runnable gate by that id is registered (the registry is closed: {})",
        registered_ids().join(", ")
    )
}
const GATE_INVALID: &str = "GHCLI018_GATE_INVALID";

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
