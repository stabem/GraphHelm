use std::collections::{BTreeMap, BTreeSet};

use graphhelm_protocols::{EventEnvelope, EventKind, NewEvent, OpaqueId, RepositoryScope};
use serde::Serialize;

use crate::{
    EventRepositoryError, PreparedAppend, SealedEvidence,
    canonical::{canonical_bytes, event_hash, serialized_len_bounded, wire_sha256},
    evidence::validate_sealed_metadata,
    limits::{MAX_BATCH_BYTES, MAX_EVENT_BYTES},
};

/// Revalidates every sealed Evidence metadata/digest relation after persistence.
pub fn validate_sealed_evidence(value: &SealedEvidence) -> Result<(), EventRepositoryError> {
    validate_sealed_metadata(value).map_err(|_| EventRepositoryError::Integrity)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RequestDigest<'a> {
    scope: &'a RepositoryScope,
    stream_id: &'a OpaqueId,
    expected_next_sequence: u64,
    events: &'a [NewEvent],
    evidence: Vec<EvidenceDigest>,
    artifacts: Vec<ArtifactDigest>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct EvidenceDigest {
    reference: graphhelm_protocols::EvidenceReference,
    scope: RepositoryScope,
    media_type: String,
    sensitivity: graphhelm_protocols::Sensitivity,
    retention_class: String,
    algorithm: String,
    nonce_sha256: String,
    wrapped_key_sha256: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ArtifactDigest {
    reference: graphhelm_protocols::ArtifactReference,
    producer_stream_id: String,
    producer_idempotency_key: String,
}

/// Adapter-neutral committed artifact identity used by append preflight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommittedArtifact {
    pub reference: graphhelm_protocols::ArtifactReference,
    pub producer_stream_id: String,
    pub producer_idempotency_key: String,
}

/// Adapter-neutral active graph identity used for lineage validation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveGraphIdentity {
    number: u64,
    semantic_hash: String,
}

impl ActiveGraphIdentity {
    pub fn new(
        number: u64,
        semantic_hash: impl Into<String>,
    ) -> Result<Self, EventRepositoryError> {
        let semantic_hash = semantic_hash.into();
        let digest = semantic_hash.strip_prefix("sha256:");
        if number == 0
            || number > 9_007_199_254_740_991
            || digest.is_none_or(|value| {
                value.len() != 64
                    || !value
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            })
        {
            return Err(EventRepositoryError::Invalid);
        }
        Ok(Self {
            number,
            semantic_hash,
        })
    }

    #[must_use]
    pub const fn number(&self) -> u64 {
        self.number
    }

    #[must_use]
    pub fn semantic_hash(&self) -> &str {
        &self.semantic_hash
    }
}

