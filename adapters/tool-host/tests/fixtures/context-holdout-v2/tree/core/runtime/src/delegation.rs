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

use std::collections::BTreeMap;

use graphhelm_events::{ExecutionProjection, SubagentRecord};
use graphhelm_policy::delegation::{DelegationPolicy, choose};
use graphhelm_protocols::{
    DelegationChosen, GraphNode, GraphSpec, NodeState, OpaqueId, SubagentBasis, SubagentKind,
    SubagentReused,
};

/// A node's `delegation` block is not the closed schema shape. Shared by both drivers, which
/// refuse such a graph before any node effect at `/spec/nodes/<id>/delegation`.
pub const DELEGATION_DECLARATION_INVALID_CODE: &str = "GHG018_DELEGATION_DECLARATION_INVALID";

/// The nodes whose `delegation` block cannot be read, in spec order. One condition for both
/// drivers' preflights, so they cannot disagree about which graphs are refusable.
#[must_use]
pub fn malformed_delegation_nodes(spec: &GraphSpec) -> Vec<String> {
    spec.nodes
        .iter()
        .filter(|(_, node)| node.delegation().is_err())
        .map(|(id, _)| id.clone())
        .collect()
}

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
    let policy = DelegationPolicy::routed();
    let choice = choose(&policy, declared.kind, red_checks);
    Ok(Some(DelegationChosen {
        node_id: OpaqueId::parse(node_id).map_err(|_| DelegationError::NodeId)?,
        policy: policy.id,
        kind: choice.kind,
        tier: choice.tier,
        effort: choice.effort,
        escalated: choice.escalated,
        red_checks,
    }))
}

/// ADR-041 point 1: the closed allowlist of `(previous kind, receiving kind)` pairs a briefed
/// subagent may be reused across. No pair receives into `reviewer` or `verifier`.
pub const REUSE_ALLOWLIST: [(SubagentKind, SubagentKind); 3] = [
    (SubagentKind::Explorer, SubagentKind::Explorer),
    (SubagentKind::Explorer, SubagentKind::Implementer),
    (SubagentKind::Implementer, SubagentKind::Implementer),
];

/// ADR-041 point 2 as amended by ADR-042: what the reuse decision compares, and whether the
/// candidate's session is still held. Each measurement is `None` when it was not measured; either
/// `None` refuses reuse, because an estimate is never substituted (D-043).
///
/// The async driver fills it at dispatch ([`crate::driver`]): `allocated` from the serving
/// route's declared `contextWindowTokens` less its `maxOutputTokens`, `used` from the sealed
/// accounting receipts read back per subagent, `held` from the model executor's session store.
/// The synchronous CLI driver holds no session and reports no counts, so it passes
/// [`ReuseBound::unmeasured`] and a candidate that passes the key is recorded `bound_unavailable`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReuseBound {
    /// ADR-042 point 6: the tokens the session may occupy on the serving route.
    pub allocated: Option<u64>,
    /// ADR-042 point 7, per subagent id: the LAST accounted attempt's provider-reported input plus
    /// output tokens (a reused call's input already contains the replayed history, so a sum would
    /// count it twice), or `None` when any receipt of that subagent has either counter
    /// `unavailable`.
    pub used: BTreeMap<String, Option<u64>>,
    /// ADR-042 point 2: the subagent ids whose session the model executor holds right now.
    pub held: std::collections::BTreeSet<String>,
    /// #298: the subagent ids whose replayed history the provider refused as larger than its
    /// context window during this drive. Such a candidate is never resumed: the provider's own
    /// refusal is the measurement, so the node is recorded fresh with `bound_exceeded`.
    pub overflowed: std::collections::BTreeSet<String>,
}

impl ReuseBound {
    /// Nothing measured and no session held: reuse is refused for every candidate.
    #[must_use]
    pub fn unmeasured() -> Self {
        Self::default()
    }
}

/// Why no `subagent_reused` could be built for a node about to dispatch.
#[derive(Debug, thiserror::Error)]
pub enum SubagentError {
    /// The stream carries no `execution_started`, so the graph-version key member is unknown.
    #[error("the execution's graph version is not recorded")]
    GraphVersion,
}

