use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::{Value, json};
use tempfile::TempDir;

const SECRET_PASSWORD: &str = "super-secret-operator-password";

fn command() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
}

fn json_output(arguments: &[&str]) -> (i32, Value) {
    let output = command().args(arguments).output().unwrap();
    assert!(
        output.stderr.is_empty(),
        "stderr must stay empty: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value =
        serde_json::from_slice(&output.stdout).expect("every invocation prints one JSON envelope");
    (output.status.code().unwrap(), value)
}

/// A repository root whose `format.json` declares a format this build does not support.
/// No superseded-format fixture is checked in; it is created per test.
fn unsupported_format_repository(directory: &TempDir) -> PathBuf {
    let root = directory.path().join("repository");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("format.json"), br#"{"formatVersion":0}"#).unwrap();
    root
}

fn supported_format_repository(directory: &TempDir) -> PathBuf {
    let root = directory.path().join("supported");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("format.json"), b"{\"formatVersion\":\"1.0.0\"}\n").unwrap();
    root
}

fn complete_local_repository(directory: &TempDir) -> PathBuf {
    let root = directory.path().join("complete");
    fs::create_dir_all(root.join("blobs")).unwrap();
    fs::create_dir(root.join(".tmp")).unwrap();
    fs::create_dir(root.join("active")).unwrap();
    fs::write(root.join("format.json"), b"{\"formatVersion\":\"1.0.0\"}\n").unwrap();
    fs::write(root.join("journal.jsonl"), []).unwrap();
    fs::write(root.join("repository.lock"), []).unwrap();
    root
}

fn tool_entry(path: &Path) -> Value {
    json!({
        "path": path,
        "sha256": "a".repeat(64),
        "version": "pg_dump (PostgreSQL) 16.14",
    })
}

/// A structurally valid operator config. Individual tests corrupt one field.
fn config_value(directory: &TempDir) -> Value {
    let passfile = directory.path().join("pgpass.conf");
    fs::write(&passfile, format!("*:*:*:*:{SECRET_PASSWORD}\n")).unwrap();
    let dump = directory.path().join("pg_dump.exe");
    let restore = directory.path().join("pg_restore.exe");
    fs::write(&dump, b"tool").unwrap();
    fs::write(&restore, b"tool").unwrap();
    let keyring = directory.path().join("keyring");
    fs::create_dir_all(&keyring).unwrap();
    json!({
        "adminUrl": format!("postgres://operator:{SECRET_PASSWORD}@127.0.0.1:5432/graphhelm"),
        "passfile": passfile,
        "keyring": {"directory": keyring, "keyId": "operator-key"},
        "pgDump": tool_entry(&dump),
        "pgRestore": tool_entry(&restore),
        "processTimeoutSeconds": 600,
    })
}

fn write_config(directory: &TempDir, value: &Value) -> PathBuf {
    let path = directory.path().join("operator.json");
    fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
    // The operator refuses a group- or world-accessible configuration on unix; a fixture
    // written under the default umask would stop at that check instead of the one under test.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    path
}

