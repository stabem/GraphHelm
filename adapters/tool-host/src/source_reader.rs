//! The production `SourceReader` (#543): the identity of the bytes a retrieval would serve NOW.
//!
//! The port's contract (`core/runtime/src/ports.rs`) is one sentence with a trap in it: the id
//! must derive from CONTENT, never from a ref — a commit id survives an uncommitted edit, so a
//! ref-answering reader makes the whole staleness mechanism say "fresh" about bytes that moved.
//! The runtime cannot check this (both are opaque ids); the guards in
//! `tests/workspace_source_reader.rs` are what catch an implementor, and the uncommitted-edit
//! cell is the one that separates the two.
//!
//! The digest oracle is `snapshot::tree_generation` — the SAME function #539's pinned snapshot
//! uses, on purpose: one place decides how a tree's bytes become an identity, so the reader and
//! the pin cannot drift into disagreeing about what "the same bytes" means.
//!
//! **Bounds, stated:** `open` walks once and refuses a workspace beyond the declared limits, so
//! the cost is paid and named at construction (`O(files + bytes)` per snapshot call, against a
//! workspace whose size `open` capped). A workspace that GROWS past the bound after `open` still
//! digests — the bound is an admission control, not a per-call guarantee, and a Tier 1 workspace
//! is house-provisioned and disposable.
//!
//! **Fail-closed unreadability:** the trait is infallible, so a read failure cannot refuse — it
//! answers instead with a one-time identity that never repeats and never equals a content id.
//! Every consumer comparison therefore reads as STALE, which is the refusing side of every gate
//! this port feeds.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use graphhelm_runtime::ports::{
    BoundedSourceReader, BoundedSourceSearch, ExecutionTreeAccess, ExecutionTreePort, ScanCancel,
    SourceExcerpt, SourceReadError,
};
use graphhelm_tool_broker::path::RelativePath;
use graphhelm_tool_broker::record::digest_hex;

use crate::process::HostError;
use crate::snapshot::tree_generation;
use crate::source_channel::{WorkspaceSourceChannel, still_names_opened_file};
use crate::workspace::resolve_within;

/// Declared admission bounds for [`WorkspaceSourceReader::open`].
#[derive(Clone, Copy, Debug)]
pub struct SourceReaderLimits {
    pub max_files: usize,
    pub max_bytes: u64,
}

/// The shipped reader: workspace-scoped, content-derived, bounded at admission.
#[derive(Debug)]
pub struct WorkspaceSourceReader {
    root: PathBuf,
    failure_counter: AtomicU64,
    /// Per-instance salt for failure identities (L's #556 finding): a counter salted only by
    /// the root REPEATS across instances — reader A and reader B, first failure each, same id —
    /// and a binding recorded during unreadability would then compare EQUAL against a fresh
    /// reader's failure and read FRESH: a fail-open inside the fail-closed mechanism, in the
    /// exact scenario it exists for. Three sources, each named for the collision IT covers: the
    /// process id separates processes, the construction instant separates pid reuse across
    /// boots, and the process-wide instance sequence is what separates two readers inside one
    /// process — including two opened in the same clock tick, which the instant alone cannot.
    instance_salt: String,
}

impl WorkspaceSourceReader {
    /// Admit a workspace under declared bounds.
    ///
    /// # Errors
    /// [`HostError::Config`] when the workspace exceeds the bounds (by name);
    /// [`HostError::Prepare`] when it cannot be walked at all.
    pub fn open(root: &Path, limits: SourceReaderLimits) -> Result<Self, HostError> {
        let mut files = 0usize;
        let mut bytes = 0u64;
        measure_tree(root, &mut files, &mut bytes)?;
        if files > limits.max_files || bytes > limits.max_bytes {
            return Err(HostError::Config {
                rule: "the workspace exceeds the reader's declared bounds",
            });
        }
        // Three sources, each covering the others' collision case: the process id separates
        // processes, the instant separates pid reuse across boots, and the process-wide
        // sequence separates two opens inside one process that share a clock tick.
        static INSTANCE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let sequence = INSTANCE_SEQUENCE.fetch_add(1, Ordering::SeqCst);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        Ok(Self {
            root: root.to_path_buf(),
            failure_counter: AtomicU64::new(0),
            instance_salt: format!("{}:{now}:{sequence}", std::process::id()),
        })
    }
}

