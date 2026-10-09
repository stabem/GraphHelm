//! `graphhelm studio start`: one command that opens an up-to-date Studio for this project.
//!
//! WHY THIS EXISTS (2026-10-05). The Studio runs from a GraphHelm clone (`apps/studio`, a Vite dev
//! server started by `apps/studio/tools/studio-up.ps1`). An owner kept opening a Studio from a
//! clone last pulled days earlier and saw none of the merged Studio work: the rail he was told was
//! fixed still looked the same. Nothing told him the clone was behind. This command finds the
//! clone, fast-forwards it when that is safe, reinstalls the Studio's dependencies when the lock
//! file moved, and then hands off to `studio-up.ps1`, which already owns starting the Runtime and
//! the dev server.
//!
//! THE UPDATE IS CONSERVATIVE. The clone is moved only by `git pull --ff-only origin main`, and
//! only when it is on `main` with a clean work tree; anything else (a feature branch, local edits,
//! a diverged `main`) is left exactly as it is and reported, never stashed, reset or merged.
//!
//! Progress goes to stderr; stdout carries only the command's envelope, as for every command.

use std::path::{Path, PathBuf};
use std::process::Command;

use graphhelm_protocols::Diagnostic;
use serde_json::json;

use crate::args::{StudioStartArgs, UpdateArgs};
use crate::commands::gateway::keyring::SEALING_KEY_ENVIRONMENT;

/// The broker passphrase `serve` reads for model routes (`serve/ports.rs::gateway_passphrase`).
const GATEWAY_KEY_ENVIRONMENT: &str = "GRAPHHELM_GATEWAY_KEY";
use crate::output::Outcome;

const COMMAND: &str = "studio";
const LAUNCHER: &str = "apps/studio/tools/studio-up.ps1";
const LOCKFILE: &str = "apps/studio/package-lock.json";
/// The clone this binary was built from: `apps/cli` is two levels below the repository root.
const BUILT_FROM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn refused(message: impl Into<String>, pointer: &str) -> Outcome {
    refused_as(COMMAND, message, pointer)
}

fn refused_as(command: &'static str, message: impl Into<String>, pointer: &str) -> Outcome {
    Outcome::domain(
        command,
        vec![Diagnostic::error(
            crate::error_codes::GHCLI032_STUDIO_REFUSED,
            message,
            pointer,
            "studio-cli",
        )],
    )
}

/// The first candidate that holds the Studio launcher: `--source`, then `GRAPHHELM_SOURCE`, then
/// the clone this binary was built from.
pub(super) fn resolve_source(candidates: &[Option<PathBuf>]) -> Option<PathBuf> {
    candidates
        .iter()
        .flatten()
        .find(|candidate| candidate.join(LAUNCHER).is_file())
        .map(|found| plain(std::fs::canonicalize(found).unwrap_or_else(|_| found.clone())))
}

/// Drops Windows' verbatim prefix (`\\\\?\\F:\\...`) that `canonicalize` adds: PowerShell's
/// `Split-Path`/`Join-Path` cannot read it, and `studio-up.ps1` failed on it (2026-10-05).
pub(super) fn plain(path: PathBuf) -> PathBuf {
    match path.to_str().and_then(|text| text.strip_prefix(r"\\?\")) {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => path,
    }
}

/// The first enabled route in the project's manifest that can write text: what `serve --route`
/// needs (cognitive nodes and suggested replies draft on it). A `typesafe` route only judges, so
/// it is never picked; it is still listed, because `serve` reads the whole manifest.
pub(super) fn text_route(manifest: &Path) -> Option<String> {
    let document: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(manifest).ok()?).ok()?;
    document["routes"].as_array()?.iter().find_map(|route| {
        let enabled = route["enabled"].as_bool().unwrap_or(true);
        (enabled && route["provider"].as_str() != Some("typesafe"))
            .then(|| route["id"].as_str().map(str::to_owned))
            .flatten()
    })
}

