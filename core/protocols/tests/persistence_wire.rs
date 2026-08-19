use std::collections::BTreeMap;

use chrono::{TimeZone, Utc};
use graphhelm_protocols::{
    ActorId, ArtifactId, ArtifactLocator, ArtifactReference, ContentFieldKind, ContentOwnerKind,
    ContentSlot, DiagnosticComponent, DiagnosticDomainPath, EdgeType, EventEnvelope, EventHash,
    EventKind, EvidenceId, EvidenceReference, ExecutionId, GraphVersionPublished,
    GraphVersionRecord, MediaType, NodeType, OpaqueId, Optionality, PersistedBudgets,
    PersistedControl, PersistedDiagnostic, PersistedEdge, PersistedGraphVersion,
    PersistedGraphVersionRef, PersistedNode, PersistedTopology, PolicyWaiver, ProjectId, RawSha256,
    RepositoryScope, SafeKey, SafeValue, SemanticVersion, Sensitivity, Severity, WaiverScope,
    WireHash, WorkspaceId,
};
use jsonschema::{Draft, Registry};
use serde_json::{Value, json};
use static_assertions::assert_not_impl_any;

const PERSISTED_ID: &str = "https://p50.dev/schemas/persisted-graph-version.schema.json";
const POLICY_WAIVER_ID: &str = "https://p50.dev/schemas/policy-waiver.schema.json";
const SENSITIVITY_ID: &str = "https://p50.dev/schemas/sensitivity.schema.json";
const ARTIFACT_ID: &str = "https://p50.dev/schemas/artifact-reference.schema.json";
const EVENT_ID: &str = "https://p50.dev/schemas/event-envelope.schema.json";

fn schema(source: &str) -> Value {
    serde_json::from_str(source).unwrap()
}

fn validator(schema_id: &str) -> jsonschema::Validator {
    let persisted = schema(include_str!(
        "../../../schemas/persisted-graph-version.schema.json"
    ));
    let waiver = schema(include_str!("../../../schemas/policy-waiver.schema.json"));
    let sensitivity = schema(include_str!("../../../schemas/sensitivity.schema.json"));
    let artifact = schema(include_str!(
        "../../../schemas/artifact-reference.schema.json"
    ));
    let event = schema(include_str!("../../../schemas/event-envelope.schema.json"));
    let scope = schema(include_str!(
        "../../../schemas/repository-scope.schema.json"
    ));
    let registry = Registry::new()
        .draft(Draft::Draft202012)
        .add(PERSISTED_ID, &persisted)
        .unwrap()
        .add(POLICY_WAIVER_ID, &waiver)
        .unwrap()
        .add(SENSITIVITY_ID, &sensitivity)
        .unwrap()
        .add(ARTIFACT_ID, &artifact)
        .unwrap()
        .add(EVENT_ID, &event)
        .unwrap()
        .add(
            "https://p50.dev/schemas/repository-scope.schema.json",
            &scope,
        )
        .unwrap()
        .prepare()
        .unwrap();
    let root = match schema_id {
        PERSISTED_ID => persisted.clone(),
        POLICY_WAIVER_ID => waiver.clone(),
        ARTIFACT_ID => artifact.clone(),
        EVENT_ID => event.clone(),
        other => panic!("unregistered test schema: {other}"),
    };
    jsonschema::options()
        .with_draft(Draft::Draft202012)
        .with_registry(&registry)
        .should_validate_formats(true)
        .build(&root)
        .unwrap()
}

fn event_fixture(kind: Value, project_level: bool) -> Value {
    let scope = if project_level {
        json!({"workspaceId":"workspace-1","projectId":"project-1"})
    } else {
        json!({
            "workspaceId":"workspace-1",
            "projectId":"project-1",
            "executionId":"execution-1"
        })
    };
    json!({
        "schemaVersion":"1.0.0",
        "eventId":"event-1",
        "scope":scope,
        "streamId":"stream-1",
        "sequence":1,
        "occurredAt":"2026-08-09T00:00:00Z",
        "idempotencyKey":"request-1",
        "actor":{"type":"system","id":"system-1"},
        "sensitivity":"internal",
        "kind":kind,
        "evidenceRefs":[],
        "artifactRefs":[],
        "previousHash":format!("sha256:{}", "0".repeat(64)),
        "eventHash":format!("sha256:{}", "1".repeat(64))
    })
}

