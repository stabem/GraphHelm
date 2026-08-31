//! The three builtin tools, each a thin explicit composition of the two primitives
//! ([`crate::process::run_in_workspace`] and the workspace/containment layer). No tool decides
//! policy: `authorize` already did, and the host routed. What lives here is only HOW a decided
//! call touches the machine.

use std::collections::BTreeMap;
use std::path::Path;

use graphhelm_tool_broker::call::valid_commit_message;
use graphhelm_tool_broker::path::RelativePath;

use crate::process::{CapturedProcess, HostError, ProcessLimits, run_in_workspace};
use crate::workspace::resolve_within;

/// Repository reads are in-process under the containment walk — Tier 0 runs no child at all
/// for `ReadFile`, and `ListFiles` is a bounded directory walk; `Diff` spawns
/// `git diff --no-ext-diff` (read-only under `GIT_OPTIONAL_LOCKS=0`). Writes run in the Tier 1
/// workspace: `ApplyPatch` pipes the patch through stdin (`git apply --index` — the one
/// per-tool stdin exception), `Commit` re-checks the argv bound as defense in depth and then
/// runs `git add -A` + `git commit -m`.
pub struct RepositoryTool;

/// Shell: exactly `run_in_workspace(root, program, arguments)` — no extra env, no shell string
/// ever.
pub struct ShellTool;

/// Tests: the runner program and its declared env come from host configuration, never the
/// caller; the caller's arguments append after nothing (the runner has no fixed base args in
/// this slice — exit code IS the contract).
pub struct TestsTool;

/// A synthesized in-process result shaped like a captured child, so every tool produces the
/// same material for the record: bytes, an exit code, truncation.
fn in_process(stdout: Vec<u8>, truncated: bool) -> CapturedProcess {
    CapturedProcess {
        exit_code: Some(0),
        stdout,
        stderr: Vec::new(),
        // This synthesizer writes STDOUT and leaves stderr empty, so its truncation is a stdout
        // truncation by construction -- and saying so is strictly more honest than the fused flag
        // was, which left a reader unable to tell which of two streams a synthesized cut belonged
        // to when only one of them can ever carry bytes.
        stdout_truncated: truncated,
        stderr_truncated: false,
        truncated,
        timed_out: false,
    }
}

impl RepositoryTool {
    /// Tier 0: read one file's bytes from `root` (the PROJECT for Tier 0 routing) under the
    /// containment walk. No child process exists to capture, so the read synthesizes one.
    pub(crate) fn read_file(
        root: &Path,
        path: &RelativePath,
        limits: &ProcessLimits,
    ) -> Result<CapturedProcess, HostError> {
        let resolved = resolve_within(root, path)?;
        let bytes = std::fs::read(&resolved).map_err(|source| HostError::Prepare { source })?;
        let truncated = bytes.len() > limits.max_output_bytes;
        let mut bytes = bytes;
        bytes.truncate(limits.max_output_bytes);
        Ok(in_process(bytes, truncated))
    }

