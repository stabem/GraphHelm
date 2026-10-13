//! #1149 (optimization plan A1): the golden corpus of the task fold. Every cell is one the Studio
//! fold (`apps/studio/src/runtime/team-task-events.test.ts`) already observed, ported so the Rust
//! fold that now serves the CLI, the MCP tool and the Studio draws the same slices from the same
//! records: a stale BLOCK after a newer head (#608), a merge that ends only its own slice (#460),
//! a plan before its claim (#880), heartbeats that move no step, releases that end one claim.
//! Cost: pure functions, no I/O, milliseconds.

use chrono::{DateTime, Duration, Utc};
use graphhelm_execution::tasks::{
    CriticOutcome, TaskEventRecord, TaskState, TaskStep, Verdict, fold_task_events,
    parse_task_event,
};
use graphhelm_protocols::PersistedTimestamp;
use serde_json::{Value, json};

fn record(sequence: u64, kind: &str, actor: &str, document: Value) -> TaskEventRecord {
    let mut base = json!({"schema": "graphhelm-task-event-v1", "taskId": "pr-384", "revision": sequence, "at": "2026-10-07T20:00:00Z"});
    for (key, value) in document.as_object().unwrap() {
        base[key] = value.clone();
    }
    parse_task_event(kind, actor, &base.to_string(), sequence, None)
        .unwrap_or_else(|| panic!("fixture {kind} did not parse"))
}

fn parse(kind: &str, actor: &str, document: Value) -> Option<TaskEventRecord> {
    parse_task_event(kind, actor, &document.to_string(), 1, None)
}

fn at(base: &str, minutes: i64) -> PersistedTimestamp {
    let when = DateTime::parse_from_rfc3339(base)
        .unwrap()
        .with_timezone(&Utc)
        + Duration::minutes(minutes);
    PersistedTimestamp::from_datetime(when).unwrap()
}

fn with_time(mut record: TaskEventRecord, when: PersistedTimestamp) -> TaskEventRecord {
    record.occurred_at = Some(when);
    record
}

fn step_of(state: &TaskState) -> TaskStep {
    state.step
}

fn blocked_head(state: &TaskState) -> Option<&str> {
    state.blocked_by.as_ref().map(|b| b.head_sha.as_str())
}

fn opened(sequence: u64, actor: &str, task: &str, pr: u64, head: &str) -> TaskEventRecord {
    record(
        sequence,
        "task.pr_opened",
        actor,
        json!({"taskId": task, "pr": pr, "headSha": head, "journeys": [], "lane": actor}),
    )
}

fn verdict(
    sequence: u64,
    reviewer: &str,
    task: &str,
    pr: u64,
    head: &str,
    word: &str,
    url: &str,
) -> TaskEventRecord {
    record(
        sequence,
        "task.review_verdict",
        reviewer,
        json!({"taskId": task, "pr": pr, "headSha": head, "reviewer": reviewer, "verdict": word, "commentUrl": url}),
    )
}

fn claimed(sequence: u64, lane: &str, task: &str, issue: u64, branch: &str) -> TaskEventRecord {
    record(
        sequence,
        "task.claimed",
        lane,
        json!({"taskId": task, "issue": issue, "lane": lane, "branch": branch}),
    )
}

fn merged(
    sequence: u64,
    merger: &str,
    task: &str,
    pr: u64,
    sha: &str,
    closes: &[u64],
) -> TaskEventRecord {
    record(
        sequence,
        "task.merged",
        merger,
        json!({"taskId": task, "pr": pr, "mergeSha": sha, "closes": closes, "merger": merger}),
    )
}

fn planned(sequence: u64, lane: &str, task: &str, mode: &str, summary: &str) -> TaskEventRecord {
    record(
        sequence,
        "task.planned",
        lane,
        json!({"taskId": task, "lane": lane, "classes": ["code"], "reviews": 1, "proof": "tests", "critic": {"mode": mode, "passScore": 8, "maxRounds": 3}, "summary": summary}),
    )
}

#[test]
fn heartbeats_mark_activity_without_changing_the_step_or_starting_a_blocked_fix() {
    let task = "issue-1024";
    let base = "2026-10-13T12:00:00Z";
    let event = |sequence: u64, kind: &str, actor: &str, fields: Value, minutes: i64| {
        let mut doc = json!({"taskId": task});
        for (k, v) in fields.as_object().unwrap() {
            doc[k] = v.clone();
        }
        with_time(record(sequence, kind, actor, doc), at(base, minutes))
    };
    let claim = event(
        1,
        "task.claimed",
        "codex-1",
        json!({"issue": 1024, "lane": "codex-1", "branch": "issue-1024-heartbeat"}),
        0,
    );
    let plan = event(
        2,
        "task.planned",
        "codex-1",
        json!({"lane": "codex-1", "classes": ["code"], "reviews": 1, "proof": "tests", "critic": {"mode": "none", "passScore": 8, "maxRounds": 3}, "summary": "Fold heartbeat activity"}),
        1,
    );
    let first = event(
        3,
        "task.heartbeat",
        "codex-1",
        json!({"lane": "codex-1"}),
        20,
    );
    let second = event(
        4,
        "task.heartbeat",
        "codex-1",
        json!({"lane": "codex-1"}),
        40,
    );
    let states = fold_task_events(&[claim.clone(), plan.clone(), first.clone(), second.clone()]);
    let state = &states[0];
    assert_eq!(state.step, TaskStep::Implement);
    assert_eq!(state.claim_sequence, Some(1));
    assert_eq!(state.author_last_sequence, Some(2));
    assert_eq!(state.last_sequence, 2);
    assert_eq!(state.last_heartbeat_at, Some(at(base, 40)));

    let foreign = event(
        5,
        "task.heartbeat",
        "codex-2",
        json!({"lane": "codex-2"}),
        50,
    );
    let state =
        &fold_task_events(&[claim.clone(), plan.clone(), first, second, foreign.clone()])[0];
    assert_eq!(state.last_heartbeat_at, Some(at(base, 40)));
    assert_eq!(state.last_sequence, 2);
    assert!(fold_task_events(&[foreign]).is_empty());

    let opened = event(
        6,
        "task.pr_opened",
        "codex-1",
        json!({"pr": 1120, "headSha": "aaaaaaaa", "journeys": [], "lane": "codex-1"}),
        60,
    );
    let block = event(
        7,
        "task.review_verdict",
        "codex-review",
        json!({"pr": 1120, "headSha": "aaaaaaaa", "reviewer": "codex-review", "verdict": "BLOCK", "commentUrl": "https://github.com/stabem/GraphHelm/pull/1120#c1"}),
        70,
    );
    let after_block = event(
        8,
        "task.heartbeat",
        "codex-1",
        json!({"lane": "codex-1"}),
        80,
    );
    let blocked = &fold_task_events(&[claim, plan, opened, block, after_block])[0];
    assert_eq!(blocked.step, TaskStep::Review);
    assert_eq!(blocked.blocked_sequence, Some(7));
    assert_eq!(blocked.author_last_sequence, Some(6));
    assert_eq!(blocked.last_heartbeat_at, Some(at(base, 80)));
}

