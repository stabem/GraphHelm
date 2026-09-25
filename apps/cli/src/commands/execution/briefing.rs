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
) -> Result<serde_json::Value, Failure> {
    execute_within(events, execution, crate::commands::status_read_budget())
}

/// The shared body, with the budget supplied rather than read from the wall clock - the seam
/// `status.rs` documents, kept for the same reason.
pub(crate) fn execute_within(
    events: &Path,
    execution: Option<&str>,
    budget: graphhelm_events::ReadBudget,
) -> Result<serde_json::Value, Failure> {
    let read = super::status::read_known_within(events, execution, budget)?;
    let answer = graphhelm_execution::attention(&read.projection, &read.inputs);
    let briefing = graphhelm_execution::briefing_view(&read.projection, &answer, &read.history);
    Ok(serde_json::to_value(briefing)
        .expect("a view built from already-serializable fold types serializes"))
}

pub fn run(events: &Path, execution: Option<&str>) -> Outcome {
    // The operator is waiting on this one, so it is the call that declares the budget (#750).
    finish(COMMAND, budgeted(events, execution), |value| value)
}
