use clap::{Parser, Subcommand};
use oxc_allocator::Allocator;
use oxc_ast::ast::{
    BindingPattern, Declaration as JsDeclaration, ExportDefaultDeclarationKind, ModuleExportName,
    Statement,
};
use oxc_parser::Parser as OxcParser;
use oxc_span::SourceType;
use same_file::Handle;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs, io,
    io::Read,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
};

#[cfg(unix)]
mod publish_unix;
#[cfg(windows)]
mod publish_windows;

const MAX_FILES: usize = 10_000;
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
const MAX_QUERY_TERM: usize = 256;
const MAX_QUERY_TERMS: usize = 64;
const MAX_QUERY_RESULTS: usize = 100;
const MAX_INDEX_BYTES: u64 = 16 * 1024 * 1024;
const MAX_INDEX_JSON_DEPTH: usize = 64;
const MAX_INDEX_JSON_STRING_BYTES: usize = 4096;
const MAX_INDEX_JSON_ARRAY_ITEMS: usize = MAX_FILES;
const MAX_INDEX_JSON_OBJECT_KEYS: usize = 16;
const MAX_INDEX_JSON_VALUES: usize = MAX_INDEX_BYTES as usize / 4;
const MAX_CARD_BYTES: usize = 4096;
const MAX_OMISSIONS: usize = 10_000;
const MAX_SCOPE_PREFIXES: usize = 128;
const MAX_SCOPE_PREFIX_BYTES: usize = 8 * 1024;
const MAX_GIT_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
const MAX_VUE_INTERPOLATION_BYTES: usize = 64 * 1024;
const MAX_VUE_INTERPOLATION_CANDIDATES: usize = 128;

#[derive(Parser)]
#[command(name = "keel-contract-index", version)]
struct Cli {
    #[command(subcommand)]
    command: CommandKind,
}

#[derive(Subcommand)]
enum CommandKind {
    Scan {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    Verify {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        index: PathBuf,
    },
    Query {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        index: PathBuf,
        #[arg(long)]
        term: String,
    },
    ProposeCard {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        index: PathBuf,
        #[arg(long)]
        term: String,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Index {
    pub schema: String,
    pub repo: String,
    pub snapshot_digest: String,
    pub files: Vec<FileCard>,
    pub omissions: Vec<Omission>,
}
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct FileCard {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
    pub language: String,
    pub parse: ParseStatus,
    pub declarations: Vec<Declaration>,
}
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParseStatus {
    Parsed,
    Partial,
    Unsupported,
    Invalid,
    InvalidUtf8,
}
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Declaration {
    pub name: String,
    pub kind: String,
}
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Omission {
    pub path: String,
    pub reason: String,
}

/// An immutable, bounded view of one repository scan.
///
/// The index cards and source bytes are acquired together. Queries on this value never touch
/// the filesystem, so a caller can pass the same evidence to both an index lookup and a source
/// reader without observing two different working-tree states.
pub struct Snapshot {
    index: Index,
    sources: BTreeMap<String, Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotQuery {
    pub files: Vec<FileCard>,
    pub omissions: Vec<Omission>,
    pub truncated: bool,
    pub coverage_gaps: CoverageGaps,
}

/// A bounded search over the source bytes retained by one [`Snapshot`].
///
/// The result contains paths only. Callers that need excerpts can use [`Snapshot::source`]
/// against the same immutable snapshot, so a search and a read cannot observe different
/// working-tree contents. Policy-specific path filtering remains with the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotSearch {
    pub paths: Vec<String>,
    pub truncated: bool,
    pub coverage_gaps: CoverageGaps,
}

/// Caller-owned ceilings for a retained snapshot search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotSearchBounds {
    pub max_entries_visited: usize,
    pub max_files_scanned: usize,
    pub max_bytes_scanned: u64,
    pub max_results: usize,
    pub max_terms: usize,
    pub max_term_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotSourceError {
    InvalidPath,
    NotIndexed,
    Omitted { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotQueryError {
    TermTooLong,
    TooManyTerms,
    ResultLimitExceeded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotSearchError {
    TermTooLong,
    TooManyTerms,
    TermsTooLarge,
    CorpusLimitExceeded,
    ResultLimitExceeded,
}

impl SnapshotSearchError {
    pub fn code(self) -> &'static str {
        "GHKEEL004_LIMIT"
    }
}

impl SnapshotQueryError {
    pub fn code(self) -> &'static str {
        match self {
            Self::TermTooLong => "GHKEEL004_LIMIT",
            Self::TooManyTerms | Self::ResultLimitExceeded => "GHKEEL004_LIMIT",
        }
    }
}

impl SnapshotSourceError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidPath => "GHKEEL005_UNSAFE_PATH",
            Self::NotIndexed => "GHKEEL001_INDEX",
            Self::Omitted { .. } => "GHKEEL002_COVERAGE_GAP",
        }
    }
}

impl Snapshot {
    pub fn build(repo: &Path) -> Result<Self, PublicError> {
        build_snapshot(repo, None, true, &[]).map_err(public_error)
    }

    /// Build a source-retaining snapshot while excluding repository-relative directory prefixes.
    ///
    /// Exclusions are part of the snapshot omissions and therefore its digest. The prefixes are
    /// deliberately supplied by the caller so this generic scanner does not encode repository
    /// policy such as `.factory/`.
    pub fn build_scoped(
        repo: &Path,
        excluded_directory_prefixes: &[&str],
    ) -> Result<Self, PublicError> {
        let prefixes =
            validate_scope_prefixes(excluded_directory_prefixes).map_err(public_error)?;
        build_snapshot(repo, None, true, &prefixes).map_err(public_error)
    }

    pub fn index(&self) -> &Index {
        &self.index
    }

    pub fn digest(&self) -> &str {
        &self.index.snapshot_digest
    }

    pub fn source(&self, path: &str) -> Result<&[u8], SnapshotSourceError> {
        if !safe_relative(path) {
            return Err(SnapshotSourceError::InvalidPath);
        }
        if let Some(bytes) = self.sources.get(path) {
            return Ok(bytes.as_slice());
        }
        if let Some(omission) = self.index.omissions.iter().find(|omission| {
            omission.path.trim_end_matches('/') == path
                || (omission.path.ends_with('/') && path.starts_with(&omission.path))
        }) {
            return Err(SnapshotSourceError::Omitted {
                reason: omission.reason.clone(),
            });
        }
        Err(SnapshotSourceError::NotIndexed)
    }

    pub fn query(&self, term: &str) -> Result<SnapshotQuery, SnapshotQueryError> {
        if term.len() > MAX_QUERY_TERM {
            return Err(SnapshotQueryError::TermTooLong);
        }
        let needle = term.to_lowercase();
        let all = self
            .index
            .files
            .iter()
            .filter(|file| {
                file.path.to_lowercase().contains(&needle)
                    || file
                        .declarations
                        .iter()
                        .any(|declaration| declaration.name.to_lowercase().contains(&needle))
            })
            .cloned()
            .collect::<Vec<_>>();
        let truncated = all.len() > MAX_QUERY_RESULTS;
        let files = all.into_iter().take(MAX_QUERY_RESULTS).collect();
        let omissions = self
            .index
            .omissions
            .iter()
            .filter(|omission| omission.path.to_lowercase().contains(&needle))
            .cloned()
            .collect();
        Ok(SnapshotQuery {
            files,
            omissions,
            truncated,
            coverage_gaps: coverage_gaps(&self.index),
        })
    }

    /// Search retained source bytes for any of the supplied terms.
    ///
    /// Terms are trimmed, case-folded, and de-duplicated. A path is ranked by the number of
    /// distinct terms it matches, descending, then by its stable relative path. The result cap
    /// is caller supplied but cannot exceed the repository-wide bound. This method never reads
    /// the filesystem after [`Snapshot::build`] or [`Snapshot::build_scoped`] returns.
    pub fn search(
        &self,
        terms: &[String],
        bounds: &SnapshotSearchBounds,
    ) -> Result<SnapshotSearch, SnapshotSearchError> {
        self.search_filtered(terms, bounds, |_| true)
    }

