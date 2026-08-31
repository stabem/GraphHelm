//! Durable memory-admission refusal events (#220 safe slice).

use graphhelm_protocols::{
    EventKind, MemoryAdmissionLocal, MemoryAdmissionRefusalCode, MemoryAdmissionRefused, NewEvent,
    OpaqueId, PersistedActor, RepositoryScope, Sensitivity,
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
