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
pub(crate) const LEDGER: &str = ".graphhelm-workspaces";
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
        WorkspaceCommand::Slot(slot) => match (&slot.action, &slot.root, &slot.lane) {
            (Some(crate::args::WorkspaceSlotAction::Status(status)), _, _) => {
                super::workspace_slot::run_status(&status.root)
            }
            (None, Some(root), Some(lane)) => {
                super::workspace_slot::run_slot(&super::workspace_slot::SlotRequest {
                    root,
                    lane,
                    label: &slot.label,
                    jobs: slot.jobs,
                    clean_workspace: slot.clean_workspace,
                    shared: slot.shared_target,
                    max_wait: slot.max_wait,
                    priority: slot.priority,
                    command: &slot.command,
                })
            }
            // clap requires --root and --lane without a subcommand; this arm is unreachable.
            (None, _, _) => super::workspace_slot::refuse_args(),
        },
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
pub(crate) fn valid_id(id: &str) -> bool {
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

/// The environment a claimed workspace builds with: its own temp directory and its own cargo
/// target (#361: a shared target lets one worktree's tests run another's bytes).
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
///
/// Fails closed: a directory or entry the scan cannot read is an `Err` naming it, and the caller
/// keeps the workspace. An unread directory may hold a link.
fn first_link(path: &Path, base: &Path) -> Result<Option<PathBuf>, (PathBuf, String)> {
    let relative = |p: &Path| p.strip_prefix(base).unwrap_or(p).to_path_buf();
    let entries = std::fs::read_dir(path).map_err(|e| (relative(path), e.to_string()))?;
    for entry in entries {
        let child = entry.map_err(|e| (relative(path), e.to_string()))?.path();
        let metadata =
            std::fs::symlink_metadata(&child).map_err(|e| (relative(&child), e.to_string()))?;
        if is_link(&metadata) {
            return Ok(Some(relative(&child)));
        }
        if metadata.is_dir()
            && let Some(found) = first_link(&child, base)?
        {
            return Ok(Some(found));
        }
    }
    Ok(None)
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

/// The owner's build-directory rule for the slot (#360): `<root>/.graphhelm-workspaces/
/// slot-targets.json`, `{"targetRoot": "<absolute directory>", "cap": <1..=16, default 3>}`.
/// With it the slot builds in `<targetRoot>/<lane>/<worktree directory name>/target`, records
/// each such directory, and holds a lane to `cap` of them. Without the file nothing changes.
pub(crate) const SLOT_TARGETS: &str = "slot-targets.json";
const TARGET_SCHEMA: &str = "graphhelm.slot-target/1";
const TARGET_RECORDS: &str = "targets";
const DEFAULT_TARGET_CAP: u64 = 3;

pub(crate) struct TargetRule {
    pub root: PathBuf,
    pub cap: u64,
    pub min_free_gb: u64,
}

/// `Ok(None)`: no rule file. A file that cannot be read or does not say where to build is an
/// error, never a silent fall back to the worktree's own disk: the owner wrote a rule.
pub(crate) fn target_rule(root: &Path) -> Result<Option<TargetRule>, String> {
    let file = root.join(LEDGER).join(SLOT_TARGETS);
    let bytes = match std::fs::read(&file) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(format!("{LEDGER}/{SLOT_TARGETS} could not be read")),
    };
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| format!("{LEDGER}/{SLOT_TARGETS} is not JSON"))?;
    let target_root = value["targetRoot"]
        .as_str()
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| {
            format!("{LEDGER}/{SLOT_TARGETS}: targetRoot must be an absolute directory")
        })?;
    let cap = match &value["cap"] {
        Value::Null => DEFAULT_TARGET_CAP,
        cap => cap
            .as_u64()
            .filter(|cap| (1..=16).contains(cap))
            .ok_or_else(|| format!("{LEDGER}/{SLOT_TARGETS}: cap must be a number from 1 to 16"))?,
    };
    let min_free_gb = match value.get("minFreeGb") {
        None => 20,
        Some(floor) => floor
            .as_u64()
            .filter(|floor| *floor <= 4096)
            .ok_or_else(|| {
                format!("{LEDGER}/{SLOT_TARGETS}: minFreeGb must be a number from 0 to 4096")
            })?,
    };
    Ok(Some(TargetRule {
        root: target_root,
        cap,
        min_free_gb,
    }))
}

