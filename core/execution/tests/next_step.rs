//! #2829: the `next` answer of a task (step, skill, actor, because). Every cell folds real task
//! records with the real fold and asks `next_step` about the slice the fold produced, so a state
//! the fold cannot reach is never asserted. Cost: pure functions, no I/O, milliseconds.

use graphhelm_execution::next::{NEXT_TABLE, NextStep, next_step};
use graphhelm_execution::tasks::{TaskEventRecord, fold_task_events, parse_task_event};
use serde_json::{Value, json};

const LANE: &str = "lane-a";
/// Assigned on the head by `assigned`.
const REVIEWER: &str = "rev-1";
/// Never assigned; the one who approves in the cells that must tell the two apart.
const APPROVER: &str = "rev-2";
const TASK: &str = "issue-7";
const HEAD: &str = "aaaaaaa1";
const FIX_HEAD: &str = "bbbbbbb2";
const URL: &str = "https://github.com/stabem/GraphHelm/pull/70#c1";

fn record(sequence: u64, kind: &str, actor: &str, document: Value) -> TaskEventRecord {
    let mut base = json!({"schema": "graphhelm-task-event-v1", "taskId": TASK, "revision": sequence, "at": "2026-10-07T20:00:00Z"});
    for (key, value) in document.as_object().unwrap() {
        base[key] = value.clone();
    }
    parse_task_event(kind, actor, &base.to_string(), sequence, None)
        .unwrap_or_else(|| panic!("fixture {kind} did not parse"))
}

fn claimed(sequence: u64) -> TaskEventRecord {
    record(
        sequence,
        "task.claimed",
        LANE,
        json!({"issue": 7, "lane": LANE, "branch": "issue-7-next"}),
    )
}

fn planned(sequence: u64, critic: &str) -> TaskEventRecord {
    record(
        sequence,
        "task.planned",
        LANE,
        json!({"lane": LANE, "classes": ["code"], "reviews": 1, "proof": "tests", "critic": {"mode": critic, "passScore": 8, "maxRounds": 3}, "summary": "Next"}),
    )
}

fn opened(sequence: u64, head: &str) -> TaskEventRecord {
    record(
        sequence,
        "task.pr_opened",
        LANE,
        json!({"pr": 70, "headSha": head, "journeys": [], "lane": LANE}),
    )
}

fn assigned(sequence: u64, head: &str) -> TaskEventRecord {
    record(
        sequence,
        "task.review_assigned",
        LANE,
        json!({"pr": 70, "headSha": head, "reviewer": REVIEWER, "lane": LANE}),
    )
}

fn verdict(sequence: u64, by: &str, head: &str, word: &str) -> TaskEventRecord {
    record(
        sequence,
        "task.review_verdict",
        by,
        json!({"pr": 70, "headSha": head, "reviewer": by, "verdict": word, "commentUrl": URL}),
    )
}

/// What `next_step` says about the one slice the fold makes of `records`.
fn next_of(records: &[TaskEventRecord]) -> NextStep {
    let tasks = fold_task_events(records);
    assert_eq!(tasks.len(), 1, "one slice: {tasks:?}");
    let next = next_step(&tasks[0]);
    assert!(
        NEXT_TABLE.contains(&(next.step, next.skill)),
        "{next:?} is not in the exported table"
    );
    next
}

fn actor(next: &NextStep) -> Option<(&str, &str)> {
    next.actor
        .as_ref()
        .map(|actor| (actor.id.as_str(), actor.role))
}

#[test]
fn a_claim_with_no_plan_is_the_authors_to_plan() {
    let next = next_of(&[claimed(1)]);
    assert_eq!(next.step, "plan");
    assert_eq!(next.skill, Some("task-plan"));
    assert_eq!(actor(&next), Some((LANE, "author")));
    assert_eq!(next.because, "claimed, no plan recorded");
}

#[test]
fn a_plan_with_no_pr_is_the_authors_to_implement() {
    let next = next_of(&[claimed(1), planned(2, "none")]);
    assert_eq!(next.step, "implement");
    assert_eq!(next.skill, Some("implement"));
    assert_eq!(actor(&next), Some((LANE, "author")));
    assert_eq!(next.because, "planned, no PR open on a head");
}

#[test]
fn a_design_plan_waits_for_its_critic_with_no_skill_and_no_actor() {
    let next = next_of(&[claimed(1), planned(2, "design")]);
    assert_eq!(next.step, "critic");
    assert_eq!(next.skill, None);
    assert_eq!(actor(&next), None);
    assert_eq!(next.because, "design plan waits for its critic");
}

