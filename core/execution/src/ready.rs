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

/// Every incoming edge of one node, indexed once so a caller asking about many nodes pays for
/// the walk once rather than per node.
fn predecessor_map(spec: &GraphSpec) -> BTreeMap<&str, Vec<&GraphEdge>> {
    let mut predecessors: BTreeMap<&str, Vec<&GraphEdge>> = BTreeMap::new();
    for edge in &spec.edges {
        predecessors.entry(edge.to.as_str()).or_default().push(edge);
    }
    predecessors
}

/// The edge half of readiness, against a prebuilt index. THE one implementation of "may this
/// node's dependencies let it run"; everything else calls it.
fn satisfied_with(
    predecessors: &BTreeMap<&str, Vec<&GraphEdge>>,
    states: &BTreeMap<String, NodeState>,
    node: &str,
) -> bool {
    predecessors.get(node).is_none_or(|incoming| {
        incoming.iter().all(|edge| {
            !edge_gates(
                edge,
                states
                    .get(edge.from.as_str())
                    .copied()
                    .unwrap_or(NodeState::Draft),
            )
        })
    })
}

/// Whether `node`'s incoming edges currently release it, independent of its own state.
///
/// Extracted from `ready_set` deliberately: the scheduler asks this of `Ready` nodes, and the
/// driver's retry chain must ask it of `Queued` ones (#80). A second copy of the rule in the
/// driver would be the same source of truth today and drift tomorrow, so there is exactly one
/// implementation and both callers reach it. Builds its own index for a single query; callers
/// asking about many nodes should use [`dispatch_candidates`], which indexes once.
#[must_use]
pub fn edges_satisfied(spec: &GraphSpec, states: &BTreeMap<String, NodeState>, node: &str) -> bool {
    satisfied_with(&predecessor_map(spec), states, node)
}

/// Computes the set of nodes that may be dispatched now.
///
/// # Errors
/// Returns `ScheduleError::ReadySetTooLarge` when more than `MAX_READY_SET` nodes are ready.
pub fn ready_set(
    spec: &GraphSpec,
    states: &BTreeMap<String, NodeState>,
) -> Result<BTreeSet<String>, ScheduleError> {
    let predecessors = predecessor_map(spec);

    let mut ready = BTreeSet::new();
    for node_id in spec.nodes.keys() {
        // An untouched node has no recorded state yet and behaves as a draft.
        let state = states.get(node_id).copied().unwrap_or(NodeState::Draft);
        if !is_dispatchable(state) {
            continue;
        }
        if satisfied_with(&predecessors, states, node_id) {
            ready.insert(node_id.clone());
            if ready.len() > MAX_READY_SET {
                return Err(ScheduleError::ReadySetTooLarge);
            }
        }
    }
    Ok(ready)
}

/// Everything the driver may dispatch on this pass: the ready set, plus nodes already `Queued`
/// and awaiting a retry.
///
/// `ready_set` alone is not the driver's candidate set, because `is_dispatchable` is `Ready`-only
/// by design — a `Queued` node is retry-pending, reached that state through the state machine,
/// and must still be dispatched. The driver used to build this union inline; it lives here so
/// the edge rule applies to BOTH halves from one implementation.
///
/// #80: the retry half is edge-gated too. It was a bare `state == Queued` filter, so a node could
/// reach dispatch with its dependencies unmet — reachable through pause/resume, which records
/// `Started` for a held-but-gated node and lands it here via `(Paused, Started) => Queued`. The
/// condition below is `satisfied_with`, the SAME rule `ready_set` applies, against the same index:
/// asking the question a second way is how the two halves would come to disagree.
///
/// This is deliberately not a `Succeeded`-predecessor check, which is the phrasing that reads
/// correctly and strands every failure route in the system — a `Failure` edge releases on `Failed`
/// and nothing else. `edge_gates` already owns that distinction, and
/// `a_queued_failure_handler_dispatches_when_its_source_failed` is what stops anyone rewriting it.
///
/// # Errors
/// Propagates `ScheduleError::ReadySetTooLarge` from [`ready_set`].
///
/// THE BOUND COVERS THE READY HALF ONLY, and the returned set can exceed `MAX_READY_SET` without
/// error, because the `Queued` insertions happen after `ready_set` has already tested its own
/// count. That is inherited behaviour — the inline union this replaced had the same property — but
/// it is stated here because this function's NAME now implies the whole set, so silence would read
/// as "the bound covers this". Making the bound mean the union is a deliberate decision nobody has
/// taken; it would newly block executions that legitimately run today.
pub fn dispatch_candidates(
    spec: &GraphSpec,
    states: &BTreeMap<String, NodeState>,
) -> Result<BTreeSet<String>, ScheduleError> {
    let mut candidates = ready_set(spec, states)?;
    // Indexed once for the whole loop, unlike `edges_satisfied`'s single-query convenience form.
    let predecessors = predecessor_map(spec);
    for (node, state) in states {
        if *state == NodeState::Queued && satisfied_with(&predecessors, states, node) {
            candidates.insert(node.clone());
        }
    }
    Ok(candidates)
}

