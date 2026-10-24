//! `GET /v1/executions/{id}/tasks`, the MCP tool `tasks` and `graphhelm execution status --tasks`
//! (#1149, optimization plan A1): the Runtime opens an execution's sealed `task.*` envelopes,
//! keeps the ones that verify, and serves `graphhelm_execution::tasks::fold_task_events_with_routes` over them.
//!
//! What verifies: the recorded event has exactly one evidence reference, naming the envelope
//! `signal-<signalId>`; the sealed content's digest is the one the event recorded; the envelope
//! opens with the Runtime's keyring; its `type` is the recorded kind and its `source.id` is the
//! actor that recorded it; and its description parses as a `graphhelm-task-event-v1` document for
//! that kind naming that actor. Anything else is counted in `rejected` and never drawn, which is
//! how the Studio read its own fold (`readTaskEventRecords`), so an old log folds the same here.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use graphhelm_events::{EvidenceOpener, EvidenceRead};
use graphhelm_execution::tasks::{
    FoldSlice, QueuedIssue, Route, TaskDurations, TaskEventRecord, TaskState, TaskStep, backlog,
    fold_task_events_with_routes, parse_task_event, task_durations,
};
use graphhelm_protocols::{EventKind, EvidenceId, PersistedTimestamp};
use serde::Serialize;

use super::signal::SignalKeyring;
use super::{Failure, resolve_stream};
use crate::commands::event_store;

/// The reply of every door: the slices, the verified records they were folded from (in sequence
/// order, with the Runtime's `occurredAt`), the stream head and how many envelopes did not verify.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TasksView {
    pub execution_id: String,
    pub head_sequence: u64,
    pub tasks: Vec<TaskState>,
    pub records: Vec<TaskEventRecord>,
    pub rejected: usize,
    /// #1192: every slice the fold created (live, ended or absorbed) with its id, and the slice
    /// each record landed in, so the Studio's lane timeline needs no fold of its own.
    pub slices: Vec<FoldSlice>,
    pub routes: Vec<Route>,
    /// #1202: how many sealed envelopes THIS read opened. A Runtime that remembers the run opens
    /// none on an unchanged head and one per `task.*` record since the last read.
    pub opened: usize,
    /// #1193: the Runtime's clock at this read, and the lease of every live pre-PR claim judged
    /// against it; the reconciler releases the `overdue` ones as `graphhelm`.
    pub now: PersistedTimestamp,
    pub leases: Vec<ClaimLease>,
    /// #1235: every open PR slice with no task record for `STALL_SECONDS`, judged at `now`, with
    /// who owes the next step. A dead fixer behind a BLOCK reads here instead of sitting silent.
    pub stalls: Vec<TaskStall>,
    /// #1234 (plan A7): lane liveness judged once, here, from the declared budgets and the
    /// Runtime's own event times at `now`; the Studio reads it instead of keeping thresholds.
    pub liveness: Liveness,
    /// #1469: the newest `task.queued` record's open wave issues no live slice holds, oldest
    /// first: the Lanes Backlog. Empty when no issue list was recorded.
    pub backlog: Vec<QueuedIssue>,
    /// The lanes the owner retired (`task.lane_retired`); the signal door refuses their records.
    pub retired_lanes: Vec<String>,
    /// #1794 (Studio v2 R3): every question a lane asked that no reply answers yet, oldest first.
    pub waiting: Vec<LaneWaiting>,
    /// #1981: the owner dispatch notes on the journal (signal id to the lane it names), read from
    /// plaintext. Not served on the tasks routes; the requests route reads it.
    #[serde(skip)]
    pub dispatch_notes: BTreeMap<String, graphhelm_execution::requests::DispatchNote>,
}

/// #1981: every owner `operator_note` whose signal id is a request dispatch id, by id, with its
/// `to` and the Runtime's append time. No envelope is opened.
pub fn dispatch_notes(
    history: &[graphhelm_protocols::EventEnvelope],
) -> BTreeMap<String, graphhelm_execution::requests::DispatchNote> {
    use graphhelm_execution::autopilot::AUTOPILOT_ACTOR;
    use graphhelm_protocols::PersistedActorType;
    // #2005: the autopilot's own notes share this map (ids never collide: each has its prefix):
    // its launches are owner notes, its escalations and pauses come from its own actor.
    let ours = |event: &graphhelm_protocols::EventEnvelope, id: &str| match event.actor.actor_type()
    {
        PersistedActorType::Owner => id.starts_with("dispatch-") || id.starts_with("autopilot-"),
        PersistedActorType::Agent => {
            event.actor.id().as_str() == AUTOPILOT_ACTOR && id.starts_with("autopilot-")
        }
        _ => false,
    };
    history
        .iter()
        .filter_map(|event| match &event.kind {
            EventKind::SignalRecorded(record)
                if record.kind == "operator_note" && ours(event, record.signal_id.as_str()) =>
            {
                record.to.as_ref().map(|to| {
                    (
                        record.signal_id.as_str().to_owned(),
                        graphhelm_execution::requests::DispatchNote {
                            lane: to.as_str().to_owned(),
                            at: event.occurred_at.clone(),
                        },
                    )
                })
            }
            _ => None,
        })
        .collect()
}

/// #1981: how long a hand-off stays live without a link: `GRAPHHELM_DISPATCH_LEASE_SECONDS` when
/// it is a positive number, else `DISPATCH_LEASE_SECONDS`.
fn dispatch_lease_seconds() -> i64 {
    std::env::var("GRAPHHELM_DISPATCH_LEASE_SECONDS")
        .ok()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .filter(|seconds| *seconds > 0)
        .unwrap_or(graphhelm_execution::requests::DISPATCH_LEASE_SECONDS)
}

/// #1981: the run's owner requests with their hand-offs judged at the read's `now`: a hand-off
/// lapses past the lease or when its lane is silent or no longer judged (retired).
#[must_use]
pub fn owner_requests(view: &TasksView) -> Vec<graphhelm_execution::requests::RequestView> {
    let mut requests = graphhelm_execution::requests::requests(&view.records, &view.tasks);
    graphhelm_execution::requests::mark_dispatched(
        &mut requests,
        &view.dispatch_notes,
        &view.now,
        dispatch_lease_seconds(),
        |lane| {
            !view
                .liveness
                .lanes
                .iter()
                .any(|row| row.lane == lane && !row.silent)
        },
    );
    requests
}

/// #1794: one open question of a lane. A lane asks with a signal of type `question` addressed
/// (`to`) to the owner (`studio-operator`) or to another lane, or with an `operator_note` to
/// `studio-operator` (the convention the reply suggestions already read). Any later signal whose
/// `replyTo` names it answers it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaneWaiting {
    pub lane: String,
    pub question: String,
    /// The actor the question is addressed to (its `to`), as the asker wrote it: the owner's
    /// `studio-operator` or a lane id. No role is inferred here; a reader labels it from the
    /// roster.
    pub waits_on: String,
    pub since: PersistedTimestamp,
    pub signal_id: String,
    pub sequence: u64,
    /// #1827: how many unanswered asks this lane has in all. Only its newest
    /// `WAITING_PER_LANE` are opened and can be rows, so `laneTotal` minus the lane's rows is the
    /// "+K more" a reader shows. An ask among the newest whose envelope is refused still counts.
    pub lane_total: usize,
}

/// #1827: the most open questions served, and envelopes opened, per lane. Older unanswered asks
/// are counted in `laneTotal` but never opened, so the read grows with lanes, not with history.
pub const WAITING_PER_LANE: usize = 5;

/// #1794: the longest question text served; a longer one is not a question to show.
pub const QUESTION_MAX_CHARS: usize = 2000;
/// The owner's address on a run, shared with the reply suggestions.
pub const OWNER_ADDRESS: &str = "studio-operator";

/// How much of the fold a read answers. `Open` is every door's default: a long-lived run's
/// record, slice and route histories grow without bound (3.9 MB on the gh-team run), past what
/// a model can read in one tool reply. `Full` is the whole `TasksView`, for the Studio's lane
/// timeline, which draws those histories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TasksDepth {
    Open,
    Full,
}

/// The default reply: the tasks still in flight and everything judged about them, sized by the
/// open work rather than by the run's history. `endedTasks` counts the merged slices left out,
/// so a reader knows the full view has more.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenTasksView {
    pub view: &'static str,
    pub execution_id: String,
    pub head_sequence: u64,
    pub tasks: Vec<TaskState>,
    pub ended_tasks: usize,
    pub rejected: usize,
    pub opened: usize,
    pub now: PersistedTimestamp,
    pub leases: Vec<ClaimLease>,
    pub stalls: Vec<TaskStall>,
    pub liveness: Liveness,
    /// #1469: small, and the one list a reader of the open board cannot rebuild from `tasks`.
    pub backlog: Vec<QueuedIssue>,
    /// #1516: step durations; the open board lists the open slices and the merged ones' summary.
    pub durations: TaskDurations,
    /// #1794: the open questions, as in the full view.
    pub waiting: Vec<LaneWaiting>,
}

/// #1516: the `?since=` window of a read: an RFC 3339 instant.
pub fn parse_since(value: &str) -> Result<PersistedTimestamp, Failure> {
    PersistedTimestamp::parse(value).map_err(|_| {
        super::signal_invalid(
            "since must be an RFC 3339 instant such as 2026-10-15T00:00:00Z",
            "/since",
        )
    })
}

/// #1449: what one lane reads of the board. `lane` keeps the tasks that lane authors or is
/// assigned to review; `issue` keeps the tasks of that issue. Both given: a task must match both.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TasksFilter {
    pub lane: Option<String>,
    pub issue: Option<u64>,
}

impl TasksFilter {
    /// Parse `lane=` and `issue=` from a raw query. A lane outside the bounded id alphabet or a
    /// non-numeric issue is a failure at `/lane` or `/issue`, never an empty answer.
    pub fn from_query(query: &str) -> Result<Self, Failure> {
        let mut filter = Self::default();
        for pair in query.split('&') {
            if let Some(raw) = pair.strip_prefix("lane=") {
                filter.lane = Some(parse_lane(raw)?);
            } else if let Some(issue) = pair.strip_prefix("issue=") {
                let issue = super::super::serve::percent_decode(issue).unwrap_or_default();
                filter.issue = Some(
                    issue
                        .parse::<u64>()
                        .map_err(|_| super::signal_invalid("issue must be a number", "/issue"))?,
                );
            }
        }
        Ok(filter)
    }

    fn keeps(&self, task: &TaskState) -> bool {
        self.lane.as_deref().is_none_or(|lane| {
            task.lane.as_deref() == Some(lane)
                || task.current_reviewer.as_deref() == Some(lane)
                || task.assigned_reviewers.iter().any(|name| name == lane)
        }) && self.issue.is_none_or(|issue| task.issue == Some(issue))
    }
}

