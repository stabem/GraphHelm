use std::path::Path;

use super::{Failure, finish};
use crate::output::Outcome;

const COMMAND: &str = "execution.briefing";

/// `execution briefing` (#1063): replay only, no appends. The resume briefing a harness that was
/// not there reads first - what the run is for, every decision with its actor, the work done,
/// what is pending and the next step - derived from the store alone by
/// `graphhelm_execution::briefing_view`.
///
/// The read is `status::read_within`'s, byte for byte: the same budgeted open, the same fold and
/// the SAME measured attention inputs, so `pending` here is the list `status` publishes as
/// `attentionReasons` and never a second opinion. `GET /v1/executions/{id}/briefing` and the MCP
/// `briefing` tool call this exact function, which is what makes the three surfaces identical.
pub(crate) fn budgeted(
    events: &Path,
    execution: Option<&str>,
    keyring: Option<&super::signal::SignalKeyring>,
) -> Result<serde_json::Value, Failure> {
    execute_within(
        events,
        execution,
        crate::commands::serve_read_budget(),
        keyring,
    )
}

/// The shared body, with the budget supplied rather than read from the wall clock - the seam
/// `status.rs` documents, kept for the same reason.
pub(crate) fn execute_within(
    events: &Path,
    execution: Option<&str>,
    budget: graphhelm_events::ReadBudget,
    keyring: Option<&super::signal::SignalKeyring>,
) -> Result<serde_json::Value, Failure> {
    let read = super::status::read_known_within(events, execution, budget.clone())?;
    let answer = graphhelm_execution::attention(&read.projection, &read.inputs);
    let mut briefing = graphhelm_execution::briefing_view(&read.projection, &answer, &read.history);
    if let (Some(keyring), Some(execution_id)) = (keyring, read.projection.execution_id.as_deref())
    {
        briefing.plan = crate::commands::journeys::newest_plan(events, execution_id, keyring);
        let mut summary = graphhelm_execution::tasks::BenchmarkProofSummary {
            schema: "graphhelm.benchmark-proofs/1".to_owned(),
            status: "unavailable".to_owned(),
            head_sequence: briefing.as_of_sequence,
            records: Vec::new(),
            next_cursor: None,
            incomplete: true,
            unsupported: 0,
            views: Vec::new(),
        };
        let catalog = (|| {
            let store = crate::commands::budgeted_event_store(events, budget.clone())
                .map_err(|e| super::repository_failure(&e))?;
            let (scope, _) = super::stream_of(execution_id)?;
            super::tasks::proof_catalog(
                &store,
                &scope,
                &read.history,
                execution_id,
                keyring,
                &budget,
            )
        })();
        if let Ok(catalog) = catalog {
            summary.status = "available".to_owned();
            summary.unsupported = catalog.unsupported;
            summary.incomplete =
                catalog.unsupported > 0 || catalog.rejected > 0 || catalog.count > 20;
            for record in catalog.latest(20)? {
                let task = record.proof.task_id.as_str();
                if let Some(issue) = task
                    .strip_prefix("issue-")
                    .and_then(|number| number.parse::<u64>().ok())
                {
                    let link =
                        format!("/v1/executions/{execution_id}/tasks?view=proofs&issue={issue}");
                    if !summary.views.contains(&link) {
                        summary.views.push(link);
                    }
                }
                summary.records.push(record.proof.clone());
            }
            summary.records.reverse();
        }
        briefing.benchmark_proofs = Some(summary);
    }
    Ok(serde_json::to_value(briefing)
        .expect("a view built from already-serializable fold types serializes"))
}

pub fn run(
    events: &Path,
    execution: Option<&str>,
    keyring: Option<&super::signal::SignalKeyring>,
) -> Outcome {
    // The operator is waiting on this one, so it is the call that declares the budget (#750).
    finish(
        COMMAND,
        execute_within(
            events,
            execution,
            crate::commands::status_read_budget(),
            keyring,
        ),
        |value| value,
    )
}
