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
        .map(|found| std::fs::canonicalize(found).unwrap_or_else(|_| found.clone()))
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
    eprintln!("[update] installing the CLI (cargo install --locked --path apps/cli)");
    let installed = Command::new("cargo")
        .current_dir(&source)
        .args(["install", "--locked", "--path", "apps/cli"])
        .status()
        .is_ok_and(|status| status.success());
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
    if let Some(name) = project.file_name() {
        launch.arg("-Project").arg(name);
    }
    if let Ok(exe) = std::env::current_exe() {
        launch.arg("-GraphHelm").arg(exe);
    }
    let keyring = state.join(super::init::KEYRING_DIRECTORY);
    let key = std::fs::read_to_string(state.join(super::init::KEY_FILE)).ok();
    if let (true, Some(key)) = (keyring.is_dir(), key) {
        // The key travels in the child's environment only, as `init` prints it; never as an
        // argument, never printed.
        launch
            .env(SEALING_KEY_ENVIRONMENT, key.trim())
            .arg("-Keyring")
            .arg(&keyring)
            .args(["-KeyId", &args.key_id]);
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
        assert_eq!(found, Some(std::fs::canonicalize(clone.path()).unwrap()));
        assert_eq!(resolve_source(&[Some(empty.path().into())]), None);
    }

    #[test]
    fn this_binary_knows_the_clone_it_was_built_from() {
        assert!(Path::new(BUILT_FROM).join(LAUNCHER).is_file());
    }
}