/// What the clone's state allows. Pure, so the decision is tested without git.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Freshness {
    UpToDate,
    /// Behind `origin/main` by this many commits, on `main`, clean: safe to fast-forward.
    FastForward(u64),
    /// Behind, but the clone must not be moved; the reason is said to the operator.
    Stale {
        behind: u64,
        reason: &'static str,
    },
}

pub(super) fn freshness(branch: &str, clean: bool, behind: u64, ahead: u64) -> Freshness {
    if behind == 0 {
        return Freshness::UpToDate;
    }
    let reason = if branch != "main" {
        "the clone is not on main"
    } else if !clean {
        "the clone has local changes"
    } else if ahead > 0 {
        "the clone's main has commits origin/main does not"
    } else {
        return Freshness::FastForward(behind);
    };
    Freshness::Stale { behind, reason }
}

fn git(source: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(source)
        .args(arguments)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// Brings the clone up to `origin/main` when [`freshness`] allows it. Returns what happened, for
/// the envelope; a clone git cannot read (no git, no remote, offline) is run as it is.
fn update(source: &Path) -> serde_json::Value {
    if git(source, &["fetch", "--quiet", "origin", "main"]).is_none() {
        eprintln!("[studio] could not fetch origin/main; running the clone as it is");
        return json!({ "state": "unchecked" });
    }
    let branch = git(source, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
    let clean = git(source, &["status", "--porcelain", "--untracked-files=no"])
        .is_some_and(|status| status.is_empty());
    let counts = git(
        source,
        &["rev-list", "--left-right", "--count", "HEAD...origin/main"],
    )
    .unwrap_or_default();
    let mut counts = counts
        .split_whitespace()
        .map(|n| n.parse::<u64>().unwrap_or(0));
    let (ahead, behind) = (counts.next().unwrap_or(0), counts.next().unwrap_or(0));
    match freshness(&branch, clean, behind, ahead) {
        Freshness::UpToDate => json!({ "state": "up_to_date" }),
        Freshness::Stale { behind, reason } => {
            eprintln!(
                "[studio] WARNING: this Studio is {behind} commit(s) behind origin/main and was NOT updated: {reason} ({})",
                source.display()
            );
            json!({ "state": "stale", "behind": behind, "reason": reason })
        }
        Freshness::FastForward(behind) => {
            let before = git(source, &["rev-parse", "HEAD"]).unwrap_or_default();
            eprintln!(
                "[studio] updating {} ({behind} new commit(s) on main)",
                source.display()
            );
            if git(source, &["pull", "--ff-only", "--quiet", "origin", "main"]).is_none() {
                eprintln!("[studio] WARNING: the fast-forward failed; running the clone as it is");
                return json!({ "state": "stale", "behind": behind, "reason": "the fast-forward failed" });
            }
            let lock_moved = git(
                source,
                &["diff", "--quiet", &before, "HEAD", "--", LOCKFILE],
            )
            .is_none();
            if lock_moved {
                eprintln!("[studio] the Studio's dependencies changed; running npm ci");
                let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
                let installed = Command::new(npm)
                    .args(["ci", "--prefix"])
                    .arg(source.join("apps/studio"))
                    .status()
                    .is_ok_and(|status| status.success());
                if !installed {
                    eprintln!("[studio] WARNING: npm ci failed; the Studio may not start");
                }
            }
            json!({ "state": "updated", "commits": behind, "dependenciesReinstalled": lock_moved })
        }
    }
}

/// `graphhelm update`: the clone first (same conservative fast-forward as `studio start`), then
/// `cargo install --locked --path apps/cli` from it, run inside the clone so its pinned toolchain
/// applies. On Windows a running `graphhelm.exe` cannot be overwritten but can be renamed, so the
/// running binary is moved aside to `graphhelm.old.exe` first and moved back if the install fails.
pub fn update_cli(args: &UpdateArgs) -> Outcome {
    const UPDATE: &str = "update";
    let candidates = [
        args.source.clone(),
        std::env::var_os("GRAPHHELM_SOURCE").map(PathBuf::from),
        Some(PathBuf::from(BUILT_FROM)),
    ];
    let Some(source) = resolve_source(&candidates) else {
        return refused_as(
            UPDATE,
            format!(
                "no GraphHelm clone with {LAUNCHER} was found; pass --source <your GraphHelm clone> or set GRAPHHELM_SOURCE"
            ),
            "/source",
        );
    };
    eprintln!("[update] GraphHelm source: {}", source.display());
    let clone = update(&source);
    let moved_aside = if cfg!(windows) {
        std::env::current_exe().ok().and_then(|exe| {
            let old = exe.with_file_name("graphhelm.old.exe");
            let _ = std::fs::remove_file(&old);
            std::fs::rename(&exe, &old).ok().map(|()| (exe, old))
        })
    } else {
        None
    };
    // Install over THIS binary, wherever it lives: a graphhelm run from a versioned directory
    // (`~/.graphhelm/versions/<sha>/bin`) updated into `~/.cargo/bin` would leave PATH on the old
    // one, and on Windows with nothing at all once it was moved aside (2026-10-05).
    let root = install_root(std::env::current_exe().ok().as_deref());
    eprintln!("[update] installing the CLI (cargo install --locked --path apps/cli)");
    let mut install = Command::new("cargo");
    install
        .current_dir(&source)
        .args(["install", "--locked", "--path", "apps/cli"]);
    if let Some(root) = &root {
        eprintln!("[update] into {}", root.join("bin").display());
        // `--force`: cargo refuses to overwrite a binary it did not install itself, and a
        // versioned directory's binary was placed there by another installer (#281 review).
        install.arg("--root").arg(root).arg("--force");
    }
    let installed = install.status().is_ok_and(|status| status.success());
    if !installed {
        if let Some((exe, old)) = &moved_aside {
            let _ = std::fs::rename(old, exe);
        }
        return refused_as(
            UPDATE,
            "cargo install failed; its output is above, and the old graphhelm is unchanged",
            "/install",
        );
    }
    Outcome::success(
        UPDATE,
        json!({
            "source": source.display().to_string(),
            "head": git(&source, &["rev-parse", "--short", "HEAD"]),
            "update": clone,
            "installed": true,
        }),
    )
}

/// The `cargo install --root` that puts the new binary exactly where the running one is: the
/// parent of its `bin` directory. `None` (cargo's default root) when the binary is not in a `bin`.
pub(super) fn install_root(exe: Option<&Path>) -> Option<PathBuf> {
    let bin = exe?.parent()?;
    (bin.file_name()? == "bin").then(|| bin.parent().map(Path::to_path_buf))?
}

/// The project reaches the launcher twice: its folder name as the rail label, and its absolute
/// path as `-ProjectPath`, which the launcher forwards to `serve --project` (without it the
/// journeys route refuses, #331).
pub(super) fn add_project(launch: &mut Command, project: &Path) {
    if let Some(name) = project.file_name() {
        launch.arg("-Project").arg(name);
    }
    launch.arg("-ProjectPath").arg(plain(project.to_path_buf()));
}

pub fn start(args: &StudioStartArgs) -> Outcome {
    let project = match args.project.clone().map_or_else(std::env::current_dir, Ok) {
        Ok(project) => project,
        Err(_) => return refused("the current directory could not be read", "/project"),
    };
    let state = project.join(".graphhelm");
    let events = state.join("events");
    if !events.is_dir() {
        return refused(
            format!(
                "{} has no .graphhelm/events; run `graphhelm init` in the project first, or pass --project",
                project.display()
            ),
            "/project",
        );
    }
    let candidates = [
        args.source.clone(),
        std::env::var_os("GRAPHHELM_SOURCE").map(PathBuf::from),
        Some(PathBuf::from(BUILT_FROM)),
    ];
    let Some(source) = resolve_source(&candidates) else {
        return refused(
            format!(
                "no GraphHelm clone with {LAUNCHER} was found; pass --source <your GraphHelm clone> or set GRAPHHELM_SOURCE"
            ),
            "/source",
        );
    };
    eprintln!("[studio] Studio source: {}", source.display());
    let freshness = if args.no_update {
        json!({ "state": "skipped" })
    } else {
        update(&source)
    };
    let head = git(&source, &["rev-parse", "--short", "HEAD"]);

    let shell = if cfg!(windows) { "powershell" } else { "pwsh" };
    let mut launch = Command::new(shell);
    launch
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(source.join(LAUNCHER))
        .arg("-Events")
        .arg(&events)
        .args(["-Bind", &args.bind]);
    add_project(&mut launch, &project);
    if let Ok(exe) = std::env::current_exe() {
        launch.arg("-GraphHelm").arg(exe);
    }
    let keyring = state.join(super::init::KEYRING_DIRECTORY);
    let key = std::fs::read_to_string(state.join(super::init::KEY_FILE)).ok();
    let sealed = keyring.is_dir() && key.is_some();
    if let (true, Some(key)) = (keyring.is_dir(), key) {
        // The key travels in the child's environment only, as `init` prints it; never as an
        // argument, never printed.
        launch
            .env(SEALING_KEY_ENVIRONMENT, key.trim())
            .arg("-Keyring")
            .arg(&keyring)
            .args(["-KeyId", &args.key_id]);
    }
    // The model half, when `gateway setup` wired one: without it the Runtime lists no routes and
    // the Studio's suggested replies say no Jev model is set up.
    let manifest = state.join("manifest.json");
    let broker = state.join("broker");
    // Only beside the keyring: `serve` refuses a model half without `--keyring`/`--key-id`, so a
    // keyless project keeps opening read-and-drive instead of not opening at all (#282 review).
    if let (true, true, Some(route)) = (sealed, broker.is_dir(), text_route(&manifest)) {
        eprintln!(
            "[studio] model routes from {} (text route {route})",
            manifest.display()
        );
        // The broker is sealed with `serve.key` (`gateway setup` stores credentials with it), and
        // `serve` reads that passphrase from `GRAPHHELM_GATEWAY_KEY`: without it every route call,
        // Jev's suggested replies included, fails on the missing key.
        if let Ok(key) = std::fs::read_to_string(state.join(super::init::KEY_FILE)) {
            launch.env(GATEWAY_KEY_ENVIRONMENT, key.trim());
        }
        launch
            .arg("-Manifest")
            .arg(&manifest)
            .arg("-Broker")
            .arg(&broker)
            .args(["-Route", &route]);
    }
    if args.no_browser {
        launch.arg("-NoBrowser");
    }
    let launched = match launch.status() {
        Ok(status) => status.success(),
        Err(_) => {
            return refused(
                format!("{shell} could not be started; the Studio launcher is a PowerShell script"),
                "/shell",
            );
        }
    };
    let data = json!({
        "source": source.display().to_string(),
        "head": head,
        "update": freshness,
        "launcherSucceeded": launched,
    });
    if launched {
        Outcome::success(COMMAND, data)
    } else {
        refused(
            "the Studio launcher stopped with an error; its output is above",
            "/launcher",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_forwards_only_a_clean_main_that_is_strictly_behind() {
        assert_eq!(freshness("main", true, 0, 0), Freshness::UpToDate);
        assert_eq!(freshness("main", true, 7, 0), Freshness::FastForward(7));
        assert!(matches!(
            freshness("feature", true, 7, 0),
            Freshness::Stale { behind: 7, .. }
        ));
        assert!(matches!(
            freshness("main", false, 7, 0),
            Freshness::Stale { behind: 7, .. }
        ));
        assert!(matches!(
            freshness("main", true, 7, 1),
            Freshness::Stale { behind: 7, .. }
        ));
    }

    #[test]
    fn resolves_the_first_candidate_that_holds_the_launcher() {
        let empty = tempfile::tempdir().unwrap();
        let clone = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(clone.path().join("apps/studio/tools")).unwrap();
        std::fs::write(clone.path().join(LAUNCHER), "").unwrap();
        let found = resolve_source(&[None, Some(empty.path().into()), Some(clone.path().into())]);
        assert_eq!(
            found,
            Some(plain(std::fs::canonicalize(clone.path()).unwrap()))
        );
        assert_eq!(resolve_source(&[Some(empty.path().into())]), None);
    }

    #[test]
    fn installs_over_the_running_binary_wherever_its_bin_is() {
        let versioned = Path::new("/home/u/.graphhelm/versions/26b8780/bin/graphhelm");
        assert_eq!(
            install_root(Some(versioned)),
            Some(PathBuf::from("/home/u/.graphhelm/versions/26b8780"))
        );
        assert_eq!(
            install_root(Some(Path::new("/repo/target/debug/graphhelm"))),
            None
        );
        assert_eq!(install_root(None), None);
    }

    #[test]
    fn drops_the_windows_verbatim_prefix_powershell_cannot_read() {
        assert_eq!(
            plain(PathBuf::from(r"\\?\F:\github\GraphHelm")),
            PathBuf::from(r"F:\github\GraphHelm")
        );
        assert_eq!(
            plain(PathBuf::from(r"\\?\UNC\server\share")),
            PathBuf::from(r"\\?\UNC\server\share")
        );
        assert_eq!(
            plain(PathBuf::from("/home/u/GraphHelm")),
            PathBuf::from("/home/u/GraphHelm")
        );
    }

    #[test]
    fn picks_the_first_enabled_text_route_never_the_judge() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = dir.path().join("manifest.json");
        std::fs::write(&manifest, r#"{"routes":[{"id":"judge","provider":"typesafe"},{"id":"off","provider":"anthropic","enabled":false},{"id":"claude","provider":"anthropic","enabled":true}]}"#).unwrap();
        assert_eq!(text_route(&manifest), Some("claude".to_owned()));
        std::fs::write(
            &manifest,
            r#"{"routes":[{"id":"judge","provider":"typesafe"}]}"#,
        )
        .unwrap();
        assert_eq!(text_route(&manifest), None);
        assert_eq!(text_route(&dir.path().join("missing.json")), None);
    }

    #[test]
    fn this_binary_knows_the_clone_it_was_built_from() {
        assert!(Path::new(BUILT_FROM).join(LAUNCHER).is_file());
    }

    #[test]
    fn the_launcher_gets_the_projects_absolute_path_for_serve() {
        let mut launch = Command::new("powershell");
        add_project(&mut launch, Path::new("/work/my project"));
        let given: Vec<_> = launch
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let at = given
            .iter()
            .position(|a| a == "-ProjectPath")
            .expect("-ProjectPath is passed");
        assert!(given[at + 1].ends_with("my project"));
        assert!(given.contains(&"-Project".to_string()));
    }

    /// Runs the real launcher against a stub `graphhelm` that records the argv it receives, in a
    /// project whose path has a space: `--project` and `--events` must each arrive as ONE argument.
    /// The launcher then times out waiting for a Runtime that never answers; that is expected.
    #[cfg(windows)]
    #[test]
    fn the_launcher_hands_serve_the_project_path_as_one_argument() {
        use std::net::TcpListener;
        // Both listeners are held while the two ports are read, so they cannot be the same number
        // (#549: one bound and released before the next could be offered again).
        let first = TcpListener::bind("127.0.0.1:0").unwrap();
        let second = TcpListener::bind("127.0.0.1:0").unwrap();
        let bind_port = first.local_addr().unwrap().port();
        let studio_port = second.local_addr().unwrap().port();
        drop((first, second));
        let root = std::env::temp_dir().join(format!("gh studio up {}", std::process::id()));
        let project = root.join("my project");
        let events = project.join(".graphhelm").join("events");
        std::fs::create_dir_all(&events).unwrap();
        std::fs::write(
            root.join("argv.ps1"),
            "$args | Set-Content -Encoding ascii (Join-Path $PSScriptRoot 'argv.txt')\r\n",
        )
        .unwrap();
        std::fs::write(
            root.join("stub.cmd"),
            "@powershell -NoProfile -ExecutionPolicy Bypass -File \"%~dp0argv.ps1\" %*\r\n",
        )
        .unwrap();
        let launcher = Path::new(env!("CARGO_MANIFEST_DIR")).join("../studio/tools/studio-up.ps1");
        let mut launch = Command::new("powershell");
        launch
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&launcher)
            .arg("-Events")
            .arg(&events)
            .args(["-Bind", &format!("127.0.0.1:{bind_port}")])
            .args(["-StudioPort", &studio_port.to_string(), "-NoBrowser"])
            .arg("-GraphHelm")
            .arg(root.join("stub.cmd"));
        add_project(&mut launch, &project);
        let _ = launch.output();
        let argv = std::fs::read_to_string(root.join("argv.txt")).expect("the stub was started");
        let lines: Vec<&str> = argv.lines().collect();
        let at = lines
            .iter()
            .position(|l| *l == "--project")
            .expect("--project is passed");
        assert!(lines[at + 1].ends_with("my project"), "{lines:?}");
        let at = lines
            .iter()
            .position(|l| *l == "--events")
            .expect("--events is passed");
        assert!(
            lines[at + 1].ends_with("events") && lines[at + 1].contains("my project"),
            "{lines:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #549: the launcher waits longer for a slow Runtime (measured: a loaded machine took 7.5 s, and
    /// once not even 60 s), so a Runtime that DIED must not cost that whole wait. The stub exits at
    /// once; the launcher says so within seconds instead of sitting out its deadline. Credible
    /// regression: a fixed wait that only ever ends at its deadline. Cost: one PowerShell launch.
    #[cfg(windows)]
    #[test]
    fn the_launcher_says_at_once_when_the_runtime_it_started_exits() {
        use std::net::TcpListener;
        let first = TcpListener::bind("127.0.0.1:0").unwrap();
        let second = TcpListener::bind("127.0.0.1:0").unwrap();
        let bind_port = first.local_addr().unwrap().port();
        let studio_port = second.local_addr().unwrap().port();
        drop((first, second));
        let root = std::env::temp_dir().join(format!("gh studio exits {}", std::process::id()));
        let events = root.join("events");
        std::fs::create_dir_all(&events).unwrap();
        std::fs::write(
            root.join("dies.cmd"),
            "@exit /b 3
",
        )
        .unwrap();
        let launcher = Path::new(env!("CARGO_MANIFEST_DIR")).join("../studio/tools/studio-up.ps1");
        let started = std::time::Instant::now();
        let output = Command::new("powershell")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&launcher)
            .arg("-Events")
            .arg(&events)
            .args(["-Bind", &format!("127.0.0.1:{bind_port}")])
            .args(["-StudioPort", &studio_port.to_string(), "-NoBrowser"])
            .arg("-GraphHelm")
            .arg(root.join("dies.cmd"))
            .output()
            .unwrap();
        let elapsed = started.elapsed();
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let _ = std::fs::remove_dir_all(&root);
        assert!(!output.status.success(), "{said}");
        assert!(
            said.contains("exited"),
            "the launcher must say the Runtime exited: {said}"
        );
        assert!(
            // Scaled like the repository's other test deadlines (#549): a loaded machine is slower.
            elapsed < crate::test_time::scaled(std::time::Duration::from_secs(15)),
            "took {elapsed:?}: {said}"
        );
    }
}
