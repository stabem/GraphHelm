//! Private queue/target lifecycle observers. Public admission has no test bypass.
use super::{SlotRequest, run_admitted_slot_in};
use serde_json::Value;
use std::path::Path;
use std::process::Command;

fn run(args: &[&str]) -> (i32, Value) {
    use clap::Parser;
    let cli = crate::args::Cli::try_parse_from(
        ["graphhelm", "workspace"]
            .into_iter()
            .chain(args.iter().copied()),
    )
    .unwrap();
    let crate::args::TopLevel::Workspace(args) = cli.command else {
        panic!("workspace expected")
    };
    let outcome = super::super::workspace::run(&args);
    (
        outcome.exit_code,
        serde_json::to_value(outcome.output).unwrap(),
    )
}

fn slot_in(root: &Path, lane: &str, worktree: &Path, log: &Path) -> (i32, Value) {
    let _ = std::fs::remove_file(log);
    let path = log.to_string_lossy().replace('\'', "''");
    let command: Vec<String> = if cfg!(windows) {
        vec![
            "powershell".into(),
            "-NoProfile".into(),
            "-Command".into(),
            format!("Set-Content -LiteralPath '{path}' $env:CARGO_TARGET_DIR"),
        ]
    } else {
        vec![
            "sh".into(),
            "-c".into(),
            format!("printf '%s\\n' \"$CARGO_TARGET_DIR\" > '{path}'"),
        ]
    };
    let request = SlotRequest {
        root,
        lane,
        label: "target-observer",
        jobs: 6,
        clean_workspace: false,
        shared: false,
        max_wait: None,
        priority: false,
        command: &command,
    };
    let outcome = run_admitted_slot_in(&request, worktree);
    (
        outcome.exit_code,
        serde_json::to_value(outcome.output).unwrap(),
    )
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
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
    assert!(status.status.success(), "git {args:?}: {status:?}");
}

