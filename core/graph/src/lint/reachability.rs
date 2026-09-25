use std::collections::{BTreeMap, BTreeSet, VecDeque};

use graphhelm_protocols::{Diagnostic, ExecutionGraph};

use super::{error, escape};

pub(super) fn check(graph: &ExecutionGraph, source: &str) -> Vec<Diagnostic> {
    let mut outgoing: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut incoming: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for edge in &graph.spec.edges {
        if graph.spec.nodes.contains_key(&edge.from) && graph.spec.nodes.contains_key(&edge.to) {
            outgoing.entry(&edge.from).or_default().push(&edge.to);
            incoming.entry(&edge.to).or_default().push(&edge.from);
        }
    }
    let reachable = traverse(
        graph
            .spec
            .entrypoints
            .iter()
            .filter(|id| graph.spec.nodes.contains_key(*id))
            .map(String::as_str),
        &outgoing,
    );
    let declared_terminals = graph
        .spec
        .completion
        .get("terminalNodes")
        .and_then(serde_json::Value::as_array);
    let terminals: Vec<&str> = if let Some(values) = declared_terminals {
        values
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect()
    } else {
        graph
            .spec
            .nodes
            .keys()
            .filter(|id| outgoing.get(id.as_str()).is_none_or(Vec::is_empty))
            .map(String::as_str)
            .collect()
    };
    let can_finish = traverse(terminals, &incoming);
    reachable
        .difference(&can_finish)
        .map(|id| {
            error(
                "GHG005_NO_TERMINAL_PATH",
                "reachable node has no path to a terminal node",
                format!("/spec/nodes/{}", escape(id)),
                source,
            )
        })
        .collect()
}

fn traverse<'a>(
    starts: impl IntoIterator<Item = &'a str>,
    edges: &BTreeMap<&'a str, Vec<&'a str>>,
) -> BTreeSet<&'a str> {
    let mut visited = BTreeSet::new();
    let mut queue: VecDeque<_> = starts.into_iter().collect();
    while let Some(node) = queue.pop_front() {
        if visited.insert(node) {
            queue.extend(edges.get(node).into_iter().flatten().copied());
        }
    }
    visited
}
