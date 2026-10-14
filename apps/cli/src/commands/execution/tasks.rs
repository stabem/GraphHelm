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
    })
}

#[cfg(test)]
mod lease_tests {
    use super::*;

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
}
