//! Keel Law 3 made executable (#1333): a new test earns its place by failing on the parent and
//! passing on the head.
//!
//! `graphhelm keel check --prove-new-tests` hands this module the diff it already classified. For
//! every new Rust `#[test]` the counter charges, [`prove_new_tests`] checks out the base and the
//! head into two temporary detached worktrees, grafts the head's test code onto the base, and runs
//! the one test by name on each side with `cargo test`. The verdict is [`verdict`], a pure function
//! of the two outcomes, so it can be read and tested without a toolchain.
//!
//! The user's checkout is never written: both trees are `git worktree add --detach` under a fresh
//! scratch directory, and `Scratch` removes exactly those two worktrees and that directory when it
//! drops, on every path out, by exact path (never `git worktree prune`). A run killed before the
//! drop (Ctrl-C) leaves its records; the next run clears the ones whose directory is gone. Every cargo run is bounded by a timeout, filtered to one test name,
//! and shares one `CARGO_TARGET_DIR` so each crate builds once per side.
//!
//! Only Rust is proven today. A test in another language is reported as `unproven` with the
//! reason, never silently skipped; `Language` is where a runner for it would plug in.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::keel::{Finding, changed_paths, is_test_path, parse};

/// One new test the diff adds, where the head declares it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewTest {
    /// The function name, the filter `cargo test` is given.
    pub name: String,
    /// Repository-relative path of the file that declares it.
    pub path: String,
    /// 1-based line of the `#[test]` attribute among the file's added lines.
    pub line: u32,
    /// True when the test sits in a production source file (an inline `mod tests`), so the parent
    /// run grafts the test alone rather than the whole head file.
    pub inline: bool,
}

/// What one run of one test on one side produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcome {
    Passed,
    Failed,
    /// The crate did not build. On the parent this usually means the thing under test is new.
    DidNotCompile,
    /// The build ran and no test by that name ran.
    NotFound,
    Ignored,
    TimedOut,
    /// The run could not be attempted (no crate found, an unsupported target, a spawn error).
    NotRun,
}

/// One side's run: the outcome and the line that shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunResult {
    pub outcome: RunOutcome,
    /// The matched `test ... ok|FAILED` line, the first compiler error, or why nothing ran.
    pub detail: String,
}

/// What the two runs say about the test.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestVerdict {
    /// Red on the parent, green on the head: the test detects the defect the change removes.
    Earned,
    /// The parent does not compile with the test grafted in (the subject is new) and the head is
    /// green. Not red-on-parent evidence; the reviewer reads the compiler line.
    NewSubject,
    /// Green on the parent: the test does not detect anything this change fixed.
    GreenOnParent,
    /// The head itself does not pass the test.
    RedOnHead,
    /// Nothing decisive ran (timeout, not found, ignored, unsupported).
    Unproven,
}

impl TestVerdict {
    /// The rule id a verdict raises, or `None` when it raises nothing.
    #[must_use]
    pub const fn rule(self) -> Option<&'static str> {
        match self {
            Self::Earned | Self::NewSubject => None,
            Self::GreenOnParent => Some("keel.test.green_on_parent"),
            Self::RedOnHead => Some("keel.test.red_on_head"),
            Self::Unproven => Some("keel.test.unproven"),
        }
    }
}

/// The two runs of one test and what they mean.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestProof {
    pub name: String,
    pub path: String,
    pub line: u32,
    pub parent: RunResult,
    pub head: RunResult,
    pub verdict: TestVerdict,
}

/// Everything `--prove-new-tests` reports.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofReport {
    pub base: String,
    pub head: String,
    pub proofs: Vec<TestProof>,
    /// One signal per test whose verdict raises a rule. Never blocking.
    pub findings: Vec<Finding>,
}

/// The verdict of one test from its two runs. Pure; the whole meaning of the feature.
#[must_use]
pub const fn verdict(parent: RunOutcome, head: RunOutcome) -> TestVerdict {
    match head {
        RunOutcome::Passed => match parent {
            RunOutcome::Failed => TestVerdict::Earned,
            RunOutcome::DidNotCompile => TestVerdict::NewSubject,
            RunOutcome::Passed => TestVerdict::GreenOnParent,
            _ => TestVerdict::Unproven,
        },
        RunOutcome::Failed | RunOutcome::DidNotCompile => TestVerdict::RedOnHead,
        _ => TestVerdict::Unproven,
    }
}

