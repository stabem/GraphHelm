//! Machine cleanup (#1578): `graphhelm workspace sweep --all [--apply]`.
//!
//! One command for everything a finished task leaves behind: the registered git worktrees of
//! `--repo` (the harness worktrees under `.claude/worktrees` among them), the build caches under
//! every slot and target root, the local branches of merged or closed PRs, and `git worktree
//! prune`. Without `--apply` nothing is changed and the report lists what would be.
//!
//! A worktree is removed only when no live process sits in it, no ancestor of its path is a link,
//! and either its PR is merged or closed (by `--pr-states`, by `gh`, or by its content already
//! being in `origin/main`), or it is clean, every commit is on a remote and it has been idle for
//! `--idle-hours`. Either way a commit that is on no remote and whose content is not in origin/main
//! keeps the worktree and its branch (`unpushed`): a branch name can be reused. Uncommitted work of a merged or closed worktree is saved to
//! `<leftovers>/<worktree name>/` first. A locked worktree (a harness locks a live session's own)
//! is skipped and reported, never unlocked. Links inside are unlinked, never followed.
//!
//! A build cache is a directory holding `CACHEDIR.TAG` (cargo writes one in every target dir) at
//! most three levels under a root and outside every registered worktree. It is removed when no
//! live process is inside and nothing in it changed for `--idle-hours`; parents it leaves empty
//! are removed too. Nothing else under a root (logs, notes, checkouts) is touched.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};

use super::workspace::{is_link, is_link_path, refuse, remove_tree};
use super::workspace_merged::{Worktree, git, remove_worktree, show, worktrees};
use crate::args::WorkspaceSweepArgs;
use crate::output::Outcome;

const COMMAND: &str = "workspace.sweep";
/// The machine's slot and target roots (LANES.md "Where lanes build").
const DEFAULT_ROOTS: [&str; 13] = [
    "D:/gh",
    "D:/gh/b",
    "D:/gh/c",
    "D:/gh/sub",
    "D:/ghb",
    "D:/ghc",
    "D:/ghopt",
    "C:/ghc",
    "C:/gh",
    "C:/w/a",
    "C:/w/b",
    "C:/w/c",
    "C:/w/d",
];
/// A cache scan that has to look at more entries than this keeps the cache.
const MAX_SCAN: usize = 2_000_000;
const CACHE_DEPTH: usize = 3;
const IDLE_DEPTH: usize = 2;

#[derive(Clone, Copy, PartialEq, Eq)]
enum PrState {
    Merged,
    Closed,
    Open,
}

type States = BTreeMap<String, PrState>;
type Places = [(u32, Vec<PathBuf>)];

fn parse_state(value: &str) -> Option<PrState> {
    match value {
        "MERGED" => Some(PrState::Merged),
        "CLOSED" => Some(PrState::Closed),
        "OPEN" => Some(PrState::Open),
        _ => None,
    }
}

/// PR state per head branch: from `--pr-states`, else one `gh pr list`. `Ok((_, Some(w)))`: gh
/// could not answer and only content already in origin/main counts as merged.
fn pr_states(args: &WorkspaceSweepArgs, repo: &Path) -> Result<(States, Option<String>), String> {
    let mut states = BTreeMap::new();
    if let Some(file) = &args.pr_states {
        let bytes = std::fs::read(file).map_err(|_| "--pr-states could not be read".to_owned())?;
        let map: BTreeMap<String, String> = serde_json::from_slice(&bytes)
            .map_err(|_| "--pr-states must be {\"<branch>\": \"MERGED|CLOSED|OPEN\"}".to_owned())?;
        for (branch, state) in map {
            let parsed =
                parse_state(&state).ok_or_else(|| format!("--pr-states: unknown state {state}"))?;
            states.insert(branch, parsed);
        }
        return Ok((states, None));
    }
    let output = graphhelm_process::command("gh")
        .current_dir(repo)
        .args([
            "pr",
            "list",
            "--state",
            "all",
            "--limit",
            "4000",
            "--json",
            "headRefName,state",
        ])
        .output();
    let rows: Vec<Value> = match output {
        Ok(out) if out.status.success() => serde_json::from_slice(&out.stdout).unwrap_or_default(),
        _ => {
            return Ok((
                states,
                Some(
                    "gh pr list failed; only content already in origin/main counts as merged"
                        .to_owned(),
                ),
            ));
        }
    };
    for row in rows {
        let branch = row["headRefName"].as_str();
        let state = row["state"].as_str().and_then(parse_state);
        if let (Some(branch), Some(state)) = (branch, state) {
            // An open PR on a reused branch name outranks an older closed or merged one.
            let entry = states.entry(branch.to_owned()).or_insert(state);
            if state == PrState::Open {
                *entry = PrState::Open;
            }
        }
    }
    Ok((states, None))
}

