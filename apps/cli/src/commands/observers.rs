//! Browser journey observers for `graphhelm setup`: whether one is ready in the project, the exact
//! commands that make it ready, and (only on `--install-observer`) running them.
//!
//! Playwright is the default observer: deterministic, no model, no AI cost
//! (`tools/playwright-observer`). `e2e` (tester.army) is opt-in: it needs a model key for agent
//! steps (`tools/e2e-observer`). Readiness reads files and PATH only; nothing runs without the flag.

use std::path::{Path, PathBuf};

use crate::args::ObserverKind;

struct Spec {
    id: &'static str,
    package: &'static str,
    install: [&'static str; 2],
    runner: &'static str,
    note: &'static str,
    /// The observer script, shipped inside the binary and written into the project on install,
    /// so a user without a GraphHelm checkout still has it.
    script_name: &'static str,
    script: &'static str,
}

/// Where `--install-observer` writes the observer scripts, relative to the project.
const SCRIPT_DIR: &str = ".graphhelm/observers";
const DRIVER_NAME: &str = "journey_driver.mjs";
const DRIVER: &[u8] = include_bytes!("../../../../tools/journey-driver/driver.mjs");

const PLAYWRIGHT: Spec = Spec {
    id: "playwright",
    package: "@playwright/test",
    install: [
        // Pinned (#519): the Studio's setup button runs exactly this, so it must not float.
        "npm install --save-dev @playwright/test@1.64.0",
        "npx playwright install chromium",
    ],
    runner: "python .graphhelm/observers/playwright_observe.py --project <project>",
    note: "default: deterministic browser tests, no model key; preview checks files/PATH only, replay observes browser launch",
    script_name: "playwright_observe.py",
    script: include_str!("../../../../tools/playwright-observer/playwright_observe.py"),
};

const E2E: Spec = Spec {
    id: "e2e",
    package: "e2e",
    install: ["npm install --save-dev e2e", "npx e2e-web install chromium"],
    runner: "python .graphhelm/observers/e2e_observe.py --project <project>",
    note: "optional: agent steps need a model key (see the e2e docs)",
    script_name: "e2e_observe.py",
    script: include_str!("../../../../tools/e2e-observer/e2e_observe.py"),
};

fn spec(kind: ObserverKind) -> &'static Spec {
    match kind {
        ObserverKind::Playwright => &PLAYWRIGHT,
        ObserverKind::E2e => &E2E,
    }
}

fn on_path(program: &str) -> bool {
    let names: Vec<String> = if cfg!(windows) {
        ["exe", "cmd"]
            .iter()
            .map(|ext| format!("{program}.{ext}"))
            .collect()
    } else {
        vec![program.to_owned()]
    };
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|dir| names.iter().any(|name| dir.join(name).is_file()))
    })
}

/// Where Playwright keeps browsers: `PLAYWRIGHT_BROWSERS_PATH`, else the per-OS cache under `home`.
fn browser_roots(project: &Path, home: &Path) -> Vec<PathBuf> {
    if let Some(path) = std::env::var_os("PLAYWRIGHT_BROWSERS_PATH") {
        if path == "0" {
            return vec![project.join("node_modules/playwright-core/.local-browsers")];
        }
        return vec![PathBuf::from(path)];
    }
    let mut roots = vec![
        home.join(".cache/ms-playwright"),
        home.join("Library/Caches/ms-playwright"),
        home.join("AppData/Local/ms-playwright"),
    ];
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        roots.push(PathBuf::from(local).join("ms-playwright"));
    }
    roots
}

fn has_chromium(roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| {
        std::fs::read_dir(root).is_ok_and(|entries| {
            entries.flatten().any(|entry| {
                entry.file_name().to_string_lossy().starts_with("chromium") && entry.path().is_dir()
            })
        })
    })
}

