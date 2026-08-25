//! #213: per-contribution MCP capability tokens. Hostile and grounding fixtures for the
//! mint/verify pipeline built on top of `graphhelm_tool_broker::mcp_capability`.
//!
//! This first test grounds the precondition every later test depends on:
//! `ValidatedExtensionPackage` must actually carry each contribution's own `surfaces`/`effects`/
//! `permissions`/`requires.capabilities` -- today (before #213) it validates them and discards
//! them, per the blueprint's own measurement (`.factory/e-agent-213-blueprint.md` §1.3).

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

/// `effects`/`permissions` must carry the real authority a declared MCP `surface` needs
/// (`validate_authority_subset` in `core/schema/src/extension.rs`) -- an escalation-shaped
/// fixture (declares a tool surface without the matching effect/permission/package grant) is
/// exactly #213's own T4 threat, so building one correctly here is the honest baseline, not
/// incidental fixture noise.
fn write_resource(
    root: &Path,
    relative: &str,
    bytes: &[u8],
    surfaces: &[&str],
    effects: &[&str],
    permissions: &[&str],
) -> Value {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, bytes).unwrap();
    json!({
        "id": relative.replace(['/', '.'], "-"),
        "kind": "fixture",
        "path": relative,
        "sha256": digest(bytes),
        "effects": effects,
        "permissions": permissions,
        "requires": {"capabilities": [], "observers": []},
        "surfaces": surfaces
    })
}

fn manifest(contributions: Vec<Value>) -> Value {
    json!({
        "apiVersion": "p50.dev/v1",
        "kind": "Extension",
        "metadata": {
            "id": "graphhelm-mcp-capability-test",
            "version": "1.0.0",
            "publisher": "graphhelm"
        },
        "spec": {
            "type": "skill-package",
            "capabilities": ["journey_proof"],
            "permissions": {
                "filesystem": {"package": "read", "workspaceArtifacts": "proposal-write"},
                "network": {"external": false, "loopbackRuntimeApi": true},
                "runtime": {
                    "read": true,
                    "mutations": ["approve", "signal"],
                    "ownerConfirmationRequired": ["approve", "signal"]
                },
                "secrets": {"artifactValues": false, "tokenFile": false}
            },
            "contracts": {"contributions": contributions},
            "runtime": {"kind": "data", "isolationMinimum": "tier_0"},
            "compatibility": {"framework": ">=0.1"}
        }
    })
}

struct PackageFixture {
    _directory: TempDir,
    root: PathBuf,
}

fn two_contribution_package() -> PackageFixture {
    let directory = TempDir::new().unwrap();
    let root = directory.path().to_path_buf();

    let approver = write_resource(
        &root,
        "fixtures/approver.json",
        b"{}",
        &["tool:approve", "tool:signal"],
        &["runtime.connect", "runtime.mutate"],
        &[
            "network.loopback",
            "owner.decision.request",
            "runtime.write",
        ],
    );
    let curator = write_resource(&root, "fixtures/curator.json", b"{}", &[], &[], &[]);

    let manifest = manifest(vec![approver, curator]);
    fs::write(
        root.join("extension.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    PackageFixture {
        _directory: directory,
        root,
    }
}

#[test]
fn validated_package_exposes_each_contributions_own_surfaces() {
    let package = two_contribution_package();
    let validated = graphhelm_schema::validate_extension_package(&package.root)
        .unwrap_or_else(|diagnostics| panic!("expected a valid package, got {diagnostics:?}"));

    let approver = validated
        .contributions
        .iter()
        .find(|c| c.id == "fixtures-approver-json")
        .expect("the approver contribution must be present");
    assert_eq!(
        approver.surfaces,
        vec!["tool:approve".to_owned(), "tool:signal".to_owned()],
        "the approver contribution's own declared surfaces must survive validation"
    );

    let curator = validated
        .contributions
        .iter()
        .find(|c| c.id == "fixtures-curator-json")
        .expect("the curator contribution must be present");
    assert!(
        curator.surfaces.is_empty(),
        "a contribution that declares no surfaces must not inherit another contribution's -- \
         got {:?}",
        curator.surfaces
    );
}

// -------------------------------------------------------------------------------------------
// Session wiring: `graphhelm mcp --actor-type agent` requires a presented capability token;
// `--actor-type owner` does not (the existing elevated-trust actor, unchanged by #213).
// Mirrors `apps/cli/tests/mcp_stdio.rs`'s own subprocess harness -- each integration test file
// is self-contained by this crate's own convention (apps/cli is bin-only, no lib target).
// -------------------------------------------------------------------------------------------

use std::time::Duration;

struct McpSession {
    replies: Vec<Value>,
    output: std::process::Output,
}

fn mcp_session_with(args: &[&str], env: &[(&str, &str)], lines: &[Value]) -> McpSession {
    let mut input = String::new();
    for line in lines {
        input.push_str(&line.to_string());
        input.push('\n');
    }
    let mut command = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
    command.arg("mcp").args(args);
    for (name, value) in env {
        command.env(name, value);
    }
    let output = command
        .write_stdin(input)
        .timeout(Duration::from_secs(30))
        .output()
        .expect("the mcp server runs to EOF");
    let replies = String::from_utf8(output.stdout.clone())
        .expect("stdout is UTF-8")
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|value| value.get("jsonrpc").is_some())
        .collect();
    McpSession { replies, output }
}