fn repo(dir: &Path) -> std::path::PathBuf {
    let repo = dir.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "--object-format=sha1"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    std::fs::write(repo.join(".gitignore"), "ignored/\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    repo
}

fn link_dir(link: &Path, target: &Path) {
    #[cfg(windows)]
    {
        let out = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
    }
    #[cfg(not(windows))]
    std::os::unix::fs::symlink(target, link).unwrap();
}

fn claim(root: &str, repo: &Path, lane: &str, task: &str) -> Value {
    let (code, reply) = run(&[
        "claim",
        "--root",
        root,
        "--lane",
        lane,
        "--task",
        task,
        "--repo",
        repo.to_str().unwrap(),
        "--base",
        "HEAD",
    ]);
    assert_eq!(code, 0, "{reply}");
    reply
}

fn release(root: &str, lane: &str, task: &str) {
    let (code, reply) = run(&["release", "--root", root, "--lane", lane, "--task", task]);
    assert_eq!(code, 0, "{reply}");
}

#[test]
fn merged_squash_target_is_reclaimed_only_when_clean_idle_and_old() {
    let dir = tempfile::tempdir().unwrap();
    let repo = repo(dir.path());
    git(&repo, &["branch", "-M", "main"]);
    let origin = dir.path().join("origin.git");
    git(&repo, &["init", "--bare", origin.to_str().unwrap()]);
    git(
        &repo,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git(&repo, &["push", "origin", "main"]);
    let tree = dir.path().join("fast").join("lane-a").join("wt-merged");
    git(
        &repo,
        &["worktree", "add", "-b", "feature", tree.to_str().unwrap()],
    );
    std::fs::write(tree.join("a.txt"), "landed\n").unwrap();
    git(&tree, &["commit", "-am", "feature"]);
    git(&repo, &["merge", "--squash", "feature"]);
    git(&repo, &["commit", "-m", "squashed"]);
    git(&repo, &["push", "origin", "main"]);
    let root = dir.path().join("root");
    let fast = dir.path().join("fast");
    let rules = root.join(".graphhelm-workspaces");
    std::fs::create_dir_all(&rules).unwrap();
    std::fs::write(
        rules.join("slot-targets.json"),
        serde_json::json!({"targetRoot": fast, "minFreeGb": 0}).to_string(),
    )
    .unwrap();
    assert_eq!(
        slot_in(&root, "lane-a", &tree, &dir.path().join("log")).0,
        0
    );
    let target = tree.join("target");
    let artifact = target.join("built.bin");
    std::fs::write(&artifact, b"build").unwrap();
    let age = || {
        let past = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
        for path in [&artifact, &target] {
            let mut options = std::fs::OpenOptions::new();
            options.read(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                options.access_mode(0x100).custom_flags(0x02000000);
            }
            options
                .open(path)
                .unwrap()
                .set_times(std::fs::FileTimes::new().set_modified(past))
                .unwrap();
        }
    };
    let sweep = |apply: bool| {
        let mut args = vec!["sweep", "--root", root.to_str().unwrap()];
        if apply {
            args.push("--apply");
        }
        let (code, reply) = run(&args);
        assert_eq!(code, 0, "{reply}");
        reply["data"]["targets"].clone()
    };
    age();
    let preview = sweep(false);
    assert_eq!(preview["removed"][0]["reason"], "merged", "{preview}");
    assert!(artifact.is_file());
    assert!(rules.join("targets/lane-a/wt-merged.json").is_file());
    git(&tree, &["update-ref", "-d", "refs/remotes/origin/main"]);
    assert_eq!(sweep(true)["kept"][0]["reason"], "merge_check_failed");
    assert!(artifact.is_file());
    git(&tree, &["update-ref", "refs/remotes/origin/main", "main"]);
    std::fs::write(tree.join("dirty.txt"), b"keep").unwrap();
    assert_eq!(sweep(true)["kept"][0]["reason"], "merged_dirty");
    std::fs::remove_file(tree.join("dirty.txt")).unwrap();
    std::fs::write(&artifact, b"recent").unwrap();
    assert_eq!(sweep(true)["kept"][0]["reason"], "merged_recent");
    age();
    let slot = rules.join("slot");
    std::fs::create_dir_all(&slot).unwrap();
    let ticket_path = slot.join("000-lane-a.ticket");
    let ticket = std::fs::File::create(&ticket_path).unwrap();
    ticket.lock().unwrap();
    std::fs::write(
        ticket_path.with_extension("info"),
        serde_json::json!({"lane": "lane-a", "worktree": tree}).to_string(),
    )
    .unwrap();
    assert_eq!(sweep(true)["kept"][0]["reason"], "merged_busy");
    std::fs::write(ticket_path.with_extension("info"), b"{}").unwrap();
    assert_eq!(sweep(true)["kept"][0]["reason"], "merged_busy");
    drop(ticket);
    let holder = std::fs::File::create(slot.join("slot.lock")).unwrap();
    holder.lock().unwrap();
    assert_eq!(sweep(true)["kept"][0]["reason"], "merged_busy");
    drop(holder);
    std::fs::write(tree.join("extra.txt"), b"unlanded").unwrap();
    git(&tree, &["add", "extra.txt"]);
    git(&tree, &["commit", "-m", "unlanded"]);
    assert_eq!(sweep(true)["kept"][0]["reason"], "not_merged");
    git(&tree, &["revert", "--no-edit", "HEAD"]);
    std::fs::write(repo.join("a.txt"), "later main edit\n").unwrap();
    git(&repo, &["commit", "-am", "later"]);
    git(&repo, &["push", "origin", "main"]);
    let later = sweep(true);
    assert!(!later["kept"].as_array().unwrap().is_empty(), "{later}");
    assert!(artifact.is_file());
    // Restore the local tracking ref only; the sweep must neither fetch nor consult the remote.
    git(&tree, &["update-ref", "refs/remotes/origin/main", "main~1"]);
    let saved = dir.path().join("saved-target");
    std::fs::rename(&target, &saved).unwrap();
    link_dir(&target, &saved);
    assert_eq!(sweep(true)["kept"][0]["reason"], "linked_path");
    assert!(saved.join("built.bin").is_file());
    #[cfg(windows)]
    std::fs::remove_dir(&target).unwrap();
    #[cfg(unix)]
    std::fs::remove_file(&target).unwrap();
    std::fs::rename(&saved, &target).unwrap();
    age();
    let applied = sweep(true);
    assert_eq!(applied["removed"][0]["reason"], "merged", "{applied}");
    assert!(!target.exists());
    assert!(!rules.join("targets/lane-a/wt-merged.json").exists());
    assert_eq!(
        std::fs::read_to_string(tree.join("a.txt")).unwrap(),
        "landed\n"
    );
    git(&repo, &["show-ref", "--verify", "refs/heads/feature"]);
    // Below the floor, preserve the current lane's target. Another lane may reclaim it under
    // the slot, but still must refuse its child when the one recheck remains below the floor.
    let log = dir.path().join("floor-child.txt");
    assert_eq!(slot_in(&root, "lane-a", &tree, &log).0, 0);
    std::fs::write(&artifact, b"build").unwrap();
    age();
    std::fs::write(
        rules.join("slot-targets.json"),
        serde_json::json!({"targetRoot": fast, "minFreeGb": 4096}).to_string(),
    )
    .unwrap();
    assert!(fs2::available_space(&fast).unwrap() < 4096 * 1024_u64.pow(3));
    assert_eq!(slot_in(&root, "lane-a", &tree, &log).0, 3);
    assert!(artifact.is_file());
    assert_eq!(slot_in(&root, "lane-b", &repo, &log).0, 3);
    assert!(!log.exists());
    assert!(!target.exists());
    assert!(tree.join("a.txt").is_file());
    git(&repo, &["show-ref", "--verify", "refs/heads/feature"]);
}

#[test]
fn slot_cap_ignores_gone_targets_until_locked_reclaim() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let fast = dir.path().join("fast");
    let rules = root.join(".graphhelm-workspaces");
    std::fs::create_dir_all(&rules).unwrap();
    std::fs::write(
        rules.join("slot-targets.json"),
        serde_json::json!({"targetRoot": fast, "cap": 1, "minFreeGb": 0}).to_string(),
    )
    .unwrap();
    let old = dir.path().join("wt-old");
    let next = dir.path().join("wt-next");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::create_dir_all(&next).unwrap();
    let log = dir.path().join("marker");
    let (code, reply) = slot_in(&root, "lane", &old, &log);
    assert_eq!(code, 0, "{reply}");
    let artifact = fast.join("lane/wt-old/target/built.bin");
    std::fs::write(&artifact, b"keep until locked").unwrap();
    std::fs::remove_dir(&old).unwrap();
    let lock = std::fs::File::create(rules.join("sweep.lock")).unwrap();
    lock.lock().unwrap();
    let (code, reply) = slot_in(&root, "lane", &next, &log);
    assert_eq!(code, 0, "{reply}");
    assert!(log.is_file());
    assert_eq!(std::fs::read(&artifact).unwrap(), b"keep until locked");
    assert!(rules.join("targets/lane/wt-old.json").is_file());
    drop(lock);
    let (code, reply) = slot_in(&root, "lane", &next, &log);
    assert_eq!(code, 0, "{reply}");
    assert_eq!(
        reply["data"]["reclaimedTargets"],
        serde_json::json!([{"lane": "lane", "name": "wt-old"}])
    );
    assert!(!fast.join("lane/wt-old").exists());
    assert!(!rules.join("targets/lane/wt-old.json").exists());
}

#[test]
fn the_slot_refuses_to_build_below_the_free_space_floor() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let fast = dir.path().join("fast");
    let tree = dir.path().join("wt-floor");
    let rules = root.join(".graphhelm-workspaces");
    for path in [&rules, &fast, &tree] {
        std::fs::create_dir_all(path).unwrap();
    }
    let log = dir.path().join("ran.txt");
    let configure = |floor: Value| {
        std::fs::write(
            rules.join("slot-targets.json"),
            serde_json::json!({"targetRoot": fast, "minFreeGb": floor}).to_string(),
        )
        .unwrap();
    };
    configure(4096.into());
    assert!(fs2::available_space(&fast).unwrap() < 4096 * 1024_u64.pow(3));
    let (code, reply) = slot_in(&root, "lane-a", &tree, &log);
    assert_eq!(code, 3, "{reply}");
    assert!(!log.exists(), "the command ran below the floor");
    assert!(!rules.join("slot").exists());
    let message = reply["diagnostics"][0]["message"].as_str().unwrap();
    assert!(
        message.contains("4096 GB")
            && message.contains("minFreeGb")
            && message.contains("graphhelm workspace sweep"),
        "{reply}"
    );
    for malformed in [
        serde_json::json!(-1),
        serde_json::json!("20"),
        serde_json::json!(4097),
    ] {
        configure(malformed);
        let (code, reply) = slot_in(&root, "lane-a", &tree, &log);
        assert_eq!(code, 3, "{reply}");
        assert_eq!(reply["diagnostics"][0]["path"], "/targetRoot");
        assert!(!log.exists());
    }
    configure(0.into());
    let (code, reply) = slot_in(&root, "lane-a", &tree, &log);
    assert_eq!(code, 0, "{reply}");
    assert!(log.exists());
    let built = fast.join("lane-a/wt-floor/target/keep.bin");
    std::fs::write(&built, b"keep").unwrap();
    configure(4096.into());
    assert_eq!(slot_in(&root, "lane-a", &tree, &log).0, 3);
    assert_eq!(std::fs::read(&built).unwrap(), b"keep");
    let (_, status) = run(&["slot", "status", "--root", root.to_str().unwrap()]);
    assert_eq!(status["data"]["targetSpace"]["minFreeGb"], 4096);
    assert!(status["data"]["targetSpace"]["freeGb"].is_number());
    assert_eq!(status["data"]["targets"]["lane-a"], 1);

    // Windows can measure the volume of a nonexistent directory, so use a genuinely
    // unavailable volume there; an absent path is sufficient for statvfs on other hosts.
    #[cfg(not(windows))]
    let missing = dir.path().join("missing-target-root");
    #[cfg(windows)]
    let missing = ('D'..='Z')
        .rev()
        .map(|letter| std::path::PathBuf::from(format!("{letter}:\\")))
        .find(|path| !path.exists() && fs2::available_space(path).is_err())
        .expect("an unavailable drive letter");
    std::fs::write(
        rules.join("slot-targets.json"),
        serde_json::json!({"targetRoot": missing}).to_string(),
    )
    .unwrap();
    let (_, status) = run(&["slot", "status", "--root", root.to_str().unwrap()]);
    assert_eq!(status["data"]["targetSpace"]["minFreeGb"], 20);
    assert!(status["data"]["targetSpace"]["freeGb"].is_null());
    let (code, reply) = slot_in(&root, "lane-a", &tree, &log);
    assert_eq!(code, 3, "{reply}");
    assert!(!log.exists());
    assert!(
        reply["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("could not be measured"),
        "{reply}"
    );
}

#[test]
fn the_slot_builds_on_the_owners_target_root_caps_a_lane_and_reclaims_orphans() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let fast = dir.path().join("fast");
    let rules = root.join(".graphhelm-workspaces");
    std::fs::create_dir_all(&rules).unwrap();
    let log = dir.path().join("target.txt");
    let worktree = |name: &str| {
        let path = dir.path().join("trees").join(name);
        std::fs::create_dir_all(&path).unwrap();
        path
    };
    let built = || std::fs::read_to_string(&log).unwrap().trim().to_owned();
    let (one, two, three) = (worktree("wt-one"), worktree("wt-two"), worktree("wt-three"));

    // Control: no rule file, the worktree's own target, and nothing recorded.
    let (code, reply) = slot_in(&root, "lane-a", &one, &log);
    assert_eq!(code, 0, "{reply}");
    assert_eq!(Path::new(&built()), one.join("target"));
    assert!(!rules.join("targets").exists());

    // A rule the slot cannot read is refused, not ignored.
    std::fs::write(rules.join("slot-targets.json"), r#"{"cap": 2}"#).unwrap();
    let (code, reply) = slot_in(&root, "lane-a", &one, &log);
    assert_eq!(code, 3, "{reply}");
    assert_eq!(reply["diagnostics"][0]["path"], "/targetRoot", "{reply}");
    assert!(!log.exists());

    std::fs::write(
        rules.join("slot-targets.json"),
        serde_json::json!({"targetRoot": fast, "cap": 2, "minFreeGb": 0}).to_string(),
    )
    .unwrap();
    for tree in [&one, &two] {
        let (code, reply) = slot_in(&root, "lane-a", tree, &log);
        assert_eq!(code, 0, "{reply}");
        let name = tree.file_name().unwrap();
        assert_eq!(
            Path::new(&built()),
            fast.join("lane-a").join(name).join("target")
        );
        assert!(
            rules
                .join("targets/lane-a")
                .join(format!("{}.json", name.to_string_lossy()))
                .is_file()
        );
    }
    // Running a worktree that already has its build directory is not a third one.
    assert_eq!(slot_in(&root, "lane-a", &one, &log).0, 0);
    // Another lane has its own cap.
    assert_eq!(slot_in(&root, "lane-b", &three, &log).0, 0);

    // The third worktree of the lane is refused before the command runs; both are named.
    std::fs::write(fast.join("lane-a/wt-one/target/built.bin"), b"x").unwrap();
    let (code, reply) = slot_in(&root, "lane-a", &three, &log);
    assert_eq!(code, 3, "{reply}");
    let message = reply["diagnostics"][0]["message"].as_str().unwrap();
    assert!(
        message.contains("wt-one") && message.contains("wt-two"),
        "{message}"
    );
    assert!(!log.exists(), "the command ran over the cap");
    assert!(fast.join("lane-a/wt-one/target/built.bin").is_file());

    let (code, status) = run(&["slot", "status", "--root", root.to_str().unwrap()]);
    assert_eq!(code, 0, "{status}");
    assert_eq!(
        status["data"]["targets"],
        serde_json::json!({"lane-a": 2, "lane-b": 1})
    );

    // A worktree that is gone frees its place: its build directory is reclaimed by the next run.
    std::fs::remove_dir_all(&one).unwrap();
    let (code, reply) = slot_in(&root, "lane-a", &three, &log);
    assert_eq!(code, 0, "{reply}");
    assert_eq!(
        reply["data"]["reclaimedTargets"],
        serde_json::json!([{"lane": "lane-a", "name": "wt-one"}])
    );
    assert!(!fast.join("lane-a/wt-one").exists());
    assert!(!rules.join("targets/lane-a/wt-one.json").exists());
    assert!(fast.join("lane-a/wt-two/target").is_dir());

    // The rule, the records and the cap belong to one slot root. A second slot (its own root,
    // its own target root) counts the same lane from zero, although this root has it at its cap.
    let (second, second_fast) = (dir.path().join("root-b"), dir.path().join("fast-b"));
    std::fs::create_dir_all(second.join(".graphhelm-workspaces")).unwrap();
    std::fs::write(
        second.join(".graphhelm-workspaces/slot-targets.json"),
        serde_json::json!({"targetRoot": second_fast, "cap": 1, "minFreeGb": 0}).to_string(),
    )
    .unwrap();
    let (code, reply) = slot_in(&second, "lane-a", &two, &log);
    assert_eq!(code, 0, "{reply}");
    assert_eq!(
        Path::new(&built()),
        second_fast.join("lane-a/wt-two/target")
    );
    assert_eq!(slot_in(&second, "lane-a", &three, &log).0, 3);
    assert!(fast.join("lane-a/wt-three/target").is_dir());

    // Sweep: a dry run lists the orphan and deletes nothing; --apply removes it; a build
    // directory reached through a link is kept, and what the link points at survives.
    let outside = dir.path().join("outside");
    std::fs::create_dir_all(outside.join("target")).unwrap();
    std::fs::write(outside.join("target/keep.txt"), b"keep").unwrap();
    std::fs::remove_dir_all(&two).unwrap();
    std::fs::remove_dir_all(&three).unwrap();
    std::fs::remove_dir_all(fast.join("lane-a/wt-three")).unwrap();
    // `mklink` reads a forward slash as a switch: the link path is joined part by part.
    link_dir(&fast.join("lane-a").join("wt-three"), &outside);
    let sweep = |apply: bool| {
        let mut args = vec!["sweep", "--root", root.to_str().unwrap()];
        if apply {
            args.push("--apply");
        }
        let (code, reply) = run(&args);
        assert_eq!(code, 0, "{reply}");
        reply["data"]["targets"].clone()
    };
    let dry = sweep(false);
    assert_eq!(dry["removed"].as_array().map(Vec::len), Some(2), "{dry}");
    assert!(fast.join("lane-a/wt-two/target").is_dir());
    let applied = sweep(true);
    assert_eq!(
        applied["removed"],
        serde_json::json!([{"lane": "lane-a", "name": "wt-two"}, {"lane": "lane-b", "name": "wt-three"}]),
        "{applied}"
    );
    assert_eq!(
        applied["kept"],
        serde_json::json!([{"lane": "lane-a", "name": "wt-three", "reason": "linked_path"}]),
        "{applied}"
    );
    assert!(!fast.join("lane-a/wt-two").exists());
    assert_eq!(
        std::fs::read(outside.join("target/keep.txt")).unwrap(),
        b"keep"
    );
}

#[test]
fn a_build_directory_is_reclaimed_only_when_its_worktree_is_positively_gone() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let fast = dir.path().join("fast");
    let rules = root.join(".graphhelm-workspaces");
    std::fs::create_dir_all(&rules).unwrap();
    let rule = |target_root: &Path| {
        std::fs::write(
            rules.join("slot-targets.json"),
            serde_json::json!({"targetRoot": target_root, "cap": 3, "minFreeGb": 0}).to_string(),
        )
        .unwrap();
    };
    rule(&fast);
    let log = dir.path().join("target.txt");
    let tree = dir.path().join("trees").join("wt-one");
    std::fs::create_dir_all(&tree).unwrap();
    assert_eq!(slot_in(&root, "lane-a", &tree, &log).0, 0);
    let record = rules.join("targets").join("lane-a").join("wt-one.json");
    let built = fast
        .join("lane-a")
        .join("wt-one")
        .join("target")
        .join("built.bin");
    std::fs::write(&built, b"x").unwrap();
    let original = std::fs::read_to_string(&record).unwrap();
    let with_worktree = |worktree: &str| {
        let mut value: Value = serde_json::from_str(&original).unwrap();
        value["worktree"] = worktree.into();
        std::fs::write(&record, value.to_string()).unwrap();
    };
    let sweep = || {
        let (code, reply) = run(&["sweep", "--root", root.to_str().unwrap(), "--apply"]);
        assert_eq!(code, 0, "{reply}");
        reply["data"]["targets"].clone()
    };
    let kept =
        |reason: &str| serde_json::json!([{"lane": "lane-a", "name": "wt-one", "reason": reason}]);

    // The worktree exists: an applied sweep keeps its build directory.
    let swept = sweep();
    assert_eq!(swept["kept"][0]["reason"], "merge_check_failed", "{swept}");
    assert!(built.is_file());

    // No worktree, or a relative one: not a record, so nothing is deleted for it.
    for worktree in ["", "trees/wt-one"] {
        with_worktree(worktree);
        let swept = sweep();
        assert_eq!(swept["removed"], serde_json::json!([]), "{swept}");
        assert!(built.is_file(), "deleted for worktree {worktree:?}");
    }

    // A worktree on a volume that does not answer cannot be read, which is not "gone". A second
    // worktree of the lane does not reclaim it either.
    #[cfg(windows)]
    {
        let offline = ('D'..='Z')
            .rev()
            .map(|letter| format!("{letter}:\\"))
            .find(|drive| !Path::new(drive).exists())
            .expect("a drive letter that is not mounted");
        with_worktree(&format!("{offline}trees\\wt-one"));
        let swept = sweep();
        assert_eq!(swept["kept"], kept("worktree_unreadable"), "{swept}");
        assert!(built.is_file());
        let other = dir.path().join("trees").join("wt-two");
        std::fs::create_dir_all(&other).unwrap();
        let (code, reply) = slot_in(&root, "lane-a", &other, &log);
        assert_eq!(code, 0, "{reply}");
        assert_eq!(reply["data"]["reclaimedTargets"], serde_json::json!([]));
        assert!(built.is_file());
        std::fs::remove_dir_all(&other).unwrap();
        let _ = sweep();
    }

    // The record names a build directory under another target root: nothing is deleted, the
    // record stays, and the sweep says why.
    with_worktree(
        dir.path()
            .join("trees")
            .join("never-there")
            .to_str()
            .unwrap(),
    );
    rule(&dir.path().join("fast-moved"));
    let swept = sweep();
    assert_eq!(swept["kept"], kept("target_root_changed"), "{swept}");
    assert!(built.is_file() && record.is_file());

    // Positively gone, on a volume that answers, under the rule it was built with: reclaimed.
    rule(&fast);
    let swept = sweep();
    assert_eq!(
        swept["removed"],
        serde_json::json!([{"lane": "lane-a", "name": "wt-one"}]),
        "{swept}"
    );
    assert!(!built.exists() && !record.exists());
}

