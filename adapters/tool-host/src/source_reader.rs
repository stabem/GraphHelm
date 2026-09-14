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

use graphhelm_runtime::ports::{BoundedSourceReader, SourceExcerpt, SourceReadError};
use graphhelm_tool_broker::path::RelativePath;
use graphhelm_tool_broker::record::digest_hex;

use crate::process::HostError;
use crate::snapshot::tree_generation;
use crate::source_channel::{open_candidate, still_names_opened_file};
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
/// **Containment, twice.** The path is parsed as a [`RelativePath`] (no `..`, no absolute or
/// drive form, no backslash) and then resolved through `workspace::resolve_within` — the
/// per-component no-follow walk plus the canonical-ancestor check the tool workspace has used
/// since #538 — so a link, a junction or a reparse point anywhere in the chain is
/// [`SourceReadError::Escape`]. The open itself uses the channel's `open_candidate` (`O_NOFOLLOW`
/// and `O_NONBLOCK` on Unix) and re-checks that the handle is a regular file that the path still
/// names, so a swap between the walk and the read refuses rather than serving foreign bytes.
///
/// **The bound is a `take`, never a `read_to_end`:** one byte past `max_bytes` is never pulled,
/// whatever the file has grown to since its length was quoted.
#[derive(Debug)]
pub struct WorkspaceExcerptReader {
    root: PathBuf,
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
        Ok(Self { root: real })
    }
}

impl BoundedSourceReader for WorkspaceExcerptReader {
    fn read_prefix(
        &self,
        relative_path: &str,
        max_bytes: u64,
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
        let file = open_candidate(&path).map_err(|_| SourceReadError::Unreadable)?;
        if !still_names_opened_file(&file, &path) {
            return Err(SourceReadError::Escape);
        }
        excerpt_still_named(&file, &path, max_bytes)
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
    use super::{excerpt_from_handle, excerpt_still_named};
    use graphhelm_runtime::ports::SourceReadError;

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
