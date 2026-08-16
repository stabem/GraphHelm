//! Which nodes may be dispatched right now.
//!
//! A total function over a graph spec and the observed node states. It consults no clock and holds
//! no state of its own, so the same inputs always yield the same set — which is what lets a replayed
//! execution schedule identically to the original.

use std::collections::{BTreeMap, BTreeSet};

use graphhelm_protocols::{EdgeType, GraphEdge, GraphSpec, NodeState};

use crate::bounds::MAX_READY_SET;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduleError {
    /// More nodes are ready at once than the execution may dispatch.
    ///
    /// This blocks for an owner decision. Returning a truncated set instead would silently drop
    /// work and look identical to a smaller graph.
    ReadySetTooLarge,
}

/// A predecessor no longer holds its dependent back.
///
/// `Waived` and `Skipped` count: an owner waiving an obligation or skipping a phase is exercising
/// the sovereignty D-019 grants, and the run must proceed. `Failed`, `Cancelled` and `Blocked`
/// deliberately do not — a dependent of a failed node stays unready until someone intervenes.
const fn satisfies_dependents(state: NodeState) -> bool {
    matches!(
        state,
        NodeState::Succeeded | NodeState::Waived | NodeState::Skipped
    )
}

/// A node in this state may be dispatched.
///
/// `Ready` alone, because dispatching means reporting `NodeOutcome::Started`, and `apply_transition`
/// accepts that only from `Ready`, `Queued` or a resumable wait. `Draft` reaches `Ready` through
/// `Approved`, so scheduling a draft would propose work the state machine rejects — the scheduler
/// and the state machine must not be able to disagree, and
/// `every_dispatchable_state_accepts_a_start` pins that.
///
/// `Ghost` is absent by construction, which is how decision 5.2's "consumes no tokens" is enforced
/// structurally. Resuming a `Paused`, `WaitingInput` or `WaitingCapacity` node is 04e's business,
/// not the scheduler's.
const fn is_dispatchable(state: NodeState) -> bool {
    matches!(state, NodeState::Ready)
}

/// A resource guard and a domain bound are different things.
///
/// `MAX_READY_SET` is a domain bound: a legitimate execution reaches it and must block for an owner.
/// The projection's node-map guard exists only to stop a corrupt history exhausting memory, so a
/// legitimate execution must never reach it — which is only true while it stays above the bound on
/// how many nodes can be in play at once.
///
/// This pins that one relationship and no more. `MAX_SIGNALS_PER_EXECUTION` is also 10_000, but it
/// counts signals rather than nodes, so it is not comparable and is deliberately not asserted here.
///
/// At module scope rather than inside a test, so it fails `cargo build`, not merely `cargo test`.
const _RESOURCE_GUARD_EXCEEDS_THE_NODE_BOUND: () = assert!(
    graphhelm_events::MAX_PROJECTION_NODES > MAX_READY_SET,
    "a legitimate execution can reach the projection guard"
);

/// Whether this edge still holds its dependent back, given the predecessor's state — the 05d
/// refinement of 04c's every-edge rule, additive on exactly two decidable cases and proven so
/// by property (`every_other_shape_is_exactly_the_04c_rule`).
///
/// Both deltas are deliberate and both are pinned by test:
///
/// - **A literal `false` condition is statically dead**: the edge never gates, no matter its
///   type or its predecessor. Execution evaluates only the simulator's deterministic literal
///   subset (minus fixtures, which execution does not have); every non-literal condition stays
///   FAIL-CLOSED and gates exactly as an unconditioned edge — 04c's rule.
/// - **A `Failure` edge releases on `Failed` and only `Failed`**: that is what a failure route
///   IS. This both GRANTS readiness 04c never granted (the handler runs when its source
///   failed) and REMOVES 04c's spurious release (the handler no longer runs when its source
///   succeeded, was waived or was skipped — nothing failed).
fn edge_gates(edge: &GraphEdge, predecessor: NodeState) -> bool {
    if edge.condition.as_ref() == Some(&serde_json::Value::Bool(false)) {
        return false;
    }
    match edge.edge_type {
        EdgeType::Failure => predecessor != NodeState::Failed,
        _ => !satisfies_dependents(predecessor),
    }
}

