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
    FoldSlice, Route, TaskEventRecord, TaskState, fold_task_events_with_routes, parse_task_event,
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
}

/// #1234: a lane with an open obligation is `silent` once it has recorded nothing of any kind for
/// this long. Was the Studio's `LIVENESS_MS` (`lane-bars.ts`); declared here, once.
pub const SILENT_AFTER_SECONDS: i64 = 30 * 60;
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Liveness {
    pub budgets: LivenessBudgets,
    pub lanes: Vec<LaneLiveness>,
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
        silent: owes && quiet.is_none_or(|seconds| seconds >= SILENT_AFTER_SECONDS),
        free: record_quiet.is_some_and(|seconds| seconds < FREE_AFTER_SECONDS),
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
    for slice in tasks {
        if let Some(lane) = &slice.lane {
            note(lane, open(slice), slice.claimed_at.clone());
        }
        if let Some(reviewer) = &slice.current_reviewer {
            let owing = open(slice) && slice.pr.is_some();
            let since = slice.pr.and_then(|pr| assigned_at(reviewer, pr));
            note(reviewer, owing, since);
        }
    }
    Liveness {
        budgets: LivenessBudgets {
            silent_after_seconds: SILENT_AFTER_SECONDS,
            free_after_seconds: FREE_AFTER_SECONDS,
            stall_after_seconds: STALL_SECONDS,
        },
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
    let store = event_store(events).map_err(|e| super::repository_failure(&e))?;
    let (scope, _, history) = resolve_stream(&store, Some(execution))?;
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
            if event.evidence_refs.len() != 1
                || event.evidence_refs[0].evidence_id().as_str() != envelope_id
            {
                return None;
            }
            let id = EvidenceId::parse(&envelope_id).ok()?;
            let EvidenceRead::Available(sealed) = store.sealed_evidence(scope, &id).ok()? else {
                return None;
            };
            if sealed.reference().content_sha256().as_str() != record.envelope_sha256.as_str() {
                return None;
            }
            let plaintext = runtime.block_on(opener.open(scope.clone(), &sealed)).ok()?;
            let envelope: serde_json::Value = plaintext
                .expose(|bytes| serde_json::from_slice(bytes))
                .ok()?;
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
        })();
        cache.outcomes.insert(event.sequence, verified.clone());
        match verified {
            Some(parsed) => records.push(parsed),
            None => rejected += 1,
        }
        budget.check_now(walked).map_err(exceeded)?;
    }
    let fold = fold_task_events_with_routes(&records);
    budget.check_now(walked).map_err(exceeded)?;
    let now = PersistedTimestamp::from_datetime(chrono::Utc::now())
        .map_err(|_| super::signal_invalid("the Runtime clock is not a persisted instant", "/"))?;
    let leases = claim_leases(&fold.tasks, &now);
    let mut last_by_actor: BTreeMap<String, PersistedTimestamp> = BTreeMap::new();
    for event in history {
        last_by_actor.insert(
            event.actor.id().as_str().to_owned(),
            event.occurred_at.clone(),
        );
    }
    let liveness = liveness(&fold.tasks, &records, &last_by_actor, &now);
    let stalls = stalled_tasks(&fold.tasks, &now);
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
    })
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

    /// #1234: the verdict at the budget edges. Silent needs an obligation; free needs a record;
    /// both edges are exact (silent AT the budget, free strictly inside it). Cost: pure calls.
    #[test]
    fn lane_liveness_judges_silence_and_freedom_at_the_declared_edges() {
        let now = at("2026-10-14T12:00:00Z");
        let fresh = at("2026-10-14T11:50:00Z"); // 10 min
        let edge_free = at("2026-10-14T11:45:00Z"); // 15 min
        let edge_silent = at("2026-10-14T11:30:00Z"); // 30 min
        let quiet = lane_liveness("a", Some(&fresh), None, true, &now);
        assert!(quiet.free && !quiet.silent, "{quiet:?}");
        let at_free = lane_liveness("a", Some(&edge_free), None, true, &now);
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
