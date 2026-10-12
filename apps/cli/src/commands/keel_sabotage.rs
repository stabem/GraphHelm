//! Sabotage proof for journeys: a green journey set must turn red when a patch breaks the product.
//!
//! Runs `keel test validation` in a temporary detached worktree of HEAD, applies the patch there,
//! and runs it again. The user's working tree is never touched and the worktree is always removed.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

use crate::output::{CommandOutput, Outcome};

const COMMAND: &str = "keel.test.sabotage";

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

/// Execution green: at least one declared path, all completed, no failures. Coverage completeness
/// is not required; sabotage asks whether execution observes the change.
fn green(report: &Value) -> bool {
    report["paths"]
        .as_array()
        .is_some_and(|paths| !paths.is_empty())
        && failing(report).is_empty()
}

/// The verdict from the baseline report, whether the patch applied, and the sabotaged report.
fn decide(baseline: &Value, applied: bool, after: Option<&Value>) -> (&'static str, Vec<Value>) {
    if !green(baseline) {
        return ("baseline_not_green", failing(baseline));
    }
    if !applied {
        return ("patch_not_applicable", Vec::new());
    }
    match after {
        Some(after) if green(after) => ("missed", Vec::new()),
        Some(after) => ("caught", failing(after)),
        None => ("caught", Vec::new()),
    }
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
    let worktree = match Worktree::create(&repo) {
        Ok(worktree) => worktree,
        Err(detail) => return error(format!("cannot create worktree: {detail}"), "/repo"),
    };
    let baseline = report(&super::keel_validation::run(&worktree.path));
    let mut applied = false;
    let mut after = None;
    if green(&baseline) {
        applied = worktree.apply(&patch).is_ok();
        if applied {
            after = Some(report(&super::keel_validation::run(&worktree.path)));
        }
    }
    drop(worktree);
    let (verdict, failed) = decide(&baseline, applied, after.as_ref());
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
        json!({"paths": states.iter().map(|state| json!({"flow":"f","path":"main","execution":state})).collect::<Vec<_>>(), "failures": []})
    }

    #[test]
    fn verdict_follows_baseline_patch_and_after() {
        let green_report = validation(&["completed"]);
        let red_report = json!({"paths":[{"flow":"f","path":"main","execution":"failed"}],
            "failures":[{"flow":"f","reason":"execution_failed"}]});
        assert_eq!(
            decide(&red_report, true, Some(&red_report)).0,
            "baseline_not_green"
        );
        assert_eq!(decide(&validation(&[]), true, None).0, "baseline_not_green");
        assert_eq!(decide(&green_report, false, None).0, "patch_not_applicable");
        assert_eq!(decide(&green_report, true, Some(&green_report)).0, "missed");
        let (verdict, failed) = decide(&green_report, true, Some(&red_report));
        assert_eq!(verdict, "caught");
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
