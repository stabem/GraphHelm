use std::collections::BTreeMap;

use graphhelm_execution::{ScheduleError, ready_set};
use graphhelm_protocols::{
    EdgeType, GraphEdge, GraphNode, GraphSpec, NodeState, NodeType, Optionality,
};
use proptest::prelude::*;

const STATES: [NodeState; 16] = [
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