/// The Rust tests a diff adds: each added `#[test]` (or `#[tokio::test]`) line and the name of the
/// first `fn` among the added lines after it. The same lines `classify_write` charges as `newTest`.
#[must_use]
pub fn new_rust_tests(diff: &str) -> Vec<NewTest> {
    let mut out = Vec::new();
    for file in parse(diff) {
        if !file.path.ends_with(".rs") {
            continue;
        }
        let inline = !is_test_path(&file.path);
        for (index, line) in file.added.iter().enumerate() {
            let trimmed = line.trim_start();
            if !(trimmed.starts_with("#[test]") || trimmed.starts_with("#[tokio::test")) {
                continue;
            }
            let Some(name) = file.added[index + 1..]
                .iter()
                .find_map(|next| fn_name(next))
            else {
                continue;
            };
            out.push(NewTest {
                name,
                path: file.path.clone(),
                line: u32::try_from(index + 1).unwrap_or(u32::MAX),
                inline,
            });
        }
    }
    out
}

/// The tests a diff adds in a language this module cannot run yet, so the report names them.
fn unsupported_new_tests(diff: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for file in parse(diff) {
        let Some(language) = Language::of(&file.path) else {
            continue;
        };
        for line in &file.added {
            if let Some(name) = language.test_name(line) {
                out.push((name, file.path.clone()));
            }
        }
    }
    out
}

/// A test language this module recognises but has no runner for yet, so its tests are reported
/// as unproven instead of vanishing. A runner for one replaces its entry here.
#[derive(Clone, Copy)]
enum Language {
    TypeScript,
    Python,
}

impl Language {
    fn of(path: &str) -> Option<Self> {
        match path.rsplit('.').next()? {
            "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs" => Some(Self::TypeScript),
            "py" => Some(Self::Python),
            _ => None,
        }
    }

    fn test_name(self, line: &str) -> Option<String> {
        let trimmed = line.trim_start();
        match self {
            Self::TypeScript => {
                (trimmed.starts_with("it(") || trimmed.starts_with("test(")).then(|| {
                    trimmed
                        .split(['"', '\'', '`'])
                        .nth(1)
                        .unwrap_or("")
                        .to_owned()
                })
            }
            Self::Python => trimmed
                .strip_prefix("def test_")
                .map(|rest| format!("test_{}", ident(rest))),
        }
    }
}

fn fn_name(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("#[") || trimmed.starts_with("//") || trimmed.is_empty() {
        return None;
    }
    let (before, after) = trimmed.split_once("fn ")?;
    if !before
        .split_whitespace()
        .all(|word| matches!(word, "pub" | "async" | "unsafe" | "const" | "pub(crate)"))
    {
        return None;
    }
    let name = ident(after);
    (!name.is_empty()).then_some(name)
}

fn ident(text: &str) -> String {
    text.chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}

/// How the prover runs: where the repository is, what it compares, and its bounds.
#[derive(Clone, Debug)]
pub struct ProveOptions {
    pub repo: PathBuf,
    pub base: String,
    pub head: String,
    /// Shared by every run, with a `parent` and a `head` subdirectory, so each crate builds once
    /// per side and a later check reuses both builds.
    pub target_dir: PathBuf,
    /// A fresh directory is made under it and removed afterwards.
    pub scratch_root: PathBuf,
    /// Bound on one `cargo test` run, build included.
    pub timeout: Duration,
}

/// The two worktrees and their directory. Dropping it removes exactly what it created.
struct Scratch {
    repo: PathBuf,
    dir: PathBuf,
    worktrees: Vec<PathBuf>,
}

