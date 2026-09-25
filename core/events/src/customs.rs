//! The customs claim and clear verbs (#159): decide against the replayed projection, then
//! append AT THE SEQUENCE THE DECISION READ.
//!
//! The append position is derived from the SAME history the decision replayed — [`next_after`]
//! — and never from a second store read. A second read (`next_sequence`) would observe a stream
//! that may already have moved past the one the decision was made against, and `append_atomic`
//! would then accept the decision at the moved head: a decision applied to a different world
//! than the one it was made in. With the position taken from the replayed history, a concurrent
//! append between the read and the write makes `append_atomic` answer `SequenceConflict`, and
//! the caller decides again against the world as it now is. A refused claim is a JOURNAL EVENT
//! with a registry code — a graph that cannot finish leaves a legible trail.
//!
//! Replay failures keep their own meaning: a corrupt journal is reported as integrity through
//! `map_replay_error`, never collapsed onto "bad input".

use std::collections::BTreeSet;

use graphhelm_protocols::{
    ClaimAttestation, ClaimEvidence, ClearanceVerifier, CompletionClaimed, CompletionCleared,
    CompletionRefused, EventEnvelope, EventKind, NewEvent, NodeState, OpaqueId, PersistedActor,
    RepositoryScope, SafeCode, Sensitivity, WireHash,
};

use crate::projection::{ClearanceOutcome, CustomsStage, map_replay_error};
use crate::store::EventRepositoryError;
use crate::{ExecutionProjection, LocalEventRepository, PreparedAppend, replay};

/// The sequence an append must carry to land directly after `history` — the history the caller
/// replayed and decided against. `1` for an empty stream, `last.sequence + 1` otherwise;
/// `LimitExceeded` when the stream has no next sequence to give.
///
/// This is what pins the decision to the world it was made in: `append_atomic` refuses with
/// `SequenceConflict` when the stream's real head is not the one this number implies.
fn next_after(history: &[EventEnvelope]) -> Result<u64, EventRepositoryError> {
    match history.last() {
        None => Ok(1),
        Some(last) => last
            .sequence
            .checked_add(1)
            .ok_or(EventRepositoryError::LimitExceeded),
    }
}

/// The refusal codes THIS verb produces, each a member of
/// [`graphhelm_protocols::REFUSAL_REASON_CODES`]. The registry is the closed vocabulary; these
/// are the names the claim verb's decision table reaches, in the order it reaches them.
pub mod refusal {
    /// The node is not parked, so there is nothing to complete.
    pub const NOT_WAITING: &str = "not_waiting";
    /// The named wait was a real wait of this node, and a later park superseded it.
    pub const STALE_RENDEZVOUS: &str = "stale_rendezvous";
    /// No scan of this node ever parked at the named sequence.
    pub const UNKNOWN_WAIT: &str = "unknown_wait";
    /// An open claim already names this node.
    pub const DUPLICATE_COMPLETION: &str = "duplicate_completion";
    /// A declared `proof_kinds` entry is not among the presented evidence kinds.
    pub const EVIDENCE_BUDGET_UNMET: &str = "evidence_budget_unmet";
}

/// What a caller asks the claim verb to journal.
///
/// `required_proof_kinds` is the node's DECLARED `proof_kinds`, read from the graph the caller
/// holds — the projection cannot supply it on the start path, and this verb never opens a graph.
pub struct ClaimRequest<'a> {
    pub node: &'a str,
    /// The exact wait this claim answers. `None` answers the node's OPEN wait — the CLI's
    /// default — and a named sequence that is not the open wait is refused, never redirected.
    pub completes_wait_seq: Option<u64>,
    pub evidence: Vec<ClaimEvidence>,
    pub attestation: ClaimAttestation,
    pub required_proof_kinds: &'a [String],
}

/// The verb's answer. Both arms are JOURNALED: a refusal is an outcome, not an error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClaimOutcome {
    Claimed {
        claim_seq: u64,
        wait_seq: u64,
    },
    Refused {
        reason_code: &'static str,
        /// The wait the refused claim NAMED (or resolved to), as the journal records it.
        wait_seq: u64,
    },
}

#[derive(Debug)]
pub enum ClaimError {
    /// The stream carries no `execution_started`, so there is no execution to claim against.
    NotStarted,
    Repository(EventRepositoryError),
}

#[derive(Debug)]
pub enum ClearError {
    NotStarted,
    /// The named sequence is not an open claim. Refused WITHOUT appending — see [`clear`].
    NotAnOpenClaim {
        claim_seq: u64,
    },
    Repository(EventRepositoryError),
}

impl From<EventRepositoryError> for ClaimError {
    fn from(error: EventRepositoryError) -> Self {
        Self::Repository(error)
    }
}

impl From<EventRepositoryError> for ClearError {
    fn from(error: EventRepositoryError) -> Self {
        Self::Repository(error)
    }
}