    /// Search only paths admitted by the caller's source policy. The predicate runs before
    /// source bytes are searched or the result cap is applied.
    pub fn search_filtered(
        &self,
        terms: &[String],
        bounds: &SnapshotSearchBounds,
        include_path: impl Fn(&str) -> bool,
    ) -> Result<SnapshotSearch, SnapshotSearchError> {
        if terms.len() > bounds.max_terms || terms.len() > MAX_QUERY_TERMS {
            return Err(SnapshotSearchError::TooManyTerms);
        }
        if bounds.max_results > MAX_QUERY_RESULTS {
            return Err(SnapshotSearchError::ResultLimitExceeded);
        }
        let mut needles = BTreeSet::new();
        let mut term_bytes = 0u64;
        for term in terms {
            let term = term.trim();
            if term.is_empty() {
                continue;
            }
            if term.len() > MAX_QUERY_TERM {
                return Err(SnapshotSearchError::TermTooLong);
            }
            term_bytes = term_bytes.saturating_add(term.len() as u64);
            needles.insert(term.to_lowercase());
        }
        if term_bytes > bounds.max_term_bytes {
            return Err(SnapshotSearchError::TermsTooLarge);
        }
        if self.sources.len() > bounds.max_entries_visited {
            return Err(SnapshotSearchError::CorpusLimitExceeded);
        }
        let eligible = self
            .sources
            .iter()
            .filter(|(path, _)| include_path(path))
            .collect::<Vec<_>>();
        let scanned_bytes = eligible
            .iter()
            .map(|(_, bytes)| *bytes)
            .try_fold(0u64, |total, bytes| total.checked_add(bytes.len() as u64))
            .ok_or(SnapshotSearchError::CorpusLimitExceeded)?;
        if eligible.len() > bounds.max_files_scanned || scanned_bytes > bounds.max_bytes_scanned {
            return Err(SnapshotSearchError::CorpusLimitExceeded);
        }
        let mut ranked = eligible
            .into_iter()
            .filter_map(|(path, bytes)| {
                let text_lower = std::str::from_utf8(bytes).ok().map(str::to_lowercase);
                let matched = needles
                    .iter()
                    .filter(|needle| {
                        text_lower
                            .as_deref()
                            .is_some_and(|text| text.contains(needle.as_str()))
                    })
                    .count();
                (matched > 0).then(|| (path.clone(), matched))
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|(left_path, left_score), (right_path, right_score)| {
            right_score
                .cmp(left_score)
                .then_with(|| left_path.cmp(right_path))
        });
        let truncated = ranked.len() > bounds.max_results;
        ranked.truncate(bounds.max_results);
        let paths = ranked.into_iter().map(|(path, _)| path).collect();
        Ok(SnapshotSearch {
            paths,
            truncated,
            coverage_gaps: coverage_gaps(&self.index),
        })
    }
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CandidateCard {
    scope_paths: Vec<String>,
    exported_symbols: Vec<Declaration>,
    exported_symbols_truncated: bool,
    source_refs: Vec<SourceRef>,
    source_parse: ParseStatus,
    ignored_directory_contents_unobserved: bool,
    coverage_gaps: CoverageGaps,
    acceptance_criteria: Vec<String>,
    refusals: Vec<String>,
    status: String,
    candidate: bool,
    snapshot_digest: String,
}
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CoverageGaps {
    pub partial_files: usize,
    pub unsupported_files: usize,
    pub invalid_rust_files: usize,
    pub omitted_paths: usize,
    pub omission_reasons: BTreeMap<String, usize>,
}
#[derive(Debug, Serialize)]
struct SourceRef {
    path: String,
    sha256: String,
}

#[derive(Debug)]
enum AppError {
    Message(String),
    Io(io::Error),
    Json(serde_json::Error),
}
impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Message(s) => f.write_str(s),
            Self::Io(e) => e.fmt(f),
            Self::Json(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for AppError {}
impl From<io::Error> for AppError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

pub fn main_entry() {
    if let Err(error) = run_cli() {
        let out = serde_json::json!({
            "ok": false,
            "error": {"code": error.code, "message": error.message}
        });
        println!(
            "{}",
            serde_json::to_string(&out).unwrap_or_else(|_| "{\"ok\":false}".into())
        );
        std::process::exit(1);
    }
}

fn run_cli() -> Result<(), PublicError> {
    let command = Cli::parse().command;
    let output = execute_public(command_to_operation(command))?;
    println!(
        "{}",
        serde_json::to_string(&output).unwrap_or_else(|_| "{\"ok\":false}".into())
    );
    Ok(())
}

#[derive(Debug)]
pub enum Operation {
    Scan {
        repo: PathBuf,
        out: PathBuf,
    },
    Verify {
        repo: PathBuf,
        index: PathBuf,
    },
    Query {
        repo: PathBuf,
        index: PathBuf,
        term: String,
    },
    ProposeCard {
        repo: PathBuf,
        index: PathBuf,
        term: String,
    },
}

fn command_to_operation(command: CommandKind) -> Operation {
    match command {
        CommandKind::Scan { repo, out } => Operation::Scan { repo, out },
        CommandKind::Verify { repo, index } => Operation::Verify { repo, index },
        CommandKind::Query { repo, index, term } => Operation::Query { repo, index, term },
        CommandKind::ProposeCard { repo, index, term } => {
            Operation::ProposeCard { repo, index, term }
        }
    }
}

pub fn execute(operation: Operation) -> Result<serde_json::Value, String> {
    execute_inner(operation).map_err(|e| e.to_string())
}

/// A bounded diagnostic for the public GraphHelm CLI. Index bytes and filesystem errors are
/// untrusted, so their original text must never be copied into the CLI JSON envelope.
#[derive(Debug)]
pub struct PublicError {
    pub code: &'static str,
    pub message: &'static str,
}

pub fn execute_public(operation: Operation) -> Result<serde_json::Value, PublicError> {
    execute_inner(operation).map_err(public_error)
}

fn public_error(error: AppError) -> PublicError {
    let kind = match &error {
        AppError::Message(message) => message.split_whitespace().next().unwrap_or(""),
        AppError::Io(_) | AppError::Json(_) => "KIDX1",
    };
    match kind {
        "KIDX2" => PublicError {
            code: "GHKEEL002_STALE",
            message: "Keel index is stale; rebuild it before retrieval",
        },
        "KIDX3" => PublicError {
            code: "GHKEEL003_REPOSITORY",
            message: "Keel repository is unavailable or invalid",
        },
        "KIDX4" => PublicError {
            code: "GHKEEL004_LIMIT",
            message: "Keel index input exceeds a supported limit",
        },
        "KIDX5" => PublicError {
            code: "GHKEEL005_UNSAFE_PATH",
            message: "Keel refused an unsafe repository or output path",
        },
        "KIDX6" => PublicError {
            code: "GHKEEL006_AMBIGUOUS",
            message: "Keel query has no unique source match",
        },
        _ => PublicError {
            code: "GHKEEL001_INDEX",
            message: "Keel index is unavailable or unreadable",
        },
    }
}

fn execute_inner(operation: Operation) -> Result<serde_json::Value, AppError> {
    match operation {
        Operation::Scan { repo, out } => {
            let target = OutputTarget::open(&repo, &out)?;
            let index = build_index(&repo, None)?;
            write_json(&target, &index)?;
            Ok(
                serde_json::json!({"ok":true,"snapshot_digest":index.snapshot_digest,"files":index.files.len(),"out":out}),
            )
        }
        Operation::Verify { repo, index } => {
            let expected = read_index(&index)?;
            let actual = fresh_index(&repo, &index, &expected)?;
            if expected.snapshot_digest != actual.snapshot_digest
                || expected.schema != actual.schema
                || expected.repo != actual.repo
                || expected.files != actual.files
                || expected.omissions != actual.omissions
            {
                return Err(AppError::Message(
                    "KIDX2 stale index: cards or omissions differ".into(),
                ));
            }
            Ok(serde_json::json!({"ok":true,"snapshot_digest":actual.snapshot_digest}))
        }
        Operation::Query { repo, index, term } => {
            if term.len() > MAX_QUERY_TERM {
                return Err(AppError::Message("KIDX4 query term limit exceeded".into()));
            }
            let idx = read_index(&index)?;
            let _actual = fresh_index(&repo, &index, &idx)?;
            let needle = term.to_lowercase();
            let all: Vec<&FileCard> = idx
                .files
                .iter()
                .filter(|f| {
                    f.path.to_lowercase().contains(&needle)
                        || f.declarations
                            .iter()
                            .any(|d| d.name.to_lowercase().contains(&needle))
                })
                .collect();
            let truncated = all.len() > MAX_QUERY_RESULTS;
            let files = all.into_iter().take(MAX_QUERY_RESULTS).collect::<Vec<_>>();
            Ok(
                serde_json::json!({"ok":true,"snapshot_digest":idx.snapshot_digest,"term":term,"truncated":truncated,"files":files,"ignoredDirectoryContentsUnobserved":ignored_directory_contents_unobserved(&idx),"coverageGaps":coverage_gaps(&idx)}),
            )
        }
        Operation::ProposeCard { repo, index, term } => {
            if term.len() > MAX_QUERY_TERM {
                return Err(AppError::Message("KIDX4 query term limit exceeded".into()));
            }
            let idx = read_index(&index)?;
            let _actual = fresh_index(&repo, &index, &idx)?;
            let needle = term.to_lowercase();
            let hits: Vec<&FileCard> = idx
                .files
                .iter()
                .filter(|f| {
                    f.path.to_lowercase().contains(&needle)
                        || f.declarations
                            .iter()
                            .any(|d| d.name.to_lowercase().contains(&needle))
                })
                .collect();
            if hits.is_empty() {
                return Err(AppError::Message("KIDX6 no unique source hit".into()));
            }
            if hits.len() != 1 {
                return Err(AppError::Message(
                    "KIDX6 term matched multiple source files".into(),
                ));
            }
            let hit = hits[0];
            let card = CandidateCard {
                scope_paths: vec![hit.path.clone()],
                exported_symbols: hit.declarations.iter().take(8).cloned().collect(),
                exported_symbols_truncated: hit.declarations.len() > 8,
                source_refs: vec![SourceRef {
                    path: hit.path.clone(),
                    sha256: hit.sha256.clone(),
                }],
                source_parse: hit.parse.clone(),
                ignored_directory_contents_unobserved: ignored_directory_contents_unobserved(&idx),
                coverage_gaps: coverage_gaps(&idx),
                acceptance_criteria: Vec::new(),
                refusals: Vec::new(),
                status: "needs_product_contract".into(),
                candidate: true,
                snapshot_digest: idx.snapshot_digest,
            };
            let encoded = serde_json::to_vec(&card)?;
            if encoded.len() > MAX_CARD_BYTES {
                return Err(AppError::Message(
                    "KIDX4 candidate card size limit exceeded".into(),
                ));
            }
            Ok(serde_json::to_value(card)?)
        }
    }
}

struct OutputTarget {
    repo_root: PathBuf,
    parent_path: PathBuf,
    parent: fs::File,
    parent_identity: Handle,
    name: OsString,
}

impl OutputTarget {
    fn open(repo: &Path, out: &Path) -> Result<Self, AppError> {
        Self::open_with(repo, out, open_output_parent)
    }

    fn open_with(
        repo: &Path,
        out: &Path,
        opener: impl FnOnce(&Path) -> io::Result<fs::File>,
    ) -> Result<Self, AppError> {
        let repo_root = fs::canonicalize(repo)?;
        let parent_path = out
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let name = out
            .file_name()
            .ok_or_else(|| AppError::Message("KIDX5 output file name is required".into()))?
            .to_os_string();
        let canonical_parent = fs::canonicalize(&parent_path)
            .map_err(|_| AppError::Message("KIDX5 output parent must exist".into()))?;
        if canonical_parent.starts_with(&repo_root) {
            return Err(AppError::Message(
                "KIDX5 output must be outside repository".into(),
            ));
        }
        let parent = opener(&canonical_parent)?;
        ensure_opened_parent_outside_repo(&parent, &repo_root)?;
        let parent_identity = Handle::from_file(parent.try_clone()?)?;
        let target = Self {
            repo_root,
            parent_path,
            parent,
            parent_identity,
            name,
        };
        target.check_alias()?;
        Ok(target)
    }

    fn check_alias(&self) -> Result<(), AppError> {
        ensure_opened_parent_outside_repo(&self.parent, &self.repo_root)?;
        let current = fs::canonicalize(&self.parent_path)
            .map_err(|_| AppError::Message("KIDX5 output parent disappeared".into()))?;
        if current.starts_with(&self.repo_root)
            || Handle::from_path(&self.parent_path)
                .map_err(|_| AppError::Message("KIDX5 output parent changed".into()))?
                != self.parent_identity
        {
            return Err(AppError::Message("KIDX5 output parent changed".into()));
        }
        Ok(())
    }
}

fn open_output_parent(path: &Path) -> io::Result<fs::File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
    }
    #[cfg(windows)]
    {
        publish_windows::open_parent(path)
    }
}

fn ensure_opened_parent_outside_repo(parent: &fs::File, repo_root: &Path) -> Result<(), AppError> {
    #[cfg(unix)]
    let inside = publish_unix::parent_is_within_repo(parent, repo_root)?;
    #[cfg(windows)]
    let inside = publish_windows::parent_is_within_repo(parent, repo_root)?;
    if inside {
        return Err(AppError::Message(
            "KIDX5 opened output parent is inside repository".into(),
        ));
    }
    Ok(())
}

fn write_json(target: &OutputTarget, value: &Index) -> Result<(), AppError> {
    validate_index_limits(value)?;
    let encoded = serde_json::to_vec_pretty(value)?;
    if encoded.len() as u64 > MAX_INDEX_BYTES {
        return Err(AppError::Message("KIDX4 index size limit exceeded".into()));
    }
    preflight_index_json(&encoded)?;
    target.check_alias()?;
    #[cfg(unix)]
    publish_unix::publish_bytes(&target.parent, &target.name, &encoded)?;
    #[cfg(windows)]
    publish_windows::write_atomic(&target.parent, &target.name, &encoded)?;
    target.check_alias()?;
    Ok(())
}
fn read_index(path: &Path) -> Result<Index, AppError> {
    let bytes = read_bounded(path, MAX_INDEX_BYTES)?;
    preflight_index_json(&bytes)?;
    let index: Index = serde_json::from_slice(&bytes)?;
    validate_index_limits(&index)?;
    Ok(index)
}

fn validate_index_limits(index: &Index) -> Result<(), AppError> {
    if index.files.len() > MAX_FILES
        || index.omissions.len() > MAX_OMISSIONS
        || index.schema.len() > MAX_INDEX_JSON_STRING_BYTES
        || index.repo.len() > MAX_INDEX_JSON_STRING_BYTES
        || index.snapshot_digest.len() > MAX_INDEX_JSON_STRING_BYTES
    {
        return Err(AppError::Message(
            "KIDX4 index collection limit exceeded".into(),
        ));
    }
    for file in &index.files {
        if file.declarations.len() > MAX_INDEX_JSON_ARRAY_ITEMS
            || file.path.len() > MAX_INDEX_JSON_STRING_BYTES
            || file.sha256.len() > MAX_INDEX_JSON_STRING_BYTES
            || file.language.len() > MAX_INDEX_JSON_STRING_BYTES
        {
            return Err(AppError::Message("KIDX4 index field limit exceeded".into()));
        }
        for declaration in &file.declarations {
            if declaration.name.len() > MAX_INDEX_JSON_STRING_BYTES
                || declaration.kind.len() > MAX_INDEX_JSON_STRING_BYTES
            {
                return Err(AppError::Message("KIDX4 index field limit exceeded".into()));
            }
        }
    }
    for omission in &index.omissions {
        if omission.path.len() > MAX_INDEX_JSON_STRING_BYTES
            || omission.reason.len() > MAX_INDEX_JSON_STRING_BYTES
        {
            return Err(AppError::Message("KIDX4 index field limit exceeded".into()));
        }
    }
    Ok(())
}

enum PreflightError {
    Malformed,
    Limit,
}

fn preflight_index_json(bytes: &[u8]) -> Result<(), AppError> {
    let mut cursor = 0;
    let mut values = 0;
    match scan_json_value(bytes, &mut cursor, 0, &mut values) {
        Err(PreflightError::Limit) => Err(AppError::Message(
            "KIDX4 index structure limit exceeded".into(),
        )),
        Err(PreflightError::Malformed) => Ok(()),
        Ok(()) => Ok(()),
    }
}

fn scan_json_value(
    bytes: &[u8],
    cursor: &mut usize,
    depth: usize,
    values: &mut usize,
) -> Result<(), PreflightError> {
    if depth > MAX_INDEX_JSON_DEPTH || *values >= MAX_INDEX_JSON_VALUES {
        return Err(PreflightError::Limit);
    }
    *values += 1;
    skip_json_whitespace(bytes, cursor);
    let Some(&byte) = bytes.get(*cursor) else {
        return Err(PreflightError::Malformed);
    };
    match byte {
        b'{' => scan_json_object(bytes, cursor, depth, values),
        b'[' => scan_json_array(bytes, cursor, depth, values),
        b'"' => scan_json_string(bytes, cursor),
        b't' => scan_json_literal(bytes, cursor, b"true"),
        b'f' => scan_json_literal(bytes, cursor, b"false"),
        b'n' => scan_json_literal(bytes, cursor, b"null"),
        b'-' | b'0'..=b'9' => scan_json_number(bytes, cursor),
        _ => Err(PreflightError::Malformed),
    }
}

fn scan_json_object(
    bytes: &[u8],
    cursor: &mut usize,
    depth: usize,
    values: &mut usize,
) -> Result<(), PreflightError> {
    *cursor += 1;
    skip_json_whitespace(bytes, cursor);
    if bytes.get(*cursor) == Some(&b'}') {
        *cursor += 1;
        return Ok(());
    }
    for key_count in 0..=MAX_INDEX_JSON_OBJECT_KEYS {
        if key_count == MAX_INDEX_JSON_OBJECT_KEYS {
            return Err(PreflightError::Limit);
        }
        skip_json_whitespace(bytes, cursor);
        scan_json_string(bytes, cursor)?;
        skip_json_whitespace(bytes, cursor);
        if bytes.get(*cursor) != Some(&b':') {
            return Err(PreflightError::Malformed);
        }
        *cursor += 1;
        scan_json_value(bytes, cursor, depth + 1, values)?;
        skip_json_whitespace(bytes, cursor);
        match bytes.get(*cursor) {
            Some(b',') => *cursor += 1,
            Some(b'}') => {
                *cursor += 1;
                return Ok(());
            }
            _ => return Err(PreflightError::Malformed),
        }
    }
    Err(PreflightError::Malformed)
}

fn scan_json_array(
    bytes: &[u8],
    cursor: &mut usize,
    depth: usize,
    values: &mut usize,
) -> Result<(), PreflightError> {
    *cursor += 1;
    skip_json_whitespace(bytes, cursor);
    if bytes.get(*cursor) == Some(&b']') {
        *cursor += 1;
        return Ok(());
    }
    for item_count in 0..=MAX_INDEX_JSON_ARRAY_ITEMS {
        if item_count == MAX_INDEX_JSON_ARRAY_ITEMS {
            return Err(PreflightError::Limit);
        }
        scan_json_value(bytes, cursor, depth + 1, values)?;
        skip_json_whitespace(bytes, cursor);
        match bytes.get(*cursor) {
            Some(b',') => *cursor += 1,
            Some(b']') => {
                *cursor += 1;
                return Ok(());
            }
            _ => return Err(PreflightError::Malformed),
        }
    }
    Err(PreflightError::Malformed)
}

fn scan_json_string(bytes: &[u8], cursor: &mut usize) -> Result<(), PreflightError> {
    if bytes.get(*cursor) != Some(&b'"') {
        return Err(PreflightError::Malformed);
    }
    *cursor += 1;
    let mut length = 0;
    while let Some(&byte) = bytes.get(*cursor) {
        *cursor += 1;
        match byte {
            b'"' => return Ok(()),
            b'\\' => {
                let Some(&escaped) = bytes.get(*cursor) else {
                    return Err(PreflightError::Malformed);
                };
                *cursor += 1;
                if escaped == b'u' {
                    if bytes.len().saturating_sub(*cursor) < 4
                        || !bytes[*cursor..*cursor + 4]
                            .iter()
                            .all(|value| value.is_ascii_hexdigit())
                    {
                        return Err(PreflightError::Malformed);
                    }
                    *cursor += 4;
                } else if !matches!(
                    escaped,
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't'
                ) {
                    return Err(PreflightError::Malformed);
                }
            }
            0..=0x1f => return Err(PreflightError::Malformed),
            _ => {}
        }
        length += 1;
        if length > MAX_INDEX_JSON_STRING_BYTES {
            return Err(PreflightError::Limit);
        }
    }
    Err(PreflightError::Malformed)
}

fn scan_json_literal(
    bytes: &[u8],
    cursor: &mut usize,
    literal: &[u8],
) -> Result<(), PreflightError> {
    if bytes.get(*cursor..(*cursor).saturating_add(literal.len())) != Some(literal) {
        return Err(PreflightError::Malformed);
    }
    *cursor += literal.len();
    Ok(())
}

fn scan_json_number(bytes: &[u8], cursor: &mut usize) -> Result<(), PreflightError> {
    let start = *cursor;
    while matches!(
        bytes.get(*cursor),
        Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
    ) {
        *cursor += 1;
    }
    if *cursor == start {
        Err(PreflightError::Malformed)
    } else {
        Ok(())
    }
}

fn skip_json_whitespace(bytes: &[u8], cursor: &mut usize) {
    while matches!(bytes.get(*cursor), Some(b' ' | b'\n' | b'\r' | b'\t')) {
        *cursor += 1;
    }
}

fn fresh_index(repo: &Path, index_path: &Path, expected: &Index) -> Result<Index, AppError> {
    let actual = build_index(repo, Some(index_path))?;
    if expected.schema != actual.schema
        || expected.repo != actual.repo
        || expected.snapshot_digest != actual.snapshot_digest
        || expected.files != actual.files
        || expected.omissions != actual.omissions
    {
        return Err(AppError::Message(
            "KIDX2 stale index: verify before query".into(),
        ));
    }
    Ok(actual)
}
fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, AppError> {
    let file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(AppError::Message("KIDX4 input size limit exceeded".into()));
    }
    Ok(bytes)
}

fn build_index(repo: &Path, excluded: Option<&Path>) -> Result<Index, AppError> {
    Ok(build_snapshot(repo, excluded, false, &[])?.index)
}

fn build_snapshot(
    repo: &Path,
    excluded: Option<&Path>,
    retain_sources: bool,
    scope_prefixes: &[String],
) -> Result<Snapshot, AppError> {
    let root = fs::canonicalize(repo)?;
    if !root.is_dir() {
        return Err(AppError::Message("KIDX3 repo is not a directory".into()));
    }
    let tracked = git_files(&root)?;
    if tracked.len() > MAX_FILES {
        return Err(AppError::Message("KIDX4 file limit exceeded".into()));
    }
    ensure_unique_tracked_paths(&tracked)?;
    let excluded = excluded.and_then(|p| fs::canonicalize(p).ok());
    let mut files = Vec::with_capacity(tracked.len());
    let mut sources = retain_sources.then(BTreeMap::new);
    let mut omissions = untracked_omissions(&root, excluded.as_deref(), scope_prefixes)?;
    if omissions.len() + scope_prefixes.len() > MAX_OMISSIONS {
        return Err(AppError::Message("KIDX4 omission limit exceeded".into()));
    }
    omissions.extend(scope_prefixes.iter().map(|prefix| Omission {
        path: format!("{prefix}/"),
        reason: "scope_excluded".into(),
    }));
    let mut total: u64 = 0;
    for (mode, rel) in tracked {
        if mode == "120000" {
            return Err(AppError::Message(format!("KIDX5 symlink rejected: {rel}")));
        }
        if !safe_relative(&rel) {
            return Err(AppError::Message(format!("KIDX5 unsafe path: {rel}")));
        }
        if scope_prefixes
            .iter()
            .any(|prefix| path_matches_prefix(&rel, prefix))
        {
            continue;
        }
        let full = root.join(&rel);
        let mut ancestor = root.clone();
        for part in Path::new(&rel).components() {
            ancestor.push(part);
            if fs::symlink_metadata(&ancestor)?.file_type().is_symlink() {
                return Err(AppError::Message(format!("KIDX5 symlink rejected: {rel}")));
            }
        }
        if !fs::canonicalize(&full)?.starts_with(&root) {
            return Err(AppError::Message(format!(
                "KIDX5 path escapes repository: {rel}"
            )));
        }
        let meta = fs::symlink_metadata(&full)?;
        if meta.file_type().is_symlink() {
            return Err(AppError::Message(format!("KIDX5 symlink rejected: {rel}")));
        }
        if !meta.is_file() {
            omissions.push(Omission {
                path: rel,
                reason: "not_a_regular_file".into(),
            });
            continue;
        }
        if meta.len() > MAX_FILE_BYTES {
            omissions.push(Omission {
                path: rel,
                reason: "file_size_limit".into(),
            });
            continue;
        }
        let opened = Handle::from_path(&full)?;
        let mut check_path = root.clone();
        for part in Path::new(&rel).components() {
            check_path.push(part);
            if fs::symlink_metadata(&check_path)?.file_type().is_symlink() {
                return Err(AppError::Message(format!("KIDX5 symlink rejected: {rel}")));
            }
        }
        if !fs::canonicalize(&full)?.starts_with(&root) || opened != Handle::from_path(&full)? {
            return Err(AppError::Message(format!(
                "KIDX5 file changed while opening: {rel}"
            )));
        }
        let mut content = Vec::new();
        opened
            .as_file()
            .take(MAX_FILE_BYTES.saturating_add(1))
            .read_to_end(&mut content)?;
        if content.len() as u64 > MAX_FILE_BYTES {
            return Err(AppError::Message(format!(
                "KIDX4 file size changed while scanning: {rel}"
            )));
        }
        let bytes = content.len() as u64;
        total = total
            .checked_add(bytes)
            .ok_or_else(|| AppError::Message("KIDX4 byte limit overflow".into()))?;
        if total > MAX_TOTAL_BYTES {
            return Err(AppError::Message(
                "KIDX4 aggregate byte limit exceeded".into(),
            ));
        }
        let digest = digest(&content);
        if let Some(sources) = &mut sources {
            sources.insert(rel.clone(), content.clone());
        }
        let lang = language(&rel);
        let (parse, declarations) = if lang == "rust" {
            match std::str::from_utf8(&content) {
                Ok(source) => match syn::parse_file(source) {
                    Ok(file) => (ParseStatus::Parsed, public_declarations(&file)),
                    Err(_) => (ParseStatus::Invalid, Vec::new()),
                },
                Err(_) => (ParseStatus::InvalidUtf8, Vec::new()),
            }
        } else if matches!(lang.as_str(), "javascript" | "typescript" | "jsx" | "tsx") {
            parse_javascript(&content, &lang)
        } else if lang == "vue" {
            parse_vue(&content)
        } else {
            (ParseStatus::Unsupported, Vec::new())
        };
        files.push(FileCard {
            path: rel,
            sha256: digest,
            bytes,
            language: lang,
            parse,
            declarations,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    omissions.sort_by(|a, b| a.path.cmp(&b.path));
    let snapshot_digest = snapshot_digest(&files, &omissions);
    Ok(Snapshot {
        index: Index {
            schema: "keel.contract-index.v1".into(),
            repo: "working-tree".into(),
            snapshot_digest,
            files,
            omissions,
        },
        sources: sources.unwrap_or_default(),
    })
}

fn ensure_unique_tracked_paths(tracked: &[(String, String)]) -> Result<(), AppError> {
    let mut paths = BTreeSet::new();
    for (_, path) in tracked {
        if !paths.insert(path) {
            return Err(AppError::Message(
                "KIDX3 duplicate path in git index".into(),
            ));
        }
    }
    Ok(())
}

fn git_files(root: &Path) -> Result<Vec<(String, String)>, AppError> {
    let text = String::from_utf8(git_stdout_bounded(root, &["ls-files", "-s", "-z"])?)
        .map_err(|_| AppError::Message("KIDX3 git output is not UTF-8".into()))?;
    let mut out = Vec::new();
    for record in text.split('\0').filter(|s| !s.is_empty()) {
        let (head, path) = record
            .split_once('\t')
            .ok_or_else(|| AppError::Message("KIDX3 malformed git index".into()))?;
        let mode = head.split_whitespace().next().unwrap_or("").to_string();
        out.push((mode, path.to_string()));
    }
    Ok(out)
}

fn untracked_omissions(
    root: &Path,
    excluded: Option<&Path>,
    scope_prefixes: &[String],
) -> Result<Vec<Omission>, AppError> {
    let mut out = Vec::new();
    for (args, reason) in [
        (
            &["ls-files", "--others", "--exclude-standard", "-z"][..],
            "untracked_excluded",
        ),
        (
            &[
                "ls-files",
                "--others",
                "--ignored",
                "--exclude-standard",
                "--directory",
                "-z",
            ][..],
            "ignored_excluded",
        ),
    ] {
        let text = String::from_utf8(git_stdout_bounded(root, args)?)
            .map_err(|_| AppError::Message("KIDX3 git output is not UTF-8".into()))?;
        for path in text.split('\0').filter(|s| !s.is_empty()) {
            if safe_relative(path)
                && excluded != Some(&root.join(path))
                && !scope_prefixes
                    .iter()
                    .any(|prefix| path_matches_prefix(path, prefix))
            {
                if out.len() >= MAX_OMISSIONS {
                    return Err(AppError::Message("KIDX4 omission limit exceeded".into()));
                }
                out.push(Omission {
                    path: path.to_string(),
                    reason: reason.into(),
                });
            }
        }
    }
    Ok(out)
}

fn git_stdout_bounded(root: &Path, args: &[&str]) -> Result<Vec<u8>, AppError> {
    let path = root
        .to_str()
        .ok_or_else(|| AppError::Message("KIDX5 invalid repo path".into()))?;
    let mut child = Command::new("git")
        .args(["-c", "core.fsmonitor=false", "-C", path])
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AppError::Message("KIDX3 git pipe missing".into()))?;
    let mut bytes = Vec::new();
    stdout
        .take((MAX_GIT_OUTPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_GIT_OUTPUT_BYTES {
        let _ = child.kill();
        let _ = child.wait();
        return Err(AppError::Message("KIDX4 git output limit exceeded".into()));
    }
    if !child.wait()?.success() {
        return Err(AppError::Message("KIDX3 git command failed".into()));
    }
    Ok(bytes)
}

fn safe_relative(path: &str) -> bool {
    let p = Path::new(path);
    !p.is_absolute() && p.components().all(|c| matches!(c, Component::Normal(_)))
}

fn validate_scope_prefixes(prefixes: &[&str]) -> Result<Vec<String>, AppError> {
    if prefixes.len() > MAX_SCOPE_PREFIXES
        || prefixes
            .iter()
            .fold(0usize, |total, prefix| total.saturating_add(prefix.len()))
            > MAX_SCOPE_PREFIX_BYTES
    {
        return Err(AppError::Message(
            "KIDX4 scope prefix limit exceeded".into(),
        ));
    }
    let mut normalized = BTreeSet::new();
    for prefix in prefixes {
        if prefix.is_empty()
            || prefix.contains('\\')
            || prefix.starts_with('/')
            || prefix.ends_with('/')
            || !safe_relative(prefix)
        {
            return Err(AppError::Message(
                "KIDX5 unsafe or non-normalized scope prefix".into(),
            ));
        }
        let candidate = Path::new(prefix)
            .components()
            .map(|component| match component {
                Component::Normal(part) => part.to_string_lossy().into_owned(),
                _ => String::new(),
            })
            .collect::<Vec<_>>()
            .join("/");
        if candidate != *prefix {
            return Err(AppError::Message(
                "KIDX5 unsafe or non-normalized scope prefix".into(),
            ));
        }
        normalized.insert(candidate);
    }
    let mut result = Vec::new();
    for prefix in normalized {
        if !result
            .iter()
            .any(|parent: &String| path_matches_prefix(&prefix, parent))
        {
            result.push(prefix);
        }
    }
    Ok(result)
}

fn path_matches_prefix(path: &str, prefix: &str) -> bool {
    path == prefix || path.starts_with(prefix) && path.as_bytes().get(prefix.len()) == Some(&b'/')
}
fn ignored_directory_contents_unobserved(index: &Index) -> bool {
    index
        .omissions
        .iter()
        .any(|o| o.reason == "ignored_excluded" && o.path.ends_with('/'))
}
fn coverage_gaps(index: &Index) -> CoverageGaps {
    let mut omission_reasons = BTreeMap::new();
    for omission in &index.omissions {
        *omission_reasons.entry(omission.reason.clone()).or_insert(0) += 1;
    }
    CoverageGaps {
        partial_files: index
            .files
            .iter()
            .filter(|file| file.parse == ParseStatus::Partial)
            .count(),
        unsupported_files: index
            .files
            .iter()
            .filter(|file| file.parse == ParseStatus::Unsupported)
            .count(),
        invalid_rust_files: index
            .files
            .iter()
            .filter(|file| matches!(file.parse, ParseStatus::Invalid | ParseStatus::InvalidUtf8))
            .count(),
        omitted_paths: index.omissions.len(),
        omission_reasons,
    }
}
fn digest(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    hex::encode(h.finalize())
}
fn snapshot_digest(files: &[FileCard], omissions: &[Omission]) -> String {
    let mut h = Sha256::new();
    for f in files {
        h.update(f.path.as_bytes());
        h.update([0]);
        h.update(f.sha256.as_bytes());
        h.update([0]);
    }
    for o in omissions {
        h.update(o.path.as_bytes());
        h.update([0]);
        h.update(o.reason.as_bytes());
        h.update([0]);
    }
    hex::encode(h.finalize())
}
fn language(path: &str) -> String {
    match Path::new(path)
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "rs" => "rust",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "js" | "mjs" => "javascript",
        "cjs" => "commonjs",
        "jsx" => "jsx",
        "vue" => "vue",
        "py" => "python",
        "go" => "go",
        "java" => "java",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "md" => "markdown",
        _ => "unknown",
    }
    .into()
}

fn parse_javascript(content: &[u8], language: &str) -> (ParseStatus, Vec<Declaration>) {
    let Ok(source) = std::str::from_utf8(content) else {
        return (ParseStatus::InvalidUtf8, Vec::new());
    };
    let source_type = match language {
        "typescript" => SourceType::ts(),
        "tsx" => SourceType::tsx(),
        "jsx" => SourceType::jsx(),
        _ => SourceType::mjs(),
    };
    parse_javascript_source(source, source_type)
}

fn parse_javascript_source(
    source: &str,
    source_type: SourceType,
) -> (ParseStatus, Vec<Declaration>) {
    let allocator = Allocator::default();
    let result = OxcParser::new(&allocator, source, source_type).parse();
    if !result.diagnostics.is_empty() || result.fatal_error {
        return (ParseStatus::Invalid, Vec::new());
    }
    (
        ParseStatus::Parsed,
        exported_declarations(&result.program.body),
    )
}

fn exported_declarations(body: &[Statement<'_>]) -> Vec<Declaration> {
    let mut out = Vec::new();
    for statement in body {
        match statement {
            Statement::ExportDeclaration(export) => {
                add_js_declaration(&export.declaration, &mut out)
            }
            Statement::ExportNamedDeclaration(export) => {
                for specifier in &export.specifiers {
                    if let Some(name) = module_export_name(&specifier.exported) {
                        out.push(Declaration {
                            name,
                            kind: "export".into(),
                        });
                    }
                }
            }
            Statement::ExportFromDeclaration(export) => {
                for specifier in &export.specifiers {
                    if let Some(name) = module_export_name(&specifier.exported) {
                        out.push(Declaration {
                            name,
                            kind: "re-export".into(),
                        });
                    }
                }
            }
            Statement::ExportAllDeclaration(export) => {
                let name = export
                    .exported
                    .as_ref()
                    .and_then(module_export_name)
                    .unwrap_or_else(|| "*".into());
                out.push(Declaration {
                    name,
                    kind: "re-export-all".into(),
                });
            }
            Statement::ExportDefaultDeclaration(export) => {
                out.push(Declaration {
                    name: "default".into(),
                    kind: "default".into(),
                });
                if let ExportDefaultDeclarationKind::FunctionDeclaration(function) =
                    &export.declaration
                    && let Some(id) = &function.id
                {
                    out.push(Declaration {
                        name: id.name.to_string(),
                        kind: "function".into(),
                    });
                }
                if let ExportDefaultDeclarationKind::ClassDeclaration(class) = &export.declaration
                    && let Some(id) = &class.id
                {
                    out.push(Declaration {
                        name: id.name.to_string(),
                        kind: "class".into(),
                    });
                }
            }
            _ => {}
        }
    }
    out.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.name.cmp(&b.name)));
    out.dedup();
    out
}

fn add_js_declaration(declaration: &JsDeclaration<'_>, out: &mut Vec<Declaration>) {
    match declaration {
        JsDeclaration::VariableDeclaration(variable) => {
            for item in &variable.declarations {
                add_binding_pattern(&item.id, out);
            }
        }
        JsDeclaration::FunctionDeclaration(function) => {
            if let Some(id) = &function.id {
                out.push(Declaration {
                    name: id.name.to_string(),
                    kind: "function".into(),
                });
            }
        }
        JsDeclaration::ClassDeclaration(class) => {
            if let Some(id) = &class.id {
                out.push(Declaration {
                    name: id.name.to_string(),
                    kind: "class".into(),
                });
            }
        }
        JsDeclaration::TSTypeAliasDeclaration(alias) => out.push(Declaration {
            name: alias.id.name.to_string(),
            kind: "type".into(),
        }),
        JsDeclaration::TSInterfaceDeclaration(interface) => out.push(Declaration {
            name: interface.id.name.to_string(),
            kind: "interface".into(),
        }),
        JsDeclaration::TSEnumDeclaration(enumeration) => out.push(Declaration {
            name: enumeration.id.name.to_string(),
            kind: "enum".into(),
        }),
        JsDeclaration::TSNamespaceDeclaration(namespace) => out.push(Declaration {
            name: namespace.id.name.to_string(),
            kind: "namespace".into(),
        }),
        _ => {}
    }
}

fn add_binding_pattern(pattern: &BindingPattern<'_>, out: &mut Vec<Declaration>) {
    match pattern {
        BindingPattern::BindingIdentifier(id) => out.push(Declaration {
            name: id.name.to_string(),
            kind: "variable".into(),
        }),
        BindingPattern::ObjectPattern(object) => {
            for property in &object.properties {
                add_binding_pattern(&property.value, out);
            }
            if let Some(rest) = &object.rest {
                add_binding_pattern(&rest.argument, out);
            }
        }
        BindingPattern::ArrayPattern(array) => {
            for element in array.elements.iter().flatten() {
                add_binding_pattern(element, out);
            }
            if let Some(rest) = &array.rest {
                add_binding_pattern(&rest.argument, out);
            }
        }
        BindingPattern::AssignmentPattern(assignment) => add_binding_pattern(&assignment.left, out),
    }
}

fn module_export_name(name: &ModuleExportName<'_>) -> Option<String> {
    match name {
        ModuleExportName::IdentifierName(name) => Some(name.name.to_string()),
        ModuleExportName::IdentifierReference(name) => Some(name.name.to_string()),
        ModuleExportName::StringLiteral(name) => Some(name.value.to_string()),
    }
}

fn parse_vue(content: &[u8]) -> (ParseStatus, Vec<Declaration>) {
    let Ok(source) = std::str::from_utf8(content) else {
        return (ParseStatus::InvalidUtf8, Vec::new());
    };
    let mut cursor = 0;
    let mut scripts = 0;
    let mut declarations = Vec::new();
    while let Some((start, open_end)) = find_vue_script(source, cursor) {
        let open = &source[start..=open_end];
        let Some(close_relative) = source[open_end + 1..].find("</script>") else {
            return (ParseStatus::Invalid, Vec::new());
        };
        let close = open_end + 1 + close_relative;
        let script = &source[open_end + 1..close];
        let source_type = match vue_attribute(open, "lang").as_deref() {
            Some("ts") => SourceType::ts(),
            Some("tsx") => SourceType::tsx(),
            _ => SourceType::mjs(),
        };
        let (status, mut script_declarations) = parse_javascript_source(script, source_type);
        if status != ParseStatus::Parsed {
            return (status, Vec::new());
        }
        scripts += 1;
        declarations.append(&mut script_declarations);
        cursor = close + "</script>".len();
    }
    if scripts == 0 {
        return (ParseStatus::Partial, Vec::new());
    }
    declarations.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.name.cmp(&b.name)));
    declarations.dedup();
    (ParseStatus::Partial, declarations)
}

fn vue_attribute(open: &str, wanted: &str) -> Option<String> {
    let body = open.strip_prefix("<script")?.strip_suffix('>')?;
    let bytes = body.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if cursor >= bytes.len() || bytes[cursor] == b'/' {
            break;
        }
        let key_start = cursor;
        while bytes
            .get(cursor)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':'))
        {
            cursor += 1;
        }
        if key_start == cursor {
            cursor += 1;
            continue;
        }
        let key = &body[key_start..cursor];
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if bytes.get(cursor) != Some(&b'=') {
            continue;
        }
        cursor += 1;
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        let quote = bytes.get(cursor).copied();
        let (value_start, value_end) = if matches!(quote, Some(b'\'' | b'"')) {
            cursor += 1;
            let start = cursor;
            let end = bytes[cursor..]
                .iter()
                .position(|byte| Some(*byte) == quote)
                .map(|offset| start + offset)?;
            cursor = end + 1;
            (start, end)
        } else {
            let start = cursor;
            while bytes
                .get(cursor)
                .is_some_and(|byte| !byte.is_ascii_whitespace() && *byte != b'>')
            {
                cursor += 1;
            }
            (start, cursor)
        };
        if key == wanted {
            return Some(body[value_start..value_end].to_string());
        }
    }
    None
}

