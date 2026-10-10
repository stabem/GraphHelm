use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use super::{SlotRequest, run_admitted_slot, run_admitted_slot_in, run_status, slot_dir};

fn marker_command(log: &Path, tag: &str, millis: u64) -> Vec<String> {
    let log = log.to_string_lossy().replace('\'', "''");
    if cfg!(windows) {
        vec![
            "powershell".into(),
            "-NoProfile".into(),
            "-Command".into(),
            format!(
                "Add-Content -LiteralPath '{log}' \"{tag} start $env:CARGO_TARGET_DIR $env:CARGO_BUILD_JOBS\"; Start-Sleep -Milliseconds {millis}; Add-Content -LiteralPath '{log}' '{tag} end'"
            ),
        ]
    } else {
        vec![
            "sh".into(),
            "-c".into(),
            format!(
                "echo \"{tag} start $CARGO_TARGET_DIR $CARGO_BUILD_JOBS\" >> '{log}'; sleep {}; echo '{tag} end' >> '{log}'",
                millis as f64 / 1000.0
            ),
        ]
    }
}

fn request<'a>(
    root: &'a Path,
    lane: &'a str,
    label: &'a str,
    command: &'a [String],
) -> SlotRequest<'a> {
    SlotRequest {
        root,
        lane,
        label,
        jobs: 3,
        clean_workspace: false,
        shared: true,
        max_wait: None,
        priority: false,
        command,
    }
}

fn tags(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .filter(|line| line.trim().ends_with(" end"))
        .map(|line| line.trim().trim_end_matches(" end").to_owned())
        .collect()
}

fn wait_for_ticket(root: &Path, lane: &str) {
    let dir = slot_dir(root);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .any(|entry| {
                entry.path().extension().is_some_and(|ext| ext == "ticket")
                    && entry.path().with_extension("info").is_file()
                    && entry
                        .file_name()
                        .to_string_lossy()
                        .contains(&format!("-{lane}-"))
            })
        {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "{lane} never queued");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_holder(root: &Path, lane: &str) {
    let holder = slot_dir(root).join("holder.json");
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if std::fs::read_to_string(&holder)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .is_some_and(|value| value["lane"] == lane)
        {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{lane} never became holder"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn admitted_slot_serializes_real_children_in_arrival_order_and_cleans_tickets() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let log = Arc::new(dir.path().join("log.txt"));
    let tickets = slot_dir(&root);
    std::fs::create_dir_all(&tickets).unwrap();
    std::fs::write(tickets.join("000000000000000000000001-ghost-1.ticket"), b"").unwrap();
    let first_command = marker_command(&log, "a", 300);
    let first_root = root.clone();
    let first_log = log.clone();
    let first = std::thread::spawn(move || {
        let request = request(&first_root, "lane-a", "first", &first_command);
        run_admitted_slot(&request)
    });
    wait_for_holder(&root, "lane-a");
    let second_command = marker_command(&log, "b", 20);
    let second_root = root.clone();
    let second = std::thread::spawn(move || {
        let request = request(&second_root, "lane-b", "second", &second_command);
        run_admitted_slot(&request)
    });
    assert!(first.join().unwrap().output.ok);
    assert!(second.join().unwrap().output.ok);
    let lines = std::fs::read_to_string(&*first_log).unwrap();
    assert_eq!(lines.lines().count(), 4, "{lines}");
    assert!(
        lines.lines().next().unwrap().starts_with("a start"),
        "{lines}"
    );
    assert_eq!(lines.lines().nth(1).unwrap(), "a end", "{lines}");
    assert!(
        lines.lines().nth(2).unwrap().starts_with("b start"),
        "{lines}"
    );
    assert_eq!(lines.lines().nth(3).unwrap(), "b end", "{lines}");
    assert!(std::fs::read_dir(&tickets).unwrap().flatten().all(|entry| {
        entry
            .path()
            .extension()
            .is_none_or(|extension| extension != "ticket")
    }));
}

#[test]
fn private_status_reports_holder_and_waiter_order() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let log = dir.path().join("log.txt");
    let holder_command = marker_command(&log, "a", 700);
    let holder_root = root.clone();
    let holder = std::thread::spawn(move || {
        let request = request(&holder_root, "lane-a", "hold", &holder_command);
        run_admitted_slot(&request)
    });
    wait_for_holder(&root, "lane-a");
    let waiter_command = marker_command(&log, "b", 10);
    let waiter_root = root.clone();
    let waiter = std::thread::spawn(move || {
        let request = request(&waiter_root, "lane-b", "wait", &waiter_command);
        run_admitted_slot(&request)
    });
    wait_for_ticket(&root, "lane-b");
    let status = run_status(&root);
    let data = status.output.data.unwrap();
    assert_eq!(data["holder"]["lane"], "lane-a", "{data}");
    assert_eq!(data["holder"]["label"], "hold", "{data}");
    assert_eq!(data["waiting"][0]["lane"], "lane-b", "{data}");
    assert_eq!(data["waiting"][0]["label"], "wait", "{data}");
    assert!(holder.join().unwrap().output.ok);
    assert!(waiter.join().unwrap().output.ok);
}

#[test]
fn timed_out_waiter_resumes_at_its_original_place() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let log = dir.path().join("log.txt");
    let holder_command = marker_command(&log, "a", 900);
    let holder_root = root.clone();
    let holder = std::thread::spawn(move || {
        let request = request(&holder_root, "lane-a", "hold", &holder_command);
        run_admitted_slot(&request)
    });
    wait_for_holder(&root, "lane-a");
    let timed_command = marker_command(&log, "b", 10);
    let timed = request(&root, "lane-b", "same", &timed_command);
    let timed = SlotRequest {
        max_wait: Some(0.001),
        ..timed
    };
    let refused = run_admitted_slot(&timed);
    assert!(!refused.output.ok);
    assert!(
        refused.output.diagnostics[0]
            .message
            .contains("keep their place")
    );
    let later_command = marker_command(&log, "c", 10);
    let later_root = root.clone();
    let later = std::thread::spawn(move || {
        let request = request(&later_root, "lane-c", "later", &later_command);
        run_admitted_slot(&request)
    });
    wait_for_ticket(&root, "lane-c");
    let again_root = root.clone();
    let again_log = log.clone();
    let again = std::thread::spawn(move || {
        let again_command = marker_command(&again_log, "b", 10);
        let again = request(&again_root, "lane-b", "same", &again_command);
        run_admitted_slot(&again)
    });
    assert!(holder.join().unwrap().output.ok);
    assert!(again.join().unwrap().output.ok);
    assert!(later.join().unwrap().output.ok);
    assert_eq!(tags(&log), ["a", "b", "c"]);
}