#[test]
fn recovers_an_issue_from_task_id_when_the_claim_is_outside_the_page_993() {
    let tasks = fold_task_events(&[
        opened(1, "codex-6", "issue-913", 914, "aaaaaaaa"),
        verdict(
            2,
            "gh-claude-4",
            "issue-913",
            914,
            "aaaaaaaa",
            "APPROVE",
            "https://github.com/stabem/GraphHelm/pull/914#c1",
        ),
        opened(3, "codex-6", "pr-384", 384, "bbbbbbbb"),
    ]);
    let first = tasks.iter().find(|t| t.task_id == "issue-913").unwrap();
    assert_eq!((first.issue, first.pr), (Some(913), Some(914)));
    assert_eq!(
        tasks.iter().find(|t| t.task_id == "pr-384").unwrap().issue,
        None
    );
}

#[test]
fn attaches_a_plan_recorded_before_its_claim_to_the_pr_that_later_merges_880() {
    let task = "issue-758";
    let tasks = fold_task_events(&[
        record(
            4864,
            "task.planned",
            "codex-13",
            json!({"taskId": task, "lane": "codex-13", "classes": ["code"], "reviews": 1, "proof": "both", "critic": {"mode": "none", "passScore": 8, "maxRounds": 3}, "summary": "Plan"}),
        ),
        claimed(4865, "codex-13", task, 758, "issue-758-evidence-cache"),
        opened(4877, "codex-13", task, 760, "aaaaaaaa"),
        merged(4905, "codexrev-1", task, 760, "bbbbbbbb", &[758]),
    ]);
    assert_eq!(tasks.len(), 1);
    assert_eq!(
        (tasks[0].issue, tasks[0].pr, tasks[0].step),
        (Some(758), Some(760), TaskStep::Merged)
    );
}

#[test]
fn does_not_reopen_a_merged_pr_on_a_later_claim_881() {
    let task = "issue-509";
    let tasks = fold_task_events(&[
        claimed(
            3460,
            "gh-claude-9",
            task,
            509,
            "issue-509-journeys-plain-words",
        ),
        opened(3475, "gh-claude-9", task, 516, "aaaaaaaa"),
        verdict(
            3486,
            "gh-claude-6",
            task,
            516,
            "aaaaaaaa",
            "APPROVE",
            "https://github.com/stabem/GraphHelm/pull/516#c1",
        ),
        merged(3487, "gh-claude-6", task, 516, "bbbbbbbb", &[509]),
        claimed(
            3528,
            "gh-claude-9",
            "issue-518",
            518,
            "issue-518-mark-act-safe",
        ),
    ]);
    let pr516: Vec<_> = tasks.iter().filter(|t| t.pr == Some(516)).collect();
    assert_eq!(pr516.len(), 1);
    assert_eq!(
        (pr516[0].step, pr516[0].issue),
        (TaskStep::Merged, Some(509))
    );
    assert!(tasks.iter().all(|t| !(t.task_id == task && t.pr.is_none())));
}

#[test]
fn keeps_an_interrupted_pre_claim_plan_out_of_a_later_claim_881() {
    let task = "issue-100";
    let tasks = fold_task_events(&[
        planned(1, "lane-a", task, "none", "Old plan"),
        claimed(2, "lane-b", task, 100, "issue-100-other"),
        claimed(3, "lane-a", task, 100, "issue-100-new"),
    ]);
    assert!(
        tasks
            .iter()
            .find(|t| t.branch.as_deref() == Some("issue-100-new"))
            .unwrap()
            .plan
            .is_none()
    );
}

#[test]
fn drops_self_assigned_or_malformed_claimed_assigners_86() {
    for assigned_by in [
        json!("lane"),
        json!("x".repeat(129)),
        json!(""),
        Value::Null,
        json!("bad\nname"),
    ] {
        assert!(parse("task.claimed", "lane", json!({"schema": "graphhelm-task-event-v1", "taskId": "issue-86", "revision": 1, "issue": 86, "lane": "lane", "branch": "issue-86-tree", "assignedBy": assigned_by})).is_none());
    }
    let assigned_by = "é".repeat(128);
    let parsed = parse("task.claimed", "lane", json!({"schema": "graphhelm-task-event-v1", "taskId": "issue-86", "revision": 1, "issue": 86, "lane": "lane", "branch": "issue-86-tree", "assignedBy": assigned_by})).unwrap();
    assert_eq!(parsed.assigned_by.as_deref(), Some(assigned_by.as_str()));
}

#[test]
fn folds_pr_opened_block_new_head_approve_and_merged_into_the_step_sequence() {
    let records = [
        opened(1, "gh-claude-4", "pr-384", 384, "aaaaaaaa"),
        verdict(
            2,
            "gh-claude-1",
            "pr-384",
            384,
            "aaaaaaaa",
            "BLOCK",
            "https://github.com/stabem/GraphHelm/pull/384#c1",
        ),
        opened(3, "gh-claude-4", "pr-384", 384, "bbbbbbbb"),
        verdict(
            4,
            "gh-claude-1",
            "pr-384",
            384,
            "bbbbbbbb",
            "APPROVE",
            "https://github.com/stabem/GraphHelm/pull/384#c2",
        ),
        merged(5, "gh-claude-1", "pr-384", 384, "cccccccc", &[382]),
    ];
    let states: Vec<TaskState> = (0..records.len())
        .map(|n| fold_task_events(&records[..=n]).remove(0))
        .collect();
    let seen: Vec<(TaskStep, Option<&str>)> = states
        .iter()
        .map(|s| (step_of(s), blocked_head(s)))
        .collect();
    assert_eq!(
        seen,
        vec![
            (TaskStep::Review, None),
            (TaskStep::Review, Some("aaaaaaaa")),
            // #608: the pushed head answers the BLOCK; it is the re-review's head, no longer blocked.
            (TaskStep::Review, None),
            (TaskStep::Merge, None),
            (TaskStep::Merged, None),
        ]
    );
    assert_eq!(
        states[1].blocked_by.as_ref().unwrap().reviewer,
        "gh-claude-1"
    );
    assert_eq!(states[2].head_sha.as_deref(), Some("bbbbbbbb"));
    assert_eq!(states[4].task_id, "pr-384");
    assert_eq!(states[4].pr, Some(384));
    assert_eq!(states[4].lane.as_deref(), Some("gh-claude-4"));
    assert_eq!(states[4].merge_sha.as_deref(), Some("cccccccc"));
}

/// #608: a `blockedBy` cleared only by a verdict drew a pushed fix as "blocked" everywhere.
#[test]
fn a_newer_head_after_a_block_reads_as_the_re_review_not_as_blocked_608() {
    let opened_a = opened(1, "gh-claude-4", "pr-384", 608, "aaaaaaaa");
    let assigned = record(
        2,
        "task.review_assigned",
        "gh-claude-4",
        json!({"pr": 608, "headSha": "aaaaaaaa", "reviewer": "gh-claude-5", "lane": "gh-claude-4"}),
    );
    let block = verdict(
        3,
        "gh-claude-5",
        "pr-384",
        608,
        "aaaaaaaa",
        "BLOCK",
        "https://github.com/stabem/GraphHelm/pull/608#c1",
    );
    let same = opened(4, "gh-claude-4", "pr-384", 608, "aaaaaaaa");
    let fix = opened(5, "gh-claude-4", "pr-384", 608, "bbbbbbbb");

    assert_eq!(
        blocked_head(&fold_task_events(&[opened_a.clone(), assigned.clone(), block.clone()])[0]),
        Some("aaaaaaaa")
    );
    assert_eq!(
        blocked_head(
            &fold_task_events(&[opened_a.clone(), assigned.clone(), block.clone(), same])[0]
        ),
        Some("aaaaaaaa")
    );

    let pushed = &fold_task_events(&[opened_a, assigned, block, fix])[0];
    assert!(pushed.blocked_by.is_none());
    assert_eq!(pushed.step, TaskStep::Review);
    assert_eq!(pushed.head_sha.as_deref(), Some("bbbbbbbb"));
    assert_eq!(pushed.reviewers, vec!["gh-claude-5"]);
    assert_eq!(pushed.rounds.len(), 1);
    assert_eq!(pushed.rounds[0].reviewer, "gh-claude-5");
    assert_eq!(pushed.rounds[0].head_sha, "aaaaaaaa");
    assert_eq!(pushed.rounds[0].fix_head.as_deref(), Some("bbbbbbbb"));
}