#[test]
fn all_twenty_five_safe_event_variants_strictly_round_trip_against_schema() {
    let diagnostic = json!({
        "code":"GHP001_SAFE","severity":"error","path":"/topology","component":"governor"
    });
    let waiver = json!({
        "id":"waiver-1","requirement":"review","executionId":"execution-1",
        "graphVersion":1,"actor":"owner-1","acknowledgedRisks":["accepted-risk"],
        "scope":"execution","createdAt":"2026-08-09T00:00:00Z","expiresAt":null
    });
    let hash = format!("sha256:{}", "a".repeat(64));
    let raw = "b".repeat(64);
    let variants = vec![
        (
            json!({"type":"graph_imported","data":{"sourceSha256":raw,"sourceKind":"graph_document"}}),
            false,
        ),
        (
            json!({"type":"graph_validation_failed","data":{"diagnostics":[diagnostic.clone()]}}),
            false,
        ),
        (
            json!({"type":"graph_version_published","data":{"version":persisted_fixture()}}),
            false,
        ),
        (
            json!({"type":"draft_proposed","data":{"draftId":"draft-1","expectedVersion":1,"expectedHash":hash,"operationCount":1}}),
            false,
        ),
        (
            json!({"type":"draft_rejected","data":{"draftId":"draft-1","reasonCode":"policy_failed","diagnostics":[diagnostic]}}),
            false,
        ),
        (
            json!({"type":"draft_applied","data":{"draftId":"draft-1","graphVersion":2,"graphHash":hash}}),
            false,
        ),
        (
            json!({"type":"policy_obligation_evaluated","data":{"draftId":"draft-1","requirementId":"review","status":"satisfied","evidenceIds":[],"reasonCode":"satisfied","overrideable":true}}),
            false,
        ),
        (
            json!({"type":"policy_waiver_created","data":{"waiver":waiver}}),
            false,
        ),
        (
            json!({"type":"simulation_started","data":{"simulationId":"simulation-1","graphVersion":1,"graphHash":hash}}),
            false,
        ),
        (
            json!({"type":"node_state_changed","data":{"simulationId":"simulation-1","nodeId":"start","previousState":null,"nextState":"running"}}),
            false,
        ),
        (
            json!({"type":"simulation_completed","data":{"simulationId":"simulation-1","status":"completed"}}),
            false,
        ),
        (
            json!({"type":"execution_started","data":{"executionId":"execution-1","graphVersion":3,"graphHash":hash,"mode":"supervised"}}),
            false,
        ),
        (
            json!({"type":"execution_mode_changed","data":{"executionId":"execution-1","previousMode":null,"mode":"manual"}}),
            false,
        ),
        (
            json!({"type":"node_outcome_recorded","data":{"executionId":"execution-1","nodeId":"start","outcome":"succeeded","nextState":"succeeded"}}),
            false,
        ),
        (
            json!({"type":"execution_completed","data":{"executionId":"execution-1","status":"completed"}}),
            false,
        ),
        (
            json!({"type":"signal_recorded","data":{"executionId":"execution-1","signalId":"signal-1","sourceKind":"node","sourceId":"node-a","kind":"unexpected_dependency","severity":"high","envelopeSha256":raw}}),
            false,
        ),
        (
            json!({"type":"ghost_node_proposed","data":{"executionId":"execution-1","nodeId":"ghost-a","draftId":"draft-1"}}),
            false,
        ),
        (
            json!({"type":"mutation_accepted","data":{"executionId":"execution-1","draftId":"draft-1","mode":"autopilot","graphVersion":4}}),
            false,
        ),
        (
            json!({"type":"execution_paused","data":{"executionId":"execution-1"}}),
            false,
        ),
        (
            json!({"type":"execution_resumed","data":{"executionId":"execution-1"}}),
            false,
        ),
        (
            json!({"type":"integrity_checkpoint_created","data":{"streamId":"stream-1","sequence":1,"eventHash":hash,"repositoryFormat":"1.0.0","authenticationTag":{"keyId":"key-1","algorithm":"hmac-sha256","tagSha256":raw}}}),
            true,
        ),
        (
            json!({"type":"evidence_erasure_requested","data":{"evidenceScope":{"workspaceId":"workspace-1","projectId":"project-1","executionId":"execution-1"},"operationId":"operation-1","evidenceId":"evidence-1","keyHandleId":"handle-1","retentionPolicyId":"retention-1","retentionPolicyVersion":"1.0.0","authority":"authority-1","reasonCode":"expired","priorState":"available","state":"erasure_pending","requestedAt":"2026-08-09T00:00:00Z"}}),
            true,
        ),
        (
            json!({"type":"evidence_erasure_completed","data":{"evidenceScope":{"workspaceId":"workspace-1","projectId":"project-1","executionId":"execution-1"},"operationId":"operation-1","evidenceId":"evidence-1","keyHandleId":"handle-1","retentionPolicyId":"retention-1","retentionPolicyVersion":"1.0.0","authority":"authority-1","reasonCode":"expired","ciphertextSha256":raw,"providerReceiptId":"receipt-1","providerEpoch":1,"priorState":"erasure_pending","state":"erased","requestedAt":"2026-08-09T00:00:00Z","completedAt":"2026-08-09T00:01:00Z"}}),
            true,
        ),
        (
            json!({"type":"evidence_ciphertext_deleted","data":{"evidenceScope":{"workspaceId":"workspace-1","projectId":"project-1","executionId":"execution-1"},"operationId":"operation-1","evidenceId":"evidence-1","ciphertextSha256":raw,"deletedAt":"2026-08-09T00:02:00Z"}}),
            true,
        ),
        (
            json!({"type":"evidence_legal_hold_changed","data":{"evidenceScope":{"workspaceId":"workspace-1","projectId":"project-1","executionId":"execution-1"},"holdId":"hold-1","evidenceId":"evidence-1","authority":"authority-1","reasonCode":"investigation","state":"placed","changedAt":"2026-08-09T00:03:00Z"}}),
            true,
        ),
    ];

    assert_eq!(variants.len(), 25);
    for (kind, project_level) in variants {
        let document = event_fixture(kind, project_level);
        assert_schema_valid(EVENT_ID, &document);
        let envelope: EventEnvelope = serde_json::from_value(document.clone()).unwrap();
        let encoded = serde_json::to_value(&envelope).unwrap();
        assert_schema_valid(EVENT_ID, &encoded);
        assert_eq!(
            serde_json::from_value::<EventEnvelope>(encoded).unwrap(),
            envelope
        );
    }

    let mut unknown = event_fixture(
        json!({"type":"simulation_completed","data":{"simulationId":"simulation-1","status":"completed","message":"plaintext"}}),
        false,
    );
    assert_schema_invalid(EVENT_ID, &unknown);
    assert!(serde_json::from_value::<EventEnvelope>(unknown.take()).is_err());
    assert_not_impl_any!(GraphVersionRecord: Into<EventKind>);
    assert_not_impl_any!(graphhelm_protocols::Diagnostic: Into<EventKind>);
}

#[test]
fn governance_event_kinds_round_trip_with_exact_wire_names() {
    let digest = "a".repeat(64);
    let cases = [
        (
            "signal_recorded",
            json!({"executionId":"execution-1","signalId":"signal-1","sourceKind":"node","sourceId":"node-a","kind":"unexpected_dependency","severity":"high","envelopeSha256":digest}),
        ),
        (
            "ghost_node_proposed",
            json!({"executionId":"execution-1","nodeId":"ghost-a","draftId":"draft-1"}),
        ),
        (
            "mutation_accepted",
            json!({"executionId":"execution-1","draftId":"draft-1","mode":"autopilot","graphVersion":4}),
        ),
    ];
    for (name, data) in cases {
        let wire = json!({"type": name, "data": data});
        let kind: EventKind = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&kind).unwrap(), wire, "{name}");
    }
}

/// The pause and resume lifecycle kinds round-trip with exact wire names, modelled on
/// `governance_event_kinds_round_trip_with_exact_wire_names`. The envelope schema does not accept
/// these kinds yet — that is Task 3 — so this test only exercises Rust serialization.
#[test]
fn lifecycle_event_kinds_round_trip_with_exact_wire_names() {
    let cases = [
        ("execution_paused", json!({"executionId":"execution-1"})),
        ("execution_resumed", json!({"executionId":"execution-1"})),
    ];
    for (name, data) in cases {
        let wire = json!({"type": name, "data": data});
        let kind: EventKind = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&kind).unwrap(), wire, "{name}");
    }
}

