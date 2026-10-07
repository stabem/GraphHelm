//! Agent workspaces (#360): one directory per lane and task under a configured root, created by
//! `claim`, declared done by `release`, and removed only by the owner's `sweep`.
//!
//! The ledger is one JSON record per workspace under `<root>/.graphhelm-workspaces/`. There is no
//! project-level event stream to put it in: one root serves many repositories and runs, and every
//! signal needs an execution. The sweeper never infers "merged" from git ancestry (a squash merge
//! never makes the branch an ancestor of main); it trusts the claimant's release and re-checks the
//! bytes: still clean, still at the released sha. A path the ledger did not create is never
//! touched, and a symlink or junction inside a workspace is removed as a link, never followed.
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use graphhelm_protocols::Diagnostic;
use serde_json::{Value, json};

use crate::args::{WorkspaceArgs, WorkspaceCommand};
use crate::output::Outcome;

pub(crate) const SCHEMA: &str = "graphhelm.workspace/1";
const LEDGER: &str = ".graphhelm-workspaces";
const PARTS: [&str; 3] = ["target", "tmp", "logs"];

pub fn run(args: &WorkspaceArgs) -> Outcome {
    match &args.command {
        WorkspaceCommand::Claim(claim) => run_claim(
            &claim.root,
            &claim.lane,
            &claim.task,
            &claim.repo,
            claim.base.as_deref().unwrap_or("origin/main"),
            claim.branch.as_deref(),
        ),
        WorkspaceCommand::Release(release) => {
            run_release(&release.root, &release.lane, &release.task)
        }
        WorkspaceCommand::List(list) => run_list(&list.root),
        WorkspaceCommand::Sweep(sweep) => run_sweep(&sweep.root, sweep.apply),
    }
}

fn refuse(command: &'static str, code: &str, message: &str, path: &str) -> Outcome {
    Outcome::domain(
        command,
        vec![Diagnostic::error(code, message, path, "graphhelm")],
    )
}

fn input(command: &'static str, message: &str, path: &str) -> Outcome {
    Outcome::application(
        command,
        Diagnostic::error(
            crate::error_codes::GHCLI001_ARGUMENT_INVALID,
            message,
            path,
            "graphhelm",
        ),
    )
}

/// Lane and task ids name directories and ledger files, so they are bounded like journey ids and
/// may not contain `--`, the ledger file name's separator.
fn valid_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes.iter().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        })
        && !id.contains("..")
        && !id.contains("--")
}

fn ledger_file(root: &Path, lane: &str, task: &str) -> PathBuf {
    root.join(LEDGER).join(format!("{lane}--{task}.json"))
}

/// The workspace directory is always derived from the ids, never read back from a record.
fn workspace_dir(root: &Path, lane: &str, task: &str) -> PathBuf {
    root.join(lane).join(task)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|_| "git could not be started".to_owned())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

fn write_record(file: &Path, record: &Value) -> std::io::Result<()> {
    let parent = file.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let staging = file.with_extension("json.tmp");
    let mut bytes = serde_json::to_vec_pretty(record).map_err(std::io::Error::other)?;
    bytes.push(b'\n');
    std::fs::write(&staging, bytes)?;
    std::fs::rename(&staging, file)
}

fn read_record(file: &Path) -> Option<Value> {
    let value: Value = serde_json::from_slice(&std::fs::read(file).ok()?).ok()?;
    (value["schema"] == SCHEMA
        && value["lane"].as_str().is_some_and(valid_id)
        && value["task"].as_str().is_some_and(valid_id)
        && file.file_name().and_then(|n| n.to_str())
            == Some(&format!(
                "{}--{}.json",
                value["lane"].as_str().unwrap_or_default(),
                value["task"].as_str().unwrap_or_default()
            )))
    .then_some(value)
}

fn env_of(dir: &Path) -> Value {
    let show = |p: PathBuf| p.to_string_lossy().into_owned();
    json!({
        "CARGO_TARGET_DIR": show(dir.join("target")),
        "TEMP": show(dir.join("tmp")),
        "TMP": show(dir.join("tmp")),
    })
}

