//! #213: per-contribution MCP capability tokens. Hostile and grounding fixtures for the
//! mint/verify pipeline built on top of `graphhelm_tool_broker::mcp_capability`.
//!
//! This first test grounds the precondition every later test depends on:
//! `ValidatedExtensionPackage` must actually carry each contribution's own `surfaces`/`effects`/
//! `permissions`/`requires.capabilities` -- today (before #213) it validates them and discards
//! them, per the blueprint's own measurement (design note #213, §1.3).

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

fn development_capability_package() -> PackageFixture {
    let directory = TempDir::new().unwrap();
    let root = directory.path().to_path_buf();
    let resolver = write_resource(
        &root,
        "fixtures/development-resolver.json",
        b"{}",
        &["tool:resolve_contract"],
        &["runtime.connect"],
        &["network.loopback"],
    );
    let manifest = manifest(vec![resolver]);
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

#[test]
fn a_minted_development_tool_is_allowed_while_another_is_refused_by_the_same_token() {
    // `resolve_contract` and `compile_context` are the MCP names for the
    // `development.resolve-contract` and `development.compile-context` operations. Keep exactly
    // one on the token so this one real session proves both sides of the per-tool allowlist
    // rather than inferring development-family coverage from other tools.
    let package = development_capability_package();
    let directory = TempDir::new().unwrap();
    let mint_output = cli()
        .args(["extension", "mint-mcp-token", "--package"])
        .arg(&package.root)
        .args([
            "--contribution",
            "fixtures-development-resolver-json",
            "--actor",
            "agent-x",
            "--ttl-seconds",
            "3600",
        ])
        .output()
        .unwrap();
    assert!(
        mint_output.status.success(),
        "the production mint command must accept the development contribution: {mint_output:?}"
    );
    let minted: Value = serde_json::from_slice(&mint_output.stdout).unwrap();
    assert_eq!(minted["ok"], true, "{minted}");
    assert_eq!(
        minted["data"]["allowedTools"],
        json!(["resolve_contract"]),
        "the production mint must derive the exact development allowlist from contribution surfaces"
    );
    let token_path = write_token_file(directory.path(), &minted["data"]);
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
                "params": {"name": "resolve_contract", "arguments": {}}}),
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": {"name": "compile_context", "arguments": {}}}),
        ],
    );

    assert_eq!(session.replies.len(), 3, "{:?}", session.replies);
    if let Some(message) = session.replies[1]["error"]["message"].as_str() {
        assert!(
            !message.contains("capability") && !message.contains("allowlist"),
            "resolve_contract IS the token's sole allowlisted development tool; any failure must \
             be downstream of the capability gate: {message}"
        );
    }
    let refusal = &session.replies[2];
    assert!(
        refusal.get("error").is_some(),
        "compile_context is outside the token's allowlist and must be refused: {refusal}"
    );

    let records = read_jsonl(&audit_log);
    assert_eq!(
        records.len(),
        2,
        "one decision per development tool: {records:?}"
    );
    assert_eq!(records[0]["toolName"], "resolve_contract");
    assert_eq!(records[0]["decision"]["outcome"], "allowed");
    assert_eq!(records[1]["toolName"], "compile_context");
    assert_eq!(records[1]["decision"]["outcome"], "refused");
    assert_eq!(
        records[1]["decision"]["code"], "tool_not_allowlisted",
        "the different development tool must be refused by the capability gate: {records:?}"
    );
}

/// #341: the two tests above prove the capability gate refuses/allows `approve` and `signal`
/// specifically -- they say nothing about the other 12 tools the server actually serves. The
/// gate's own code is structurally centralized (one `tools/call` match arm, checked before any
/// per-tool dispatch -- see `session.rs`), so a per-tool bypass is not the live risk; an
/// UNTESTED population is. This reads the real `tools/list` reply -- never the `TOOLS` const,
/// never a hand-typed array -- so a tool silently added or removed from that list changes what
/// this test covers without anyone needing to update it (population-is-the-instrument, #318's
/// sibling discipline applied to this subsystem).
fn real_tool_population() -> Vec<String> {
    let session = mcp_session_with(
        &["--url", "http://127.0.0.1:9", "--actor", "agent-x"],
        &[("GRAPHHELM_API_TOKEN", "test-token")],
        &[
            initialize_request(1),
            initialized_notification(),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        ],
    );
    session.replies[1]["result"]["tools"]
        .as_array()
        .expect("tools/list must reply with a \"tools\" array")
        .iter()
        .map(|tool| {
            tool["name"]
                .as_str()
                .expect("each listed tool has a string \"name\"")
                .to_owned()
        })
        .collect()
}

