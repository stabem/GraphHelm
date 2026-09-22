use std::io::Read;
use std::path::Path;

use graphhelm_protocols::adoption::{AdoptionError, AdoptionReason, Coverage};
use serde_json::{Value, json};

const MAX_FILE_BYTES: u64 = 1024 * 1024;

pub fn inventory(project: &Path, home: &Path) -> Result<Value, AdoptionError> {
    validate_root(project)?;
    validate_root(home)?;
    let claude = inspect_host(
        "claude",
        project,
        home,
        &[
            ".claude/settings.json",
            ".claude/settings.local.json",
            ".claude.json",
            ".mcp.json",
            "CLAUDE.md",
        ],
    )?;
    let codex = inspect_host("codex", project, home, &[".codex/config.toml", "AGENTS.md"])?;
    let coverage = [claude["coverage"].as_str(), codex["coverage"].as_str()]
        .iter()
        .map(|value| match *value {
            Some("complete") => Coverage::Complete,
            Some("truncated") => Coverage::Truncated,
            Some("inaccessible") => Coverage::Inaccessible,
            _ => Coverage::Unsupported,
        })
        .collect::<Vec<_>>();
    let document = json!({
        "apiVersion": "p50.dev/adoption/v1",
        "kind": "Inventory",
        "id": "inventory/local",
        "spec": {
            "roots": [{"id": "project", "scope": "project"}, {"id": "home", "scope": "user"}],
            "hosts": [claude, codex],
            "coverage": if graphhelm_protocols::adoption::coverage_complete(&coverage) { "complete" } else { "incomplete" }
        }
    });
    // This adapter constructs the closed envelope itself. The repository's public schema
    // catalog is intentionally frozen at 1.0.0; a new host-local report cannot silently
    // enlarge that published wire surface before its versioned contract is accepted.
    Ok(document)
}

fn inspect_host(
    host: &str,
    project: &Path,
    home: &Path,
    surfaces: &[&str],
) -> Result<Value, AdoptionError> {
    let mut items = Vec::new();
    let mut coverage = "complete";
    for surface in surfaces {
        let (root, relative) = candidate(host, project, home, surface);
        let Some(bytes) = read_candidate(root, relative)? else {
            continue;
        };
        if bytes.len() as u64 > MAX_FILE_BYTES {
            coverage = "truncated";
            items.push(json!({"kind": surface, "status": "truncated"}));
            continue;
        }
        let parse_status = parse_status(surface, &bytes);
        items.push(json!({"kind": surface, "status": parse_status}));
    }
    Ok(json!({"host": host, "coverage": coverage, "items": items}))
}

fn candidate<'a, 'b>(
    host: &str,
    project: &'a Path,
    home: &'a Path,
    surface: &'b str,
) -> (&'a Path, &'b str) {
    match surface {
        ".mcp.json" | "CLAUDE.md" | "AGENTS.md" | ".claude/settings.local.json" => {
            (project, surface)
        }
        ".claude.json" => (home, surface),
        _ if host == "claude" || host == "codex" => (home, surface),
        _ => (home, surface),
    }
}

fn validate_root(root: &Path) -> Result<(), AdoptionError> {
    let metadata = std::fs::symlink_metadata(root).map_err(|_| AdoptionError {
        reason: AdoptionReason::CoverageIncomplete,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(AdoptionError {
            reason: AdoptionReason::PathUnsafe,
        });
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(AdoptionError {
                reason: AdoptionReason::PathUnsafe,
            });
        }
    }
    open_root(root).map(|_| ()).map_err(|_| AdoptionError {
        reason: AdoptionReason::CoverageIncomplete,
    })
}

fn read_candidate(root: &Path, relative: &str) -> Result<Option<Vec<u8>>, AdoptionError> {
    let file = match open_beneath(root, relative) {
        Ok(Some(file)) => file,
        Ok(None) => return Ok(None),
        Err(reason) => return Err(AdoptionError { reason }),
    };
    let metadata = file.metadata().map_err(|_| AdoptionError {
        reason: AdoptionReason::CoverageIncomplete,
    })?;
    if !metadata.is_file() {
        return Err(AdoptionError {
            reason: AdoptionReason::PathUnsafe,
        });
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| AdoptionError {
            reason: AdoptionReason::CoverageIncomplete,
        })?;
    Ok(Some(bytes))
}

#[cfg(unix)]
fn open_root(root: &Path) -> std::io::Result<std::fs::File> {
    open_unix_root(root).map(std::fs::File::from)
}

#[cfg(all(not(unix), not(windows)))]
fn open_root(root: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::open(root)
}

#[cfg(windows)]
fn open_root(root: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    std::fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(root)
}

#[cfg(unix)]
fn open_unix_root(root: &Path) -> std::io::Result<std::os::fd::OwnedFd> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::path::Component;

    if !root.is_absolute() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "root must be absolute",
        ));
    }
    let fd = unsafe {
        libc::open(
            c"/".as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut current = unsafe { OwnedFd::from_raw_fd(fd) };
    for component in root.components() {
        let Component::Normal(component) = component else {
            if matches!(component, Component::RootDir) {
                continue;
            }
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "unsafe root component",
            ));
        };
        let name = CString::new(component.as_encoded_bytes()).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "NUL in root component")
        })?;
        let next = unsafe {
            libc::openat(
                current.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if next < 0 {
            return Err(std::io::Error::last_os_error());
        }
        current = unsafe { OwnedFd::from_raw_fd(next) };
    }
    Ok(current)
}

