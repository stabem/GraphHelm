use std::path::Path;

use graphhelm_postgres_event_store::backup::PostgresBackupOperator;
use serde_json::json;
use sqlx::postgres::PgPoolOptions;

use super::{Failure, config, config_error, finish, require_existing_file};
use crate::output::Outcome;

const COMMAND: &str = "events.restore";

pub(in crate::commands) struct Request<'a> {
    pub(in crate::commands) config: Option<&'a Path>,
    pub(in crate::commands) archive: &'a Path,
}

pub(in crate::commands) fn run(request: Request<'_>) -> Outcome {
    finish(COMMAND, execute(&request), |value| value)
}

fn execute(request: &Request<'_>) -> Result<serde_json::Value, Failure> {
    let config_path = config::resolve_path(request.config)?;
    let configuration = config::load(&config_path)?;
    require_existing_file(request.archive, "/archive", "--archive")?;

    let provider = configuration.key_provider()?;
    let admin_url = configuration.admin_url().to_owned();
    let timeout = configuration.process_timeout();
    let (profile, pg_dump, pg_restore) = configuration.into_tools();
    let archive = request.archive.to_path_buf();

    let receipt = super::runtime()?.block_on(async move {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&admin_url)
            .await
            .map_err(|_| operator_error("the administrative database endpoint is unreachable"))?;
        let operator =
            PostgresBackupOperator::new(pool, provider, profile, pg_dump, pg_restore, timeout)
                .await
                .map_err(|_| operator_error("the restore operator could not be constructed"))?;
        operator
            .restore_from_path(&archive)
            .await
            .map_err(|_| operator_error("the verified restore did not complete"))
    })?;
    Ok(json!({
        "restored": true,
        "sourceIdentitySha256": receipt.source_identity_sha256(),
        "targetIdentitySha256": receipt.target_identity_sha256(),
        "manifestSha256": receipt.manifest_sha256(),
        "authenticationKeyId": receipt.authentication_key_id(),
        "authenticationAlgorithm": receipt.authentication_algorithm(),
    }))
}

/// Restore failures are reported by stable class only; a failed target is left disabled behind its
/// authenticated marker by the adapter and is never named here.
fn operator_error(message: &str) -> Failure {
    config_error(message, "/restore")
}
