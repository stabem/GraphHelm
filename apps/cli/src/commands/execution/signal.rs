use std::path::{Path, PathBuf};

use graphhelm_events::{EvidenceInput, EvidenceProtector, EvidenceSealer, SecretBytes};
use graphhelm_sealed_key_provider::SealedKeyProvider;

use graphhelm_governor::{
    GovernanceError, MutationDecision, RejectionReason, admit_signal, decide_mutation,
};
use graphhelm_protocols::{
    EventKind, NewEvent, OpaqueId, PersistedActor, Sensitivity, SignalRecorded,
};

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
///
/// Reads `--signal` itself (the one CLI-only step in this command — Milestone 05a Task 3 moved it
/// out of `execute` so the Public Runtime API's `POST .../signal` can drive the same shared logic
/// from a JSON body's inline envelope instead of a file), then calls `execute` with the owner actor
/// and a fresh per-invocation idempotency key, exactly as before this task — byte-identical CLI
/// behaviour.
/// The keyring coordinates the CLI surface makes mandatory (Milestone 05d Task 6): the
/// signal command refuses to run without a keyring rather than silently skipping the seal.
/// The API seam still passes `None` — HTTP-side sealing lands with the serve keyring wiring
/// (a declared Task 6 discrepancy, not a silent skip: the CLI cannot reach that path).
pub(crate) struct SignalKeyring {
    pub(crate) directory: PathBuf,
    pub(crate) key_id: String,
}

pub fn run(
    events: &Path,
    execution: Option<&str>,
    signal: &Path,
    evidence_out: &Path,
    keyring: &Path,
    key_id: &str,
) -> Outcome {
    finish(
        COMMAND,
        run_from_file(
            events,
            execution,
            signal,
            evidence_out,
            &SignalKeyring {
                directory: keyring.to_path_buf(),
                key_id: key_id.to_owned(),
            },
        ),
        |value| value,
    )
}

fn run_from_file(
    events: &Path,
    execution: Option<&str>,
    signal: &Path,
    evidence_out: &Path,
    keyring: &SignalKeyring,
) -> Result<serde_json::Value, Failure> {
    let raw = std::fs::read(signal)
        .map_err(|_| argument("--signal does not name a readable file", "/signal"))?;
    execute(
        events,
        execution,
        &raw,
        evidence_out,
        owner_actor(),
        idempotency_key("signal-recorded"),
        Some(keyring),
    )
}

/// Mirrors `commands::events::config`'s provider construction: the 32-byte key never rides a
/// flag — it arrives out of band via `GRAPHHELM_EVENTS_KEY` as 64 lowercase hex characters.
fn open_sealer(keyring: &SignalKeyring) -> Result<EvidenceProtector<SealedKeyProvider>, Failure> {
    let invalid = || {
        argument(
            "GRAPHHELM_EVENTS_KEY must supply 64 lowercase hexadecimal characters",
            "/keyring",
        )
    };
    let encoded = std::env::var("GRAPHHELM_EVENTS_KEY").map_err(|_| invalid())?;
    if encoded.len() != 64
        || !encoded
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        return Err(invalid());
    }
    let mut material = Vec::with_capacity(32);
    let bytes = encoded.as_bytes();
    for pair in bytes.chunks(2) {
        let value = u8::from_str_radix(std::str::from_utf8(pair).map_err(|_| invalid())?, 16)
            .map_err(|_| invalid())?;
        material.push(value);
    }
    let provider = SealedKeyProvider::open(
        &keyring.directory,
        keyring.key_id.clone(),
        SecretBytes::new(material),
    )
    .map_err(|_| argument("the sealed keyring could not be opened", "/keyring"))?;
    Ok(EvidenceProtector::new(provider))
}

/// The shared core: admits `signal` (already-read bytes — a file's contents from the CLI, or an
/// inline JSON body re-serialized from the API, see `commands::serve::routes::signal`), externalizes
/// its evidence, appends the recorded signal attributed to `actor` under `key`, and reports the
/// governance verdict for the mode in force.
///
/// Widened from private to `pub(crate)` (Milestone 05a Task 3), gaining `actor` and `key` as
/// explicit parameters — the mechanical widening the plan's file table names, plus the one
/// additional parameter the idempotent-retry semantics require: a caller-supplied key rather than
/// this function minting its own fresh one, so the API path can derive it deterministically from
/// `Idempotency-Key` while the CLI (`run_from_file`, above) keeps minting a fresh one exactly as
/// before. No other logic changed.
pub(crate) fn execute(
    events: &Path,
    execution: Option<&str>,
    signal: &[u8],
    evidence_out: &Path,
    actor: PersistedActor,
    key: OpaqueId,
    sealing: Option<&SignalKeyring>,
) -> Result<serde_json::Value, Failure> {
    let store = event_store(events).map_err(|error| repository_failure(&error))?;
    let (scope, stream, projection) = load_projection(&store, execution)?;

    let raw = signal;
    let envelope: serde_json::Value = serde_json::from_slice(raw)
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
            std::fs::write(evidence_out, raw).map_err(|_| {
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

    // Milestone 05d Task 6: the envelope ALSO seals into the Evidence store, before the
    // append — the same fail-closed rule. `envelope_sha256` digests exactly these bytes, so
    // the sealed reference's content digest and the record agree by construction.
    let sealed = match sealing {
        Some(keyring) => {
            let protector = open_sealer(keyring)?;
            let input = EvidenceInput::new(
                format!("signal-{}", signal_id(&admitted.record)),
                "application/json",
                Sensitivity::Confidential,
                "standard",
                SecretBytes::new(admitted.externalize.clone()),
            )
            .map_err(|_| execution_state("the signal envelope cannot be sealed", "/signal"))?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .build()
                .map_err(|_| execution_state("the sealing runtime could not start", "/keyring"))?;
            let sealed = runtime
                .block_on(protector.seal(scope.clone(), input))
                .map_err(|_| {
                    execution_state(
                        "the envelope could not be sealed; nothing was recorded",
                        "/keyring",
                    )
                })?;
            Some(sealed)
        }
        None => None,
    };

    let stream_id = OpaqueId::parse(&stream)
        .map_err(|_| execution_state("the stream identifier is not wire-safe", "/execution"))?;
    let evidence_refs = sealed
        .as_ref()
        .map(|item| vec![item.reference().clone()])
        .unwrap_or_default();
    let event = NewEvent::new(
        key,
        actor,
        Sensitivity::Internal,
        EventKind::SignalRecorded(admitted.record.clone()),
        evidence_refs,
        vec![],
    );
    match sealed {
        Some(item) => {
            let next_sequence = store
                .next_sequence(&scope, stream_id.as_str())
                .map_err(|error| repository_failure(&error))?;
            let request = graphhelm_events::PreparedAppend::new(
                scope.clone(),
                stream_id.clone(),
                next_sequence,
                vec![event],
                vec![item],
                vec![],
            )
            .map_err(|error| repository_failure(&error))?;
            store
                .append_atomic(&request)
                .map_err(|error| repository_failure(&error))?;
        }
        None => append_event(&store, &scope, &stream_id, event)?,
    }

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
