//! #1066: one Tier 1 workspace per execution, and the commit lands as a ref.
//!
//! `ToolHost::invoke` keeps its per-call contract (a fresh tree, torn down after the call — the
//! broker suite and the CLI's `tool invoke` are written against it). `invoke_for_execution` is the
//! execution-shaped door: the first Tier 1 call of an execution provisions its tree, every later
//! call reuses it, a completed `commit` moves `refs/graphhelm/executions/<id>` in the PROJECT, and
//! `release` removes the tree while the ref stays. The operator's `HEAD` never moves.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use graphhelm_tool_broker::call::{RepositoryAction, ShellAction, ToolCall};
use graphhelm_tool_broker::lease::{Capability, ToolLease};
use graphhelm_tool_broker::record::{ToolDisposition, execution_ref};
use graphhelm_tool_host::host::{HostConfig, ToolHost};
use graphhelm_tool_host::process::ProcessLimits;
use graphhelm_tool_host::workspace::WorkspaceConfig;

fn git(project: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(project)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "scratch")
        .env("GIT_AUTHOR_EMAIL", "scratch@test.invalid")
        .env("GIT_COMMITTER_NAME", "scratch")
        .env("GIT_COMMITTER_EMAIL", "scratch@test.invalid")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn scratch_repo() -> (tempfile::TempDir, PathBuf) {
    scratch_repo_with(&["init", "--quiet"])
}

/// `scratch_repo` with an explicit `git init` argv — the SHA-256 cell needs
/// `--object-format=sha256`.
fn scratch_repo_with(init: &[&str]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join("src/lib.rs"), "// scratch\n").unwrap();
    git(&project, init);
    git(&project, &["add", "-A"]);
    git(&project, &["commit", "--quiet", "-m", "scratch"]);
    (dir, project)
}

fn host(project: &Path, staging: &Path, keep_workspace: bool) -> ToolHost {
    let tool_dir = Path::new(env!("CARGO_BIN_EXE_fake_tool"))
        .parent()
        .unwrap()
        .to_path_buf();
    ToolHost::new(HostConfig {
        workspace: WorkspaceConfig::validated(project, staging, &[]).unwrap(),
        limits: ProcessLimits {
            timeout: Duration::from_secs(30),
            max_output_bytes: 1024 * 1024,
        },
        tests_runner: "fake_tool".to_owned(),
        tests_runner_env: BTreeMap::new(),
        path_prepend: vec![tool_dir],
        keep_workspace,
    })
}

/// #1086 (Codex P1 on #1092): a ref probe that does not answer is `Unavailable`, never `Absent`.
/// The host's `path_prepend` puts a `git` shim first that is `fake_tool` under another name: it
/// exits 64 for the unknown mode `-C`, which is neither 0 (present) nor 1 (verified absent), so
/// the probe fails deterministically on every host -- a 1 ms budget, the first shape of this
/// cell, let a fast `rev-parse` answer 1 before the clock was checked (gate `ffbea56e`). The
/// compile must report that it could not see the tree instead of reading the project checkout.
#[test]
fn a_failed_ref_probe_is_unavailable_not_absent() {
    use graphhelm_runtime::ports::{ExecutionTreeAccess, ExecutionTreePort};
    use graphhelm_tool_host::source_reader::ExecutionContextTree;

    let (dir, project) = scratch_repo();
    let staging = dir.path().join("staging");
    std::fs::create_dir_all(&staging).unwrap();
    let fake_tool = Path::new(env!("CARGO_BIN_EXE_fake_tool"));
    let shim = dir.path().join("shim");
    std::fs::create_dir_all(&shim).unwrap();
    let git_shim = shim.join(if cfg!(windows) { "git.exe" } else { "git" });
    std::fs::copy(fake_tool, &git_shim).unwrap();
    let host = std::sync::Arc::new(ToolHost::new(HostConfig {
        workspace: WorkspaceConfig::validated(&project, &staging, &[]).unwrap(),
        limits: ProcessLimits {
            timeout: Duration::from_secs(30),
            max_output_bytes: 1024 * 1024,
        },
        tests_runner: "fake_tool".to_owned(),
        tests_runner_env: BTreeMap::new(),
        path_prepend: vec![shim, fake_tool.parent().unwrap().to_path_buf()],
        keep_workspace: false,
    }));
    let tree = ExecutionContextTree::new(host, "exec-probe");
    let cancel = graphhelm_runtime::ports::ScanCancel::new();
    let mut compiled = false;
    let access = tree.with_tree(&cancel, &mut |_search, _reader| compiled = true);
    assert_eq!(access, ExecutionTreeAccess::Unavailable);
    assert!(
        !compiled,
        "nothing was compiled over a tree the probe could not see"
    );
    assert!(
        workspaces_in(&staging).is_empty(),
        "a failed probe never provisions a tree from HEAD"
    );
}

fn writer_lease() -> ToolLease {
    ToolLease {
        actor: "agent-writer".to_owned(),
        capabilities: [
            Capability::RepositoryRead,
            Capability::RepositoryWrite,
            Capability::ShellExecute,
            Capability::TestsExecute,
        ]
        .into_iter()
        .collect(),
        programs: ["fake_tool".to_owned()].into_iter().collect(),
    }
}

/// A unified diff that appends one line to `src/lib.rs` — the "fix" every test below lands.
const PATCH: &str = "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1,2 @@\n // scratch\n+pub const FIXED: bool = true;\n";

fn apply() -> ToolCall {
    ToolCall::Repository(RepositoryAction::ApplyPatch {
        patch: PATCH.to_owned(),
    })
}

fn commit(message: &str) -> ToolCall {
    ToolCall::Repository(RepositoryAction::Commit {
        message: message.to_owned(),
    })
}

fn completed(disposition: &ToolDisposition) -> bool {
    matches!(disposition, ToolDisposition::Completed { exit_code: 0 })
}

/// Entries under the staging directory that are workspaces (everything the host provisions is
/// named `ghtool-…`; the read cache lives beside them and is not a tree).
fn workspaces_in(staging: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(staging)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("ghtool-"))
        .collect();
    names.sort();
    names
}

