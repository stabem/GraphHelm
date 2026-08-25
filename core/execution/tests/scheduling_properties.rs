use std::collections::BTreeMap;

use graphhelm_execution::{ScheduleError, ready_set};
use graphhelm_protocols::{
    EdgeType, GraphEdge, GraphNode, GraphSpec, NodeState, NodeType, Optionality,
};
use proptest::prelude::*;

const STATES: &[NodeState] = NodeState::every();

fn agent_node() -> GraphNode {
    GraphNode {
        node_type: NodeType::Agent,
        name: "n".to_owned(),
        objective: "o".to_owned(),
        optionality: Optionality::Required,
        properties: BTreeMap::new(),
    }
}

/// A chain of eight nodes, each depending on the one before it.
fn chain() -> GraphSpec {
    let mut spec = GraphSpec {
        entrypoints: vec!["n0".to_owned()],
        nodes: BTreeMap::new(),
        edges: Vec::new(),
        budgets: Default::default(),
        policies: Vec::new(),
        completion: serde_json::Value::Null,
    };
    for index in 0..8 {
        spec.nodes.insert(format!("n{index}"), agent_node());
        if index > 0 {
            spec.edges.push(GraphEdge {
                id: format!("e{index}"),
                from: format!("n{}", index - 1),
                to: format!("n{index}"),
                edge_type: EdgeType::Control,
                payload_schema: None,
                condition: None,
                on_false: None,
                on_unknown: None,
                bindings: BTreeMap::new(),
                priority: None,
            });
        }
    }
    spec
}

fn assignment(seeds: Vec<usize>) -> BTreeMap<String, NodeState> {
    seeds
        .into_iter()
        .enumerate()
        .map(|(index, seed)| (format!("n{index}"), STATES[seed % STATES.len()]))
        .collect()
}

proptest! {
    /// The same graph and the same states always yield the same set. Any ordering or iteration
    /// dependence here would make a replayed execution schedule differently from the original.
    #[test]
    fn scheduling_is_deterministic(seeds in prop::collection::vec(0usize..64, 0..8)) {
        let spec = chain();
        let states = assignment(seeds);
        prop_assert_eq!(ready_set(&spec, &states), ready_set(&spec, &states));
    }

    /// A ghost is never dispatched, at any assignment of every other node's state.
    #[test]
    fn a_ghost_is_never_scheduled(seeds in prop::collection::vec(0usize..64, 0..8)) {
        let spec = chain();
        let mut states = assignment(seeds);
        states.insert("n3".to_owned(), NodeState::Ghost);
        if let Ok(ready) = ready_set(&spec, &states) {
            prop_assert!(!ready.contains("n3"), "a ghost was scheduled");
        }
    }

    /// Never more than the bound, and never a truncated success.
    #[test]
    fn the_ready_set_is_bounded_or_blocks(seeds in prop::collection::vec(0usize..64, 0..8)) {
        let spec = chain();
        match ready_set(&spec, &assignment(seeds)) {
            Ok(ready) => prop_assert!(ready.len() <= graphhelm_execution::MAX_READY_SET),
            Err(ScheduleError::ReadySetTooLarge) => {}
        }
    }

    /// A node is only ever ready when every predecessor is satisfied. This is the safety property:
    /// nothing runs before what it depends on.
    #[test]
    fn nothing_is_ready_before_its_predecessor(seeds in prop::collection::vec(0usize..64, 0..8)) {
        let spec = chain();
        let states = assignment(seeds);
        if let Ok(ready) = ready_set(&spec, &states) {
            for node in &ready {
                let index: usize = node.trim_start_matches('n').parse().unwrap();
                if index > 0 {
                    let predecessor = states
                        .get(&format!("n{}", index - 1))
                        .copied()
                        .unwrap_or(NodeState::Draft);
                    prop_assert!(
                        matches!(
                            predecessor,
                            NodeState::Succeeded | NodeState::Waived | NodeState::Skipped
                        ),
                        "{node} was ready with predecessor {predecessor:?}"
                    );
                }
            }
        }
    }
}

// ---- Task 3 (05d): edge-aware readiness for the decidable subset ----

/// A two-node spec with one configurable edge a→b.
fn pair_spec(edge_type: EdgeType, condition: Option<serde_json::Value>) -> GraphSpec {
    let mut spec = GraphSpec {
        entrypoints: vec!["a".to_owned()],
        nodes: BTreeMap::new(),
        edges: Vec::new(),
        budgets: Default::default(),
        policies: Vec::new(),
        completion: serde_json::Value::Null,
    };
    spec.nodes.insert("a".to_owned(), agent_node());
    spec.nodes.insert("b".to_owned(), agent_node());
    spec.edges.push(GraphEdge {
        id: "e0".to_owned(),
        from: "a".to_owned(),
        to: "b".to_owned(),
        edge_type,
        payload_schema: None,
        condition,
        on_false: None,
        on_unknown: None,
        bindings: BTreeMap::new(),
        priority: None,
    });
    spec
}

