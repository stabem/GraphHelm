//! Descriptor-relative atomic publication for Unix.
//!
//! The caller must retain an open descriptor for the output parent for the
//! whole operation.  All temporary and final path operations are relative to
//! that descriptor, so renaming the parent path or swapping an entry in the
//! writable parent cannot redirect the operation through a new parent.
//!
//! The private staging directory is intentionally left empty after a
//! successful publication.  Unix has no portable operation which removes a
//! directory by retained descriptor; removing it by name after checking its
//! identity would re-open a race with an attacker replacing that name.  A
//! caller may clean these directories only under a separate trusted policy.

#![cfg(unix)]

use std::ffi::{CStr, CString, OsStr};
use std::fs::File;
use std::io::{self, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

const STAGE_MODE: libc::mode_t = 0o700;
const FILE_MODE: libc::mode_t = 0o600;
const MAX_ANCESTRY_DEPTH: usize = 256;
static NEXT_STAGE: AtomicU64 = AtomicU64::new(0);

/// Publish `bytes` as `destination`, relative to the retained `parent` fd.
///
/// The destination rename is atomic.  The temporary file is created with
/// `O_EXCL|O_NOFOLLOW` inside the fixed private staging directory, and all
/// subsequent operations use the retained directory fd.  If publication
/// fails, the temporary file is removed through that retained fd; the empty
/// private directory is deliberately retained because safe portable cleanup
/// by descriptor is unavailable.
pub(crate) fn publish_bytes(parent: &File, destination: &OsStr, bytes: &[u8]) -> io::Result<()> {
    let destination = c_string(destination, "destination")?;
    let temp_name = CString::new(format!(
        "payload-{:08x}-{:016x}",
        unsafe { libc::getpid() as u64 },
        NEXT_STAGE.fetch_add(1, Ordering::Relaxed)
    ))
    .expect("generated payload name has no NUL");
    publish_bytes_with_temp_name(parent, &destination, &temp_name, bytes)
}

fn publish_bytes_with_temp_name(
    parent: &File,
    destination: &CStr,
    temp_name: &CStr,
    bytes: &[u8],
) -> io::Result<()> {
    let parent_fd = parent.as_raw_fd();
    let stage = create_stage(parent_fd)?;
    let stage_fd = stage.as_raw_fd();
    (|| {
        let raw = unsafe {
            libc::openat(
                stage_fd,
                temp_name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                FILE_MODE,
            )
        };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut output = unsafe { File::from_raw_fd(raw) };
        if let Err(error) = output.write_all(bytes).and_then(|_| output.sync_all()) {
            let _ = unlink_file(stage_fd, temp_name);
            return Err(error);
        }
        drop(output);

        let renamed = unsafe {
            libc::renameat(
                stage_fd,
                temp_name.as_ptr(),
                parent_fd,
                destination.as_ptr(),
            )
        };
        if renamed != 0 {
            let error = io::Error::last_os_error();
            let _ = unlink_file(stage_fd, temp_name);
            return Err(error);
        }

        // Persist the directory entry update as well as the file contents.
        sync_fd(parent_fd)
    })()
}

fn create_stage(parent_fd: libc::c_int) -> io::Result<File> {
    let name = CString::new(".keel-stage").expect("static name has no NUL");
    let made = unsafe { libc::mkdirat(parent_fd, name.as_ptr(), STAGE_MODE) };
    if made != 0 {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::AlreadyExists {
            return Err(error);
        }
    }

    let raw = unsafe {
        libc::openat(
            parent_fd,
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    let stage = unsafe { File::from_raw_fd(raw) };
    if let Err(error) = validate_private_stage(stage.as_raw_fd()) {
        drop(stage);
        return Err(error);
    }
    Ok(stage)
}

fn validate_private_stage(fd: libc::c_int) -> io::Result<()> {
    let mut metadata = unsafe { std::mem::zeroed::<libc::stat>() };
    if unsafe { libc::fstat(fd, &mut metadata) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let mode = metadata.st_mode as libc::mode_t;
    let is_directory = mode & libc::S_IFMT as libc::mode_t == libc::S_IFDIR as libc::mode_t;
    let is_private = mode & 0o077 == 0;
    let is_owned = metadata.st_uid == unsafe { libc::geteuid() };
    if is_directory && is_private && is_owned {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Keel staging entry is not a private directory owned by the effective user",
        ))
    }
}

fn c_string(value: &OsStr, label: &str) -> io::Result<CString> {
    CString::new(value.as_bytes()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{label} contains an embedded NUL"),
        )
    })
}

