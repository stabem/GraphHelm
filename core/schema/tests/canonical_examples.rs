use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn all_canonical_graph_examples_validate_offline() {
    for relative in [
        "examples/graphs/software-feature.yaml",
        "examples/graphs/manual-override-deploy.yaml",
        "examples/graphs/research-to-publish.yaml",
    ] {
        let path = root().join(relative);
        let loaded = graphhelm_schema::load_graph(&path)
            .unwrap_or_else(|diagnostics| panic!("{relative}: {diagnostics:?}"));
        assert_eq!(loaded.graph.kind, "ExecutionGraph");
    }
}

#[test]
fn invalid_edge_reports_schema_code_pointer_and_source() {
    let path = root().join("tests/fixtures/invalid/schema-invalid-edge.yaml");
    let diagnostics = graphhelm_schema::load_graph(&path).unwrap_err();
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "GHS002_SCHEMA"
            && diagnostic.path == "/spec/edges/0/type"
            && diagnostic.source.ends_with("schema-invalid-edge.yaml")
    }));
}

#[test]
fn missing_required_field_reports_its_parent_pointer() {
    let path = root().join("tests/fixtures/invalid/schema-missing-kind.yaml");
    let diagnostics = graphhelm_schema::load_graph(&path).unwrap_err();
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == "GHS002_SCHEMA" && diagnostic.path == "/" })
    );
}

#[test]
fn policy_waiver_schema_is_validated_with_embedded_resources() {
    let waiver = serde_json::json!({
        "id": "waiver-1",
        "requirement": "review",
        "executionId": "exec-1",
        "graphVersion": 2,
        "actor": "owner-local",
        "reason": "accepted risk",
        "acknowledgedRisks": ["unreviewed change"],
        "scope": "execution",
        "createdAt": "2026-08-08T12:00:00Z",
        "expiresAt": null
    });
    assert!(graphhelm_schema::validate_waiver(&waiver, "waiver-1").is_empty());

    let mut invalid = waiver;
    invalid["acknowledgedRisks"] = serde_json::json!([]);
    let diagnostics = graphhelm_schema::validate_waiver(&invalid, "waiver-1");
    assert!(
        diagnostics
            .iter()
            .any(|item| { item.code == "GHS002_SCHEMA" && item.path == "/acknowledgedRisks" })
    );
}

#[test]
fn typed_graph_serialization_remains_schema_valid() {
    let path = root().join("examples/graphs/software-feature.yaml");
    let loaded = graphhelm_schema::load_graph(&path).unwrap();
    let serialized = serde_json::to_value(&loaded.graph).unwrap();
    let diagnostics = graphhelm_schema::validate_graph_value(&serialized, "typed-round-trip");
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
}
