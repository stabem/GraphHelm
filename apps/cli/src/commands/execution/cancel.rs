use std::path::Path;

use graphhelm_protocols::{
    EventKind, ExecutionCompleted, NewEvent, NodeOutcome, OpaqueId, Sensitivity, SimulationStatus,
};

use super::{
    Failure, append_event, execution_state, finish, idempotency_key, is_terminal, load_projection,
    owner_actor, record_outcome, render, replay_projection, repository_failure,
    simulation_status_label,
};
use crate::commands::event_store;
use crate::output::Outcome;

const COMMAND: &str = "execution.cancel";

/// Cancels every non-terminal node and completes the execution as `Cancelled`, refusing when the
/// execution is already terminal — said here, before the fold would silently accept a second
/// `execution_completed` (it checks only that `execution_id` matches, not the prior status).
pub fn run(events: &Path, execution: Option<&str>) -> Outcome {
    finish(COMMAND, execute(events, execution), |value| value)
}

fn execute(events: &Path, execution: Option<&str>) -> Result<serde_json::Value, Failure> {
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
    let actor = owner_actor();

    // Cancellation is owner sovereignty and applies from any non-terminal state
    // (`apply_transition`'s own short-circuit); every node not already terminal is cancelled.
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
            NodeOutcome::Cancelled,
        )?;
    }

    append_event(
        &store,
        &scope,
        &stream_id,
        NewEvent::new(
            idempotency_key("execution-completed"),
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
    Ok(render(&projection))
}