#[test]
fn tracks_the_newest_pushed_head_as_the_fix_before_the_next_verdict_990() {
    let state = &fold_task_events(&[
        opened(1, "codex-1", "pr-384", 990, "cccccccc"),
        verdict(
            2,
            "codex-review",
            "pr-384",
            990,
            "cccccccc",
            "BLOCK",
            "https://github.com/stabem/GraphHelm/pull/990#c1",
        ),
        opened(3, "codex-1", "pr-384", 990, "dddddddd"),
        opened(4, "codex-1", "pr-384", 990, "eeeeeeee"),
    ])[0];
    assert_eq!(
        state.rounds.last().unwrap().fix_head.as_deref(),
        Some("eeeeeeee")
    );
    assert_eq!(state.head_sha.as_deref(), Some("eeeeeeee"));
}

#[test]
fn keeps_a_block_until_its_reviewer_answers_1002() {
    let task = "issue-1002";
    let o = opened(1, "codex-1", task, 1002, "aaaaaaaa");
    let block = verdict(
        2,
        "reviewer-one",
        task,
        1002,
        "aaaaaaaa",
        "BLOCK",
        "https://github.com/stabem/GraphHelm/pull/1002#c1",
    );
    let other = verdict(
        3,
        "reviewer-two",
        task,
        1002,
        "aaaaaaaa",
        "APPROVE",
        "https://github.com/stabem/GraphHelm/pull/1002#c2",
    );
    let own = verdict(
        4,
        "reviewer-one",
        task,
        1002,
        "aaaaaaaa",
        "APPROVE",
        "https://github.com/stabem/GraphHelm/pull/1002#c3",
    );
    let s = &fold_task_events(&[o.clone(), block.clone(), other.clone()])[0];
    assert_eq!(s.step, TaskStep::Review);
    assert_eq!(s.blocked_by.as_ref().unwrap().reviewer, "reviewer-one");
    let s = &fold_task_events(&[o, block, other, own])[0];
    assert_eq!((s.step, s.blocked_by.is_none()), (TaskStep::Merge, true));
}

#[test]
fn keeps_every_reviewers_unanswered_block_on_the_current_head_1002() {
    let task = "issue-1002";
    let url_one = "https://github.com/stabem/GraphHelm/pull/1002#c1";
    let url_two = "https://github.com/stabem/GraphHelm/pull/1002#c2";
    let o = opened(1, "codex-1", task, 1002, "aaaaaaaa");
    let block_one = verdict(2, "reviewer-one", task, 1002, "aaaaaaaa", "BLOCK", url_one);
    let block_two = verdict(3, "reviewer-two", task, 1002, "aaaaaaaa", "BLOCK", url_two);
    let approve_two = verdict(
        4,
        "reviewer-two",
        task,
        1002,
        "aaaaaaaa",
        "APPROVE",
        "https://github.com/stabem/GraphHelm/pull/1002#c3",
    );
    let approve_one = verdict(
        5,
        "reviewer-one",
        task,
        1002,
        "aaaaaaaa",
        "APPROVE",
        "https://github.com/stabem/GraphHelm/pull/1002#c4",
    );

    let s = &fold_task_events(&[
        o.clone(),
        block_one.clone(),
        block_two.clone(),
        approve_two.clone(),
    ])[0];
    assert_eq!(s.step, TaskStep::Review);
    assert_eq!(
        (
            s.blocked_by.as_ref().unwrap().reviewer.as_str(),
            s.blocked_by.as_ref().unwrap().comment_url.as_str(),
            s.blocked_sequence
        ),
        ("reviewer-one", url_one, Some(2))
    );
    let s = &fold_task_events(&[
        o.clone(),
        block_one.clone(),
        block_two.clone(),
        approve_one.clone(),
    ])[0];
    assert_eq!(
        (
            s.blocked_by.as_ref().unwrap().reviewer.as_str(),
            s.blocked_by.as_ref().unwrap().comment_url.as_str(),
            s.blocked_sequence
        ),
        ("reviewer-two", url_two, Some(3))
    );
    let s = &fold_task_events(&[o, block_one, block_two, approve_two, approve_one])[0];
    assert_eq!(
        (s.step, s.blocked_by.is_none(), s.blocked_sequence),
        (TaskStep::Merge, true, None)
    );
}

#[test]
fn records_approve_with_risk_and_preserves_it_after_a_plain_approval_1002() {
    let task = "issue-1002";
    let o = opened(1, "codex-1", task, 1002, "aaaaaaaa");
    let risk = verdict(
        2,
        "reviewer-two",
        task,
        1002,
        "aaaaaaaa",
        "APPROVE-WITH-RISK",
        "https://github.com/stabem/GraphHelm/pull/1002#c4",
    );
    let plain = verdict(
        3,
        "reviewer-one",
        task,
        1002,
        "aaaaaaaa",
        "APPROVE",
        "https://github.com/stabem/GraphHelm/pull/1002#c5",
    );
    let s = &fold_task_events(&[o.clone(), risk.clone()])[0];
    assert_eq!(s.step, TaskStep::Merge);
    let awr = s.approved_with_risk.as_ref().unwrap();
    assert_eq!(
        (awr.reviewer.as_str(), awr.comment_url.as_str()),
        (
            "reviewer-two",
            "https://github.com/stabem/GraphHelm/pull/1002#c4"
        )
    );
    let s = &fold_task_events(&[o, risk, plain])[0];
    assert_eq!(
        s.approved_with_risk.as_ref().unwrap().reviewer,
        "reviewer-two"
    );
}

#[test]
fn routes_a_shared_pr_review_by_the_head_named_in_the_event_1002() {
    let states = fold_task_events(&[
        claimed(1, "lane-a", "issue-1002-a", 1002, "a"),
        opened(2, "lane-a", "issue-1002-a", 1002, "aaaaaaaa"),
        claimed(3, "lane-b", "issue-1002-b", 1002, "b"),
        opened(4, "lane-b", "issue-1002-b", 1002, "bbbbbbbb"),
        verdict(
            5,
            "reviewer",
            "issue-1002-b",
            1002,
            "bbbbbbbb",
            "APPROVE",
            "https://github.com/stabem/GraphHelm/pull/1002#c1",
        ),
    ]);
    let a = states
        .iter()
        .find(|s| s.head_sha.as_deref() == Some("aaaaaaaa"))
        .unwrap();
    let b = states
        .iter()
        .find(|s| s.head_sha.as_deref() == Some("bbbbbbbb"))
        .unwrap();
    assert_eq!((a.step, a.blocked_by.is_none()), (TaskStep::Review, true));
    assert_eq!((b.step, b.blocked_by.is_none()), (TaskStep::Merge, true));
}

