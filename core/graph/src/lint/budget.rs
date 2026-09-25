use std::collections::{BTreeMap, BTreeSet, VecDeque};

use graphhelm_protocols::{Diagnostic, ExecutionGraph};

use super::{error, escape};

pub(super) fn check(graph: &ExecutionGraph, source: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    if graph
        .spec
        .budgets
        .max_nodes
        .is_some_and(|limit| graph.spec.nodes.len() as u64 > limit)
    {
        diagnostics.push(error(
            "GHG011_NODE_BUDGET_EXCEEDED",
            "graph node count exceeds maxNodes",
            "/spec/budgets/maxNodes",
            source,
        ));
    }
    if graph
        .spec
        .budgets
        .max_depth
        .is_some_and(|limit| longest_acyclic_depth(graph) > limit)
    {
        diagnostics.push(error(
            "GHG012_DEPTH_BUDGET_EXCEEDED",
            "graph depth exceeds maxDepth",
            "/spec/budgets/maxDepth",
            source,
        ));
    }
    if let Some(retries) = graph.spec.budgets.max_retries_per_node {
        for (id, node) in &graph.spec.nodes {
            if node
                .properties
                .get("retry")
                .and_then(|value| value.get("maxAttempts"))
                .and_then(serde_json::Value::as_u64)
                .is_some_and(|attempts| attempts > retries + 1)
            {
                diagnostics.push(error(
                    "GHG013_RETRY_BUDGET_EXCEEDED",
                    "node attempts exceed the graph retry budget",
                    format!("/spec/nodes/{}/retry/maxAttempts", escape(id)),
                    source,
                ));
            }
        }
    }
    diagnostics
}

fn longest_acyclic_depth(graph: &ExecutionGraph) -> u64 {
    let components = super::cycle::components(graph);
    let mut component_by_node = BTreeMap::new();
    for (index, component) in components.iter().enumerate() {
        for node in component {
            component_by_node.insert(*node, index);
        }
    }
    let mut indegree = vec![0_u64; components.len()];
    let mut outgoing = vec![BTreeSet::new(); components.len()];
    for edge in &graph.spec.edges {
        if let (Some(&from), Some(&to)) = (
            component_by_node.get(edge.from.as_str()),
            component_by_node.get(edge.to.as_str()),
        ) && from != to
            && outgoing[from].insert(to)
        {
            indegree[to] += 1;
        }
    }
    let mut queue: VecDeque<_> = indegree
        .iter()
        .enumerate()
        .filter_map(|(id, degree)| (*degree == 0).then_some(id))
        .collect();
    let mut depths = vec![1_u64; components.len()];
    let mut maximum = u64::from(!graph.spec.nodes.is_empty());
    while let Some(component) = queue.pop_front() {
        let depth = depths[component];
        maximum = maximum.max(depth);
        for &target in &outgoing[component] {
            depths[target] = depths[target].max(depth + 1);
            indegree[target] -= 1;
            if indegree[target] == 0 {
                queue.push_back(target);
            }
        }
    }
    maximum
}
