//! #2005: which stage of the delivery pipeline is broken, and the one action that fixes it.
//!
//! The Runtime already folds every task record into slices (`tasks`), joins owner requests to
//! their issues (`requests`) and judges each lane's liveness. Nobody acted on those reads unless a
//! person was watching. This module reads them, deterministically, and names for each open item
//! the broken stage and the role whose agent fixes it. It decides nothing about processes: the
//! serve executor (`apps/cli/src/commands/serve/autopilot.rs`) launches, caps and records.
//!
//! Pure: the caller passes the slices, the requests, the set of silent lanes, the Runtime's `now`
//! and the grace windows. No model is asked; the same inputs always give the same diagnoses.
//!
//! `merged but not recorded` has no stage of its own: the Runtime reads no GitHub, so a PR merged
//! without `task.merged` reads as an approved head nobody merged. The merger started for
//! `approved_not_merged` checks GitHub first and records `task.merged` when it already landed.

use std::collections::BTreeSet;

use graphhelm_protocols::PersistedTimestamp;
use serde::Serialize;

use crate::requests::{RequestStatus, RequestView};
use crate::tasks::{TaskEventKind, TaskEventRecord, TaskState, TaskStep};

/// The actor the executor asks the owner as (escalations and pauses). It never owns or reviews a
/// slice, so `takes_work` never hands it anything.
pub const AUTOPILOT_ACTOR: &str = "graphhelm-autopilot";

/// The broken stage of one open item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// An owner request with no linked issue and no live hand-off past its grace.
    RequestUnclaimed,
    /// A slice before its PR (plan, critic, implement) whose author lane is silent.
    AuthorSilent,
    /// A recorded PR waiting for review with no reviewer assigned past its grace.
    ReviewUnassigned,
    /// A recorded PR whose assigned reviewer is silent.
    ReviewerSilent,
    /// A BLOCK on the current head whose author lane is silent.
    BlockUnanswered,
    /// An approved head nobody merged (or merged without `task.merged`) past its grace.
    ApprovedNotMerged,
}

impl Stage {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RequestUnclaimed => "request_unclaimed",
            Self::AuthorSilent => "author_silent",
            Self::ReviewUnassigned => "review_unassigned",
            Self::ReviewerSilent => "reviewer_silent",
            Self::BlockUnanswered => "block_unanswered",
            Self::ApprovedNotMerged => "approved_not_merged",
        }
    }

    /// The role whose agent fixes this stage.
    #[must_use]
    pub const fn role(self) -> Role {
        match self {
            Self::RequestUnclaimed | Self::AuthorSilent | Self::BlockUnanswered => Role::Author,
            Self::ReviewUnassigned | Self::ReviewerSilent => Role::Reviewer,
            Self::ApprovedNotMerged => Role::Merger,
        }
    }
}

/// Who the executor starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Author,
    Reviewer,
    Merger,
}

impl Role {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Author => "author",
            Self::Reviewer => "reviewer",
            Self::Merger => "merger",
        }
    }
}

/// One broken item and its fix.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnosis {
    /// Stable identity of the item: `request:<id>` or `slice:<key>`.
    pub item: String,
    pub stage: Stage,
    pub role: Role,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head_sha: Option<String>,
    /// The lane whose work stalled (the author, or the reviewer), when the records name one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stalled_lane: Option<String>,
}

impl Diagnosis {
    /// What the item is called in a note the owner reads: `PR #N`, `issue #N` or `request`.
    /// Built from numbers only, so no lane- or owner-written text reaches a note's summary.
    #[must_use]
    pub fn subject(&self) -> String {
        match (self.pr, self.issue) {
            (Some(pr), _) => format!("PR #{pr}"),
            (None, Some(issue)) => format!("issue #{issue}"),
            (None, None) => "an owner request".to_owned(),
        }
    }
}

/// How long a fresh state may wait before it counts as broken, in seconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grace {
    /// An unlinked request (the #1990 dispatcher gets this long to hand it to a live lane).
    pub request: i64,
    /// A recorded PR with no reviewer.
    pub review: i64,
    /// An approved head with no merge.
    pub merge: i64,
}

