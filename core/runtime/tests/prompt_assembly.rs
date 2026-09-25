//! Task 4: prompt assembly is deterministic, complete, refuses a broken contract, and its
//! output never enters an event payload — Evidence is its only serialization target.

use std::collections::BTreeMap;

use graphhelm_protocols::{GraphNode, NodeType, Optionality};
use graphhelm_runtime::executor::ExecutorRefusal;
use graphhelm_runtime::prompt::assemble;

/// The software-feature example's `implement` node shape, verbatim property paths.
fn implement_node() -> GraphNode {
    let agent: serde_json::Value = serde_json::json!({
        "ephemeral": {
            "purpose": "Executar o plano.",
            "capabilities": ["repository.write_patch", "tests.execute_targeted"],
            "allowedTools": ["repository.read", "repository.write", "shell.execute_restricted"],
            "prohibitedActions": ["production.deploy"],
            "inputSchema": "schema://ImplementationPlan@1",
            "outputSchema": "schema://ImplementationResult@1",
            "instructions": "Siga o plano; não invente etapas.",
            "isolationMinimum": "tier_0",
        }
    });
    let mut properties = BTreeMap::new();
    properties.insert("agent".to_owned(), agent);
    GraphNode {
        node_type: NodeType::Agent,
        name: "Implementar".to_owned(),
        objective: "Aplicar a mudança no worktree isolado.".to_owned(),
        optionality: Optionality::Required,
        properties,
    }
}

#[test]
fn assembly_is_deterministic_and_complete() {
    let node = implement_node();
    let first = assemble(&node).expect("the example node assembles");
    let second = assemble(&node).expect("the example node assembles");
    assert_eq!(first, second, "same node, byte-identical prompt");

    let rendered = format!("{}\n{}", first.system, first.task);
    for required in [
        "Aplicar a mudança no worktree isolado.",
        "Siga o plano; não invente etapas.",
        "schema://ImplementationPlan@1",
        "schema://ImplementationResult@1",
    ] {
        assert!(
            rendered.contains(required),
            "the prompt must carry {required:?}"
        );
    }

    // Field order is the assembler's, fixed: purpose, instructions, input schema, output
    // schema in system; the objective is the task. Positions, not just presence.
    let purpose = first
        .system
        .find("Executar o plano.")
        .expect("purpose present");
    let instructions = first
        .system
        .find("Siga o plano")
        .expect("instructions present");
    let input = first
        .system
        .find("ImplementationPlan")
        .expect("input schema present");
    let output = first
        .system
        .find("ImplementationResult")
        .expect("output schema present");
    assert!(
        purpose < instructions && instructions < input && input < output,
        "assembler-fixed field order, never map iteration"
    );
    assert!(
        first.task.contains("Aplicar a mudança"),
        "the objective is the task"
    );
}

#[test]
fn the_digest_is_computed_at_assembly_over_system_and_task() {
    let prompt = assemble(&implement_node()).expect("assembles");
    assert_eq!(prompt.sha256.len(), 64, "bare hex, the record convention");
    assert!(prompt.sha256.chars().all(|c| c.is_ascii_hexdigit()));

    // Any content change moves the digest — Evidence and record agree on identity.
    let mut other = implement_node();
    other.objective.push('!');
    let moved = assemble(&other).expect("assembles");
    assert_ne!(
        prompt.sha256, moved.sha256,
        "content change must move the digest"
    );
}

#[test]
fn assembly_refuses_a_node_missing_its_contract() {
    let mut tool = implement_node();
    tool.node_type = NodeType::Tool;
    assert_eq!(
        assemble(&tool),
        Err(ExecutorRefusal::Unassemblable),
        "a Tool node has no prompt"
    );

    let mut no_objective = implement_node();
    no_objective.objective = "   ".to_owned();
    assert_eq!(
        assemble(&no_objective),
        Err(ExecutorRefusal::Unassemblable),
        "an Agent node without an objective is unassemblable — never an empty prompt"
    );

    let mut no_contract = implement_node();
    no_contract.properties.clear();
    assert_eq!(
        assemble(&no_contract),
        Err(ExecutorRefusal::Unassemblable),
        "an Agent node without its agent.ephemeral block has no contract to assemble"
    );
}

#[test]
fn the_prompt_never_enters_an_event_payload() {
    // The honest executable form of the compile-shaped claim: nothing in prompt.rs names an
    // event kind, so no path from AssembledPrompt into an event payload exists in this module.
    let source = include_str!("../src/prompt.rs");
    assert!(
        !source.contains("EventKind::"),
        "prompt assembly must never construct an event kind"
    );
    assert!(
        !source.contains("graphhelm_events"),
        "prompt assembly must never reach the event store"
    );
}
