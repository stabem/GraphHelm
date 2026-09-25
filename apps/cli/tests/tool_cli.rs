//! `graphhelm tool invoke` end to end through the compiled binary: the four-key envelope,
//! GHCLI012/013/014, mandatory `--capture-out`, and the sentinel discipline restated through
//! the real CLI (Task 8's assertion D, operator-facing form).

use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use assert_cmd::Command;
use serde_json::Value;

fn command() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
}

fn json(output: &[u8]) -> Value {
    serde_json::from_slice(output).unwrap()
}

// Duplicated scratch-repo helper (test binaries do not share helpers across crates or files).
fn scratch_repo() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join("src/lib.rs"), "// scratch\n").unwrap();
    let git = |args: &[&str]| {
        let status = StdCommand::new("git")
            .args(args)
            .current_dir(&project)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "scratch")
            .env("GIT_AUTHOR_EMAIL", "scratch@test.invalid")
            .env("GIT_COMMITTER_NAME", "scratch")
            .env("GIT_COMMITTER_EMAIL", "scratch@test.invalid")
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    };
    git(&["init", "--quiet"]);
    git(&["add", "-A"]);
    git(&["commit", "--quiet", "-m", "scratch"]);
    (dir, project)
}

fn sha256_hex(bytes: &[u8]) -> String {
    graphhelm_tool_broker::record::digest_hex(bytes)
}

struct Dirs {
    _root: tempfile::TempDir,
    project: PathBuf,
    staging: PathBuf,
    capture: PathBuf,
}

fn dirs() -> Dirs {
    let (root, project) = scratch_repo();
    let staging = root.path().join("staging");
    let capture = root.path().join("capture");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::create_dir_all(&capture).unwrap();
    Dirs {
        _root: root,
        project,
        staging,
        capture,
    }
}

fn write_request(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join("request.json");
    std::fs::write(&path, body).unwrap();
    path
}

fn base_invoke(dirs: &Dirs, request: &Path) -> Command {
    let mut cmd = command();
    cmd.args([
        "tool",
        "invoke",
        "--project",
        &dirs.project.display().to_string(),
        "--staging",
        &dirs.staging.display().to_string(),
        "--request",
        &request.display().to_string(),
        "--actor",
        "agent-cli",
        "--capture-out",
        &dirs.capture.display().to_string(),
    ]);
    cmd
}

