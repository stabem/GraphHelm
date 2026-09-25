use std::path::PathBuf;
use std::process::Command;

use graphhelm_tool_broker::path::RelativePath;
use graphhelm_tool_host::workspace::{Tier1Workspace, WorkspaceConfig};

fn rel(text: &str) -> RelativePath {
    RelativePath::parse(text).unwrap()
}

/// Builds a throwaway git repository with one commit carrying `src/lib.rs`. The test harness
/// may spawn what it likes (the argv discipline binds the broker and host, not the harness).
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
        assert!(status.success(), "git {args:?} failed");
    };
    git(&["init", "--quiet"]);
    git(&["add", "-A"]);
    git(&["commit", "--quiet", "-m", "scratch"]);
    (dir, project)
}

#[test]
fn provision_creates_a_detached_worktree_and_remove_cleans_it_up() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let config = WorkspaceConfig::validated(&project, staging.path(), &[]).unwrap();
    let workspace = Tier1Workspace::provision(&config, "call-1", None).unwrap();
    assert!(
        workspace.root().join("src/lib.rs").is_file(),
        "the worktree carries the project"
    );
    assert!(
        workspace
            .root()
            .starts_with(staging.path().canonicalize().unwrap())
    );
    let root = workspace.root().to_path_buf();
    workspace.remove().unwrap();
    assert!(!root.exists(), "the workspace outlives nothing");
    // And the project repository holds no stale worktree registration: `git worktree list`
    // names exactly one tree (the project itself).
    let listed = Command::new("git")
        .args(["worktree", "list"])
        .current_dir(&project)
        .output()
        .unwrap();
    let lines = String::from_utf8_lossy(&listed.stdout);
    assert_eq!(
        lines.lines().count(),
        1,
        "stale worktree registration left behind: {lines}"
    );
}

#[test]
fn a_staging_directory_inside_the_project_is_refused() {
    let (_dir, project) = scratch_repo();
    assert!(WorkspaceConfig::validated(&project, &project.join(".ghtool"), &[]).is_err());
}

#[test]
fn a_workspace_never_contains_or_equals_a_sensitive_directory() {
    // The structural half of the hard constraint: the caller passes its keyring/broker/events
    // directories as protected, and the constructor is the only door — a staging area that
    // equals, contains, or is contained by any protected path never becomes a WorkspaceConfig.
    let (_dir, project) = scratch_repo();
    let keyring = tempfile::tempdir().unwrap();
    let inside = keyring.path().join("staging");
    assert!(
        WorkspaceConfig::validated(&project, &inside, &[keyring.path().to_path_buf()]).is_err()
    );
    let around = keyring.path().parent().unwrap();
    assert!(
        WorkspaceConfig::validated(&project, around, &[keyring.path().to_path_buf()]).is_err(),
        "a staging area CONTAINING the keyring must be refused too"
    );
}

#[test]
fn provisioning_never_runs_repository_hooks() {
    // Threat model §13: "disable hooks by default". `git worktree add` runs the repository's
    // post-checkout hook; a hostile project must not get code execution out of being
    // provisioned.
    let (_dir, project) = scratch_repo();
    let hooks = project.join(".git/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let marker = project.join("HOOK-RAN");
    // A portable-enough hook: git-for-windows executes hooks through its own sh.
    std::fs::write(
        hooks.join("post-checkout"),
        format!(
            "#!/bin/sh\ntouch '{}'\n",
            marker.display().to_string().replace('\\', "/")
        ),
    )
    .unwrap();
    let staging = tempfile::tempdir().unwrap();
    let config = WorkspaceConfig::validated(&project, staging.path(), &[]).unwrap();
    let workspace = Tier1Workspace::provision(&config, "call-1", None).unwrap();
    assert!(
        !marker.exists(),
        "the repository's post-checkout hook ran during provisioning"
    );
    workspace.remove().unwrap();
}

#[test]
fn resolve_contains_paths_and_refuses_a_junction_escape() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let config = WorkspaceConfig::validated(&project, staging.path(), &[]).unwrap();
    let workspace = Tier1Workspace::provision(&config, "call-1", None).unwrap();

    let fine = workspace.resolve(&rel("src/lib.rs")).unwrap();
    assert!(fine.starts_with(workspace.root()));

    // A directory junction does not need Windows privileges (symlinks do): create
    // workspace/escape -> outside, then ask for escape/x.txt. The junction is the test's own
    // fixture; creating it via cmd is harness territory, not broker territory. Unix has no
    // junction; a directory symlink is the same escape there.
    let junction = workspace.root().join("escape");
    #[cfg(windows)]
    {
        let status = Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(&junction)
            .arg(outside.path())
            .status()
            .unwrap();
        assert!(
            status.success(),
            "junction creation is the test's own precondition"
        );
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.path(), &junction)
        .expect("symlink creation is the test's own precondition");
    assert!(
        workspace.resolve(&rel("escape/x.txt")).is_err(),
        "junction escape must be refused"
    );

    workspace.remove().unwrap();
}