impl Default for Grace {
    fn default() -> Self {
        Self {
            request: 15 * 60,
            review: 10 * 60,
            merge: 15 * 60,
        }
    }
}

fn age(since: Option<&PersistedTimestamp>, now: &PersistedTimestamp) -> Option<i64> {
    since.map(|at| (*now.as_datetime() - *at.as_datetime()).num_seconds())
}

/// #2005: a lane may be handed an owner request only if it ever owned or reviewed a slice (the
/// Studio's #1987 rule) and it never recorded an owner request itself. The second half is what
/// stops the orchestrator: on the live `studio-v2` run its lane owns 25 merged slices (its
/// subagents record under it), so "owned a slice" alone still calls it free; but the lane that
/// records requests is the one that hands them out, never one that takes them.
#[must_use]
pub fn takes_work(lane: &str, tasks: &[TaskState], records: &[TaskEventRecord]) -> bool {
    let worked = tasks.iter().any(|task| {
        task.lane.as_deref() == Some(lane)
            || task.current_reviewer.as_deref() == Some(lane)
            || task.reviewers.iter().any(|reviewer| reviewer == lane)
    });
    let requester = records
        .iter()
        .any(|record| record.kind == TaskEventKind::RequestRecorded && record.actor_id == lane);
    worked && !requester
}

/// Every broken open item at `now`, requests first (oldest first), then slices in fold order.
/// `silent` holds the lanes the liveness read judged silent; a slice with no lane counts as having
/// no live author.
#[must_use]
pub fn classify(
    tasks: &[TaskState],
    requests: &[RequestView],
    silent: &BTreeSet<String>,
    now: &PersistedTimestamp,
    grace: Grace,
) -> Vec<Diagnosis> {
    let gone = |lane: Option<&str>| lane.is_none_or(|lane| silent.contains(lane));
    let open = |task: &TaskState| {
        task.released_by.is_none() && task.step != TaskStep::Merged && !task.orphaned
    };
    // Another open slice of the same issue with a live lane: someone already took the work over.
    let taken_over = |task: &TaskState| {
        task.issue.is_some()
            && tasks.iter().any(|other| {
                other.key != task.key
                    && other.issue == task.issue
                    && open(other)
                    && !gone(other.lane.as_deref())
            })
    };
    let mut out = Vec::new();
    let mut ordered: Vec<&RequestView> = requests.iter().collect();
    ordered.sort_by_key(|view| view.sequence);
    for view in ordered {
        let waiting = view.status == RequestStatus::Open
            && view.issues.is_empty()
            && view.dispatched_to.is_none();
        if waiting
            && age(view.recorded_at.as_ref(), now).is_some_and(|seconds| seconds >= grace.request)
        {
            out.push(Diagnosis {
                item: format!("request:{}", view.request_id),
                stage: Stage::RequestUnclaimed,
                role: Role::Author,
                request_id: Some(view.request_id.clone()),
                issue: None,
                pr: None,
                head_sha: None,
                stalled_lane: None,
            });
        }
    }
    for task in tasks {
        if !open(task) {
            continue;
        }
        let idle = age(
            task.clock.last_at.as_ref().or(task.clock.since.as_ref()),
            now,
        );
        let author_gone = gone(task.lane.as_deref()) && !taken_over(task);
        // #2027: a recorded PR, an assigned reviewer or any verdict means the author handed the
        // work on, whatever step the fold left; such a slice is never author_silent.
        let handed_on = task.pr.is_some()
            || task.current_reviewer.is_some()
            || task.pending_reviewer.is_some()
            || !task.assigned_reviewers.is_empty()
            || !task.reviewers.is_empty();
        let early = matches!(
            task.step,
            TaskStep::Plan | TaskStep::Critic | TaskStep::Implement
        );
        let found = if task.blocked_by.is_some() {
            author_gone.then(|| (Stage::BlockUnanswered, task.lane.clone()))
        } else if early && handed_on {
            // Read as the review it waits for, with the review grace from its newest record.
            match &task.current_reviewer {
                // #2039: only the current head's reviewer counts, as at the review step; one
                // assigned or heard from for an older head does not review this head.
                None if task.pr.is_some()
                    && task.pending_reviewer.is_none()
                    && idle.is_some_and(|seconds| seconds >= grace.review) =>
                {
                    Some((Stage::ReviewUnassigned, None))
                }
                Some(reviewer) if task.pr.is_some() && silent.contains(reviewer) => {
                    Some((Stage::ReviewerSilent, Some(reviewer.clone())))
                }
                _ => None,
            }
        } else {
            match task.step {
                TaskStep::Plan | TaskStep::Critic | TaskStep::Implement => {
                    author_gone.then(|| (Stage::AuthorSilent, task.lane.clone()))
                }
                TaskStep::Review if task.pr.is_some() => match &task.current_reviewer {
                    None if task.pending_reviewer.is_none()
                        && idle.is_some_and(|seconds| seconds >= grace.review) =>
                    {
                        Some((Stage::ReviewUnassigned, None))
                    }
                    Some(reviewer) if silent.contains(reviewer) => {
                        Some((Stage::ReviewerSilent, Some(reviewer.clone())))
                    }
                    _ => None,
                },
                TaskStep::Merge if idle.is_some_and(|seconds| seconds >= grace.merge) => Some((
                    Stage::ApprovedNotMerged,
                    task.current_reviewer
                        .clone()
                        .or_else(|| task.reviewers.last().cloned()),
                )),
                _ => None,
            }
        };
        let Some((stage, stalled_lane)) = found else {
            continue;
        };
        out.push(Diagnosis {
            item: format!("slice:{}", task.key),
            stage,
            role: stage.role(),
            request_id: None,
            issue: task.issue,
            pr: task.pr,
            head_sha: task.head_sha.clone(),
            stalled_lane,
        });
    }
    out
}

