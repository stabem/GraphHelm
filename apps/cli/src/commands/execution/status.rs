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
    let store = event_store(events).map_err(|error| repository_failure(&error))?;
    // Not `load_projection`: that helper only returns the folded projection, and `headSequence`
    // needs the raw history's last sequence too. Inlining `load_projection`'s own two-line body
    // here (resolve, then replay) avoids widening `load_projection` itself — it is also called by
    // `cancel.rs` and `pause.rs`, outside this task's file set, and changing its return shape would
    // have forced edits there.
    let (scope, stream, history) = resolve_stream(&store, execution)?;
    let projection = graphhelm_events::replay(&scope, &stream, &history)
        .map_err(|error| replay_failure(&error))?;
    let head_sequence = history.last().map_or(0, |event| event.sequence);
    let mut value = render(&projection);
    value["headSequence"] = serde_json::json!(head_sequence);
    Ok(value)
}

pub fn run(events: &Path, execution: Option<&str>) -> Outcome {
    finish(COMMAND, execute(events, execution), |value| value)
}
