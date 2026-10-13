//! #927: a backend-only project proves a journey without a browser. The flow runs a `cli` act
//! (`sh greet.sh`, exit code and stdout) and an `http` act (GET on a small Python fixture server,
//! status and a JSON field). Credible defects: a backend flow that still demands the browser
//! observer or browser coverage (validation never green), an expectation that passes on the
//! wrong value (sabotage `missed` where it must be `caught`), a draft that runs a command, and a
//! mutating request sent without the owner's click. Real CLI, Git worktree, launcher and child
//! processes; no browser, Node or provider. Cost: a few seconds after build; needs `sh` and
//! `python3`.
use std::path::Path;

use serde_json::Value;

const FLOW: &str = include_str!("fixtures/journey_backend/backend.journey.yaml");
const CLEANUP: &str = include_str!("fixtures/journey_backend/cleanup.journey.yaml");
const LAUNCHER: &str = include_str!("fixtures/journey_backend/fixture.sh");
const SERVER: &str = include_str!("fixtures/journey_backend/server.py");
/// `graphhelm init` owns the ignore file; keep the journeys and the launcher declaration tracked
/// while owner and runtime material stays private.
const IGNORE: &str = "deletes.count\n.graphhelm/*\n!.graphhelm/journeys/\n!.graphhelm/journeys/**\n!.graphhelm/fixtures/\n!.graphhelm/fixtures/**\n!.graphhelm/journey-fixture.json\n";

fn graphhelm(args: &[&str], project: &Path, extra: &[&Path]) -> (i32, Value) {
    let mut command = graphhelm_process::command(assert_cmd::cargo::cargo_bin!("graphhelm"));
    // CLI acts inherit PATH from the replay worker. Resolve this test's built CLI there,
    // so the fixture never depends on a machine-installed version.
    let mut path = std::ffi::OsString::from(
        assert_cmd::cargo::cargo_bin!("graphhelm")
            .parent()
            .unwrap()
            .as_os_str(),
    );
    path.push(if cfg!(windows) { ";" } else { ":" });
    path.push(std::env::var_os("PATH").unwrap_or_default());
    #[cfg(windows)]
    {
        // Git Bash is the shell used by the fixture launcher, but its bin directory is not
        // necessarily on PATH for the backend `sh greet.sh` act.
        let git_bin = Path::new(r"C:\Program Files\Git\bin");
        if git_bin.join("sh.exe").is_file() {
            let mut with_shell = std::ffi::OsString::from(git_bin.as_os_str());
            with_shell.push(";");
            with_shell.push(path);
            path = with_shell;
        }
    }
    command.env("PATH", path);
    command.env("GRAPHHELM_CLI", assert_cmd::cargo::cargo_bin!("graphhelm"));
    let out = command
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

/// #1047: replay four CLI acts against disposable bare Git repositories and literal
/// comment snapshots. A stale approval must not appear at B; a same-head BLOCK must.
/// Cost: four short Python/CLI/Git processes; no network, Runtime or browser.
#[test]
fn pinned_verdict_journey_replays_all_four_obligations() {
    const VERDICT_FLOW: &str =
        include_str!("../../../.graphhelm/journeys/keel-pinned-verdict-merge.journey.yaml");
    let dir = project("keel-pinned-verdict-merge", VERDICT_FLOW);
    let root = dir.path();
    let fixture = root.join(".graphhelm/fixtures/keel-pinned-verdict-merge");
    std::fs::create_dir_all(&fixture).unwrap();
    for name in ["run.py", "approval-a.json", "review-b.json"] {
        std::fs::write(
            fixture.join(name),
            std::fs::read(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../.graphhelm/fixtures/keel-pinned-verdict-merge")
                    .join(name),
            )
            .unwrap(),
        )
        .unwrap();
    }
    approve(root, "keel-pinned-verdict-merge");
    let (code, report) = graphhelm(&["keel", "test", "validation", "--repo"], root, &[]);
    assert_eq!(code, 0, "{report}");
    let data = &report["data"];
    assert_eq!(data["failures"], serde_json::json!([]), "{data}");
    assert_eq!(data["paths"][0]["execution"], "completed", "{data}");
    assert_eq!(data["paths"][0]["collection"], "not_applicable", "{data}");
    let state: Value = serde_json::from_slice(
        &std::fs::read(
            Path::new(data["flows"][0]["resultDir"].as_str().unwrap()).join("state.json"),
        )
        .unwrap(),
    )
    .unwrap();
    for edge in ["review.a", "review.stale", "review.b", "review.block"] {
        assert_eq!(state["edges"][edge]["result"], "pass", "{edge}: {state}");
    }
}

fn git(project: &Path, args: &[&str]) -> Vec<u8> {
    let out = graphhelm_process::command("git")
        .arg("-C")
        .arg(project)
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}");
    out.stdout
}

