//! Browser journey observers for `graphhelm setup`: whether one is ready in the project, the exact
//! commands that make it ready, and installation for setup and journey Run.
//!
//! Playwright is the default observer: deterministic, no model, no AI cost
//! (`tools/playwright-observer`). `e2e` (tester.army) is opt-in: it needs a model key for agent
//! steps (`tools/e2e-observer`). Readiness reads files and PATH only.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

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
/// The driver imports `./judge.mjs`, so it ships beside it.
const JUDGE_NAME: &str = "judge.mjs";
const JUDGE: &[u8] = include_bytes!("../../../../tools/journey-driver/judge.mjs");

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
    let local = project.join("node_modules/playwright-core/.local-browsers");
    if let Some(path) = std::env::var_os("PLAYWRIGHT_BROWSERS_PATH") {
        if path == "0" {
            return vec![local];
        }
        return vec![PathBuf::from(path), local];
    }
    let mut roots = vec![
        local,
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
        if std::fs::read(project.join(SCRIPT_DIR).join(JUDGE_NAME))
            .ok()
            .as_deref()
            != Some(JUDGE)
        {
            missing.push(format!(
                "{SCRIPT_DIR}/{JUDGE_NAME} (this GraphHelm version)"
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

fn safe_observer_target(project: &Path) -> std::io::Result<()> {
    let root = project.canonicalize()?;
    for relative in [
        ".graphhelm",
        SCRIPT_DIR,
        ".graphhelm/npm-cache",
        "node_modules",
        "node_modules/@playwright",
        "node_modules/@playwright/test",
        "node_modules/playwright",
        "node_modules/playwright-core",
        "node_modules/playwright-core/.local-browsers",
    ] {
        let path = project.join(relative);
        if let Ok(meta) = std::fs::symlink_metadata(&path) {
            if meta.file_type().is_symlink()
                || graphhelm_process::fs::is_reparse_point(&meta)
                || !meta.is_dir()
            {
                return Err(std::io::Error::other("unsafe observer target"));
            }
            if path.canonicalize()?.strip_prefix(&root).is_err() {
                return Err(std::io::Error::other("observer target escapes project"));
            }
        }
    }
    for relative in [
        "package.json",
        "package-lock.json",
        ".graphhelm/observer-install.lock",
    ] {
        if let Ok(meta) = std::fs::symlink_metadata(project.join(relative))
            && (meta.file_type().is_symlink()
                || graphhelm_process::fs::is_reparse_point(&meta)
                || !meta.is_file())
        {
            return Err(std::io::Error::other(format!(
                "unsafe install target: {relative}"
            )));
        }
    }
    Ok(())
}

fn write_script(spec: &Spec, project: &Path) -> std::io::Result<()> {
    safe_observer_target(project)?;
    let path = script_path(spec, project);
    std::fs::create_dir_all(project.join(SCRIPT_DIR))?;
    safe_observer_target(project)?;
    super::atomic::atomic_write(&path, spec.script.as_bytes())?;
    if spec.id == "playwright" {
        super::atomic::atomic_write(&project.join(SCRIPT_DIR).join(DRIVER_NAME), DRIVER)?;
        super::atomic::atomic_write(&project.join(SCRIPT_DIR).join(JUDGE_NAME), JUDGE)?;
    }
    Ok(())
}

/// What running one install step came to.
enum Step {
    Exited(Option<i32>, String),
    /// The step outlived the deadline and was killed, with its process tree.
    TimedOut,
}

fn run_step(
    command: &str,
    project: &Path,
    deadline: Option<std::time::Instant>,
) -> std::io::Result<Step> {
    let deadline = deadline.unwrap_or_else(|| Instant::now() + Duration::from_secs(900));
    if Instant::now() >= deadline {
        return Ok(Step::TimedOut);
    }
    let mut words = command.split_whitespace();
    let program = words.next().unwrap_or_default();
    let mut process = if cfg!(windows) {
        let mut process = graphhelm_process::command("cmd");
        process.args(["/C", program]);
        process
    } else {
        graphhelm_process::command(program)
    };
    // Keep dependency and browser writes in the selected project, including npm workspaces.
    let cache = project.join(".graphhelm/npm-cache");
    safe_observer_target(project)?;
    std::fs::create_dir_all(&cache)?;
    if std::fs::symlink_metadata(&cache).is_ok_and(|meta| {
        meta.file_type().is_symlink()
            || graphhelm_process::fs::is_reparse_point(&meta)
            || !meta.is_dir()
    }) {
        return Err(std::io::Error::other("unsafe npm cache target"));
    }
    let mut child = process
        .args(words)
        .current_dir(project)
        .env("PLAYWRIGHT_BROWSERS_PATH", "0")
        .env("npm_config_cache", &cache)
        .env("npm_config_prefix", project)
        .env("npm_config_workspaces", "false")
        .env("npm_config_ignore_scripts", "true")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    let stderr = child.stderr.take();
    let secrets: Vec<String> = std::env::vars()
        .filter(|(key, _)| {
            key.starts_with("GRAPHHELM_SECRET_")
                || key.ends_with("_TOKEN")
                || key.ends_with("_KEY")
                || key.ends_with("_PASSWORD")
        })
        .map(|(_, value)| value)
        .collect();
    let slack = secrets.iter().map(String::len).max().unwrap_or_default();
    let (stderr_tx, stderr_rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut tail = Vec::new();
        let mut cut = false;
        if let Some(mut stderr) = stderr {
            let mut chunk = [0_u8; 1024];
            loop {
                match stderr.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(size) => {
                        tail.extend_from_slice(&chunk[..size]);
                        let max = 4096 + slack;
                        if tail.len() > max {
                            tail.drain(..tail.len() - max);
                            cut = true;
                        }
                    }
                }
            }
        }
        let _ = stderr_tx.send(
            super::journey_replay::masked_tail(&tail, cut, slack, &secrets)
                .trim()
                .to_owned(),
        );
    });
    loop {
        if let Some(status) = child.try_wait()? {
            let remaining = deadline.saturating_duration_since(Instant::now());
            return match stderr_rx.recv_timeout(remaining) {
                Ok(detail) => Ok(Step::Exited(status.code(), detail)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Ok(Step::TimedOut),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    Ok(Step::Exited(status.code(), String::new()))
                }
            };
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
        let _ = graphhelm_process::command("taskkill")
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
const TOUCHED: [&str; 6] = [
    "package.json",
    "package-lock.json",
    "node_modules/@playwright/test/package.json",
    ".graphhelm/observers/journey_driver.mjs",
    ".graphhelm/observers/judge.mjs",
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
    let deadline = deadline.unwrap_or_else(|| Instant::now() + Duration::from_secs(900));
    let _lock = match observer_install_lock(project, deadline) {
        Ok(lock) => lock,
        Err(error) => {
            return (
                serde_json::json!({"installed":[{"error":error.to_string()}]}),
                Installed::Failed,
            );
        }
    };
    install_until_progress(kinds, project, home, Some(deadline), |_| {}, run_step)
}

fn install_until_progress(
    kinds: &[ObserverKind],
    project: &Path,
    home: &Path,
    deadline: Option<Instant>,
    mut progress: impl FnMut(&str),
    mut run: impl FnMut(&str, &Path, Option<Instant>) -> std::io::Result<Step>,
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
        for command in spec.install {
            progress(command);
            let (exit, error, timed_out, detail) = match run(command, project, deadline) {
                Ok(Step::Exited(code, detail)) => (code, None, false, detail),
                Ok(Step::TimedOut) => (None, Some("timed out".to_owned()), true, String::new()),
                Err(error) => (None, Some(error.to_string()), false, String::new()),
            };
            steps.push(serde_json::json!({
                "observer": spec.id, "command": command, "exitCode": exit, "error": error,
                "detail": if detail.is_empty() { serde_json::Value::Null } else { detail.into() },
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
        {
            progress("Writing the current browser observer");
            if let Err(error) = write_script(spec, project) {
                steps.push(serde_json::json!({"observer": spec.id, "error": error.to_string()}));
                outcome = Installed::Failed;
                break;
            }
            steps.push(serde_json::json!({
                "observer": spec.id,
                "wrote": format!("{SCRIPT_DIR}/{}", spec.script_name),
            }));
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

fn observer_install_lock(project: &Path, deadline: Instant) -> std::io::Result<std::fs::File> {
    use fs2::FileExt;
    safe_observer_target(project)?;
    std::fs::create_dir_all(project.join(".graphhelm"))?;
    safe_observer_target(project)?;
    let path = project.join(".graphhelm/observer-install.lock");
    if std::fs::symlink_metadata(&path).is_ok_and(|meta| {
        meta.file_type().is_symlink() || graphhelm_process::fs::is_reparse_point(&meta)
    }) {
        return Err(std::io::Error::other("unsafe observer lock"));
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(file),
            Err(_error) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25))
            }
            Err(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "timed out waiting for observer installation",
                ));
            }
        }
    }
}

/// Ensures the Playwright journey observer is ready, serializing setup and preserving failed
/// installs for a safe retry. The progress callback is emitted before each external stage.
pub(super) fn ensure_playwright(
    project: &Path,
    deadline: Instant,
    progress: impl FnMut(&str),
) -> Result<serde_json::Value, serde_json::Value> {
    ensure_playwright_with(project, deadline, progress, run_step)
}

fn ensure_playwright_with(
    project: &Path,
    deadline: Instant,
    mut progress: impl FnMut(&str),
    run: impl FnMut(&str, &Path, Option<Instant>) -> std::io::Result<Step>,
) -> Result<serde_json::Value, serde_json::Value> {
    if super::journey_replay::observer_ready(project).is_ok() {
        return Ok(serde_json::json!({"status":"ready", "steps":[]}));
    }
    let install_deadline = deadline
        .checked_sub(Duration::from_secs(10))
        .unwrap_or(deadline);
    progress("Preparing the browser observer");
    let _lock = observer_install_lock(project, install_deadline).map_err(
        |error| serde_json::json!({"code":if error.kind() == std::io::ErrorKind::TimedOut {"observer.setup_timeout"} else {"observer.setup_failed"}, "message":error.to_string()}),
    )?;
    if super::journey_replay::observer_ready(project).is_ok() {
        return Ok(serde_json::json!({"status":"ready", "steps":[]}));
    }
    let (data, outcome) = install_until_progress(
        &[ObserverKind::Playwright],
        project,
        project,
        Some(install_deadline),
        &mut progress,
        run,
    );
    if matches!(outcome, Installed::Ok) {
        Ok(data)
    } else {
        let steps = data
            .get("installed")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]));
        let timed_out = matches!(outcome, Installed::TimedOut);
        let failed = steps.as_array().and_then(|items| items.last());
        let command = failed
            .and_then(|step| step.get("command"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("observer install");
        let detail = failed
            .and_then(|step| step.get("detail"))
            .and_then(serde_json::Value::as_str)
            .filter(|detail| !detail.is_empty())
            .map(|detail| format!(": {detail}"))
            .unwrap_or_default();
        let error = failed
            .and_then(|step| step.get("error"))
            .and_then(serde_json::Value::as_str)
            .filter(|error| !error.is_empty())
            .map(|error| format!(": {error}"))
            .unwrap_or_default();
        let exit = failed
            .and_then(|step| step["exitCode"].as_i64())
            .map(|code| format!(" (exit {code})"))
            .unwrap_or_default();
        Err(serde_json::json!({
            "code": if timed_out { "observer.setup_timeout" } else { "observer.setup_failed" },
            "message": if timed_out { format!("timed out: {command}") } else { format!("failed: {command}{exit}{error}{detail}") },
            "steps": steps,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_refreshes_missing_and_stale_observers_and_skips_a_current_copy() {
        let (_root, project, _home) = fixture();
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut commands = Vec::new();
        let mut stages = Vec::new();
        ensure_playwright_with(
            &project,
            deadline,
            |stage| stages.push(stage.to_owned()),
            |command, cwd, _| {
                assert_eq!(cwd, project);
                commands.push(command.to_owned());
                Ok(Step::Exited(Some(0), String::new()))
            },
        )
        .unwrap();
        assert_eq!(commands, PLAYWRIGHT.install);
        assert!(stages.iter().any(|stage| stage == PLAYWRIGHT.install[1]));
        assert!(super::super::journey_replay::observer_ready(&project).is_ok());
        ensure_playwright_with(
            &project,
            deadline,
            |_| panic!("current observer reported installation"),
            |_, _, _| panic!("current observer reinstalled"),
        )
        .unwrap();
        std::fs::write(project.join(SCRIPT_DIR).join(JUDGE_NAME), "old judge").unwrap();
        commands.clear();
        ensure_playwright_with(
            &project,
            deadline,
            |_| {},
            |command, _, _| {
                commands.push(command.to_owned());
                Ok(Step::Exited(Some(0), String::new()))
            },
        )
        .unwrap();
        assert_eq!(commands, PLAYWRIGHT.install);
        assert_eq!(
            std::fs::read(project.join(SCRIPT_DIR).join(JUDGE_NAME)).unwrap(),
            JUDGE
        );
    }

    #[test]
    fn a_failed_install_preserves_stale_scripts_and_retries_with_the_exact_failure() {
        let (_root, project, _home) = fixture();
        std::fs::create_dir_all(project.join(SCRIPT_DIR)).unwrap();
        let driver = project.join(SCRIPT_DIR).join(DRIVER_NAME);
        std::fs::write(&driver, "old driver").unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        let failed = ensure_playwright_with(
            &project,
            deadline,
            |_| {},
            |_, _, _| {
                Ok(Step::Exited(
                    Some(23),
                    "fixture registry unavailable".into(),
                ))
            },
        )
        .unwrap_err();
        assert_eq!(failed["code"], "observer.setup_failed");
        assert_eq!(
            failed["message"],
            format!(
                "failed: {} (exit 23): fixture registry unavailable",
                PLAYWRIGHT.install[0]
            )
        );
        assert_eq!(std::fs::read_to_string(&driver).unwrap(), "old driver");
        ensure_playwright_with(
            &project,
            deadline,
            |_| {},
            |_, _, _| Ok(Step::Exited(Some(0), String::new())),
        )
        .unwrap();
        assert!(super::super::journey_replay::observer_ready(&project).is_ok());
    }

    #[test]
    fn a_setup_timeout_names_the_command_and_does_not_publish_scripts() {
        let (_root, project, _home) = fixture();
        let failed = ensure_playwright_with(
            &project,
            Instant::now() + Duration::from_secs(60),
            |_| {},
            |_, _, _| Ok(Step::TimedOut),
        )
        .unwrap_err();
        assert_eq!(failed["code"], "observer.setup_timeout");
        assert_eq!(
            failed["message"],
            format!("timed out: {}", PLAYWRIGHT.install[0])
        );
        assert!(!project.join(SCRIPT_DIR).join(DRIVER_NAME).exists());
    }

    #[test]
    fn concurrent_setup_waits_with_a_deadline_and_never_starts_commands() {
        let (_root, project, _home) = fixture();
        let held =
            observer_install_lock(&project, Instant::now() + Duration::from_secs(60)).unwrap();
        let failed = ensure_playwright_with(
            &project,
            Instant::now(),
            |_| {},
            |_, _, _| panic!("concurrent install started"),
        )
        .unwrap_err();
        assert_eq!(failed["code"], "observer.setup_timeout");
        drop(held);
        ensure_playwright_with(
            &project,
            Instant::now() + Duration::from_secs(60),
            |_| {},
            |_, _, _| Ok(Step::Exited(Some(0), String::new())),
        )
        .unwrap();
    }

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
                {"path": ".graphhelm/observers/judge.mjs", "change": "created"},
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
        assert_eq!(missing.len(), 6, "{missing:?}");
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

    #[cfg(unix)]
    #[test]
    fn observer_scripts_refuse_a_symlinked_destination() {
        let (_root, project, _home) = fixture();
        let outside = tempfile::tempdir().expect("outside");
        std::os::unix::fs::symlink(outside.path(), project.join(".graphhelm")).expect("symlink");
        let error = write_script(&PLAYWRIGHT, &project).expect_err("unsafe path accepted");
        assert!(error.to_string().contains("unsafe observer target"));
    }
}