fn diagnostic_codes(value: &Value) -> Vec<String> {
    value["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["code"].as_str().unwrap().to_owned())
        .collect()
}

// ---------------------------------------------------------------------------
// Unsupported format, with no legacy or import fallback
// ---------------------------------------------------------------------------

#[test]
fn unsupported_repository_format_fails_without_import_fallback() {
    let directory = TempDir::new().unwrap();
    let root = unsupported_format_repository(&directory);
    let (code, value) = json_output(&["events", "verify", "--repository", root.to_str().unwrap()]);
    assert_eq!(code, 2);
    assert_eq!(value["ok"], false);
    assert_eq!(value["command"], "events.verify");
    assert!(diagnostic_codes(&value).contains(&"GHE007_UNSUPPORTED_FORMAT".to_owned()));
    let rendered = value.to_string().to_lowercase();
    for forbidden in ["legacy", "import", "downgrade", "migrate the repository"] {
        assert!(
            !rendered.contains(forbidden),
            "unsupported format output must not offer {forbidden}: {rendered}"
        );
    }
}

#[test]
fn verify_reports_a_missing_repository_as_an_argument_without_creating_it() {
    let directory = TempDir::new().unwrap();
    let root = directory.path().join("missing-repository");
    let (code, value) = json_output(&["events", "verify", "--repository", root.to_str().unwrap()]);

    assert_eq!(code, 2);
    assert!(diagnostic_codes(&value).contains(&"GHCLI001_ARGUMENT_INVALID".to_owned()));
    assert_eq!(value["diagnostics"][0]["path"], "/repository");
    assert!(!value.to_string().contains(root.to_str().unwrap()));
    assert!(!root.exists());
}

#[test]
fn verify_reports_a_format_only_repository_as_integrity_without_repair() {
    let directory = TempDir::new().unwrap();
    let root = supported_format_repository(&directory);
    let (code, value) = json_output(&["events", "verify", "--repository", root.to_str().unwrap()]);

    assert_eq!(code, 2);
    assert!(diagnostic_codes(&value).contains(&"GHE005_INTEGRITY_FAILURE".to_owned()));
    assert_eq!(value["diagnostics"][0]["path"], "/repository");
    assert!(!value.to_string().contains(root.to_str().unwrap()));
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
}

#[test]
fn verify_reports_an_ordinary_wrong_required_slot_type_as_integrity() {
    let directory = TempDir::new().unwrap();
    let root = complete_local_repository(&directory);
    fs::remove_file(root.join("journal.jsonl")).unwrap();
    fs::create_dir(root.join("journal.jsonl")).unwrap();

    let (code, value) = json_output(&["events", "verify", "--repository", root.to_str().unwrap()]);

    assert_eq!(code, 2);
    assert!(diagnostic_codes(&value).contains(&"GHE005_INTEGRITY_FAILURE".to_owned()));
    assert_eq!(value["diagnostics"][0]["path"], "/repository");
    assert!(!value.to_string().contains(root.to_str().unwrap()));
    assert!(root.join("journal.jsonl").is_dir());
}

#[test]
fn verify_accepts_a_valid_repository_without_recreating_transient_directories() {
    let directory = TempDir::new().unwrap();
    let root = complete_local_repository(&directory);
    fs::remove_dir(root.join(".tmp")).unwrap();
    fs::remove_dir(root.join("active")).unwrap();
    let names_before = fs::read_dir(&root).unwrap().count();
    let (code, value) = json_output(&["events", "verify", "--repository", root.to_str().unwrap()]);

    assert_eq!(code, 0);
    assert_eq!(value["ok"], true);
    assert_eq!(value["data"]["formatSupported"], true);
    assert_eq!(fs::read_dir(&root).unwrap().count(), names_before);
    assert!(!root.join(".tmp").exists());
    assert!(!root.join("active").exists());
}

#[test]
fn no_event_import_or_repository_migration_command_exists() {
    for arguments in [
        vec!["events", "import"],
        vec!["events", "migrate"],
        vec!["events", "upgrade"],
        vec!["events", "convert"],
    ] {
        let output = command().args(&arguments).output().unwrap();
        assert_ne!(
            output.status.code(),
            Some(0),
            "{arguments:?} must not be a supported command"
        );
        let rendered = String::from_utf8_lossy(&output.stdout).to_lowercase();
        assert!(!rendered.contains("\"ok\":true"));
    }
}

// ---------------------------------------------------------------------------
// Mutually exclusive and required selectors
// ---------------------------------------------------------------------------

#[test]
fn verify_requires_exactly_one_repository_selector() {
    let directory = TempDir::new().unwrap();
    let root = supported_format_repository(&directory);
    let config = write_config(&directory, &config_value(&directory));

    let (neither_code, neither) = json_output(&["events", "verify"]);
    assert_eq!(neither_code, 2);
    assert!(diagnostic_codes(&neither).contains(&"GHCLI001_ARGUMENT_INVALID".to_owned()));

    let (both_code, both) = json_output(&[
        "events",
        "verify",
        "--repository",
        root.to_str().unwrap(),
        "--config",
        config.to_str().unwrap(),
    ]);
    assert_eq!(both_code, 2);
    assert!(diagnostic_codes(&both).contains(&"GHCLI001_ARGUMENT_INVALID".to_owned()));
}

// ---------------------------------------------------------------------------
// Verify range bounds
// ---------------------------------------------------------------------------

/// Uses `--config`, not `--repository`. Against a local repository `verify_local` refuses any range
/// with the same code the bounds check produces, so a `--repository` variant of this test stays
/// green even with the bounds checks deleted and cannot fail on the property it names.
#[test]
fn verify_rejects_out_of_range_sequence_windows() {
    let directory = TempDir::new().unwrap();
    let config = write_config(&directory, &config_value(&directory));
    let config = config.to_str().unwrap();
    for (start, max_events) in [("0", "10"), ("1", "0"), ("1", "100001")] {
        let (code, value) = json_output(&[
            "events",
            "verify",
            "--config",
            config,
            "--stream",
            "exec-stream",
            "--workspace",
            "ws-operator",
            "--project",
            "prj-operator",
            "--start",
            start,
            "--max-events",
            max_events,
        ]);
        assert_eq!(code, 2, "start={start} max-events={max_events}");
        assert!(diagnostic_codes(&value).contains(&"GHCLI001_ARGUMENT_INVALID".to_owned()));
    }
}

#[test]
fn verify_range_requires_a_complete_scope() {
    let directory = TempDir::new().unwrap();
    let root = supported_format_repository(&directory);
    let (code, value) = json_output(&[
        "events",
        "verify",
        "--repository",
        root.to_str().unwrap(),
        "--stream",
        "exec-stream",
    ]);
    assert_eq!(code, 2);
    assert!(diagnostic_codes(&value).contains(&"GHCLI001_ARGUMENT_INVALID".to_owned()));
}

// ---------------------------------------------------------------------------
// Rebuild generation bounds
// ---------------------------------------------------------------------------

/// Projection generations are durable only in the PostgreSQL adapter, so rebuild is configured
/// rather than pointed at a local repository directory.
#[test]
fn rebuild_rejects_a_zero_generation() {
    let directory = TempDir::new().unwrap();
    let config = write_config(&directory, &config_value(&directory));
    let (code, value) = json_output(&[
        "events",
        "rebuild",
        "--config",
        config.to_str().unwrap(),
        "--workspace",
        "ws-operator",
        "--project",
        "prj-operator",
        "--stream",
        "exec-stream",
        "--generation",
        "0",
    ]);
    assert_eq!(code, 2);
    assert!(diagnostic_codes(&value).contains(&"GHCLI001_ARGUMENT_INVALID".to_owned()));
}

#[test]
fn rebuild_requires_a_repository_configuration() {
    let (code, value) = json_output(&[
        "events",
        "rebuild",
        "--workspace",
        "ws-operator",
        "--project",
        "prj-operator",
        "--stream",
        "exec-stream",
    ]);
    assert_eq!(code, 2);
    assert!(diagnostic_codes(&value).contains(&"GHCLI001_ARGUMENT_INVALID".to_owned()));
}

// ---------------------------------------------------------------------------
// Backup and restore target validation
// ---------------------------------------------------------------------------

#[test]
fn backup_refuses_an_existing_output_target() {
    let directory = TempDir::new().unwrap();
    let config = write_config(&directory, &config_value(&directory));
    let output = directory.path().join("already-there.ghbak");
    fs::write(&output, b"occupied").unwrap();
    let (code, value) = json_output(&[
        "events",
        "backup",
        "--config",
        config.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
    ]);
    assert_eq!(code, 2);
    assert!(diagnostic_codes(&value).contains(&"GHCLI001_ARGUMENT_INVALID".to_owned()));
    assert_eq!(fs::read(&output).unwrap(), b"occupied");
}

#[test]
fn restore_refuses_a_missing_archive() {
    let directory = TempDir::new().unwrap();
    let config = write_config(&directory, &config_value(&directory));
    let (code, value) = json_output(&[
        "events",
        "restore",
        "--config",
        config.to_str().unwrap(),
        "--archive",
        directory.path().join("absent.ghbak").to_str().unwrap(),
    ]);
    assert_eq!(code, 2);
    assert!(diagnostic_codes(&value).contains(&"GHCLI001_ARGUMENT_INVALID".to_owned()));
}

#[test]
fn restore_refuses_an_archive_that_is_not_a_regular_file() {
    let directory = TempDir::new().unwrap();
    let config = write_config(&directory, &config_value(&directory));
    let archive = directory.path().join("archive-directory");
    fs::create_dir_all(&archive).unwrap();
    let (code, value) = json_output(&[
        "events",
        "restore",
        "--config",
        config.to_str().unwrap(),
        "--archive",
        archive.to_str().unwrap(),
    ]);
    assert_eq!(code, 2);
    assert!(diagnostic_codes(&value).contains(&"GHCLI001_ARGUMENT_INVALID".to_owned()));
}

/// The destination is the database named by the configuration, and the operator refuses to proceed
/// unless it is already empty. There is deliberately no `--target` flag to validate and discard.
#[test]
fn restore_exposes_no_target_database_flag() {
    let directory = TempDir::new().unwrap();
    let config = write_config(&directory, &config_value(&directory));
    let archive = directory.path().join("present.ghbak");
    fs::write(&archive, b"archive").unwrap();
    let output = command()
        .args([
            "events",
            "restore",
            "--config",
            config.to_str().unwrap(),
            "--archive",
            archive.to_str().unwrap(),
            "--target",
            "graphhelm_restored",
        ])
        .output()
        .unwrap();
    assert_ne!(output.status.code(), Some(0));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("\"ok\":true"));
}