/// A lane named in a query or a path. The MCP door percent-encodes the value (`:` is `%3A`), so
/// it is decoded before it is judged; outside the bounded id alphabet it is a failure at `/lane`.
pub fn parse_lane(raw: &str) -> Result<String, Failure> {
    let lane = super::super::serve::percent_decode(raw).unwrap_or_default();
    let valid = !lane.is_empty()
        && lane.len() <= 128
        && lane
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._:-".contains(&c));
    if !valid {
        return Err(super::signal_invalid(
            "lane must be 1 to 128 characters of [A-Za-z0-9._:-]",
            "/lane",
        ));
    }
    Ok(lane)
}

/// #1793: how many actions the activity log answers when the query names no `limit`, and the
/// most it ever answers, so one read stays small whatever the run's history holds.
pub const ACTIVITY_DEFAULT_LIMIT: usize = 10;
pub const ACTIVITY_MAX_LIMIT: usize = 50;

/// #1793: `GET /v1/executions/{id}/lanes/{lane}/activity`: the lane's last `limit` actions,
/// newest first (by event sequence). An action is a verified `task.*` record the lane itself
/// recorded; a heartbeat counts only when it carries a reason (the #1556 rule), since a
/// reasonless one only renews a lease. The records are the ones `tasks?full=1` already serves.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaneActivity {
    pub execution_id: String,
    pub lane: String,
    pub limit: usize,
    pub actions: Vec<TaskEventRecord>,
}

/// `limit=` from a raw query: absent is the default; anything but 1..=max is a failure at `/limit`.
pub fn parse_activity_limit(query: &str) -> Result<usize, Failure> {
    let Some(raw) = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("limit="))
    else {
        return Ok(ACTIVITY_DEFAULT_LIMIT);
    };
    raw.parse::<usize>()
        .ok()
        .filter(|limit| (1..=ACTIVITY_MAX_LIMIT).contains(limit))
        .ok_or_else(|| super::signal_invalid("limit must be a number from 1 to 50", "/limit"))
}

impl TasksView {
    /// #1793: the lane's activity log over this view's records (see [`LaneActivity`]).
    pub fn lane_activity(&self, lane: &str, limit: usize) -> serde_json::Value {
        let mut actions: Vec<TaskEventRecord> = self
            .records
            .iter()
            .filter(|record| record.actor_id == lane)
            .filter(|record| {
                record.kind != graphhelm_execution::tasks::TaskEventKind::Heartbeat
                    || record.reason.is_some()
            })
            .cloned()
            .collect();
        actions.sort_by_key(|record| std::cmp::Reverse(record.sequence));
        actions.truncate(limit);
        serde_json::to_value(LaneActivity {
            execution_id: self.execution_id.clone(),
            lane: lane.to_owned(),
            limit,
            actions,
        })
        .expect("an activity log of serializable task records")
    }
}

impl TasksView {
    /// #1851: the view an agent credential reads: the lanes' reported kind and model are the
    /// owner's (the Lanes card), so they leave the liveness rows, their only serialized home
    /// (`TaskEventRecord` never serializes them, so no door that serves records carries them).
    #[must_use]
    pub fn without_lane_identity(mut self) -> Self {
        for row in &mut self.liveness.lanes {
            row.agent_kind = None;
            row.agent_model = None;
        }
        self
    }

    /// #1449: keep only the tasks `filter` names, and the leases and stalls of those tasks. The
    /// header fields, records and slices are untouched; an empty filter returns `self` as is.
    pub fn filtered(mut self, filter: &TasksFilter) -> Self {
        if *filter == TasksFilter::default() {
            return self;
        }
        self.tasks.retain(|task| filter.keeps(task));
        let keys: std::collections::BTreeSet<&str> =
            self.tasks.iter().map(|task| task.key.as_str()).collect();
        self.leases
            .retain(|lease| keys.contains(lease.key.as_str()));
        self.stalls
            .retain(|stall| keys.contains(stall.key.as_str()));
        if let Some(lane) = &filter.lane {
            self.waiting.retain(|row| &row.lane == lane);
        }
        self
    }

    /// The reply a door serves at `depth`: the full view, or the open board. Both carry
    /// `durations` judged against `since` (#1516).
    pub fn at_depth(
        self,
        depth: TasksDepth,
        since: Option<&PersistedTimestamp>,
    ) -> serde_json::Value {
        let mut durations = task_durations(&self.records, &self.slices, since);
        let value = match depth {
            TasksDepth::Full => serde_json::to_value(&self).map(|mut value| {
                value["durations"] = serde_json::to_value(&durations).expect("durations serialize");
                value
            }),
            TasksDepth::Open => {
                durations.slices.retain(|row| !row.merged);
                let total = self.tasks.len();
                let tasks: Vec<TaskState> = self
                    .tasks
                    .into_iter()
                    .filter(|task| task.step != TaskStep::Merged)
                    .collect();
                serde_json::to_value(OpenTasksView {
                    view: "open",
                    execution_id: self.execution_id,
                    head_sequence: self.head_sequence,
                    ended_tasks: total - tasks.len(),
                    tasks,
                    rejected: self.rejected,
                    opened: self.opened,
                    now: self.now,
                    leases: self.leases,
                    stalls: self.stalls,
                    liveness: self.liveness,
                    backlog: self.backlog,
                    durations,
                    waiting: self.waiting,
                })
            }
        };
        value.expect("a view of serializable fold types")
    }
}

/// #1234: a lane with an open obligation is `silent` once it has recorded nothing of any kind for
/// this long. Was the Studio's `LIVENESS_MS` (`lane-bars.ts`); declared here, once.
pub const SILENT_AFTER_SECONDS: i64 = 30 * 60;

/// #1808: the silence budget in force. A journey fixture's Runtime may be started with a shorter
/// one (`GRAPHHELM_FIXTURE_SILENT_AFTER_SECONDS`, 1 to 1800) so it can show a silent lane without
/// waiting half an hour; anything else, absent or out of range, is `SILENT_AFTER_SECONDS`. It only
/// ever shortens the budget, and it changes what the board shows, never who may act.
pub fn silent_after_seconds() -> i64 {
    silent_budget(
        std::env::var("GRAPHHELM_FIXTURE_SILENT_AFTER_SECONDS")
            .ok()
            .as_deref(),
    )
}

fn silent_budget(raw: Option<&str>) -> i64 {
    raw.and_then(|value| value.trim().parse::<i64>().ok())
        .filter(|seconds| (1..=SILENT_AFTER_SECONDS).contains(seconds))
        .unwrap_or(SILENT_AFTER_SECONDS)
}
/// #1234: a lane that recorded something within this long counts as live on the board. Was the
/// Studio's `FREE_LIVENESS_MS` (`agent-board.ts`). The board's third threshold, a PR-less claim
/// quiet for 2 h, has no twin here: a claim is stale exactly when its lease is `overdue` (#1193).
pub const FREE_AFTER_SECONDS: i64 = 15 * 60;

/// #1234: the budgets the verdicts below were judged against, served beside them so a reader
/// never needs a copy of its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LivenessBudgets {
    pub silent_after_seconds: i64,
    pub free_after_seconds: i64,
    /// #1235's `STALL_SECONDS`, served here too so every threshold a reader needs is in one place.
    pub stall_after_seconds: i64,
    /// #1920: the lane-silence policy window; see [`retire_after_seconds`].
    pub retire_after_seconds: i64,
}

/// #1920: the project policy a Runtime retirement cites. A lane silent this long with no open
/// obligation is retired by the Runtime itself; one that still owes work is only flagged.
pub const LANE_SILENCE_POLICY: &str = "lane-silence-retire";
/// #1920: the default policy window, 12 h.
pub const RETIRE_AFTER_SECONDS: i64 = 12 * 60 * 60;
/// #1920: the actor a Runtime retirement is attributed to, the Stop reconciler's name.
pub const RUNTIME_ACTOR: &str = "graphhelm";

/// #1920: the window in force. A project sets it with `GRAPHHELM_LANE_RETIRE_AFTER_SECONDS` on the
/// Runtime (a positive whole number of seconds); absent or garbled, it is 12 h.
pub fn retire_after_seconds() -> i64 {
    retire_budget(
        std::env::var("GRAPHHELM_LANE_RETIRE_AFTER_SECONDS")
            .ok()
            .as_deref(),
    )
}

fn retire_budget(raw: Option<&str>) -> i64 {
    raw.and_then(|value| value.trim().parse::<i64>().ok())
        .filter(|seconds| *seconds > 0)
        .unwrap_or(RETIRE_AFTER_SECONDS)
}

/// #1920: one Runtime verdict on a silent lane: a retirement (no open obligation) or a flag (it
/// still owes work, so nothing is dropped). It cites the policy, its window and the time of the
/// lane's last record (absent when it has none). It is derived at every read, so the lane's next
/// record restores it: no stored retirement exists for the signal door to refuse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaneSilence {
    pub lane: String,
    pub by: &'static str,
    pub policy: &'static str,
    pub window_seconds: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_record_at: Option<PersistedTimestamp>,
}

/// #1935: what a lane's signals (not its task records) say for the silence policy: the newest
/// signal each agent lane sent, and the lanes with a question to the owner nothing answers yet.
/// Task records (`task.*` kinds) are left to [`last_step_by_actor`], which drops the reasonless
/// heartbeat, so that heartbeat stays excluded here too.
#[derive(Debug, Default)]
pub struct LaneSignals {
    pub last_signal_at: BTreeMap<String, PersistedTimestamp>,
    pub asking: std::collections::BTreeSet<String>,
}

/// #1794/#1935: an agent signal that asks the owner: a `question`, or an `operator_note` to them.
fn asks_owner(record: &graphhelm_protocols::SignalRecorded) -> bool {
    record.kind == "question"
        || (record.kind == "operator_note"
            && record
                .to
                .as_ref()
                .is_some_and(|to| to.as_str() == OWNER_ADDRESS))
}