impl Drop for Scratch {
    fn drop(&mut self) {
        for worktree in &self.worktrees {
            remove_worktree_record(&self.repo, worktree);
        }
        let _ = std::fs::remove_dir_all(&self.dir);
        // A remove that failed while the directory still existed (a locked file on Windows) leaves
        // the record behind; with the directory now gone, the same exact-path remove clears it.
        let listed = listed_worktrees(&self.repo);
        for worktree in &self.worktrees {
            if listed.iter().any(|entry| same_path(entry, worktree)) {
                remove_worktree_record(&self.repo, worktree);
            }
        }
    }
}

/// `git worktree remove --force <path>`, which also clears the record of a directory that is gone.
/// Always one exact path: never `git worktree prune`, which would touch records this run did not make.
fn remove_worktree_record(repo: &Path, worktree: &Path) {
    let _ = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["worktree", "remove", "--force"])
        .arg(worktree)
        .output();
}

fn listed_worktrees(repo: &Path) -> Vec<(PathBuf, bool)> {
    let Ok(output) = git(repo, &["worktree", "list", "--porcelain"]) else {
        return Vec::new();
    };
    let mut entries: Vec<(PathBuf, bool)> = Vec::new();
    for line in output.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            entries.push((PathBuf::from(path), false));
        } else if line.starts_with("prunable")
            && let Some(last) = entries.last_mut()
        {
            last.1 = true;
        }
    }
    entries
}

fn path_key(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let text = text.trim_end_matches('/');
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text.to_owned()
    }
}

fn same_path(entry: &(PathBuf, bool), path: &Path) -> bool {
    path_key(&entry.0) == path_key(path)
}

/// Clears the records an interrupted earlier run left: a `parent` or `head` worktree directly under
/// a `keel-prove-*` directory of this scratch root whose directory is gone (git marks it prunable).
/// Each is removed by its exact path; a record whose directory still exists may belong to a run in
/// progress and is left alone.
fn clear_stale_records(repo: &Path, scratch_root: &Path) {
    let root = path_key(scratch_root);
    for (path, prunable) in listed_worktrees(repo) {
        if !prunable {
            continue;
        }
        let side = path.file_name().and_then(|name| name.to_str());
        let run = path
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str());
        let under_root = path
            .parent()
            .and_then(Path::parent)
            .is_some_and(|parent| path_key(parent) == root);
        if matches!(side, Some("parent" | "head"))
            && run.is_some_and(|name| name.starts_with("keel-prove-"))
            && under_root
        {
            remove_worktree_record(repo, &path);
        }
    }
}

fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|error| format!("git did not start: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// Proves every new test in `diff`. `Err` only when the trees cannot be set up; a test that cannot
/// run is an `unproven` entry, not an error.
///
/// # Errors
/// When a revision does not resolve, the scratch directory cannot be made, or a worktree cannot be
/// added.
pub fn prove_new_tests(diff: &str, options: &ProveOptions) -> Result<ProofReport, String> {
    let resolve = |rev: &str| {
        git(
            &options.repo,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{rev}^{{commit}}"),
            ],
        )
    };
    let base = resolve(&options.base)?;
    let head = resolve(&options.head)?;
    let tests = new_rust_tests(diff);
    let mut proofs = Vec::new();
    if !tests.is_empty() {
        proofs.extend(prove_rust(diff, &tests, &base, &head, options)?);
    }
    for (name, path) in unsupported_new_tests(diff) {
        let not_run = RunResult {
            outcome: RunOutcome::NotRun,
            detail: "no runner for this language yet; only Rust #[test] is proven".into(),
        };
        proofs.push(TestProof {
            name,
            path,
            line: 0,
            parent: not_run.clone(),
            head: not_run,
            verdict: TestVerdict::Unproven,
        });
    }
    let findings = proofs
        .iter()
        .filter_map(|proof| {
            proof.verdict.rule().map(|rule| Finding {
                rule: rule.into(),
                path: Some(proof.path.clone()),
                detail: format!(
                    "{}: parent {:?} ({}), head {:?} ({})",
                    proof.name,
                    proof.parent.outcome,
                    proof.parent.detail,
                    proof.head.outcome,
                    proof.head.detail
                ),
                blocking: false,
            })
        })
        .collect();
    Ok(ProofReport {
        base,
        head,
        proofs,
        findings,
    })
}

