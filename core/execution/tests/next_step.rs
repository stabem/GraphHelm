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
            ("journey-qa", Some("keel-journey-qa")),
            ("done", None),
        ]
    );
}

// ---- #2940: the journey QA hand-off ----

const QA: &str = "qa-1";

fn qa(sequence: u64, by: &str, flow: &str, verdict: &str) -> TaskEventRecord {
    record(
        sequence,
        "task.journey_qa",
        by,
        json!({"flow": flow, "verdict": verdict, "by": by}),
    )
}

#[test]
fn a_flow_handed_to_qa_waits_for_the_qa_not_the_author() {
    let next = next_of(&[
        claimed(1),
        planned(2, "none"),
        qa(3, LANE, "checkout", "requested"),
    ]);
    assert_eq!(next.step, "journey-qa");
    assert_eq!(next.skill, Some("keel-journey-qa"));
    assert_eq!(actor(&next), None);
    assert_eq!(next.because, "journey checkout waits for its QA");
}

#[test]
fn a_rejected_flow_sends_the_author_back_naming_the_flow() {
    let next = next_of(&[
        claimed(1),
        planned(2, "none"),
        qa(3, LANE, "checkout", "requested"),
        qa(4, QA, "checkout", "reject"),
    ]);
    assert_eq!(next.step, "implement");
    assert_eq!(next.skill, Some("implement"));
    assert_eq!(actor(&next), Some((LANE, "author")));
    assert_eq!(next.because, "journey QA rejected checkout");
}

#[test]
fn a_passed_flow_restores_the_answer_the_task_had() {
    let before = next_of(&[
        claimed(1),
        planned(2, "none"),
        opened(3, HEAD),
        assigned(4, HEAD),
    ]);
    let next = next_of(&[
        claimed(1),
        planned(2, "none"),
        opened(3, HEAD),
        assigned(4, HEAD),
        qa(5, LANE, "checkout", "requested"),
        qa(6, QA, "checkout", "pass"),
    ]);
    assert_eq!(next, before);
}

#[test]
fn a_request_after_a_reject_is_waiting_again_and_the_newest_record_per_flow_decides() {
    let next = next_of(&[
        claimed(1),
        planned(2, "none"),
        qa(3, LANE, "checkout", "requested"),
        qa(4, QA, "checkout", "reject"),
        qa(5, LANE, "checkout", "requested"),
    ]);
    assert_eq!(next.step, "journey-qa");
    assert_eq!(next.because, "journey checkout waits for its QA");
}

#[test]
fn with_several_flows_a_reject_comes_first_then_the_first_flow_by_id_waits() {
    let waiting = next_of(&[
        claimed(1),
        planned(2, "none"),
        qa(3, LANE, "zeta", "requested"),
        qa(4, LANE, "alpha", "requested"),
        qa(5, LANE, "mid", "pass"),
    ]);
    assert_eq!(waiting.step, "journey-qa");
    assert_eq!(waiting.because, "journey alpha waits for its QA");
    let rejected = next_of(&[
        claimed(1),
        planned(2, "none"),
        qa(3, LANE, "alpha", "requested"),
        qa(4, LANE, "zeta", "requested"),
        qa(5, QA, "zeta", "reject"),
        qa(6, QA, "mid", "reject"),
    ]);
    assert_eq!(rejected.step, "implement");
    assert_eq!(rejected.because, "journey QA rejected mid");
}

#[test]
fn a_flow_waiting_for_qa_is_named_in_review_too_and_a_block_stays_unmoved_after_a_pass() {
    let reviewing = next_of(&[
        claimed(1),
        planned(2, "none"),
        opened(3, HEAD),
        assigned(4, HEAD),
        qa(5, LANE, "checkout", "requested"),
    ]);
    assert_eq!(reviewing.step, "journey-qa");
    let blocked = next_of(&[
        claimed(1),
        planned(2, "none"),
        opened(3, HEAD),
        assigned(4, HEAD),
        verdict(5, REVIEWER, HEAD, "BLOCK"),
        qa(6, LANE, "checkout", "requested"),
        qa(7, QA, "checkout", "pass"),
    ]);
    assert_eq!(blocked.because, "BLOCK on head by rev-1");
}