/// The `subagent_reused` record for dispatching the delegated node `chosen` names, decided from
/// the journal `projection` folds and the measured `bound`. `fresh_id` is the Runtime-minted id
/// used when no subagent is reused.
///
/// The execution and repository-scope members of the key hold by construction: `projection` is
/// one execution stream in one scope. Candidates are the subagents already recorded in it, each
/// judged by the node it took most recently; a candidate qualifies only when that node ran on the
/// same graph version, finished (`succeeded`), and its kind pair is in [`REUSE_ALLOWLIST`]. The
/// newest qualifying candidate is then held to the bound. A `reviewer` or `verifier` node never
/// reuses: authorship per subject is not projected, so any recorded subagent counts as a possible
/// author of what it reviews (ADR-041 point 3).
///
/// # Errors
/// [`SubagentError::GraphVersion`] when the stream has no `execution_started`.
pub fn subagent_event(
    chosen: &DelegationChosen,
    projection: &ExecutionProjection,
    bound: &ReuseBound,
    fresh_id: OpaqueId,
) -> Result<SubagentReused, SubagentError> {
    let graph_version = projection
        .started_graph_version
        .ok_or(SubagentError::GraphVersion)?;
    let node = chosen.node_id.as_str();
    let fresh = |basis, tokens_used, tokens_allocated| SubagentReused {
        node_id: chosen.node_id.clone(),
        subagent_id: fresh_id.clone(),
        kind: chosen.kind,
        graph_version,
        basis,
        from_node_id: None,
        from_kind: None,
        tokens_used,
        tokens_allocated,
    };

    let others = || {
        projection
            .subagents
            .iter()
            .filter(move |(other, _)| other.as_str() != node)
            .map(|(_, record)| record)
    };
    if matches!(chosen.kind, SubagentKind::Reviewer | SubagentKind::Verifier) {
        let basis = if others().next().is_some() {
            SubagentBasis::AuthorUnderReview
        } else {
            SubagentBasis::NoEligibleSubagent
        };
        return Ok(fresh(basis, None, None));
    }

    // Each subagent is judged by the node it took most recently.
    let mut latest: BTreeMap<&str, &SubagentRecord> = BTreeMap::new();
    for record in others() {
        let id = record.record.subagent_id.as_str();
        if latest
            .get(id)
            .is_none_or(|held| held.at_sequence < record.at_sequence)
        {
            latest.insert(id, record);
        }
    }
    let candidate = latest
        .values()
        .filter(|record| {
            record.record.graph_version == graph_version
                && REUSE_ALLOWLIST.contains(&(record.record.kind, chosen.kind))
                && projection
                    .node_states
                    .get(record.record.node_id.as_str())
                    .copied()
                    == Some(NodeState::Succeeded)
        })
        .max_by_key(|record| record.at_sequence);
    let Some(candidate) = candidate else {
        return Ok(fresh(SubagentBasis::NoEligibleSubagent, None, None));
    };

    let used = bound
        .used
        .get(candidate.record.subagent_id.as_str())
        .copied()
        .flatten();
    let held = bound.held.contains(candidate.record.subagent_id.as_str());
    if bound
        .overflowed
        .contains(candidate.record.subagent_id.as_str())
    {
        return Ok(fresh(SubagentBasis::BoundExceeded, used, bound.allocated));
    }
    match (used, bound.allocated) {
        // ADR-042 point 2: under the bound but the executor no longer holds the session. Reuse
        // never reconstructs one from sealed evidence.
        (Some(used), Some(allocated)) if used < allocated && !held => Ok(fresh(
            SubagentBasis::SessionUnavailable,
            Some(used),
            Some(allocated),
        )),
        (Some(used), Some(allocated)) if used < allocated => Ok(SubagentReused {
            node_id: chosen.node_id.clone(),
            subagent_id: candidate.record.subagent_id.clone(),
            kind: chosen.kind,
            graph_version,
            basis: SubagentBasis::Reused,
            from_node_id: Some(candidate.record.node_id.clone()),
            from_kind: Some(candidate.record.kind),
            tokens_used: Some(used),
            tokens_allocated: Some(allocated),
        }),
        (Some(used), Some(allocated)) => Ok(fresh(
            SubagentBasis::BoundExceeded,
            Some(used),
            Some(allocated),
        )),
        (used, allocated) => Ok(fresh(SubagentBasis::BoundUnavailable, used, allocated)),
    }
}

#[cfg(test)]
mod tests {
    use graphhelm_protocols::{DelegationEffort, DelegationPolicyId, DelegationTier};

    use super::*;

    const FRESH: &str = "subagent-fresh";

    fn id(value: &str) -> OpaqueId {
        OpaqueId::parse(value).unwrap()
    }

    fn chosen(node: &str, kind: SubagentKind) -> DelegationChosen {
        DelegationChosen {
            node_id: id(node),
            policy: DelegationPolicyId::Routed,
            kind,
            tier: DelegationTier::Standard,
            effort: DelegationEffort::Medium,
            escalated: false,
            red_checks: 0,
        }
    }

    /// A projection of an execution on graph version 3 where `prior` (of `kind`, in `state`, on
    /// `graph_version`) was taken by subagent `sa-1`.
    fn after(
        prior: &str,
        kind: SubagentKind,
        state: NodeState,
        graph_version: u64,
    ) -> ExecutionProjection {
        let mut projection = ExecutionProjection {
            started_graph_version: Some(3),
            ..ExecutionProjection::default()
        };
        projection.node_states.insert(prior.to_owned(), state);
        projection.subagents.insert(
            prior.to_owned(),
            SubagentRecord {
                at_sequence: 7,
                record: SubagentReused {
                    node_id: id(prior),
                    subagent_id: id("sa-1"),
                    kind,
                    graph_version,
                    basis: SubagentBasis::NoEligibleSubagent,
                    from_node_id: None,
                    from_kind: None,
                    tokens_used: None,
                    tokens_allocated: None,
                },
            },
        );
        projection
    }

