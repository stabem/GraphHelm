//! The freeze rule, applied to THIS branch's real diff (M08).
//!
//! M06's binding decision 5 shipped `freeze_violation` as a pure check — and nothing ever
//! called it on a real changed-path list. It was exercised only by its own unit test, so a
//! pull request mixing the judge and the judged passed green. Our own words condemn that,
//! from `source_invariants`' header: *a documented control which no test enforces is not a
//! control*.
//!
//! This test is the enforcement. It runs inside the workspace-test stage the gate already
//! has, so wiring it needs no edit to `ci/gate.ps1` — which matters, because `ci/` is NOT
//! gate machinery by the rule's own list, and adding the stage there would have made the
//! enforcing commit violate the very rule it enforces.
//!
//! Scope, stated so nobody reads more into a green: this compares the branch against
//! `main`'s merge base. On `main` itself there is no diff and the check is vacuously clean —
//! that is correct (nothing is being proposed), not a hole.

use std::process::Command;

use graphhelm_quality::{GateManifestChange, freeze_violation, freeze_violation_with_lockfile};

/// Paths this branch changes relative to where it left `main`, or `None` when git cannot
/// answer (a tarball checkout, a shallow clone, no `main` ref). An unanswerable question is
/// reported as unanswerable — never as a clean bill of health.
/// The merge base against one ref, or `None` when git cannot answer for it.
fn merge_base(root: &std::path::Path, head: &str, reference: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["merge-base", head, reference])
        .current_dir(root)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).ok())
        .flatten()
        .map(|text| text.trim().to_owned())
}

/// Which ref answered, so an accusation can be audited rather than believed. A reader who
/// sees this check name files deserves to know WHICH `main` it measured against.
fn base_provenance(root: &std::path::Path, head: &str) -> String {
    match (
        merge_base(root, head, "origin/main"),
        merge_base(root, head, "main"),
    ) {
        (Some(remote), Some(local)) if remote != local => format!(
            "origin/main (base {remote}); the LOCAL main disagrees (base {local}) and was ignored — a stale local ref is a cache, never the authority"
        ),
        (Some(remote), _) => format!("origin/main (base {remote})"),
        (None, Some(local)) => format!("main (base {local}); origin/main did not resolve"),
        (None, None) => "no ref resolved".to_owned(),
    }
}

fn changed_paths() -> Option<(String, String, Vec<String>)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .parent()?
        .to_path_buf();
    // `origin/main` FIRST, and this is Agent B's finding paid for in a false accusation:
    // the local `main` ref lags by construction in a worktree setup that fetches without
    // checking out. Resolving against a stale `main` widens the merge base backwards, so
    // the "changed paths" list swells with files the branch never touched — and the check
    // ACCUSED a clean branch, naming two of them. A confident false positive erodes a rule
    // faster than a silent false negative, because it teaches the reader to ignore the
    // alarm. The remote ref is what `main` MEANS; the local one is a cache of it.
    let head_output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&root)
        .output()
        .ok()?;
    if !head_output.status.success() {
        return None;
    }
    let head = String::from_utf8(head_output.stdout)
        .ok()?
        .trim()
        .to_owned();
    let base =
        merge_base(&root, &head, "origin/main").or_else(|| merge_base(&root, &head, "main"))?;
    let diff = Command::new("git")
        .args(["diff", "--name-only", &base, &head])
        .current_dir(&root)
        .output()
        .ok()?;
    if !diff.status.success() {
        return None;
    }
    let paths = String::from_utf8(diff.stdout)
        .ok()?
        .lines()
        .map(str::to_owned)
        .filter(|line| !line.is_empty())
        .collect();
    Some((head, base, paths))
}

fn git_show(root: &std::path::Path, revision: &str, path: &str) -> Option<String> {
    let spec = format!("{revision}:{path}");
    let output = Command::new("git")
        .args(["show", &spec])
        .current_dir(root)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).ok())
        .flatten()
}

fn branch_freeze_violation(
    root: &std::path::Path,
    base: &str,
    head: &str,
    paths: &[String],
) -> Option<(String, String)> {
    let borrowed: Vec<&str> = paths.iter().map(String::as_str).collect();
    if !paths.iter().any(|path| path == "Cargo.lock") {
        return freeze_violation(&borrowed);
    }

    let Some(base_lockfile) = git_show(root, base, "Cargo.lock") else {
        return freeze_violation(&borrowed);
    };
    let Some(current_lockfile) = git_show(root, head, "Cargo.lock") else {
        return freeze_violation(&borrowed);
    };
    let mut storage = Vec::new();
    for path in paths
        .iter()
        .filter(|path| is_gate_path(path) && path.ends_with("Cargo.toml"))
    {
        let Some(base_manifest) = git_show(root, base, path) else {
            return freeze_violation(&borrowed);
        };
        let Some(current_manifest) = git_show(root, head, path) else {
            return freeze_violation(&borrowed);
        };
        storage.push((path.as_str(), base_manifest, current_manifest));
    }
    let manifests: Vec<GateManifestChange<'_>> = storage
        .iter()
        .map(
            |(path, base_manifest, current_manifest)| GateManifestChange {
                path,
                base: base_manifest,
                current: current_manifest,
            },
        )
        .collect();
    freeze_violation_with_lockfile(&borrowed, &base_lockfile, &current_lockfile, &manifests)
}