#[test]
fn priority_is_allowed_only_for_listed_lanes_and_runs_next() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let ledger = root.join(".graphhelm-workspaces");
    std::fs::create_dir_all(&ledger).unwrap();
    std::fs::write(ledger.join("slot-priority-lanes"), "coordinator\n").unwrap();
    let log = dir.path().join("log.txt");
    let refused_command = marker_command(&log, "x", 10);
    let refused = SlotRequest {
        priority: true,
        ..request(&root, "lane-x", "jump", &refused_command)
    };
    let refused = run_admitted_slot(&refused);
    assert!(!refused.output.ok);
    assert!(!log.exists());
    let holder_command = marker_command(&log, "a", 500);
    let holder_root = root.clone();
    let holder = std::thread::spawn(move || {
        run_admitted_slot(&request(&holder_root, "lane-a", "hold", &holder_command))
    });
    wait_for_holder(&root, "lane-a");
    let normal_command = marker_command(&log, "b", 10);
    let normal_root = root.clone();
    let normal = std::thread::spawn(move || {
        run_admitted_slot(&request(&normal_root, "lane-b", "wait", &normal_command))
    });
    wait_for_ticket(&root, "lane-b");
    let priority_root = root.clone();
    let priority_log = log.clone();
    let priority = std::thread::spawn(move || {
        let priority_command = marker_command(&priority_log, "p", 10);
        let priority = SlotRequest {
            priority: true,
            ..request(&priority_root, "coordinator", "owner", &priority_command)
        };
        run_admitted_slot(&priority)
    });
    assert!(holder.join().unwrap().output.ok);
    assert!(priority.join().unwrap().output.ok);
    assert!(normal.join().unwrap().output.ok);
    assert_eq!(tags(&log), ["a", "p", "b"]);
}

