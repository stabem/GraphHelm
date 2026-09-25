use std::collections::{BTreeMap, BTreeSet};

use graphhelm_protocols::{Diagnostic, ExecutionGraph};

use super::{error, escape};

pub(super) fn check(graph: &ExecutionGraph, source: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for component in components(graph) {
        let node = component[0];
        let self_loop = graph
            .spec
            .edges
            .iter()
            .any(|edge| edge.from == node && edge.to == node);
        if component.len() > 1 || self_loop {
            let controlled = component.iter().any(|id| {
                graph.spec.nodes[*id]
                    .properties
                    .get("loop")
                    .and_then(|value| value.get("maxIterations"))
                    .and_then(serde_json::Value::as_u64)
                    .is_some_and(|limit| limit > 0)
            });
            if !controlled {
                diagnostics.extend(component.into_iter().map(|id| {
                    error(
                        "GHG006_UNCONTROLLED_CYCLE",
                        "cyclic component requires a positive loop.maxIterations",
                        format!("/spec/nodes/{}/loop", escape(id)),
                        source,
                    )
                }));
            }
        }
    }
    diagnostics
}

pub(super) fn components(graph: &ExecutionGraph) -> Vec<Vec<&str>> {
    let mut outgoing: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut incoming: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for edge in &graph.spec.edges {
        if graph.spec.nodes.contains_key(&edge.from) && graph.spec.nodes.contains_key(&edge.to) {
            outgoing.entry(&edge.from).or_default().push(&edge.to);
            incoming.entry(&edge.to).or_default().push(&edge.from);
        }
    }
    let mut visited = BTreeSet::new();
    let mut order = Vec::new();
    for start in graph.spec.nodes.keys().map(String::as_str) {
        if visited.contains(start) {
            continue;
        }
        let mut stack = vec![(start, false)];
        while let Some((node, expanded)) = stack.pop() {
            if expanded {
                order.push(node);
            } else if visited.insert(node) {
                stack.push((node, true));
                for next in outgoing.get(node).into_iter().flatten().rev() {
                    if !visited.contains(next) {
                        stack.push((next, false));
                    }
                }
            }
        }
    }
    visited.clear();
    let mut result = Vec::new();
    while let Some(start) = order.pop() {
        if visited.contains(start) {
            continue;
        }
        let mut component = Vec::new();
        let mut stack = vec![start];
        while let Some(node) = stack.pop() {
            if visited.insert(node) {
                component.push(node);
                stack.extend(incoming.get(node).into_iter().flatten().copied());
            }
        }
        component.sort_unstable();
        result.push(component);
    }
    result.sort_by(|left, right| left[0].cmp(right[0]));
    result
}
