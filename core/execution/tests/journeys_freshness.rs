//! Journey capture freshness and arrows, folded against real temporary git repositories.

use std::path::Path;
use std::process::{Command, Stdio};

use graphhelm_execution::{
    ArrowState, CaptureRecord, ContractInput, Freshness, GitHistory, JourneysView, ScreenInput,
    StepInput, TransitionRecord, UnknownCause, Viewport, fold_journeys,
};

fn git_available() -> bool {
    let available = Command::new("git")
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .is_ok_and(|output| output.status.success());
    if !available {
        println!("skipped: `git --version` failed, git is not available on this host");
    }
    available
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("utf-8 output")
        .trim()
        .to_owned()
}

/// Writes `path` with `content` and commits it; returns the new HEAD.
fn commit_file(dir: &Path, path: &str, content: &str) -> String {
    let full = dir.join(path);
    std::fs::create_dir_all(full.parent().expect("a parent")).expect("mkdir");
    std::fs::write(&full, content).expect("write");
    git(dir, &["add", "--", path]);
    git(dir, &["commit", "-q", "-m", path]);
    git(dir, &["rev-parse", "HEAD"])
}

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    git(dir.path(), &["init", "-q"]);
    commit_file(dir.path(), "web/cart/Line.tsx", "v1");
    dir
}

fn screen(scope: &[&str]) -> Option<ScreenInput> {
    Some(ScreenInput {
        screen_id: "cart-screen".into(),
        title: "Cart".into(),
        scope_paths: scope.iter().map(|path| (*path).to_owned()).collect(),
    })
}

fn contract(steps: Vec<StepInput>) -> ContractInput {
    ContractInput {
        contract_id: "cart".into(),
        title: "Cart".into(),
        steps,
    }
}

fn step(id: &str, screen: Option<ScreenInput>) -> StepInput {
    StepInput {
        step_id: id.into(),
        screen,
        promises: Vec::new(),
        action: None,
        expected_states: Vec::new(),
    }
}

fn capture(id: &str, sequence: u64, step: &str, revision: &str, dirty: bool) -> CaptureRecord {
    CaptureRecord {
        signal_id: id.into(),
        execution_id: "run-a".into(),
        sequence,
        recorded_at_ms: 0,
        recorded_at: "2026-10-06T12:00:00Z".into(),
        image_evidence_id: format!("{id}-img"),
        contract_id: "cart".into(),
        step_id: step.into(),
        revision: revision.into(),
        dirty,
        viewport: Viewport {
            width: 800,
            height: 600,
        },
        observer: "agent-1".into(),
        pr: None,
        phase: None,
    }
}

fn transition(id: &str, from: (&str, &str), to: (&str, &str), revision: &str) -> TransitionRecord {
    TransitionRecord {
        signal_id: id.into(),
        execution_id: "run-a".into(),
        sequence: 100,
        recorded_at_ms: 0,
        contract_id: "cart".into(),
        from_step_id: from.0.into(),
        to_step_id: to.0.into(),
        revision: revision.into(),
        observer: "agent-1".into(),
        from_capture_id: from.1.into(),
        to_capture_id: to.1.into(),
    }
}

fn only_capture(view: &JourneysView) -> &graphhelm_execution::CaptureView {
    view.journeys[0].steps[0]
        .capture
        .as_ref()
        .expect("a capture is shown")
}

#[test]
fn a_capture_at_head_with_no_change_is_fresh() {
    if !git_available() {
        return;
    }
    let dir = repo();
    let head = git(dir.path(), &["rev-parse", "HEAD"]);
    let view = fold_journeys(
        &[contract(vec![step("view", screen(&["web/cart/"]))])],
        &[capture("c1", 1, "view", &head, false)],
        &[],
        &GitHistory::new(dir.path()),
    );
    assert_eq!(view.head.as_deref(), Some(head.as_str()));
    let shown = only_capture(&view);
    assert_eq!(shown.freshness, Freshness::Fresh);
    assert!(shown.changed_files.is_empty());
    assert_eq!(shown.unknown_cause, None);
}