/// #1086 item 5: the context compile reads the execution's own tree, under the lock tool calls
/// hold. No tree and no landed ref: `Absent`, and nothing is provisioned (a read never provisions
/// from `HEAD`). After a patch: the tree, patched, while the checkout is not. After a commit, a
/// release and a NEW host: the read provisions the tree from the landed ref.
#[test]
fn the_context_compile_reads_the_execution_tree_and_never_provisions_one_from_head() {
    use graphhelm_runtime::context::SEARCH_BOUNDS;
    use graphhelm_runtime::ports::{ExecutionTreeAccess, ExecutionTreePort};
    use graphhelm_tool_host::source_reader::ExecutionContextTree;

    let (dir, project) = scratch_repo();
    let staging = dir.path().join("staging");
    std::fs::create_dir_all(&staging).unwrap();
    let first = std::sync::Arc::new(host(&project, &staging, false));
    let read_fixed = |tree: &ExecutionContextTree| {
        let mut seen = None;
        let cancel = graphhelm_runtime::ports::ScanCancel::new();
        let access = tree.with_tree(&cancel, &mut |search, reader| {
            let hits = search
                .search(&["fixed".to_owned()], &SEARCH_BOUNDS)
                .unwrap();
            let bytes = reader.read_prefix("src/lib.rs", 1024).unwrap().bytes;
            seen = Some((hits, String::from_utf8(bytes).unwrap()));
        });
        (access, seen)
    };

    let tree = ExecutionContextTree::new(first.clone(), "exec-context");
    let (access, seen) = read_fixed(&tree);
    assert_eq!(access, ExecutionTreeAccess::Absent);
    assert!(seen.is_none());
    assert!(
        workspaces_in(&staging).is_empty(),
        "a read never provisions a tree from HEAD"
    );

    let (record, _) =
        first.invoke_for_execution("exec-context", &apply(), &writer_lease(), "agent-writer");
    assert!(completed(&record.disposition), "{record:?}");
    let (access, seen) = read_fixed(&tree);
    assert_eq!(access, ExecutionTreeAccess::Read);
    let (hits, lib) = seen.unwrap();
    assert!(hits.contains(&"src/lib.rs".to_owned()), "{hits:?}");
    assert!(lib.contains("FIXED"), "the compile sees the patch: {lib}");
    assert!(
        !std::fs::read_to_string(project.join("src/lib.rs"))
            .unwrap()
            .contains("FIXED"),
        "the checkout never saw it"
    );

    let (record, _) = first.invoke_for_execution(
        "exec-context",
        &commit("fix: land for the context read"),
        &writer_lease(),
        "agent-writer",
    );
    assert!(completed(&record.disposition), "{record:?}");
    first.release("exec-context").unwrap();
    assert!(workspaces_in(&staging).is_empty());

    let second = std::sync::Arc::new(host(&project, &staging, false));
    let tree = ExecutionContextTree::new(second.clone(), "exec-context");
    let (access, seen) = read_fixed(&tree);
    assert_eq!(access, ExecutionTreeAccess::Read);
    assert!(
        seen.unwrap().1.contains("FIXED"),
        "provisioned from the ref"
    );
    second.release("exec-context").unwrap();
    assert!(workspaces_in(&staging).is_empty());
}

/// A scratch repository with a provisioned execution tree for `execution` (one patch applied).
fn provisioned(execution: &str) -> (tempfile::TempDir, PathBuf, std::sync::Arc<ToolHost>) {
    let (dir, project) = scratch_repo();
    let staging = dir.path().join("staging");
    std::fs::create_dir_all(&staging).unwrap();
    let host = std::sync::Arc::new(host(&project, &staging, false));
    let (record, _) =
        host.invoke_for_execution(execution, &apply(), &writer_lease(), "agent-writer");
    assert!(completed(&record.disposition), "{record:?}");
    (dir, staging, host)
}

/// A context scan parked inside the execution tree until `unblock` fires: the stand-in for a
/// `read_dir`, open or read that blocks in the kernel, which no token can interrupt.
fn blocked_scan(
    host: &std::sync::Arc<ToolHost>,
    execution: &str,
    cancel: &graphhelm_runtime::ports::ScanCancel,
) -> (
    std::thread::JoinHandle<Result<Option<()>, graphhelm_tool_host::process::HostError>>,
    std::sync::mpsc::Sender<()>,
) {
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (unblock_tx, unblock_rx) = std::sync::mpsc::channel::<()>();
    let scan = {
        let host = host.clone();
        let execution = execution.to_owned();
        let cancel = cancel.clone();
        std::thread::spawn(move || {
            host.with_execution_tree(&execution, &cancel, |_root| {
                entered_tx.send(()).unwrap();
                let _ = unblock_rx.recv();
            })
        })
    };
    entered_rx
        .recv_timeout(Duration::from_secs(60))
        .expect("the scan entered the tree");
    (scan, unblock_tx)
}

/// Codex P1 on #1092: a drive cancelled while a context scan is blocked inside the execution
/// tree must not wait for that scan. The release returns within its bounded wait while the scan
/// is still blocked, and the tree goes when the scan lets go.
#[test]
fn a_release_never_waits_on_an_abandoned_scan_and_the_tree_goes_when_the_scan_ends() {
    let (_dir, staging, host) = provisioned("exec-cancel");
    let cancel = graphhelm_runtime::ports::ScanCancel::new();
    let (scan, unblock) = blocked_scan(&host, "exec-cancel", &cancel);
    cancel.cancel();

    let (done_tx, done_rx) = std::sync::mpsc::channel();
    {
        let host = host.clone();
        std::thread::spawn(move || {
            let started = std::time::Instant::now();
            let released = host.release("exec-cancel");
            done_tx.send((started.elapsed(), released)).unwrap();
        });
    }
    let (elapsed, released) = done_rx
        .recv_timeout(Duration::from_secs(20))
        .expect("the release returned while the scan was still blocked");
    released.unwrap();
    assert!(elapsed < Duration::from_secs(20), "{elapsed:?}");
    assert_eq!(
        workspaces_in(&staging).len(),
        1,
        "the tree is still held by the scan, so its removal was deferred, not raced"
    );

    unblock.send(()).unwrap();
    assert_eq!(scan.join().unwrap().unwrap(), Some(()));
    assert!(
        workspaces_in(&staging).is_empty(),
        "the scan removed the released tree as it let go"
    );
    assert!(host.live_executions().is_empty());
}