#[test]
fn every_real_tool_is_gated_by_capability_not_just_the_two_already_covered() {
    let population = real_tool_population();
    // non-empty-is-not-a-control: asserted BEFORE the sweep below, as its own assertion. A
    // broken extraction (wrong JSON pointer, over-restrictive filter) would otherwise yield an
    // empty population, and a `for` loop over zero items makes every assertion in that loop
    // pass vacuously -- this test would report green while checking nothing.
    assert!(
        !population.is_empty(),
        "tools/list returned no tools -- either the server or this extraction is broken"
    );
    // A shrinking population is exactly as invisible to a bare non-empty check as an empty
    // one would be to no check at all: naming today's real count catches a tool quietly
    // dropped from tools/list, not only a tool added.
    //
    // #421: this said 14 while the server served 20, so six tools could be dropped and the floor
    // would stay green -- the precise regression the comment above claims it catches. Measured:
    // 14 WAS the real count when the floor was written (24bb6c5, #345), so this is drift and not
    // a number taken from the wrong list.
    //
    // The history says what KIND of failure it is, and D measured it: the floor and all six tools
    // that overtook it are dated 2026-08-25 -- resolve_contract (1c115b1), memory_status (e01cb5a),
    // present (9746bd1), compile_context (846bbf4), memory_propose (508038f), accounting (a9e3a39).
    // Set the same day, passed six times the same day. A number taken from the wrong list is a
    // mistake made once; a number that was correct and was passed six times is missing MAINTENANCE.
    //
    // #288 raises it a seventh time, in this same commit: sweep (62cf3d1) is the tool that
    // arrived, per the rule two paragraphs down.
    //
    // So the rule beside it needs BOTH directions, and the second is the load-bearing one here.
    // The scan floors of #368/#369 carry the first: **lower this only in the same commit as the
    // removal that caused it, and name the removed tool.** This floor rises, so it also carries:
    // **RAISE THIS IN THE SAME COMMIT AS THE ADDITION, AND NAME THE TOOL THAT ARRIVED.** A floor
    // pinned to a count decays by construction unless something moves it, and a rule that only
    // moves it downward catches half the ways it goes stale.
    //
    // This floor stays a floor and is deliberately NOT turned into an exact set:
    // `mcp_stdio.rs::tools_list_names_exactly_the_twenty_one_tools_with_closed_schemas` already pins
    // the exact list, in order. That test is the one that says WHICH tool went missing; a second
    // exact list here would duplicate an ORACLE rather than a mechanism, and two copies of an
    // oracle disagree in silence.
    assert!(
        population.len() >= 21,
        "expected at least 21 tools (today's real count); got {}: {population:?}",
        population.len()
    );

    // Negative sweep: an EMPTY capability allowlist must refuse every single tool the server
    // actually serves -- one session, one subprocess, one tools/call per population member.
    let package = two_contribution_package();
    let directory = TempDir::new().unwrap();
    let token_path = write_token_file(directory.path(), &approver_token(&package.root, &[]));
    let audit_log = directory.path().join("audit.jsonl");
    let args = gated_session_args(
        &package.root,
        token_path.to_str().unwrap(),
        "agent-x",
        audit_log.to_str().unwrap(),
    );
    let mut lines = vec![initialize_request(1), initialized_notification()];
    for (index, name) in population.iter().enumerate() {
        lines.push(json!({
            "jsonrpc": "2.0",
            "id": 2 + index as u64,
            "method": "tools/call",
            "params": {"name": name, "arguments": {}}
        }));
    }
    let session = mcp_session_with(&args, &[("GRAPHHELM_API_TOKEN", "test-token")], &lines);
    assert_eq!(
        session.replies.len(),
        1 + population.len(),
        "one reply for initialize, then one per tools/call: {:?}",
        session.replies
    );
    for (name, reply) in population.iter().zip(session.replies[1..].iter()) {
        assert!(
            reply.get("error").is_some(),
            "{name} was called with an EMPTY capability allowlist and must be refused, got: \
             {reply}"
        );
        let message = reply["error"]["message"].as_str().unwrap_or_default();
        assert!(
            message.contains("capability") || message.contains("allow"),
            "{name}'s refusal must name the capability gate, not a downstream error: {message}"
        );
    }
    let records = read_jsonl(&audit_log);
    assert_eq!(records.len(), population.len(), "{records:?}");
    assert!(
        records
            .iter()
            .all(|record| record["decision"]["code"] == "tool_not_allowlisted"),
        "every one of the {} calls must be refused for the SAME reason (empty allowlist), not \
         a mix: {records:?}",
        population.len()
    );

    // Positive control: with the SAME population, a NON-empty allowlist containing one real
    // name must actually pass the gate. Without this arm, the sweep above cannot tell "the
    // gate correctly checks every tool" from "the gate refuses unconditionally regardless of
    // the allowlist" (instrument-speaks-is-not-subject-exists) -- both would look identical
    // from the negative sweep alone.
    let allowed_name = population[0].as_str();
    let control_directory = TempDir::new().unwrap();
    let control_token_path = write_token_file(
        control_directory.path(),
        &approver_token(&package.root, &[allowed_name]),
    );
    let control_audit_log = control_directory.path().join("audit.jsonl");
    let control_args = gated_session_args(
        &package.root,
        control_token_path.to_str().unwrap(),
        "agent-x",
        control_audit_log.to_str().unwrap(),
    );
    mcp_session_with(
        &control_args,
        &[("GRAPHHELM_API_TOKEN", "test-token")],
        &[
            initialize_request(1),
            initialized_notification(),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": {"name": allowed_name, "arguments": {}}}),
        ],
    );
    let control_records = read_jsonl(&control_audit_log);
    assert_eq!(control_records.len(), 1, "{control_records:?}");
    assert_eq!(
        control_records[0]["decision"]["outcome"], "allowed",
        "positive control: {allowed_name} IS on the allowlist and must pass the gate -- {:?}",
        control_records
    );
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
