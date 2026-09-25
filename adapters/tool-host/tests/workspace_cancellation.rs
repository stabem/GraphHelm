//! #617: a cancellation reaches the workspace's own subprocesses, and WAITS for the teardown.
//!
//! `Tier1Workspace::provision` and `remove` spawned `git` through `Command::status`, outside
//! `CancelSignal` entirely. `Command::status` blocks, so there was no poll loop in which to notice
//! a signal — and, the half that actually bites, `ToolHost::cancel_all` could return while a
//! cleanup child was still to come.
//!
//! **The reachable half is CLEANUP, not provisioning**, and the issue says why: provisioning
//! precedes the call a caller would cancel, so a cancellation reaching it arrived before the tool
//! ran at all. Cleanup runs AFTER the tool child has been killed, which is exactly when a caller
//! that just cancelled is waiting for `cancel` to return. So the cell that decides this is a
//! cancel-during-teardown one, and it is the first below.
//!
//! Counted per child, the in-flight count reached zero when the tool child was reaped, `cancel`
//! returned, and the `git worktree remove` spawned afterwards — the promise "a cancelled execution
//! leaves no live child" kept by the letter and broken by the clock. The fix is a counted SPAN
//! from provision to removal.

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use graphhelm_tool_host::process::{CancelSignal, HostError};
use graphhelm_tool_host::workspace::{Tier1Workspace, WorkspaceConfig};

/// Same idiom as `tests/workspace_containment.rs`: a throwaway repository with one commit. The
/// harness may spawn what it likes -- the argv discipline binds the broker and the host, not this.
fn scratch_repo() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join("src/lib.rs"), "// scratch\n").unwrap();
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(&project)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "scratch")
            .env("GIT_AUTHOR_EMAIL", "scratch@test.invalid")
            .env("GIT_COMMITTER_NAME", "scratch")
            .env("GIT_COMMITTER_EMAIL", "scratch@test.invalid")
            .status()
            .unwrap();
        assert!(status.success(), "HARNESS-BROKE: git {args:?} failed");
    };
    git(&["init", "--quiet"]);
    git(&["add", "-A"]);
    git(&["commit", "--quiet", "-m", "scratch"]);
    (dir, project)
}

/// How long the teardown is deliberately delayed. `cancel` must not return inside this window.
const TEARDOWN_DELAY: Duration = Duration::from_millis(500);

/// The #617 cell: `cancel` waits for the teardown its own cancellation caused.
///
/// The assertion is a ONE-SIDED bound — `cancel` returned no earlier than the delay — and that is
/// deliberate. A lower bound cannot flake on a loaded host: load only makes the return later, and
/// later still passes. An upper bound here would be a duration assertion on a machine four lanes
/// are building on, which is how a correct change gets a red.
///
/// Before #617 this is red at its own assertion: with no counted span the in-flight count is zero
/// the moment `cancel` looks, so it returns in microseconds while the `git worktree remove` has
/// not been spawned yet.
#[test]
fn cancel_waits_for_the_teardown_its_own_cancellation_caused() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let config = WorkspaceConfig::validated(&project, staging.path(), &[]).unwrap();

    let signal = CancelSignal::new();
    let workspace = Tier1Workspace::provision(&config, "call-1", Some(&signal))
        .expect("HARNESS-BROKE: the workspace was not provisioned, so there is no span to wait on");
    let root = workspace.root().to_path_buf();
    assert!(
        root.join("src/lib.rs").is_file(),
        "HARNESS-BROKE: the worktree does not carry the project, so the removal below has nothing \
         real to tear down"
    );

    let started = Instant::now();
    let teardown = std::thread::spawn(move || {
        // Stands in for the work between the tool child's reap and the removal. In production
        // that gap is the rest of `invoke`; here it is a sleep, because the span is what is
        // under test and not what fills it.
        std::thread::sleep(TEARDOWN_DELAY);
        workspace.remove()
    });

    signal.cancel();
    let waited = started.elapsed();
    // Read AT the instant cancel returned, before anything can tidy up afterwards.
    //
    // The bound above alone does not discriminate WHERE inside the teardown the span is
    // released: a version that dropped the hold on entry to `remove` still waits out the delay
    // and still passes it. Measured, not reasoned -- that sabotage was applied and the cell went
    // green. This is the assertion that separates them, and its margin is the whole
    // `git worktree remove`, which is milliseconds against a condvar wake of microseconds.
    let gone_when_cancel_returned = !root.exists();

    teardown
        .join()
        .expect("the teardown thread returns")
        .expect("the workspace is removed");

    assert!(
        waited >= TEARDOWN_DELAY,
        "cancel returned after {waited:?}, inside the {TEARDOWN_DELAY:?} window in which the \
         teardown had not even been spawned: the cancellation does not wait for the cleanup it \
         caused"
    );
    assert!(!root.exists(), "the workspace outlives nothing");
    assert!(
        gone_when_cancel_returned,
        "cancel returned while {} still existed: the span was released before the teardown finished, so the wait covers the delay in front of it and not the removal itself",
        root.display()
    );
}