fn prove_rust(
    diff: &str,
    tests: &[NewTest],
    base: &str,
    head: &str,
    options: &ProveOptions,
) -> Result<Vec<TestProof>, String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    clear_stale_records(&options.repo, &options.scratch_root);
    let dir = options
        .scratch_root
        .join(format!("keel-prove-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("scratch {} not created: {error}", dir.display()))?;
    let mut scratch = Scratch {
        repo: options.repo.clone(),
        dir: dir.clone(),
        worktrees: Vec::new(),
    };
    let parent_tree = dir.join("parent");
    let head_tree = dir.join("head");
    for (tree, rev) in [(&parent_tree, base), (&head_tree, head)] {
        let tree_arg = tree.to_string_lossy().into_owned();
        // Recorded before the add: an add that fails halfway can still leave a record, and the
        // drop removes it by this exact path.
        scratch.worktrees.push(tree.clone());
        git(
            &options.repo,
            &["worktree", "add", "--detach", "--quiet", &tree_arg, rev],
        )?;
    }
    let grafted = graft_onto_parent(diff, tests, &parent_tree, &head_tree);
    // Every parent run first, then every head run, so each side's build is reused by the next test.
    let parent_runs: Vec<RunResult> = tests
        .iter()
        .map(|test| match grafted.get(&test.path) {
            Some(Err(reason)) => RunResult {
                outcome: RunOutcome::DidNotCompile,
                detail: reason.clone(),
            },
            _ => run_test(&parent_tree, "parent", test, options),
        })
        .collect();
    let proofs = tests
        .iter()
        .zip(parent_runs)
        .map(|(test, parent)| {
            let head_run = run_test(&head_tree, "head", test, options);
            TestProof {
                name: test.name.clone(),
                path: test.path.clone(),
                line: test.line,
                verdict: verdict(parent.outcome, head_run.outcome),
                parent,
                head: head_run,
            }
        })
        .collect();
    drop(scratch);
    Ok(proofs)
}

/// Puts the head's test code into the parent tree: every changed test-path file whole, and each
/// inline test alone, appended to its file inside a `#[cfg(test)]` module. Returns, per inline
/// file, `Err` when the test cannot be placed on the parent (its file is new there).
fn graft_onto_parent(
    diff: &str,
    tests: &[NewTest],
    parent: &Path,
    head: &Path,
) -> BTreeMap<String, Result<(), String>> {
    for changed in changed_paths(diff) {
        if changed.is_deleted || !changed.plain || !is_test_path(&changed.path) {
            continue;
        }
        let to = parent.join(&changed.path);
        if let Some(dir) = to.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::copy(head.join(&changed.path), to);
    }
    let mut by_file: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for test in tests.iter().filter(|test| test.inline) {
        by_file
            .entry(test.path.clone())
            .or_default()
            .push(test.name.as_str());
    }
    by_file
        .into_iter()
        .map(|(path, names)| {
            let target = parent.join(&path);
            let placed = match (
                std::fs::read_to_string(head.join(&path)),
                std::fs::read_to_string(&target),
            ) {
                (Ok(head_text), Ok(mut parent_text)) => {
                    parent_text.push_str(&graft_module(&head_text, &names));
                    std::fs::write(&target, parent_text)
                        .map_err(|error| format!("graft not written: {error}"))
                }
                (_, Err(_)) => Err(format!(
                    "{path} does not exist on the parent: the subject is new"
                )),
                (Err(error), _) => Err(format!("{path} unreadable on the head: {error}")),
            };
            (path, placed)
        })
        .collect()
}

/// A `#[cfg(test)]` module holding the named test functions from `source`, with the `use` lines of
/// the module that encloses each one on the head. Braces are matched by count, not parsed.
fn graft_module(source: &str, names: &[&str]) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let mut uses: Vec<String> = Vec::new();
    let mut bodies: Vec<String> = Vec::new();
    for name in names {
        let Some(fn_line) = lines
            .iter()
            .position(|line| fn_name(line).as_deref() == Some(name))
        else {
            continue;
        };
        let mut start = fn_line;
        while start > 0 {
            let above = lines[start - 1].trim_start();
            if above.starts_with("#[") || above.starts_with("///") {
                start -= 1;
            } else {
                break;
            }
        }
        let mut depth = 0_i32;
        let mut opened = false;
        let mut end = fn_line;
        for (index, line) in lines.iter().enumerate().skip(fn_line) {
            for character in line.chars() {
                match character {
                    '{' => {
                        depth += 1;
                        opened = true;
                    }
                    '}' => depth -= 1,
                    _ => {}
                }
            }
            end = index;
            if opened && depth <= 0 {
                break;
            }
        }
        bodies.push(lines[start..=end].join("\n"));
        let module_start = lines[..start]
            .iter()
            .rposition(|line| {
                let trimmed = line.trim();
                trimmed.contains("mod ") && trimmed.ends_with('{')
            })
            .map_or(0, |index| index + 1);
        let mut index = module_start;
        while index < start {
            let trimmed = lines[index].trim();
            if trimmed.starts_with("use ") {
                let mut statement = trimmed.to_owned();
                while !statement.ends_with(';') && index + 1 < start {
                    index += 1;
                    statement.push(' ');
                    statement.push_str(lines[index].trim());
                }
                if !uses.contains(&statement) {
                    uses.push(statement);
                }
            }
            index += 1;
        }
    }
    if !uses.iter().any(|line| line == "use super::*;") {
        uses.insert(0, "use super::*;".into());
    }
    format!(
        "\n#[cfg(test)]\nmod keel_prove_graft {{\n#![allow(unused_imports, dead_code)]\n{}\n{}\n}}\n",
        uses.join("\n"),
        bodies.join("\n\n")
    )
}

