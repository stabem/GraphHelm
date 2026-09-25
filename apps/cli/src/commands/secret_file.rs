//! The one implementation of "a 32-byte OS-random secret, hex-encoded, in a file only its owner
//! can read", shared by `serve` (the bearer token) and `init` (the bearer token AND the sealing
//! key). #1062 moved it here from `serve/mod.rs` so that `init` could mint the same token
//! `serve` later reads without a second copy of the create/validate rules drifting apart.
//!
//! Two callers, two files, one set of rules:
//!
//! - **Create-only-if-absent.** `create_new` is atomic against a racing second process on the
//!   same path; an existing file is kept byte for byte and never rotated. Rotating the token
//!   would strand every harness whose `.mcp.json` points at it; rotating the key would orphan
//!   every piece of evidence sealed under the old one (`gateway::keyring` says why at length).
//! - **Owner-only at creation.** `0o600` on Unix, set in the same `open` that creates the file
//!   so there is no window with looser bits. Windows ACLs are not evaluated, matching
//!   `events/config.rs`'s own Windows branch.
//! - **Validated on read**, through the same read-side checks `events/config.rs`'s
//!   `read_bounded`/`reject_insecure_permissions` apply to the operator configuration: not a
//!   symlink, a regular file, no group/world access on Unix, exactly 64 lowercase hex characters.
//!
//! The CLI binary may use OS randomness directly — the purity rules bind the core crates, not the
//! operator binary.

use std::io::Write;
use std::path::{Path, PathBuf};

const SECRET_BYTES: usize = 32;
const SECRET_HEX_LEN: usize = SECRET_BYTES * 2;
const TOKEN_SUFFIX: &str = ".token";

/// Whether [`ensure`] found the file or made it. Reported by `init` (the operator reads
/// `created`/`existing` per artifact) and ignored by `serve`, which only needs the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Provision {
    Created,
    Existing,
}

/// Why a secret file could not be provided. Carries the full message so each command family can
/// wrap it in its own `Failure` type and pointer without re-deriving the wording; the message
/// names the file's ROLE (`what`), never its contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SecretFileError {
    message: String,
}

impl SecretFileError {
    pub(crate) fn message(&self) -> &str {
        &self.message
    }

    fn new(what: &str, problem: &str) -> Self {
        Self {
            message: format!("the {what} file {problem}"),
        }
    }
}

/// The token's path: a *sibling* of the events directory, never a child of it — named
/// `<events-directory-name>.token` in the same parent directory. `commands::event_store`'s
/// `LocalEventRepository::open` treats the events directory as its own exclusively-owned
/// namespace: `classify_layout` in `core/events/src/local.rs` enumerates the directory's entries
/// against a closed allowlist (`blobs`, `.tmp`, `active`, `format.json`, `journal.jsonl`,
/// `repository.lock`) and refuses the *entire* repository with `GHE007_UNSUPPORTED_FORMAT` the
/// moment it finds anything else — a deliberate integrity guard, not a bug to work around from the
/// inside. Milestone 05a Task 1 originally wrote the token to `events/token`, which satisfies that
/// guard only until a real repository also exists there; from that point on, *every* command
/// against the directory — the CLI's own, not just the server's — starts failing
/// `GHE007_UNSUPPORTED_FORMAT`. This was caught empirically while building Task 2's first test
/// (`status_over_http_matches_the_cli_and_the_events_tail_pages`): a `graphhelm execution status`
/// run by hand against a directory `serve` had already touched reproduced the same failure with no
/// server involved, confirming the cause sits in the token's location, not in anything Tasks 2/3
/// added. Keeping the token outside the directory `LocalEventRepository` owns avoids the collision
/// entirely without weakening that crate's allowlist — the correct fix is on the operator side of
/// the boundary, not a loosened integrity guard on the store side. Callers guarantee the parent
/// directory exists (`serve::execute` and `init` both `create_dir_all(events)` first, which creates
/// every ancestor, including `events`'s own parent).
pub(crate) fn token_path(events: &Path) -> PathBuf {
    let mut name = events.file_name().map_or_else(
        || std::ffi::OsString::from("events"),
        std::ffi::OsStr::to_os_string,
    );
    name.push(TOKEN_SUFFIX);
    events.with_file_name(name)
}

/// Creates the bearer token beside `events` on first use and validates it thereafter.
pub(crate) fn ensure_token(events: &Path) -> Result<(Provision, String), SecretFileError> {
    ensure(&token_path(events), "bearer token")
}

/// Creates the file at `path` with a fresh secret if it does not exist, then reads and validates
/// whatever is there. `what` names the file's role in every diagnostic (`bearer token`,
/// `sealing key`).
pub(crate) fn ensure(path: &Path, what: &str) -> Result<(Provision, String), SecretFileError> {
    let provision = match create_secret_file(path) {
        Ok(()) => Provision::Created,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Provision::Existing,
        Err(_) => return Err(SecretFileError::new(what, "could not be created")),
    };
    let value = read_validated(path, what)?;
    Ok((provision, value))
}

