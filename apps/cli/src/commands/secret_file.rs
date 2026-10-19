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

/// The agent session token beside an owner token file: `events.token` → `events.agent.token`
/// (#380). Derived from the owner token's path so a discovery record, which names only that
/// path, locates both.
pub(crate) fn agent_token_beside(owner_token: &Path) -> PathBuf {
    let mut name = owner_token.file_stem().map_or_else(
        || std::ffi::OsString::from("events"),
        std::ffi::OsStr::to_os_string,
    );
    name.push(".agent");
    name.push(TOKEN_SUFFIX);
    owner_token.with_file_name(name)
}

/// Creates the agent session token beside `events` on first use and validates it thereafter.
/// It authenticates agent sessions and is never accepted where the owner token is required.
pub(crate) fn ensure_agent_token(events: &Path) -> Result<(Provision, String), SecretFileError> {
    ensure(
        &agent_token_beside(&token_path(events)),
        "agent session token",
    )
}

/// The per-lane credential file beside an owner token file: `events.token` →
/// `events.agent.<lane>.token` (#1148). `task_record.py` and the reconciler look here, beside the
/// `--token-file` they are given, so a lane is handed one path and never another lane's secret.
pub(crate) fn lane_credential_beside(owner_token: &Path, lane: &str) -> PathBuf {
    let mut name = owner_token.file_stem().map_or_else(
        || std::ffi::OsString::from("events"),
        std::ffi::OsStr::to_os_string,
    );
    name.push(".agent.");
    name.push(lane);
    name.push(TOKEN_SUFFIX);
    owner_token.with_file_name(name)
}

/// A lane's credential, derived from the owner token, the execution and the lane (#1148):
/// `sha256("graphhelm-lane-credential-v1" NUL owner NUL execution NUL lane)`, hex. The Runtime
/// recomputes it to verify, so nothing new is persisted and no restart is needed; a holder of the
/// shared agent session token cannot derive it, because that token is not an input. The limit is
/// D-058's: the owner token sits on the same disk, under the same OS user.
pub(crate) fn lane_credential(owner_token: &str, execution: &str, lane: &str) -> String {
    use sha2::Digest as _;
    let mut hasher = sha2::Sha256::new();
    hasher.update(b"graphhelm-lane-credential-v1\0");
    hasher.update(owner_token.as_bytes());
    hasher.update(b"\0");
    hasher.update(execution.as_bytes());
    hasher.update(b"\0");
    hasher.update(lane.as_bytes());
    encode_hex(&hasher.finalize())
}

/// Writes a derived secret to `path` under the same owner-only creation rules as a minted one.
/// An existing file holding the same bytes is kept (`Existing`); one holding anything else is
/// replaced, since a derived value has exactly one correct content.
pub(crate) fn write_derived(
    path: &Path,
    value: &str,
    what: &str,
) -> Result<Provision, SecretFileError> {
    if path.exists() {
        if read_validated(path, what).is_ok_and(|current| current == value) {
            return Ok(Provision::Existing);
        }
        std::fs::remove_file(path)
            .map_err(|_| SecretFileError::new(what, "could not be replaced"))?;
    }
    create_secret_file_with(path, value)
        .map_err(|_| SecretFileError::new(what, "could not be created"))?;
    Ok(Provision::Created)
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
    create_secret_file_with(path, &encode_hex(&bytes))
}