/// Complete adapter-neutral static append preflight shared by every repository.
pub fn validate_prepared_append(request: &PreparedAppend) -> Result<(), EventRepositoryError> {
    scan_safe_value(
        &serde_json::to_value(request.scope()).map_err(|_| EventRepositoryError::Invalid)?,
        &[request.stream_id().as_str()],
    )?;
    let keys = request
        .events()
        .iter()
        .map(|event| event.idempotency_key.as_str())
        .collect::<BTreeSet<_>>();
    if keys.len() != request.events().len() {
        return Err(EventRepositoryError::Invalid);
    }
    let mut event_bytes = 0_usize;
    for event in request.events() {
        let length = serialized_len_bounded(event, MAX_EVENT_BYTES)?;
        event_bytes = event_bytes
            .checked_add(length)
            .ok_or(EventRepositoryError::LimitExceeded)?;
        if event_bytes > MAX_BATCH_BYTES {
            return Err(EventRepositoryError::LimitExceeded);
        }
        scan_safe_value(
            &serde_json::to_value(event).map_err(|_| EventRepositoryError::Invalid)?,
            &[],
        )?;
        if event.kind.is_project_level() == request.scope().execution_id().is_some() {
            return Err(EventRepositoryError::Invalid);
        }
        if event.evidence_refs.len() > 8192 || event.artifact_refs.len() > 64 {
            return Err(EventRepositoryError::LimitExceeded);
        }
        if let EventKind::GraphVersionPublished(payload) = &event.kind
            && (graphhelm_graph::validate_persisted_projection(&payload.version).is_err()
                || graphhelm_graph::validate_publication_evidence_ids(
                    request.scope(),
                    &payload.version,
                )
                .is_err()
                || graphhelm_graph::validate_evidence_bijection(
                    payload.version.content_slots(),
                    &event.evidence_refs,
                )
                .is_err()
                || request.scope().execution_id()
                    != Some(payload.version.topology().execution_id())
                || event.actor != *payload.version.created_by())
        {
            return Err(EventRepositoryError::Invalid);
        }
    }

    let prepared_evidence = request
        .evidence()
        .iter()
        .map(|item| (item.reference().evidence_id().as_str(), item.reference()))
        .collect::<BTreeMap<_, _>>();
    if prepared_evidence.len() != request.evidence().len()
        || request
            .evidence()
            .iter()
            .any(|item| item.scope() != request.scope() || validate_sealed_metadata(item).is_err())
    {
        return Err(EventRepositoryError::Invalid);
    }
    for evidence in request.evidence() {
        scan_safe_value(
            &serde_json::to_value(evidence.reference())
                .map_err(|_| EventRepositoryError::Invalid)?,
            &[
                evidence.scope().workspace_id().as_str(),
                evidence.scope().project_id().as_str(),
                evidence
                    .scope()
                    .execution_id()
                    .map_or("", |value| value.as_str()),
                evidence.media_type().as_str(),
                evidence.retention_class(),
                evidence.algorithm(),
                evidence.wrapped_key().key_id(),
                evidence.wrapped_key().handle(),
                evidence.wrapped_key().algorithm(),
                evidence.wrapped_key().aad_sha256().as_str(),
            ],
        )?;
    }
    let referenced_evidence = request
        .events()
        .iter()
        .flat_map(|event| &event.evidence_refs)
        .map(|reference| reference.evidence_id().as_str())
        .collect::<BTreeSet<_>>();
    if !prepared_evidence
        .keys()
        .all(|evidence_id| referenced_evidence.contains(evidence_id))
    {
        return Err(EventRepositoryError::Invalid);
    }
    validate_evidence_references(request.events(), &prepared_evidence, |_| Ok(true))?;

    let registered_artifacts = request
        .artifacts()
        .iter()
        .map(|item| (item.reference().artifact_id().as_str(), item))
        .collect::<BTreeMap<_, _>>();
    let producer_artifacts = request
        .events()
        .iter()
        .flat_map(|event| {
            event.artifact_refs.iter().map(move |reference| {
                (
                    (
                        event.idempotency_key.as_str(),
                        reference.artifact_id().as_str(),
                    ),
                    reference,
                )
            })
        })
        .collect::<BTreeMap<_, _>>();
    let producer_count = request.events().iter().try_fold(0_usize, |total, event| {
        total
            .checked_add(event.artifact_refs.len())
            .ok_or(EventRepositoryError::LimitExceeded)
    })?;
    if registered_artifacts.len() != request.artifacts().len()
        || producer_artifacts.len() != producer_count
        || request.artifacts().iter().any(|item| {
            producer_artifacts.get(&(
                item.producer_idempotency_key().as_str(),
                item.reference().artifact_id().as_str(),
            )) != Some(&item.reference())
        })
    {
        return Err(EventRepositoryError::Invalid);
    }
    for artifact in request.artifacts() {
        scan_safe_value(
            &serde_json::to_value(artifact.reference())
                .map_err(|_| EventRepositoryError::Invalid)?,
            &[artifact.producer_idempotency_key().as_str()],
        )?;
    }
    request_digest(request).map(|_| ())
}

/// Canonical request identity shared by local and PostgreSQL repositories.
pub fn request_digest(request: &PreparedAppend) -> Result<String, EventRepositoryError> {
    let mut evidence = request
        .evidence()
        .iter()
        .map(evidence_digest)
        .collect::<Vec<_>>();
    evidence.sort_by(|left, right| {
        left.reference
            .evidence_id()
            .as_str()
            .cmp(right.reference.evidence_id().as_str())
    });
    let artifacts = request
        .artifacts()
        .iter()
        .map(|item| ArtifactDigest {
            reference: item.reference().clone(),
            producer_stream_id: request.stream_id().to_string(),
            producer_idempotency_key: item.producer_idempotency_key().to_string(),
        })
        .collect();
    let input = RequestDigest {
        scope: request.scope(),
        stream_id: request.stream_id(),
        // `expected_next_sequence` is in this digest ON PURPOSE, and something two crates away
        // depends on it. The wake recorder keys each consumption with the sequence it appends
        // at and reports how many it recorded; that count is honest only because a second
        // attempt at a LATER sequence is refused rather than resolved as a retry against the
        // first attempt's events. Remove this field and that refusal becomes a replay: the
        // recorder reports a count for events it never wrote, and nothing in its own crate
        // notices. The executable statement is
        // `the_same_key_at_a_later_sequence_conflicts_rather_than_replaying`
        // (core/events/tests/local_atomicity.rs). Several other guards fall too, none of them
        // about this field — that is the diagnosability problem this note exists to prevent.
        expected_next_sequence: request.expected_next_sequence(),
        events: request.events(),
        evidence,
        artifacts,
    };
    serialized_len_bounded(&input, MAX_BATCH_BYTES)?;
    Ok(wire_sha256(&canonical_bytes(&input)?))
}