/// The package that owns `path` inside `tree` and the cargo target selector for it.
fn cargo_target(tree: &Path, path: &str) -> Result<(String, Vec<String>), String> {
    let file = tree.join(path);
    let mut dir = file.parent();
    while let Some(candidate) = dir {
        if !candidate.starts_with(tree) {
            break;
        }
        if let Ok(text) = std::fs::read_to_string(candidate.join("Cargo.toml"))
            && let Some(name) = package_name(&text)
        {
            let relative = file
                .strip_prefix(candidate)
                .map_err(|_| "path outside its crate".to_owned())?;
            let parts: Vec<String> = relative
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect();
            let selector = match parts.first().map(String::as_str) {
                Some("tests") if parts.len() == 2 => {
                    vec!["--test".into(), parts[1].trim_end_matches(".rs").to_owned()]
                }
                Some("tests") if parts.len() > 2 => vec!["--test".into(), parts[1].clone()],
                Some("src") if candidate.join("src/lib.rs").exists() => vec!["--lib".into()],
                Some("src") => vec!["--bins".into()],
                _ => {
                    return Err(format!(
                        "{path}: only src/ and tests/ targets are proven today"
                    ));
                }
            };
            return Ok((name, selector));
        }
        dir = candidate.parent();
    }
    Err(format!("{path}: no Cargo.toml with a [package] above it"))
}

fn package_name(manifest: &str) -> Option<String> {
    let mut in_package = false;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]";
            continue;
        }
        if in_package
            && let Some(value) = trimmed.strip_prefix("name")
            && let Some(value) = value.trim_start().strip_prefix('=')
        {
            return Some(value.trim().trim_matches('"').to_owned());
        }
    }
    None
}

