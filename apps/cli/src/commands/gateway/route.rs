//! `graphhelm gateway route set` (#1171): write ONE `direct_api` route into a manifest file.
//!
//! `gateway setup` (#1139) already wires a provider end to end, and it stays the command a person
//! runs first: it mints the keyring, asks for the key once, stores it and probes. What it cannot do
//! is EDIT — it derives the base URL and the credential reference from a closed provider table,
//! always writes `enabled: true`, and reads its key from a terminal. An operator who wants to move
//! a route to a different model, correct a base URL, or turn one off has to hand-edit the manifest
//! today, and a Studio screen cannot hand-edit anything. So this module is the write, and D-039's
//! rule decides its shape: the same function serves the CLI verb and `PUT /v1/gateway/routes`,
//! because a mutation that exists on one door and not the other is a second operational path.
//!
//! WHAT IT REFUSES TO TOUCH, and the list is short on purpose:
//!
//! - **Only `direct_api`.** A `native_runtime` route names a CLI and carries no credential; it is
//!   not a provider card with an API key, and a screen offering to edit one would be offering to
//!   key something that has no key. Writing one is a different decision, not a flag on this one.
//! - **Never the credential value.** This moves one route's declaration. The secret lives in the
//!   broker and enters through `gateway credential set`, a different surface with a different
//!   failure mode — and keeping them apart is what lets this one be called with no secret in scope.
//! - **Never a document the loader would reject.** The whole manifest is serialized and put through
//!   [`RouteManifest::from_json`] BEFORE a byte reaches disk, so a write that would make the file
//!   unloadable refuses instead of landing and breaking every later read. `serve` re-reads this
//!   file on every request, so "unloadable" would be immediate and shared, not latent.
//!
//! The credential reference defaults to `secret_<route id>`, NOT `setup`'s `secret_<provider>`.
//! Provider is the wire format an adapter selects on, not the vendor: a DeepSeek route and an
//! OpenAI route are both `provider: "openai"`, so the provider-keyed default names one secret for
//! two different keys and the second write silently takes the first one's name. A route id is
//! unique within the manifest by construction, so keying the secret to it cannot collide.

use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use fs2::FileExt;
use graphhelm_gateway::manifest::{RouteManifest, Transport};
use serde_json::{Map, Value, json};

use super::{Failure, finish, invalid};
use crate::output::Outcome;

#[cfg(all(test, windows))]
struct RetryObserver {
    signal: std::sync::mpsc::Sender<()>,
    released: std::sync::mpsc::Receiver<()>,
    waited: bool,
}

#[cfg(all(test, windows))]
thread_local! {
    static RETRY_OBSERVER: std::cell::RefCell<Option<RetryObserver>> =
        const { std::cell::RefCell::new(None) };
}

const COMMAND: &str = "gateway.route.set";
/// The one profile a written route advertises, matching `setup`'s own choice. Scoring within a
/// profile is deferred (`core/gateway/src/manifest.rs`); the tag only has to be a legal member.
const PROFILE: &str = "balanced_reasoning";
const MANIFEST_VERSION: u64 = 1;

/// What the caller asked to write. Every field is the operator's; the only value this module
/// supplies on its own is the credential reference's default, and the reply names it.
#[derive(Debug, Clone)]
pub(in crate::commands) struct RouteWrite {
    pub id: String,
    pub provider: String,
    pub base_url: String,
    pub model: String,
    pub credential_ref: Option<String>,
    pub profiles: Option<Vec<String>>,
    pub enabled: bool,
    pub replace: bool,
}

/// How the manifest file changed — the same vocabulary `gateway setup` reports, so an operator who
/// has read one reply can read the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::commands) enum ManifestState {
    Created,
    Merged,
    Replaced,
}

impl ManifestState {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Merged => "merged",
            Self::Replaced => "replaced",
        }
    }
}