/// A scan whose drive gave it up while it WAITS for the tree never takes the tree: it answers
/// `Cancelled` while the holder still holds it.
#[test]
fn a_scan_cancelled_while_waiting_for_the_tree_gives_up_without_taking_it() {
    let (_dir, _staging, host) = provisioned("exec-wait");
    let holder_cancel = graphhelm_runtime::ports::ScanCancel::new();
    let (holder, unblock) = blocked_scan(&host, "exec-wait", &holder_cancel);

    let waiting_cancel = graphhelm_runtime::ports::ScanCancel::new();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    {
        let host = host.clone();
        let waiting_cancel = waiting_cancel.clone();
        std::thread::spawn(move || {
            let outcome = host.with_execution_tree("exec-wait", &waiting_cancel, |_| {
                panic!("a cancelled scan must never run over the tree")
            });
            done_tx.send(outcome.is_err()).unwrap();
        });
    }
    assert!(
        done_rx.recv_timeout(Duration::from_millis(300)).is_err(),
        "the second scan waits while the tree is held"
    );
    waiting_cancel.cancel();
    assert!(
        done_rx
            .recv_timeout(Duration::from_secs(20))
            .expect("the cancelled waiter returned while the holder still held the tree"),
        "the cancelled waiter answered Cancelled"
    );
    unblock.send(()).unwrap();
    assert_eq!(holder.join().unwrap().unwrap(), Some(()));
    host.release("exec-wait").unwrap();
}

/// The guarantee item 5 of #1086 rests on still holds: a tool call of the execution waits for a
/// context scan that is reading the tree, and runs once the scan lets go.
#[test]
fn a_tool_call_waits_for_a_context_scan_reading_the_tree() {
    let (_dir, staging, host) = provisioned("exec-read");
    let cancel = graphhelm_runtime::ports::ScanCancel::new();
    let (scan, unblock) = blocked_scan(&host, "exec-read", &cancel);

    let (done_tx, done_rx) = std::sync::mpsc::channel();
    {
        let host = host.clone();
        std::thread::spawn(move || {
            let (record, _) = host.invoke_for_execution(
                "exec-read",
                &commit("fix: after the read"),
                &writer_lease(),
                "agent-writer",
            );
            done_tx.send(record).unwrap();
        });
    }
    assert!(
        done_rx.recv_timeout(Duration::from_millis(500)).is_err(),
        "no tool call writes while the scan reads"
    );
    unblock.send(()).unwrap();
    assert_eq!(scan.join().unwrap().unwrap(), Some(()));
    let record = done_rx
        .recv_timeout(Duration::from_secs(60))
        .expect("the tool call ran once the scan let go");
    assert!(completed(&record.disposition), "{record:?}");
    host.release("exec-read").unwrap();
    assert!(workspaces_in(&staging).is_empty());
}

/// THE journey at the host layer: apply, then commit, in ONE execution — the patch node A applied
/// is what node C commits, the commit is reachable from `refs/graphhelm/executions/<id>` in the
/// project, its tree carries the fix, the operator's `HEAD` did not move, and the record names
/// both the commit and the ref.
#[test]
fn a_patch_applied_by_one_call_is_committed_by_the_next_and_lands_as_a_ref() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let host = host(&project, staging.path(), false);
    let lease = writer_lease();
    let execution = "exec-useful-change";
    let head_before = git(&project, &["rev-parse", "HEAD"]);

    let (apply_record, apply_streams) =
        host.invoke_for_execution(execution, &apply(), &lease, "agent-writer");
    assert!(
        completed(&apply_record.disposition),
        "apply must complete: {:?} / {}",
        apply_record.disposition,
        String::from_utf8_lossy(&apply_streams.stderr)
    );
    assert_eq!(apply_record.commit, None, "a patch is not a commit");
    assert_eq!(apply_record.landed_ref, None);

    let (commit_record, commit_streams) =
        host.invoke_for_execution(execution, &commit("land the fix"), &lease, "agent-writer");
    assert!(
        completed(&commit_record.disposition),
        "commit must complete: {:?} / {}",
        commit_record.disposition,
        String::from_utf8_lossy(&commit_streams.stderr)
    );
    let reference = execution_ref(execution);
    assert_eq!(reference, "refs/graphhelm/executions/exec-useful-change");
    assert_eq!(
        commit_record.landed_ref.as_deref(),
        Some(reference.as_str())
    );

    // The ref resolves IN THE PROJECT to the commit the record names…
    let landed = git(&project, &["rev-parse", &reference]);
    assert_eq!(commit_record.commit.as_deref(), Some(landed.as_str()));
    // …its tree carries the fix (read from the object store — no worktree needed)…
    let blob = git(&project, &["show", &format!("{reference}:src/lib.rs")]);
    assert!(
        blob.contains("pub const FIXED: bool = true;"),
        "the landed tree must carry the patch: {blob:?}"
    );
    // …its parent is the scratch commit (one commit, on top of what the execution started from)…
    assert_eq!(
        git(&project, &["rev-parse", &format!("{reference}^")]),
        head_before
    );
    // …and the operator's checkout is untouched: HEAD did not move and the file is unchanged.
    assert_eq!(git(&project, &["rev-parse", "HEAD"]), head_before);
    assert_eq!(
        std::fs::read_to_string(project.join("src/lib.rs")).unwrap(),
        "// scratch\n"
    );
    // The ref is a plain ref, not a branch.
    assert_eq!(git(&project, &["branch", "--list"]).lines().count(), 1);

    // The execution's tree is alive until release, and release removes it while the ref stays.
    assert_eq!(host.live_executions(), vec![execution.to_owned()]);
    assert_eq!(
        workspaces_in(staging.path()).len(),
        1,
        "one tree for one execution"
    );
    host.release(execution).unwrap();
    assert!(host.live_executions().is_empty());
    assert!(
        workspaces_in(staging.path()).is_empty(),
        "release must remove the tree: {:?}",
        workspaces_in(staging.path())
    );
    assert_eq!(git(&project, &["rev-parse", &reference]), landed);
    // Releasing twice is a no-op, not an error.
    host.release(execution).unwrap();
}