/// Complete envelope schema and safe-content verification shared by repositories.
pub fn validate_envelope(event: &EventEnvelope) -> Result<(), EventRepositoryError> {
    validate_envelope_content(event)?;
    let value = serde_json::to_value(event).map_err(|_| EventRepositoryError::Invalid)?;
    let schemas =
        graphhelm_schema::repository_schema_set().map_err(|_| EventRepositoryError::Invalid)?;
    if !schemas.validate_event(&value).is_empty() {
        return Err(EventRepositoryError::Invalid);
    }
    if event.kind.is_project_level() == event.scope.execution_id().is_some() {
        return Err(EventRepositoryError::Invalid);
    }
    if let Some(evidence_scope) = retention_evidence_scope(&event.kind)
        && (evidence_scope.workspace_id() != event.scope.workspace_id()
            || evidence_scope.project_id() != event.scope.project_id())
    {
        return Err(EventRepositoryError::Invalid);
    }
    if let EventKind::GraphVersionPublished(payload) = &event.kind
        && (event.scope.execution_id() != Some(payload.version.topology().execution_id())
            || event.actor != *payload.version.created_by()
            || graphhelm_graph::validate_publication_evidence_ids(&event.scope, &payload.version)
                .is_err()
            || graphhelm_graph::validate_evidence_bijection(
                payload.version.content_slots(),
                &event.evidence_refs,
            )
            .is_err())
    {
        return Err(EventRepositoryError::Invalid);
    }
    if canonical_bytes(event)?.len() > MAX_EVENT_BYTES {
        return Err(EventRepositoryError::LimitExceeded);
    }
    Ok(())
}

fn retention_evidence_scope(kind: &EventKind) -> Option<&RepositoryScope> {
    match kind {
        EventKind::EvidenceErasureRequested(payload) => Some(&payload.evidence_scope),
        EventKind::EvidenceErasureCompleted(payload) => Some(&payload.evidence_scope),
        EventKind::EvidenceCiphertextDeleted(payload) => Some(&payload.evidence_scope),
        EventKind::EvidenceLegalHoldChanged(payload) => Some(&payload.evidence_scope),
        _ => None,
    }
}

/// Applies the bounded durable-content checks required before hashing an envelope.
pub fn validate_envelope_content(event: &EventEnvelope) -> Result<(), EventRepositoryError> {
    serialized_len_bounded(event, MAX_EVENT_BYTES)?;
    let value = serde_json::to_value(event).map_err(|_| EventRepositoryError::Invalid)?;
    scan_safe_value(&value, &[])
}

/// Recomputes the canonical event hash through the single shared integrity path.
pub fn compute_event_hash(
    event: &EventEnvelope,
    previous_hash: &str,
) -> Result<String, EventRepositoryError> {
    event_hash(event, previous_hash)
}

pub fn validate_evidence_references<'a>(
    events: &'a [NewEvent],
    prepared: &BTreeMap<&'a str, &'a graphhelm_protocols::EvidenceReference>,
    mut verify_committed: impl FnMut(
        &graphhelm_protocols::EvidenceReference,
    ) -> Result<bool, EventRepositoryError>,
) -> Result<(), EventRepositoryError> {
    let mut verified_committed = BTreeMap::new();
    for reference in events.iter().flat_map(|event| &event.evidence_refs) {
        if let Some(prepared_reference) = prepared.get(reference.evidence_id().as_str()) {
            if **prepared_reference != *reference {
                return Err(EventRepositoryError::Invalid);
            }
            continue;
        }
        match verified_committed.get(reference.evidence_id().as_str()) {
            Some(existing) if *existing != reference => return Err(EventRepositoryError::Invalid),
            Some(_) => continue,
            None => {}
        }
        if !verify_committed(reference)? {
            return Err(EventRepositoryError::Invalid);
        }
        verified_committed.insert(reference.evidence_id().as_str(), reference);
    }
    Ok(())
}