fn hash(text: &str) -> bool {
    matches!(text.len(), 40 | 64) && text.bytes().all(|c| c.is_ascii_hexdigit())
}

/// Content, never ancestry, proves a squash merge: merging `rev` into origin/main changes nothing.
fn content_merged(dir: &Path, rev: &str) -> bool {
    let Ok(main) = git(dir, &["rev-parse", "refs/remotes/origin/main^{tree}"]) else {
        return false;
    };
    let Ok(merged) = git(
        dir,
        &[
            "merge-tree",
            "--write-tree",
            "refs/remotes/origin/main",
            rev,
        ],
    ) else {
        return false;
    };
    let (main, merged) = (main.trim(), merged.trim());
    hash(main) && hash(merged) && main == merged
}

/// Every commit of `rev` is on some remote-tracking ref.
fn pushed(dir: &Path, rev: &str) -> bool {
    git(dir, &["rev-list", "--count", rev, "--not", "--remotes"])
        .is_ok_and(|count| count.trim() == "0")
}

/// No commit of `rev` can be lost by deleting its branch: each is on a remote-tracking ref, or
/// the whole of it is already in origin/main (a squash merge leaves the commits on no remote).
fn recoverable(dir: &Path, rev: &str) -> bool {
    pushed(dir, rev) || content_merged(dir, rev)
}

/// The worktree's own git dir files (index, HEAD, its reflog) are all older than `cutoff`.
fn worktree_idle(path: &Path, cutoff: SystemTime) -> bool {
    let Ok(gitdir) = git(path, &["rev-parse", "--absolute-git-dir"]) else {
        return false;
    };
    let gitdir = PathBuf::from(gitdir.trim());
    ["index", "HEAD", "logs/HEAD"]
        .iter()
        .map(|f| gitdir.join(f))
        .filter(|f| f.exists())
        .all(|f| {
            std::fs::symlink_metadata(&f)
                .and_then(|m| m.modified())
                .is_ok_and(|t| t < cutoff)
        })
}

/// Fails closed: an ancestor that cannot be read counts as a link.
fn has_linked_ancestor(path: &Path) -> bool {
    path.ancestors()
        .filter(|p| p.parent().is_some())
        .any(|p| std::fs::symlink_metadata(p).is_err() || is_link_path(p))
}

struct Decision {
    remove: bool,
    reason: &'static str,
}

fn keep(reason: &'static str) -> Decision {
    Decision {
        remove: false,
        reason,
    }
}

/// A process of the one snapshot sits in or runs from `dir`.
fn held(places: &Places, dir: &Path) -> bool {
    places.iter().any(|(_, at)| {
        at.iter()
            .any(|place| graphhelm_process_tree::path_is_inside(place, dir))
    })
}

