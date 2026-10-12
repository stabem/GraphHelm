//! #927: a backend-only project proves a journey without a browser. The flow runs a `cli` act
//! (`sh greet.sh`, exit code and stdout) and an `http` act (GET on a `python3 -m http.server`
//! fixture, status and a JSON field). Credible defects: a backend flow that still demands the
//! browser observer or browser coverage (validation never green), and an expectation that passes
//! on the wrong value (sabotage `missed` where it must be `caught`). Real CLI, Git worktree,
//! launcher and child processes; no browser, Node or provider. Cost: a few seconds after build;
//! needs `sh` and `python3`.
use std::path::Path;
use std::process::Command;

use serde_json::Value;

const FLOW: &str = include_str!("fixtures/journey_backend/backend.journey.yaml");
const LAUNCHER: &str = include_str!("fixtures/journey_backend/fixture.sh");

fn graphhelm(args: &[&str], project: &Path, extra: &[&Path]) -> (i32, Value) {
    let out = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .arg("--json")
        .args(args)
        .arg(project)
        .args(extra.iter().flat_map(|path| [Path::new("--patch"), path]))
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        serde_json::from_slice(&out.stdout).unwrap_or(Value::Null),
    )
}

fn git(project: &Path, args: &[&str]) -> Vec<u8> {
    let out = Command::new("git")
        .arg("-C")
        .arg(project)
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}");
    out.stdout
}

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for (path, text) in [
        ("site/status.json", r#"{"status":"ok"}"#),
        ("site/items.json", r#"{"items":[{"name":"apple"}]}"#),
        ("greet.sh", "echo hello from the cli\n"),
        ("README.txt", "not observed\n"),
        ("fixture.sh", LAUNCHER),
        (
            ".graphhelm/journey-fixture.json",
            r#"{"schema":"graphhelm-journey-fixture/1","script":"fixture.sh","isolated":true}"#,
        ),
        (".graphhelm/journeys/backend.journey.yaml", FLOW),
    ] {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    let (code, reply) = graphhelm(
        &[
            "journey",
            "compile",
            "backend",
            "--fmt",
            "--include-draft",
            "--project",
        ],
        root,
        &[],
    );
    assert_eq!(code, 0, "{reply}");
    git(root, &["init", "--quiet", "--object-format=sha1"]);
    git(root, &["config", "user.email", "backend@example.invalid"]);
    git(root, &["config", "user.name", "Backend observer"]);
    git(root, &["add", "."]);
    git(root, &["commit", "--quiet", "--no-verify", "-m", "fixture"]);
    dir
}

#[test]
fn a_backend_journey_is_green_on_good_code_and_sabotage_catches_a_break() {
    let dir = project();
    let root = dir.path();
    let scratch = tempfile::tempdir().unwrap();

    let (code, report) = graphhelm(&["keel", "test", "validation", "--repo"], root, &[]);
    assert_eq!(code, 0, "{report}");
    let data = &report["data"];
    assert_eq!(data["failures"], serde_json::json!([]), "{data}");
    assert_eq!(data["paths"][0]["execution"], "completed", "{data}");
    assert_eq!(data["paths"][0]["collection"], "not_applicable", "{data}");

    for (name, file, from, to, verdict) in [
        // The HTTP start screen expects `status=ok`.
        ("json", "site/status.json", "ok", "down", "caught"),
        // The CLI screen expects `hello` on stdout.
        ("stdout", "greet.sh", "hello", "goodbye", "caught"),
        // Nothing observes the README.
        ("unobserved", "README.txt", "not", "never", "missed"),
    ] {
        let target = root.join(file);
        let original = std::fs::read_to_string(&target).unwrap();
        std::fs::write(&target, original.replacen(from, to, 1)).unwrap();
        let diff = git(root, &["diff", "--", file]);
        std::fs::write(&target, &original).unwrap();
        let patch = scratch.path().join(format!("{name}.diff"));
        std::fs::write(&patch, diff).unwrap();
        let (code, report) = graphhelm(&["keel", "test", "sabotage", "--repo"], root, &[&patch]);
        assert_eq!(report["data"]["verdict"], verdict, "{name}: {report}");
        if verdict == "caught" {
            assert_eq!(code, 0, "{name}: {report}");
            assert_eq!(
                report["data"]["failing"][0]["execution"], "drift",
                "{report}"
            );
        }
    }
}