/// An acceptance that does not say which mode it was accepted under is not evidence of anything.
#[test]
fn a_mutation_acceptance_without_a_mode_is_rejected() {
    let wire = json!({
        "type": "mutation_accepted",
        "data": {"executionId":"execution-1","draftId":"draft-1","graphVersion":4}
    });
    assert!(serde_json::from_value::<EventKind>(wire).is_err());
}

fn assert_schema_valid(schema_id: &str, document: &Value) {
    let errors = validator(schema_id)
        .iter_errors(document)
        .map(|error| error.to_string())
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "schema errors: {errors:?}");
}

fn assert_schema_invalid(schema_id: &str, document: &Value) {
    assert!(
        !validator(schema_id).is_valid(document),
        "document unexpectedly passed schema validation: {document}"
    );
}

fn persisted_fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../conformance/schemas/valid/persisted-graph-version.json"
    ))
    .unwrap()
}

fn construct_persisted_version(
    number: u64,
    predecessor: Option<PersistedGraphVersionRef>,
) -> Result<PersistedGraphVersion, graphhelm_protocols::PersistenceError> {
    let fixture: PersistedGraphVersion = serde_json::from_value(persisted_fixture()).unwrap();
    PersistedGraphVersion::new(
        number,
        predecessor,
        fixture.topology().clone(),
        fixture.topology_hash().clone(),
        fixture.semantic_hash().clone(),
        fixture.content_slots().to_vec(),
        fixture.created_by().clone(),
        fixture.created_at().clone(),
    )
}

#[test]
fn safe_projection_round_trips_and_validates() {
    let version: PersistedGraphVersion = serde_json::from_value(persisted_fixture()).unwrap();
    let json = serde_json::to_value(&version).unwrap();
    assert_schema_valid(PERSISTED_ID, &json);
    assert_eq!(
        serde_json::from_value::<PersistedGraphVersion>(json).unwrap(),
        version
    );
}

#[test]
fn persisted_graph_version_constructor_enforces_present_predecessor_successor() {
    const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

    for (number, predecessor_number) in
        [(3, 1), (2, 2), (2, 3), (MAX_SAFE_INTEGER, MAX_SAFE_INTEGER)]
    {
        let predecessor = PersistedGraphVersionRef::new(
            predecessor_number,
            WireHash::parse(format!("sha256:{}", "b".repeat(64))).unwrap(),
        )
        .unwrap();
        assert!(
            construct_persisted_version(number, Some(predecessor)).is_err(),
            "accepted graph version {number} after predecessor {predecessor_number}"
        );
    }

    let valid_predecessor = PersistedGraphVersionRef::new(
        1,
        WireHash::parse(format!("sha256:{}", "b".repeat(64))).unwrap(),
    )
    .unwrap();
    assert!(construct_persisted_version(2, Some(valid_predecessor)).is_ok());
    assert!(construct_persisted_version(3, None).is_ok());
}

#[test]
fn persisted_graph_version_deserialization_enforces_present_predecessor_successor() {
    const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

    for (number, predecessor_number) in
        [(3, 1), (2, 2), (2, 3), (MAX_SAFE_INTEGER, MAX_SAFE_INTEGER)]
    {
        let mut document = persisted_fixture();
        document["number"] = json!(number);
        document["predecessor"]["number"] = json!(predecessor_number);
        assert!(
            serde_json::from_value::<PersistedGraphVersion>(document).is_err(),
            "deserialized graph version {number} after predecessor {predecessor_number}"
        );
    }

    let mut without_predecessor = persisted_fixture();
    without_predecessor["number"] = json!(3);
    without_predecessor["predecessor"] = Value::Null;
    assert!(serde_json::from_value::<PersistedGraphVersion>(without_predecessor).is_ok());
}

#[test]
fn persisted_graph_version_rejects_space_as_timestamp_separator() {
    let mut document = persisted_fixture();
    document["createdAt"] = json!("2026-08-09 00:00:00Z");

    assert_schema_invalid(PERSISTED_ID, &document);
    assert!(serde_json::from_value::<PersistedGraphVersion>(document).is_err());
}

#[test]
fn policy_waiver_rejects_leap_second_outside_utc_end_of_day() {
    let document = json!({
        "id": "waiver-1",
        "requirement": "review",
        "executionId": "exec-1",
        "graphVersion": 1,
        "actor": "owner-1",
        "acknowledgedRisks": ["accepted-risk"],
        "scope": "execution",
        "createdAt": "2026-08-09T12:00:60Z",
        "expiresAt": null
    });

    assert_schema_invalid(POLICY_WAIVER_ID, &document);
    assert!(serde_json::from_value::<PolicyWaiver>(document).is_err());
}

#[test]
fn persistence_timestamps_require_canonical_utc_wire_form() {
    for created_at in [
        "0000-01-01T00:00:00+23:59",
        "9999-12-31T23:59:59-23:59",
        "2026-08-09t01:02:03z",
        "2026-08-09T01:02:03+00:00",
        "2026-08-09T01:02:03.1234567890Z",
    ] {
        let mut graph = persisted_fixture();
        graph["createdAt"] = json!(created_at);
        assert_schema_invalid(PERSISTED_ID, &graph);
        assert!(serde_json::from_value::<PersistedGraphVersion>(graph).is_err());

        let waiver = json!({
            "id": "waiver-1",
            "requirement": "review",
            "executionId": "exec-1",
            "graphVersion": 1,
            "actor": "owner-1",
            "acknowledgedRisks": ["accepted-risk"],
            "scope": "execution",
            "createdAt": created_at,
            "expiresAt": created_at
        });
        assert_schema_invalid(POLICY_WAIVER_ID, &waiver);
        assert!(serde_json::from_value::<PolicyWaiver>(waiver).is_err());
    }
}

#[test]
fn canonical_utc_timestamp_boundaries_match_schema_acceptance() {
    for created_at in [
        "0000-01-01T00:00:00Z",
        "9999-12-31T23:59:59Z",
        "2026-08-09T01:02:03.1Z",
        "2026-08-09T01:02:03.123456789Z",
        "2026-08-09T23:59:60Z",
        "2026-08-09T23:59:60.123456789Z",
    ] {
        let mut graph = persisted_fixture();
        graph["createdAt"] = json!(created_at);
        assert_schema_valid(PERSISTED_ID, &graph);
        let decoded = serde_json::from_value::<PersistedGraphVersion>(graph).unwrap();
        assert_schema_valid(PERSISTED_ID, &serde_json::to_value(decoded).unwrap());

        let waiver = json!({
            "id": "waiver-1",
            "requirement": "review",
            "executionId": "exec-1",
            "graphVersion": 1,
            "actor": "owner-1",
            "acknowledgedRisks": ["accepted-risk"],
            "scope": "execution",
            "createdAt": created_at,
            "expiresAt": created_at
        });
        assert_schema_valid(POLICY_WAIVER_ID, &waiver);
        let decoded = serde_json::from_value::<PolicyWaiver>(waiver).unwrap();
        assert_schema_valid(POLICY_WAIVER_ID, &serde_json::to_value(decoded).unwrap());
    }
}