fn pair_states(a: NodeState) -> BTreeMap<String, NodeState> {
    let mut states = BTreeMap::new();
    states.insert("a".to_owned(), a);
    states.insert("b".to_owned(), NodeState::Ready);
    states
}

#[test]
fn a_literal_false_condition_ungates_its_edge() {
    // 04c gated b on a unconditionally; a literally-false condition is statically dead — the
    // edge does not gate. A json!(true) condition gates exactly as before, and a non-literal
    // condition (string, object) stays FAIL-CLOSED: gate as if unconditioned — execution
    // evaluates only the simulator's deterministic literal subset, minus fixtures.
    let dead = pair_spec(EdgeType::Control, Some(serde_json::json!(false)));
    let ready = ready_set(&dead, &pair_states(NodeState::Running)).unwrap();
    assert!(ready.contains("b"), "a dead edge must not gate");
    assert!(!ready.contains("a"), "a is untouched");

    for gating in [
        Some(serde_json::json!(true)),
        Some(serde_json::json!("false")),
        Some(serde_json::json!({ "op": "eq" })),
        None,
    ] {
        let spec = pair_spec(EdgeType::Control, gating.clone());
        let held = ready_set(&spec, &pair_states(NodeState::Running)).unwrap();
        assert!(!held.contains("b"), "{gating:?} must gate like 04c");
        let released = ready_set(&spec, &pair_states(NodeState::Succeeded)).unwrap();
        assert!(released.contains("b"), "{gating:?} must release like 04c");
    }
}

#[test]
fn a_failure_edge_releases_on_failed_and_blocks_otherwise() {
    // Both deltas of the refinement are deliberate, and this test pins each: a failure route
    // RELEASES on Failed (that is what a failure route is — new readiness 04c never granted),
    // and it does NOT release on Succeeded/Waived/Skipped (04c's every-edge rule would have —
    // spurious handler work removed). Nothing failed in the waive/skip cases.
    let spec = pair_spec(EdgeType::Failure, None);
    let released = ready_set(&spec, &pair_states(NodeState::Failed)).unwrap();
    assert!(released.contains("b"), "a failure handler runs on Failed");
    for not_failed in [
        NodeState::Succeeded,
        NodeState::Waived,
        NodeState::Skipped,
        NodeState::Running,
        NodeState::Blocked,
    ] {
        let held = ready_set(&spec, &pair_states(not_failed)).unwrap();
        assert!(
            !held.contains("b"),
            "{not_failed:?} must not release a failure edge"
        );
    }
}

proptest! {
    /// For graphs whose edges are all non-Failure with no literal-false conditions, the new
    /// ready_set equals the 04c rule's output across randomized states — the refinement is
    /// additive on exactly the two named cases. The 04c oracle is re-stated inline: every
    /// incoming edge's source must be Succeeded/Waived/Skipped.
    #[test]
    fn every_other_shape_is_exactly_the_04c_rule(
        state_indices in proptest::collection::vec(0_usize..16, 8),
        edge_kinds in proptest::collection::vec(0_usize..3, 7),
    ) {
        let mut spec = chain();
        for (edge, kind) in spec.edges.iter_mut().zip(edge_kinds) {
            edge.edge_type = match kind {
                0 => EdgeType::Control,
                1 => EdgeType::Data,
                _ => EdgeType::Evidence,
            };
            // Conditions in the pool are gating shapes only (true / non-literal), never the
            // literal false this property excludes by construction.
            edge.condition = match kind {
                0 => None,
                1 => Some(serde_json::json!(true)),
                _ => Some(serde_json::json!("dynamic")),
            };
        }
        let states: BTreeMap<String, NodeState> = (0..8)
            .map(|index| (format!("n{index}"), STATES[state_indices[index]]))
            .collect();

        let new_rule = ready_set(&spec, &states).unwrap();
        let mut oracle = std::collections::BTreeSet::new();
        for (index, node) in (0..8).map(|i| (i, format!("n{i}"))) {
            if states[&node] != NodeState::Ready {
                continue;
            }
            let satisfied = index == 0
                || matches!(
                    states[&format!("n{}", index - 1)],
                    NodeState::Succeeded | NodeState::Waived | NodeState::Skipped
                );
            if satisfied {
                oracle.insert(node);
            }
        }
        prop_assert_eq!(new_rule, oracle);
    }
}