#[cfg(test)]
mod tests {
    use super::*;
    use graphhelm_protocols::{EdgeType, GraphEdge, GraphNode, NodeState, NodeType, Optionality};
    use std::collections::BTreeMap;

    const ALL_STATES: &[NodeState] = NodeState::every();

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

    // ---------------------------------------------------------------------------------------
    // #80: the driver's candidate set. `ready_set` was always edge-gated; the retry-pending half
    // the driver chained in beside it was a bare `state == Queued` filter, so a node could reach
    // dispatch with its dependencies unmet. These pin the union, one door each.
    // ---------------------------------------------------------------------------------------

    /// THE DEFECT (#80). A node sitting `Queued` behind an unfinished predecessor must not be a
    /// dispatch candidate.
    ///
    /// This is the state `resume` manufactures: `pause` records a bare-`Ready` but edge-gated node
    /// as `Paused`, and `(Paused, Started) => Queued` (`transition.rs:109`) puts it in the retry
    /// chain. The node never ran, so nothing about its own history says it should not run — only
    /// its edges do.
    #[test]
    fn a_queued_node_behind_an_unfinished_predecessor_is_not_a_candidate() {
        let spec = spec(&["a", "b"], &[("a", "b")]);
        let states = states(&[("a", NodeState::Running), ("b", NodeState::Queued)]);

        let candidates = dispatch_candidates(&spec, &states).unwrap();

        assert!(
            !candidates.contains("b"),
            "b is queued behind a predecessor that has not satisfied its edge; \
             dispatching it runs work whose precondition was never met: {candidates:?}"
        );
    }

    /// THE NO-REGRESSION TWIN. A genuinely retrying node — queued with its predecessor finished —
    /// must still dispatch.
    ///
    /// Deliberately paired with the test above: a fix that simply dropped `Queued` from the union
    /// would satisfy that one and silently stop every retry in the system. This is the guard that
    /// makes over-tightening fail loudly.
    #[test]
    fn a_queued_node_whose_predecessor_finished_is_still_a_candidate() {
        let spec = spec(&["a", "b"], &[("a", "b")]);
        let states = states(&[("a", NodeState::Succeeded), ("b", NodeState::Queued)]);

        let candidates = dispatch_candidates(&spec, &states).unwrap();

        assert!(
            candidates.contains("b"),
            "b's predecessor succeeded, so this is an ordinary retry and must dispatch: \
             {candidates:?}"
        );
    }

    /// THE THIRD DOOR. A predecessor can stop satisfying its dependents AFTER the dependent was
    /// queued: `(Succeeded, Invalidated) => Invalidated` (`transition.rs:60`) reopens a completed
    /// node.
    ///
    /// Pinned because it exists nowhere else. It is not reachable through the pause/resume path
    /// #80 reported, so a fix aimed only at that path would leave it open — and nothing in the
    /// tree would notice.
    #[test]
    fn a_queued_node_whose_predecessor_was_invalidated_is_not_a_candidate() {
        let spec = spec(&["a", "b"], &[("a", "b")]);
        let states = states(&[("a", NodeState::Invalidated), ("b", NodeState::Queued)]);

        let candidates = dispatch_candidates(&spec, &states).unwrap();

        assert!(
            !candidates.contains("b"),
            "a was invalidated after b queued, so b's dependency is unmet again: {candidates:?}"
        );
    }

