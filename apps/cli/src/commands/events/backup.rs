use std::path::Path;

use graphhelm_postgres_event_store::backup::PostgresBackupOperator;
use serde_json::json;
use sqlx::postgres::PgPoolOptions;

use super::{Failure, config, config_error, finish, require_absent_file};
use crate::output::Outcome;

const COMMAND: &str = "events.backup";

pub(in crate::commands) struct Request<'a> {
    pub(in crate::commands) repository: Option<&'a Path>,
    pub(in crate::commands) config: Option<&'a Path>,
    pub(in crate::commands) output: &'a Path,
}

pub(in crate::commands) fn run(request: Request<'_>) -> Outcome {
    finish(COMMAND, execute(&request), |value| value)
}

/// Dispatch is by the PRESENCE of `--repository`, not by `single_selector` (#336).
///
/// `verify` uses `single_selector`, which requires exactly one of the two flags. This verb cannot:
/// `config::resolve_path` falls back to `GRAPHHELM_EVENTS_CONFIG` when `--config` is absent, and
/// `config_is_accepted_from_the_environment_variable` in `event_store_cli.rs` holds that behaviour.
/// Adopting the shared selector wholesale would make "neither flag" an error and break that test —
/// a change to the Postgres path, which this work is required to leave alone.
///
/// So: `--repository` present selects the local store; absent leaves the existing path untouched,
/// environment fallback and all. Both flags together is refused, which is the one part of the
/// selector's contract that applies here.
fn execute(request: &Request<'_>) -> Result<serde_json::Value, Failure> {
    if let Some(root) = request.repository {
        if request.config.is_some() {
            return Err(super::argument(
                "--repository and --config are mutually exclusive",
                "/repository",
            ));
        }
        return execute_local(root, request.output);
    }
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

/// The archive carries EVIDENCE and nothing that describes the root's own state (#336).
///
/// `journal.jsonl` and `blobs/` only. The layout — format marker, lock, workspace directories — is
/// not archived because it is not data: `restore` has the store build it, so there is exactly one
/// definition of what a valid root looks like and it is not in this file.
///
/// What is NOT the reason, recorded because it was believed and measured false: an earlier design
/// left `format.json` out so the restored root would classify as `RecognizedPartial` and be
/// initialised on first open. It cannot. `classify_layout`'s `has_format == false` branch refuses a
/// non-empty `blobs/` and a non-empty `journal.jsonl` outright — `RecognizedPartial` is the state of
/// an EMPTY root. The archive's contents are unchanged by that correction; only the reason is.
const ARCHIVE_VERSION: &str = "1.0.0";

fn execute_local(root: &Path, output: &Path) -> Result<serde_json::Value, Failure> {
    super::require_supported_format(root)?;
    require_absent_file(output, "/output", "--output")?;

    let journal = std::fs::read_to_string(root.join("journal.jsonl"))
        .map_err(|_| local_error("the repository journal could not be read"))?;

    let mut blobs = serde_json::Map::new();
    let entries = std::fs::read_dir(root.join("blobs"))
        .map_err(|_| local_error("the repository blob directory could not be read"))?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(entry.path())
            .map_err(|_| local_error("a repository blob could not be read"))?;
        blobs.insert(name, serde_json::Value::String(text));
    }
    let blob_count = blobs.len();

    let archive = json!({
        "archiveVersion": ARCHIVE_VERSION,
        "journal": journal,
        "blobs": serde_json::Value::Object(blobs),
    });
    let encoded = serde_json::to_vec(&archive)
        .map_err(|_| local_error("the archive could not be encoded"))?;
    std::fs::write(output, encoded).map_err(|_| local_error("the archive could not be written"))?;

    Ok(json!({"journalBytes": journal.len(), "blobCount": blob_count}))
}

/// Local failures name a class and never a path: the same posture the Postgres arm keeps, for the
/// same reason — an operator-facing envelope is not a place to reconstruct filesystem layout.
fn local_error(message: &str) -> Failure {
    config_error(message, "/repository")
}

/// Backup failures are reported by stable class only; the adapter's redacted error carries no
/// path, DSN, or credential and none is reconstructed here.
fn operator_error(message: &str) -> Failure {
    config_error(message, "/backup")
}
