use serde_json::{Value, json};
use sha2::{Digest, Sha256};
fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
#[test]
fn cli_previews_then_restores_only_the_exact_accepted_offline_plan() {
    let p = tempfile::tempdir().unwrap();
    let h = tempfile::tempdir().unwrap();
    let s = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(s.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    std::fs::write(p.path().join("AGENTS.md"), b"original\r\n").unwrap();
    let mut setup = json!({"apiVersion":"p50.dev/adoption/v1","kind":"AdoptionPlan","id":"cli-restore","spec":{"coverage":"complete","rootBindings":graphhelm_host_adoption::root_bindings(p.path(),h.path()).unwrap(),"scopes":["project"],"packages":[],"hostBoundary":"quiescent","decisions":[{"operationIndex":0,"decision":"replace","protected":false}],"operations":[{"root":"project","path":"AGENTS.md","beforeDigest":digest(b"original\r\n"),"afterDigest":digest(b"installed\n"),"after":"installed\n"}]}});
    setup["digest"] = json!(format!(
        "sha256:{}",
        digest(&serde_json::to_vec(&setup).unwrap())
    ));
    graphhelm_host_adoption::apply(
        p.path(),
        h.path(),
        s.path(),
        &setup,
        setup["digest"].as_str().unwrap(),
    )
    .unwrap();
    let before = std::fs::read_dir(s.path()).unwrap().count();
    let preview = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .arg("restore")
        .arg("--state-root")
        .arg(s.path())
        .output()
        .unwrap();
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    assert_eq!(std::fs::read_dir(s.path()).unwrap().count(), before);
    let preview: Value = serde_json::from_slice(&preview.stdout).unwrap();
    let plan = &preview["data"]["plan"];
    assert_eq!(plan["kind"], "RestorePlan");
    let planfile = s.path().join("reviewed.json");
    std::fs::write(&planfile, serde_json::to_vec(plan).unwrap()).unwrap();
    let mut run = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
    run.arg("restore")
        .arg("--state-root")
        .arg(s.path())
        .arg("--apply")
        .arg(&planfile)
        .arg("--accept")
        .arg("wrong");
    assert!(!run.output().unwrap().status.success());
    assert_eq!(
        std::fs::read(p.path().join("AGENTS.md")).unwrap(),
        b"installed\n"
    );
    let applied = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .arg("restore")
        .arg("--state-root")
        .arg(s.path())
        .arg("--apply")
        .arg(&planfile)
        .arg("--accept")
        .arg(plan["digest"].as_str().unwrap())
        .output()
        .unwrap();
    assert!(
        applied.status.success(),
        "{}",
        String::from_utf8_lossy(&applied.stdout)
    );
    let applied: Value = serde_json::from_slice(&applied.stdout).unwrap();
    assert_eq!(applied["data"]["receipt"]["spec"]["state"], "restored");
    assert_eq!(
        std::fs::read(p.path().join("AGENTS.md")).unwrap(),
        b"original\r\n"
    );
}