/// Runs one test in one tree. Each side builds into its own subdirectory of the shared target:
/// cargo's freshness check can take one checkout's build as fresh for another checkout of the same
/// package, which would run the parent's binary as the head's (seen on the #1333 specimens).
fn run_test(tree: &Path, side: &str, test: &NewTest, options: &ProveOptions) -> RunResult {
    let (package, selector) = match cargo_target(tree, &test.path) {
        Ok(found) => found,
        Err(detail) => {
            return RunResult {
                outcome: RunOutcome::NotRun,
                detail,
            };
        }
    };
    let mut command = Command::new("cargo");
    command
        .current_dir(tree)
        .env("CARGO_TARGET_DIR", options.target_dir.join(side))
        .args(["test", "-p", &package])
        .args(&selector)
        .args(["--", &test.name, "--test-threads=1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match run_bounded(command, options.timeout) {
        Ok(Some((succeeded, stdout, stderr))) => {
            classify_run(&test.name, succeeded, &stdout, &stderr)
        }
        Ok(None) => RunResult {
            outcome: RunOutcome::TimedOut,
            detail: format!("killed after {}s", options.timeout.as_secs()),
        },
        Err(detail) => RunResult {
            outcome: RunOutcome::NotRun,
            detail,
        },
    }
}

/// Reads one `cargo test` run for one test name: the `test <path> ... <status>` lines whose path
/// is the name or ends in `::<name>`. Any such line failing is a failure.
fn classify_run(name: &str, succeeded: bool, stdout: &str, stderr: &str) -> RunResult {
    let suffix = format!("::{name}");
    let matched: Vec<(&str, RunOutcome)> = stdout
        .lines()
        .filter_map(|line| line.strip_prefix("test "))
        .filter_map(|rest| {
            let (path, status) = rest.split_once(" ... ")?;
            if path != name && !path.ends_with(&suffix) {
                return None;
            }
            let outcome = if status.starts_with("FAILED") {
                RunOutcome::Failed
            } else if status.starts_with("ignored") {
                RunOutcome::Ignored
            } else {
                RunOutcome::Passed
            };
            Some((rest, outcome))
        })
        .collect();
    for wanted in [RunOutcome::Failed, RunOutcome::Passed, RunOutcome::Ignored] {
        if let Some((line, _)) = matched.iter().find(|(_, outcome)| *outcome == wanted) {
            return RunResult {
                outcome: wanted,
                detail: format!("test {line}"),
            };
        }
    }
    let compiled = stdout.lines().any(|line| line.starts_with("running "));
    if !compiled && !succeeded {
        return RunResult {
            outcome: RunOutcome::DidNotCompile,
            detail: stderr
                .lines()
                .find(|line| line.starts_with("error"))
                .unwrap_or("cargo failed before any test ran")
                .to_owned(),
        };
    }
    RunResult {
        outcome: RunOutcome::NotFound,
        detail: format!("no test named {name} ran"),
    }
}

type Finished = (bool, String, String);

/// Runs `command` with piped output, killing the process tree after `timeout`. `Ok(None)` is a
/// timeout.
fn run_bounded(mut command: Command, timeout: Duration) -> Result<Option<Finished>, String> {
    let mut child = command
        .spawn()
        .map_err(|error| format!("cargo did not start: {error}"))?;
    let readers = [
        child
            .stdout
            .take()
            .map(|pipe| Box::new(pipe) as Box<dyn Read + Send>),
        child
            .stderr
            .take()
            .map(|pipe| Box::new(pipe) as Box<dyn Read + Send>),
    ]
    .map(|pipe| {
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut bytes);
            }
            let _ = sender.send(String::from_utf8_lossy(&bytes).into_owned());
        });
        receiver
    });
    let started = Instant::now();
    let deadline = started + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if Instant::now() >= deadline {
                    return Ok(None);
                }
                let read_to_deadline = |reader: &std::sync::mpsc::Receiver<String>| match reader
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                {
                    Ok(output) => Some(output),
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Some(String::new()),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
                };
                let Some(out) = read_to_deadline(&readers[0]) else {
                    return Ok(None);
                };
                let Some(err) = read_to_deadline(&readers[1]) else {
                    return Ok(None);
                };
                return Ok(Some((status.success(), out, err)));
            }
            Ok(None) if started.elapsed() >= timeout => {
                kill_tree(&mut child);
                // The readers are not joined: a straggling grandchild holding a pipe must not hold
                // this call open past its bound.
                return Ok(None);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(error) => return Err(format!("waiting on cargo failed: {error}")),
        }
    }
}

