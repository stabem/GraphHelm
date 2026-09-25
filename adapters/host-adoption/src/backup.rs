//! Private local snapshots for the supported adoption configuration surfaces.

use std::collections::BTreeMap;
#[cfg(windows)]
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;

use graphhelm_protocols::adoption::{AdoptionError, AdoptionReason};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const BACKUPS: &str = "backups";
/// A checkpoint is local recovery data, never an unbounded archive of a home directory.
const MAX_BACKUP_BYTES: u64 = 1024 * 1024 * 1024;
// 10,000 bounded discovery entries can carry long, nested relative names. The manifest contains
// only those names and fixed-size digests, so 64 MiB safely covers the worst accepted key set.
const MAX_MANIFEST_BYTES: u64 = 64 * 1024 * 1024;
const MAX_BACKUP_FILES: usize = crate::surfaces::ALL.len() + 10_000;

#[cfg(all(test, windows))]
fn observe_nt_child_failure(status: i32, directory: bool, create: bool, mutable: bool, share: u32) {
    eprintln!(
        "GH_ADOPTION_NT_CHILD_FAILURE source=windows_nt_child_shared status=0x{:08x} directory={directory} create={create} mutable={mutable} share=0x{share:08x}",
        status as u32
    );
}

#[cfg(all(test, windows))]
thread_local! {
    static LAST_NT_PUBLISH_FAILURE: std::cell::Cell<Option<u32>> = const { std::cell::Cell::new(None) };
    static RELEASE_ON_NT_PUBLISH_FAILURE: std::cell::RefCell<Option<std::sync::mpsc::Sender<()>>> = const { std::cell::RefCell::new(None) };
    static RELEASE_ACKNOWLEDGEMENT: std::cell::RefCell<Option<std::sync::mpsc::Receiver<()>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(all(test, windows))]
fn observe_nt_publish_failure(status: i32) {
    let status = status as u32;
    eprintln!(
        "GH_ADOPTION_NT_PUBLISH_FAILURE source=windows_publish_directory status=0x{:08x}",
        status
    );
    LAST_NT_PUBLISH_FAILURE.with(|last| last.set(Some(status)));
    RELEASE_ON_NT_PUBLISH_FAILURE.with(|sender| {
        if let Some(sender) = sender.borrow().as_ref() {
            let _ = sender.send(());
        }
    });
    RELEASE_ACKNOWLEDGEMENT.with(|acknowledgement| {
        if let Some(receiver) = acknowledgement.borrow_mut().take() {
            receiver
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("release thread must acknowledge the dropped handle");
        }
    });
}

#[cfg(all(test, windows))]
fn take_nt_publish_failure() -> Option<u32> {
    LAST_NT_PUBLISH_FAILURE.with(|last| last.take())
}

#[cfg(all(test, windows))]
fn set_release_on_nt_publish_failure(
    sender: Option<std::sync::mpsc::Sender<()>>,
    acknowledgement: Option<std::sync::mpsc::Receiver<()>>,
) {
    RELEASE_ON_NT_PUBLISH_FAILURE.with(|slot| *slot.borrow_mut() = sender);
    RELEASE_ACKNOWLEDGEMENT.with(|slot| *slot.borrow_mut() = acknowledgement);
}

pub fn valid_backup_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

pub fn backup(project: &Path, home: &Path, state_root: &Path) -> Result<Value, AdoptionError> {
    backup_with_limit(project, home, state_root, MAX_BACKUP_BYTES)
}

/// Testable bounded backup entry point. Production callers use [`backup`].
pub fn backup_with_limit(
    project: &Path,
    home: &Path,
    state_root: &Path,
    limit_bytes: u64,
) -> Result<Value, AdoptionError> {
    let project_root = crate::storage::Root::observe(project)?;
    let home_root = crate::storage::Root::observe(home)?;
    let _home_lock = home_root.lock(".graphhelm-adoption.lock")?;
    let _project_lock = project_root.lock(".graphhelm-adoption.lock")?;
    project_root.verify()?;
    home_root.verify()?;
    backup_with_limit_unlocked(&project_root, &home_root, state_root, limit_bytes)
}

/// Internal entry point for apply/restore, which already hold both authority locks.
pub(crate) fn backup_locked(
    project: &crate::storage::Root,
    home: &crate::storage::Root,
    state_root: &Path,
    limit_bytes: u64,
) -> Result<Value, AdoptionError> {
    backup_with_limit_unlocked(project, home, state_root, limit_bytes)
}

fn backup_with_limit_unlocked(
    project: &crate::storage::Root,
    home: &crate::storage::Root,
    state_root: &Path,
    limit_bytes: u64,
) -> Result<Value, AdoptionError> {
    project.verify()?;
    home.verify()?;
    let mut files = BTreeMap::new();
    let mut surface_metadata = BTreeMap::new();
    let mut surface_identities = BTreeMap::new();
    let mut total = 0_u64;
    for surface in crate::surfaces::ALL {
        let name = surface.id;
        let root = if surface.scope == "project" {
            project
        } else {
            home
        };
        let Some(observed) =
            observe_surface(root, surface.path, limit_bytes.saturating_sub(total))?
        else {
            surface_metadata.insert(name, json!({"state":"absent","accessDigest":Value::Null}));
            continue;
        };
        surface_metadata.insert(
            name,
            json!({"state":"present","accessDigest":observed.access_digest,"access":observed.access}),
        );
        surface_identities.insert(name, observed.identity);
        total = total
            .checked_add(
                u64::try_from(observed.bytes.len()).map_err(|_| AdoptionError {
                    reason: AdoptionReason::LimitExceeded,
                })?,
            )
            .ok_or(AdoptionError {
                reason: AdoptionReason::LimitExceeded,
            })?;
        if total > limit_bytes {
            return Err(AdoptionError {
                reason: AdoptionReason::LimitExceeded,
            });
        }
        files.insert(name.to_owned(), observed.bytes);
    }
    // Discovered package manifests are private recovery evidence. Restore never treats these
    // dynamic package sources as mutable adoption surfaces.
    let discovery_limit = limit_bytes.saturating_sub(total);
    let discovered = crate::inventory::discovered_manifest_files(project, home, discovery_limit)?;
    let discovered_names = discovered.keys().cloned().collect::<Vec<_>>();
    for (name, bytes) in discovered {
        total = total.checked_add(bytes.len() as u64).ok_or(AdoptionError {
            reason: AdoptionReason::LimitExceeded,
        })?;
        if total > limit_bytes {
            return Err(AdoptionError {
                reason: AdoptionReason::LimitExceeded,
            });
        }
        files.insert(name, bytes);
    }
    // Cooperative writers are excluded by the authority locks. A second pass detects source
    // drift from writers that do not participate in that locking protocol.
    for surface in crate::surfaces::ALL {
        let root = if surface.scope == "project" {
            project
        } else {
            home
        };
        let current = observe_surface(root, surface.path, MAX_BACKUP_BYTES)?;
        if current.as_ref().map(|value| &value.bytes) != files.get(surface.id) {
            return Err(AdoptionError {
                reason: AdoptionReason::Busy,
            });
        }
        if current.as_ref().map(|value| &value.identity) != surface_identities.get(surface.id) {
            return Err(AdoptionError {
                reason: AdoptionReason::Busy,
            });
        }
        let metadata = surface_metadata
            .get(surface.id)
            .and_then(Value::as_object)
            .ok_or(AdoptionError {
                reason: AdoptionReason::InvalidConfiguration,
            })?;
        let current_access = current.as_ref().map(|value| value.access_digest.as_str());
        if current_access != metadata.get("accessDigest").and_then(Value::as_str) {
            return Err(AdoptionError {
                reason: AdoptionReason::Busy,
            });
        }
    }
    project.verify()?;
    home.verify()?;
    let discovered_again =
        crate::inventory::discovered_manifest_files(project, home, discovery_limit)?;
    if discovered_again.len() != discovered_names.len()
        || discovered_names
            .iter()
            .any(|name| discovered_again.get(name) != files.get(name))
    {
        return Err(AdoptionError {
            reason: AdoptionReason::Busy,
        });
    }
    let state = crate::storage::Root::open(state_root, true)?;
    // A manual checkpoint may be the first adoption operation. Keep the private journal
    // directory available so its read-only restore planner can distinguish an empty history from
    // a malformed journal without creating state during planning.
    state.private_child("journals")?;
    let provenance = provenance_for(project, home, &state)?;
    #[cfg(unix)]
    {
        backup_unix(&files, &surface_metadata, &provenance, state_root)
    }
    #[cfg(windows)]
    {
        backup_windows(&files, &surface_metadata, &provenance, state_root)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (files, state_root);
        Err(AdoptionError {
            reason: AdoptionReason::CoverageIncomplete,
        })
    }
}