fn decide(
    worktree: &Worktree,
    states: &States,
    cutoff: SystemTime,
    me: &Path,
    places: &Places,
) -> Decision {
    let path = &worktree.path;
    if !path.exists() {
        return keep("missing_prunable");
    }
    let short = worktree
        .branch
        .as_deref()
        .and_then(|b| b.strip_prefix("refs/heads/"));
    if matches!(short, Some("main" | "master")) {
        return keep("main_branch");
    }
    if graphhelm_process_tree::path_is_inside(me, path) {
        return keep("current_directory");
    }
    if has_linked_ancestor(path) {
        return keep("linked_path");
    }
    if held(places, path) {
        return keep("live");
    }
    let remove = |reason| Decision {
        remove: true,
        reason,
    };
    // Cheapest question first: `git status` on a big worktree on a spinning disk takes seconds,
    // so it runs only for the one case it decides. Merged or closed work needs no status here:
    // removal saves whatever is uncommitted to the leftovers folder first.
    let idle = worktree_idle(path, cutoff);
    match short.and_then(|b| states.get(b)).copied() {
        // A branch name can be reused after its PR ends: commits made since then count.
        Some(PrState::Merged | PrState::Closed) if !recoverable(path, &worktree.head) => {
            return keep("unpushed");
        }
        Some(PrState::Merged) => return remove("merged"),
        Some(PrState::Closed) => return remove("closed"),
        // An open PR is in flight; only being clean, pushed and idle frees its checkout.
        Some(PrState::Open) => {}
        // A fresh worktree cut from main is "content merged" too; idleness keeps a new claim.
        None if hash(&worktree.head) && idle && content_merged(path, &worktree.head) => {
            return remove("merged");
        }
        None => {}
    }
    if !hash(&worktree.head) || !pushed(path, &worktree.head) {
        return keep("unpushed");
    }
    if !idle {
        return keep("active");
    }
    // `--no-optional-locks`: the sweep's own read never refreshes the index. `normal` stops at
    // an untracked directory instead of walking a whole node_modules.
    match git(
        path,
        &[
            "--no-optional-locks",
            "status",
            "--porcelain",
            "--untracked-files=normal",
        ],
    ) {
        Ok(status) if status.trim().is_empty() => remove("pushed_idle"),
        Ok(_) => keep("dirty_unmerged"),
        Err(_) => keep("status_failed"),
    }
}

fn bump(counts: &mut BTreeMap<String, u64>, key: &str) {
    *counts.entry(key.to_owned()).or_default() += 1;
}

/// Directories named `target` holding `CACHEDIR.TAG` up to `depth` levels below `dir`, never
/// through a link and never inside `skip` (registered worktrees and nested roots; an ancestor of
/// one is still walked). Links met are counted. The type of each entry comes from the directory
/// listing itself, so only a `target` directory costs a probe: a spinning disk answered about 30
/// metadata calls a second on the real machine.
fn find_caches(dir: &Path, skip: &[PathBuf], depth: usize, out: &mut Vec<PathBuf>) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut links = 0;
    for entry in entries.filter_map(Result::ok) {
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if is_link(&metadata) {
            links += 1;
            continue;
        }
        let path = entry.path();
        if !metadata.is_dir()
            || skip
                .iter()
                .any(|s| graphhelm_process_tree::path_is_inside(&path, s))
        {
            continue;
        }
        if entry.file_name() == "target" && path.join("CACHEDIR.TAG").is_file() {
            out.push(path);
        } else if depth > 1 {
            links += find_caches(&path, skip, depth - 1, out);
        }
    }
    links
}

/// No entry up to `IDLE_DEPTH` levels under `dir` (links not followed) was modified at or after
/// `cutoff`. A build creates or replaces files in `target/<profile>/deps` and `.fingerprint`,
/// which moves the mtime of those depth-2 directories, so the shallow walk sees every build
/// without reading the tens of thousands of files each one holds (on a spinning disk the deep
/// walk took more than half an hour across the machine's caches).
fn cache_idle(dir: &Path, cutoff: SystemTime) -> bool {
    let mut pending = vec![(dir.to_path_buf(), 0usize)];
    let mut seen = 0usize;
    while let Some((path, depth)) = pending.pop() {
        seen += 1;
        if seen > MAX_SCAN {
            return false;
        }
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            return false;
        };
        if !metadata.modified().is_ok_and(|t| t < cutoff) {
            return false;
        }
        if depth < IDLE_DEPTH && metadata.is_dir() && !is_link_path(&path) {
            let Ok(entries) = std::fs::read_dir(&path) else {
                return false;
            };
            pending.extend(
                entries
                    .filter_map(Result::ok)
                    .map(|e| (e.path(), depth + 1)),
            );
        }
    }
    true
}

struct Report {
    removed: Vec<Value>,
    kept: BTreeMap<String, u64>,
    errors: Vec<Value>,
}

impl Report {
    fn new() -> Self {
        Self {
            removed: Vec::new(),
            kept: BTreeMap::new(),
            errors: Vec::new(),
        }
    }
}