/// Scratch never lands (#1073): every spawn redirects the child's HOME, TEMP and the index
/// cache to `.home`, `.tmp` and `.cbm-cache` INSIDE the execution's tree, so whatever a shell or
/// tests call drops there — `.cargo/credentials` under HOME is the sharp case — sits beside the
/// tracked files when `commit` runs. The commit must carry the real change and none of the
/// scratch.
///
/// Untracked scratch is the easy half. `git` is on the child's PATH and the tree persists across
/// the execution's calls, so a shell call can also STAGE a scratch path before the commit runs;
/// the add's pathspec exclusion does not unstage what is already in the index, and without a
/// reset that path lands. The cell plants both: untracked files, and one path staged through a
/// shell call.
#[test]
fn scratch_written_by_a_shell_call_never_lands_in_the_ref() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let host = host(&project, staging.path(), false);
    let mut lease = writer_lease();
    lease.programs.insert("git".to_owned());
    let execution = "exec-scratch";

    // The first call provisions the tree and lands the real change in it.
    let (apply_record, _) = host.invoke_for_execution(execution, &apply(), &lease, "agent-writer");
    assert!(completed(&apply_record.disposition));
    let workspaces = workspaces_in(staging.path());
    assert_eq!(workspaces.len(), 1, "{workspaces:?}");
    let tree = staging.path().join(&workspaces[0]);

    // What a shell/tests call would have left under its redirected HOME and TEMP, and in the
    // index cache — planted directly so the cell does not depend on what `fake_tool` writes.
    for planted in [".home/.cargo/credentials", ".tmp/x", ".cbm-cache/index.bin"] {
        let path = tree.join(planted);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            b"[registry]
token = \"never-committed\"
",
        )
        .unwrap();
    }

    // A scratch path STAGED by a shell call of the same execution — the sharp half. The tree is
    // the one the apply provisioned, so the add lands in the index the commit will read.
    std::fs::write(tree.join(".home/token"), b"staged-never-committed\n").unwrap();
    let stage = ToolCall::Shell(ShellAction {
        program: "git".to_owned(),
        arguments: vec!["add".to_owned(), ".home/token".to_owned()],
    });
    let (stage_record, stage_streams) =
        host.invoke_for_execution(execution, &stage, &lease, "agent-writer");
    assert!(
        completed(&stage_record.disposition),
        "the shell add must complete: {:?} / {}",
        stage_record.disposition,
        String::from_utf8_lossy(&stage_streams.stderr)
    );
    assert_eq!(
        git(&tree, &["ls-files", "--", ".home"]),
        ".home/token",
        "the shell call must have staged the scratch path — otherwise this cell proves nothing"
    );

    let (commit_record, commit_streams) =
        host.invoke_for_execution(execution, &commit("land the fix"), &lease, "agent-writer");
    assert!(
        completed(&commit_record.disposition),
        "commit must complete: {:?} / {}",
        commit_record.disposition,
        String::from_utf8_lossy(&commit_streams.stderr)
    );
    let reference = execution_ref(execution);
    assert_eq!(
        commit_record.landed_ref.as_deref(),
        Some(reference.as_str())
    );

    let listed = git(&project, &["ls-tree", "-r", "--name-only", &reference]);
    let paths: Vec<&str> = listed.lines().collect();
    assert!(
        paths.contains(&"src/lib.rs"),
        "the real change must land: {paths:?}"
    );
    let blob = git(&project, &["show", &format!("{reference}:src/lib.rs")]);
    assert!(blob.contains("pub const FIXED: bool = true;"), "{blob:?}");
    assert!(
        !paths.contains(&".home/token"),
        "a pre-staged scratch path must be unstaged before the commit: {paths:?}"
    );
    for scratch in [".home", ".tmp", ".cbm-cache"] {
        assert!(
            !paths
                .iter()
                .any(|path| *path == scratch || path.starts_with(&format!("{scratch}/"))),
            "{scratch} must not land in the ref: {paths:?}"
        );
    }
    // The scratch is still on disk in the tree — excluded from the commit, not deleted.
    assert!(tree.join(".home/.cargo/credentials").is_file());
    assert!(tree.join(".home/token").is_file());
    host.release(execution).unwrap();
}

/// A later drive of the SAME execution continues from its own ref, not from the operator's
/// `HEAD`: the second tree is provisioned at the landed commit, so a second commit stacks on the
/// first and the ref moves forward.
#[test]
fn a_released_execution_resumes_from_its_own_ref() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let host = host(&project, staging.path(), false);
    let lease = writer_lease();
    let execution = "exec-resumed";

    let (record, _) = host.invoke_for_execution(execution, &apply(), &lease, "agent-writer");
    assert!(completed(&record.disposition));
    let (first, _) = host.invoke_for_execution(execution, &commit("first"), &lease, "agent-writer");
    assert!(completed(&first.disposition));
    host.release(execution).unwrap();

    // Second drive: an empty commit is enough to prove where the tree started.
    let (second, streams) =
        host.invoke_for_execution(execution, &commit("second"), &lease, "agent-writer");
    assert!(
        completed(&second.disposition),
        "{:?} / {}",
        second.disposition,
        String::from_utf8_lossy(&streams.stderr)
    );
    let reference = execution_ref(execution);
    let tip = git(&project, &["rev-parse", &reference]);
    assert_eq!(second.commit.as_deref(), Some(tip.as_str()));
    assert_eq!(
        git(&project, &["rev-parse", &format!("{reference}^")]),
        first.commit.unwrap(),
        "the second commit must stack on the first, not on HEAD"
    );
    host.release(execution).unwrap();
}