pub fn verify_backup(state_root: &Path, id: &str) -> Result<Value, AdoptionError> {
    if !valid_backup_id(id) {
        return Err(AdoptionError {
            reason: AdoptionReason::PathUnsafe,
        });
    }
    #[cfg(unix)]
    {
        let state = unix_open_dir_chain(state_root, false)?;
        unix_check_private_dir(&state)?;
        let backups = unix_open_child_dir(&state, BACKUPS, false)?;
        unix_check_private_dir(&backups)?;
        let destination = unix_open_child_dir(&backups, id, false)?;
        unix_check_private_dir(&destination)?;
        verify_unix_dir(&destination, id)
    }
    #[cfg(windows)]
    {
        let state = windows_open_directory_chain(state_root, false)?;
        let backups = windows_open_child(&state, BACKUPS, true)?.ok_or(AdoptionError {
            reason: AdoptionReason::CoverageIncomplete,
        })?;
        windows_check_private(&backups)?;
        let destination = windows_open_child(&backups, id, true)?.ok_or(AdoptionError {
            reason: AdoptionReason::CoverageIncomplete,
        })?;
        windows_check_private(&destination)?;
        verify_windows_dir(&destination, id)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (state_root, id);
        Err(AdoptionError {
            reason: AdoptionReason::CoverageIncomplete,
        })
    }
}

pub(crate) fn read_supported(
    root: &crate::storage::Root,
    relative: &str,
) -> Result<Option<Vec<u8>>, AdoptionError> {
    crate::storage::relative_ok(relative)?;
    root.verify()?;
    read_bounded_regular(&root.record.path, relative, 1024 * 1024)
}

#[cfg(unix)]
fn backup_unix(
    files: &BTreeMap<String, Vec<u8>>,
    surface_metadata: &BTreeMap<&str, Value>,
    provenance: &Value,
    state_root: &Path,
) -> Result<Value, AdoptionError> {
    use std::os::fd::AsRawFd;

    let state = unix_open_dir_chain(state_root, true)?;
    unix_check_private_dir(&state)?;
    let backups = unix_open_child_dir(&state, BACKUPS, true)?;
    unix_check_private_dir(&backups)?;
    let manifest = manifest_for(files, surface_metadata, provenance)?;
    let id = digest(&manifest);
    if let Some(destination) = unix_open_optional_child_dir(&backups, &id)? {
        return verify_unix_dir(&destination, &id);
    }
    let pending_name = format!(".pending-{}", uuid::Uuid::new_v4());
    let pending = unix_create_child_dir(&backups, &pending_name)?;
    for (index, bytes) in files.values().enumerate() {
        unix_write_new(&pending, &format!("blob-{index}"), bytes)?;
    }
    unix_write_new(&pending, "manifest.json", &manifest)?;
    unix_sync_fd(pending.as_raw_fd())?;
    unix_rename_child(&backups, &pending_name, &id)?;
    unix_sync_fd(backups.as_raw_fd())?;
    Ok(
        json!({"apiVersion":"p50.dev/adoption/v1","kind":"BackupReceipt","id":id,"spec":{"verified":true,"restoreEligibility":"bound","fileCount":files.len()}}),
    )
}

#[cfg(unix)]
pub(super) fn unix_open_dir_chain(
    path: &Path,
    create: bool,
) -> Result<std::fs::File, AdoptionError> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_err(unavailable)?.join(path)
    };
    let root = CString::new("/").unwrap();
    let fd = unsafe {
        libc::open(
            root.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(unavailable(std::io::Error::last_os_error()));
    }
    let mut current = unsafe { std::fs::File::from_raw_fd(fd) };
    for component in absolute.components() {
        let component = match component {
            std::path::Component::RootDir => continue,
            std::path::Component::Normal(component) => component,
            _ => {
                return Err(AdoptionError {
                    reason: AdoptionReason::PathUnsafe,
                });
            }
        };
        let name = CString::new(component.as_bytes()).map_err(|_| AdoptionError {
            reason: AdoptionReason::PathUnsafe,
        })?;
        let next = unsafe {
            libc::openat(
                current.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if next < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::NotFound || !create {
                return Err(map_open_error(error));
            }
            let made = unsafe { libc::mkdirat(current.as_raw_fd(), name.as_ptr(), 0o700) };
            if made < 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST) {
                return Err(unavailable(std::io::Error::last_os_error()));
            }
            let next = unsafe {
                libc::openat(
                    current.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            if next < 0 {
                return Err(map_open_error(std::io::Error::last_os_error()));
            }
            current = unsafe { std::fs::File::from_raw_fd(next) };
        } else {
            current = unsafe { std::fs::File::from_raw_fd(next) };
        }
    }
    Ok(current)
}

#[cfg(unix)]
fn unix_open_optional_child_dir(
    parent: &std::fs::File,
    name: &str,
) -> Result<Option<std::fs::File>, AdoptionError> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    let name = CString::new(name).map_err(|_| AdoptionError {
        reason: AdoptionReason::PathUnsafe,
    })?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        let error = std::io::Error::last_os_error();
        return if error.kind() == std::io::ErrorKind::NotFound {
            Ok(None)
        } else {
            Err(map_open_error(error))
        };
    }
    Ok(Some(unsafe { std::fs::File::from_raw_fd(fd) }))
}

#[cfg(unix)]
pub(super) fn unix_open_child_dir(
    parent: &std::fs::File,
    name: &str,
    create: bool,
) -> Result<std::fs::File, AdoptionError> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    let name = CString::new(name).map_err(|_| AdoptionError {
        reason: AdoptionReason::PathUnsafe,
    })?;
    let mut fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 && create && std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound {
        if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } < 0
            && std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST)
        {
            return Err(unavailable(std::io::Error::last_os_error()));
        }
        fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
    }
    if fd < 0 {
        return Err(map_open_error(std::io::Error::last_os_error()));
    }
    Ok(unsafe { std::fs::File::from_raw_fd(fd) })
}

#[cfg(unix)]
pub(super) fn unix_create_child_dir(
    parent: &std::fs::File,
    name: &str,
) -> Result<std::fs::File, AdoptionError> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    let name = CString::new(name).map_err(|_| AdoptionError {
        reason: AdoptionReason::PathUnsafe,
    })?;
    if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } < 0 {
        return Err(unavailable(std::io::Error::last_os_error()));
    }
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(map_open_error(std::io::Error::last_os_error()));
    }
    Ok(unsafe { std::fs::File::from_raw_fd(fd) })
}