    fn measured(used: Option<u64>, allocated: u64) -> ReuseBound {
        ReuseBound {
            allocated: Some(allocated),
            used: BTreeMap::from([("sa-1".to_owned(), used)]),
            held: std::collections::BTreeSet::from(["sa-1".to_owned()]),
            overflowed: std::collections::BTreeSet::new(),
        }
    }

    fn decide(
        node_kind: SubagentKind,
        projection: &ExecutionProjection,
        bound: &ReuseBound,
    ) -> SubagentReused {
        subagent_event(&chosen("next", node_kind), projection, bound, id(FRESH)).unwrap()
    }

    // The one path that reuses: key matches and the measured tokens are under the budget. The
    // reused record carries the earlier subagent's id, not the freshly minted one.
    #[test]
    fn a_key_match_under_a_measured_bound_reuses_the_briefed_subagent() {
        let projection = after("prior", SubagentKind::Explorer, NodeState::Succeeded, 3);
        let record = decide(
            SubagentKind::Implementer,
            &projection,
            &measured(Some(900), 1000),
        );
        assert_eq!(record.basis, SubagentBasis::Reused);
        assert_eq!(record.subagent_id.as_str(), "sa-1");
        assert_eq!(record.from_node_id, Some(id("prior")));
        assert_eq!(record.from_kind, Some(SubagentKind::Explorer));
        assert_eq!(
            (record.tokens_used, record.tokens_allocated),
            (Some(900), Some(1000))
        );
    }

    // The bound is strict, and an `unavailable` counter or an unknown budget refuses reuse.
    #[test]
    fn an_exhausted_or_unmeasurable_bound_records_a_fresh_subagent() {
        let projection = after("prior", SubagentKind::Implementer, NodeState::Succeeded, 3);
        let at_budget = decide(
            SubagentKind::Implementer,
            &projection,
            &measured(Some(1000), 1000),
        );
        assert_eq!(at_budget.basis, SubagentBasis::BoundExceeded);
        assert_eq!(at_budget.subagent_id.as_str(), FRESH);
        assert_eq!(at_budget.from_node_id, None);
        for bound in [measured(None, 1000), ReuseBound::unmeasured()] {
            let record = decide(SubagentKind::Implementer, &projection, &bound);
            assert_eq!(record.basis, SubagentBasis::BoundUnavailable);
            assert_eq!(record.subagent_id.as_str(), FRESH);
        }
    }

    // ADR-042 point 2: a measured bound with room left does not reuse a session the executor no
    // longer holds (a drive that parked, a restart, an executor that holds none).
    #[test]
    fn a_session_the_executor_does_not_hold_is_never_reused() {
        let projection = after("prior", SubagentKind::Explorer, NodeState::Succeeded, 3);
        let mut bound = measured(Some(10), 1000);
        bound.held.clear();
        let record = decide(SubagentKind::Implementer, &projection, &bound);
        assert_eq!(record.basis, SubagentBasis::SessionUnavailable);
        assert_eq!(record.subagent_id.as_str(), FRESH);
        assert_eq!(record.from_node_id, None);
    }

    // Every other key member: graph version, the kind-pair allowlist, and a finished prior node.
    #[test]
    fn any_key_mismatch_means_a_fresh_subagent_even_under_a_measured_bound() {
        let bound = measured(Some(1), 1000);
        for (projection, kind) in [
            (
                after("prior", SubagentKind::Explorer, NodeState::Succeeded, 2),
                SubagentKind::Implementer,
            ),
            (
                after("prior", SubagentKind::Implementer, NodeState::Succeeded, 3),
                SubagentKind::Explorer,
            ),
            (
                after("prior", SubagentKind::Explorer, NodeState::Running, 3),
                SubagentKind::Explorer,
            ),
        ] {
            let record = decide(kind, &projection, &bound);
            assert_eq!(record.basis, SubagentBasis::NoEligibleSubagent);
            assert_eq!(record.subagent_id.as_str(), FRESH);
        }
    }

    // Point 3: a reviewer or verifier never reuses a subagent that authored work here.
    #[test]
    fn a_reviewer_or_verifier_never_reuses_an_author() {
        let projection = after("prior", SubagentKind::Implementer, NodeState::Succeeded, 3);
        for kind in [SubagentKind::Reviewer, SubagentKind::Verifier] {
            let record = decide(kind, &projection, &measured(Some(1), 1000));
            assert_eq!(record.basis, SubagentBasis::AuthorUnderReview);
            assert_eq!(record.subagent_id.as_str(), FRESH);
        }
    }

    #[test]
    fn a_stream_without_a_recorded_graph_version_is_refused() {
        let projection = ExecutionProjection::default();
        assert!(matches!(
            subagent_event(
                &chosen("next", SubagentKind::Explorer),
                &projection,
                &ReuseBound::unmeasured(),
                id(FRESH)
            ),
            Err(SubagentError::GraphVersion)
        ));
    }
}
