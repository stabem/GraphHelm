use std::path::Path;

use graphhelm_postgres_event_store::backup::PostgresBackupOperator;
use serde_json::json;
use sqlx::postgres::PgPoolOptions;

use super::{Failure, config, config_error, finish, require_absent_file};
use crate::output::Outcome;

const COMMAND: &str = "events.backup";

pub(in crate::commands) struct Request<'a> {
    pub(in crate::commands) config: Option<&'a Path>,
    pub(in crate::commands) output: &'a Path,
}

pub(in crate::commands) fn run(request: Request<'_>) -> Outcome {
    finish(COMMAND, execute(&request), |value| value)
}

fn execute(request: &Request<'_>) -> Result<serde_json::Value, Failure> {
    let config_path = config::resolve_path(request.config)?;
    let configuration = config::load(&config_path)?;
    require_absent_file(request.output, "/output", "--output")?;

    let provider = configuration.key_provider()?;
    let admin_url = configuration.admin_url().to_owned();
    let timeout = configuration.process_timeout();
    let (profile, pg_dump, pg_restore) = configuration.into_tools();
    let destination = request.output.to_path_buf();

    let receipt = super::runtime()?.block_on(async move {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&admin_url)
            .await
            .map_err(|_| operator_error("the administrative database endpoint is unreachable"))?;
        let operator =
            PostgresBackupOperator::new(pool, provider, profile, pg_dump, pg_restore, timeout)
                .await
                .map_err(|_| operator_error("the backup operator could not be constructed"))?;
        operator
            .backup_to_path(&destination)
            .await
            .map_err(|_| operator_error("the encrypted backup did not complete"))
    })?;
    Ok(json!({"chunkCount": receipt.chunk_count()}))
}

/// Backup failures are reported by stable class only; the adapter's redacted error carries no
/// path, DSN, or credential and none is reconstructed here.
fn operator_error(message: &str) -> Failure {
    config_error(message, "/backup")
}