/// The decision table, first match wins. `Ok` carries the wait the claim answers; `Err` carries
/// the registry code and the wait the refusal is recorded against — `None` when the claim named
/// no wait and the node has none to resolve it to (see [`claim`] for what is journaled then).
fn decide_claim(
    projection: &ExecutionProjection,
    request: &ClaimRequest<'_>,
) -> Result<u64, (&'static str, Option<u64>)> {
    let node = request.node;
    let state = projection
        .node_states
        .get(node)
        .copied()
        .unwrap_or(NodeState::Draft);
    if state != NodeState::WaitingInput {
        return Err((refusal::NOT_WAITING, request.completes_wait_seq));
    }
    let open = projection.open_waits.get(node).map(|wait| wait.at_sequence);
    let wait_seq = match (request.completes_wait_seq, open) {
        (None, Some(open)) => open,
        (Some(named), Some(open)) if named == open => open,
        (Some(named), _) => {
            // A sequence this node once PARKED at is a superseded rendezvous; any other
            // sequence was never a wait of this node at all. The scan history is the fold's
            // record of every park, so it answers which of the two this is.
            let parked_there = projection.customs_scans.get(node).is_some_and(|scans| {
                scans
                    .iter()
                    .any(|scan| scan.stage == CustomsStage::Parked && scan.at_sequence == named)
            });
            return Err((
                if parked_there {
                    refusal::STALE_RENDEZVOUS
                } else {
                    refusal::UNKNOWN_WAIT
                },
                Some(named),
            ));
        }
        (None, None) => return Err((refusal::UNKNOWN_WAIT, None)),
    };
    if projection
        .open_claims
        .values()
        .any(|claim| claim.node == node)
    {
        return Err((refusal::DUPLICATE_COMPLETION, Some(wait_seq)));
    }
    let presented: BTreeSet<&str> = request
        .evidence
        .iter()
        .map(|item| item.kind.as_str())
        .collect();
    if request
        .required_proof_kinds
        .iter()
        .any(|kind| !presented.contains(kind.as_str()))
    {
        return Err((refusal::EVIDENCE_BUDGET_UNMET, Some(wait_seq)));
    }
    Ok(wait_seq)
}

/// Journal a completion claim, or the refusal of one.
///
/// Returns the outcome and the batch that was appended, so a caller — or a test — reads the
/// sequence the journal actually assigned rather than being told about it.
#[allow(clippy::missing_errors_doc)]
pub fn claim(
    repository: &LocalEventRepository,
    scope: &RepositoryScope,
    stream: &str,
    actor: &PersistedActor,
    key: &OpaqueId,
    request: ClaimRequest<'_>,
) -> Result<(ClaimOutcome, Vec<EventEnvelope>), ClaimError> {
    let history = repository.read_replay_stream(scope, stream)?;
    let projection = replay(scope, stream, &history).map_err(map_replay_error)?;
    let execution_id = projection
        .execution_id
        .as_deref()
        .and_then(|id| OpaqueId::parse(id).ok())
        .ok_or(ClaimError::NotStarted)?;
    let node_id = OpaqueId::parse(request.node).map_err(|_| EventRepositoryError::Invalid)?;
    let at = next_after(&history)?;
    // A refusal that resolved to NO wait — nothing named, nothing open — still has to be
    // journaled, and the wire requires `claimedWaitSeq` to be a positive sequence (no schema
    // change in this slice, D7). It names its OWN sequence: no wait can carry that number, so a
    // reader cannot mistake the record for a rendezvous, and the trail stays legible.
    let decision = decide_claim(&projection, &request)
        .map_err(|(code, wait_seq)| (code, wait_seq.unwrap_or(at)));
    let kind = match &decision {
        Ok(wait_seq) => EventKind::CompletionClaimed(CompletionClaimed {
            execution_id,
            node: node_id,
            completes_wait_seq: *wait_seq,
            evidence: request.evidence.clone(),
            attestation: request.attestation.clone(),
        }),
        Err((code, wait_seq)) => EventKind::CompletionRefused(CompletionRefused {
            execution_id,
            node: node_id,
            claimed_wait_seq: *wait_seq,
            reason_code: SafeCode::parse(*code).expect("registry codes are valid SafeCodes"),
        }),
    };
    let appended = append_one(repository, scope, stream, at, key, actor, kind)?;
    let claim_seq = appended
        .first()
        .map(|envelope| envelope.sequence)
        .ok_or(EventRepositoryError::Invalid)?;
    let outcome = match decision {
        Ok(wait_seq) => ClaimOutcome::Claimed {
            claim_seq,
            wait_seq,
        },
        Err((reason_code, wait_seq)) => ClaimOutcome::Refused {
            reason_code,
            wait_seq,
        },
    };
    Ok((outcome, appended))
}

