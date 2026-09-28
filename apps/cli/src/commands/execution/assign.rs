use std::path::Path;

use graphhelm_protocols::{
    ActorId, EventKind, NewEvent, NodeAssigned, NodeState, OpaqueId, PersistedActor,
    PersistedActorType, Sensitivity,
};

use super::{
    Failure, append_event, execution_state, idempotency_key, load_projection, owner_actor,
    replay_projection, repository_failure,
};
use crate::commands::event_store;
use crate::output::Outcome;

const COMMAND: &str = "execution.assign";

/// Assigns one approved ghost node to an agent. The assignment is an append-only fact and is
/// bound to the exact sealed proposal digest already recorded for that node.
pub fn run(events: &Path, execution: Option<&str>, node: &str, actor_id: &str) -> Outcome {
    super::finish(
        COMMAND,
        execute(
            events,
            execution,
            node,
            actor_id,
            owner_actor(),
            idempotency_key("node-assigned"),
        ),
        |value| value,
    )
}

pub(crate) fn execute(
    events: &Path,
    execution: Option<&str>,
    node: &str,
    actor_id: &str,
    owner: PersistedActor,
    key: OpaqueId,
) -> Result<serde_json::Value, Failure> {
    if owner.actor_type() != PersistedActorType::Owner {
        return Err(execution_state(
            "only the owner may assign a node",
            "/actor",
        ));
    }
    let assigned_id = ActorId::parse(actor_id)
        .map_err(|_| execution_state("the assigned actor identifier is not wire-safe", "/actor"))?;
    let store = event_store(events).map_err(|error| repository_failure(&error))?;
    let (scope, stream, projection) = load_projection(&store, execution)?;
    let execution_id = projection
        .execution_id
        .clone()
        .ok_or_else(|| execution_state("no execution has started on this stream", "/execution"))?;
    if projection
        .node_states
        .get(node)
        .copied()
        .unwrap_or(NodeState::Draft)
        != NodeState::Ghost
    {
        return Err(execution_state(
            "only a ghost node can be assigned",
            "/node",
        ));
    }
    if projection.node_assignments.contains_key(node) {
        return Err(execution_state(
            "the node already has an assignment",
            "/node",
        ));
    }
    let draft_id = projection
        .ghost_drafts
        .get(node)
        .ok_or_else(|| execution_state("the ghost node has no recoverable proposal", "/node"))?;
    let digest = projection
        .proposed_draft_sha256
        .get(draft_id)
        .ok_or_else(|| execution_state("the proposal has no sealed digest", "/node"))?
        .clone();
    let stream_id = OpaqueId::parse(&stream)
        .map_err(|_| execution_state("the stream identifier is not wire-safe", "/execution"))?;
    let execution_id = OpaqueId::parse(&execution_id)
        .map_err(|_| execution_state("the execution identifier is not wire-safe", "/execution"))?;
    append_event(
        &store,
        &scope,
        &stream_id,
        NewEvent::new(
            key,
            owner,
            Sensitivity::Internal,
            EventKind::NodeAssigned(NodeAssigned {
                execution_id,
                node_id: OpaqueId::parse(node).map_err(|_| {
                    execution_state("the node identifier is not wire-safe", "/node")
                })?,
                assigned_actor: PersistedActor::new(PersistedActorType::Agent, assigned_id),
                proposal_sha256: digest,
            }),
            vec![],
            vec![],
        ),
    )?;
    let projection = replay_projection(&store, &scope, &stream)?;
    Ok(super::render(
        &projection,
        &graphhelm_execution::AttentionInputs::for_surface(
            &projection,
            std::collections::BTreeMap::new(),
            None,
        ),
        &super::Liveness::default(),
        None,
    ))
}