/// A signal already raised refuses a NEW provision rather than building a workspace for a call
/// that will not run.
///
/// Fail-closed, and the same answer `CancelSignal::attach` gives a spawn. Deterministic: it turns
/// on the flag, not on a clock.
#[test]
fn a_raised_signal_refuses_a_new_provision() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let config = WorkspaceConfig::validated(&project, staging.path(), &[]).unwrap();

    let signal = CancelSignal::new();
    signal.cancel();

    let refused = Tier1Workspace::provision(&config, "call-1", Some(&signal));

    // `err()` rather than `{refused:?}`: `Tier1Workspace` is not `Debug`, and making it so to
    // print a value this assertion only ever sees on failure would widen the type's surface for
    // the sake of a message. The refusal itself is what the message needs.
    let refused = refused.map(|_| ()).err();
    assert!(
        matches!(refused, Some(HostError::Cancelled)),
        "a raised signal must refuse the provision, got {refused:?}"
    );
    assert!(
        !staging.path().join("ghtool-call-1").exists(),
        "the refusal must leave no workspace directory behind"
    );
    assert!(
        !staging.path().join("ghtool-call-1-nohooks").exists(),
        "the refusal must leave no scratch directory behind either -- staging is empty again \
         after a call, refused or not"
    );
}

/// CONTROL: without a signal the lifecycle is what it was.
///
/// Without this, a `provision` that refused everything would satisfy the cell above by refusing,
/// and a `remove` that never removed would satisfy it by returning early.
#[test]
fn without_a_signal_the_lifecycle_is_unchanged() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let config = WorkspaceConfig::validated(&project, staging.path(), &[]).unwrap();

    let workspace = Tier1Workspace::provision(&config, "call-1", None).expect("no signal, no gate");
    let root = workspace.root().to_path_buf();
    assert!(root.join("src/lib.rs").is_file());
    workspace.remove().expect("the workspace is removed");
    assert!(!root.exists());

    let listed = Command::new("git")
        .args(["worktree", "list"])
        .current_dir(&project)
        .output()
        .expect("git worktree list runs");
    let listed = String::from_utf8_lossy(&listed.stdout);
    assert_eq!(
        listed.lines().count(),
        1,
        "the project must hold no stale worktree registration, listed:\n{listed}"
    );
}

/// The unreachable-today twin, labelled rather than dressed up as coverage.
///
/// `provision`'s own `git worktree add` is now INTERRUPTIBLE: a cancellation arriving while it
/// runs kills the tree and the call returns `HostError::Cancelled`. There is no cell for that
/// arm, and the reason is that the window cannot be entered reliably — `git worktree add` on a
/// one-commit repository finishes in milliseconds, so a cancellation aimed at it either arrives
/// before the spawn (which is the cell above, a different code path) or after it has already
/// exited. A cell that raced for that window would pass by luck and fail by load, which is worse
/// than no cell.
///
/// What IS measured about that arm: the refusal path above, and `run_supervised`'s poll loop,
/// which is the same loop `run_in_workspace` uses and which `tests/process_isolation.rs` covers
/// through the tool child.
///
/// It stays here as a named gap so the next person does not read the green above as covering it.
#[test]
#[ignore = "no reliable way to enter the mid-provision window; see the doc comment"]
fn a_cancellation_mid_provision_has_no_deterministic_cell() {}