fn run_claim(
    root: &Path,
    lane: &str,
    task: &str,
    repo: &Path,
    base: &str,
    branch: Option<&str>,
) -> Outcome {
    const COMMAND: &str = "workspace.claim";
    if !valid_id(lane) {
        return input(
            COMMAND,
            "lane must match ^[a-z0-9][a-z0-9._-]{0,63}$ without `..` or `--`",
            "/lane",
        );
    }
    if !valid_id(task) {
        return input(
            COMMAND,
            "task must match ^[a-z0-9][a-z0-9._-]{0,63}$ without `..` or `--`",
            "/task",
        );
    }
    let file = ledger_file(root, lane, task);
    let dir = workspace_dir(root, lane, task);
    if file.exists() || dir.exists() {
        return refuse(
            COMMAND,
            crate::error_codes::GHCLI037_WORKSPACE_REFUSED,
            "this lane already has a workspace for this task; release it and let the owner sweep it first",
            "/task",
        );
    }
    let branch = branch.map_or_else(|| format!("issue-{task}-{lane}"), str::to_owned);
    if base.starts_with('-') || branch.starts_with('-') {
        return input(COMMAND, "base and branch may not start with `-`", "/base");
    }
    if std::fs::create_dir_all(&dir).is_err() {
        return input(
            COMMAND,
            "the workspace directory could not be created",
            "/root",
        );
    }
    let worktree = dir.join("wt");
    if let Err(message) = git(
        repo,
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            &worktree.to_string_lossy(),
            base,
        ],
    ) {
        // Nothing was recorded; remove only the empty directories this call made.
        let _ = std::fs::remove_dir(&dir);
        let _ = std::fs::remove_dir(dir.parent().unwrap_or(root));
        return refuse(
            COMMAND,
            crate::error_codes::GHCLI037_WORKSPACE_REFUSED,
            &format!("git worktree add failed: {message}"),
            "/repo",
        );
    }
    for part in PARTS {
        let _ = std::fs::create_dir_all(dir.join(part));
    }
    let record = json!({
        "schema": SCHEMA, "lane": lane, "task": task, "branch": branch, "base": base,
        "repo": std::fs::canonicalize(repo).unwrap_or_else(|_| repo.to_path_buf()).to_string_lossy(),
        "state": "claimed", "claimedAt": now(), "releasedAt": null, "releasedHead": null,
        "sweptAt": null,
    });
    if write_record(&file, &record).is_err() {
        return Outcome::internal(COMMAND, "the workspace record could not be written");
    }
    Outcome::success(
        COMMAND,
        json!({"lane": lane, "task": task, "branch": branch,
            "path": dir.to_string_lossy(), "worktree": worktree.to_string_lossy(),
            "env": env_of(&dir)}),
    )
}

fn run_release(root: &Path, lane: &str, task: &str) -> Outcome {
    const COMMAND: &str = "workspace.release";
    if !valid_id(lane) || !valid_id(task) {
        return input(COMMAND, "lane and task must be workspace ids", "/task");
    }
    let file = ledger_file(root, lane, task);
    let Some(mut record) = read_record(&file) else {
        return refuse(
            COMMAND,
            crate::error_codes::GHCLI037_WORKSPACE_REFUSED,
            "no claimed workspace for this lane and task",
            "/task",
        );
    };
    if record["state"] != "claimed" {
        return refuse(
            COMMAND,
            crate::error_codes::GHCLI037_WORKSPACE_REFUSED,
            "only a claimed workspace can be released",
            "/state",
        );
    }
    let worktree = workspace_dir(root, lane, task).join("wt");
    let Ok(head) = git(&worktree, &["rev-parse", "HEAD"]) else {
        return refuse(
            COMMAND,
            crate::error_codes::GHCLI037_WORKSPACE_REFUSED,
            "the workspace has no readable worktree",
            "/worktree",
        );
    };
    let dirty = git(&worktree, &["status", "--porcelain"]).map_or(true, |s| !s.is_empty());
    record["state"] = json!("released");
    record["releasedAt"] = json!(now());
    record["releasedHead"] = json!(head);
    if write_record(&file, &record).is_err() {
        return Outcome::internal(COMMAND, "the workspace record could not be written");
    }
    Outcome::success(
        COMMAND,
        json!({"lane": lane, "task": task, "releasedHead": head, "dirty": dirty}),
    )
}

fn records(root: &Path) -> Vec<Value> {
    let Ok(entries) = std::fs::read_dir(root.join(LEDGER)) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    files.iter().filter_map(|f| read_record(f)).collect()
}

/// Whether `path` is a link of any kind: a symlink, or on Windows any reparse point (junctions
/// are not reported by `is_symlink`).
fn is_link(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        metadata.file_type().is_symlink()
            || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

/// The first link (symlink, junction or other reparse point) under `path`, relative to it, found
/// without following any link. `git worktree remove` recurses through a junction at an ignored
/// path and deletes what it points at, so a worktree holding one is never handed to git (#374).
fn first_link(path: &Path, base: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(path).ok()?;
    for entry in entries.filter_map(Result::ok) {
        let child = entry.path();
        let Ok(metadata) = std::fs::symlink_metadata(&child) else {
            continue;
        };
        if is_link(&metadata) {
            return Some(child.strip_prefix(base).unwrap_or(&child).to_path_buf());
        }
        if metadata.is_dir()
            && let Some(found) = first_link(&child, base)
        {
            return Some(found);
        }
    }
    None
}

fn size_of(path: &Path) -> u64 {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if is_link(&metadata) || !metadata.is_dir() {
        return metadata.len();
    }
    std::fs::read_dir(path)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| size_of(&e.path()))
                .sum()
        })
        .unwrap_or(0)
}

