use std::path::Path;

use graphhelm_protocols::{
    EventKind, ExecutionCompleted, NewEvent, NodeOutcome, OpaqueId, PersistedActor, Sensitivity,
    SimulationStatus,
};

use super::{
    Failure, RecordedOutcome, append_event, execution_state, finish, idempotency_key, is_terminal,
    load_projection, owner_actor, record_outcome, render, replay_projection, repository_failure,
    simulation_status_label,
};
use crate::commands::event_store;
use crate::output::Outcome;

const COMMAND: &str = "execution.cancel";

/// Cancels every non-terminal node and completes the execution as `Cancelled`, refusing when the
/// execution is already terminal — said here, before the fold would silently accept a second
/// `execution_completed` (it checks only that `execution_id` matches, not the prior status).
///
/// Calls `execute` with the owner actor and a fresh per-invocation idempotency key, exactly as
/// before Milestone 05a Task 4 — byte-identical CLI behaviour (the same pattern Task 3 established
/// for `signal`/`approve`).
pub fn run(events: &Path, execution: Option<&str>) -> Outcome {
    finish(
        COMMAND,
        execute(
            events,
            execution,
            owner_actor(),
            idempotency_key("execution-completed"),
        ),
        |value| value,
    )
}

/// Widened from private to `pub(crate)` (Milestone 05a Task 4), gaining `actor` and `key` as
/// explicit parameters, exactly as Task 3 did for `signal`/`approve`.
///
/// `key` is used for exactly the one event that identifies "this cancel command happened":
/// the terminal `ExecutionCompleted(Cancelled)`. The per-node `Cancelled` outcomes below still mint
/// their own fresh keys through `record_outcome` (unchanged) for the same reason `pause::execute`'s
/// per-node holds do — see that function's doc comment for the full reasoning, which applies here
/// unchanged: a variable-count fan-out cannot derive deterministic keys from a fixed suffix, and a
/// retry recognized as `Complete` by the store never re-enters this function to re-run it.
pub(crate) fn execute(
    events: &Path,
    execution: Option<&str>,
    actor: PersistedActor,
    key: OpaqueId,
) -> Result<serde_json::Value, Failure> {
    let store = event_store(events).map_err(|error| repository_failure(&error))?;
    let (scope, stream, projection) = load_projection(&store, execution)?;

    let execution_id = projection
        .execution_id
        .clone()
        .ok_or_else(|| execution_state("no execution has started on this stream", "/execution"))?;
    if matches!(
        projection.simulation_status,
        Some(SimulationStatus::Completed | SimulationStatus::Failed | SimulationStatus::Cancelled)
    ) {
        return Err(execution_state(
            &format!(
                "the execution is already {}",
                simulation_status_label(projection.simulation_status.as_ref())
            ),
            "/execution",
        ));
    }

    let stream_id = OpaqueId::parse(&stream)
        .map_err(|_| execution_state("the stream identifier is not wire-safe", "/execution"))?;
    let execution_id = OpaqueId::parse(&execution_id)
        .map_err(|_| execution_state("the execution identifier is not wire-safe", "/execution"))?;

    // Cancellation is owner sovereignty and applies from any non-terminal state
    // (`apply_transition`'s own short-circuit); every node not already terminal is cancelled.
    //
    // THE SWEEP IS LOAD-BEARING BEYOND THIS FILE. Because it takes EVERY non-terminal node in one
    // pass, a `Cancelled` node can never leave a live dependent behind it. #80 leans on that: it
    // gates dispatch on unsatisfied edges but leaves attention state-only, which is safe only
    // while no reachable predecessor can gate a dependent without raising a reason of its own.
    // A CANCEL PATH THAT CANCELS SOME NODES AND LEAVES DEPENDENTS ALIVE MUST MAKE ATTENTION
    // EDGE-AWARE — see #95, which carries the clause design ready to apply.
    let non_terminal: Vec<String> = projection
        .node_states
        .iter()
        .filter(|(_, state)| !is_terminal(**state))
        .map(|(node, _)| node.clone())
        .collect();
    for node in &non_terminal {
        record_outcome(
            &store,
            &scope,
            &stream_id,
            &execution_id,
            &actor,
            node,
            RecordedOutcome::uncaused(NodeOutcome::Cancelled),
        )?;
    }

    append_event(
        &store,
        &scope,
        &stream_id,
        NewEvent::new(
            key,
            actor,
            Sensitivity::Internal,
            EventKind::ExecutionCompleted(ExecutionCompleted {
                execution_id,
                status: SimulationStatus::Cancelled,
            }),
            vec![],
            vec![],
        ),
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
