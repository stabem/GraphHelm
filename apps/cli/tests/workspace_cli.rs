//! #360: agent workspaces are claimed, released, and removed only by the sweep, and only when the
//! claimant released them and the tree is still clean at the released commit. Credible
//! regressions: a sweep that deletes unreleased or dirty work, follows a junction or symlink out of
//! the workspace, or touches a directory the ledger never created. No existing test covers any
//! workspace command. Cost: a temp git repository and a handful of CLI subprocesses, a few seconds
//! after the build; no network.
use std::path::Path;
use std::process::Command;

use serde_json::Value;

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

fn run(args: &[&str]) -> (i32, Value) {
    let out = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["--json", "workspace"])
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        serde_json::from_slice(&out.stdout).unwrap_or(Value::Null),
    )
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

/// A directory link the sweep must not follow: a junction on Windows (what `mklink /J` makes and
/// `is_symlink` misses), a symlink elsewhere.
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

#[test]
fn claim_lays_out_the_workspace_and_refuses_a_second_claim() {
    let dir = tempfile::tempdir().unwrap();
    let repo = repo(dir.path());
    let root = dir.path().join("root");
    let root_s = root.to_str().unwrap();
    let reply = claim(root_s, &repo, "lane-a", "360");
    let ws = root.join("lane-a").join("360");
    for part in ["wt", "target", "tmp", "logs"] {
        assert!(ws.join(part).is_dir(), "{part} missing: {reply}");
    }
    assert!(ws.join("wt").join("a.txt").is_file());
    assert_eq!(reply["data"]["branch"], "issue-360-lane-a");
    assert_eq!(
        Path::new(reply["data"]["env"]["CARGO_TARGET_DIR"].as_str().unwrap()),
        ws.join("target"),
        "a claimed workspace builds in its own target (#361: never another worktree's bytes)"
    );
    let (code, again) = run(&[
        "claim",
        "--root",
        root_s,
        "--lane",
        "lane-a",
        "--task",
        "360",
        "--repo",
        repo.to_str().unwrap(),
        "--base",
        "HEAD",
    ]);
    assert_eq!(code, 2, "{again}");
    assert_eq!(
        again["diagnostics"][0]["code"],
        "GHCLI037_WORKSPACE_REFUSED"
    );
    let (code, bad) = run(&[
        "claim",
        "--root",
        root_s,
        "--lane",
        "../x",
        "--task",
        "1",
        "--repo",
        repo.to_str().unwrap(),
    ]);
    assert_eq!(code, 3, "{bad}");
}

