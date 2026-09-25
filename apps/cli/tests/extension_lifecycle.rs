//! CLI-level lifecycle conformance (#212): install, switch, rollback and uninstall through the
//! REAL binary, asserting the same JSON envelope every other command speaks.
//!
//! The fixture is a SYNTHETIC package built by the same rules `extension_cli.rs` uses -- valid
//! by construction, not by hoping the shipped package stays valid. That choice was forced by
//! measurement (#589): the shipped builtin stopped validating on main when undeclared benchmark
//! fixtures landed inside its tree (GHEX012), and every suite pinned to it went red at once. A
//! marker folded into the skill body gives each fixture its own identity, because the digest
//! binds the bytes; no manifest surgery is needed to make two versions.

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::{Value, json};
use tempfile::TempDir;

fn digest(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    format!("sha256:{}", hex::encode(sha2::Sha256::digest(bytes)))
}

fn write_resource(root: &Path, relative: &str, bytes: &[u8]) -> Value {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, bytes).unwrap();
    json!({
        "id": relative.replace(['/', '.'], "-"),
        "kind": "fixture",
        "path": relative,
        "sha256": digest(bytes),
        "effects": [],
        "permissions": [],
        "requires": {"capabilities": [], "observers": []}
    })
}

/// A package that validates by construction, its identity decided by `marker`.
fn synthetic_package(marker: &str) -> (TempDir, PathBuf) {
    let directory = TempDir::new().unwrap();
    let root = directory.path().to_path_buf();

    let skill = format!(
        "---\nname: journey-contract\ndescription: Compile one user journey into observable \
         promises and failure contracts.\n---\n\n# Journey contract ({marker})\n\nRead with \
         `tool:status`, then validate locally with `cli:graph validate`.\n"
    );
    let skill = skill.as_bytes();
    std::fs::create_dir_all(root.join("skills/journey-contract")).unwrap();
    std::fs::write(root.join("skills/journey-contract/SKILL.md"), skill).unwrap();
    let skill_contribution = json!({
        "id": "journey-contract",
        "kind": "skill",
        "path": "skills/journey-contract/SKILL.md",
        "sha256": digest(skill),
        "surfaces": ["tool:status", "cli:graph validate"],
        "effects": ["runtime.connect", "runtime.read"],
        "permissions": ["network.loopback", "runtime.read"],
        "requires": {"capabilities": [], "observers": []},
        "family": "journey"
    });

    let claude = br#"{"name":"graphhelm-jpd-test","version":"1.0.0"}"#;
    let mut claude_contribution = write_resource(&root, ".claude-plugin/plugin.json", claude);
    claude_contribution["id"] = json!("claude-host");
    claude_contribution["kind"] = json!("host-adapter");

    let codex = br#"{"id":"graphhelm-jpd-test","name":"graphhelm-jpd-test","version":"1.0.0"}"#;
    let mut codex_contribution = write_resource(&root, ".codex-plugin/plugin.json", codex);
    codex_contribution["id"] = json!("codex-host");
    codex_contribution["kind"] = json!("host-adapter");

    let mcp = br#"{"mcpServers":{"graphhelm":{"command":"${GRAPHHELM_CLI}","args":["mcp","--url","http://127.0.0.1:8080","--token-file","${GRAPHHELM_TOKEN_FILE}","--actor","${GRAPHHELM_ACTOR}"]}}}"#;
    let mut mcp_contribution = write_resource(&root, ".mcp.json", mcp);
    mcp_contribution["id"] = json!("graphhelm-mcp-host");
    mcp_contribution["kind"] = json!("host-adapter");
    mcp_contribution["surfaces"] = json!(["cli:mcp"]);
    mcp_contribution["effects"] = json!(["runtime.connect"]);
    mcp_contribution["permissions"] = json!(["network.loopback", "token.reference.read"]);

    let manifest = json!({
        "apiVersion": "p50.dev/v1",
        "kind": "Extension",
        "metadata": {
            "id": "graphhelm-jpd-test",
            "version": "1.0.0",
            "publisher": "graphhelm"
        },
        "spec": {
            "type": "skill-package",
            "capabilities": ["journey_proof"],
            "permissions": {
                "filesystem": {
                    "package": "read",
                    "workspaceArtifacts": "proposal-write"
                },
                "network": {
                    "external": false,
                    "loopbackRuntimeApi": true
                },
                "runtime": {
                    "read": true,
                    "mutations": [],
                    "ownerConfirmationRequired": []
                },
                "secrets": {
                    "artifactValues": false,
                    "tokenFile": "reference-only"
                }
            },
            "contracts": {"contributions": [
                skill_contribution,
                claude_contribution,
                codex_contribution,
                mcp_contribution
            ]},
            "runtime": {"kind": "data", "isolationMinimum": "tier_0"},
            "compatibility": {"framework": ">=0.1"}
        }
    });
    std::fs::write(
        root.join("extension.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    (directory, root)
}

fn extension(args: &[&str]) -> std::process::Output {
    Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .arg("extension")
        .args(args)
        .output()
        .expect("the binary runs")
}

fn ok_json(output: &std::process::Output, command: &str) -> Value {
    assert!(
        output.status.success(),
        "expected success from {command}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("stdout is one JSON value");
    assert_eq!(value["ok"], true);
    assert_eq!(value["command"], command);
    value
}

fn domain_json(output: &std::process::Output, command: &str, code: &str, pointer: &str) -> Value {
    assert_eq!(
        output.status.code(),
        Some(2),
        "a refusal is a DOMAIN outcome, exit 2, never a panic or a success: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("stdout is one JSON value");
    assert_eq!(value["ok"], false);
    assert_eq!(value["command"], command);
    let codes: Vec<&str> = value["diagnostics"]
        .as_array()
        .expect("diagnostics is an array")
        .iter()
        .filter_map(|d| d["code"].as_str())
        .collect();
    assert!(codes.contains(&code), "expected {code} among {codes:?}");
    let pointers: Vec<&str> = value["diagnostics"]
        .as_array()
        .expect("diagnostics is an array")
        .iter()
        .filter_map(|d| d["path"].as_str())
        .collect();
    assert!(
        pointers.contains(&pointer),
        "the pointer must name an argument the INVOKING command has, or a client cannot \
         associate the diagnostic with a field: expected {pointer} among {pointers:?}"
    );
    value
}

#[test]
fn the_lifecycle_holds_end_to_end_through_the_real_binary() {
    let root = tempfile::tempdir().expect("a temp dir");
    let root_arg = root.path().to_str().expect("root path is unicode");
    let (_keep_a, package_a) = synthetic_package("version-a");
    let (_keep_b, package_b) = synthetic_package("version-b");

    let installed_a = ok_json(
        &extension(&[
            "install",
            "--root",
            root_arg,
            "--package",
            package_a.to_str().unwrap(),
        ]),
        "extension.install",
    );
    let digest_a = installed_a["data"]["digest"]
        .as_str()
        .expect("a digest")
        .to_owned();
    assert!(
        digest_a.starts_with("sha256:"),
        "the digest is the validator's"
    );
    assert!(
        installed_a["data"]["root"].is_null(),
        "normal CLI JSON must not echo the adopted path: stdout is persisted into logs by \
         automation, and an absolute path under the caller's root is the #592 class of leak"
    );

    let installed_b = ok_json(
        &extension(&[
            "install",
            "--root",
            root_arg,
            "--package",
            package_b.to_str().unwrap(),
        ]),
        "extension.install",
    );
    let digest_b = installed_b["data"]["digest"]
        .as_str()
        .expect("b digest")
        .to_owned();
    assert_ne!(
        digest_a, digest_b,
        "HARNESS-BROKE: one identity, two fixtures"
    );

    let first = ok_json(
        &extension(&["switch", "--root", root_arg, "--digest", &digest_a]),
        "extension.switch",
    );
    assert_eq!(first["data"]["current"], digest_a.as_str());
    assert_eq!(first["data"]["previous"], Value::Null);

    let second = ok_json(
        &extension(&["switch", "--root", root_arg, "--digest", &digest_b]),
        "extension.switch",
    );
    assert_eq!(second["data"]["current"], digest_b.as_str());
    assert_eq!(second["data"]["previous"], digest_a.as_str());

    let rolled = ok_json(
        &extension(&["rollback", "--root", root_arg]),
        "extension.rollback",
    );
    assert_eq!(rolled["data"]["current"], digest_a.as_str());
    assert_eq!(
        rolled["data"]["previous"],
        digest_b.as_str(),
        "the rolled-away version stays recorded, so the flip is reversible"
    );

    // B is PREVIOUS after the rollback: uninstalling it must refuse as a domain outcome with the
    // lifecycle code, and the refusal must reach a script as exit 2 -- not a panic, not a 0.
    domain_json(
        &extension(&["uninstall", "--root", root_arg, "--digest", &digest_b]),
        "extension.uninstall",
        "GHCLI024_EXTENSION_LIFECYCLE_REFUSED",
        "/digest",
    );

    // A third, free version uninstalls cleanly.
    let (_keep_c, package_c) = synthetic_package("version-c");
    let installed_c = ok_json(
        &extension(&[
            "install",
            "--root",
            root_arg,
            "--package",
            package_c.to_str().unwrap(),
        ]),
        "extension.install",
    );
    let digest_c = installed_c["data"]["digest"]
        .as_str()
        .expect("c digest")
        .to_owned();
    ok_json(
        &extension(&["uninstall", "--root", root_arg, "--digest", &digest_c]),
        "extension.uninstall",
    );
}

#[test]
fn a_switch_to_an_unknown_digest_is_a_domain_refusal_not_a_panic() {
    let root = tempfile::tempdir().expect("a temp dir");
    let root_arg = root.path().to_str().expect("root path is unicode");
    let absent = format!("sha256:{}", "0".repeat(64));
    domain_json(
        &extension(&["switch", "--root", root_arg, "--digest", &absent]),
        "extension.switch",
        "GHCLI024_EXTENSION_LIFECYCLE_REFUSED",
        "/digest",
    );
}
