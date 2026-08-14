use std::path::Path;

use graphhelm_protocols::{NodeOutcome, NodeState, OpaqueId};

use super::{
    Failure, execution_state, finish, load_projection, node_state_label, owner_actor,
    record_outcome, render, replay_projection, repository_failure,
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
pub fn run(events: &Path, execution: Option<&str>, node: &str) -> Outcome {
    finish(COMMAND, execute(events, execution, node), |value| value)
}

fn execute(
    events: &Path,
    execution: Option<&str>,
    node: &str,
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
    let actor = owner_actor();
    record_outcome(
        &store,
        &scope,
        &stream_id,
        &execution_id,
        &actor,
        node,
        NodeOutcome::Approved,
    )?;

    let projection = replay_projection(&store, &scope, &stream)?;
    Ok(render(&projection))
}