/// #1935: read [`LaneSignals`] from the journal's plaintext; no envelope is opened.
pub fn lane_signals(history: &[graphhelm_protocols::EventEnvelope]) -> LaneSignals {
    let answered: std::collections::BTreeSet<&str> = history
        .iter()
        .filter_map(|event| match &event.kind {
            EventKind::SignalRecorded(record) => record.reply_to.as_ref().map(|id| id.as_str()),
            _ => None,
        })
        .collect();
    let mut facts = LaneSignals::default();
    for event in history {
        let EventKind::SignalRecorded(record) = &event.kind else {
            continue;
        };
        if event.actor.actor_type() != graphhelm_protocols::PersistedActorType::Agent
            || record.kind.starts_with("task.")
        {
            continue;
        }
        let lane = event.actor.id().as_str();
        if asks_owner(record) && !answered.contains(record.signal_id.as_str()) {
            facts.asking.insert(lane.to_owned());
        }
        let newer = facts
            .last_signal_at
            .get(lane)
            .is_none_or(|current| event.occurred_at.as_datetime() > current.as_datetime());
        if newer {
            facts
                .last_signal_at
                .insert(lane.to_owned(), event.occurred_at.clone());
        }
    }
    facts
}

/// #1920: split the judged lanes by the silence policy at `now`. Lanes silent for `window` with no
/// obligation leave `lanes` and are returned as retired; silent lanes that owe stay and are
/// returned as flagged. Pure, so the three cases are unit cells.
///
/// #1935: silence is measured from the later of the lane's last task record and its last signal,
/// and a lane with an unanswered question to the owner is only flagged, never retired, so its
/// question stays in `waiting`.
pub fn apply_lane_silence(
    lanes: &mut Vec<LaneLiveness>,
    signals: &LaneSignals,
    window: i64,
    now: &PersistedTimestamp,
) -> (Vec<LaneSilence>, Vec<LaneSilence>) {
    let mut retired = Vec::new();
    let mut flagged = Vec::new();
    lanes.retain(|row| {
        let quiet = [
            row.last_record_at.as_ref(),
            signals.last_signal_at.get(&row.lane),
        ]
        .into_iter()
        .flatten()
        .map(|at| (*now.as_datetime() - *at.as_datetime()).num_seconds())
        .min();
        if quiet.is_some_and(|seconds| seconds < window) {
            return true;
        }
        let verdict = LaneSilence {
            lane: row.lane.clone(),
            by: RUNTIME_ACTOR,
            policy: LANE_SILENCE_POLICY,
            window_seconds: window,
            last_record_at: row.last_record_at.clone(),
        };
        if row.owes || signals.asking.contains(&row.lane) {
            flagged.push(verdict);
            true
        } else {
            retired.push(verdict);
            false
        }
    });
    (retired, flagged)
}

/// #1234: one lane's liveness at the read's `now`. `last_record_at` is the Runtime's time of the
/// lane actor's newest event of any kind in this run (absent when it has none); `owes` is whether
/// it holds an open obligation (an unreleased, unmerged slice it authored, or the current review
/// of an open PR) and `owes_since` when the oldest of those began (the claim, or the review
/// assignment). `silent` is `owes` and quiet for `silent_after_seconds`, quiet measured from the
/// LATER of the last record and `owes_since`: a reviewer assigned a minute ago is not silent
/// because its own last record is old (gh-claude-opt-2 on PR #1249). `free` is a record within
/// `free_after_seconds`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaneLiveness {
    pub lane: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_record_at: Option<PersistedTimestamp>,
    pub owes: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owes_since: Option<PersistedTimestamp>,
    pub silent: bool,
    pub free: bool,
    /// #1851: the kind and model the lane reported on its newest `task.claimed` that names them;
    /// absent when it never did (never guessed from the name). Served to owner credentials only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Liveness {
    pub budgets: LivenessBudgets,
    pub lanes: Vec<LaneLiveness>,
    /// #1920: lanes the Runtime retired under the silence policy at this read; not in `lanes`.
    pub retired: Vec<LaneSilence>,
    /// #1920: lanes silent past the policy window that still owe work; flagged, never retired.
    pub flagged: Vec<LaneSilence>,
}

/// #1556: the newest task STEP each actor recorded. A reasonless `task.heartbeat` renews a lease,
/// and the Stop hook sends one for an idle session, so it is not activity (a heartbeat WITH a
/// reason is: the lane said why it waits): counting it kept a dead claim
/// "active 12s ago" for 37 h and kept it out of Silent. Slot work (building, waiting) is read from
/// the slot directory, not from events, and is added by the reader.
pub fn last_step_by_actor(records: &[TaskEventRecord]) -> BTreeMap<String, PersistedTimestamp> {
    let mut last: BTreeMap<String, PersistedTimestamp> = BTreeMap::new();
    for record in records {
        // #1556: a heartbeat that says why the lane waits is a deliberate statement and counts;
        // the Stop hook's reasonless heartbeat does not.
        if record.kind == graphhelm_execution::tasks::TaskEventKind::Heartbeat
            && record.reason.is_none()
        {
            continue;
        }
        let Some(at) = &record.occurred_at else {
            continue;
        };
        let newer = last
            .get(&record.actor_id)
            .is_none_or(|current| at.as_datetime() > current.as_datetime());
        if newer {
            last.insert(record.actor_id.clone(), at.clone());
        }
    }
    last
}

/// #1234: the verdict for one lane, pure (`now` injected), so the edges are a unit cell. Silence
/// is measured from the later of the lane's last record and when its oldest open obligation began;
/// with neither, a lane that owes is silent. Freedom reads the record alone.
pub fn lane_liveness(
    lane: &str,
    last_record_at: Option<&PersistedTimestamp>,
    owes_since: Option<&PersistedTimestamp>,
    owes: bool,
    now: &PersistedTimestamp,
) -> LaneLiveness {
    let seconds_since =
        |at: &PersistedTimestamp| (*now.as_datetime() - *at.as_datetime()).num_seconds();
    let record_quiet = last_record_at.map(seconds_since);
    let quiet = match (record_quiet, owes_since.map(seconds_since)) {
        (Some(record), Some(since)) => Some(record.min(since)),
        (record, since) => record.or(since),
    };
    LaneLiveness {
        lane: lane.to_owned(),
        last_record_at: last_record_at.cloned(),
        owes,
        owes_since: owes_since.cloned(),
        silent: owes && quiet.is_none_or(|seconds| seconds >= silent_after_seconds()),
        // #1281: a lane that owes a step is busy with it, never free for new work.
        free: !owes && record_quiet.is_some_and(|seconds| seconds < FREE_AFTER_SECONDS),
        agent_kind: None,
        agent_model: None,
    }
}

/// #1234: every lane that appears in the fold (as a slice's lane or its current reviewer), judged.
/// An obligation begins at the slice's claim (its author) or at the newest `task.review_assigned`
/// naming the reviewer on that PR (its current reviewer); `owes_since` is the oldest open one.
fn liveness(
    tasks: &[TaskState],
    records: &[TaskEventRecord],
    last_by_actor: &BTreeMap<String, PersistedTimestamp>,
    now: &PersistedTimestamp,
) -> Liveness {
    use graphhelm_execution::tasks::TaskEventKind;
    let open = |slice: &TaskState| {
        slice.step != graphhelm_execution::tasks::TaskStep::Merged && slice.released_by.is_none()
    };
    let assigned_at = |reviewer: &str, pr: u64| {
        records
            .iter()
            .rev()
            .find(|record| {
                record.kind == TaskEventKind::ReviewAssigned
                    && record.pr == Some(pr)
                    && record.reviewer.as_deref() == Some(reviewer)
            })
            .and_then(|record| record.occurred_at.clone())
    };
    let mut owes: BTreeMap<String, (bool, Option<PersistedTimestamp>)> = BTreeMap::new();
    let mut note = |lane: &str, owing: bool, since: Option<PersistedTimestamp>| {
        let entry = owes.entry(lane.to_owned()).or_default();
        entry.0 |= owing;
        if owing && let Some(since) = since {
            let older = entry
                .1
                .as_ref()
                .is_none_or(|current| since.as_datetime() < current.as_datetime());
            if older {
                entry.1 = Some(since);
            }
        }
    };
    // #1281: an obligation with no claim (a slice born at pr_opened) or no assignment record (a
    // reviewer named by a verdict) still began somewhere: at the oldest record that bears on it.
    let first_at = |matches: &dyn Fn(&TaskEventRecord) -> bool| {
        records
            .iter()
            .filter(|record| matches(record))
            .filter_map(|record| record.occurred_at.clone())
            .min_by_key(|at| *at.as_datetime())
    };
    for slice in tasks {
        if let Some(lane) = &slice.lane {
            let since = slice.claimed_at.clone().or_else(|| {
                first_at(&|record| {
                    record.task_id == slice.task_id || (slice.pr.is_some() && record.pr == slice.pr)
                })
            });
            note(lane, open(slice), since);
        }
        if let Some(reviewer) = &slice.current_reviewer {
            let owing = open(slice) && slice.pr.is_some();
            let since = slice.pr.and_then(|pr| {
                assigned_at(reviewer, pr)
                    .or_else(|| {
                        first_at(&|record| {
                            record.pr == Some(pr) && record.reviewer.as_deref() == Some(reviewer)
                        })
                    })
                    .or_else(|| first_at(&|record| record.pr == Some(pr)))
            });
            note(reviewer, owing, since);
        }
    }
    Liveness {
        budgets: LivenessBudgets {
            silent_after_seconds: silent_after_seconds(),
            free_after_seconds: FREE_AFTER_SECONDS,
            stall_after_seconds: STALL_SECONDS,
            retire_after_seconds: retire_after_seconds(),
        },
        retired: Vec::new(),
        flagged: Vec::new(),
        lanes: owes
            .into_iter()
            .map(|(lane, (owing, since))| {
                lane_liveness(&lane, last_by_actor.get(&lane), since.as_ref(), owing, now)
            })
            .collect(),
    }
}

/// #1202: what one Runtime remembers of an execution's `task.*` envelopes it already opened.
/// Recorded events and sealed envelopes are immutable, so an outcome keyed by sequence never
/// changes; the head's event id is kept so a store rebuilt under the same path (sequences
/// restart) is noticed and the memory dropped rather than trusted.
#[derive(Debug, Clone, Default)]
pub(crate) struct TaskRecordCache {
    /// Sequence of a `task.*` record → its verified parse, or `None` when it did not verify.
    outcomes: BTreeMap<u64, Option<TaskEventRecord>>,
    /// The last walked head: its sequence and event id.
    head: Option<(u64, String)>,
}

impl TaskRecordCache {
    /// Folds a cache that read the same execution (possibly concurrently) into this one: the
    /// union of outcomes, and the later head.
    pub(crate) fn absorb(&mut self, other: TaskRecordCache) {
        self.outcomes.extend(other.outcomes);
        let later = other.head.as_ref().map(|(sequence, _)| *sequence)
            >= self.head.as_ref().map(|(sequence, _)| *sequence);
        if later {
            self.head = other.head;
        }
    }
}