/// Deletes `path` without following links: a link (file or directory symlink, or a junction) is
/// removed as the link itself, so whatever it points at survives.
fn remove_tree(path: &Path) -> std::io::Result<()> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if is_link(&metadata) {
        return std::fs::remove_dir(path).or_else(|_| std::fs::remove_file(path));
    }
    if metadata.is_dir() {
        for entry in std::fs::read_dir(path)? {
            remove_tree(&entry?.path())?;
        }
        return std::fs::remove_dir(path);
    }
    std::fs::remove_file(path)
}

fn live(root: &Path, record: &Value) -> Value {
    let (lane, task) = (
        record["lane"].as_str().unwrap_or_default(),
        record["task"].as_str().unwrap_or_default(),
    );
    let dir = workspace_dir(root, lane, task);
    let worktree = dir.join("wt");
    let head = git(&worktree, &["rev-parse", "HEAD"]).ok();
    let dirty = head
        .as_ref()
        .map(|_| git(&worktree, &["status", "--porcelain"]).map_or(true, |s| !s.is_empty()));
    // A lane, task or worktree directory that is itself a link would make every path below it
    // resolve somewhere the ledger never created; such a workspace is reported, never removed.
    let linked = [root.join(lane), dir.clone(), worktree.clone()]
        .iter()
        .any(|p| std::fs::symlink_metadata(p).is_ok_and(|m| is_link(&m)));
    let link_in_worktree = if linked {
        None
    } else {
        first_link(&worktree, &worktree).map(|p| p.to_string_lossy().replace('\\', "/"))
    };
    json!({"lane": lane, "task": task, "branch": record["branch"], "state": record["state"],
        "linked": linked, "linkInWorktree": link_in_worktree,
        "path": dir.to_string_lossy(), "exists": dir.exists(), "head": head, "dirty": dirty,
        "releasedHead": record["releasedHead"], "sizeBytes": size_of(&dir)})
}

pub(crate) fn run_list(root: &Path) -> Outcome {
    const COMMAND: &str = "workspace.list";
    let listed: Vec<Value> = records(root).iter().map(|r| live(root, r)).collect();
    Outcome::success(
        COMMAND,
        json!({"root": root.to_string_lossy(), "workspaces": listed}),
    )
}

/// Why a workspace is kept, or `None` when the sweep may remove it.
fn keep_reason(view: &Value) -> Option<&'static str> {
    if view["state"] == "swept" {
        return Some("already_swept");
    }
    if view["state"] != "released" {
        return Some("not_released");
    }
    if view["linked"] == json!(true) {
        return Some("linked_path");
    }
    if !view["linkInWorktree"].is_null() {
        return Some("contains_link");
    }
    if view["head"].is_null() {
        return Some("worktree_unreadable");
    }
    if view["dirty"] != json!(false) {
        return Some("dirty");
    }
    if view["head"] != view["releasedHead"] {
        return Some("moved_after_release");
    }
    None
}

pub(crate) fn run_sweep(root: &Path, apply: bool) -> Outcome {
    const COMMAND: &str = "workspace.sweep";
    let mut removed = Vec::new();
    let mut kept = Vec::new();
    for mut record in records(root) {
        let view = live(root, &record);
        let (lane, task) = (
            view["lane"].as_str().unwrap_or_default().to_owned(),
            view["task"].as_str().unwrap_or_default().to_owned(),
        );
        if let Some(reason) = keep_reason(&view) {
            if reason != "already_swept" {
                kept.push(json!({"lane": lane, "task": task, "reason": reason,
                    "link": view["linkInWorktree"]}));
            }
            continue;
        }
        if !apply {
            removed.push(json!({"lane": lane, "task": task, "sizeBytes": view["sizeBytes"]}));
            continue;
        }
        let dir = workspace_dir(root, &lane, &task);
        let worktree = dir.join("wt");
        // `git worktree remove` without --force refuses a dirty tree: a second check at the act.
        let common = git(
            &worktree,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        );
        let removal = common.and_then(|common| {
            Command::new("git")
                .arg("--git-dir")
                .arg(&common)
                .args(["worktree", "remove"])
                .arg(&worktree)
                .output()
                .map_err(|_| "git could not be started".to_owned())
                .and_then(|o| {
                    if o.status.success() {
                        Ok(())
                    } else {
                        Err(String::from_utf8_lossy(&o.stderr).trim().to_owned())
                    }
                })
        });
        if let Err(message) = removal {
            kept.push(
                json!({"lane": lane, "task": task, "reason": "worktree_remove_failed",
                "message": message}),
            );
            continue;
        }
        let failed: Vec<&str> = PARTS
            .iter()
            .copied()
            .filter(|part| remove_tree(&dir.join(part)).is_err())
            .collect();
        let _ = std::fs::remove_dir(&dir);
        record["state"] = json!("swept");
        record["sweptAt"] = json!(now());
        let _ = write_record(&ledger_file(root, &lane, &task), &record);
        removed.push(
            json!({"lane": lane, "task": task, "sizeBytes": view["sizeBytes"],
            "notRemoved": failed}),
        );
    }
    Outcome::success(
        COMMAND,
        json!({"root": root.to_string_lossy(), "applied": apply, "removed": removed, "kept": kept}),
    )
}