/// Journal a machine-replay clearance for an open claim and return the FOLD's verdict on it.
#[allow(clippy::missing_errors_doc)]
pub fn clear(
    repository: &LocalEventRepository,
    scope: &RepositoryScope,
    stream: &str,
    actor: &PersistedActor,
    key: &OpaqueId,
    claim_seq: u64,
    manifest_hash: &WireHash,
) -> Result<(ClearanceOutcome, Vec<EventEnvelope>), ClearError> {
    let history = repository.read_replay_stream(scope, stream)?;
    let projection = replay(scope, stream, &history).map_err(map_replay_error)?;
    let execution_id = projection
        .execution_id
        .as_deref()
        .and_then(|id| OpaqueId::parse(id).ok())
        .ok_or(ClearError::NotStarted)?;
    // REFUSED WITHOUT APPENDING: the fold reads a clearance with no claim under it as Corrupt, and a
    // verb that journaled one would poison every later replay of this stream.
    if !projection.open_claims.contains_key(&claim_seq) {
        return Err(ClearError::NotAnOpenClaim { claim_seq });
    }
    let at = next_after(&history)?;
    let kind = EventKind::CompletionCleared(CompletionCleared {
        execution_id,
        claim_seq,
        verifier: ClearanceVerifier::MachineReplay {
            manifest_hash: manifest_hash.clone(),
        },
    });
    let appended = append_one(repository, scope, stream, at, key, actor, kind)?;
    // The VERDICT is the fold's, read back from the journal that now holds the clearance — never
    // recomputed here, which would be a second oracle.
    let mut full = history;
    full.extend(appended.iter().cloned());
    let after = replay(scope, stream, &full).map_err(map_replay_error)?;
    let outcome = after
        .clearances
        .get(&claim_seq)
        .cloned()
        .ok_or(EventRepositoryError::Invalid)?;
    Ok((outcome, appended))
}

fn append_one(
    repository: &LocalEventRepository,
    scope: &RepositoryScope,
    stream: &str,
    at: u64,
    key: &OpaqueId,
    actor: &PersistedActor,
    kind: EventKind,
) -> Result<Vec<EventEnvelope>, EventRepositoryError> {
    let event = NewEvent::new(
        key.clone(),
        actor.clone(),
        Sensitivity::Internal,
        kind,
        Vec::new(),
        Vec::new(),
    );
    let request = PreparedAppend::new(
        scope.clone(),
        OpaqueId::parse(stream).map_err(|_| EventRepositoryError::Invalid)?,
        at,
        vec![event],
        Vec::new(),
        Vec::new(),
    )?;
    repository.append_atomic(&request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use graphhelm_protocols::{
        ActorId, EventHash, ExecutionId, PersistedActorType, PersistedTimestamp, ProjectId,
        WorkspaceId,
    };

    /// An envelope whose only meaningful field is its sequence: `next_after` reads nothing else.
    fn envelope_at(sequence: u64) -> EventEnvelope {
        let scope = RepositoryScope::new(
            WorkspaceId::parse("workspace-test").unwrap(),
            ProjectId::parse("project-test").unwrap(),
            Some(ExecutionId::parse("execution-test").unwrap()),
        );
        let actor = PersistedActor::new(
            PersistedActorType::System,
            ActorId::parse("system-test").unwrap(),
        );
        let event = NewEvent::new(
            OpaqueId::parse(format!("key-{sequence}")).unwrap(),
            actor,
            Sensitivity::Internal,
            EventKind::ExecutionPaused(graphhelm_protocols::ExecutionPaused {
                execution_id: OpaqueId::parse("execution-test").unwrap(),
            }),
            Vec::new(),
            Vec::new(),
        );
        EventEnvelope::new(
            OpaqueId::parse(format!("event-{sequence}")).unwrap(),
            scope,
            OpaqueId::parse("stream-execution-test").unwrap(),
            sequence,
            PersistedTimestamp::parse("2026-08-10T12:00:00Z").unwrap(),
            event,
            EventHash::parse(format!("sha256:{}", "0".repeat(64))).unwrap(),
            EventHash::parse(format!("sha256:{}", "1".repeat(64))).unwrap(),
        )
    }

    #[test]
    fn an_empty_history_appends_at_one() {
        assert_eq!(next_after(&[]).unwrap(), 1);
    }

    #[test]
    fn a_history_ending_at_n_appends_at_n_plus_one() {
        let history = [envelope_at(1), envelope_at(2), envelope_at(7)];
        assert_eq!(next_after(&history).unwrap(), 8);
    }

    #[test]
    fn a_history_at_the_last_sequence_has_no_next_and_says_so() {
        let history = [envelope_at(u64::MAX)];
        assert!(matches!(
            next_after(&history),
            Err(EventRepositoryError::LimitExceeded)
        ));
    }
}