fn unlink_file(parent_fd: libc::c_int, name: &CStr) -> io::Result<()> {
    let result = unsafe { libc::unlinkat(parent_fd, name.as_ptr(), 0) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn sync_fd(fd: libc::c_int) -> io::Result<()> {
    if unsafe { libc::fsync(fd) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Return whether `parent` is the repository root or one of its descendants.
///
/// The ancestry walk is descriptor-relative.  It does not reconstruct a path
/// from `parent`, so a rename or alias swap after acquisition cannot redirect
/// the check.  Failure to open or inspect any ancestor is an error rather than
/// an outside-repository result, allowing callers to fail closed.
pub(crate) fn parent_is_within_repo(parent: &File, repo_root: &Path) -> io::Result<bool> {
    use std::os::unix::fs::OpenOptionsExt;

    let repo = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(repo_root)?;
    let repo_identity = file_identity(repo.as_raw_fd())?;
    let mut current = parent.try_clone()?;
    let dotdot = CString::new("..").expect("static parent name has no NUL");

    for _ in 0..=MAX_ANCESTRY_DEPTH {
        if file_identity(current.as_raw_fd())? == repo_identity {
            return Ok(true);
        }
        let raw = unsafe {
            libc::openat(
                current.as_raw_fd(),
                dotdot.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        let ancestor = unsafe { File::from_raw_fd(raw) };
        if file_identity(ancestor.as_raw_fd())? == file_identity(current.as_raw_fd())? {
            return Ok(false);
        }
        current = ancestor;
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "repository ancestry exceeded the safety bound",
    ))
}

fn file_identity(fd: libc::c_int) -> io::Result<(libc::dev_t, libc::ino_t)> {
    let mut metadata = unsafe { std::mem::zeroed::<libc::stat>() };
    if unsafe { libc::fstat(fd, &mut metadata) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((metadata.st_dev, metadata.st_ino))
}

#[cfg(test)]
mod tests {
    use super::{parent_is_within_repo, publish_bytes, publish_bytes_with_temp_name};
    use std::ffi::{CString, OsStr};
    use std::fs::{self, OpenOptions};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use tempfile::tempdir;

    #[test]
    fn publishes_from_retained_parent_after_parent_path_is_renamed() {
        // Observable contract: a retained parent fd remains the publication
        // boundary after its path is renamed.  This catches implementations
        // that reconstruct the parent path for staging or cleanup, which can
        // follow an attacker-controlled alias.
        let root = tempdir().unwrap();
        let original = root.path().join("output");
        let alias = root.path().join("alias");
        fs::create_dir(&original).unwrap();
        let parent = OpenOptions::new().read(true).open(&original).unwrap();
        fs::rename(&original, &alias).unwrap();
        fs::create_dir(&original).unwrap();

        publish_bytes(&parent, OsStr::new("index.json"), b"keel").unwrap();

        assert_eq!(fs::read(alias.join("index.json")).unwrap(), b"keel");
        assert!(!original.join("index.json").exists());
        publish_bytes(&parent, OsStr::new("second.json"), b"keel-2").unwrap();

        let entries = fs::read_dir(&alias)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        let stage = entries
            .iter()
            .find(|path| path.file_name().and_then(|name| name.to_str()) == Some(".keel-stage"))
            .unwrap();
        let metadata = fs::metadata(stage).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
        assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
        assert_eq!(
            entries
                .iter()
                .filter(
                    |path| path.file_name().and_then(|name| name.to_str()) == Some(".keel-stage")
                )
                .count(),
            1
        );
        assert_eq!(fs::read_dir(stage).unwrap().count(), 0);
        assert_eq!(fs::read(alias.join("second.json")).unwrap(), b"keel-2");
    }

    #[test]
    fn refuses_preplanted_temp_entry_without_unlinking_it() {
        // Observable contract: an attacker-controlled file already occupying
        // the selected temporary name is never opened, overwritten, or
        // removed by publication cleanup.
        let root = tempdir().unwrap();
        let parent_path = root.path().join("output");
        fs::create_dir(&parent_path).unwrap();
        let stage_path = parent_path.join(".keel-stage");
        fs::create_dir(&stage_path).unwrap();
        fs::set_permissions(&stage_path, fs::Permissions::from_mode(0o700)).unwrap();
        let planted_path = stage_path.join("chosen-payload");
        fs::write(&planted_path, b"attacker-bytes").unwrap();
        fs::write(parent_path.join("index.json"), b"old-index").unwrap();
        let parent = OpenOptions::new().read(true).open(&parent_path).unwrap();
        let destination = CString::new("index.json").unwrap();
        let temp_name = CString::new("chosen-payload").unwrap();

        let error = publish_bytes_with_temp_name(&parent, &destination, &temp_name, b"new-index")
            .unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&planted_path).unwrap(), b"attacker-bytes");
        assert_eq!(
            fs::read(parent_path.join("index.json")).unwrap(),
            b"old-index"
        );
    }

    #[test]
    fn ancestry_check_distinguishes_repo_descendant_from_outside_directory() {
        let root = tempdir().unwrap();
        let repo = root.path().join("repo");
        let inside = repo.join("nested");
        let outside = root.path().join("outside");
        fs::create_dir(&repo).unwrap();
        fs::create_dir(&inside).unwrap();
        fs::create_dir(&outside).unwrap();

        let inside_handle = OpenOptions::new().read(true).open(&inside).unwrap();
        let outside_handle = OpenOptions::new().read(true).open(&outside).unwrap();
        assert!(parent_is_within_repo(&inside_handle, &repo).unwrap());
        assert!(!parent_is_within_repo(&outside_handle, &repo).unwrap());
    }
}
