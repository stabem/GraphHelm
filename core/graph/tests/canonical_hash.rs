use std::path::Path;

use graphhelm_graph::semantic_hash;
use graphhelm_protocols::ExecutionGraph;
use proptest::prelude::*;

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
            "sha256:989a231a5d19d8f80f02c39229273460fa0c01997e9a9a4daa563a2eec470288".to_string(),
        ),
    ];
    assert_eq!(actual, expected);
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