#[test]
fn sweep_removes_only_released_clean_unmoved_workspaces_and_never_follows_links() {
    let dir = tempfile::tempdir().unwrap();
    let repo = repo(dir.path());
    let root = dir.path().join("root");
    let root_s = root.to_str().unwrap();
    for task in ["done", "open", "dirty", "moved"] {
        claim(root_s, &repo, "lane", task);
    }
    let ws = |task: &str| root.join("lane").join(task);
    // Sabotage: a link inside the released workspace's target pointing at work outside it.
    let victim = dir.path().join("victim");
    std::fs::create_dir_all(&victim).unwrap();
    std::fs::write(victim.join("keep.txt"), "precious").unwrap();
    link_dir(&ws("done").join("target").join("escape"), &victim);
    std::fs::write(ws("done").join("target").join("big.bin"), vec![0u8; 4096]).unwrap();
    // A directory under the root that no claim created.
    std::fs::create_dir_all(root.join("lane").join("stranger")).unwrap();
    release(root_s, "lane", "done");
    release(root_s, "lane", "dirty");
    std::fs::write(ws("dirty").join("wt").join("a.txt"), "edited\n").unwrap();
    release(root_s, "lane", "moved");
    std::fs::write(ws("moved").join("wt").join("b.txt"), "b\n").unwrap();
    git(&ws("moved").join("wt"), &["add", "-A"]);
    git(
        &ws("moved").join("wt"),
        &["commit", "-q", "-m", "after release"],
    );

    let (code, dry) = run(&["sweep", "--root", root_s]);
    assert_eq!(code, 0, "{dry}");
    assert_eq!(dry["data"]["applied"], false);
    assert!(
        ws("done").join("wt").is_dir(),
        "a dry run removed something"
    );

    let (code, swept) = run(&["sweep", "--root", root_s, "--apply"]);
    assert_eq!(code, 0, "{swept}");
    let removed: Vec<&str> = swept["data"]["removed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["task"].as_str().unwrap())
        .collect();
    assert_eq!(removed, ["done"], "{swept}");
    let kept = |task: &str| {
        swept["data"]["kept"]
            .as_array()
            .unwrap()
            .iter()
            .find(|k| k["task"] == task)
            .map(|k| k["reason"].as_str().unwrap().to_owned())
    };
    assert_eq!(kept("open").as_deref(), Some("not_released"));
    assert_eq!(kept("dirty").as_deref(), Some("dirty"));
    assert_eq!(kept("moved").as_deref(), Some("moved_after_release"));
    assert!(
        !ws("done").exists(),
        "the released workspace survived: {swept}"
    );
    assert_eq!(
        std::fs::read_to_string(victim.join("keep.txt")).unwrap(),
        "precious",
        "the sweep followed a link out of the workspace"
    );
    for task in ["open", "dirty", "moved"] {
        assert!(ws(task).join("wt").is_dir(), "{task} was removed");
    }
    assert!(root.join("lane").join("stranger").is_dir());
    let (_, listed) = run(&["list", "--root", root_s]);
    let state = listed["data"]["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["task"] == "done")
        .map(|w| w["state"].clone());
    assert_eq!(state, Some(Value::from("swept")), "{listed}");
}

/// #374 BLOCK: `git worktree remove` recursed through a junction at an IGNORED path inside a clean,
/// released worktree and deleted its target outside the root. The worktree stays clean (git does
/// not report ignored paths), so only a link scan of `wt/` before git runs can stop it.
#[test]
fn a_link_anywhere_in_the_worktree_keeps_the_workspace_and_its_target_survives() {
    let dir = tempfile::tempdir().unwrap();
    let repo = repo(dir.path());
    let root = dir.path().join("root");
    let root_s = root.to_str().unwrap();
    claim(root_s, &repo, "lane", "trap");
    let wt = root.join("lane").join("trap").join("wt");
    let victim = dir.path().join("victim");
    std::fs::create_dir_all(&victim).unwrap();
    std::fs::write(victim.join("keep.txt"), "precious").unwrap();
    std::fs::create_dir_all(wt.join("ignored")).unwrap();
    link_dir(&wt.join("ignored").join("link"), &victim);
    release(root_s, "lane", "trap");

    let (code, swept) = run(&["sweep", "--root", root_s, "--apply"]);
    assert_eq!(code, 0, "{swept}");
    assert_eq!(swept["data"]["removed"], serde_json::json!([]), "{swept}");
    assert_eq!(
        swept["data"]["kept"][0]["reason"], "contains_link",
        "{swept}"
    );
    assert_eq!(swept["data"]["kept"][0]["link"], "ignored/link", "{swept}");
    assert_eq!(
        std::fs::read_to_string(victim.join("keep.txt")).unwrap(),
        "precious",
        "the sweep deleted through a link inside the worktree"
    );
    assert!(wt.is_dir());
    let (code, refused) = run(&[
        "claim",
        "--root",
        root_s,
        "--lane",
        "lane",
        "--task",
        "opt",
        "--repo",
        repo.to_str().unwrap(),
        "--base=--orphan",
    ]);
    assert_eq!(code, 3, "{refused}");
}

/// Makes `dir` unlistable for the current user until the guard drops (Windows: a deny ACE for
/// list-directory; elsewhere mode 000).
struct Unreadable(std::path::PathBuf);

impl Unreadable {
    fn new(dir: &Path) -> Self {
        #[cfg(windows)]
        {
            let user = std::env::var("USERNAME").unwrap();
            let out = Command::new("icacls")
                .arg(dir)
                .args(["/deny", &format!("{user}:(RD)")])
                .output()
                .unwrap();
            assert!(out.status.success(), "{out:?}");
        }
        #[cfg(not(windows))]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o000)).unwrap();
        }
        Self(dir.to_path_buf())
    }
}

impl Drop for Unreadable {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            let user = std::env::var("USERNAME").unwrap_or_default();
            let _ = Command::new("icacls")
                .arg(&self.0)
                .args(["/remove:d", &user])
                .output();
        }
        #[cfg(not(windows))]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
        }
    }
}

/// #374 review: the link scan must fail closed. A folder it cannot read may hold a junction, so a
/// read error keeps the workspace (`scan_failed`, naming the folder) instead of sweeping it.
#[test]
fn an_unreadable_folder_in_the_worktree_keeps_the_workspace() {
    let dir = tempfile::tempdir().unwrap();
    let repo = repo(dir.path());
    let root = dir.path().join("root");
    let root_s = root.to_str().unwrap();
    claim(root_s, &repo, "lane", "locked");
    let wt = root.join("lane").join("locked").join("wt");
    let hidden = wt.join("ignored").join("sealed");
    std::fs::create_dir_all(&hidden).unwrap();
    release(root_s, "lane", "locked");
    let guard = Unreadable::new(&hidden);
    if std::fs::read_dir(&hidden).is_ok() {
        drop(guard);
        panic!("precondition: the folder must be unreadable to this user (running as root?)");
    }
    let (code, swept) = run(&["sweep", "--root", root_s, "--apply"]);
    drop(guard);
    assert_eq!(code, 0, "{swept}");
    assert_eq!(swept["data"]["removed"], serde_json::json!([]), "{swept}");
    assert_eq!(swept["data"]["kept"][0]["reason"], "scan_failed", "{swept}");
    assert_eq!(
        swept["data"]["kept"][0]["scanError"]["path"], "ignored/sealed",
        "{swept}"
    );
    assert!(hidden.is_dir(), "the unreadable folder was removed");
}