#[test]
fn persisted_timestamp_serde_acceptance_matches_checked_in_schema() {
    let cases = [
        "0000-01-01T00:00:00Z",
        "9999-12-31T23:59:59Z",
        "2024-02-29T23:59:59Z",
        "2023-02-29T23:59:59Z",
        "2026-08-09t01:02:03z",
        "2026-08-09T01:02:03.123456789Z",
        "2026-08-09T01:02:03.1234567890Z",
        "2026-08-09T23:59:60Z",
        "2026-08-10T00:59:60+01:00",
        "2026-08-09T22:59:60-01:00",
        "2026-08-09T23:59:60+00:30",
        "2026-08-09T12:00:60Z",
        "2026-08-09 01:02:03Z",
        "2026-08-09T24:00:00Z",
        "2026-08-09T23:60:00Z",
        "2026-08-09T23:59:61Z",
        "2026-08-09T01:02:03.Z",
        "2026-08-09T01:02:03",
        "2026-08-09T01:02:03+23:59",
        "2026-08-09T01:02:03+24:00",
        "2026-08-09T01:02:03+0000",
    ];
    let schema = validator(PERSISTED_ID);

    for created_at in cases {
        let mut document = persisted_fixture();
        document["createdAt"] = json!(created_at);
        let schema_accepts = schema.is_valid(&document);
        let serde_accepts = serde_json::from_value::<PersistedGraphVersion>(document).is_ok();
        assert_eq!(
            serde_accepts, schema_accepts,
            "Serde/schema acceptance mismatch for {created_at}"
        );
    }
}

#[test]
fn persisted_projection_rejects_unknown_plaintext_and_unordered_slots() {
    let mut plaintext = persisted_fixture();
    plaintext["topology"]["nodes"]["start"]["objective"] = json!("must not persist");
    assert!(serde_json::from_value::<PersistedGraphVersion>(plaintext).is_err());

    let mut unordered = persisted_fixture();
    let second = json!({
        "slotId": "slot-description",
        "ownerKind": "graph",
        "ownerId": "graph-fixture",
        "fieldKind": "description",
        "ordinal": 0,
        "evidenceId": "evidence-description",
        "contentSha256": "1111111111111111111111111111111111111111111111111111111111111111",
        "sensitivity": "internal",
        "requiredForExecution": false
    });
    unordered["contentSlots"]
        .as_array_mut()
        .unwrap()
        .push(second);
    assert!(serde_json::from_value::<PersistedGraphVersion>(unordered).is_err());

    let mut duplicate_slot_id = persisted_fixture();
    let existing_slot_id = duplicate_slot_id["contentSlots"][0]["slotId"].clone();
    duplicate_slot_id["contentSlots"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "slotId": existing_slot_id,
            "ownerKind": "node",
            "ownerId": "zz",
            "fieldKind": "objective",
            "ordinal": 0,
            "evidenceId": "evidence-second",
            "contentSha256": "2222222222222222222222222222222222222222222222222222222222222222",
            "sensitivity": "internal",
            "requiredForExecution": true
        }));
    assert!(serde_json::from_value::<PersistedGraphVersion>(duplicate_slot_id).is_err());
}

#[test]
fn persistence_string_types_enforce_schema_boundaries() {
    assert!(OpaqueId::parse("!").is_ok());
    assert!(OpaqueId::parse("x".repeat(128)).is_ok());
    for invalid in [
        String::new(),
        "x".repeat(129),
        "with/slash".into(),
        "with space".into(),
    ] {
        assert!(OpaqueId::parse(invalid).is_err());
    }

    assert!(ActorId::parse("a".repeat(256)).is_ok());
    assert!(ActorId::parse("a".repeat(257)).is_err());
    assert!(ActorId::parse("-not-leading").is_err());

    assert!(RawSha256::parse("a".repeat(64)).is_ok());
    assert!(RawSha256::parse("A".repeat(64)).is_err());
    assert!(WireHash::parse(format!("sha256:{}", "f".repeat(64))).is_ok());
    assert!(WireHash::parse("f".repeat(64)).is_err());

    assert!(SafeKey::parse("retryPolicy").is_ok());
    assert!(SafeKey::parse("pro_mpt").is_err());
    assert!(SafeKey::parse("nestedDescriptionValue").is_err());
    assert!(SafeValue::parse("terminal_nodes:v1").is_ok());
    assert!(SafeValue::parse("human authored prose").is_err());
}

