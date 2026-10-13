use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use super::{
    BatchRun, BatchSpec, SlotRequest, batch_boundary, open_build_admission, parse_slot_batch,
    run_admitted_slot, run_admitted_slot_in, run_admitted_slot_in_with_batch,
    run_slot_with_batch_payload, run_status, slot_dir,
};

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

#[test]
fn batch_parser_rejects_invalid_tail_before_any_slot_effect() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let first = vec![
        "cargo".into(),
        "test".into(),
        "-pfoo".into(),
        "--test=cli".into(),
    ];
    let raw = r#"{"schema":"graphhelm.slot-batch/1","commands":[["cargo","test","-pfoo","--test=cli"],["cargo","test","-pfoo","--test=cli"],["cargo","test","-pfoo","--test=cli"],["sh","-c","touch marker"]],"budgetSeconds":10,"leaseSeconds":1}"#;
    assert!(parse_slot_batch(raw, &first).is_err());
    let first_request = request(&root, "lane", "batch", &first);
    let outcome = run_slot_with_batch_payload(&first_request, Some(raw), std::time::Instant::now());
    assert!(!outcome.output.ok);
    assert!(!root.exists());
    let expired = serde_json::json!({"schema":"graphhelm.slot-batch/1", "commands":[first],
        "budgetSeconds":180, "deadlineUnixMs":1, "leaseSeconds":30})
    .to_string();
    assert_eq!(
        parse_slot_batch(&expired, &first).ok().unwrap().budget,
        Duration::ZERO
    );
    let result =
        run_slot_with_batch_payload(&first_request, Some(&expired), std::time::Instant::now());
    let data = result.output.data.unwrap();
    assert_eq!(data["batch"]["stopReason"], "deadline");
    assert_eq!(data["batch"]["completed"], serde_json::json!([]));
}

#[test]
fn batch_boundary_checks_deadline_before_first_and_lease_only_for_next() {
    assert_eq!(
        batch_boundary(
            Duration::from_secs(2),
            Duration::from_secs(0),
            Duration::from_secs(2),
            Duration::from_secs(10),
            0
        ),
        Some("deadline")
    );
    assert_eq!(
        batch_boundary(
            Duration::from_secs(1),
            Duration::from_secs(1),
            Duration::from_secs(2),
            Duration::from_secs(1),
            0
        ),
        None
    );
    assert_eq!(
        batch_boundary(
            Duration::from_secs(1),
            Duration::from_secs(1),
            Duration::from_secs(2),
            Duration::from_secs(1),
            1
        ),
        Some("leaseBoundary")
    );
}

#[test]
fn batch_child_failure_keeps_prefix_and_stops_children() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let log = dir.path().join("log.txt");
    let mut first = marker_command(&log, "first", 0);
    first.last_mut().unwrap().push_str("; exit 7");
    let second = marker_command(&log, "second", 1);
    let request = request(&root, "lane", "batch", &first);
    let outcome = run_admitted_slot_in_with_batch(
        &request,
        dir.path(),
        Some(BatchRun {
            spec: BatchSpec {
                commands: vec![first.clone(), second],
                budget: Duration::from_secs(10),
                lease: Duration::from_secs(60),
            },
            started: std::time::Instant::now(),
        }),
    );
    let data = outcome.output.data.unwrap();
    assert_eq!(data["exitCode"], 7);
    assert_eq!(data["batch"]["stopReason"], "childFailure");
    assert_eq!(data["batch"]["completed"].as_array().unwrap().len(), 1);
    assert_eq!(data["batch"]["remaining"], serde_json::json!([1]));
    assert_eq!(tags(&log), ["first"]);

    // A failed child must release the shared authority so a later build can enter.
    let retry = marker_command(&log, "retry", 0);
    let retry_request = request(&root, "lane", "retry", &retry);
    let retry_outcome = run_admitted_slot_in(&retry_request, dir.path());
    assert!(
        retry_outcome.output.ok,
        "retry child failed: {:?}",
        retry_outcome.output
    );
    assert_eq!(tags(&log), ["first", "retry"]);
}

