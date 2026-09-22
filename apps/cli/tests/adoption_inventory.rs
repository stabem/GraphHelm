use std::path::Path;

use serde_json::Value;

fn graphhelm() -> assert_cmd::Command {
    assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
}

fn output_json(output: std::process::Output) -> Value {
    assert!(
        output.status.success(),
        "setup failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn setup(project: &Path, home: &Path) -> Value {
    output_json(
        graphhelm()
            .args(["setup", "--dry-run", "--project"])
            .arg(project)
            .args(["--home"])
            .arg(home)
            .output()
            .unwrap(),
    )
}

fn setup_default(project: &Path, home: &Path) -> std::process::Output {
    graphhelm()
        .args(["setup", "--project"])
        .arg(project)
        .args(["--home"])
        .arg(home)
        .output()
        .unwrap()
}

#[test]
fn setup_defaults_to_safe_preview_without_dry_run() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();

    let output = setup_default(project.path(), home.path());

    assert!(
        output.status.success(),
        "setup failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["data"]["plan"]["kind"], "AdoptionPlan");
}

#[test]
fn setup_rejects_a_missing_explicit_root() {
    let project = tempfile::tempdir().unwrap();
    let home = project.path().join("missing-home");

    let output = setup_default(project.path(), &home);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("GHCLI029_ADOPTION_REFUSED"));
}

#[cfg(unix)]
#[test]
fn setup_rejects_a_broken_symlink_surface() {
    use std::os::unix::fs::symlink;

    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    symlink(
        home.path().join("does-not-exist"),
        home.path().join(".claude.json"),
    )
    .unwrap();

    let output = setup_default(project.path(), home.path());

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("GHCLI029_ADOPTION_REFUSED"));
}

#[cfg(unix)]
#[test]
fn setup_rejects_a_symlink_in_the_root_ancestry() {
    use std::os::unix::fs::symlink;

    let outside = tempfile::tempdir().unwrap();
    let holder = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let link = holder.path().join("linked");
    symlink(outside.path(), &link).unwrap();

    let output = setup_default(&link, home.path());

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("GHCLI029_ADOPTION_REFUSED"));
}

#[test]
fn preview_keeps_existing_settings_byte_identical() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir(home.path().join(".claude")).unwrap();
    let path = home.path().join(".claude/settings.json");
    let original = br#"{"permissions":{"deny":["Read(.env)"]},"unknown":7}"#;
    std::fs::write(&path, original).unwrap();

    let value = setup(project.path(), home.path());

    assert_eq!(value["data"]["inventory"]["kind"], "Inventory");
    assert_eq!(std::fs::read(path).unwrap(), original);
}

#[test]
fn preview_reports_each_supported_host_in_stable_order() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir(home.path().join(".claude")).unwrap();
    std::fs::create_dir(home.path().join(".codex")).unwrap();
    std::fs::write(home.path().join(".claude/settings.json"), b"{}").unwrap();
    std::fs::write(home.path().join(".codex/config.toml"), b"").unwrap();

    let value = setup(project.path(), home.path());
    let hosts = value["data"]["inventory"]["spec"]["hosts"]
        .as_array()
        .unwrap();

    assert_eq!(hosts[0]["host"], "claude");
    assert_eq!(hosts[1]["host"], "codex");
}

#[test]
fn preview_inventories_project_local_claude_settings_without_reading_home_contents() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir(project.path().join(".claude")).unwrap();
    std::fs::write(
        project.path().join(".claude/settings.local.json"),
        br#"{"permissions":{"deny":["Read(.env)"]}}"#,
    )
    .unwrap();

    let value = setup(project.path(), home.path());
    let items = value["data"]["inventory"]["spec"]["hosts"][0]["items"]
        .as_array()
        .unwrap();

    assert!(
        items
            .iter()
            .any(|item| item["kind"] == ".claude/settings.local.json")
    );
}

#[test]
fn preview_leaves_unclassified_methodology_unresolved() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    std::fs::write(
        project.path().join("AGENTS.md"),
        b"Use a custom software-factory methodology.",
    )
    .unwrap();

    let value = setup(project.path(), home.path());

    assert_eq!(value["data"]["plan"]["kind"], "AdoptionPlan");
    assert_eq!(
        value["data"]["plan"]["spec"]["decisions"][0]["decision"],
        "unresolved"
    );
}
