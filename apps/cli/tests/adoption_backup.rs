#[test]
fn backup_command_creates_a_verified_receipt() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    std::fs::write(project.path().join("AGENTS.md"), b"keep me").unwrap();

    let output = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["backup", "--project"])
        .arg(project.path())
        .args(["--home"])
        .arg(home.path())
        .args(["--state-root"])
        .arg(state.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["command"], "backup");
    assert_eq!(value["data"]["receipt"]["spec"]["verified"], true);
}