/// Two executions hold two trees and land two refs; neither sees the other's patch.
#[test]
fn two_executions_land_two_refs_without_sharing_a_tree() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let host = host(&project, staging.path(), false);
    let lease = writer_lease();

    let (a, _) = host.invoke_for_execution("exec-a", &apply(), &lease, "agent-writer");
    assert!(completed(&a.disposition));
    assert_eq!(workspaces_in(staging.path()).len(), 1);
    // exec-b commits WITHOUT applying: its tree must not carry exec-a's patch.
    let (b, _) = host.invoke_for_execution("exec-b", &commit("b"), &lease, "agent-writer");
    assert!(completed(&b.disposition));
    assert_eq!(
        workspaces_in(staging.path()).len(),
        2,
        "one tree per execution"
    );
    let (a_commit, _) = host.invoke_for_execution("exec-a", &commit("a"), &lease, "agent-writer");
    assert!(completed(&a_commit.disposition));

    let a_blob = git(
        &project,
        &["show", "refs/graphhelm/executions/exec-a:src/lib.rs"],
    );
    let b_blob = git(
        &project,
        &["show", "refs/graphhelm/executions/exec-b:src/lib.rs"],
    );
    assert!(a_blob.contains("FIXED"));
    assert!(
        !b_blob.contains("FIXED"),
        "exec-b must not see exec-a's tree"
    );
    let mut live = host.live_executions();
    live.sort();
    assert_eq!(live, vec!["exec-a".to_owned(), "exec-b".to_owned()]);
    host.release("exec-a").unwrap();
    host.release("exec-b").unwrap();
    assert!(workspaces_in(staging.path()).is_empty());
}

/// The per-call door is UNCHANGED: `invoke` still provisions a fresh tree per call and removes
/// it after, so a commit made through it names its object id but lands no ref — there is no
/// execution to land under — and the next call starts from `HEAD` again.
#[test]
fn a_per_call_invoke_keeps_its_ephemeral_contract_and_lands_no_ref() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let host = host(&project, staging.path(), false);
    let lease = writer_lease();
    let head_before = git(&project, &["rev-parse", "HEAD"]);

    let (apply_record, _) = host.invoke(&apply(), &lease, "agent-writer");
    assert!(completed(&apply_record.disposition));
    assert!(
        workspaces_in(staging.path()).is_empty(),
        "torn down after the call"
    );
    let (commit_record, _) = host.invoke(&commit("ephemeral"), &lease, "agent-writer");
    assert!(completed(&commit_record.disposition));
    assert!(
        commit_record.commit.is_some(),
        "the record still names the object id it made"
    );
    assert_eq!(commit_record.landed_ref, None, "no execution, no ref");
    assert!(workspaces_in(staging.path()).is_empty());
    assert_eq!(git(&project, &["rev-parse", "HEAD"]), head_before);
    assert!(host.live_executions().is_empty());
    // No ref of ours exists in the project at all.
    let refs = git(&project, &["for-each-ref", "refs/graphhelm/"]);
    assert!(
        refs.is_empty(),
        "a per-call commit must land nothing: {refs}"
    );
}

/// `keep_workspace` keeps an execution's tree past `release`, exactly as it keeps a per-call
/// tree past the call: kept is the operator's to delete.
#[test]
fn keep_workspace_leaves_the_execution_tree_for_the_operator() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let host = host(&project, staging.path(), true);
    let lease = writer_lease();
    let (record, _) = host.invoke_for_execution("exec-kept", &apply(), &lease, "agent-writer");
    assert!(completed(&record.disposition));
    host.release("exec-kept").unwrap();
    assert_eq!(workspaces_in(staging.path()).len(), 1, "kept, not removed");
    assert!(
        host.live_executions().is_empty(),
        "released from the map regardless"
    );
    // This test's own tree to delete; the registration goes with it.
    for name in workspaces_in(staging.path()) {
        let _ = Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(staging.path().join(name))
            .current_dir(&project)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status();
    }
}

/// A commit that does not complete lands nothing and names nothing: the ref is untouched and the
/// record carries neither field. Here the tree has a conflicting patch applied twice.
#[test]
fn a_commit_that_fails_lands_no_ref_and_names_no_commit() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let host = host(&project, staging.path(), false);
    let lease = writer_lease();
    let execution = "exec-refused";
    // A message over the argv bound is refused by the tool before any git runs.
    let too_long = "x".repeat(513);
    let (record, _) =
        host.invoke_for_execution(execution, &commit(&too_long), &lease, "agent-writer");
    assert!(
        matches!(record.disposition, ToolDisposition::HostError { .. }),
        "{:?}",
        record.disposition
    );
    assert_eq!(record.commit, None);
    assert_eq!(record.landed_ref, None);
    let refs = git(&project, &["for-each-ref", "refs/graphhelm/"]);
    assert!(refs.is_empty(), "{refs}");
    host.release(execution).unwrap();
}

/// An execution id git would refuse in a ref name lands under the digest spelling, and the record
/// names THAT ref — so an auditor never has to guess how the id was mangled.
#[test]
fn a_hostile_execution_id_lands_under_the_digest_ref() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let host = host(&project, staging.path(), false);
    let lease = writer_lease();
    let execution = "exec:with..hostile~bytes";
    let (record, streams) =
        host.invoke_for_execution(execution, &commit("hostile id"), &lease, "agent-writer");
    assert!(
        completed(&record.disposition),
        "{:?} / {}",
        record.disposition,
        String::from_utf8_lossy(&streams.stderr)
    );
    let reference = execution_ref(execution);
    assert!(
        reference.starts_with("refs/graphhelm/executions/sha256-"),
        "{reference}"
    );
    assert_eq!(record.landed_ref.as_deref(), Some(reference.as_str()));
    assert_eq!(
        git(&project, &["rev-parse", &reference]),
        record.commit.clone().unwrap()
    );
    host.release(execution).unwrap();
}

/// A repository in git's SHA-256 object format has 64-hex object ids; the commit still lands and
/// the record still names it (Codex, on #1073: a 40-only parser dropped the commit and the ref).
/// Skips honestly when the installed git cannot initialise such a repository.
#[test]
fn a_sha256_repository_lands_its_commit_and_ref() {
    let probe = tempfile::tempdir().unwrap();
    let supported = Command::new("git")
        .args(["init", "--quiet", "--object-format=sha256"])
        .current_dir(probe.path())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .is_ok_and(|status| status.success());
    if !supported {
        eprintln!("skipped: this git cannot initialise a SHA-256 repository");
        return;
    }
    let (_dir, project) = scratch_repo_with(&["init", "--quiet", "--object-format=sha256"]);
    assert_eq!(git(&project, &["rev-parse", "HEAD"]).len(), 64);
    let staging = tempfile::tempdir().unwrap();
    let host = host(&project, staging.path(), false);
    let lease = writer_lease();
    let execution = "exec-sha256";
    let (record, _) = host.invoke_for_execution(execution, &apply(), &lease, "agent-writer");
    assert!(completed(&record.disposition));
    let (record, streams) =
        host.invoke_for_execution(execution, &commit("sha256 land"), &lease, "agent-writer");
    assert!(
        completed(&record.disposition),
        "{:?} / {}",
        record.disposition,
        String::from_utf8_lossy(&streams.stderr)
    );
    let reference = execution_ref(execution);
    let landed = git(&project, &["rev-parse", &reference]);
    assert_eq!(landed.len(), 64, "{landed}");
    assert_eq!(record.commit.as_deref(), Some(landed.as_str()));
    assert_eq!(record.landed_ref.as_deref(), Some(reference.as_str()));
    assert!(git(&project, &["show", &format!("{reference}:src/lib.rs")]).contains("FIXED"));
    host.release(execution).unwrap();
}

