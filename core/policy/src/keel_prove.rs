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
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
    /// The crate did not build. The cause may be a new subject or an incomplete test graft.
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
    /// Reserved for a subject proven new independently of a failed parent build. The current
    /// outcome-only prover cannot establish that distinction and does not emit this verdict.
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
            // A failed graft can omit a test helper while the production subject already exists.
            // Compilation failure alone does not prove that the subject is new.
            RunOutcome::DidNotCompile => TestVerdict::Unproven,
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
        let mut added_line = 0u32;
        for (index, line) in file.added_with_context.iter().enumerate() {
            let Some(line) = line.strip_prefix('+') else {
                continue;
            };
            added_line = added_line.saturating_add(1);
            let trimmed = line.trim_start();
            if !(trimmed.starts_with("#[test]") || trimmed.starts_with("#[tokio::test")) {
                continue;
            }
            let mut name = None;
            for next in &file.added_with_context[index + 1..] {
                let Some(next) = next.strip_prefix('+').or_else(|| next.strip_prefix(' ')) else {
                    continue;
                };
                let next_trimmed = next.trim_start();
                if next_trimmed.starts_with("#[test]") || next_trimmed.starts_with("#[tokio::test")
                {
                    break;
                }
                if let Some(found) = fn_name(next) {
                    name = Some(found);
                    break;
                }
            }
            let name =
                name.unwrap_or_else(|| format!("__keel_unidentified_rust_test_{added_line}"));
            out.push(NewTest {
                name,
                path: file.path.clone(),
                line: added_line,
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
            Self::TypeScript => (trimmed.starts_with("it(")
                || trimmed.starts_with("test(")
                || trimmed.starts_with("it.each(")
                || trimmed.starts_with("test.each("))
            .then(|| {
                trimmed
                    .split(['"', '\'', '`'])
                    .nth(1)
                    .unwrap_or("")
                    .to_owned()
            }),
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
    /// How to run one non-Rust test and write a JUnit report, e.g.
    /// `npx vitest run {file} -t {name} --reporter=junit --outputFile={report}`. The placeholders
    /// are replaced, shell-quoted, and the line runs under `sh -c` in each worktree. Only the
    /// report decides: the testcase named exactly `{name}` failed, passed or was skipped; no
    /// report or no such testcase is `not_run`. Without it those tests stay unproven.
    pub command: Option<String>,
}

/// The process runner is supplied by the CLI so policy stays independent of operating-system
/// process adapters.
pub type ProveOutput = (bool, String, String);
pub type ProveRunner = fn(Command, Duration) -> Result<Option<ProveOutput>, String>;

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
pub fn prove_new_tests(
    diff: &str,
    options: &ProveOptions,
    runner: ProveRunner,
) -> Result<ProofReport, String> {
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
        proofs.extend(prove_rust(diff, &tests, &base, &head, options, runner)?);
    }
    let others = unsupported_new_tests(diff);
    match options.command.as_deref() {
        Some(template) if !others.is_empty() => {
            proofs.extend(prove_with_command(
                template, &others, &base, &head, options, runner,
            )?);
        }
        _ => {
            for (name, path) in others {
                let not_run = RunResult {
                    outcome: RunOutcome::NotRun,
                    detail: "no runner for this language; pass --prove-command to run it".into(),
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
        }
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
    runner: ProveRunner,
) -> Result<Vec<TestProof>, String> {
    let (scratch, parent_tree, head_tree) = scratch(base, head, options)?;
    let grafted = graft_onto_parent(diff, tests, &parent_tree, &head_tree);
    // Every parent run first, then every head run, so each side's build is reused by the next test.
    let parent_runs: Vec<RunResult> = tests
        .iter()
        .map(|test| match grafted.get(&test.path) {
            Some(Err(reason)) => RunResult {
                outcome: RunOutcome::DidNotCompile,
                detail: reason.clone(),
            },
            _ => run_test(&parent_tree, "parent", test, options, runner),
        })
        .collect();
    let proofs = tests
        .iter()
        .zip(parent_runs)
        .map(|(test, parent)| {
            let head_run = run_test(&head_tree, "head", test, options, runner);
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

/// Makes the scratch directory with a `parent` worktree at `base` and a `head` worktree at `head`.
fn scratch(
    base: &str,
    head: &str,
    options: &ProveOptions,
) -> Result<(Scratch, PathBuf, PathBuf), String> {
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
    Ok((scratch, parent_tree, head_tree))
}

/// Proves non-Rust tests with the caller's command line. The parent tree gets the head's whole
/// test file; a `node_modules` directory at the repository root is linked into both trees so the
/// runner finds its dependencies.
fn prove_with_command(
    template: &str,
    tests: &[(String, String)],
    base: &str,
    head: &str,
    options: &ProveOptions,
    runner: ProveRunner,
) -> Result<Vec<TestProof>, String> {
    let (scratch, parent_tree, head_tree) = scratch(base, head, options)?;
    for tree in [&parent_tree, &head_tree] {
        link_dependencies(&options.repo, tree);
    }
    let mut proofs = Vec::new();
    for (name, path) in tests {
        let grafted = std::fs::read(head_tree.join(path)).and_then(|bytes| {
            let target = parent_tree.join(path);
            if let Some(dir) = target.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(target, bytes)
        });
        let parent = match grafted {
            Ok(()) => run_command(&parent_tree, template, name, path, options, runner),
            Err(error) => RunResult {
                outcome: RunOutcome::NotRun,
                detail: format!("test file not copied to the parent: {error}"),
            },
        };
        let head_run = run_command(&head_tree, template, name, path, options, runner);
        proofs.push(TestProof {
            name: name.clone(),
            path: path.clone(),
            line: 0,
            verdict: verdict(parent.outcome, head_run.outcome),
            parent,
            head: head_run,
        });
    }
    drop(scratch);
    Ok(proofs)
}

#[cfg(unix)]
fn link_dependencies(repo: &Path, tree: &Path) {
    let source = repo.join("node_modules");
    if source.is_dir() && !tree.join("node_modules").exists() {
        let _ = std::os::unix::fs::symlink(source, tree.join("node_modules"));
    }
}

#[cfg(windows)]
fn link_dependencies(repo: &Path, tree: &Path) {
    let source = repo.join("node_modules");
    if source.is_dir() && !tree.join("node_modules").exists() {
        let _ = std::os::windows::fs::symlink_dir(source, tree.join("node_modules"));
    }
}

/// Single-quotes `text` for `sh`.
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn run_command(
    tree: &Path,
    template: &str,
    name: &str,
    path: &str,
    options: &ProveOptions,
    runner: ProveRunner,
) -> RunResult {
    let side = tree
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let report = tree.with_file_name(format!("{side}-report.xml"));
    let _ = std::fs::remove_file(&report);
    let line = template
        .replace("{file}", &shell_quote(path))
        .replace("{name}", &shell_quote(name))
        .replace("{report}", &shell_quote(&report.to_string_lossy()));
    let mut command = Command::new("sh");
    command
        .current_dir(tree)
        .args(["-c", &line])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match runner(command, options.timeout) {
        // The exit code and the console text are never read as the verdict: only the JUnit
        // report the runner wrote says whether the named test ran and how it ended.
        Ok(Some(_)) => match std::fs::read_to_string(&report) {
            Ok(xml) => junit_outcome(&xml, name).unwrap_or_else(|| RunResult {
                outcome: RunOutcome::NotRun,
                detail: format!("the report has no testcase named {name:?}"),
            }),
            Err(error) => RunResult {
                outcome: RunOutcome::NotRun,
                detail: format!("`{line}` wrote no readable report: {error}"),
            },
        },
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

/// The outcome of the `<testcase>` whose `name` attribute is exactly `name` in a JUnit report:
/// `<failure>` or `<error>` inside it is a failure, `<skipped>` an ignored test, anything else a
/// pass. `None` when no such testcase exists. Several matches: a failure wins.
fn junit_outcome(xml: &str, name: &str) -> Option<RunResult> {
    let mut found: Option<RunOutcome> = None;
    let mut rest = xml;
    while let Some(start) = rest.find("<testcase") {
        rest = &rest[start + "<testcase".len()..];
        let open_end = rest.find('>')?;
        let attrs = &rest[..open_end];
        let self_closing = attrs.ends_with('/');
        let body = if self_closing {
            ""
        } else {
            let close = rest.find("</testcase>").unwrap_or(rest.len());
            &rest[open_end..close]
        };
        if junit_attr(attrs, "name").as_deref() != Some(name) {
            continue;
        }
        let outcome = if body.contains("<failure") || body.contains("<error") {
            RunOutcome::Failed
        } else if body.contains("<skipped") {
            RunOutcome::Ignored
        } else {
            RunOutcome::Passed
        };
        found = Some(match (found, outcome) {
            (Some(RunOutcome::Failed), _) | (_, RunOutcome::Failed) => RunOutcome::Failed,
            (_, other) => other,
        });
    }
    found.map(|outcome| RunResult {
        outcome,
        detail: format!("JUnit testcase {name:?}: {outcome:?}"),
    })
}

fn junit_attr(attrs: &str, key: &str) -> Option<String> {
    let mut rest = attrs;
    loop {
        let at = rest.find(key)?;
        let before_ok = at == 0 || rest[..at].ends_with(char::is_whitespace);
        let after = rest[at + key.len()..].trim_start();
        if before_ok && let Some(value) = after.strip_prefix('=') {
            let value = value.trim_start();
            let quote = value.chars().next()?;
            if quote == '"' || quote == '\'' {
                let inner = &value[1..];
                let end = inner.find(quote)?;
                return Some(
                    inner[..end]
                        .replace("&lt;", "<")
                        .replace("&gt;", ">")
                        .replace("&quot;", "\"")
                        .replace("&apos;", "'")
                        .replace("&amp;", "&"),
                );
            }
        }
        rest = &rest[at + key.len()..];
    }
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
fn run_test(
    tree: &Path,
    side: &str,
    test: &NewTest,
    options: &ProveOptions,
    runner: ProveRunner,
) -> RunResult {
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
    match runner(command, options.timeout) {
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
    fn verdict_separates_every_outcome_pair_that_means_something() {
        use RunOutcome::{DidNotCompile, Failed, NotFound, Passed, TimedOut};
        assert_eq!(verdict(Failed, Passed), TestVerdict::Earned);
        assert_eq!(verdict(DidNotCompile, Passed), TestVerdict::Unproven);
        assert_eq!(verdict(Passed, Passed), TestVerdict::GreenOnParent);
        assert_eq!(verdict(Failed, Failed), TestVerdict::RedOnHead);
        assert_eq!(verdict(TimedOut, Passed), TestVerdict::Unproven);
        assert_eq!(verdict(Failed, NotFound), TestVerdict::Unproven);
    }

    #[test]
    fn an_attribute_added_above_an_unchanged_rust_test_is_proven() {
        let diff = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,3 +1,4 @@\n pub fn answer() -> u32 { 42 }\n \n+#[test]\n fn answer_is_forty_two() { assert_eq!(answer(), 42); }\n";
        let tests = new_rust_tests(diff);
        assert_eq!(tests.len(), 1);
        assert_eq!(tests[0].name, "answer_is_forty_two");
        assert_eq!(tests[0].path, "src/lib.rs");
        assert_eq!(tests[0].line, 1);
    }

    #[test]
    fn an_unidentified_rust_test_still_gets_an_unproven_entry() {
        let diff = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1,2 @@\n pub fn answer() -> u32 { 42 }\n+#[test]\n";
        let tests = new_rust_tests(diff);
        assert_eq!(tests.len(), 1);
        assert!(tests[0].name.starts_with("__keel_unidentified_rust_test_"));
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