fn write_token_file(directory: &Path, token: &Value) -> PathBuf {
    let path = directory.join("capability-token.json");
    fs::write(&path, serde_json::to_vec(token).unwrap()).unwrap();
    path
}

#[test]
fn a_session_with_no_capability_token_file_is_unaffected_by_213() {
    // #213 is opt-in by FLAG PRESENCE, not by `--actor-type`: `agent` is the default for
    // ordinary chat sessions with no extension contribution to scope (mcp_stdio.rs's whole
    // existing suite runs this way), so gating on actor-type alone would refuse the common
    // case, not the threat. Regression pin -- this exact shape broke 13 existing tests during
    // development before the design was corrected to presence-based gating.
    let session = mcp_session_with(
        &["--url", "http://127.0.0.1:9", "--actor", "agent-x"],
        &[("GRAPHHELM_API_TOKEN", "test-token")],
        &[serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "ping"})],
    );
    assert!(
        session.output.status.success(),
        "no capability token, no change: {:?}",
        session.output
    );
    assert_eq!(session.replies.len(), 1, "{:?}", session.replies);
    assert_eq!(session.replies[0]["result"], serde_json::json!({}));
}

#[test]
fn a_capability_token_file_without_a_package_is_refused_before_any_protocol_byte() {
    let directory = TempDir::new().unwrap();
    let token_path = write_token_file(
        directory.path(),
        &json!({
            "packageDigest": "sha256:aaaa",
            "contributionId": "skill/code-contract",
            "actor": "agent-x",
            "allowedTools": ["approve"],
            "revoked": false,
            "expiresAt": 9_999_999_999_u64
        }),
    );
    let session = mcp_session_with(
        &[
            "--url",
            "http://127.0.0.1:9",
            "--actor",
            "agent-x",
            "--capability-token-file",
            token_path.to_str().unwrap(),
        ],
        &[("GRAPHHELM_API_TOKEN", "test-token")],
        &[],
    );
    assert!(
        !session.output.status.success(),
        "a capability token without --package must refuse to start: {:?}",
        session.output
    );
    let stdout = String::from_utf8_lossy(&session.output.stdout);
    assert!(stdout.contains("GHCLI015"), "{stdout}");
}

#[test]
fn a_capability_token_file_without_an_audit_log_path_is_refused_before_any_protocol_byte() {
    let package = two_contribution_package();
    let directory = TempDir::new().unwrap();
    let token_path = write_token_file(
        directory.path(),
        &json!({
            "packageDigest": "sha256:aaaa",
            "contributionId": "skill/code-contract",
            "actor": "agent-x",
            "allowedTools": ["approve"],
            "revoked": false,
            "expiresAt": 9_999_999_999_u64
        }),
    );
    let session = mcp_session_with(
        &[
            "--url",
            "http://127.0.0.1:9",
            "--actor",
            "agent-x",
            "--capability-token-file",
            token_path.to_str().unwrap(),
            "--package",
            package.root.to_str().unwrap(),
        ],
        &[("GRAPHHELM_API_TOKEN", "test-token")],
        &[],
    );
    assert!(
        !session.output.status.success(),
        "a capability token without --capability-audit-log must refuse to start: {:?}",
        session.output
    );
    let stdout = String::from_utf8_lossy(&session.output.stdout);
    assert!(stdout.contains("GHCLI015"), "{stdout}");
}