    /// A `Queued` node with no incoming edges has nothing to wait for. Guards the gate against
    /// the opposite error — an edge rule that accidentally excludes roots would stall every
    /// entrypoint retry, and the three tests above would all still pass.
    #[test]
    fn a_queued_root_node_is_always_a_candidate() {
        let spec = spec(&["a", "b"], &[("a", "b")]);
        let states = states(&[("a", NodeState::Queued), ("b", NodeState::Draft)]);

        let candidates = dispatch_candidates(&spec, &states).unwrap();

        assert!(
            candidates.contains("a"),
            "a has no predecessors, so no edge can gate it: {candidates:?}"
        );
    }

    /// The union must not lose the half that was already correct: a `Ready` node with satisfied
    /// edges is still a candidate. Pins that the fix touched the retry chain only.
    #[test]
    fn the_ready_half_of_the_union_is_unchanged() {
        let spec = spec(&["a", "b"], &[("a", "b")]);
        let states = states(&[("a", NodeState::Succeeded), ("b", NodeState::Ready)]);

        let candidates = dispatch_candidates(&spec, &states).unwrap();

        assert!(
            candidates.contains("b"),
            "b is ready with its edge satisfied — the ready_set half is untouched: {candidates:?}"
        );
        assert_eq!(
            candidates,
            ready_set(&spec, &states).unwrap(),
            "with no node queued, the candidate set IS the ready set"
        );
    }

    /// A `Queued` failure handler whose source FAILED must dispatch.
    ///
    /// The sharpest guard against over-tightening, and the reason the union calls `edge_gates`
    /// rather than asking a question of its own. Every other test here would still pass if the
    /// rule were written as "queued dispatches when its predecessor SUCCEEDED" — that phrasing is
    /// the obvious one, it reads correctly, and it silently stops every retry of every failure
    /// route in the system, because a failure handler's source is precisely the thing that did not
    /// succeed. `edge_gates` already knows this (`EdgeType::Failure` releases on `Failed` and only
    /// `Failed`); this pins that the retry chain inherits that knowledge instead of paraphrasing it.
    #[test]
    fn a_queued_failure_handler_dispatches_when_its_source_failed() {
        let mut spec = spec(&["a", "handler"], &[("a", "handler")]);
        spec.edges[0].edge_type = EdgeType::Failure;
        let states = states(&[("a", NodeState::Failed), ("handler", NodeState::Queued)]);

        let candidates = dispatch_candidates(&spec, &states).unwrap();

        assert!(
            candidates.contains("handler"),
            "a failure route releases exactly when its source failed; gating this one would \
             strand every failure handler that ever retries: {candidates:?}"
        );
    }

    /// The same edge, the other way. A failure handler whose source SUCCEEDED must not dispatch —
    /// nothing failed, so there is nothing to handle.
    ///
    /// Paired with the test above so neither direction can be satisfied by a constant. Together
    /// they also prove the union consults the edge's TYPE: a rule that only asked "is the
    /// predecessor terminal" would pass the first and fail this one.
    #[test]
    fn a_queued_failure_handler_is_not_a_candidate_when_its_source_succeeded() {
        let mut spec = spec(&["a", "handler"], &[("a", "handler")]);
        spec.edges[0].edge_type = EdgeType::Failure;
        let states = states(&[("a", NodeState::Succeeded), ("handler", NodeState::Queued)]);

        let candidates = dispatch_candidates(&spec, &states).unwrap();

        assert!(
            !candidates.contains("handler"),
            "nothing failed, so the failure route has nothing to run: {candidates:?}"
        );
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
        for state in ALL_STATES.iter().copied() {
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
        for state in ALL_STATES.iter().copied() {
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
