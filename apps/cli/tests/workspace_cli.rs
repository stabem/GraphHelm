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
        root.join("target-shared"),
        "claimed workspaces build in the root's one shared target (#360 phase 2)"
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

fn slot(root: &str, lane: &str, command: &[String]) -> std::process::Child {
    Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args([
            "--json",
            "workspace",
            "slot",
            "--root",
            root,
            "--lane",
            lane,
            "--jobs",
            "3",
            "--",
        ])
        .args(command)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap()
}

/// #360 phase 2: the shared build slot serves one command at a time, in arrival order, with the
/// root's shared target and the job count, and a waiter that died does not hold the queue.
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
    std::thread::sleep(std::time::Duration::from_millis(400));
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
    let shared = root.join("target-shared");
    assert!(
        lines[0].contains(shared.to_str().unwrap()) && lines[0].ends_with(" 3"),
        "the command gets the shared target and the job count: {text}"
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
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
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
