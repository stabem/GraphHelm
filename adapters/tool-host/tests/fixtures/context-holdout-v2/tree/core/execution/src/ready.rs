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

/// #134: why a dispatch view could not be derived. Each variant is a fact about the execution or
/// the caller, never about the tree, and each has one sentence for the wire so no surface invents
/// its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispatchUnavailable {
    /// The caller did not hold the graph; without edges there is no gate to read.
    NoGraph,
    /// The execution is paused: both drivers suppress every dispatch until resume, so a count of
    /// "ready to dispatch" above zero would over-promise exactly the way `nodeStateCounts.ready`
    /// did.
    Paused,
    /// More nodes are dispatchable at once than the execution may dispatch; the driver blocks for
    /// an owner decision there, and so does this view.
    ReadySetTooLarge,
}

impl DispatchUnavailable {
    /// The wire sentence, owned here so the CLI and any other consumer say the same thing.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::NoGraph => {
                "this command did not have the graph; `execution status --file <graph>` derives it"
            }
            Self::Paused => "the execution is paused; the driver dispatches nothing until resume",
            Self::ReadySetTooLarge => {
                "the ready set exceeds the dispatch bound; the driver blocks for an owner decision before any of it moves"
            }
        }
    }
}

/// #134: the dispatch gate beside the state. `ready` is the Ready half of the driver's candidate
/// set -- Ready nodes whose predecessors are satisfied, by the same `ready_set` the driver
/// consults -- and `gated` names the Ready nodes of the ACTIVE graph that are not. The driver's
/// full answer is `dispatch_candidates`, which adds the retry-pending `Queued` nodes; those are
/// not this view's subject, which is `Ready`, the state the vocabulary over-promised.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DispatchView {
    /// Nodes the driver would dispatch NOW: Ready, predecessors satisfied, and inside the
    /// capacity `dispatch_plan` leaves after the in-flight nodes are counted.
    ///
    /// THE LIMIT, stated once (Codex on #1014): this view is a function of PERSISTED state -- the
    /// spec, the folded node states, the attempts, the aggregate status. The Runtime driver also
    /// keeps a private `refused` set for nodes `build_work` would not dispatch (an unsupported node
    /// type, a gate whose certification is missing or stale) and leaves such a node `Ready`; that
    /// set lives in the driver's memory and reaches no event, so no reader of the stream -- this
    /// view, `status`, the monitor -- can see it. A node the Runtime has refused is therefore
    /// counted here as ready. The remedy is a persisted refusal, a Runtime change outside this
    /// crate; until then "ready" means "the stream says nothing stops it".
    pub ready: usize,
    /// Ready nodes of the active graph whose predecessors are not satisfied.
    pub gated: Vec<String>,
    /// Ready nodes whose predecessors ARE satisfied but which `max_parallel` holds back behind
    /// the in-flight ones (Codex on #1014): eligible, not dispatchable now. Named rather than
    /// folded into `ready`, because "ready to dispatch" on the wire means what it means to the
    /// driver, and the driver's answer is the plan, not the edge predicate alone.
    pub waiting_capacity: Vec<String>,
}

