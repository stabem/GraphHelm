use std::path::Path;

use super::{Failure, finish, render, replay_failure, repository_failure, resolve_stream};
use crate::commands::event_store;
use crate::output::Outcome;

const COMMAND: &str = "execution.status";

/// `execution status`: replay only, no appends. The output is the operator's triage view — the
/// same `render` every mutating command replies with, including the untriaged-interruption list,
/// plus `headSequence` (Milestone 05a Task 2): the raw stream's last sequence, or 0 for an empty
/// stream. The Public Runtime API's `GET /v1/executions/{id}` calls this exact function (see
/// `commands::serve::routes::status`), so the CLI and the API report byte-identical `data` for the
/// same stream — "one store, one truth" — and the API's `If-Match` workflow (a later task) reads
/// `headSequence` as the version to race against.
///
/// Widened from private to `pub(crate)`: the one cross-module visibility widening this task needs
/// for status, so the server can call the exact same code path the CLI does rather than a second
/// implementation of "replay, then render."
pub(crate) fn execute(
    events: &Path,
    execution: Option<&str>,
) -> Result<serde_json::Value, Failure> {
    execute_within(events, execution, graphhelm_events::ReadBudget::unbounded())
}

/// [`execute`] under the wall-clock budget an OPERATOR-FACING read declares (#750).
///
/// **Why this is a second entry point rather than a change to `execute`.** One function renders
/// three different things: the answer to "what is this run doing", the reply a mutation returns
/// AFTER it has already committed (`serve::reply_with_status_value`), and each poll of the
/// immediate pause's wait loop (`serve::routes::pause`). The budget belongs to the first and to
/// neither of the others. Bounding the second would let a slow read turn a mutation that DID
/// commit into a refusal - the exact ambiguity the evidence contract exists to remove. Bounding
/// the third would make a large store's immediate pause report `unknown` forever while the
/// pause itself worked. So the read an operator is waiting on declares a budget, and the
/// renders that merely describe work already done do not.
pub(crate) fn budgeted(
    events: &Path,
    execution: Option<&str>,
) -> Result<serde_json::Value, Failure> {
    execute_within(events, execution, crate::commands::status_read_budget())
}

/// One operator-facing read: the folded projection, the raw history it was folded from, and the
/// attention inputs MEASURED from that history. `status` renders it; `briefing` (#1063) briefs
/// from it. One measurement, two renders, so the two surfaces can never disagree about what is
/// pending.
pub(crate) struct Read {
    pub(crate) projection: graphhelm_events::ExecutionProjection,
    pub(crate) history: Vec<graphhelm_protocols::EventEnvelope>,
    pub(crate) inputs: graphhelm_execution::AttentionInputs,
    /// `at_sequence(&history)`: `None` for an empty history, never `Some(0)`.
    pub(crate) at_sequence: Option<u64>,
}

/// The shared body, with the budget supplied rather than read from the wall clock.
///
/// The seam exists so the bound can be PROVEN. `budgeted` reads the system clock, and a cell
/// that had to wait out a real five-second budget would be five seconds of every gate run, for
/// a fact a supplied clock states exactly.
pub(crate) fn execute_within(
    events: &Path,
    execution: Option<&str>,
    budget: graphhelm_events::ReadBudget,
) -> Result<serde_json::Value, Failure> {
    let read = read_within(events, execution, budget)?;
    let mut value = render(
        &read.projection,
        &read.inputs,
        &super::Liveness::measured(&read.history),
    );
    // The WIRE field keeps its existing shape on purpose, zero and all: `headSequence` is a
    // different contract from `at_sequence`, read by clients that already treat 0 as "nothing
    // yet", and widening it to null is a wire change that needs its own justification. The
    // flattening is left here DECLARED rather than silently carried into the seam.
    value["headSequence"] = serde_json::json!(read.at_sequence.unwrap_or(0));
    Ok(value)
}