/// #1220: the one memory this process keeps of each execution's task records, shared by the tasks
/// route and claim admission so both read the same fold and neither reopens what the other
/// already verified. Keyed by the store's root AND the execution id (`cache_key`), so two stores
/// in one process never share or merge a memory (gh-claude-opt-2 on PR #1221); within one store,
/// `read_history` still checks the remembered head's event id, so a store rebuilt under the same
/// path drops the memory instead of trusting it.
fn shared_caches() -> &'static Mutex<HashMap<String, TaskRecordCache>> {
    static CACHES: OnceLock<Mutex<HashMap<String, TaskRecordCache>>> = OnceLock::new();
    CACHES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Runs `read` with this process's memory of `execution`, outside the lock (a cold read can take
/// seconds), then folds what it learned back in, a refusal included (#1202, #1220). A poisoned
/// lock reads as an empty memory: the read still runs, it only opens more.
pub(crate) fn with_shared_cache<T>(key: &str, read: impl FnOnce(&mut TaskRecordCache) -> T) -> T {
    let mut cache = shared_caches()
        .lock()
        .map(|map| map.get(key).cloned().unwrap_or_default())
        .unwrap_or_default();
    let result = read(&mut cache);
    if let Ok(mut map) = shared_caches().lock() {
        map.entry(key.to_owned()).or_default().absorb(cache);
    }
    result
}

/// The shared cache's key: the events store's root and the execution id.
pub(crate) fn cache_key(root: &Path, execution: &str) -> String {
    format!("{}\u{0}{execution}", root.display())
}

/// #1560: how many opened `task.*` envelopes one process keeps. Above it the memory starts over
/// (the next reads open again); a run of the owner's size holds about 6k.
const OPENED_ENVELOPES_CAP: usize = 65_536;

/// #1560: this process's memory of `task.*` envelopes it already opened AND authenticated, keyed
/// by keyring generation (key id and revocation epoch), store root, scope, evidence id and the
/// envelope digest the journal records. Only a successful open is remembered, so a record that
/// fails to read, to match its digest or to authenticate is refused again on every read instead
/// of being cached as a verdict; after a key rotation or a revocation the generation differs, so
/// nothing opened under the old key is served.
fn opened_envelopes() -> &'static Mutex<HashMap<String, std::sync::Arc<serde_json::Value>>> {
    static OPENED: OnceLock<Mutex<HashMap<String, std::sync::Arc<serde_json::Value>>>> =
        OnceLock::new();
    OPENED.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The memory itself: `key`'s remembered envelope, or `open`'s answer, kept only when it is one.
fn remembered_or(
    key: String,
    open: impl FnOnce() -> Option<serde_json::Value>,
) -> Option<std::sync::Arc<serde_json::Value>> {
    if let Some(hit) = opened_envelopes()
        .lock()
        .ok()
        .and_then(|map| map.get(&key).cloned())
    {
        return Some(hit);
    }
    let opened = std::sync::Arc::new(super::timing::phase("envelope.open", open)?);
    if let Ok(mut map) = opened_envelopes().lock() {
        if map.len() >= OPENED_ENVELOPES_CAP {
            map.clear();
        }
        map.insert(key, std::sync::Arc::clone(&opened));
    }
    Some(opened)
}

fn envelope_key(
    generation: &str,
    root: &Path,
    scope: &graphhelm_protocols::RepositoryScope,
    evidence_id: &str,
    envelope_sha256: &str,
) -> String {
    format!(
        "{generation}\u{0}{}\u{0}{scope:?}\u{0}{evidence_id}\u{0}{envelope_sha256}",
        root.display()
    )
}

/// #1560: the parsed envelope of the sealed record `evidence_id`, whose content digest must be
/// `envelope_sha256` (the digest the journal's event names), opened under `generation`
/// (`signal::sealer_generation`). A miss reads the sealed blob, checks the digest and runs `open`
/// (decrypt and parse); a hit skips all three. The heartbeat and release admissions and the tasks
/// read used to open every record they walk on every request.
pub(crate) fn opened_envelope(
    store: &graphhelm_events::LocalEventRepository,
    scope: &graphhelm_protocols::RepositoryScope,
    generation: &str,
    evidence_id: &EvidenceId,
    envelope_sha256: &str,
    open: impl FnOnce(&graphhelm_events::SealedEvidence) -> Option<serde_json::Value>,
) -> Option<std::sync::Arc<serde_json::Value>> {
    let key = envelope_key(
        generation,
        store.root(),
        scope,
        evidence_id.as_str(),
        envelope_sha256,
    );
    remembered_or(key, || {
        let EvidenceRead::Available(sealed) = store.sealed_evidence(scope, evidence_id).ok()?
        else {
            return None;
        };
        if sealed.reference().content_sha256().as_str() != envelope_sha256 {
            return None;
        }
        open(&sealed)
    })
}

/// #1220: the slice that holds `task_id` against `lane`: a pre-PR claim of another lane, not
/// merged, not released, and whose lease (#1193) has not lapsed, so a dead lane's claim stops
/// holding the issue the moment it is overdue, before the Stop reconciler releases it.
pub(crate) fn holder<'a>(
    tasks: &'a [TaskState],
    leases: &[ClaimLease],
    task_id: &str,
    lane: &str,
) -> Option<&'a TaskState> {
    tasks.iter().find(|slice| {
        slice.task_id == task_id
            && slice.pr.is_none()
            && slice.released_by.is_none()
            && slice.step != graphhelm_execution::tasks::TaskStep::Merged
            && slice.lane.as_deref().is_some_and(|holder| holder != lane)
            && !leases
                .iter()
                .any(|lease| lease.key == slice.key && lease.overdue)
    })
}

/// #1193: one `ClaimLease` per live slice that still holds a pre-PR claim (a slice with a PR, a
/// merge or a release is not leased), judged at `now`.
pub fn claim_leases(tasks: &[TaskState], now: &PersistedTimestamp) -> Vec<ClaimLease> {
    tasks
        .iter()
        .filter(|state| {
            state.pr.is_none()
                && state.released_by.is_none()
                && state.step != graphhelm_execution::tasks::TaskStep::Merged
        })
        .filter_map(|state| {
            let (lane, claim_sequence, claimed_at) = (
                state.lane.as_ref()?,
                state.claim_sequence?,
                state.claimed_at.as_ref()?,
            );
            let lease_seconds = state.lease_seconds.unwrap_or(1800);
            let (expires_at, overdue) = lease_state(
                claimed_at,
                state.last_heartbeat_at.as_ref(),
                lease_seconds,
                now,
            );
            Some(ClaimLease {
                key: state.key.clone(),
                task_id: state.task_id.clone(),
                lane: lane.clone(),
                claim_sequence,
                lease_seconds,
                expires_at,
                overdue,
            })
        })
        .collect()
}

/// #1193 (plan A2): the lease of one open pre-PR claim, read at `now`. `expires_at` is the later
/// of the claim and its last heartbeat plus the lease; `overdue` is `now` past it. Computed here,
/// beside the Runtime's clock, never in the pure fold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimLease {
    /// The slice key the lease belongs to (`TaskState.key`).
    pub key: String,
    pub task_id: String,
    pub lane: String,
    pub claim_sequence: u64,
    pub lease_seconds: u64,
    pub expires_at: PersistedTimestamp,
    pub overdue: bool,
}

/// The lease arithmetic, total for any instants: a heartbeat before the claim (a corrupt or
/// reordered log) cannot move expiry earlier than the claim itself, and a `now` before the claim
/// reads as not overdue. `lease_seconds` is bounded by admission (300..86400, never below the heartbeat cooldown) and the fold's
/// default; any value is accepted here.
pub fn lease_state(
    claimed_at: &PersistedTimestamp,
    last_heartbeat_at: Option<&PersistedTimestamp>,
    lease_seconds: u64,
    now: &PersistedTimestamp,
) -> (PersistedTimestamp, bool) {
    let claimed = *claimed_at.as_datetime();
    let since = last_heartbeat_at
        .map(|at| *at.as_datetime())
        .filter(|heartbeat| *heartbeat > claimed)
        .unwrap_or(claimed);
    let lease = chrono::Duration::seconds(i64::try_from(lease_seconds).unwrap_or(i64::MAX));
    // An expiry that cannot be represented (a lease past the persisted range) fails CLOSED: the
    // claim reads as not overdue and expiry is reported as `now`, never as an earlier instant
    // that would release a live claim (gh-claude-opt-1 on PR #1205).
    let Some(expires_at) = since
        .checked_add_signed(lease)
        .and_then(|expires| PersistedTimestamp::from_datetime(expires).ok())
    else {
        return (now.clone(), false);
    };
    let overdue = *now.as_datetime() > *expires_at.as_datetime();
    (expires_at, overdue)
}

/// Reads one execution's `task.*` records through the Runtime's keyring and folds them. An
/// unknown execution is `not_found`; a keyring that cannot open is the usual `/keyring` failure.
pub(crate) fn read(
    events: &Path,
    execution: &str,
    keyring: &SignalKeyring,
    budget: &graphhelm_events::ReadBudget,
) -> Result<TasksView, Failure> {
    read_cached(
        events,
        execution,
        keyring,
        budget,
        &mut TaskRecordCache::default(),
    )
}

/// The same read, remembering across calls (#1202): an envelope whose outcome `cache` holds is
/// not opened again; the ones it opens are added to `cache`, including on a budget refusal, so
/// the next read continues where this one stopped.
pub(crate) fn read_cached(
    events: &Path,
    execution: &str,
    keyring: &SignalKeyring,
    budget: &graphhelm_events::ReadBudget,
    cache: &mut TaskRecordCache,
) -> Result<TasksView, Failure> {
    // ONE budget for the whole read (the status read's, #750): the store walk, every sealed
    // envelope opened and the fold are all inside it, so `status --tasks` cannot spend a second
    // unbounded history read after the budgeted status fold (codexrev-2 on PR #1190). The checks
    // read the clock (`check_now`), since envelope opens are not events the walk counts.
    let store = super::timing::phase("store.open", || event_store(events))
        .map_err(|e| super::repository_failure(&e))?;
    let (scope, _, history) =
        super::timing::phase("stream.read", || resolve_stream(&store, Some(execution)))?;
    if history.is_empty() {
        return Err(super::not_found());
    }
    read_history(&store, &scope, &history, execution, keyring, budget, cache)
}

