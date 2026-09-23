use graphhelm_host_adoption::{apply, apply_restore, backup, plan_restore, root_bindings};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[cfg(windows)]
fn sddl(path: &std::path::Path) -> String {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Security::Authorization::ConvertSecurityDescriptorToStringSecurityDescriptorW;
    use windows_sys::Win32::Security::{
        DACL_SECURITY_INFORMATION, GROUP_SECURITY_INFORMATION, GetKernelObjectSecurity,
        OWNER_SECURITY_INFORMATION,
    };

    let file = std::fs::File::open(path).unwrap();
    let information =
        OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
    let mut length = 0u32;
    unsafe {
        GetKernelObjectSecurity(
            file.as_raw_handle(),
            information,
            std::ptr::null_mut(),
            0,
            &mut length,
        );
    }
    assert!(length > 0 && length <= 64 * 1024);
    let mut descriptor = vec![0u32; (length as usize).div_ceil(4)];
    let mut needed = length;
    assert_ne!(
        unsafe {
            GetKernelObjectSecurity(
                file.as_raw_handle(),
                information,
                descriptor.as_mut_ptr().cast(),
                length,
                &mut needed,
            )
        },
        0
    );
    let mut text = std::ptr::null_mut();
    let mut text_length = 0u32;
    assert_ne!(
        unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                descriptor.as_mut_ptr().cast(),
                1,
                information,
                &mut text,
                &mut text_length,
            )
        },
        0
    );
    let result = String::from_utf16(unsafe {
        std::slice::from_raw_parts(text, text_length.saturating_sub(1) as usize)
    })
    .unwrap();
    unsafe { windows_sys::Win32::Foundation::LocalFree(text.cast()) };
    // Production access metadata deliberately drops the historical auto-inherited marker.
    result.trim_end_matches('\0').replace("D:AI", "D:")
}

#[cfg(windows)]
fn mutate_acl(path: &std::path::Path) {
    use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
    use windows_sys::Win32::Security::{
        DACL_SECURITY_INFORMATION, GetKernelObjectSecurity, SE_DACL_PROTECTED,
        SetKernelObjectSecurity, SetSecurityDescriptorControl,
    };
    use windows_sys::Win32::Storage::FileSystem::{READ_CONTROL, WRITE_DAC};

    let file = std::fs::OpenOptions::new()
        .access_mode(READ_CONTROL | WRITE_DAC)
        .open(path)
        .unwrap();
    let mut length = 0u32;
    unsafe {
        GetKernelObjectSecurity(
            file.as_raw_handle(),
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            0,
            &mut length,
        );
    }
    assert!(length > 0 && length <= 64 * 1024);
    let mut descriptor = vec![0u32; (length as usize).div_ceil(4)];
    let mut needed = length;
    assert_ne!(
        unsafe {
            GetKernelObjectSecurity(
                file.as_raw_handle(),
                DACL_SECURITY_INFORMATION,
                descriptor.as_mut_ptr().cast(),
                length,
                &mut needed,
            )
        },
        0
    );
    assert_ne!(
        unsafe {
            SetSecurityDescriptorControl(
                descriptor.as_mut_ptr().cast(),
                SE_DACL_PROTECTED,
                SE_DACL_PROTECTED,
            )
        },
        0
    );
    assert_ne!(
        unsafe {
            SetKernelObjectSecurity(
                file.as_raw_handle(),
                DACL_SECURITY_INFORMATION,
                descriptor.as_mut_ptr().cast(),
            )
        },
        0
    );
}
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
fn a_direct_manual_checkpoint_restores_bytes_and_access_before_adoption() {
    let f = Fixture::new();
    let path = f.p.path().join("AGENTS.md");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
    }
    let checkpoint = backup(f.p.path(), f.h.path(), f.s.path()).unwrap();
    #[cfg(windows)]
    let original_sddl = sddl(&path);
    std::fs::write(&path, b"later user bytes\n").unwrap();
    #[cfg(windows)]
    {
        mutate_acl(&path);
        assert_ne!(sddl(&path), original_sddl);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let plan = plan_restore(f.s.path(), checkpoint["id"].as_str().unwrap()).unwrap();
    assert_eq!(plan["spec"]["selection"], "checkpoint");
    assert_eq!(
        apply_restore(f.s.path(), &plan, plan["digest"].as_str().unwrap()).unwrap()["spec"]["state"],
        "restored"
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"\xef\xbb\xbfOriginal\r\n");
    #[cfg(windows)]
    assert_eq!(sddl(&path), original_sddl);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }
}

