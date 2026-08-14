use std::path::Path;

use graphhelm_protocols::{
    EventKind, ExecutionPaused, NewEvent, NodeOutcome, NodeState, OpaqueId, Sensitivity,
    SimulationStatus,
};

use super::{
    Failure, append_event, execution_state, finish, idempotency_key, load_projection, owner_actor,
    record_outcome, render, replay_projection, repository_failure, simulation_status_label,
};
use crate::commands::event_store;
use crate::output::Outcome;

const COMMAND: &str = "execution.pause";

/// Holds every dispatchable node (`Ready`/`Queued`), refusing unless the aggregate status is
/// `None` or `Running` — said here, before the fold's own `ExecutionPaused` guard would call a
/// second pause corrupt.
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
    if !matches!(
        projection.simulation_status,
        None | Some(SimulationStatus::Running)
    ) {
        return Err(execution_state(
            &format!(
                "the execution is {}, not none or running",
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

    append_event(
        &store,
        &scope,
        &stream_id,
        NewEvent::new(
            idempotency_key("execution-paused"),
            actor.clone(),
            Sensitivity::Internal,
            EventKind::ExecutionPaused(ExecutionPaused {
                execution_id: execution_id.clone(),
            }),
            vec![],
            vec![],
        ),
    )?;

    // Held exactly here, from the projection as read before the pause itself: every node still
    // dispatchable (`Ready`) or retry-pending (`Queued`) right now. `resume` re-derives its own
    // list independently rather than reading this one back, so a node this pause missed cannot be
    // silently re-dispatched by trusting a stale record of what was held.
    let held: Vec<String> = projection
        .node_states
        .iter()
        .filter(|(_, state)| matches!(state, NodeState::Ready | NodeState::Queued))
        .map(|(node, _)| node.clone())
        .collect();
    for node in &held {
        record_outcome(
            &store,
            &scope,
            &stream_id,
            &execution_id,
            &actor,
            node,
            NodeOutcome::Paused,
        )?;
    }

    let projection = replay_projection(&store, &scope, &stream)?;
    let mut data = render(&projection);
    if let serde_json::Value::Object(ref mut map) = data {
        map.insert("heldNodes".to_owned(), serde_json::json!(held));
    }
    Ok(data)
}
