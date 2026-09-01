//! The immutable, digest-pinned index snapshot copied into Tier 1 (#539, D-042's middle clause).
//!
//! Three words, three mechanisms:
//!
//! - **Copied** — the provider must never read the host index in place; the copy under the
//!   Tier 1 root IS the boundary, not an optimisation.
//! - **Immutable** — the live index is written by a background watcher; a retrieval against the
//!   copy cannot observe the source moving, by construction rather than by locking.
//! - **Digest-pinned** — the generation derives from the copied BYTES (paths bound beside
//!   contents), never from a ref: the live index's `head_sha` and its content disagreed by two
//!   hours and nine merges when #539 was filed. A ref can be stale and still answer with
//!   confidence; bytes cannot.
//!
//! The walk is deterministic (paths sorted, relative, forward-slashed) so the same tree pins to
//! the same generation on every platform. `verify_pinned` re-derives and refuses on mismatch
//! with both values — re-pin or investigate, never read anyway.

use std::path::{Path, PathBuf};

use graphhelm_tool_broker::record::digest_hex;

use crate::process::HostError;

/// A snapshot the broker pinned: where the copy lives, and the generation its bytes hash to.
/// Construction only via [`pin_snapshot`], so an unpinned path cannot impersonate a snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinnedSnapshot {
    root: PathBuf,
    generation: String,
}

impl PinnedSnapshot {
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The content-derived identity a `RetrievalCoverageReceipt` binds as its index generation.
    pub fn generation(&self) -> &str {
        &self.generation
    }
}

/// Copy `source` into `<tier1_root>/.index-snapshot/` and pin the copy by content digest.
///
/// # Errors
/// [`HostError::Prepare`] on any filesystem failure; [`HostError::Config`] if the snapshot
/// directory already exists (a workspace hosts one snapshot per provision — a second pin is a
/// protocol error, not an overwrite).
pub fn pin_snapshot(source: &Path, tier1_root: &Path) -> Result<PinnedSnapshot, HostError> {
    let destination = tier1_root.join(".index-snapshot");
    if destination.exists() {
        return Err(HostError::Config {
            rule: "the workspace already holds an index snapshot",
        });
    }
    copy_tree(source, &destination)?;
    let generation = tree_generation(&destination)?;
    Ok(PinnedSnapshot {
        root: destination,
        generation,
    })
}

/// Re-derive the copy's generation and refuse if it no longer matches the recorded value.
///
/// # Errors
/// [`HostError::SnapshotMismatch`] carrying both digests; [`HostError::Prepare`] if the copy
/// cannot be read at all.
pub fn verify_pinned(root: &Path, expected_generation: &str) -> Result<(), HostError> {
    let actual = tree_generation(root)?;
    if actual != expected_generation {
        return Err(HostError::SnapshotMismatch {
            expected: expected_generation.to_owned(),
            actual,
        });
    }
    Ok(())
}

pub(crate) fn copy_tree(source: &Path, destination: &Path) -> Result<(), HostError> {
    std::fs::create_dir_all(destination).map_err(|source| HostError::Prepare { source })?;
    let entries = std::fs::read_dir(source).map_err(|source| HostError::Prepare { source })?;
    for entry in entries {
        let entry = entry.map_err(|source| HostError::Prepare { source })?;
        let path = entry.path();
        let target = destination.join(entry.file_name());
        if path.is_dir() {
            copy_tree(&path, &target)?;
        } else {
            std::fs::copy(&path, &target).map_err(|source| HostError::Prepare { source })?;
        }
    }
    Ok(())
}

/// The deterministic content digest: every file's relative forward-slashed path beside the
/// digest of its bytes, in sorted order, hashed once. Binding the path beside the bytes is what
/// makes two files swapping contents a DIFFERENT generation — which bytes live where is part of
/// the identity.
pub(crate) fn tree_generation(root: &Path) -> Result<String, HostError> {
    let mut files: Vec<PathBuf> = Vec::new();
    collect_files(root, &mut files)?;
    // L's #551 fold: the empty population's digest is the sha256 of the empty string — one
    // identity shared by every empty snapshot in the world. A provision that silently copied
    // NOTHING (wrong path, watcher race, permissions) would pin and verify forever, reading as
    // success. Empty is a refusal, not an identity.
    if files.is_empty() {
        return Err(HostError::Config {
            rule: "an index snapshot cannot be empty",
        });
    }
    let mut relative: Vec<String> = files
        .iter()
        .map(|file| {
            file.strip_prefix(root)
                .unwrap_or(file)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    relative.sort_unstable();
    let mut lines = String::new();
    for rel in &relative {
        let bytes = std::fs::read(root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR)))
            .map_err(|source| HostError::Prepare { source })?;
        lines.push_str(rel);
        lines.push(':');
        lines.push_str(&digest_hex(&bytes));
        lines.push('\n');
    }
    Ok(format!("sha256-{}", digest_hex(lines.as_bytes())))
}

fn collect_files(root: &Path, into: &mut Vec<PathBuf>) -> Result<(), HostError> {
    let entries = std::fs::read_dir(root).map_err(|source| HostError::Prepare { source })?;
    for entry in entries {
        let entry = entry.map_err(|source| HostError::Prepare { source })?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, into)?;
        } else {
            into.push(path);
        }
    }
    Ok(())
}
