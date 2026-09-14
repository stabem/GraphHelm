//! The three builtin tools, each a thin explicit composition of the two primitives
//! ([`crate::process::run_in_workspace`] and the workspace/containment layer). No tool decides
//! policy: `authorize` already did, and the host routed. What lives here is only HOW a decided
//! call touches the machine.

use std::collections::BTreeMap;
use std::path::Path;

use graphhelm_tool_broker::call::valid_commit_message;
use graphhelm_tool_broker::path::RelativePath;

use crate::process::{
    CancelSignal, CapturedProcess, HostError, ProcessLimits, WORKSPACE_SCRATCH_NAMES,
    run_in_workspace,
};
use crate::workspace::resolve_within;

/// Repository reads are in-process under the containment walk — Tier 0 runs no child at all
/// for `ReadFile`, and `ListFiles` is a bounded directory walk; `Diff` spawns
/// `git diff --no-ext-diff` (read-only under `GIT_OPTIONAL_LOCKS=0`). Writes run in the Tier 1
/// workspace: `ApplyPatch` pipes the patch through stdin (`git apply --index` — the one
/// per-tool stdin exception), `Commit` re-checks the argv bound as defense in depth and then
/// runs `git add -A` (minus the funnel's scratch directories) + `git commit -m`.
pub struct RepositoryTool;

/// Shell: exactly `run_in_workspace(root, program, arguments)` — no extra env, no shell string
/// ever.
pub struct ShellTool;

/// Tests: the runner program and its declared env come from host configuration, never the
/// caller; the caller's arguments append after nothing (the runner has no fixed base args in
/// this slice — exit code IS the contract).
pub struct TestsTool;