fn find_vue_script(source: &str, cursor: usize) -> Option<(usize, usize)> {
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    let mut index = cursor;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"<!--") {
            index = bytes[index + 4..]
                .windows(3)
                .position(|window| window == b"-->")
                .map(|offset| index + 4 + offset + 3)
                .unwrap_or(bytes.len());
            continue;
        }
        if bytes[index..].starts_with(b"{{") {
            index = vue_interpolation_end(bytes, index).unwrap_or(bytes.len());
            continue;
        }
        if bytes[index] != b'<' {
            index += 1;
            continue;
        }
        let end = vue_tag_end(source, index)?;
        let mut name_start = index + 1;
        let closing = bytes.get(name_start) == Some(&b'/');
        if closing {
            name_start += 1;
        }
        while bytes
            .get(name_start)
            .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            name_start += 1;
        }
        let mut name_end = name_start;
        while bytes
            .get(name_end)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b':' | b'_'))
        {
            name_end += 1;
        }
        if name_start == name_end {
            index = end + 1;
            continue;
        }
        let name = &source[name_start..name_end];
        if closing {
            depth = depth.saturating_sub(1);
        } else if depth == 0 && name.eq_ignore_ascii_case("script") {
            return Some((index, end));
        } else if !vue_tag_is_self_closing(source, index, end)
            && ["style", "textarea", "title", "script"]
                .iter()
                .any(|raw| name.eq_ignore_ascii_case(raw))
        {
            // Raw-text contents are not tags. In particular, CSS strings can contain `</x>`
            // and `<script>` without changing template depth or declaring a script block.
            index = vue_raw_text_end(bytes, end + 1, name).unwrap_or(bytes.len());
            continue;
        } else if !vue_tag_is_self_closing(source, index, end) && !vue_tag_is_void(name) {
            depth = depth.saturating_add(1);
        }
        index = end + 1;
    }
    None
}