    /// Tier 0: list files under an optional prefix — a bounded walk that skips `.git`, emits
    /// forward-slash workspace-relative paths, sorted, newline-joined (git ls-files
    /// semantics without spawning git).
    pub(crate) fn list_files(
        root: &Path,
        prefix: Option<&RelativePath>,
        limits: &ProcessLimits,
    ) -> Result<CapturedProcess, HostError> {
        let base = match prefix {
            Some(prefix) => resolve_within(root, prefix)?,
            None => root.to_path_buf(),
        };
        let mut names = Vec::new();
        let mut pending = vec![base];
        while let Some(dir) = pending.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if path.file_name().is_some_and(|n| n == ".git") {
                    continue;
                }
                if path.is_dir() {
                    pending.push(path);
                } else if let Ok(relative) = path.strip_prefix(root) {
                    names.push(relative.display().to_string().replace('\\', "/"));
                }
            }
        }
        names.sort();
        let joined = names.join("\n").into_bytes();
        let truncated = joined.len() > limits.max_output_bytes;
        let mut joined = joined;
        joined.truncate(limits.max_output_bytes);
        Ok(in_process(joined, truncated))
    }

    /// Worktree-vs-HEAD diff. `-C <target>` keeps git aimed at the right tree while the
    /// child's CWD (and its `.home`/`.tmp` droppings) stay in `scratch_root` — for Tier 0
    /// that scratch is an ephemeral sibling the host removes, so a read never writes a byte
    /// into the project.
    pub(crate) fn diff(
        scratch_root: &Path,
        target: &Path,
        path_prepend: &[std::path::PathBuf],
        limits: &ProcessLimits,
    ) -> Result<CapturedProcess, HostError> {
        let target_text = crate::workspace::git_safe(target);
        run_in_workspace(
            scratch_root,
            "git",
            &[
                "-C".to_owned(),
                target_text,
                "diff".to_owned(),
                "--no-ext-diff".to_owned(),
            ],
            &BTreeMap::new(),
            path_prepend,
            None,
            limits,
        )
    }

    /// Tier 1: apply a unified diff inside the workspace, patch bytes via piped stdin —
    /// never argv, never a temp file.
    pub(crate) fn apply_patch(
        workspace_root: &Path,
        patch: &str,
        path_prepend: &[std::path::PathBuf],
        limits: &ProcessLimits,
    ) -> Result<CapturedProcess, HostError> {
        run_in_workspace(
            workspace_root,
            "git",
            &["apply".to_owned(), "--index".to_owned()],
            &BTreeMap::new(),
            path_prepend,
            Some(patch.as_bytes()),
            limits,
        )
    }

    /// Tier 1: `git add -A` then `git commit -m <message>`. The message bound is re-checked
    /// here as defense in depth — `from_json` guards the trust boundary, but a
    /// directly-constructed call never passed through it, and this message rides argv.
    pub(crate) fn commit(
        workspace_root: &Path,
        message: &str,
        path_prepend: &[std::path::PathBuf],
        limits: &ProcessLimits,
    ) -> Result<CapturedProcess, HostError> {
        if !valid_commit_message(message) {
            return Err(HostError::Config {
                rule: "the commit message exceeds the argv bound",
            });
        }
        let add = run_in_workspace(
            workspace_root,
            "git",
            &["add".to_owned(), "-A".to_owned()],
            &BTreeMap::new(),
            path_prepend,
            None,
            limits,
        )?;
        if add.exit_code != Some(0) {
            return Ok(add);
        }
        run_in_workspace(
            workspace_root,
            "git",
            &[
                "commit".to_owned(),
                "--quiet".to_owned(),
                // Each invoke gets a FRESH ephemeral workspace, so a commit-after-apply
                // arrives in a tree with no changes of its own — and the commit's semantic
                // here is "record the workspace state on the detached HEAD", unchanged
                // included. Without this flag the empty case exits 1 and the plan's own
                // ephemeral contract would make the Commit action unreachable across calls.
                "--allow-empty".to_owned(),
                "-m".to_owned(),
                message.to_owned(),
            ],
            &BTreeMap::new(),
            path_prepend,
            None,
            limits,
        )
    }
}

impl ShellTool {
    pub(crate) fn run(
        workspace_root: &Path,
        program: &str,
        arguments: &[String],
        path_prepend: &[std::path::PathBuf],
        limits: &ProcessLimits,
    ) -> Result<CapturedProcess, HostError> {
        run_in_workspace(
            workspace_root,
            program,
            arguments,
            &BTreeMap::new(),
            path_prepend,
            None,
            limits,
        )
    }
}

impl TestsTool {
    pub(crate) fn run(
        workspace_root: &Path,
        runner: &str,
        runner_env: &BTreeMap<String, String>,
        arguments: &[String],
        path_prepend: &[std::path::PathBuf],
        limits: &ProcessLimits,
    ) -> Result<CapturedProcess, HostError> {
        run_in_workspace(
            workspace_root,
            runner,
            arguments,
            runner_env,
            path_prepend,
            None,
            limits,
        )
    }
}