fn target_record_file(root: &Path, lane: &str, name: &str) -> PathBuf {
    root.join(LEDGER)
        .join(TARGET_RECORDS)
        .join(lane)
        .join(format!("{name}.json"))
}

/// One lane's recorded build directories: `(name, worktree)`. A record whose own lane or name
/// disagrees with where it sits, or whose `worktree` is not an absolute path, is not a record:
/// nothing is counted for it and nothing is deleted for it.
fn target_records(root: &Path, lane: &str) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(root.join(LEDGER).join(TARGET_RECORDS).join(lane)) else {
        return Vec::new();
    };
    let mut found: Vec<(String, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let file = entry.path();
            let value: Value = serde_json::from_slice(&std::fs::read(&file).ok()?).ok()?;
            let name = value["name"].as_str().filter(|name| valid_id(name))?;
            let worktree = value["worktree"]
                .as_str()
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())?;
            (value["schema"] == TARGET_SCHEMA
                && value["lane"] == lane
                && file.file_name().and_then(|n| n.to_str()) == Some(&format!("{name}.json")))
            .then(|| (name.to_owned(), worktree))
        })
        .collect();
    found.sort();
    found
}

/// Whether a recorded worktree is POSITIVELY gone: the path is not found, and some directory
/// above it answers. "Could not be read" is not "gone": a worktree on a volume that is offline or
/// not ready, or behind a permission or I/O error, still exists, and its build directory is kept.
/// On Windows a missing drive reports every path on it as not found, the drive's own root
/// included, which is why an ancestor has to answer before the path counts as gone.
fn worktree_gone(path: &Path) -> bool {
    use std::io::ErrorKind::NotFound;
    if !path.is_absolute() {
        return false;
    }
    match std::fs::symlink_metadata(path) {
        Ok(_) => false,
        Err(error) if error.kind() == NotFound => path
            .ancestors()
            .skip(1)
            .find_map(|above| match std::fs::symlink_metadata(above) {
                Ok(_) => Some(true),
                Err(error) if error.kind() == NotFound => None,
                Err(_) => Some(false),
            })
            .unwrap_or(false),
        Err(_) => false,
    }
}

/// The build directory is always derived from the rule and the ids, never read back from a
/// record: a record cannot point the delete at another path.
fn target_dir(rule: &TargetRule, lane: &str, name: &str) -> PathBuf {
    rule.root.join(lane).join(name).join("target")
}

/// Validate the recorded target against its derived path and refuse linked ancestors or target.
/// Shared by preview and reclaim; the record never supplies the deletion path.
fn check_target(root: &Path, rule: &TargetRule, lane: &str, name: &str) -> Result<(), String> {
    let holder = rule.root.join(lane).join(name);
    // The record was written under another target root: the directory this rule derives is not
    // the one that was built in. Deleting nothing and saying "reclaimed" would drop the record
    // and leak the real directory, so the record stays and says why.
    let recorded = std::fs::read(target_record_file(root, lane, name))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|value| value["target"].as_str().map(PathBuf::from));
    if recorded.as_deref() != Some(holder.join("target").as_path()) {
        return Err("target_root_changed".to_owned());
    }
    for step in target_dir(rule, lane, name).ancestors() {
        match std::fs::symlink_metadata(step) {
            Ok(metadata) if is_link(&metadata) => return Err("linked_path".to_owned()),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("target_unreadable".to_owned()),
        }
    }
    Ok(())
}

fn reclaim_target(root: &Path, rule: &TargetRule, lane: &str, name: &str) -> Result<(), String> {
    check_target(root, rule, lane, name)?;
    let holder = rule.root.join(lane).join(name);
    remove_tree(&holder.join("target")).map_err(|_| "remove_failed".to_owned())?;
    let _ = std::fs::remove_dir(&holder);
    std::fs::remove_file(target_record_file(root, lane, name))
        .map_err(|_| "record_not_removed".to_owned())
}

