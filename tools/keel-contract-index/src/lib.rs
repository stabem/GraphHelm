use clap::{Parser, Subcommand};
use same_file::Handle;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
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
const MAX_QUERY_RESULTS: usize = 100;
const MAX_INDEX_BYTES: u64 = 16 * 1024 * 1024;
const MAX_INDEX_JSON_DEPTH: usize = 64;
const MAX_INDEX_JSON_STRING_BYTES: usize = 4096;
const MAX_INDEX_JSON_ARRAY_ITEMS: usize = MAX_FILES;
const MAX_INDEX_JSON_OBJECT_KEYS: usize = 16;
const MAX_INDEX_JSON_VALUES: usize = MAX_INDEX_BYTES as usize / 4;
const MAX_CARD_BYTES: usize = 4096;
const MAX_OMISSIONS: usize = 10_000;
const MAX_GIT_OUTPUT_BYTES: usize = 64 * 1024 * 1024;

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
struct Index {
    schema: String,
    repo: String,
    snapshot_digest: String,
    files: Vec<FileCard>,
    omissions: Vec<Omission>,
}
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
struct FileCard {
    path: String,
    sha256: String,
    bytes: u64,
    language: String,
    parse: ParseStatus,
    declarations: Vec<Declaration>,
}
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ParseStatus {
    Parsed,
    Unsupported,
    Invalid,
    InvalidUtf8,
}
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
struct Declaration {
    name: String,
    kind: String,
}
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
struct Omission {
    path: String,
    reason: String,
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
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CoverageGaps {
    unsupported_files: usize,
    invalid_rust_files: usize,
    omitted_paths: usize,
    omission_reasons: BTreeMap<String, usize>,
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
    let root = fs::canonicalize(repo)?;
    if !root.is_dir() {
        return Err(AppError::Message("KIDX3 repo is not a directory".into()));
    }
    let tracked = git_files(&root)?;
    if tracked.len() > MAX_FILES {
        return Err(AppError::Message("KIDX4 file limit exceeded".into()));
    }
    let excluded = excluded.and_then(|p| fs::canonicalize(p).ok());
    let mut files = Vec::with_capacity(tracked.len());
    let mut omissions = untracked_omissions(&root, excluded.as_deref())?;
    let mut total: u64 = 0;
    for (mode, rel) in tracked {
        if mode == "120000" {
            return Err(AppError::Message(format!("KIDX5 symlink rejected: {rel}")));
        }
        if !safe_relative(&rel) {
            return Err(AppError::Message(format!("KIDX5 unsafe path: {rel}")));
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
        let lang = language(&rel);
        let (parse, declarations) = if lang == "rust" {
            match std::str::from_utf8(&content) {
                Ok(source) => match syn::parse_file(source) {
                    Ok(file) => (ParseStatus::Parsed, public_declarations(&file)),
                    Err(_) => (ParseStatus::Invalid, Vec::new()),
                },
                Err(_) => (ParseStatus::InvalidUtf8, Vec::new()),
            }
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
    Ok(Index {
        schema: "keel.contract-index.v1".into(),
        repo: "working-tree".into(),
        snapshot_digest,
        files,
        omissions,
    })
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

fn untracked_omissions(root: &Path, excluded: Option<&Path>) -> Result<Vec<Omission>, AppError> {
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
            if safe_relative(path) && excluded != Some(&root.join(path)) {
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
        "ts" => "typescript",
        "tsx" => "tsx",
        "js" => "javascript",
        "jsx" => "jsx",
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

    fn sample_index() -> Index {
        Index {
            schema: "keel.contract-index.v1".into(),
            repo: "fixture".into(),
            snapshot_digest: "fixture".into(),
            files: Vec::new(),
            omissions: Vec::new(),
        }
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
