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
use crate::progress::Progress;

const COMMAND: &str = "studio";
const LAUNCHER: &str = "apps/studio/tools/studio-up.ps1";
/// The POSIX twin of the launcher (#1152): Linux and macOS start the Studio without PowerShell.
const POSIX_LAUNCHER: &str = "apps/studio/tools/studio-up.sh";

/// A launcher flag in the running platform's spelling: `-ProjectPath` for the PowerShell
/// launcher, `--project-path` for the POSIX one. Both launchers take the same flags.
pub(super) fn launcher_flag(name: &str, posix: bool) -> String {
    if !posix {
        return format!("-{name}");
    }
    let mut flag = String::from("-");
    for c in name.chars() {
        if c.is_ascii_uppercase() {
            flag.push('-');
        }
        flag.push(c.to_ascii_lowercase());
    }
    flag
}
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
    let output = graphhelm_process::command("git")
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

/// The default upstream of the managed install clone when the seed clone names none.
const UPSTREAM: &str = "https://github.com/stabem/GraphHelm.git";

/// #1134: the install clone `graphhelm update` and `studio start` use when no `--source` or
/// `GRAPHHELM_SOURCE` names one: `<home>/.graphhelm/source`. It belongs to GraphHelm alone, so
/// agents working in the owner's own clone can never block an update with local changes. It is
/// created on first use from the seed clone's `origin` (or the public upstream).
pub(super) fn managed_dir(home: Option<std::ffi::OsString>) -> Option<PathBuf> {
    home.filter(|value| !value.is_empty())
        .map(|home| PathBuf::from(home).join(".graphhelm").join("source"))
}

fn user_home() -> Option<std::ffi::OsString> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
}

/// The managed install clone, cloned from `seed`'s origin on first use.
fn managed_clone(seed: Option<&Path>, progress: &Progress) -> Result<PathBuf, String> {
    let dir = managed_dir(user_home())
        .ok_or_else(|| "no home directory for the GraphHelm install clone".to_string())?;
    if dir.join(LAUNCHER).is_file() {
        return Ok(plain(std::fs::canonicalize(&dir).unwrap_or(dir)));
    }
    let url = seed
        .and_then(|seed| git(seed, &["remote", "get-url", "origin"]))
        .filter(|url| !url.is_empty())
        .unwrap_or_else(|| UPSTREAM.to_owned());
    Progress::say(&progress.info(&format!(
        "creating the GraphHelm install clone {} from {url}",
        dir.display()
    )));
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|_| format!("could not create {}", parent.display()))?;
    }
    let cloned = graphhelm_process::command("git")
        .args(["clone", "--quiet", "--branch", "main", &url])
        .arg(&dir)
        .status()
        .is_ok_and(|status| status.success());
    if !cloned || !dir.join(LAUNCHER).is_file() {
        return Err(format!("could not clone {url} into {}", dir.display()));
    }
    Ok(plain(std::fs::canonicalize(&dir).unwrap_or(dir)))
}

