use clap::{Parser, Subcommand};
use same_file::Handle;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs, io,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
};

const MAX_FILES: usize = 10_000;
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
const MAX_QUERY_TERM: usize = 256;
const MAX_QUERY_RESULTS: usize = 100;
const MAX_INDEX_BYTES: u64 = 16 * 1024 * 1024;
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

fn main() {
    if let Err(e) = run() {
        let out =
            serde_json::json!({"ok": false, "error": {"code":"KIDX1", "message": e.to_string()}});
        println!(
            "{}",
            serde_json::to_string(&out).unwrap_or_else(|_| "{\"ok\":false}".into())
        );
        std::process::exit(1);
    }
}

fn run() -> Result<(), AppError> {
    match Cli::parse().command {
        CommandKind::Scan { repo, out } => {
            reject_inside_repo(&repo, &out)?;
            let index = build_index(&repo, None)?;
            write_json(&out, &index)?;
            println!(
                "{}",
                serde_json::to_string(
                    &serde_json::json!({"ok":true,"snapshot_digest":index.snapshot_digest,"files":index.files.len(),"out":out})
                )?
            );
        }
        CommandKind::Verify { repo, index } => {
            let expected = read_index(&index)?;
            let actual = fresh_index(&repo, &index, &expected)?;
            if expected.snapshot_digest != actual.snapshot_digest {
                return Err(AppError::Message(format!(
                    "KIDX2 stale index: expected {}, current {}",
                    expected.snapshot_digest, actual.snapshot_digest
                )));
            }
            if expected.schema != actual.schema
                || expected.repo != actual.repo
                || expected.files != actual.files
                || expected.omissions != actual.omissions
            {
                return Err(AppError::Message(
                    "KIDX2 stale index: cards or omissions differ".into(),
                ));
            }
            println!(
                "{}",
                serde_json::to_string(
                    &serde_json::json!({"ok":true,"snapshot_digest":actual.snapshot_digest})
                )?
            );
        }
        CommandKind::Query { repo, index, term } => {
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
            println!(
                "{}",
                serde_json::to_string(
                    &serde_json::json!({"ok":true,"snapshot_digest":idx.snapshot_digest,"term":term,"truncated":truncated,"files":files,"ignoredDirectoryContentsUnobserved":ignored_directory_contents_unobserved(&idx),"coverageGaps":coverage_gaps(&idx)})
                )?
            );
        }
        CommandKind::ProposeCard { repo, index, term } => {
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
            let exported_symbols_truncated = hit.declarations.len() > 8;
            let card = CandidateCard {
                scope_paths: vec![hit.path.clone()],
                exported_symbols: hit.declarations.iter().take(8).cloned().collect(),
                exported_symbols_truncated,
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
            println!("{}", serde_json::to_string(&card)?);
        }
    }
    Ok(())
}

fn write_json(path: &Path, value: &Index) -> Result<(), AppError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    let encoded = serde_json::to_vec_pretty(value)?;
    if encoded.len() as u64 > MAX_INDEX_BYTES {
        return Err(AppError::Message("KIDX4 index size limit exceeded".into()));
    }
    temp.write_all(&encoded)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| AppError::Io(e.error))?;
    Ok(())
}
fn read_index(path: &Path) -> Result<Index, AppError> {
    let bytes = read_bounded(path, MAX_INDEX_BYTES)?;
    Ok(serde_json::from_slice(&bytes)?)
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
fn reject_inside_repo(repo: &Path, out: &Path) -> Result<(), AppError> {
    let root = fs::canonicalize(repo)?;
    let parent = out.parent().unwrap_or_else(|| Path::new("."));
    let candidate = fs::canonicalize(parent)
        .map_err(|_| AppError::Message("KIDX5 output parent must exist".into()))?
        .join(out.file_name().unwrap_or_default());
    if candidate.starts_with(&root) {
        return Err(AppError::Message(
            "KIDX5 output must be outside repository".into(),
        ));
    }
    Ok(())
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