#[test]
fn a_change_in_scope_is_stale_and_names_the_file_while_one_outside_stays_fresh() {
    if !git_available() {
        return;
    }
    let dir = repo();
    let captured = git(dir.path(), &["rev-parse", "HEAD"]);
    commit_file(dir.path(), "docs/readme.md", "outside");
    let history = GitHistory::new(dir.path());
    let contracts = [contract(vec![step("view", screen(&["web/cart/"]))])];
    let captures = [capture("c1", 1, "view", &captured, false)];

    let view = fold_journeys(&contracts, &captures, &[], &history);
    assert_eq!(only_capture(&view).freshness, Freshness::Fresh);

    commit_file(dir.path(), "web/cart/Line.tsx", "v2");
    let view = fold_journeys(&contracts, &captures, &[], &history);
    let shown = only_capture(&view);
    assert_eq!(shown.freshness, Freshness::Stale);
    assert_eq!(shown.changed_files, ["web/cart/Line.tsx"]);
}

#[test]
fn a_scope_path_does_not_match_a_longer_file_name_it_prefixes() {
    if !git_available() {
        return;
    }
    let dir = repo();
    let captured = git(dir.path(), &["rev-parse", "HEAD"]);
    commit_file(dir.path(), "web/cartography.ts", "map");
    let view = fold_journeys(
        &[contract(vec![step("view", screen(&["web/cart"]))])],
        &[capture("c1", 1, "view", &captured, false)],
        &[],
        &GitHistory::new(dir.path()),
    );
    assert_eq!(only_capture(&view).freshness, Freshness::Fresh);
}

#[test]
fn a_directory_that_is_not_a_repository_is_unknown_no_git() {
    if !git_available() {
        return;
    }
    let dir = tempfile::tempdir().expect("temp dir");
    let view = fold_journeys(
        &[contract(vec![step("view", screen(&["web/cart/"]))])],
        &[capture("c1", 1, "view", &"a".repeat(40), false)],
        &[],
        &GitHistory::new(dir.path()),
    );
    assert_eq!(view.head, None);
    let shown = only_capture(&view);
    assert_eq!(shown.freshness, Freshness::Unknown);
    assert_eq!(shown.unknown_cause, Some(UnknownCause::NoGit));
}

#[test]
fn a_well_formed_revision_absent_from_the_repository_is_unknown_revision_missing() {
    if !git_available() {
        return;
    }
    let dir = repo();
    let view = fold_journeys(
        &[contract(vec![step("view", screen(&["web/cart/"]))])],
        &[capture("c1", 1, "view", &"0".repeat(40), false)],
        &[],
        &GitHistory::new(dir.path()),
    );
    let shown = only_capture(&view);
    assert_eq!(shown.freshness, Freshness::Unknown);
    assert_eq!(shown.unknown_cause, Some(UnknownCause::RevisionMissing));
}

#[test]
fn a_step_without_a_screen_is_unknown_no_scope_paths() {
    if !git_available() {
        return;
    }
    let dir = repo();
    let head = git(dir.path(), &["rev-parse", "HEAD"]);
    let view = fold_journeys(
        &[contract(vec![
            step("view", None),
            step("next", screen(&[])),
        ])],
        &[
            capture("c1", 1, "view", &head, false),
            capture("c2", 2, "next", &head, false),
        ],
        &[],
        &GitHistory::new(dir.path()),
    );
    for step in &view.journeys[0].steps {
        let shown = step.capture.as_ref().expect("a capture");
        assert_eq!(shown.freshness, Freshness::Unknown);
        assert_eq!(shown.unknown_cause, Some(UnknownCause::NoScopePaths));
    }
}