/// The read half of [`execute_within`], shared with `briefing::execute_within`.
pub(crate) fn read_within(
    events: &Path,
    execution: Option<&str>,
    budget: graphhelm_events::ReadBudget,
) -> Result<Read, Failure> {
    // ONE budget for the whole read. Both halves are linear in the history - the journal
    // verification `open` performs, and then the fold - so both are checked against this same
    // deadline. Bounding only the fold would cover 42% of the measured cost while reading as a
    // boundary; two budgets started separately would let one read spend the budget twice.
    let store = crate::commands::budgeted_event_store(events, budget.clone())
        .map_err(|error| repository_failure(&error))?;
    // Not `load_projection`: that helper only returns the folded projection, and `headSequence`
    // needs the raw history's last sequence too. Inlining `load_projection`'s own two-line body
    // here (resolve, then replay) avoids widening `load_projection` itself — it is also called by
    // `cancel.rs` and `pause.rs`, outside this task's file set, and changing its return shape would
    // have forced edits there.
    let (scope, stream, history) = resolve_stream(&store, execution)?;
    let projection = graphhelm_events::replay_within(&scope, &stream, &history, &budget)
        .map_err(|error| replay_failure(&error))?;
    // DO NOT INLINE THIS BACK to `Some(history.last().map_or(0, …))`. The guard for it lives in
    // `mod.rs`'s tests and calls the helper directly, so it CANNOT SEE THIS LINE: inlining the old
    // spelling here leaves every test green. What protects this call site is this comment, and
    // nothing else.
    let at_sequence = super::at_sequence(&history);
    // The read surface is the one that CAN measure: it is holding the history. It supplies
    // the subtraction; the budget stays empty until a surface can see the manifest that
    // declares it, and an unbudgeted node comes back as unevaluated rather than as calm.
    let inputs = graphhelm_execution::AttentionInputs {
        node_silence_seconds: super::node_silence_seconds(&history, chrono::Utc::now()),
        // The budgets the operator declared, now that persistence carries them. Before this
        // they never reached the seam at all, so every node in flight came back unevaluated
        // and the verdict was PERMANENTLY unknown -- honest, and useless.
        silence_budget_seconds: graphhelm_execution::effective_budgets(&projection),
        // Where this read was looking, so a remedy can be placed in the history later. `None`
        // when there was nothing to look at: an empty history has no vantage point, and
        // `Some(0)` would claim one at a sequence streams never issue.
        at_sequence,
    };
    Ok(Read {
        projection,
        history,
        inputs,
        at_sequence,
    })
}

pub fn run(events: &Path, execution: Option<&str>, html: Option<&Path>) -> Outcome {
    if let Some(html) = html
        && let Err(failure) = write_snapshot(events, execution, html)
    {
        return finish::<serde_json::Value>(COMMAND, Err(failure), |value| value);
    }
    // The operator is waiting on this one, so it is the call that declares the budget (#750).
    finish(COMMAND, budgeted(events, execution), |value| value)
}