// ---------------------------------------------------------------------------
// Config loading: bounded, structured, and safe
// ---------------------------------------------------------------------------

#[test]
fn config_rejects_an_oversized_document() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("huge.json");
    let padding = "p".repeat(5 * 1024 * 1024);
    fs::write(&path, format!("{{\"adminUrl\":\"{padding}\"}}")).unwrap();
    let (code, value) = json_output(&[
        "events",
        "backup",
        "--config",
        path.to_str().unwrap(),
        "--output",
        directory.path().join("out.ghbak").to_str().unwrap(),
    ]);
    assert_eq!(code, 2);
    assert!(diagnostic_codes(&value).contains(&"GHCLI002_CONFIG_INVALID".to_owned()));
}

#[test]
fn config_rejects_a_malformed_document() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("bad.json");
    fs::write(&path, b"{not json").unwrap();
    let (code, value) = json_output(&[
        "events",
        "backup",
        "--config",
        path.to_str().unwrap(),
        "--output",
        directory.path().join("out.ghbak").to_str().unwrap(),
    ]);
    assert_eq!(code, 2);
    assert!(diagnostic_codes(&value).contains(&"GHCLI002_CONFIG_INVALID".to_owned()));
}

#[test]
fn config_rejects_an_out_of_range_process_timeout() {
    let directory = TempDir::new().unwrap();
    for seconds in [json!(0), json!(86_401)] {
        let mut value = config_value(&directory);
        value["processTimeoutSeconds"] = seconds.clone();
        let path = write_config(&directory, &value);
        let (code, rendered) = json_output(&[
            "events",
            "backup",
            "--config",
            path.to_str().unwrap(),
            "--output",
            directory.path().join("out.ghbak").to_str().unwrap(),
        ]);
        assert_eq!(code, 2, "seconds={seconds}");
        assert!(diagnostic_codes(&rendered).contains(&"GHCLI002_CONFIG_INVALID".to_owned()));
    }
}