/// #1220: the read over a history the caller already loaded (claim admission holds the store and
/// the stream), so the route and admission fold the same records the same way.
pub(crate) fn read_history(
    store: &graphhelm_events::LocalEventRepository,
    scope: &graphhelm_protocols::RepositoryScope,
    history: &[graphhelm_protocols::EventEnvelope],
    execution: &str,
    keyring: &SignalKeyring,
    budget: &graphhelm_events::ReadBudget,
    cache: &mut TaskRecordCache,
) -> Result<TasksView, Failure> {
    let exceeded = |exceeded: graphhelm_events::ReadBudgetExceeded| {
        super::replay_failure(&graphhelm_events::ReplayError::BudgetExceeded {
            walked: exceeded.walked,
            limit_millis: exceeded.limit_millis,
        })
    };
    let walked = history.len() as u64;
    budget.check_now(walked).map_err(exceeded)?;
    // A remembered head whose event is no longer at that sequence is another store's memory.
    if let Some((sequence, event_id)) = &cache.head
        && history
            .iter()
            .find(|event| event.sequence == *sequence)
            .is_none_or(|event| event.event_id.as_str() != event_id)
    {
        cache.outcomes.clear();
        cache.head = None;
    }
    // The head is remembered before any envelope is opened: a read refused by its budget keeps
    // the outcomes it learned AND the identity they belong to, so the next read continues
    // instead of rechecking nothing (gh-claude-opt-2 on PR #1206).
    if let Some(last) = history.last() {
        cache.head = Some((last.sequence, last.event_id.as_str().to_owned()));
    }
    let opener = super::signal::open_sealer(keyring)?;
    let generation = super::signal::sealer_generation(keyring)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .map_err(|_| super::execution_state("the evidence reader could not start", "/keyring"))?;
    let mut records = Vec::new();
    let mut rejected = 0_usize;
    let mut opened = 0_usize;
    for event in history {
        let EventKind::SignalRecorded(record) = &event.kind else {
            continue;
        };
        if !record.kind.starts_with("task.") {
            continue;
        }
        if let Some(outcome) = cache.outcomes.get(&event.sequence) {
            match outcome {
                Some(parsed) => records.push(parsed.clone()),
                None => rejected += 1,
            }
            continue;
        }
        opened += 1;
        let envelope_id = format!("signal-{}", record.signal_id.as_str());
        let verified = (|| {
            // #1839: a `task.journey_frames` record also carries its sealed frames, one per screen
            // after the envelope, at the ids `signal-<id>-image-<n>`; every other kind has none.
            let frames = record.kind == "task.journey_frames";
            if event.evidence_refs.is_empty()
                || (!frames && event.evidence_refs.len() != 1)
                || event.evidence_refs[0].evidence_id().as_str() != envelope_id
            {
                return None;
            }
            let frame_ids: Vec<String> = event.evidence_refs[1..]
                .iter()
                .enumerate()
                .map(|(index, reference)| {
                    let id = reference.evidence_id().as_str();
                    (id == format!("{envelope_id}-image-{}", index + 1)).then(|| id.to_owned())
                })
                .collect::<Option<_>>()?;
            let id = EvidenceId::parse(&envelope_id).ok()?;
            let envelope = opened_envelope(
                store,
                scope,
                &generation,
                &id,
                record.envelope_sha256.as_str(),
                |sealed| {
                    let plaintext = runtime.block_on(opener.open(scope.clone(), sealed)).ok()?;
                    plaintext.expose(|bytes| serde_json::from_slice(bytes)).ok()
                },
            )?;
            let actor_id = event.actor.id().as_str();
            if envelope.get("type").and_then(serde_json::Value::as_str)
                != Some(record.kind.as_str())
                || envelope
                    .pointer("/source/id")
                    .and_then(serde_json::Value::as_str)
                    != Some(actor_id)
            {
                return None;
            }
            let description = envelope.get("description")?.as_str()?;
            parse_task_event(
                &record.kind,
                actor_id,
                description,
                event.sequence,
                Some(event.occurred_at.clone()),
            )
            .and_then(|mut parsed| {
                if let Some(run) = parsed.journey_frames.as_mut() {
                    if run.screens.len() != frame_ids.len() {
                        return None;
                    }
                    run.evidence = frame_ids;
                }
                Some(parsed)
            })
            // A release by slice key and a lane retirement are the owner's records only.
            .filter(|parsed| {
                (parsed.slice_key.is_none()
                    && parsed.kind != graphhelm_execution::tasks::TaskEventKind::LaneRetired)
                    || event.actor.actor_type() == graphhelm_protocols::PersistedActorType::Owner
            })
        })();
        cache.outcomes.insert(event.sequence, verified.clone());
        match verified {
            Some(parsed) => records.push(parsed),
            None => rejected += 1,
        }
        budget.check_now(walked).map_err(exceeded)?;
    }
    let fold = super::timing::phase("tasks.fold", || fold_task_events_with_routes(&records));
    budget.check_now(walked).map_err(exceeded)?;
    let now = PersistedTimestamp::from_datetime(chrono::Utc::now())
        .map_err(|_| super::signal_invalid("the Runtime clock is not a persisted instant", "/"))?;
    let leases = claim_leases(&fold.tasks, &now);
    let last_by_actor = last_step_by_actor(&records);
    let mut liveness = liveness(&fold.tasks, &records, &last_by_actor, &now);
    // A retired lane owes nothing and is judged neither silent nor free.
    liveness
        .lanes
        .retain(|row| !fold.retired_lanes.contains(&row.lane));
    // #1920: the Runtime retires a lane silent past the policy window with nothing open, and only
    // flags one that still owes work. Judged here, at read time: a new record restores it.
    let window = liveness.budgets.retire_after_seconds;
    let signals = lane_signals(history);
    let (auto_retired, flagged) = apply_lane_silence(&mut liveness.lanes, &signals, window, &now);
    liveness.retired = auto_retired;
    liveness.flagged = flagged;
    let mut hidden_lanes = fold.retired_lanes.clone();
    hidden_lanes.extend(liveness.retired.iter().map(|row| row.lane.clone()));
    // #1851: each lane's reported kind and model, from its newest claim that names them.
    let identities = graphhelm_execution::tasks::lane_identities(&records);
    for row in &mut liveness.lanes {
        if let Some((kind, model)) = identities.get(&row.lane) {
            row.agent_kind.clone_from(kind);
            row.agent_model.clone_from(model);
        }
    }
    let stalls = stalled_tasks(&fold.tasks, &now);
    let backlog = backlog(&records, &fold.tasks);
    let waiting = open_questions(history, &hidden_lanes, |event, record| {
        let envelope_id = format!("signal-{}", record.signal_id.as_str());
        if event.evidence_refs.len() != 1
            || event.evidence_refs[0].evidence_id().as_str() != envelope_id
        {
            return None;
        }
        let id = EvidenceId::parse(&envelope_id).ok()?;
        opened_envelope(
            store,
            scope,
            &generation,
            &id,
            record.envelope_sha256.as_str(),
            |sealed| {
                let plaintext = runtime.block_on(opener.open(scope.clone(), sealed)).ok()?;
                plaintext.expose(|bytes| serde_json::from_slice(bytes)).ok()
            },
        )
    });
    budget.check_now(walked).map_err(exceeded)?;
    Ok(TasksView {
        execution_id: execution.to_owned(),
        head_sequence: history.last().map_or(0, |event| event.sequence),
        tasks: fold.tasks,
        records,
        rejected,
        slices: fold.slices,
        routes: fold.routes,
        opened,
        now,
        leases,
        liveness,
        stalls,
        backlog,
        retired_lanes: fold.retired_lanes,
        waiting,
        dispatch_notes: dispatch_notes(history),
    })
}

/// #1794: the questions lanes asked that nothing answers yet. Only an agent's `question` signal,
/// or its `operator_note` routed to the owner, is opened (`open` reads the sealed envelope); the
/// answer side is the journal's plaintext `replyTo`, so answers cost no envelope. A question whose
/// envelope cannot be read, names another source, has no `to`, or runs past `QUESTION_MAX_CHARS`
/// is left out rather than guessed.
///
/// #1827: the history is walked newest first and each lane opens at most `WAITING_PER_LANE`
/// envelopes; older asks are only counted (`laneTotal`). A retired lane's asks are left out, as
/// its leases and liveness are. Rows are served oldest first.
fn open_questions<F>(
    history: &[graphhelm_protocols::EventEnvelope],
    retired: &[String],
    mut open: F,
) -> Vec<LaneWaiting>
where
    F: FnMut(
        &graphhelm_protocols::EventEnvelope,
        &graphhelm_protocols::SignalRecorded,
    ) -> Option<std::sync::Arc<serde_json::Value>>,
{
    let answered: std::collections::BTreeSet<&str> = history
        .iter()
        .filter_map(|event| match &event.kind {
            EventKind::SignalRecorded(record) => record.reply_to.as_ref().map(|id| id.as_str()),
            _ => None,
        })
        .collect();
    let mut totals: BTreeMap<&str, usize> = BTreeMap::new();
    let mut waiting = Vec::new();
    for event in history.iter().rev() {
        let EventKind::SignalRecorded(record) = &event.kind else {
            continue;
        };
        if event.actor.actor_type() != graphhelm_protocols::PersistedActorType::Agent {
            continue;
        }
        let asks = asks_owner(record);
        let lane = event.actor.id().as_str();
        if !asks
            || answered.contains(record.signal_id.as_str())
            || retired.iter().any(|retired| retired == lane)
        {
            continue;
        }
        let total = totals.entry(lane).or_insert(0);
        *total += 1;
        if *total > WAITING_PER_LANE {
            continue;
        }
        let Some(envelope) = open(event, record) else {
            continue;
        };
        if envelope
            .pointer("/source/id")
            .and_then(serde_json::Value::as_str)
            != Some(lane)
        {
            continue;
        }
        let (Some(to), Some(question)) = (
            envelope.get("to").and_then(serde_json::Value::as_str),
            envelope
                .get("description")
                .and_then(serde_json::Value::as_str),
        ) else {
            continue;
        };
        if question.chars().count() > QUESTION_MAX_CHARS {
            continue;
        }
        waiting.push(LaneWaiting {
            lane: lane.to_owned(),
            question: question.to_owned(),
            waits_on: to.to_owned(),
            since: event.occurred_at.clone(),
            signal_id: record.signal_id.as_str().to_owned(),
            sequence: event.sequence,
            lane_total: 0,
        });
    }
    waiting.reverse();
    for row in &mut waiting {
        row.lane_total = totals.get(row.lane.as_str()).copied().unwrap_or(0);
    }
    waiting
}

/// #1235: how long an open PR slice may go without any task record before it reads as stalled.
/// Two hours: longer than a reached test run plus a review round, far shorter than the 21 h a
/// BLOCK on #1136 and #1125 sat behind a dead fixer (2026-10-14).
pub const STALL_SECONDS: i64 = 7200;

