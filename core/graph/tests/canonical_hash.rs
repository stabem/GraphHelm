use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use chrono::{TimeZone, Utc};
use graphhelm_graph::{GraphVersion, canonical_content_bytes, raw_content_sha256, semantic_hash};
use graphhelm_protocols::{Actor, ActorType, ExecutionGraph};
use graphhelm_schema::OfflineSchemaSet;
use proptest::prelude::*;
use serde_json::{Value, json};

fn load() -> ExecutionGraph {
    graphhelm_schema::load_graph(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/graphs/software-feature.yaml"),
    )
    .unwrap()
    .graph
}

#[test]
fn ui_coordinates_do_not_change_semantic_hash() {
    let base = load();
    let mut moved = base.clone();
    moved.spec.nodes.get_mut("plan").unwrap().properties.insert(
        "ui".into(),
        serde_json::json!({"position": {"x": 999, "y": -42}}),
    );

    assert_eq!(
        semantic_hash(&base).unwrap(),
        semantic_hash(&moved).unwrap()
    );
}

#[test]
fn operational_edge_change_changes_semantic_hash() {
    let base = load();
    let mut changed = base.clone();
    changed.spec.edges[0].priority = Some(99);

    assert_ne!(
        semantic_hash(&base).unwrap(),
        semantic_hash(&changed).unwrap()
    );
}

#[test]
fn identity_and_history_metadata_do_not_change_semantic_hash() {
    let base = load();
    let mut renamed = base.clone();
    renamed.metadata.id = "copy".into();
    renamed.metadata.name = "Copy".into();
    renamed.metadata.execution_id = "copy-execution".into();
    renamed.metadata.version += 1;
    renamed.metadata.based_on = Some("other".into());

    assert_eq!(
        semantic_hash(&base).unwrap(),
        semantic_hash(&renamed).unwrap()
    );
}

#[test]
fn operational_metadata_changes_semantic_hash() {
    let base = load();
    let mut changed = base.clone();
    changed
        .metadata
        .properties
        .insert("policyHash".into(), serde_json::json!("sha256:new-policy"));

    assert_ne!(
        semantic_hash(&base).unwrap(),
        semantic_hash(&changed).unwrap()
    );
}

fn event_schema_set() -> OfflineSchemaSet {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let resources = [
        "agent",
        "artifact-reference",
        "claim",
        "context-capsule",
        "edge",
        "event-envelope",
        "evidence-record",
        "extension",
        "graph",
        "graph-signal",
        "node",
        "persisted-graph-version",
        "policy-waiver",
        "repository-scope",
        "sensitivity",
    ]
    .into_iter()
    .map(|name| {
        let document: Value = serde_json::from_slice(
            &fs::read(root.join(format!("schemas/{name}.schema.json"))).unwrap(),
        )
        .unwrap();
        (name.to_owned(), document)
    })
    .collect::<BTreeMap<_, _>>();
    OfflineSchemaSet::compile(resources).unwrap()
}

