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
        ws.join("target")
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
