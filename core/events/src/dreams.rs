//! Append-only Dreams shadow results.

use graphhelm_protocols::{
    DreamShadowRecorded, EventKind, NewEvent, OpaqueId, PersistedActor, RepositoryScope,
    Sensitivity,
};

use crate::{EventRepositoryError, PreparedAppend};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DreamShadowAppend {
    scope: RepositoryScope,
    stream_id: OpaqueId,
    expected_next_sequence: u64,
    idempotency_key: OpaqueId,
    actor: PersistedActor,
    result: DreamShadowRecorded,
}

impl DreamShadowAppend {
    #[must_use]
    pub fn new(
        scope: RepositoryScope,
        stream_id: OpaqueId,
        expected_next_sequence: u64,
        idempotency_key: OpaqueId,
        actor: PersistedActor,
        result: DreamShadowRecorded,
    ) -> Self {
        Self {
            scope,
            stream_id,
            expected_next_sequence,
            idempotency_key,
            actor,
            result,
        }
    }
}

pub fn prepare_dream_shadow(
    append: DreamShadowAppend,
) -> Result<PreparedAppend, EventRepositoryError> {
    PreparedAppend::new(
        append.scope,
        append.stream_id,
        append.expected_next_sequence,
        vec![NewEvent::new(
            append.idempotency_key,
            append.actor,
            Sensitivity::Internal,
            EventKind::DreamShadowRecorded(append.result),
            vec![],
            vec![],
        )],
        vec![],
        vec![],
    )
}
