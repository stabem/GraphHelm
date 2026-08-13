use graphhelm_events::EventRepositoryError;
use graphhelm_protocols::RepositoryScope;
use sqlx::{Postgres, Transaction};

pub(crate) struct ScopeParts<'a> {
    pub workspace: &'a str,
    pub project: &'a str,
    pub execution: &'a str,
}

pub(crate) fn parts(scope: &RepositoryScope) -> ScopeParts<'_> {
    ScopeParts {
        workspace: scope.workspace_id().as_str(),
        project: scope.project_id().as_str(),
        execution: scope.execution_id().map_or("", |value| value.as_str()),
    }
}

pub(crate) async fn set_local(
    transaction: &mut Transaction<'_, Postgres>,
    scope: &RepositoryScope,
) -> Result<(), EventRepositoryError> {
    let scope = parts(scope);
    sqlx::query(
        "SELECT set_config('graphhelm.workspace_id', $1, true), \
         set_config('graphhelm.project_id', $2, true), \
         set_config('graphhelm.execution_id', $3, true)",
    )
    .bind(scope.workspace)
    .bind(scope.project)
    .bind(scope.execution)
    .execute(&mut **transaction)
    .await
    .map_err(crate::error::storage)?;
    Ok(())
}