fn vue_raw_text_end(bytes: &[u8], start: usize, name: &str) -> Option<usize> {
    let name_bytes = name.as_bytes();
    let mut cursor = start;
    while cursor + 2 + name_bytes.len() <= bytes.len() {
        if bytes[cursor..].starts_with(b"</")
            && bytes[cursor + 2..cursor + 2 + name_bytes.len()].eq_ignore_ascii_case(name_bytes)
        {
            let mut end = cursor + 2 + name_bytes.len();
            while bytes.get(end).is_some_and(u8::is_ascii_whitespace) {
                end += 1;
            }
            if bytes.get(end) == Some(&b'>') {
                return Some(end + 1);
            }
        }
        cursor += 1;
    }
    None
}

fn vue_interpolation_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut quote = None;
    let mut line_comment = false;
    let mut block_comment = false;
    let mut candidates = 0usize;
    let mut index = start + 2;
    while index + 1 < bytes.len() {
        if index.saturating_sub(start) > MAX_VUE_INTERPOLATION_BYTES
            || candidates >= MAX_VUE_INTERPOLATION_CANDIDATES
        {
            return None;
        }
        if line_comment {
            if bytes[index] == b'\n' {
                line_comment = false;
            }
        } else if block_comment {
            if bytes[index..].starts_with(b"*/") {
                block_comment = false;
                index += 1;
            }
        } else if let Some(value) = quote {
            if bytes[index] == b'\\' {
                index += 1;
            } else if bytes[index] == value {
                quote = None;
            }
        } else if bytes[index..].starts_with(b"//") {
            line_comment = true;
            index += 1;
        } else if bytes[index..].starts_with(b"/*") {
            block_comment = true;
            index += 1;
        } else if matches!(bytes[index], b'\'' | b'"' | b'`') {
            quote = Some(bytes[index]);
        } else if bytes[index..].starts_with(b"}}") {
            candidates += 1;
            if vue_expression_parses(&bytes[start + 2..index]) {
                return Some(index + 2);
            }
        }
        index += 1;
    }
    None
}