#[cfg(unix)]
fn unix_write_new(parent: &std::fs::File, name: &str, bytes: &[u8]) -> Result<(), AdoptionError> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    let name = CString::new(name).map_err(|_| AdoptionError {
        reason: AdoptionReason::PathUnsafe,
    })?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(unavailable(std::io::Error::last_os_error()));
    }
    let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
    file.write_all(bytes).map_err(unavailable)?;
    file.sync_all().map_err(unavailable)
}

#[cfg(unix)]
fn unix_sync_fd(fd: std::os::fd::RawFd) -> Result<(), AdoptionError> {
    if unsafe { libc::fsync(fd) } == 0 {
        Ok(())
    } else {
        Err(unavailable(std::io::Error::last_os_error()))
    }
}

#[cfg(unix)]
fn unix_rename_child(parent: &std::fs::File, from: &str, to: &str) -> Result<(), AdoptionError> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    let from = CString::new(from).unwrap();
    let to = CString::new(to).unwrap();
    if unsafe {
        libc::renameat(
            parent.as_raw_fd(),
            from.as_ptr(),
            parent.as_raw_fd(),
            to.as_ptr(),
        )
    } == 0
    {
        Ok(())
    } else {
        Err(unavailable(std::io::Error::last_os_error()))
    }
}

#[cfg(unix)]
fn unix_read_at_limited(
    parent: &std::fs::File,
    name: &str,
    limit: u64,
) -> Result<Vec<u8>, AdoptionError> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    let name = CString::new(name).unwrap();
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(AdoptionError {
            reason: AdoptionReason::BackupCorrupt,
        });
    }
    let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
    let metadata = file.metadata().map_err(|_| AdoptionError {
        reason: AdoptionReason::BackupCorrupt,
    })?;
    if !metadata.is_file() {
        return Err(AdoptionError {
            reason: AdoptionReason::BackupCorrupt,
        });
    }
    let size = metadata.len();
    if size > limit {
        return Err(AdoptionError {
            reason: AdoptionReason::BackupCorrupt,
        });
    }
    bounded_read(&mut file, size, limit).map_err(|_| AdoptionError {
        reason: AdoptionReason::BackupCorrupt,
    })
}

#[cfg(unix)]
fn verify_unix_dir(root: &std::fs::File, id: &str) -> Result<Value, AdoptionError> {
    let manifest = unix_read_at_limited(root, "manifest.json", MAX_MANIFEST_BYTES)?;
    if digest(&manifest) != id {
        return Err(AdoptionError {
            reason: AdoptionReason::BackupCorrupt,
        });
    }
    let value: Value = serde_json::from_slice(&manifest).map_err(|_| AdoptionError {
        reason: AdoptionReason::BackupCorrupt,
    })?;
    validate_manifest_version(&value)?;
    let files = value
        .get("files")
        .and_then(Value::as_object)
        .ok_or(AdoptionError {
            reason: AdoptionReason::BackupCorrupt,
        })?;
    validate_manifest_files(files)?;
    validate_manifest_surface_metadata(
        value.get("surfaceMetadata"),
        files,
        value.get("provenance").is_some(),
    )?;
    if let Some(provenance) = value.get("provenance") {
        validate_provenance(provenance)?;
    }
    let mut total = 0_u64;
    for (index, (_, expected)) in files.iter().enumerate() {
        let expected = expected.as_str().ok_or(AdoptionError {
            reason: AdoptionReason::BackupCorrupt,
        })?;
        let blob = unix_read_at_limited(
            root,
            &format!("blob-{index}"),
            MAX_BACKUP_BYTES.saturating_sub(total),
        )?;
        total = total
            .checked_add(u64::try_from(blob.len()).map_err(|_| AdoptionError {
                reason: AdoptionReason::BackupCorrupt,
            })?)
            .ok_or(AdoptionError {
                reason: AdoptionReason::BackupCorrupt,
            })?;
        if digest(&blob) != expected {
            return Err(AdoptionError {
                reason: AdoptionReason::BackupCorrupt,
            });
        }
    }
    Ok(
        json!({"apiVersion":"p50.dev/adoption/v1","kind":"BackupReceipt","id":id,"spec":{"verified":true,"restoreEligibility":if value.get("provenance").is_some() {"bound"} else {"integrity_only"},"fileCount":files.len()}}),
    )
}

fn validate_manifest_files(files: &serde_json::Map<String, Value>) -> Result<(), AdoptionError> {
    if files.len() > MAX_BACKUP_FILES {
        return Err(AdoptionError {
            reason: AdoptionReason::BackupCorrupt,
        });
    }
    for (name, expected) in files {
        let parts = name.split('/').collect::<Vec<_>>();
        let discovered = name.len() <= 4096
            && !name.contains(['\\', ':', '\0'])
            && parts.len() >= 5
            && parts[0] == "discovered"
            && matches!(parts[1], "project" | "home")
            && matches!(parts[2], "skill" | "plugin")
            && !parts
                .iter()
                .any(|part| part.is_empty() || matches!(*part, "." | ".."))
            && matches!(
                parts.last().copied(),
                Some("SKILL.md" | "plugin.json" | "manifest.json")
            );
        if (crate::surfaces::by_id(name).is_none() && !discovered)
            || !expected.as_str().is_some_and(valid_backup_id)
        {
            return Err(AdoptionError {
                reason: AdoptionReason::BackupCorrupt,
            });
        }
    }
    Ok(())
}

fn validate_manifest_version(value: &Value) -> Result<(), AdoptionError> {
    if value.get("version").and_then(Value::as_u64) != Some(1) {
        return Err(AdoptionError {
            reason: AdoptionReason::BackupCorrupt,
        });
    }
    Ok(())
}

fn validate_provenance(value: &Value) -> Result<(), AdoptionError> {
    let object = value.as_object().ok_or(AdoptionError {
        reason: AdoptionReason::BackupCorrupt,
    })?;
    if object.get("kind").and_then(Value::as_str) != Some("manual") {
        return Err(AdoptionError {
            reason: AdoptionReason::BackupCorrupt,
        });
    }
    for name in ["project", "home", "state"] {
        let record: crate::storage::RootRecord =
            serde_json::from_value(object.get(name).cloned().ok_or(AdoptionError {
                reason: AdoptionReason::BackupCorrupt,
            })?)
            .map_err(|_| AdoptionError {
                reason: AdoptionReason::BackupCorrupt,
            })?;
        let value = serde_json::to_value(&record).map_err(|_| AdoptionError {
            reason: AdoptionReason::BackupCorrupt,
        })?;
        let digest = crate::apply::digest(
            &graphhelm_graph::canonical_content_bytes(&value).map_err(|_| AdoptionError {
                reason: AdoptionReason::BackupCorrupt,
            })?,
        );
        let recorded = object
            .get("bindings")
            .and_then(Value::as_object)
            .and_then(|bindings| bindings.get(name))
            .and_then(Value::as_str);
        if recorded != Some(digest.as_str()) {
            return Err(AdoptionError {
                reason: AdoptionReason::BackupCorrupt,
            });
        }
    }
    Ok(())
}