/// Threat model §13 applied to EVERY git spawn, not only provisioning (Codex, on #1073: the
/// project's `pre-commit` ran on `git commit` and `reference-transaction` on `update-ref`). The
/// project carries both hooks, each dropping a marker; the execution applies, commits and lands,
/// and no marker appears.
///
/// Second pass (Codex, on #1073, reproduced on git 2.43): the first fix pointed `core.hooksPath`
/// at the call's redirected HOME, `<tree>/.home` — a directory the execution's own untrusted
/// children write to and that persists across calls. So a shell call of the SAME execution
/// plants an executable `.home/pre-commit` (and `.home/hooks/pre-commit`, for a hooks path
/// one level down) before the commit, and the cell asserts those did not run either. The
/// planting is asserted first: a hook that was never planted proves nothing.
#[test]
fn no_repository_hook_runs_on_commit_or_landing() {
    let (_dir, project) = scratch_repo();
    let hooks = project.join(".git/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let marker = |name: &str| project.join(format!("HOOK-RAN-{name}"));
    let hook_script = |name: &str| {
        format!(
            "#!/bin/sh\ntouch '{}'\n",
            marker(name).display().to_string().replace('\\', "/")
        )
    };
    for hook in [
        "pre-commit",
        "post-commit",
        "reference-transaction",
        "post-checkout",
    ] {
        std::fs::write(hooks.join(hook), hook_script(hook)).unwrap();
    }
    // The hooks are real: run one by hand the way git would, to prove the fixture can fire.
    let probe = Command::new("sh")
        .arg(hooks.join("pre-commit"))
        .current_dir(&project)
        .status();
    if !probe.is_ok_and(|status| status.success()) || !marker("pre-commit").exists() {
        eprintln!("skipped: no `sh` to run the hook fixture on this machine");
        return;
    }
    std::fs::remove_file(marker("pre-commit")).unwrap();

    let staging = tempfile::tempdir().unwrap();
    let host = host(&project, staging.path(), false);
    let mut lease = writer_lease();
    lease.programs.insert("sh".to_owned());
    let execution = "exec-no-hooks";
    let (record, _) = host.invoke_for_execution(execution, &apply(), &lease, "agent-writer");
    assert!(completed(&record.disposition));

    // An untrusted child of the execution plants hooks under its own redirected HOME, through
    // the brokered shell door and before the brokered commit — the arrangement the finding
    // names, not a file written by the test from outside.
    let planted = [
        (".home/pre-commit", "home-pre-commit"),
        (".home/hooks/pre-commit", "home-hooks-pre-commit"),
    ];
    let mut script = String::from("set -e\nmkdir -p .home/hooks\n");
    for (path, name) in planted {
        script.push_str(&format!(
            "printf '%b' '{}' > {path}\nchmod +x {path}\n",
            hook_script(name).replace('\n', "\\n")
        ));
    }
    let plant = ToolCall::Shell(ShellAction {
        program: "sh".to_owned(),
        arguments: vec!["-c".to_owned(), script],
    });
    let (record, streams) = host.invoke_for_execution(execution, &plant, &lease, "agent-writer");
    assert!(
        completed(&record.disposition),
        "the planting shell call must complete: {:?} / {}",
        record.disposition,
        String::from_utf8_lossy(&streams.stderr)
    );
    let tree = staging.path().join(&workspaces_in(staging.path())[0]);
    for (path, name) in planted {
        let planted_hook = tree.join(path);
        assert!(planted_hook.is_file(), "{path} must have been planted");
        // The planted hook is real: run it by hand the way git would, then clear its marker.
        let status = Command::new("sh")
            .arg(&planted_hook)
            .current_dir(&tree)
            .status()
            .unwrap();
        assert!(
            status.success() && marker(name).exists(),
            "{path} must fire"
        );
        std::fs::remove_file(marker(name)).unwrap();
    }

    let (record, streams) =
        host.invoke_for_execution(execution, &commit("hooked land"), &lease, "agent-writer");
    assert!(
        completed(&record.disposition),
        "{:?} / {}",
        record.disposition,
        String::from_utf8_lossy(&streams.stderr)
    );
    assert!(
        record.landed_ref.is_some(),
        "the landing must have happened for the cell to mean anything"
    );
    for hook in [
        "pre-commit",
        "post-commit",
        "reference-transaction",
        "post-checkout",
    ] {
        assert!(
            !marker(hook).exists(),
            "the repository's {hook} hook ran on the execution's behalf"
        );
    }
    for (path, name) in planted {
        assert!(
            !marker(name).exists(),
            "the hook a child planted at {path} ran on the brokered commit"
        );
    }
    host.release(execution).unwrap();
    // No hook-free directory outlives its call.
    assert!(
        workspaces_in(staging.path()).is_empty(),
        "{:?}",
        workspaces_in(staging.path())
    );
}

/// Repository state is untrusted (Codex, on #1073, reproduced): a repository carrying
/// `refs/graphhelm/executions/<id>` as a SYMBOLIC ref aimed at the operator's branch made
/// `update-ref` follow it and move the branch. With `--no-deref` the landing rewrites the
/// execution ref itself into a direct ref at the new commit; the branch stays where it was.
#[test]
fn a_symbolic_execution_ref_is_replaced_and_the_branch_never_moves() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let host = host(&project, staging.path(), false);
    let lease = writer_lease();
    let execution = "exec-symref";
    let reference = execution_ref(execution);
    let branch = git(&project, &["symbolic-ref", "HEAD"]);
    let branch_tip = git(&project, &["rev-parse", "HEAD"]);
    git(&project, &["symbolic-ref", &reference, &branch]);

    let (record, _) = host.invoke_for_execution(execution, &apply(), &lease, "agent-writer");
    assert!(completed(&record.disposition));
    let (record, streams) = host.invoke_for_execution(
        execution,
        &commit("through a symref"),
        &lease,
        "agent-writer",
    );
    assert!(
        completed(&record.disposition),
        "{:?} / {}",
        record.disposition,
        String::from_utf8_lossy(&streams.stderr)
    );
    let landed = record.commit.clone().unwrap();
    assert_eq!(record.landed_ref.as_deref(), Some(reference.as_str()));

    assert_eq!(
        git(&project, &["rev-parse", &branch]),
        branch_tip,
        "the operator's branch must not move"
    );
    assert_eq!(git(&project, &["rev-parse", "HEAD"]), branch_tip);
    assert_eq!(
        git(&project, &["rev-parse", &reference]),
        landed,
        "the execution ref must hold the landed commit"
    );
    let still_symbolic = Command::new("git")
        .args(["symbolic-ref", "--quiet", &reference])
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(
        !still_symbolic.status.success(),
        "the execution ref must be a direct ref now: {}",
        String::from_utf8_lossy(&still_symbolic.stdout)
    );
    host.release(execution).unwrap();
}

