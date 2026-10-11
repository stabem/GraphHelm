//! Bounded, immutable index snapshots.
//!
//! Snapshot input is untrusted. Limits are enforced while walking, opening and reading; a
//! metadata preflight alone cannot stop a file growing during the operation.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use graphhelm_tool_broker::path::{MAX_PATH_BYTES, RelativePath};
use graphhelm_tool_broker::record::digest_hex;
use sha2::{Digest, Sha256};

use crate::process::HostError;
use crate::source_reader::open_snapshot_file;

/// Conservative limits for an index snapshot or selected source manifest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SnapshotLimits {
    pub max_entries: usize,
    pub max_files: usize,
    pub max_depth: usize,
    pub max_total_bytes: u64,
    pub max_file_bytes: u64,
}

impl Default for SnapshotLimits {
    fn default() -> Self {
        Self {
            max_entries: 50_000,
            max_files: 50_000,
            max_depth: 64,
            max_total_bytes: 512 * 1024 * 1024,
            max_file_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinnedSnapshot {
    root: PathBuf,
    generation: String,
}

impl PinnedSnapshot {
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn generation(&self) -> &str {
        &self.generation
    }
}

pub fn pin_snapshot(source: &Path, tier1_root: &Path) -> Result<PinnedSnapshot, HostError> {
    // The broker chooses these roots from its provisioned Tier 1 workspace. Descendants are
    // untrusted and are checked below; the caller remains responsible for keeping the selected
    // root and its existing ancestors stable for the duration of the operation.
    pin_snapshot_with_limits(source, tier1_root, SnapshotLimits::default())
}

fn pin_snapshot_with_limits(
    source: &Path,
    tier1_root: &Path,
    limits: SnapshotLimits,
) -> Result<PinnedSnapshot, HostError> {
    let destination = tier1_root.join(".index-snapshot");
    if destination.exists() {
        return Err(HostError::Config {
            rule: "the workspace already holds an index snapshot",
        });
    }
    copy_tree_with_limits(source, &destination, limits)?;
    let generation = tree_generation_with_limits(&destination, limits)?;
    Ok(PinnedSnapshot {
        root: destination,
        generation,
    })
}

pub fn verify_pinned(root: &Path, expected_generation: &str) -> Result<(), HostError> {
    let actual = tree_generation_with_limits(root, SnapshotLimits::default())?;
    if actual != expected_generation {
        return Err(HostError::SnapshotMismatch {
            expected: expected_generation.to_owned(),
            actual,
        });
    }
    Ok(())
}

/// Hash selected workspace-relative regular files using the same bounded, no-follow reads as
/// snapshot pinning. Keys are canonical relative paths and values are raw lowercase SHA-256.
/// The selected root is a provisioned workspace boundary; callers must not replace that root or
/// its pre-existing ancestors while this operation runs.
pub fn file_manifest(
    root: &Path,
    relative_paths: &[String],
) -> Result<BTreeMap<String, String>, HostError> {
    let limits = SnapshotLimits::default();
    if relative_paths.len() > limits.max_files {
        return Err(bound("snapshot file count exceeds the configured limit"));
    }
    let mut result = BTreeMap::new();
    let mut total = 0u64;
    for text in relative_paths {
        let relative = RelativePath::parse(text).map_err(|_| HostError::Escape)?;
        if relative.as_str().split('/').count() > limits.max_depth + 1 {
            return Err(bound(
                "snapshot directory depth exceeds the configured limit",
            ));
        }
        let mut per_file = limits;
        per_file.max_file_bytes = per_file.max_file_bytes.min(limits.max_total_bytes - total);
        let (_file, _guards, digest, bytes) = open_and_digest(root, relative.as_str(), &per_file)?;
        total = total
            .checked_add(bytes)
            .ok_or_else(|| bound("snapshot byte count overflowed"))?;
        if total > limits.max_total_bytes {
            return Err(bound("snapshot bytes exceed the configured limit"));
        }
        if result.insert(text.clone(), digest).is_some() {
            return Err(HostError::Config {
                rule: "snapshot manifest contains a duplicate path",
            });
        }
    }
    Ok(result)
}

pub(crate) fn copy_tree(source: &Path, destination: &Path) -> Result<(), HostError> {
    copy_tree_with_limits(source, destination, SnapshotLimits::default())
}

fn copy_tree_with_limits(
    source: &Path,
    destination: &Path,
    limits: SnapshotLimits,
) -> Result<(), HostError> {
    fs::create_dir_all(destination).map_err(|source| HostError::Prepare { source })?;
    let mut state = WalkState {
        entries: 0,
        files: 0,
        bytes: 0,
        limits,
    };
    let root_directory = open_directory(source)?;
    copy_dir(
        source,
        source,
        destination,
        "",
        0,
        &mut state,
        root_directory,
    )?;
    if state.files == 0 {
        return Err(HostError::Config {
            rule: "an index snapshot cannot be empty",
        });
    }
    Ok(())
}

struct WalkState {
    entries: usize,
    files: usize,
    bytes: u64,
    limits: SnapshotLimits,
}

fn copy_dir(
    source_root: &Path,
    source: &Path,
    destination: &Path,
    relative_prefix: &str,
    depth: usize,
    state: &mut WalkState,
    directory: File,
) -> Result<(), HostError> {
    if depth > state.limits.max_depth {
        return Err(bound(
            "snapshot directory depth exceeds the configured limit",
        ));
    }
    let names_remaining = state.limits.max_entries.saturating_sub(state.entries);
    let names = directory_entries(source, &directory, names_remaining)?;
    // Parent name buffers remain alive during recursion, so charge the entire buffer now.
    state.entries = state
        .entries
        .checked_add(names.len())
        .ok_or_else(|| bound("snapshot entry count overflowed"))?;
    if state.entries > state.limits.max_entries {
        return Err(bound("snapshot entry count exceeds the configured limit"));
    }
    for file_name in names {
        let path = source.join(&file_name);
        let target = destination.join(&file_name);
        let name = file_name.to_str().ok_or(HostError::Config {
            rule: "snapshot paths must be valid UTF-8",
        })?;
        let path_bytes = relative_prefix
            .len()
            .checked_add(if relative_prefix.is_empty() { 0 } else { 1 })
            .and_then(|bytes| bytes.checked_add(name.len()))
            .ok_or_else(|| bound("snapshot path length overflowed"))?;
        if path_bytes > MAX_PATH_BYTES {
            return Err(bound("snapshot path exceeds the configured limit"));
        }
        let relative = if relative_prefix.is_empty() {
            name.to_owned()
        } else {
            format!("{relative_prefix}/{name}")
        };
        let _ = RelativePath::parse(&relative).map_err(|_| HostError::Escape)?;
        let metadata =
            fs::symlink_metadata(&path).map_err(|source| HostError::Prepare { source })?;
        reject_reparse(&metadata)?;
        if metadata.is_dir() {
            fs::create_dir(&target).map_err(|source| HostError::Prepare { source })?;
            let child_directory = open_directory_child(&directory, &file_name, &path)?;
            copy_dir(
                source_root,
                &path,
                &target,
                &relative,
                depth + 1,
                state,
                child_directory,
            )?;
        } else if metadata.is_file() {
            state.files = state
                .files
                .checked_add(1)
                .ok_or_else(|| bound("snapshot file count overflowed"))?;
            if state.files > state.limits.max_files {
                return Err(bound("snapshot file count exceeds the configured limit"));
            }
            let relative_path = RelativePath::parse(&relative).map_err(|_| HostError::Escape)?;
            let (mut input, _guards) = open_snapshot_file(source_root, &relative_path)?;
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)
                .map_err(|source| HostError::Prepare { source })?;
            let remaining = state
                .limits
                .max_total_bytes
                .checked_sub(state.bytes)
                .ok_or_else(|| bound("snapshot bytes exceed the configured limit"))?;
            let bytes = copy_stream_hashed(
                &mut input,
                &mut output,
                state.limits.max_file_bytes.min(remaining),
            )?;
            state.bytes = state
                .bytes
                .checked_add(bytes)
                .ok_or_else(|| bound("snapshot byte count overflowed"))?;
            if state.bytes > state.limits.max_total_bytes {
                return Err(bound("snapshot bytes exceed the configured limit"));
            }
            output
                .flush()
                .map_err(|source| HostError::Prepare { source })?;
            output
                .sync_all()
                .map_err(|source| HostError::Prepare { source })?;
        } else {
            return Err(HostError::Config {
                rule: "snapshot contains a non-regular filesystem entry",
            });
        }
    }
    Ok(())
}

pub(crate) fn tree_generation(root: &Path) -> Result<String, HostError> {
    tree_generation_with_limits(root, SnapshotLimits::default())
}

fn tree_generation_with_limits(root: &Path, limits: SnapshotLimits) -> Result<String, HostError> {
    let mut files = Vec::new();
    let mut state = WalkState {
        entries: 0,
        files: 0,
        bytes: 0,
        limits,
    };
    let root_directory = open_directory(root)?;
    collect_files(root, root, 0, &mut files, &mut state, root_directory)?;
    if files.is_empty() {
        return Err(HostError::Config {
            rule: "an index snapshot cannot be empty",
        });
    }
    files.sort_unstable();
    let mut lines = String::new();
    for relative in files {
        let mut per_file = limits;
        per_file.max_file_bytes = per_file
            .max_file_bytes
            .min(limits.max_total_bytes - state.bytes);
        let (_file, _guards, digest, bytes) = open_and_digest(root, &relative, &per_file)?;
        state.bytes = state
            .bytes
            .checked_add(bytes)
            .ok_or_else(|| bound("snapshot byte count overflowed"))?;
        if state.bytes > limits.max_total_bytes {
            return Err(bound("snapshot bytes exceed the configured limit"));
        }
        lines.push_str(&relative);
        lines.push(':');
        lines.push_str(&digest);
        lines.push('\n');
    }
    Ok(format!("sha256-{}", digest_hex(lines.as_bytes())))
}

fn collect_files(
    root: &Path,
    current: &Path,
    depth: usize,
    files: &mut Vec<String>,
    state: &mut WalkState,
    directory: File,
) -> Result<(), HostError> {
    if depth > state.limits.max_depth {
        return Err(bound(
            "snapshot directory depth exceeds the configured limit",
        ));
    }
    let names_remaining = state.limits.max_entries.saturating_sub(state.entries);
    let names = directory_entries(current, &directory, names_remaining)?;
    // Parent name buffers remain alive during recursion, so charge the entire buffer now.
    state.entries = state
        .entries
        .checked_add(names.len())
        .ok_or_else(|| bound("snapshot entry count overflowed"))?;
    if state.entries > state.limits.max_entries {
        return Err(bound("snapshot entry count exceeds the configured limit"));
    }
    for file_name in names {
        let path = current.join(&file_name);
        let metadata =
            fs::symlink_metadata(&path).map_err(|source| HostError::Prepare { source })?;
        reject_reparse(&metadata)?;
        if metadata.is_dir() {
            let child_directory = open_directory_child(&directory, &file_name, &path)?;
            collect_files(root, &path, depth + 1, files, state, child_directory)?;
        } else if metadata.is_file() {
            state.files = state
                .files
                .checked_add(1)
                .ok_or_else(|| bound("snapshot file count overflowed"))?;
            if state.files > state.limits.max_files {
                return Err(bound("snapshot file count exceeds the configured limit"));
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|_| HostError::Escape)?
                .to_str()
                .ok_or(HostError::Config {
                    rule: "snapshot paths must be valid UTF-8",
                })?
                .replace('\\', "/");
            files.push(relative);
        } else {
            return Err(HostError::Config {
                rule: "snapshot contains a non-regular filesystem entry",
            });
        }
    }
    Ok(())
}

fn open_and_digest(
    root: &Path,
    relative_text: &str,
    limits: &SnapshotLimits,
) -> Result<(File, Vec<File>, String, u64), HostError> {
    let relative = RelativePath::parse(relative_text).map_err(|_| HostError::Escape)?;
    let (mut file, guards) = open_snapshot_file(root, &relative)?;
    let mut hasher = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let remaining = limits
            .max_file_bytes
            .saturating_sub(bytes)
            .saturating_add(1);
        let chunk = usize::try_from(remaining)
            .unwrap_or(usize::MAX)
            .min(buffer.len());
        let count = file
            .read(&mut buffer[..chunk])
            .map_err(|source| HostError::Prepare { source })?;
        if count == 0 {
            break;
        }
        bytes = bytes
            .checked_add(count as u64)
            .ok_or_else(|| bound("snapshot byte count overflowed"))?;
        if bytes > limits.max_file_bytes {
            return Err(bound("snapshot file exceeds the configured limit"));
        }
        hasher.update(&buffer[..count]);
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|source| HostError::Prepare { source })?;
    Ok((file, guards, hex::encode(hasher.finalize()), bytes))
}

fn copy_stream_hashed(
    input: &mut File,
    output: &mut File,
    max_bytes: u64,
) -> Result<u64, HostError> {
    let mut bytes = 0u64;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let remaining = max_bytes.saturating_sub(bytes).saturating_add(1);
        let chunk = usize::try_from(remaining)
            .unwrap_or(usize::MAX)
            .min(buffer.len());
        let count = input
            .read(&mut buffer[..chunk])
            .map_err(|source| HostError::Prepare { source })?;
        if count == 0 {
            let _ = hasher.finalize();
            return Ok(bytes);
        }
        bytes += count as u64;
        if bytes > max_bytes {
            return Err(bound("snapshot file exceeds the configured limit"));
        }
        hasher.update(&buffer[..count]);
        output
            .write_all(&buffer[..count])
            .map_err(|source| HostError::Prepare { source })?;
    }
}