/// Validates committed artifact ownership without adapter-specific branches.
pub fn validate_artifact_relations(
    request: &PreparedAppend,
    committed: &BTreeMap<String, CommittedArtifact>,
) -> Result<(), EventRepositoryError> {
    let prepared = request
        .artifacts()
        .iter()
        .map(|item| (item.reference().artifact_id().as_str(), item))
        .collect::<BTreeMap<_, _>>();
    for item in request.artifacts() {
        let expected = CommittedArtifact {
            reference: item.reference().clone(),
            producer_stream_id: request.stream_id().to_string(),
            producer_idempotency_key: item.producer_idempotency_key().to_string(),
        };
        if committed
            .get(item.reference().artifact_id().as_str())
            .is_some_and(|existing| existing != &expected)
        {
            return Err(EventRepositoryError::Invalid);
        }
    }
    for event in request.events() {
        for reference in &event.artifact_refs {
            if !prepared.contains_key(reference.artifact_id().as_str())
                && !committed
                    .get(reference.artifact_id().as_str())
                    .is_some_and(|existing| {
                        existing.producer_stream_id == request.stream_id().as_str()
                            && existing.reference == *reference
                    })
            {
                return Err(EventRepositoryError::Invalid);
            }
        }
    }
    Ok(())
}

/// Validates the full ordered graph-publication lineage after retry resolution.
pub fn validate_graph_lineage(
    request: &PreparedAppend,
    active: Option<&ActiveGraphIdentity>,
) -> Result<(), EventRepositoryError> {
    let mut current = active.cloned();
    for event in request.events() {
        if let EventKind::GraphVersionPublished(payload) = &event.kind {
            match &current {
                Some(active)
                    if !is_graph_successor(
                        &payload.version,
                        active.number,
                        &active.semantic_hash,
                    ) =>
                {
                    return Err(EventRepositoryError::Invalid);
                }
                None if payload.version.number() != 1
                    || payload.version.predecessor().is_some() =>
                {
                    return Err(EventRepositoryError::Invalid);
                }
                _ => {}
            }
            current = Some(ActiveGraphIdentity {
                number: payload.version.number(),
                semantic_hash: payload.version.semantic_hash().to_string(),
            });
        }
    }
    Ok(())
}

/// Checks one exact immutable graph-version successor relation.
pub fn is_graph_successor(
    version: &graphhelm_protocols::PersistedGraphVersion,
    active_number: u64,
    active_semantic_hash: &str,
) -> bool {
    active_number.checked_add(1) == Some(version.number())
        && version.predecessor().is_some_and(|predecessor| {
            predecessor.number() == active_number
                && predecessor.semantic_hash().as_str() == active_semantic_hash
        })
}

fn evidence_digest(item: &SealedEvidence) -> EvidenceDigest {
    let mut wrapped = Vec::new();
    wrapped.extend_from_slice(item.wrapped_key().key_id().as_bytes());
    wrapped.extend_from_slice(item.wrapped_key().handle().as_bytes());
    wrapped.extend_from_slice(item.wrapped_key().nonce());
    wrapped.extend_from_slice(item.wrapped_key().ciphertext());
    EvidenceDigest {
        reference: item.reference().clone(),
        scope: item.scope().clone(),
        media_type: item.media_type().to_string(),
        sensitivity: item.sensitivity(),
        retention_class: item.retention_class().into(),
        algorithm: item.algorithm().into(),
        nonce_sha256: wire_sha256(item.nonce()),
        wrapped_key_sha256: wire_sha256(&wrapped),
    }
}

fn scan_safe_value(
    value: &serde_json::Value,
    strings: &[&str],
) -> Result<(), EventRepositoryError> {
    graphhelm_graph::validate_durable_content(value, strings).map_err(|error| match error {
        graphhelm_graph::DurableContentError::Unsafe => EventRepositoryError::UnsafePersistence,
        graphhelm_graph::DurableContentError::LimitExceeded => EventRepositoryError::LimitExceeded,
    })
}
