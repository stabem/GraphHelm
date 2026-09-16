//! #1049: a node is the TASK, so more than one agent may work it.
//!
//! The acceptance of the crew form, and every refusal that keeps it honest, observed against the
//! LIVE validator alone. That boundary is deliberate. `node` carries `additionalProperties: true`,
//! so the frozen `schemas/releases/1.0.0` validator accepts a node whose `agents` key holds
//! anything at all; a refusal fixture in `conformance/manifest.json` would be compared against
//! that snapshot too and would read as the live validator invalidating history. The acceptance
//! case lives there (`schema.node.valid.agents`); the refusals live here.

use serde_json::{Value, json};

fn graph_with_node(node: Value) -> Value {
    json!({
        "apiVersion": "p50.dev/graph/v1",
        "kind": "ExecutionGraph",
        "metadata": {
            "id": "graph-crew",
            "name": "Crew",
            "executionId": "execution-crew",
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

fn agent_node(agents: Value) -> Value {
    json!({
        "type": "agent",
        "name": "Ship the change",
        "objective": "One task, several workers",
        "optionality": "required",
        "agent": {"ref": "agent://primary"},
        "agents": agents
    })
}

fn refusals(node: Value) -> Vec<(String, String)> {
    graphhelm_schema::validate_graph_value(&graph_with_node(node), "node_agents")
        .into_iter()
        .map(|diagnostic| (diagnostic.code, diagnostic.path))
        .collect()
}

#[test]
fn a_task_worked_by_several_agents_validates() {
    assert_eq!(
        refusals(agent_node(json!([
            {"ref": "agent://second"},
            {"ref": "agent://third"}
        ]))),
        Vec::new()
    );
}

// Absent and empty must not mean the same thing: a node that names no crew says so by carrying no
// `agents` key, never by carrying an empty one.
#[test]
fn an_empty_crew_is_refused_at_its_own_pointer() {
    assert!(
        refusals(agent_node(json!([]))).contains(&(
            "GHS002_SCHEMA".to_owned(),
            "/spec/nodes/start/agents".to_owned()
        )),
        "an empty crew was accepted"
    );
}

// A crew member is NAMED, never defined inline: an inline `ephemeral` agent externalizes into an
// `agent_configuration` control and a persisted node carries at most one control of each type, so
// a second inline definition on one node has nowhere to be published.
#[test]
fn a_crew_member_must_be_a_reference_binding() {
    for (case, value) in [
        ("scalar", json!("agent://second")),
        ("scalar member", json!(["agent://second"])),
        ("empty ref", json!([{"ref": ""}])),
        ("no ref", json!([{"ephemeral": {"purpose": "help"}}])),
        (
            "ref plus a stray key",
            json!([{"ref": "agent://second", "ephemeral": {"purpose": "help"}}]),
        ),
    ] {
        let reported = refusals(agent_node(value));
        assert!(
            reported.iter().any(|(code, path)| code == "GHS002_SCHEMA"
                && path.starts_with("/spec/nodes/start/agents")),
            "{case} was accepted as a crew: {reported:?}"
        );
    }
}

// Bound the authored list before anything expensive walks it, as every other authored list in a
// graph document already is.
#[test]
fn a_crew_is_bounded() {
    let oversized = (0..65)
        .map(|index| json!({"ref": format!("agent://worker-{index}")}))
        .collect::<Vec<_>>();
    assert!(
        refusals(agent_node(json!(oversized))).contains(&(
            "GHS002_SCHEMA".to_owned(),
            "/spec/nodes/start/agents".to_owned()
        )),
        "a crew of 65 was accepted"
    );
}

// The singular form is untouched by all of the above: `agent` still admits an inline definition,
// and a `type == "agent"` node still has to name its primary worker.
#[test]
fn the_singular_binding_keeps_its_contract() {
    let inline = json!({
        "type": "agent",
        "name": "Ship the change",
        "objective": "One task, one worker",
        "optionality": "required",
        "agent": {"ephemeral": {
            "purpose": "Evaluate one conformance input",
            "capabilities": ["classify"],
            "inputSchema": "fixture-input",
            "outputSchema": "fixture-output",
            "instructions": "Return deterministic evidence.",
            "completionContract": "evidence-recorded"
        }}
    });
    assert_eq!(refusals(inline), Vec::new());

    let crew_only = json!({
        "type": "agent",
        "name": "Ship the change",
        "objective": "One task, no primary",
        "optionality": "required",
        "agents": [{"ref": "agent://second"}]
    });
    assert!(
        refusals(crew_only)
            .iter()
            .any(|(code, path)| code == "GHS002_SCHEMA" && path == "/spec/nodes/start"),
        "an agent node with no primary worker was accepted"
    );
}
