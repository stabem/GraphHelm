//! #932: the portable JPD guide (`docs/jpd/README.md`) walks a reader through
//! `examples/jpd-quickstart/`. This test runs that walk-through exactly: init, compile, commit,
//! approve, `keel test validation` green, and the shipped sabotage patch `caught`. Credible
//! defect: the example or the CLI drifts (a renamed act, a changed expectation role, an ignore
//! rule that drops the journey from the commit) and the guide's documented commands stop working.
//! Real CLI, Git and child processes; no browser, Node or provider. Cost: a few seconds after
//! build; needs `sh` and `python3`.
use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn graphhelm(project: &Path, args: &[&str]) -> (i32, Value) {
    let out = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .current_dir(project)
        .arg("--json")
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        serde_json::from_slice(&out.stdout).unwrap_or(Value::Null),
    )
}

fn git(project: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(project)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
fn the_quickstart_example_validates_green_and_its_sabotage_is_caught() {
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/jpd-quickstart");
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    copy(&example, root);

    git(root, &["init", "--quiet", "--object-format=sha1"]);
    git(
        root,
        &["config", "user.email", "quickstart@example.invalid"],
    );
    git(root, &["config", "user.name", "Quickstart"]);
    for args in [
        &["init", "--harness", "claude-code", "--project", "."][..],
        &[
            "journey",
            "compile",
            "quickstart",
            "--fmt",
            "--include-draft",
            "--project",
            ".",
        ],
    ] {
        let (code, reply) = graphhelm(root, args);
        assert_eq!(code, 0, "{args:?}: {reply}");
    }
    git(root, &["add", "-A"]);
    git(
        root,
        &[
            "commit",
            "--quiet",
            "--no-verify",
            "-m",
            "app and draft journey",
        ],
    );

    // A draft never runs its `cli` act, so validation cannot be green yet.
    let (code, report) = graphhelm(root, &["keel", "test", "validation", "--repo", "."]);
    assert_eq!(code, 2, "{report}");
    // ml-saas trial: the exit 2 must say why (a draft), not only `ok: false`.
    assert_eq!(
        report["data"]["failures"][0]["reason"], "flow_not_approved",
        "{report}"
    );
    assert_eq!(
        report["diagnostics"][0]["code"], "GHCLI039_KEEL_VALIDATION_BLOCKED",
        "{report}"
    );

    let (code, reply) = graphhelm(
        root,
        &[
            "journey",
            "approve",
            "quickstart",
            "--token-file",
            ".graphhelm/events.token",
            "--project",
            ".",
        ],
    );
    assert_eq!(code, 0, "{reply}");
    git(root, &["add", "-A"]);
    git(
        root,
        &[
            "commit",
            "--quiet",
            "--no-verify",
            "-m",
            "approve quickstart",
        ],
    );

    let (code, report) = graphhelm(root, &["keel", "test", "validation", "--repo", "."]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(
        report["data"]["paths"][0]["execution"], "completed",
        "{report}"
    );

    // ml-saas trial: a hand-written `*.json` beside the flows is named `unsupported` with its
    // reason and never blocks a sweep whose every declared path completed.
    std::fs::write(root.join(".graphhelm/journeys/legacy.json"), "{}").unwrap();
    let (code, report) = graphhelm(root, &["keel", "test", "validation", "--repo", "."]);
    assert_eq!(code, 0, "{report}");
    let legacy = &report["data"]["standaloneContracts"][0];
    assert_eq!(
        legacy["reason"], "standalone_contract_not_executed",
        "{report}"
    );
    std::fs::remove_file(root.join(".graphhelm/journeys/legacy.json")).unwrap();

    let (code, report) = graphhelm(
        root,
        &[
            "keel",
            "test",
            "sabotage",
            "--repo",
            ".",
            "--patch",
            "sabotage/break-items.diff",
        ],
    );
    assert_eq!(code, 0, "{report}");
    assert_eq!(report["data"]["verdict"], "caught", "{report}");
    assert_eq!(
        report["data"]["failing"][0]["execution"], "drift",
        "{report}"
    );
}
