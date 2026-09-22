fn private_state() -> tempfile::TempDir {
    let state = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    state
}
use graphhelm_protocols::adoption::AdoptionReason;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn seal(mut plan: Value) -> Value {
    plan.as_object_mut().unwrap().remove("digest");
    plan["digest"] = json!(format!(
        "sha256:{}",
        hash(&serde_json::to_vec(&plan).unwrap())
    ));
    plan
}
fn plan(before: &[u8], p: &std::path::Path, h: &std::path::Path) -> Value {
    seal(
        json!({"apiVersion":"p50.dev/adoption/v1","kind":"AdoptionPlan","id":"test-plan",
      "spec":{"coverage":"complete","rootBindings":graphhelm_host_adoption::root_bindings(p,h).unwrap(),"scopes":["project"],"packages":[],"hostBoundary":"quiescent",
        "decisions":[{"operationIndex":0,"decision":"replace","protected":false}],
        "operations":[{"root":"project","path":"AGENTS.md","beforeDigest":hash(before),"afterDigest":hash(b"new method\n"),"after":"new method\n"}]}}),
    )
}
#[test]
fn edited_plan_cannot_reuse_approval() {
    let p = tempfile::tempdir().unwrap();
    let h = tempfile::tempdir().unwrap();
    let s = private_state();
    std::fs::write(p.path().join("AGENTS.md"), b"old").unwrap();
    let mut value = plan(b"old", p.path(), h.path());
    let approved = value["digest"].as_str().unwrap().to_owned();
    value["spec"]["operations"][0]["after"] = json!("silently edited");
    assert_eq!(
        graphhelm_host_adoption::apply(p.path(), h.path(), s.path(), &value, &approved)
            .unwrap_err()
            .reason,
        AdoptionReason::ReviewRequired
    );
    assert_eq!(std::fs::read(p.path().join("AGENTS.md")).unwrap(), b"old");
}
#[test]
fn full_preflight_prevents_partial_write_when_second_source_is_stale() {
    let p = tempfile::tempdir().unwrap();
    let h = tempfile::tempdir().unwrap();
    let s = private_state();
    std::fs::write(p.path().join("AGENTS.md"), b"old").unwrap();
    std::fs::write(p.path().join("CLAUDE.md"), b"changed after preview").unwrap();
    let mut value = plan(b"old", p.path(), h.path());
    value["spec"]["operations"].as_array_mut().unwrap().push(json!({"root":"project","path":"CLAUDE.md","beforeDigest":hash(b"previous"),"afterDigest":hash(b"new"),"after":"new"}));
    value["spec"]["decisions"]
        .as_array_mut()
        .unwrap()
        .push(json!({"operationIndex":1,"decision":"replace","protected":false}));
    value = seal(value);
    assert_eq!(
        graphhelm_host_adoption::apply(
            p.path(),
            h.path(),
            s.path(),
            &value,
            value["digest"].as_str().unwrap()
        )
        .unwrap_err()
        .reason,
        AdoptionReason::PlanStale
    );
    assert_eq!(std::fs::read(p.path().join("AGENTS.md")).unwrap(), b"old");
}
#[test]
fn applying_again_returns_the_same_receipt_and_keeps_original_baseline() {
    let p = tempfile::tempdir().unwrap();
    let h = tempfile::tempdir().unwrap();
    let s = private_state();
    std::fs::write(p.path().join("AGENTS.md"), b"old").unwrap();
    let value = plan(b"old", p.path(), h.path());
    let first = graphhelm_host_adoption::apply(
        p.path(),
        h.path(),
        s.path(),
        &value,
        value["digest"].as_str().unwrap(),
    )
    .unwrap();
    let baseline = std::fs::read(s.path().join("original.json")).unwrap();
    let again = graphhelm_host_adoption::apply(
        p.path(),
        h.path(),
        s.path(),
        &value,
        value["digest"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(first, again);
    assert_eq!(
        std::fs::read(s.path().join("original.json")).unwrap(),
        baseline
    );
    assert_eq!(first["spec"]["state"], "installed_unverified");
    let mut next = plan(b"new method\n", p.path(), h.path());
    next["spec"]["operations"][0]["after"] = json!("newer method\n");
    next["spec"]["operations"][0]["afterDigest"] = json!(hash(b"newer method\n"));
    next = seal(next);
    graphhelm_host_adoption::apply(
        p.path(),
        h.path(),
        s.path(),
        &next,
        next["digest"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(
        std::fs::read(s.path().join("original.json")).unwrap(),
        baseline
    );
}
#[test]
fn user_scope_must_be_explicit_and_protected_settings_stay_unchanged() {
    let p = tempfile::tempdir().unwrap();
    let h = tempfile::tempdir().unwrap();
    let s = private_state();
    std::fs::write(h.path().join("AGENTS.md"), b"old").unwrap();
    let mut value = plan(b"old", p.path(), h.path());
    value["spec"]["operations"][0]["root"] = json!("home");
    value = seal(value);
    assert!(
        graphhelm_host_adoption::apply(
            p.path(),
            h.path(),
            s.path(),
            &value,
            value["digest"].as_str().unwrap()
        )
        .is_err()
    );
    assert_eq!(std::fs::read(h.path().join("AGENTS.md")).unwrap(), b"old");
    std::fs::write(p.path().join("AGENTS.md"), b"Never expose secrets\n").unwrap();
    let value = plan(b"Never expose secrets\n", p.path(), h.path());
    assert_eq!(
        graphhelm_host_adoption::apply(
            p.path(),
            h.path(),
            s.path(),
            &value,
            value["digest"].as_str().unwrap()
        )
        .unwrap_err()
        .reason,
        AdoptionReason::ReviewRequired
    );
    assert_eq!(
        std::fs::read(p.path().join("AGENTS.md")).unwrap(),
        b"Never expose secrets\n"
    );
}
#[test]
fn plan_bound_to_another_root_cannot_write_identical_source_bytes() {
    let p = tempfile::tempdir().unwrap();
    let h = tempfile::tempdir().unwrap();
    let s = private_state();
    std::fs::write(p.path().join("AGENTS.md"), b"old").unwrap();
    let mut value = plan(b"old", p.path(), h.path());
    value["spec"]["rootBindings"] = json!({"project":"a".repeat(64),"home":"b".repeat(64)});
    value = seal(value);
    assert_eq!(
        graphhelm_host_adoption::apply(
            p.path(),
            h.path(),
            s.path(),
            &value,
            value["digest"].as_str().unwrap()
        )
        .unwrap_err()
        .reason,
        AdoptionReason::PlanStale
    );
    assert_eq!(std::fs::read(p.path().join("AGENTS.md")).unwrap(), b"old");
}

#[cfg(windows)]
#[test]
fn state_cannot_hide_inside_project_through_path_casing() {
    let p = tempfile::tempdir().unwrap();
    let h = tempfile::tempdir().unwrap();
    std::fs::write(p.path().join("AGENTS.md"), b"old").unwrap();
    let value = plan(b"old", p.path(), h.path());
    let state =
        std::path::PathBuf::from(p.path().to_string_lossy().to_uppercase()).join("private-state");
    assert_eq!(
        graphhelm_host_adoption::apply(
            p.path(),
            h.path(),
            &state,
            &value,
            value["digest"].as_str().unwrap()
        )
        .unwrap_err()
        .reason,
        AdoptionReason::PathUnsafe
    );
    assert_eq!(std::fs::read(p.path().join("AGENTS.md")).unwrap(), b"old");
    assert!(!state.exists());
}
