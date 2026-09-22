#[test]
fn manual_backup_is_verified_without_changing_the_source() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let path = project.path().join("AGENTS.md");
    let bytes = b"\xef\xbb\xbfKeep this exact file.\r\n";
    std::fs::write(&path, bytes).unwrap();

    let receipt =
        graphhelm_host_adoption::backup(project.path(), home.path(), state.path()).unwrap();
    let id = receipt["id"].as_str().unwrap();
    let checked = graphhelm_host_adoption::verify_backup(state.path(), id).unwrap();

    assert_eq!(checked["spec"]["verified"], true);
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
fn backup_id_is_never_a_path() {
    assert!(!graphhelm_host_adoption::valid_backup_id("../../.ssh"));
    assert!(!graphhelm_host_adoption::valid_backup_id("C:\\outside"));
    assert!(graphhelm_host_adoption::valid_backup_id(&"a".repeat(64)));
}

#[test]
fn corrupt_blob_is_reported_as_backup_corrupt() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("AGENTS.md"), b"original").unwrap();
    let receipt =
        graphhelm_host_adoption::backup(project.path(), home.path(), state.path()).unwrap();
    let id = receipt["id"].as_str().unwrap();
    std::fs::write(
        state.path().join("backups").join(id).join("blob-0"),
        b"corrupt",
    )
    .unwrap();

    let error = graphhelm_host_adoption::verify_backup(state.path(), id).unwrap_err();

    assert_eq!(
        error.reason,
        graphhelm_protocols::adoption::AdoptionReason::BackupCorrupt
    );
}

#[test]
fn a_second_distinct_backup_reuses_the_private_backup_directory() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let source = project.path().join("AGENTS.md");
    std::fs::write(&source, b"first").unwrap();
    let first = graphhelm_host_adoption::backup(project.path(), home.path(), state.path()).unwrap();

    std::fs::write(&source, b"second").unwrap();
    let second =
        graphhelm_host_adoption::backup(project.path(), home.path(), state.path()).unwrap();

    assert_ne!(first["id"], second["id"]);
    assert_eq!(second["spec"]["verified"], true);
}

#[test]
fn unsupported_manifest_version_is_rejected() {
    use sha2::{Digest, Sha256};

    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("AGENTS.md"), b"original").unwrap();
    let receipt =
        graphhelm_host_adoption::backup(project.path(), home.path(), state.path()).unwrap();
    let old_id = receipt["id"].as_str().unwrap();
    let old_dir = state.path().join("backups").join(old_id);
    let manifest_path = old_dir.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["version"] = serde_json::json!(2);
    let bytes = serde_json::to_vec(&manifest).unwrap();
    let new_id = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    std::fs::write(&manifest_path, bytes).unwrap();
    std::fs::rename(&old_dir, state.path().join("backups").join(&new_id)).unwrap();

    let error = graphhelm_host_adoption::verify_backup(state.path(), &new_id).unwrap_err();

    assert_eq!(
        error.reason,
        graphhelm_protocols::adoption::AdoptionReason::BackupCorrupt
    );
}
#[test]
fn backup_stops_before_writing_when_total_limit_is_exceeded() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("AGENTS.md"), b"12345").unwrap();

    let error =
        graphhelm_host_adoption::backup_with_limit(project.path(), home.path(), state.path(), 4)
            .unwrap_err();

    assert_eq!(
        error.reason,
        graphhelm_protocols::adoption::AdoptionReason::LimitExceeded
    );
    assert!(!state.path().join("backups").exists());
}

#[test]
fn backup_refuses_a_symlinked_state_root() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let project = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let holder = tempfile::tempdir().unwrap();
        let state = holder.path().join("state");
        symlink(outside.path(), &state).unwrap();

        let error =
            graphhelm_host_adoption::backup(project.path(), home.path(), &state).unwrap_err();

        assert_eq!(
            error.reason,
            graphhelm_protocols::adoption::AdoptionReason::PathUnsafe
        );
    }
}

