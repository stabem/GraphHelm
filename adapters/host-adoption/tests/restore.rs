use graphhelm_host_adoption::{apply, apply_restore, backup, plan_restore, root_bindings};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn seal(mut value: Value) -> Value {
    value.as_object_mut().unwrap().remove("digest");
    value["digest"] = json!(format!(
        "sha256:{}",
        hash(&serde_json::to_vec(&value).unwrap())
    ));
    value
}
struct Fixture {
    p: tempfile::TempDir,
    h: tempfile::TempDir,
    s: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let f = Self {
            p: tempfile::tempdir().unwrap(),
            h: tempfile::tempdir().unwrap(),
            s: tempfile::tempdir().unwrap(),
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(f.s.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        std::fs::write(f.p.path().join("AGENTS.md"), b"\xef\xbb\xbfOriginal\r\n").unwrap();
        f
    }
    fn install(&self, after: &str) -> Value {
        let before = std::fs::read(self.p.path().join("AGENTS.md")).unwrap();
        let plan = seal(
            json!({"apiVersion":"p50.dev/adoption/v1","kind":"AdoptionPlan","id":"restore-test","spec":{"coverage":"complete","rootBindings":root_bindings(self.p.path(),self.h.path()).unwrap(),"scopes":["project"],"packages":[],"hostBoundary":"quiescent","decisions":[{"operationIndex":0,"decision":"replace","protected":false}],"operations":[{"root":"project","path":"AGENTS.md","beforeDigest":hash(&before),"afterDigest":hash(after.as_bytes()),"after":after}]}}),
        );
        apply(
            self.p.path(),
            self.h.path(),
            self.s.path(),
            &plan,
            plan["digest"].as_str().unwrap(),
        )
        .unwrap()
    }
    fn restore(&self) -> Value {
        let plan = plan_restore(self.s.path(), "original").unwrap();
        apply_restore(self.s.path(), &plan, plan["digest"].as_str().unwrap()).unwrap()
    }
}

#[test]
fn restore_exact_bytes_and_access_without_host_or_runtime() {
    let f = Fixture::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            f.p.path().join("AGENTS.md"),
            std::fs::Permissions::from_mode(0o640),
        )
        .unwrap();
    }
    f.install("Installed\n");
    let receipt = f.restore();
    assert_eq!(receipt["spec"]["state"], "restored");
    assert_eq!(
        std::fs::read(f.p.path().join("AGENTS.md")).unwrap(),
        b"\xef\xbb\xbfOriginal\r\n"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(f.p.path().join("AGENTS.md"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o640
        );
    }
}
#[test]
fn original_survives_upgrade_and_manual_checkpoint() {
    let f = Fixture::new();
    f.install("First\n");
    let original = std::fs::read(f.s.path().join("original.json")).unwrap();
    let checkpoint = backup(f.p.path(), f.h.path(), f.s.path()).unwrap();
    f.install("Second\n");
    assert_eq!(
        std::fs::read(f.s.path().join("original.json")).unwrap(),
        original
    );
    let plan = plan_restore(f.s.path(), checkpoint["id"].as_str().unwrap()).unwrap();
    assert_eq!(plan["spec"]["selection"], "checkpoint");
    assert_eq!(f.restore()["spec"]["state"], "restored");
    assert_eq!(
        std::fs::read(f.p.path().join("AGENTS.md")).unwrap(),
        b"\xef\xbb\xbfOriginal\r\n"
    );
}
#[test]
fn prose_overlap_is_a_conflict_and_preserves_later_bytes() {
    let f = Fixture::new();
    f.install("Installed\n");
    std::fs::write(f.p.path().join("AGENTS.md"), b"Later choice\n").unwrap();
    let plan = plan_restore(f.s.path(), "original").unwrap();
    assert!(!plan["spec"]["conflicts"].as_array().unwrap().is_empty());
    let result = apply_restore(f.s.path(), &plan, plan["digest"].as_str().unwrap()).unwrap();
    assert_eq!(result["spec"]["state"], "recovery_required");
    assert_eq!(
        std::fs::read(f.p.path().join("AGENTS.md")).unwrap(),
        b"Later choice\n"
    );
}
#[test]
fn exact_approval_and_current_snapshot_are_required() {
    let f = Fixture::new();
    f.install("Installed\n");
    let plan = plan_restore(f.s.path(), "original").unwrap();
    assert!(apply_restore(f.s.path(), &plan, "wrong").is_err());
    let mut changed = plan.clone();
    changed["spec"]["backupId"] = json!("a".repeat(64));
    assert!(apply_restore(f.s.path(), &changed, plan["digest"].as_str().unwrap()).is_err());
    std::fs::write(f.p.path().join("AGENTS.md"), b"Drift\n").unwrap();
    assert!(apply_restore(f.s.path(), &plan, plan["digest"].as_str().unwrap()).is_err());
    assert_eq!(
        std::fs::read(f.p.path().join("AGENTS.md")).unwrap(),
        b"Drift\n"
    );
}
#[test]
fn missing_or_corrupt_backup_refuses_before_writes() {
    let f = Fixture::new();
    let applied = f.install("Installed\n");
    assert!(plan_restore(f.s.path(), &"a".repeat(64)).is_err());
    let id = applied["spec"]["backupId"].as_str().unwrap();
    std::fs::write(
        f.s.path().join("backups").join(id).join("blob-0"),
        b"corrupt",
    )
    .unwrap();
    assert!(plan_restore(f.s.path(), "original").is_err());
    assert_eq!(
        std::fs::read(f.p.path().join("AGENTS.md")).unwrap(),
        b"Installed\n"
    );
}
#[test]
fn originally_absent_unowned_files_are_retained_and_reported() {
    let f = Fixture::new();
    f.install("Installed\n");
    assert!(!f.p.path().join("CLAUDE.md").exists());
    std::fs::write(f.p.path().join("CLAUDE.md"), b"User created this\n").unwrap();
    let plan = plan_restore(f.s.path(), "original").unwrap();
    assert!(
        plan["spec"]["effects"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["path"] == "CLAUDE.md" && e["action"] == "retain_unowned")
    );
    assert_eq!(f.restore()["spec"]["state"], "restored");
    assert_eq!(
        std::fs::read(f.p.path().join("CLAUDE.md")).unwrap(),
        b"User created this\n"
    );
}