#[test]
fn invalid_build_admission_directory_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let invalid = dir.path().join("authority-file");
    std::fs::write(&invalid, b"not a directory").unwrap();

    // Contract: an unusable authority refuses admission instead of falling back.
    // This catches a regression that silently permits independent build execution.
    // Cost: one local filesystem probe; no child process or network.
    assert!(open_build_admission(&invalid).is_err());
}

#[test]
fn batch_child_environment_is_cleared_in_child() {
    if std::env::var_os("GRAPHHELM_SLOT_BATCH").is_none() {
        let output = graphhelm_process::command(std::env::current_exe().unwrap())
            .args(["--exact", "commands::workspace_slot::workspace_slot_tests::batch_child_environment_is_cleared_in_child", "--test-threads=2", "--nocapture"])
            .env("GRAPHHELM_SLOT_BATCH", "sentinel")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "{output:?}");
        assert!(
            stdout.contains("1 passed"),
            "child observer did not run: {stdout}"
        );
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let marker = dir.path().join("env.txt");
    let marker_text = marker.to_string_lossy().replace('\'', "''");
    let command = if cfg!(windows) {
        vec![
            "powershell".into(),
            "-NoProfile".into(),
            "-Command".into(),
            format!(
                "if (Test-Path Env:GRAPHHELM_SLOT_BATCH) {{ Set-Content -LiteralPath '{marker_text}' present }} else {{ Set-Content -LiteralPath '{marker_text}' absent }}"
            ),
        ]
    } else {
        vec![
            "sh".into(),
            "-c".into(),
            format!(
                "if [ -n \"$GRAPHHELM_SLOT_BATCH\" ]; then printf present > '{}'; else printf absent > '{}'; fi",
                marker.display(),
                marker.display()
            ),
        ]
    };
    for batch in [false, true] {
        let request = request(&root, "env", "batch", &command);
        let batch = batch.then(|| BatchRun {
            spec: BatchSpec {
                commands: vec![command.clone()],
                budget: Duration::from_secs(10),
                lease: Duration::from_secs(60),
            },
            started: std::time::Instant::now(),
        });
        let outcome = run_admitted_slot_in_with_batch(&request, dir.path(), batch);
        assert!(outcome.output.ok);
        assert_eq!(std::fs::read_to_string(&marker).unwrap().trim(), "absent");
    }
}