/// The create-only-if-absent, owner-only write, for a value the caller chose.
fn create_secret_file_with(path: &Path, hex: &str) -> std::io::Result<()> {
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

/// Reads an existing secret file under the same validation, never creating one: the MCP bridge's
/// discovery path (#1325) reads the token a Runtime's record names, and must not mint a new one.
pub(crate) fn read_existing(path: &Path, what: &str) -> Result<String, SecretFileError> {
    read_validated(path, what)
}

/// #1523: a secret file only its owner may read, checked on every platform and read from the one
/// handle the checks ran on, so a path swapped after the checks cannot supply the bytes.
/// Refused: any link, junction or other reparse point as the file or as ANY ancestor directory;
/// anything but a regular file; group or world bits on Unix; on Windows a DACL other than the
/// keyring's own (owner, SYSTEM and Administrators, protected from inheritance, owned by the
/// calling user: `graphhelm_sealed_key_provider::verify_owner_only`); anything but exactly 64
/// lowercase hex characters. Messages name the role and the rule, never the bytes.
pub(crate) fn read_owner_only(path: &Path, what: &str) -> Result<String, SecretFileError> {
    use std::io::Read;
    let unreadable = || SecretFileError::new(what, "could not be read");
    let absolute = std::path::absolute(path).map_err(|_| unreadable())?;
    for ancestor in absolute.ancestors().skip(1) {
        if ancestor.parent().is_none() {
            break;
        }
        let metadata = std::fs::symlink_metadata(ancestor).map_err(|_| unreadable())?;
        if is_link(&metadata) {
            return Err(SecretFileError::new(
                what,
                "must not sit below a link, junction or other reparse point",
            ));
        }
    }
    let file = open_without_following(&absolute).map_err(|_| unreadable())?;
    // #1523 slice 2b: the ancestor walk above ran by path before the open; the handle's real path
    // proves no ancestor was swapped for a junction in between (and refuses mapped drives).
    #[cfg(windows)]
    {
        let real = final_path(&file).map_err(|_| unreadable())?;
        if !final_path_matches(&absolute, &real) {
            return Err(SecretFileError::new(
                what,
                "must be reached by its real path, not through a link, junction or mapped drive",
            ));
        }
    }
    let metadata = file.metadata().map_err(|_| unreadable())?;
    if is_link(&metadata) {
        return Err(SecretFileError::new(
            what,
            "must not be a link, junction or other reparse point",
        ));
    }
    if !metadata.is_file() {
        return Err(SecretFileError::new(what, "must be a regular file"));
    }
    reject_insecure_permissions(&metadata, what)?;
    #[cfg(windows)]
    if graphhelm_sealed_key_provider::verify_owner_only(&file).is_err() {
        return Err(SecretFileError::new(
            what,
            "must be readable only by its owner, SYSTEM and Administrators, with inheritance turned off",
        ));
    }
    if metadata.len() != SECRET_HEX_LEN as u64 {
        return Err(SecretFileError::new(what, "is not the expected size"));
    }
    let mut raw = zeroize::Zeroizing::new(Vec::with_capacity(SECRET_HEX_LEN + 1));
    (&file)
        .take(SECRET_HEX_LEN as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|_| unreadable())?;
    if raw.len() != SECRET_HEX_LEN
        || !raw
            .iter()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(SecretFileError::new(
            what,
            "is not 64 lowercase hexadecimal characters",
        ));
    }
    Ok(raw.iter().map(|&byte| char::from(byte)).collect())
}

/// The real path Windows reports for an open handle (`GetFinalPathNameByHandleW`, DOS volume
/// names), such as `\\?\D:\dir\file`.
#[cfg(windows)]
fn final_path(file: &std::fs::File) -> std::io::Result<String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{GetFinalPathNameByHandleW, VOLUME_NAME_DOS};
    let mut buffer = vec![0_u16; 512];
    for _ in 0..3 {
        let capacity = u32::try_from(buffer.len()).map_err(std::io::Error::other)?;
        // SAFETY: the handle is owned by `file` and stays open for the call; `buffer` holds
        // `capacity` writable UTF-16 units.
        let length = unsafe {
            GetFinalPathNameByHandleW(
                file.as_raw_handle(),
                buffer.as_mut_ptr(),
                capacity,
                VOLUME_NAME_DOS,
            )
        } as usize;
        if length == 0 {
            return Err(std::io::Error::last_os_error());
        }
        if length < buffer.len() {
            return Ok(String::from_utf16_lossy(&buffer[..length]));
        }
        buffer.resize(length + 1, 0);
    }
    Err(std::io::Error::other("final path did not settle"))
}