#[test]
fn an_open_pr_without_a_reviewer_names_no_actor() {
    let next = next_of(&[claimed(1), planned(2, "none"), opened(3, HEAD)]);
    assert_eq!(next.step, "review");
    assert_eq!(next.skill, Some("blind-review"));
    assert_eq!(actor(&next), None);
    assert_eq!(next.because, "PR open on head, no reviewer assigned");
}

#[test]
fn an_open_pr_with_a_reviewer_is_the_reviewers_to_review() {
    let next = next_of(&[
        claimed(1),
        planned(2, "none"),
        opened(3, HEAD),
        assigned(4, HEAD),
    ]);
    assert_eq!(next.step, "review");
    assert_eq!(next.skill, Some("blind-review"));
    assert_eq!(actor(&next), Some((REVIEWER, "reviewer")));
    assert_eq!(next.because, "PR open on head, no verdict");
}

#[test]
fn a_block_on_the_head_sends_the_author_back_to_implement() {
    let next = next_of(&[
        claimed(1),
        planned(2, "none"),
        opened(3, HEAD),
        assigned(4, HEAD),
        verdict(5, REVIEWER, HEAD, "BLOCK"),
    ]);
    assert_eq!(next.step, "implement");
    assert_eq!(next.skill, Some("implement"));
    assert_eq!(actor(&next), Some((LANE, "author")));
    assert_eq!(next.because, "BLOCK on head by rev-1");
}

#[test]
fn a_new_head_after_a_block_is_back_in_review_until_a_reviewer_is_assigned_on_it() {
    let after_fix = [
        claimed(1),
        planned(2, "none"),
        opened(3, HEAD),
        assigned(4, HEAD),
        verdict(5, REVIEWER, HEAD, "BLOCK"),
        opened(6, FIX_HEAD),
    ];
    let next = next_of(&after_fix);
    assert_eq!(next.step, "review");
    assert_eq!(next.skill, Some("blind-review"));
    assert_eq!(
        actor(&next),
        None,
        "the old head's reviewer does not carry over"
    );
    assert_eq!(next.because, "PR open on head, no reviewer assigned");

    let mut reassigned = after_fix.to_vec();
    reassigned.push(assigned(7, FIX_HEAD));
    let next = next_of(&reassigned);
    assert_eq!(next.step, "review");
    assert_eq!(actor(&next), Some((REVIEWER, "reviewer")));
    assert_eq!(next.because, "PR open on head, no verdict");
}

#[test]
fn an_approval_hands_the_merge_to_the_approving_reviewer() {
    let next = next_of(&[
        claimed(1),
        planned(2, "none"),
        opened(3, HEAD),
        assigned(4, HEAD),
        verdict(5, REVIEWER, HEAD, "APPROVE"),
    ]);
    assert_eq!(next.step, "merge");
    assert_eq!(next.skill, Some("merge"));
    assert_eq!(actor(&next), Some((REVIEWER, "approver")));
    assert_eq!(next.because, "approved on head");
}

#[test]
fn an_approval_with_risk_says_so() {
    let next = next_of(&[
        claimed(1),
        planned(2, "none"),
        opened(3, HEAD),
        assigned(4, HEAD),
        verdict(5, APPROVER, HEAD, "APPROVE-WITH-RISK"),
    ]);
    assert_eq!(next.step, "merge");
    assert_eq!(next.skill, Some("merge"));
    assert_eq!(actor(&next), Some((APPROVER, "approver")));
    assert_eq!(next.because, "approved with risk on head");
}

#[test]
fn an_approval_by_someone_not_assigned_names_the_one_who_approved() {
    // The assigned reviewer is REVIEWER; APPROVER approves. The merge is APPROVER's.
    let next = next_of(&[
        claimed(1),
        planned(2, "none"),
        opened(3, HEAD),
        assigned(4, HEAD),
        verdict(5, APPROVER, HEAD, "APPROVE"),
    ]);
    assert_eq!(next.step, "merge");
    assert_eq!(actor(&next), Some((APPROVER, "approver")));
    assert_eq!(next.because, "approved on head");
}

#[test]
fn an_approval_with_nobody_assigned_still_names_the_one_who_approved() {
    let next = next_of(&[
        claimed(1),
        planned(2, "none"),
        opened(3, HEAD),
        verdict(4, APPROVER, HEAD, "APPROVE"),
    ]);
    assert_eq!(actor(&next), Some((APPROVER, "approver")));
}

#[test]
fn the_newest_approval_on_the_head_is_the_approver() {
    let next = next_of(&[
        claimed(1),
        planned(2, "none"),
        opened(3, HEAD),
        verdict(4, REVIEWER, HEAD, "APPROVE"),
        verdict(5, APPROVER, HEAD, "APPROVE"),
    ]);
    assert_eq!(actor(&next), Some((APPROVER, "approver")));
}