#[cfg(windows)]
fn open_windows_root_chain(root: &Path) -> Result<Vec<std::fs::File>, AdoptionReason> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use std::path::Component;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    if !root.is_absolute() {
        return Err(AdoptionReason::PathUnsafe);
    }
    let mut current = std::path::PathBuf::new();
    let mut retained = Vec::new();
    for component in root.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                current.push(component.as_os_str());
                if !matches!(component, Component::RootDir) {
                    continue;
                }
            }
            Component::Normal(_) => current.push(component.as_os_str()),
            Component::CurDir | Component::ParentDir => return Err(AdoptionReason::PathUnsafe),
        }
        let handle = std::fs::OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&current)
            .map_err(|_| AdoptionReason::CoverageIncomplete)?;
        let metadata = handle
            .metadata()
            .map_err(|_| AdoptionReason::CoverageIncomplete)?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 || !metadata.is_dir() {
            return Err(AdoptionReason::PathUnsafe);
        }
        retained.push(handle);
    }
    Ok(retained)
}

#[cfg(unix)]
fn open_beneath(root: &Path, relative: &str) -> Result<Option<std::fs::File>, AdoptionReason> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

    let mut current = open_unix_root(root).map_err(|error| match error.raw_os_error() {
        Some(libc::ELOOP) | Some(libc::ENOTDIR) => AdoptionReason::PathUnsafe,
        _ => AdoptionReason::CoverageIncomplete,
    })?;
    let components: Vec<&str> = relative.split('/').collect();
    for (index, component) in components.iter().enumerate() {
        let name = CString::new(*component).map_err(|_| AdoptionReason::PathUnsafe)?;
        let flags = if index + 1 == components.len() {
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC
        } else {
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC
        };
        let fd = unsafe { libc::openat(current.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return match std::io::Error::last_os_error().raw_os_error() {
                Some(libc::ENOENT) => Ok(None),
                Some(libc::ELOOP) | Some(libc::ENOTDIR) => Err(AdoptionReason::PathUnsafe),
                _ => Err(AdoptionReason::CoverageIncomplete),
            };
        }
        current = unsafe { OwnedFd::from_raw_fd(fd) };
    }
    Ok(Some(std::fs::File::from(current)))
}

#[cfg(windows)]
fn open_beneath(root: &Path, relative: &str) -> Result<Option<std::fs::File>, AdoptionReason> {
    use std::mem::{size_of, zeroed};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::MetadataExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
    use windows_sys::Wdk::Storage::FileSystem::{
        FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_REPARSE_POINT,
        FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
    };
    use windows_sys::Win32::Foundation::{HANDLE, UNICODE_STRING};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_READ_DATA,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE, READ_CONTROL,
        SYNCHRONIZE,
    };
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

    enum ChildOpenError {
        Missing,
        Reason(AdoptionReason),
    }

    fn open_child(
        parent: &std::fs::File,
        name: &std::ffi::OsStr,
        directory: bool,
    ) -> Result<std::fs::File, ChildOpenError> {
        let mut wide = name.encode_wide().collect::<Vec<_>>();
        let byte_length = wide
            .len()
            .checked_mul(2)
            .and_then(|length| u16::try_from(length).ok())
            .ok_or(ChildOpenError::Reason(AdoptionReason::PathUnsafe))?;
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
        let mut status: IO_STATUS_BLOCK = unsafe { zeroed() };
        let access = if directory {
            FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE
        } else {
            FILE_READ_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE
        };
        let result = unsafe {
            NtCreateFile(
                &mut handle,
                access,
                &attributes,
                &mut status,
                std::ptr::null(),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                FILE_OPEN,
                FILE_OPEN_REPARSE_POINT
                    | FILE_SYNCHRONOUS_IO_NONALERT
                    | if directory {
                        FILE_DIRECTORY_FILE
                    } else {
                        FILE_NON_DIRECTORY_FILE
                    },
                std::ptr::null(),
                0,
            )
        };
        if result < 0 || handle.is_null() {
            return if result == 0xC000_0034_u32 as i32
                || result == 0xC000_003A_u32 as i32
                || result == 0xC000_000F_u32 as i32
            {
                Err(ChildOpenError::Missing)
            } else {
                Err(ChildOpenError::Reason(AdoptionReason::CoverageIncomplete))
            };
        }
        let file = unsafe { std::fs::File::from_raw_handle(handle) };
        let metadata = file
            .metadata()
            .map_err(|_| ChildOpenError::Reason(AdoptionReason::CoverageIncomplete))?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || metadata.is_dir() != directory
            || (!directory && !metadata.is_file())
        {
            return Err(ChildOpenError::Reason(AdoptionReason::PathUnsafe));
        }
        Ok(file)
    }

    let mut current = open_windows_root_chain(root)?
        .pop()
        .ok_or(AdoptionReason::CoverageIncomplete)?;
    let components: Vec<&str> = relative.split('/').collect();
    for (index, component) in components.iter().enumerate() {
        let child = match open_child(
            &current,
            std::ffi::OsStr::new(component),
            index + 1 != components.len(),
        ) {
            Ok(child) => child,
            Err(ChildOpenError::Missing) => return Ok(None),
            Err(ChildOpenError::Reason(reason)) => return Err(reason),
        };
        current = child;
    }
    Ok(Some(current))
}

fn parse_status(surface: &str, bytes: &[u8]) -> &'static str {
    if surface.ends_with(".json") {
        if serde_json::from_slice::<Value>(bytes).is_ok() {
            "parsed"
        } else {
            "invalid"
        }
    } else if surface.ends_with(".toml") {
        if std::str::from_utf8(bytes)
            .ok()
            .and_then(|text| toml::from_str::<toml::Value>(text).ok())
            .is_some()
        {
            "parsed"
        } else {
            "invalid"
        }
    } else {
        "observed"
    }
}
