use std::path::Path;

use graphhelm_protocols::{NodeOutcome, NodeState, OpaqueId, PersistedActor};

use super::{
    Failure, RecordedOutcome, execution_state, finish, idempotency_key, load_projection,
    node_state_label, owner_actor, record_outcome_with_key, render, replay_projection,
    repository_failure,
};
use crate::commands::event_store;
use crate::output::Outcome;

const COMMAND: &str = "execution.approve";

/// The owner approves a `Ghost` or `Blocked` node, readying it — for a `Blocked` node with
/// `last_outcome == Interrupted`, this *is* the triage act `resume_preconditions` waits for.
///
/// Does not auto-drive afterwards: D-020's rule that nothing auto-starts out of a manual
/// intervention. The owner runs `resume` next (an already-`start`ed execution quiesces again on
/// its next `resume` too).
///
/// Calls `execute` with the owner actor and a fresh per-invocation idempotency key, exactly as
/// before Milestone 05a Task 3 — byte-identical CLI behaviour.
pub fn run(events: &Path, execution: Option<&str>, node: &str) -> Outcome {
    finish(
        COMMAND,
        execute(
            events,
            execution,
            node,
            owner_actor(),
            idempotency_key("node-outcome"),
        ),
        |value| value,
    )
}

/// Widened from private to `pub(crate)` (Milestone 05a Task 3), gaining `actor` and `key` as
/// explicit parameters — the mechanical widening the plan's file table names, plus the one
/// additional parameter the idempotent-retry semantics require (see `signal::execute`'s identical
/// note). No other logic changed.
pub(crate) fn execute(
    events: &Path,
    execution: Option<&str>,
    node: &str,
    actor: PersistedActor,
    key: OpaqueId,
) -> Result<serde_json::Value, Failure> {
    let store = event_store(events).map_err(|error| repository_failure(&error))?;
    let (scope, stream, projection) = load_projection(&store, execution)?;

    let execution_id = projection
        .execution_id
        .clone()
        .ok_or_else(|| execution_state("no execution has started on this stream", "/execution"))?;

    // Absent from `node_states` reads as `Draft`, the same convention `ready_set` and the driver
    // use — indistinguishable, from the projection alone, from a real node nobody has approved
    // yet, and `Draft` is correctly refused by the same check below.
    let state = projection
        .node_states
        .get(node)
        .copied()
        .unwrap_or(NodeState::Draft);
    if !matches!(state, NodeState::Ghost | NodeState::Blocked) {
        return Err(execution_state(
            &format!("node is {}, not ghost or blocked", node_state_label(state)),
            "/node",
        ));
    }

    let stream_id = OpaqueId::parse(&stream)
        .map_err(|_| execution_state("the stream identifier is not wire-safe", "/execution"))?;
    let execution_id = OpaqueId::parse(&execution_id)
        .map_err(|_| execution_state("the execution identifier is not wire-safe", "/execution"))?;
    record_outcome_with_key(
        &store,
        &scope,
        &stream_id,
        &execution_id,
        &actor,
        node,
        RecordedOutcome::uncaused(NodeOutcome::Approved),
        key,
    )?;

    let projection = replay_projection(&store, &scope, &stream)?;
    Ok(render(
        &projection,
        // Nothing measured here on purpose: this command reports the mutation it just made,
        // not a liveness reading. The seam turns "not measured" into `silenceUnevaluated`
        // rather than into calm, so the omission is stated instead of implied.
        // But the BUDGET is not a measurement. It is declared in the graph this projection
        // already holds, and `default()` asserted there was none -- so `attention` took the
        // `(None, measured)` arm and answered `NoDeclaredBudget` with the remedy "declare a
        // budget for this node", to an operator who had declared one (G's measurement on
        // #1013). `for_surface` derives it from the projection; the empty map is the AGE,
        // which really is unmeasured here, and that lands on the honest `(Some, None)` arm:
        // `NotMeasured`, remedy `Unavailable { SurfaceMeasuredNoAge }`.
        &graphhelm_execution::AttentionInputs::for_surface(
            &projection,
            std::collections::BTreeMap::new(),
            None,
        ),
        // Same posture for the instants: a mutation reply publishes null rather than a
        // stillness it never looked for.
        &super::Liveness::default(),
        None,
    ))
}