#[test]
fn keeps_an_approved_same_head_repeat_in_merge_and_preserves_its_review_clock_1002() {
    let t = |s: &str| PersistedTimestamp::parse(s).unwrap();
    let first = with_time(
        record(
            1,
            "task.pr_opened",
            "codex-3",
            json!({"pr": 1002, "headSha": "aaaaaaaa", "journeys": [], "lane": "codex-3", "title": "Before", "summary": "Before summary"}),
        ),
        t("2026-10-10T00:00:00Z"),
    );
    let approve = with_time(
        verdict(
            2,
            "codex-review",
            "pr-384",
            1002,
            "aaaaaaaa",
            "APPROVE",
            "https://github.com/stabem/GraphHelm/pull/1002#c1",
        ),
        t("2026-10-10T00:01:00Z"),
    );
    let repeat = with_time(
        record(
            3,
            "task.pr_opened",
            "codex-3",
            json!({"pr": 1002, "headSha": "aaaaaaaa", "journeys": [], "lane": "codex-3", "title": "After", "summary": "Updated summary"}),
        ),
        t("2026-10-10T01:00:00Z"),
    );
    let s = &fold_task_events(&[first.clone(), approve.clone(), repeat])[0];
    assert_eq!(s.step, TaskStep::Merge);
    assert_eq!(s.clock.since, Some(t("2026-10-10T00:01:00Z")));
    // The one minute in Review is booked; the same-head repeat books nothing more.
    assert_eq!(s.clock.spent.get("review"), Some(&60_000));
    assert_eq!(s.clock.spent.len(), 1);
    assert_eq!(s.clock.last_at, Some(t("2026-10-10T01:00:00Z")));
    assert_eq!(s.pr_title.as_deref(), Some("After"));
    assert_eq!(s.pr_summary.as_deref(), Some("Updated summary"));
    let new_head = with_time(
        record(
            3,
            "task.pr_opened",
            "codex-3",
            json!({"pr": 1002, "headSha": "bbbbbbbb", "journeys": [], "lane": "codex-3", "title": "After", "summary": "Updated summary"}),
        ),
        t("2026-10-10T01:00:00Z"),
    );
    assert_eq!(
        fold_task_events(&[first, approve, new_head])[0].step,
        TaskStep::Review
    );
}

#[test]
fn keeps_an_early_assignment_for_an_unseen_fix_head_until_that_head_opens_983() {
    let task = "issue-983";
    let records = [
        claimed(1, "codex-1", task, 983, "issue-983-fix"),
        opened(2, "codex-1", task, 1004, "aaaaaaaa"),
        verdict(
            3,
            "codex-review",
            task,
            1004,
            "aaaaaaaa",
            "BLOCK",
            "https://github.com/stabem/GraphHelm/pull/1004#c1",
        ),
        record(
            4,
            "task.review_assigned",
            "codex-1",
            json!({"taskId": task, "pr": 1004, "headSha": "bbbbbbbb", "reviewer": "codexrev-3"}),
        ),
        opened(5, "codex-1", task, 1004, "bbbbbbbb"),
    ];
    let pending = &fold_task_events(&records[..4])[0];
    assert_eq!(pending.head_sha.as_deref(), Some("aaaaaaaa"));
    assert!(pending.current_reviewer.is_none());
    assert_eq!(blocked_head(pending), Some("aaaaaaaa"));
    let after = &fold_task_events(&records)[0];
    assert_eq!(after.head_sha.as_deref(), Some("bbbbbbbb"));
    assert_eq!(after.current_reviewer.as_deref(), Some("codexrev-3"));
    assert_eq!(
        (after.step, after.blocked_by.is_none()),
        (TaskStep::Review, true)
    );
    assert_eq!(
        after.rounds.last().unwrap().fix_head.as_deref(),
        Some("bbbbbbbb")
    );
    let stale = [
        opened(1, "codex-1", task, 1004, "aaaaaaaa"),
        verdict(
            2,
            "codex-review",
            task,
            1004,
            "aaaaaaaa",
            "BLOCK",
            "https://github.com/stabem/GraphHelm/pull/1004#c1",
        ),
        opened(3, "codex-1", task, 1004, "bbbbbbbb"),
        record(
            4,
            "task.review_assigned",
            "codex-1",
            json!({"taskId": task, "pr": 1004, "headSha": "aaaaaaaa", "reviewer": "old-reviewer"}),
        ),
        opened(5, "codex-1", task, 1004, "cccccccc"),
    ];
    let s = &fold_task_events(&stale)[0];
    assert_eq!(s.head_sha.as_deref(), Some("cccccccc"));
    assert!(s.current_reviewer.is_none());
    assert_eq!((s.step, s.blocked_by.is_none()), (TaskStep::Review, true));
}

#[test]
fn links_the_task_to_the_journeys_its_claim_names_and_an_empty_pr_opened_list_keeps_them_577() {
    let claim = record(
        1,
        "task.claimed",
        "gh-claude-4",
        json!({"issue": 577, "lane": "gh-claude-4", "branch": "issue-577-x", "journeys": ["studio-graph-tab"]}),
    );
    assert_eq!(
        fold_task_events(std::slice::from_ref(&claim))[0].journeys,
        vec!["studio-graph-tab"]
    );
    let o = opened(2, "gh-claude-4", "pr-384", 600, "aaaaaaaa");
    assert_eq!(
        fold_task_events(&[claim.clone(), o.clone()])[0].journeys,
        vec!["studio-graph-tab"]
    );
    let renamed = record(
        3,
        "task.pr_opened",
        "gh-claude-4",
        json!({"pr": 600, "headSha": "bbbbbbbb", "journeys": ["studio-connect"], "lane": "gh-claude-4"}),
    );
    assert_eq!(
        fold_task_events(&[claim, o, renamed])[0].journeys,
        vec!["studio-connect"]
    );
}

#[test]
fn keeps_a_claim_whose_journeys_field_is_malformed_and_drops_only_the_field_577() {
    let claim = record(
        1,
        "task.claimed",
        "gh-claude-4",
        json!({"issue": 577, "lane": "gh-claude-4", "branch": "issue-577-x", "journeys": ["ok", "bad id!"]}),
    );
    assert!(claim.journeys.is_none());
    let s = &fold_task_events(&[claim])[0];
    assert_eq!(s.issue, Some(577));
    assert!(s.journeys.is_empty());
}

#[test]
fn refuses_a_record_whose_lane_names_another_lane_than_the_recording_actor() {
    assert!(parse("task.pr_opened", "agent-chat", json!({"schema": "graphhelm-task-event-v1", "taskId": "pr-1", "revision": 1, "at": "2026-10-07T20:00:00Z", "pr": 1, "headSha": "aaaaaaaa", "journeys": [], "lane": "gh-claude-4"})).is_none());
}

#[test]
fn parses_a_release_only_for_its_recording_lane_and_exact_positive_claim_sequence() {
    let doc = json!({"schema": "graphhelm-task-event-v1", "taskId": "issue-729", "revision": 2, "at": "now", "lane": "lane-a", "claimSequence": 4, "reason": "Work moved"});
    let ok = parse("task.released", "lane-a", doc.clone()).unwrap();
    assert_eq!(ok.claim_sequence, Some(4));
    assert!(parse("task.released", "lane-b", doc.clone()).is_none());
    let mut zero = doc.clone();
    zero["claimSequence"] = json!(0);
    assert!(parse("task.released", "lane-a", zero).is_none());
    let mut empty = doc;
    empty["reason"] = json!("");
    assert!(parse("task.released", "lane-a", empty).is_none());
}