fn vue_expression_parses(bytes: &[u8]) -> bool {
    let Ok(expression) = std::str::from_utf8(bytes) else {
        return false;
    };
    parse_javascript_source(expression, SourceType::mjs()).0 == ParseStatus::Parsed
}

fn vue_tag_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut quote = None;
    for (index, byte) in bytes.iter().enumerate().skip(start + 1) {
        match quote {
            Some(value) if *byte == value => quote = None,
            Some(_) => {}
            None if matches!(*byte, b'\'' | b'"') => quote = Some(*byte),
            None if *byte == b'>' => return Some(index),
            None => {}
        }
    }
    None
}

fn vue_tag_is_self_closing(source: &str, start: usize, end: usize) -> bool {
    source.as_bytes()[start..end]
        .iter()
        .rev()
        .find(|byte| !byte.is_ascii_whitespace())
        == Some(&b'/')
}

fn vue_tag_is_void(name: &str) -> bool {
    [
        "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param",
        "source", "track", "wbr",
    ]
    .iter()
    .any(|void| name.eq_ignore_ascii_case(void))
}
fn public_declarations(file: &syn::File) -> Vec<Declaration> {
    let mut out = Vec::new();
    for item in &file.items {
        add_item(item, &mut out);
    }
    out.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.name.cmp(&b.name)));
    out
}
fn add_item(item: &syn::Item, out: &mut Vec<Declaration>) {
    let (vis, name, kind) = match item {
        syn::Item::Const(x) => (&x.vis, x.ident.to_string(), "const"),
        syn::Item::Enum(x) => (&x.vis, x.ident.to_string(), "enum"),
        syn::Item::Fn(x) => (&x.vis, x.sig.ident.to_string(), "function"),
        syn::Item::Mod(x) => (&x.vis, x.ident.to_string(), "module"),
        syn::Item::Struct(x) => (&x.vis, x.ident.to_string(), "struct"),
        syn::Item::Trait(x) => (&x.vis, x.ident.to_string(), "trait"),
        syn::Item::Type(x) => (&x.vis, x.ident.to_string(), "type"),
        syn::Item::Union(x) => (&x.vis, x.ident.to_string(), "union"),
        syn::Item::Static(x) => (&x.vis, x.ident.to_string(), "static"),
        _ => return,
    };
    if matches!(vis, syn::Visibility::Public(_)) {
        out.push(Declaration {
            name,
            kind: kind.into(),
        });
    }
}