impl graphhelm_runtime::ports::SourceReader for WorkspaceSourceReader {
    fn current_snapshot(&self) -> graphhelm_protocols::OpaqueId {
        match tree_generation(&self.root) {
            Ok(generation) => graphhelm_protocols::OpaqueId::parse(generation)
                .expect("tree_generation emits sha256-hex, which is opaque-legal"),
            Err(_) => {
                // Fail closed: a one-time identity that never repeats and never collides with a
                // content id (content ids are `sha256-<hex>`; this is `unreadable-<n>-<hex>`).
                // Every comparison against it reads as stale, which is the refusing side of
                // every gate this port feeds.
                let count = self.failure_counter.fetch_add(1, Ordering::SeqCst);
                let marker = format!("{}:{}:{count}", self.instance_salt, self.root.display());
                graphhelm_protocols::OpaqueId::parse(format!(
                    "unreadable-{count}-{}",
                    &digest_hex(marker.as_bytes())[..16]
                ))
                .expect("the failure marker is opaque-legal")
            }
        }
    }
}

fn measure_tree(root: &Path, files: &mut usize, bytes: &mut u64) -> Result<(), HostError> {
    let entries = std::fs::read_dir(root).map_err(|source| HostError::Prepare { source })?;
    for entry in entries {
        let entry = entry.map_err(|source| HostError::Prepare { source })?;
        let path = entry.path();
        if path.is_dir() {
            measure_tree(&path, files, bytes)?;
        } else {
            *files += 1;
            *bytes += entry
                .metadata()
                .map_err(|source| HostError::Prepare { source })?
                .len();
        }
    }
    Ok(())
}

/// The production [`BoundedSourceReader`] (#1065): a bounded PREFIX of one regular file inside
/// a canonical project root, and nothing else.
///
/// It is a separate type from [`WorkspaceSourceReader`] on purpose. That reader's `open` walks
/// and sizes the whole tree at admission — the right price for a tree IDENTITY — but a project
/// with a `target/` or `node_modules/` beside its sources is refused there by declared bounds,
/// and refusing every such project the capsule is exactly the outcome this chain exists to avoid.
/// Reading one file's prefix costs `O(max_bytes)` whatever the tree holds, so this reader admits
/// the root the same way the search channel does (a real, canonical directory) and pays per read.
///
/// **Containment, three times.** The path is parsed as a [`RelativePath`] (no `..`, no absolute
/// or drive form, no backslash) and then resolved through `workspace::resolve_within` — the
/// per-component no-follow walk plus the canonical-ancestor check the tool workspace has used
/// since #538 — so a link, a junction or a reparse point anywhere in the chain is
/// [`SourceReadError::Escape`]. That walk is over PATHS, so the open does not trust it (#1086):
/// `open_beneath` traverses again from the root's own handle, refusing a link at EVERY component
/// (Unix: `openat` with `O_NOFOLLOW`; Windows: each ancestor opened as itself and held against
/// rename), and the handle is re-checked after the read to be a regular file the path still
/// names, so a swap of the file or of any ancestor refuses rather than serving foreign bytes.
///
/// **The bound is a `take`, never a `read_to_end`:** one byte past `max_bytes` is never pulled,
/// whatever the file has grown to since its length was quoted.
#[derive(Debug)]
pub struct WorkspaceExcerptReader {
    root: PathBuf,
    /// Checked before the open and before the read (#1086): a read whose drive gave it up
    /// refuses `Unreadable` instead of touching the file.
    cancel: Option<graphhelm_runtime::ports::ScanCancel>,
}