#[test]
fn qa_records_never_touch_plan_critic_merge_merged_or_released() {
    let plan = next_of(&[claimed(1), qa(2, LANE, "checkout", "requested")]);
    assert_eq!(plan.step, "plan");
    let critic = next_of(&[
        claimed(1),
        planned(2, "design"),
        qa(3, LANE, "checkout", "requested"),
    ]);
    assert_eq!(critic.step, "critic");
    let merge = next_of(&[
        claimed(1),
        planned(2, "none"),
        opened(3, HEAD),
        assigned(4, HEAD),
        verdict(5, REVIEWER, HEAD, "APPROVE"),
        qa(6, LANE, "checkout", "requested"),
    ]);
    assert_eq!(merge.step, "merge");
    let released = record(
        3,
        "task.released",
        LANE,
        json!({"lane": LANE, "claimSequence": 1, "reason": "handing back"}),
    );
    let done = next_of(&[claimed(1), qa(2, LANE, "checkout", "requested"), released]);
    assert_eq!(done.step, "done");
    assert_eq!(done.because, "released by lane-a");
    let merged = record(
        7,
        "task.merged",
        REVIEWER,
        json!({"pr": 70, "mergeSha": "cccccccc", "closes": [7], "merger": REVIEWER}),
    );
    let finished = next_of(&[
        claimed(1),
        planned(2, "none"),
        opened(3, HEAD),
        assigned(4, HEAD),
        qa(5, LANE, "checkout", "reject"),
        verdict(6, REVIEWER, HEAD, "APPROVE"),
        merged,
    ]);
    assert_eq!(finished.step, "done");
    assert_eq!(finished.because, "merged");
}

#[test]
fn the_fold_serves_the_newest_record_per_flow_ordered_by_flow_with_who_and_digest() {
    let with_digest = record(
        4,
        "task.journey_qa",
        QA,
        json!({"flow": "alpha", "verdict": "pass", "by": QA, "digest": "sha256:abc123"}),
    );
    let tasks = fold_task_events(&[
        claimed(1),
        qa(2, LANE, "zeta", "requested"),
        qa(3, LANE, "alpha", "requested"),
        with_digest,
    ]);
    let served = serde_json::to_value(&tasks[0]).unwrap();
    assert_eq!(
        served["journeyQa"],
        json!([
            {"flow": "alpha", "verdict": "pass", "by": QA, "digest": "sha256:abc123"},
            {"flow": "zeta", "verdict": "requested", "by": LANE}
        ])
    );
    let none = serde_json::to_value(&fold_task_events(&[claimed(1)])[0]).unwrap();
    assert!(none.get("journeyQa").is_none());
}

#[test]
fn a_qa_record_naming_another_actor_or_a_bad_word_does_not_parse() {
    let parse = |document: Value| {
        let mut base = json!({"schema": "graphhelm-task-event-v1", "taskId": TASK, "revision": 1, "at": "2026-10-07T20:00:00Z"});
        for (key, value) in document.as_object().unwrap() {
            base[key] = value.clone();
        }
        parse_task_event("task.journey_qa", QA, &base.to_string(), 1, None)
    };
    assert!(parse(json!({"flow": "a", "verdict": "pass", "by": QA})).is_some());
    assert!(parse(json!({"flow": "a", "verdict": "pass", "by": LANE})).is_none());
    assert!(parse(json!({"flow": "a", "verdict": "approve", "by": QA})).is_none());
    assert!(parse(json!({"flow": "a/b", "verdict": "pass", "by": QA})).is_none());
    assert!(parse(json!({"flow": "a", "verdict": "pass", "by": QA, "digest": "x y"})).is_none());
    assert!(parse(json!({"flow": "a", "verdict": "pass", "by": QA, "note": "free"})).is_none());
}
