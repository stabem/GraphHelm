use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::json;

use super::*;

static TEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Self {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "graphhelm-extension-security-{name}-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn graph_resource(path: PathBuf, bytes: Vec<u8>) -> ValidatedResource {
    ValidatedResource {
        contribution: Contribution {
            id: "security-test-graph".to_owned(),
            kind: "graph".to_owned(),
            path: "graphs/security-test.yaml".to_owned(),
            sha256: sha256(&bytes),
            effects: Vec::new(),
            permissions: Vec::new(),
            requires: ContributionRequires {
                capabilities: Vec::new(),
                observers: Vec::new(),
            },
            surfaces: Vec::new(),
            family: None,
            schema: None,
        },
        path,
        bytes,
        base_path: "/spec/contracts/contributions/0".to_owned(),
    }
}

fn validate_graph_resource(resource: ValidatedResource) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    validate_contribution_contents(
        &[resource],
        &BTreeSet::new(),
        &BTreeMap::new(),
        "security-test-extension",
        "1.0.0",
        &mut diagnostics,
    );
    diagnostics
}

fn schema_resource(index: usize, members: usize) -> ValidatedResource {
    let path = format!("schemas/security-{index}.schema.json");
    let bytes = serde_json::to_vec(&json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": format!("https://p50.dev/schemas/security-{index}.schema.json"),
        "allOf": vec![serde_json::Value::Bool(true); members]
    }))
    .unwrap();
    ValidatedResource {
        contribution: Contribution {
            id: format!("schema/security-{index}"),
            kind: "schema".to_owned(),
            path,
            sha256: sha256(&bytes),
            effects: Vec::new(),
            permissions: Vec::new(),
            requires: ContributionRequires {
                capabilities: Vec::new(),
                observers: Vec::new(),
            },
            surfaces: Vec::new(),
            family: None,
            schema: None,
        },
        path: PathBuf::from(format!("security-{index}.schema.json")),
        bytes,
        base_path: format!("/spec/contracts/contributions/{index}"),
    }
}

#[test]
fn schema_set_aggregate_parse_budget_is_enforced_before_retaining_every_value() {
    let resources = (0..9)
        .map(|index| schema_resource(index, 16_000))
        .collect::<Vec<_>>();
    let mut diagnostics = Vec::new();

    validate_contribution_contents(
        &resources,
        &resources
            .iter()
            .map(|resource| resource.contribution.path.clone())
            .collect(),
        &BTreeMap::new(),
        "security-test-extension",
        "1.0.0",
        &mut diagnostics,
    );

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "GHEX011_LIMIT"
            && diagnostic.message
                == "extension schemas exceed deterministic aggregate JSON structure limits"
    }));
}

#[test]
fn structured_extension_resources_stop_at_the_value_limit_during_parse() {
    let json = serde_json::to_vec(&vec![serde_json::Value::Null; 32_769]).unwrap();
    let yaml = format!("{}\n", "- null\n".repeat(32_769));

    assert_eq!(
        parse_resource_value(&json, Some("json")),
        Err(BoundedValueError::ValueLimit)
    );
    assert_eq!(
        parse_resource_value(yaml.as_bytes(), Some("yaml")),
        Err(BoundedValueError::ValueLimit)
    );
}

#[test]
fn graph_yaml_aliases_are_rejected_before_materialization() {
    let directory = TestDirectory::new("yaml-alias");
    let path = directory.0.join("graph.yaml");
    let source = include_str!("../../../examples/graphs/software-feature.yaml")
        .replace("origin: user", "origin: &shared user")
        .replace("mode: autopilot", "mode: *shared");
    fs::write(&path, source.as_bytes()).unwrap();

    let diagnostics = validate_graph_resource(graph_resource(path, source.into_bytes()));

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "GHEX010_GRAPH"),
        "expected YAML alias rejection, got {diagnostics:?}"
    );
}

#[test]
fn graph_yaml_bom_cannot_hide_a_line_initial_anchor_or_alias() {
    let directory = TestDirectory::new("yaml-bom-alias");
    let path = directory.0.join("graph.yaml");
    let source = include_str!("../../../examples/graphs/software-feature.yaml")
        .replace("origin: user", "origin: \u{feff}&shared user")
        .replace("mode: autopilot", "mode: \u{feff}*shared");
    fs::write(&path, source.as_bytes()).unwrap();

    let diagnostics = validate_graph_resource(graph_resource(path, source.into_bytes()));

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "GHEX010_GRAPH"),
        "expected BOM-prefixed YAML alias rejection, got {diagnostics:?}"
    );
}

#[test]
fn validation_uses_captured_graph_bytes_instead_of_reopening_the_path() {
    let directory = TestDirectory::new("snapshot");
    let path = directory.0.join("graph.yaml");
    fs::write(&path, b"not: the captured graph\n").unwrap();
    let captured = include_bytes!("../../../examples/graphs/software-feature.yaml").to_vec();

    let diagnostics = validate_graph_resource(graph_resource(path, captured));

    assert!(
        !diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "GHEX010_GRAPH"),
        "captured valid bytes must be authoritative: {diagnostics:?}"
    );
}

