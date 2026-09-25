use graphhelm_protocols::{Diagnostic, ExecutionGraph, NodeType};

use super::{error, escape};

pub(super) fn check(graph: &ExecutionGraph, source: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (id, node) in &graph.spec.nodes {
        let base = format!("/spec/nodes/{}", escape(id));
        if node.node_type == NodeType::Deploy
            && !node
                .properties
                .get("targetRef")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
        {
            diagnostics.push(error(
                "GHG009_DEPLOY_TARGET_MISSING",
                "deploy node requires a non-empty targetRef",
                format!("{base}/targetRef"),
                source,
            ));
        }
        if let Some(effects) = node
            .properties
            .get("effects")
            .and_then(|value| value.as_object())
        {
            let requires_compensation = effects
                .get("reversible")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
                || effects
                    .get("compensationRequired")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
            let compensation = effects
                .get("compensationNode")
                .and_then(serde_json::Value::as_str);
            if requires_compensation
                && !compensation.is_some_and(|target| {
                    graph
                        .spec
                        .nodes
                        .get(target)
                        .is_some_and(|node| node.node_type == NodeType::Rollback)
                })
            {
                diagnostics.push(error(
                    "GHG010_COMPENSATION_MISSING",
                    "reversible effect requires an existing rollback compensation node",
                    format!("{base}/effects/compensationNode"),
                    source,
                ));
            }
        }
    }
    diagnostics
}
