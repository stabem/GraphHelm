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

/// One directory listing yields one `Result` per ENTRY, and the second kind of `Err` is the one
/// this seam exists for (#444). `read_dir` itself can fail, and that was always reported; each
/// entry the iterator hands back can ALSO be an `Err` -- a name the OS could not read, an entry
/// that vanished mid-walk -- and those were `flatten`ed away, so the archive was written smaller
/// than the source and the command said success. The lister is a parameter so a test can hand
/// the capture an iterator whose second item is an error, deterministically, from outside the
/// filesystem; production passes `std::fs::read_dir` and nothing else.
type BlobEntries = Box<dyn Iterator<Item = std::io::Result<std::fs::DirEntry>>>;
type BlobLister<'a> = &'a mut dyn FnMut(&Path) -> std::io::Result<BlobEntries>;

fn read_dir_lister(directory: &Path) -> std::io::Result<BlobEntries> {
    std::fs::read_dir(directory).map(|entries| Box::new(entries) as BlobEntries)
}

fn execute_local(root: &Path, output: &Path) -> Result<serde_json::Value, Failure> {
    execute_local_with(root, output, &mut read_dir_lister)
}

fn execute_local_with(
    root: &Path,
    output: &Path,
    list: BlobLister<'_>,
) -> Result<serde_json::Value, Failure> {
    super::require_supported_format(root)?;
    require_absent_file(output, "/output", "--output")?;

    // THE WHOLE SNAPSHOT RUNS UNDER THE STORE'S OWN SHARED LOCK (#664).
    //
    // The journal and the blobs were read as two independent passes with nothing between them.
    // A writer appending in that gap - a local CLI invocation, which `deploy/backup-vps.sh`'s
    // operation lock does not serialise because that lock covers the deploy scripts and the
    // service, not an independent CLI run - produced a bundle whose two halves never coexisted,
    // and the command reported SUCCESS. There are two distinct tears, not one: a retention
    // cleanup between the passes removes a blob the captured journal still references, and an
    // append DURING the journal read can leave its final line truncated.
    //
    // The lock is the store's, taken through the store, not reimplemented here: it is two levels
    // (the root directory, then `repository.lock`), and half of it would be a lock that looks
    // like the store's without being it.
    //
    // NOT by opening the store: `open` refuses a root carrying `format.json` without
    // `repository.lock` - the shape a restored-but-never-opened directory has, which this command
    // must still archive - and opening would repair a damaged layout, a write to the very thing
    // being backed up.
    //
    // A root with no lock file has no writer to race: `open` refuses that shape rather than
    // creating the lock, so nothing can attach to it while the capture runs. That case is
    // REPORTED rather than assumed - `lockHeld` in the envelope - because a snapshot that
    // silently could not lock is the same false confidence, one level down.
    let (captured, lock_held) =
        graphhelm_events::with_repository_read_lock(root, || capture_local(root, list)).map_err(
            |_| local_error("the repository could not be held for a consistent snapshot"),
        )?;
    let (journal, blobs) = captured?;

    let blob_count = blobs.len();

    let archive = json!({
        "archiveVersion": ARCHIVE_VERSION,
        "journal": journal,
        "blobs": serde_json::Value::Object(blobs),
    });
    let encoded = serde_json::to_vec(&archive)
        .map_err(|_| local_error("the archive could not be encoded"))?;
    std::fs::write(output, encoded).map_err(|_| local_error("the archive could not be written"))?;

    Ok(json!({
        "journalBytes": journal.len(),
        "blobCount": blob_count,
        // The operator can tell a locked snapshot from an unlocked one without reading this code.
        "lockHeld": lock_held == graphhelm_events::ReadLockHeld::Held,
    }))
}