#[test]
fn the_authors_own_approval_is_never_a_merge_instruction() {
    let next = next_of(&[
        claimed(1),
        planned(2, "none"),
        opened(3, HEAD),
        assigned(4, HEAD),
        verdict(5, LANE, HEAD, "APPROVE"),
    ]);
    assert_eq!(next.step, "merge");
    assert_eq!(next.skill, Some("merge"));
    assert_eq!(actor(&next), None);
    assert_eq!(
        next.because,
        "approval is the author's own; another reviewer must approve"
    );
}

#[test]
fn a_reviewer_assignment_naming_the_author_names_no_reviewer() {
    let by_author = record(
        4,
        "task.review_assigned",
        LANE,
        json!({"pr": 70, "headSha": HEAD, "reviewer": LANE, "lane": LANE}),
    );
    let next = next_of(&[claimed(1), planned(2, "none"), opened(3, HEAD), by_author]);
    assert_eq!(next.step, "review");
    assert_eq!(next.skill, Some("blind-review"));
    assert_eq!(actor(&next), None);
    assert_eq!(
        next.because,
        "reviewer assigned is the author; another reviewer must be assigned"
    );
}

#[test]
fn an_approval_then_a_new_head_is_back_in_review_with_no_approver() {
    let tasks = fold_task_events(&[
        claimed(1),
        planned(2, "none"),
        opened(3, HEAD),
        assigned(4, HEAD),
        verdict(5, APPROVER, HEAD, "APPROVE"),
        opened(6, FIX_HEAD),
    ]);
    assert_eq!(tasks[0].approved_by, None);
    let next = next_step(&tasks[0]);
    assert_eq!(next.step, "review");
    assert_eq!(actor(&next), None);
    assert_eq!(next.because, "PR open on head, no reviewer assigned");
}

#[test]
fn an_approval_then_a_block_by_another_reviewer_sends_the_author_back() {
    let tasks = fold_task_events(&[
        claimed(1),
        planned(2, "none"),
        opened(3, HEAD),
        assigned(4, HEAD),
        verdict(5, APPROVER, HEAD, "APPROVE"),
        verdict(6, REVIEWER, HEAD, "BLOCK"),
    ]);
    assert_eq!(tasks[0].approved_by, None);
    let next = next_step(&tasks[0]);
    assert_eq!(next.step, "implement");
    assert_eq!(actor(&next), Some((LANE, "author")));
    assert_eq!(next.because, "BLOCK on head by rev-1");
}

#[test]
fn a_merged_task_is_done_with_nothing_to_load() {
    let merged = record(
        6,
        "task.merged",
        REVIEWER,
        json!({"pr": 70, "mergeSha": "cccccccc", "closes": [7], "merger": REVIEWER}),
    );
    let next = next_of(&[
        claimed(1),
        planned(2, "none"),
        opened(3, HEAD),
        assigned(4, HEAD),
        verdict(5, REVIEWER, HEAD, "APPROVE"),
        merged,
    ]);
    assert_eq!(next.step, "done");
    assert_eq!(next.skill, None);
    assert_eq!(actor(&next), None);
    assert_eq!(next.because, "merged");
}

#[test]
fn a_released_claim_is_done_and_says_who_released_it() {
    let released = record(
        2,
        "task.released",
        LANE,
        json!({"lane": LANE, "claimSequence": 1, "reason": "handing back"}),
    );
    let next = next_of(&[claimed(1), released]);
    assert_eq!(next.step, "done");
    assert_eq!(next.skill, None);
    assert_eq!(actor(&next), None);
    assert_eq!(next.because, "released by lane-a");
}

#[test]
fn a_task_with_no_lane_names_no_author() {
    // A slice opened by an assignment and a BLOCK alone (no claim, no pr_opened) has no lane.
    let assigned_without_lane = record(
        1,
        "task.review_assigned",
        LANE,
        json!({"pr": 70, "headSha": HEAD, "reviewer": REVIEWER}),
    );
    let next = next_of(&[assigned_without_lane, verdict(2, REVIEWER, HEAD, "BLOCK")]);
    assert_eq!(next.step, "implement");
    assert_eq!(actor(&next), None);
    assert_eq!(next.because, "BLOCK on head by rev-1");
}

#[test]
fn the_exported_table_is_exactly_what_the_function_can_say() {
    assert_eq!(
        NEXT_TABLE,
        &[
            ("plan", Some("task-plan")),
            ("critic", None),
            ("implement", Some("implement")),
            ("review", Some("blind-review")),
            ("merge", Some("merge")),
            ("done", None),
        ]
    );
}