fn commit(root: &Path, message: &str) {
    std::fs::write(root.join(".gitignore"), IGNORE).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "--quiet", "--no-verify", "-m", message]);
}

/// A committed project holding one DRAFT flow `id`, with the fixture app and its launcher.
fn project(id: &str, flow: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for (path, text) in [
        ("site/status.json", r#"{"status":"ok"}"#),
        ("site/items.json", r#"{"items":[{"name":"apple"}]}"#),
        ("greet.sh", "echo hello from the cli\n"),
        ("README.txt", "not observed\n"),
        ("fixture.sh", LAUNCHER),
        ("server.py", SERVER),
        (
            ".graphhelm/journey-fixture.json",
            r#"{"schema":"graphhelm-journey-fixture/1","script":"fixture.sh","isolated":true}"#,
        ),
        (&format!(".graphhelm/journeys/{id}.journey.yaml"), flow),
    ] {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    let (code, reply) = graphhelm(
        &[
            "journey",
            "compile",
            id,
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
    let (code, reply) = graphhelm(
        &["init", "--harness", "claude-code", "--project"],
        root,
        &[],
    );
    assert_eq!(code, 0, "{reply}");
    commit(root, "fixture");
    dir
}

/// The owner approves `id` and the approval is committed.
fn approve(root: &Path, id: &str) {
    let token = root.join(".graphhelm/events.token");
    let token = token.to_str().unwrap();
    let (code, reply) = graphhelm(
        &["journey", "approve", id, "--token-file", token, "--project"],
        root,
        &[],
    );
    assert_eq!(code, 0, "{reply}");
    commit(root, "approved flow");
}

fn deletes(root: &Path) -> usize {
    std::fs::read_to_string(root.join("deletes.count")).map_or(0, |count| count.len())
}

#[test]
fn a_backend_journey_is_green_on_good_code_and_sabotage_catches_a_break() {
    let dir = project("backend", FLOW);
    let root = dir.path();
    let scratch = tempfile::tempdir().unwrap();

    // A draft never runs a command: nobody has reviewed its argv.
    let (code, report) = graphhelm(&["keel", "test", "validation", "--repo"], root, &[]);
    assert_eq!(code, 2, "{report}");
    assert_eq!(
        report["data"]["paths"][0]["execution"], "skipped",
        "{report}"
    );
    let state = std::fs::read_dir(report["data"]["resultDir"].as_str().unwrap())
        .unwrap()
        .flatten()
        .find_map(|entry| std::fs::read(entry.path().join("state.json")).ok())
        .map(|bytes| serde_json::from_slice::<Value>(&bytes).unwrap());
    let state = state.expect("the validation run keeps its state");
    assert_eq!(
        state["edges"]["greet"]["reason"], "cli_needs_approval",
        "{state}"
    );

    approve(root, "backend");
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

/// #927 review: a DELETE is a mutation whatever its path says. A draft never sends it, an
/// approved flow holds it until the owner's click, and only the confirmed run reaches the app,
/// counted by the server itself.
#[test]
fn a_backend_delete_is_held_until_the_owner_confirms() {
    let dir = project("cleanup", CLEANUP);
    let root = dir.path();
    // Start a preview in the background and read its finished result.
    let preview = |extra: &[&str]| -> Value {
        let mut args = vec!["journey", "preview", "cleanup"];
        args.extend(extra);
        args.push("--project");
        let (code, reply) = graphhelm(&args, root, &[]);
        assert_eq!(code, 0, "{reply}");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        loop {
            let (_, read) = graphhelm(
                &["journey", "preview", "cleanup", "--read", "--project"],
                root,
                &[],
            );
            if read["data"]["state"] != "running" && read["data"]["ranAt"].is_string() {
                return read["data"].clone();
            }
            assert!(std::time::Instant::now() < deadline, "never ended: {read}");
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    };

    let draft = preview(&[]);
    assert_eq!(draft["edges"]["wipe"]["reason"], "guard_refused", "{draft}");
    assert_eq!(deletes(root), 0, "a draft sent a DELETE");

    approve(root, "cleanup");
    let held = preview(&[]);
    assert_eq!(held["held"]["edge"], "wipe", "{held}");
    assert_eq!(deletes(root), 0, "a held DELETE reached the app");

    let played = preview(&["--confirm"]);
    assert_eq!(played["edges"]["wipe"]["result"], "pass", "{played}");
    assert_eq!(
        deletes(root),
        1,
        "the confirmed DELETE did not reach the app once"
    );
}
