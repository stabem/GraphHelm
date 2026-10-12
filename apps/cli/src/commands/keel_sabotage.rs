//! Sabotage proof for journeys: a green journey set must turn red when a patch breaks the product.
//!
//! Runs `keel test validation` in a temporary detached worktree of HEAD, applies the patch there,
//! and runs it again. The user's working tree is never touched and the worktree is always removed.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

use crate::output::{CommandOutput, Outcome};

const COMMAND: &str = "keel.test.sabotage";

const SCHEMA: &str = "graphhelm-keel-validation/1";

/// A validation report the sabotage verdict may read: the validation schema, at least one path.
fn well_formed(report: &Value) -> bool {
    report["schema"] == SCHEMA
        && report["paths"]
            .as_array()
            .is_some_and(|paths| !paths.is_empty())
}

/// Paths that did not complete, or flows that failed, in one validation report.
fn failing(report: &Value) -> Vec<Value> {
    let mut failed: Vec<Value> = report["paths"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|path| path["execution"] != "completed")
        .map(|path| json!({"flow": path["flow"], "path": path["path"], "execution": path["execution"]}))
        .collect();
    for failure in report["failures"].as_array().into_iter().flatten() {
        let flow = &failure["flow"];
        if !failed.iter().any(|entry| entry["flow"] == *flow) {
            failed.push(json!({"flow": flow, "reason": failure["reason"]}));
        }
    }
    failed
}

/// Execution green: a well-formed report, every path completed, no failures. Coverage
/// completeness is not required; sabotage asks whether execution observes the change.
fn green(report: &Value) -> bool {
    well_formed(report) && failing(report).is_empty()
}

/// The `(flow, path)` set a report declares; sabotage compares it before and after the patch.
fn path_set(report: &Value) -> std::collections::BTreeSet<(String, String)> {
    report["paths"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|path| {
            let text = |key: &str| path[key].as_str().unwrap_or_default().to_owned();
            (text("flow"), text("path"))
        })
        .collect()
}

/// The verdict. `caught` needs the same declared journey paths to have run after the patch, with
/// at least one executed and failed. A missing, malformed, held or not-run report never counts.
fn decide(
    baseline: &Value,
    touches_observer: bool,
    applied: bool,
    after: Option<&Value>,
) -> (&'static str, Vec<Value>) {
    if touches_observer {
        return ("patch_touches_observer", Vec::new());
    }
    if !green(baseline) {
        return ("baseline_not_green", failing(baseline));
    }
    if !applied {
        return ("patch_not_applicable", Vec::new());
    }
    let Some(after) = after.filter(|after| well_formed(after)) else {
        return ("no_journey_ran", Vec::new());
    };
    if path_set(after) != path_set(baseline) {
        return ("journey_set_changed", Vec::new());
    }
    if green(after) {
        return ("missed", Vec::new());
    }
    let executed_failures: Vec<Value> = failing(after)
        .into_iter()
        // `failed`: a step failed; `drift`: the journey ran and an expected screen was missing.
        .filter(|entry| entry["execution"] == "failed" || entry["execution"] == "drift")
        .collect();
    if executed_failures.is_empty() {
        ("no_journey_ran", failing(after))
    } else {
        ("caught", executed_failures)
    }
}

/// Whether the patch touches `.graphhelm/` (journeys, observers): sabotage must break the product,
/// not the test. Read with `git apply --numstat` before anything runs.
fn touches_observer(repo: &Path, patch: &Path) -> Result<bool, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["apply", "--numstat", "-z"])
        .arg(patch)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("git did not start: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(output
        .stdout
        .split(|byte| *byte == 0)
        .map(|field| String::from_utf8_lossy(field).into_owned())
        .map(|field| field.rsplit('\t').next().unwrap_or_default().to_owned())
        .any(|path| path == ".graphhelm" || path.starts_with(".graphhelm/")))
}

