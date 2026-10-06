use std::path::{Path, PathBuf};

use graphhelm_events::{
    EvidenceInput, EvidenceProtector, EvidenceSealer, SealedEvidence, SecretBytes,
};
use graphhelm_graph::raw_content_sha256;
use graphhelm_sealed_key_provider::SealedKeyProvider;

use graphhelm_governor::{
    GovernanceError, MutationDecision, RejectionReason, admit_signal, decide_mutation,
};
use graphhelm_protocols::{
    DraftProposed, EventKind, NewEvent, OpaqueId, PersistedActor, Sensitivity, SignalRecorded,
};

use super::attachments::{ImageAttachment, MAX_ATTACHMENTS};
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
/// The keyring coordinates (Milestone 05d Task 6). The API seam passes `None` — HTTP-side
/// sealing lands with the serve keyring wiring.
///
/// #135: the pair stays required. What changed is that the refusal now names the command that
/// mints a keyring, because the operator who filed that issue was blocked by not knowing it rather
/// than by the requirement. See `sealing`.
pub(crate) struct SignalKeyring {
    pub(crate) directory: PathBuf,
    pub(crate) key_id: String,
}

pub fn run(
    events: &Path,
    execution: Option<&str>,
    signal: &Path,
    evidence_out: &Path,
    keyring: Option<&Path>,
    key_id: Option<&str>,
    attach: &[PathBuf],
) -> Outcome {
    finish(
        COMMAND,
        sealing(keyring, key_id).and_then(|sealing| {
            run_from_file(events, execution, signal, evidence_out, &sealing, attach)
        }),
        |value| value,
    )
}

/// THE REFUSAL CARRIES ITS REMEDY (#135).
///
/// The pair is still required. It is `Option` here only so this function produces the refusal
/// rather than clap, because clap can say which arguments are missing and cannot say what to do
/// about it — and that sentence is the whole fix.
///
/// #135 reported that a keyless store could not record a signal at all. Measured, that is not a
/// capability gap: `gateway keyring init` mints exactly the keyring this command opens, and the
/// signal then records, sealed, on a store created without one. Two commands. The operator was
/// blocked by not knowing the first, which is a discoverability defect and is repaired by writing
/// the reason where the refusal is — the second remedy the issue itself offered.
///
/// The first attempt at this fix made the pair optional behind `--unsealed`. It was withdrawn: it
/// reversed the rule recorded in the real-executor plan
/// ("refusing to run without a keyring rather than silently skipping the seal") to solve a problem
/// that already had a compliant answer, and — because this command reads invocation flags and never
/// the store — it also let an operator on a SEALED store skip the seal by forgetting two flags.
///
/// Half a pair stays refused for the older reason: `--keyring` with no `--key-id` asks for a seal
/// without naming a key, and the only things that could do are seal under a guessed identity or
/// skip the seal.
fn sealing(keyring: Option<&Path>, key_id: Option<&str>) -> Result<SignalKeyring, Failure> {
    match (keyring, key_id) {
        (Some(directory), Some(key_id)) => Ok(SignalKeyring {
            directory: directory.to_path_buf(),
            key_id: key_id.to_owned(),
        }),
        (None, None) => Err(argument(
            "this signal would be recorded with no seal. If this store has no keyring, make one: \
             create an empty directory, then run `gateway keyring init --keyring <dir> --key-id \
             <id>` with GRAPHHELM_EVENTS_KEY set to 64 lowercase hex characters, then pass the \
             same \"keyring\" and \"key-id\" here. That command does not create the directory",
            "/keyring",
        )),
        (Some(_), None) => Err(argument(
            "\"keyring\" was given without \"key-id\", so the envelope could only be sealed under a \
             key nobody named; give \"key-id\" as well",
            "/keyId",
        )),
        (None, Some(_)) => Err(argument(
            "\"key-id\" was given without \"keyring\", so there is no keyring to find that key in; \
             give \"keyring\" as well",
            "/keyring",
        )),
    }
}

