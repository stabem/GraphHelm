use std::path::Path;

use graphhelm_postgres_event_store::backup::PostgresBackupOperator;
use serde_json::json;
use sqlx::postgres::PgPoolOptions;

use super::{Failure, config, config_error, finish, require_existing_file};
use crate::output::Outcome;

const COMMAND: &str = "events.restore";
const SUPPORTED_LOCAL_ARCHIVE_VERSION: &str = "1.0.0";

pub(in crate::commands) struct Request<'a> {
    pub(in crate::commands) repository: Option<&'a Path>,
    pub(in crate::commands) config: Option<&'a Path>,
    pub(in crate::commands) archive: &'a Path,
}

pub(in crate::commands) fn run(request: Request<'_>) -> Outcome {
    finish(COMMAND, execute(&request), |value| value)
}

/// Dispatch by the PRESENCE of `--repository`, for the reason written on `backup::execute`: this
/// verb's Postgres path resolves its configuration from the environment when `--config` is absent,
/// and adopting `single_selector` wholesale would turn "neither flag" into an error and change a
/// behaviour this work must leave alone.
fn execute(request: &Request<'_>) -> Result<serde_json::Value, Failure> {
    if let Some(root) = request.repository {
        if request.config.is_some() {
            return Err(super::argument(
                "--repository and --config are mutually exclusive",
                "/repository",
            ));
        }
        return execute_local(root, request.archive);
    }
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

/// Lets the STORE build the layout, then writes the evidence into it (#336).
///
/// An earlier design wrote `journal.jsonl` and `blobs/` and left `open` to initialise the rest. It
/// was reviewed, agreed by two of us, and REFUTED by the first run: `classify_layout`'s
/// `has_format == false` branch refuses a non-empty `blobs/` and a non-empty `journal.jsonl`
/// outright, so a root carrying evidence without a format marker is `GHE007_UNSUPPORTED_FORMAT`.
/// `RecognizedPartial` is the state of an EMPTY root, not of a root with data in it. We had both
/// reasoned from the `has_format == true` branch — where a missing lock is `Integrity` — and
/// neither of us read what the other branch permits.
///
/// So the store initialises an empty root first and the evidence goes in afterwards. The CLI does
/// not reproduce `FORMAT_BYTES` or the lock's shape: the layout has ONE definition and it is the
/// store's. What was right in the earlier reading survives — the lock is not carried IN the archive
/// — but the file is part of the layout; only HOLDING it is a property of a running process.
fn execute_local(root: &Path, archive: &Path) -> Result<serde_json::Value, Failure> {
    require_existing_file(archive, "/archive", "--archive")?;

    // Refuse rather than merge. A restore landing beside existing events produces a store whose
    // history is neither the archive's nor the destination's, and no later check can separate them.
    if let Ok(mut entries) = std::fs::read_dir(root)
        && entries.next().is_some()
    {
        return Err(local_error(
            "the destination repository is not empty; restoring would write over an existing store",
        ));
    }

    let text = std::fs::read_to_string(archive)
        .map_err(|_| local_error("the archive could not be read"))?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|_| local_error("the archive is not a valid archive"))?;
    let archive_version = value["archiveVersion"]
        .as_str()
        .ok_or_else(unsupported_archive_version)?;
    if archive_version != SUPPORTED_LOCAL_ARCHIVE_VERSION {
        return Err(unsupported_archive_version());
    }
    let journal = value["journal"]
        .as_str()
        .ok_or_else(|| local_error("the archive carries no journal"))?;
    let blobs = value["blobs"]
        .as_object()
        .ok_or_else(|| local_error("the archive carries no blob set"))?;

    std::fs::create_dir_all(root)
        .map_err(|_| local_error("the destination repository could not be created"))?;
    // The store initialises its own layout — format marker, lock, workspace directories — and the
    // handle is dropped immediately so nothing holds the lock while the evidence is written. This
    // is the only step that knows what a valid root looks like, and it is not this file.
    drop(crate::commands::event_store(root).map_err(|error| super::repository_failure(&error))?);

    std::fs::write(root.join("journal.jsonl"), journal)
        .map_err(|_| local_error("the journal could not be written"))?;
    for (name, content) in blobs {
        // The archive is UNTRUSTED input and these names become path components. A name carrying a
        // separator or a parent hop would write outside the destination entirely — a restore turned
        // into an arbitrary write. Rejected by SHAPE rather than sanitised: a name that is not a
        // plain file name is not a blob, and repairing it would be guessing at intent.
        if name.is_empty() || Path::new(name).file_name() != Some(std::ffi::OsStr::new(name)) {
            return Err(local_error(
                "the archive names a blob that is not a plain file name",
            ));
        }
        let content = content
            .as_str()
            .ok_or_else(|| local_error("the archive carries a blob that is not text"))?;
        std::fs::write(root.join("blobs").join(name), content)
            .map_err(|_| local_error("a blob could not be written"))?;
    }

    Ok(json!({"journalBytes": journal.len(), "blobCount": blobs.len()}))
}

/// Local failures name a class and never a path, matching the Postgres arm's posture.
fn local_error(message: &str) -> Failure {
    config_error(message, "/repository")
}

fn unsupported_archive_version() -> Failure {
    config_error(
        "the archive version is missing, malformed, or unsupported",
        "/archiveVersion",
    )
}

/// Restore failures are reported by stable class only; a failed target is left disabled behind its
/// authenticated marker by the adapter and is never named here.
fn operator_error(message: &str) -> Failure {
    config_error(message, "/restore")
}