#[test]
fn two_projects_cannot_claim_the_same_user_surface() {
    let f = Fixture::new();
    std::fs::write(f.h.path().join("AGENTS.md"), b"Shared original\n").unwrap();
    let make = |project: &std::path::Path, before: &[u8], after: &str| {
        seal(
            json!({"apiVersion":"p50.dev/adoption/v1","kind":"AdoptionPlan","id":"shared","spec":{"coverage":"complete","rootBindings":root_bindings(project,f.h.path()).unwrap(),"scopes":["user"],"packages":[],"hostBoundary":"quiescent","decisions":[{"operationIndex":0,"decision":"replace","protected":false}],"operations":[{"root":"home","path":"AGENTS.md","beforeDigest":hash(before),"afterDigest":hash(after.as_bytes()),"after":after}]}}),
        )
    };
    let first = make(f.p.path(), b"Shared original\n", "First owner\n");
    apply(
        f.p.path(),
        f.h.path(),
        f.s.path(),
        &first,
        first["digest"].as_str().unwrap(),
    )
    .unwrap();
    let other = Fixture::new();
    let second = make(other.p.path(), b"First owner\n", "Second owner\n");
    assert!(
        apply(
            other.p.path(),
            f.h.path(),
            other.s.path(),
            &second,
            second["digest"].as_str().unwrap()
        )
        .is_err()
    );
    assert_eq!(
        std::fs::read(f.h.path().join("AGENTS.md")).unwrap(),
        b"First owner\n"
    );
    assert_eq!(std::fs::read_dir(other.s.path()).unwrap().count(), 0);
    assert_eq!(f.restore()["spec"]["state"], "restored");
    let second = make(other.p.path(), b"Shared original\n", "Second owner\n");
    apply(
        other.p.path(),
        f.h.path(),
        other.s.path(),
        &second,
        second["digest"].as_str().unwrap(),
    )
    .unwrap();
}

#[test]
fn checkpoint_restore_keeps_the_predecessor_user_owner_claimed() {
    let first = Fixture::new();
    std::fs::write(first.h.path().join("AGENTS.md"), b"Shared original\n").unwrap();
    let plan = |project: &std::path::Path, before: &[u8], after: &str| {
        seal(
            json!({"apiVersion":"p50.dev/adoption/v1","kind":"AdoptionPlan","id":"shared-checkpoint","spec":{"coverage":"complete","rootBindings":root_bindings(project,first.h.path()).unwrap(),"scopes":["user"],"packages":[],"hostBoundary":"quiescent","decisions":[{"operationIndex":0,"decision":"replace","protected":false}],"operations":[{"root":"home","path":"AGENTS.md","beforeDigest":hash(before),"afterDigest":hash(after.as_bytes()),"after":after}]}}),
        )
    };
    let adoption_a = plan(first.p.path(), b"Shared original\n", "First owner\n");
    apply(
        first.p.path(),
        first.h.path(),
        first.s.path(),
        &adoption_a,
        adoption_a["digest"].as_str().unwrap(),
    )
    .unwrap();
    let checkpoint = backup(first.p.path(), first.h.path(), first.s.path()).unwrap();
    let adoption_b = plan(first.p.path(), b"First owner\n", "Second owner\n");
    apply(
        first.p.path(),
        first.h.path(),
        first.s.path(),
        &adoption_b,
        adoption_b["digest"].as_str().unwrap(),
    )
    .unwrap();
    let restore = plan_restore(first.s.path(), checkpoint["id"].as_str().unwrap()).unwrap();
    assert_eq!(
        apply_restore(
            first.s.path(),
            &restore,
            restore["digest"].as_str().unwrap(),
        )
        .unwrap()["spec"]["state"],
        "restored"
    );
    assert_eq!(
        std::fs::read(first.h.path().join("AGENTS.md")).unwrap(),
        b"First owner\n"
    );

    let other = Fixture::new();
    let overlapping = plan(other.p.path(), b"First owner\n", "Other project\n");
    assert!(
        apply(
            other.p.path(),
            first.h.path(),
            other.s.path(),
            &overlapping,
            overlapping["digest"].as_str().unwrap(),
        )
        .is_err()
    );
    assert_eq!(std::fs::read_dir(other.s.path()).unwrap().count(), 0);
}

#[test]
fn checkpoint_then_original_restores_both_reviewed_baselines() {
    let f = Fixture::new();
    f.install("First\n");
    let checkpoint = backup(f.p.path(), f.h.path(), f.s.path()).unwrap();
    f.install("Second\n");
    let plan = plan_restore(f.s.path(), checkpoint["id"].as_str().unwrap()).unwrap();
    assert_eq!(
        apply_restore(f.s.path(), &plan, plan["digest"].as_str().unwrap()).unwrap()["spec"]["state"],
        "restored"
    );
    assert_eq!(
        std::fs::read(f.p.path().join("AGENTS.md")).unwrap(),
        b"First\n"
    );
    assert_eq!(f.restore()["spec"]["state"], "restored");
    assert_eq!(
        std::fs::read(f.p.path().join("AGENTS.md")).unwrap(),
        b"\xef\xbb\xbfOriginal\r\n"
    );
}
