//! `graphhelm workspace remove <dir>` (#1663): the one command a merger or reviewer runs to delete
//! a worktree or a directory. It never follows a link: `rm -rf` and `git worktree remove --force`
//! both recurse through a junction (a worktree's `apps/studio/node_modules`) into the owner's
//! checkout and delete what it points at.
//!
//! A linked git worktree goes through the merge cleanup's own `remove_worktree` (stop the
//! processes inside, unlink links, save uncommitted work to `<leftovers>/<name>/`, unregister,
//! link-safe delete). Any other directory: links are unlinked first, then the link-safe
//! `remove_tree`. A repository's main checkout is refused.
use std::path::Path;

use serde_json::json;

use super::workspace::{input, refuse, remove_tree};
use super::workspace_merged::{git, remove_worktree, show, unlink_links, worktrees};
use crate::args::WorkspaceRemoveArgs;
use crate::output::Outcome;

const COMMAND: &str = "workspace.remove";

fn same(a: &Path, b: &Path) -> bool {
    graphhelm_process_tree::path_is_inside(a, b) && graphhelm_process_tree::path_is_inside(b, a)
}

pub(crate) fn run(args: &WorkspaceRemoveArgs) -> Outcome {
    let path = &args.path;
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return Outcome::success(
            COMMAND,
            json!({"path": show(path), "ok": true, "removed": false, "absent": true}),
        );
    };
    if super::workspace::is_link(&metadata) {
        // The path itself is a link: remove the link, never what it points at.
        let mut row = json!({"path": show(path), "worktree": false, "link": true});
        match remove_tree(path) {
            Ok(()) => row["removed"] = json!(true),
            Err(e) => row["error"] = json!(format!("remove failed: {e}")),
        }
        return finish(row);
    }
    if !metadata.is_dir() {
        return input(COMMAND, "the path is not a directory", "/path");
    }
    // The path is a git worktree only when git names it as its own top level.
    let top = git(path, &["rev-parse", "--show-toplevel"]).ok();
    if let Some(top) = &top
        && !same(Path::new(top.trim()), path)
    {
        return refused("the directory is inside a git checkout; remove the checkout itself");
    }
    let worktree = match top {
        Some(top) if same(Path::new(top.trim()), path) => match worktrees(path) {
            Ok(list) => {
                if list.first().is_some_and(|main| same(&main.path, path)) {
                    return refuse(
                        COMMAND,
                        crate::error_codes::GHCLI037_WORKSPACE_REFUSED,
                        "this is a repository's main checkout, not a worktree",
                        "/path",
                    );
                }
                let main = list.first().map(|m| m.path.clone());
                main.zip(list.into_iter().find(|w| same(&w.path, path)))
            }
            Err(message) => return input(COMMAND, &message, "/path"),
        },
        _ => None,
    };
    let row = match worktree {
        Some((_, worktree)) if worktree.locked.is_some() => {
            json!({"path": show(path), "error": format!(
                "the worktree is locked: {}", worktree.locked.unwrap_or_default())})
        }
        Some((main, worktree)) => {
            let name = path.file_name().map_or_else(
                || "worktree".to_owned(),
                |n| n.to_string_lossy().into_owned(),
            );
            let mut row = remove_worktree(&main, &worktree, &args.leftovers.join(name));
            row["worktree"] = json!(true);
            row
        }
        None => {
            if let Some(message) = guard_plain(path, args.repo.as_deref()) {
                return refused(&message);
            }
            let mut row = json!({"path": show(path), "worktree": false});
            match unlink_links(path) {
                Ok(unlinked) => {
                    row["unlinked"] = json!(unlinked);
                    match remove_tree(path) {
                        Ok(()) => row["removed"] = json!(true),
                        Err(e) => row["error"] = json!(format!("remove failed: {e}")),
                    }
                }
                Err(message) => row["error"] = json!(message),
            }
            row
        }
    };
    finish(row)
}

fn refused(message: &str) -> Outcome {
    refuse(
        COMMAND,
        crate::error_codes::GHCLI037_WORKSPACE_REFUSED,
        message,
        "/path",
    )
}

/// Why a directory that is not a worktree must not be deleted, or `None` when it may be: it holds
/// a checkout (a `.git` entry anywhere under it, found without following links), it is an
/// ancestor of a worktree `--repo` registers, or a live process holds it. Fails closed.
fn guard_plain(path: &Path, repo: Option<&Path>) -> Option<String> {
    match holds_git(path) {
        Ok(Some(found)) => {
            return Some(format!(
                "the directory holds a git checkout at {}",
                show(&found)
            ));
        }
        Ok(None) => {}
        Err(message) => return Some(message),
    }
    if let Some(repo) = repo {
        match worktrees(repo) {
            Ok(list) => {
                if let Some(w) = list
                    .iter()
                    .find(|w| graphhelm_process_tree::path_is_inside(&w.path, path))
                {
                    return Some(format!(
                        "the directory contains the worktree {}",
                        show(&w.path)
                    ));
                }
            }
            Err(message) => {
                return Some(format!("--repo worktrees could not be listed: {message}"));
            }
        }
    }
    let held = graphhelm_process_tree::processes_inside(path);
    if !held.is_empty() {
        return Some(format!("live processes {held:?} hold the directory"));
    }
    None
}

/// The first `.git` entry under `dir`, relative to it, without following links.
fn holds_git(dir: &Path) -> Result<Option<std::path::PathBuf>, String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("{} could not be read: {e}", show(dir)))?;
    for entry in entries {
        let child = entry
            .map_err(|e| format!("{} could not be read: {e}", show(dir)))?
            .path();
        if child.file_name().is_some_and(|n| n == ".git") {
            return Ok(Some(child));
        }
        let metadata = std::fs::symlink_metadata(&child)
            .map_err(|e| format!("{} could not be read: {e}", show(&child)))?;
        if metadata.is_dir()
            && !super::workspace::is_link(&metadata)
            && let Some(found) = holds_git(&child)?
        {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

fn finish(mut row: serde_json::Value) -> Outcome {
    let ok = row.get("error").is_none();
    row["ok"] = json!(ok);
    let mut outcome = Outcome::success(COMMAND, row);
    if !ok {
        outcome.exit_code = 3;
    }
    outcome
}