fn validate_manifest_surface_metadata(
    value: Option<&Value>,
    files: &serde_json::Map<String, Value>,
    bound: bool,
) -> Result<(), AdoptionError> {
    let metadata = value.and_then(Value::as_object).ok_or(AdoptionError {
        reason: AdoptionReason::BackupCorrupt,
    })?;
    if metadata.len() != crate::surfaces::ALL.len()
        || crate::surfaces::ALL
            .iter()
            .any(|surface| !metadata.contains_key(surface.id))
    {
        return Err(AdoptionError {
            reason: AdoptionReason::BackupCorrupt,
        });
    }
    for (id, entry) in metadata {
        let object = entry.as_object().ok_or(AdoptionError {
            reason: AdoptionReason::BackupCorrupt,
        })?;
        let state = object.get("state").and_then(Value::as_str);
        let access = object.get("accessDigest");
        let access_pair_valid = !bound
            || access
                .and_then(Value::as_str)
                .zip(object.get("access"))
                .is_some_and(|(expected, value)| {
                    serde_json::from_value::<crate::storage::Access>(value.clone())
                        .ok()
                        .and_then(|access| access_digest_value(&access).ok())
                        .is_some_and(|actual| actual == expected)
                });
        let in_files = files.contains_key(id);
        let valid_state = match state {
            Some("present") => {
                in_files
                    && access.and_then(Value::as_str).is_some_and(valid_backup_id)
                    && access_pair_valid
                    && (object.get("access").is_some() || !bound)
            }
            Some("absent") => !in_files && matches!(access, Some(Value::Null)),
            _ => false,
        };
        if !valid_state {
            return Err(AdoptionError {
                reason: AdoptionReason::BackupCorrupt,
            });
        }
    }
    Ok(())
}

/// Returns authenticated private checkpoint metadata. `None` is a legacy, integrity-only backup.
pub(crate) fn checkpoint_provenance(
    state_root: &Path,
    id: &str,
) -> Result<Option<Value>, AdoptionError> {
    verify_backup(state_root, id)?;
    #[cfg(unix)]
    let manifest = {
        let state = unix_open_dir_chain(state_root, false)?;
        unix_check_private_dir(&state)?;
        let backups = unix_open_child_dir(&state, BACKUPS, false)?;
        unix_check_private_dir(&backups)?;
        let destination = unix_open_child_dir(&backups, id, false)?;
        unix_check_private_dir(&destination)?;
        unix_read_at_limited(&destination, "manifest.json", MAX_MANIFEST_BYTES)?
    };
    #[cfg(windows)]
    let manifest = {
        let state = windows_open_directory_chain(state_root, false)?;
        let backups = windows_open_child(&state, BACKUPS, true)?.ok_or(AdoptionError {
            reason: AdoptionReason::BackupCorrupt,
        })?;
        let destination = windows_open_child(&backups, id, true)?.ok_or(AdoptionError {
            reason: AdoptionReason::BackupCorrupt,
        })?;
        windows_read_limited(&destination, "manifest.json", MAX_MANIFEST_BYTES)?
    };
    let value: Value = serde_json::from_slice(&manifest).map_err(|_| AdoptionError {
        reason: AdoptionReason::BackupCorrupt,
    })?;
    if digest(&manifest) != id {
        return Err(AdoptionError {
            reason: AdoptionReason::BackupCorrupt,
        });
    }
    Ok(value.get("provenance").cloned())
}

pub(crate) fn verified_surface_access(
    state_root: &Path,
    id: &str,
    key: &str,
) -> Result<Option<crate::storage::Access>, AdoptionError> {
    let Some(provenance) = checkpoint_provenance(state_root, id)? else {
        return Ok(None);
    };
    let _ = provenance;
    #[cfg(unix)]
    let manifest = {
        let state = unix_open_dir_chain(state_root, false)?;
        let backups = unix_open_child_dir(&state, BACKUPS, false)?;
        let destination = unix_open_child_dir(&backups, id, false)?;
        unix_read_at_limited(&destination, "manifest.json", MAX_MANIFEST_BYTES)?
    };
    #[cfg(windows)]
    let manifest = {
        let state = windows_open_directory_chain(state_root, false)?;
        let backups = windows_open_child(&state, BACKUPS, true)?.ok_or(AdoptionError {
            reason: AdoptionReason::BackupCorrupt,
        })?;
        let destination = windows_open_child(&backups, id, true)?.ok_or(AdoptionError {
            reason: AdoptionReason::BackupCorrupt,
        })?;
        windows_read_limited(&destination, "manifest.json", MAX_MANIFEST_BYTES)?
    };
    let value: Value = serde_json::from_slice(&manifest).map_err(|_| AdoptionError {
        reason: AdoptionReason::BackupCorrupt,
    })?;
    if digest(&manifest) != id {
        return Err(AdoptionError {
            reason: AdoptionReason::BackupCorrupt,
        });
    }
    let files = value
        .get("files")
        .and_then(Value::as_object)
        .ok_or(AdoptionError {
            reason: AdoptionReason::BackupCorrupt,
        })?;
    validate_manifest_surface_metadata(value.get("surfaceMetadata"), files, true)?;
    let Some(access) = value["surfaceMetadata"][key]["access"]
        .clone()
        .as_object()
        .map(|_| value["surfaceMetadata"][key]["access"].clone())
    else {
        return Err(AdoptionError {
            reason: AdoptionReason::BackupUnverified,
        });
    };
    serde_json::from_value(access)
        .map(Some)
        .map_err(|_| AdoptionError {
            reason: AdoptionReason::BackupCorrupt,
        })
}

#[cfg(unix)]
pub(super) fn unix_check_private_dir(path: &std::fs::File) -> Result<(), AdoptionError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let metadata = path.metadata().map_err(unavailable)?;
    if metadata.permissions().mode() & 0o077 != 0 || metadata.uid() != unsafe { libc::geteuid() } {
        return Err(AdoptionError {
            reason: AdoptionReason::PathUnsafe,
        });
    }
    Ok(())
}

#[cfg(windows)]
fn backup_windows(
    files: &BTreeMap<String, Vec<u8>>,
    surface_metadata: &BTreeMap<&str, Value>,
    provenance: &Value,
    state_root: &Path,
) -> Result<Value, AdoptionError> {
    // The caller may place GraphHelm state below a shared application-data root. The retained,
    // owner-only `backups` child is the Windows privacy boundary; every checkpoint stays below it.
    let state = windows_open_directory_chain(state_root, true)?;
    let backups = windows_open_or_create_private_dir(&state, BACKUPS)?;
    let manifest = manifest_for(files, surface_metadata, provenance)?;
    let id = digest(&manifest);
    if let Some(destination) = windows_open_child(&backups, &id, true)? {
        windows_check_private(&destination)?;
        return verify_windows_dir(&destination, &id);
    }

    let pending_name = format!(".pending-{}", uuid::Uuid::new_v4());
    let pending = windows_create_child(&backups, &pending_name, true)?;
    graphhelm_sealed_key_provider::protect_owner_only(&pending).map_err(|_| AdoptionError {
        reason: AdoptionReason::CoverageIncomplete,
    })?;
    for (index, bytes) in files.values().enumerate() {
        windows_write_new(&pending, &format!("blob-{index}"), bytes)?;
    }
    windows_write_new(&pending, "manifest.json", &manifest)?;
    pending.sync_all().map_err(unavailable)?;
    windows_publish_directory(&pending, &backups, &id)?;
    pending.sync_all().map_err(unavailable)?;
    backups.sync_all().map_err(unavailable)?;
    Ok(
        json!({"apiVersion":"p50.dev/adoption/v1","kind":"BackupReceipt","id":id,"spec":{"verified":true,"restoreEligibility":"bound","fileCount":files.len()}}),
    )
}

#[cfg(windows)]
pub(super) fn windows_open_directory_chain(
    path: &Path,
    create: bool,
) -> Result<std::fs::File, AdoptionError> {
    windows_open_directory_chain_access(path, create, false)
}

