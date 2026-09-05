//! Durable memory lifecycle events (#220).

use graphhelm_protocols::{
    EventKind, MemoryAdmissionLocal, MemoryAdmissionRefusalCode, MemoryAdmissionRefused, NewEvent,
    OpaqueId, PersistedActor, PersistedMemoryPublicationState,
    PersistedMemoryPublicationTransition, PersistedMemorySemanticState,
    PersistedSupersessionReason, RepositoryScope, Sensitivity,
};

use crate::{EventRepositoryError, PreparedAppend};

/// Complete bounded input for one atomic memory-admission refusal append.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryAdmissionRefusalAppend {
    scope: RepositoryScope,
    stream_id: OpaqueId,
    expected_next_sequence: u64,
    idempotency_key: OpaqueId,
    actor: PersistedActor,
    code: MemoryAdmissionRefusalCode,
    local: MemoryAdmissionLocal,
    bytes: u64,
}

impl MemoryAdmissionRefusalAppend {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        scope: RepositoryScope,
        stream_id: OpaqueId,
        expected_next_sequence: u64,
        idempotency_key: OpaqueId,
        actor: PersistedActor,
        code: MemoryAdmissionRefusalCode,
        local: MemoryAdmissionLocal,
        bytes: u64,
    ) -> Self {
        Self {
            scope,
            stream_id,
            expected_next_sequence,
            idempotency_key,
            actor,
            code,
            local,
            bytes,
        }
    }
}

/// Builds the exact append request for a refusal without accepting rejected content or its digest.
///
/// The caller must append the returned request through [`crate::EventRepository::append_atomic`].
pub fn prepare_memory_admission_refusal(
    refusal: MemoryAdmissionRefusalAppend,
) -> Result<PreparedAppend, EventRepositoryError> {
    PreparedAppend::new(
        refusal.scope,
        refusal.stream_id,
        refusal.expected_next_sequence,
        vec![NewEvent::new(
            refusal.idempotency_key,
            refusal.actor,
            Sensitivity::Internal,
            EventKind::MemoryAdmissionRefused(MemoryAdmissionRefused {
                code: refusal.code,
                local: refusal.local,
                bytes: refusal.bytes,
            }),
            vec![],
            vec![],
        )],
        vec![],
        vec![],
    )
}

/// Complete bounded input for one atomic memory publication-transition append.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryPublicationTransitionAppend {
    scope: RepositoryScope,
    stream_id: OpaqueId,
    expected_next_sequence: u64,
    idempotency_key: OpaqueId,
    actor: PersistedActor,
    record_id: OpaqueId,
    transition: PersistedMemoryPublicationTransition,
    resulting_state: PersistedMemoryPublicationState,
}

impl MemoryPublicationTransitionAppend {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        scope: RepositoryScope,
        stream_id: OpaqueId,
        expected_next_sequence: u64,
        idempotency_key: OpaqueId,
        actor: PersistedActor,
        record_id: OpaqueId,
        transition: PersistedMemoryPublicationTransition,
        resulting_state: PersistedMemoryPublicationState,
    ) -> Self {
        Self {
            scope,
            stream_id,
            expected_next_sequence,
            idempotency_key,
            actor,
            record_id,
            transition,
            resulting_state,
        }
    }
}

/// Builds the exact append request for a memory record's publication-axis move.
///
/// The caller must append the returned request through [`crate::EventRepository::append_atomic`],
/// and must call it only after the transition has already been applied in memory (ADR-032): this
/// function persists whatever `(record_id, transition, resulting_state)` it is given, and does not
/// re-derive or re-validate the transition matrix itself.
pub fn prepare_memory_publication_transition(
    move_: MemoryPublicationTransitionAppend,
) -> Result<PreparedAppend, EventRepositoryError> {
    PreparedAppend::new(
        move_.scope,
        move_.stream_id,
        move_.expected_next_sequence,
        vec![NewEvent::new(
            move_.idempotency_key,
            move_.actor,
            Sensitivity::Internal,
            EventKind::MemoryPublicationTransitioned(
                graphhelm_protocols::MemoryPublicationTransitioned {
                    record_id: move_.record_id,
                    transition: move_.transition,
                    resulting_state: move_.resulting_state,
                },
            ),
            vec![],
            vec![],
        )],
        vec![],
        vec![],
    )
}

/// Complete bounded input for one atomic memory-record-supersession append.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryRecordSupersededAppend {
    scope: RepositoryScope,
    stream_id: OpaqueId,
    expected_next_sequence: u64,
    idempotency_key: OpaqueId,
    actor: PersistedActor,
    predecessor_id: OpaqueId,
    successor_id: OpaqueId,
    reason: PersistedSupersessionReason,
    predecessor_new_semantic_state: PersistedMemorySemanticState,
}

impl MemoryRecordSupersededAppend {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        scope: RepositoryScope,
        stream_id: OpaqueId,
        expected_next_sequence: u64,
        idempotency_key: OpaqueId,
        actor: PersistedActor,
        predecessor_id: OpaqueId,
        successor_id: OpaqueId,
        reason: PersistedSupersessionReason,
        predecessor_new_semantic_state: PersistedMemorySemanticState,
    ) -> Self {
        Self {
            scope,
            stream_id,
            expected_next_sequence,
            idempotency_key,
            actor,
            predecessor_id,
            successor_id,
            reason,
            predecessor_new_semantic_state,
        }
    }
}

/// Builds the exact append request for a memory record supersession.
///
/// The caller must append the returned request through [`crate::EventRepository::append_atomic`],
/// and must call it only after `supersede` has already succeeded in memory (ADR-032): this
/// function persists whatever it is given, the same division of labor
/// [`prepare_memory_publication_transition`] keeps between deciding a transition and persisting
/// one.
pub fn prepare_memory_record_superseded(
    supersession: MemoryRecordSupersededAppend,
) -> Result<PreparedAppend, EventRepositoryError> {
    PreparedAppend::new(
        supersession.scope,
        supersession.stream_id,
        supersession.expected_next_sequence,
        vec![NewEvent::new(
            supersession.idempotency_key,
            supersession.actor,
            Sensitivity::Internal,
            EventKind::MemoryRecordSuperseded(graphhelm_protocols::MemoryRecordSuperseded {
                predecessor_id: supersession.predecessor_id,
                successor_id: supersession.successor_id,
                reason: supersession.reason,
                predecessor_new_semantic_state: supersession.predecessor_new_semantic_state,
            }),
            vec![],
            vec![],
        )],
        vec![],
        vec![],
    )
}
