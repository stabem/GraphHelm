//! The capability catalog is derived from what the runtime executes (#184), never declared by
//! hand: a node type the executor refuses is never offered to the model, and a program the
//! operator did not allow is never invented.

use std::collections::BTreeSet;

use graphhelm_architect::CapabilityCatalog;
use graphhelm_protocols::NodeType;

/// Exhaustiveness by construction: the catalog walks `NodeType::EVERY_VARIANT`, and a new
/// variant makes this match fail to compile — the moment the catalog decision for it (offered
/// to the model, or refused by `work_kind`) has to be read and confirmed rather than inherited
/// in silence. The list itself is the protocols crate's own, so there is no second array here
/// that could fall behind it.
#[test]
fn every_variant_reaches_the_catalog_decision() {
    for variant in NodeType::EVERY_VARIANT {
        match variant {
            NodeType::Agent
            | NodeType::Tool
            | NodeType::Classifier
            | NodeType::Planner
            | NodeType::Gate
            | NodeType::Evaluator
            | NodeType::Fork
            | NodeType::Join
            | NodeType::HumanDecision
            | NodeType::Timer
            | NodeType::Trigger
            | NodeType::Subgraph
            | NodeType::Materializer
            | NodeType::Deploy
            | NodeType::Rollback
            | NodeType::ArtifactTransform
            | NodeType::DeadLetter => {}
        }
    }
    let unique: BTreeSet<String> = NodeType::EVERY_VARIANT
        .iter()
        .map(|variant| serde_json::to_string(variant).unwrap())
        .collect();
    assert_eq!(
        unique.len(),
        NodeType::EVERY_VARIANT.len(),
        "no variant is listed twice"
    );
}

#[test]
fn the_catalog_admits_exactly_the_types_the_runtime_executes() {
    let catalog = CapabilityCatalog::from_runtime(&[]);
    for variant in NodeType::EVERY_VARIANT {
        let wire = serde_json::to_string(variant)
            .unwrap()
            .trim_matches('"')
            .to_owned();
        assert_eq!(
            catalog.allows_node_type(&wire),
            graphhelm_runtime::classify::work_kind(variant).is_ok(),
            "{wire}"
        );
    }
    assert!(catalog.allows_node_type("agent"));
    assert!(catalog.allows_node_type("tool"));
    assert!(!catalog.allows_node_type("deploy"));
    assert!(!catalog.allows_node_type("not_a_type"));
    let sorted = {
        let mut copy = catalog.node_types.clone();
        copy.sort();
        copy
    };
    assert_eq!(catalog.node_types, sorted, "node types are sorted");
    assert_eq!(catalog.tool_families, vec!["repository", "shell", "tests"]);
}

#[test]
fn programs_are_the_operators_allowlist_and_nothing_else() {
    let catalog = CapabilityCatalog::from_runtime(&[
        "cargo".to_owned(),
        "git".to_owned(),
        "cargo".to_owned(),
    ]);
    assert_eq!(catalog.programs, vec!["cargo", "git"]);
    assert!(catalog.allows_program("git"));
    assert!(!catalog.allows_program("python"));
    assert!(
        CapabilityCatalog::from_runtime(&[]).programs.is_empty(),
        "no default program: the allowlist is never invented"
    );
}

#[test]
fn the_catalog_serializes_as_camel_case_wire_json() {
    let catalog = CapabilityCatalog::from_runtime(&["cargo".to_owned()]);
    let value = serde_json::to_value(&catalog).unwrap();
    assert_eq!(value["programs"], serde_json::json!(["cargo"]));
    assert_eq!(
        value["toolFamilies"],
        serde_json::json!(["repository", "shell", "tests"])
    );
    assert!(value["nodeTypes"].as_array().unwrap().len() >= 2, "{value}");
}