#[test]
#[cfg(windows)]
fn manual_checkpoint_restores_windows_acl_when_only_metadata_changed() {
    let f = Fixture::new();
    let path = f.p.path().join("AGENTS.md");
    let checkpoint = backup(f.p.path(), f.h.path(), f.s.path()).unwrap();
    let original_sddl = sddl(&path);
    mutate_acl(&path);
    assert_ne!(sddl(&path), original_sddl);
    let plan = plan_restore(f.s.path(), checkpoint["id"].as_str().unwrap()).unwrap();
    assert_eq!(
        apply_restore(f.s.path(), &plan, plan["digest"].as_str().unwrap()).unwrap()["spec"]["state"],
        "restored"
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"\xef\xbb\xbfOriginal\r\n");
    assert_eq!(sddl(&path), original_sddl);
}

#[test]
#[cfg(unix)]
fn manual_checkpoint_restores_access_when_only_metadata_changed() {
    let f = Fixture::new();
    let path = f.p.path().join("AGENTS.md");
    let checkpoint = backup(f.p.path(), f.h.path(), f.s.path()).unwrap();
    let original_readonly = std::fs::metadata(&path).unwrap().permissions().readonly();
    let mut changed = std::fs::metadata(&path).unwrap().permissions();
    changed.set_readonly(!original_readonly);
    std::fs::set_permissions(&path, changed).unwrap();
    assert_ne!(
        std::fs::metadata(&path).unwrap().permissions().readonly(),
        original_readonly
    );
    let plan = plan_restore(f.s.path(), checkpoint["id"].as_str().unwrap()).unwrap();
    apply_restore(f.s.path(), &plan, plan["digest"].as_str().unwrap()).unwrap();
    assert_eq!(
        std::fs::metadata(path).unwrap().permissions().readonly(),
        original_readonly
    );
}

#[test]
fn a_foreign_replacement_of_a_checkpoint_root_refuses_before_writes() {
    let f = Fixture::new();
    let checkpoint = backup(f.p.path(), f.h.path(), f.s.path()).unwrap();
    let original = f.p.path().to_path_buf();
    let moved = original.with_extension("moved");
    std::fs::rename(&original, &moved).unwrap();
    std::fs::create_dir(&original).unwrap();
    std::fs::write(original.join("AGENTS.md"), b"foreign root\n").unwrap();
    let error = plan_restore(f.s.path(), checkpoint["id"].as_str().unwrap()).unwrap_err();
    assert!(matches!(
        error.reason,
        graphhelm_protocols::adoption::AdoptionReason::PathUnsafe
            | graphhelm_protocols::adoption::AdoptionReason::PlanStale
    ));
    assert_eq!(
        std::fs::read(original.join("AGENTS.md")).unwrap(),
        b"foreign root\n"
    );
}

#[test]
fn manual_restore_rejects_edit_after_preview_without_writing() {
    let f = Fixture::new();
    let path = f.p.path().join("AGENTS.md");
    let checkpoint = backup(f.p.path(), f.h.path(), f.s.path()).unwrap();
    let plan = plan_restore(f.s.path(), checkpoint["id"].as_str().unwrap()).unwrap();
    std::fs::write(&path, b"edited after preview\n").unwrap();
    let error = apply_restore(f.s.path(), &plan, plan["digest"].as_str().unwrap()).unwrap_err();
    assert_eq!(
        error.reason,
        graphhelm_protocols::adoption::AdoptionReason::PlanStale
    );
    assert_eq!(std::fs::read(path).unwrap(), b"edited after preview\n");
}