/// The slot's build directory for `worktree` under the owner's rule (#360), and the directories
/// reclaimed on the way. `record: false` checks the cap and free-space floor before
/// the caller queues; `record: true` is called while holding the slot, so it is serialized: it
/// reclaims the lane's build directories whose worktree is positively gone, checks the floor and cap, and
/// records this one. Below the floor, other lanes may lose only eligible targets, never worktrees.
pub(crate) fn slot_target(
    root: &Path,
    rule: &TargetRule,
    lane: &str,
    worktree: &Path,
    record: bool,
) -> Result<(PathBuf, Vec<Value>), String> {
    // The caller holds slot.lock here. Never wait for sweep.lock: run_sweep takes these
    // in the other order and only tries the slot. Contention refuses this attempt.
    let _sweep_lock = if record {
        Some(sweep_lock(root)?)
    } else {
        None
    };
    let name = worktree
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| valid_id(name))
        .ok_or("the worktree directory name must be a workspace id (lowercase letters, digits, . _ -) to get a build directory")?
        .to_owned();
    let same = |other: &Path| {
        other == worktree
            || (other.canonicalize().ok().is_some()
                && other.canonicalize().ok() == worktree.canonicalize().ok())
    };
    let mut held = Vec::new();
    let mut reclaimed = Vec::new();
    let mut first_used = None;
    for (other, path) in target_records(root, lane) {
        if other == name {
            if same(&path) {
                first_used = std::fs::read(target_record_file(root, lane, &name))
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                    .and_then(|value| value["firstUsedAt"].as_u64());
                continue;
            }
            if !worktree_gone(&path) {
                return Err(format!(
                    "another worktree of lane {lane} already builds as {name}: {}",
                    path.display()
                ));
            }
        }
        if !worktree_gone(&path) {
            held.push(other);
        } else if record {
            match reclaim_target(root, rule, lane, &other) {
                Ok(()) => reclaimed.push(json!({"lane": lane, "name": other})),
                Err(_) => held.push(other),
            }
        }
    }
    // Sample again while holding the slot: space may have fallen during the queue wait.
    // This is a check, not a reservation, and never evicts a live target to make room.
    if rule.min_free_gb != 0 {
        let mut free = fs2::available_space(&rule.root).map_err(|_| {
            format!(
                "target root {} free space could not be measured (slot-targets.json minFreeGb); check the root and graphhelm workspace sweep",
                rule.root.display()
            )
        })?;
        if free < rule.min_free_gb * 1024_u64.pow(3) && record {
            let swept = sweep_targets(root, true, Some((lane, worktree)));
            if let Some(removed) = swept["removed"].as_array() {
                reclaimed.extend(removed.iter().cloned());
            }
            free = fs2::available_space(&rule.root).map_err(|_| {
                "target root free space could not be measured after reclaim".to_owned()
            })?;
        }
        // Only queue below the floor when another lane has records to inspect under the lock.
        // Never reclaim before acquiring the slot, and never reclaim this lane or current tree.
        if free < rule.min_free_gb * 1024_u64.pow(3)
            && (record
                || !all_target_records(root)
                    .iter()
                    .any(|(other, _, _)| other != lane))
        {
            return Err(format!(
                "target root {} has {} GB free, below the floor of {} GB (slot-targets.json minFreeGb); inspect graphhelm workspace sweep",
                rule.root.display(),
                free / 1024_u64.pow(3),
                rule.min_free_gb
            ));
        }
    }
    if first_used.is_none() && held.len() as u64 >= rule.cap {
        return Err(format!(
            "lane {lane} already holds {} build directories ({}); remove a worktree it no longer needs (its build directory is reclaimed by the next slot run or by workspace sweep), then run again",
            rule.cap,
            held.join(", ")
        ));
    }
    let target = target_dir(rule, lane, &name);
    if record {
        std::fs::create_dir_all(&target)
            .map_err(|_| "the build directory could not be created".to_owned())?;
        let stamp = now();
        write_record(
            &target_record_file(root, lane, &name),
            &json!({"schema": TARGET_SCHEMA, "lane": lane, "name": name,
                "worktree": worktree.to_string_lossy(), "target": target.to_string_lossy(),
                "firstUsedAt": first_used.unwrap_or(stamp), "lastUsedAt": stamp}),
        )
        .map_err(|_| "the build directory could not be recorded".to_owned())?;
    }
    Ok((target, reclaimed))
}