/// Computes the set of nodes that may be dispatched now.
///
/// # Errors
/// Returns `ScheduleError::ReadySetTooLarge` when more than `MAX_READY_SET` nodes are ready.
pub fn ready_set(
    spec: &GraphSpec,
    states: &BTreeMap<String, NodeState>,
) -> Result<BTreeSet<String>, ScheduleError> {
    let mut predecessors: BTreeMap<&str, Vec<&GraphEdge>> = BTreeMap::new();
    for edge in &spec.edges {
        predecessors.entry(edge.to.as_str()).or_default().push(edge);
    }

    let mut ready = BTreeSet::new();
    for node_id in spec.nodes.keys() {
        // An untouched node has no recorded state yet and behaves as a draft.
        let state = states.get(node_id).copied().unwrap_or(NodeState::Draft);
        if !is_dispatchable(state) {
            continue;
        }
        let satisfied = predecessors.get(node_id.as_str()).is_none_or(|incoming| {
            incoming.iter().all(|edge| {
                !edge_gates(
                    edge,
                    states
                        .get(edge.from.as_str())
                        .copied()
                        .unwrap_or(NodeState::Draft),
                )
            })
        });
        if satisfied {
            ready.insert(node_id.clone());
            if ready.len() > MAX_READY_SET {
                return Err(ScheduleError::ReadySetTooLarge);
            }
        }
    }
    Ok(ready)
}

#[cfg(test)]
mod tests {
    use super::*;
    use graphhelm_protocols::{EdgeType, GraphEdge, GraphNode, NodeState, NodeType, Optionality};
    use std::collections::BTreeMap;

    const ALL_STATES: [NodeState; 16] = [
        NodeState::Draft,
        NodeState::Ghost,
        NodeState::Linting,
        NodeState::Ready,
        NodeState::Queued,
        NodeState::Running,
        NodeState::WaitingInput,
        NodeState::WaitingCapacity,
        NodeState::Paused,
        NodeState::Blocked,
        NodeState::Succeeded,
        NodeState::Failed,
        NodeState::Waived,
        NodeState::Skipped,
        NodeState::Cancelled,
        NodeState::Invalidated,
    ];

    fn agent_node() -> GraphNode {
        GraphNode {
            node_type: NodeType::Agent,
            name: "n".to_owned(),
            objective: "o".to_owned(),
            optionality: Optionality::Required,
            properties: BTreeMap::new(),
        }
    }

    fn spec(nodes: &[&str], edges: &[(&str, &str)]) -> GraphSpec {
        let mut spec = GraphSpec {
            entrypoints: vec![nodes[0].to_owned()],
            nodes: BTreeMap::new(),
            edges: Vec::new(),
            budgets: Default::default(),
            policies: Vec::new(),
            completion: serde_json::Value::Null,
        };
        for node in nodes {
            spec.nodes.insert((*node).to_owned(), agent_node());
        }
        for (from, to) in edges {
            spec.edges.push(GraphEdge {
                id: format!("{from}-to-{to}"),
                from: (*from).to_owned(),
                to: (*to).to_owned(),
                edge_type: EdgeType::Control,
                payload_schema: None,
                condition: None,
                on_false: None,
                on_unknown: None,
                bindings: BTreeMap::new(),
                priority: None,
            });
        }
        spec
    }

    fn states(pairs: &[(&str, NodeState)]) -> BTreeMap<String, NodeState> {
        pairs
            .iter()
            .map(|(node, state)| ((*node).to_owned(), *state))
            .collect()
    }

    /// An untouched node defaults to `Draft`, which is not dispatchable: it reaches `Ready` through
    /// approval. Scheduling it would propose work `apply_transition` rejects.
    #[test]
    fn an_untouched_node_is_not_dispatchable_until_it_is_ready() {
        let spec = spec(&["a", "b"], &[("a", "b")]);
        assert!(ready_set(&spec, &BTreeMap::new()).unwrap().is_empty());

        let ready = ready_set(&spec, &states(&[("a", NodeState::Ready)])).unwrap();
        assert_eq!(ready, ["a".to_owned()].into_iter().collect());
    }