#[cfg(windows)]
pub(super) fn windows_open_directory_chain_access(
    path: &Path,
    create: bool,
    mutable: bool,
) -> Result<std::fs::File, AdoptionError> {
    use std::path::Component;

    if !path.is_absolute() {
        return Err(AdoptionError {
            reason: AdoptionReason::PathUnsafe,
        });
    }
    let mut root_path = PathBuf::new();
    let mut normals = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => root_path.push(component.as_os_str()),
            Component::Normal(name) => normals.push(name.to_os_string()),
            Component::CurDir | Component::ParentDir => {
                return Err(AdoptionError {
                    reason: AdoptionReason::PathUnsafe,
                });
            }
        }
    }
    if normals.is_empty() {
        return Err(AdoptionError {
            reason: AdoptionReason::PathUnsafe,
        });
    }
    let mut current = open_windows_component(&root_path, true)?.ok_or(AdoptionError {
        reason: AdoptionReason::CoverageIncomplete,
    })?;
    let count = normals.len();
    for (index, name) in normals.into_iter().enumerate() {
        let name = name.to_str().ok_or(AdoptionError {
            reason: AdoptionReason::PathUnsafe,
        })?;
        current =
            match windows_nt_child(&current, name, true, false, mutable && index + 1 == count)? {
                Some(child) => child,
                None if create => {
                    let child = windows_create_child(&current, name, true)?;
                    graphhelm_sealed_key_provider::protect_owner_only(&child).map_err(|_| {
                        AdoptionError {
                            reason: AdoptionReason::CoverageIncomplete,
                        }
                    })?;
                    child
                }
                None => {
                    return Err(AdoptionError {
                        reason: AdoptionReason::CoverageIncomplete,
                    });
                }
            };
    }
    Ok(current)
}

#[cfg(windows)]
pub(super) fn windows_open_or_create_private_dir(
    parent: &std::fs::File,
    name: &str,
) -> Result<std::fs::File, AdoptionError> {
    let directory = match windows_open_mutable_directory(parent, name)? {
        Some(directory) => {
            windows_check_private(&directory)?;
            directory
        }
        None => {
            let directory = windows_create_child(parent, name, true)?;
            graphhelm_sealed_key_provider::protect_owner_only(&directory).map_err(|_| {
                AdoptionError {
                    reason: AdoptionReason::CoverageIncomplete,
                }
            })?;
            directory
        }
    };
    Ok(directory)
}

#[cfg(windows)]
pub(super) fn windows_check_private(file: &std::fs::File) -> Result<(), AdoptionError> {
    graphhelm_sealed_key_provider::verify_owner_only(file).map_err(|_| AdoptionError {
        reason: AdoptionReason::PathUnsafe,
    })
}

#[cfg(windows)]
pub(super) fn windows_open_child(
    parent: &std::fs::File,
    name: &str,
    directory: bool,
) -> Result<Option<std::fs::File>, AdoptionError> {
    windows_nt_child(parent, name, directory, false, false)
}

#[cfg(windows)]
fn windows_open_mutable_directory(
    parent: &std::fs::File,
    name: &str,
) -> Result<Option<std::fs::File>, AdoptionError> {
    windows_nt_child(parent, name, true, false, true)
}

#[cfg(windows)]
pub(super) fn windows_create_child(
    parent: &std::fs::File,
    name: &str,
    directory: bool,
) -> Result<std::fs::File, AdoptionError> {
    windows_nt_child(parent, name, directory, true, true)?.ok_or(AdoptionError {
        reason: AdoptionReason::CoverageIncomplete,
    })
}

#[cfg(windows)]
pub(super) fn windows_nt_child(
    parent: &std::fs::File,
    name: &str,
    directory: bool,
    create: bool,
    mutable: bool,
) -> Result<Option<std::fs::File>, AdoptionError> {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    windows_nt_child_shared(
        parent,
        name,
        directory,
        create,
        mutable,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
    )
}

#[cfg(windows)]
pub(super) fn windows_nt_child_shared(
    parent: &std::fs::File,
    name: &str,
    directory: bool,
    create: bool,
    mutable: bool,
    share: u32,
) -> Result<Option<std::fs::File>, AdoptionError> {
    use std::mem::{size_of, zeroed};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::MetadataExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
    use windows_sys::Wdk::Storage::FileSystem::{
        FILE_CREATE, FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN,
        FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
    };
    use windows_sys::Win32::Foundation::{HANDLE, UNICODE_STRING};
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_ADD_FILE, FILE_ADD_SUBDIRECTORY, FILE_ATTRIBUTE_NORMAL,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_READ_DATA,
        FILE_TRAVERSE, FILE_WRITE_ATTRIBUTES, FILE_WRITE_DATA, READ_CONTROL, SYNCHRONIZE,
        WRITE_DAC,
    };
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

    const STATUS_NO_SUCH_FILE: i32 = 0xC000_000Fu32 as i32;
    const STATUS_OBJECT_NAME_NOT_FOUND: i32 = 0xC000_0034u32 as i32;
    const STATUS_OBJECT_PATH_NOT_FOUND: i32 = 0xC000_003Au32 as i32;

    if name.is_empty() || matches!(name, "." | "..") || name.contains('/') || name.contains('\\') {
        return Err(AdoptionError {
            reason: AdoptionReason::PathUnsafe,
        });
    }
    let mut wide = std::ffi::OsStr::new(name).encode_wide().collect::<Vec<_>>();
    let byte_length = wide
        .len()
        .checked_mul(2)
        .and_then(|length| u16::try_from(length).ok())
        .ok_or(AdoptionError {
            reason: AdoptionReason::PathUnsafe,
        })?;
    let unicode = UNICODE_STRING {
        Length: byte_length,
        MaximumLength: byte_length,
        Buffer: wide.as_mut_ptr(),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent.as_raw_handle(),
        ObjectName: &unicode,
        Attributes: 0,
        SecurityDescriptor: std::ptr::null(),
        SecurityQualityOfService: std::ptr::null(),
    };
    let mut handle: HANDLE = std::ptr::null_mut();
    let mut io_status: IO_STATUS_BLOCK = unsafe { zeroed() };
    let access = if directory {
        FILE_LIST_DIRECTORY
            | FILE_TRAVERSE
            | FILE_READ_ATTRIBUTES
            | READ_CONTROL
            | SYNCHRONIZE
            | if create || mutable {
                FILE_ADD_FILE | FILE_ADD_SUBDIRECTORY | WRITE_DAC | if create { DELETE } else { 0 }
            } else {
                0
            }
    } else if create || mutable {
        DELETE
            | FILE_READ_DATA
            | FILE_WRITE_DATA
            | FILE_READ_ATTRIBUTES
            | FILE_WRITE_ATTRIBUTES
            | READ_CONTROL
            | WRITE_DAC
            | SYNCHRONIZE
    } else {
        FILE_READ_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE
    };
    let options = FILE_OPEN_REPARSE_POINT
        | FILE_SYNCHRONOUS_IO_NONALERT
        | if directory {
            FILE_DIRECTORY_FILE
        } else {
            FILE_NON_DIRECTORY_FILE
        };
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            access,
            &attributes,
            &mut io_status,
            std::ptr::null(),
            FILE_ATTRIBUTE_NORMAL,
            share,
            if create { FILE_CREATE } else { FILE_OPEN },
            options,
            std::ptr::null(),
            0,
        )
    };
    if status < 0 || handle.is_null() {
        if !create
            && matches!(
                status,
                STATUS_NO_SUCH_FILE | STATUS_OBJECT_NAME_NOT_FOUND | STATUS_OBJECT_PATH_NOT_FOUND
            )
        {
            return Ok(None);
        }
        #[cfg(test)]
        observe_nt_child_failure(status, directory, create, mutable, share);
        return Err(AdoptionError {
            reason: AdoptionReason::CoverageIncomplete,
        });
    }
    let file = unsafe { std::fs::File::from_raw_handle(handle) };
    let metadata = file.metadata().map_err(unavailable)?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || metadata.is_dir() != directory
        || (!directory && !metadata.is_file())
    {
        return Err(AdoptionError {
            reason: AdoptionReason::PathUnsafe,
        });
    }
    Ok(Some(file))
}