fn initialize_request(id: u64) -> Value {
    json!({
        "jsonrpc": "2.0", "id": id, "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "conformance", "version": "0"}
        }
    })
}

fn initialized_notification() -> Value {
    json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
}

/// A package with one contribution's real digest, for the capability-gating tests below --
/// `allowed_tools` on the minted token names only `signal`, never `approve`.
fn gated_session_args<'a>(
    package_root: &'a Path,
    token_path: &'a str,
    actor: &'a str,
    audit_log_path: &'a str,
) -> Vec<&'a str> {
    vec![
        "--url",
        "http://127.0.0.1:9",
        "--actor",
        actor,
        "--capability-token-file",
        token_path,
        "--capability-audit-log",
        audit_log_path,
        "--package",
    ]
    .into_iter()
    .chain(std::iter::once(package_root.to_str().unwrap()))
    .collect()
}

fn approver_token(package: &Path, allowed_tools: &[&str]) -> Value {
    let digest = graphhelm_schema::validate_extension_package(package)
        .unwrap()
        .package_digest;
    json!({
        "packageDigest": digest,
        "contributionId": "fixtures-approver-json",
        "actor": "agent-x",
        "allowedTools": allowed_tools,
        "revoked": false,
        "expiresAt": 9_999_999_999_u64
    })
}

fn read_jsonl(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn tools_call_is_refused_when_the_tool_is_outside_the_presented_tokens_allowlist() {
    let package = two_contribution_package();
    let directory = TempDir::new().unwrap();
    let token_path = write_token_file(
        directory.path(),
        &approver_token(&package.root, &["signal"]),
    );
    let audit_log = directory.path().join("audit.jsonl");
    let args = gated_session_args(
        &package.root,
        token_path.to_str().unwrap(),
        "agent-x",
        audit_log.to_str().unwrap(),
    );
    let session = mcp_session_with(
        &args,
        &[("GRAPHHELM_API_TOKEN", "test-token")],
        &[
            initialize_request(1),
            initialized_notification(),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": {"name": "approve", "arguments": {"executionId": "exec-x"}}}),
        ],
    );
    assert_eq!(session.replies.len(), 2, "{:?}", session.replies);
    let refusal = &session.replies[1];
    assert!(
        refusal.get("error").is_some(),
        "approve is outside the token's allowlist and must be refused: {refusal}"
    );
    let message = refusal["error"]["message"].as_str().unwrap();
    assert!(
        message.contains("capability") || message.contains("allow"),
        "the refusal names the capability gate, not a downstream transport error: {message}"
    );

    let records = read_jsonl(&audit_log);
    assert_eq!(records.len(), 1, "one decision recorded: {records:?}");
    assert_eq!(records[0]["toolName"], "approve");
    assert_eq!(records[0]["decision"]["outcome"], "refused");
    assert_eq!(records[0]["decision"]["code"], "tool_not_allowlisted");
    assert!(
        records[0].get("arguments").is_none() && !records[0].to_string().contains("exec-x"),
        "the call's arguments (executionId exec-x) must never reach the audit record: {:?}",
        records[0]
    );
}

#[test]
fn tools_call_proceeds_past_the_capability_gate_when_the_tool_is_allowlisted() {
    let package = two_contribution_package();
    let directory = TempDir::new().unwrap();
    let token_path = write_token_file(
        directory.path(),
        &approver_token(&package.root, &["signal"]),
    );
    let audit_log = directory.path().join("audit.jsonl");
    let args = gated_session_args(
        &package.root,
        token_path.to_str().unwrap(),
        "agent-x",
        audit_log.to_str().unwrap(),
    );
    let session = mcp_session_with(
        &args,
        &[("GRAPHHELM_API_TOKEN", "test-token")],
        &[
            initialize_request(1),
            initialized_notification(),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": {"name": "signal", "arguments": {"executionId": "exec-x",
                    "envelope": {}}}}),
        ],
    );
    assert_eq!(session.replies.len(), 2, "{:?}", session.replies);
    let reply = &session.replies[1];
    if let Some(message) = reply["error"]["message"].as_str() {
        assert!(
            !message.contains("capability") && !message.contains("allowlist"),
            "signal IS allowlisted; any failure here must be the dead-port transport, not the \
             capability gate: {message}"
        );
    }

    let records = read_jsonl(&audit_log);
    assert_eq!(records.len(), 1, "one decision recorded: {records:?}");
    assert_eq!(records[0]["toolName"], "signal");
    assert_eq!(records[0]["decision"]["outcome"], "allowed");
}