fn bound(rule: &'static str) -> HostError {
    HostError::Config { rule }
}

#[cfg(unix)]
fn directory_entries(
    _path: &Path,
    directory: &File,
    budget: usize,
) -> Result<Vec<OsString>, HostError> {
    use std::ffi::CStr;
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStringExt;
    let duplicate = unsafe { libc::dup(directory.as_raw_fd()) };
    if duplicate < 0 {
        return Err(HostError::Prepare {
            source: std::io::Error::last_os_error(),
        });
    }
    let stream = unsafe { libc::fdopendir(duplicate) };
    if stream.is_null() {
        let source = std::io::Error::last_os_error();
        // fdopendir takes ownership only on success.
        unsafe { libc::close(duplicate) };
        return Err(HostError::Prepare { source });
    }
    let result = (|| {
        let mut result = Vec::new();
        loop {
            let errno = unsafe { unix_errno_location() };
            if errno.is_null() {
                return Err(bound("snapshot directory enumeration failed"));
            }
            unsafe { *errno = 0 };
            let entry = unsafe { libc::readdir(stream) };
            if entry.is_null() {
                if unsafe { *errno } != 0 {
                    return Err(bound("snapshot directory enumeration failed"));
                }
                break;
            }
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
            if name.to_bytes() == b"." || name.to_bytes() == b".." {
                continue;
            }
            if result.len() >= budget {
                return Err(bound("snapshot entry count exceeds the configured limit"));
            }
            result.push(OsString::from_vec(name.to_bytes().to_vec()));
        }
        Ok(result)
    })();
    if unsafe { libc::closedir(stream) } != 0 {
        return Err(bound("snapshot directory enumeration failed"));
    }
    result
}