/// A command that appends `<tag> start <CARGO_TARGET_DIR> <CARGO_BUILD_JOBS>` to `log`, waits
/// `millis`, then appends `<tag> end`.
fn marker_command(log: &Path, tag: &str, millis: u64) -> Vec<String> {
    let log = log.to_str().unwrap().to_owned();
    if cfg!(windows) {
        vec![
            "powershell".into(),
            "-NoProfile".into(),
            "-Command".into(),
            format!(
                "Add-Content -LiteralPath '{log}' \"{tag} start $env:CARGO_TARGET_DIR $env:CARGO_BUILD_JOBS\"; \
                 Start-Sleep -Milliseconds {millis}; Add-Content -LiteralPath '{log}' '{tag} end'"
            ),
        ]
    } else {
        vec![
            "sh".into(),
            "-c".into(),
            format!(
                "echo \"{tag} start $CARGO_TARGET_DIR $CARGO_BUILD_JOBS\" >> '{log}'; \
                 sleep {}; echo '{tag} end' >> '{log}'",
                millis as f64 / 1000.0
            ),
        ]
    }
}
/// Public admission must reject script wrappers before creating a ticket, target, or child.
/// Observable: a real marker child is not started and the slot ledger stays untouched. The
/// private queue tests cover the queue behavior because this command is intentionally refused
/// before admission; existing integration coverage cannot prove queue state after refusal.
#[test]
fn the_public_slot_refuses_script_commands_before_any_effect() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let marker = dir.path().join("marker.txt");
    let output = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .current_dir(dir.path())
        .args(["--json", "workspace", "slot", "--root"])
        .arg(&root)
        .args(["--lane", "lane-a", "--"])
        .args(marker_command(&marker, "marker", 10))
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "script command was admitted: {output:?}"
    );
    let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(reply["diagnostics"][0]["path"], "/command", "{reply}");
    assert!(
        reply["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("direct cargo"),
        "{reply}"
    );
    assert!(!marker.exists(), "the refused marker child ran");
    assert!(
        !root.join(".graphhelm-workspaces").exists(),
        "refusal created ledger state"
    );
}

/// #360 phase 2: the build slot serves one command at a time, in arrival order, with the
/// worktree's own target (#361) and the job count, and a waiter that died does not hold the queue.
/// Credible regressions: two builds overlap in the shared target (the stale-artifact and
/// lock-contention failure), a later waiter overtakes, or a dead waiter's ticket blocks everyone
/// (the lost-ticket failure of the script this replaces). Cost: three short child commands.
#[test]
fn the_slot_serves_one_command_at_a_time_in_order_and_skips_dead_waiters() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let root_s = root.to_str().unwrap();
    // A dead waiter: a ticket older than everyone, whose lock nobody holds.
    let tickets = root.join(".graphhelm-workspaces").join("slot");
    std::fs::create_dir_all(&tickets).unwrap();
    std::fs::write(tickets.join(format!("{:024}-ghost-1.ticket", 1)), "").unwrap();
    let log = dir.path().join("log.txt");
    let first = slot(root_s, "lane-a", &marker_command(&log, "a", 1500));
    // #549: wait for lane-a's ticket itself, not 400 ms and a hope. Under load lane-a could
    // still be starting when lane-b took the older ticket, and the order asserted below inverted.
    let waited = std::time::Instant::now();
    let ceiling = support::time_scale::scaled(std::time::Duration::from_secs(30));
    while !std::fs::read_dir(&tickets)
        .unwrap()
        .flatten()
        .any(|entry| entry.file_name().to_string_lossy().contains("-lane-a-"))
    {
        assert!(
            waited.elapsed() < ceiling,
            "lane-a never took a slot ticket within {ceiling:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let second = slot(root_s, "lane-b", &marker_command(&log, "b", 100));
    let first = first.wait_with_output().unwrap();
    let second = second.wait_with_output().unwrap();
    assert!(
        first.status.success() && second.status.success(),
        "{first:?} {second:?}"
    );
    let reply: Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(reply["data"]["exitCode"], 0, "{reply}");
    let text = std::fs::read_to_string(&log).unwrap();
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    assert_eq!(lines.len(), 4, "{text}");
    assert!(lines[0].starts_with("a start"), "{text}");
    assert_eq!(
        lines[1], "a end",
        "the second command ran inside the first: {text}"
    );
    assert!(lines[2].starts_with("b start"), "{text}");
    assert_eq!(lines[3], "b end");
    let own = dir.path().join("target");
    assert!(
        lines[0].contains(own.to_str().unwrap()) && lines[0].ends_with(" 3"),
        "the command gets its own worktree's target and the job count: {text}"
    );
    let left: Vec<_> = std::fs::read_dir(&tickets)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "ticket"))
        .collect();
    assert!(
        left.is_empty(),
        "every ticket, the dead one included, is gone: {left:?}"
    );
}

#[path = "support/mod.rs"]
mod support;