#[test]
fn manual_checkpoint_after_adoption_keeps_original_rollback_available() {
    let f = Fixture::new();
    f.install("Installed\n");
    let checkpoint = backup(f.p.path(), f.h.path(), f.s.path()).unwrap();
    std::fs::write(f.p.path().join("AGENTS.md"), b"later\n").unwrap();
    let plan = plan_restore(f.s.path(), checkpoint["id"].as_str().unwrap()).unwrap();
    assert_eq!(plan["spec"]["manual"], true);
    assert_eq!(
        apply_restore(f.s.path(), &plan, plan["digest"].as_str().unwrap()).unwrap()["spec"]["state"],
        "restored"
    );
    assert_eq!(
        std::fs::read(f.p.path().join("AGENTS.md")).unwrap(),
        b"Installed\n"
    );
    assert_eq!(f.restore()["spec"]["state"], "restored");
    assert_eq!(
        std::fs::read(f.p.path().join("AGENTS.md")).unwrap(),
        b"\xef\xbb\xbfOriginal\r\n"
    );
}

#[test]
fn a_legacy_integrity_only_snapshot_is_not_restore_eligible() {
    let f = Fixture::new();
    let checkpoint = backup(f.p.path(), f.h.path(), f.s.path()).unwrap();
    let old_id = checkpoint["id"].as_str().unwrap().to_owned();
    let old_dir = f.s.path().join("backups").join(&old_id);
    let manifest_path = old_dir.join("manifest.json");
    let mut manifest: Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest.as_object_mut().unwrap().remove("provenance");
    let bytes = serde_json::to_vec(&manifest).unwrap();
    let new_id = hash(&bytes);
    std::fs::write(&manifest_path, &bytes).unwrap();
    std::fs::rename(old_dir, f.s.path().join("backups").join(&new_id)).unwrap();
    assert!(graphhelm_host_adoption::verify_backup(f.s.path(), &new_id).is_ok());
    let error = plan_restore(f.s.path(), &new_id).unwrap_err();
    assert_eq!(
        error.reason,
        graphhelm_protocols::adoption::AdoptionReason::BackupUnverified
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
fn checkpoint_rejects_a_surface_access_digest_pair_mismatch() {
    let f = Fixture::new();
    let checkpoint = backup(f.p.path(), f.h.path(), f.s.path()).unwrap();
    let old_id = checkpoint["id"].as_str().unwrap().to_owned();
    let old_dir = f.s.path().join("backups").join(&old_id);
    let manifest_path = old_dir.join("manifest.json");
    let mut manifest: Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["surfaceMetadata"]["project/AGENTS.md"]["accessDigest"] = json!("0".repeat(64));
    let bytes = serde_json::to_vec(&manifest).unwrap();
    let new_id = hash(&bytes);
    std::fs::write(&manifest_path, bytes).unwrap();
    std::fs::rename(old_dir, f.s.path().join("backups").join(&new_id)).unwrap();

    let error = plan_restore(f.s.path(), &new_id).unwrap_err();

    assert_eq!(
        error.reason,
        graphhelm_protocols::adoption::AdoptionReason::BackupCorrupt
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
fn unchanged_foreign_owned_home_surface_does_not_block_project_restore() {
    let first = Fixture::new();
    std::fs::write(first.h.path().join("AGENTS.md"), b"Shared original\n").unwrap();
    let checkpoint = backup(first.p.path(), first.h.path(), first.s.path()).unwrap();
    let other = Fixture::new();
    let adoption = seal(
        json!({"apiVersion":"p50.dev/adoption/v1","kind":"AdoptionPlan","id":"same-bytes-owner","spec":{"coverage":"complete","rootBindings":root_bindings(other.p.path(),first.h.path()).unwrap(),"scopes":["user"],"packages":[],"hostBoundary":"quiescent","decisions":[{"operationIndex":0,"decision":"replace","protected":false}],"operations":[{"root":"home","path":"AGENTS.md","beforeDigest":hash(b"Shared original\n"),"afterDigest":hash(b"Shared original\n"),"after":"Shared original\n"}]}}),
    );
    apply(
        other.p.path(),
        first.h.path(),
        other.s.path(),
        &adoption,
        adoption["digest"].as_str().unwrap(),
    )
    .unwrap();
    first.install("Later project bytes\n");
    let restore = plan_restore(first.s.path(), checkpoint["id"].as_str().unwrap()).unwrap();
    assert_eq!(
        restore["spec"]["effects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|effect| effect["root"] == "home" && effect["path"] == "AGENTS.md")
            .unwrap()["action"],
        "unchanged"
    );
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
        std::fs::read(first.p.path().join("AGENTS.md")).unwrap(),
        b"\xef\xbb\xbfOriginal\r\n"
    );
    assert_eq!(
        std::fs::read(first.h.path().join("AGENTS.md")).unwrap(),
        b"Shared original\n"
    );
}

#[test]
fn same_edit_checkpoint_cycles_bind_history_and_retries_are_idempotent() {
    let f = Fixture::new();
    let mut bindings = Vec::new();
    for _ in 0..3 {
        let checkpoint = backup(f.p.path(), f.h.path(), f.s.path()).unwrap();
        std::fs::write(f.p.path().join("AGENTS.md"), b"same edit\n").unwrap();
        let plan = plan_restore(f.s.path(), checkpoint["id"].as_str().unwrap()).unwrap();
        bindings.push(plan["spec"]["journalBinding"].clone());
        let accepted = plan["digest"].as_str().unwrap();
        assert_eq!(
            apply_restore(f.s.path(), &plan, accepted).unwrap()["spec"]["state"],
            "restored"
        );
        assert_eq!(
            apply_restore(f.s.path(), &plan, accepted).unwrap()["spec"]["state"],
            "restored"
        );
        assert_eq!(
            std::fs::read(f.p.path().join("AGENTS.md")).unwrap(),
            b"\xef\xbb\xbfOriginal\r\n"
        );
    }
    assert!(bindings.windows(2).all(|pair| pair[0] != pair[1]));
}

#[test]
fn unchanged_manual_checkpoint_persists_a_durable_history_marker() {
    let f = Fixture::new();
    let checkpoint = backup(f.p.path(), f.h.path(), f.s.path()).unwrap();
    let first = plan_restore(f.s.path(), checkpoint["id"].as_str().unwrap()).unwrap();
    assert_eq!(first["spec"]["effects"][0]["action"], "unchanged");
    assert_eq!(
        apply_restore(f.s.path(), &first, first["digest"].as_str().unwrap()).unwrap()["spec"]["state"],
        "restored"
    );
    let second = plan_restore(f.s.path(), checkpoint["id"].as_str().unwrap()).unwrap();
    assert_ne!(
        first["spec"]["journalBinding"],
        second["spec"]["journalBinding"]
    );
    assert_eq!(
        apply_restore(f.s.path(), &second, second["digest"].as_str().unwrap()).unwrap()["spec"]["state"],
        "restored"
    );
}

#[test]
fn manual_preview_refuses_after_durable_history_changes() {
    let f = Fixture::new();
    let checkpoint = backup(f.p.path(), f.h.path(), f.s.path()).unwrap();
    std::fs::write(f.p.path().join("AGENTS.md"), b"first edit\n").unwrap();
    let stale = plan_restore(f.s.path(), checkpoint["id"].as_str().unwrap()).unwrap();

    let next = backup(f.p.path(), f.h.path(), f.s.path()).unwrap();
    std::fs::write(f.p.path().join("AGENTS.md"), b"second edit\n").unwrap();
    let current = plan_restore(f.s.path(), next["id"].as_str().unwrap()).unwrap();
    apply_restore(f.s.path(), &current, current["digest"].as_str().unwrap()).unwrap();

    let error = apply_restore(f.s.path(), &stale, stale["digest"].as_str().unwrap()).unwrap_err();
    assert_eq!(
        error.reason,
        graphhelm_protocols::adoption::AdoptionReason::PlanStale
    );
    assert_eq!(
        std::fs::read(f.p.path().join("AGENTS.md")).unwrap(),
        b"first edit\n"
    );
}

#[test]
fn legacy_manual_plan_requires_a_fresh_preview_with_journal_binding() {
    let f = Fixture::new();
    let checkpoint = backup(f.p.path(), f.h.path(), f.s.path()).unwrap();
    std::fs::write(f.p.path().join("AGENTS.md"), b"later\n").unwrap();
    let mut plan = plan_restore(f.s.path(), checkpoint["id"].as_str().unwrap()).unwrap();
    plan["spec"]
        .as_object_mut()
        .unwrap()
        .remove("journalBinding");
    let legacy = seal(plan);
    let error = apply_restore(f.s.path(), &legacy, legacy["digest"].as_str().unwrap()).unwrap_err();
    assert_eq!(
        error.reason,
        graphhelm_protocols::adoption::AdoptionReason::PlanStale
    );
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