#[cfg(windows)]
fn windows_write_new(
    parent: &std::fs::File,
    name: &str,
    bytes: &[u8],
) -> Result<(), AdoptionError> {
    let mut file = windows_create_child(parent, name, false)?;
    graphhelm_sealed_key_provider::protect_owner_only(&file).map_err(|_| AdoptionError {
        reason: AdoptionReason::CoverageIncomplete,
    })?;
    file.write_all(bytes).map_err(unavailable)?;
    file.sync_all().map_err(unavailable)
}

#[cfg(windows)]
fn windows_publish_directory(
    source: &std::fs::File,
    parent: &std::fs::File,
    destination: &str,
) -> Result<(), AdoptionError> {
    use std::mem::{offset_of, size_of, zeroed};
    use std::os::windows::io::AsRawHandle;
    use std::time::{Duration, Instant};
    use windows_sys::Wdk::Storage::FileSystem::{
        FILE_RENAME_INFORMATION, FileRenameInformation, NtSetInformationFile,
    };
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

    let name = destination.encode_utf16().collect::<Vec<_>>();
    let bytes = offset_of!(FILE_RENAME_INFORMATION, FileName)
        .checked_add(name.len() * size_of::<u16>())
        .ok_or(AdoptionError {
            reason: AdoptionReason::CoverageIncomplete,
        })?;
    let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
    let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
    unsafe {
        (*info).Anonymous.ReplaceIfExists = false;
        (*info).RootDirectory = parent.as_raw_handle();
        (*info).FileNameLength = u32::try_from(name.len() * 2).map_err(|_| AdoptionError {
            reason: AdoptionReason::CoverageIncomplete,
        })?;
        std::ptr::copy_nonoverlapping(name.as_ptr(), (*info).FileName.as_mut_ptr(), name.len());
    }
    let mut io_status: IO_STATUS_BLOCK = unsafe { zeroed() };
    let source_identity = crate::storage::identity(source).map_err(|_| AdoptionError {
        reason: AdoptionReason::CoverageIncomplete,
    })?;
    let parent_identity = crate::storage::identity(parent).map_err(|_| AdoptionError {
        reason: AdoptionReason::CoverageIncomplete,
    })?;
    let mut retry_until = None;
    for attempt in 0..4 {
        let status = unsafe {
            NtSetInformationFile(
                source.as_raw_handle(),
                &mut io_status,
                info.cast_const().cast(),
                u32::try_from(bytes).map_err(|_| AdoptionError {
                    reason: AdoptionReason::CoverageIncomplete,
                })?,
                FileRenameInformation,
            )
        };
        if status >= 0 {
            return Ok(());
        }
        #[cfg(test)]
        observe_nt_publish_failure(status);
        // Start one shared retry window after the first failure observer returns. The
        // test observer may wait for its bounded release acknowledgement without
        // spending the product's 100 ms retry budget.
        let retry_until =
            retry_until.get_or_insert_with(|| Instant::now() + Duration::from_millis(100));
        let retryable = crate::storage::transient_rename_status(status);
        if !retryable || attempt == 3 || Instant::now() >= *retry_until {
            return Err(AdoptionError {
                reason: AdoptionReason::CoverageIncomplete,
            });
        }
        // The clock bounds whether another native call may begin. It cannot bound the
        // duration of NtSetInformationFile itself, so this is deliberately not a wall-clock
        // guarantee.
        std::thread::sleep(Duration::from_millis(20));
        if Instant::now() >= *retry_until
            || crate::storage::identity(source).map_err(|_| AdoptionError {
                reason: AdoptionReason::CoverageIncomplete,
            })? != source_identity
            || crate::storage::identity(parent).map_err(|_| AdoptionError {
                reason: AdoptionReason::CoverageIncomplete,
            })? != parent_identity
            || windows_open_child(parent, destination, true)
                .map_err(|_| AdoptionError {
                    reason: AdoptionReason::CoverageIncomplete,
                })?
                .is_some()
        {
            return Err(AdoptionError {
                reason: AdoptionReason::CoverageIncomplete,
            });
        }
    }
    Err(AdoptionError {
        reason: AdoptionReason::CoverageIncomplete,
    })
}

#[cfg(all(test, windows))]
mod windows_publish_tests {
    use super::{
        set_release_on_nt_publish_failure, take_nt_publish_failure, windows_create_child,
        windows_open_directory_chain, windows_publish_directory,
    };
    use graphhelm_protocols::adoption::AdoptionReason;
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};

    #[test]
    fn child_without_delete_share_reports_publish_status_then_succeeds_after_release() {
        let root = tempfile::tempdir().unwrap();
        let parent = windows_open_directory_chain(root.path(), true).unwrap();
        let source = windows_create_child(&parent, "pending", true).unwrap();
        let manifest = root.path().join("pending").join("manifest.json");
        let held = std::fs::OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(manifest)
            .unwrap();

        let error = windows_publish_directory(&source, &parent, "published").unwrap_err();
        assert_eq!(error.reason, AdoptionReason::CoverageIncomplete);
        let status = take_nt_publish_failure().unwrap();
        assert!(status == 0xc000_0022 || status == 0xc000_0043);

        drop(held);
        assert!(!root.path().join("published").exists());
        windows_publish_directory(&source, &parent, "published").unwrap();
        assert!(root.path().join("published").is_dir());
    }

    #[test]
    fn child_released_during_bounded_retry_is_published() {
        let root = tempfile::tempdir().unwrap();
        let parent = windows_open_directory_chain(root.path(), true).unwrap();
        let source = windows_create_child(&parent, "pending", true).unwrap();
        let manifest = root.path().join("pending").join("manifest.json");
        let held = std::fs::OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(manifest)
            .unwrap();
        let (release_signal, release_ready) = std::sync::mpsc::channel();
        let (release_ack, release_acknowledged) = std::sync::mpsc::channel();
        let release = std::thread::spawn(move || {
            release_ready.recv().unwrap();
            drop(held);
            release_ack.send(()).unwrap();
        });
        let publish_source = source.try_clone().unwrap();
        let publish_parent = parent.try_clone().unwrap();
        let publish = std::thread::spawn(move || {
            set_release_on_nt_publish_failure(Some(release_signal), Some(release_acknowledged));
            let result = windows_publish_directory(&publish_source, &publish_parent, "published");
            set_release_on_nt_publish_failure(None, None);
            (result, take_nt_publish_failure())
        });

        let (result, status) = publish.join().unwrap();
        result.unwrap();
        release.join().unwrap();
        let status = status.unwrap();
        assert!(status == 0xc000_0022 || status == 0xc000_0043);
        assert!(root.path().join("published").is_dir());
    }
}

#[cfg(windows)]
fn windows_read_limited(
    parent: &std::fs::File,
    name: &str,
    limit: u64,
) -> Result<Vec<u8>, AdoptionError> {
    let mut file = windows_open_child(parent, name, false)?.ok_or(AdoptionError {
        reason: AdoptionReason::BackupCorrupt,
    })?;
    let size = file
        .metadata()
        .map_err(|_| AdoptionError {
            reason: AdoptionReason::BackupCorrupt,
        })?
        .len();
    if size > limit {
        return Err(AdoptionError {
            reason: AdoptionReason::BackupCorrupt,
        });
    }
    bounded_read(&mut file, size, limit).map_err(|_| AdoptionError {
        reason: AdoptionReason::BackupCorrupt,
    })
}

