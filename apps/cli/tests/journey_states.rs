//! #1784: a flow is seen in every starting state it declares. A delivered item stayed on
//! Zuvemo's agenda because the agenda journey only ever ran on a seed of items "on the way".
//! Credible defects: validation runs a stateful flow once (the other states are never seen), the
//! launcher is not told which state to seed, a state the launcher cannot seed is reported as
//! passed, or a flow without `states` stops running exactly as before. Real CLI, Git, launcher
//! (`sh`) and fixture server (`python3 -m http.server`); no browser, Node or provider.
//! Cost: a few seconds after build.
use std::path::Path;

use serde_json::Value;

const FLOW: &str = include_str!("fixtures/journey_states/agenda.journey.yaml");
const LAUNCHER: &str = include_str!("fixtures/journey_states/fixture.sh");
const IGNORE: &str = ".graphhelm/*\n!.graphhelm/journeys/\n!.graphhelm/journeys/**\n!.graphhelm/journey-fixture.json\n";

fn graphhelm(args: &[&str], project: &Path, log: &Path) -> (i32, Value) {
    let mut command = graphhelm_process::command(assert_cmd::cargo::cargo_bin!("graphhelm"));
    #[cfg(windows)]
    {
        // Git Bash runs the launcher; put its bin first so `sh` resolves the same way.
        let git_bin = Path::new(r"C:\Program Files\Git\bin");
        if git_bin.join("sh.exe").is_file() {
            let mut path = std::ffi::OsString::from(git_bin.as_os_str());
            path.push(";");
            path.push(std::env::var_os("PATH").unwrap_or_default());
            command.env("PATH", path);
        }
    }
    let out = command
        .env("STATES_LOG", log)
        .arg("--json")
        .args(args)
        .arg(project)
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        serde_json::from_slice(&out.stdout).unwrap_or(Value::Null),
    )
}

fn git(project: &Path, args: &[&str]) {
    let out = graphhelm_process::command("git")
        .arg("-C")
        .arg(project)
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}");
}

/// A committed project holding the draft `agenda` flow with `states` as given (`None`: the line
/// removed), its launcher, and a log file outside the project for what each `up` was told.
fn project(states: Option<&str>) -> (tempfile::TempDir, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let logs = tempfile::tempdir().unwrap();
    let root = dir.path();
    let flow = match states {
        Some(states) => FLOW.replace(
            "states: [on_the_way, delivered, lost]",
            &format!("states: {states}"),
        ),
        None => FLOW.replace("states: [on_the_way, delivered, lost]\n", ""),
    };
    for (path, text) in [
        ("fixture.sh", LAUNCHER),
        (
            ".graphhelm/journey-fixture.json",
            r#"{"schema":"graphhelm-journey-fixture/1","script":"fixture.sh","isolated":true}"#,
        ),
        (".graphhelm/journeys/agenda.journey.yaml", flow.as_str()),
    ] {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    let log = logs.path().join("states.log");
    let (code, reply) = graphhelm(
        &[
            "journey",
            "compile",
            "agenda",
            "--fmt",
            "--include-draft",
            "--project",
        ],
        root,
        &log,
    );
    assert_eq!(code, 0, "{reply}");
    git(root, &["init", "--quiet", "--object-format=sha1"]);
    git(root, &["config", "user.email", "states@example.invalid"]);
    git(root, &["config", "user.name", "States"]);
    std::fs::write(root.join(".gitignore"), IGNORE).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "--quiet", "--no-verify", "-m", "fixture"]);
    (dir, logs)
}

fn validate(root: &Path, logs: &Path) -> (i32, Value, String) {
    let log = logs.join("states.log");
    let _ = std::fs::remove_file(&log);
    let (code, report) = graphhelm(&["keel", "test", "validation", "--repo"], root, &log);
    let told = std::fs::read_to_string(&log).unwrap_or_default();
    (code, report["data"].clone(), told)
}

#[test]
fn a_flow_runs_once_per_declared_state_and_an_unseeded_state_is_unobserved() {
    // Two states: two runs, each told its own state, two passing results.
    let (dir, logs) = project(Some("[on_the_way, delivered]"));
    let (code, data, told) = validate(dir.path(), logs.path());
    assert_eq!(told, "agenda on_the_way\nagenda delivered\n", "{data}");
    assert_eq!(code, 0, "{data}");
    assert_eq!(data["failures"], serde_json::json!([]), "{data}");
    let states = &data["flows"][0]["states"];
    assert_eq!(states[0]["id"], "on_the_way", "{data}");
    assert_eq!(states[0]["result"], "passed", "{data}");
    assert_eq!(states[1]["id"], "delivered", "{data}");
    assert_eq!(states[1]["result"], "passed", "{data}");
    assert_eq!(data["paths"].as_array().unwrap().len(), 2, "{data}");
    for (path, state) in data["paths"]
        .as_array()
        .unwrap()
        .iter()
        .zip(["on_the_way", "delivered"])
    {
        assert_eq!(path["state"], state, "{data}");
        assert_eq!(path["execution"], "completed", "{data}");
    }

    // A state the launcher does not confirm is unobserved: never passed, and the sweep blocks.
    let (dir, logs) = project(Some("[on_the_way, delivered, lost]"));
    let (code, data, told) = validate(dir.path(), logs.path());
    assert_eq!(
        told, "agenda on_the_way\nagenda delivered\nagenda lost\n",
        "{data}"
    );
    assert_eq!(code, 2, "{data}");
    let states = &data["flows"][0]["states"];
    assert_eq!(states[2]["id"], "lost", "{data}");
    assert_eq!(states[2]["result"], "unobserved", "{data}");
    assert_eq!(data["flows"][0]["status"], "failed", "{data}");
    let failures = data["failures"].as_array().unwrap();
    assert_eq!(failures.len(), 1, "{data}");
    assert_eq!(failures[0]["state"], "lost", "{data}");
    assert_eq!(failures[0]["reason"], "state_unobserved", "{data}");
    assert_eq!(failures[0]["detail"], "watch.state_not_seeded", "{data}");
    assert_ne!(data["paths"][2]["execution"], "completed", "{data}");

    // No `states`: one run, the launcher told no state, entries carry none.
    let (dir, logs) = project(None);
    let (code, data, told) = validate(dir.path(), logs.path());
    assert_eq!(told, "agenda -\n", "{data}");
    assert_eq!(code, 0, "{data}");
    assert!(data["flows"][0].get("states").is_none(), "{data}");
    assert_eq!(data["paths"].as_array().unwrap().len(), 1, "{data}");
    assert!(data["paths"][0].get("state").is_none(), "{data}");
}