/// `--html`: the monitor page as a frozen incident snapshot — the SAME `render_snapshot`
/// the serve layer's live page is built from (05f Task 5), written before the envelope so
/// a write failure is the command's failure, not a silent skip.
fn write_snapshot(events: &Path, execution: Option<&str>, html: &Path) -> Result<(), Failure> {
    let store = event_store(events).map_err(|error| repository_failure(&error))?;
    let (scope, stream, history) = resolve_stream(&store, execution)?;
    let projection = graphhelm_events::replay(&scope, &stream, &history)
        .map_err(|error| replay_failure(&error))?;
    let page = crate::commands::serve::monitor::render_snapshot(
        &projection,
        &history,
        chrono::Utc::now(),
        events,
    );
    std::fs::write(html, page)
        .map_err(|_| super::argument("--html does not name a writable file path", "/html"))
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };

    use chrono::{TimeZone, Utc};
    use graphhelm_events::{LocalEventRepository, PreparedAppend, ReadBudget};
    use graphhelm_protocols::{
        ActorId, Clock, EventKind, ExecutionId, ExecutionMode, ExecutionStarted, IdGenerator,
        NewEvent, OpaqueId, PersistedActor, PersistedActorType, ProjectId, RawSha256,
        RepositoryScope, Sensitivity, SignalRecorded, SignalSeverity, SignalSourceKind, WireHash,
        WorkspaceId,
    };

    struct FrozenClock;
    impl Clock for FrozenClock {
        fn now(&self) -> chrono::DateTime<Utc> {
            Utc.with_ymd_and_hms(2026, 9, 3, 12, 0, 0).unwrap()
        }
    }
    /// Start instant once - the read `starting_now` spends on the deadline - then an hour on.
    struct LapsingClock(AtomicU64);
    impl Clock for LapsingClock {
        fn now(&self) -> chrono::DateTime<Utc> {
            let base = Utc.with_ymd_and_hms(2026, 9, 3, 12, 0, 0).unwrap();
            if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
                base
            } else {
                base + chrono::Duration::hours(1)
            }
        }
    }
    #[derive(Default)]
    struct Ids(AtomicU64);
    impl IdGenerator for Ids {
        fn next_id(&self, prefix: &'static str) -> String {
            format!("{prefix}-{}", self.0.fetch_add(1, Ordering::SeqCst) + 1)
        }
    }

    fn seed(directory: &std::path::Path, count: u64) {
        let scope = RepositoryScope::new(
            WorkspaceId::parse(super::super::WORKSPACE).unwrap(),
            ProjectId::parse(super::super::PROJECT).unwrap(),
            Some(ExecutionId::parse("execution-budget").unwrap()),
        );
        let actor = PersistedActor::new(
            PersistedActorType::System,
            ActorId::parse("system-test").unwrap(),
        );
        let store =
            LocalEventRepository::open(directory, Arc::new(FrozenClock), Arc::new(Ids::default()))
                .unwrap();
        let mut next = 1_u64;
        while next <= count {
            let size = 50.min(count - next + 1);
            let mut events = Vec::new();
            if next == 1 {
                events.push(NewEvent::new(
                    OpaqueId::parse("key-root").unwrap(),
                    actor.clone(),
                    Sensitivity::Internal,
                    EventKind::ExecutionStarted(ExecutionStarted {
                        execution_id: OpaqueId::parse("execution-budget").unwrap(),
                        graph_version: 1,
                        graph_hash: WireHash::parse(format!("sha256:{}", "b".repeat(64))).unwrap(),
                        mode: ExecutionMode::Supervised,
                    }),
                    vec![],
                    vec![],
                ));
            }
            for index in next + events.len() as u64..next + size {
                events.push(NewEvent::new(
                    OpaqueId::parse(format!("key-{index}")).unwrap(),
                    actor.clone(),
                    Sensitivity::Internal,
                    EventKind::SignalRecorded(SignalRecorded {
                        execution_id: OpaqueId::parse("execution-budget").unwrap(),
                        signal_id: OpaqueId::parse(format!("signal-{index}")).unwrap(),
                        source_kind: SignalSourceKind::Node,
                        source_id: OpaqueId::parse("implementation").unwrap(),
                        kind: "no_progress".to_owned(),
                        severity: SignalSeverity::High,
                        envelope_sha256: RawSha256::parse("a".repeat(64)).unwrap(),
                    }),
                    vec![],
                    vec![],
                ));
            }
            store
                .append_atomic(
                    &PreparedAppend::new(
                        scope.clone(),
                        OpaqueId::parse("execution-budget").unwrap(),
                        next,
                        events,
                        vec![],
                        vec![],
                    )
                    .unwrap(),
                )
                .unwrap();
            next += size;
        }
    }

    /// The WIRING, which the two `core/events` cells cannot see: `status::execute` builds one
    /// budget and hands it to BOTH halves of its read.
    ///
    /// The pointer is what makes this cell discriminating, and it was added because without it
    /// the cell was not: a first version asserted only the code, and it stayed green when the
    /// status read was put back on an UNBUDGETED store - because the fold, still holding the
    /// same lapsed budget, refused with the same code one step later. Two halves, one code,
    /// and an assertion that could not tell which had spoken. `/repository` is the open's
    /// pointer and `/events` is the fold's, so this line names the half.
    #[test]
    fn a_status_read_whose_budget_has_lapsed_refuses_in_the_open_it_budgeted() {
        let directory = tempfile::tempdir().unwrap();
        seed(directory.path(), 400);

        let lapsed = ReadBudget::starting_now(
            Arc::new(LapsingClock(AtomicU64::new(0))),
            chrono::Duration::seconds(5),
        );
        let failure = super::execute_within(directory.path(), Some("execution-budget"), lapsed)
            .expect_err("a lapsed budget must refuse the read");

        assert_eq!(failure.code, "GHE013_READ_BUDGET_EXCEEDED");
        assert_eq!(
            failure.pointer, "/repository",
            "the budget reached the journal verification, not only the fold: {}",
            failure.message
        );
        assert!(
            failure.message.contains("5000 ms budget"),
            "the refusal names the budget it was given: {}",
            failure.message
        );
    }

    /// The control. The same store, the same entry point, a budget that has NOT lapsed: the
    /// read answers, and it answers the whole history. Without this, a status read that refused
    /// every store would pass the cell above.
    #[test]
    fn a_status_read_within_its_budget_answers_the_whole_history() {
        let directory = tempfile::tempdir().unwrap();
        seed(directory.path(), 400);

        let ample = ReadBudget::starting_now(Arc::new(FrozenClock), chrono::Duration::seconds(5));
        // `Failure` carries no `Debug`, so the error arm names itself rather than being unwrapped.
        let Ok(value) = super::execute_within(directory.path(), Some("execution-budget"), ample)
        else {
            panic!("a budget that has not lapsed refuses nothing");
        };

        assert_eq!(value["headSequence"], serde_json::json!(400));
        assert_eq!(value["signalsRecorded"], serde_json::json!(399));
    }
}