impl WorkspaceExcerptReader {
    /// Admit a project root: a real directory, resolved through every link, never a link itself.
    ///
    /// # Errors
    /// [`HostError::Escape`] when the root is a link; [`HostError::Prepare`] when it cannot be
    /// resolved or is not a directory.
    pub fn open(root: &Path) -> Result<Self, HostError> {
        let metadata =
            std::fs::symlink_metadata(root).map_err(|source| HostError::Prepare { source })?;
        if metadata.file_type().is_symlink() {
            return Err(HostError::Escape);
        }
        let real = std::fs::canonicalize(root).map_err(|source| HostError::Prepare { source })?;
        if !real.is_dir() {
            return Err(HostError::Prepare {
                source: std::io::Error::new(
                    std::io::ErrorKind::NotADirectory,
                    "the project root is not a directory",
                ),
            });
        }
        Ok(Self {
            root: real,
            cancel: None,
        })
    }

    /// The same reader, refusing every read once `cancel` is set (#1086).
    #[must_use]
    pub fn with_cancel(mut self, cancel: graphhelm_runtime::ports::ScanCancel) -> Self {
        self.cancel = Some(cancel);
        self
    }

    fn cancelled(&self) -> bool {
        self.cancel
            .as_ref()
            .is_some_and(graphhelm_runtime::ports::ScanCancel::is_cancelled)
    }
}

impl BoundedSourceReader for WorkspaceExcerptReader {
    fn read_prefix(
        &self,
        relative_path: &str,
        max_bytes: u64,
    ) -> Result<SourceExcerpt, SourceReadError> {
        self.read_prefix_racing(relative_path, max_bytes, &mut || {})
    }
}

impl WorkspaceExcerptReader {
    /// [`BoundedSourceReader::read_prefix`] with a seam between the containment walk and the
    /// open: `between_walk_and_open` runs exactly where a concurrent writer could rename an
    /// ancestor. Production passes a no-op; the unit test below swaps a parent directory for a
    /// link there, which is the race #1086 names.
    fn read_prefix_racing(
        &self,
        relative_path: &str,
        max_bytes: u64,
        between_walk_and_open: &mut dyn FnMut(),
    ) -> Result<SourceExcerpt, SourceReadError> {
        let relative = RelativePath::parse(relative_path).map_err(|_| SourceReadError::Escape)?;
        let path = resolve_within(&self.root, &relative).map_err(|error| match error {
            HostError::Escape => SourceReadError::Escape,
            _ => SourceReadError::Unreadable,
        })?;
        // NO-FOLLOW metadata: a link that `resolve_within` could not classify is refused here
        // as not-a-regular-file rather than followed.
        let metadata = std::fs::symlink_metadata(&path).map_err(|_| SourceReadError::Unreadable)?;
        if metadata.file_type().is_symlink() {
            return Err(SourceReadError::Escape);
        }
        if !metadata.is_file() {
            return Err(SourceReadError::Unreadable);
        }
        between_walk_and_open();
        if self.cancelled() {
            return Err(SourceReadError::Unreadable);
        }
        // The walk above ran over PATHS; a parent renamed to a link after it would redirect a
        // path-based open (#1086 item 1). The open below does not trust the walk: it traverses
        // from the root handle, refusing a link at every component.
        #[cfg(unix)]
        let file = open_beneath(&self.root, &relative)?;
        // `_pinned` keeps every ancestor open, denying rename and delete, until the post-read
        // identity check below has run.
        #[cfg(windows)]
        let (file, _pinned) = open_beneath(&self.root, &relative)?;
        #[cfg(not(any(unix, windows)))]
        let file = crate::source_channel::open_candidate(&path)
            .map_err(|_| SourceReadError::Unreadable)?;
        if !still_names_opened_file(&file, &path) {
            return Err(SourceReadError::Escape);
        }
        if self.cancelled() {
            return Err(SourceReadError::Unreadable);
        }
        excerpt_still_named(&file, &path, max_bytes)
    }
}