fn check(spec: &Spec, project: &Path, roots: &[PathBuf], node: bool) -> serde_json::Value {
    let mut missing = Vec::new();
    if !node {
        missing.push("node and npm on PATH (install Node.js 18 or newer)".to_owned());
    }
    if !project
        .join("node_modules")
        .join(spec.package)
        .join("package.json")
        .is_file()
    {
        missing.push(format!("{} in the project's node_modules", spec.package));
    }
    // Both runners drive Playwright's Chromium build.
    if !has_chromium(roots) {
        missing.push("a Playwright Chromium build".to_owned());
    }
    if std::fs::read(script_path(spec, project)).ok().as_deref() != Some(spec.script.as_bytes()) {
        missing.push(format!(
            "{SCRIPT_DIR}/{} (this GraphHelm version)",
            spec.script_name
        ));
    }
    if spec.id == "playwright" {
        use std::io::Read;
        let mut bytes = Vec::new();
        let matches = std::fs::File::open(project.join(SCRIPT_DIR).join(DRIVER_NAME))
            .and_then(|file| file.take(DRIVER.len() as u64 + 1).read_to_end(&mut bytes))
            .is_ok()
            && bytes == DRIVER;
        if !matches {
            missing.push(format!(
                "{SCRIPT_DIR}/{DRIVER_NAME} (graphhelm-journey-driver/1, this GraphHelm version)"
            ));
        }
    }
    serde_json::json!({
        "id": spec.id,
        "status": if missing.is_empty() { "ready" } else { "missing" },
        "missing": missing,
        "install": spec.install,
        "installFlag": format!("--install-observer {}", spec.id),
        "runner": spec.runner,
        "note": spec.note,
    })
}

/// The `observers` section of the setup preview. Reads only.
pub(super) fn readiness(project: &Path, home: &Path) -> serde_json::Value {
    let node = on_path("node") && on_path("npm");
    let roots = browser_roots(project, home);
    let browser = check(&PLAYWRIGHT, project, &roots, node);
    let summary = if browser["status"] == "ready" {
        "The browser journey observer (Playwright) is ready."
    } else {
        "No browser journey observer is ready: browser journeys end OBSERVER_MISSING. Run setup with --install-observer playwright, or the install commands yourself."
    };
    serde_json::json!({
        "browser": browser,
        "optional": [check(&E2E, project, &roots, node)],
        "summary": summary,
    })
}

fn script_path(spec: &Spec, project: &Path) -> PathBuf {
    project.join(SCRIPT_DIR).join(spec.script_name)
}

/// Writes the shipped observer script into the project, replacing an older copy.
fn write_script(spec: &Spec, project: &Path) -> std::io::Result<()> {
    let path = script_path(spec, project);
    std::fs::create_dir_all(project.join(SCRIPT_DIR))?;
    std::fs::write(path, spec.script)?;
    if spec.id == "playwright" {
        std::fs::write(project.join(SCRIPT_DIR).join(DRIVER_NAME), DRIVER)?;
    }
    Ok(())
}

/// What running one install step came to.
enum Step {
    Exited(Option<i32>),
    /// The step outlived the deadline and was killed, with its process tree.
    TimedOut,
}