/// Whether the handle's real path names the requested file: equal after dropping the `\\?\`
/// (or `\\?\UNC\` → `\\`) prefix, ignoring ASCII case as NTFS does.
#[cfg_attr(not(windows), allow(dead_code))]
fn final_path_matches(requested: &Path, final_path: &str) -> bool {
    fn plain(path: &str) -> std::borrow::Cow<'_, str> {
        if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
            std::borrow::Cow::Owned(format!(r"\\{rest}"))
        } else {
            std::borrow::Cow::Borrowed(path.strip_prefix(r"\\?\").unwrap_or(path))
        }
    }
    let requested = requested.to_string_lossy();
    plain(final_path).eq_ignore_ascii_case(&plain(&requested))
}

fn is_link(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink() || graphhelm_process::fs::is_reparse_point(metadata)
}

/// Opens the last component itself, never what it points to: `FILE_FLAG_OPEN_REPARSE_POINT` on
/// Windows, `O_NOFOLLOW` on Unix.
fn open_without_following(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path)
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

/// Lowercase hex, two digits per byte. #1325: `hex::encode` (a pinned workspace dependency the
/// crate already uses) replaces the hand-rolled table; the cell below pins the same output.
fn encode_hex(bytes: &[u8]) -> String {
    hex::encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #1599 review (TOCTOU): the key file's checks and its bytes must come from ONE handle. A
    /// race between a path check and a path read cannot be staged without a production seam, so
    /// this guards the structure instead: after its single no-follow open, `read_owner_only`
    /// never touches the file by path again. Cost: one string scan of this file.
    #[test]
    fn the_key_file_is_opened_once_and_never_reread_by_path() {
        let source = include_str!("secret_file.rs");
        let start = source.find("pub(crate) fn read_owner_only").unwrap();
        let end = start
            + source[start..]
                .find(
                    "
fn is_link",
                )
                .unwrap();
        let body = &source[start..end];
        let open = body.find("open_without_following(").unwrap();
        assert_eq!(body.matches("open_without_following(").count(), 1, "{body}");
        let after_open = &body[open..];
        for path_access in [
            "fs::read(",
            "File::open(",
            "metadata(&absolute",
            "metadata(path",
            "read_validated(",
            "read_existing(",
            "OpenOptions",
        ] {
            assert!(
                !after_open.contains(path_access),
                "read_owner_only reaches the file by path after its open: {path_access}"
            );
        }
        // #1523 slice 2b: the ancestor walk runs before the open, so the handle's real path is
        // what proves no ancestor was swapped for a junction in between.
        assert!(
            after_open.contains("final_path_matches("),
            "read_owner_only must compare the opened handle's real path"
        );
    }

    /// #1523 slice 2b: the opened handle's real path (`GetFinalPathNameByHandleW`) must be the
    /// requested path. The prefix Windows adds and ASCII case are not a difference; another
    /// directory, a UNC share named for a drive path, or a longer path is. Cost: pure strings.
    #[test]
    fn the_final_path_must_be_the_requested_path() {
        let requested = Path::new(r"D:\gh\keys\serve.key");
        assert!(final_path_matches(requested, r"\\?\D:\gh\keys\serve.key"));
        assert!(final_path_matches(requested, r"\\?\d:\GH\Keys\Serve.KEY"));
        assert!(final_path_matches(requested, r"D:\gh\keys\serve.key"));
        assert!(!final_path_matches(requested, r"\\?\D:\gh\real\serve.key"));
        assert!(!final_path_matches(
            requested,
            r"\\?\UNC\host\gh\keys\serve.key"
        ));
        assert!(!final_path_matches(
            requested,
            r"\\?\D:\gh\keys\serve.key\x"
        ));
        assert!(!final_path_matches(requested, r"\\?\E:\gh\keys\serve.key"));
    }

    /// #1325: pinned BEFORE `encode_hex` becomes `hex::encode`: lowercase, two digits per byte,
    /// no separators, every byte value.
    #[test]
    fn hex_encode_matches_known_strings_for_every_byte() {
        assert_eq!(encode_hex(&[0x00, 0x0f, 0xf0, 0xff, 0xab]), "000ff0ffab");
        assert_eq!(encode_hex(&[]), "");
        let all: Vec<u8> = (0_u8..=255).collect();
        let expected: String = all.iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(encode_hex(&all), expected);
    }

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
