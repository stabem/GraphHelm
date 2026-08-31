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

/// Reads a path that must be a regular file, refusing to follow a link to get there.
///
/// **The previous version of this helper made a claim it did not keep**, and the correction is the
/// reason it now forks. It opened the path and then read the type from `File::metadata` -- but
/// `File::metadata` is `fstat` on the OPEN HANDLE, and fstat describes the RESOLVED target, never
/// the link that led to it. So a swap-to-symlink-pointing-at-a-regular-file passed: the open
/// followed the link, `is_file()` answered about the target and said yes, and the bytes were
/// archived. It closed swap-to-DIRECTORY and swap-to-device; it did not close the swap the threat
/// actually uses. (E, re-review on #602.)
///
/// **On unix the open itself now refuses.** `O_NOFOLLOW` makes `open` fail with `ELOOP` when the
/// final component is a symlink, so the race has no window: there is no moment at which a linked
/// target is open. `libc` is already a direct dependency of this crate, so the fork costs a `cfg`
/// block and nothing else -- which is the objection I weighed wrongly the first time, having
/// assumed a dependency the manifest already carried.
///
/// **On Windows the entry guard carries it**, and the asymmetry is deliberate rather than
/// overlooked. `DirEntry::file_type` does not traverse (documented std contract, not measured
/// here), so a symlink PRESENT at listing time is refused before this function is reached. What
/// remains is the swap DURING the run, and planting a symlink there needs a privilege an
/// unprivileged repository owner does not have -- measured on this machine as
/// `"A required privilege is not held by the client. (os error 1314)"`. The threat is Linux-shaped
/// and it is closed on Linux.
///
/// The handle check stays on both platforms: it is what refuses a swap to a directory or a device,
/// which `O_NOFOLLOW` does not address.
fn read_regular_file(path: &Path) -> std::io::Result<String> {
    use std::io::Read as _;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path)?;
    if !file.metadata()?.file_type().is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    Ok(text)
}

fn execute_local(root: &Path, output: &Path) -> Result<serde_json::Value, Failure> {
    super::require_supported_format(root)?;
    require_absent_file(output, "/output", "--output")?;

    let journal = read_regular_file(&root.join("journal.jsonl"))
        .map_err(|_| local_error("the repository journal could not be read"))?;

    let mut blobs = serde_json::Map::new();
    // The DIRECTORY is judged before it is walked. `read_dir` follows a symlinked `blobs`, so a
    // guard that only classifies ENTRIES is the same escalation one level up: replace the
    // directory itself and every entry inside it is legitimately regular. `symlink_metadata` is
    // the read that does NOT traverse, which is the whole reason it is the one used here.
    let blobs_root = root.join("blobs");
    let blobs_kind = std::fs::symlink_metadata(&blobs_root)
        .map_err(|_| local_error("the repository blob directory could not be read"))?
        .file_type();
    if blobs_kind.is_symlink() {
        return Err(local_error(
            "the repository blob directory is a link, so it was not archived",
        ));
    }
    let entries = std::fs::read_dir(&blobs_root)
        .map_err(|_| local_error("the repository blob directory could not be read"))?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        // #601: REFUSE anything that is not a regular file, BEFORE reading it.
        //
        // `read_to_string` follows symlinks, so without this an entry in `blobs/` could name a
        // path outside the repository and have that file's bytes copied into the archive under the
        // link's name. It matters because a backup legitimately runs as root over a repository the
        // service user owns (#595's scheduled unit): the reader is privileged, the directory is
        // not, and a symlink is the one link type whose creation requires no access to its target.
        // A hard link needs the target opened, so it can only reach what the planter could already
        // read -- which is why refusing non-regular entries closes the escalation without reasoning
        // about link counts.
        //
        // `file_type()` on the DirEntry does not traverse: on both platforms it reports the link
        // itself, which is the thing being judged.
        let kind = entry
            .file_type()
            .map_err(|_| local_error("a repository blob's type could not be read"))?;
        if !kind.is_file() {
            return Err(local_error(
                "a repository blob entry is not a regular file, so it was not archived",
            ));
        }
        // The directory listing decided WHETHER to read; `read_regular_file` decides WHAT was
        // opened. Between those two moments the entry can be swapped for a link -- the
        // check-to-open race -- and a name checked as regular then read through would defeat the
        // guard above entirely.
        let text = read_regular_file(&entry.path())
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