/// #1235: what an open PR slice is waiting on, and so who owes the next step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StallReason {
    /// A BLOCK on the current head that no push answered; the author owes the fix.
    BlockUnanswered,
    /// A recorded head with no verdict; the reviewer owes it.
    AwaitingVerdict,
    /// An approved head nobody merged; the approving reviewer owes the merge.
    AwaitingMerge,
}

/// #1235: one stalled PR slice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStall {
    pub key: String,
    pub task_id: String,
    pub pr: u64,
    pub reason: StallReason,
    /// The lane that owes the next step, when the records name one.
    pub owner: Option<String>,
    /// The newest task record of the slice; idle time counts from here.
    pub idle_since: PersistedTimestamp,
    pub idle_seconds: i64,
}

/// #1235: the open PR slices (a PR, no merge, no release) idle past `STALL_SECONDS` at `now`.
/// A slice with no record time is never judged: no time, no claim of silence.
pub fn stalled_tasks(tasks: &[TaskState], now: &PersistedTimestamp) -> Vec<TaskStall> {
    use graphhelm_execution::tasks::TaskStep;
    tasks
        .iter()
        .filter(|state| state.released_by.is_none() && state.step != TaskStep::Merged)
        .filter_map(|state| {
            let pr = state.pr?;
            let idle_since = state
                .clock
                .last_at
                .as_ref()
                .or(state.clock.since.as_ref())?;
            let idle_seconds = (*now.as_datetime() - *idle_since.as_datetime()).num_seconds();
            if idle_seconds <= STALL_SECONDS {
                return None;
            }
            let (reason, owner) = match (&state.blocked_by, state.step) {
                (Some(_), _) => (StallReason::BlockUnanswered, state.lane.clone()),
                (None, TaskStep::Merge) => (
                    StallReason::AwaitingMerge,
                    state
                        .current_reviewer
                        .clone()
                        .or_else(|| state.reviewers.last().cloned()),
                ),
                (None, _) => (StallReason::AwaitingVerdict, state.current_reviewer.clone()),
            };
            Some(TaskStall {
                key: state.key.clone(),
                task_id: state.task_id.clone(),
                pr,
                reason,
                owner,
                idle_since: idle_since.clone(),
                idle_seconds,
            })
        })
        .collect()
}

#[cfg(test)]
mod lease_tests {
    use super::*;

    /// #1560: an envelope opened under one keyring generation is never served under another.
    /// Credible regression: a cache key without the key id or revocation epoch, which would keep
    /// serving a plaintext after its key was revoked or rotated. The generation is read from a
    /// real sealed keyring before and after a real revocation.
    #[test]
    fn a_revoked_or_rotated_key_is_never_served_an_envelope_opened_under_the_old_one() {
        use graphhelm_events::KeyProvider as _;
        let directory = tempfile::tempdir().unwrap();
        let provider = graphhelm_sealed_key_provider::SealedKeyProvider::create(
            directory.path(),
            "rotation-key",
            graphhelm_events::SecretBytes::new(vec![7; 32]),
        )
        .unwrap();
        let scope = graphhelm_protocols::RepositoryScope::new(
            graphhelm_protocols::WorkspaceId::parse("workspace-rotation").unwrap(),
            graphhelm_protocols::ProjectId::parse("project-rotation").unwrap(),
            None,
        );
        let root = directory.path().join("events");
        let key_for = |generation: &str| {
            envelope_key(
                generation,
                &root,
                &scope,
                "signal-rotation",
                &"c".repeat(64),
            )
        };
        let before = super::super::signal::generation_of(&provider, "rotation-key")
            .ok()
            .expect("the keyring generation reads");
        let opens = std::cell::Cell::new(0);
        let open = || {
            opens.set(opens.get() + 1);
            Some(serde_json::json!({"type": "task.claimed"}))
        };
        assert!(remembered_or(key_for(&before), open).is_some());
        assert!(remembered_or(key_for(&before), open).is_some());
        assert_eq!(opens.get(), 1, "the same generation opens once");

        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(
                provider.revoke(
                    graphhelm_events::RevokeKeyRequest::new(
                        "handle-rotation",
                        "operation-rotation",
                    )
                    .unwrap(),
                ),
            )
            .unwrap();
        let after = super::super::signal::generation_of(&provider, "rotation-key")
            .ok()
            .expect("the keyring generation reads");
        assert_ne!(after, before, "a revocation is a new generation");
        let refused = remembered_or(key_for(&after), || None);
        assert!(
            refused.is_none(),
            "nothing opened before the revocation is served"
        );
        let rotated = super::super::signal::generation_of(&provider, "rotated-key")
            .ok()
            .expect("the keyring generation reads");
        assert!(remembered_or(key_for(&rotated), || None).is_none());
    }

    /// #1556: a lane that only heartbeats is idle, not active; a recent step keeps it live.
    #[test]
    fn a_heartbeat_is_not_activity_and_a_recent_step_is() {
        let claim = record(
            "task.claimed",
            "lane-idle",
            1,
            "2026-10-13T00:00:00Z",
            r#""revision":1,"lane":"lane-idle","branch":"issue-7-work""#,
        );
        let beat = graphhelm_execution::tasks::parse_task_event(
            "task.heartbeat",
            "lane-idle",
            r#"{"schema":"graphhelm-task-event-v1","taskId":"issue-7","revision":1,"at":"2026-10-14T11:59:48Z","lane":"lane-idle"}"#,
            2,
            Some(at("2026-10-14T11:59:48Z")),
        )
        .expect("a valid heartbeat");
        let step = record(
            "task.claimed",
            "lane-busy",
            3,
            "2026-10-14T11:55:00Z",
            r#""revision":1,"lane":"lane-busy","branch":"issue-7-work""#,
        );
        let now = at("2026-10-14T12:00:00Z");
        let last = last_step_by_actor(&[claim.clone(), beat.clone(), step]);
        let idle = lane_liveness(
            "lane-idle",
            last.get("lane-idle"),
            Some(&at("2026-10-13T00:00:00Z")),
            true,
            &now,
        );
        assert!(
            idle.silent,
            "heartbeats alone must not keep it live: {idle:?}"
        );
        let busy = lane_liveness(
            "lane-busy",
            last.get("lane-busy"),
            Some(&at("2026-10-14T11:55:00Z")),
            true,
            &now,
        );
        assert!(!busy.silent, "{busy:?}");
        assert_eq!(last.get("lane-idle"), claim.occurred_at.as_ref());
    }

    /// #1808: a fixture may only shorten the silence budget; a longer, zero, negative or garbled
    /// value keeps the real one. Credible regression: honouring any parsed number. Cost: pure calls.
    #[test]
    fn a_fixture_silence_budget_only_shortens() {
        assert_eq!(super::silent_budget(Some("60")), 60);
        assert_eq!(super::silent_budget(Some(" 1800 ")), 1800);
        for raw in [None, Some("0"), Some("-5"), Some("1801"), Some("soon")] {
            assert_eq!(
                super::silent_budget(raw),
                super::SILENT_AFTER_SECONDS,
                "{raw:?}"
            );
        }
    }