/// Two hosts over ONE staging area, probing at once (Codex, on #1073): the serve path builds
/// a host per drive, every host's counter starts at zero, and both first probes named
/// `ghtool-scratch-c0` — one host removed the directory the other had just prepared, that
/// probe answered `false`, and its tree was provisioned from `HEAD` instead of the execution's
/// ref. Two executions land once each; then two fresh hosts, released at a barrier, probe
/// their refs concurrently, and each must continue from its own landing.
#[test]
fn two_hosts_sharing_a_staging_probe_their_refs_at_once() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let lease = writer_lease();
    let executions = ["exec-probe-a", "exec-probe-b"];

    let first = host(&project, staging.path(), false);
    let mut first_commits = Vec::new();
    for execution in executions {
        let (record, _) = first.invoke_for_execution(execution, &apply(), &lease, "agent-writer");
        assert!(completed(&record.disposition));
        let (record, _) =
            first.invoke_for_execution(execution, &commit("first"), &lease, "agent-writer");
        assert!(completed(&record.disposition));
        first_commits.push(record.commit.unwrap());
        first.release(execution).unwrap();
    }
    assert!(workspaces_in(staging.path()).is_empty());

    let barrier = std::sync::Barrier::new(executions.len());
    let records: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = executions
            .iter()
            .map(|execution| {
                let barrier = &barrier;
                let project = &project;
                let staging = staging.path();
                let lease = &lease;
                scope.spawn(move || {
                    let host = host(project, staging, false);
                    barrier.wait();
                    let (record, streams) = host.invoke_for_execution(
                        execution,
                        &commit("second"),
                        lease,
                        "agent-writer",
                    );
                    host.release(execution).unwrap();
                    (
                        record,
                        String::from_utf8_lossy(&streams.stderr).into_owned(),
                    )
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect()
    });
    for ((execution, first_commit), (record, stderr)) in
        executions.iter().zip(&first_commits).zip(&records)
    {
        assert!(
            completed(&record.disposition),
            "{execution}: {:?} / {stderr}",
            record.disposition
        );
        assert_eq!(
            git(
                &project,
                &["rev-parse", &format!("{}^", execution_ref(execution))]
            ),
            *first_commit,
            "{execution}: the second drive must continue from its own ref, not from HEAD"
        );
    }
    assert!(workspaces_in(staging.path()).is_empty());
}

/// A server killed mid-drive leaves the execution's tree and its `.git/worktrees/` registration
/// behind; before #1073 the next drive refused "already exists" forever. Simulated by dropping a
/// host that never released: the next host reclaims the stale tree, provisions afresh from the
/// ref, lands, and the record says `recoveredWorkspace: true`.
#[test]
fn a_stale_tree_from_a_dead_server_is_reclaimed_on_the_next_drive() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let lease = writer_lease();
    let execution = "exec-dead-server";

    let first_commit = {
        let dead = host(&project, staging.path(), false);
        let (record, _) = dead.invoke_for_execution(execution, &apply(), &lease, "agent-writer");
        assert!(completed(&record.disposition));
        let (record, _) = dead.invoke_for_execution(
            execution,
            &commit("before the crash"),
            &lease,
            "agent-writer",
        );
        assert!(completed(&record.disposition));
        assert!(!record.recovered_workspace);
        // No `release`: the process died here.
        record.commit.unwrap()
    };
    assert_eq!(workspaces_in(staging.path()).len(), 1, "the leftover tree");
    let registered = git(&project, &["worktree", "list", "--porcelain"]);
    assert!(
        registered.contains("ghtool-exec-"),
        "the leftover registration: {registered}"
    );

    let next = host(&project, staging.path(), false);
    let (record, streams) = next.invoke_for_execution(
        execution,
        &commit("after the crash"),
        &lease,
        "agent-writer",
    );
    assert!(
        completed(&record.disposition),
        "the next drive must not be refused: {:?} / {}",
        record.disposition,
        String::from_utf8_lossy(&streams.stderr)
    );
    assert!(
        record.recovered_workspace,
        "the record must say the tree was reclaimed"
    );
    let reference = execution_ref(execution);
    assert_eq!(record.landed_ref.as_deref(), Some(reference.as_str()));
    assert_eq!(
        git(&project, &["rev-parse", &format!("{reference}^")]),
        first_commit,
        "the fresh tree must continue from the ref, not from HEAD"
    );
    assert_eq!(
        workspaces_in(staging.path()).len(),
        1,
        "one tree, the reclaimed root reused"
    );
    next.release(execution).unwrap();
    assert!(workspaces_in(staging.path()).is_empty());
    // A second call on the same host is an ordinary call: nothing to reclaim.
    let (record, _) =
        next.invoke_for_execution(execution, &commit("plain"), &lease, "agent-writer");
    assert!(completed(&record.disposition));
    assert!(!record.recovered_workspace);
    next.release(execution).unwrap();
}