#[cfg(windows)]
fn verify_windows_dir(root: &std::fs::File, id: &str) -> Result<Value, AdoptionError> {
    let manifest = windows_read_limited(root, "manifest.json", MAX_MANIFEST_BYTES)?;
    if digest(&manifest) != id {
        return Err(AdoptionError {
            reason: AdoptionReason::BackupCorrupt,
        });
    }
    let value: Value = serde_json::from_slice(&manifest).map_err(|_| AdoptionError {
        reason: AdoptionReason::BackupCorrupt,
    })?;
    validate_manifest_version(&value)?;
    let files = value
        .get("files")
        .and_then(Value::as_object)
        .ok_or(AdoptionError {
            reason: AdoptionReason::BackupCorrupt,
        })?;
    validate_manifest_files(files)?;
    validate_manifest_surface_metadata(
        value.get("surfaceMetadata"),
        files,
        value.get("provenance").is_some(),
    )?;
    if let Some(provenance) = value.get("provenance") {
        validate_provenance(provenance)?;
    }
    let mut total = 0_u64;
    for (index, (_, expected)) in files.iter().enumerate() {
        let expected = expected.as_str().ok_or(AdoptionError {
            reason: AdoptionReason::BackupCorrupt,
        })?;
        let blob = windows_read_limited(
            root,
            &format!("blob-{index}"),
            MAX_BACKUP_BYTES.saturating_sub(total),
        )?;
        total = total
            .checked_add(u64::try_from(blob.len()).map_err(|_| AdoptionError {
                reason: AdoptionReason::BackupCorrupt,
            })?)
            .ok_or(AdoptionError {
                reason: AdoptionReason::BackupCorrupt,
            })?;
        if digest(&blob) != expected {
            return Err(AdoptionError {
                reason: AdoptionReason::BackupCorrupt,
            });
        }
    }
    Ok(
        json!({"apiVersion":"p50.dev/adoption/v1","kind":"BackupReceipt","id":id,"spec":{"verified":true,"restoreEligibility":if value.get("provenance").is_some() {"bound"} else {"integrity_only"},"fileCount":files.len()}}),
    )
}

fn manifest_for(
    files: &BTreeMap<String, Vec<u8>>,
    surface_metadata: &BTreeMap<&str, Value>,
    provenance: &Value,
) -> Result<Vec<u8>, AdoptionError> {
    let hashes = files
        .iter()
        .map(|(name, bytes)| (name.clone(), digest(bytes)))
        .collect::<BTreeMap<_, _>>();
    serde_json::to_vec(&json!({"version":1,"files":hashes,"surfaceMetadata":surface_metadata,"provenance":provenance}))
        .map_err(|_| AdoptionError {
            reason: AdoptionReason::InvalidConfiguration,
        })
}