/// Writes `write` into the manifest at `path`, or refuses without touching the file.
pub(in crate::commands) fn set(path: &Path, write: &RouteWrite) -> Outcome {
    finish(COMMAND, execute(path, write), |summary| summary)
}

fn execute(path: &Path, write: &RouteWrite) -> Result<Value, Failure> {
    validate(write)?;
    let _lock = acquire_manifest_lock(path)?;
    let credential_ref = write
        .credential_ref
        .clone()
        .unwrap_or_else(|| format!("secret_{}", write.id));
    let (text, state) = merged(path, write, &credential_ref)?;
    // The loader is the judge of what a legal route is, and it runs on the COMPOSED document
    // rather than on this entry alone: a route that is fine by itself can still be the one that
    // makes the file unloadable, and the caller has to learn that before the write, not after.
    let manifest = RouteManifest::from_json(&text)
        .map_err(|error| invalid(&error.to_string(), "/manifest"))?;
    write_atomically(path, &text)?;
    Ok(json!({
        "manifest": {
            "path": path.to_string_lossy(),
            "state": state.as_str(),
        },
        "route": {
            "id": write.id,
            "provider": write.provider,
            "baseUrl": write.base_url,
            "model": write.model,
            "credentialRef": credential_ref,
            "enabled": write.enabled,
        },
        "routes": listing(&manifest),
    }))
}

/// The bounds this module owns, checked before the document is read. Everything a loaded manifest
/// already decides — which providers a `direct_api` route may name, what a base URL may look like,
/// whether a credential reference is required — is left to [`RouteManifest::from_json`], so those
/// questions have one answer in this codebase rather than two that can drift apart.
fn validate(write: &RouteWrite) -> Result<(), Failure> {
    for (value, field, pointer) in [
        (&write.id, "id", "/id"),
        (&write.provider, "provider", "/provider"),
        (&write.base_url, "baseUrl", "/baseUrl"),
        (&write.model, "model", "/model"),
    ] {
        if value.trim().is_empty() {
            return Err(invalid(&format!("{field} must not be empty"), pointer));
        }
    }
    if let Some(reference) = &write.credential_ref
        && reference.trim().is_empty()
    {
        return Err(invalid(
            "credentialRef must not be empty when it is given",
            "/credentialRef",
        ));
    }
    Ok(())
}

fn merged(
    path: &Path,
    write: &RouteWrite,
    credential_ref: &str,
) -> Result<(String, ManifestState), Failure> {
    let (mut document, existed) = load_document(path)?;
    let routes = match document
        .entry("routes")
        .or_insert_with(|| Value::Array(Vec::new()))
    {
        Value::Array(routes) => routes,
        _ => return Err(unusable()),
    };
    let entry = json!({
        "id": write.id,
        "provider": write.provider,
        "transport": "direct_api",
        "authentication": "api_key",
        "billingMode": "per_token",
        "baseUrl": write.base_url,
        "model": write.model,
        "credentialRef": credential_ref,
        "profiles": write.profiles.clone().unwrap_or_else(|| vec![PROFILE.to_owned()]),
        "enabled": write.enabled,
    });
    let position = routes
        .iter()
        .position(|existing| existing.get("id").and_then(Value::as_str) == Some(write.id.as_str()));
    let state = match (position, write.replace) {
        (Some(_), false) => {
            return Err(invalid(
                "the manifest already declares a route with this id; ask for a replace to swap it",
                "/id",
            ));
        }
        (Some(index), true) => {
            // A replace REWRITES the entry rather than patching the fields the caller named.
            // A partial patch would let an operator correct a base URL while a stale model from a
            // previous life stays behind it, and the reply would name only the field they touched:
            // a route nobody declared as a whole is a route nobody reviewed as a whole.
            let previous = routes[index].clone();
            refuse_foreign_transport(&previous)?;
            routes[index] = entry;
            ManifestState::Replaced
        }
        (None, _) => {
            routes.push(entry);
            if existed {
                ManifestState::Merged
            } else {
                ManifestState::Created
            }
        }
    };
    document
        .entry("manifestVersion")
        .or_insert_with(|| json!(MANIFEST_VERSION));
    let mut text = serde_json::to_string_pretty(&Value::Object(document))
        .map_err(|_| invalid("the manifest could not be serialized", "/manifest"))?;
    text.push('\n');
    Ok((text, state))
}

