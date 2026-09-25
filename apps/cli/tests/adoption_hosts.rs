use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;
fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn host(root: &Path, name: &str) -> std::path::PathBuf {
    let executable = root.join("inert-host.exe");
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../adapters/host-adoption/tests/fixtures/inert-host.rs");
    let status = std::process::Command::new("rustc")
        .args(["--edition=2024"])
        .arg(source)
        .arg("-o")
        .arg(&executable)
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::write(
        executable.with_extension("input"),
        if name == "claude" {
            "2.1.265 (Claude Code)\n--plugin-dir\n"
        } else {
            "codex-cli 0.114.0\nplugins browser\n"
        },
    )
    .unwrap();
    executable
}
fn exercise(name: &str) -> (Value, bool, Vec<u8>) {
    let p = tempfile::tempdir().unwrap();
    let h = tempfile::tempdir().unwrap();
    let s = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(s.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let program = host(p.path(), name);
    std::fs::write(p.path().join("AGENTS.md"), b"old\n").unwrap();
    let packages = graphhelm_host_adoption::hosts::release_packages().unwrap();
    let mut plan = json!({"apiVersion":"p50.dev/adoption/v1","kind":"AdoptionPlan","id":"cli-host-plan","spec":{"coverage":"complete","rootBindings":graphhelm_host_adoption::root_bindings(p.path(),h.path()).unwrap(),"scopes":["project"],"packages":packages.iter().map(|p| json!({"id":p.id,"version":p.version,"digest":p.digest})).collect::<Vec<_>>(),"hostBoundary":"quiescent","host":{"name":name,"program":program,"version":if name == "claude" {"2.1.265"} else {"0.114.0"},"mode":"local_plugin_dir"},"decisions":[{"operationIndex":0,"decision":"replace","protected":false}],"operations":[{"root":"project","path":"AGENTS.md","beforeDigest":hash(b"old\n"),"afterDigest":hash(b"new\n"),"after":"new\n"}]}});
    let accepted = format!(
        "sha256:{}",
        hash(&graphhelm_graph::canonical_content_bytes(&plan).unwrap())
    );
    plan["digest"] = json!(accepted);
    let planfile = s.path().join("plan.json");
    std::fs::write(&planfile, serde_json::to_vec(&plan).unwrap()).unwrap();
    let mut command = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
    command
        .arg("setup")
        .arg("--project")
        .arg(p.path())
        .arg("--home")
        .arg(h.path())
        .arg("--state-root")
        .arg(s.path())
        .arg("--apply")
        .arg(planfile)
        .arg("--accept")
        .arg(accepted);
    for package in packages {
        command.arg("--package").arg(package.path);
    }
    let output = command.output().unwrap();
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    (
        serde_json::from_slice(&output.stdout).unwrap(),
        output.status.success(),
        std::fs::read(p.path().join("AGENTS.md")).unwrap(),
    )
}
#[test]
#[cfg(windows)]
fn cli_local_packages_keep_the_json_envelope_and_unverified_state() {
    let (reply, success, bytes) = exercise("claude");
    assert!(success, "{reply}");
    assert_eq!(reply["command"], "setup");
    assert_eq!(
        reply["data"]["receipt"]["spec"]["state"],
        "installed_unverified"
    );
    assert_eq!(
        reply["data"]["receipt"]["packages"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(bytes, b"new\n");
}
#[test]
#[cfg(windows)]
fn cli_codex_browser_required_is_actionable_and_leaves_instruction_bytes_intact() {
    let (reply, success, bytes) = exercise("codex");
    assert!(!success);
    assert_eq!(reply["diagnostics"][0]["code"], "GHCLI029_ADOPTION_REFUSED");
    assert_eq!(
        reply["diagnostics"][0]["path"],
        "/adoption/host_action_required"
    );
    let message = reply["diagnostics"][0]["message"].as_str().unwrap();
    assert!(
        message.contains("graphhelm-jpd")
            && message.contains("graphhelm-development-contracts")
            && message.contains("browser")
    );
    assert_eq!(bytes, b"old\n");
}

#[cfg(not(windows))]
#[test]
fn cli_host_operations_refuse_unsupported_containment_without_changes() {
    for name in ["claude", "codex"] {
        let (reply, success, bytes) = exercise(name);
        assert!(!success);
        assert_eq!(reply["diagnostics"][0]["code"], "GHCLI029_ADOPTION_REFUSED");
        assert_eq!(
            reply["diagnostics"][0]["path"],
            "/adoption/host_containment_unavailable"
        );
        assert_eq!(bytes, b"old\n");
    }
}
