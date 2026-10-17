//! #1456: after a merge the merger runs one command that removes everything made only for that
//! PR. Credible regressions: a held worktree survives (Windows refuses to delete a directory a
//! process sits in), a junction is followed out of the worktree, dirty work is lost, an unrelated
//! worktree or a lane's reused review worktree is deleted, or the branch is left behind. Cost: a
//! temp git repository, one hidden child process and one CLI run; no network.
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

fn worktree_paths(repo: &Path) -> String {
    git(repo, &["worktree", "list", "--porcelain"]).replace('\\', "/")
}

fn s(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

#[test]
fn workspace_merged_cleanup_removes_the_prs_worktrees_and_keeps_everything_else() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let base = PathBuf::from(s(&base).trim_start_matches("//?/"));
    let repo = base.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main", "--object-format=sha1"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    git(&repo, &["branch", "other"]);

    let author = base.join("wt-author");
    let review = base.join("wt-review");
    let reused = base.join("wt-reused");
    let unrelated = base.join("wt-other");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "issue-9-x",
            &s(&author),
            "main",
        ],
    );
    std::fs::write(author.join("b.txt"), "b\n").unwrap();
    git(&author, &["add", "-A"]);
    git(&author, &["commit", "-q", "-m", "feature"]);
    let head = git(&author, &["rev-parse", "HEAD"]);
    // Squash-merged: main gets the same change as a new commit.
    std::fs::write(repo.join("b.txt"), "b\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "feature (#9)"]);
    let main_head = git(&repo, &["rev-parse", "HEAD"]);
    git(
        &repo,
        &["worktree", "add", "-q", "--detach", &s(&review), &head],
    );
    git(
        &repo,
        &["worktree", "add", "-q", "--detach", &s(&reused), &head],
    );
    git(&repo, &["worktree", "add", "-q", &s(&unrelated), "other"]);

    // Uncommitted work in the author's worktree, and a junction pointing outside it.
    std::fs::write(author.join("wip.txt"), "unsaved\n").unwrap();
    let outside = base.join("owner-node-modules");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("keep.txt"), "owner\n").unwrap();
    link_dir(&review.join("node_modules"), &outside);
    let target = base.join("t9");
    let shared = base.join("shared-target");
    for t in [&target, &shared] {
        std::fs::create_dir_all(t.join("debug")).unwrap();
        std::fs::write(t.join("debug").join("x"), "x").unwrap();
    }
    let mut holder = hold(&review);
    let leftovers = base.join("leftovers");

    let out = graphhelm_process::command(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args([
            "--json",
            "workspace",
            "sweep",
            "--apply",
            "--merged-branch",
            "issue-9-x",
        ])
        .args(["--repo", &s(&repo), "--pr", "9", "--head", &head])
        .args(["--target", &s(&target), "--keep-worktree", &s(&reused)])
        .args(["--base", "main", "--leftovers", &s(&leftovers)])
        .output()
        .unwrap();
    let reply: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    let stopped = holder.try_wait().unwrap().is_some() || holder.wait_timeout_free().is_some();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(reply["data"]["ok"], true, "{reply}");

    assert!(
        stopped,
        "the process holding the review checkout was not stopped: {reply}"
    );
    let listed = worktree_paths(&repo);
    for gone in [&author, &review] {
        assert!(!gone.exists(), "{} survived: {reply}", s(gone));
        assert!(!listed.contains(&s(gone)), "{} still registered", s(gone));
    }
    assert!(!target.exists(), "the per-PR target survived");
    assert_eq!(
        git(&repo, &["branch", "--list", "issue-9-x"]),
        "",
        "branch survived"
    );
    assert!(
        outside.join("keep.txt").is_file(),
        "the junction was followed"
    );
    let saved = leftovers
        .join("9")
        .join("wt-author")
        .join("untracked")
        .join("wip.txt");
    assert_eq!(std::fs::read_to_string(saved).unwrap(), "unsaved\n");

    assert!(
        unrelated.join("a.txt").is_file(),
        "an unrelated worktree was touched"
    );
    assert!(
        shared.join("debug").join("x").is_file(),
        "the shared target was touched"
    );
    assert!(
        reused.is_dir() && listed.contains(&s(&reused)),
        "the reused review worktree went"
    );
    assert_eq!(
        git(&reused, &["rev-parse", "HEAD"]),
        main_head,
        "reused stayed on the PR"
    );
}

/// #1459 item 17: a harness locks the worktree of a live agent session. The merge sweep found
/// such a worktree on the merged branch and would have stopped about 20 of the session's live
/// processes. A locked worktree is held: its processes keep running, it stays registered, the
/// reply names it with the lock reason, and the run is not ok and exits non-zero.
#[test]
fn workspace_merged_cleanup_holds_a_locked_live_session_worktree() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let base = PathBuf::from(s(&base).trim_start_matches("//?/"));
    let repo = base.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main", "--object-format=sha1"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    let agent = base.join("agent-live");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "issue-9-x",
            &s(&agent),
            "main",
        ],
    );
    let mut session = hold(&agent);
    let reason = format!("claude agent test (pid {})", session.id());
    git(
        &repo,
        &["worktree", "lock", "--reason", &reason, &s(&agent)],
    );

    let out = graphhelm_process::command(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["--json", "workspace", "sweep", "--apply"])
        .args([
            "--merged-branch",
            "issue-9-x",
            "--repo",
            &s(&repo),
            "--pr",
            "9",
        ])
        .args(["--base", "main", "--leftovers", &s(&base.join("leftovers"))])
        .output()
        .unwrap();
    let reply: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    let alive = session.try_wait().unwrap().is_none();
    let _ = session.kill();
    let _ = session.wait();
    git(&repo, &["worktree", "unlock", &s(&agent)]);

    assert!(alive, "the live session's process was stopped: {reply}");
    assert!(
        agent.join("a.txt").is_file(),
        "the locked worktree was removed: {reply}"
    );
    assert!(
        worktree_paths(&repo).contains(&s(&agent)),
        "the locked worktree was unregistered"
    );
    assert_eq!(reply["data"]["ok"], false, "{reply}");
    assert_eq!(
        reply["data"]["held"][0]["reason"],
        reason.as_str(),
        "{reply}"
    );
    assert_eq!(reply["data"]["removed"], serde_json::json!([]), "{reply}");
    assert_eq!(
        out.status.code(),
        Some(3),
        "an incomplete cleanup must exit non-zero: {out:?}"
    );
}

trait WaitBounded {
    fn wait_timeout_free(&mut self) -> Option<std::process::ExitStatus>;
}

impl WaitBounded for std::process::Child {
    /// Polls for up to five seconds; a survivor is killed so the test never leaks it.
    fn wait_timeout_free(&mut self) -> Option<std::process::ExitStatus> {
        for _ in 0..50 {
            if let Ok(Some(status)) = self.try_wait() {
                return Some(status);
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let _ = self.kill();
        let _ = self.wait();
        None
    }
}