/// A replace may not convert someone else's `native_runtime` route into a `direct_api` one.
///
/// The transport decides what the route IS: a native-runtime route names a CLI that owns its own
/// authentication, and rewriting it as `direct_api` would silently point the same id at a network
/// endpoint with a key — the operator asking for "edit this provider card" would be changing what
/// the id means for every execution that names it. An id already taken by a native route is a
/// refusal with its own sentence, not a field this write may overwrite.
fn refuse_foreign_transport(previous: &Value) -> Result<(), Failure> {
    match previous.get("transport").and_then(Value::as_str) {
        Some("direct_api") | None => Ok(()),
        Some(other) => Err(invalid(
            &format!(
                "the manifest declares this id as a {other} route; this write only replaces direct_api routes"
            ),
            "/id",
        )),
    }
}

/// The listing shape `gateway routes` publishes, so a caller that just wrote a route can render the
/// result without a second request.
fn listing(manifest: &RouteManifest) -> Value {
    Value::Array(
        manifest
            .routes()
            .iter()
            .map(|route| {
                json!({
                    "id": route.id(),
                    "provider": route.provider(),
                    "transport": match route.transport() {
                        Transport::DirectApi => "direct_api",
                        Transport::NativeRuntime => "native_runtime",
                    },
                    "authentication": route.authentication(),
                    "billingMode": route.billing_mode(),
                    "baseUrl": route.base_url(),
                    "credentialRef": route.credential_ref(),
                    "model": route.model(),
                    "profiles": route.profiles(),
                    "enabled": route.enabled(),
                })
            })
            .collect(),
    )
}

/// Reads the manifest as a JSON object, reporting an absent file as an empty document. A file that
/// exists and is not a JSON object is a refusal: writing beside bytes nobody can parse would
/// destroy whatever the operator had there.
fn load_document(path: &Path) -> Result<(Map<String, Value>, bool), Failure> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(invalid(
                "the manifest path must name a regular file",
                "/manifest",
            ));
        }
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((Map::new(), false));
        }
        Err(_) => return Err(invalid("the manifest could not be read", "/manifest")),
    };
    debug_assert!(metadata.is_file());
    match super::read_bounded_manifest(path) {
        Ok(bytes) => match String::from_utf8(bytes)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        {
            Some(Value::Object(document)) => Ok((document, true)),
            _ => Err(unusable()),
        },
        Err(super::ManifestReadError::TooLarge) => Err(invalid(
            "the manifest exceeds the maximum supported size",
            "/manifest",
        )),
        Err(super::ManifestReadError::Unreadable) => {
            Err(invalid("the manifest could not be read", "/manifest"))
        }
    }
}

fn unusable() -> Failure {
    invalid(
        "the manifest file is not a JSON object this command can extend",
        "/manifest",
    )
}

/// Writes through a temporary file renamed into place, so a reader never sees half a manifest.
/// `serve` re-reads this file on every request that resolves a route, so a torn read is a live
/// hazard here rather than a theoretical one.
pub(super) fn write_atomically(path: &Path, text: &str) -> Result<(), Failure> {
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| invalid("the manifest path names no file", "/manifest"))?;
    if let Ok(metadata) = std::fs::symlink_metadata(path)
        && (metadata.file_type().is_symlink() || !metadata.is_file())
    {
        return Err(invalid(
            "the manifest path must name a regular file",
            "/manifest",
        ));
    }
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    let attempt = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let temporary = directory.join(format!(".{name}.{}.{}.tmp", std::process::id(), attempt));
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .and_then(|mut file| {
            file.write_all(text.as_bytes())?;
            file.sync_all()
        });
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
        return Err(invalid("the manifest could not be written", "/manifest"));
    }
    replace_file(&temporary, path).map_err(|_| {
        let _ = std::fs::remove_file(&temporary);
        invalid("the manifest could not be replaced", "/manifest")
    })
}