// -------------------------------------------------------------------------------------------
// `graphhelm extension mint-mcp-token` / `revoke-mcp-token`: the operator tooling that
// actually produces the files the session-wiring tests above consume. Prints JSON to stdout,
// matching `extension validate`'s own established idiom -- the caller redirects to a file
// itself; no new file-write semantics invented for this one command.
// -------------------------------------------------------------------------------------------

fn cli() -> assert_cmd::Command {
    assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
}

#[test]
fn mint_mcp_token_derives_the_allowlist_from_the_named_contributions_own_surfaces() {
    let package = two_contribution_package();
    let output = cli()
        .args(["extension", "mint-mcp-token", "--package"])
        .arg(&package.root)
        .args([
            "--contribution",
            "fixtures-approver-json",
            "--actor",
            "agent-x",
            "--ttl-seconds",
            "3600",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "mint-mcp-token must succeed: {output:?}"
    );
    let outcome: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(outcome["ok"], true, "{outcome}");
    let token = &outcome["data"];
    assert_eq!(token["contributionId"], "fixtures-approver-json");
    assert_eq!(token["actor"], "agent-x");
    assert_eq!(token["revoked"], false);
    let allowed: std::collections::BTreeSet<String> = token["allowedTools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        allowed,
        ["approve", "signal"]
            .map(str::to_owned)
            .into_iter()
            .collect(),
        "derived from the approver contribution's own surfaces: {token}"
    );
    let digest = graphhelm_schema::validate_extension_package(&package.root)
        .unwrap()
        .package_digest;
    assert_eq!(token["packageDigest"], digest);
}

#[test]
fn mint_mcp_token_refuses_an_unknown_contribution_id() {
    let package = two_contribution_package();
    let output = cli()
        .args(["extension", "mint-mcp-token", "--package"])
        .arg(&package.root)
        .args([
            "--contribution",
            "fixtures-does-not-exist",
            "--actor",
            "agent-x",
            "--ttl-seconds",
            "3600",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success(), "{output:?}");
}

#[test]
fn revoke_mcp_token_flips_the_revoked_flag_and_nothing_else() {
    let package = two_contribution_package();
    let mint_output = cli()
        .args(["extension", "mint-mcp-token", "--package"])
        .arg(&package.root)
        .args([
            "--contribution",
            "fixtures-approver-json",
            "--actor",
            "agent-x",
            "--ttl-seconds",
            "3600",
        ])
        .output()
        .unwrap();
    let minted: Value = serde_json::from_slice(&mint_output.stdout).unwrap();
    let directory = TempDir::new().unwrap();
    let token_path = write_token_file(directory.path(), &minted["data"]);

    let revoke_output = cli()
        .args(["extension", "revoke-mcp-token", "--token-file"])
        .arg(&token_path)
        .output()
        .unwrap();
    assert!(revoke_output.status.success(), "{revoke_output:?}");
    let revoked: Value = serde_json::from_slice(&revoke_output.stdout).unwrap();
    let mut expected = minted["data"].clone();
    expected["revoked"] = json!(true);
    assert_eq!(
        revoked["data"], expected,
        "only `revoked` flips; every other bound field is untouched"
    );
}