fn git(dir: &Path, args: &[&str]) -> Result<(), String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("git did not start: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

/// A detached worktree of HEAD that is removed when dropped.
struct Worktree {
    repo: PathBuf,
    path: PathBuf,
}

impl Worktree {
    fn create(repo: &Path) -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!(
            "graphhelm-keel-sabotage-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let text = path.to_string_lossy().to_string();
        git(
            repo,
            &["worktree", "add", "--detach", "--quiet", &text, "HEAD"],
        )?;
        Ok(Self {
            repo: repo.to_path_buf(),
            path,
        })
    }

    /// Git-ignored toolchain the journeys need but a fresh checkout lacks: the installed
    /// observer driver is copied, `node_modules` is linked read-only by convention.
    fn carry_toolchain(&self, repo: &Path) -> Result<(), String> {
        let observers = Path::new(".graphhelm").join("observers");
        let source = repo.join(&observers);
        if source.is_dir() {
            let target = self.path.join(&observers);
            std::fs::create_dir_all(&target).map_err(|error| error.to_string())?;
            for entry in std::fs::read_dir(&source).map_err(|error| error.to_string())? {
                let entry = entry.map_err(|error| error.to_string())?;
                if entry.file_type().is_ok_and(|kind| kind.is_file()) {
                    let to = target.join(entry.file_name());
                    if !to.exists() {
                        std::fs::copy(entry.path(), to).map_err(|error| error.to_string())?;
                    }
                }
            }
        }
        let modules = repo.join("node_modules");
        let link = self.path.join("node_modules");
        if modules.exists() && !link.exists() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(&modules, &link).map_err(|error| error.to_string())?;
            #[cfg(windows)]
            std::os::windows::fs::symlink_dir(&modules, &link)
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn apply(&self, patch: &Path) -> Result<(), String> {
        git(&self.path, &["apply", &patch.to_string_lossy()])
    }
}

impl Drop for Worktree {
    fn drop(&mut self) {
        let text = self.path.to_string_lossy().to_string();
        let _ = git(&self.repo, &["worktree", "remove", "--force", &text]);
        let _ = std::fs::remove_dir_all(&self.path);
        let _ = git(&self.repo, &["worktree", "prune"]);
    }
}

fn error(message: impl Into<String>, path: &str) -> Outcome {
    Outcome::application(
        COMMAND,
        graphhelm_protocols::Diagnostic::error(
            crate::error_codes::GHCLI001_ARGUMENT_INVALID,
            message,
            path,
            "keel",
        ),
    )
}

fn report(outcome: &Outcome) -> Value {
    outcome.output.data.clone().unwrap_or(Value::Null)
}

pub(crate) fn run(repo: &Path, patch: &Path) -> Outcome {
    let Ok(repo) = repo.canonicalize() else {
        return error("--repo must name a directory", "/repo");
    };
    let Ok(patch) = patch.canonicalize() else {
        return error("--patch must name a readable file", "/patch");
    };
    let observer = match touches_observer(&repo, &patch) {
        Ok(touches) => touches,
        Err(detail) => return error(format!("cannot read patch: {detail}"), "/patch"),
    };
    let mut baseline = Value::Null;
    let mut applied = false;
    let mut after = None;
    let mut after_diagnostics = Value::Null;
    if !observer {
        let worktree = match Worktree::create(&repo) {
            Ok(worktree) => worktree,
            Err(detail) => return error(format!("cannot create worktree: {detail}"), "/repo"),
        };
        if let Err(detail) = worktree.carry_toolchain(&repo) {
            return error(format!("cannot prepare worktree: {detail}"), "/repo");
        }
        baseline = report(&super::keel_validation::run(&worktree.path));
        if green(&baseline) {
            applied = worktree.apply(&patch).is_ok();
            if applied {
                let outcome = super::keel_validation::run(&worktree.path);
                after_diagnostics = json!(outcome.output.diagnostics);
                after = Some(report(&outcome));
            }
        }
    }
    let (verdict, failed) = decide(&baseline, observer, applied, after.as_ref());
    let ok = verdict == "caught";
    Outcome {
        output: CommandOutput {
            ok,
            command: COMMAND,
            data: Some(json!({
                "schema": "graphhelm-keel-sabotage/1",
                "repo": repo,
                "patch": patch,
                "verdict": verdict,
                "failing": failed,
                "baselinePaths": baseline["paths"].as_array().map_or(0, Vec::len),
                "afterPaths": after.as_ref().map(|after| after["paths"].clone()),
                "afterDiagnostics": after_diagnostics,
            })),
            diagnostics: Vec::new(),
        },
        exit_code: if ok { 0 } else { 2 },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validation(states: &[&str]) -> Value {
        let paths: Vec<Value> = states
            .iter()
            .map(|state| json!({"flow":"f","path":"main","execution":state}))
            .collect();
        let failures: Vec<Value> = states
            .iter()
            .filter(|state| **state != "completed")
            .map(|_| json!({"flow":"f","reason":"execution_failed"}))
            .collect();
        json!({"schema": SCHEMA, "paths": paths, "failures": failures})
    }

    #[test]
    fn caught_needs_the_same_paths_executed_and_failed() {
        let green_report = validation(&["completed"]);
        let red_report = validation(&["failed"]);
        let decide_after = |after: Option<&Value>| decide(&green_report, false, true, after).0;
        assert_eq!(
            decide(&red_report, false, true, Some(&red_report)).0,
            "baseline_not_green"
        );
        assert_eq!(
            decide(&validation(&[]), false, true, None).0,
            "baseline_not_green"
        );
        assert_eq!(
            decide(&green_report, true, true, Some(&red_report)).0,
            "patch_touches_observer"
        );
        assert_eq!(
            decide(&green_report, false, false, None).0,
            "patch_not_applicable"
        );
        assert_eq!(decide_after(Some(&green_report)), "missed");
        // A validation error (no report), a missing after-run, or a malformed report: nothing ran.
        assert_eq!(decide_after(Some(&Value::Null)), "no_journey_ran");
        assert_eq!(decide_after(None), "no_journey_ran");
        assert_eq!(
            decide_after(Some(
                &json!({"paths":[{"flow":"f","path":"main","execution":"failed"}]})
            )),
            "no_journey_ran"
        );
        // Held or never-run paths observed nothing.
        assert_eq!(
            decide_after(Some(&validation(&["not_run"]))),
            "no_journey_ran"
        );
        assert_eq!(decide_after(Some(&validation(&["held"]))), "no_journey_ran");
        // The patch removed or added a journey path: not the journeys that were green.
        let mut shrunk = red_report.clone();
        shrunk["paths"][0]["path"] = "other".into();
        assert_eq!(decide_after(Some(&shrunk)), "journey_set_changed");
        let (verdict, failed) = decide(&green_report, false, true, Some(&red_report));
        assert_eq!(verdict, "caught");
        assert_eq!(decide_after(Some(&validation(&["drift"]))), "caught");
        assert_eq!(
            failed,
            vec![json!({"flow":"f","path":"main","execution":"failed"})]
        );
    }

    #[test]
    fn patch_lands_in_a_removed_worktree_not_the_user_tree() {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path();
        git(repo, &["init", "-q"]).unwrap();
        git(repo, &["config", "user.email", "t@example.com"]).unwrap();
        git(repo, &["config", "user.name", "t"]).unwrap();
        std::fs::write(repo.join("app.js"), "ok\n").unwrap();
        git(repo, &["add", "."]).unwrap();
        git(repo, &["commit", "-qm", "init"]).unwrap();
        let patch = root.path().join("break.diff");
        std::fs::write(
            &patch,
            "--- a/app.js\n+++ b/app.js\n@@ -1 +1 @@\n-ok\n+broken\n",
        )
        .unwrap();
        assert!(!touches_observer(repo, &patch).unwrap());
        let journey = root.path().join("journey.diff");
        std::fs::write(
            &journey,
            "--- /dev/null\n+++ b/.graphhelm/journeys/a.journey.yaml\n@@ -0,0 +1 @@\n+id: a\n",
        )
        .unwrap();
        assert!(touches_observer(repo, &journey).unwrap());
        let worktree = Worktree::create(repo).unwrap();
        worktree.apply(&patch).unwrap();
        let path = worktree.path.clone();
        assert_eq!(
            std::fs::read_to_string(path.join("app.js")).unwrap(),
            "broken\n"
        );
        assert!(worktree.apply(&patch).is_err());
        drop(worktree);
        assert!(!path.exists());
        assert_eq!(
            std::fs::read_to_string(repo.join("app.js")).unwrap(),
            "ok\n"
        );
    }
}
