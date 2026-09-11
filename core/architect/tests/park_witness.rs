//! The cross-crate witness for #183: the lint's PARK set (`core/graph`, the nodes that raise
//! `GHG102_UNBOUNDED_CUSTOMS` without customs) and the compiler's STAMP set (`core/architect`,
//! the nodes `stamp_customs` covers) live in two crates and are maintained by hand in each. This
//! test walks every `NodeType` variant through both and holds them to one sentence:
//!
//! > GHG102 after stamping ⇔ (the lint's park set contains the type ∧ the compiler did not
//! > stamp it).
//!
//! Every type the runtime executes (agent, planner, classifier, evaluator, tool, gate) must
//! therefore leave with zero GHG102. A parkable-but-refused type (`human_decision`) keeps its
//! warning: the compiler stamps only what the executor dispatches, and `viability` refuses such
//! a node before the warning could matter — that is the arrangement, and this is the measurement
//! of it, per variant, rather than a belief about two lists that happen to agree today.

use graphhelm_architect::{TaskProfile, stamp_customs};
use graphhelm_protocols::NodeType;
use graphhelm_runtime::classify::{NodeWorkKind, work_kind};
use serde_json::{Value, json};

const NODE_ID: &str = "only";

/// Variants whose minimal one-node document cannot be authored under the checked-in schemas.
/// Asserted to be EXACTLY the set the loader refused, so a schema that starts accepting one of
/// them (or refusing another) reddens here instead of silently narrowing the witness.
const SKIPPED: [NodeType; 0] = [];

/// The smallest document of one node of `node_type` the schema accepts: the four required node
/// fields, the `agent` block the cognitive types carry, a `tests` call for the tool type, and
/// the `completion` block the schema requires of a gate.
fn one_node_document(node_type: &NodeType) -> Value {
    let mut node = json!({
        "type": node_type.as_str(),
        "name": "The only node",
        "objective": "Be the one node of a witness graph.",
        "optionality": "required",
    });
    match work_kind(node_type) {
        Ok(NodeWorkKind::Cognitive) => {
            node["agent"] = json!({
                "ephemeral": {
                    "purpose": "Witness.",
                    "capabilities": ["witness"],
                    "inputSchema": "schema://Witness@1",
                    "outputSchema": "schema://Witness@1",
                    "completionContract": { "requires": ["witness"] },
                    "instructions": "Witness.",
                }
            });
        }
        Ok(NodeWorkKind::Tool) => {
            node["tool"] = json!({ "call": { "tool": "tests", "arguments": [] } });
        }
        Ok(NodeWorkKind::GateCheck) => {
            node["completion"] = json!({ "requires": [] });
        }
        Err(_) => {}
    }
    json!({
        "apiVersion": "p50.dev/graph/v1",
        "kind": "ExecutionGraph",
        "metadata": {
            "id": "witness",
            "name": "witness",
            "executionId": "exec_witness",
            "version": 1,
        },
        "spec": {
            "entrypoints": [NODE_ID],
            "nodes": { NODE_ID: node },
            "edges": [],
            "budgets": { "maxNodes": 1 },
            "completion": { "terminalNodes": [NODE_ID] },
        },
    })
}

/// Whether the lint raises GHG102 on `document`'s one node. `Err` when the schema refuses the
/// document (the skip list's population).
fn ghg102(document: &Value) -> Result<bool, Vec<graphhelm_protocols::Diagnostic>> {
    let loaded =
        graphhelm_schema::load_graph_json(&serde_json::to_vec(document).unwrap(), "witness")?;
    let report = graphhelm_graph::lint(&loaded.graph, &loaded.source);
    let pointer = format!("/spec/nodes/{NODE_ID}/completion/customs");
    Ok(report
        .warnings
        .iter()
        .any(|warning| warning.code == "GHG102_UNBOUNDED_CUSTOMS" && warning.path == pointer))
}

#[test]
fn ghg102_after_stamping_iff_the_lint_parks_the_type_and_the_compiler_did_not_stamp_it() {
    let profile = TaskProfile::new("witness");
    let mut skipped = Vec::new();
    let mut executable_with_ghg102 = Vec::new();
    for node_type in NodeType::EVERY_VARIANT {
        let name = node_type.as_str();
        let document = one_node_document(node_type);
        let parks = match ghg102(&document) {
            Ok(parks) => parks,
            Err(diagnostics) => {
                assert!(
                    SKIPPED.contains(node_type),
                    "{name}: the schema refused a node the skip list does not name: {diagnostics:?}"
                );
                skipped.push(node_type.clone());
                continue;
            }
        };
        assert!(
            !SKIPPED.contains(node_type),
            "{name}: listed as unauthorable, but the schema accepted it"
        );

        let Value::Object(mut map) = document else {
            unreachable!("built as an object above")
        };
        let stamped = stamp_customs(&profile, &mut map);
        assert!(
            stamped.is_empty() || stamped == [NODE_ID],
            "{name}: the stamp names the one node or nothing: {stamped:?}"
        );
        let was_stamped = !stamped.is_empty();
        let after = ghg102(&Value::Object(map)).unwrap_or_else(|diagnostics| {
            panic!("{name}: stamping broke the schema: {diagnostics:?}")
        });

        assert_eq!(
            after,
            parks && !was_stamped,
            "{name}: GHG102 after stamping ({after}) must equal parks ({parks}) && !stamped ({was_stamped})"
        );
        if work_kind(node_type).is_ok() && after {
            executable_with_ghg102.push(name);
        }
    }
    assert_eq!(
        skipped,
        SKIPPED.to_vec(),
        "the skip list is exactly the set the schema refused"
    );
    assert!(
        executable_with_ghg102.is_empty(),
        "a type the runtime executes left with GHG102 — the #183 defect: {executable_with_ghg102:?}"
    );
}

/// The six executable types, by name, so the list above is not the only place they are held:
/// a reader who does not trust `work_kind` can read these six and the zero beside each.
#[test]
fn every_executable_type_leaves_with_zero_ghg102_by_name() {
    let profile = TaskProfile::new("witness");
    for node_type in [
        NodeType::Agent,
        NodeType::Planner,
        NodeType::Classifier,
        NodeType::Evaluator,
        NodeType::Tool,
        NodeType::Gate,
    ] {
        let Value::Object(mut map) = one_node_document(&node_type) else {
            unreachable!()
        };
        let _ = stamp_customs(&profile, &mut map);
        assert!(
            !ghg102(&Value::Object(map)).unwrap(),
            "{}: zero GHG102 after stamping",
            node_type.as_str()
        );
    }
    // A control that the instrument sees a positive: an unstamped agent DOES warn.
    let agent = one_node_document(&NodeType::Agent);
    assert!(
        ghg102(&agent).unwrap(),
        "the control: an unstamped agent must raise GHG102, or this witness measures nothing"
    );
}