fn provenance_for(
    project: &crate::storage::Root,
    home: &crate::storage::Root,
    state: &crate::storage::Root,
) -> Result<Value, AdoptionError> {
    let binding = |record: &crate::storage::RootRecord| {
        let value = serde_json::to_value(record).map_err(|_| AdoptionError {
            reason: AdoptionReason::InvalidConfiguration,
        })?;
        let bytes =
            graphhelm_graph::canonical_content_bytes(&value).map_err(|_| AdoptionError {
                reason: AdoptionReason::InvalidConfiguration,
            })?;
        Ok::<String, AdoptionError>(crate::apply::digest(&bytes))
    };
    Ok(json!({
        "kind":"manual",
        "project": serde_json::to_value(&project.record).map_err(|_| AdoptionError { reason: AdoptionReason::InvalidConfiguration })?,
        "home": serde_json::to_value(&home.record).map_err(|_| AdoptionError { reason: AdoptionReason::InvalidConfiguration })?,
        "state": serde_json::to_value(&state.record).map_err(|_| AdoptionError { reason: AdoptionReason::InvalidConfiguration })?,
        "bindings": {"project":binding(&project.record)?,"home":binding(&home.record)?,"state":binding(&state.record)?}
    }))
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn access_digest_value(access: &crate::storage::Access) -> Result<String, AdoptionError> {
    let encoded = serde_json::to_vec(&access).map_err(|_| AdoptionError {
        reason: AdoptionReason::InvalidConfiguration,
    })?;
    Ok(digest(&encoded))
}

struct ObservedSurface {
    bytes: Vec<u8>,
    identity: crate::storage::Identity,
    access_digest: String,
    access: crate::storage::Access,
}

/// Read only below the retained authority root. The caller performs a second full pass before
/// publication to detect drift from writers outside the cooperative authority locks.
fn observe_surface(
    root: &crate::storage::Root,
    relative: &str,
    remaining: u64,
) -> Result<Option<ObservedSurface>, AdoptionError> {
    crate::storage::relative_ok(relative)?;
    root.verify()?;
    let Some(source) = root.source_optional(relative)? else {
        return Ok(None);
    };
    let file = &source.file;
    let identity = crate::storage::identity(file)?;
    let access = crate::storage::access(file)?;
    let stored_access_digest = access_digest_value(&access)?;
    let bytes = crate::storage::read_file(file, remaining.min(1024 * 1024))?;
    root.verify()?;
    let current = root.source_optional(relative)?.ok_or(AdoptionError {
        reason: AdoptionReason::Busy,
    })?;
    let current_access = crate::storage::access(&current.file)?;
    if crate::storage::identity(&current.file)? != identity
        || access_digest_value(&current_access)? != stored_access_digest
        || crate::storage::read_file(&current.file, remaining.min(1024 * 1024))? != bytes
    {
        return Err(AdoptionError {
            reason: AdoptionReason::Busy,
        });
    }
    Ok(Some(ObservedSurface {
        bytes,
        identity,
        access_digest: stored_access_digest,
        access,
    }))
}

fn read_bounded_regular(
    root: &Path,
    relative: &str,
    remaining: u64,
) -> Result<Option<Vec<u8>>, AdoptionError> {
    if relative.is_empty()
        || relative
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(AdoptionError {
            reason: AdoptionReason::PathUnsafe,
        });
    }
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
        let Some(mut current) = open_unix_root(root)? else {
            return Ok(None);
        };
        let components: Vec<&str> = relative.split('/').collect();
        for component in &components[..components.len() - 1] {
            let name = CString::new(*component).unwrap();
            // SAFETY: current is a retained directory; name is bounded and NUL terminated.
            let fd = unsafe {
                libc::openat(
                    current.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            if fd < 0 {
                let error = std::io::Error::last_os_error();
                return if error.kind() == std::io::ErrorKind::NotFound {
                    Ok(None)
                } else {
                    Err(map_open_error(error))
                };
            }
            current = unsafe { OwnedFd::from_raw_fd(fd) };
        }
        let name = CString::new(components[components.len() - 1]).unwrap();
        // SAFETY: final component is opened from the retained parent without following links.
        let fd = unsafe {
            libc::openat(
                current.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            let error = std::io::Error::last_os_error();
            return if error.kind() == std::io::ErrorKind::NotFound {
                Ok(None)
            } else {
                Err(map_open_error(error))
            };
        }
        let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
        let metadata = file.metadata().map_err(unavailable)?;
        if !metadata.is_file() {
            return Err(AdoptionError {
                reason: AdoptionReason::PathUnsafe,
            });
        }
        bounded_read(&mut file, metadata.len(), remaining).map(Some)
    }
    #[cfg(windows)]
    {
        let components: Vec<&str> = relative.split('/').collect();
        let mut current = windows_open_directory_chain(root, false)?;
        for component in &components[..components.len() - 1] {
            let Some(directory) = windows_open_child(&current, component, true)? else {
                return Ok(None);
            };
            current = directory;
        }
        let Some(mut file) = windows_open_child(&current, components[components.len() - 1], false)?
        else {
            return Ok(None);
        };
        let metadata = file.metadata().map_err(unavailable)?;
        if !metadata.is_file() {
            return Err(AdoptionError {
                reason: AdoptionReason::PathUnsafe,
            });
        }
        bounded_read(&mut file, metadata.len(), remaining).map(Some)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let path = root.join(relative);
        let metadata = std::fs::symlink_metadata(&path).map_err(unavailable)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(AdoptionError {
                reason: AdoptionReason::PathUnsafe,
            });
        }
        let mut file = OpenOptions::new()
            .read(true)
            .open(path)
            .map_err(unavailable)?;
        bounded_read(&mut file, metadata.len(), remaining).map(Some)
    }
}

#[cfg(unix)]
fn open_unix_root(root: &Path) -> Result<Option<std::os::fd::OwnedFd>, AdoptionError> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

    if !root.is_absolute() {
        return Err(AdoptionError {
            reason: AdoptionReason::PathUnsafe,
        });
    }
    // SAFETY: the static path is NUL terminated and the returned descriptor is owned below.
    let fd = unsafe {
        libc::open(
            c"/".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(unavailable(std::io::Error::last_os_error()));
    }
    let mut current = unsafe { OwnedFd::from_raw_fd(fd) };
    for component in root.components() {
        use std::path::Component;
        let Component::Normal(component) = component else {
            if matches!(component, Component::RootDir) {
                continue;
            }
            return Err(AdoptionError {
                reason: AdoptionReason::PathUnsafe,
            });
        };
        let name = CString::new(component.as_encoded_bytes()).map_err(|_| AdoptionError {
            reason: AdoptionReason::PathUnsafe,
        })?;
        // SAFETY: current pins the parent and name is NUL terminated.
        let next = unsafe {
            libc::openat(
                current.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if next < 0 {
            let error = std::io::Error::last_os_error();
            return if error.kind() == std::io::ErrorKind::NotFound {
                Ok(None)
            } else {
                Err(map_open_error(error))
            };
        }
        current = unsafe { OwnedFd::from_raw_fd(next) };
    }
    Ok(Some(current))
}

fn bounded_read(
    file: &mut std::fs::File,
    size: u64,
    remaining: u64,
) -> Result<Vec<u8>, AdoptionError> {
    if size > remaining {
        return Err(AdoptionError {
            reason: AdoptionReason::LimitExceeded,
        });
    }
    let mut bytes = Vec::with_capacity(usize::try_from(size).map_err(|_| AdoptionError {
        reason: AdoptionReason::LimitExceeded,
    })?);
    let mut bounded = file.take(remaining.saturating_add(1));
    bounded.read_to_end(&mut bytes).map_err(unavailable)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > remaining {
        return Err(AdoptionError {
            reason: AdoptionReason::LimitExceeded,
        });
    }
    Ok(bytes)
}

#[cfg(unix)]
fn map_open_error(error: std::io::Error) -> AdoptionError {
    // O_DIRECTORY | O_NOFOLLOW reports ENOTDIR for a symlink on Linux.
    if matches!(
        error.raw_os_error(),
        Some(libc::ELOOP | libc::EMLINK | libc::ENOTDIR)
    ) {
        AdoptionError {
            reason: AdoptionReason::PathUnsafe,
        }
    } else {
        unavailable(error)
    }
}

#[cfg(windows)]
fn open_windows_component(
    path: &Path,
    directory: bool,
) -> Result<Option<std::fs::File>, AdoptionError> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    let mut options = OpenOptions::new();
    if directory {
        options
            .access_mode(FILE_READ_ATTRIBUTES)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
    } else {
        options
            .read(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = match options
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(unavailable(error)),
    };
    let metadata = file.metadata().map_err(unavailable)?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(AdoptionError {
            reason: AdoptionReason::PathUnsafe,
        });
    }
    if metadata.is_dir() != directory {
        return Err(AdoptionError {
            reason: AdoptionReason::PathUnsafe,
        });
    }
    Ok(Some(file))
}

fn unavailable(_: std::io::Error) -> AdoptionError {
    AdoptionError {
        reason: AdoptionReason::CoverageIncomplete,
    }
}

/// Reads the selected blob only from the same retained snapshot directory that was verified.
pub(crate) fn verified_blob(
    state_root: &Path,
    id: &str,
    key: &str,
) -> Result<Vec<u8>, AdoptionError> {
    verified_optional_blob(state_root, id, key)?.ok_or(AdoptionError {
        reason: AdoptionReason::BackupUnverified,
    })
}

pub(crate) fn verified_optional_blob(
    state_root: &Path,
    id: &str,
    key: &str,
) -> Result<Option<Vec<u8>>, AdoptionError> {
    if !valid_backup_id(id) {
        return Err(AdoptionError {
            reason: AdoptionReason::PathUnsafe,
        });
    }
    #[cfg(unix)]
    let destination = {
        let state = unix_open_dir_chain(state_root, false)?;
        unix_check_private_dir(&state)?;
        let backups = unix_open_child_dir(&state, BACKUPS, false)?;
        unix_check_private_dir(&backups)?;
        let destination = unix_open_child_dir(&backups, id, false)?;
        unix_check_private_dir(&destination)?;
        verify_unix_dir(&destination, id)?;
        destination
    };
    #[cfg(windows)]
    let destination = {
        let state = windows_open_directory_chain(state_root, false)?;
        let backups = windows_open_child(&state, BACKUPS, true)?.ok_or(AdoptionError {
            reason: AdoptionReason::BackupCorrupt,
        })?;
        windows_check_private(&backups)?;
        let destination = windows_open_child(&backups, id, true)?.ok_or(AdoptionError {
            reason: AdoptionReason::BackupCorrupt,
        })?;
        windows_check_private(&destination)?;
        verify_windows_dir(&destination, id)?;
        destination
    };
    #[cfg(unix)]
    let manifest = unix_read_at_limited(&destination, "manifest.json", MAX_MANIFEST_BYTES)?;
    #[cfg(windows)]
    let manifest = windows_read_limited(&destination, "manifest.json", MAX_MANIFEST_BYTES)?;
    if digest(&manifest) != id {
        return Err(AdoptionError {
            reason: AdoptionReason::BackupCorrupt,
        });
    }
    let manifest: Value = serde_json::from_slice(&manifest).map_err(|_| AdoptionError {
        reason: AdoptionReason::BackupCorrupt,
    })?;
    let files = manifest["files"].as_object().ok_or(AdoptionError {
        reason: AdoptionReason::BackupCorrupt,
    })?;
    validate_manifest_files(files)?;
    let Some((index, (_, expected))) = files.iter().enumerate().find(|(_, (name, _))| *name == key)
    else {
        return Ok(None);
    };
    #[cfg(unix)]
    let bytes = unix_read_at_limited(&destination, &format!("blob-{index}"), 1024 * 1024)?;
    #[cfg(windows)]
    let bytes = windows_read_limited(&destination, &format!("blob-{index}"), 1024 * 1024)?;
    if expected.as_str() != Some(digest(&bytes).as_str()) {
        return Err(AdoptionError {
            reason: AdoptionReason::BackupCorrupt,
        });
    }
    Ok(Some(bytes))
}
