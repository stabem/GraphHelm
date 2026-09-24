//! Keel-backed source ports for context retrieval.
//!
//! A selected [`Snapshot`] owns both the searchable bytes and the bytes returned by the reader.
//! This keeps one context compile on one immutable generation.  If the snapshot cannot account
//! for a source the live pair remains the complete fallback, and its search provenance says so.

use std::path::Path;
use std::sync::Arc;

use graphhelm_runtime::ports::{
    BoundedSourceReader, BoundedSourceSearch, SourceExcerpt, SourceReadError, SourceSearchBounds,
    SourceSearchError, SourceSearchProvenance, SourceSearchReason, SourceSearchResult,
};
use keel_contract_index::{
    Snapshot, SnapshotSearchBounds, SnapshotSearchError, SnapshotSourceError,
};

const SNAPSHOT_SCOPE: &[&str] = &[
    ".factory",
    ".superpowers",
    ".git",
    ".graphhelm",
    "keyring",
    "docs/superpowers/plans",
];

/// The snapshot and its two context ports. Both ports borrow the same immutable source store.
#[derive(Clone)]
pub struct KeelSnapshotPorts {
    snapshot: Arc<Snapshot>,
}

/// The result of selecting a source generation for one context compile.
///
/// The fallback keeps its reason closed and content-free so callers can report why the live pair
/// was selected without exposing an error string or an omitted path.
pub enum KeelSnapshotSelection {
    Snapshot(KeelSnapshotPorts),
    LiveFallback { reason: SourceSearchReason },
}

impl KeelSnapshotPorts {
    /// Build a snapshot when it can represent the live source policy completely.
    ///
    /// Select a complete immutable snapshot or a live pair with a closed fallback reason.
    pub fn try_open(root: &Path) -> Result<KeelSnapshotSelection, String> {
        let snapshot = match Snapshot::build_scoped(root, SNAPSHOT_SCOPE) {
            Ok(snapshot) => snapshot,
            Err(_) => {
                return Ok(KeelSnapshotSelection::LiveFallback {
                    reason: SourceSearchReason::SnapshotUnavailable,
                });
            }
        };
        if snapshot
            .index()
            .omissions
            .iter()
            .any(|omission| !policy_omission(&omission.path))
        {
            return Ok(KeelSnapshotSelection::LiveFallback {
                reason: SourceSearchReason::SnapshotCoverageGap,
            });
        }
        Ok(KeelSnapshotSelection::Snapshot(Self {
            snapshot: Arc::new(snapshot),
        }))
    }

    #[must_use]
    pub fn search(&self) -> KeelSnapshotSearch {
        KeelSnapshotSearch {
            snapshot: self.snapshot.clone(),
        }
    }

    #[must_use]
    pub fn reader(&self) -> KeelSnapshotReader {
        KeelSnapshotReader {
            snapshot: self.snapshot.clone(),
        }
    }
}

/// Search over one retained Keel snapshot.
pub struct KeelSnapshotSearch {
    snapshot: Arc<Snapshot>,
}

impl BoundedSourceSearch for KeelSnapshotSearch {
    fn search(
        &self,
        terms: &[String],
        bounds: &SourceSearchBounds,
    ) -> Result<Vec<String>, SourceSearchError> {
        self.search_with_provenance(terms, bounds)
            .map(|result| result.paths)
    }

    fn search_with_provenance(
        &self,
        terms: &[String],
        bounds: &SourceSearchBounds,
    ) -> Result<SourceSearchResult, SourceSearchError> {
        let result = self
            .snapshot
            .search_filtered(terms, &snapshot_bounds(bounds), is_live_source_path)
            .map_err(map_search_error)?;
        Ok(SourceSearchResult {
            paths: result.paths,
            provenance: SourceSearchProvenance {
                origin: graphhelm_runtime::ports::SourceSearchOrigin::Snapshot,
                reason: SourceSearchReason::ImmutableSnapshot,
            },
        })
    }
}

/// Prefix reader over the same retained snapshot as [`KeelSnapshotSearch`].
pub struct KeelSnapshotReader {
    snapshot: Arc<Snapshot>,
}

impl BoundedSourceReader for KeelSnapshotReader {
    fn read_prefix(
        &self,
        relative_path: &str,
        max_bytes: u64,
    ) -> Result<SourceExcerpt, SourceReadError> {
        let bytes = self
            .snapshot
            .source(relative_path)
            .map_err(|error| match error {
                SnapshotSourceError::InvalidPath => SourceReadError::Escape,
                SnapshotSourceError::NotIndexed | SnapshotSourceError::Omitted { .. } => {
                    SourceReadError::Unreadable
                }
            })?;
        if !is_live_source_path(relative_path) {
            return Err(SourceReadError::Unreadable);
        }
        let file_len = bytes.len() as u64;
        let take = usize::try_from(max_bytes.min(file_len)).unwrap_or(bytes.len());
        Ok(SourceExcerpt {
            bytes: bytes[..take].to_vec(),
            file_len,
        })
    }
}