pub(super) fn acquire_manifest_lock(path: &Path) -> Result<std::fs::File, Failure> {
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| invalid("the manifest path names no file", "/manifest"))?;
    let lock_path = directory.join(format!(".{name}.lock"));
    let lock = match std::fs::OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(&lock_path)
    {
        Ok(lock) => lock,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            open_existing_lock_no_follow(&lock_path)?
        }
        Err(_) => return Err(invalid("the manifest could not be locked", "/manifest")),
    };
    let metadata = lock
        .metadata()
        .map_err(|_| invalid("the manifest could not be locked", "/manifest"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(invalid(
            "the manifest lock is not a regular file",
            "/manifest",
        ));
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        match FileExt::try_lock_exclusive(&lock) {
            Ok(()) if lock_path_matches(&lock_path, &lock) => return Ok(lock),
            Ok(()) => {
                let _ = FileExt::unlock(&lock);
                return Err(invalid(
                    "the manifest lock changed while it was being acquired",
                    "/manifest",
                ));
            }
            Err(error) if lock_is_contended(&error) && std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(_) => {
                return Err(invalid(
                    "the manifest could not be locked within ten seconds",
                    "/manifest",
                ));
            }
        }
    }
}

#[cfg(unix)]
fn open_existing_lock_no_follow(path: &Path) -> Result<std::fs::File, Failure> {
    use std::os::unix::fs::OpenOptionsExt;

    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| invalid("the manifest could not be locked", "/manifest"))
}

#[cfg(windows)]
fn open_existing_lock_no_follow(path: &Path) -> Result<std::fs::File, Failure> {
    use std::os::windows::fs::OpenOptionsExt;

    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| invalid("the manifest could not be locked", "/manifest"))
}

fn lock_is_contended(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::WouldBlock
        || cfg!(windows) && matches!(error.raw_os_error(), Some(32) | Some(33))
}

fn lock_path_matches(path: &Path, held: &std::fs::File) -> bool {
    let Ok(by_path) = open_existing_lock_no_follow(path) else {
        return false;
    };
    same_file_identity(held, &by_path)
}

#[cfg(unix)]
fn same_file_identity(left: &std::fs::File, right: &std::fs::File) -> bool {
    use std::os::unix::fs::MetadataExt;

    let (Ok(left), Ok(right)) = (left.metadata(), right.metadata()) else {
        return false;
    };
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(windows)]
fn same_file_identity(left: &std::fs::File, right: &std::fs::File) -> bool {
    use std::mem::MaybeUninit;
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    fn identity(file: &std::fs::File) -> Option<(u64, u64)> {
        let mut information = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
        // SAFETY: `file` owns a live handle and the successful call initializes the structure.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), information.as_mut_ptr()) }
            == 0
        {
            return None;
        }
        // SAFETY: the successful call above initialized every field.
        let information = unsafe { information.assume_init() };
        Some((
            u64::from(information.dwVolumeSerialNumber),
            (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow),
        ))
    }

    matches!((identity(left), identity(right)), (Some(left), Some(right)) if left == right)
}

