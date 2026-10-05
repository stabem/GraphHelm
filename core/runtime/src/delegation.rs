//! ADR-040 (#290): the delegation choice recorded at dispatch.
//!
//! Both dispatch drivers (this crate's async loop and `apps/cli`'s synchronous one) call
//! [`delegation_event`] at the point a node is dispatched. A node that declares `delegation` gets
//! exactly one `delegation_chosen` per dispatch; a node without the field gets `None`, and its
//! dispatch is unchanged. The choice is `graphhelm_policy::delegation::choose` over the built-in
//! [`DelegationPolicy::routed`] policy (no policy source is configurable yet) and the node's red
//! check count, so the same journal always yields the same choice.
//!
//! **Red checks, defined.** The count of failing `gate_verdict` events (`passed: false`) naming
//! this node, already recorded in this execution, as folded into
//! [`ExecutionProjection::red_checks`]. A gate verdict is the only mechanical check result the
//! Event Store records against a node today, so it is the only honest source; nothing is inferred
//! from outcome reasons or free text.
//!
//! Recording is not enforcement: the tier is not mapped to a model route here.

use graphhelm_events::ExecutionProjection;
use graphhelm_policy::delegation::{DelegationPolicy, choose};
use graphhelm_protocols::{DelegationChosen, GraphNode, OpaqueId};

/// Why a delegation choice could not be built for a node that is about to dispatch.
#[derive(Debug, thiserror::Error)]
pub enum DelegationError {
    /// The node carries a `delegation` block that is not the closed schema shape.
    #[error("the node's delegation declaration is malformed")]
    Malformed(#[from] serde_json::Error),
    /// The node id is not a valid opaque id, so no event could name it.
    #[error("the node id cannot be recorded as an opaque id")]
    NodeId,
}

/// The `delegation_chosen` payload for dispatching `node_id` now, or `None` when the node
/// declares no delegation.
///
/// # Errors
/// [`DelegationError`] when the declaration is malformed or the node id is not recordable; the
/// caller refuses the dispatch rather than dispatching without the record.
pub fn delegation_event(
    node_id: &str,
    node: &GraphNode,
    projection: &ExecutionProjection,
) -> Result<Option<DelegationChosen>, DelegationError> {
    let Some(declared) = node.delegation()? else {
        return Ok(None);
    };
    let red_checks = projection.red_checks.get(node_id).copied().unwrap_or(0);
    let choice = choose(&DelegationPolicy::routed(), declared.kind, red_checks);
    Ok(Some(DelegationChosen {
        node_id: OpaqueId::parse(node_id).map_err(|_| DelegationError::NodeId)?,
        kind: choice.kind,
        tier: choice.tier,
        effort: choice.effort,
        escalated: choice.escalated,
        red_checks,
    }))
}