/// Marks the live source channel as an explicit Keel fallback.
pub struct LiveFallbackSourceSearch {
    inner: crate::source_channel::WorkspaceSourceChannel,
    reason: SourceSearchReason,
}

impl LiveFallbackSourceSearch {
    pub fn new(inner: crate::source_channel::WorkspaceSourceChannel) -> Self {
        Self::with_reason(inner, SourceSearchReason::LiveFallback)
    }

    pub fn with_reason(
        inner: crate::source_channel::WorkspaceSourceChannel,
        reason: SourceSearchReason,
    ) -> Self {
        Self { inner, reason }
    }
}

impl BoundedSourceSearch for LiveFallbackSourceSearch {
    fn search(
        &self,
        terms: &[String],
        bounds: &SourceSearchBounds,
    ) -> Result<Vec<String>, SourceSearchError> {
        self.inner.search(terms, bounds)
    }

    fn search_with_provenance(
        &self,
        terms: &[String],
        bounds: &SourceSearchBounds,
    ) -> Result<SourceSearchResult, SourceSearchError> {
        self.inner
            .search(terms, bounds)
            .map(|paths| SourceSearchResult {
                paths,
                provenance: SourceSearchProvenance::fallback(self.reason),
            })
    }
}

fn snapshot_bounds(bounds: &SourceSearchBounds) -> SnapshotSearchBounds {
    SnapshotSearchBounds {
        max_entries_visited: bounds.max_entries_visited,
        max_files_scanned: bounds.max_files_scanned,
        max_bytes_scanned: bounds.max_bytes_scanned,
        max_results: bounds.max_results as usize,
        max_terms: bounds.max_terms,
        max_term_bytes: bounds.max_term_bytes,
    }
}

fn map_search_error(error: SnapshotSearchError) -> SourceSearchError {
    match error {
        SnapshotSearchError::TermTooLong
        | SnapshotSearchError::TooManyTerms
        | SnapshotSearchError::TermsTooLarge
        | SnapshotSearchError::CorpusLimitExceeded
        | SnapshotSearchError::ResultLimitExceeded => SourceSearchError::BoundExceeded,
    }
}

fn policy_omission(path: &str) -> bool {
    if path.ends_with('/') {
        // `git ls-files --directory` reports an ignored directory as `dir/`. A normal
        // directory may contain source files the live channel would serve, so only the
        // deliberately excluded directories are safe to keep in snapshot mode.
        return is_policy_excluded_directory(path.trim_end_matches('/'));
    }
    is_policy_excluded_directory(path) || !is_live_source_path(path)
}

fn is_live_source_path(path: &str) -> bool {
    if graphhelm_runtime::context::sensitive_path(path) || is_policy_excluded_directory(path) {
        return false;
    }
    let Some(last) = path.rsplit('/').next() else {
        return false;
    };
    let last = last.to_ascii_lowercase();
    [
        ".rs", ".toml", ".json", ".md", ".yaml", ".yml", ".ps1", ".sh", ".py", ".sql", ".ts",
        ".tsx", ".js", ".jsx", ".go", ".java", ".kt", ".kts", ".rb", ".cs", ".c", ".h", ".cc",
        ".cpp", ".hpp", ".swift", ".php", ".scala", ".ex", ".exs", ".proto", ".graphql", ".txt",
        ".cfg", ".ini",
    ]
    .iter()
    .any(|suffix| last.ends_with(suffix))
}

fn is_policy_excluded_directory(path: &str) -> bool {
    let folded_path = path.to_ascii_lowercase();
    if folded_path == "docs/superpowers/plans" || folded_path.starts_with("docs/superpowers/plans/")
    {
        return true;
    }
    path.split('/').any(|segment| {
        matches!(
            segment.to_ascii_lowercase().as_str(),
            ".factory"
                | ".superpowers"
                | ".git"
                | ".graphhelm"
                | "keyring"
                | ".claude"
                | ".codex"
                | ".cursor"
                | ".windsurf"
                | ".aider"
                | ".worktrees"
                | ".idea"
                | ".vscode"
                | "node_modules"
                | "target"
                | ".venv"
                | "venv"
                | "vendor"
                | "dist"
                | "build"
                | "__pycache__"
                | ".next"
                | ".cache"
        )
    })
}
