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

use graphhelm_quality::freeze_violation;

/// Paths this branch changes relative to where it left `main`, or `None` when git cannot
/// answer (a tarball checkout, a shallow clone, no `main` ref). An unanswerable question is
/// reported as unanswerable — never as a clean bill of health.
/// The merge base against one ref, or `None` when git cannot answer for it.
fn merge_base(root: &std::path::Path, reference: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["merge-base", "HEAD", reference])
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
fn base_provenance(root: &std::path::Path) -> String {
    match (merge_base(root, "origin/main"), merge_base(root, "main")) {
        (Some(remote), Some(local)) if remote != local => format!(
            "origin/main (base {remote}); the LOCAL main disagrees (base {local}) and was              ignored — a stale local ref is a cache, never the authority"
        ),
        (Some(remote), _) => format!("origin/main (base {remote})"),
        (None, Some(local)) => format!("main (base {local}); origin/main did not resolve"),
        (None, None) => "no ref resolved".to_owned(),
    }
}

fn changed_paths() -> Option<Vec<String>> {
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
    let base = merge_base(&root, "origin/main").or_else(|| merge_base(&root, "main"))?;
    let diff = Command::new("git")
        .args(["diff", "--name-only", &base, "HEAD"])
        .current_dir(&root)
        .output()
        .ok()?;
    if !diff.status.success() {
        return None;
    }
    Some(
        String::from_utf8(diff.stdout)
            .ok()?
            .lines()
            .map(str::to_owned)
            .filter(|line| !line.is_empty())
            .collect(),
    )
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
    let Some(paths) = changed_paths() else {
        panic!(
            "cannot ask whether this branch mixes the judge and the judged: git could not              answer `merge-base HEAD main`. Refusing rather than passing — an unanswerable              question is not evidence of a clean diff."
        )
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("the quality crate sits two levels below the workspace root");
    let borrowed: Vec<&str> = paths.iter().map(String::as_str).collect();
    assert!(
        freeze_violation(&borrowed).is_none(),
        "this branch moves gate machinery and gated code together, which M06's binding          decision 5 forbids: {:?}. Measured against {}, over {} changed path(s). Split it          into two pull requests — the judge and the judged never travel in one.",
        freeze_violation(&borrowed),
        base_provenance(root),
        paths.len()
    );
}
