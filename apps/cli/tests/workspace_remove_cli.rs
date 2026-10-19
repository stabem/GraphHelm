//! #1663: `workspace remove <dir>` deletes a worktree or a directory without following a junction.
//! Credible regression: a cleanup recurses through a worktree's `apps/studio/node_modules`
//! junction and deletes the owner checkout's files. Cost: a temp repository, one worktree, two
//! CLI runs; no network.
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

fn s(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

fn remove(path: &Path, leftovers: &Path) -> Value {
    let out = graphhelm_process::command(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args([
            "--json",
            "workspace",
            "remove",
            &s(path),
            "--leftovers",
            &s(leftovers),
        ])
        .output()
        .unwrap();
    let reply: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(reply["data"]["ok"], true, "{reply}");
    reply
}

/// A sentinel directory with files, standing in for the owner checkout's `node_modules`.
fn sentinel(at: &Path) -> PathBuf {
    std::fs::create_dir_all(at.join("pkg")).unwrap();
    std::fs::write(at.join("pkg").join("index.js"), "keep\n").unwrap();
    std::fs::write(at.join("top.txt"), "keep\n").unwrap();
    at.to_path_buf()
}

fn intact(at: &Path) -> bool {
    at.join("pkg").join("index.js").is_file() && at.join("top.txt").is_file()
}

#[test]
fn remove_deletes_a_worktree_and_a_directory_without_following_their_junctions() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let base = PathBuf::from(s(&base).trim_start_matches("//?/"));
    let repo = base.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join(".gitignore"), "node_modules\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "init"]);

    // A worktree whose ignored apps/studio/node_modules is a junction to the sentinel.
    let wt = base.join("wt");
    git(
        &repo,
        &["worktree", "add", "-q", "-b", "pr", &s(&wt), "main"],
    );
    std::fs::create_dir_all(wt.join("apps").join("studio")).unwrap();
    let owner_modules = sentinel(&base.join("owner").join("node_modules"));
    link_dir(
        &wt.join("apps").join("studio").join("node_modules"),
        &owner_modules,
    );

    let reply = remove(&wt, &base.join("leftovers"));
    assert!(
        intact(&owner_modules),
        "the worktree junction was followed: {reply}"
    );
    assert!(!wt.exists(), "the worktree folder stayed: {reply}");
    assert_eq!(reply["data"]["worktree"], true, "{reply}");
    assert!(
        !git(&repo, &["worktree", "list", "--porcelain"]).contains("/wt"),
        "the worktree is still registered"
    );

    // A plain directory (a review checkout copy, a build target) holding a nested junction.
    let plain = base.join("plain");
    std::fs::create_dir_all(plain.join("a").join("b")).unwrap();
    let other = sentinel(&base.join("other"));
    link_dir(&plain.join("a").join("b").join("link"), &other);
    let reply = remove(&plain, &base.join("leftovers"));
    assert!(
        intact(&other),
        "the directory junction was followed: {reply}"
    );
    assert!(!plain.exists(), "the directory stayed: {reply}");

    // The main checkout is refused and untouched.
    let out = graphhelm_process::command(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["--json", "workspace", "remove", &s(&repo)])
        .output()
        .unwrap();
    assert!(!out.status.success(), "{out:?}");
    assert!(repo.join(".git").exists());
}
