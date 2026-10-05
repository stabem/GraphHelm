//! #290 (ADR-040): a node may declare which kind of subagent takes it.
//!
//! Observed against the LIVE validator, for the reason `node_agents.rs` gives: `node` carries
//! `additionalProperties: true`, so the frozen 1.0.0 validator accepts any `delegation` key at all,
//! and a refusal fixture in the shared manifest would read as the live validator invalidating
//! history.

use serde_json::{Value, json};

fn graph_with_node(node: Value) -> Value {
    json!({
        "apiVersion": "p50.dev/graph/v1",
        "kind": "ExecutionGraph",
        "metadata": {
            "id": "graph-delegation",
            "name": "Delegation",
            "executionId": "execution-delegation",
            "version": 1
        },
        "spec": {
            "entrypoints": ["start"],
            "nodes": {"start": node},
            "edges": [],
            "budgets": {},
            "completion": {"status": "complete"}
        }
    })
}

fn agent_node(delegation: Option<Value>) -> Value {
    let mut node = json!({
        "type": "agent",
        "name": "Ship the change",
        "objective": "Implement the change",
        "optionality": "required",
        "agent": {"ref": "agent://primary"}
    });
    if let Some(delegation) = delegation {
        node["delegation"] = delegation;
    }
    node
}

fn refusals(node: Value) -> Vec<(String, String)> {
    graphhelm_schema::validate_graph_value(&graph_with_node(node), "node_delegation")
        .into_iter()
        .map(|diagnostic| (diagnostic.code, diagnostic.path))
        .collect()
}

/// Refused, and refused AT the delegation pointer (or below it), so the refusal is about this
/// field and not about some other part of the fixture.
fn refused_at_delegation(node: Value) -> bool {
    refusals(node).iter().any(|(code, path)| {
        code == "GHS002_SCHEMA" && path.starts_with("/spec/nodes/start/delegation")
    })
}

#[test]
fn every_declared_kind_validates_and_absence_is_unchanged() {
    assert_eq!(refusals(agent_node(None)), Vec::new());
    for kind in ["explorer", "implementer", "reviewer", "verifier"] {
        assert_eq!(
            refusals(agent_node(Some(json!({"kind": kind})))),
            Vec::new(),
            "{kind} was refused"
        );
    }
}

#[test]
fn an_unknown_kind_is_refused() {
    assert!(refused_at_delegation(agent_node(Some(
        json!({"kind": "planner"})
    ))));
    assert!(refused_at_delegation(agent_node(Some(
        json!({"kind": "Implementer"})
    ))));
}

#[test]
fn an_extra_field_is_refused_rather_than_ignored() {
    assert!(refused_at_delegation(agent_node(Some(
        json!({"kind": "implementer", "tier": "large"})
    ))));
}

#[test]
fn a_delegation_without_a_kind_or_of_the_wrong_shape_is_refused() {
    assert!(refused_at_delegation(agent_node(Some(json!({})))));
    assert!(refused_at_delegation(agent_node(Some(json!(
        "implementer"
    )))));
}