/// Derives the dispatch view, with the policy that used to live in the CLI (Codex on #1014):
/// a paused execution has no view; an absent graph has no view; the bound is the driver's bound;
/// `gated` is intersected with the active spec's node set, because `node_states` is folded from
/// the whole history and a node a later publication removed keeps its historical `Ready` there.
///
/// Pure over its arguments, like everything else in this module, so a replayed execution reads
/// the same view the original did.
pub fn dispatch_view(
    spec: Option<&GraphSpec>,
    states: &BTreeMap<String, NodeState>,
    attempts: &BTreeMap<String, u32>,
    status: Option<&graphhelm_protocols::SimulationStatus>,
) -> Result<DispatchView, DispatchUnavailable> {
    if matches!(status, Some(graphhelm_protocols::SimulationStatus::Paused)) {
        return Err(DispatchUnavailable::Paused);
    }
    let Some(spec) = spec else {
        return Err(DispatchUnavailable::NoGraph);
    };
    // THE READY HALF, which is this view's SUBJECT (see `DispatchView`) -- reported, not planned.
    let dispatchable = match ready_set(spec, states) {
        Ok(set) => set,
        Err(ScheduleError::ReadySetTooLarge) => return Err(DispatchUnavailable::ReadySetTooLarge),
    };
    // THE WHOLE CANDIDATE SET, which is what CAPACITY is spent on (Codex on #1014). Both drivers
    // plan over `dispatch_candidates`, so an edge-ready `Queued` retry competes for the same slots.
    // Planning over the Ready half alone over-reported `ready`: with capacity one and a queued
    // candidate sorting first, the driver gives the slot to the retry while this view still counted
    // the Ready node as dispatching now.
    //
    // The view's SUBJECT does not widen -- `Queued` nodes are still absent from every field. Only
    // the plan they are weighed against does, which is the difference between "what is Ready" and
    // "what will actually move".
    let candidates = match dispatch_candidates(spec, states) {
        Ok(set) => set,
        Err(ScheduleError::ReadySetTooLarge) => return Err(DispatchUnavailable::ReadySetTooLarge),
    };
    let gated: Vec<String> = states
        .iter()
        .filter(|(node, state)| {
            **state == NodeState::Ready
                && spec.nodes.contains_key(node.as_str())
                && !dispatchable.contains(node.as_str())
        })
        .map(|(node, _)| node.clone())
        .collect();
    // CAPACITY, the way both drivers apply it: the in-flight count is the `Running` nodes, the
    // limit is `parallel_limit(&spec.budgets)`, and `dispatch_plan` takes the attempt-fair prefix
    // that fits. A zero limit cannot progress and the driver refuses it; here it reads as the
    // bound, since the operator's question is "what moves now" and the answer is nothing.
    let in_flight = states
        .values()
        .filter(|state| **state == NodeState::Running)
        .count();
    let max_parallel = crate::parallel_limit(&spec.budgets);
    // A zero limit is `ZeroParallelism`, the one error the planner has: nothing moves, and the
    // driver refuses the execution; here the empty plan says the same.
    let planned =
        crate::dispatch_plan(&candidates, attempts, in_flight, max_parallel).unwrap_or_default();
    // Classified back down to the Ready half AFTER the plan, exactly as the thread asked: the plan
    // decides who moves, and this view reports the Ready members of that real answer.
    let ready = planned
        .iter()
        .filter(|node| dispatchable.contains(node.as_str()))
        .count();
    let waiting_capacity: Vec<String> = dispatchable
        .iter()
        .filter(|node| !planned.contains(node))
        .cloned()
        .collect();
    Ok(DispatchView {
        ready,
        gated,
        waiting_capacity,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #134: a node a later publication removed keeps its historical `Ready` in the folded
    /// states; it is neither dispatchable nor gated, because it is not in the graph. The premise
    /// lives in `core/events/src/projection.rs`: the fold only ever INSERTS into `node_states`
    /// (two sites) and `GraphVersionPublished` touches it not at all -- measured 2026-09-08, two
    /// inserts, zero removals. If a later fold prunes on publication, this synthesised state
    /// becomes unreachable and the cell should say so rather than keep passing.
    #[test]
    fn dispatch_view_ignores_a_node_the_active_graph_no_longer_has() {
        let spec = spec(&["a", "b"], &[("a", "b")]);
        let states = states(&[
            ("a", NodeState::Ready),
            ("b", NodeState::Ready),
            ("removed-by-a-later-publication", NodeState::Ready),
        ]);
        let view = dispatch_view(Some(&spec), &states, &BTreeMap::new(), None).unwrap();
        assert_eq!(view.ready, 1, "a has no predecessor");
        assert_eq!(
            view.gated,
            vec!["b".to_owned()],
            "b waits on a; the removed node is in neither"
        );
    }

    /// #134: capacity is the driver's, not the edge predicate's (Codex on #1014). With
    /// `max_parallel_model_calls: 1` and one node Running, an independent Ready node is
    /// edge-ready and still goes nowhere until capacity frees: `ready` is 0 and the node is named
    /// under `waiting_capacity`, never counted as dispatchable.
    #[test]
    fn dispatch_view_holds_an_edge_ready_node_behind_a_full_capacity() {
        let mut spec = spec(&["a", "b"], &[]);
        spec.budgets.max_parallel_model_calls = Some(1);
        let held = states(&[("a", NodeState::Running), ("b", NodeState::Ready)]);
        let view = dispatch_view(Some(&spec), &held, &BTreeMap::new(), None).unwrap();
        assert_eq!(view.ready, 0, "capacity 1 is spent on a: {view:?}");
        assert_eq!(view.waiting_capacity, vec!["b".to_owned()]);
        assert!(view.gated.is_empty());

        // CONTROL: the same graph with a finished a -- capacity is free and b is dispatchable.
        let freed = states(&[("a", NodeState::Succeeded), ("b", NodeState::Ready)]);
        let view = dispatch_view(Some(&spec), &freed, &BTreeMap::new(), None).unwrap();
        assert_eq!(view.ready, 1);
        assert!(view.waiting_capacity.is_empty());
    }

    /// #1014: capacity is spent by the DRIVER'S candidate set, which is `dispatch_candidates` --
    /// the Ready nodes plus the edge-ready `Queued` retries -- not by the Ready half alone.
    ///
    /// Both drivers plan over that union. This view planned over `ready_set`, so with capacity one
    /// and a queued retry sorting first, the driver handed its only slot to the retry while this
    /// view still reported the Ready node as dispatching now. `a` is a `Queued` root, always a
    /// candidate; equal attempts make `dispatch_plan` order by name, so `a` takes the slot.
    ///
    /// The view's subject does not widen: `a` appears in no field. What changed is that `b` is
    /// weighed against the plan that will really run.
    #[test]
    fn dispatch_view_spends_capacity_on_the_queued_retries_the_driver_also_plans() {
        let mut spec = spec(&["a", "b"], &[]);
        spec.budgets.max_parallel_model_calls = Some(1);
        let contended = states(&[("a", NodeState::Queued), ("b", NodeState::Ready)]);
        let view = dispatch_view(Some(&spec), &contended, &BTreeMap::new(), None).unwrap();
        assert_eq!(
            view.ready, 0,
            "the queued retry a takes the only slot, so no Ready node dispatches now: {view:?}"
        );
        assert_eq!(
            view.waiting_capacity,
            vec!["b".to_owned()],
            "b is eligible and held, and it is the Ready half that is reported"
        );
        assert!(
            !view.gated.contains(&"a".to_owned()),
            "a is Queued, not a Ready node of the active graph, so it is in no field of this view"
        );

        // CONTROL: the same graph with a finished a -- nothing competes, and b dispatches. Without
        // this arm the assertion above would also pass for a view that simply never counts anyone.
        let freed = states(&[("a", NodeState::Succeeded), ("b", NodeState::Ready)]);
        let view = dispatch_view(Some(&spec), &freed, &BTreeMap::new(), None).unwrap();
        assert_eq!(view.ready, 1);
        assert!(view.waiting_capacity.is_empty());
    }

    /// #134: a paused execution dispatches nothing, however Ready its nodes are -- and an
    /// execution started held (an `ExecutionPaused` in the same request as the start) has no node
    /// states at all; both are `Paused`, never a zero-of-zero that reads as live authority.
    #[test]
    fn dispatch_view_has_no_answer_for_a_paused_execution() {
        use graphhelm_protocols::SimulationStatus;
        let spec = spec(&["a"], &[]);
        let states = states(&[("a", NodeState::Ready)]);
        let running = dispatch_view(
            Some(&spec),
            &states,
            &BTreeMap::new(),
            Some(&SimulationStatus::Running),
        )
        .unwrap();
        assert_eq!(running.ready, 1, "CONTROL: running, a is dispatchable");
        assert_eq!(
            dispatch_view(
                Some(&spec),
                &states,
                &BTreeMap::new(),
                Some(&SimulationStatus::Paused)
            ),
            Err(DispatchUnavailable::Paused)
        );
        assert_eq!(
            dispatch_view(
                Some(&spec),
                &BTreeMap::new(),
                &BTreeMap::new(),
                Some(&SimulationStatus::Paused)
            ),
            Err(DispatchUnavailable::Paused),
            "paused with no node states is not zero-of-zero"
        );
        assert_eq!(
            dispatch_view(None, &states, &BTreeMap::new(), None),
            Err(DispatchUnavailable::NoGraph)
        );
        assert!(DispatchUnavailable::Paused.reason().contains("paused"));
    }
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