    /// The scheduler and the state machine must not be able to disagree. Every state the scheduler
    /// will dispatch has to accept a `Started` outcome, or it proposes work that fails.
    #[test]
    fn every_dispatchable_state_accepts_a_start() {
        for state in ALL_STATES {
            if !is_dispatchable(state) {
                continue;
            }
            let request = crate::TransitionRequest {
                current: state,
                outcome: graphhelm_protocols::NodeOutcome::Started,
                attempts: 0,
                identical_outcomes: 0,
            };
            assert!(
                crate::apply_transition(&request).is_ok(),
                "{state:?} is dispatchable but rejects a start"
            );
        }
    }

    #[test]
    fn a_successor_becomes_ready_once_every_predecessor_is_satisfied() {
        let spec = spec(&["a", "b", "c"], &[("a", "c"), ("b", "c")]);
        let half = ready_set(
            &spec,
            &states(&[("a", NodeState::Succeeded), ("c", NodeState::Ready)]),
        )
        .unwrap();
        assert!(!half.contains("c"), "c ran with b unfinished");

        let full = ready_set(
            &spec,
            &states(&[
                ("a", NodeState::Succeeded),
                ("b", NodeState::Waived),
                ("c", NodeState::Ready),
            ]),
        )
        .unwrap();
        assert!(full.contains("c"));
    }

    /// The rule with the widest blast radius: a data edge gates exactly like a control edge. Until
    /// 04d evaluates edge conditions, treating any edge type as non-gating could let a node run
    /// before what it depends on.
    #[test]
    fn a_non_control_edge_gates_just_like_a_control_edge() {
        let mut spec = spec(&["a", "b"], &[("a", "b")]);
        spec.edges[0].edge_type = EdgeType::Data;
        assert!(
            !ready_set(&spec, &states(&[("b", NodeState::Ready)]))
                .unwrap()
                .contains("b"),
            "a data edge did not gate its dependent"
        );
        assert!(
            ready_set(
                &spec,
                &states(&[("a", NodeState::Succeeded), ("b", NodeState::Ready)])
            )
            .unwrap()
            .contains("b")
        );
    }

    /// A ghost is a proposal. Excluding it here is what makes "consumes no tokens" structural
    /// rather than a convention someone has to remember.
    #[test]
    fn a_ghost_is_never_ready_and_never_satisfies_a_dependent() {
        let spec = spec(&["a", "b"], &[("a", "b")]);
        let ready = ready_set(&spec, &states(&[("a", NodeState::Ghost)])).unwrap();
        assert!(ready.is_empty(), "a ghost or its dependent was scheduled");
    }

    /// Exhaustive over all sixteen states, not a sample. A state added later is non-dispatchable
    /// until someone decides otherwise, and this test is where they must decide it.
    #[test]
    fn only_ready_nodes_are_dispatchable() {
        let spec = spec(&["a"], &[]);
        for state in ALL_STATES {
            let ready = ready_set(&spec, &states(&[("a", state)])).unwrap();
            assert_eq!(
                ready.contains("a"),
                state == NodeState::Ready,
                "{state:?} dispatchability is wrong"
            );
        }
    }

    /// Decision 5.7: exceeding a bound blocks for an owner decision. It never truncates, because a
    /// truncated ready set looks exactly like a smaller graph and loses work silently.
    #[test]
    fn an_oversized_ready_set_blocks_rather_than_truncating() {
        let names: Vec<String> = (0..=MAX_READY_SET)
            .map(|index| format!("n{index}"))
            .collect();
        let borrowed: Vec<&str> = names.iter().map(String::as_str).collect();
        let spec = spec(&borrowed, &[]);
        let all_ready: BTreeMap<String, NodeState> = names
            .iter()
            .map(|name| (name.clone(), NodeState::Ready))
            .collect();
        assert_eq!(
            ready_set(&spec, &all_ready).unwrap_err(),
            ScheduleError::ReadySetTooLarge
        );
    }
}