#[test]
fn a_tier_0_read_reports_the_record_and_captures_the_bytes() {
    let dirs = dirs();
    let request = write_request(
        dirs.project.parent().unwrap(),
        r#"{"tool":"repository","action":"read_file","path":"src/lib.rs"}"#,
    );
    let output = base_invoke(&dirs, &request)
        .args(["--capability", "repository.read"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let reply = json(&output.stdout);
    assert_eq!(reply["ok"], Value::Bool(true));
    assert_eq!(reply["data"]["record"]["tier"], "tier_0");
    let file_bytes = std::fs::read(dirs.project.join("src/lib.rs")).unwrap();
    assert_eq!(
        reply["data"]["record"]["stdoutSha256"],
        Value::String(sha256_hex(&file_bytes))
    );
    // --capture-out wrote the bytes, and their digest equals the record's.
    let captured = std::fs::read(Path::new(
        reply["data"]["capturedTo"]["stdout"].as_str().unwrap(),
    ))
    .unwrap();
    assert_eq!(captured, file_bytes);
}

#[test]
fn a_tier_1_apply_leaves_project_and_staging_untouched_after_the_call() {
    let dirs = dirs();
    let patch = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1,2 @@\n // scratch\n+// patched\n";
    let request_body = serde_json::json!({
        "tool": "repository",
        "action": "apply_patch",
        "patch": patch,
    });
    let request = write_request(dirs.project.parent().unwrap(), &request_body.to_string());
    let before = std::fs::read(dirs.project.join("src/lib.rs")).unwrap();
    let output = base_invoke(&dirs, &request)
        .args(["--capability", "repository.write"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let reply = json(&output.stdout);
    assert_eq!(reply["data"]["record"]["disposition"]["kind"], "completed");
    assert_eq!(reply["data"]["record"]["disposition"]["exit_code"], 0);
    let after = std::fs::read(dirs.project.join("src/lib.rs")).unwrap();
    assert_eq!(before, after, "the project must be untouched");
    let staging_left: Vec<_> = std::fs::read_dir(&dirs.staging)
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert!(staging_left.is_empty(), "staging must be empty afterward");
}

/// #860, the CLI twin of #845's end-to-end cell: the operator-facing envelope names the NEW rule
/// for a malformed program, and never echoes the name. `GHCLI013` carries the rule string by
/// construction (`tool/mod.rs`), which is exactly the sentence that stops being true the day
/// someone normalises messages -- so it is pinned at the last surface an operator reads.
#[test]
fn a_malformed_shell_program_is_ghcli013_naming_the_shape_rule_without_echo() {
    let dirs = dirs();
    let malformed = "SENTINEL-bin/curl";
    let request = write_request(
        dirs.project.parent().unwrap(),
        &format!(
            r#"{{"tool":"shell","program":"{malformed}","arguments":["https://example.com"]}}"#
        ),
    );
    let output = base_invoke(&dirs, &request)
        .args(["--capability", "shell.execute", "--allow-program", "git"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let reply = json(&output.stdout);
    assert_eq!(reply["ok"], Value::Bool(false));
    assert_eq!(reply["diagnostics"][0]["code"], "GHCLI013_TOOL_DENIED");
    let message = reply["diagnostics"][0]["message"].as_str().unwrap();
    assert!(
        message.contains("program_name_invalid"),
        "the operator must be told to fix the NAME, not the lease: {message}"
    );
    let envelope = String::from_utf8_lossy(&output.stdout);
    assert!(
        !envelope.contains("SENTINEL"),
        "the malformed name reached the envelope: {envelope}"
    );
    let staging_left: Vec<_> = std::fs::read_dir(&dirs.staging)
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert!(staging_left.is_empty(), "a denial must not provision");
}

#[test]
fn a_non_allowlisted_shell_program_is_ghcli013_and_staging_untouched() {
    let dirs = dirs();
    let request = write_request(
        dirs.project.parent().unwrap(),
        r#"{"tool":"shell","program":"curl","arguments":["https://example.com"]}"#,
    );
    let output = base_invoke(&dirs, &request)
        .args(["--capability", "shell.execute", "--allow-program", "git"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let reply = json(&output.stdout);
    assert_eq!(reply["ok"], Value::Bool(false));
    assert_eq!(reply["diagnostics"][0]["code"], "GHCLI013_TOOL_DENIED");
    assert!(
        reply["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("program_denied")
    );
    let staging_left: Vec<_> = std::fs::read_dir(&dirs.staging)
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert!(staging_left.is_empty(), "a denial must not provision");
}

#[test]
fn an_unknown_field_in_the_request_is_ghcli012() {
    let dirs = dirs();
    let request = write_request(
        dirs.project.parent().unwrap(),
        r#"{"tool":"repository","action":"read_file","path":"src/lib.rs","extra":true}"#,
    );
    let output = base_invoke(&dirs, &request)
        .args(["--capability", "repository.read"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let reply = json(&output.stdout);
    assert_eq!(reply["diagnostics"][0]["code"], "GHCLI012_TOOL_INVALID");
}

#[test]
fn a_missing_capture_out_is_refused_before_anything_runs() {
    // Review finding 7 closed the ambiguity: every invoke captures, so --capture-out is
    // mandatory and its absence is GHCLI012 before any filesystem action (staging untouched).
    let dirs = dirs();
    let request = write_request(
        dirs.project.parent().unwrap(),
        r#"{"tool":"repository","action":"read_file","path":"src/lib.rs"}"#,
    );
    let output = command()
        .args([
            "tool",
            "invoke",
            "--project",
            &dirs.project.display().to_string(),
            "--staging",
            &dirs.staging.display().to_string(),
            "--request",
            &request.display().to_string(),
            "--actor",
            "agent-cli",
            "--capability",
            "repository.read",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let reply = json(&output.stdout);
    assert_eq!(reply["diagnostics"][0]["code"], "GHCLI012_TOOL_INVALID");
    assert!(
        reply["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("capture-out")
    );
}

#[test]
fn the_cli_never_echoes_an_ambient_secret() {
    // Task 8's assertion D restated through the real binary: a sentinel in the CLI process's
    // own environment must reach neither stdout nor stderr, on the success path.
    let dirs = dirs();
    let patch = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1,2 @@\n // scratch\n+// patched\n";
    let request_body = serde_json::json!({
        "tool": "repository",
        "action": "apply_patch",
        "patch": patch,
    });
    let request = write_request(dirs.project.parent().unwrap(), &request_body.to_string());
    let output = base_invoke(&dirs, &request)
        .args(["--capability", "repository.write"])
        .env("GRAPHHELM_EVENTS_KEY", "SENTINEL-cli-ambient-secret")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains("SENTINEL"), "sentinel reached stdout");
    assert!(!stderr.contains("SENTINEL"), "sentinel reached stderr");
    // And the captured stream files hold tool output, not the ambient secret.
    let stderr_file = std::fs::read_to_string(Path::new(
        json(&output.stdout)["data"]["capturedTo"]["stderr"]
            .as_str()
            .unwrap(),
    ))
    .unwrap();
    assert!(!stderr_file.contains("SENTINEL"));
}