#[cfg(test)]
#[test]
fn slot_target_reclaims_gone_worktrees_before_refusing_the_held_slot_floor() {
    // The pre-queue refusal cannot reclaim: that operation must remain serialized. Exercise
    // the held-slot call directly, without a disk-filling stress test or a production test hook.
    // Defect: moving the floor ahead of reclaim, or deleting a live target under pressure.
    // Existing cap/reclaim tests never refuse for disk pressure. Cost: local temp files only.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let fast = dir.path().join("fast");
    let live = dir.path().join("wt-live");
    let gone = dir.path().join("wt-gone");
    for path in [root.join(LEDGER), fast.clone(), live.clone(), gone.clone()] {
        std::fs::create_dir_all(path).unwrap();
    }
    let configure = |floor| {
        std::fs::write(
            root.join(LEDGER).join(SLOT_TARGETS),
            json!({"targetRoot": fast, "minFreeGb": floor}).to_string(),
        )
        .unwrap();
        target_rule(&root).unwrap().unwrap()
    };
    let rule = configure(0);
    let (live_target, _) = slot_target(&root, &rule, "lane-a", &live, true).unwrap();
    let (gone_target, _) = slot_target(&root, &rule, "lane-a", &gone, true).unwrap();
    std::fs::remove_dir(&gone).unwrap();
    let rule = configure(4096);
    assert!(fs2::available_space(&fast).unwrap() < 4096 * 1024_u64.pow(3));
    let result = slot_target(&root, &rule, "lane-a", &live, true);
    assert!(
        result.is_err(),
        "held-slot check ignored the floor: {result:?}"
    );
    assert!(!gone_target.exists());
    assert!(!target_record_file(&root, "lane-a", "wt-gone").exists());
    assert!(live_target.is_dir());
    assert!(target_record_file(&root, "lane-a", "wt-live").is_file());
}

/// Every lane's recorded build directories for `workspace sweep` and `workspace slot status`.
fn all_target_records(root: &Path) -> Vec<(String, String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(root.join(LEDGER).join(TARGET_RECORDS)) else {
        return Vec::new();
    };
    let mut lanes: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|lane| valid_id(lane))
        .collect();
    lanes.sort();
    lanes
        .into_iter()
        .flat_map(|lane| {
            target_records(root, &lane)
                .into_iter()
                .map(move |(name, worktree)| (lane.clone(), name, worktree))
        })
        .collect()
}

/// Lane counts and the target root's free-space margin for `workspace slot status`.
pub(crate) fn target_counts(root: &Path) -> (Value, Value) {
    let mut counts = serde_json::Map::new();
    for (lane, _, _) in all_target_records(root) {
        let count = counts.get(&lane).and_then(Value::as_u64).unwrap_or(0);
        counts.insert(lane, json!(count + 1));
    }
    let space = target_rule(root).ok().flatten().map(|rule| {
        json!({"targetRoot": rule.root, "minFreeGb": rule.min_free_gb,
            "freeGb": fs2::available_space(&rule.root).ok().map(|bytes| bytes / 1024_u64.pow(3))})
    });
    (Value::Object(counts), space.unwrap_or(Value::Null))
}

