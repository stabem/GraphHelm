use std::path::Path;

use graphhelm_governor::{
    GovernanceError, MutationDecision, RejectionReason, admit_signal, decide_mutation,
};
use graphhelm_protocols::{EventKind, NewEvent, OpaqueId, Sensitivity, SignalRecorded};

use super::{
    Failure, append_event, argument, execution_state, finish, idempotency_key, load_projection,
    owner_actor, repository_failure, signal_invalid, signal_unrecordable,
};
use crate::commands::event_store;
use crate::output::Outcome;

const COMMAND: &str = "execution.signal";

/// Admits one signal envelope, externalizes its evidence, and reports the governance verdict for
/// the mode in force.
///
/// Evidence externalizes to `--evidence-out`, a plain operator file — not the encrypted Evidence
/// store. The sealed-provider externalization pipeline expects the Governor's own content slots
/// (`core/governor/src/externalize.rs`), and a signal envelope has none of those; inventing one
/// would fabricate a content position 04d never defined. `envelope_sha256` on the recorded event
/// still binds the record to these exact bytes. Operator-grade encrypted externalization of
/// signal envelopes is Milestone 05 work.
pub fn run(events: &Path, execution: Option<&str>, signal: &Path, evidence_out: &Path) -> Outcome {
    finish(
        COMMAND,
        execute(events, execution, signal, evidence_out),
        |value| value,
    )
}

fn execute(
    events: &Path,
    execution: Option<&str>,
    signal: &Path,
    evidence_out: &Path,
) -> Result<serde_json::Value, Failure> {
    let store = event_store(events).map_err(|error| repository_failure(&error))?;
    let (scope, stream, projection) = load_projection(&store, execution)?;

    let raw = std::fs::read(signal)
        .map_err(|_| argument("--signal does not name a readable file", "/signal"))?;
    let envelope: serde_json::Value = serde_json::from_slice(&raw)
        .map_err(|_| signal_invalid("the signal envelope is not valid JSON", "/signal"))?;

    let admitted = match admit_signal(&projection, &envelope) {
        Ok(admitted) => admitted,
        Err(GovernanceError::InvalidSignal) => {
            return Err(signal_invalid(
                "the signal envelope failed schema validation",
                "/signal",
            ));
        }
        Err(GovernanceError::UnrecordableIdentity) => {
            // The record cannot go on the wire, but the envelope itself is still evidence: write
            // the original bytes before refusing, so nothing the operator submitted is lost.
            std::fs::write(evidence_out, &raw).map_err(|_| {
                execution_state(
                    "the signal's identity cannot be recorded, and the evidence file could not \
                     be written either; nothing was preserved",
                    "/evidenceOut",
                )
            })?;
            return Err(signal_unrecordable(
                "the signal's id or source id cannot be represented on the wire; the evidence \
                 was preserved at --evidence-out",
                "/signal",
            ));
        }
        Err(GovernanceError::SignalBudgetExhausted) => {
            return Err(execution_state(
                "the execution has recorded its full signal budget and must be blocked for an \
                 owner decision",
                "/signal",
            ));
        }
        Err(GovernanceError::NotStarted) => {
            return Err(execution_state(
                "no execution has started on this stream",
                "/execution",
            ));
        }
        // `admit_signal` never returns these two: they belong to `override_with_waiver`'s
        // contract, a different function in the same crate. Handled defensively rather than with
        // `unreachable!()`, because this command is driven by an operator-supplied file and a
        // future change to `admit_signal` should degrade gracefully, not panic.
        Err(GovernanceError::UnacknowledgedRisk | GovernanceError::InvalidWaiver) => {
            return Err(execution_state(
                "admit_signal produced an unexpected governance error",
                "/signal",
            ));
        }
    };

    // Fail closed: the evidence write happens before the append, and if it fails nothing is
    // appended — an event whose evidence was not preserved would be a record with no backing.
    std::fs::write(evidence_out, &admitted.externalize).map_err(|_| {
        execution_state(
            "the evidence could not be written; nothing was recorded",
            "/evidenceOut",
        )
    })?;

    let stream_id = OpaqueId::parse(&stream)
        .map_err(|_| execution_state("the stream identifier is not wire-safe", "/execution"))?;
    append_event(
        &store,
        &scope,
        &stream_id,
        NewEvent::new(
            idempotency_key("signal-recorded"),
            owner_actor(),
            Sensitivity::Internal,
            EventKind::SignalRecorded(admitted.record.clone()),
            vec![],
            vec![],
        ),
    )?;

    // "Right now" per `decide_mutation`'s own contract means the projection folded to the append
    // point; nothing between the read above and this append could have changed `mode` or
    // `accepted_mutations` (the `signal_recorded` fold touches neither), so the projection already
    // in hand is that point.
    let decision = decide_mutation(&projection, &admitted);
    Ok(serde_json::json!({
        "executionId": projection.execution_id,
        "signalId": signal_id(&admitted.record),
        "kind": admitted.record.kind,
        "mayProposeMutation": admitted.may_propose_mutation,
        "decision": decision_label(decision),
        "rejectionReason": rejection_label(decision),
    }))
}

fn signal_id(record: &SignalRecorded) -> &str {
    record.signal_id.as_str()
}

/// `MutationDecision` carries no `Serialize` impl (`core/governor` exposes it as a plain enum,
/// per the plan: "serialize the enum in snake_case by hand ... no new public API"); this is that
/// hand-written mapping.
fn decision_label(decision: MutationDecision) -> &'static str {
    match decision {
        MutationDecision::Accept => "accept",
        MutationDecision::RequiresApproval => "requires_approval",
        MutationDecision::Rejected(_) => "rejected",
        MutationDecision::Blocked => "blocked",
    }
}

/// The rejection reason, present only when `decision` is `Rejected`.
fn rejection_label(decision: MutationDecision) -> Option<&'static str> {
    match decision {
        MutationDecision::Rejected(RejectionReason::ManualMode) => Some("manual_mode"),
        MutationDecision::Rejected(RejectionReason::SignalNotActionable) => {
            Some("signal_not_actionable")
        }
        MutationDecision::Accept
        | MutationDecision::RequiresApproval
        | MutationDecision::Blocked => None,
    }
}