struct Server(std::process::Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// `graphhelm serve` on an ephemeral port; returns the guard, base URL and token file.
fn serve(dir: &Path, extra: &[&str]) -> (Server, String, std::path::PathBuf) {
    use std::io::BufRead;
    let events = dir.join("events");
    let mut child = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args([
            "serve",
            "--events",
            events.to_str().unwrap(),
            "--bind",
            "127.0.0.1:0",
        ])
        .args(extra)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut line = String::new();
    std::io::BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let started: Value = serde_json::from_str(line.trim()).unwrap();
    let base = format!("http://{}", started["data"]["address"].as_str().unwrap());
    let token = dir.join("events.token");
    let deadline =
        std::time::Instant::now() + support::time_scale::scaled(std::time::Duration::from_secs(10));
    while support::raw_request(&format!("{base}/health"), None).map_or(true, |r| r.status != 200) {
        assert!(
            std::time::Instant::now() < deadline,
            "serve never answered /health"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    (Server(child), base, token)
}

/// One MCP stdio session against `base`: initialize, then one `tools/call`; returns its reply.
fn mcp_call(base: &str, token: &Path, actor_type: &str, tool: &str) -> Value {
    use std::io::Write;
    let mut child = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args([
            "mcp",
            "--url",
            base,
            "--token-file",
            token.to_str().unwrap(),
        ])
        .args(["--actor", "lane-test", "--actor-type", actor_type])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let lines = [
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "t", "version": "0"}}}),
        serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {"name": tool, "arguments": {}}}),
    ];
    let mut stdin = child.stdin.take().unwrap();
    for line in lines {
        writeln!(stdin, "{line}").unwrap();
    }
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find(|v| v["id"] == 2)
        .unwrap_or(Value::Null)
}