/// Open `relative` beneath `root` with no link followed anywhere in the chain (#1086 item 1),
/// Unix half: `openat` one component at a time from the root's own handle, every ancestor with
/// `O_DIRECTORY | O_NOFOLLOW` and the final component with `O_NOFOLLOW | O_NONBLOCK`. Each step
/// resolves a NAME inside a directory already held open, so renaming an ancestor to a symlink
/// after the path walk is refused at that component instead of being followed.
///
/// **Which errno means a link.** `ELOOP` (Linux, macOS) and `EMLINK` (FreeBSD) are what
/// `O_NOFOLLOW` returns for a link and are [`SourceReadError::Escape`]. Linux checks
/// `O_DIRECTORY` first, so an ANCESTOR that is a link fails with `ENOTDIR` — the same errno a
/// component that is a regular file produces. The two are told apart by `fstatat` with
/// `AT_SYMLINK_NOFOLLOW` on the name that failed: a link is `Escape`, anything else stays
/// `Unreadable`. Both are refusals; a swap between the failed open and the `fstatat` changes
/// only which refusal is reported, never whether bytes are served.
///
/// **Residual, declared:** a directory held open and then MOVED OUT of the root by rename keeps
/// resolving names under its new location. The post-read `still_names_opened_file` check (the
/// path inside the root must still name the inode that was read) refuses that case after the
/// read; closing it before the read needs `openat2(RESOLVE_BENEATH)`, which is Linux-only and
/// not in the pinned `libc` surface this crate uses on every Unix.
#[cfg(unix)]
fn open_beneath(root: &Path, relative: &RelativePath) -> Result<std::fs::File, SourceReadError> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd};
    use std::os::unix::ffi::OsStrExt as _;

    let root_name =
        CString::new(root.as_os_str().as_bytes()).map_err(|_| SourceReadError::Unreadable)?;
    // SAFETY: `root_name` is NUL-terminated and outlives the call; the result is checked before
    // it is used.
    let fd = unsafe {
        libc::open(
            root_name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(refusal(
            std::io::Error::last_os_error(),
            libc::AT_FDCWD,
            &root_name,
        ));
    }
    // SAFETY: `fd` was just returned by a successful `open` and nothing else owns it.
    let mut current = unsafe { OwnedFd::from_raw_fd(fd) };
    let components: Vec<&str> = relative.as_str().split('/').collect();
    let last = components.len() - 1;
    for (index, component) in components.iter().enumerate() {
        let name = CString::new(*component).map_err(|_| SourceReadError::Escape)?;
        let flags = if index == last {
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC
        } else {
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC
        };
        // SAFETY: `current` is an open directory descriptor for the whole call and `name` is
        // NUL-terminated; the result is checked before it is used.
        let fd = unsafe { libc::openat(current.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(refusal(
                std::io::Error::last_os_error(),
                current.as_raw_fd(),
                &name,
            ));
        }
        // SAFETY: `fd` was just returned by a successful `openat` and nothing else owns it.
        current = unsafe { OwnedFd::from_raw_fd(fd) };
    }
    Ok(std::fs::File::from(current))
}

/// Classify a failed `O_NOFOLLOW` open of `name` in `directory` (see `open_beneath`): `ELOOP`
/// and `EMLINK` are a link; `ENOTDIR` is a link only when `fstatat(AT_SYMLINK_NOFOLLOW)` says the
/// name is one now; every other errno, and an `ENOTDIR` on a regular file, is `Unreadable`.
#[cfg(unix)]
fn refusal(
    error: std::io::Error,
    directory: std::os::fd::RawFd,
    name: &std::ffi::CStr,
) -> SourceReadError {
    match error.raw_os_error() {
        Some(code) if code == libc::ELOOP || code == libc::EMLINK => SourceReadError::Escape,
        Some(code) if code == libc::ENOTDIR && names_a_link(directory, name) => {
            SourceReadError::Escape
        }
        _ => SourceReadError::Unreadable,
    }
}

/// Whether `name` in `directory` is a symbolic link itself, never following it. A failed
/// `fstatat` answers `false`, which keeps the caller's refusal at `Unreadable`.
#[cfg(unix)]
fn names_a_link(directory: std::os::fd::RawFd, name: &std::ffi::CStr) -> bool {
    let mut status = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `name` is NUL-terminated and outlives the call, `status` is writable storage for
    // one `stat`, and `directory` is either `AT_FDCWD` or a descriptor the caller holds open.
    let result = unsafe {
        libc::fstatat(
            directory,
            name.as_ptr(),
            status.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result != 0 {
        return false;
    }
    // SAFETY: a successful `fstatat` initialised `status`.
    let status = unsafe { status.assume_init() };
    status.st_mode & libc::S_IFMT == libc::S_IFLNK
}

/// Open `relative` beneath `root` with no reparse point followed anywhere in the chain (#1086
/// item 1), Windows half. Windows has no `openat` in std, so every ancestor is opened BY PATH
/// with `FILE_FLAG_OPEN_REPARSE_POINT` (a junction or symlink is opened as itself, never
/// traversed) and refused if it carries `FILE_ATTRIBUTE_REPARSE_POINT`, and every handle is held
/// with a share mode that DENIES delete — which is what a rename needs — so no ancestor already
/// checked can be renamed or replaced while the next one is opened and the file is read. The final
/// handle is then identity-checked: `GetFinalPathNameByHandleW` must place the file inside the
/// canonical root.
///
/// **Cost, declared:** for the length of one bounded read, another process renaming or deleting a
/// directory on the path gets a sharing violation. **Residual, declared:** a regular file whose
/// reparse tag is not a link (a cloud-storage placeholder, a deduplicated file) is refused as a
/// reparse point; that costs one candidate and a count.
#[cfg(windows)]
fn open_beneath(
    root: &Path,
    relative: &RelativePath,
) -> Result<(std::fs::File, Vec<std::fs::File>), SourceReadError> {
    use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    let open = |path: &Path, directory: bool| -> Result<std::fs::File, SourceReadError> {
        let mut options = std::fs::OpenOptions::new();
        if directory {
            options
                .access_mode(FILE_READ_ATTRIBUTES)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS);
        } else {
            options
                .read(true)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        }
        let file = options
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(path)
            .map_err(|_| SourceReadError::Unreadable)?;
        let metadata = file.metadata().map_err(|_| SourceReadError::Unreadable)?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(SourceReadError::Escape);
        }
        if metadata.is_dir() != directory {
            return Err(SourceReadError::Unreadable);
        }
        Ok(file)
    };

    let mut pinned = vec![open(root, true)?];
    let components: Vec<&str> = relative.as_str().split('/').collect();
    let (last, ancestors) = components.split_last().ok_or(SourceReadError::Escape)?;
    let mut current = root.to_path_buf();
    for component in ancestors {
        current.push(component);
        pinned.push(open(&current, true)?);
    }
    current.push(last);
    let file = open(&current, false)?;
    if !final_path_within(&file, root) {
        return Err(SourceReadError::Escape);
    }
    Ok((file, pinned))
}

/// Whether the file behind `file`'s handle lives inside `root`, by the path the handle itself
/// resolves to (`GetFinalPathNameByHandleW`, the same call `std::fs::canonicalize` makes, so the
/// two spellings compare component for component).
#[cfg(windows)]
fn final_path_within(file: &std::fs::File, root: &Path) -> bool {
    use std::os::windows::ffi::OsStringExt as _;
    use std::os::windows::io::AsRawHandle as _;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_NAME_NORMALIZED, GetFinalPathNameByHandleW, VOLUME_NAME_DOS,
    };

    let mut buffer = vec![0u16; 32_768];
    let capacity = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
    // SAFETY: the handle is open for the whole call and `capacity` is the buffer's real length.
    let written = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            buffer.as_mut_ptr(),
            capacity,
            FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
        )
    };
    let Ok(written) = usize::try_from(written) else {
        return false;
    };
    if written == 0 || written >= buffer.len() {
        return false;
    }
    PathBuf::from(std::ffi::OsString::from_wide(&buffer[..written])).starts_with(root)
}