#[cfg(test)]
mod publication_tests {
    use super::*;
    use std::process::Command;

    fn sample_index() -> Index {
        Index {
            schema: "keel.contract-index.v1".into(),
            repo: "fixture".into(),
            snapshot_digest: "fixture".into(),
            files: Vec::new(),
            omissions: Vec::new(),
        }
    }

    fn snapshot_repo() -> tempfile::TempDir {
        let repo = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init", "-q", repo.path().to_str().unwrap()])
            .status()
            .unwrap();
        fs::write(repo.path().join("lib.rs"), "pub fn original() {}\n").unwrap();
        fs::write(repo.path().join("README.md"), "plain source\n").unwrap();
        Command::new("git")
            .args(["-C", repo.path().to_str().unwrap(), "add", "."])
            .status()
            .unwrap();
        repo
    }

    fn search_bounds() -> SnapshotSearchBounds {
        SnapshotSearchBounds {
            max_entries_visited: 10_000,
            max_files_scanned: 10_000,
            max_bytes_scanned: 256 * 1024 * 1024,
            max_results: 100,
            max_terms: MAX_QUERY_TERMS,
            max_term_bytes: (MAX_QUERY_TERM * MAX_QUERY_TERMS) as u64,
        }
    }

    #[test]
    fn snapshot_keeps_index_and_source_on_the_same_bytes_after_worktree_mutation() {
        // Contract: a caller can query declarations and read source from one immutable view.
        // Defect caught: query metadata comes from the old scan while a later source read sees
        // a changed working tree, producing an impossible contract for an agent.
        let repo = snapshot_repo();
        let snapshot = Snapshot::build(repo.path()).unwrap();
        let before = snapshot.source("lib.rs").unwrap().to_vec();
        fs::write(repo.path().join("lib.rs"), "pub fn changed() {}\n").unwrap();

        let query = snapshot.query("original").unwrap();
        assert_eq!(query.files.len(), 1);
        assert_eq!(query.files[0].path, "lib.rs");
        assert_eq!(snapshot.source("lib.rs").unwrap(), before.as_slice());
        assert!(snapshot.query("changed").unwrap().files.is_empty());

        let fresh = Snapshot::build(repo.path()).unwrap();
        assert_ne!(snapshot.digest(), fresh.digest());
        assert_eq!(fresh.source("lib.rs").unwrap(), b"pub fn changed() {}\n");
    }