/// Landing is a compare-and-swap (#1073, 4a): a ref moved by someone else between provisioning
/// and landing refuses; the refused record still names the commit it made (4b) and no ref.
#[test]
fn a_ref_moved_by_someone_else_refuses_the_landing_and_keeps_the_commit() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let host = host(&project, staging.path(), false);
    let lease = writer_lease();
    let execution = "exec-cas";
    let reference = execution_ref(execution);

    // Provision the tree (the ref does not exist yet: the landing expects "must not exist").
    let (record, _) = host.invoke_for_execution(execution, &apply(), &lease, "agent-writer");
    assert!(completed(&record.disposition));
    // Someone else lands something under the execution's ref meanwhile.
    let head = git(&project, &["rev-parse", "HEAD"]);
    git(&project, &["update-ref", &reference, &head]);

    let (record, _) =
        host.invoke_for_execution(execution, &commit("raced"), &lease, "agent-writer");
    assert!(
        matches!(record.disposition, ToolDisposition::HostError { .. }),
        "{:?}",
        record.disposition
    );
    assert!(
        record.commit.is_some(),
        "the commit exists and the record must say so"
    );
    assert_eq!(record.landed_ref, None);
    assert_eq!(
        git(&project, &["rev-parse", &reference]),
        head,
        "the ref must not be overwritten"
    );
    host.release(execution).unwrap();
}

/// The cancellation span is held for a CALL, not between calls (#1073): after a Tier 1 call
/// returned and its tree was kept for the execution, `cancel_all` must return at once — it used
/// to wait the signal's full grace (30 s) on a tree with nothing running in it, which is what an
/// immediate pause after a tool node paid.
#[test]
fn cancel_all_returns_at_once_between_calls_of_a_kept_workspace() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let host = host(&project, staging.path(), false);
    let lease = writer_lease();
    let (record, _) = host.invoke_for_execution("exec-parked", &apply(), &lease, "agent-writer");
    assert!(completed(&record.disposition));
    assert_eq!(workspaces_in(staging.path()).len(), 1, "the tree is kept");
    let started = std::time::Instant::now();
    host.cancel_all();
    let waited = started.elapsed();
    assert!(
        waited < Duration::from_secs(5),
        "cancel_all waited {waited:?} on a kept tree with no call in flight"
    );
    // A call after the cancellation is refused fail-closed (the signal is raised), and the
    // tree is still the execution's to release.
    let (record, _) = host.invoke_for_execution(
        "exec-parked",
        &commit("after cancel"),
        &lease,
        "agent-writer",
    );
    assert!(
        matches!(record.disposition, ToolDisposition::HostError { .. }),
        "{:?}",
        record.disposition
    );
    host.release("exec-parked").unwrap();
    assert!(workspaces_in(staging.path()).is_empty());
}

/// A registration whose directory is gone (a provision cancelled or killed after git registered
/// the path) is reclaimed on the next provision (#1073): before, `git worktree add` refused the
/// deterministic root as "missing but already registered" until an operator pruned by hand.
#[test]
fn a_registration_without_a_directory_is_reclaimed_on_the_next_provision() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let lease = writer_lease();
    let execution = "exec-registered-only";
    {
        let dead = host(&project, staging.path(), false);
        let (record, _) = dead.invoke_for_execution(execution, &apply(), &lease, "agent-writer");
        assert!(completed(&record.disposition));
        // The directory vanishes, the registration stays — the shape a killed provision leaves.
        for name in workspaces_in(staging.path()) {
            std::fs::remove_dir_all(staging.path().join(name)).unwrap();
        }
    }
    assert!(workspaces_in(staging.path()).is_empty());
    let registered = git(&project, &["worktree", "list", "--porcelain"]);
    assert!(
        registered.contains("ghtool-exec-"),
        "the registration must survive for the cell to mean anything: {registered}"
    );

    let next = host(&project, staging.path(), false);
    let (record, streams) = next.invoke_for_execution(
        execution,
        &commit("after the ghost"),
        &lease,
        "agent-writer",
    );
    assert!(
        completed(&record.disposition),
        "{:?} / {}",
        record.disposition,
        String::from_utf8_lossy(&streams.stderr)
    );
    assert!(
        record.recovered_workspace,
        "the record must say the registration was reclaimed"
    );
    assert!(record.landed_ref.is_some());
    next.release(execution).unwrap();
    let after = git(&project, &["worktree", "list", "--porcelain"]);
    assert!(!after.contains("ghtool-exec-"), "{after}");
}

/// A commit whose object id cannot be read back is a LANDING FAILURE (#1073), never a completed
/// commit node that published nothing. The readback shares the call's output cap, so a cap
/// below an object id's width makes `rev-parse` truncate deterministically.
#[test]
fn a_commit_whose_id_cannot_be_read_back_is_a_landing_failure() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let tool_dir = Path::new(env!("CARGO_BIN_EXE_fake_tool"))
        .parent()
        .unwrap()
        .to_path_buf();
    let host = ToolHost::new(HostConfig {
        workspace: WorkspaceConfig::validated(&project, staging.path(), &[]).unwrap(),
        limits: ProcessLimits {
            timeout: Duration::from_secs(30),
            // Below an object id's 40 bytes: `git commit --quiet` and `git add` print nothing,
            // so the commit itself completes and only the readback truncates.
            max_output_bytes: 16,
        },
        tests_runner: "fake_tool".to_owned(),
        tests_runner_env: BTreeMap::new(),
        path_prepend: vec![tool_dir],
        keep_workspace: false,
    });
    let lease = writer_lease();
    let execution = "exec-unreadable-id";
    let (record, _) = host.invoke_for_execution(execution, &apply(), &lease, "agent-writer");
    assert!(completed(&record.disposition));
    let (record, _) =
        host.invoke_for_execution(execution, &commit("unreadable"), &lease, "agent-writer");
    assert!(
        matches!(record.disposition, ToolDisposition::HostError { .. }),
        "a commit with no readable id must not record as completed: {:?}",
        record.disposition
    );
    assert_eq!(record.commit, None);
    assert_eq!(record.landed_ref, None);
    let refs = git(&project, &["for-each-ref", "refs/graphhelm/"]);
    assert!(
        refs.is_empty(),
        "nothing may land for an unreadable id: {refs}"
    );
    host.release(execution).unwrap();
}