#[test]
fn inline_schema_applicator_nesting_has_a_deterministic_limit() {
    let mut schema = json!({"type": "string"});
    for _ in 0..17 {
        schema = json!({"allOf": [schema]});
    }

    assert_eq!(
        crate::compile_inline_schema(&schema),
        Err(crate::InlineSchemaError::LimitExceeded)
    );
}

#[test]
fn inline_instance_node_count_has_a_deterministic_limit() {
    let schema = json!({"type": "array"});
    let instance = serde_json::Value::Array(vec![serde_json::Value::Null; 32_769]);

    assert_eq!(
        crate::validate_inline_value(&schema, &instance, "security-test"),
        Err(crate::InlineSchemaError::LimitExceeded)
    );
}

#[test]
fn schema_validation_caps_diagnostics_and_reports_omissions() {
    let schema = json!({"type": "array", "items": {"type": "integer"}});
    let instance = serde_json::Value::Array(vec![json!("invalid"); 1_000]);

    let diagnostics = crate::validate_inline_value(&schema, &instance, "security-test").unwrap();

    assert!(diagnostics.len() < 1_000, "diagnostics were not capped");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "GHS002_SCHEMA"
            && diagnostic.message
                == "schema validation diagnostic limit reached; further failures omitted"
    }));
}

fn high_branch_schema() -> serde_json::Value {
    json!({"allOf": vec![serde_json::Value::Bool(true); 1_024]})
}

fn high_member_instance() -> serde_json::Value {
    serde_json::Value::Array(vec![serde_json::Value::Null; 8_192])
}

#[test]
fn inline_validation_rejects_pathological_schema_times_instance_work() {
    assert_eq!(
        crate::validate_inline_value(
            &high_branch_schema(),
            &high_member_instance(),
            "security-test",
        ),
        Err(crate::InlineSchemaError::LimitExceeded)
    );
}

#[test]
fn offline_schema_set_rejects_pathological_schema_times_instance_work() {
    let schema_id = "https://p50.dev/schemas/security-work-test.json";
    let mut schema = high_branch_schema();
    schema["$schema"] = json!("https://json-schema.org/draft/2020-12/schema");
    schema["$id"] = json!(schema_id);
    let schemas =
        crate::OfflineSchemaSet::compile(BTreeMap::from([(schema_id.to_owned(), schema)]))
            .expect("bounded schema should compile");

    let diagnostics = schemas.validate(schema_id, &high_member_instance(), "security-test");

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "GHS002_SCHEMA"
            && diagnostic.message == "document exceeds deterministic validation work limits"
    }));
}

#[test]
fn bounded_fancy_regex_preserves_lookbehind_compatibility() {
    let schema = json!({"type": "string", "pattern": "(?<=a)b"});

    assert_eq!(crate::compile_inline_schema(&schema), Ok(()));
    assert!(
        crate::validate_inline_value(&schema, &json!("ab"), "security-test")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn schema_work_score_costs_each_instance_amplifying_keyword() {
    let cases = [
        (
            "patternProperties",
            json!({"patternProperties": {"^item-[0-9]+$": true}}),
            json!({"unknown": {"^item-[0-9]+$": true}}),
        ),
        (
            "dependentSchemas",
            json!({"dependentSchemas": {"enabled": true}}),
            json!({"unknown": {"enabled": true}}),
        ),
        ("items", json!({"items": true}), json!({"unknown": true})),
        (
            "prefixItems",
            json!({"prefixItems": [true, true]}),
            json!({"unknown": [true, true]}),
        ),
        (
            "properties",
            json!({"properties": {"enabled": true}}),
            json!({"unknown": {"enabled": true}}),
        ),
        (
            "additionalProperties",
            json!({"additionalProperties": true}),
            json!({"unknown": true}),
        ),
        (
            "unevaluatedProperties",
            json!({"unevaluatedProperties": true}),
            json!({"unknown": true}),
        ),
        (
            "unevaluatedItems",
            json!({"unevaluatedItems": true}),
            json!({"unknown": true}),
        ),
        (
            "contains",
            json!({"contains": true}),
            json!({"unknown": true}),
        ),
        (
            "propertyNames",
            json!({"propertyNames": true}),
            json!({"unknown": true}),
        ),
        (
            "pattern",
            json!({"pattern": "(?<=a)b"}),
            json!({"unknown": "(?<=a)b"}),
        ),
    ];

    for (keyword, schema, control) in cases {
        let score = crate::registry::schema_validation_work_score(&schema).unwrap();
        let control_score = crate::registry::schema_validation_work_score(&control).unwrap();
        assert!(
            score > control_score,
            "{keyword} was not represented in the work score: {score} <= {control_score}"
        );
    }
}