#[test]
fn batch_lease_boundary_releases_slot_after_completed_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let log = dir.path().join("log.txt");
    let release = dir.path().join("release.flag");
    let release_text = release.to_string_lossy().replace('\'', "''");
    let log_text = log.to_string_lossy().replace('\'', "''");
    let first = if cfg!(windows) {
        vec![
            "powershell".into(),
            "-NoProfile".into(),
            "-Command".into(),
            format!(
                "Add-Content -LiteralPath '{log_text}' \"first start $env:CARGO_TARGET_DIR $env:CARGO_BUILD_JOBS\"; for ($i=0; $i -lt 1000 -and -not (Test-Path '{release_text}'); $i++) {{ Start-Sleep -Milliseconds 10 }}; if (-not (Test-Path '{release_text}')) {{ exit 91 }}; Add-Content -LiteralPath '{log_text}' 'first end'"
            ),
        ]
    } else {
        vec![
            "sh".into(),
            "-c".into(),
            format!(
                "echo \"first start $CARGO_TARGET_DIR $CARGO_BUILD_JOBS\" >> '{}'; for i in $(seq 1 1000); do [ -f '{}' ] && break; sleep .01; done; [ -f '{}' ] || exit 91; echo 'first end' >> '{}'",
                log.display(),
                release.display(),
                release.display(),
                log.display()
            ),
        ]
    };
    let second = marker_command(&log, "second", 1);
    let batch_root = root.clone();
    let batch_cwd = dir.path().to_owned();
    let batch_first = first.clone();
    let batch_second = second.clone();
    let batch = std::thread::spawn(move || {
        let first_request = request(&batch_root, "lane", "batch", &batch_first);
        run_admitted_slot_in_with_batch(
            &first_request,
            &batch_cwd,
            Some(BatchRun {
                spec: BatchSpec {
                    commands: vec![batch_first.clone(), batch_second],
                    budget: Duration::from_secs(10),
                    lease: Duration::from_millis(10),
                },
                started: std::time::Instant::now(),
            }),
        )
    });
    wait_for_holder(&root, "lane");
    let contender_root = root.clone();
    let contender_cwd = dir.path().to_owned();
    let contender_command = second.clone();
    let contender = std::thread::spawn(move || {
        let contender_request = request(&contender_root, "contender", "after", &contender_command);
        run_admitted_slot_in(&contender_request, &contender_cwd)
    });
    wait_for_ticket(&root, "contender");
    std::fs::write(&release, b"go").unwrap();
    let outcome = batch.join().unwrap();
    let released = contender.join().unwrap();
    let data = outcome.output.data.unwrap();
    assert_eq!(data["batch"]["stopReason"], "leaseBoundary");
    assert_eq!(data["batch"]["remaining"], serde_json::json!([1]));
    assert!(released.output.ok);
    let lines = std::fs::read_to_string(&log).unwrap();
    let target = super::shared_target(&root).display().to_string();
    assert_eq!(
        lines.lines().map(str::to_owned).collect::<Vec<_>>(),
        vec![
            format!("first start {target} 3"),
            "first end".to_owned(),
            format!("second start {target} 3"),
            "second end".to_owned()
        ]
    );
}

#[test]
fn batch_success_runs_two_children_with_one_target_and_jobs() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let log = dir.path().join("log.txt");
    let first = marker_command(&log, "first", 1);
    let second = marker_command(&log, "second", 1);
    let request = request(&root, "success", "batch", &first);
    let outcome = run_admitted_slot_in_with_batch(
        &request,
        dir.path(),
        Some(BatchRun {
            spec: BatchSpec {
                commands: vec![first.clone(), second],
                budget: Duration::from_secs(10),
                lease: Duration::from_secs(60),
            },
            started: std::time::Instant::now(),
        }),
    );
    let data = outcome.output.data.unwrap();
    assert_eq!(data["batch"]["stopReason"], "exhausted");
    assert_eq!(data["batch"]["completed"].as_array().unwrap().len(), 2);
    assert_eq!(data["batch"]["remaining"], serde_json::json!([]));
    let text = std::fs::read_to_string(&log).unwrap();
    let target = super::shared_target(&root).display().to_string();
    assert_eq!(
        text.lines().map(str::to_owned).collect::<Vec<_>>(),
        vec![
            format!("first start {target} 3"),
            "first end".to_owned(),
            format!("second start {target} 3"),
            "second end".to_owned(),
        ]
    );
}

#[test]
fn batch_budget_expires_in_queue_without_spawning_first_child() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let child_log = dir.path().join("child.log");
    std::fs::create_dir_all(slot_dir(&root)).unwrap();
    let slot = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(slot_dir(&root).join("slot.lock"))
        .unwrap();
    slot.lock().unwrap();
    let child = marker_command(&child_log, "child", 1);
    let request = request(&root, "queued", "deadline", &child);
    let outcome = run_admitted_slot_in_with_batch(
        &request,
        dir.path(),
        Some(BatchRun {
            spec: BatchSpec {
                commands: vec![child.clone()],
                budget: Duration::from_millis(10),
                lease: Duration::from_secs(60),
            },
            started: std::time::Instant::now(),
        }),
    );
    let data = outcome.output.data.unwrap();
    assert_eq!(data["batch"]["stopReason"], "deadline");
    assert_eq!(data["batch"]["completed"], serde_json::json!([]));
    assert_eq!(data["batch"]["remaining"], serde_json::json!([0]));
    assert!(!child_log.exists());
    drop(slot);
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
    let first = first.join().unwrap();
    let second = second.join().unwrap();
    assert!(first.output.ok);
    assert!(second.output.ok);
    let first_data = first.output.data.unwrap();
    let second_data = second.output.data.unwrap();
    assert!(first_data["heldSeconds"].is_f64(), "{first_data}");
    assert!(second_data["waitedSeconds"].is_f64(), "{second_data}");
    assert!(second_data["heldSeconds"].is_f64(), "{second_data}");
    assert!(
        first_data["heldSeconds"]
            .as_f64()
            .is_some_and(|seconds| seconds > 0.0),
        "{first_data}"
    );
    assert!(
        second_data["waitedSeconds"]
            .as_f64()
            .is_some_and(|seconds| seconds > 0.0),
        "{second_data}"
    );
    assert!(
        second_data["heldSeconds"]
            .as_f64()
            .is_some_and(|seconds| seconds > 0.0),
        "{second_data}"
    );
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