#[test]
fn target_root_floor_cap_and_reclaim_are_observed_through_private_admission() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let fast = dir.path().join("fast");
    let rules = root.join(".graphhelm-workspaces");
    let first = dir.path().join("wt-first");
    let second = dir.path().join("wt-second");
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();
    std::fs::create_dir_all(&rules).unwrap();
    std::fs::write(
        rules.join("slot-targets.json"),
        serde_json::json!({"targetRoot": fast, "cap": 1, "minFreeGb": 0}).to_string(),
    )
    .unwrap();
    let log = dir.path().join("target.log");
    let command = marker_command(&log, "first", 10);
    let first_request = SlotRequest {
        shared: false,
        ..request(&root, "lane-a", "first", &command)
    };
    let first_outcome = run_admitted_slot_in(&first_request, &first);
    assert!(first_outcome.output.ok);
    assert_eq!(
        Path::new(
            first_outcome.output.data.as_ref().unwrap()["targetDir"]
                .as_str()
                .unwrap()
        ),
        fast.join("lane-a").join("wt-first").join("target")
    );
    let second_command = marker_command(&log, "second", 10);
    let second_request = SlotRequest {
        shared: false,
        ..request(&root, "lane-a", "second", &second_command)
    };
    let refused = run_admitted_slot_in(&second_request, &second);
    assert!(!refused.output.ok);
    assert!(refused.output.diagnostics[0].message.contains("cap"));
    std::fs::remove_dir_all(&first).unwrap();
    let reclaimed = run_admitted_slot_in(&second_request, &second);
    assert!(reclaimed.output.ok);
    assert_eq!(
        reclaimed.output.data.as_ref().unwrap()["reclaimedTargets"],
        serde_json::json!([{"lane":"lane-a","name":"wt-first"}])
    );
    std::fs::write(
        rules.join("slot-targets.json"),
        serde_json::json!({"targetRoot": fast, "cap": 1, "minFreeGb": 4096}).to_string(),
    )
    .unwrap();
    let floor_command = marker_command(&log, "floor", 10);
    let floor_request = SlotRequest {
        shared: false,
        ..request(&root, "lane-b", "floor", &floor_command)
    };
    let floor = run_admitted_slot_in(&floor_request, &second);
    assert!(!floor.output.ok);
    assert_eq!(floor.output.diagnostics[0].path, "/target");
    assert!(
        !std::fs::read_to_string(&log)
            .unwrap()
            .contains("floor start")
    );
}

#[test]
fn isolated_and_shared_targets_and_jobs_reach_the_child() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let cwd = dir.path().join("worktree");
    std::fs::create_dir_all(&cwd).unwrap();
    for shared in [false, true] {
        let log = dir
            .path()
            .join(if shared { "shared.log" } else { "own.log" });
        let command = marker_command(&log, "target", 0);
        let request = SlotRequest {
            shared,
            ..request(&root, "lane-a", "target", &command)
        };
        assert!(run_admitted_slot_in(&request, &cwd).output.ok);
        let text = std::fs::read_to_string(log).unwrap();
        let target = if shared {
            root.join("target-shared")
        } else {
            cwd.join("target")
        };
        assert!(
            text.contains(&format!("target start {} 3", target.display())),
            "{text}"
        );
    }
}

#[test]
fn failed_clean_never_starts_the_marker_child_and_releases_the_slot() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let cwd = dir.path().join("not-a-workspace");
    std::fs::create_dir_all(&cwd).unwrap();
    let log = dir.path().join("child.log");
    let command = marker_command(&log, "forbidden", 0);
    let request = SlotRequest {
        clean_workspace: true,
        ..request(&root, "lane-a", "failed-clean", &command)
    };
    let outcome = run_admitted_slot_in(&request, &cwd);
    assert!(!outcome.output.ok);
    assert_eq!(outcome.output.diagnostics[0].path, "/cleanWorkspace");
    assert!(!log.exists());
    assert!(
        std::fs::read_dir(slot_dir(&root))
            .unwrap()
            .flatten()
            .all(|entry| entry.path().extension().is_none_or(|ext| ext != "ticket"))
    );
}