#[test]
fn config_rejects_a_relative_tool_path() {
    let directory = TempDir::new().unwrap();
    let mut value = config_value(&directory);
    value["pgDump"]["path"] = json!("relative/pg_dump");
    let path = write_config(&directory, &value);
    let (code, rendered) = json_output(&[
        "events",
        "backup",
        "--config",
        path.to_str().unwrap(),
        "--output",
        directory.path().join("out.ghbak").to_str().unwrap(),
    ]);
    assert_eq!(code, 2);
    assert!(diagnostic_codes(&rendered).contains(&"GHCLI002_CONFIG_INVALID".to_owned()));
}

#[test]
fn config_rejects_a_non_sha256_tool_digest() {
    let directory = TempDir::new().unwrap();
    let mut value = config_value(&directory);
    value["pgRestore"]["sha256"] = json!("not-a-digest");
    let path = write_config(&directory, &value);
    let (code, rendered) = json_output(&[
        "events",
        "backup",
        "--config",
        path.to_str().unwrap(),
        "--output",
        directory.path().join("out.ghbak").to_str().unwrap(),
    ]);
    assert_eq!(code, 2);
    assert!(diagnostic_codes(&rendered).contains(&"GHCLI002_CONFIG_INVALID".to_owned()));
}

#[test]
fn config_is_accepted_from_the_environment_variable() {
    let directory = TempDir::new().unwrap();
    let mut value = config_value(&directory);
    value["processTimeoutSeconds"] = json!(0);
    let path = write_config(&directory, &value);
    let output = command()
        .args([
            "events",
            "backup",
            "--output",
            directory.path().join("out.ghbak").to_str().unwrap(),
        ])
        .env("GRAPHHELM_EVENTS_CONFIG", &path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let rendered: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(diagnostic_codes(&rendered).contains(&"GHCLI002_CONFIG_INVALID".to_owned()));
}

#[cfg(unix)]
#[test]
fn config_rejects_group_or_world_readable_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let directory = TempDir::new().unwrap();
    let path = write_config(&directory, &config_value(&directory));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    let (code, value) = json_output(&[
        "events",
        "backup",
        "--config",
        path.to_str().unwrap(),
        "--output",
        directory.path().join("out.ghbak").to_str().unwrap(),
    ]);
    assert_eq!(code, 2);
    assert!(diagnostic_codes(&value).contains(&"GHCLI002_CONFIG_INVALID".to_owned()));
}

