use std::path::{Path, PathBuf};

use assert_cmd::Command;
use graphhelm_protocols::{
    Actor, ActorType, DraftOperation, GraphDraft, ManualOverride, NodeType, WaiverScope,
};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn command() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
}

fn json(output: &[u8]) -> serde_json::Value {
    serde_json::from_slice(output).unwrap()
}

#[test]
fn validate_returns_one_json_document_and_zero_for_canonical_yaml() {
    let output = command()
        .args([
            "graph",
            "validate",
            root()
                .join("examples/graphs/software-feature.yaml")
                .to_str()
                .unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json(&output.stdout);
    assert_eq!(value["ok"], true);
    assert_eq!(value["command"], "graph.validate");
}

#[test]
fn lint_failure_is_json_and_exit_two() {
    let output = command()
        .args([
            "graph",
            "lint",
            root()
                .join("tests/fixtures/invalid/entrypoint.yaml")
                .to_str()
                .unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let value = json(&output.stdout);
    assert_eq!(value["diagnostics"][0]["code"], "GHG001_ENTRYPOINT_UNKNOWN");
}

#[test]
fn json_graph_hash_matches_yaml_hash() {
    let directory = tempfile::tempdir().unwrap();
    let yaml_path = root().join("examples/graphs/software-feature.yaml");
    let yaml: serde_json::Value =
        serde_yaml_ng::from_str(&std::fs::read_to_string(&yaml_path).unwrap()).unwrap();
    let json_path = directory.path().join("graph.json");
    std::fs::write(&json_path, serde_json::to_vec_pretty(&yaml).unwrap()).unwrap();
    let yaml_output = command()
        .args(["graph", "hash", yaml_path.to_str().unwrap()])
        .output()
        .unwrap();
    let json_output = command()
        .args(["graph", "hash", json_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(yaml_output.status.success());
    assert!(json_output.status.success());
    assert_eq!(
        json(&yaml_output.stdout)["data"]["hash"],
        json(&json_output.stdout)["data"]["hash"]
    );
}

#[test]
fn secret_values_are_never_echoed_or_persisted_by_cli_preflight() {
    let directory = tempfile::tempdir().unwrap();
    let mut graph =
        graphhelm_schema::load_graph(&root().join("examples/graphs/software-feature.yaml"))
            .unwrap()
            .graph;
    graph
        .spec
        .nodes
        .get_mut("implement")
        .unwrap()
        .properties
        .insert("apiKey".into(), serde_json::json!("TOP-SECRET-DO-NOT-ECHO"));
    let graph_path = directory.path().join("secret.json");
    std::fs::write(&graph_path, serde_json::to_vec(&graph).unwrap()).unwrap();

    for subcommand in ["validate", "hash"] {
        let output = command()
            .args(["graph", subcommand, graph_path.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(!String::from_utf8_lossy(&output.stdout).contains("TOP-SECRET"));
    }

    let events = directory.path().join("events.jsonl");
    let output = command()
        .args([
            "graph",
            "simulate",
            graph_path.to_str().unwrap(),
            "--events",
            events.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        json(&output.stdout)["diagnostics"][0]["code"],
        "GHG008_INLINE_SECRET"
    );
    assert!(!events.exists());
}

#[test]
fn simulate_then_fresh_process_replay_reconstructs_terminal_state() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events.jsonl");
    let graph = root().join("examples/graphs/software-feature.yaml");
    let simulated = command()
        .args([
            "graph",
            "simulate",
            graph.to_str().unwrap(),
            "--events",
            events.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        simulated.status.success(),
        "{}",
        String::from_utf8_lossy(&simulated.stderr)
    );
    assert_eq!(json(&simulated.stdout)["data"]["status"], "completed");

    let replayed = command()
        .args(["graph", "replay", "--events", events.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        replayed.status.success(),
        "{}",
        String::from_utf8_lossy(&replayed.stderr)
    );
    let projection = json(&replayed.stdout);
    assert_eq!(projection["data"]["simulationStatus"], "completed");
    assert_eq!(
        projection["data"]["currentGraph"]["graph"]["metadata"]["version"],
        1
    );
}

#[test]
fn draft_apply_emits_waiver_and_impossible_deploy_exits_three() {
    let directory = tempfile::tempdir().unwrap();
    let mut graph =
        graphhelm_schema::load_graph(&root().join("examples/graphs/software-feature.yaml"))
            .unwrap()
            .graph;
    graph.spec.nodes.get_mut("review").unwrap().node_type = NodeType::Gate;
    let base_path = directory.path().join("base.json");
    std::fs::write(&base_path, serde_json::to_vec_pretty(&graph).unwrap()).unwrap();
    let review_edges: Vec<_> = graph
        .spec
        .edges
        .iter()
        .filter(|edge| edge.from == "review" || edge.to == "review")
        .map(|edge| DraftOperation::RemoveEdge {
            id: edge.id.clone(),
        })
        .collect();
    let mut operations = review_edges;
    operations.push(DraftOperation::RemoveNode {
        id: "review".into(),
    });
    let waiver_draft = GraphDraft {
        id: "draft-waiver".into(),
        expected_version: graph.metadata.version,
        expected_hash: graphhelm_graph::semantic_hash(&graph).unwrap(),
        operations,
        manual_override: Some(ManualOverride {
            actor: Actor::new(ActorType::Owner, "owner-local"),
            reason: "accepted review bypass".into(),
            waived_requirements: vec!["review".into()],
            acknowledged_risks: vec!["unreviewed change".into()],
            scope: WaiverScope::Execution,
        }),
    };
    let waiver_path = directory.path().join("waiver.json");
    std::fs::write(
        &waiver_path,
        serde_json::to_vec_pretty(&waiver_draft).unwrap(),
    )
    .unwrap();
    let waiver_events = directory.path().join("waiver-events.jsonl");
    let output = command()
        .args([
            "graph",
            "draft",
            "apply",
            base_path.to_str().unwrap(),
            waiver_path.to_str().unwrap(),
            "--actor",
            "owner-local",
            "--events",
            waiver_events.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        json(&output.stdout)["data"]["waivers"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let impossible = GraphDraft {
        id: "draft-impossible".into(),
        expected_version: graph.metadata.version,
        expected_hash: graphhelm_graph::semantic_hash(&graph).unwrap(),
        operations: vec![DraftOperation::PatchNode {
            id: "implement".into(),
            patch: serde_json::json!({"type": "deploy", "targetRef": null}),
        }],
        manual_override: Some(ManualOverride {
            actor: Actor::new(ActorType::Owner, "owner-local"),
            reason: "emergency".into(),
            waived_requirements: vec!["deploy_target".into()],
            acknowledged_risks: vec!["unknown destination".into()],
            scope: WaiverScope::Execution,
        }),
    };
    let impossible_path = directory.path().join("impossible.json");
    std::fs::write(&impossible_path, serde_json::to_vec(&impossible).unwrap()).unwrap();
    let impossible_events = directory.path().join("impossible-events.jsonl");
    let output = command()
        .args([
            "graph",
            "draft",
            "apply",
            base_path.to_str().unwrap(),
            impossible_path.to_str().unwrap(),
            "--actor",
            "owner-local",
            "--events",
            impossible_events.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(
        json(&output.stdout)["diagnostics"][0]["code"],
        "GHP001_STRUCTURAL_IMPOSSIBILITY"
    );
}
