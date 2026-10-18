//! #1578: `workspace sweep --all` cleans the whole machine in one command. Credible regressions:
//! a live, locked or unmerged-and-unpushed worktree is deleted; dirty work of a merged PR is lost;
//! a junction is followed out of a worktree or a build cache; the dry run changes something, or
//! lists something apply would not remove. Cost: a temp repository with a bare origin, one hidden
//! child process and two CLI runs; no network (`--pr-states` stands in for `gh`).
use std::path::{Path, PathBuf};

use serde_json::Value;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = graphhelm_process::command("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn link_dir(link: &Path, target: &Path) {
    #[cfg(windows)]
    {
        let out = graphhelm_process::command("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link.to_string_lossy().replace('/', "\\"))
            .arg(target.to_string_lossy().replace('/', "\\"))
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
    }
    #[cfg(not(windows))]
    std::os::unix::fs::symlink(target, link).unwrap();
}

/// A long-lived hidden child whose current directory is `dir`.
fn hold(dir: &Path) -> std::process::Child {
    #[cfg(windows)]
    let mut command = {
        let mut c = graphhelm_process::command("ping");
        c.args(["-n", "120", "127.0.0.1"]);
        c
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut c = graphhelm_process::command("sleep");
        c.arg("120");
        c
    };
    command
        .current_dir(dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap()
}

fn s(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

fn commit(dir: &Path, file: &str, message: &str) {
    std::fs::write(dir.join(file), format!("{file}\n")).unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", message]);
}

fn add_worktree(repo: &Path, path: &Path, branch: &str) {
    git(
        repo,
        &["worktree", "add", "-q", "-b", branch, &s(path), "main"],
    );
}

fn sweep(repo: &Path, states: &Path, root: &Path, leftovers: &Path, apply: bool) -> Value {
    let mut command = graphhelm_process::command(assert_cmd::cargo::cargo_bin!("graphhelm"));
    command.args(["--json", "workspace", "sweep", "--all"]);
    if apply {
        command.arg("--apply");
    }
    let out = command
        .args(["--repo", &s(repo), "--pr-states", &s(states)])
        .args(["--sweep-root", &s(root), "--idle-hours", "0"])
        .args(["--leftovers", &s(leftovers)])
        .output()
        .unwrap();
    let reply: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(reply["data"]["ok"], true, "{reply}");
    reply
}

#[test]
fn sweep_all_removes_finished_work_and_keeps_live_locked_and_unpushed_work() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let base = PathBuf::from(s(&base).trim_start_matches("//?/"));
    let origin = base.join("origin.git");
    let repo = base.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&base, &["init", "-q", "--bare", "-b", "main", &s(&origin)]);
    git(&repo, &["init", "-q", "-b", "main", "--object-format=sha1"]);
    git(&repo, &["remote", "add", "origin", &s(&origin)]);
    commit(&repo, "a.txt", "init");

    let merged = base.join("wt-merged");
    let closed = base.join("wt-closed");
    let dirty = base.join("wt-dirty");
    let live = base.join("wt-live");
    let unpushed = base.join("wt-unpushed");
    let locked = base.join("wt-locked");
    let reused = base.join("wt-reused");
    for (path, branch) in [
        (&merged, "issue-1-merged"),
        (&closed, "issue-2-closed"),
        (&dirty, "issue-3-dirty"),
        (&live, "issue-4-live"),
        (&unpushed, "issue-5-wip"),
        (&locked, "issue-6-locked"),
        (&reused, "issue-7-reused"),
    ] {
        add_worktree(&repo, path, branch);
        commit(path, &format!("{branch}.txt"), branch);
    }
    // The finished PRs' branches were pushed; issue-7's PR is merged too, but its branch name was
    // reused for a commit that is on no remote and not in main (#1620). issue-8 is the same with
    // no worktree.
    for branch in ["issue-2-closed", "issue-3-dirty"] {
        git(&repo, &["push", "-q", "origin", branch]);
    }
    git(&repo, &["branch", "issue-8-orphan", "issue-7-reused"]);
    // issue-1 is squash-merged with no PR state recorded: only its content proves it.
    commit(&repo, "issue-1-merged.txt", "issue 1 (#1)");
    git(&repo, &["push", "-q", "origin", "main"]);
    git(&repo, &["fetch", "-q", "origin"]);
    let states = base.join("states.json");
    std::fs::write(
        &states,
        r#"{"issue-2-closed":"CLOSED","issue-3-dirty":"MERGED","issue-4-live":"MERGED","issue-6-locked":"MERGED","issue-7-reused":"MERGED","issue-8-orphan":"MERGED"}"#,
    )
    .unwrap();

    // Dirty work in a merged PR's worktree, and a junction out of the merged worktree.
    std::fs::write(dirty.join("wip.txt"), "unsaved\n").unwrap();
    let outside = base.join("owner-node-modules");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("keep.txt"), "owner\n").unwrap();
    link_dir(&merged.join("node_modules"), &outside);
    let mut holder = hold(&live);
    git(
        &repo,
        &[
            "worktree",
            "lock",
            "--reason",
            "claude agent t",
            &s(&locked),
        ],
    );

    // A target root: one idle build cache holding a junction, one note that is not a cache.
    let root = base.join("ghc");
    let cache = root.join("lane-a").join("wt-gone").join("target");
    std::fs::create_dir_all(cache.join("debug")).unwrap();
    std::fs::write(
        cache.join("CACHEDIR.TAG"),
        "Signature: 8a477f597d28d172789f06886806bc55",
    )
    .unwrap();
    std::fs::write(cache.join("debug").join("x"), "x").unwrap();
    let outside_cache = base.join("owner-cache");
    std::fs::create_dir_all(&outside_cache).unwrap();
    std::fs::write(outside_cache.join("keep.txt"), "owner\n").unwrap();
    link_dir(&cache.join("debug").join("deps"), &outside_cache);
    std::fs::create_dir_all(root.join("notes")).unwrap();
    std::fs::write(root.join("notes").join("n.md"), "keep\n").unwrap();
    let leftovers = base.join("leftovers");

    let preview = sweep(&repo, &states, &root, &leftovers, false);
    for path in [&merged, &closed, &dirty, &cache] {
        assert!(path.exists(), "the dry run removed {}: {preview}", s(path));
    }
    let applied = sweep(&repo, &states, &root, &leftovers, true);
    let alive = holder.try_wait().unwrap().is_none();
    let _ = holder.kill();
    let _ = holder.wait();
    let listed = git(&repo, &["worktree", "list", "--porcelain"]).replace('\\', "/");
    git(&repo, &["worktree", "unlock", &s(&locked)]);
    assert_eq!(
        preview["data"]["counts"]["worktreesRemoved"],
        applied["data"]["counts"]["worktreesRemoved"]
    );
    assert_eq!(
        preview["data"]["counts"]["cachesRemoved"],
        applied["data"]["counts"]["cachesRemoved"]
    );

    for gone in [&merged, &closed, &dirty] {
        assert!(!gone.exists(), "{} survived: {applied}", s(gone));
        assert!(!listed.contains(&s(gone)), "{} still registered", s(gone));
    }
    let branches = git(&repo, &["branch", "--list"]);
    for gone in ["issue-1-merged", "issue-2-closed", "issue-3-dirty"] {
        assert!(!branches.contains(gone), "branch {gone} survived");
    }
    let saved = leftovers.join("wt-dirty").join("untracked").join("wip.txt");
    assert_eq!(std::fs::read_to_string(saved).unwrap(), "unsaved\n");

    assert!(alive, "the live worktree's process was stopped: {applied}");
    for kept in [&live, &unpushed, &locked, &reused] {
        assert!(
            kept.join("a.txt").is_file(),
            "{} was removed: {applied}",
            s(kept)
        );
        assert!(listed.contains(&s(kept)), "{} unregistered", s(kept));
    }
    assert_eq!(applied["data"]["worktrees"]["kept"]["live"], 1, "{applied}");
    assert_eq!(
        applied["data"]["worktrees"]["kept"]["unpushed"], 2,
        "{applied}"
    );
    assert_eq!(applied["data"]["counts"]["branchesKept"], 1, "{applied}");
    assert!(
        branches.contains("issue-7-reused") && branches.contains("issue-8-orphan"),
        "a branch with unpushed, unmerged commits was deleted: {applied}"
    );
    assert_eq!(applied["data"]["counts"]["worktreesLocked"], 1, "{applied}");
    assert!(
        listed.contains("locked"),
        "the locked worktree was unlocked by the sweep"
    );

    assert!(!cache.exists(), "the idle build cache survived: {applied}");
    assert!(!root.join("lane-a").exists(), "an emptied lane dir stayed");
    assert!(
        root.join("notes").join("n.md").is_file(),
        "a note was touched"
    );
    assert!(
        outside.join("keep.txt").is_file(),
        "the worktree junction was followed"
    );
    assert!(
        outside_cache.join("keep.txt").is_file(),
        "the cache junction was followed"
    );
}