fn sweep_caches(
    args: &WorkspaceSweepArgs,
    roots: &[PathBuf],
    worktree_paths: &[PathBuf],
    cutoff: SystemTime,
    me: &Path,
    places: &Places,
) -> (Report, usize, Vec<String>) {
    let mut report = Report::new();
    let mut links = 0;
    let mut scanned = Vec::new();
    for root in roots {
        if !root.is_dir() || has_linked_ancestor(root) {
            continue;
        }
        scanned.push(show(root));
        let mut skip = worktree_paths.to_vec();
        skip.extend(
            roots
                .iter()
                .filter(|r| *r != root && graphhelm_process_tree::path_is_inside(r, root))
                .cloned(),
        );
        let mut caches = Vec::new();
        links += find_caches(root, &skip, CACHE_DEPTH, &mut caches);
        for cache in caches {
            if graphhelm_process_tree::path_is_inside(me, &cache) {
                bump(&mut report.kept, "current_directory");
                continue;
            }
            if held(places, &cache) {
                bump(&mut report.kept, "live");
                continue;
            }
            if !cache_idle(&cache, cutoff) {
                bump(&mut report.kept, "active");
                continue;
            }
            if args.apply && !graphhelm_process_tree::processes_inside(&cache).is_empty() {
                bump(&mut report.kept, "live");
                continue;
            }
            let mut row = json!({"path": show(&cache)});
            if args.apply {
                if let Err(e) = remove_tree(&cache) {
                    row["error"] = json!(e.to_string());
                    report.errors.push(row);
                    continue;
                }
                // Parents left empty, up to (never including) the root.
                let mut parent = cache.parent();
                while let Some(dir) = parent {
                    if dir == root.as_path()
                        || !dir.starts_with(root)
                        || std::fs::remove_dir(dir).is_err()
                    {
                        break;
                    }
                    parent = dir.parent();
                }
            }
            report.removed.push(row);
        }
    }
    (report, links, scanned)
}

fn short_branch(worktree: &Worktree) -> Option<&str> {
    worktree
        .branch
        .as_deref()
        .and_then(|b| b.strip_prefix("refs/heads/"))
}

/// Local branches of merged or closed PRs that no remaining worktree has checked out.
fn sweep_branches(
    args: &WorkspaceSweepArgs,
    repo: &Path,
    states: &States,
    mut chosen: Vec<String>,
    remaining: &[String],
) -> Report {
    let mut report = Report::new();
    if let Ok(listed) = git(
        repo,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    ) {
        for branch in listed.lines().map(str::trim).filter(|b| !b.is_empty()) {
            if matches!(branch, "main" | "master")
                || chosen.iter().any(|b| b == branch)
                || remaining.iter().any(|b| b == branch)
            {
                continue;
            }
            let merged = match states.get(branch) {
                Some(PrState::Merged | PrState::Closed) => true,
                Some(PrState::Open) => false,
                // No PR on record: kept. A content check per branch costs two git runs, and the
                // machine holds over a thousand local branches; a worktree's branch got one above.
                None => false,
            };
            if !merged {
                continue;
            }
            if recoverable(repo, &format!("refs/heads/{branch}")) {
                chosen.push(branch.to_owned());
            } else {
                bump(&mut report.kept, "unpushed");
            }
        }
    }
    for branch in chosen {
        let mut row = json!({"name": branch});
        if args.apply
            && let Err(message) = git(repo, &["branch", "-D", &branch])
        {
            row["error"] = json!(message);
            report.errors.push(row);
            continue;
        }
        report.removed.push(row);
    }
    report
}