#[cfg(unix)]
#[test]
fn backup_refuses_a_symlinked_destination_ancestor() {
    use std::os::unix::fs::symlink;
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), state.path().join("backups")).unwrap();
    std::fs::write(project.path().join("AGENTS.md"), b"private").unwrap();

    let error =
        graphhelm_host_adoption::backup(project.path(), home.path(), state.path()).unwrap_err();

    assert_eq!(
        error.reason,
        graphhelm_protocols::adoption::AdoptionReason::PathUnsafe
    );
    assert!(outside.path().read_dir().unwrap().next().is_none());
}

#[cfg(unix)]
#[test]
fn backup_refuses_a_broken_source_symlink() {
    use std::os::unix::fs::symlink;
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    symlink(
        project.path().join("missing-target"),
        project.path().join("AGENTS.md"),
    )
    .unwrap();

    let error =
        graphhelm_host_adoption::backup(project.path(), home.path(), state.path()).unwrap_err();

    assert_eq!(
        error.reason,
        graphhelm_protocols::adoption::AdoptionReason::PathUnsafe
    );
}

#[cfg(unix)]
fn replace_with_fifo(path: &std::path::Path) {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    std::fs::remove_file(path).unwrap();
    let path = CString::new(path.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
}

#[cfg(unix)]
#[test]
fn verify_rejects_a_manifest_fifo_without_blocking() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("AGENTS.md"), b"original").unwrap();
    let receipt =
        graphhelm_host_adoption::backup(project.path(), home.path(), state.path()).unwrap();
    let id = receipt["id"].as_str().unwrap();
    replace_with_fifo(&state.path().join("backups").join(id).join("manifest.json"));

    let error = graphhelm_host_adoption::verify_backup(state.path(), id).unwrap_err();
    assert_eq!(
        error.reason,
        graphhelm_protocols::adoption::AdoptionReason::BackupCorrupt
    );
}

#[cfg(unix)]
#[test]
fn verify_rejects_a_blob_fifo_without_blocking() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("AGENTS.md"), b"original").unwrap();
    let receipt =
        graphhelm_host_adoption::backup(project.path(), home.path(), state.path()).unwrap();
    let id = receipt["id"].as_str().unwrap();
    replace_with_fifo(&state.path().join("backups").join(id).join("blob-0"));

    let error = graphhelm_host_adoption::verify_backup(state.path(), id).unwrap_err();
    assert_eq!(
        error.reason,
        graphhelm_protocols::adoption::AdoptionReason::BackupCorrupt
    );
}

#[cfg(unix)]
#[test]
fn backup_refuses_a_symlink_in_the_source_root_ancestry() {
    use std::os::unix::fs::symlink;
    let outside = tempfile::tempdir().unwrap();
    let holder = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("AGENTS.md"), b"outside").unwrap();
    let link = holder.path().join("linked");
    symlink(outside.path(), &link).unwrap();

    let error = graphhelm_host_adoption::backup(&link, home.path(), state.path()).unwrap_err();

    assert_eq!(
        error.reason,
        graphhelm_protocols::adoption::AdoptionReason::PathUnsafe
    );
}

#[cfg(unix)]
#[test]
fn backup_blobs_and_directories_are_private_before_bytes_are_written() {
    use std::os::unix::fs::PermissionsExt;
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("AGENTS.md"), b"private").unwrap();

    let receipt =
        graphhelm_host_adoption::backup(project.path(), home.path(), state.path()).unwrap();
    let id = receipt["id"].as_str().unwrap();
    let backup = state.path().join("backups").join(id);

    assert_eq!(
        std::fs::metadata(&backup).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(backup.join("blob-0"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[cfg(windows)]
#[test]
fn backup_blob_has_the_repository_owner_only_acl() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("AGENTS.md"), b"private").unwrap();

    let receipt =
        graphhelm_host_adoption::backup(project.path(), home.path(), state.path()).unwrap();
    let id = receipt["id"].as_str().unwrap();
    let blob = std::fs::File::open(state.path().join("backups").join(id).join("blob-0")).unwrap();

    graphhelm_sealed_key_provider::verify_owner_only(&blob).unwrap();
}