/// The context port over an execution's own Tier 1 tree (#1086 item 5): the search channel and
/// the excerpt reader, opened over the tree `ToolHost` keeps for the execution, for exactly as
/// long as the host holds that tree still.
///
/// The tree is read LIVE, not from a snapshot, and that is safe for one reason stated here:
/// `ToolHost::with_execution_tree` holds the execution's slot lock for the whole compile, and
/// every tool call of the execution holds the same lock for its whole duration, so no tool of the
/// execution writes while the compile reads. Nothing outside the host writes into a Tier 1 tree.
pub struct ExecutionContextTree {
    host: std::sync::Arc<crate::host::ToolHost>,
    execution_id: String,
}

impl ExecutionContextTree {
    #[must_use]
    pub fn new(host: std::sync::Arc<crate::host::ToolHost>, execution_id: &str) -> Self {
        Self {
            host,
            execution_id: execution_id.to_owned(),
        }
    }
}

impl ExecutionTreePort for ExecutionContextTree {
    fn with_tree(
        &self,
        cancel: &ScanCancel,
        compile: &mut dyn FnMut(&dyn BoundedSourceSearch, &dyn BoundedSourceReader),
    ) -> ExecutionTreeAccess {
        let ran = self
            .host
            .with_execution_tree(&self.execution_id, cancel, |root| {
                let Ok(search) = WorkspaceSourceChannel::open(root) else {
                    return false;
                };
                let Ok(reader) = WorkspaceExcerptReader::open(root) else {
                    return false;
                };
                let search = search.with_cancel(cancel.clone());
                let reader = reader.with_cancel(cancel.clone());
                compile(&search, &reader);
                true
            });
        match ran {
            Ok(None) => ExecutionTreeAccess::Absent,
            Ok(Some(true)) => ExecutionTreeAccess::Read,
            Ok(Some(false)) | Err(_) => ExecutionTreeAccess::Unavailable,
        }
    }
}