pub(crate) fn run(args: &WorkspaceSweepArgs) -> Outcome {
    let refused = |message: &str, path: &str| {
        refuse(
            COMMAND,
            crate::error_codes::GHCLI037_WORKSPACE_REFUSED,
            message,
            path,
        )
    };
    let repo = args.repo.clone().unwrap_or_else(|| PathBuf::from("."));
    let all = match worktrees(&repo) {
        Ok(all) if !all.is_empty() => all,
        Ok(_) => return refused("git listed no worktrees for this repository", "/repo"),
        Err(message) => return refused(&message, "/repo"),
    };
    let (states, warning) = match pr_states(args, &repo) {
        Ok(found) => found,
        Err(message) => return refused(&message, "/prStates"),
    };
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(args.idle_hours.saturating_mul(3600)))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let me = std::env::current_dir().unwrap_or_default();
    // One process listing for the whole run; a removal re-asks just before it acts.
    let places = graphhelm_process_tree::process_places();

    let mut worktree_report = Report::new();
    let mut locked = Vec::new();
    let mut chosen_branches = Vec::new();
    let mut remaining_branches = Vec::new();
    for worktree in all.iter().skip(1) {
        if let Some(reason) = &worktree.locked {
            locked.push(json!({"path": show(&worktree.path), "reason": reason}));
            remaining_branches.extend(short_branch(worktree).map(str::to_owned));
            continue;
        }
        let decision = decide(worktree, &states, cutoff, &me, &places);
        if !decision.remove {
            bump(&mut worktree_report.kept, decision.reason);
            remaining_branches.extend(short_branch(worktree).map(str::to_owned));
            continue;
        }
        let name = worktree.path.file_name().map_or_else(
            || "worktree".to_owned(),
            |n| n.to_string_lossy().into_owned(),
        );
        // A process that arrived after the snapshot keeps the worktree; it is never stopped.
        if args.apply && !graphhelm_process_tree::processes_inside(&worktree.path).is_empty() {
            bump(&mut worktree_report.kept, "live");
            remaining_branches.extend(short_branch(worktree).map(str::to_owned));
            continue;
        }
        let mut row = if args.apply {
            remove_worktree(&repo, worktree, &args.leftovers.join(&name))
        } else {
            json!({"path": show(&worktree.path)})
        };
        row["reason"] = json!(decision.reason);
        if row.get("error").is_some() {
            worktree_report.errors.push(row);
            remaining_branches.extend(short_branch(worktree).map(str::to_owned));
            continue;
        }
        if let Some(branch) = short_branch(worktree) {
            if decision.reason == "pushed_idle" {
                remaining_branches.push(branch.to_owned());
            } else {
                chosen_branches.push(branch.to_owned());
            }
        }
        worktree_report.removed.push(row);
    }
    remaining_branches.extend(all.first().and_then(short_branch).map(str::to_owned));
    let branch_report = sweep_branches(args, &repo, &states, chosen_branches, &remaining_branches);

    let prune: &[&str] = if args.apply {
        &["worktree", "prune", "-v"]
    } else {
        &["worktree", "prune", "-n", "-v"]
    };
    let pruned = git(&repo, prune)
        .map(|out| out.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0);

    let roots: Vec<PathBuf> = if args.sweep_roots.is_empty() {
        DEFAULT_ROOTS.iter().map(PathBuf::from).collect()
    } else {
        args.sweep_roots.clone()
    };
    let worktree_paths: Vec<PathBuf> = all.iter().map(|w| w.path.clone()).collect();
    let (cache_report, links, scanned) =
        sweep_caches(args, &roots, &worktree_paths, cutoff, &me, &places);

    let errors: Vec<Value> = [&worktree_report, &branch_report, &cache_report]
        .iter()
        .flat_map(|r| r.errors.iter().cloned())
        .collect();
    let ok = errors.is_empty();
    let counts = json!({
        "worktreesRemoved": worktree_report.removed.len(),
        "worktreesKept": worktree_report.kept.values().sum::<u64>(),
        "worktreesLocked": locked.len(),
        "branchesRemoved": branch_report.removed.len(),
        "branchesKept": branch_report.kept.values().sum::<u64>(),
        "pruned": pruned,
        "cachesRemoved": cache_report.removed.len(),
        "cachesKept": cache_report.kept.values().sum::<u64>(),
        "linksNotFollowed": links,
        "errors": errors.len(),
    });
    let mut outcome = Outcome::success(
        COMMAND,
        json!({"all": true, "applied": args.apply, "ok": ok, "counts": counts,
            "leftovers": show(&args.leftovers), "roots": scanned, "warning": warning,
            "worktrees": {"removed": worktree_report.removed, "kept": worktree_report.kept,
                "locked": locked},
            "branches": branch_report.removed,
            "caches": {"removed": cache_report.removed, "kept": cache_report.kept},
            "errors": errors}),
    );
    if !ok {
        // An incomplete cleanup must not read as success to a script or the scheduled run.
        outcome.exit_code = 3;
    }
    outcome
}