/// Moves the managed clone to `origin/main`, whatever is in it: nobody edits it, so a change
/// there is GraphHelm's own leftover and is discarded rather than blocking the update.
fn sync_managed(source: &Path, progress: Progress) -> serde_json::Value {
    let progress = progress.retag("studio");
    if git(source, &["fetch", "--quiet", "origin", "main"]).is_none() {
        Progress::say(&progress.warn("could not fetch origin/main; running the clone as it is"));
        return json!({ "state": "unchecked" });
    }
    let before = git(source, &["rev-parse", "HEAD"]).unwrap_or_default();
    let target = git(source, &["rev-parse", "origin/main"]).unwrap_or_default();
    let clean = git(source, &["status", "--porcelain", "--untracked-files=no"])
        .is_some_and(|status| status.is_empty());
    let modules = source.join("apps/studio/node_modules").is_dir();
    if before == target && clean && modules {
        Progress::say(&progress.ok("already up to date with origin/main"));
        return json!({ "state": "up_to_date" });
    }
    if git(
        source,
        &["checkout", "--quiet", "--force", "--detach", "origin/main"],
    )
    .is_none()
    {
        Progress::say(&progress.warn("could not move the install clone; running it as it is"));
        return json!({ "state": "stale", "reason": "the install clone could not be moved" });
    }
    let behind = git(
        source,
        &["rev-list", "--count", &format!("{before}..{target}")],
    )
    .and_then(|count| count.parse::<u64>().ok())
    .unwrap_or(0);
    let lock_moved = !modules
        || git(
            source,
            &["diff", "--quiet", &before, "HEAD", "--", LOCKFILE],
        )
        .is_none();
    if lock_moved {
        Progress::say(&progress.info("installing the Studio's dependencies (npm ci)"));
        let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
        let installed = graphhelm_process::command(npm)
            .args(["ci", "--prefix"])
            .arg(source.join("apps/studio"))
            .status()
            .is_ok_and(|status| status.success());
        if !installed {
            Progress::say(&progress.warn("npm ci failed; the Studio may not start"));
        }
    }
    Progress::say(&progress.ok(&format!(
        "install clone at origin/main ({behind} new commit(s))"
    )));
    json!({ "state": "updated", "commits": behind, "dependenciesReinstalled": lock_moved, "managed": true })
}

/// Which clone to run and how to bring it up to date: an explicit `--source` or
/// `GRAPHHELM_SOURCE` is the operator's own clone and is only fast-forwarded when clean; otherwise
/// the managed install clone (#1134), seeded from the clone this binary was built from.
fn choose_source(
    explicit: Option<PathBuf>,
    progress: &Progress,
) -> Result<(PathBuf, bool), String> {
    let explicit = [
        explicit,
        std::env::var_os("GRAPHHELM_SOURCE").map(PathBuf::from),
    ];
    if explicit.iter().any(Option::is_some) {
        return resolve_source(&explicit)
            .map(|source| (source, false))
            .ok_or_else(|| {
                format!(
                    "no GraphHelm clone with {LAUNCHER} was found at --source or GRAPHHELM_SOURCE"
                )
            });
    }
    let seed = resolve_source(&[Some(PathBuf::from(BUILT_FROM))]);
    managed_clone(seed.as_deref(), progress).map(|source| (source, true))
}

