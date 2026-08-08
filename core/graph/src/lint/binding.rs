use graphhelm_protocols::{Diagnostic, ExecutionGraph};

use super::{error, escape};

pub(super) fn check(graph: &ExecutionGraph, source: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (index, edge) in graph.spec.edges.iter().enumerate() {
        for (target, binding) in &edge.bindings {
            check_binding(
                graph,
                binding,
                format!("/spec/edges/{index}/map/{}", escape(target)),
                source,
                &mut diagnostics,
            );
        }
    }
    for (id, node) in &graph.spec.nodes {
        if let Some(bindings) = node
            .properties
            .get("input")
            .and_then(|value| value.get("bindings"))
            .and_then(serde_json::Value::as_object)
        {
            for (target, binding) in bindings {
                if let Some(binding) = binding.as_str() {
                    check_binding(
                        graph,
                        binding,
                        format!(
                            "/spec/nodes/{}/input/bindings/{}",
                            escape(id),
                            escape(target)
                        ),
                        source,
                        &mut diagnostics,
                    );
                }
            }
        }
    }
    diagnostics
}

fn check_binding(
    graph: &ExecutionGraph,
    binding: &str,
    path: String,
    source: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let referenced = binding
        .strip_prefix("outputs.")
        .and_then(|rest| rest.split_once('.').map(|(node, _)| node))
        .or_else(|| {
            binding
                .strip_prefix("nodes.")
                .and_then(|rest| rest.split_once(".output").map(|(node, _)| node))
        });
    if referenced.is_some_and(|node| !graph.spec.nodes.contains_key(node)) {
        diagnostics.push(error(
            "GHG007_BINDING_SOURCE_UNKNOWN",
            "binding references an unknown source node",
            path,
            source,
        ));
    }
}
