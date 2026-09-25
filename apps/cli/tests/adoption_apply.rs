use serde_json::{Value, json};
use sha2::{Digest, Sha256};
fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
#[test]
fn cli_applies_only_the_accepted_plan_and_reports_unverified() {
    let p = tempfile::tempdir().unwrap();
    let h = tempfile::tempdir().unwrap();
    let s = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(s.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let planfile = s.path().join("plan.json");
    std::fs::write(p.path().join("AGENTS.md"), b"old method\n").unwrap();
    let mut plan = json!({"apiVersion":"p50.dev/adoption/v1","kind":"AdoptionPlan","id":"cli-plan","spec":{"coverage":"complete","rootBindings":graphhelm_host_adoption::root_bindings(p.path(),h.path()).unwrap(),"scopes":["project"],"packages":[],"hostBoundary":"quiescent","decisions":[{"operationIndex":0,"decision":"replace","protected":false}],"operations":[{"root":"project","path":"AGENTS.md","beforeDigest":digest(b"old method\n"),"afterDigest":digest(b"new method\n"),"after":"new method\n"}]}});
    let accepted = format!("sha256:{}", digest(&serde_json::to_vec(&plan).unwrap()));
    plan["digest"] = json!(accepted);
    std::fs::write(&planfile, serde_json::to_vec(&plan).unwrap()).unwrap();
    let output = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .arg("setup")
        .arg("--project")
        .arg(p.path())
        .arg("--home")
        .arg(h.path())
        .arg("--state-root")
        .arg(s.path())
        .arg("--apply")
        .arg(&planfile)
        .arg("--accept")
        .arg(&accepted)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        value["data"]["receipt"]["spec"]["state"],
        "installed_unverified"
    );
    assert_eq!(
        std::fs::read(p.path().join("AGENTS.md")).unwrap(),
        b"new method\n"
    );
    assert!(!p.path().join(".graphhelm").exists());
    let recovered = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .arg("setup")
        .arg("--project")
        .arg(p.path())
        .arg("--home")
        .arg(h.path())
        .arg("--state-root")
        .arg(s.path())
        .arg("--recover")
        .arg(
            value["data"]["receipt"]["spec"]["transactionId"]
                .as_str()
                .unwrap(),
        )
        .output()
        .unwrap();
    assert!(recovered.status.success());
}
#[test]
fn setup_preview_describes_provisioning_without_creating_it() {
    let p = tempfile::tempdir().unwrap();
    let h = tempfile::tempdir().unwrap();
    let output = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .arg("setup")
        .arg("--project")
        .arg(p.path())
        .arg("--home")
        .arg(h.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["data"]["provisioning"]["root"], ".graphhelm");
    assert_eq!(
        value["data"]["provisioning"]["writes"][0]["path"],
        ".graphhelm/events"
    );
    assert_eq!(std::fs::read_dir(p.path()).unwrap().count(), 0);
}