/// Content, never ancestry, proves a squash-merged worktree has nothing left to land.
/// The caller holds the build slot throughout eligibility and deletion. Any uncertainty keeps it.
fn merged_target(
    root: &Path,
    rule: &TargetRule,
    lane: &str,
    name: &str,
    worktree: &Path,
) -> Result<(), (String, String)> {
    let keep = |reason: &str| (reason.to_owned(), String::new());
    check_target(root, rule, lane, name).map_err(|reason| keep(&reason))?;
    for path in worktree.ancestors() {
        let metadata = std::fs::symlink_metadata(path).map_err(|_| keep("worktree_unreadable"))?;
        if is_link(&metadata) {
            return Err(keep("linked_path"));
        }
    }
    let strict_git = |args: &[&str]| -> Result<String, (String, String)> {
        let output = Command::new("git")
            .arg("-C")
            .arg(worktree)
            .args(args)
            .output()
            .map_err(|error| ("merge_check_failed".to_owned(), error.to_string()))?;
        if !output.status.success() || !output.stderr.is_empty() {
            return Err((
                "merge_check_failed".to_owned(),
                format!(
                    "git {}: {} {}",
                    args[0],
                    output.status,
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            ));
        }
        String::from_utf8(output.stdout)
            .map(|text| text.trim().to_owned())
            .map_err(|_| keep("merge_check_failed"))
    };
    let top = strict_git(&["rev-parse", "--show-toplevel"])?;
    if Path::new(&top).canonicalize().ok() != worktree.canonicalize().ok() {
        return Err(keep("merge_check_failed"));
    }
    let target = target_dir(rule, lane, name);
    let exclusion = target.strip_prefix(worktree).ok().map(|path| {
        format!(
            ":(exclude,literal){}",
            path.to_string_lossy().replace('\\', "/")
        )
    });
    let mut args = vec!["status", "--porcelain", "--untracked-files=all", "--", "."];
    if let Some(exclusion) = &exclusion {
        args.push(exclusion);
    }
    if !strict_git(&args)?.is_empty() {
        return Err(keep("merged_dirty"));
    }
    // We own slot.lock; its synthetic unknown holder is ourselves. Waiter metadata from older
    // binaries has no worktree, so conservatively protect every target of that lane.
    for entry in
        std::fs::read_dir(root.join(LEDGER).join("slot")).map_err(|_| keep("merged_busy"))?
    {
        let path = entry.map_err(|_| keep("merged_busy"))?.path();
        if path.extension().is_some_and(|ext| ext == "ticket") && std::fs::File::open(path).is_err()
        {
            return Err(keep("merged_busy"));
        }
    }
    let status = super::workspace_slot::run_status(root);
    let data = status
        .output
        .data
        .filter(|_| status.exit_code == 0)
        .ok_or_else(|| keep("merged_busy"))?;
    let waiting = data["waiting"]
        .as_array()
        .ok_or_else(|| keep("merged_busy"))?;
    if waiting.iter().any(|waiter| {
        waiter["lane"].as_str().is_none_or(|other| other == lane)
            && waiter["worktree"].as_str().is_none_or(|other| {
                let other = Path::new(other);
                other == worktree
                    || other.canonicalize().ok().is_none()
                    || other.canonicalize().ok() == worktree.canonicalize().ok()
            })
    }) {
        return Err(keep("merged_busy"));
    }
    let cutoff = SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(30 * 60))
        .ok_or_else(|| keep("merged_recent"))?;
    let mut pending = vec![target];
    let mut seen = 0usize;
    while let Some(path) = pending.pop() {
        seen += 1;
        if seen > 1_000_000 {
            return Err(keep("merged_recent"));
        }
        let metadata = std::fs::symlink_metadata(&path).map_err(|_| keep("merged_recent"))?;
        if is_link(&metadata) {
            return Err(keep("linked_path"));
        }
        if metadata.modified().map_err(|_| keep("merged_recent"))? >= cutoff {
            return Err(keep("merged_recent"));
        }
        if metadata.is_dir() {
            for entry in std::fs::read_dir(path).map_err(|_| keep("merged_recent"))? {
                pending.push(entry.map_err(|_| keep("merged_recent"))?.path());
                if pending.len() > 1_000_000 {
                    return Err(keep("merged_recent"));
                }
            }
        }
    }
    let version = strict_git(&["--version"])?;
    let mut parts = version
        .strip_prefix("git version ")
        .unwrap_or("")
        .split('.');
    let major = parts.next().and_then(|part| part.parse::<u32>().ok());
    let minor = parts.next().and_then(|part| part.parse::<u32>().ok());
    if !matches!((major, minor), (Some(2), Some(38..)) | (Some(3..), Some(_))) {
        return Err((
            "merge_check_failed".to_owned(),
            "git >= 2.38 is required".to_owned(),
        ));
    }
    let main = strict_git(&["rev-parse", "refs/remotes/origin/main^{tree}"])?;
    let merged = strict_git(&[
        "merge-tree",
        "--write-tree",
        "refs/remotes/origin/main",
        "HEAD",
    ])?;
    let hash =
        |text: &str| matches!(text.len(), 40 | 64) && text.bytes().all(|c| c.is_ascii_hexdigit());
    if !hash(&main) || !hash(&merged) {
        return Err(keep("merge_check_failed"));
    }
    if main != merged {
        return Err(keep("not_merged"));
    }
    Ok(())
}