    #[test]
    fn retained_search_finds_body_terms_in_unsupported_files() {
        // Contract: source search covers retained UTF-8 bytes even when the declaration parser
        // cannot classify the file. Defect caught: an adapter that searches cards only loses
        // README/config evidence that the live source channel can return.
        let repo = snapshot_repo();
        let snapshot = Snapshot::build(repo.path()).unwrap();
        let terms = vec!["plain source".to_owned()];
        let mut bounds = search_bounds();
        bounds.max_results = 10;
        let result = snapshot.search(&terms, &bounds).unwrap();

        assert_eq!(result.paths, vec!["README.md"]);
        assert_eq!(result.coverage_gaps.unsupported_files, 1);
    }

    #[test]
    fn retained_search_is_stable_after_worktree_mutation() {
        // Contract: searching a retained snapshot does not read the changed checkout. Defect
        // caught: search and source reads observing different generations of the worktree.
        let repo = snapshot_repo();
        let snapshot = Snapshot::build(repo.path()).unwrap();
        fs::write(repo.path().join("README.md"), "changed after snapshot\n").unwrap();

        let bounds = search_bounds();
        let old = snapshot
            .search(&["plain source".to_owned()], &bounds)
            .unwrap();
        let new = snapshot
            .search(&["changed after snapshot".to_owned()], &bounds)
            .unwrap();
        assert_eq!(old.paths, vec!["README.md"]);
        assert!(new.paths.is_empty());
    }

    #[test]
    fn retained_search_reports_scoped_coverage_gaps() {
        // Contract: an excluded prefix is visible to the caller as a coverage gap. Defect
        // caught: a scoped snapshot silently presenting incomplete evidence as complete.
        let repo = snapshot_repo();
        fs::create_dir(repo.path().join(".factory")).unwrap();
        fs::write(
            repo.path().join(".factory").join("secret.txt"),
            "scoped secret\n",
        )
        .unwrap();
        Command::new("git")
            .args(["-C", repo.path().to_str().unwrap(), "add", "."])
            .status()
            .unwrap();

        let snapshot = Snapshot::build_scoped(repo.path(), &[".factory"]).unwrap();
        let result = snapshot
            .search(&["scoped secret".to_owned()], &search_bounds())
            .unwrap();
        assert!(result.paths.is_empty());
        assert!(
            result
                .coverage_gaps
                .omission_reasons
                .contains_key("scope_excluded")
        );
        assert!(matches!(
            snapshot.source(".factory/secret.txt"),
            Err(SnapshotSourceError::Omitted { .. })
        ));
    }

    #[test]
    fn retained_search_enforces_term_and_result_bounds() {
        // Contract: caller-controlled search work stays bounded. Defect caught: an adapter can
        // accidentally turn untrusted model terms or result caps into unbounded work.
        let repo = snapshot_repo();
        let snapshot = Snapshot::build(repo.path()).unwrap();
        let too_long = vec!["x".repeat(MAX_QUERY_TERM + 1)];
        let bounds = search_bounds();
        assert_eq!(
            snapshot.search(&too_long, &bounds),
            Err(SnapshotSearchError::TermTooLong)
        );
        let mut too_many_results = bounds;
        too_many_results.max_results = MAX_QUERY_RESULTS + 1;
        assert_eq!(
            snapshot.search(&["plain".to_owned()], &too_many_results),
            Err(SnapshotSearchError::ResultLimitExceeded)
        );
        let terms = (0..=MAX_QUERY_TERMS)
            .map(|number| format!("term-{number}"))
            .collect::<Vec<_>>();
        assert_eq!(
            snapshot.search(&terms, &bounds),
            Err(SnapshotSearchError::TooManyTerms)
        );
    }

    #[test]
    fn retained_search_filters_before_ranking_and_truncates_complete_results() {
        // Contract: the adapter's path policy runs before results are capped. A skipped working
        // note must not consume a slot intended for searchable source evidence.
        let repo = snapshot_repo();
        fs::write(repo.path().join("a.md"), "needle\n").unwrap();
        fs::write(repo.path().join("b.md"), "needle\n").unwrap();
        Command::new("git")
            .args(["-C", repo.path().to_str().unwrap(), "add", "."])
            .status()
            .unwrap();
        let snapshot = Snapshot::build(repo.path()).unwrap();
        let mut bounds = search_bounds();
        bounds.max_results = 1;

        let broad = snapshot.search(&["needle".to_owned()], &bounds).unwrap();
        assert_eq!(broad.paths, vec!["a.md"]);
        assert!(broad.truncated);

        let filtered = snapshot
            .search_filtered(&["needle".to_owned()], &bounds, |path| path == "b.md")
            .unwrap();
        assert_eq!(filtered.paths, vec!["b.md"]);
        assert!(!filtered.truncated);
    }

    #[test]
    fn retained_search_does_not_treat_a_filename_as_source_text() {
        // Contract: the existing Runtime source channel matches UTF-8 content, not path names.
        // Defect caught: filename-only evidence changes the candidate set during adoption.
        let repo = snapshot_repo();
        fs::write(repo.path().join("auth_note.md"), "ordinary content\n").unwrap();
        Command::new("git")
            .args(["-C", repo.path().to_str().unwrap(), "add", "."])
            .status()
            .unwrap();
        let snapshot = Snapshot::build(repo.path()).unwrap();
        assert!(
            snapshot
                .search(&["auth".to_owned()], &search_bounds())
                .unwrap()
                .paths
                .is_empty()
        );
    }

    #[test]
    fn vue_partial_parse_remains_visible_when_a_symbol_query_misses() {
        // Contract: a missed symbol cannot imply complete coverage when a Vue file was only
        // partially parsed. Defect caught: script-setup bindings can be absent from the index.
        let repo = snapshot_repo();
        fs::write(
            repo.path().join("Component.vue"),
            "<script setup>const hiddenFeature = 1;</script>\n",
        )
        .unwrap();
        Command::new("git")
            .args(["-C", repo.path().to_str().unwrap(), "add", "Component.vue"])
            .status()
            .unwrap();
        let query = Snapshot::build(repo.path())
            .unwrap()
            .query("hiddenFeature")
            .unwrap();
        assert!(query.files.is_empty());
        assert_eq!(query.coverage_gaps.partial_files, 1);
    }

    #[test]
    fn vue_style_text_cannot_fabricate_a_script_export() {
        // Contract: CSS strings are not Vue tags. Defect caught: a closing tag inside CSS
        // reduced template depth and made a following script-looking string public.
        let source = r#"<style>.x::after { content: "</x><script>export const forged=1;</script>"; }</style>
<script>export const real=1;</script>"#;
        let (status, declarations) = parse_vue(source.as_bytes());
        assert_eq!(status, ParseStatus::Partial);
        assert!(declarations.iter().any(|item| item.name == "real"));
        assert!(!declarations.iter().any(|item| item.name == "forged"));
    }