#[test]
fn workspace_sweep_lock_serializes_manual_http_and_slot_reclaim() {
    let dir = tempfile::tempdir().unwrap();
    let repo = repo(dir.path());
    let root = dir.path().join("root");
    let root_s = root.to_str().unwrap();
    claim(root_s, &repo, "lane", "done");
    release(root_s, "lane", "done");
    let fast = dir.path().join("fast");
    let rules = root.join(".graphhelm-workspaces");
    std::fs::create_dir_all(&fast).unwrap();
    std::fs::write(
        rules.join("slot-targets.json"),
        serde_json::json!({"targetRoot": fast, "minFreeGb": 0}).to_string(),
    )
    .unwrap();
    let tree = dir.path().join("wt-old");
    std::fs::create_dir_all(&tree).unwrap();
    let log = dir.path().join("marker");
    assert_eq!(slot_in(&root, "lane", &tree, &log).0, 0);
    std::fs::remove_dir(&tree).unwrap();
    let next = dir.path().join("wt-next");
    std::fs::create_dir_all(&next).unwrap();
    let lock = std::fs::File::create(rules.join("sweep.lock")).unwrap();
    lock.lock().unwrap();
    let (code, reply) = run(&["sweep", "--root", root_s, "--apply"]);
    assert_ne!(code, 0, "{reply}");
    assert!(root.join("lane/done/wt/a.txt").is_file());
    assert_eq!(slot_in(&root, "lane", &next, &log).0, 0);
    assert!(log.exists());
    assert!(fast.join("lane/wt-old/target").is_dir());
    drop(lock);
    assert_eq!(run(&["sweep", "--root", root_s, "--apply"]).0, 0);
    assert!(!root.join("lane/done").exists());
    assert!(!fast.join("lane/wt-old/target").exists());
    assert_eq!(slot_in(&root, "lane", &next, &log).0, 0);
}