/// Whether a multi-stage tool must stop at its first stage and return THAT capture.
///
/// `commit` is the only tool here that runs the funnel twice, and it used to test the first stage's
/// exit code alone. A `git add -A` that exits 0 while its readers were abandoned then had its
/// capture DISCARDED and replaced by the second stage's, so a lost capture in the first half could
/// be recorded as a wholly successful commit (Codex, on #703).
///
/// Reachable, and not only in theory: an untrusted repository whose `.gitattributes` selects a
/// clean filter (`filter.<driver>.clean`) has `git add` spawn that filter, and a descendant of it
/// holding a pipe is exactly the escape this issue is about.
///
/// A stage is final when it FAILED or when its capture was lost. The second is not a lesser case of
/// the first: an exit code of 0 whose bytes were never read is the more dangerous of the two,
/// because it looks like success rather than like an error.
fn first_stage_is_final(captured: &CapturedProcess) -> bool {
    // `reader_lost` joins for the reason the paragraph above already gives, applied to the other
    // way a capture goes missing (#790): an exit code of 0 whose bytes were never read looks like
    // success rather than like an error, and it does not matter to that argument WHETHER the bytes
    // were lost to an escaped descendant or to a reader thread that died holding them.
    captured.exit_code != Some(0) || captured.readers_abandoned || captured.reader_lost
}

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
        // No pipe and no reader: Tier 0 synthesizes its bytes rather than reading them.
        readers_abandoned: false,
        reader_lost: false,
        tree_kill: None,
        // Tier 0 spawns nothing, so no cancellation could have stopped a child here. A
        // measurement about a path with no child, not a default.
        cancelled: false,
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
        cancel: Option<&CancelSignal>,
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
            cancel,
        )
    }

    /// Tier 1: apply a unified diff inside the workspace, patch bytes via piped stdin —
    /// never argv, never a temp file.
    pub(crate) fn apply_patch(
        workspace_root: &Path,
        patch: &str,
        path_prepend: &[std::path::PathBuf],
        limits: &ProcessLimits,
        cancel: Option<&CancelSignal>,
    ) -> Result<CapturedProcess, HostError> {
        run_in_workspace(
            workspace_root,
            "git",
            &["apply".to_owned(), "--index".to_owned()],
            &BTreeMap::new(),
            path_prepend,
            Some(patch.as_bytes()),
            limits,
            cancel,
        )
    }

    /// Tier 1: `git add -A` then `git commit -m <message>`. The message bound is re-checked
    /// here as defense in depth — `from_json` guards the trust boundary, but a
    /// directly-constructed call never passed through it, and this message rides argv.
    ///
    /// The add excludes the funnel's scratch directories ([`WORKSPACE_SCRATCH_NAMES`]) by
    /// pathspec: inside an execution the root is the execution's own worktree, so `.home`
    /// (the child's HOME — `.cargo/credentials`, tool caches), `.tmp` and `.cbm-cache` sit
    /// beside the tracked tree and `-A` alone would land them in the ref (#1073). Pathspec
    /// rather than `info/exclude`: a linked worktree shares `info/exclude` with the project
    /// (`$GIT_COMMON_DIR/info/exclude`), so writing it would edit operator state the host has
    /// no business touching, and an exclude file inside the tree would itself be a change the
    /// commit carries. The argv is the host's alone; nothing here is caller input.
    ///
    /// The pathspec exclusion only governs what THIS add stages. `git` is on the child's PATH
    /// and in lease allowlists, and the execution's tree persists across calls, so an earlier
    /// shell call may already have staged a scratch path (`git add .home/token`); `-A` with an
    /// exclusion leaves the index alone for excluded paths, and the commit would carry it.
    /// So the scratch names are reset to `HEAD` in the index first — a `git reset -- <paths>`
    /// touches only the index (the files stay on disk), matches nothing without complaint, and
    /// keeps whatever `HEAD` itself tracks under those names.
    pub(crate) fn commit(
        workspace_root: &Path,
        message: &str,
        path_prepend: &[std::path::PathBuf],
        limits: &ProcessLimits,
        cancel: Option<&CancelSignal>,
    ) -> Result<CapturedProcess, HostError> {
        if !valid_commit_message(message) {
            return Err(HostError::Config {
                rule: "the commit message exceeds the argv bound",
            });
        }
        let mut reset_argv = vec!["reset".to_owned(), "-q".to_owned(), "--".to_owned()];
        reset_argv.extend(
            WORKSPACE_SCRATCH_NAMES
                .iter()
                .map(|name| (*name).to_owned()),
        );
        let reset = run_in_workspace(
            workspace_root,
            "git",
            &reset_argv,
            &BTreeMap::new(),
            path_prepend,
            None,
            limits,
            cancel,
        )?;
        if first_stage_is_final(&reset) {
            return Ok(reset);
        }
        let mut add_argv = vec![
            "add".to_owned(),
            "-A".to_owned(),
            "--".to_owned(),
            ".".to_owned(),
        ];
        add_argv.extend(
            WORKSPACE_SCRATCH_NAMES
                .iter()
                .map(|name| format!(":(exclude,top){name}")),
        );
        let add = run_in_workspace(
            workspace_root,
            "git",
            &add_argv,
            &BTreeMap::new(),
            path_prepend,
            None,
            limits,
            cancel,
        )?;
        if first_stage_is_final(&add) {
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
            cancel,
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
        cancel: Option<&CancelSignal>,
    ) -> Result<CapturedProcess, HostError> {
        run_in_workspace(
            workspace_root,
            program,
            arguments,
            &BTreeMap::new(),
            path_prepend,
            None,
            limits,
            cancel,
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
        cancel: Option<&CancelSignal>,
    ) -> Result<CapturedProcess, HostError> {
        run_in_workspace(
            workspace_root,
            runner,
            arguments,
            runner_env,
            path_prepend,
            None,
            limits,
            cancel,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::first_stage_is_final;
    use crate::process::CapturedProcess;

    /// A stage whose reader was lost is FINAL, even at exit code 0 (#790).
    ///
    /// The boundary again. `first_stage_is_final`'s own doc already argues this for the other
    /// cause: an exit code of 0 whose bytes were never read looks like success rather than like
    /// an error, and it is the more dangerous of the two. That argument does not care which way
    /// the bytes went missing, and without this cell dropping `|| captured.reader_lost` is silent.
    #[test]
    fn a_stage_whose_reader_was_lost_is_final_despite_a_zero_exit() {
        let mut lost = staged(Some(0), false);
        lost.reader_lost = true;
        assert!(
            first_stage_is_final(&lost),
            "a zero exit whose bytes were never read must stop the sequence, not continue it"
        );
    }

    /// CONTROL: an ordinary zero-exit stage still continues.
    #[test]
    fn an_ordinary_first_stage_with_a_zero_exit_still_continues() {
        assert!(
            !first_stage_is_final(&staged(Some(0), false)),
            "a clean stage is not final, or the predicate stops every sequence"
        );
    }

    fn staged(exit_code: Option<i32>, readers_abandoned: bool) -> CapturedProcess {
        CapturedProcess {
            exit_code,
            stdout: Vec::new(),
            stderr: Vec::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            truncated: false,
            timed_out: false,
            readers_abandoned,
            reader_lost: false,
            tree_kill: None,
            cancelled: false,
        }
    }

    /// THE cell: a first stage that SUCCEEDED and lost its capture. The old test read the exit code
    /// alone, so this one passed straight through and the second stage's result replaced it.
    #[test]
    fn a_successful_first_stage_that_lost_its_capture_is_final() {
        assert!(first_stage_is_final(&staged(Some(0), true)));
    }

    /// The control, without which the cell above would be satisfied by a predicate that always
    /// stops: an ordinary first stage must NOT stop, or the second stage would never run.
    #[test]
    fn an_ordinary_first_stage_continues() {
        assert!(!first_stage_is_final(&staged(Some(0), false)));
    }

    /// And the case that already worked, kept so the addition cannot quietly remove it.
    #[test]
    fn a_failed_first_stage_is_still_final() {
        assert!(first_stage_is_final(&staged(Some(1), false)));
    }

    /// A stage with no exit code at all is final too -- unknown is not success.
    #[test]
    fn an_unknown_exit_code_is_final() {
        assert!(first_stage_is_final(&staged(None, false)));
    }
}