    #[test]
    fn snapshot_reports_omissions_and_retains_unsupported_source_bytes() {
        // Contract: omitted paths are explicit coverage gaps, while unsupported files remain
        // readable bytes. Defect caught: returning None for both would falsely imply complete
        // source coverage and make an agent trust an incomplete index.
        let repo = snapshot_repo();
        fs::write(repo.path().join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(repo.path().join("ignored.txt"), "hidden\n").unwrap();
        let snapshot = Snapshot::build(repo.path()).unwrap();

        assert_eq!(snapshot.source("README.md").unwrap(), b"plain source\n");
        assert_eq!(
            snapshot.source("ignored.txt"),
            Err(SnapshotSourceError::Omitted {
                reason: "ignored_excluded".into()
            })
        );
        let query = snapshot.query("ignored.txt").unwrap();
        assert_eq!(query.files.len(), 0);
        assert_eq!(query.omissions.len(), 1);
        assert_eq!(query.coverage_gaps.omitted_paths, 2);
        assert_eq!(
            snapshot.source("missing.rs"),
            Err(SnapshotSourceError::NotIndexed)
        );
    }

    #[test]
    fn snapshot_marks_files_under_an_omitted_directory_as_omitted() {
        // Contract: an ignored-directory card covers descendants as a coverage gap. Defect
        // caught: treating the child as NotIndexed would hide the reason the source is absent.
        let repo = snapshot_repo();
        fs::write(repo.path().join(".gitignore"), "ignored/\n").unwrap();
        fs::create_dir(repo.path().join("ignored")).unwrap();
        fs::write(
            repo.path().join("ignored").join("hidden.rs"),
            "pub fn hidden() {}\n",
        )
        .unwrap();
        let snapshot = Snapshot::build(repo.path()).unwrap();

        assert_eq!(
            snapshot.source("ignored/hidden.rs"),
            Err(SnapshotSourceError::Omitted {
                reason: "ignored_excluded".into()
            })
        );
    }

    #[test]
    fn scoped_snapshot_skips_unreadable_files_and_binds_exclusion_to_digest() {
        // Contract: an excluded directory is not read, is disclosed as one coverage gap, and
        // changes the immutable snapshot identity. Defect caught: reading before applying scope
        // would reject the missing tracked file, while omitting scope from the digest would let
        // a caller reuse evidence with different coverage.
        let repo = snapshot_repo();
        let generated = repo.path().join("generated");
        fs::create_dir(&generated).unwrap();
        let missing = generated.join("receipt.bin");
        fs::write(&missing, b"generated receipt\n").unwrap();
        Command::new("git")
            .args(["-C", repo.path().to_str().unwrap(), "add", "."])
            .status()
            .unwrap();
        fs::remove_file(&missing).unwrap();

        let scoped =
            Snapshot::build_scoped(repo.path(), &["generated", "generated/child"]).unwrap();
        assert_eq!(
            scoped.source("generated/receipt.bin"),
            Err(SnapshotSourceError::Omitted {
                reason: "scope_excluded".into()
            })
        );
        assert_eq!(
            scoped
                .query("generated")
                .unwrap()
                .omissions
                .iter()
                .filter(|omission| omission.reason == "scope_excluded")
                .count(),
            1
        );

        let unscoped = Snapshot::build(repo.path());
        assert!(unscoped.is_err());
        let normal = snapshot_repo();
        let default_snapshot = Snapshot::build(normal.path()).unwrap();
        let scoped_snapshot = Snapshot::build_scoped(normal.path(), &["lib.rs"]).unwrap();
        assert_ne!(default_snapshot.digest(), scoped_snapshot.digest());
        assert_eq!(
            scoped_snapshot.source("lib.rs"),
            Err(SnapshotSourceError::Omitted {
                reason: "scope_excluded".into()
            })
        );
    }

    #[test]
    fn scoped_snapshot_rejects_unsafe_prefixes_and_deduplicates_nested_prefixes() {
        // Contract: scope inputs are stable repository-relative paths, and redundant nested
        // prefixes do not create duplicate omission records. Defect caught: traversal-shaped or
        // platform-dependent prefixes could widen or change the excluded scope.
        let repo = snapshot_repo();
        let error = Snapshot::build_scoped(repo.path(), &["../generated"])
            .err()
            .expect("unsafe scope prefix must fail");
        assert_eq!(error.code, "GHKEEL005_UNSAFE_PATH");
        let scoped =
            Snapshot::build_scoped(repo.path(), &["lib.rs", "lib.rs", "lib.rs/child"]).unwrap();
        assert_eq!(
            scoped
                .index()
                .omissions
                .iter()
                .filter(|omission| omission.reason == "scope_excluded")
                .count(),
            1
        );

        let many = (0..=MAX_SCOPE_PREFIXES)
            .map(|index| format!("generated-{index}"))
            .collect::<Vec<_>>();
        let many_refs = many.iter().map(String::as_str).collect::<Vec<_>>();
        assert_eq!(
            Snapshot::build_scoped(repo.path(), &many_refs)
                .err()
                .expect("prefix count must be bounded")
                .code,
            "GHKEEL004_LIMIT"
        );
        let long = "a".repeat(MAX_SCOPE_PREFIX_BYTES + 1);
        assert_eq!(
            Snapshot::build_scoped(repo.path(), &[&long])
                .err()
                .expect("prefix bytes must be bounded")
                .code,
            "GHKEEL004_LIMIT"
        );
    }

    #[test]
    fn duplicate_git_index_paths_fail_closed_before_snapshot_construction() {
        // Contract: one path has one card and one byte source. Defect caught: conflict stages
        // from `git ls-files -s` could create duplicate cards while the source map overwrote one.
        let tracked = vec![
            ("100644".into(), "src/lib.rs".into()),
            ("100644".into(), "src/lib.rs".into()),
        ];
        let error = ensure_unique_tracked_paths(&tracked)
            .unwrap_err()
            .to_string();
        assert!(error.starts_with("KIDX3"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn acquisition_swap_cannot_open_a_repository_directory_as_output() {
        use std::os::unix::fs::symlink;

        // Defect: canonicalize outside, then open through a changed ancestor into the repo.
        // This injects the swap at that exact boundary; a path-only post-check can be raced.
        let sandbox = tempfile::tempdir().unwrap();
        let repo = sandbox.path().join("repo");
        let inside = repo.join("output");
        let outside_root = sandbox.path().join("switch");
        let outside = outside_root.join("output");
        let moved = sandbox.path().join("moved-switch");
        fs::create_dir_all(&inside).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let sentinel = inside.join("index.json");
        fs::write(&sentinel, b"tracked sentinel").unwrap();

        let result = OutputTarget::open_with(&repo, &outside.join("index.json"), |canonical| {
            fs::rename(&outside_root, &moved)?;
            symlink(&repo, &outside_root)?;
            open_output_parent(canonical)
        });
        let error = result.err().unwrap().to_string();
        assert!(
            error.contains("KIDX5 opened output parent is inside repository"),
            "{error}"
        );
        assert_eq!(fs::read(sentinel).unwrap(), b"tracked sentinel");
    }

    #[cfg(unix)]
    #[test]
    fn output_parent_swap_refuses_before_touching_a_repository_file() {
        use std::os::unix::fs::symlink;

        // Contract: a path alias replaced after validation cannot turn index publication into
        // a repository write. The old path-based tempfile::persist would overwrite sentinel.
        let sandbox = tempfile::tempdir().unwrap();
        let repo = sandbox.path().join("repo");
        let output = sandbox.path().join("output");
        let moved = sandbox.path().join("moved-output");
        fs::create_dir(&repo).unwrap();
        fs::create_dir(&output).unwrap();
        let sentinel = repo.join("index.json");
        fs::write(&sentinel, b"tracked sentinel").unwrap();
        let target = OutputTarget::open(&repo, &output.join("index.json")).unwrap();

        fs::rename(&output, &moved).unwrap();
        symlink(&repo, &output).unwrap();
        let error = write_json(&target, &sample_index())
            .unwrap_err()
            .to_string();
        assert!(error.contains("KIDX5"), "{error}");
        assert_eq!(fs::read(sentinel).unwrap(), b"tracked sentinel");
        assert!(!moved.join("index.json").exists());
    }

    #[cfg(windows)]
    #[test]
    fn acquisition_of_a_repository_handle_refuses_before_alias_checks() {
        // Inject the exact handle acquisition outcome of an ancestor swap. The diagnostic
        // distinguishes the retained-handle guard from the older mutable-alias check.
        let sandbox = tempfile::tempdir().unwrap();
        let repo = sandbox.path().join("repo");
        let inside = repo.join("output");
        let outside = sandbox.path().join("outside");
        fs::create_dir_all(&inside).unwrap();
        fs::create_dir(&outside).unwrap();
        let result = OutputTarget::open_with(&repo, &outside.join("index.json"), |_| {
            publish_windows::open_parent(&inside)
        });
        let error = result.err().unwrap().to_string();
        assert!(
            error.contains("KIDX5 opened output parent is inside repository"),
            "{error}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn retained_parent_refuses_or_blocks_a_swap_without_touching_the_repository() {
        // Contract: Windows denies a direct rename while the parent handle is retained.
        // If the filesystem permits renaming an ancestor, the alias check must refuse.
        let sandbox = tempfile::tempdir().unwrap();
        let repo = sandbox.path().join("repo");
        let root = sandbox.path().join("output-root");
        let output = root.join("output");
        let moved = sandbox.path().join("moved-output-root");
        fs::create_dir(&repo).unwrap();
        fs::create_dir_all(&output).unwrap();
        let sentinel = repo.join("index.json");
        fs::write(&sentinel, b"tracked sentinel").unwrap();
        let target = OutputTarget::open(&repo, &output.join("index.json")).unwrap();

        if fs::rename(&root, &moved).is_ok() {
            let error = write_json(&target, &sample_index())
                .unwrap_err()
                .to_string();
            assert!(error.contains("KIDX5"), "{error}");
        } else {
            assert!(write_json(&target, &sample_index()).is_ok());
        }
        assert_eq!(fs::read(sentinel).unwrap(), b"tracked sentinel");
    }
}