/// The id of the note that records the executor's `attempt`th (from 0) action on `item` at
/// `stage`. Derived from the three alone, so two ticks racing name the same signal and the store
/// admits it once: an action is idempotent per (item, stage, attempt).
#[must_use]
pub fn action_signal_id(item: &str, stage: Stage, attempt: u32) -> String {
    let seed = format!("autopilot\n{item}\n{}\n{attempt}", stage.as_str());
    let digest = graphhelm_protocols::hash::sha256_hex(seed.as_bytes());
    format!("autopilot-{}", &digest[..32])
}

/// The id of the owner escalation for `item` at `stage` once its attempts ran out.
#[must_use]
pub fn escalation_signal_id(item: &str, stage: Stage) -> String {
    let seed = format!("autopilot-escalate\n{item}\n{}", stage.as_str());
    let digest = graphhelm_protocols::hash::sha256_hex(seed.as_bytes());
    format!("autopilot-escalate-{}", &digest[..24])
}

/// The lane name the executor gives the agent it starts. Built only from the role and numbers (or
/// a digest), so no lane- or owner-written text reaches it: `[a-z0-9-]` only.
#[must_use]
pub fn lane_name(diagnosis: &Diagnosis) -> String {
    let role = diagnosis.role.as_str();
    match (diagnosis.pr, diagnosis.issue) {
        (Some(pr), _) => format!("auto-{role}-pr-{pr}"),
        (None, Some(issue)) => format!("auto-{role}-issue-{issue}"),
        (None, None) => {
            let digest = graphhelm_protocols::hash::sha256_hex(diagnosis.item.as_bytes());
            format!("auto-{role}-req-{}", &digest[..10])
        }
    }
}

/// What the executor does about one diagnosis this tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// Start the role's agent; `attempt` is from 0.
    Launch { diagnosis: Diagnosis, attempt: u32 },
    /// The attempts ran out and the item is still broken: tell the owner once.
    Escalate { diagnosis: Diagnosis, attempts: u32 },
}