#[test]
fn http_and_mcp_list_like_the_cli_and_only_an_owner_session_sweeps() {
    let dir = tempfile::tempdir().unwrap();
    let repo = repo(dir.path());
    let root = dir.path().join("root");
    let root_s = root.to_str().unwrap();
    claim(root_s, &repo, "lane", "done");
    release(root_s, "lane", "done");
    let (_cli_code, cli) = run(&["list", "--root", root_s]);

    let (_server, base, token_file) = serve(dir.path(), &["--workspace-root", root_s]);
    let token = std::fs::read_to_string(&token_file).unwrap();
    let http = support::raw_request(&format!("{base}/v1/workspaces"), Some(token.trim())).unwrap();
    assert_eq!(http.status, 200, "{}", http.body);
    let http: Value = serde_json::from_str(&http.body).unwrap();
    assert_eq!(
        http["data"]["workspaces"], cli["data"]["workspaces"],
        "{http}"
    );

    let agent = mcp_call(&base, &token_file, "agent", "workspace_sweep");
    assert_eq!(
        agent["error"]["data"]["code"], "GHCLI037_WORKSPACE_REFUSED",
        "an agent session must not sweep: {agent}"
    );
    assert!(root.join("lane").join("done").join("wt").is_dir());

    let listed = mcp_call(&base, &token_file, "agent", "workspace_list");
    let text: Value =
        serde_json::from_str(listed["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        text["data"]["workspaces"], cli["data"]["workspaces"],
        "{listed}"
    );

    let owner = mcp_call(&base, &token_file, "owner", "workspace_sweep");
    assert!(owner["error"].is_null(), "{owner}");
    assert!(!root.join("lane").join("done").exists(), "{owner}");

    let bare = tempfile::tempdir().unwrap();
    let (_plain, plain_base, plain_token) = serve(bare.path(), &[]);
    let plain_token = std::fs::read_to_string(plain_token).unwrap();
    let refused = support::raw_request(
        &format!("{plain_base}/v1/workspaces"),
        Some(plain_token.trim()),
    )
    .unwrap();
    assert_ne!(refused.status, 200);
    assert!(refused.body.contains("/workspaceRoot"), "{}", refused.body);
}

/// Contract: the Runtime reclaims released work without a request, reports the outcome,
/// and leaves live work intact; zero disables it. Regression: no timer, ignored zero,
/// or a tick bypassing the manual rules. Existing HTTP tests only request manual sweeps.
/// Cost: three temp Git repos and local servers, about ten seconds. The hidden debug-only
/// period override accelerates the real timer; no deletion or filesystem I/O is mocked.
#[cfg(debug_assertions)]
#[test]
fn runtime_workspace_sweep_runs_without_a_request_and_zero_disables_it() {
    for seconds in [Some("60"), None, Some("0")] {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo(dir.path());
        let root = dir.path().join("root");
        let root_s = root.to_str().unwrap();
        claim(root_s, &repo, "lane", "done");
        release(root_s, "lane", "done");
        claim(root_s, &repo, "lane", "live");
        // A failed first tick must leave the Runtime alive and retry next period.
        let mut held = (seconds == Some("60")).then(|| {
            let file =
                std::fs::File::create(root.join(".graphhelm-workspaces/sweep.lock")).unwrap();
            file.lock().unwrap();
            file
        });
        let mut saw_failure = false;
        let mut args = vec![
            "--workspace-root",
            root_s,
            "--workspace-sweep-test-seconds",
            "1",
        ];
        if let Some(seconds) = seconds {
            args.extend(["--workspace-sweep-seconds", seconds]);
        }
        let (_server, base, token_file) = serve(dir.path(), &args);
        let token = std::fs::read_to_string(token_file).unwrap();
        let started = std::time::Instant::now();
        loop {
            let reply =
                support::raw_request(&format!("{base}/v1/workspaces"), Some(token.trim())).unwrap();
            assert_eq!(reply.status, 200, "{}", reply.body);
            let reply: Value = serde_json::from_str(&reply.body).unwrap();
            assert!(reply["data"].get("lastSweep").is_some(), "{reply}");
            assert!(root.join("lane/live/wt/a.txt").is_file());
            if seconds == Some("0") {
                assert!(root.join("lane/done/wt/a.txt").is_file());
                assert!(reply["data"]["lastSweep"].is_null(), "{reply}");
                if started.elapsed() >= std::time::Duration::from_secs(3) {
                    break;
                }
            } else if !reply["data"]["lastSweep"].is_null() {
                let last = &reply["data"]["lastSweep"];
                assert!(last["at"].as_u64().is_some_and(|at| at > 0), "{last}");
                if last["ok"] == false {
                    assert_eq!(
                        last["diagnostics"][0]["code"], "GHCLI037_WORKSPACE_REFUSED",
                        "{last}"
                    );
                    assert!(last["removed"].as_array().unwrap().is_empty(), "{last}");
                    if held.is_some() {
                        assert!(root.join("lane/done/wt/a.txt").is_file());
                    }
                    saw_failure = true;
                    drop(held.take());
                } else {
                    assert_eq!(last["ok"], true, "{last}");
                    assert_eq!(saw_failure, seconds == Some("60"));
                    assert_eq!(last["removed"][0]["task"], "done", "{last}");
                    assert_eq!(last["kept"][0]["task"], "live", "{last}");
                    assert_eq!(last["kept"][0]["reason"], "not_released", "{last}");
                    assert!(!root.join("lane/done").exists());
                    break;
                }
            }
            // Advisory poll ceiling: a request can additionally take REQUEST_TIMEOUT.
            assert!(
                started.elapsed() < std::time::Duration::from_secs(15),
                "no sweep: {reply}"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}

/// Contract: bad periods refuse before startup, not a tight background loop. Existing
/// serve parsing has no workspace period. No seam; four short CLI calls in a temp dir.
#[test]
fn runtime_workspace_sweep_refuses_periods_outside_the_safe_range() {
    let dir = tempfile::tempdir().unwrap();
    for seconds in ["1", "59", "86401", "18446744073709551615"] {
        let output = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
            .args(["serve", "--events"])
            .arg(dir.path().join("events"))
            .args([
                "--bind",
                "127.0.0.1:0",
                "--workspace-sweep-seconds",
                seconds,
            ])
            .output()
            .unwrap();
        assert!(!output.status.success());
        let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            reply["diagnostics"][0]["code"], "GHCLI001_ARGUMENT_INVALID",
            "{reply}"
        );
        assert!(!dir.path().join("events").exists());
    }
}

/// Contract: CLI, HTTP and held-slot reclaim share the same OS lock. Regression: one
/// door deletes or refuses a build while a sweep owns the lock. Existing slot tests
/// only hold slot.lock; this cell must observe the child still running under sweep.lock.
/// No seam: hold the actual file lock; temp files, Git and short children only.
#[test]
fn workspace_sweep_lock_serializes_manual_and_http_sweep() {
    let dir = tempfile::tempdir().unwrap();
    let repo = repo(dir.path());
    let root = dir.path().join("root");
    let root_s = root.to_str().unwrap();
    claim(root_s, &repo, "lane", "done");
    release(root_s, "lane", "done");
    let lock = std::fs::File::create(root.join(".graphhelm-workspaces/sweep.lock")).unwrap();
    lock.lock().unwrap();
    assert_ne!(run(&["sweep", "--root", root_s, "--apply"]).0, 0);
    let (_server, base, token) = serve(dir.path(), &["--workspace-root", root_s]);
    let token = std::fs::read_to_string(token).unwrap();
    let reply = post(&format!("{base}/v1/workspaces/sweep"), token.trim());
    assert_ne!(reply.status, 200, "{}", reply.body);
    assert!(root.join("lane/done/wt/a.txt").is_file());
    drop(lock);
    assert_eq!(run(&["sweep", "--root", root_s, "--apply"]).0, 0);
    assert!(!root.join("lane/done").exists());
}

/// #380: the declared actor type is not a credential. The Runtime's agent session token
/// (`events.agent.token`) cannot sweep over HTTP or through an MCP session that declares itself
/// `owner`; the workspace stays. The same token still lists, as agents do.
#[test]
fn an_agent_session_token_cannot_sweep_whatever_type_it_declares() {
    let dir = tempfile::tempdir().unwrap();
    let repo = repo(dir.path());
    let root = dir.path().join("root");
    let root_s = root.to_str().unwrap();
    claim(root_s, &repo, "lane", "done");
    release(root_s, "lane", "done");
    let (_server, base, _owner) = serve(dir.path(), &["--workspace-root", root_s]);
    let agent_file = dir.path().join("events.agent.token");
    let agent = std::fs::read_to_string(&agent_file)
        .expect("serve mints the agent session token beside the owner token");

    let swept = post(&format!("{base}/v1/workspaces/sweep"), agent.trim());
    assert_eq!(swept.status, 403, "{}", swept.body);
    let spoofed = mcp_call(&base, &agent_file, "owner", "workspace_sweep");
    assert!(
        !spoofed["error"].is_null() || spoofed["result"]["isError"] == true,
        "a self-declared owner holding the agent token swept: {spoofed}"
    );
    assert!(root.join("lane").join("done").join("wt").is_dir());

    let listed =
        support::raw_request(&format!("{base}/v1/workspaces"), Some(agent.trim())).unwrap();
    assert_eq!(listed.status, 200, "{}", listed.body);
}

/// A bodyless `POST` with a bearer token, answered as `support::parse_response` reads it.
fn post(url: &str, token: &str) -> support::RawResponse {
    use std::io::{Read, Write};
    let (host, port, path) = support::split_url(url);
    let mut stream = std::net::TcpStream::connect((host.as_str(), port)).unwrap();
    stream
        .set_read_timeout(Some(support::REQUEST_TIMEOUT))
        .unwrap();
    write!(
        stream,
        "POST {path} HTTP/1.1
Host: {host}
Connection: close
Content-Length: 0
Authorization: Bearer {token}

"
    )
    .unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    support::parse_response(&String::from_utf8_lossy(&raw)).unwrap()
}

/// Review of #417 (coordinator): `--clean-workspace` from a directory that is no cargo workspace
/// used to report `cleaned: null` and run the command anyway, against whatever another lane left
/// in the shared target. It must fail closed: refuse, name why, never run the command, and free
/// the slot. Credible regression: the fail-open path coming back. Cost: one CLI run, no cargo build.
#[test]
fn clean_workspace_that_cannot_clean_refuses_and_never_runs_the_command() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let not_a_workspace = dir.path().join("plain");
    std::fs::create_dir_all(&not_a_workspace).unwrap();
    let output = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .current_dir(&not_a_workspace)
        .args([
            "--json",
            "workspace",
            "slot",
            "--root",
            root.to_str().unwrap(),
        ])
        .args(["--lane", "lane-a", "--clean-workspace", "--"])
        .args([
            "cargo",
            "+1.97.1",
            "test",
            "-p",
            "graphhelm-cli",
            "--test",
            "workspace_cli",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success(), "{output:?}");
    let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(reply["ok"], false, "{reply}");
    let diagnostic = &reply["diagnostics"][0];
    assert_eq!(diagnostic["path"], "/cleanWorkspace", "{reply}");
    assert!(
        diagnostic["message"]
            .as_str()
            .unwrap()
            .contains("no cargo workspace"),
        "the refusal says why: {reply}"
    );
    let tickets = root.join(".graphhelm-workspaces").join("slot");
    let left = std::fs::read_dir(&tickets)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "ticket"))
        .count();
    assert_eq!(left, 0, "the refusal frees the slot");
}

/// #557 review: a finite but huge `--max-wait` overflows a Duration; it is refused with the
/// argument's own words, never a panic. Cost: one CLI run, no command started.
#[test]
fn a_huge_max_wait_is_refused_not_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let log = dir.path().join("log.txt");
    let out = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["--json", "workspace", "slot", "--root"])
        .arg(&root)
        .args([
            "--lane",
            "lane-a",
            "--label",
            "huge",
            "--max-wait",
            "1e300",
            "--",
            "cargo",
            "+1.97.1",
            "test",
            "-p",
            "graphhelm-cli",
            "--test",
            "workspace_cli",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let reply: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    assert!(
        reply["diagnostics"][0]["message"]
            .as_str()
            .unwrap_or("")
            .contains("--max-wait must be"),
        "status {:?}, reply {reply}",
        out.status
    );
    assert!(!log.exists(), "the command never ran");
}

/// #557 review: a `holder.json` left by a holder killed hard names a ticket that is no longer
/// live; `status` must not name that dead lane as the holder. Cost: one CLI run.
#[test]
fn slot_status_does_not_trust_a_stale_holder_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let slot_dir = root.join(".graphhelm-workspaces").join("slot");
    std::fs::create_dir_all(&slot_dir).unwrap();
    std::fs::write(
        slot_dir.join("holder.json"),
        r#"{"lane":"ghost","label":"dead","pid":1,"sinceNanos":"1","ticket":"000000000000000000000001-ghost-1.ticket"}"#,
    )
    .unwrap();
    // A live holder that wrote no holder.json (an older binary): it holds slot.lock.
    let lock = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(slot_dir.join("slot.lock"))
        .unwrap();
    lock.lock().unwrap();
    let (code, status) = run(&["slot", "status", "--root", root.to_str().unwrap()]);
    drop(lock);
    assert_eq!(code, 0, "{status}");
    assert_ne!(
        status["data"]["holder"]["lane"], "ghost",
        "a stale holder.json was trusted: {status}"
    );
    assert!(
        status["data"]["holder"].is_object(),
        "the slot is held, by someone unnamed: {status}"
    );
}