/// Writes a fresh 32-byte OS-random secret, hex-encoded, refusing to overwrite an existing file
/// (`create_new`, atomic against a racing second process on the same path) and restricting access
/// to the owner alone on Unix (`0o600`) at creation time.
fn create_secret_file(path: &Path) -> std::io::Result<()> {
    let mut bytes = [0_u8; SECRET_BYTES];
    getrandom::fill(&mut bytes)
        .map_err(|_| std::io::Error::other("the OS random source is unavailable"))?;
    let hex = encode_hex(&bytes);

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(windows)]
    {
        // `WRITE_DAC` and `READ_CONTROL` so the protected DACL can be applied and read back on
        // this same handle, before a byte of the secret is written.
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Foundation::GENERIC_WRITE;
        use windows_sys::Win32::Storage::FileSystem::{READ_CONTROL, WRITE_DAC};
        options.access_mode(GENERIC_WRITE | READ_CONTROL | WRITE_DAC);
    }
    let mut file = options.open(path)?;
    // Windows: a fresh file inherits the directory's ACL, which lets every local user read it
    // (PR #1070 review, measured with `icacls`). The keyring's own protected owner-only DACL is
    // applied here through the ONE exported routine, and the empty file is removed if that fails
    // so a weakly-protected secret never exists on disk.
    #[cfg(windows)]
    if let Err(_error) = graphhelm_sealed_key_provider::protect_owner_only(&file) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(std::io::Error::other(
            "the owner-only ACL could not be applied",
        ));
    }
    file.write_all(hex.as_bytes())?;
    file.sync_all()
}

/// The read-side safety checks, narrowed from `events/config.rs`'s `read_bounded` to what a
/// fixed 64-character hex secret needs.
fn read_validated(path: &Path, what: &str) -> Result<String, SecretFileError> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| SecretFileError::new(what, "could not be read"))?;
    if metadata.file_type().is_symlink() {
        return Err(SecretFileError::new(what, "must not be a symbolic link"));
    }
    if !metadata.is_file() {
        return Err(SecretFileError::new(what, "must be a regular file"));
    }
    reject_insecure_permissions(&metadata, what)?;
    if metadata.len() != SECRET_HEX_LEN as u64 {
        return Err(SecretFileError::new(what, "is not the expected size"));
    }
    let raw = std::fs::read(path).map_err(|_| SecretFileError::new(what, "could not be read"))?;
    let value =
        String::from_utf8(raw).map_err(|_| SecretFileError::new(what, "is not valid UTF-8"))?;
    if value.len() != SECRET_HEX_LEN
        || !value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(SecretFileError::new(
            what,
            "is not 64 lowercase hexadecimal characters",
        ));
    }
    Ok(value)
}

#[cfg(unix)]
fn reject_insecure_permissions(
    metadata: &std::fs::Metadata,
    what: &str,
) -> Result<(), SecretFileError> {
    use std::os::unix::fs::MetadataExt;

    if metadata.mode() & 0o077 != 0 {
        return Err(SecretFileError::new(
            what,
            "must not be group or world accessible",
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn reject_insecure_permissions(_: &std::fs::Metadata, _: &str) -> Result<(), SecretFileError> {
    // Windows ACL evaluation is not attempted; the symlink and regular-file rules still apply —
    // identical no-op to `events/config.rs`'s own Windows branch.
    Ok(())
}

/// Hand-rolled to match `events/config.rs`'s own house style: that file validates and decodes
/// hexadecimal by hand (`hex_value`, the sha256 pin check) rather than pulling in the `hex`
/// crate for a few lines of work, even though `hex` is already an approved, pinned workspace
/// dependency used elsewhere. Encoding here follows the same convention.
fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_path_is_a_sibling_named_after_the_events_directory() {
        let events = Path::new("/srv/graphhelm/events");
        assert_eq!(token_path(events), Path::new("/srv/graphhelm/events.token"));
    }

    #[test]
    fn ensure_creates_once_and_keeps_the_bytes_on_the_second_call() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("serve.key");
        let (first, value) = ensure(&path, "sealing key").unwrap();
        assert_eq!(first, Provision::Created);
        assert_eq!(value.len(), 64);
        assert!(
            value
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        );
        let (second, again) = ensure(&path, "sealing key").unwrap();
        assert_eq!(second, Provision::Existing);
        assert_eq!(again, value);
    }

    #[test]
    fn a_malformed_existing_file_is_refused_by_role() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.token");
        std::fs::write(&path, "not-hex").unwrap();
        // Private, so the refusal comes from the size check this test is about and not from
        // the unix mode check that runs before it.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let error = ensure(&path, "bearer token").unwrap_err();
        assert_eq!(
            error.message(),
            "the bearer token file is not the expected size"
        );
    }
}
