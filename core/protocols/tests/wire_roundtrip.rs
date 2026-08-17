use chrono::{TimeZone, Utc};
use graphhelm_protocols::{
    Actor, ActorType, DraftOperation, DraftRejected, EventKind, ExecutionGraph, ExecutionMode,
    GraphDraft, GraphImported, GraphSourceKind, ManualOverride, NodeOutcome, NodeState, OpaqueId,
    PolicyWaiver, RawSha256, SafeCode, SemanticHash, SignalSeverity, SignalSourceKind,
    SimulationStarted, SimulationStatus, WaiverScope, WireHash,
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
    assert_eq!(
        serde_json::from_value::<PolicyWaiver>(waiver_value).unwrap(),
        waiver
    );
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
        (NodeState::Ghost, "ghost"),
        (NodeState::WaitingInput, "waiting_input"),
        (NodeState::WaitingCapacity, "waiting_capacity"),
        (NodeState::Invalidated, "invalidated"),
    ];
    for (state, expected) in states {
        assert_eq!(serde_json::to_value(state).unwrap(), expected);
    }

    // All thirteen, not a sample. The point of this loop is pinning snake_case conversion, and
    // leaving NeedsInput out while pinning its sibling NeedsCapacity would miss exactly the
    // multi-word case it exists to catch.
    let outcomes = [
        (NodeOutcome::Started, "started"),
        (NodeOutcome::Succeeded, "succeeded"),
        (NodeOutcome::RetryableFailure, "retryable_failure"),
        (NodeOutcome::TerminalFailure, "terminal_failure"),
        (NodeOutcome::NeedsInput, "needs_input"),
        (NodeOutcome::NeedsCapacity, "needs_capacity"),
        (NodeOutcome::Approved, "approved"),
        (NodeOutcome::Waived, "waived"),
        (NodeOutcome::Skipped, "skipped"),
        (NodeOutcome::Cancelled, "cancelled"),
        (NodeOutcome::Invalidated, "invalidated"),
        (NodeOutcome::Paused, "paused"),
        (NodeOutcome::Interrupted, "interrupted"),
    ];
    for (outcome, expected) in outcomes {
        assert_eq!(serde_json::to_value(outcome).unwrap(), expected);
        assert_eq!(
            serde_json::from_value::<NodeOutcome>(serde_json::json!(expected)).unwrap(),
            outcome
        );
    }

    for (status, expected) in [
        (SimulationStatus::Running, "running"),
        (SimulationStatus::Completed, "completed"),
        (SimulationStatus::Failed, "failed"),
        (SimulationStatus::Paused, "paused"),
        (SimulationStatus::Cancelled, "cancelled"),
    ] {
        assert_eq!(serde_json::to_value(status).unwrap(), expected);
    }

    let kinds = [
        (
            EventKind::GraphImported(GraphImported {
                source_sha256: RawSha256::parse("a".repeat(64)).unwrap(),
                source_kind: GraphSourceKind::GraphDocument,
            }),
            "graph_imported",
        ),
        (
            EventKind::DraftRejected(DraftRejected {
                draft_id: OpaqueId::parse("draft-1").unwrap(),
                reason_code: SafeCode::parse("blocked").unwrap(),
                diagnostics: vec![],
                detail_evidence_id: None,
            }),
            "draft_rejected",
        ),
        (
            EventKind::SimulationStarted(SimulationStarted {
                simulation_id: OpaqueId::parse("simulation-1").unwrap(),
                graph_version: 1,
                graph_hash: WireHash::parse(format!("sha256:{}", "b".repeat(64))).unwrap(),
            }),
            "simulation_started",
        ),
    ];
    for (kind, expected) in kinds {
        assert_eq!(serde_json::to_value(kind).unwrap()["type"], expected);
    }

    for (mode, expected) in [
        (ExecutionMode::Autopilot, "autopilot"),
        (ExecutionMode::Supervised, "supervised"),
        (ExecutionMode::Manual, "manual"),
    ] {
        assert_eq!(serde_json::to_value(mode).unwrap(), expected);
        assert_eq!(
            serde_json::from_value::<ExecutionMode>(serde_json::json!(expected)).unwrap(),
            mode
        );
    }
    assert!(serde_json::from_value::<ExecutionMode>(serde_json::json!("god_mode")).is_err());

    for (severity, expected) in [
        (SignalSeverity::Low, "low"),
        (SignalSeverity::Medium, "medium"),
        (SignalSeverity::High, "high"),
        (SignalSeverity::Critical, "critical"),
    ] {
        assert_eq!(serde_json::to_value(severity).unwrap(), expected);
    }
    for (source, expected) in [
        (SignalSourceKind::Node, "node"),
        (SignalSourceKind::Runtime, "runtime"),
        (SignalSourceKind::Tool, "tool"),
        (SignalSourceKind::Test, "test"),
        (SignalSourceKind::User, "user"),
        (SignalSourceKind::Dream, "dream"),
        (SignalSourceKind::System, "system"),
    ] {
        assert_eq!(serde_json::to_value(source).unwrap(), expected);
        assert_eq!(
            serde_json::from_value::<SignalSourceKind>(serde_json::json!(expected)).unwrap(),
            source
        );
    }
}

/// M07 F3, the guard that protects every stream ever written: an outcome event recorded
/// before causes existed must re-serialize BYTE-IDENTICALLY, because replay re-serializes
/// each deserialized envelope and recomputes its hash against the stored one
/// (`core/events/src/projection.rs`). If `reason` were emitted as an explicit `null`, every
/// pre-M07 event's hash would change and every existing stream — the committed acceptance
/// evidence included — would fail replay with an integrity error. `skip_serializing_if` is
/// therefore load-bearing, not cosmetic, and this test is why it can never be removed.
#[test]
fn an_outcome_written_before_causes_existed_round_trips_byte_identically() {
    let stored = serde_json::json!({
        "executionId": "exec-1",
        "nodeId": "implement",
        "outcome": "retryable_failure",
        "nextState": "queued",
    });
    let parsed: graphhelm_protocols::NodeOutcomeRecorded =
        serde_json::from_value(stored.clone()).expect("an old outcome still deserializes");
    assert_eq!(
        parsed.reason, None,
        "absence reads as 'written before causes'"
    );
    assert_eq!(
        serde_json::to_value(&parsed).unwrap(),
        stored,
        "re-serializing must not add a key: the event hash is computed over these bytes"
    );
}

/// The other half: a cause, when present, rides the wire under its snake_case name.
#[test]
fn a_recorded_cause_uses_its_stable_wire_name() {
    let value = serde_json::json!({
        "executionId": "exec-1",
        "nodeId": "implement",
        "outcome": "retryable_failure",
        "nextState": "queued",
        "reason": "provider_unavailable",
    });
    let parsed: graphhelm_protocols::NodeOutcomeRecorded =
        serde_json::from_value(value.clone()).expect("a caused outcome deserializes");
    assert_eq!(
        parsed.reason,
        Some(graphhelm_protocols::NodeOutcomeReason::ProviderUnavailable)
    );
    assert_eq!(serde_json::to_value(&parsed).unwrap(), value);
}