/// #612: the Runtime reports the build-slot queues so the Studio can say "waiting for a build
/// (Nth)" instead of "Slow". `GET /v1/workspaces/slots` returns, per `--slot-root`, exactly what
/// `workspace slot status` sees: here one holder and one waiter on a temp root, plus an empty
/// second root and a missing root reported as an error. Read-only: the holder and waiter both still finish their own commands. Owner only:
/// the agent session token is refused. Credible regression: no route at all (the Studio guesses
/// from elapsed time), or one that agents can read. Cost: one Runtime, two short slot commands.
#[test]
fn the_runtime_reports_each_slot_roots_holder_and_waiters_to_the_owner_only() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("slot-a");
    let root_s = root.to_str().unwrap();
    let empty = dir.path().join("slot-b");
    std::fs::create_dir(&empty).unwrap();
    let missing = dir.path().join("slot-missing");
    let slot_dir = root.join(".graphhelm-workspaces").join("slot");
    std::fs::create_dir_all(&slot_dir).unwrap();
    let holder_ticket_path = slot_dir.join("000000000000000000000001-lane-a-1.ticket");
    let holder_ticket = std::fs::File::create(&holder_ticket_path).unwrap();
    holder_ticket.lock().unwrap();
    let waiter_ticket_path = slot_dir.join("000000000000000000000002-lane-b-2.ticket");
    let waiter_ticket = std::fs::File::create(&waiter_ticket_path).unwrap();
    waiter_ticket.lock().unwrap();
    std::fs::write(
        holder_ticket_path.with_extension("info"),
        serde_json::json!({"lane":"lane-a","label":"hold","pid":1,"arrivedNanos":"1","priority":false}).to_string(),
    ).unwrap();
    std::fs::write(
        waiter_ticket_path.with_extension("info"),
        serde_json::json!({"lane":"lane-b","label":"wait","pid":2,"arrivedNanos":"2","priority":false}).to_string(),
    ).unwrap();
    std::fs::write(
        slot_dir.join("holder.json"),
        serde_json::json!({"lane":"lane-a","label":"hold","pid":1,"sinceNanos":"1","ticket":holder_ticket_path.file_name().unwrap().to_string_lossy()}).to_string(),
    ).unwrap();
    let slot_lock = std::fs::File::create(slot_dir.join("slot.lock")).unwrap();
    slot_lock.lock().unwrap();
    let (_server, base, owner) = serve(
        dir.path(),
        &[
            "--slot-root",
            root_s,
            "--slot-root",
            empty.to_str().unwrap(),
            "--slot-root",
            missing.to_str().unwrap(),
        ],
    );
    let owner = std::fs::read_to_string(owner).unwrap();
    let reply =
        support::raw_request(&format!("{base}/v1/workspaces/slots"), Some(owner.trim())).unwrap();
    let agent = std::fs::read_to_string(dir.path().join("events.agent.token")).unwrap();
    let refused =
        support::raw_request(&format!("{base}/v1/workspaces/slots"), Some(agent.trim())).unwrap();
    assert_eq!(reply.status, 200, "{}", reply.body);
    let body: Value = serde_json::from_str(&reply.body).unwrap();
    let slots = body["data"]["slots"].as_array().expect("slots");
    assert_eq!(slots.len(), 3, "{body}");
    assert_eq!(slots[0]["root"], root_s, "{body}");
    assert_eq!(slots[0]["holder"]["lane"], "lane-a", "{body}");
    assert_eq!(slots[0]["holder"]["label"], "hold", "{body}");
    let waiting = slots[0]["waiting"].as_array().expect("waiting");
    assert_eq!(waiting.len(), 1, "{body}");
    assert_eq!(waiting[0]["lane"], "lane-b", "{body}");
    assert!(waiting[0]["waitedSeconds"].as_u64().is_some(), "{body}");
    assert_eq!(slots[1]["holder"], Value::Null, "{body}");
    assert_eq!(slots[1]["waiting"], serde_json::json!([]), "{body}");
    assert!(slots[1].get("error").is_none(), "{body}");
    assert_eq!(slots[2]["root"], missing.to_str().unwrap(), "{body}");
    assert_eq!(
        slots[2]["error"][0]["code"], "workspace.slot_root_missing",
        "{body}"
    );
    assert!(slots[2].get("holder").is_none(), "{body}");
    assert!(slots[2].get("waiting").is_none(), "{body}");
    assert_eq!(refused.status, 403, "{}", refused.body);
}