#[test]
fn a_dirty_only_capture_is_unknown_and_a_clean_one_wins_over_a_newer_dirty_one() {
    if !git_available() {
        return;
    }
    let dir = repo();
    let head = git(dir.path(), &["rev-parse", "HEAD"]);
    let history = GitHistory::new(dir.path());
    let contracts = [contract(vec![step("view", screen(&["web/cart/"]))])];

    let view = fold_journeys(
        &contracts,
        &[capture("dirty", 5, "view", &head, true)],
        &[],
        &history,
    );
    let shown = only_capture(&view);
    assert_eq!(shown.signal_id, "dirty");
    assert_eq!(shown.freshness, Freshness::Unknown);
    assert_eq!(shown.unknown_cause, Some(UnknownCause::Dirty));

    let view = fold_journeys(
        &contracts,
        &[
            capture("clean", 1, "view", &head, false),
            capture("dirty", 5, "view", &head, true),
        ],
        &[],
        &history,
    );
    let shown = only_capture(&view);
    assert_eq!(shown.signal_id, "clean");
    assert_eq!(shown.freshness, Freshness::Fresh);
}

#[test]
fn arrows_are_walked_never_walked_or_stale() {
    if !git_available() {
        return;
    }
    let dir = repo();
    let captured = git(dir.path(), &["rev-parse", "HEAD"]);
    let history = GitHistory::new(dir.path());
    let contracts = [contract(vec![
        step("a", screen(&["web/a/"])),
        step("b", screen(&["web/cart/"])),
        step("c", screen(&["web/c/"])),
    ])];
    let captures = [
        capture("ca", 1, "a", &captured, false),
        capture("cb", 2, "b", &captured, false),
        capture("cc", 3, "c", &captured, false),
    ];
    let walked = [transition("t1", ("a", "ca"), ("b", "cb"), &captured)];

    let view = fold_journeys(&contracts, &captures, &walked, &history);
    let arrows = &view.journeys[0].arrows;
    assert_eq!(arrows.len(), 2);
    assert_eq!(arrows[0].state, ArrowState::Walked);
    assert_eq!(arrows[0].transition_signal_id.as_deref(), Some("t1"));
    assert_eq!(arrows[1].state, ArrowState::NeverWalked);
    assert_eq!(arrows[1].transition_signal_id, None);

    let missing = [transition(
        "t2",
        ("a", "ca"),
        ("b", "not-in-run"),
        &captured,
    )];
    let view = fold_journeys(&contracts, &captures, &missing, &history);
    assert_eq!(view.journeys[0].arrows[0].state, ArrowState::Stale);

    commit_file(dir.path(), "web/cart/Line.tsx", "v2");
    let view = fold_journeys(&contracts, &captures, &walked, &history);
    assert_eq!(view.journeys[0].arrows[0].state, ArrowState::Stale);
}

#[test]
fn the_view_serializes_in_the_served_shape() {
    if !git_available() {
        return;
    }
    let dir = repo();
    let head = git(dir.path(), &["rev-parse", "HEAD"]);
    let view = fold_journeys(
        &[contract(vec![step("view", None)])],
        &[capture("c1", 1, "view", &head, true)],
        &[],
        &GitHistory::new(dir.path()),
    );
    let value = serde_json::to_value(&view).expect("serializes");
    let shown = &value["journeys"][0]["steps"][0]["capture"];
    assert_eq!(shown["freshness"], "unknown");
    assert_eq!(shown["unknownCause"], "dirty");
    assert_eq!(shown["changedFiles"], serde_json::json!([]));
    assert_eq!(shown["imageEvidenceId"], "c1-img");
    assert!(shown.get("pr").is_none());
    assert_eq!(value["journeys"][0]["contractId"], "cart");
}