/// The two reads the bundle is built from, run as one unit so the caller can hold a lock across
/// them. Split out of `execute_local` for exactly that reason and for no other: the body below is
/// unchanged from what shipped, so what this commit changes is WHEN it runs, not what it does.
fn capture_local(
    root: &Path,
    list: BlobLister<'_>,
) -> Result<(String, serde_json::Map<String, serde_json::Value>), Failure> {
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
    let entries = list(&blobs_root)
        .map_err(|_| local_error("the repository blob directory could not be read"))?;
    for entry in entries {
        // AN ENTRY THE LISTING COULD NOT PRODUCE IS A FAILED BACKUP, NOT A SHORTER ONE (#444).
        // This loop used to `flatten()` the iterator, which discards every per-entry `Err`: the
        // archive was then written without that blob and the envelope reported success. A backup
        // that is complete and a backup that is missing an entry are the same bytes to whoever
        // restores it, and the second is worse than an explicit failure because it is false
        // recovery evidence. So the error is returned as the class it is, before anything is
        // written -- and, like every other local failure here, it names no path.
        let entry = entry.map_err(|_| {
            local_error(
                "a repository blob entry could not be listed, so the archive was not written",
            )
        })?;
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

    Ok((journal, blobs))
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

#[cfg(test)]
mod entry_errors {
    //! #444: the listing's per-entry errors reach the operator as a failure, never as a smaller
    //! successful archive. Injected at the enumeration boundary, because no filesystem can be
    //! asked to produce a per-entry `Err` on demand, and a cell that waits for one never runs.

    use std::path::{Path, PathBuf};

    use super::{BlobEntries, execute_local_with, read_dir_lister};

    /// The bytes `require_supported_format` compares against, taken from a committed store rather
    /// than retyped: the check is byte-exact.
    const FORMAT_JSON: &[u8] =
        include_bytes!("../../../../../docs/acceptance/m05-run-2026-08-16/events/format.json");

    /// A root the local backup accepts: a format marker, a journal, and two regular blobs. No
    /// `repository.lock` -- the capture reports the unlocked snapshot rather than refusing it.
    fn root_with_two_blobs(directory: &Path) -> PathBuf {
        let root = directory.join("source");
        std::fs::create_dir_all(root.join("blobs")).unwrap();
        std::fs::write(root.join("format.json"), FORMAT_JSON).unwrap();
        std::fs::write(root.join("journal.jsonl"), b"{\"line\":1}\n").unwrap();
        std::fs::write(root.join("blobs").join("first.json"), b"{}").unwrap();
        std::fs::write(root.join("blobs").join("second.json"), b"{}").unwrap();
        root
    }

    /// The real listing, with its `at`-th entry (0-based) replaced by an error. Everything before
    /// it is genuine, so the failure lands in the MIDDLE of a walk that had already produced a
    /// blob -- the shape in which a `flatten` quietly produces a smaller archive.
    fn listing_failing_at(at: usize) -> impl FnMut(&Path) -> std::io::Result<BlobEntries> {
        move |directory: &Path| {
            let real = read_dir_lister(directory)?;
            let entries = real.enumerate().map(move |(index, entry)| {
                if index == at {
                    Err(std::io::Error::other(
                        "injected: this entry could not be listed",
                    ))
                } else {
                    entry
                }
            });
            Ok(Box::new(entries) as BlobEntries)
        }
    }

    /// CONTROL FIRST: the same root and the real listing succeed, or the failure below is about
    /// the fixture rather than about the injected entry.
    #[test]
    fn the_real_listing_archives_both_blobs() {
        let directory = tempfile::tempdir().unwrap();
        let root = root_with_two_blobs(directory.path());
        let output = directory.path().join("archive.json");

        // `let ... else` rather than `expect`: `Failure` carries no `Debug`, deliberately.
        let Ok(envelope) = execute_local_with(&root, &output, &mut read_dir_lister) else {
            panic!("CONTROL: the real listing must back this root up")
        };

        assert_eq!(
            envelope["blobCount"], 2,
            "both blobs are archived: {envelope}"
        );
        assert!(output.is_file(), "the archive is written on success");
    }

    /// THE CELL. Before #444 this root produced an archive holding ONE blob and an envelope
    /// reporting success; the operator had recovery evidence that could not recover the store.
    #[test]
    fn an_entry_the_listing_cannot_produce_fails_the_backup_and_writes_no_archive() {
        let directory = tempfile::tempdir().unwrap();
        let root = root_with_two_blobs(directory.path());
        let output = directory.path().join("archive.json");

        let failure = execute_local_with(&root, &output, &mut listing_failing_at(1))
            .expect_err("a per-entry listing error is a failed backup");

        assert_eq!(
            failure.message,
            "a repository blob entry could not be listed, so the archive was not written"
        );
        assert_eq!(failure.pointer, "/repository");
        // The class and nothing else: no path, no OS text, no temp directory.
        let leaked = directory.path().to_string_lossy();
        assert!(
            !failure.message.contains(&*leaked) && !failure.message.contains("injected"),
            "the failure must name a class, not the filesystem: {}",
            failure.message
        );
        assert!(
            !output.exists(),
            "NO archive may exist after a listing error: a smaller archive reporting success is exactly the defect"
        );
    }

    /// The first entry failing is the other edge: nothing was archived yet, and the answer is the
    /// same failure rather than an empty success.
    #[test]
    fn a_failure_on_the_very_first_entry_is_the_same_refusal() {
        let directory = tempfile::tempdir().unwrap();
        let root = root_with_two_blobs(directory.path());
        let output = directory.path().join("archive.json");

        let failure = execute_local_with(&root, &output, &mut listing_failing_at(0))
            .expect_err("a per-entry listing error is a failed backup");

        assert_eq!(failure.pointer, "/repository");
        assert!(!output.exists(), "no archive after a listing error");
    }
}