/// Reclaim orphan targets, plus targets whose existing worktrees are clean, idle and merged.
/// This does not remove a worktree or branch. Preview performs the same safety checks.
fn sweep_targets(root: &Path, apply: bool, held_lane: Option<(&str, &Path)>) -> Value {
    let mut removed = Vec::new();
    let mut kept = Vec::new();
    let rule = target_rule(root).ok().flatten();
    // A live build cannot start between our checks and reclaim. Do not wait for a holder.
    let slot_dir = root.join(LEDGER).join("slot");
    let slot = held_lane
        .is_none()
        .then(|| std::fs::create_dir_all(&slot_dir))
        .and_then(Result::ok)
        .and_then(|()| {
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .open(slot_dir.join("slot.lock"))
                .ok()
        })
        .filter(|file| file.try_lock().is_ok());
    for (lane, name, worktree) in all_target_records(root) {
        let entry = |reason: &str| json!({"lane": lane, "name": name, "reason": reason});
        if let Some((active, current)) = held_lane
            && (lane == active
                || worktree == current
                || current.canonicalize().is_err()
                || (worktree.canonicalize().ok().is_some()
                    && worktree.canonicalize().ok() == current.canonicalize().ok()))
        {
            kept.push(entry("merged_busy"));
            continue;
        }
        let Some(rule) = &rule else {
            kept.push(entry("no_target_root"));
            continue;
        };
        if slot.is_none() && held_lane.is_none() {
            kept.push(entry("merged_busy"));
            continue;
        }
        let merged = !worktree_gone(&worktree);
        if merged {
            if std::fs::symlink_metadata(&worktree).is_err() {
                kept.push(entry("worktree_unreadable"));
                continue;
            }
            if let Err((reason, message)) = merged_target(root, rule, &lane, &name, &worktree) {
                let mut item = entry(&reason);
                if !message.is_empty() {
                    item["message"] = json!(message);
                }
                kept.push(item);
                continue;
            }
        }
        if let Err(reason) = check_target(root, rule, &lane, &name) {
            kept.push(entry(&reason));
            continue;
        }
        let item = if merged {
            entry("merged")
        } else {
            json!({"lane": lane, "name": name})
        };
        if !apply {
            removed.push(item);
            continue;
        }
        match reclaim_target(root, rule, &lane, &name) {
            Ok(()) => removed.push(item),
            Err(reason) => kept.push(entry(&reason)),
        }
    }
    json!({"removed": removed, "kept": kept})
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
    let slash = |p: &Path| p.to_string_lossy().replace('\\', "/");
    let mut scan_error = None;
    let link_in_worktree = if linked {
        None
    } else {
        match first_link(&worktree, &worktree) {
            Ok(found) => found.map(|p| slash(&p)),
            Err((at, error)) => {
                scan_error = Some(json!({"path": slash(&at), "error": error}));
                None
            }
        }
    };
    json!({"lane": lane, "task": task, "branch": record["branch"], "state": record["state"],
        "linked": linked, "linkInWorktree": link_in_worktree, "scanError": scan_error,
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
    if !view["scanError"].is_null() {
        return Some("scan_failed");
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

/// Shared by manual CLI/HTTP sweeps, the Runtime tick and held-slot target reclaim.
/// The handle's lifetime covers eligibility checks and removal, never a sleep or build.
fn sweep_lock(root: &Path) -> Result<std::fs::File, String> {
    let dir = root.join(LEDGER);
    std::fs::create_dir_all(&dir)
        .map_err(|_| "the workspace sweep lock directory could not be created".to_owned())?;
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join("sweep.lock"))
        .map_err(|_| "the workspace sweep lock could not be opened".to_owned())?;
    file.try_lock()
        .map_err(|_| "the workspace sweep lock is busy or unavailable; retry later".to_owned())?;
    Ok(file)
}

pub(crate) fn run_sweep(root: &Path, apply: bool) -> Outcome {
    const COMMAND: &str = "workspace.sweep";
    let _sweep_lock = if apply {
        match sweep_lock(root) {
            Ok(lock) => Some(lock),
            Err(message) => {
                return refuse(
                    COMMAND,
                    crate::error_codes::GHCLI037_WORKSPACE_REFUSED,
                    &message,
                    "/root",
                );
            }
        }
    } else {
        None
    };
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
                    "link": view["linkInWorktree"], "scanError": view["scanError"]}));
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
        json!({"root": root.to_string_lossy(), "applied": apply, "removed": removed, "kept": kept,
            "targets": sweep_targets(root, apply, None)}),
    )
}