#[test]
fn persistence_domain_ids_are_nominal_and_wire_compatible() {
    assert_not_impl_any!(WorkspaceId: From<ProjectId>, From<ExecutionId>, From<EvidenceId>, From<ArtifactId>);
    assert_not_impl_any!(ProjectId: From<WorkspaceId>, From<ExecutionId>, From<EvidenceId>, From<ArtifactId>);
    assert_not_impl_any!(ExecutionId: From<WorkspaceId>, From<ProjectId>, From<EvidenceId>, From<ArtifactId>);
    assert_not_impl_any!(EvidenceId: From<WorkspaceId>, From<ProjectId>, From<ExecutionId>, From<ArtifactId>);
    assert_not_impl_any!(EvidenceId: From<OpaqueId>);
    assert_not_impl_any!(OpaqueId: From<EvidenceId>);
    assert_not_impl_any!(ArtifactId: From<WorkspaceId>, From<ProjectId>, From<ExecutionId>, From<EvidenceId>);
    assert_not_impl_any!(EventHash: From<WireHash>, Into<WireHash>);

    let workspace = WorkspaceId::parse("workspace-1").unwrap();
    let project = ProjectId::parse("project-1").unwrap();
    let execution = ExecutionId::parse("execution-1").unwrap();
    let evidence = EvidenceId::parse("evidence-1").unwrap();
    let artifact = ArtifactId::parse("artifact-1").unwrap();
    let event_hash = EventHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap();

    assert_eq!(
        serde_json::to_value(&workspace).unwrap(),
        json!("workspace-1")
    );
    assert_eq!(serde_json::to_value(&project).unwrap(), json!("project-1"));
    assert_eq!(
        serde_json::to_value(&execution).unwrap(),
        json!("execution-1")
    );
    assert_eq!(
        serde_json::to_value(&evidence).unwrap(),
        json!("evidence-1")
    );
    assert_eq!(
        serde_json::to_value(&artifact).unwrap(),
        json!("artifact-1")
    );
    assert_eq!(
        serde_json::to_value(&event_hash).unwrap(),
        json!(format!("sha256:{}", "a".repeat(64)))
    );
    assert_eq!(
        serde_json::from_value::<WorkspaceId>(json!("workspace-1")).unwrap(),
        workspace
    );
    assert_eq!(
        serde_json::from_value::<ProjectId>(json!("project-1")).unwrap(),
        project
    );
    assert_eq!(
        serde_json::from_value::<ExecutionId>(json!("execution-1")).unwrap(),
        execution
    );
    assert_eq!(
        serde_json::from_value::<EvidenceId>(json!("evidence-1")).unwrap(),
        evidence
    );
    assert_eq!(
        serde_json::from_value::<ArtifactId>(json!("artifact-1")).unwrap(),
        artifact
    );
    assert_eq!(
        serde_json::from_value::<EventHash>(json!(format!("sha256:{}", "a".repeat(64)))).unwrap(),
        event_hash
    );

    assert!(WorkspaceId::parse("with space").is_err());
    assert!(ProjectId::parse("with/slash").is_err());
    assert!(ExecutionId::parse(String::new()).is_err());
    assert!(EvidenceId::parse("x".repeat(129)).is_err());
    assert!(ArtifactId::parse("with/slash").is_err());
    assert!(EventHash::parse("a".repeat(64)).is_err());
    assert!(serde_json::from_value::<WorkspaceId>(json!("with space")).is_err());
    assert!(serde_json::from_value::<ProjectId>(json!("with/slash")).is_err());
    assert!(serde_json::from_value::<ExecutionId>(json!("")).is_err());
    assert!(serde_json::from_value::<EvidenceId>(json!("x".repeat(129))).is_err());
    assert!(serde_json::from_value::<ArtifactId>(json!("with/slash")).is_err());
    assert!(serde_json::from_value::<EventHash>(json!("a".repeat(64))).is_err());

    let reference = PersistedGraphVersionRef::new(
        1,
        WireHash::parse(format!("sha256:{}", "b".repeat(64))).unwrap(),
    )
    .unwrap();
    assert_eq!(reference.number(), 1);
}