#[cfg(unix)]
#[allow(unreachable_code)]
unsafe fn unix_errno_location() -> *mut libc::c_int {
    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "emscripten",
        target_os = "redox",
        target_os = "hurd",
        target_os = "dragonfly"
    ))]
    {
        return unsafe { libc::__errno_location() };
    }
    #[cfg(any(
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "cygwin",
        target_os = "nuttx"
    ))]
    {
        return unsafe { libc::__errno() };
    }
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos",
        target_os = "freebsd"
    ))]
    {
        return unsafe { libc::__error() };
    }
    std::ptr::null_mut()
}

#[cfg(not(unix))]
fn directory_entries(
    path: &Path,
    _directory: &File,
    budget: usize,
) -> Result<Vec<OsString>, HostError> {
    fs::read_dir(path)
        .map_err(|source| HostError::Prepare { source })?
        .map(|entry| {
            entry
                .map(|entry| entry.file_name())
                .map_err(|source| HostError::Prepare { source })
        })
        .take(budget.saturating_add(1))
        .collect::<Result<Vec<_>, _>>()
        .and_then(|entries| {
            if entries.len() > budget {
                Err(bound("snapshot entry count exceeds the configured limit"))
            } else {
                Ok(entries)
            }
        })
}

fn open_directory_child(_parent: &File, _name: &OsString, _path: &Path) -> Result<File, HostError> {
    #[cfg(unix)]
    {
        use std::os::fd::{AsRawFd, FromRawFd};
        use std::os::unix::ffi::OsStrExt;
        let name =
            std::ffi::CString::new(_name.as_os_str().as_bytes()).map_err(|_| HostError::Escape)?;
        let fd = unsafe {
            libc::openat(
                _parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(HostError::Prepare {
                source: std::io::Error::last_os_error(),
            });
        }
        return Ok(unsafe { File::from_raw_fd(fd) });
    }
    #[cfg(not(unix))]
    {
        open_directory(_path)
    }
}

fn reject_reparse(metadata: &fs::Metadata) -> Result<(), HostError> {
    if metadata.file_type().is_symlink() {
        return Err(HostError::Escape);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
        {
            return Err(HostError::Escape);
        }
    }
    Ok(())
}

fn open_directory(path: &Path) -> Result<File, HostError> {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options
            .access_mode(windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES)
            .custom_flags(
                windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS
                    | windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT,
            )
            .share_mode(
                windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ
                    | windows_sys::Win32::Storage::FileSystem::FILE_SHARE_WRITE,
            );
    }
    let file = options
        .open(path)
        .map_err(|source| HostError::Prepare { source })?;
    let metadata = file
        .metadata()
        .map_err(|source| HostError::Prepare { source })?;
    reject_reparse(&metadata)?;
    if !metadata.is_dir() {
        return Err(HostError::Config {
            rule: "snapshot directory was replaced by a file",
        });
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small() -> SnapshotLimits {
        SnapshotLimits {
            max_entries: 8,
            max_files: 4,
            max_depth: 2,
            max_total_bytes: 8,
            max_file_bytes: 4,
        }
    }

    // Cost: three local directories and two bounded walks; no provider or network.
    #[test]
    fn queued_directory_names_are_charged_before_recursion() {
        let source = tempfile::tempdir().unwrap();
        for name in ["a", "b", "c"] {
            fs::create_dir(source.path().join(name)).unwrap();
        }
        let destination = tempfile::tempdir().unwrap();
        for copy in [true, false] {
            let mut state = WalkState {
                entries: 0,
                files: 0,
                bytes: 0,
                limits: SnapshotLimits {
                    max_entries: 3,
                    max_depth: 0,
                    ..small()
                },
            };
            let directory = open_directory(source.path()).unwrap();
            let result = if copy {
                copy_dir(
                    source.path(),
                    source.path(),
                    destination.path(),
                    "",
                    0,
                    &mut state,
                    directory,
                )
            } else {
                collect_files(
                    source.path(),
                    source.path(),
                    0,
                    &mut Vec::new(),
                    &mut state,
                    directory,
                )
            };
            assert!(matches!(
                result,
                Err(HostError::Config {
                    rule: "snapshot directory depth exceeds the configured limit"
                })
            ));
            assert_eq!(
                state.entries, 3,
                "all queued names consume the global memory budget before recursion"
            );
        }
    }

    #[test]
    fn pin_refuses_a_file_over_the_declared_read_bound() {
        let source = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("index.bin"), b"12345").unwrap();
        let destination = tempfile::tempdir().unwrap();
        let error = copy_tree_with_limits(source.path(), &destination.path().join("copy"), small())
            .expect_err("a file larger than the bound must never be copied as a complete snapshot");
        assert!(matches!(
            error,
            HostError::Config {
                rule: "snapshot file exceeds the configured limit"
            }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn copy_refuses_a_symlink_instead_of_following_it() {
        let source = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret"), b"secret").unwrap();
        std::os::unix::fs::symlink(outside.path(), source.path().join("escape"))
            .expect("the platform supports symlink fixtures");
        let destination = tempfile::tempdir().unwrap();
        let error = pin_snapshot(source.path(), destination.path())
            .expect_err("a symlink must not be traversed into the host tree");
        assert!(matches!(error, HostError::Escape));
    }

    #[cfg(windows)]
    #[test]
    fn pin_refuses_a_child_reparse_point_instead_of_following_it() {
        let source = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret"), b"secret").unwrap();
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(source.path().join("escape"))
            .arg(outside.path())
            .output()
            .unwrap();
        assert!(
            made.status.success(),
            "the private junction fixture must be created"
        );
        let destination = tempfile::tempdir().unwrap();
        let error = pin_snapshot(source.path(), destination.path())
            .expect_err("a reparse point must not be traversed into the host tree");
        assert!(matches!(error, HostError::Escape));
    }
}