/// Brings the clone up to `origin/main` when [`freshness`] allows it. Returns what happened, for
/// the envelope; a clone git cannot read (no git, no remote, offline) is run as it is.
fn update(source: &Path, progress: Progress) -> serde_json::Value {
    let progress = progress.retag("studio");
    if git(source, &["fetch", "--quiet", "origin", "main"]).is_none() {
        Progress::say(&progress.warn("could not fetch origin/main; running the clone as it is"));
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
        Freshness::UpToDate => {
            Progress::say(&progress.ok("already up to date with origin/main"));
            json!({ "state": "up_to_date" })
        }
        Freshness::Stale { behind, reason } => {
            Progress::say(&progress.warn(&format!(
                "this Studio is {behind} commit(s) behind origin/main and was NOT updated: {reason} ({})",
                source.display()
            )));
            json!({ "state": "stale", "behind": behind, "reason": reason })
        }
        Freshness::FastForward(behind) => {
            let before = git(source, &["rev-parse", "HEAD"]).unwrap_or_default();
            Progress::say(&progress.info(&format!(
                "updating {} ({behind} new commit(s) on main)",
                source.display()
            )));
            if git(source, &["pull", "--ff-only", "--quiet", "origin", "main"]).is_none() {
                Progress::say(
                    &progress.warn("the fast-forward failed; running the clone as it is"),
                );
                return json!({ "state": "stale", "behind": behind, "reason": "the fast-forward failed" });
            }
            let lock_moved = git(
                source,
                &["diff", "--quiet", &before, "HEAD", "--", LOCKFILE],
            )
            .is_none();
            if lock_moved {
                Progress::say(&progress.info("the Studio's dependencies changed; running npm ci"));
                let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
                let installed = graphhelm_process::command(npm)
                    .args(["ci", "--prefix"])
                    .arg(source.join("apps/studio"))
                    .status()
                    .is_ok_and(|status| status.success());
                if !installed {
                    Progress::say(&progress.warn("npm ci failed; the Studio may not start"));
                }
            }
            Progress::say(&progress.ok(&format!("pulled {behind} new commit(s)")));
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
    let started = std::time::Instant::now();
    let progress = Progress::decide("update");
    Progress::say(&progress.banner("graphhelm update: sync, build, install"));
    Progress::say(&progress.step(1, 3, "Syncing the clone"));
    let (source, managed) = match choose_source(args.source.clone(), &progress) {
        Ok(chosen) => chosen,
        Err(message) => return refused_as(UPDATE, message, "/source"),
    };
    Progress::say(&progress.info(&format!("GraphHelm source: {}", source.display())));
    let clone = if managed {
        sync_managed(&source, progress)
    } else {
        update(&source, progress)
    };
    // A clone that could not be fast-forwarded would only rebuild the old code: a long compile
    // that installs nothing new (owner run, 2026-10-12: 18 minutes, 28 commits behind).
    if clone["state"] == "stale" && !args.keep_local {
        let reason = clone["reason"]
            .as_str()
            .unwrap_or("the clone could not be updated");
        return refused_as(
            UPDATE,
            format!(
                "nothing was built: {reason}, so the clone is still {} commit(s) behind origin/main. \
                 To set the local changes aside and update: git -C \"{dir}\" stash, then git -C \"{dir}\" switch main, \
                 then graphhelm update again. To build the clone as it is: graphhelm update --keep-local",
                clone["behind"],
                dir = source.display()
            ),
            "/source",
        );
    }
    let head = git(&source, &["rev-parse", "HEAD"]);
    let root = install_root(std::env::current_exe().ok().as_deref());
    let stamp = root.as_ref().map(|root| root.join(STAMP));
    let installed_head = stamp
        .as_ref()
        .and_then(|stamp| std::fs::read_to_string(stamp).ok());
    // Uncommitted edits are not in HEAD, so a matching stamp says nothing about them: build.
    let clean = git(&source, &["status", "--porcelain", "--untracked-files=no"])
        .is_some_and(|status| status.is_empty());
    Progress::say(&progress.step(2, 3, "Building the CLI"));
    if clean && already_installed(head.as_deref(), installed_head.as_deref()) {
        Progress::say(
            &progress.ok("graphhelm is already built from this commit; nothing to install"),
        );
        Progress::say(&progress.step(3, 3, "Done"));
        Progress::say(&progress.ok(&format!(
            "nothing new to install ({}s)",
            started.elapsed().as_secs()
        )));
        return Outcome::success(
            UPDATE,
            json!({
                "source": source.display().to_string(),
                "head": git(&source, &["rev-parse", "--short", "HEAD"]),
                "update": clone,
                "installed": false,
            }),
        );
    }
    let admission_path = match super::workspace_slot::build_admission_dir() {
        Ok(path) => path,
        Err(message) => return refused_as(UPDATE, message, "/admission"),
    };
    // Install over THIS binary, wherever it lives: a graphhelm run from a versioned directory
    // (`~/.graphhelm/versions/<sha>/bin`) updated into `~/.cargo/bin` would leave PATH on the old
    // one, and on Windows with nothing at all once it was moved aside (2026-10-05).
    Progress::say(
        &progress
            .info("installing the CLI (cargo install --locked --profile update --path apps/cli)"),
    );
    // #1140: the managed clone builds into its own cache, `~/.graphhelm/target`. A shared
    // `CARGO_TARGET_DIR` (the owner's agents build in one) holds cargo's lock while any agent
    // compiles, and the update then waited there silently (owner run, 2026-10-13).
    let target = match (managed, managed_target(user_home())) {
        (true, Some(own)) => own,
        _ => target_dir(std::env::var_os("CARGO_TARGET_DIR"), &source),
    };
    Progress::say(&progress.info(&format!("reusing build cache {}", target.display())));
    let mut install = graphhelm_process::command("cargo");
    install
        .current_dir(&source)
        .args([
            "install",
            "--locked",
            "--profile",
            "update",
            "--path",
            "apps/cli",
            "--target-dir",
        ])
        .arg(&target);
    if let Some(root) = &root {
        Progress::say(&progress.info(&format!("into {}", root.join("bin").display())));
        // `--force`: cargo refuses to overwrite a binary it did not install itself, and a
        // versioned directory's binary was placed there by another installer (#281 review).
        install.arg("--root").arg(root).arg("--force");
    }
    let installed = run_install(
        &mut install,
        &source,
        progress,
        &admission_path,
        std::env::current_exe().ok().as_deref(),
    );
    if !installed {
        return refused_as(
            UPDATE,
            "cargo install failed; its output is above, and the old graphhelm is unchanged",
            "/install",
        );
    }
    if let (Some(stamp), Some(head)) = (&stamp, &head) {
        let _ = std::fs::write(stamp, head);
    }
    Progress::say(&progress.step(3, 3, "Done"));
    Progress::say(&progress.ok(&format!(
        "graphhelm updated in {}s",
        started.elapsed().as_secs()
    )));
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

/// Beside `bin`, the commit the installed binary was built from: lets an update with nothing new
/// return in seconds instead of rebuilding.
const STAMP: &str = ".graphhelm-installed-head";

/// True when the clone's HEAD is exactly the commit the installed binary was last built from.
pub(super) fn already_installed(head: Option<&str>, installed: Option<&str>) -> bool {
    matches!((head, installed), (Some(head), Some(installed)) if !head.is_empty() && head == installed.trim())
}

/// `cargo install` builds in a fresh temporary directory and deletes it, so every update rebuilt
/// all ~300 crates from zero (over 10 minutes on Windows, 2026-10-12). A persistent target dir
/// keeps the dependencies compiled between updates; `CARGO_TARGET_DIR` wins when set.
/// How many packages the build may compile: the `[[package]]` entries of the clone's lockfile.
/// An upper bound (platform-only crates are skipped), so the bar ends at the build's own end.
pub(super) fn crate_total(lockfile: &str) -> usize {
    lockfile
        .lines()
        .filter(|line| line.trim() == "[[package]]")
        .count()
}

/// The crate a cargo JSON line finished compiling, when it is a `compiler-artifact` message.
pub(super) fn compiled_crate(line: &str) -> Option<String> {
    let message: serde_json::Value = serde_json::from_str(line).ok()?;
    (message["reason"] == "compiler-artifact")
        .then(|| message["target"]["name"].as_str().map(str::to_owned))
        .flatten()
}

/// Runs `cargo install` with its output piped (#1143). A child started with `CREATE_NO_WINDOW`
/// gets its own hidden console, so output it wrote to the inherited console never reached the
/// owner's terminal: a 53-minute build showed nothing at all. Cargo's JSON messages on stdout now
/// drive a progress bar; its stderr is kept, a lock wait is said at once, and on failure the
/// whole of it is printed. In plain mode (not a terminal) stderr is forwarded line by line.
pub(super) fn run_install(
    install: &mut std::process::Command,
    source: &Path,
    progress: Progress,
    admission_path: &Path,
    current_exe: Option<&Path>,
) -> bool {
    use std::io::{BufRead, BufReader, Write};
    let admission = match super::workspace_slot::open_build_admission(admission_path) {
        Ok(lock) => lock,
        Err(message) => {
            Progress::say(&progress.warn(&message));
            return false;
        }
    };
    Progress::say(&progress.info("waiting for the shared build admission lock"));
    if let Err(message) = super::workspace_slot::lock_build_admission(&admission) {
        Progress::say(&progress.warn(&message));
        return false;
    }
    let moved_aside = if cfg!(windows) {
        current_exe.and_then(|exe| {
            let old = exe.with_file_name("graphhelm.old.exe");
            let _ = std::fs::remove_file(&old);
            std::fs::rename(exe, &old)
                .ok()
                .map(|()| (exe.to_owned(), old))
        })
    } else {
        None
    };
    install
        .arg("--message-format")
        .arg("json-render-diagnostics")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let Ok(mut child) = install.spawn() else {
        if let Some((exe, old)) = &moved_aside {
            let _ = std::fs::rename(old, exe);
        }
        Progress::say(&progress.warn("cargo could not be started; is it installed?"));
        return false;
    };
    let total = std::fs::read_to_string(source.join("Cargo.lock"))
        .map(|lock| crate_total(&lock))
        .unwrap_or(0);
    let stderr = child.stderr.take().map(|stderr| {
        std::thread::spawn(move || {
            let mut kept = Vec::new();
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if !progress.redraws() {
                    eprintln!("{line}");
                } else if line.contains("Blocking waiting for file lock") {
                    eprintln!();
                    Progress::say(&progress.warn(&format!(
                        "{} (another build holds this cache; it continues when that one ends)",
                        line.trim()
                    )));
                }
                kept.push(line);
            }
            kept
        })
    });
    let mut done = 0usize;
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(name) = compiled_crate(&line) {
                done += 1;
                if progress.redraws() {
                    eprint!("{}", progress.bar(done, total.max(done + 1), &name));
                    let _ = std::io::stderr().flush();
                }
            }
        }
    }
    let ok = child.wait().is_ok_and(|status| status.success());
    let kept = stderr
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    if progress.redraws() {
        if ok {
            eprintln!("{}", progress.bar(done, done.max(1), "done"));
        } else {
            eprintln!();
            for line in &kept {
                eprintln!("{line}");
            }
        }
    }
    if !ok {
        if let Some((exe, old)) = &moved_aside {
            let _ = std::fs::rename(old, exe);
        }
    }
    ok
}