#[test]
fn promises_and_capture_sequence_reach_the_serialized_view() {
    if !git_available() {
        return;
    }
    let dir = repo();
    let mut first = step("pay", None);
    first.promises = vec!["Total is shown".into(), "Pay button works".into()];
    let contract = ContractInput {
        contract_id: "cart".into(),
        title: "Cart".into(),
        steps: vec![first],
    };
    let head = git(dir.path(), &["rev-parse", "HEAD"]);
    let captures = [capture("c1", 7, "pay", &head, false)];
    let view = fold_journeys(&[contract], &captures, &[], &GitHistory::new(dir.path()));
    let json = serde_json::to_value(&view).unwrap();
    let step = &json["journeys"][0]["steps"][0];
    assert_eq!(
        step["promises"],
        serde_json::json!(["Total is shown", "Pay button works"])
    );
    assert_eq!(step["capture"]["sequence"], 7);
}

/// #332: journeys are a project property. Captures from two executions fold into one map; the
/// newer one wins by recording time even when its in-execution sequence is lower, the view names
/// the execution it came from, and a newer capture that is stale is still reported stale.
#[test]
fn captures_from_two_executions_fold_newest_first_and_stale_is_still_detected() {
    if !git_available() {
        return;
    }
    let dir = repo();
    let first = git(dir.path(), &["rev-parse", "HEAD"]);
    let contracts = [contract(vec![step("view", screen(&["web/cart/"]))])];
    let baseline = CaptureRecord {
        execution_id: "baseline".into(),
        sequence: 90,
        recorded_at_ms: 1_000,
        recorded_at: "2026-10-06T12:00:01Z".into(),
        ..capture("c1", 90, "view", &first, false)
    };
    let later = CaptureRecord {
        execution_id: "later-run".into(),
        sequence: 3,
        recorded_at_ms: 2_000,
        recorded_at: "2026-10-06T12:00:02Z".into(),
        ..capture("c1", 3, "view", &first, false)
    };
    let history = GitHistory::new(dir.path());
    let view = fold_journeys(
        &contracts,
        &[later.clone(), baseline.clone()],
        &[],
        &history,
    );
    let shown = only_capture(&view);
    assert_eq!(shown.execution_id, "later-run");
    assert_eq!(shown.recorded_at, "2026-10-06T12:00:02Z");
    assert_eq!(shown.freshness, Freshness::Fresh);

    // Only the baseline captured: it still shows for the project.
    let view = fold_journeys(&contracts, &[baseline], &[], &history);
    assert_eq!(only_capture(&view).execution_id, "baseline");

    commit_file(dir.path(), "web/cart/Line.tsx", "v2");
    let view = fold_journeys(&contracts, &[later], &[], &history);
    let shown = only_capture(&view);
    assert_eq!(shown.freshness, Freshness::Stale);
    assert_eq!(shown.changed_files, ["web/cart/Line.tsx"]);
}

/// #332: a transition's capture citations resolve inside its own execution first, so an equal
/// signal id in another execution cannot stand in for the cited capture.
#[test]
fn a_transition_cites_captures_in_its_own_execution_first() {
    if !git_available() {
        return;
    }
    let dir = repo();
    let head = git(dir.path(), &["rev-parse", "HEAD"]);
    let contracts = [contract(vec![
        step("view", screen(&["web/cart/"])),
        step("pay", screen(&["web/pay/"])),
    ])];
    let in_run = |id: &str, step: &str, run: &str, revision: &str| CaptureRecord {
        execution_id: run.into(),
        ..capture(id, 1, step, revision, false)
    };
    let walked = TransitionRecord {
        execution_id: "run-b".into(),
        ..transition("t1", ("view", "c1"), ("pay", "c2"), &head)
    };
    // run-b's own c1 is at an unknown revision; run-a's c1 would be fresh.
    let captures = [
        in_run("c1", "view", "run-a", &head),
        in_run("c1", "view", "run-b", &"b".repeat(40)),
        in_run("c2", "pay", "run-b", &head),
    ];
    let view = fold_journeys(
        &contracts,
        &captures,
        &[walked],
        &GitHistory::new(dir.path()),
    );
    assert_eq!(view.journeys[0].arrows[0].state, ArrowState::Stale);
}
