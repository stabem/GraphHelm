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

use graphhelm_tool_broker::record::digest_hex;

use crate::process::HostError;
use crate::snapshot::tree_generation;

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
    /// exact scenario it exists for. The salt mixes the process id and the instant of
    /// construction, so no two instances — across threads or processes — share a failure id.
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
