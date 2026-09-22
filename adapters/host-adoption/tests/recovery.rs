#[test]
fn corrupt_journal_refuses_recovery_without_guessing() {
    let state = tempfile::tempdir().unwrap();
    std::fs::create_dir(state.path().join("journals")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            state.path().join("journals"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, READ_CONTROL, WRITE_DAC,
        };
        let dir = std::fs::OpenOptions::new()
            .access_mode(READ_CONTROL | WRITE_DAC)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(state.path().join("journals"))
            .unwrap();
        graphhelm_sealed_key_provider::protect_owner_only(&dir).unwrap();
    }
    std::fs::write(
        state
            .path()
            .join("journals")
            .join(format!("{}.json", "a".repeat(64))),
        b"{truncated",
    )
    .unwrap();

    let error = graphhelm_host_adoption::recover(state.path(), &"a".repeat(64)).unwrap_err();

    assert_eq!(
        error.reason,
        graphhelm_protocols::adoption::AdoptionReason::RecoveryRequired
    );
}