fn released(sequence: u64, lane: &str, claim: u64, reason: &str) -> TaskEventRecord {
    record(
        sequence,
        "task.released",
        lane,
        json!({"taskId": "issue-729", "lane": lane, "claimSequence": claim, "reason": reason}),
    )
}

#[test]
fn a_release_ends_only_its_latest_open_claim_while_other_claims_stay_open() {
    let folded = fold_task_events(&[
        claimed(10, "lane-a", "issue-729", 729, "old"),
        claimed(12, "lane-a", "issue-729", 729, "new"),
        claimed(13, "lane-b", "issue-729", 729, "foreign"),
        released(14, "lane-a", 12, "Stopped"),
    ]);
    let seen: Vec<(Option<&str>, TaskStep, Option<&str>)> = folded
        .iter()
        .map(|t| (t.branch.as_deref(), t.step, t.released_by.as_deref()))
        .collect();
    assert_eq!(
        seen,
        vec![
            (Some("old"), TaskStep::Plan, None),
            (Some("new"), TaskStep::Plan, Some("lane-a")),
            (Some("foreign"), TaskStep::Plan, None)
        ]
    );
}

#[test]
fn does_not_release_a_claim_after_that_claim_has_a_pr() {
    let s = &fold_task_events(&[
        claimed(10, "lane-a", "issue-729", 729, "old"),
        record(
            11,
            "task.pr_opened",
            "lane-a",
            json!({"taskId": "issue-729", "pr": 800, "headSha": "aaaaaaa", "journeys": [], "lane": "lane-a"}),
        ),
        released(12, "lane-a", 10, "Too late"),
    ])[0];
    assert_eq!((s.pr, s.step), (Some(800), TaskStep::Review));
}

#[test]
fn a_new_claim_after_release_stays_a_separate_active_slice() {
    let tasks = fold_task_events(&[
        claimed(10, "lane-a", "issue-729", 729, "same"),
        released(11, "lane-a", 10, "Stopped"),
        claimed(12, "lane-a", "issue-729", 729, "same"),
    ]);
    let seen: Vec<(Option<&str>, Option<&str>)> = tasks
        .iter()
        .map(|t| (t.branch.as_deref(), t.released_by.as_deref()))
        .collect();
    assert_eq!(
        seen,
        vec![(Some("same"), Some("lane-a")), (Some("same"), None)]
    );
}

#[test]
fn routes_a_review_record_to_the_new_claim_instead_of_the_released_slice_851() {
    let task = "issue-851";
    let slices = fold_task_events(&[
        claimed(1, "lane-a", task, 851, "old"),
        record(
            2,
            "task.released",
            "lane-a",
            json!({"taskId": task, "lane": "lane-a", "claimSequence": 1, "reason": "Handed over"}),
        ),
        claimed(3, "lane-b", task, 851, "new"),
        record(
            4,
            "task.review_assigned",
            "lane-b",
            json!({"taskId": task, "pr": 852, "headSha": "aaaaaaaa", "reviewer": "reviewer"}),
        ),
    ]);
    assert_eq!(slices.len(), 2);
    let old = slices
        .iter()
        .find(|s| s.branch.as_deref() == Some("old"))
        .unwrap();
    assert_eq!(
        (
            old.lane.as_deref(),
            old.released_by.as_deref(),
            old.pr,
            old.reviewers.len()
        ),
        (Some("lane-a"), Some("lane-a"), None, 0)
    );
    let new = slices
        .iter()
        .find(|s| s.branch.as_deref() == Some("new"))
        .unwrap();
    assert_eq!(
        (new.lane.as_deref(), new.pr, new.reviewers.clone()),
        (Some("lane-b"), Some(852), vec!["reviewer".to_owned()])
    );
}

#[test]
fn a_release_of_an_older_claim_leaves_both_claims_open() {
    let tasks = fold_task_events(&[
        claimed(10, "lane-a", "issue-729", 729, "same"),
        claimed(11, "lane-a", "issue-729", 729, "same"),
        released(12, "lane-a", 10, "Stopped"),
    ]);
    assert!(tasks.iter().all(|t| t.released_by.is_none()));
    assert_eq!(tasks.len(), 2);
}

#[test]
fn knows_the_repository_from_task_claimed_before_any_verdict_420() {
    let states = fold_task_events(&[
        record(
            1,
            "task.claimed",
            "gh-claude-4",
            json!({"issue": 902, "lane": "gh-claude-4", "branch": "issue-902-x", "repo": "stabem/GraphHelm"}),
        ),
        opened(2, "gh-claude-4", "pr-384", 9002, "dddddddd"),
    ]);
    assert_eq!(
        (states[0].issue, states[0].pr, states[0].repo_url.as_deref()),
        (
            Some(902),
            Some(9002),
            Some("https://github.com/stabem/GraphHelm")
        )
    );
}

#[test]
fn refuses_a_repo_that_is_not_owner_name() {
    for repo in [
        "stabem",
        "https://evil.example/x/y",
        "a/b/c",
        "../x",
        "a /b",
    ] {
        assert!(parse("task.claimed", "gh-claude-4", json!({"schema": "graphhelm-task-event-v1", "taskId": "issue-1", "revision": 1, "at": "2026-10-08T00:00:00Z", "issue": 1, "lane": "gh-claude-4", "branch": "b", "repo": repo})).is_none(), "{repo}");
    }
}

fn slices_460() -> Vec<TaskEventRecord> {
    let task = "issue-356";
    vec![
        claimed(1, "gh-claude-4", task, 356, "issue-356-heal"),
        opened(2, "gh-claude-4", task, 456, "aaaaaaaa"),
        record(
            3,
            "task.review_assigned",
            "gh-claude-4",
            json!({"taskId": task, "pr": 456, "headSha": "aaaaaaaa", "reviewer": "gh-claude-6", "ordinal": 1}),
        ),
        claimed(4, "gh-claude-4", task, 356, "issue-356-explore-faults"),
        verdict(
            5,
            "gh-claude-6",
            task,
            456,
            "aaaaaaaa",
            "APPROVE",
            "https://github.com/stabem/GraphHelm/pull/456#c1",
        ),
        merged(6, "gh-claude-6", task, 456, "cccccccc", &[]),
        opened(7, "gh-claude-4", task, 461, "dddddddd"),
    ]
}

#[test]
fn keeps_the_second_slice_visible_with_its_own_pr_after_the_first_one_merged_460() {
    let tasks = fold_task_events(&slices_460());
    let seen: Vec<(&str, Option<u64>, TaskStep, Option<u64>)> = tasks
        .iter()
        .map(|t| (t.task_id.as_str(), t.pr, t.step, t.issue))
        .collect();
    assert_eq!(
        seen,
        vec![
            ("issue-356", Some(456), TaskStep::Merged, Some(356)),
            ("issue-356", Some(461), TaskStep::Review, Some(356))
        ]
    );
    assert_ne!(tasks[0].key, tasks[1].key);
    assert_eq!(tasks[1].head_sha.as_deref(), Some("dddddddd"));
}