#[test]
fn config_rejects_a_symlinked_path() {
    let directory = TempDir::new().unwrap();
    let real = write_config(&directory, &config_value(&directory));
    let link = directory.path().join("linked.json");
    #[cfg(unix)]
    let created = std::os::unix::fs::symlink(&real, &link);
    #[cfg(windows)]
    let created = std::os::windows::fs::symlink_file(&real, &link);
    if created.is_err() {
        // Unprivileged Windows hosts cannot create symlinks; the rule is still enforced in code.
        return;
    }
    let (code, value) = json_output(&[
        "events",
        "backup",
        "--config",
        link.to_str().unwrap(),
        "--output",
        directory.path().join("out.ghbak").to_str().unwrap(),
    ]);
    assert_eq!(code, 2);
    assert!(diagnostic_codes(&value).contains(&"GHCLI002_CONFIG_INVALID".to_owned()));
}

// ---------------------------------------------------------------------------
// Redaction: no secrets, paths, or backtraces reach public output
// ---------------------------------------------------------------------------

#[test]
fn failures_never_leak_secrets_paths_or_backtraces() {
    let directory = TempDir::new().unwrap();
    let mut value = config_value(&directory);
    value["processTimeoutSeconds"] = json!(0);
    let path = write_config(&directory, &value);
    let output_path = directory.path().join("out.ghbak");
    let (code, rendered) = json_output(&[
        "events",
        "backup",
        "--config",
        path.to_str().unwrap(),
        "--output",
        output_path.to_str().unwrap(),
    ]);
    assert_eq!(code, 2);
    let text = rendered.to_string();
    assert!(!text.contains(SECRET_PASSWORD), "password leaked: {text}");
    assert!(!text.contains("postgres://"), "DSN leaked: {text}");
    let root = directory.path().to_string_lossy().replace('\\', "\\\\");
    assert!(
        !text.contains(root.as_str()),
        "filesystem path leaked: {text}"
    );
    for forbidden in ["panicked", "RUST_BACKTRACE", "stack backtrace", ".rs:"] {
        assert!(!text.contains(forbidden), "{forbidden} leaked: {text}");
    }
}

// ---------------------------------------------------------------------------
// Bounded, single-envelope output
// ---------------------------------------------------------------------------

#[test]
fn every_events_invocation_emits_exactly_one_bounded_envelope() {
    let directory = TempDir::new().unwrap();
    let root = unsupported_format_repository(&directory);
    let output = command()
        .args(["events", "verify", "--repository", root.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(text.lines().count(), 1, "output must be one JSON line");
    assert!(text.len() < 64 * 1024, "output must stay bounded");
    let value: Value = serde_json::from_str(&text).unwrap();
    for key in ["ok", "command", "data", "diagnostics"] {
        assert!(value.get(key).is_some(), "missing {key}");
    }
}
