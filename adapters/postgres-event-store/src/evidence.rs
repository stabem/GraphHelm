use graphhelm_events::{EventRepositoryError, EvidenceRead, EvidenceUnavailableReason};
use graphhelm_protocols::{EvidenceId, RepositoryScope};
use sqlx::Row;

use crate::{PostgresEventStore, rows::StoredEvidence};

pub(crate) async fn get(
    store: &PostgresEventStore,
    scope_value: RepositoryScope,
    evidence_id: EvidenceId,
) -> Result<EvidenceRead, EventRepositoryError> {
    let mut transaction = store.pool().begin().await.map_err(crate::error::storage)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *transaction)
        .await
        .map_err(crate::error::storage)?;
    store.set_scope(&mut transaction, &scope_value).await?;
    let row =
        sqlx::query("SELECT record,state FROM public.graphhelm_evidence WHERE evidence_id=$1")
            .bind(evidence_id.as_str())
            .fetch_optional(&mut *transaction)
            .await
            .map_err(crate::error::storage)?
            .ok_or(EventRepositoryError::Invalid)?;
    let state: String = row.try_get("state").map_err(crate::error::decode)?;
    let result = match state.as_str() {
        "available" => {
            let value = row.try_get("record").map_err(crate::error::decode)?;
            let stored: StoredEvidence =
                serde_json::from_value(value).map_err(|_| EventRepositoryError::Integrity)?;
            let sealed = stored.into_sealed()?;
            if sealed.scope() != &scope_value || sealed.reference().evidence_id() != &evidence_id {
                return Err(EventRepositoryError::Integrity);
            }
            graphhelm_events::validate_sealed_evidence(&sealed)?;
            EvidenceRead::Available(sealed)
        }
        "erasure_pending" => EvidenceRead::Unavailable(EvidenceUnavailableReason::ErasurePending),
        "erased" => EvidenceRead::Unavailable(EvidenceUnavailableReason::Erased),
        "expired" => EvidenceRead::Unavailable(EvidenceUnavailableReason::Expired),
        "missing_key" => EvidenceRead::Unavailable(EvidenceUnavailableReason::MissingKey),
        "integrity_failed" => EvidenceRead::Unavailable(EvidenceUnavailableReason::IntegrityFailed),
        _ => return Err(EventRepositoryError::Integrity),
    };
    transaction.commit().await.map_err(crate::error::storage)?;
    Ok(result)
}