#[test]
fn joins_the_latest_open_claim_and_absorbs_earlier_claims_of_the_same_lane() {
    let tasks = fold_task_events(&[
        claimed(1, "gh-claude-4", "issue-356", 356, "issue-356-heal"),
        claimed(2, "gh-claude-4", "issue-356", 356, "issue-356-explore"),
        opened(3, "gh-claude-4", "issue-356", 456, "aaaaaaaa"),
    ]);
    let seen: Vec<(Option<&str>, Option<u64>, TaskStep)> = tasks
        .iter()
        .map(|t| (t.branch.as_deref(), t.pr, t.step))
        .collect();
    assert_eq!(
        seen,
        vec![(Some("issue-356-explore"), Some(456), TaskStep::Review)]
    );
}

#[test]
fn keeps_a_later_head_of_the_same_pr_on_the_latest_claim_859() {
    let tasks = fold_task_events(&[
        claimed(1, "codex-2", "issue-830", 830, "issue-830-fix"),
        claimed(2, "codex-2", "issue-830", 830, "issue-830-fix"),
        opened(3, "codex-2", "issue-830", 854, "aaaaaaaa"),
        opened(4, "codex-2", "issue-830", 854, "bbbbbbbb"),
    ]);
    let pr: Vec<_> = tasks.iter().filter(|t| t.pr == Some(854)).collect();
    assert_eq!(pr.len(), 1);
    assert_eq!(
        (pr[0].head_sha.as_deref(), pr[0].branch.as_deref()),
        (Some("bbbbbbbb"), Some("issue-830-fix"))
    );
    assert!(tasks.iter().all(|t| t.pr.is_some()));
}

#[test]
fn shows_the_second_claim_as_its_own_slice_before_it_has_a_pr() {
    let tasks = fold_task_events(&slices_460()[..4]);
    let seen: Vec<(Option<u64>, TaskStep)> = tasks.iter().map(|t| (t.pr, t.step)).collect();
    assert_eq!(
        seen,
        vec![(Some(456), TaskStep::Review), (None, TaskStep::Plan)]
    );
}

#[test]
fn removes_a_closed_pr_slice_from_the_work_model_without_ending_a_sibling_pr() {
    let tasks = fold_task_events(&[
        opened(1, "codex-6", "issue-685", 685, "aaaaaaaa"),
        opened(2, "codex-6", "issue-685", 688, "bbbbbbbb"),
        record(
            3,
            "task.closed",
            "codex-6",
            json!({"taskId": "issue-685", "pr": 685, "reason": "superseded", "by": 740}),
        ),
    ]);
    assert_eq!(
        tasks.iter().map(|t| t.pr).collect::<Vec<_>>(),
        vec![Some(688)]
    );
}

#[test]
fn restores_only_the_closed_pr_slice_reopened_by_its_author_1003() {
    let task = "issue-1003";
    let o = opened(1, "codex-1", task, 1004, "aaaaaaaa");
    let sibling = opened(2, "codex-1", task, 1005, "bbbbbbbb");
    let closed = record(
        3,
        "task.closed",
        "coordinator",
        json!({"taskId": task, "pr": 1004, "reason": "superseded", "by": 1006}),
    );
    let sibling_closed = record(
        4,
        "task.closed",
        "coordinator",
        json!({"taskId": task, "pr": 1005, "reason": "superseded", "by": 1006}),
    );
    let reopened = opened(5, "codex-1", task, 1004, "cccccccc");
    let approved = verdict(
        6,
        "codex-review",
        task,
        1004,
        "cccccccc",
        "APPROVE",
        "https://github.com/stabem/GraphHelm/pull/1004#c1",
    );
    let m = merged(7, "codex-review", task, 1004, "dddddddd", &[]);
    let repeated = opened(8, "codex-1", task, 1004, "eeeeeeee");

    assert!(
        fold_task_events(&[
            o.clone(),
            sibling.clone(),
            closed.clone(),
            sibling_closed.clone()
        ])
        .is_empty()
    );
    let t = fold_task_events(&[
        o.clone(),
        sibling.clone(),
        closed.clone(),
        sibling_closed.clone(),
        reopened.clone(),
    ]);
    assert_eq!(t.len(), 1);
    assert_eq!(
        (t[0].pr, t[0].head_sha.as_deref(), t[0].step),
        (Some(1004), Some("cccccccc"), TaskStep::Review)
    );
    let t = fold_task_events(&[
        o.clone(),
        sibling.clone(),
        closed.clone(),
        sibling_closed.clone(),
        reopened.clone(),
        approved.clone(),
    ]);
    assert_eq!(
        (t[0].step, t[0].reviewers.clone()),
        (TaskStep::Merge, vec!["codex-review".to_owned()])
    );
    let t = fold_task_events(&[
        o,
        sibling,
        closed,
        sibling_closed,
        reopened,
        approved,
        m,
        repeated,
    ]);
    assert_eq!(t.len(), 1);
    assert_eq!(
        (
            t[0].pr,
            t[0].head_sha.as_deref(),
            t[0].step,
            t[0].merge_sha.as_deref()
        ),
        (
            Some(1004),
            Some("cccccccc"),
            TaskStep::Merged,
            Some("dddddddd")
        )
    );
}

#[test]
fn absorbs_a_same_lane_re_claim_into_its_existing_pr_before_merge_871() {
    let events = [
        claimed(4417, "codex-4", "issue-86", 86, "issue-86-live-tree"),
        opened(4422, "codex-4", "issue-86", 658, "aaaaaaaa"),
        claimed(4426, "codex-4", "issue-86", 86, "issue-86-live-tree"),
        opened(4430, "codex-4", "issue-86", 658, "bbbbbbbb"),
        merged(4437, "coordinator", "pr-658", 658, "cccccccc", &[]),
    ];
    assert!(
        fold_task_events(&events[..4])
            .iter()
            .all(|t| t.pr.is_some())
    );
    let tasks = fold_task_events(&events);
    assert!(tasks.iter().all(|t| t.pr.is_some()));
    let pr: Vec<_> = tasks.iter().filter(|t| t.pr == Some(658)).collect();
    assert_eq!(pr.len(), 1);
    assert_eq!(
        (pr[0].step, pr[0].merge_sha.as_deref()),
        (TaskStep::Merged, Some("cccccccc"))
    );
}

#[test]
fn joins_the_latest_pre_pr_claim_and_absorbs_earlier_same_lane_claims_871() {
    let tasks = fold_task_events(&[
        claimed(5299, "codex-2", "issue-830", 830, "issue-830-proof"),
        claimed(5409, "codex-2", "issue-830", 830, "issue-830-proof"),
        opened(5430, "codex-2", "issue-830", 854, "aaaaaaaa"),
        opened(5460, "codex-2", "issue-830", 854, "bbbbbbbb"),
        merged(5618, "coordinator", "pr-854", 854, "cccccccc", &[]),
    ]);
    assert_eq!(tasks.len(), 1);
    assert_eq!(
        (
            tasks[0].claim_sequence,
            tasks[0].pr,
            tasks[0].step,
            tasks[0].merge_sha.as_deref()
        ),
        (Some(5409), Some(854), TaskStep::Merged, Some("cccccccc"))
    );
}