fn run_from_file(
    events: &Path,
    execution: Option<&str>,
    signal: &Path,
    evidence_out: &Path,
    keyring: &SignalKeyring,
    attach: &[PathBuf],
) -> Result<serde_json::Value, Failure> {
    let raw = std::fs::read(signal)
        .map_err(|_| argument("--signal does not name a readable file", "/signal"))?;
    let attachments = super::attachments::from_files(attach)?;
    execute(
        events,
        execution,
        &raw,
        Some(evidence_out),
        owner_actor(),
        idempotency_key("signal-recorded"),
        Some(keyring),
        &attachments,
    )
}

/// Mirrors `commands::events::config`'s provider construction: the 32-byte key never rides a
/// flag — it arrives out of band via `GRAPHHELM_EVENTS_KEY` as 64 lowercase hex characters.
///
/// `pub(crate)` so `serve` can pre-flight the same open at startup when `--keyring` is given
/// (PR #1070 review): before that, a missing variable or a wrong `--key-id` produced a healthy
/// `serve.started` and surfaced only on the first message send.
pub(crate) fn open_sealer(
    keyring: &SignalKeyring,
) -> Result<EvidenceProtector<SealedKeyProvider>, Failure> {
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
/// `evidence_out` is OPTIONAL, and the condition on it is the whole point.
///
/// The envelope's raw bytes must be durable BEFORE the append - an event whose evidence was not
/// preserved is a record with no backing, and that rule is fail-closed in both directions below.
/// A path satisfies it. So does the Evidence store, when a keyring is configured: the seal happens
/// before the append under the same rule, and `envelope_sha256` digests exactly those bytes.
///
/// So `None` is admissible only WITH sealing, and the check is the first thing this does - before
/// the store is opened, before anything is read, because a caller who can preserve nothing must
/// learn that before the work rather than after it. A browser is exactly that caller: it has no
/// path on the Runtime's host, and until this it could not record a signal at all.
#[allow(clippy::too_many_arguments)]
pub(crate) fn execute(
    events: &Path,
    execution: Option<&str>,
    signal: &[u8],
    evidence_out: Option<&Path>,
    actor: PersistedActor,
    key: OpaqueId,
    sealing: Option<&SignalKeyring>,
    attachments: &[ImageAttachment],
) -> Result<serde_json::Value, Failure> {
    execute_authenticated(
        events,
        execution,
        signal,
        evidence_out,
        actor,
        key,
        sealing,
        false,
        attachments,
    )
}

/// The transport supplies the scoped authentication result; the signal body cannot assert it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn execute_authenticated(
    events: &Path,
    execution: Option<&str>,
    signal: &[u8],
    evidence_out: Option<&Path>,
    actor: PersistedActor,
    key: OpaqueId,
    sealing: Option<&SignalKeyring>,
    scoped_agent_authenticated: bool,
    attachments: &[ImageAttachment],
) -> Result<serde_json::Value, Failure> {
    execute_authenticated_inner(
        events,
        execution,
        signal,
        evidence_out,
        actor,
        key,
        sealing,
        scoped_agent_authenticated,
        false,
        attachments,
    )
}

/// Runtime-only observer path for native chat receipts. It uses the same sealing and append
/// implementation as generic signals while keeping the reserved kinds out of the public signal
/// transport.
#[allow(clippy::too_many_arguments)]
pub(crate) fn execute_native_observer(
    events: &Path,
    execution: &str,
    signal: &[u8],
    actor: PersistedActor,
    key: OpaqueId,
    sealing: &SignalKeyring,
) -> Result<serde_json::Value, Failure> {
    execute_authenticated_inner(
        events,
        Some(execution),
        signal,
        None,
        actor,
        key,
        Some(sealing),
        false,
        true,
        &[],
    )
}

