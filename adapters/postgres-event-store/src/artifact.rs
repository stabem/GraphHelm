use graphhelm_events::EventRepositoryError;
use graphhelm_protocols::{ArtifactId, ArtifactReference, RepositoryScope};
use sqlx::Row;

use crate::PostgresEventStore;

pub(crate) async fn resolve(
    store: &PostgresEventStore,
    scope_value: RepositoryScope,
    artifact_id: ArtifactId,
) -> Result<Option<ArtifactReference>, EventRepositoryError> {
    let mut transaction = store.pool().begin().await.map_err(crate::error::storage)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *transaction)
        .await
        .map_err(crate::error::storage)?;
    store.set_scope(&mut transaction, &scope_value).await?;
    let row = sqlx::query("SELECT reference FROM public.graphhelm_artifacts WHERE artifact_id=$1")
        .bind(artifact_id.as_str())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(crate::error::storage)?;
    let result = row
        .map(|row| {
            let value = row.try_get("reference").map_err(crate::error::decode)?;
            let reference: ArtifactReference =
                serde_json::from_value(value).map_err(|_| EventRepositoryError::Integrity)?;
            if reference.artifact_id() != &artifact_id {
                return Err(EventRepositoryError::Integrity);
            }
            graphhelm_events::validate_artifact_reference(&reference)
                .map_err(|_| EventRepositoryError::Integrity)?;
            Ok(reference)
        })
        .transpose()?;
    transaction.commit().await.map_err(crate::error::storage)?;
    Ok(result)
}