#[test]
fn ends_both_claims_of_one_pr_when_its_merge_closes_only_the_first_issue_807() {
    let tasks = fold_task_events(&[
        claimed(1, "codex-14", "issue-691", 691, "issue-691-fix"),
        opened(2, "codex-14", "issue-691", 9, "aaaaaaaa"),
        claimed(3, "codex-14", "issue-722", 722, "issue-722-followup"),
        opened(4, "codex-14", "issue-722", 9, "aaaaaaaa"),
        merged(5, "coordinator", "pr-9", 9, "bbbbbbbb", &[691]),
    ]);
    let mut pr: Vec<_> = tasks
        .iter()
        .filter(|t| t.pr == Some(9))
        .map(|t| (t.issue, t.step, t.merge_sha.clone(), t.last_sequence))
        .collect();
    pr.sort();
    assert_eq!(
        pr,
        vec![
            (Some(691), TaskStep::Merged, Some("bbbbbbbb".to_owned()), 5),
            (Some(722), TaskStep::Merged, Some("bbbbbbbb".to_owned()), 5)
        ]
    );
}

#[test]
fn removes_the_closed_pr_slice_while_a_duplicate_claim_stays_pr_less_807_859() {
    let events = [
        claimed(1, "codex-4", "issue-86", 86, "issue-86-first"),
        opened(2, "codex-4", "issue-86", 9, "aaaaaaaa"),
        claimed(3, "codex-4", "issue-86", 86, "issue-86-second"),
        opened(4, "codex-4", "issue-86", 9, "aaaaaaaa"),
        record(
            5,
            "task.closed",
            "coordinator",
            json!({"taskId": "pr-9", "pr": 9, "reason": "superseded", "by": 10}),
        ),
    ];
    assert_eq!(
        fold_task_events(&events[..4])
            .iter()
            .filter(|t| t.pr == Some(9))
            .count(),
        1
    );
    assert_eq!(
        fold_task_events(&events)
            .iter()
            .filter(|t| t.pr == Some(9))
            .count(),
        0
    );
}

#[test]
fn ends_a_duplicate_pr_claim_when_the_existing_pr_slice_ends_795() {
    let tasks = fold_task_events(&[
        claimed(1, "lane-a", "issue-1", 1, "issue-1-first"),
        opened(2, "lane-a", "issue-1", 9, "aaaaaaaa"),
        claimed(3, "lane-b", "issue-1", 1, "issue-1-duplicate"),
        opened(4, "lane-b", "issue-1", 9, "cccccccc"),
        merged(5, "coordinator", "pr-9", 9, "bbbbbbbb", &[1]),
    ]);
    let b = tasks
        .iter()
        .find(|t| t.lane.as_deref() == Some("lane-b") && t.pr == Some(9))
        .unwrap();
    assert_eq!(
        (b.step, b.merge_sha.as_deref()),
        (TaskStep::Merged, Some("bbbbbbbb"))
    );
}

#[test]
fn ends_the_opened_pr_slice_even_when_the_merge_uses_a_pr_task_id() {
    let tasks = fold_task_events(&[
        claimed(1, "codex-14", "issue-691", 691, "issue-691-fix"),
        opened(2, "codex-14", "issue-691", 722, "aaaaaaaa"),
        record(
            3,
            "task.review_assigned",
            "codex-14",
            json!({"taskId": "issue-691", "pr": 722, "headSha": "aaaaaaaa", "reviewer": "gh-claude-1"}),
        ),
        merged(4, "coordinator", "pr-722", 722, "bbbbbbbb", &[691]),
    ]);
    let pr: Vec<_> = tasks.iter().filter(|t| t.pr == Some(722)).collect();
    assert_eq!(pr.len(), 1);
    assert_eq!(
        (
            pr[0].issue,
            pr[0].lane.as_deref(),
            pr[0].step,
            pr[0].merge_sha.as_deref()
        ),
        (
            Some(691),
            Some("codex-14"),
            TaskStep::Merged,
            Some("bbbbbbbb")
        )
    );
}

#[test]
fn ends_a_pr_less_claim_only_when_the_merge_closes_its_issue() {
    let tasks = fold_task_events(&[
        claimed(1, "codex-13", "issue-758", 758, "issue-758-fix"),
        claimed(2, "codex-12", "issue-759", 759, "issue-759-work"),
        merged(3, "coordinator", "pr-760", 760, "cccccccc", &[758]),
    ]);
    let a = tasks.iter().find(|t| t.issue == Some(758)).unwrap();
    let b = tasks.iter().find(|t| t.issue == Some(759)).unwrap();
    assert_eq!((a.pr, a.step), (None, TaskStep::Merged));
    assert_eq!((b.pr, b.step), (None, TaskStep::Plan));
}

fn plan_480(sequence: u64, mode: &str) -> TaskEventRecord {
    record(
        sequence,
        "task.planned",
        "gh-claude-1",
        json!({"taskId": "issue-480", "lane": "gh-claude-1", "classes": ["user_visible"], "reviews": 1, "proof": "both", "critic": {"mode": mode, "passScore": 8, "maxRounds": 3}, "summary": "Record the plan; light Plan."}),
    )
}

fn round_480(sequence: u64, lane: &str, n: u64, score: u64, word: &str) -> TaskEventRecord {
    record(
        sequence,
        "task.critic_verdict",
        lane,
        json!({"taskId": "issue-480", "lane": lane, "round": n, "score": score, "passScore": 8, "maxRounds": 3, "verdict": word, "designRef": "d", "reasons": ["r"]}),
    )
}

#[test]
fn lights_plan_on_the_claim_then_implement_or_critic_once_the_plan_is_recorded_480() {
    let claim = claimed(1, "gh-claude-1", "issue-480", 480, "issue-480-plan-step");
    let o = opened(3, "gh-claude-1", "issue-480", 481, "aaaaaaaa");
    assert_eq!(
        fold_task_events(std::slice::from_ref(&claim))[0].step,
        TaskStep::Plan
    );
    let coded = &fold_task_events(&[claim.clone(), plan_480(2, "none")])[0];
    assert_eq!(coded.step, TaskStep::Implement);
    let plan = coded.plan.as_ref().unwrap();
    assert_eq!(
        (plan.summary.as_str(), plan.critic.mode.as_str()),
        ("Record the plan; light Plan.", "none")
    );
    assert_eq!(
        fold_task_events(&[claim.clone(), plan_480(2, "design")])[0].step,
        TaskStep::Critic
    );
    assert_eq!(
        fold_task_events(&[claim, plan_480(2, "design"), o])[0].step,
        TaskStep::Review
    );
}

#[test]
fn ends_critic_only_on_a_passing_round() {
    let claim = claimed(1, "gh-claude-1", "issue-480", 480, "issue-480-plan-step");
    assert_eq!(
        fold_task_events(&[
            claim.clone(),
            plan_480(2, "design"),
            round_480(3, "gh-claude-1", 1, 5, "revise")
        ])[0]
            .step,
        TaskStep::Critic
    );
    assert_eq!(
        fold_task_events(&[
            claim.clone(),
            plan_480(2, "design"),
            round_480(3, "gh-claude-1", 1, 5, "revise"),
            round_480(4, "gh-claude-1", 2, 9, "pass")
        ])[0]
            .step,
        TaskStep::Implement
    );
    assert_eq!(
        fold_task_events(&[
            claim,
            plan_480(2, "design"),
            round_480(3, "gh-claude-1", 3, 5, "exhausted")
        ])[0]
            .step,
        TaskStep::Critic
    );
}