fn is_gate_path(path: &str) -> bool {
    freeze_violation(&[path, "README.md"]).is_some()
}

#[test]
fn a_benign_working_tree_copy_cannot_hide_the_committed_lock_delta() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);

    let root = std::env::temp_dir().join(format!(
        "graphhelm-freeze-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&root).unwrap();
    struct Remove(std::path::PathBuf);
    impl Drop for Remove {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _remove = Remove(root.clone());
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?}: {output:?}");
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };

    git(&["init", "-q"]);
    std::fs::create_dir_all(root.join("tools/pathogens")).unwrap();
    let manifest_base =
        "[package]\nname = \"pathogens\"\n[dependencies]\nserde = { workspace = true }\n";
    let manifest_head =
        format!("{manifest_base}graphhelm-policy = {{ path = \"../../core/policy\" }}\n");
    let lock_base = "version = 4\n\n[[package]]\nname = \"pathogens\"\nversion = \"0.1.0\"\ndependencies = [\"serde\"]\n\n[[package]]\nname = \"graphhelm-policy\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.0\"\n";
    let lock_head = lock_base.replace(
        "dependencies = [\"serde\"]",
        "dependencies = [\"serde\", \"graphhelm-policy\"]\nchecksum = \"unexpected\"",
    );
    std::fs::write(root.join("Cargo.lock"), lock_base).unwrap();
    std::fs::write(root.join("tools/pathogens/Cargo.toml"), manifest_base).unwrap();
    git(&["add", "."]);
    git(&[
        "-c",
        "user.name=Freeze Test",
        "-c",
        "user.email=freeze@example.invalid",
        "commit",
        "-qm",
        "base",
    ]);
    let base = git(&["rev-parse", "HEAD"]);

    std::fs::write(root.join("Cargo.lock"), lock_head).unwrap();
    std::fs::write(root.join("tools/pathogens/Cargo.toml"), manifest_head).unwrap();
    git(&["add", "."]);
    git(&[
        "-c",
        "user.name=Freeze Test",
        "-c",
        "user.email=freeze@example.invalid",
        "commit",
        "-qm",
        "head",
    ]);
    let head = git(&["rev-parse", "HEAD"]);
    let benign_working_copy = lock_base.replace(
        "dependencies = [\"serde\"]",
        "dependencies = [\"serde\", \"graphhelm-policy\"]",
    );
    std::fs::write(root.join("Cargo.lock"), benign_working_copy).unwrap();

    let paths = vec![
        "tools/pathogens/Cargo.toml".to_owned(),
        "Cargo.lock".to_owned(),
    ];
    let borrowed: Vec<&str> = paths.iter().map(String::as_str).collect();
    let manifest_base_from_commit = git_show(&root, &base, "tools/pathogens/Cargo.toml").unwrap();
    let manifest_head_from_commit = git_show(&root, &head, "tools/pathogens/Cargo.toml").unwrap();
    assert_eq!(
        freeze_violation_with_lockfile(
            &borrowed,
            lock_base,
            &std::fs::read_to_string(root.join("Cargo.lock")).unwrap(),
            &[GateManifestChange {
                path: "tools/pathogens/Cargo.toml",
                base: &manifest_base_from_commit,
                current: &manifest_head_from_commit,
            }],
        ),
        None,
        "control: the misleading working-tree lock is an otherwise qualifying gate-only delta"
    );
    assert!(
        branch_freeze_violation(&root, &base, &head, &paths).is_some(),
        "the committed checksum change remains visible despite a benign working-tree copy"
    );
}

#[test]
fn this_branch_does_not_move_the_judge_and_the_judged_together() {
    // The unanswerable case FAILS. The first version printed a line and returned — and a
    // Rust test that does not panic PASSES, with the print captured and never shown. So the
    // branch written to say "I cannot answer" was issuing a silent CLEAN BILL OF HEALTH,
    // and the only check stopping the judge and the judged from travelling together would
    // evaporate into a green on a shallow clone, a tarball, or any checkout without `main`.
    // Found by Agent B with a broken `git` on PATH — measured, not deduced. Third time in
    // one day that prose asserted what the check did not sustain; the first two were ours
    // too.
    //
    // If a legitimate environment ever needs to run without `main` reachable, that becomes
    // a DECLARED exception naming the environment — never a return that reads like approval.
    let Some((head, base, paths)) = changed_paths() else {
        panic!(
            "cannot ask whether this branch mixes the judge and the judged: git could not answer `merge-base HEAD main`. Refusing rather than passing — an unanswerable question is not evidence of a clean diff."
        )
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("the quality crate sits two levels below the workspace root");
    assert!(
        branch_freeze_violation(root, &base, &head, &paths).is_none(),
        "this branch moves gate machinery and gated code together, which M06's binding decision 5 forbids: {:?}. Measured against {}, over {} changed path(s). Split it into two pull requests — the judge and the judged never travel in one.",
        branch_freeze_violation(root, &base, &head, &paths),
        base_provenance(root, &head),
        paths.len()
    );
}