#[test]
fn a_malformed_call_id_is_refused_before_any_filesystem_action() {
    // Review finding 3: the id flows into a path join; "../x" must never become a staging
    // escape even though today's only caller is internal.
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let config = WorkspaceConfig::validated(&project, staging.path(), &[]).unwrap();
    for bad in ["", "../x", "UPPER", "a b", "x/y", &"a".repeat(65)] {
        assert!(
            Tier1Workspace::provision(&config, bad, None).is_err(),
            "{bad:?} must be refused"
        );
    }
    assert_eq!(
        std::fs::read_dir(staging.path()).unwrap().count(),
        0,
        "a refused id must leave staging untouched"
    );
}

#[test]
fn provision_leaves_nothing_in_staging_but_the_workspace_itself() {
    // Review finding 2: the no-hooks scratch dir must not outlive worktree add — the Task 7
    // broker asserts staging is EMPTY after a completed call, so nothing here may linger.
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let config = WorkspaceConfig::validated(&project, staging.path(), &[]).unwrap();
    let workspace = Tier1Workspace::provision(&config, "call-1", None).unwrap();
    let entries: Vec<_> = std::fs::read_dir(staging.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(entries, vec!["ghtool-call-1"], "stray entries: {entries:?}");
    workspace.remove().unwrap();
    assert_eq!(
        std::fs::read_dir(staging.path()).unwrap().count(),
        0,
        "staging must be empty after remove"
    );
}

#[test]
fn resolve_accepts_a_not_yet_existing_file_inside_the_workspace() {
    // Write targets do not exist yet; resolve must vet the existing ancestor chain and still
    // produce the joined path for the host's write primitives.
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let config = WorkspaceConfig::validated(&project, staging.path(), &[]).unwrap();
    let workspace = Tier1Workspace::provision(&config, "call-1", None).unwrap();
    let target = workspace.resolve(&rel("src/new_file.rs")).unwrap();
    assert!(target.starts_with(workspace.root()));
    assert!(!target.exists());
    workspace.remove().unwrap();
}

/// A configured checkout FILTER runs during `git worktree add`, and it must not see the host's
/// environment (#491).
///
/// **The hostile arrangement is the one the ticket names, not a proxy for it.** `core.hooksPath`
/// pointed at an empty directory closes the HOOK door; a `filter.<driver>.smudge` is a different
/// door and it opens during the checkout `worktree add` performs. The filter is an arbitrary
/// program, configured by the project rather than by us, and before this fix it inherited whatever
/// the parent process happened to hold.
///
/// The filter here writes its own environment into the new worktree and passes the content
/// through, so the leak is observable as a file rather than inferred.
///
/// **The ARRANGEMENT assertion is what stops this passing vacuously**, and it is the whole reason
/// the cell is worth having: a filter that never ran leaves no file, and "the probe is not in a
/// file that does not exist" is true of every possible implementation. So the file must exist
/// FIRST, and only then is its content a verdict.
#[test]
fn a_checkout_filter_does_not_see_the_hosts_environment() {
    const PROBE: &str = "GH_TOOLHOST_LEAK_PROBE";
    const SECRET: &str = "a-value-no-filter-should-ever-read";

    let (_dir, project) = scratch_repo();
    // The filter is configured in the PROJECT's own repository, which is the threat model: the
    // repository being provisioned is untrusted.
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
    std::fs::write(project.join(".gitattributes"), "src/lib.rs filter=leak\n").unwrap();
    // Runs with the new worktree as its working directory, so the relative path lands there.
    // `cat` passes the blob through, so the checkout still produces the real file.
    git(&["config", "filter.leak.smudge", "env > leaked.txt; cat"]);
    git(&["add", "-A"]);
    git(&["commit", "--quiet", "-m", "attributes"]);

    // SAFETY: single-threaded at this point in the cell, and the value is removed below.
    unsafe { std::env::set_var(PROBE, SECRET) };
    let staging = tempfile::tempdir().unwrap();
    let config = WorkspaceConfig::validated(&project, staging.path(), &[]).unwrap();
    let workspace = Tier1Workspace::provision(&config, "call-1", None).unwrap();
    let leaked = workspace.root().join("leaked.txt");
    let dumped = std::fs::read_to_string(&leaked).ok();
    unsafe { std::env::remove_var(PROBE) };

    let dumped = dumped.unwrap_or_else(|| {
        panic!(
            "HARNESS-BROKE: the smudge filter never ran, so {leaked:?} does not exist and the \
             absence below would be true of every implementation, including one that leaks"
        )
    });
    assert!(
        !dumped.is_empty(),
        "HARNESS-BROKE: the filter ran but dumped nothing, so this cell has no environment to \
         inspect"
    );
    assert!(
        !dumped.contains(PROBE),
        "the checkout filter saw the host's environment: {PROBE} reached a program the project \
         configured, and a filter is arbitrary code chosen by the repository being provisioned"
    );
    workspace.remove().unwrap();
}
