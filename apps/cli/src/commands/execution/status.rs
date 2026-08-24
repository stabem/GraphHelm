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
    let mut value = render(&projection, &inputs, &super::Liveness::measured(&history));
    // The WIRE field keeps its existing shape on purpose, zero and all: `headSequence` is a
    // different contract from `at_sequence`, read by clients that already treat 0 as "nothing
    // yet", and widening it to null is a wire change that needs its own justification. The
    // flattening is left here DECLARED rather than silently carried into the seam.
    value["headSequence"] = serde_json::json!(at_sequence.unwrap_or(0));
    Ok(value)
}

pub fn run(events: &Path, execution: Option<&str>, html: Option<&Path>) -> Outcome {
    if let Some(html) = html
        && let Err(failure) = write_snapshot(events, execution, html)
    {
        return finish::<serde_json::Value>(COMMAND, Err(failure), |value| value);
    }
    finish(COMMAND, execute(events, execution), |value| value)
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