fn kill_tree(child: &mut std::process::Child) {
    if cfg!(windows) {
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &child.id().to_string()])
            .output();
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_repo(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let root =
            std::env::temp_dir().join(format!("keel-prove-{label}-{}-{nanos}", std::process::id()));
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        for args in [
            &["init", "--quiet"][..],
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "--quiet",
                "--allow-empty",
                "-m",
                "x",
            ][..],
        ] {
            git(&repo, args).unwrap();
        }
        root
    }

    fn add_worktree(repo: &Path, tree: &Path) {
        let arg = tree.to_string_lossy().into_owned();
        git(
            repo,
            &["worktree", "add", "--detach", "--quiet", &arg, "HEAD"],
        )
        .unwrap();
    }

    #[test]
    fn a_stale_record_of_an_interrupted_run_is_cleared_and_no_other_record_is_touched() {
        let root = temp_repo("stale");
        let repo = root.join("repo");
        let scratch_root = root.join("scratch");
        let own = scratch_root.join("keel-prove-1-2").join("parent");
        let foreign = root.join("elsewhere").join("parent");
        let live = scratch_root.join("keel-prove-3-4").join("head");
        for tree in [&own, &foreign, &live] {
            add_worktree(&repo, tree);
        }
        // An interrupted run: its directory is gone, its record stays. So is a stranger's.
        std::fs::remove_dir_all(scratch_root.join("keel-prove-1-2")).unwrap();
        std::fs::remove_dir_all(root.join("elsewhere")).unwrap();

        clear_stale_records(&repo, &scratch_root);

        let listed = listed_worktrees(&repo);
        assert!(!listed.iter().any(|entry| same_path(entry, &own)));
        assert!(listed.iter().any(|entry| same_path(entry, &foreign)));
        assert!(listed.iter().any(|entry| same_path(entry, &live)));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dropping_the_scratch_leaves_no_record_even_for_a_tree_whose_add_never_ran() {
        let root = temp_repo("drop");
        let repo = root.join("repo");
        let dir = root.join("scratch").join("keel-prove-5-6");
        let added = dir.join("parent");
        add_worktree(&repo, &added);
        let scratch = Scratch {
            repo: repo.clone(),
            dir: dir.clone(),
            worktrees: vec![added.clone(), dir.join("head")],
        };
        drop(scratch);
        let listed = listed_worktrees(&repo);
        assert_eq!(listed.len(), 1, "{listed:?}");
        assert!(!dir.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_finished_process_cannot_extend_the_proof_deadline_through_an_inherited_pipe() {
        let mut command = if cfg!(windows) {
            let mut command = Command::new("cmd");
            command.args(["/C", "start \"\" /b ping -n 2 127.0.0.1"]);
            command
        } else {
            let mut command = Command::new("sh");
            command.args(["-c", "sleep 1 & exit 0"]);
            command
        };
        let result = run_bounded(command, Duration::from_millis(100)).unwrap();

        assert!(result.is_none(), "inherited pipe bypassed the deadline");
    }

    #[test]
    fn verdict_separates_every_outcome_pair_that_means_something() {
        use RunOutcome::{DidNotCompile, Failed, NotFound, Passed, TimedOut};
        assert_eq!(verdict(Failed, Passed), TestVerdict::Earned);
        assert_eq!(verdict(DidNotCompile, Passed), TestVerdict::NewSubject);
        assert_eq!(verdict(Passed, Passed), TestVerdict::GreenOnParent);
        assert_eq!(verdict(Failed, Failed), TestVerdict::RedOnHead);
        assert_eq!(verdict(TimedOut, Passed), TestVerdict::Unproven);
        assert_eq!(verdict(Failed, NotFound), TestVerdict::Unproven);
    }

    #[test]
    fn a_run_is_read_from_the_named_test_line_not_from_a_neighbour() {
        let stdout = "running 2 tests\ntest a::other ... FAILED\ntest a::tests::mine ... ok\n";
        assert_eq!(
            classify_run("mine", false, stdout, "").outcome,
            RunOutcome::Passed
        );
        assert_eq!(
            classify_run("absent", true, stdout, "").outcome,
            RunOutcome::NotFound
        );
        assert_eq!(
            classify_run(
                "mine",
                false,
                "",
                "error[E0425]: cannot find function `new`"
            )
            .outcome,
            RunOutcome::DidNotCompile
        );
    }
}