#[test]
fn repository_scope_omits_optional_execution_and_is_closed() {
    let scope = RepositoryScope::new(
        WorkspaceId::parse("workspace-1").unwrap(),
        ProjectId::parse("project-1").unwrap(),
        None,
    );
    let encoded = serde_json::to_value(&scope).unwrap();
    assert_eq!(
        encoded,
        json!({"workspaceId":"workspace-1","projectId":"project-1"})
    );
    assert!(
        serde_json::from_value::<RepositoryScope>(json!({
            "workspaceId":"workspace-1", "projectId":"project-1", "path":"C:/private"
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<RepositoryScope>(json!({
            "workspaceId":"workspace-1", "projectId":"project-1", "executionId":null
        }))
        .is_err()
    );
}

#[test]
fn required_nullable_and_optional_non_null_fields_match_the_schema() {
    let mut missing_predecessor = persisted_fixture();
    missing_predecessor
        .as_object_mut()
        .unwrap()
        .remove("predecessor");
    assert!(serde_json::from_value::<PersistedGraphVersion>(missing_predecessor).is_err());

    let mut zero_predecessor = persisted_fixture();
    zero_predecessor["predecessor"]["number"] = json!(0);
    assert!(serde_json::from_value::<PersistedGraphVersion>(zero_predecessor).is_err());

    let mut null_budget = persisted_fixture();
    null_budget["topology"]["budgets"]["maxNodes"] = Value::Null;
    assert!(serde_json::from_value::<PersistedGraphVersion>(null_budget).is_err());

    assert!(
        serde_json::from_value::<PersistedDiagnostic>(json!({
            "code":"GHP001_SAFE", "severity":"error", "path":"/", "component":"governor",
            "sourceContentSha256":null
        }))
        .is_err()
    );
}

#[test]
fn evidence_and_artifact_references_are_closed_and_schema_exact() {
    let evidence = EvidenceReference::new(
        EvidenceId::parse("evidence-1").unwrap(),
        RawSha256::parse("a".repeat(64)).unwrap(),
        RawSha256::parse("b".repeat(64)).unwrap(),
    );
    assert_eq!(
        serde_json::to_value(evidence).unwrap(),
        json!({
            "evidenceId":"evidence-1",
            "contentSha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "ciphertextSha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        })
    );

    let artifact = ArtifactReference::new(
        ArtifactId::parse("artifact-1").unwrap(),
        ArtifactLocator::parse(format!("artifact://sha256/{}", "c".repeat(64))).unwrap(),
        RawSha256::parse("c".repeat(64)).unwrap(),
        MediaType::parse("application/json").unwrap(),
        42,
        Sensitivity::Internal,
        SemanticVersion::parse("1.0.0").unwrap(),
    )
    .unwrap();
    assert_schema_valid(ARTIFACT_ID, &serde_json::to_value(&artifact).unwrap());

    let mut unknown = serde_json::to_value(artifact).unwrap();
    unknown["path"] = json!("C:/private");
    assert!(serde_json::from_value::<ArtifactReference>(unknown).is_err());
}

#[test]
fn private_topology_constructors_validate_collections() {
    let control = PersistedControl::new(
        SafeValue::parse("terminal_nodes").unwrap(),
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
    )
    .unwrap();
    let node = PersistedNode::new(
        NodeType::Agent,
        Optionality::Required,
        vec![],
        vec![OpaqueId::parse("slot-objective").unwrap()],
        None,
    )
    .unwrap();
    let topology = PersistedTopology::new(
        OpaqueId::parse("graph-fixture").unwrap(),
        ExecutionId::parse("execution-fixture").unwrap(),
        BTreeMap::new(),
        vec![OpaqueId::parse("start").unwrap()],
        BTreeMap::from([(OpaqueId::parse("start").unwrap(), node)]),
        vec![],
        PersistedBudgets::default(),
        vec![],
        control,
    );
    assert!(topology.is_ok());

    let empty = PersistedTopology::new(
        OpaqueId::parse("graph-fixture").unwrap(),
        ExecutionId::parse("execution-fixture").unwrap(),
        BTreeMap::new(),
        vec![],
        BTreeMap::new(),
        vec![],
        PersistedBudgets::default(),
        vec![],
        PersistedControl::new(
            SafeValue::parse("terminal_nodes").unwrap(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
        )
        .unwrap(),
    );
    assert!(empty.is_err());
}

#[test]
fn safe_diagnostic_has_only_stable_bounded_fields() {
    let path = DiagnosticDomainPath::parse("/topology/nodes/start").unwrap();
    let source_content_sha256 = RawSha256::parse("a".repeat(64)).unwrap();
    let detail_evidence_id = EvidenceId::parse("evidence-detail").unwrap();
    let diagnostic = PersistedDiagnostic::new(
        "GHP001_SAFE".to_owned(),
        Severity::Error,
        path.clone(),
        DiagnosticComponent::Governor,
        Some(source_content_sha256.clone()),
        Some(detail_evidence_id.clone()),
    )
    .unwrap();
    assert_eq!(diagnostic.code(), "GHP001_SAFE");
    assert_eq!(diagnostic.severity(), &Severity::Error);
    assert_eq!(diagnostic.path(), &path);
    assert_eq!(diagnostic.component(), DiagnosticComponent::Governor);
    assert_eq!(
        diagnostic.source_content_sha256(),
        Some(&source_content_sha256)
    );
    assert_eq!(diagnostic.detail_evidence_id(), Some(&detail_evidence_id));
    assert_eq!(
        serde_json::to_value(&diagnostic).unwrap(),
        json!({
            "code":"GHP001_SAFE",
            "severity":"error",
            "path":"/topology/nodes/start",
            "component":"governor",
            "sourceContentSha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "detailEvidenceId":"evidence-detail"
        })
    );
    assert!(
        serde_json::from_value::<PersistedDiagnostic>(json!({
            "code":"GHP001_SAFE", "severity":"error", "path":"/", "component":"governor",
            "message":"plaintext", "source":"C:/private"
        }))
        .is_err()
    );
}

#[test]
fn diagnostic_domain_path_rejects_structurally_impossible_registered_descendants() {
    for invalid in [
        "/number/Users/gabri/secret.json",
        "/spec/.ssh/id_rsa",
        "/topology/home/gabri/private.yaml",
    ] {
        assert!(
            DiagnosticDomainPath::parse(invalid).is_err(),
            "structurally impossible path was accepted: {invalid}"
        );
    }
}

#[test]
fn diagnostic_domain_path_accepts_only_complete_registered_contract_shapes() {
    for valid in [
        "",
        "/apiVersion",
        "/metadata/id",
        "/metadata/labels/team-owner",
        "/spec/nodes/node@1",
        "/spec/nodes/node@1/objective",
        "/spec/edges/0/from",
        "/topology/nodes/node@1/contentSlotIds/0",
        "/topology/nodes/node~01",
        "/topology/completion/identifiers/route_key",
        "/contentSlots/0/evidenceId",
        "/scope/workspaceId",
        "/wrappedKey/keyId",
        "/kind/data/diagnostics/0/path",
        "/kind/data/version/topology/nodes/node@1/nodeType",
        "/artifactRefs/0/locator",
    ] {
        let path = DiagnosticDomainPath::parse(valid).unwrap();
        assert_eq!(path.as_str(), valid);
    }

    for invalid in [
        // Previously verified structural bypasses under registered scalar/object roots.
        "/number/Users/gabri/secret.json",
        "/spec/.ssh/id_rsa",
        "/topology/home/gabri/private.yaml",
        // Unknown fields and descendants under scalar fields.
        "/spec/nodes/node@1/unknownField",
        "/topology/nodes/node@1/nodeType/child",
        "/kind/data/unknown",
        "/wrappedKey/keyId/child",
        // Wrong array indices and map-key grammars.
        "/spec/edges/-1",
        "/spec/edges/01",
        "/spec/edges/4096",
        "/topology/entrypoints/node@1",
        "/topology/nodes/C:/nodeType",
        "/topology/nodes/node@1/contentSlotIds/64",
        "/topology/completion/identifiers/content_key",
        // Non-domain source shapes remain rejected.
        "/home/user/secret",
        "/C:/Users/name",
        "C:/Users/name",
        "graph.yaml",
        "relative/graph.yaml",
        r"\\server\share",
        r"/topology/C:\Users\name",
        "file:///tmp/secret",
        "/topology/../secret",
        "/topology/%2e%2e/secret",
        "/topology/~1etc~1passwd",
    ] {
        assert!(
            DiagnosticDomainPath::parse(invalid).is_err(),
            "filesystem-shaped path was accepted: {invalid}"
        );

        let document = json!({
            "code": "GHP001_SAFE",
            "severity": "error",
            "path": invalid,
            "component": "governor"
        });
        assert!(serde_json::from_value::<PersistedDiagnostic>(document).is_err());
    }
}

#[test]
fn diagnostic_domain_path_uses_the_exact_opaque_id_character_grammar() {
    for byte in 0_u8..=127 {
        let character = char::from(byte);
        let candidate = format!("node{character}1");
        let pointer_token = candidate.replace('~', "~0").replace('/', "~1");
        let path = format!("/topology/nodes/{pointer_token}");
        assert_eq!(
            DiagnosticDomainPath::parse(path).is_ok(),
            OpaqueId::parse(candidate).is_ok(),
            "diagnostic path and OpaqueId disagreed for ASCII byte {byte:#04x}"
        );
    }
}

#[test]
fn persistence_read_api_exposes_budget_edge_and_topology_fields() {
    let budgets = PersistedBudgets::new(
        Some(10),
        Some(11),
        Some(12),
        Some(13),
        Some(14),
        Some(15.5),
        Some(16),
    )
    .unwrap();
    assert_eq!(budgets.max_nodes(), Some(10));
    assert_eq!(budgets.max_depth(), Some(11));
    assert_eq!(budgets.max_mutations(), Some(12));
    assert_eq!(budgets.max_retries_per_node(), Some(13));
    assert_eq!(budgets.max_wall_clock_seconds(), Some(14));
    assert_eq!(budgets.max_api_cost_usd(), Some(15.5));
    assert_eq!(budgets.max_parallel_model_calls(), Some(16));

    let condition = PersistedControl::new(
        SafeValue::parse("edge_condition").unwrap(),
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
    )
    .unwrap();
    let bindings = BTreeMap::from([(
        SafeKey::parse("resultBinding").unwrap(),
        SafeValue::parse("node.result").unwrap(),
    )]);
    let edge = PersistedEdge::new(
        OpaqueId::parse("start-to-finish").unwrap(),
        OpaqueId::parse("start").unwrap(),
        OpaqueId::parse("finish").unwrap(),
        EdgeType::Control,
        Some(7),
        bindings.clone(),
        Some(condition.clone()),
    )
    .unwrap();
    assert_eq!(edge.priority(), Some(7));
    assert_eq!(edge.bindings(), &bindings);
    assert_eq!(edge.condition(), Some(&condition));

    let completion = PersistedControl::new(
        SafeValue::parse("terminal_nodes").unwrap(),
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
    )
    .unwrap();
    let node =
        PersistedNode::new(NodeType::Agent, Optionality::Required, vec![], vec![], None).unwrap();
    let topology = PersistedTopology::new(
        OpaqueId::parse("graph-fixture").unwrap(),
        ExecutionId::parse("execution-fixture").unwrap(),
        BTreeMap::new(),
        vec![OpaqueId::parse("start").unwrap()],
        BTreeMap::from([(OpaqueId::parse("start").unwrap(), node)]),
        vec![edge],
        budgets,
        vec![],
        completion,
    )
    .unwrap();
    assert_eq!(topology.api_version(), "p50.dev/graph/v1");
    assert_eq!(topology.kind(), "ExecutionGraph");
}

#[test]
fn sensitivity_rejects_unknown_wire_variant() {
    assert!(serde_json::from_value::<Sensitivity>(json!("secret")).is_err());
}

#[test]
fn policy_waiver_omits_absent_reason_and_rejects_null_or_unbounded_input() {
    let waiver = PolicyWaiver {
        id: "waiver-1".into(),
        requirement: "review".into(),
        execution_id: "exec-1".into(),
        graph_version: 1,
        actor: "owner-1".into(),
        reason: None,
        acknowledged_risks: vec!["accepted-risk".into()],
        scope: WaiverScope::Execution,
        created_at: Utc.with_ymd_and_hms(2026, 8, 9, 0, 0, 0).unwrap(),
        expires_at: None,
    };
    let encoded = serde_json::to_value(&waiver).unwrap();
    assert!(encoded.get("reason").is_none());
    assert_schema_valid(POLICY_WAIVER_ID, &encoded);
    assert_eq!(
        serde_json::from_value::<PolicyWaiver>(encoded).unwrap(),
        waiver
    );

    let mut null_reason = serde_json::to_value(&waiver).unwrap();
    null_reason["reason"] = Value::Null;
    assert!(serde_json::from_value::<PolicyWaiver>(null_reason).is_err());

    let mut unknown = serde_json::to_value(&waiver).unwrap();
    unknown["message"] = json!("not durable");
    assert!(serde_json::from_value::<PolicyWaiver>(unknown).is_err());

    let mut empty_risks = serde_json::to_value(&waiver).unwrap();
    empty_risks["acknowledgedRisks"] = json!([]);
    assert!(serde_json::from_value::<PolicyWaiver>(empty_risks).is_err());
}

#[test]
fn invalid_public_policy_waiver_cannot_cross_the_serialization_boundary() {
    let valid = PolicyWaiver {
        id: "waiver-1".into(),
        requirement: "review".into(),
        execution_id: "exec-1".into(),
        graph_version: 1,
        actor: "owner-1".into(),
        reason: None,
        acknowledged_risks: vec!["accepted-risk".into()],
        scope: WaiverScope::Execution,
        created_at: Utc.with_ymd_and_hms(2026, 8, 9, 0, 0, 0).unwrap(),
        expires_at: None,
    };
    assert!(serde_json::to_value(&valid).is_ok());

    let mut invalid_version = valid.clone();
    invalid_version.graph_version = 0;
    assert!(serde_json::to_value(&invalid_version).is_err());

    let mut invalid_id = valid.clone();
    invalid_id.execution_id = "with space".into();
    assert!(serde_json::to_value(&invalid_id).is_err());

    let mut invalid_actor = valid.clone();
    invalid_actor.actor = "-invalid".into();
    assert!(serde_json::to_value(&invalid_actor).is_err());

    let mut missing_risk = valid.clone();
    missing_risk.acknowledged_risks.clear();
    assert!(serde_json::to_value(&missing_risk).is_err());

    let mut oversized_reason = valid.clone();
    oversized_reason.reason = Some("x".repeat(2049));
    assert!(serde_json::to_value(&oversized_reason).is_err());

    let mut oversized_risk = valid.clone();
    oversized_risk.acknowledged_risks = vec!["x".repeat(513)];
    assert!(serde_json::to_value(&oversized_risk).is_err());

    let mut before_profile_timestamp = valid.clone();
    before_profile_timestamp.created_at = Utc.with_ymd_and_hms(-1, 1, 1, 0, 0, 0).unwrap();
    assert!(serde_json::to_value(&before_profile_timestamp).is_err());

    let mut out_of_profile_timestamp = valid;
    out_of_profile_timestamp.created_at = Utc.with_ymd_and_hms(10_000, 1, 1, 0, 0, 0).unwrap();
    assert!(serde_json::to_value(&out_of_profile_timestamp).is_err());
}

#[test]
fn safe_projection_constructors_keep_typed_content_only() {
    let slot = ContentSlot::new(
        OpaqueId::parse("slot-objective").unwrap(),
        ContentOwnerKind::Node,
        OpaqueId::parse("start").unwrap(),
        ContentFieldKind::Objective,
        0,
        EvidenceId::parse("evidence-objective").unwrap(),
        RawSha256::parse("a".repeat(64)).unwrap(),
        Sensitivity::Internal,
        true,
    );
    assert_eq!(slot.ordinal(), 0);

    let edge = PersistedEdge::new(
        OpaqueId::parse("a-to-b").unwrap(),
        OpaqueId::parse("a").unwrap(),
        OpaqueId::parse("b").unwrap(),
        EdgeType::Control,
        None,
        BTreeMap::new(),
        None,
    )
    .unwrap();
    assert_eq!(edge.id().as_str(), "a-to-b");

    assert_not_impl_any!(GraphVersionRecord: Into<GraphVersionPublished>);
    assert_not_impl_any!(GraphVersionRecord: Into<PersistedGraphVersion>);
}

#[test]
fn path_content_field_kinds_have_exact_closed_wire_values() {
    for (kind, wire) in [
        (ContentFieldKind::ContextPath, "context_path"),
        (ContentFieldKind::PermissionPath, "permission_path"),
        (ContentFieldKind::IsolationPath, "isolation_path"),
    ] {
        assert_eq!(serde_json::to_value(kind).unwrap(), json!(wire));
        assert_eq!(
            serde_json::from_value::<ContentFieldKind>(json!(wire)).unwrap(),
            kind
        );
    }

    assert!(
        serde_json::from_value::<ContentFieldKind>(json!("filesystem_path")).is_err(),
        "the closed enum must reject an unregistered path position"
    );
}

#[test]
fn persisted_schema_accepts_only_registered_path_content_field_kinds() {
    for wire in ["context_path", "permission_path", "isolation_path"] {
        let mut projection = persisted_fixture();
        projection["contentSlots"][0]["fieldKind"] = json!(wire);
        assert_schema_valid(PERSISTED_ID, &projection);
    }

    let mut projection = persisted_fixture();
    projection["contentSlots"][0]["fieldKind"] = json!("filesystem_path");
    assert_schema_invalid(PERSISTED_ID, &projection);
}

/// `NodeState::Ghost` is part of the one shared vocabulary (see
/// `graphhelm_protocols::simulation::NodeState`), so the wire schema's `nodeState` enum must
/// accept `"ghost"` too. This guards against the Rust vocabulary and the wire contract drifting
/// apart again.
/// Builds a full envelope around `kind`, validates it against the schema, then round-trips it
/// through `EventEnvelope` and back, re-validating the re-encoded form.
fn assert_envelope_valid(kind: Value) {
    let document = event_fixture(kind, false);
    assert_schema_valid(EVENT_ID, &document);
    let envelope: EventEnvelope = serde_json::from_value(document.clone()).unwrap();
    let encoded = serde_json::to_value(&envelope).unwrap();
    assert_schema_valid(EVENT_ID, &encoded);
    assert_eq!(
        serde_json::from_value::<EventEnvelope>(encoded).unwrap(),
        envelope
    );
}

#[test]
fn node_state_changed_accepts_the_ghost_state_on_the_wire() {
    assert_envelope_valid(json!({
        "type":"node_state_changed",
        "data":{
            "simulationId":"simulation-1",
            "nodeId":"start",
            "previousState":null,
            "nextState":"ghost"
        }
    }));
}

#[test]
fn execution_event_kinds_round_trip_with_exact_wire_names() {
    let hash = format!("sha256:{}", "a".repeat(64));
    let cases = [
        (
            "execution_started",
            json!({"executionId":"execution-1","graphVersion":3,"graphHash":hash,"mode":"supervised"}),
        ),
        (
            "execution_mode_changed",
            json!({"executionId":"execution-1","previousMode":"supervised","mode":"manual"}),
        ),
        (
            "node_outcome_recorded",
            json!({"executionId":"execution-1","nodeId":"start","outcome":"retryable_failure","nextState":"queued"}),
        ),
        (
            "execution_completed",
            json!({"executionId":"execution-1","status":"failed"}),
        ),
    ];
    for (name, data) in cases {
        let wire = json!({"type": name, "data": data});
        let kind: EventKind = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&kind).unwrap(), wire, "{name}");
    }
}

/// An absent mode must be an error too. Rejecting only unknown strings leaves the more dangerous
/// hole open: if `mode` ever gained a serde default, a payload omitting it would deserialize to
/// Autopilot, the most permissive mode, and no test would notice.
#[test]
fn an_execution_cannot_start_without_a_mode() {
    let hash = format!("sha256:{}", "a".repeat(64));
    let wire = json!({
        "type": "execution_started",
        "data": {"executionId":"execution-1","graphVersion":3,"graphHash":hash}
    });
    assert!(serde_json::from_value::<EventKind>(wire).is_err());
}

/// An unknown mode must not silently become the permissive one.
#[test]
fn an_execution_cannot_start_in_an_unknown_mode() {
    let hash = format!("sha256:{}", "a".repeat(64));
    let wire = json!({
        "type": "execution_started",
        "data": {"executionId":"execution-1","graphVersion":3,"graphHash":hash,"mode":"god_mode"}
    });
    assert!(serde_json::from_value::<EventKind>(wire).is_err());
}

#[test]
fn the_envelope_schema_accepts_every_execution_event_kind() {
    let hash = format!("sha256:{}", "a".repeat(64));
    for data in [
        json!({"type":"execution_started","data":{"executionId":"execution-1","graphVersion":3,"graphHash":hash,"mode":"supervised"}}),
        json!({"type":"execution_mode_changed","data":{"executionId":"execution-1","previousMode":null,"mode":"manual"}}),
        json!({"type":"node_outcome_recorded","data":{"executionId":"execution-1","nodeId":"start","outcome":"succeeded","nextState":"succeeded"}}),
        json!({"type":"execution_completed","data":{"executionId":"execution-1","status":"completed"}}),
    ] {
        assert_envelope_valid(data);
    }
}

/// The three governance kinds are execution-scoped, exactly like the durable execution kinds
/// above. Modelled on `the_envelope_schema_accepts_every_execution_event_kind`.
#[test]
fn the_envelope_schema_accepts_every_governance_event_kind() {
    let digest = "a".repeat(64);
    for data in [
        json!({"type":"signal_recorded","data":{"executionId":"execution-1","signalId":"signal-1","sourceKind":"node","sourceId":"node-a","kind":"unexpected_dependency","severity":"high","envelopeSha256":digest}}),
        json!({"type":"ghost_node_proposed","data":{"executionId":"execution-1","nodeId":"ghost-a","draftId":"draft-1"}}),
        json!({"type":"mutation_accepted","data":{"executionId":"execution-1","draftId":"draft-1","mode":"autopilot","graphVersion":4}}),
    ] {
        assert_envelope_valid(data);
    }
}

/// The envelope schema accepts the two lifecycle kinds themselves, and the widened `nodeOutcome`
/// and `simulationStatus` enums accept the new values they gained: `interrupted` (paired with the
/// `Blocked` consequence the transition table requires) and `cancelled`. Modelled on
/// `the_envelope_schema_accepts_every_execution_event_kind`.
#[test]
fn the_envelope_schema_accepts_the_lifecycle_event_kinds() {
    for data in [
        json!({"type":"execution_paused","data":{"executionId":"execution-1"}}),
        json!({"type":"execution_resumed","data":{"executionId":"execution-1"}}),
        json!({"type":"execution_completed","data":{"executionId":"execution-1","status":"cancelled"}}),
        json!({"type":"node_outcome_recorded","data":{"executionId":"execution-1","nodeId":"start","outcome":"interrupted","nextState":"blocked"}}),
    ] {
        assert_envelope_valid(data);
    }
}