fn run_step(
    command: &str,
    project: &Path,
    deadline: Option<std::time::Instant>,
) -> std::io::Result<Step> {
    let mut words = command.split_whitespace();
    let program = words.next().unwrap_or_default();
    let mut process = if cfg!(windows) {
        let mut process = std::process::Command::new("cmd");
        process.args(["/C", program]);
        process
    } else {
        std::process::Command::new(program)
    };
    // The child's output would corrupt the JSON on stdout; it goes to stderr for the owner.
    let mut child = process
        .args(words)
        .current_dir(project)
        .stdin(std::process::Stdio::null())
        .stdout(std::io::stderr())
        .spawn()?;
    let Some(deadline) = deadline else {
        return Ok(Step::Exited(child.wait()?.code()));
    };
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Step::Exited(status.code()));
        }
        if std::time::Instant::now() >= deadline {
            kill_tree(&mut child);
            return Ok(Step::TimedOut);
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

/// `cmd /C npm` leaves npm and node as children of `cmd`; killing `cmd` alone would orphan them.
fn kill_tree(child: &mut std::process::Child) {
    if cfg!(windows) {
        let _ = std::process::Command::new("taskkill")
            .args(["/T", "/F", "/PID", &child.id().to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Runs each requested observer's install commands in `project`, stopping at the first failure,
/// then re-reads readiness. Returns the steps run and whether all succeeded.
pub(super) fn install(
    kinds: &[ObserverKind],
    project: &Path,
    home: &Path,
) -> (serde_json::Value, bool) {
    let (data, outcome) = install_until(kinds, project, home, None);
    (data, matches!(outcome, Installed::Ok))
}

/// How an install ended.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Installed {
    Ok,
    Failed,
    TimedOut,
}

/// The files an install may write in the project, reported back to the owner by name (#519):
/// the button says up front that it edits `package.json`, and the result says what it did.
const TOUCHED: [&str; 5] = [
    "package.json",
    "package-lock.json",
    "node_modules/@playwright/test/package.json",
    ".graphhelm/observers/journey_driver.mjs",
    ".graphhelm/observers/playwright_observe.py",
];

fn snapshot(project: &Path) -> Snapshot {
    TOUCHED
        .iter()
        .map(|path| {
            std::fs::metadata(project.join(path))
                .ok()
                .map(|meta| (meta.len(), meta.modified().ok()))
        })
        .collect()
}

/// The Studio's one-button setup (#519): exactly `setup --install-observer playwright`, bounded
/// by `budget`, and naming each file it created or changed. No caller input reaches the commands.
pub(crate) fn install_playwright_bounded(
    project: &Path,
    home: &Path,
    budget: std::time::Duration,
) -> (serde_json::Value, Installed) {
    let before = snapshot(project);
    let (mut data, outcome) = install_until(
        &[ObserverKind::Playwright],
        project,
        home,
        Some(std::time::Instant::now() + budget),
    );
    data["changed"] = changed(&before, &snapshot(project)).into();
    (data, outcome)
}

type Snapshot = Vec<Option<(u64, Option<std::time::SystemTime>)>>;

/// Each `TOUCHED` file that appeared or changed between two snapshots.
fn changed(before: &Snapshot, after: &Snapshot) -> Vec<serde_json::Value> {
    TOUCHED
        .iter()
        .zip(before.iter().zip(after))
        .filter_map(|(path, (old, new))| match (old, new) {
            (None, Some(_)) => Some(serde_json::json!({"path": path, "change": "created"})),
            (Some(_), Some(_)) if old != new => {
                Some(serde_json::json!({"path": path, "change": "modified"}))
            }
            _ => None,
        })
        .collect()
}

fn install_until(
    kinds: &[ObserverKind],
    project: &Path,
    home: &Path,
    deadline: Option<std::time::Instant>,
) -> (serde_json::Value, Installed) {
    let mut steps = Vec::new();
    let mut outcome = Installed::Ok;
    let mut seen = Vec::new();
    'outer: for kind in kinds {
        let spec = spec(*kind);
        if seen.contains(&spec.id) {
            continue;
        }
        seen.push(spec.id);
        let written = write_script(spec, project);
        steps.push(serde_json::json!({
            "observer": spec.id,
            "wrote": format!("{SCRIPT_DIR}/{}", spec.script_name),
            "error": written.as_ref().err().map(ToString::to_string),
        }));
        if written.is_err() {
            outcome = Installed::Failed;
            break;
        }
        for command in spec.install {
            let (exit, error, timed_out) = match run_step(command, project, deadline) {
                Ok(Step::Exited(code)) => (code, None, false),
                Ok(Step::TimedOut) => (None, Some("timed out".to_owned()), true),
                Err(error) => (None, Some(error.to_string()), false),
            };
            steps.push(serde_json::json!({
                "observer": spec.id, "command": command, "exitCode": exit, "error": error,
            }));
            if timed_out {
                outcome = Installed::TimedOut;
                break 'outer;
            }
            if exit != Some(0) {
                outcome = Installed::Failed;
                break 'outer;
            }
        }
    }
    (
        serde_json::json!({
            "installed": steps,
            "observers": readiness(project, home),
        }),
        outcome,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir().expect("tempdir");
        let project = root.path().join("project");
        let home = root.path().join("home");
        std::fs::create_dir_all(&project).expect("project");
        std::fs::create_dir_all(&home).expect("home");
        (root, project, home)
    }

    #[test]
    fn a_step_past_its_deadline_is_killed_and_reported_as_timed_out() {
        let (_root, project, _home) = fixture();
        let slow = if cfg!(windows) {
            "ping -n 30 127.0.0.1"
        } else {
            "sleep 30"
        };
        let started = std::time::Instant::now();
        let step = run_step(
            slow,
            &project,
            Some(started + std::time::Duration::from_secs(1)),
        )
        .expect("spawned");
        assert!(matches!(step, Step::TimedOut));
        assert!(
            started.elapsed() < std::time::Duration::from_secs(15),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn the_result_names_each_file_the_install_created_or_changed() {
        let (_root, project, _home) = fixture();
        std::fs::write(project.join("package.json"), "{}").expect("manifest");
        let before = snapshot(&project);
        write_script(&PLAYWRIGHT, &project).expect("script");
        std::fs::write(project.join("package.json"), r#"{"devDependencies":{}}"#).expect("edit");
        let changed = changed(&before, &snapshot(&project));
        assert_eq!(
            serde_json::Value::from(changed),
            serde_json::json!([
                {"path": "package.json", "change": "modified"},
                {"path": ".graphhelm/observers/journey_driver.mjs", "change": "created"},
                {"path": ".graphhelm/observers/playwright_observe.py", "change": "created"},
            ])
        );
        assert!(
            PLAYWRIGHT.install[0].ends_with("@playwright/test@1.64.0"),
            "unpinned"
        );
    }

    #[test]
    fn an_empty_project_names_each_missing_piece_and_the_commands() {
        let (_root, project, home) = fixture();
        let browser = check(&PLAYWRIGHT, &project, &[home.join("cache")], false);
        assert_eq!(browser["status"], "missing");
        let missing = browser["missing"].as_array().expect("missing");
        assert_eq!(missing.len(), 5, "{missing:?}");
        assert!(missing.iter().any(|entry| {
            entry
                .as_str()
                .is_some_and(|s| s.contains("journey_driver.mjs"))
        }));
        assert_eq!(browser["install"][1], "npx playwright install chromium");
        assert_eq!(browser["installFlag"], "--install-observer playwright");
    }

    #[test]
    fn package_plus_chromium_plus_node_is_ready() {
        let (_root, project, home) = fixture();
        let package = project.join("node_modules/@playwright/test");
        std::fs::create_dir_all(&package).expect("package");
        std::fs::write(package.join("package.json"), "{}").expect("manifest");
        write_script(&PLAYWRIGHT, &project).expect("script");
        let roots = [home.join("cache")];
        std::fs::create_dir_all(roots[0].join("chromium-1200")).expect("chromium");
        let browser = check(&PLAYWRIGHT, &project, &roots, true);
        assert_eq!(browser["status"], "ready", "{browser}");
        // e2e is a different package, so it stays missing: it is opt-in.
        assert_eq!(check(&E2E, &project, &roots, true)["status"], "missing");
    }

    #[test]
    fn the_shipped_script_lands_in_the_project_and_a_stale_copy_is_missing() {
        let (_root, project, _home) = fixture();
        write_script(&PLAYWRIGHT, &project).expect("script");
        let written =
            std::fs::read_to_string(project.join(SCRIPT_DIR).join("playwright_observe.py"))
                .expect("written");
        assert!(written.contains("def observe("));
        assert_eq!(written, PLAYWRIGHT.script);
        // Actual installed artifact, not source spelling: the companion must
        // match this binary and a stale companion must change read-only preview.
        let driver_path = project.join(SCRIPT_DIR).join("journey_driver.mjs");
        assert_eq!(
            std::fs::read(&driver_path).unwrap(),
            include_bytes!("../../../../tools/journey-driver/driver.mjs")
        );
        std::fs::write(&driver_path, "old driver").unwrap();
        let stale_driver = check(&PLAYWRIGHT, &project, &[], true);
        assert!(
            stale_driver["missing"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry
                    .as_str()
                    .is_some_and(|s| s.contains("journey_driver.mjs")))
        );
        assert_eq!(
            std::fs::read_to_string(&driver_path).unwrap(),
            "old driver",
            "preview modified the observer"
        );
        std::fs::write(script_path(&PLAYWRIGHT, &project), "old").expect("stale");
        let browser = check(&PLAYWRIGHT, &project, &[], true);
        assert!(
            browser["missing"]
                .to_string()
                .contains("playwright_observe.py"),
            "{browser}"
        );
    }
}
