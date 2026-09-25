use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

const PACKAGE_SCHEMA_PREFIX: &str = "https://p50.dev/extensions/graphhelm-jpd/schemas/";
const COUNCIL_RESULT_SCHEMA: &str =
    "https://p50.dev/extensions/graphhelm-jpd/schemas/council-result.schema.json";

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn package_root() -> PathBuf {
    repository_root().join("extensions/builtin/graphhelm-jpd")
}

fn load_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn load_graph(root: &Path) -> Value {
    serde_yaml_ng::from_slice(&fs::read(root.join("graphs/jpd-self-validation.yaml")).unwrap())
        .unwrap()
}

fn agent_definition(root: &Path, reference: &str) -> Value {
    let contribution = reference
        .strip_prefix("extension://graphhelm-jpd/agent/")
        .unwrap_or_else(|| panic!("unexpected agent reference: {reference}"));
    load_json(&root.join("agents").join(format!("{contribution}.json")))
}

fn primary_schema(root: &Path, graph: &Value, node_id: &str, direction: &str) -> String {
    let node = &graph["spec"]["nodes"][node_id];
    if let Some(schema) = node[direction]["schema"].as_str() {
        return schema.to_owned();
    }

    let reference = node["agent"]["ref"]
        .as_str()
        .unwrap_or_else(|| panic!("node {node_id} has no {direction} schema"));
    let definition = agent_definition(root, reference);
    let field = format!("{direction}Schema");
    definition[&field]
        .as_str()
        .unwrap_or_else(|| panic!("agent {reference} has no {field}"))
        .to_owned()
}

fn collect_schema_references(value: &Value, references: &mut BTreeSet<String>) {
    match value {
        Value::Array(values) => {
            for value in values {
                collect_schema_references(value, references);
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                if (key == "schema" || key.ends_with("Schema"))
                    && let Some(reference) = value.as_str()
                {
                    references.insert(reference.to_owned());
                }
                collect_schema_references(value, references);
            }
        }
        _ => {}
    }
}

fn edge_inventory(graph: &Value) -> BTreeMap<String, (String, String, String)> {
    graph["spec"]["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|edge| {
            (
                edge["id"].as_str().unwrap().to_owned(),
                (
                    edge["from"].as_str().unwrap().to_owned(),
                    edge["to"].as_str().unwrap().to_owned(),
                    edge["type"].as_str().unwrap().to_owned(),
                ),
            )
        })
        .collect()
}

#[test]
fn jpd_dogfood_graph_has_closed_and_type_honest_primary_data_flow() {
    let root = package_root();
    let graph = load_graph(&root);
    let nodes = graph["spec"]["nodes"].as_object().unwrap();

    let mut schema_references = BTreeSet::new();
    collect_schema_references(&graph, &mut schema_references);
    for node in nodes.values().filter(|node| node["type"] == "agent") {
        let definition = agent_definition(&root, node["agent"]["ref"].as_str().unwrap());
        collect_schema_references(&definition, &mut schema_references);
    }

    assert!(
        schema_references
            .iter()
            .all(|reference| !reference.starts_with("schema://")),
        "the task-local dogfood graph must not use unresolved schema:// references: {schema_references:?}"
    );
    for reference in schema_references
        .iter()
        .filter_map(|reference| reference.strip_prefix(PACKAGE_SCHEMA_PREFIX))
    {
        assert!(
            root.join("schemas").join(reference).is_file(),
            "package schema reference does not resolve: {reference}"
        );
    }

    for edge in graph["spec"]["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|edge| edge["type"] == "data")
    {
        let source = edge["from"].as_str().unwrap();
        let target = edge["to"].as_str().unwrap();
        assert_eq!(
            primary_schema(&root, &graph, source, "output"),
            primary_schema(&root, &graph, target, "input"),
            "data edge {} must connect equal primary schemas",
            edge["id"].as_str().unwrap()
        );
    }

    let edges = edge_inventory(&graph);
    let contract_schema = primary_schema(&root, &graph, "contract_journey", "output");
    for (node_id, _) in nodes.iter().filter(|(_, node)| node["type"] == "agent") {
        if primary_schema(&root, &graph, node_id, "input") == contract_schema {
            assert!(
                edges.values().any(|(source, target, edge_type)| {
                    source == "contract_journey" && target == node_id && edge_type == "data"
                }),
                "contract-input agent {node_id} must receive the contract through a data edge"
            );
        }
    }

    for (source, target) in [
        ("defect_hunter", "evidence_advocate"),
        ("idea_generator", "adversarial_critic"),
        ("adversarial_critic", "resolve_disagreement"),
        ("resolve_disagreement", "verify_journey"),
    ] {
        assert!(
            edges.values().any(|(edge_source, edge_target, edge_type)| {
                edge_source == source && edge_target == target && edge_type == "data"
            }),
            "missing required primary data edge {source} -> {target}"
        );
    }

    assert_eq!(
        primary_schema(&root, &graph, "resolve_disagreement", "output"),
        COUNCIL_RESULT_SCHEMA
    );
    assert_eq!(
        primary_schema(&root, &graph, "verify_journey", "input"),
        COUNCIL_RESULT_SCHEMA
    );
}

#[test]
fn jpd_verifier_receives_direct_contract_observation_retry_and_council_evidence() {
    let root = package_root();
    let graph = load_graph(&root);
    let edges = edge_inventory(&graph);

    for source in [
        "contract_journey",
        "compile_observations",
        "defect_hunter",
        "idea_generator",
        "adversarial_critic",
        "evidence_advocate",
        "accessibility_user",
        "recovery_operator",
    ] {
        assert!(
            edges.values().any(|(edge_source, target, edge_type)| {
                edge_source == source && target == "verify_journey" && edge_type == "evidence"
            }),
            "verify_journey must receive direct evidence from {source}"
        );
    }

    let fixture = load_json(&root.join("graphs/jpd-self-validation.fixtures.json"));
    let fixture_nodes = fixture["nodeOutcomes"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    let graph_nodes = graph["spec"]["nodes"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    assert_eq!(fixture_nodes, graph_nodes, "simulation fixture is stale");
}