// Prevents persistence hardening from requiring the real canonicalizer to rename safe
// operational metadata into an x-* namespace.
#[test]
fn current_graph_version_producer_validates_safe_operational_metadata_without_renaming() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let schemas = event_schema_set();
    let persisted_version: Value = serde_json::from_slice(
        &fs::read(root.join("conformance/schemas/valid/persisted-graph-version.json")).unwrap(),
    )
    .unwrap();
    for name in [
        "software-feature.yaml",
        "manual-override-deploy.yaml",
        "research-to-publish.yaml",
    ] {
        let mut graph = graphhelm_schema::load_graph(&root.join("examples/graphs").join(name))
            .unwrap()
            .graph;
        graph.metadata.properties.insert(
            "retryPolicy".into(),
            json!({
                "maxAttempts": 3,
                "strategy": "fixed",
                "retryableCodes": ["capacity", "timeout"]
            }),
        );
        let version = GraphVersion::publish(
            graph,
            None,
            Actor::new(ActorType::Owner, "owner-test"),
            Utc.with_ymd_and_hms(2026, 8, 9, 0, 0, 0).unwrap(),
        )
        .unwrap()
        .to_record();
        let record = serde_json::to_value(version).unwrap();
        assert_eq!(
            record["graph"]["metadata"]["retryPolicy"],
            record["semantic"]["metadata"]["retryPolicy"],
            "{name}"
        );
    }

    let event = json!({
        "schemaVersion": "1.0.0",
        "eventId": "event-test",
        "scope": {
            "workspaceId": "workspace-test",
            "projectId": "project-test",
            "executionId": "execution-test"
        },
        "streamId": "stream-test",
        "sequence": 1,
        "occurredAt": "2026-08-09T00:00:00Z",
        "idempotencyKey": "idempotency-test",
        "actor": {"type": "system", "id": "system-test"},
        "sensitivity": "internal",
        "kind": {"type": "graph_version_published", "data": {"version": persisted_version}},
        "evidenceRefs": [],
        "artifactRefs": [],
        "previousHash": "sha256:35c8ab0717bef1684ad07efcf3bedd4648c778a2c944cbd2c7e6a4802e2237b3",
        "eventHash": "sha256:0000000000000000000000000000000000000000000000000000000000000000"
    });
    let diagnostics = schemas.validate(
        "https://p50.dev/schemas/event-envelope.schema.json",
        &event,
        "conformance",
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn descriptive_and_history_metadata_do_not_change_semantic_hash() {
    let base = load();
    let mut changed = base.clone();
    for key in [
        "description",
        "annotations",
        "createdAt",
        "createdBy",
        "mutationId",
    ] {
        changed
            .metadata
            .properties
            .insert(key.into(), serde_json::json!("non-semantic"));
    }
    assert_eq!(
        semantic_hash(&base).unwrap(),
        semantic_hash(&changed).unwrap()
    );
}

#[test]
fn canonical_examples_have_reviewed_golden_hashes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let actual: Vec<_> = [
        "software-feature.yaml",
        "manual-override-deploy.yaml",
        "research-to-publish.yaml",
    ]
    .into_iter()
    .map(|name| {
        let graph = graphhelm_schema::load_graph(&root.join("examples/graphs").join(name))
            .unwrap()
            .graph;
        (name, semantic_hash(&graph).unwrap().to_string())
    })
    .collect();
    let expected = vec![
        (
            "software-feature.yaml",
            "sha256:ca0484b161029b3cdded19d872665a319a608e240bb66964864ee2ef33bf92c6".to_string(),
        ),
        (
            "manual-override-deploy.yaml",
            "sha256:aa9b0715df457c2a1a364c1ab5ddee2c88bc390742c078fe6725b4b823e8a9bb".to_string(),
        ),
        (
            "research-to-publish.yaml",
            "sha256:9f8fff5d5f7d4b5bf0af3a38f06aa3492ba9656943bd0a798cbe81bcf5aebf43".to_string(),
        ),
    ];
    assert_eq!(actual, expected);
}

#[test]
fn externalized_json_content_is_canonical_across_nested_map_order() {
    let forward = json!({"outer": {"zeta": 2, "alpha": 1}, "items": [{"b": true, "a": false}]});
    let reverse = json!({"items": [{"a": false, "b": true}], "outer": {"alpha": 1, "zeta": 2}});
    let forward = canonical_content_bytes(&forward).unwrap();
    let reverse = canonical_content_bytes(&reverse).unwrap();

    assert_eq!(forward, reverse);
    assert_eq!(
        raw_content_sha256(&forward).unwrap(),
        raw_content_sha256(&reverse).unwrap()
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn arbitrary_ui_coordinates_never_change_hash(x in any::<i64>(), y in any::<i64>()) {
        let base = load();
        let mut moved = base.clone();
        moved.spec.nodes.get_mut("plan").unwrap().properties.insert(
            "ui".into(),
            serde_json::json!({"position": {"x": x, "y": y}}),
        );
        prop_assert_eq!(semantic_hash(&base).unwrap(), semantic_hash(&moved).unwrap());
    }
}
