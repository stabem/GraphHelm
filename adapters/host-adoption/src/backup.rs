//! Private local snapshots for the supported adoption configuration surfaces.

use std::collections::BTreeMap;
#[cfg(windows)]
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use graphhelm_protocols::adoption::{AdoptionError, AdoptionReason};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const BACKUPS: &str = "backups";
/// A checkpoint is local recovery data, never an unbounded archive of a home directory.
const MAX_BACKUP_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_BACKUP_FILES: usize = 8;

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
    checked_directory(project)?;
    checked_directory(home)?;
    let mut files = BTreeMap::new();
    let mut total = 0_u64;
    for (name, path) in sources(project, home) {
        let Some((root, relative)) = source_root_and_relative(project, home, &path) else {
            continue;
        };
        let Some(bytes) =
            read_bounded_regular(&root, &relative, limit_bytes.saturating_sub(total))?
        else {
            continue;
        };
        total = total
            .checked_add(u64::try_from(bytes.len()).map_err(|_| AdoptionError {
                reason: AdoptionReason::LimitExceeded,
            })?)
            .ok_or(AdoptionError {
                reason: AdoptionReason::LimitExceeded,
            })?;
        if total > limit_bytes {
            return Err(AdoptionError {
                reason: AdoptionReason::LimitExceeded,
            });
        }
        files.insert(name, bytes);
    }
    #[cfg(unix)]
    {
        backup_unix(&files, state_root)
    }
    #[cfg(windows)]
    {
        backup_windows(&files, state_root)
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
        let backups = unix_open_child_dir(&state, BACKUPS, true)?;
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

fn sources(project: &Path, home: &Path) -> [(&'static str, PathBuf); 8] {
    [
        ("project/AGENTS.md", project.join("AGENTS.md")),
        ("project/CLAUDE.md", project.join("CLAUDE.md")),
        ("project/.mcp.json", project.join(".mcp.json")),
        (
            "project/.claude/settings.local.json",
            project.join(".claude/settings.local.json"),
        ),
        (
            "home/.claude/settings.json",
            home.join(".claude/settings.json"),
        ),
        ("home/.claude.json", home.join(".claude.json")),
        ("home/.codex/config.toml", home.join(".codex/config.toml")),
        ("home/AGENTS.md", home.join("AGENTS.md")),
    ]
}

fn source_root_and_relative(project: &Path, home: &Path, path: &Path) -> Option<(PathBuf, String)> {
    let (root, relative) = if path.starts_with(project) {
        (project, path.strip_prefix(project).ok()?)
    } else {
        (home, path.strip_prefix(home).ok()?)
    };
    let relative = relative.to_str()?.replace('\\', "/");
    Some((
        root.to_path_buf(),
        relative.trim_start_matches('/').to_owned(),
    ))
}

#[cfg(unix)]
fn backup_unix(files: &BTreeMap<&str, Vec<u8>>, state_root: &Path) -> Result<Value, AdoptionError> {
    use std::os::fd::AsRawFd;

    let state = unix_open_dir_chain(state_root, true)?;
    unix_check_private_dir(&state)?;
    let backups = unix_open_child_dir(&state, BACKUPS, true)?;
    unix_check_private_dir(&backups)?;
    let manifest = manifest_for(files)?;
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
        json!({"apiVersion":"p50.dev/adoption/v1","kind":"BackupReceipt","id":id,"spec":{"verified":true,"fileCount":files.len()}}),
    )
}

#[cfg(unix)]
fn unix_open_dir_chain(path: &Path, create: bool) -> Result<std::fs::File, AdoptionError> {
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
fn unix_open_child_dir(
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
fn unix_create_child_dir(
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
        json!({"apiVersion":"p50.dev/adoption/v1","kind":"BackupReceipt","id":id,"spec":{"verified":true,"fileCount":files.len()}}),
    )
}

fn validate_manifest_files(files: &serde_json::Map<String, Value>) -> Result<(), AdoptionError> {
    if files.len() > MAX_BACKUP_FILES {
        return Err(AdoptionError {
            reason: AdoptionReason::BackupCorrupt,
        });
    }
    for (name, expected) in files {
        if !matches!(
            name.as_str(),
            "project/AGENTS.md"
                | "project/CLAUDE.md"
                | "project/.mcp.json"
                | "project/.claude/settings.local.json"
                | "home/.claude/settings.json"
                | "home/.claude.json"
                | "home/.codex/config.toml"
                | "home/AGENTS.md"
        ) || !expected.as_str().is_some_and(valid_backup_id)
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

#[cfg(unix)]
fn unix_check_private_dir(path: &std::fs::File) -> Result<(), AdoptionError> {
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
    files: &BTreeMap<&str, Vec<u8>>,
    state_root: &Path,
) -> Result<Value, AdoptionError> {
    // The caller may place GraphHelm state below a shared application-data root. The retained,
    // owner-only `backups` child is the Windows privacy boundary; every checkpoint stays below it.
    let state = windows_open_directory_chain(state_root, true)?;
    let backups = windows_open_or_create_private_dir(&state, BACKUPS)?;
    let manifest = manifest_for(files)?;
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
        json!({"apiVersion":"p50.dev/adoption/v1","kind":"BackupReceipt","id":id,"spec":{"verified":true,"fileCount":files.len()}}),
    )
}

#[cfg(windows)]
fn windows_open_directory_chain(path: &Path, create: bool) -> Result<std::fs::File, AdoptionError> {
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
    for name in normals {
        let name = name.to_str().ok_or(AdoptionError {
            reason: AdoptionReason::PathUnsafe,
        })?;
        current = match windows_open_child(&current, name, true)? {
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
fn windows_open_or_create_private_dir(
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
fn windows_check_private(file: &std::fs::File) -> Result<(), AdoptionError> {
    graphhelm_sealed_key_provider::verify_owner_only(file).map_err(|_| AdoptionError {
        reason: AdoptionReason::PathUnsafe,
    })
}

#[cfg(windows)]
fn windows_open_child(
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
fn windows_create_child(
    parent: &std::fs::File,
    name: &str,
    directory: bool,
) -> Result<std::fs::File, AdoptionError> {
    windows_nt_child(parent, name, directory, true, true)?.ok_or(AdoptionError {
        reason: AdoptionReason::CoverageIncomplete,
    })
}

#[cfg(windows)]
fn windows_nt_child(
    parent: &std::fs::File,
    name: &str,
    directory: bool,
    create: bool,
    mutable: bool,
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
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE, FILE_WRITE_ATTRIBUTES,
        FILE_WRITE_DATA, READ_CONTROL, SYNCHRONIZE, WRITE_DAC,
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
                FILE_ADD_FILE | FILE_ADD_SUBDIRECTORY | WRITE_DAC | DELETE
            } else {
                0
            }
    } else if create {
        FILE_READ_DATA
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
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
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
    if status < 0 {
        Err(AdoptionError {
            reason: AdoptionReason::CoverageIncomplete,
        })
    } else {
        Ok(())
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
        json!({"apiVersion":"p50.dev/adoption/v1","kind":"BackupReceipt","id":id,"spec":{"verified":true,"fileCount":files.len()}}),
    )
}

fn manifest_for(files: &BTreeMap<&str, Vec<u8>>) -> Result<Vec<u8>, AdoptionError> {
    let hashes = files
        .iter()
        .map(|(name, bytes)| (*name, digest(bytes)))
        .collect::<BTreeMap<_, _>>();
    serde_json::to_vec(&json!({"version":1,"files":hashes})).map_err(|_| AdoptionError {
        reason: AdoptionReason::InvalidConfiguration,
    })
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn checked_directory(path: &Path) -> Result<(), AdoptionError> {
    let metadata = std::fs::symlink_metadata(path).map_err(unavailable)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(AdoptionError {
            reason: AdoptionReason::PathUnsafe,
        });
    }
    Ok(())
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
    if matches!(error.raw_os_error(), Some(libc::ELOOP | libc::EMLINK)) {
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