/// Read the bounded prefix and RE-CHECK, after the read, that the path still names the file
/// the bytes came from — the same post-read half `source_channel` keeps. A check before the
/// read alone leaves the read itself as the window: a file swapped or moved through the path
/// while the bytes were in flight would be cited under a path that no longer holds them.
/// Refused as the same [`SourceReadError::Escape`] the pre-read check refuses with.
fn excerpt_still_named(
    file: &std::fs::File,
    path: &Path,
    max_bytes: u64,
) -> Result<SourceExcerpt, SourceReadError> {
    let excerpt = excerpt_from_handle(file, max_bytes)?;
    if !still_names_opened_file(file, path) {
        return Err(SourceReadError::Escape);
    }
    Ok(excerpt)
}

/// Read the bounded prefix of an OPEN regular file and declare its length from the same
/// handle.
///
/// The length is `fstat` on the handle the bytes are read from, taken after the identity check
/// — never the path's `lstat` from before the open, which a replace or a resize between the two
/// leaves stale: a file swapped or truncated in that window would otherwise be declared with
/// the OLD length and a complete read reported as a prefix, or a prefix as complete. The
/// declared length is also never below the bytes actually read: a file that grew between the
/// `fstat` and the read is declared at least as long as what came back, so a consumer's
/// "full-vs-partial" (`bytes read < length`) is derived from what was read against a length
/// the read cannot contradict.
fn excerpt_from_handle(
    file: &std::fs::File,
    max_bytes: u64,
) -> Result<SourceExcerpt, SourceReadError> {
    let handle_len = match file.metadata() {
        Ok(now) if now.is_file() => now.len(),
        _ => return Err(SourceReadError::Unreadable),
    };
    let mut bytes = Vec::new();
    {
        use std::io::Read as _;
        file.take(max_bytes)
            .read_to_end(&mut bytes)
            .map_err(|_| SourceReadError::Unreadable)?;
    }
    let file_len = handle_len.max(bytes.len() as u64);
    Ok(SourceExcerpt { bytes, file_len })
}