#[cfg(unix)]
fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // This deadline bounds the number of retry iterations. A native MoveFileExW call can still
    // block beyond it; the clock is consulted before each new call and is deliberately not used
    // to claim a wall-clock bound for the current OS operation. It starts at the FIRST failure,
    // after the test observer below has returned, so a test's own synchronisation never spends
    // the window it exercises (review of #1231; the same shape #1230 landed in backup.rs).
    let mut deadline: Option<std::time::Instant> = None;
    loop {
        // SAFETY: both buffers are NUL-terminated UTF-16 strings and remain alive for the call.
        // MoveFileEx replaces the destination directory entry; it does not open the destination
        // for content writes, so a racing reparse point cannot redirect the manifest bytes.
        let result = unsafe {
            MoveFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if result != 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        let retryable = matches!(error.raw_os_error(), Some(5 | 32 | 33));
        #[cfg(all(test, windows))]
        if retryable {
            RETRY_OBSERVER.with(|observer| {
                if let Some(observer) = observer.borrow_mut().as_mut()
                    && !observer.waited
                {
                    observer.waited = true;
                    let _ = observer.signal.send(());
                    let _ = observer
                        .released
                        .recv_timeout(std::time::Duration::from_secs(1));
                }
            });
        }
        let deadline = *deadline.get_or_insert_with(|| {
            std::time::Instant::now()
                .checked_add(std::time::Duration::from_millis(100))
                .unwrap_or_else(std::time::Instant::now)
        });
        if !retryable || std::time::Instant::now() >= deadline {
            return Err(error);
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
        if std::time::Instant::now() >= deadline {
            return Err(error);
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use std::io::Read;
    use std::sync::mpsc;

    use super::{RETRY_OBSERVER, RetryObserver, replace_file, write_atomically};

    #[test]
    fn short_reader_window_is_retried_and_publishes_new_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let manifest = directory.path().join("manifest.json");
        std::fs::write(&manifest, b"old manifest").unwrap();
        let (ready_sender, ready_receiver) = mpsc::channel();
        let (retry_sender, retry_receiver) = mpsc::channel();
        let (released_sender, released_receiver) = mpsc::channel();
        RETRY_OBSERVER.with(|observer| {
            *observer.borrow_mut() = Some(RetryObserver {
                signal: retry_sender,
                released: released_receiver,
                waited: false,
            })
        });
        let reader_path = manifest.clone();
        let reader = std::thread::spawn(move || {
            let mut held = super::super::open_manifest_no_follow(&reader_path).unwrap();
            ready_sender.send(()).unwrap();
            retry_receiver
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap();
            let mut bytes = Vec::new();
            held.read_to_end(&mut bytes).unwrap();
            drop(held);
            released_sender.send(()).unwrap();
            bytes
        });
        ready_receiver.recv().unwrap();

        assert!(write_atomically(&manifest, "new manifest").is_ok());
        RETRY_OBSERVER.with(|observer| *observer.borrow_mut() = None);
        assert_eq!(std::fs::read(&manifest).unwrap(), b"new manifest");
        assert_eq!(reader.join().unwrap(), b"old manifest");
    }

    #[test]
    fn persistent_reader_returns_error_and_cleans_the_temporary_file() {
        let directory = tempfile::tempdir().unwrap();
        let manifest = directory.path().join("manifest.json");
        std::fs::write(&manifest, b"old manifest").unwrap();
        let held = super::super::open_manifest_no_follow(&manifest).unwrap();

        assert!(write_atomically(&manifest, "new manifest").is_err());
        assert_eq!(std::fs::read(&manifest).unwrap(), b"old manifest");
        assert!(
            std::fs::read_dir(directory.path())
                .unwrap()
                .filter_map(Result::ok)
                .all(|entry| !entry.file_name().to_string_lossy().ends_with(".tmp"))
        );
        drop(held);
    }

    #[test]
    fn persistent_reader_exposes_the_native_sharing_error() {
        let directory = tempfile::tempdir().unwrap();
        let manifest = directory.path().join("manifest.json");
        let source = directory.path().join("source.tmp");
        std::fs::write(&manifest, b"old manifest").unwrap();
        std::fs::write(&source, b"new manifest").unwrap();
        let held = super::super::open_manifest_no_follow(&manifest).unwrap();

        let error = replace_file(&source, &manifest).unwrap_err();
        assert!(matches!(error.raw_os_error(), Some(5 | 32 | 33)));
        drop(held);
    }
}