#[allow(clippy::too_many_arguments)]
fn execute_authenticated_inner(
    events: &Path,
    execution: Option<&str>,
    signal: &[u8],
    evidence_out: Option<&Path>,
    actor: PersistedActor,
    key: OpaqueId,
    sealing: Option<&SignalKeyring>,
    scoped_agent_authenticated: bool,
    allow_native: bool,
    attachments: &[ImageAttachment],
) -> Result<serde_json::Value, Failure> {
    if evidence_out.is_none() && sealing.is_none() {
        return Err(argument(
            "this Runtime has no keyring, so the signal envelope can only be preserved as a file; \
             give \"evidenceOut\", or start the Runtime with --keyring and --key-id",
            "/evidenceOut",
        ));
    }
    // Images are only ever kept sealed Confidential; an operator file cannot hold them (#313).
    if !attachments.is_empty() && sealing.is_none() {
        return Err(signal_invalid(
            "attachments require a sealed keyring; start the Runtime with --keyring and --key-id",
            "/attachments",
        ));
    }
    if attachments.len() > MAX_ATTACHMENTS {
        return Err(signal_invalid(
            "a signal carries at most 4 attachments",
            "/attachments",
        ));
    }
    let store = event_store(events).map_err(|error| repository_failure(&error))?;
    let (scope, stream, projection) = load_projection(&store, execution)?;

    let raw = signal;
    let envelope: serde_json::Value = serde_json::from_slice(raw)
        .map_err(|_| signal_invalid("the signal envelope is not valid JSON", "/signal"))?;
    if !allow_native
        && envelope
            .get("type")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|kind| kind.starts_with("native_chat_"))
    {
        return Err(signal_invalid(
            "native chat receipts are reserved for the Runtime bridge",
            "/signal/type",
        ));
    }
    super::documents::validate_owner_signal(&envelope, &actor, sealing.is_some())?;
    if matches!(
        envelope.get("type").and_then(serde_json::Value::as_str),
        Some("actor_alias" | "owner_refusal")
    ) {
        let (_, _, history) = super::resolve_stream(&store, execution)?;
        super::owner_records::validate_owner_record(
            &envelope,
            &actor,
            sealing.is_some(),
            &history,
        )?;
    }

    // This reserved signal is a typed, sealed ledger record, not arbitrary prose. Validate before
    // any evidence file write or append so the generic signal command cannot bypass its contract.
    if envelope.get("type").and_then(serde_json::Value::as_str) == Some("node_delivery") {
        if sealing.is_none() {
            return Err(signal_invalid(
                "node deliveries require a sealed keyring",
                "/keyring",
            ));
        }
        let source = &envelope["source"];
        let node = source["id"].as_str().ok_or_else(|| {
            signal_invalid("a delivery must name its source node", "/signal/source")
        })?;
        // A held run has a declared graph but no lifecycle outcomes yet. Both the declared
        // shape and later event-derived nodes establish membership; arbitrary IDs do not.
        let belongs = projection.node_states.contains_key(node)
            || projection.declared_form.as_ref().is_some_and(|form| {
                form.node_ids
                    .iter()
                    .any(|declared| declared.as_str() == node)
            });
        if source["type"].as_str() != Some("node") || !belongs {
            return Err(signal_invalid(
                "the delivery source must be a node in this execution",
                "/signal/source",
            ));
        }
        let description = envelope["description"].as_str().ok_or_else(|| {
            signal_invalid(
                "a delivery must carry its structured record",
                "/signal/description",
            )
        })?;
        let delivery = super::delivery::parse_record(description.as_bytes())?;
        super::delivery::validate_graph_binding(&delivery, &projection)?;
    }

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
            //
            // THIS BRANCH IS BEFORE THE SEAL, and it is reached because `admit_signal` refused -
            // so there is no `externalize` to seal and a path is the only place these bytes can
            // go. A caller without one is told exactly that, rather than being told the identity
            // was the problem while their envelope evaporated.
            let Some(evidence_out) = evidence_out else {
                return Err(signal_unrecordable(
                    "the signal's id or source id cannot be represented on the wire, and with no \
                     \"evidenceOut\" there is nowhere to preserve the envelope; nothing was kept",
                    "/signal",
                ));
            };
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
    //
    // Skipped only when no path was given, which the guard at the top of this function has already
    // established means a keyring is configured - so the seal below carries the same guarantee.
    // The rule never relaxes; only which copy satisfies it.
    if let Some(evidence_out) = evidence_out {
        std::fs::write(evidence_out, &admitted.externalize).map_err(|_| {
            execution_state(
                "the evidence could not be written; nothing was recorded",
                "/evidenceOut",
            )
        })?;
    }

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
            let mut images = Vec::with_capacity(attachments.len());
            for (index, image) in attachments.iter().enumerate() {
                let input = EvidenceInput::new(
                    image_evidence_id(&admitted.record, index),
                    image.media_type,
                    Sensitivity::Confidential,
                    "standard",
                    SecretBytes::new(image.bytes.clone()),
                )
                .map_err(|_| execution_state("an attachment cannot be sealed", "/attachments"))?;
                images.push(
                    runtime
                        .block_on(protector.seal(scope.clone(), input))
                        .map_err(|_| {
                            execution_state(
                                "an attachment could not be sealed; nothing was recorded",
                                "/attachments",
                            )
                        })?,
                );
            }
            Some((sealed, images))
        }
        None => None,
    };

    let stream_id = OpaqueId::parse(&stream)
        .map_err(|_| execution_state("the stream identifier is not wire-safe", "/execution"))?;
    if admitted.proposal.is_some() && sealed.is_none() {
        return Err(execution_state(
            "a graph proposal requires sealed evidence; start the Runtime with --keyring and --key-id",
            "/signal/proposal",
        ));
    }
    // Envelope first, then images in request order (#313).
    let evidence_refs: Vec<_> = sealed
        .iter()
        .flat_map(|(item, images)| std::iter::once(item).chain(images))
        .map(|item| item.reference().clone())
        .collect();
    let attachment_reply: Vec<_> = sealed
        .iter()
        .flat_map(|(_, images)| images)
        .zip(attachments)
        .map(|(item, image)| {
            serde_json::json!({
                "evidenceId": item.reference().evidence_id().as_str(),
                "mediaType": image.media_type,
                "bytes": image.bytes.len(),
            })
        })
        .collect();
    let mut recorded = admitted.record.clone();
    recorded.scoped_agent_authenticated = (scoped_agent_authenticated
        && actor.actor_type() == graphhelm_protocols::PersistedActorType::Agent)
        .then_some(true);
    let event = NewEvent::new(
        key,
        actor.clone(),
        Sensitivity::Internal,
        EventKind::SignalRecorded(recorded),
        evidence_refs.clone(),
        vec![],
    );
    let mut pending = vec![event];
    let mut proposal_sealed: Option<SealedEvidence> = None;
    if let Some(draft) = admitted.proposal.as_ref() {
        if !admitted.may_propose_mutation {
            return Err(signal_invalid(
                "an unrecognized signal cannot carry a graph proposal",
                "/signal/proposal",
            ));
        }
        let proposal_bytes = serde_json::to_vec(draft).map_err(|_| {
            signal_invalid("the graph proposal is not serializable", "/signal/proposal")
        })?;
        let proposal_sha256 = raw_content_sha256(&proposal_bytes).map_err(|_| {
            execution_state(
                "the graph proposal digest could not be computed",
                "/signal/proposal",
            )
        })?;
        let protector = open_sealer(sealing.ok_or_else(|| {
            execution_state(
                "a graph proposal requires sealed evidence; start the Runtime with --keyring and --key-id",
                "/signal/proposal",
            )
        })?)?;
        let input = EvidenceInput::new(
            format!("proposal-{}", proposal_sha256.as_str()),
            "application/json",
            Sensitivity::Confidential,
            "standard",
            SecretBytes::new(proposal_bytes),
        )
        .map_err(|_| execution_state("the graph proposal cannot be sealed", "/signal/proposal"))?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .map_err(|_| execution_state("the sealing runtime could not start", "/keyring"))?;
        proposal_sealed = Some(
            runtime
                .block_on(protector.seal(scope.clone(), input))
                .map_err(|_| {
                    execution_state("the graph proposal could not be sealed", "/signal/proposal")
                })?,
        );
        let proposal_refs = vec![
            proposal_sealed
                .as_ref()
                .expect("proposal evidence was assigned above")
                .reference()
                .clone(),
        ];
        let proposal_key = OpaqueId::parse(format!("proposal-{}", proposal_sha256.as_str()))
            .map_err(|_| {
                execution_state(
                    "the graph proposal key is not wire-safe",
                    "/signal/proposal",
                )
            })?;
        pending.push(NewEvent::new(
            proposal_key,
            actor.clone(),
            Sensitivity::Internal,
            EventKind::DraftProposed(DraftProposed {
                draft_id: OpaqueId::parse(&draft.id).map_err(|_| {
                    signal_invalid(
                        "the graph proposal id is not wire-safe",
                        "/signal/proposal/id",
                    )
                })?,
                expected_version: draft.expected_version,
                expected_hash: graphhelm_protocols::WireHash::parse(draft.expected_hash.as_str())
                    .map_err(|_| {
                    signal_invalid(
                        "the graph proposal hash is invalid",
                        "/signal/proposal/expectedHash",
                    )
                })?,
                operation_count: u16::try_from(draft.operations.len()).map_err(|_| {
                    signal_invalid(
                        "the graph proposal has too many operations",
                        "/signal/proposal/operations",
                    )
                })?,
                proposal_sha256: Some(proposal_sha256.clone()),
            }),
            proposal_refs,
            vec![],
        ));
        // Ghost nodes are published only by the Governor after owner approval. Recording them at
        // proposal submission would make an agent signal mutate the operational graph directly.
    }
    match sealed {
        Some((item, images)) => {
            let next_sequence = store
                .next_sequence(&scope, stream_id.as_str())
                .map_err(|error| repository_failure(&error))?;
            let request = graphhelm_events::PreparedAppend::new(
                scope.clone(),
                stream_id.clone(),
                next_sequence,
                pending,
                std::iter::once(item)
                    .chain(images)
                    .chain(proposal_sealed)
                    .collect(),
                vec![],
            )
            .map_err(|error| repository_failure(&error))?;
            store
                .append_atomic(&request)
                .map_err(|error| repository_failure(&error))?;
        }
        None => {
            for event in pending {
                append_event(&store, &scope, &stream_id, event)?;
            }
        }
    }

    // "Right now" per `decide_mutation`'s own contract means the projection folded to the append
    // point; nothing between the read above and this append could have changed `mode` or
    // `accepted_mutations` (the `signal_recorded` fold touches neither), so the projection already
    // in hand is that point.
    let decision = decide_mutation(&projection, &admitted);
    let mut reply = serde_json::json!({
        "executionId": projection.execution_id,
        "signalId": signal_id(&admitted.record),
        "kind": admitted.record.kind,
        "mayProposeMutation": admitted.may_propose_mutation,
        "decision": decision_label(decision),
        "rejectionReason": rejection_label(decision),
    });
    // Absent when empty, so replies without attachments stay byte-identical.
    if !attachment_reply.is_empty() {
        reply["attachments"] = serde_json::Value::Array(attachment_reply);
    }
    Ok(reply)
}

fn signal_id(record: &SignalRecorded) -> &str {
    record.signal_id.as_str()
}

/// `signal-<signalId>-image-<n>`, n from 1: deterministic so the envelope can cite it beforehand.
fn image_evidence_id(record: &SignalRecorded, index: usize) -> String {
    format!("signal-{}-image-{}", signal_id(record), index + 1)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachments_without_a_keyring_are_refused_before_the_store_opens() {
        let dir = tempfile::tempdir().unwrap();
        let image = ImageAttachment {
            media_type: "image/png",
            bytes: vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A],
        };
        let refused = execute(
            &dir.path().join("absent"),
            None,
            b"{}",
            Some(&dir.path().join("evidence.json")),
            owner_actor(),
            OpaqueId::parse("key").ok().unwrap(),
            None,
            &[image],
        )
        .err()
        .unwrap();
        assert_eq!(refused.pointer, "/attachments");
        assert_eq!(refused.code, super::super::SIGNAL_INVALID_CODE);
        assert!(!dir.path().join("absent").exists());
    }
}
