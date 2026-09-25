//! The snapshot-keyed read cache: provably-exact-only, per the amended
//! `AGENTS_SKILLS_PLUGINS.md` §11.2-3. The key is a declared subset of
//! `SYSTEM_ARCHITECTURE.md` §7.2's dependency-hash components — tool version, canonical
//! input, lease scope, source snapshot — and carries **no TTL anywhere**: the snapshot key IS
//! the freshness (the amended spec deleted TTLs deliberately), and eligibility additionally
//! requires a clean working tree, because Tier 0 reads touch the LIVE tree and HEAD only pins
//! it when nothing is dirty. An `EvidenceErasureCompleted` for an entry's evidence is a
//! mandatory invalidation input — a cache must never serve cryptographically erased evidence —
//! surfaced here as [`ReadCache::invalidate_evidence`], which the 05d executor wires to the
//! event.

use std::path::{Path, PathBuf};
use std::process::Command;

use graphhelm_tool_broker::record::{ToolCallRecord, digest_hex};

use crate::process::HostError;
use crate::workspace::git_safe;

/// One cached read: the record as it was first produced plus the captured bytes.
#[derive(serde::Serialize, serde::Deserialize)]
struct Entry {
    record: ToolCallRecord,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    /// The evidence this entry's bytes became, once the 05d executor externalizes them.
    /// `None` until then; erasure invalidation matches on it.
    evidence_ref: Option<String>,
}

/// Directory-backed cache under the host's staging area.
pub struct ReadCache {
    directory: PathBuf,
}

impl ReadCache {
    #[must_use]
    pub fn new(staging: &Path) -> Self {
        Self {
            directory: staging.join("ghtool-read-cache"),
        }
    }

    /// The composed key. Every component is named in `ReuseKeyComponent`'s closed vocabulary:
    /// tool version (this crate's), canonical call JSON, lease scope (actor + sorted
    /// capability set), and the source snapshot (HEAD commit).
    #[must_use]
    pub fn key(tool_version: &str, canonical_call: &str, lease_scope: &str, head: &str) -> String {
        digest_hex(
            format!("{tool_version}\u{1f}{canonical_call}\u{1f}{lease_scope}\u{1f}{head}")
                .as_bytes(),
        )
    }

    /// The project's HEAD commit — the source-snapshot key component — or `None` when the
    /// working tree is dirty: `git status --porcelain` printing ANYTHING means the live bytes
    /// are not HEAD's bytes, and the HEAD key is only exact when tree == HEAD. The status call
    /// per lookup is the price of "provably exact" being true.
    #[must_use]
    pub fn clean_head(project: &Path) -> Option<String> {
        let status = Command::new("git")
            .arg("-C")
            .arg(git_safe(project))
            .args(["status", "--porcelain"])
            .env("GIT_OPTIONAL_LOCKS", "0")
            .output()
            .ok()?;
        if !status.status.success() || !status.stdout.is_empty() {
            return None;
        }
        let head = Command::new("git")
            .arg("-C")
            .arg(git_safe(project))
            .args(["rev-parse", "HEAD"])
            .env("GIT_OPTIONAL_LOCKS", "0")
            .output()
            .ok()?;
        if !head.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&head.stdout).trim().to_owned())
    }

    /// A stored entry for `key`, if one exists.
    #[must_use]
    pub fn load(&self, key: &str) -> Option<(ToolCallRecord, Vec<u8>, Vec<u8>)> {
        let bytes = std::fs::read(self.directory.join(key)).ok()?;
        let entry: Entry = serde_json::from_slice(&bytes).ok()?;
        Some((entry.record, entry.stdout, entry.stderr))
    }

    /// Stores an entry. A storage failure is not a call failure — the call already succeeded;
    /// the cache is an economy, not a dependency.
    ///
    /// # Errors
    /// [`HostError::Prepare`] when the directory or file cannot be written.
    pub fn store(
        &self,
        key: &str,
        record: &ToolCallRecord,
        stdout: &[u8],
        stderr: &[u8],
    ) -> Result<(), HostError> {
        std::fs::create_dir_all(&self.directory).map_err(|source| HostError::Prepare { source })?;
        let entry = Entry {
            record: record.clone(),
            stdout: stdout.to_vec(),
            stderr: stderr.to_vec(),
            evidence_ref: None,
        };
        let bytes = serde_json::to_vec(&entry).map_err(|_| HostError::Prepare {
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, "unserializable entry"),
        })?;
        std::fs::write(self.directory.join(key), bytes)
            .map_err(|source| HostError::Prepare { source })
    }

    /// Removes every entry whose `evidence_ref` names `evidence_id` — the erasure hard
    /// constraint's cache half: after this returns, no lookup can serve bytes whose evidence
    /// was cryptographically erased.
    pub fn invalidate_evidence(&self, evidence_id: &str) {
        let Ok(entries) = std::fs::read_dir(&self.directory) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let matches = std::fs::read(&path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Entry>(&bytes).ok())
                .is_some_and(|cached| cached.evidence_ref.as_deref() == Some(evidence_id));
            if matches {
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    /// Tags a stored entry with the evidence its bytes became (the 05d executor calls this
    /// after externalization, so erasure invalidation has something to match on).
    pub fn tag_evidence(&self, key: &str, evidence_id: &str) {
        let path = self.directory.join(key);
        let Some(mut entry) = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Entry>(&bytes).ok())
        else {
            return;
        };
        entry.evidence_ref = Some(evidence_id.to_owned());
        if let Ok(bytes) = serde_json::to_vec(&entry) {
            let _ = std::fs::write(&path, bytes);
        }
    }
}
