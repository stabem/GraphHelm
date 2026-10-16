//! Merge cleanup (#1456): `graphhelm workspace sweep --merged-branch <b> --repo <r> --apply`.
//!
//! The merger runs it once, after the merge lands, and it removes what existed only for that PR:
//! every git worktree on the merged branch (on any drive git knows of), the review checkouts
//! detached at `--head`, the `--target` build directories and the local branch. Per worktree, in
//! this order: stop the processes inside it, unlink links without following them, save
//! uncommitted work to `<leftovers>/<PR>/`, then `git worktree remove --force`.
//!
//! One exception: a `--keep-worktree` (a lane's single reused review worktree) is kept and only
//! moved off the PR to `--base`; its shared target is never named, so it is kept too. Nothing
//! the command did not select is touched, and `git worktree prune` is not run: it would drop
//! other lanes' entries for worktrees on a drive that is not mounted right now.
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use super::workspace::{first_link, input, refuse, remove_tree};
use crate::args::WorkspaceSweepArgs;
use crate::output::Outcome;

const COMMAND: &str = "workspace.sweep";
/// A link scan that keeps finding links past this many is refused rather than looped on.
const MAX_LINKS: usize = 4096;

struct Worktree {
    path: PathBuf,
    head: String,
    branch: Option<String>,
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = graphhelm_process::command("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|_| "git could not be started".to_owned())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

fn worktrees(repo: &Path) -> Result<Vec<Worktree>, String> {
    let listed = git(repo, &["worktree", "list", "--porcelain"])?;
    let mut found = Vec::new();
    for block in listed.split("\n\n") {
        let mut path = None;
        let mut head = String::new();
        let mut branch = None;
        for line in block.lines() {
            if let Some(p) = line.strip_prefix("worktree ") {
                path = Some(PathBuf::from(p));
            } else if let Some(h) = line.strip_prefix("HEAD ") {
                h.clone_into(&mut head);
            } else if let Some(b) = line.strip_prefix("branch ") {
                branch = Some(b.to_owned());
            }
        }
        if let Some(path) = path {
            found.push(Worktree { path, head, branch });
        }
    }
    Ok(found)
}

fn same(a: &Path, b: &Path) -> bool {
    graphhelm_process_tree::path_is_inside(a, b) && graphhelm_process_tree::path_is_inside(b, a)
}

fn show(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Stops every process inside `dir`; returns the ids stopped and the ids that would not stop.
fn stop_processes(dir: &Path) -> (Vec<u32>, Vec<u32>) {
    let mut stopped = Vec::new();
    let mut survived = Vec::new();
    for pid in graphhelm_process_tree::processes_inside(dir) {
        let Ok(identity) = graphhelm_process_tree::ProcessIdentity::capture(pid) else {
            continue; // already gone, or not ours to ask about
        };
        let gone = identity.terminate().is_ok()
            && identity
                .wait_until_gone(Duration::from_secs(10))
                .unwrap_or(false);
        if gone { &mut stopped } else { &mut survived }.push(pid);
    }
    (stopped, survived)
}

/// Removes every link (symlink or junction) under `dir` as a link; what it points at survives.
fn unlink_links(dir: &Path) -> Result<Vec<String>, String> {
    let mut unlinked = Vec::new();
    for _ in 0..MAX_LINKS {
        match first_link(dir, dir) {
            Ok(None) => return Ok(unlinked),
            Ok(Some(relative)) => {
                let link = dir.join(&relative);
                std::fs::remove_dir(&link)
                    .or_else(|_| std::fs::remove_file(&link))
                    .map_err(|e| format!("link {} could not be removed: {e}", show(&relative)))?;
                unlinked.push(show(&relative));
            }
            Err((relative, e)) => {
                return Err(format!(
                    "{} could not be scanned for links: {e}",
                    show(&relative)
                ));
            }
        }
    }
    Err(format!("more than {MAX_LINKS} links inside the worktree"))
}

/// Saves tracked changes as `changes.patch` and copies untracked files under `untracked/`.
/// `Ok(None)`: the worktree was clean and nothing was written.
fn save_leftovers(worktree: &Path, into: &Path) -> Result<Option<String>, String> {
    let status = git(
        worktree,
        &["status", "--porcelain", "--untracked-files=all"],
    )?;
    if status.trim().is_empty() {
        return Ok(None);
    }
    std::fs::create_dir_all(into).map_err(|e| format!("leftovers folder: {e}"))?;
    let patch = git(worktree, &["diff", "HEAD", "--binary"])?;
    std::fs::write(into.join("changes.patch"), patch).map_err(|e| format!("patch: {e}"))?;
    let untracked = git(
        worktree,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )?;
    for name in untracked.split('\0').filter(|n| !n.is_empty()) {
        let from = worktree.join(name);
        let to = into.join("untracked").join(name);
        if std::fs::symlink_metadata(&from).is_ok_and(|m| m.is_file()) {
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("untracked: {e}"))?;
            }
            std::fs::copy(&from, &to).map_err(|e| format!("untracked {name}: {e}"))?;
        }
    }
    Ok(Some(show(into)))
}

fn remove_worktree(repo: &Path, worktree: &Worktree, leftovers: &Path) -> Value {
    let path = &worktree.path;
    let (stopped, survived) = stop_processes(path);
    let mut row = json!({"path": show(path), "stopped": stopped});
    if !survived.is_empty() {
        row["error"] = json!(format!("processes {survived:?} would not stop"));
        return row;
    }
    match unlink_links(path) {
        Ok(unlinked) => row["unlinked"] = json!(unlinked),
        Err(message) => {
            row["error"] = json!(message);
            return row;
        }
    }
    match save_leftovers(path, leftovers) {
        Ok(saved) => row["leftovers"] = json!(saved),
        Err(message) => {
            row["error"] = json!(format!("leftovers not saved, worktree kept: {message}"));
            return row;
        }
    }
    let target = show(path);
    if let Err(message) = git(repo, &["worktree", "remove", "--force", &target]) {
        row["error"] = json!(message);
        return row;
    }
    if let Err(e) = remove_tree(path) {
        row["error"] = json!(format!("worktree unregistered but its folder stayed: {e}"));
        return row;
    }
    row["removed"] = json!(true);
    row
}

fn keep_worktree(worktree: &Worktree, base: &str, leftovers: &Path) -> Value {
    let path = &worktree.path;
    let mut row = json!({"path": show(path), "kept": "reused_review_worktree"});
    match save_leftovers(path, leftovers) {
        Ok(saved) => row["leftovers"] = json!(saved),
        Err(message) => {
            row["error"] = json!(format!(
                "leftovers not saved, checkout left as is: {message}"
            ));
            return row;
        }
    }
    match git(path, &["checkout", "--detach", "--force", base]) {
        Ok(_) => row["detachedTo"] = json!(base),
        Err(message) => row["error"] = json!(message),
    }
    row
}

pub(crate) fn run(args: &WorkspaceSweepArgs) -> Outcome {
    let (Some(branch), Some(repo)) = (&args.merged_branch, &args.repo) else {
        return input(COMMAND, "--merged-branch needs --repo", "/repo");
    };
    let refused = |message: &str, path: &str| {
        refuse(
            COMMAND,
            crate::error_codes::GHCLI037_WORKSPACE_REFUSED,
            message,
            path,
        )
    };
    if branch.is_empty() || branch.starts_with('-') || branch == "main" || branch == "master" {
        return refused(
            "the merged branch must be a PR branch, never main",
            "/mergedBranch",
        );
    }
    if let Some(head) = &args.head
        && (head.len() < 7 || !head.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return refused("--head must be a commit sha", "/head");
    }
    let all = match worktrees(repo) {
        Ok(all) => all,
        Err(message) => return refused(&message, "/repo"),
    };
    let Some(main_checkout) = all.first() else {
        return refused("git listed no worktrees for this repository", "/repo");
    };
    let reference = format!("refs/heads/{branch}");
    if main_checkout.branch.as_deref() == Some(reference.as_str()) {
        return refused(
            "the main checkout is on the merged branch; switch it first",
            "/repo",
        );
    }
    for target in &args.targets {
        if !target.is_absolute()
            || all
                .iter()
                .any(|w| graphhelm_process_tree::path_is_inside(&w.path, target))
        {
            return refused(
                "a --target must be an absolute build directory that holds no worktree",
                "/target",
            );
        }
    }
    let folder = args
        .pr
        .map_or_else(|| branch.replace('/', "-"), |pr| pr.to_string());
    let leftovers = args.leftovers.join(&folder);
    let selected = |w: &Worktree| {
        w.branch.as_deref() == Some(reference.as_str())
            || (w.branch.is_none()
                && args
                    .head
                    .as_deref()
                    .is_some_and(|h| w.head.starts_with(&h.to_ascii_lowercase())))
    };
    let mut removed = Vec::new();
    let mut kept = Vec::new();
    for worktree in all.iter().skip(1).filter(|w| selected(w)) {
        let name = worktree.path.file_name().map_or_else(
            || "worktree".to_owned(),
            |n| n.to_string_lossy().into_owned(),
        );
        let into = leftovers.join(&name);
        let keep = args.keep_worktrees.iter().any(|k| same(k, &worktree.path));
        if !args.apply {
            let row = json!({"path": show(&worktree.path),
                "processes": graphhelm_process_tree::processes_inside(&worktree.path)});
            if keep { &mut kept } else { &mut removed }.push(row);
            continue;
        }
        if keep {
            kept.push(keep_worktree(worktree, &args.base, &into));
        } else {
            removed.push(remove_worktree(repo, worktree, &into));
        }
    }
    let mut targets = Vec::new();
    for target in &args.targets {
        let mut row = json!({"path": show(target)});
        if args.apply {
            match remove_tree(target) {
                Ok(()) => row["removed"] = json!(true),
                Err(e) => row["error"] = json!(e.to_string()),
            }
        }
        targets.push(row);
    }
    let exists = git(repo, &["rev-parse", "--verify", "--quiet", &reference]).is_ok();
    let branch_row = if !exists {
        json!({"name": branch, "removed": false, "reason": "absent"})
    } else if !args.apply {
        json!({"name": branch})
    } else {
        match git(repo, &["branch", "-D", branch]) {
            Ok(_) => json!({"name": branch, "removed": true}),
            Err(message) => json!({"name": branch, "removed": false, "error": message}),
        }
    };
    let ok = [&removed, &kept, &targets]
        .iter()
        .all(|rows| rows.iter().all(|r| r.get("error").is_none()))
        && branch_row.get("error").is_none();
    Outcome::success(
        COMMAND,
        json!({"mergedBranch": branch, "applied": args.apply, "ok": ok,
            "leftovers": show(&leftovers), "removed": removed, "kept": kept,
            "targets": targets, "branch": branch_row}),
    )
}