#[cfg(test)]
mod tests {
    use super::{WorkspaceExcerptReader, excerpt_from_handle, excerpt_still_named};
    use graphhelm_runtime::ports::SourceReadError;

    /// Replace the directory `at` with a link (a symlink on Unix, a junction on Windows) to
    /// `target`, moving the real directory aside first.
    fn swap_directory_for_link(at: &std::path::Path, target: &std::path::Path) {
        std::fs::rename(at, at.with_file_name("moved-aside")).unwrap();
        #[cfg(windows)]
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(at)
            .arg(target)
            .output()
            .is_ok_and(|output| output.status.success());
        #[cfg(not(windows))]
        let made = std::os::unix::fs::symlink(target, at).is_ok();
        assert!(made, "arrangement: the ancestor link could not be staged");
    }

    /// #1086 item 1: an ANCESTOR renamed to a link between the containment walk and the open.
    /// The walk saw a real directory; the open then went through the link. The final component
    /// is a regular file with the same name on both sides, so a check that pins only the final
    /// component (`O_NOFOLLOW`, the path's own `lstat`) passes, and the bytes outside the root
    /// were served under a path inside it.
    #[test]
    fn an_ancestor_swapped_for_a_link_between_the_walk_and_the_open_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/alpha.rs"), b"fn alpha() {}\n").unwrap();
        let outside = directory.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("alpha.rs"), b"OUTSIDE-MARKER-1086\n").unwrap();

        let reader = WorkspaceExcerptReader::open(&root).unwrap();
        let src = reader.root.join("src");
        let mut swapped = false;
        let result = reader.read_prefix_racing("src/alpha.rs", 1024, &mut || {
            swap_directory_for_link(&src, &outside);
            swapped = true;
        });
        assert!(swapped, "the race window was reached");
        assert!(
            !matches!(&result, Ok(excerpt) if excerpt.bytes.starts_with(b"OUTSIDE-MARKER")),
            "bytes outside the root were served under a path inside it: {result:?}"
        );
        assert_eq!(result, Err(SourceReadError::Escape));
    }

    /// Each errno `open_beneath` can meet on Unix, asserted against its refusal, with no race:
    /// the states are staged before the call. A link at the root, at an ancestor (`ENOTDIR` on
    /// Linux, because `O_DIRECTORY` is checked first) or at the final component (`ELOOP`) is
    /// `Escape`; an ancestor that is a REGULAR FILE also answers `ENOTDIR` and must stay
    /// `Unreadable`, as must an absent name.
    #[cfg(unix)]
    #[test]
    fn unix_open_failures_name_a_link_only_when_the_component_is_one() {
        use graphhelm_tool_broker::path::RelativePath;
        let open = |root: &std::path::Path, relative: &str| {
            super::open_beneath(root, &RelativePath::parse(relative).unwrap()).map(|_| ())
        };
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/alpha.rs"), b"fn alpha() {}\n").unwrap();
        std::fs::write(root.join("plain.rs"), b"fn plain() {}\n").unwrap();
        let outside = directory.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("alpha.rs"), b"OUTSIDE\n").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("linked")).unwrap();
        std::os::unix::fs::symlink(outside.join("alpha.rs"), root.join("src/link.rs")).unwrap();
        let root_link = directory.path().join("root-link");
        std::os::unix::fs::symlink(&root, &root_link).unwrap();

        // Control: the unrefused path opens.
        assert_eq!(open(&root, "src/alpha.rs"), Ok(()));
        assert_eq!(
            open(&root, "linked/alpha.rs"),
            Err(SourceReadError::Escape),
            "an ancestor link"
        );
        assert_eq!(
            open(&root, "src/link.rs"),
            Err(SourceReadError::Escape),
            "a final-component link"
        );
        assert_eq!(
            open(&root_link, "src/alpha.rs"),
            Err(SourceReadError::Escape),
            "a root that is a link"
        );
        assert_eq!(
            open(&root, "plain.rs/alpha.rs"),
            Err(SourceReadError::Unreadable),
            "a regular-file ancestor answers ENOTDIR too and is not a link"
        );
        assert_eq!(
            open(&root, "src/missing.rs"),
            Err(SourceReadError::Unreadable),
            "an absent name is not a link"
        );
    }

    /// The identity check runs AFTER the read too: a path that stops naming the opened file
    /// between the open and the post-read check is refused, never cited.
    #[test]
    fn a_path_that_no_longer_names_the_opened_file_after_the_read_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("moves.rs");
        std::fs::write(&path, b"fn alpha() {}\n").unwrap();
        let file = std::fs::File::open(&path).unwrap();
        // Untouched: the same read the production path performs succeeds.
        assert!(excerpt_still_named(&file, &path, 1024).is_ok());

        // The file is moved away through the path after the open: the bytes are still
        // readable from the handle, and the path names nothing.
        let moved = directory.path().join("moved.rs");
        std::fs::rename(&path, &moved).unwrap();
        assert_eq!(
            excerpt_still_named(&file, &path, 1024),
            Err(SourceReadError::Escape)
        );

        // A regular file swapped in at the path is a different identity on Unix; on Windows
        // the regular-file swap stays the declared residual of `still_names_opened_file`.
        #[cfg(unix)]
        {
            std::fs::write(&path, b"fn swapped() {}\n").unwrap();
            assert_eq!(
                excerpt_still_named(&file, &path, 1024),
                Err(SourceReadError::Escape)
            );
        }
    }

    /// The length is the handle's, read after the open: a file resized once the path was
    /// measured is declared at the size the handle sees, not the stale one.
    #[test]
    fn the_declared_length_is_the_open_handles_not_the_paths_earlier_measure() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("grows.rs");
        std::fs::write(&path, b"fn alpha() {}\n").unwrap();
        let stale = std::fs::symlink_metadata(&path).unwrap().len();
        assert_eq!(stale, 14);
        let file = std::fs::File::open(&path).unwrap();
        // The resize lands between the path measure and the read, through the path.
        std::fs::write(&path, b"fn alpha() {}\nfn beta() {}\n").unwrap();
        let excerpt = excerpt_from_handle(&file, 1024).unwrap();
        assert_eq!(excerpt.file_len, 27, "the handle's length, not {stale}");
        assert_eq!(excerpt.bytes, b"fn alpha() {}\nfn beta() {}\n");
        assert_eq!(excerpt.file_len, file.metadata().unwrap().len());

        // A truncation lands the same way: the declared length shrinks with the file.
        let file = std::fs::File::open(&path).unwrap();
        std::fs::write(&path, b"fn a() {}\n").unwrap();
        let excerpt = excerpt_from_handle(&file, 1024).unwrap();
        assert_eq!(excerpt.file_len, 10);
        assert_eq!(excerpt.bytes, b"fn a() {}\n");

        // The bound still holds and the declared length is still the whole file (a fresh
        // handle, as production opens one per read).
        let file = std::fs::File::open(&path).unwrap();
        let excerpt = excerpt_from_handle(&file, 4).unwrap();
        assert_eq!(excerpt.bytes, b"fn a");
        assert_eq!(excerpt.file_len, 10);
    }
}
