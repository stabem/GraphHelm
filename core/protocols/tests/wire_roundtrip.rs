use chrono::{TimeZone, Utc};
use graphhelm_protocols::{
    Actor, ActorType, DraftOperation, EventKind, ExecutionGraph, GraphDraft, ManualOverride,
    NodeState, PolicyWaiver, SemanticHash, SimulationStatus, WaiverScope,
};

#[test]
fn canonical_graph_wire_shape_round_trips() {
    let source = include_str!("../../../examples/graphs/manual-override-deploy.yaml");
    let value: serde_json::Value = serde_yaml_ng::from_str(source).unwrap();
    let graph: ExecutionGraph = serde_json::from_value(value).unwrap();

    assert_eq!(graph.metadata.version, 13);
    assert_eq!(graph.spec.nodes["deploy"].node_type.as_str(), "deploy");
    assert_eq!(
        serde_json::to_value(NodeState::WaitingCapacity).unwrap(),
        "waiting_capacity"
    );
}

#[test]
fn unknown_node_fields_survive_round_trip_but_unknown_types_fail() {
    let source = include_str!("../../../examples/graphs/manual-override-deploy.yaml");
    let value: serde_json::Value = serde_yaml_ng::from_str(source).unwrap();
    let graph: ExecutionGraph = serde_json::from_value(value.clone()).unwrap();
    let round_trip = serde_json::to_value(graph).unwrap();
    assert_eq!(
        round_trip["spec"]["nodes"]["deploy"]["targetRef"],
        "environment://staging"
    );

    let mut unknown = value;
    unknown["spec"]["nodes"]["deploy"]["type"] = "future_node".into();
    assert!(serde_json::from_value::<ExecutionGraph>(unknown).is_err());
}

#[test]
fn draft_override_and_waiver_use_stable_camel_case_fields() {
    let override_request = ManualOverride {
        actor: Actor::new(ActorType::Owner, "owner-local"),
        reason: "release is explicitly accepted".into(),
        waived_requirements: vec!["review".into()],
        acknowledged_risks: vec!["unreviewed change".into()],
        scope: WaiverScope::Execution,
    };
    let draft = GraphDraft {
        id: "draft-1".into(),
        expected_version: 13,
        expected_hash: SemanticHash::new("sha256:abc"),
        operations: vec![DraftOperation::RemoveNode {
            id: "review".into(),
        }],
        manual_override: Some(override_request.clone()),
    };
    let draft_value = serde_json::to_value(&draft).unwrap();
    assert_eq!(draft_value["expectedVersion"], 13);
    assert_eq!(draft_value["operations"][0]["op"], "removeNode");
    assert_eq!(draft_value["operations"][0]["path"], "/spec/nodes/review");
    assert!(draft_value["operations"][0].get("id").is_none());
    assert_eq!(
        draft_value["manualOverride"]["waivedRequirements"][0],
        "review"
    );

    let waiver = PolicyWaiver {
        id: "waiver-1".into(),
        requirement: "review".into(),
        execution_id: "exec-1".into(),
        graph_version: 14,
        actor: override_request.actor.id.clone(),
        reason: Some(override_request.reason),
        acknowledged_risks: override_request.acknowledged_risks,
        scope: WaiverScope::Execution,
        created_at: Utc.with_ymd_and_hms(2026, 8, 8, 12, 0, 0).unwrap(),
        expires_at: None,
    };
    let waiver_value = serde_json::to_value(&waiver).unwrap();
    assert_eq!(waiver_value["executionId"], "exec-1");
    assert_eq!(waiver_value["graphVersion"], 14);
    assert_eq!(waiver_value["acknowledgedRisks"][0], "unreviewed change");
}

#[test]
fn normative_add_edge_uses_value_without_path() {
    let value = serde_json::json!({
        "op": "addEdge",
        "value": {
            "id": "a-to-b",
            "from": "a",
            "to": "b",
            "type": "control",
            "map": {}
        }
    });
    let operation: DraftOperation = serde_json::from_value(value).unwrap();
    let encoded = serde_json::to_value(operation).unwrap();
    assert_eq!(encoded["value"]["id"], "a-to-b");
    assert!(encoded.get("path").is_none());
}

#[test]
fn normative_states_statuses_and_event_kinds_have_exact_wire_names() {
    let states = [
        (NodeState::Draft, "draft"),
        (NodeState::WaitingInput, "waiting_input"),
        (NodeState::WaitingCapacity, "waiting_capacity"),
        (NodeState::Invalidated, "invalidated"),
    ];
    for (state, expected) in states {
        assert_eq!(serde_json::to_value(state).unwrap(), expected);
    }

    for (status, expected) in [
        (SimulationStatus::Running, "running"),
        (SimulationStatus::Completed, "completed"),
        (SimulationStatus::Failed, "failed"),
        (SimulationStatus::Paused, "paused"),
    ] {
        assert_eq!(serde_json::to_value(status).unwrap(), expected);
    }

    let kinds = [
        (EventKind::graph_imported("source.yaml"), "graph_imported"),
        (
            EventKind::draft_rejected("draft-1", "blocked"),
            "draft_rejected",
        ),
        (EventKind::simulation_started(), "simulation_started"),
    ];
    for (kind, expected) in kinds {
        assert_eq!(serde_json::to_value(kind).unwrap()["type"], expected);
    }
}