/// What the executor knows about one (item, stage) from the journal and its own processes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct History {
    /// How many action notes the journal holds for it.
    pub attempts: u32,
    /// Seconds since the newest action note, when there is one.
    pub last_age: Option<i64>,
    /// A process this Runtime started for it is still running.
    pub running: bool,
    /// The newest attempt's process ended in failure (known only to this Runtime instance).
    pub failed: bool,
    /// The escalation note is already on the journal.
    pub escalated: bool,
}

/// The executor's limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_attempts: u32,
    /// Agent processes running at once, across every item.
    pub max_concurrent: usize,
    /// How long one attempt is given before the next may start, unless it failed.
    pub retry_seconds: i64,
}

/// Plan this tick: for each diagnosis, launch the next attempt when none is running and the last
/// one had `retry_seconds`, failed or not; escalate once when attempts ran out (at once when the
/// last one failed). Launches stop at
/// `max_concurrent` minus `running`. Pure; `history` is looked up per diagnosis.
#[must_use]
pub fn plan(
    diagnoses: &[Diagnosis],
    history: impl Fn(&Diagnosis) -> History,
    running: usize,
    limits: Limits,
) -> Vec<Step> {
    let mut slots = limits.max_concurrent.saturating_sub(running);
    let mut out = Vec::new();
    for diagnosis in diagnoses {
        let seen = history(diagnosis);
        if seen.running {
            continue;
        }
        // #2027: a failure does not shorten the wait before the next attempt (three launcher
        // failures in one minute spent every attempt); it only lets the escalation go at once.
        let lapsed = seen
            .last_age
            .is_none_or(|seconds| seconds >= limits.retry_seconds);
        let spent = seen.attempts >= limits.max_attempts;
        let due = seen.attempts == 0 || lapsed || (spent && seen.failed);
        if !due {
            continue;
        }
        if spent {
            if !seen.escalated {
                out.push(Step::Escalate {
                    diagnosis: diagnosis.clone(),
                    attempts: seen.attempts,
                });
            }
            continue;
        }
        if slots == 0 {
            continue;
        }
        slots -= 1;
        out.push(Step::Launch {
            diagnosis: diagnosis.clone(),
            attempt: seen.attempts,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::requests::RequestIssue;
    use crate::tasks::{Blocker, fold_task_events, parse_task_event};

    fn at(value: &str) -> PersistedTimestamp {
        PersistedTimestamp::parse(value).unwrap()
    }

    const NOW: &str = "2026-10-24T12:00:00Z";
    const OLD: &str = "2026-10-24T10:00:00Z";
    const FRESH: &str = "2026-10-24T11:59:00Z";

    fn slice(lane: &str, step: TaskStep, last: &str) -> TaskState {
        let document = serde_json::json!({"schema": "graphhelm-task-event-v1", "taskId": "issue-7",
            "revision": 1, "at": OLD, "issue": 7, "lane": lane, "branch": "issue-7-x"});
        let record = parse_task_event(
            "task.claimed",
            lane,
            &document.to_string(),
            1,
            Some(at(OLD)),
        )
        .unwrap();
        let mut state = fold_task_events(&[record]).remove(0);
        state.step = step;
        state.clock.last_at = Some(at(last));
        state
    }

    fn with_pr(mut state: TaskState) -> TaskState {
        state.pr = Some(70);
        state.head_sha = Some("abc".into());
        state
    }

    fn request(id: &str, recorded: &str) -> RequestView {
        RequestView {
            request_id: id.into(),
            ask: "do it".into(),
            title: None,
            recorded_by: "owner".into(),
            revision: 1,
            sequence: 1,
            recorded_at: Some(at(recorded)),
            status: RequestStatus::Open,
            issues: Vec::new(),
            dispatched_to: None,
            dispatch_lapsed: Vec::new(),
            dispatch_attempts: 0,
        }
    }

    fn silent(lanes: &[&str]) -> BTreeSet<String> {
        lanes.iter().map(|lane| (*lane).to_owned()).collect()
    }

    fn found(tasks: &[TaskState], requests: &[RequestView], quiet: &[&str]) -> Vec<Diagnosis> {
        classify(tasks, requests, &silent(quiet), &at(NOW), Grace::default())
    }

    fn stages(tasks: &[TaskState], requests: &[RequestView], quiet: &[&str]) -> Vec<Stage> {
        found(tasks, requests, quiet)
            .into_iter()
            .map(|diagnosis| diagnosis.stage)
            .collect()
    }

    #[test]
    fn an_old_unlinked_request_with_no_live_hand_off_is_request_unclaimed() {
        let diagnosis = found(&[], &[request("r1", OLD)], &[]);
        assert_eq!(diagnosis.len(), 1);
        assert_eq!(diagnosis[0].stage, Stage::RequestUnclaimed);
        assert_eq!(diagnosis[0].role, Role::Author);
        // Inside the grace the #1990 dispatcher still has it.
        assert!(stages(&[], &[request("r1", FRESH)], &[]).is_empty());
        let mut handed = request("r1", OLD);
        handed.dispatched_to = Some("lane-a".into());
        assert!(stages(&[], &[handed], &[]).is_empty());
        let mut linked = request("r1", OLD);
        linked.issues.push(RequestIssue {
            issue: 1,
            slices: Vec::new(),
        });
        linked.status = RequestStatus::InProgress;
        assert!(stages(&[], &[linked], &[]).is_empty());
    }

    #[test]
    fn a_silent_author_before_its_pr_is_author_silent() {
        let task = slice("lane-a", TaskStep::Implement, OLD);
        assert_eq!(
            stages(std::slice::from_ref(&task), &[], &["lane-a"]),
            [Stage::AuthorSilent]
        );
        assert!(stages(std::slice::from_ref(&task), &[], &[]).is_empty());
        // A live lane's own slice of the same issue: the work was taken over, nothing to start.
        let mut taker = slice("auto-author-issue-7", TaskStep::Implement, FRESH);
        taker.key = "issue-7#claim-1".into();
        assert!(stages(&[task, taker], &[], &["lane-a"]).is_empty());
    }

    #[test]
    fn a_slice_with_an_open_pr_never_reads_author_silent() {
        // #2027: PR #2025 read author_silent minutes after it was opened. A recorded PR (or any
        // review record) means the author handed the work on, whatever step the fold left.
        for step in [TaskStep::Plan, TaskStep::Critic, TaskStep::Implement] {
            let fresh = with_pr(slice("lane-a", step, FRESH));
            assert!(
                stages(&[fresh], &[], &["lane-a"]).is_empty(),
                "{step:?}: a freshly opened PR gets its grace"
            );
            let old = with_pr(slice("lane-a", step, OLD));
            assert_eq!(
                stages(&[old], &[], &["lane-a"]),
                [Stage::ReviewUnassigned],
                "{step:?}: past the grace it waits for a reviewer, not an author"
            );
            let mut reviewed = slice("lane-a", step, OLD);
            reviewed.reviewers.push("rev".into());
            assert!(
                stages(&[reviewed], &[], &["lane-a"]).is_empty(),
                "{step:?}: a slice with a verdict is not author_silent"
            );
            let mut assigned = slice("lane-a", step, OLD);
            assigned.assigned_reviewers.push("rev".into());
            assert!(
                stages(&[assigned], &[], &["lane-a"]).is_empty(),
                "{step:?}: a slice with a reviewer assigned is not author_silent"
            );
        }
    }

    #[test]
    fn a_reviewer_of_an_older_head_does_not_hide_review_unassigned_before_the_review_step() {
        // #2039: the review-step branch ignores assigned/past reviewers; the early branch did not.
        for step in [TaskStep::Plan, TaskStep::Critic, TaskStep::Implement] {
            let mut task = with_pr(slice("lane-a", step, OLD));
            task.assigned_reviewers.push("rev".into());
            task.reviewers.push("rev".into());
            assert_eq!(
                stages(std::slice::from_ref(&task), &[], &[]),
                [Stage::ReviewUnassigned],
                "{step:?}"
            );
            // The current head's reviewer, gone silent, is reviewer_silent.
            task.current_reviewer = Some("rev".into());
            assert_eq!(
                stages(std::slice::from_ref(&task), &[], &["rev"]),
                [Stage::ReviewerSilent],
                "{step:?}"
            );
            assert!(stages(&[task], &[], &[]).is_empty(), "{step:?}");
        }
    }

    #[test]
    fn a_pr_with_no_reviewer_past_grace_is_review_unassigned() {
        let task = with_pr(slice("lane-a", TaskStep::Review, OLD));
        let diagnosis = found(std::slice::from_ref(&task), &[], &[]);
        assert_eq!(diagnosis[0].stage, Stage::ReviewUnassigned);
        assert_eq!(diagnosis[0].role, Role::Reviewer);
        assert_eq!(lane_name(&diagnosis[0]), "auto-reviewer-pr-70");
        // An author waiting on review is not silent work, even when its lane is quiet.
        assert_eq!(
            stages(std::slice::from_ref(&task), &[], &["lane-a"]),
            [Stage::ReviewUnassigned]
        );
        let fresh = with_pr(slice("lane-a", TaskStep::Review, FRESH));
        assert!(stages(&[fresh], &[], &[]).is_empty());
    }

    #[test]
    fn an_assigned_reviewer_that_went_silent_is_reviewer_silent() {
        let mut task = with_pr(slice("lane-a", TaskStep::Review, OLD));
        task.current_reviewer = Some("rev".into());
        assert!(stages(std::slice::from_ref(&task), &[], &[]).is_empty());
        let diagnosis = found(&[task], &[], &["rev"]);
        assert_eq!(diagnosis[0].stage, Stage::ReviewerSilent);
        assert_eq!(diagnosis[0].stalled_lane.as_deref(), Some("rev"));
    }

    #[test]
    fn a_block_with_a_silent_author_is_block_unanswered() {
        let mut task = with_pr(slice("lane-a", TaskStep::Implement, OLD));
        task.blocked_by = Some(Blocker {
            reviewer: "rev".into(),
            head_sha: "abc".into(),
            comment_url: "u".into(),
        });
        assert_eq!(
            stages(std::slice::from_ref(&task), &[], &["lane-a"]),
            [Stage::BlockUnanswered]
        );
        // A live author is working on it.
        assert!(stages(&[task], &[], &[]).is_empty());
    }

    #[test]
    fn an_approved_head_unmerged_past_grace_is_approved_not_merged() {
        let mut task = with_pr(slice("lane-a", TaskStep::Merge, OLD));
        task.reviewers.push("rev".into());
        let diagnosis = found(std::slice::from_ref(&task), &[], &[]);
        assert_eq!(diagnosis[0].stage, Stage::ApprovedNotMerged);
        assert_eq!(diagnosis[0].role, Role::Merger);
        assert_eq!(diagnosis[0].stalled_lane.as_deref(), Some("rev"));
        let fresh = with_pr(slice("lane-a", TaskStep::Merge, FRESH));
        assert!(stages(&[fresh], &[], &[]).is_empty());
    }

    #[test]
    fn merged_released_and_orphaned_slices_are_never_diagnosed() {
        let merged = slice("lane-a", TaskStep::Merged, OLD);
        let mut released = slice("lane-a", TaskStep::Implement, OLD);
        released.released_by = Some("lane-a".into());
        let mut orphaned = slice("lane-b", TaskStep::Implement, OLD);
        orphaned.orphaned = true;
        assert!(stages(&[merged, released, orphaned], &[], &["lane-a", "lane-b"]).is_empty());
    }

    #[test]
    fn a_lane_that_records_requests_or_never_worked_takes_no_work() {
        let author = slice("author", TaskStep::Merged, OLD);
        let lead = slice("lead", TaskStep::Merged, OLD);
        let tasks = [author, lead];
        let document = serde_json::json!({"schema": "graphhelm-task-event-v1",
            "taskId": "request-r1", "revision": 1, "at": OLD, "requestId": "r1",
            "ask": "do it", "recordedBy": "lead"});
        let request = parse_task_event(
            "task.request_recorded",
            "lead",
            &document.to_string(),
            2,
            Some(at(OLD)),
        )
        .unwrap();
        let records = [request];
        assert!(takes_work("author", &tasks, &records));
        assert!(
            !takes_work("lead", &tasks, &records),
            "owns merged slices but records requests: the orchestrator"
        );
        assert!(!takes_work("stranger", &tasks, &records));
    }

    fn limits() -> Limits {
        Limits {
            max_attempts: 3,
            max_concurrent: 2,
            retry_seconds: 600,
        }
    }

    #[test]
    fn plan_launches_once_then_waits_retries_on_failure_and_escalates_at_the_cap() {
        let diagnosis = found(&[], &[request("r1", OLD)], &[]);
        let first = plan(&diagnosis, |_| History::default(), 0, limits());
        assert!(matches!(
            first.as_slice(),
            [Step::Launch { attempt: 0, .. }]
        ));
        let running = History {
            attempts: 1,
            last_age: Some(60),
            running: true,
            ..History::default()
        };
        assert!(plan(&diagnosis, |_| running, 1, limits()).is_empty());
        let ended = History {
            running: false,
            ..running
        };
        assert!(plan(&diagnosis, |_| ended, 0, limits()).is_empty());
        // #2027: a failed attempt is no reason to retry at once; three login failures in one
        // minute spent every attempt. The next one waits retry_seconds like any other.
        let failed = History {
            failed: true,
            ..ended
        };
        assert!(plan(&diagnosis, |_| failed, 0, limits()).is_empty());
        let failed_lapsed = History {
            last_age: Some(600),
            ..failed
        };
        assert!(matches!(
            plan(&diagnosis, |_| failed_lapsed, 0, limits()).as_slice(),
            [Step::Launch { attempt: 1, .. }]
        ));
        let lapsed = History {
            last_age: Some(600),
            ..ended
        };
        assert!(matches!(
            plan(&diagnosis, |_| lapsed, 0, limits()).as_slice(),
            [Step::Launch { attempt: 1, .. }]
        ));
        let spent = History {
            attempts: 3,
            failed: true,
            ..ended
        };
        assert!(matches!(
            plan(&diagnosis, |_| spent, 0, limits()).as_slice(),
            [Step::Escalate { attempts: 3, .. }]
        ));
        let told = History {
            escalated: true,
            ..spent
        };
        assert!(plan(&diagnosis, |_| told, 0, limits()).is_empty());
    }

    #[test]
    fn plan_respects_the_concurrency_bound() {
        let tasks: Vec<TaskState> = (0..4)
            .map(|n| {
                let mut task = with_pr(slice("lane-a", TaskStep::Review, OLD));
                task.key = format!("k{n}");
                task.pr = Some(100 + n);
                task
            })
            .collect();
        let diagnosis = found(&tasks, &[], &[]);
        assert_eq!(diagnosis.len(), 4);
        assert_eq!(
            plan(&diagnosis, |_| History::default(), 0, limits()).len(),
            2
        );
        assert!(plan(&diagnosis, |_| History::default(), 2, limits()).is_empty());
    }

    #[test]
    fn action_ids_are_stable_per_item_stage_attempt() {
        let id = action_signal_id("slice:k", Stage::ReviewUnassigned, 0);
        assert_eq!(id, action_signal_id("slice:k", Stage::ReviewUnassigned, 0));
        assert_ne!(id, action_signal_id("slice:k", Stage::ReviewUnassigned, 1));
        assert_ne!(id, action_signal_id("slice:k", Stage::ReviewerSilent, 0));
        assert!(id.starts_with("autopilot-"));
    }

    #[test]
    fn a_lane_name_never_carries_request_text() {
        let diagnosis = found(&[], &[request("x\"; rm -rf / #", OLD)], &[]);
        let lane = lane_name(&diagnosis[0]);
        assert!(lane.starts_with("auto-author-req-"));
        assert!(
            lane.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        );
        assert_eq!(diagnosis[0].subject(), "an owner request");
    }
}