/// Contract: opt-in sizes count regular files only and agree over CLI and HTTP.
/// Regression: missing flag, following links, incomplete totals or a separate HTTP sizing path.
/// Gap: existing list tests only observe the legacy sizeBytes field. No production seams.
/// Cost: a temp repository, small files, CLI processes and one loopback server; a few seconds.
#[test]
fn sizes_count_files_and_recorded_targets_without_following_links_with_http_parity() {
    let dir = tempfile::tempdir().unwrap();
    let repo = repo(dir.path());
    let root = dir.path().join("root");
    let root_s = root.to_str().unwrap();
    claim(root_s, &repo, "lane", "sizes");
    let wt = root.join("lane").join("sizes").join("wt");
    let sizes = || {
        let (code, reply) = run(&["list", "--root", root_s, "--sizes"]);
        assert_eq!(code, 0, "--sizes must succeed: {reply}");
        reply["data"].clone()
    };
    let before = sizes()["workspaces"][0]["bytes"].as_u64().unwrap();
    std::fs::write(wt.join("one.bin"), vec![0; 1000]).unwrap();
    std::fs::write(wt.join("two.bin"), vec![0; 24]).unwrap();
    let after = sizes()["workspaces"][0]["bytes"].as_u64().unwrap();
    assert_eq!(after - before, 1024);
    let outside = dir.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("large.bin"), vec![0; 1024 * 1024]).unwrap();
    let link = wt.join("escape");
    link_dir(&link, &outside);
    let linked = sizes();
    #[cfg(windows)]
    std::fs::remove_dir(&link).unwrap();
    #[cfg(unix)]
    std::fs::remove_file(&link).unwrap();
    assert_eq!(linked["workspaces"][0]["bytes"], after);
    let hidden = wt.join("sealed");
    std::fs::create_dir_all(&hidden).unwrap();
    std::fs::write(hidden.join("unread.bin"), vec![0; 32]).unwrap();
    let guard = Unreadable::new(&hidden);
    assert!(
        std::fs::read_dir(&hidden).is_err(),
        "unreadable fixture must deny access"
    );
    let unreadable = sizes();
    drop(guard);
    assert!(unreadable["workspaces"][0]["bytes"].is_null());
    assert_eq!(unreadable["workspaces"][0]["sizeError"], "wt/sealed");
    assert!(unreadable["lanes"]["lane"].is_null());
    std::fs::remove_file(hidden.join("unread.bin")).unwrap();
    std::fs::remove_dir(&hidden).unwrap();
    // Unrecorded siblings must not enter totals.
    std::fs::create_dir_all(root.join("unrecorded")).unwrap();
    std::fs::write(root.join("unrecorded/large.bin"), vec![0; 2048]).unwrap();
    let fast = dir.path().join("fast");
    let rules = root.join(".graphhelm-workspaces");
    std::fs::create_dir_all(&fast).unwrap();
    std::fs::write(
        rules.join("slot-targets.json"),
        serde_json::json!({"targetRoot": fast, "minFreeGb": 0}).to_string(),
    )
    .unwrap();
    // Record creation is covered by the private slot-target tests; this public test observes the
    // persisted record consumed by the CLI and HTTP size readers.
    let target = fast.join("lane/wt/target");
    std::fs::create_dir_all(&target).unwrap();
    let records = rules.join("targets").join("lane");
    std::fs::create_dir_all(&records).unwrap();
    std::fs::write(
        records.join("wt.json"),
        serde_json::json!({
            "schema": "graphhelm.slot-target/1",
            "lane": "lane",
            "name": "wt",
            "worktree": wt,
            "target": target,
            "firstUsedAt": 1,
            "lastUsedAt": 1
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(fast.join("lane/wt/target/built.bin"), vec![0; 4096]).unwrap();
    let unrecorded = fast.join("unrecorded");
    std::fs::create_dir_all(&unrecorded).unwrap();
    std::fs::write(unrecorded.join("large.bin"), vec![0; 2048]).unwrap();
    let cli = sizes();
    assert_eq!(cli["targets"][0]["bytes"], 4096);
    assert_eq!(
        Path::new(cli["targets"][0]["worktree"].as_str().unwrap()),
        wt.as_path()
    );
    assert_eq!(cli["lanes"]["lane"], after + 4096);
    let plain = run(&["list", "--root", root_s]).1["data"].clone();
    let mut legacy = cli.clone();
    legacy.as_object_mut().unwrap().remove("targets");
    legacy.as_object_mut().unwrap().remove("lanes");
    legacy["workspaces"][0]
        .as_object_mut()
        .unwrap()
        .remove("bytes");
    assert_eq!(legacy, plain);
    let (_server, base, token_file) = serve(dir.path(), &["--workspace-root", root_s]);
    let token = std::fs::read_to_string(token_file).unwrap();
    let response =
        support::raw_request(&format!("{base}/v1/workspaces?sizes=1"), Some(token.trim())).unwrap();
    assert_eq!(response.status, 200);
    let mut http: Value = serde_json::from_str(&response.body).unwrap();
    http["data"].as_object_mut().unwrap().remove("lastSweep");
    assert_eq!(http["data"], cli);
    // A link in a target's parent must also be skipped before reaching its files.
    let holder = fast.join("lane").join("wt");
    let saved = fast.join("saved");
    std::fs::rename(&holder, &saved).unwrap();
    link_dir(&holder, &saved);
    let linked_target = sizes();
    #[cfg(windows)]
    std::fs::remove_dir(&holder).unwrap();
    #[cfg(unix)]
    std::fs::remove_file(&holder).unwrap();
    std::fs::rename(&saved, &holder).unwrap();
    assert_eq!(linked_target["targets"][0]["bytes"], 0);
    assert_eq!(linked_target["lanes"]["lane"], after);
    std::fs::remove_file(fast.join("lane/wt/target/built.bin")).unwrap();
    std::fs::remove_dir(fast.join("lane/wt/target")).unwrap();
    let missing = sizes();
    assert!(missing["targets"][0]["bytes"].is_null());
    assert_eq!(missing["targets"][0]["sizeError"], ".");
    assert!(missing["lanes"]["lane"].is_null());

    // A tampered record must not redirect the size walk outside the configured target root.
    let record = root.join(".graphhelm-workspaces/targets/lane/wt.json");
    let mut value: Value = serde_json::from_slice(&std::fs::read(&record).unwrap()).unwrap();
    value["target"] = serde_json::json!(outside);
    std::fs::write(&record, serde_json::to_vec(&value).unwrap()).unwrap();
    let redirected = sizes();
    assert!(redirected["targets"][0]["bytes"].is_null());
    assert_eq!(redirected["targets"][0]["sizeError"], "target_outside_root");
    assert!(redirected["lanes"]["lane"].is_null());
}