/// Two independent slot roots must serialize their build children through one shared OS
/// admission authority. Cost: two local marker children, about two seconds.
#[test]
fn shared_admission_serializes_children_across_slot_roots() {
    let dir = tempfile::tempdir().unwrap();
    let root_a = dir.path().join("root-a");
    let root_b = dir.path().join("root-b");
    let log = dir.path().join("cross-root.log");
    let command_for = |tag: &str, release: &Path| {
        let log = log.to_string_lossy().replace('\'', "''");
        let release = release.to_string_lossy().replace('\'', "''");
        if cfg!(windows) {
            vec![
                "powershell".into(),
                "-NoProfile".into(),
                "-Command".into(),
                format!(
                    "$deadline=(Get-Date).AddSeconds(10); Add-Content -LiteralPath '{log}' '{tag} start'; while (!(Test-Path -LiteralPath '{release}') -and (Get-Date) -lt $deadline) {{ Start-Sleep -Milliseconds 10 }}; if (!(Test-Path -LiteralPath '{release}')) {{ exit 91 }}; Add-Content -LiteralPath '{log}' '{tag} end'"
                ),
            ]
        } else {
            vec![
                "sh".into(),
                "-c".into(),
                format!(
                    "echo '{tag} start' >> '{log}'; i=0; while [ ! -f '{release}' ] && [ $i -lt 1000 ]; do sleep 0.01; i=$((i+1)); done; if [ ! -f '{release}' ]; then exit 91; fi; echo '{tag} end' >> '{log}'"
                ),
            ]
        }
    };
    let release_a = dir.path().join("release-a");
    let release_b = dir.path().join("release-b");
    let command_a = command_for("a", &release_a);
    let command_b = command_for("b", &release_b);
    let cwd = dir.path().to_owned();
    let admission = dir.path().join("shared-test-admission");
    let expected_target_a = super::shared_target(&root_a).to_string_lossy().into_owned();
    let expected_target_b = super::shared_target(&root_b).to_string_lossy().into_owned();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let first_barrier = Arc::clone(&barrier);
    let first_admission = admission.clone();
    let first = std::thread::spawn({
        let cwd = cwd.clone();
        move || {
            first_barrier.wait();
            let request = request(&root_a, "lane-a", "cross-root-a", &command_a);
            super::run_admitted_slot_core(&request, &cwd, None, &first_admission)
        }
    });
    let second_barrier = Arc::clone(&barrier);
    let second = std::thread::spawn({
        let cwd = cwd.clone();
        let second_admission = admission;
        move || {
            second_barrier.wait();
            let request = request(&root_b, "lane-b", "cross-root-b", &command_b);
            super::run_admitted_slot_core(&request, &cwd, None, &second_admission)
        }
    });
    let started = std::time::Instant::now();
    while started.elapsed() < Duration::from_secs(3)
        && !std::fs::read_to_string(&log)
            .unwrap_or_default()
            .contains(" start")
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    let first_log = std::fs::read_to_string(&log).unwrap_or_default();
    let first_tag = first_log
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().next())
        .map(str::to_owned);
    match first_tag.as_deref() {
        Some("a") => std::fs::write(&release_a, b"release").unwrap(),
        Some("b") => std::fs::write(&release_b, b"release").unwrap(),
        _ => {
            std::fs::write(&release_a, b"release").unwrap();
            std::fs::write(&release_b, b"release").unwrap();
        }
    }
    let second_tag = match first_tag.as_deref() {
        Some("a") => "b",
        Some("b") => "a",
        _ => "",
    };
    let first_end = first_tag.as_deref().map(|tag| format!("{tag} end"));
    let ended = std::time::Instant::now();
    while ended.elapsed() < Duration::from_secs(3)
        && first_end.as_deref().is_some_and(|line| {
            !std::fs::read_to_string(&log)
                .unwrap_or_default()
                .lines()
                .any(|entry| entry == line)
        })
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    if second_tag == "a" {
        std::fs::write(&release_a, b"release").unwrap();
    } else if second_tag == "b" {
        std::fs::write(&release_b, b"release").unwrap();
    }
    let first = first.join().unwrap();
    let second = second.join().unwrap();
    let lines = std::fs::read_to_string(log).unwrap_or_default();
    let lines = lines
        .lines()
        .map(|line| {
            line.split_whitespace()
                .take(2)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>();
    let first_target = first
        .output
        .data
        .as_ref()
        .and_then(|data| data["targetDir"].as_str())
        .unwrap_or_default()
        .to_owned();
    let second_target = second
        .output
        .data
        .as_ref()
        .and_then(|data| data["targetDir"].as_str())
        .unwrap_or_default()
        .to_owned();
    assert!(first.output.ok, "first child failed: {:?}", first.output);
    assert!(second.output.ok, "second child failed: {:?}", second.output);
    assert_eq!(first_target, expected_target_a);
    assert_eq!(second_target, expected_target_b);
    assert_ne!(first_target, second_target);
    assert!(
        lines == vec!["a start", "a end", "b start", "b end"]
            || lines == vec!["b start", "b end", "a start", "a end"],
        "cross-root children entered the build section together: {lines:?}"
    );
}

#[test]
fn update_installer_waits_for_slot_admission_before_starting_child() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let log = dir.path().join("update-v-slot.log");
    let release = dir.path().join("release");
    let admission = dir.path().join("shared-test-admission");
    let log_text = log.to_string_lossy().replace('\'', "''");
    let release_text = release.to_string_lossy().replace('\'', "''");
    let slot_command = if cfg!(windows) {
        vec![
            "powershell".into(),
            "-NoProfile".into(),
            "-Command".into(),
            format!(
                "$deadline=(Get-Date).AddSeconds(10); Add-Content -LiteralPath '{log_text}' 'slot start'; while (!(Test-Path -LiteralPath '{release_text}') -and (Get-Date) -lt $deadline) {{ Start-Sleep -Milliseconds 10 }}; if (!(Test-Path -LiteralPath '{release_text}')) {{ exit 91 }}; Add-Content -LiteralPath '{log_text}' 'slot end'"
            ),
        ]
    } else {
        vec![
            "sh".into(),
            "-c".into(),
            format!(
                "echo 'slot start' >> '{log_text}'; i=0; while [ ! -f '{release_text}' ] && [ $i -lt 1000 ]; do sleep 0.01; i=$((i+1)); done; if [ ! -f '{release_text}' ]; then exit 91; fi; echo 'slot end' >> '{log_text}'"
            ),
        ]
    };
    let slot_root = root.clone();
    let slot_cwd = dir.path().to_owned();
    let slot_admission = admission.clone();
    let slot = std::thread::spawn(move || {
        let request = request(&slot_root, "lane", "slot", &slot_command);
        super::run_admitted_slot_core(&request, &slot_cwd, None, &slot_admission)
    });

    let started = std::time::Instant::now();
    while started.elapsed() < Duration::from_secs(3) {
        if std::fs::read_to_string(&log)
            .unwrap_or_default()
            .contains("slot start")
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let slot_started = std::fs::read_to_string(&log)
        .unwrap_or_default()
        .contains("slot start");

    let update_command = if cfg!(windows) {
        let script = dir.path().join("update-fixture.ps1");
        std::fs::write(
            &script,
            format!(
                "Add-Content -LiteralPath '{log_text}' 'update start'\nAdd-Content -LiteralPath '{log_text}' 'update end'\n"
            ),
        )
        .unwrap();
        vec![
            "powershell".into(),
            "-NoProfile".into(),
            "-File".into(),
            script.to_string_lossy().into_owned(),
        ]
    } else {
        vec![
            "sh".into(),
            "-c".into(),
            format!(
                "echo 'update start' >> '{}'; echo 'update end' >> '{}'",
                log.display(),
                log.display()
            ),
        ]
    };
    let source = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fake_exe = dir.path().join(if cfg!(windows) {
        "graphhelm.exe"
    } else {
        "graphhelm"
    });
    std::fs::write(&fake_exe, b"fixture executable").unwrap();
    let update_exe = fake_exe.clone();
    let update_admission = admission.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let update = std::thread::spawn(move || {
        ready_tx.send(()).unwrap();
        let mut command = graphhelm_process::command(&update_command[0]);
        command.args(&update_command[1..]);
        super::super::studio::run_install(
            &mut command,
            source,
            crate::progress::Progress::decide("update-test"),
            &update_admission,
            Some(&update_exe),
        )
    });
    let update_started = ready_rx.recv_timeout(Duration::from_secs(3)).is_ok();
    // Let the installer reach its real lock call while the slot child still holds admission.
    std::thread::sleep(Duration::from_millis(100));
    let original_present_while_held = fake_exe.exists();
    let old_absent_while_held = !fake_exe
        .with_file_name(if cfg!(windows) {
            "graphhelm.old.exe"
        } else {
            "graphhelm.old"
        })
        .exists();
    let update_not_started_while_held = !std::fs::read_to_string(&log)
        .unwrap_or_default()
        .contains("update start");
    std::fs::write(&release, b"release").unwrap();
    let slot = slot.join().unwrap();
    let update_ok = update.join().unwrap();
    let lines = std::fs::read_to_string(&log).unwrap();
    let tags = lines
        .lines()
        .map(|line| {
            line.split_whitespace()
                .take(2)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>();
    assert!(slot_started, "slot child did not enter the build section");
    assert!(update_started, "update fixture thread did not start");
    assert!(slot.output.ok, "slot child failed: {:?}", slot.output);
    assert!(update_ok);
    assert!(
        original_present_while_held,
        "update renamed the running executable before admission"
    );
    assert!(
        old_absent_while_held,
        "update moved the running executable before admission"
    );
    assert!(
        update_not_started_while_held,
        "update child entered while slot held admission"
    );
    assert_eq!(
        tags,
        vec!["slot start", "slot end", "update start", "update end"]
    );
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
    assert_eq!(refused.exit_code, 3);
    assert!(!refused.output.ok);
    assert_eq!(
        refused.output.diagnostics[0].code,
        crate::error_codes::GHCLI037_WORKSPACE_REFUSED
    );
    assert_eq!(refused.output.diagnostics[0].path, "/slot");
    let data = refused.output.data.as_ref().unwrap();
    assert!(data["waitedSeconds"].as_f64().unwrap() >= 0.06);
    assert_eq!(data["heldSeconds"], 0);
    assert!(
        refused.output.diagnostics[0]
            .message
            .contains("keep their place")
    );
    assert!(
        !std::fs::read_to_string(&log)
            .unwrap_or_default()
            .contains("b start")
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
    assert_eq!(refused.output.diagnostics[0].path, "/target");
    assert!(
        !std::fs::read_to_string(&log)
            .unwrap()
            .contains("second start"),
        "a command refused by the lane target cap must not run"
    );
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