#[test]
fn lands_a_lanes_plan_and_critic_round_on_that_lanes_own_claim() {
    let plan_of = |sequence: u64, lane: &str| {
        record(
            sequence,
            "task.planned",
            lane,
            json!({"taskId": "issue-480", "lane": lane, "classes": ["code"], "reviews": 1, "proof": "tests", "critic": {"mode": "design", "passScore": 8, "maxRounds": 3}, "summary": format!("{lane}'s plan")}),
        )
    };
    let tasks = fold_task_events(&[
        claimed(1, "lane-a", "issue-480", 480, "issue-480-lane-a"),
        claimed(2, "lane-b", "issue-480", 480, "issue-480-lane-b"),
        plan_of(3, "lane-b"),
        round_480(4, "lane-b", 1, 9, "pass"),
    ]);
    let a = tasks
        .iter()
        .find(|t| t.lane.as_deref() == Some("lane-a"))
        .unwrap();
    let b = tasks
        .iter()
        .find(|t| t.lane.as_deref() == Some("lane-b"))
        .unwrap();
    assert_eq!((a.step, a.plan.is_none()), (TaskStep::Plan, true));
    assert_eq!(
        (b.step, b.plan.as_ref().unwrap().summary.as_str()),
        (TaskStep::Implement, "lane-b's plan")
    );
}

#[test]
fn keeps_a_plan_recorded_after_the_pr_without_moving_the_graph_back() {
    let state = fold_task_events(&[
        claimed(1, "gh-claude-1", "issue-480", 480, "issue-480-plan-step"),
        opened(2, "gh-claude-1", "issue-480", 481, "aaaaaaaa"),
        plan_480(3, "design"),
    ]);
    assert_eq!(state.len(), 1);
    assert_eq!(
        (
            state[0].step,
            state[0].plan.as_ref().unwrap().critic.mode.as_str()
        ),
        (TaskStep::Review, "design")
    );
}

#[test]
fn starts_a_new_lane_owned_slice_for_plan_and_critic_records_after_merge() {
    let tasks = fold_task_events(&[
        claimed(1, "lane-a", "issue-480", 480, "first"),
        opened(2, "lane-a", "issue-480", 481, "aaaaaaaa"),
        merged(3, "reviewer", "issue-480", 481, "bbbbbbbb", &[]),
        claimed(4, "lane-b", "issue-480", 480, "foreign"),
        record(
            5,
            "task.planned",
            "lane-a",
            json!({"taskId": "issue-480", "lane": "lane-a", "classes": ["code"], "reviews": 1, "proof": "tests", "critic": {"mode": "design", "passScore": 8, "maxRounds": 3}, "summary": "next plan"}),
        ),
        round_480(6, "lane-a", 1, 9, "pass"),
    ]);
    assert_eq!(tasks.len(), 3);
    let done = tasks.iter().find(|t| t.pr == Some(481)).unwrap();
    assert_eq!(
        (
            done.step,
            done.merge_sha.as_deref(),
            done.plan.is_none(),
            done.critic.is_none()
        ),
        (TaskStep::Merged, Some("bbbbbbbb"), true, true)
    );
    let foreign = tasks
        .iter()
        .find(|t| t.branch.as_deref() == Some("foreign"))
        .unwrap();
    assert_eq!(
        (foreign.lane.as_deref(), foreign.pr, foreign.plan.is_none()),
        (Some("lane-b"), None, true)
    );
    let next = tasks
        .iter()
        .find(|t| t.lane.as_deref() == Some("lane-a") && t.pr.is_none())
        .unwrap();
    assert_eq!(
        (
            next.step,
            next.plan.as_ref().unwrap().summary.as_str(),
            next.critic.as_ref().unwrap().verdict
        ),
        (TaskStep::Implement, "next plan", CriticOutcome::Pass)
    );
}

#[test]
fn marks_only_the_merged_prs_own_slice_as_completion_recorded() {
    let tasks = fold_task_events(&[
        claimed(1, "lane-a", "issue-1", 1, "first"),
        opened(2, "lane-a", "issue-1", 10, "aaaaaaaa"),
        claimed(3, "lane-b", "issue-2", 2, "second"),
        opened(4, "lane-b", "issue-2", 11, "bbbbbbbb"),
        verdict(
            5,
            "reviewer",
            "issue-1",
            10,
            "aaaaaaaa",
            "APPROVE",
            "https://github.com/o/r/pull/10#c1",
        ),
        merged(6, "reviewer", "issue-1", 10, "cccccccc", &[2]),
    ]);
    assert_eq!(
        tasks.iter().filter(|t| t.step == TaskStep::Merged).count(),
        2
    );
    let recorded: Vec<_> = tasks
        .iter()
        .filter(|t| t.step == TaskStep::Merged && t.completion_recorded)
        .collect();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].pr, Some(10));
}

#[test]
fn refuses_a_planned_record_for_another_lane_or_out_of_the_schemas_bounds() {
    let parse_plan = |actor: &str, patch: Value| {
        let mut doc = json!({"schema": "graphhelm-task-event-v1", "taskId": "issue-480", "revision": 1, "at": "2026-10-08T00:00:00Z", "lane": "gh-claude-1", "classes": ["code"], "reviews": 1, "proof": "tests", "critic": {"mode": "none", "passScore": 8, "maxRounds": 3}, "summary": "s"});
        for (k, v) in patch.as_object().unwrap() {
            doc[k] = v.clone();
        }
        parse("task.planned", actor, doc)
    };
    assert!(parse_plan("gh-claude-1", json!({})).is_some());
    assert!(parse_plan("gh-claude-2", json!({})).is_none());
    assert!(parse_plan("gh-claude-1", json!({"classes": ["code", "code"]})).is_none());
    assert!(
        parse_plan(
            "gh-claude-1",
            json!({"critic": {"mode": "design", "passScore": 11, "maxRounds": 3}})
        )
        .is_none()
    );
    assert!(parse_plan("gh-claude-1", json!({"summary": "x".repeat(301)})).is_none());
}

/// The wire shape every door serves: camelCase keys, the step as its lowercase word, the verdict
/// as the comment-text word, so the Studio reads the route reply as it read its own fold.
#[test]
fn serializes_a_slice_with_the_studio_field_names() {
    let state = &fold_task_events(&[
        opened(1, "gh-claude-4", "issue-5", 6, "aaaaaaaa"),
        verdict(
            2,
            "gh-claude-1",
            "issue-5",
            6,
            "aaaaaaaa",
            "BLOCK",
            "https://github.com/stabem/GraphHelm/pull/6#c1",
        ),
    ])[0];
    let value = serde_json::to_value(state).unwrap();
    assert_eq!(value["key"], "issue-5#pr-6");
    assert_eq!(value["step"], "review");
    assert_eq!(value["headSha"], "aaaaaaaa");
    assert_eq!(value["blockedBy"]["reviewer"], "gh-claude-1");
    assert_eq!(value["blockedSequence"], 2);
    assert_eq!(value["rounds"][0]["fixHead"], Value::Null);
    assert_eq!(value["clock"]["spent"], json!({}));
    assert_eq!(value["issue"], 5);
    assert_eq!(
        serde_json::to_value(Verdict::ApproveWithRisk).unwrap(),
        "APPROVE-WITH-RISK"
    );
    let occurred = PersistedTimestamp::parse("2026-10-13T12:00:00Z").unwrap();
    let timed = with_time(claimed(1, "lane-a", "issue-7", 7, "b"), occurred);
    assert_eq!(
        serde_json::to_value(&fold_task_events(&[timed])[0]).unwrap()["clock"]["since"],
        "2026-10-13T12:00:00Z"
    );
}