    /// #1556 (orchestrator ruling): a heartbeat that says why the lane waits is a step, so the
    /// lane is not Silent; the Stop hook's reasonless heartbeat still is not (the 37 h dead claim).
    /// Credible regression: counting every heartbeat, or none. Cost: pure calls.
    #[test]
    fn a_heartbeat_with_a_reason_clears_silent_and_a_reasonless_one_does_not() {
        let claim = record(
            "task.claimed",
            "lane-w",
            1,
            "2026-10-13T00:00:00Z",
            r#""revision":1,"lane":"lane-w","branch":"issue-7-work""#,
        );
        let beat = |reason: &str| {
            graphhelm_execution::tasks::parse_task_event(
                "task.heartbeat",
                "lane-w",
                &format!(
                    r#"{{"schema":"graphhelm-task-event-v1","taskId":"issue-7","revision":1,"at":"2026-10-14T11:59:48Z","lane":"lane-w"{reason}}}"#
                ),
                2,
                Some(at("2026-10-14T11:59:48Z")),
            )
            .expect("a valid heartbeat")
        };
        let now = at("2026-10-14T12:00:00Z");
        let verdict = |records: &[TaskEventRecord]| {
            let last = last_step_by_actor(records);
            lane_liveness(
                "lane-w",
                last.get("lane-w"),
                Some(&at("2026-10-13T00:00:00Z")),
                true,
                &now,
            )
        };
        let reasoned = verdict(&[
            claim.clone(),
            beat(r#","reason":"waiting on the reached plan""#),
        ]);
        assert!(
            !reasoned.silent,
            "a stated wait is not silence: {reasoned:?}"
        );
        let reasonless = verdict(&[claim, beat("")]);
        assert!(
            reasonless.silent,
            "the hook's heartbeat stays silent: {reasonless:?}"
        );
    }

    /// #1920: the silence policy's three cases at one read: a lane silent past the window with
    /// nothing open is retired by the Runtime (citing policy and last record) and leaves the board;
    /// a silent lane that still owes is flagged and stays; the same lane after a new record is
    /// restored. Cost: pure calls.
    #[test]
    fn the_runtime_retires_a_silent_idle_lane_flags_an_owing_one_and_restores_on_a_record() {
        let now = at("2026-10-23T12:00:00Z");
        let old = at("2026-10-22T23:00:00Z"); // 13 h
        let fresh = at("2026-10-23T11:00:00Z"); // 1 h
        let window = super::RETIRE_AFTER_SECONDS;
        let mut lanes = vec![
            lane_liveness("idle", Some(&old), None, false, &now),
            lane_liveness("holder", Some(&old), Some(&old), true, &now),
            lane_liveness("live", Some(&fresh), None, false, &now),
        ];
        let (retired, flagged) =
            super::apply_lane_silence(&mut lanes, &super::LaneSignals::default(), window, &now);
        assert_eq!(retired.len(), 1, "{retired:?}");
        assert_eq!(retired[0].lane, "idle");
        assert_eq!(retired[0].by, "graphhelm");
        assert_eq!(retired[0].policy, "lane-silence-retire");
        assert_eq!(retired[0].window_seconds, 12 * 3600);
        assert_eq!(retired[0].last_record_at.as_ref(), Some(&old));
        assert_eq!(flagged.len(), 1, "{flagged:?}");
        assert_eq!(flagged[0].lane, "holder");
        let left: Vec<&str> = lanes.iter().map(|row| row.lane.as_str()).collect();
        assert_eq!(
            left,
            ["holder", "live"],
            "the retired lane leaves the board"
        );
        // Restored: the idle lane records again, and the next read keeps it.
        let mut back = vec![lane_liveness("idle", Some(&fresh), None, false, &now)];
        let (retired, flagged) =
            super::apply_lane_silence(&mut back, &super::LaneSignals::default(), window, &now);
        assert!(retired.is_empty() && flagged.is_empty());
        assert_eq!(back.len(), 1);
        // The window edge is exact, and the policy window only takes a positive number.
        let edge = at("2026-10-23T00:00:00Z");
        let mut at_edge = vec![lane_liveness("e", Some(&edge), None, false, &now)];
        assert_eq!(
            super::apply_lane_silence(&mut at_edge, &super::LaneSignals::default(), window, &now)
                .0
                .len(),
            1
        );
        assert_eq!(super::retire_budget(Some("3600")), 3600);
        assert_eq!(super::retire_budget(Some("0")), window);
        assert_eq!(super::retire_budget(Some("x")), window);
        assert_eq!(super::retire_budget(None), window);
    }

    /// #1234: the verdict at the budget edges. Silent needs an obligation; free needs a record;
    /// both edges are exact (silent AT the budget, free strictly inside it). Cost: pure calls.
    #[test]
    fn lane_liveness_judges_silence_and_freedom_at_the_declared_edges() {
        let now = at("2026-10-14T12:00:00Z");
        let fresh = at("2026-10-14T11:50:00Z"); // 10 min
        let edge_free = at("2026-10-14T11:45:00Z"); // 15 min
        let edge_silent = at("2026-10-14T11:30:00Z"); // 30 min
        let quiet = lane_liveness("a", Some(&fresh), None, false, &now);
        assert!(quiet.free && !quiet.silent, "{quiet:?}");
        // #1281 (seen live on a84c52a7): owing and free at once. A lane that owes is never free.
        let owing = lane_liveness("a", Some(&fresh), Some(&fresh), true, &now);
        assert!(!owing.free && !owing.silent, "owes, so not free: {owing:?}");
        let at_free = lane_liveness("a", Some(&edge_free), None, false, &now);
        assert!(
            !at_free.free && !at_free.silent,
            "free is strictly inside 15 min: {at_free:?}"
        );
        let at_silent = lane_liveness("a", Some(&edge_silent), None, true, &now);
        assert!(at_silent.silent, "silent from 30 min on: {at_silent:?}");
        let idle = lane_liveness("a", Some(&edge_silent), None, false, &now);
        assert!(!idle.silent, "no obligation, no silence: {idle:?}");
        let never = lane_liveness("a", None, None, true, &now);
        assert!(never.silent && !never.free, "no record at all: {never:?}");
        // gh-claude-opt-2 on PR #1249: the obligation's start counts. A reviewer assigned a minute
        // ago whose own last record is two hours old is not silent, and not free either.
        let two_hours = at("2026-10-14T10:00:00Z");
        let a_minute_ago = at("2026-10-14T11:59:00Z");
        let assigned = lane_liveness("r", Some(&two_hours), Some(&a_minute_ago), true, &now);
        assert!(
            !assigned.silent && !assigned.free,
            "assigned 1 min ago: {assigned:?}"
        );
        // Assigned 31 minutes ago and never recorded anything: silent.
        let thirty_one = at("2026-10-14T11:29:00Z");
        let unanswered = lane_liveness("r", None, Some(&thirty_one), true, &now);
        assert!(
            unanswered.silent && !unanswered.free,
            "assigned 31 min ago, no record: {unanswered:?}"
        );
    }

    /// #1220 (orchestration session): a claim whose lease has lapsed no longer holds its task, even
    /// before the Stop reconciler releases it, so a dead lane cannot block an issue until someone's
    /// next Stop. Also: a live claim of another lane holds, the lane's own claim never holds
    /// against itself. Cost: one fold of one record.
    #[test]
    fn an_overdue_claim_stops_holding_its_task() {
        let claim = graphhelm_execution::tasks::parse_task_event(
            "task.claimed",
            "lane-alpha",
            r#"{"schema":"graphhelm-task-event-v1","taskId":"issue-7","revision":1,"at":"2026-10-14T03:00:00Z","issue":7,"lane":"lane-alpha","branch":"issue-7-work"}"#,
            1,
            Some(at("2026-10-14T03:00:00Z")),
        )
        .expect("a valid claim");
        let tasks = graphhelm_execution::tasks::fold_task_events(&[claim]);
        let live = claim_leases(&tasks, &at("2026-10-14T03:10:00Z"));
        assert!(
            holder(&tasks, &live, "issue-7", "lane-beta").is_some(),
            "a live claim holds"
        );
        assert!(
            holder(&tasks, &live, "issue-7", "lane-alpha").is_none(),
            "never against itself"
        );
        let lapsed = claim_leases(&tasks, &at("2026-10-14T03:30:01Z"));
        assert!(
            lapsed.iter().all(|lease| lease.overdue),
            "1800 s default lapsed: {lapsed:?}"
        );
        assert!(
            holder(&tasks, &lapsed, "issue-7", "lane-beta").is_none(),
            "an overdue claim does not hold"
        );
    }

    fn at(value: &str) -> PersistedTimestamp {
        PersistedTimestamp::parse(value).unwrap()
    }

    /// #1193: the plan's cell. A claim at T with no heartbeat is overdue at T+lease+1s and not at
    /// T+lease-1s; a heartbeat at T+lease-1s moves expiry to that instant plus the lease.
    #[test]
    fn a_claim_goes_overdue_one_second_past_its_lease_and_a_heartbeat_moves_expiry() {
        let claimed = at("2026-10-14T03:00:00Z");
        let (expires, overdue) = lease_state(&claimed, None, 1800, &at("2026-10-14T03:29:59Z"));
        assert_eq!(expires, at("2026-10-14T03:30:00Z"));
        assert!(!overdue, "one second inside the lease");
        let (_, overdue) = lease_state(&claimed, None, 1800, &at("2026-10-14T03:30:01Z"));
        assert!(overdue, "one second past the lease");
        let heartbeat = at("2026-10-14T03:29:59Z");
        let (expires, overdue) = lease_state(
            &claimed,
            Some(&heartbeat),
            1800,
            &at("2026-10-14T03:30:01Z"),
        );
        assert_eq!(expires, at("2026-10-14T03:59:59Z"));
        assert!(!overdue, "the heartbeat moved expiry");
        // Exactly at expiry is not yet overdue; a heartbeat older than the claim counts for nothing.
        let (_, overdue) = lease_state(&claimed, None, 1800, &at("2026-10-14T03:30:00Z"));
        assert!(!overdue);
        let stale = at("2026-10-14T02:00:00Z");
        let (expires, _) = lease_state(&claimed, Some(&stale), 60, &at("2026-10-14T03:00:30Z"));
        assert_eq!(expires, at("2026-10-14T03:01:00Z"));
    }

    /// #1281 (seen live on a84c52a7): a lane owed with no `owesSince`. A slice born at
    /// `pr_opened` has no claim time and a reviewer named only by a verdict has no assignment
    /// record; each obligation still began at the oldest record bearing on it. Cost: one fold.
    #[test]
    fn every_owing_lane_has_an_owes_since() {
        let head = "a".repeat(40);
        let opened = record(
            "task.pr_opened",
            "lane-alpha",
            1,
            "2026-10-14T00:10:00Z",
            &format!(
                r#""revision":2,"lane":"lane-alpha","branch":"issue-7-work","pr":9,"headSha":"{head}","journeys":[]"#
            ),
        );
        let approved = record(
            "task.review_verdict",
            "lane-beta",
            2,
            "2026-10-14T01:00:00Z",
            &format!(
                r#""revision":4,"pr":9,"headSha":"{head}","reviewer":"lane-beta","verdict":"APPROVE","commentUrl":"https://github.com/o/r/pull/9#c1""#
            ),
        );
        let records = vec![opened, approved];
        let tasks = graphhelm_execution::tasks::fold_task_events(&records);
        let view = liveness(
            &tasks,
            &records,
            &BTreeMap::new(),
            &at("2026-10-14T02:00:00Z"),
        );
        let owing: Vec<&LaneLiveness> = view.lanes.iter().filter(|row| row.owes).collect();
        assert!(!owing.is_empty(), "HARNESS-BROKE: nobody owes: {view:?}");
        for row in owing {
            assert!(row.owes_since.is_some(), "owes with no owesSince: {row:?}");
            assert!(!row.free, "owes, so not free: {row:?}");
        }
    }

    fn record(kind: &str, actor: &str, sequence: u64, time: &str, body: &str) -> TaskEventRecord {
        let document = format!(
            r#"{{"schema":"graphhelm-task-event-v1","taskId":"issue-7","at":"{time}","issue":7,{body}}}"#
        );
        graphhelm_execution::tasks::parse_task_event(
            kind,
            actor,
            &document,
            sequence,
            Some(at(time)),
        )
        .unwrap_or_else(|| panic!("a valid {kind}: {document}"))
    }

    /// #1235: the #1136 shape. A BLOCK that no push answers reads as stalled on the author two
    /// hours later and not before; a fix push clears it; an approved head nobody merges reads as
    /// owed by the reviewer; a merged slice is never stalled. Cost: one fold of five records.
    #[test]
    fn an_unanswered_block_stalls_on_its_author_and_a_push_clears_it() {
        let head = "a".repeat(40);
        let fix = "b".repeat(40);
        let claimed = record(
            "task.claimed",
            "lane-alpha",
            1,
            "2026-10-14T00:00:00Z",
            r#""revision":1,"lane":"lane-alpha","branch":"issue-7-work""#,
        );
        let opened = record(
            "task.pr_opened",
            "lane-alpha",
            2,
            "2026-10-14T00:10:00Z",
            &format!(
                r#""revision":2,"lane":"lane-alpha","branch":"issue-7-work","pr":9,"headSha":"{head}","journeys":[]"#
            ),
        );
        let blocked = record(
            "task.review_verdict",
            "lane-beta",
            3,
            "2026-10-14T01:00:00Z",
            &format!(
                r#""revision":4,"pr":9,"headSha":"{head}","reviewer":"lane-beta","verdict":"BLOCK","commentUrl":"https://github.com/o/r/pull/9#c1""#
            ),
        );
        let base = vec![claimed, opened, blocked];
        let tasks = graphhelm_execution::tasks::fold_task_events(&base);
        assert!(
            stalled_tasks(&tasks, &at("2026-10-14T03:00:00Z")).is_empty(),
            "exactly two hours is not yet stalled"
        );
        let stalls = stalled_tasks(&tasks, &at("2026-10-14T22:00:00Z"));
        assert_eq!(stalls.len(), 1, "{stalls:?}");
        assert_eq!(stalls[0].reason, StallReason::BlockUnanswered);
        assert_eq!(stalls[0].owner.as_deref(), Some("lane-alpha"));
        assert_eq!(stalls[0].pr, 9);
        assert_eq!(stalls[0].idle_seconds, 21 * 3600);

        let mut fixed = base.clone();
        fixed.push(record("task.pr_opened", "lane-alpha", 4, "2026-10-14T21:00:00Z",
            &format!(r#""revision":2,"lane":"lane-alpha","branch":"issue-7-work","pr":9,"headSha":"{fix}","journeys":[]"#)));
        let tasks = graphhelm_execution::tasks::fold_task_events(&fixed);
        assert!(
            stalled_tasks(&tasks, &at("2026-10-14T22:00:00Z")).is_empty(),
            "the push answered the BLOCK"
        );

        // A PR closed without a merge (`task.closed`, which the Stop reconciler records for a PR
        // GitHub reports closed) ends its slice, so it is never stalled.
        let mut abandoned = base.clone();
        abandoned.push(record(
            "task.closed",
            "lane-alpha",
            4,
            "2026-10-14T02:00:00Z",
            r#""revision":6,"pr":9,"reason":"abandoned","by":9"#,
        ));
        let tasks = graphhelm_execution::tasks::fold_task_events(&abandoned);
        assert!(
            stalled_tasks(&tasks, &at("2026-10-15T00:00:00Z")).is_empty(),
            "a closed PR is not stalled"
        );

        let mut approved = fixed.clone();
        approved.push(record("task.review_verdict", "lane-beta", 5, "2026-10-14T21:30:00Z",
            &format!(r#""revision":4,"pr":9,"headSha":"{fix}","reviewer":"lane-beta","verdict":"APPROVE","commentUrl":"https://github.com/o/r/pull/9#c2""#)));
        let tasks = graphhelm_execution::tasks::fold_task_events(&approved);
        let stalls = stalled_tasks(&tasks, &at("2026-10-15T00:00:00Z"));
        assert_eq!(stalls.len(), 1, "{stalls:?}");
        assert_eq!(stalls[0].reason, StallReason::AwaitingMerge);
        assert_eq!(stalls[0].owner.as_deref(), Some("lane-beta"));

        let mut merged = approved;
        merged.push(record(
            "task.merged",
            "lane-beta",
            6,
            "2026-10-14T21:40:00Z",
            &format!(
                r#""revision":5,"merger":"lane-beta","pr":9,"mergeSha":"{}","closes":[7]"#,
                "c".repeat(40)
            ),
        ));
        let tasks = graphhelm_execution::tasks::fold_task_events(&merged);
        assert!(
            stalled_tasks(&tasks, &at("2026-10-16T00:00:00Z")).is_empty(),
            "a merged slice is done"
        );
    }
}

#[cfg(test)]
mod waiting_tests {
    use super::*;
    use graphhelm_protocols::{
        ActorId, EventEnvelope, EventHash, NewEvent, OpaqueId, PersistedActor, PersistedActorType,
        RawSha256, RepositoryScope, Sensitivity, SignalRecorded, SignalSeverity, SignalSourceKind,
    };

    fn ask(sequence: u64, lane: &str, reply_to: Option<&str>) -> EventEnvelope {
        signal(sequence, lane, reply_to, "question", "2026-10-21T00:00:00Z")
    }

    fn signal(
        sequence: u64,
        lane: &str,
        reply_to: Option<&str>,
        kind: &str,
        occurred_at: &str,
    ) -> EventEnvelope {
        let hash = EventHash::parse(format!("sha256:{}", "0".repeat(64))).unwrap();
        EventEnvelope::new(
            OpaqueId::parse(format!("event-{sequence}")).unwrap(),
            RepositoryScope::new(
                graphhelm_protocols::WorkspaceId::parse("workspace-waiting").unwrap(),
                graphhelm_protocols::ProjectId::parse("project-waiting").unwrap(),
                None,
            ),
            OpaqueId::parse("run-waiting").unwrap(),
            sequence,
            PersistedTimestamp::parse(occurred_at).unwrap(),
            NewEvent::new(
                OpaqueId::parse(format!("key-{sequence}")).unwrap(),
                PersistedActor::new(PersistedActorType::Agent, ActorId::parse(lane).unwrap()),
                Sensitivity::Internal,
                EventKind::SignalRecorded(SignalRecorded {
                    to: Some(OpaqueId::parse(OWNER_ADDRESS).unwrap()),
                    reply_to: reply_to.map(|id| OpaqueId::parse(id).unwrap()),
                    scoped_agent_authenticated: None,
                    execution_id: OpaqueId::parse("run-waiting").unwrap(),
                    signal_id: OpaqueId::parse(format!("signal-{sequence}")).unwrap(),
                    source_kind: SignalSourceKind::Node,
                    source_id: OpaqueId::parse(lane).unwrap(),
                    kind: kind.to_owned(),
                    severity: SignalSeverity::Medium,
                    envelope_sha256: RawSha256::parse("a".repeat(64)).unwrap(),
                }),
                vec![],
                vec![],
            ),
            hash.clone(),
            hash,
        )
    }

    /// `asks` unanswered questions per lane for `lanes` lanes, interleaved; returns the rows and
    /// how many envelopes the read opened.
    fn read(lanes: usize, asks: usize, retired: &[String]) -> (Vec<LaneWaiting>, usize) {
        let mut history = Vec::new();
        for round in 0..asks {
            for lane in 0..lanes {
                let sequence = (round * lanes + lane + 1) as u64;
                history.push(ask(sequence, &format!("lane-{lane}"), None));
            }
        }
        let mut opened = 0;
        let rows = open_questions(&history, retired, |event, _| {
            opened += 1;
            Some(std::sync::Arc::new(serde_json::json!({
                "source": {"id": event.actor.id().as_str()},
                "to": OWNER_ADDRESS,
                "description": format!("question {}", event.sequence),
            })))
        });
        (rows, opened)
    }

    /// #1827: a lane with more unanswered asks than `WAITING_PER_LANE` serves only its newest,
    /// oldest first, and reports the rest in `laneTotal`. Credible regression: the unbounded list
    /// #1822 shipped, where every old note to the owner stays on the board forever.
    #[test]
    fn each_lane_serves_its_newest_questions_and_counts_the_rest() {
        let (rows, _) = read(2, WAITING_PER_LANE + 3, &[]);
        for lane in ["lane-0", "lane-1"] {
            let mine: Vec<&LaneWaiting> = rows.iter().filter(|row| row.lane == lane).collect();
            assert_eq!(mine.len(), WAITING_PER_LANE, "{lane}");
            assert!(
                mine.iter()
                    .all(|row| row.lane_total == WAITING_PER_LANE + 3)
            );
            // The newest asks are rounds 3.. of 0..8; the oldest three are dropped.
            let first_kept = (3 * 2 + 1 + usize::from(lane == "lane-1")) as u64;
            assert_eq!(mine[0].sequence, first_kept, "{lane}");
        }
        let sequences: Vec<u64> = rows.iter().map(|row| row.sequence).collect();
        let mut sorted = sequences.clone();
        sorted.sort_unstable();
        assert_eq!(sequences, sorted, "rows are served oldest first");
        let (rows, _) = read(2, 3, &["lane-1".to_owned()]);
        assert!(
            rows.iter().all(|row| row.lane == "lane-0"),
            "a retired lane is left out"
        );
        assert_eq!(rows.len(), 3);
    }

    /// #1935: a lane silent past the window with nothing open but an unanswered question to the
    /// owner is flagged, not retired, and its question stays in `waiting`; a lane whose task
    /// records are old but which signalled inside the window is kept; a `task.heartbeat` signal
    /// is not activity here, and an answered question no longer protects its lane. Credible
    /// regression: #1930 counting only task records, which retired a waiting lane and hid its
    /// question. Cost: pure calls.
    #[test]
    fn a_lane_with_an_open_question_is_flagged_and_a_signalling_lane_is_kept() {
        let now = PersistedTimestamp::parse("2026-10-23T12:00:00Z").unwrap();
        let old = PersistedTimestamp::parse("2026-10-20T00:00:00Z").unwrap();
        let window = RETIRE_AFTER_SECONDS;
        let history = vec![
            ask(1, "asker", None),
            signal(2, "talker", None, "progress", "2026-10-23T11:00:00Z"),
            signal(3, "beater", None, "task.heartbeat", "2026-10-23T11:00:00Z"),
            ask(4, "answered", None),
            signal(
                5,
                "owner",
                Some("signal-4"),
                "reply",
                "2026-10-21T01:00:00Z",
            ),
        ];
        let signals = lane_signals(&history);
        let mut lanes: Vec<LaneLiveness> = ["answered", "asker", "beater", "talker"]
            .into_iter()
            .map(|lane| lane_liveness(lane, Some(&old), None, false, &now))
            .collect();
        let (retired, flagged) = apply_lane_silence(&mut lanes, &signals, window, &now);
        let names =
            |rows: &[LaneSilence]| -> Vec<String> { rows.iter().map(|r| r.lane.clone()).collect() };
        assert_eq!(names(&flagged), ["asker"], "{flagged:?}");
        assert_eq!(names(&retired), ["answered", "beater"], "{retired:?}");
        let left: Vec<&str> = lanes.iter().map(|row| row.lane.as_str()).collect();
        assert_eq!(left, ["asker", "talker"]);
        let rows = open_questions(&history, &names(&retired), |event, _| {
            Some(std::sync::Arc::new(serde_json::json!({
                "source": {"id": event.actor.id().as_str()},
                "to": OWNER_ADDRESS,
                "description": "may I merge?",
            })))
        });
        let lanes_waiting: Vec<&str> = rows.iter().map(|row| row.lane.as_str()).collect();
        assert_eq!(lanes_waiting, ["asker"], "the question stays in waiting");
    }

    /// #1827 (`keel.bigo.scale`): at N and 10N unanswered asks per lane the read opens the same
    /// `lanes * WAITING_PER_LANE` envelopes and serves the same rows; only `laneTotal` grows.
    /// Counts are the oracle, never wall time.
    #[test]
    fn opened_envelopes_stay_flat_from_n_to_ten_n_asks() {
        let lanes = 3;
        for asks in [20, 200] {
            let (rows, opened) = read(lanes, asks, &[]);
            eprintln!(
                "waiting asks_per_lane={asks} opened={opened} rows={}",
                rows.len()
            );
            assert_eq!(opened, lanes * WAITING_PER_LANE, "asks={asks}");
            assert_eq!(rows.len(), lanes * WAITING_PER_LANE, "asks={asks}");
            assert!(rows.iter().all(|row| row.lane_total == asks));
        }
    }
}