/// The managed install clone's own build cache, beside it under `~/.graphhelm` (#1140).
pub(super) fn managed_target(home: Option<std::ffi::OsString>) -> Option<PathBuf> {
    managed_dir(home).and_then(|dir| dir.parent().map(|parent| parent.join("target")))
}

pub(super) fn target_dir(environment: Option<std::ffi::OsString>, source: &Path) -> PathBuf {
    environment
        .filter(|value| !value.is_empty())
        .map_or_else(|| source.join("target"), PathBuf::from)
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
    let posix = !cfg!(windows);
    if let Some(name) = project.file_name() {
        launch.arg(launcher_flag("Project", posix)).arg(name);
    }
    launch
        .arg(launcher_flag("ProjectPath", posix))
        .arg(plain(project.to_path_buf()));
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
    let studio_progress = Progress::new("studio", false, crate::palette::Palette::plain());
    let (source, managed) = match choose_source(args.source.clone(), &studio_progress) {
        Ok(chosen) => chosen,
        Err(message) => return refused(message, "/source"),
    };
    eprintln!("[studio] Studio source: {}", source.display());
    let freshness = if args.no_update {
        json!({ "state": "skipped" })
    } else if managed {
        sync_managed(&source, studio_progress)
    } else {
        update(&source, studio_progress)
    };
    let head = git(&source, &["rev-parse", "--short", "HEAD"]);

    let posix = !cfg!(windows);
    let flag = |name: &str| launcher_flag(name, posix);
    let shell = if posix { "sh" } else { "powershell" };
    let mut launch = graphhelm_process::command(shell);
    if posix {
        launch.arg(source.join(POSIX_LAUNCHER));
    } else {
        launch
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(source.join(LAUNCHER));
    }
    launch
        .arg(flag("Events"))
        .arg(&events)
        .args([flag("Bind"), args.bind.clone()]);
    add_project(&mut launch, &project);
    if let Ok(exe) = std::env::current_exe() {
        launch.arg(flag("GraphHelm")).arg(exe);
    }
    let keyring = state.join(super::init::KEYRING_DIRECTORY);
    let key = std::fs::read_to_string(state.join(super::init::KEY_FILE)).ok();
    let sealed = keyring.is_dir() && key.is_some();
    if let (true, Some(key)) = (keyring.is_dir(), key) {
        // The key travels in the child's environment only, as `init` prints it; never as an
        // argument, never printed.
        launch
            .env(SEALING_KEY_ENVIRONMENT, key.trim())
            .arg(flag("Keyring"))
            .arg(&keyring)
            .args([flag("KeyId"), args.key_id.clone()]);
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
            .arg(flag("Manifest"))
            .arg(&manifest)
            .arg(flag("Broker"))
            .arg(&broker)
            .args([flag("Route"), route]);
    }
    if args.no_browser {
        launch.arg(flag("NoBrowser"));
    }
    let launched = match launch.status() {
        Ok(status) => status.success(),
        Err(_) => {
            return refused(
                format!("{shell} could not be started to run the Studio launcher"),
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

    /// #1143: the bar's total counts the lockfile's packages, and only cargo's
    /// `compiler-artifact` lines advance it.
    #[test]
    fn the_build_bar_counts_lockfile_packages_and_compiled_artifacts() {
        let lock = "version = 4\n\n[[package]]\nname = \"a\"\n\n[[package]]\nname = \"b\"\n";
        assert_eq!(crate_total(lock), 2);
        assert_eq!(
            compiled_crate(r#"{"reason":"compiler-artifact","target":{"name":"serde"}}"#),
            Some("serde".into())
        );
        assert_eq!(
            compiled_crate(r#"{"reason":"build-script-executed"}"#),
            None
        );
        assert_eq!(compiled_crate("Compiling serde v1"), None);
    }

    /// #1140: the managed clone never builds in a shared target directory another build locks.
    #[test]
    fn the_install_clone_builds_into_its_own_cache() {
        assert_eq!(
            managed_target(Some("/home/u".into())),
            Some(PathBuf::from("/home/u/.graphhelm/target"))
        );
        assert_eq!(managed_target(None), None);
    }

    /// #1134: the install clone lives under the user's home, never in a clone agents work in.
    #[test]
    fn the_install_clone_lives_under_the_home_graphhelm_directory() {
        assert_eq!(
            managed_dir(Some("/home/u".into())),
            Some(PathBuf::from("/home/u/.graphhelm/source"))
        );
        assert_eq!(managed_dir(Some("".into())), None);
        assert_eq!(managed_dir(None), None);
    }

    /// #1134: local changes in the managed install clone never stop an update; it is moved to
    /// origin/main. Defect named: the owner's update refused twice in a day because agents had
    /// edited the clone it built from. Cost: two temporary git repositories, no network.
    #[test]
    fn a_dirty_install_clone_is_moved_to_origin_main_instead_of_refusing() {
        let run = |dir: &Path, args: &[&str]| {
            let ok = graphhelm_process::command("git")
                .arg("-C")
                .arg(dir)
                .args(["-c", "user.email=t@t", "-c", "user.name=t"])
                .args(args)
                .output()
                .unwrap()
                .status
                .success();
            assert!(ok, "git {args:?}");
        };
        let scratch = tempfile::tempdir().unwrap();
        let origin = scratch.path().join("origin");
        std::fs::create_dir_all(origin.join("apps/studio/tools")).unwrap();
        run(&origin, &["init", "--quiet", "--initial-branch", "main"]);
        std::fs::write(origin.join(LAUNCHER), "v1").unwrap();
        std::fs::write(origin.join(LOCKFILE), "{}").unwrap();
        run(&origin, &["add", "."]);
        run(&origin, &["commit", "--quiet", "-m", "one"]);
        let clone = scratch.path().join("clone");
        run(
            scratch.path(),
            &["clone", "--quiet", origin.to_str().unwrap(), "clone"],
        );
        std::fs::create_dir_all(clone.join("apps/studio/node_modules")).unwrap();
        std::fs::write(origin.join(LAUNCHER), "v2").unwrap();
        run(&origin, &["commit", "--quiet", "-am", "two"]);
        std::fs::write(clone.join(LAUNCHER), "an agent's edit").unwrap();

        let result = sync_managed(
            &clone,
            Progress::new("test", false, crate::palette::Palette::plain()),
        );

        assert_eq!(result["state"], "updated", "{result}");
        assert_eq!(result["commits"], 1, "{result}");
        assert_eq!(std::fs::read_to_string(clone.join(LAUNCHER)).unwrap(), "v2");
        assert_eq!(
            sync_managed(
                &clone,
                Progress::new("test", false, crate::palette::Palette::plain())
            )["state"],
            "up_to_date"
        );
    }

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
    fn update_reuses_a_persistent_target_dir() {
        let source = Path::new("/clone");
        assert_eq!(target_dir(None, source), source.join("target"));
        assert_eq!(target_dir(Some("".into()), source), source.join("target"));
        assert_eq!(
            target_dir(Some("/cache".into()), source),
            PathBuf::from("/cache")
        );
    }

    #[test]
    fn update_skips_only_when_the_installed_head_matches() {
        assert!(already_installed(Some("abc"), Some("abc\n")));
        assert!(!already_installed(Some("abc"), Some("def")));
        assert!(!already_installed(Some("abc"), None));
        assert!(!already_installed(None, Some("abc")));
        assert!(!already_installed(Some(""), Some("")));
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
        let mut launch = graphhelm_process::command("powershell");
        add_project(&mut launch, Path::new("/work/my project"));
        let given: Vec<_> = launch
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let at = given
            .iter()
            .position(|a| *a == launcher_flag("ProjectPath", !cfg!(windows)))
            .expect("the project path is passed");
        assert!(given[at + 1].ends_with("my project"));
        assert!(given.contains(&launcher_flag("Project", !cfg!(windows))));
    }

    #[test]
    fn posix_launcher_flags_are_the_kebab_case_twins() {
        assert_eq!(launcher_flag("ProjectPath", true), "--project-path");
        assert_eq!(launcher_flag("NoBrowser", true), "--no-browser");
        assert_eq!(launcher_flag("KeyId", false), "-KeyId");
    }

    /// #1152: the POSIX launcher hands `serve` every path as one argument and says at once when
    /// the Runtime it started exits, as the PowerShell launcher does on Windows.
    #[cfg(unix)]
    #[test]
    fn the_posix_launcher_starts_serve_with_the_projects_paths() {
        use std::net::TcpListener;
        let first = TcpListener::bind("127.0.0.1:0").unwrap();
        let second = TcpListener::bind("127.0.0.1:0").unwrap();
        let bind_port = first.local_addr().unwrap().port();
        let studio_port = second.local_addr().unwrap().port();
        drop((first, second));
        let root = std::env::temp_dir().join(format!("gh studio up sh {}", std::process::id()));
        let project = root.join("my project");
        let events = project.join(".graphhelm").join("events");
        std::fs::create_dir_all(&events).unwrap();
        let stub = root.join("stub.sh");
        std::fs::write(
            &stub,
            format!(
                "#!/bin/sh\nfor a in \"$@\"; do echo \"$a\"; done > '{}'\nexit 7\n",
                root.join("argv.txt").display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        let launcher = Path::new(env!("CARGO_MANIFEST_DIR")).join("../studio/tools/studio-up.sh");
        let started = std::time::Instant::now();
        let mut launch = graphhelm_process::command("sh");
        launch
            .arg(&launcher)
            .arg("--events")
            .arg(&events)
            .args(["--bind", &format!("127.0.0.1:{bind_port}")])
            .args(["--studio-port", &studio_port.to_string(), "--no-browser"])
            .arg("--graph-helm")
            .arg(&stub);
        add_project(&mut launch, &project);
        let output = launch.output().unwrap();
        assert!(!output.status.success());
        assert!(
            started.elapsed().as_secs() < 30,
            "a dead Runtime is said at once"
        );
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
        let mut launch = graphhelm_process::command("powershell");
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
        let output = graphhelm_process::command("powershell")
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
