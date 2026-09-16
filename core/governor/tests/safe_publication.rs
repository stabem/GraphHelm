#![recursion_limit = "256"]

use std::{
    future::Future,
    path::Path,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    thread,
};

use chrono::{TimeZone, Utc};
use graphhelm_events::{
    AuthenticateRequest, AuthenticationTag, EvidenceError, EvidenceInput, EvidenceProtector,
    EvidenceSealer, KeyError, KeyProvider, KeyProviderMetadata, RepositoryFuture,
    RevocationReceipt, RevokeKeyRequest, SealedEvidence, SecretBytes, VerifyAuthenticationRequest,
    WrapKeyRequest, WrappedKey,
};
use graphhelm_governor::{GraphExternalizer, SealingGraphExternalizer};
use graphhelm_governor::{
    PublicationPreparationObserver, PublicationPreparationServices, PublicationStage,
    prepare_draft_publication, prepare_draft_publication_observed,
};
use graphhelm_graph::{
    GraphVersion, decode_persisted_reference, encode_persisted_reference, persisted_hashes,
    raw_content_sha256, validate_persisted_projection, validate_persisted_references,
};
use graphhelm_protocols::{
    Actor, ActorType, Clock, ContentFieldKind, ContentOwnerKind, DraftOperation, EvidenceId,
    EvidenceReference, ExecutionId, GraphDraft, GraphEdge, GraphNode, GraphVersionRecord,
    GraphVersionRef, ManualOverride, PersistedGraphVersionRef, ProjectId, RawSha256,
    RepositoryScope, Sensitivity, WaiverScope, WireHash, WorkspaceId,
};

fn block_on<F: Future>(future: F) -> F::Output {
    struct ThreadWaker(thread::Thread);
    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match Pin::as_mut(&mut future).poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => thread::park(),
        }
    }
}

struct FixedKeyProvider;

impl KeyProvider for FixedKeyProvider {
    fn metadata<'a>(&'a self) -> RepositoryFuture<'a, Result<KeyProviderMetadata, KeyError>> {
        Box::pin(async { KeyProviderMetadata::new("fixed-key", "fixed", "1.0.0", 0) })
    }

    fn wrap<'a>(
        &'a self,
        request: WrapKeyRequest,
    ) -> RepositoryFuture<'a, Result<WrappedKey, KeyError>> {
        Box::pin(async move {
            let aad_sha256 = raw_content_sha256(request.aad()).unwrap();
            WrappedKey::new(
                "fixed-key",
                request.handle(),
                "xchacha20poly1305",
                vec![1; 24],
                vec![2; 48],
                aad_sha256,
            )
        })
    }

    fn unwrap<'a>(
        &'a self,
        _wrapped: WrappedKey,
    ) -> RepositoryFuture<'a, Result<SecretBytes, KeyError>> {
        Box::pin(async { Err(KeyError::Unavailable) })
    }

    fn revoke<'a>(
        &'a self,
        _request: RevokeKeyRequest,
    ) -> RepositoryFuture<'a, Result<RevocationReceipt, KeyError>> {
        Box::pin(async { Err(KeyError::Unavailable) })
    }

    fn authenticate<'a>(
        &'a self,
        _request: AuthenticateRequest,
    ) -> RepositoryFuture<'a, Result<AuthenticationTag, KeyError>> {
        Box::pin(async { Err(KeyError::Unavailable) })
    }

    fn verify<'a>(
        &'a self,
        _request: VerifyAuthenticationRequest,
    ) -> RepositoryFuture<'a, Result<(), KeyError>> {
        Box::pin(async { Err(KeyError::Unavailable) })
    }
}

fn scope(execution: &str) -> RepositoryScope {
    RepositoryScope::new(
        WorkspaceId::parse("workspace-test").unwrap(),
        ProjectId::parse("project-test").unwrap(),
        Some(ExecutionId::parse(execution).unwrap()),
    )
}

fn version(name: &str) -> GraphVersion {
    let graph = graphhelm_schema::load_graph(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/graphs")
            .join(name),
    )
    .unwrap()
    .graph;
    GraphVersion::publish(
        graph,
        None,
        Actor::new(ActorType::Owner, "owner-test"),
        Utc.with_ymd_and_hms(2026, 8, 9, 12, 0, 0).unwrap(),
    )
    .unwrap()
}

fn externalizer() -> SealingGraphExternalizer<EvidenceProtector<FixedKeyProvider>> {
    SealingGraphExternalizer::new(EvidenceProtector::new(FixedKeyProvider))
}

fn refresh_record(version: &mut GraphVersionRecord) {
    version.semantic = graphhelm_graph::canonicalize(&version.graph).unwrap().value;
    version.content_hash = graphhelm_graph::semantic_hash(&version.graph).unwrap();
}

fn registered_controls_record() -> GraphVersionRecord {
    let mut record = version("software-feature.yaml").to_record();
    record
        .graph
        .spec
        .nodes
        .get_mut("map_repository")
        .unwrap()
        .properties
        .insert(
            "agent".into(),
            serde_json::json!({"ref": "project/security-reviewer@3"}),
        );
    let node = record.graph.spec.nodes.get_mut("plan").unwrap();
    let ephemeral = node
        .properties
        .get_mut("agent")
        .unwrap()
        .get_mut("ephemeral")
        .unwrap()
        .as_object_mut()
        .unwrap();
    ephemeral.remove("instructions");
    ephemeral.insert(
        "instructionsRef".into(),
        serde_json::json!("contract://planning-instructions@2"),
    );
    ephemeral.insert(
        "allowedTools".into(),
        serde_json::json!(["repository.read", "tests.execute_targeted"]),
    );
    ephemeral.insert(
        "prohibitedActions".into(),
        serde_json::json!(["production.deploy"]),
    );
    ephemeral.insert(
        "modelRequirements".into(),
        serde_json::json!({"profile": "critical_reasoning"}),
    );
    ephemeral.insert(
        "contextStrategy".into(),
        serde_json::json!({"includeScopes": ["project", "tests"], "maxTokens": 24000}),
    );
    ephemeral.insert(
        "evidenceRequirements".into(),
        serde_json::json!([{"type": "source_location", "min": 1}]),
    );
    ephemeral.insert(
        "memoryPolicy".into(),
        serde_json::json!({"writeCandidates": true, "defaultTtlDays": 90}),
    );
    node.properties.insert(
        "model".into(),
        serde_json::json!({
            "profile": "software_execution",
            "routePolicy": "dynamic",
            "requireIndependentFrom": ["implement"]
        }),
    );
    node.properties.insert(
        "input".into(),
        serde_json::json!({
            "schema": "schema://PlanningRequest@1",
            "bindings": {"request": "context://task/request"},
            "title": "not semantic"
        }),
    );
    node.properties.insert(
        "output".into(),
        serde_json::json!({
            "schema": "schema://PlanningResult@1",
            "bindings": {"plan": format!("artifact://sha256/{}", "b".repeat(64))},
            "publishAs": format!("artifact://sha256/{}", "c".repeat(64)),
            "examples": ["not semantic"]
        }),
    );
    node.properties.insert(
        "context".into(),
        serde_json::json!({
            "policyRef": "context-policy://blind-review@1",
            "include": [
                {"type": "project_kernel"},
                {"type": "document", "ref": "document://architecture/auth"},
                {"type": "dependency_output", "node": "implement"}
            ],
            "exclude": ["executor_subjective_summary"],
            "conflicts": "present_all",
            "freshness": {"maxAgeDays": 30, "requireRevalidationFor": ["source_code_claims"]},
            "budget": {"initialTokens": 12000, "maxTokens": 24000},
            "expansion": {"allowed": true, "requiresReason": true}
        }),
    );
    node.properties.insert(
        "permissions".into(),
        serde_json::json!([
            "repository.read",
            {"capability": "network.request", "scope": {"allowlist": ["api.example.com"]}, "duration": "call"}
        ]),
    );
    node.properties.insert(
        "isolation".into(),
        serde_json::json!({
            "minimum": "tier_2",
            "filesystem": {"mode": "execution_worktree"},
            "network": {"mode": "allowlist", "allowlist": ["api.example.com"]},
            "secrets": {"mode": "broker_only"},
            "resources": {"cpu": 4, "memoryMb": 8192, "diskMb": 20480}
        }),
    );
    node.properties.insert(
        "retry".into(),
        serde_json::json!({
            "maxAttempts": 3,
            "backoff": "exponential",
            "maxBackoffSeconds": 120,
            "retryOn": ["transient_provider_error", "tool_timeout"],
            "doNotRetryOn": ["policy_denied", "invalid_user_input"],
            "beforeRetry": ["reset_sandbox", "recompile_context"]
        }),
    );
    node.properties.insert(
        "resources".into(),
        serde_json::json!({"cpu": 2, "memoryMb": 4096, "diskMb": 10240}),
    );
    node.properties.insert(
        "memory".into(),
        serde_json::json!({"writeCandidates": false, "defaultTtlDays": 30}),
    );
    node.properties
        .insert("timeoutSeconds".into(), serde_json::json!(600));
    node.properties
        .insert("userEditable".into(), serde_json::json!(true));
    node.properties
        .insert("userOverrideAllowed".into(), serde_json::json!(false));
    let edge = record.graph.spec.edges.first_mut().unwrap();
    edge.payload_schema = Some("schema://PlanTransition@1".into());
    edge.bindings
        .insert("request".into(), "context://task/request".into());
    edge.bindings.insert(
        "artifact".into(),
        format!("artifact://sha256/{}", "a".repeat(64)),
    );
    edge.condition = Some(serde_json::json!("nodes.plan.output.ready == true"));
    edge.on_unknown = Some(graphhelm_protocols::UnknownConditionBehavior::Pause);
    refresh_record(&mut record);
    assert!(
        graphhelm_schema::validate_graph_value(
            &serde_json::to_value(&record.graph).unwrap(),
            "registered-controls"
        )
        .is_empty()
    );
    record
}

fn record_with_registered_path_scopes() -> GraphVersionRecord {
    let mut record = registered_controls_record();
    let node = record.graph.spec.nodes.get_mut("plan").unwrap();
    node.properties.insert(
        "context".into(),
        serde_json::json!({
            "include": [
                {"type": "project_kernel"},
                {"type": "source_scope", "paths": ["src/auth/**", "tests/auth/**"]}
            ]
        }),
    );
    node.properties.insert(
        "permissions".into(),
        serde_json::json!([{
            "capability": "repository.read",
            "scope": {"paths": ["src/auth/**", "tests/auth/**"]},
            "duration": "node"
        }]),
    );
    node.properties.insert(
        "isolation".into(),
        serde_json::json!({
            "minimum": "tier_2",
            "filesystem": {"mode": "execution_worktree", "writablePaths": ["workspace/output", "workspace/tmp"]}
        }),
    );
    refresh_record(&mut record);
    record
}

#[test]
fn registered_path_scopes_are_externalized_as_exact_typed_slots() {
    let record = record_with_registered_path_scopes();
    let prepared =
        block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
            .unwrap();
    let slots = prepared
        .version()
        .content_slots()
        .iter()
        .filter(|slot| {
            matches!(
                slot.field_kind(),
                ContentFieldKind::ContextPath
                    | ContentFieldKind::PermissionPath
                    | ContentFieldKind::IsolationPath
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(slots.len(), 6);
    for (kind, expected) in [
        (ContentFieldKind::ContextPath, vec![64, 65]),
        (ContentFieldKind::PermissionPath, vec![0, 1]),
        (ContentFieldKind::IsolationPath, vec![0, 1]),
    ] {
        assert_eq!(
            slots
                .iter()
                .filter(|slot| slot.field_kind() == kind)
                .map(|slot| slot.ordinal())
                .collect::<Vec<_>>(),
            expected
        );
    }
    let wire = serde_json::to_string(prepared.version()).unwrap();
    for canary in [
        "src/auth/**",
        "tests/auth/**",
        "workspace/output",
        "workspace/tmp",
    ] {
        assert!(!wire.contains(canary));
    }
    validate_persisted_projection(prepared.version()).unwrap();
}

fn assert_typed_path_slot_count(field: ContentFieldKind, expected: usize) {
    let record = record_with_registered_path_scopes();
    let prepared =
        block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
            .unwrap();
    assert_eq!(
        prepared
            .version()
            .content_slots()
            .iter()
            .filter(|slot| slot.field_kind() == field)
            .count(),
        expected
    );
}

#[test]
fn context_paths_have_a_dedicated_projection_gate() {
    assert_typed_path_slot_count(ContentFieldKind::ContextPath, 2);
}

#[test]
fn permission_paths_have_a_dedicated_projection_gate() {
    assert_typed_path_slot_count(ContentFieldKind::PermissionPath, 2);
}

#[test]
fn isolation_paths_have_a_dedicated_projection_gate() {
    assert_typed_path_slot_count(ContentFieldKind::IsolationPath, 2);
}

#[test]
fn path_content_and_typed_position_have_distinct_hash_semantics() {
    let baseline = record_with_registered_path_scopes();
    let scope = scope(&baseline.graph.metadata.execution_id);
    let original = block_on(externalizer().prepare(scope.clone(), &baseline)).unwrap();

    let mut content_changed = baseline.clone();
    content_changed
        .graph
        .spec
        .nodes
        .get_mut("plan")
        .unwrap()
        .properties
        .get_mut("context")
        .unwrap()["include"][1]["paths"][0] = serde_json::json!("src/security/**");
    refresh_record(&mut content_changed);
    let content = block_on(externalizer().prepare(scope.clone(), &content_changed)).unwrap();
    assert_eq!(
        original.version().topology_hash(),
        content.version().topology_hash()
    );
    assert_ne!(
        original.version().semantic_hash(),
        content.version().semantic_hash()
    );

    let mut position_changed = baseline;
    position_changed
        .graph
        .spec
        .nodes
        .get_mut("plan")
        .unwrap()
        .properties
        .get_mut("context")
        .unwrap()["include"]
        .as_array_mut()
        .unwrap()
        .swap(0, 1);
    refresh_record(&mut position_changed);
    let position = block_on(externalizer().prepare(scope, &position_changed)).unwrap();
    assert_ne!(
        original.version().topology_hash(),
        position.version().topology_hash()
    );
    assert_ne!(
        original.version().semantic_hash(),
        position.version().semantic_hash()
    );
}

#[test]
fn path_scope_limits_fail_before_sealing_and_sealer_failure_is_redacted() {
    for paths in [serde_json::json!([]), serde_json::json!(vec!["x"; 65])] {
        let mut record = registered_controls_record();
        record
            .graph
            .spec
            .nodes
            .get_mut("plan")
            .unwrap()
            .properties
            .get_mut("isolation")
            .unwrap()["filesystem"]["writablePaths"] = paths;
        refresh_record(&mut record);
        let calls = Arc::new(AtomicUsize::new(0));
        let externalizer = SealingGraphExternalizer::new(CountingFailureSealer(calls.clone()));
        assert!(
            block_on(externalizer.prepare(scope(&record.graph.metadata.execution_id), &record))
                .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    let record = record_with_registered_path_scopes();
    let calls = Arc::new(AtomicUsize::new(0));
    let externalizer = SealingGraphExternalizer::new(CountingFailureSealer(calls.clone()));
    let error = block_on(externalizer.prepare(scope(&record.graph.metadata.execution_id), &record))
        .unwrap_err();
    assert_eq!(error, graphhelm_governor::GovernorError::SealingFailed);
    assert!(calls.load(Ordering::SeqCst) > 0);
    let surface = format!("{error:?} {error}");
    assert!(!surface.contains("src/auth/**"));
    assert!(!surface.contains("workspace/output"));
}

#[test]
fn completion_artifacts_are_normalized_into_the_artifact_reference_domain() {
    let mut record = registered_controls_record();
    record
        .graph
        .spec
        .nodes
        .get_mut("plan")
        .unwrap()
        .properties
        .insert(
            "completion".into(),
            serde_json::json!({
                "requires": [{"artifactExists": "implementation.patch"}],
                "forbids": [{"artifactExists": "unsafe-output"}]
            }),
        );
    refresh_record(&mut record);
    let prepared =
        block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
            .unwrap();
    let completion = control(&prepared, "plan", "node_completion");
    assert_eq!(
        decode_reference(identifier(completion, "requiresArtifact.000")),
        "artifact://implementation.patch"
    );
    assert_eq!(
        decode_reference(identifier(completion, "forbidsArtifact.000")),
        "artifact://unsafe-output"
    );
    validate_persisted_projection(prepared.version()).unwrap();
}

#[test]
fn whole_record_preflight_rejects_huge_non_value_scalars_before_sealing() {
    let mut record = registered_controls_record();
    record.graph.metadata.name = "n".repeat(16 * 1024 * 1024 + 1);
    assert_eq!(
        graphhelm_graph::preflight_graph_version_record_values(&record),
        Err(graphhelm_graph::DurableContentError::LimitExceeded)
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let externalizer = SealingGraphExternalizer::new(CountingFailureSealer(calls.clone()));
    let error = block_on(externalizer.prepare(scope(&record.graph.metadata.execution_id), &record))
        .unwrap_err();
    assert_eq!(error, graphhelm_governor::GovernorError::LimitExceeded);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn replay_rejects_rehashed_noncanonical_node_control_order() {
    let record = registered_controls_record();
    let prepared =
        block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
            .unwrap();
    let mut wire = serde_json::to_value(prepared.version()).unwrap();
    wire["topology"]["nodes"]["plan"]["controls"]
        .as_array_mut()
        .unwrap()
        .reverse();
    let reordered = rehashed_projection(wire);
    assert!(validate_persisted_projection(&reordered).is_err());
}

#[test]
fn model_and_context_relations_require_existing_nonself_nodes() {
    for property in [
        serde_json::json!({"model": {"requireIndependentFrom": ["ghost"]}}),
        serde_json::json!({"context": {"include": [{"type": "dependency_output", "node": "ghost"}]}}),
        serde_json::json!({"model": {"requireIndependentFrom": ["plan"]}}),
        serde_json::json!({"context": {"include": [{"type": "dependency_output", "node": "plan"}]}}),
    ] {
        let mut record = registered_controls_record();
        record
            .graph
            .spec
            .nodes
            .get_mut("plan")
            .unwrap()
            .properties
            .extend(property.as_object().unwrap().clone());
        refresh_record(&mut record);
        let calls = Arc::new(AtomicUsize::new(0));
        let externalizer = SealingGraphExternalizer::new(CountingFailureSealer(calls.clone()));
        assert!(
            block_on(externalizer.prepare(scope(&record.graph.metadata.execution_id), &record))
                .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}

fn control<'a>(
    preparation: &'a graphhelm_governor::ProjectionPreparation,
    node_id: &str,
    control_type: &str,
) -> &'a graphhelm_protocols::PersistedControl {
    preparation
        .version()
        .topology()
        .nodes()
        .iter()
        .find(|(id, _)| id.as_str() == node_id)
        .unwrap()
        .1
        .controls()
        .iter()
        .find(|control| control.control_type().as_str() == control_type)
        .unwrap()
}

fn identifier<'a>(control: &'a graphhelm_protocols::PersistedControl, key: &str) -> &'a str {
    control
        .identifiers()
        .iter()
        .find(|(candidate, _)| candidate.as_str() == key)
        .unwrap()
        .1
        .as_str()
}

fn integer(control: &graphhelm_protocols::PersistedControl, key: &str) -> i64 {
    *control
        .integers()
        .iter()
        .find(|(candidate, _)| candidate.as_str() == key)
        .unwrap()
        .1
}

fn flag(control: &graphhelm_protocols::PersistedControl, key: &str) -> bool {
    *control
        .flags()
        .iter()
        .find(|(candidate, _)| candidate.as_str() == key)
        .unwrap()
        .1
}

fn decode_reference(encoded: &str) -> String {
    decode_persisted_reference(encoded).unwrap()
}

fn normative_node(kind: &str, properties: serde_json::Value) -> GraphNode {
    let mut value = serde_json::json!({
        "type": kind,
        "name": format!("{kind} node"),
        "objective": format!("Execute {kind} deterministically"),
        "optionality": "required"
    });
    value
        .as_object_mut()
        .unwrap()
        .extend(properties.as_object().unwrap().clone());
    serde_json::from_value(value).unwrap()
}

fn node_kind_registry_record() -> GraphVersionRecord {
    let mut record = version("software-feature.yaml").to_record();
    let artifact = format!("artifact://sha256/{}", "d".repeat(64));
    let mut nodes = std::collections::BTreeMap::new();
    nodes.insert(
        "agent".into(),
        normative_node(
            "agent",
            serde_json::json!({
                "tags": ["foundation", "controlled"],
                "onCancel": "pause",
                "onFailure": "route_remediation",
                "agent": {"ephemeral": {
                    "purpose": "Carry out the bounded task",
                    "capabilities": ["repository.read"],
                    "inputSchema": "schema://AgentInput@1",
                    "outputSchema": "schema://AgentOutput@1",
                    "instructions": "Use only registered controls",
                    "completionContract": {"expression": "output.ready == true"}
                }},
                "completion": {
                    "requires": [
                        {"outputSchemaValid": true},
                        {"evidence": {"type": "source_location", "min": 1}},
                        {"expression": "output.confidence >= 0.7"}
                    ],
                    "forbids": [{"expression": "output.unsupportedClaims > 0"}]
                }
            }),
        ),
    );
    nodes.insert(
        "planner".into(),
        normative_node("planner", serde_json::json!({"tags": []})),
    );
    nodes.insert(
        "tool".into(),
        normative_node(
            "tool",
            serde_json::json!({"tool": {"ref": "builtin/test-runner@1", "action": "execute"}}),
        ),
    );
    nodes.insert(
        "classifier".into(),
        normative_node(
            "classifier",
            serde_json::json!({"classifier": {
                "method": "hybrid",
                "profile": "fast_classification",
                "deterministicRulesRef": "rules://risk-signals@2"
            }}),
        ),
    );
    nodes.insert(
        "gate".into(),
        normative_node(
            "gate",
            serde_json::json!({
                "gate": {
                    "requirements": ["no_blocking_security_findings", "evidence_coverage_complete"],
                    "evaluators": ["evaluator://security-report-validator@1"],
                    "passWhen": "all"
                },
                "onFail": {"routeTo": "agent"},
                "override": {"allowedRoles": ["owner"], "resultLabel": "completed_with_security_waiver"},
                "completion": {"requires": ["verified_claims"]}
            }),
        ),
    );
    nodes.insert(
        "evaluator".into(),
        normative_node("evaluator", serde_json::json!({})),
    );
    nodes.insert(
        "fork".into(),
        normative_node("fork", serde_json::json!({"strategy": "all"})),
    );
    nodes.insert(
        "join".into(),
        normative_node(
            "join",
            serde_json::json!({
                "strategy": "all_completed",
                "merge": {"method": "artifact_bundle", "outputSchema": "schema://ReviewBundle@1"}
            }),
        ),
    );
    nodes.insert(
        "human".into(),
        normative_node(
            "human_decision",
            serde_json::json!({
                "prompt": "Select the deployment environment",
                "options": ["staging", "production", "cancel"],
                "timeout": {"seconds": 86400, "onTimeout": "pause"}
            }),
        ),
    );
    nodes.insert(
        "timer".into(),
        normative_node("timer", serde_json::json!({})),
    );
    nodes.insert(
        "trigger".into(),
        normative_node("trigger", serde_json::json!({})),
    );
    nodes.insert(
        "subgraph".into(),
        normative_node(
            "subgraph",
            serde_json::json!({
                "graphRef": "graph-template://security-review@3",
                "parameters": {"scope": artifact},
                "expose": {"outputs": ["security_report"]}
            }),
        ),
    );
    nodes.insert(
        "materializer".into(),
        normative_node(
            "materializer",
            serde_json::json!({"materializer": {
                "target": "document://architecture/authentication.md",
                "strategy": "evidence_backed_patch"
            }}),
        ),
    );
    nodes.insert(
        "deploy".into(),
        normative_node(
            "deploy",
            serde_json::json!({
                "adapterRef": "deploy://docker-compose@1",
                "targetRef": "environment://staging",
                "preconditions": [artifact],
                "effects": {"reversible": true, "compensationNode": "rollback"}
            }),
        ),
    );
    nodes.insert(
        "rollback".into(),
        normative_node(
            "rollback",
            serde_json::json!({
                "adapterRef": "deploy://docker-compose@1",
                "input": {"schema": "schema://DeploymentReceipt@1"}
            }),
        ),
    );
    nodes.insert(
        "transform".into(),
        normative_node(
            "artifact_transform",
            serde_json::json!({"tags": ["artifact"]}),
        ),
    );
    record.graph.spec.nodes = nodes;
    record.graph.spec.entrypoints = vec!["agent".into()];
    record.graph.spec.edges = vec![
        serde_json::from_value(serde_json::json!({
            "id": "agent-to-deploy",
            "from": "agent",
            "to": "deploy",
            "type": "control"
        }))
        .unwrap(),
    ];
    record.graph.spec.policies = vec![
        serde_json::json!("policy://workspace/security-baseline@2"),
        serde_json::json!({"ref": "policy://project/deploy-rules@4"}),
        serde_json::json!({"inlineConstraint": {
            "deny": ["production.deploy"],
            "reason": "user_request_scope"
        }}),
    ];
    record.graph.spec.completion = serde_json::json!({
        "terminalNodes": ["deploy"],
        "requires": ["document://task-summary.md", artifact],
        "allowWaivers": true,
        "statuses": {"full": "completed", "waived": "completed_with_waivers"}
    });
    refresh_record(&mut record);
    assert!(
        graphhelm_schema::validate_graph_value(
            &serde_json::to_value(&record.graph).unwrap(),
            "node-kind-registry"
        )
        .is_empty()
    );
    record
}

#[test]
fn every_normative_node_kind_and_completion_control_is_reconstructible() {
    let record = node_kind_registry_record();
    let prepared =
        block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
            .unwrap();

    let common = control(&prepared, "agent", "node_common");
    assert_eq!(identifier(common, "tag.001"), "controlled");
    assert_eq!(integer(common, "tagCount"), 2);
    assert_eq!(identifier(common, "onCancel"), "pause");
    assert_eq!(identifier(common, "onFailure"), "route_remediation");
    let tool = control(&prepared, "tool", "tool_configuration");
    assert_eq!(
        decode_reference(identifier(tool, "toolRef")),
        "builtin/test-runner@1"
    );
    assert_eq!(identifier(tool, "action"), "execute");
    let classifier = control(&prepared, "classifier", "classifier_configuration");
    assert_eq!(
        decode_reference(identifier(classifier, "rulesRef")),
        "rules://risk-signals@2"
    );
    assert_eq!(identifier(classifier, "method"), "hybrid");
    assert_eq!(identifier(classifier, "profile"), "fast_classification");
    let gate = control(&prepared, "gate", "gate_configuration");
    assert_eq!(integer(gate, "requirementCount"), 2);
    assert_eq!(
        identifier(gate, "requirement.001"),
        "evidence_coverage_complete"
    );
    assert_eq!(integer(gate, "evaluatorCount"), 1);
    assert_eq!(
        decode_reference(identifier(gate, "evaluator.000")),
        "evaluator://security-report-validator@1"
    );
    assert_eq!(identifier(gate, "passWhen"), "all");
    assert_eq!(identifier(gate, "failureRoute"), "agent");
    assert_eq!(identifier(gate, "overrideRole.000"), "owner");
    assert_eq!(
        identifier(gate, "overrideResult"),
        "completed_with_security_waiver"
    );
    assert_eq!(
        identifier(control(&prepared, "fork", "fork_configuration"), "strategy"),
        "all"
    );
    let join = control(&prepared, "join", "join_configuration");
    assert_eq!(identifier(join, "strategy"), "all_completed");
    assert_eq!(identifier(join, "mergeMethod"), "artifact_bundle");
    assert_eq!(
        decode_reference(identifier(join, "resultSchema")),
        "schema://ReviewBundle@1"
    );
    let human = control(&prepared, "human", "human_decision");
    assert_eq!(identifier(human, "option.001"), "production");
    assert_eq!(integer(human, "optionCount"), 3);
    assert_eq!(integer(human, "timeoutSeconds"), 86400);
    assert_eq!(identifier(human, "timeoutAction"), "pause");
    assert_eq!(
        prepared
            .version()
            .content_slots()
            .iter()
            .filter(|slot| {
                slot.owner_kind() == ContentOwnerKind::Node
                    && slot.owner_id().as_str() == "human"
                    && slot.field_kind() == ContentFieldKind::Instructions
            })
            .count(),
        1
    );
    let subgraph = control(&prepared, "subgraph", "subgraph_configuration");
    assert_eq!(
        decode_reference(identifier(subgraph, "graphRef")),
        "graph-template://security-review@3"
    );
    assert_eq!(integer(subgraph, "parameterCount"), 1);
    assert_eq!(identifier(subgraph, "parameterKey.000"), "scope");
    assert_eq!(identifier(subgraph, "exposedResult.000"), "security_report");
    let materializer = control(&prepared, "materializer", "materializer_configuration");
    assert_eq!(
        decode_reference(identifier(materializer, "target")),
        "document://architecture/authentication.md"
    );
    assert_eq!(
        identifier(materializer, "strategy"),
        "evidence_backed_patch"
    );
    let deploy = control(&prepared, "deploy", "deploy_configuration");
    assert_eq!(
        decode_reference(identifier(deploy, "adapterRef")),
        "deploy://docker-compose@1"
    );
    assert_eq!(integer(deploy, "preconditionCount"), 1);
    assert!(flag(deploy, "reversible"));
    assert_eq!(identifier(deploy, "compensationNode"), "rollback");
    assert_eq!(
        decode_reference(identifier(
            control(&prepared, "rollback", "rollback_configuration"),
            "adapterRef"
        )),
        "deploy://docker-compose@1"
    );
    for id in ["planner", "evaluator", "timer", "trigger", "transform"] {
        assert!(
            prepared
                .version()
                .topology()
                .nodes()
                .keys()
                .any(|node| node.as_str() == id)
        );
    }

    let completion = prepared.version().topology().completion();
    assert_eq!(identifier(completion, "terminal.000"), "deploy");
    assert_eq!(integer(completion, "requirementCount"), 2);
    assert_eq!(
        decode_reference(identifier(completion, "requirement.000")),
        "document://task-summary.md"
    );
    assert!(flag(completion, "allowWaivers"));
    assert_eq!(identifier(completion, "statusFull"), "completed");
    assert_eq!(
        identifier(completion, "statusWaived"),
        "completed_with_waivers"
    );
    assert_eq!(prepared.version().topology().policies().len(), 3);
    let policies = prepared.version().topology().policies();
    assert_eq!(
        decode_reference(identifier(&policies[0], "policyRef")),
        "policy://workspace/security-baseline@2"
    );
    assert_eq!(identifier(&policies[2], "deny.000"), "production.deploy");
    assert_eq!(identifier(&policies[2], "reasonCode"), "user_request_scope");
    let node_completion = control(&prepared, "agent", "node_completion");
    assert_eq!(integer(node_completion, "requiresCount"), 3);
    assert!(flag(node_completion, "requiresSchemaValid.000"));
    assert_eq!(
        identifier(node_completion, "requiresEvidenceType.001"),
        "source_location"
    );
    assert_eq!(integer(node_completion, "requiresEvidenceMin.001"), 1);
    assert_eq!(integer(node_completion, "forbidsCount"), 1);
    validate_persisted_references(prepared.version()).unwrap();

    let wire = serde_json::to_string(prepared.version()).unwrap();
    for prose in [
        "Select the deployment environment",
        "output.confidence >= 0.7",
        "output.unsupportedClaims > 0",
    ] {
        assert!(!wire.contains(prose));
    }
}

#[test]
fn registered_presence_cardinality_and_empty_items_are_fail_closed() {
    let baseline = node_kind_registry_record();
    let scope = scope(&baseline.graph.metadata.execution_id);
    let prepared = block_on(externalizer().prepare(scope.clone(), &baseline)).unwrap();
    let planner = control(&prepared, "planner", "node_common");
    assert_eq!(planner.integers().values().copied().next(), Some(0));

    let mut absent = baseline.clone();
    absent
        .graph
        .spec
        .nodes
        .get_mut("planner")
        .unwrap()
        .properties
        .remove("tags");
    refresh_record(&mut absent);
    let absent = block_on(externalizer().prepare(scope.clone(), &absent)).unwrap();
    assert_ne!(
        prepared.version().topology_hash(),
        absent.version().topology_hash()
    );

    let mut empty_item = baseline.clone();
    empty_item
        .graph
        .spec
        .nodes
        .get_mut("agent")
        .unwrap()
        .properties
        .get_mut("completion")
        .unwrap()["requires"] = serde_json::json!([{}]);
    refresh_record(&mut empty_item);
    assert_rejected_before_sealing(&empty_item, scope.clone(), "empty-completion-item");

    let mut duplicate = baseline.clone();
    duplicate
        .graph
        .spec
        .nodes
        .get_mut("agent")
        .unwrap()
        .properties
        .insert("tags".into(), serde_json::json!(["same", "same"]));
    refresh_record(&mut duplicate);
    assert_rejected_before_sealing(&duplicate, scope.clone(), "duplicate-tag");

    let mut overflow = baseline;
    overflow
        .graph
        .spec
        .nodes
        .get_mut("agent")
        .unwrap()
        .properties
        .insert(
            "tags".into(),
            serde_json::json!(
                (0..65)
                    .map(|index| format!("tag-{index}"))
                    .collect::<Vec<_>>()
            ),
        );
    refresh_record(&mut overflow);
    let calls = Arc::new(AtomicUsize::new(0));
    let externalizer = SealingGraphExternalizer::new(CountingFailureSealer(Arc::clone(&calls)));
    assert_eq!(
        block_on(externalizer.prepare(scope, &overflow))
            .unwrap_err()
            .code(),
        "GHE006_LIMIT_EXCEEDED"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn node_kind_completion_and_cardinality_mutations_change_safe_identity() {
    let baseline = node_kind_registry_record();
    let scope = scope(&baseline.graph.metadata.execution_id);
    let original = block_on(externalizer().prepare(scope.clone(), &baseline)).unwrap();
    for mutation in ["node-kind-child", "completion", "cardinality"] {
        let mut changed = baseline.clone();
        match mutation {
            "node-kind-child" => {
                changed
                    .graph
                    .spec
                    .nodes
                    .get_mut("tool")
                    .unwrap()
                    .properties
                    .get_mut("tool")
                    .unwrap()["ref"] = serde_json::json!("builtin/test-runner@2");
            }
            "completion" => {
                changed
                    .graph
                    .spec
                    .nodes
                    .get_mut("agent")
                    .unwrap()
                    .properties
                    .get_mut("completion")
                    .unwrap()["requires"][0]["outputSchemaValid"] = serde_json::json!(false);
            }
            "cardinality" => {
                changed
                    .graph
                    .spec
                    .nodes
                    .get_mut("agent")
                    .unwrap()
                    .properties
                    .insert("tags".into(), serde_json::json!(["foundation"]));
            }
            _ => unreachable!(),
        }
        refresh_record(&mut changed);
        let projection = block_on(externalizer().prepare(scope.clone(), &changed)).unwrap();
        assert_ne!(
            original.version().topology_hash(),
            projection.version().topology_hash(),
            "{mutation}"
        );
        assert_ne!(
            original.version().semantic_hash(),
            projection.version().semantic_hash(),
            "{mutation}"
        );
    }
}

#[test]
fn foreign_node_kind_children_and_permission_expansion_fail_before_sealing() {
    for case in [
        "foreign_kind",
        "unknown_child",
        "permission_expansion",
        "invalid_enum",
        "human_prompt_type",
    ] {
        let mut record = node_kind_registry_record();
        match case {
            "foreign_kind" => {
                record
                    .graph
                    .spec
                    .nodes
                    .get_mut("planner")
                    .unwrap()
                    .properties
                    .insert(
                        "tool".into(),
                        serde_json::json!({"ref": "builtin/test-runner@1", "action": "execute"}),
                    );
            }
            "unknown_child" => {
                record
                    .graph
                    .spec
                    .nodes
                    .get_mut("tool")
                    .unwrap()
                    .properties
                    .get_mut("tool")
                    .unwrap()["future"] = serde_json::json!("unknown-kind-child");
            }
            "permission_expansion" => {
                record.graph.spec.policies[2]["inlineConstraint"]["allow"] =
                    serde_json::json!(["production.deploy"]);
            }
            "invalid_enum" => {
                record
                    .graph
                    .spec
                    .nodes
                    .get_mut("join")
                    .unwrap()
                    .properties
                    .insert("strategy".into(), serde_json::json!("invented_strategy"));
            }
            "human_prompt_type" => {
                record
                    .graph
                    .spec
                    .nodes
                    .get_mut("human")
                    .unwrap()
                    .properties
                    .insert("prompt".into(), serde_json::json!({"text": "not-a-string"}));
            }
            _ => unreachable!(),
        }
        refresh_record(&mut record);
        assert_rejected_before_sealing(&record, scope(&record.graph.metadata.execution_id), case);
    }
}

#[test]
fn newly_registered_reference_positions_fail_closed_after_deserialization() {
    let record = node_kind_registry_record();
    let prepared =
        block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
            .unwrap();

    for (node, control_type, key, replacement) in [
        ("tool", "tool_configuration", "toolRef", "refv1:A"),
        (
            "classifier",
            "classifier_configuration",
            "rulesRef",
            "refv1:A",
        ),
        ("gate", "gate_configuration", "evaluator.000", "refv1:A"),
        ("join", "join_configuration", "resultSchema", "refv1:A"),
        ("subgraph", "subgraph_configuration", "graphRef", "refv1:A"),
        (
            "materializer",
            "materializer_configuration",
            "target",
            "refv1:A",
        ),
        ("deploy", "deploy_configuration", "adapterRef", "refv1:A"),
        (
            "rollback",
            "rollback_configuration",
            "adapterRef",
            "refv1:A",
        ),
    ] {
        let mut wire = serde_json::to_value(prepared.version()).unwrap();
        let controls = wire["topology"]["nodes"][node]["controls"]
            .as_array_mut()
            .unwrap();
        let target = controls
            .iter_mut()
            .find(|control| control["controlType"] == control_type)
            .unwrap();
        target["identifiers"][key] = serde_json::json!(replacement);
        let version: graphhelm_protocols::PersistedGraphVersion =
            serde_json::from_value(wire).unwrap();
        assert!(
            validate_persisted_references(&version).is_err(),
            "{node}.{key}"
        );
    }

    for pointer in [
        "/topology/policies/0/identifiers/policyRef",
        "/topology/completion/identifiers/requirement.000",
    ] {
        let mut wire = serde_json::to_value(prepared.version()).unwrap();
        *wire.pointer_mut(pointer).unwrap() = serde_json::json!("refv1:A");
        let version: graphhelm_protocols::PersistedGraphVersion =
            serde_json::from_value(wire).unwrap();
        assert!(
            validate_persisted_references(&version).is_err(),
            "{pointer}"
        );
    }
}

#[test]
fn registered_controls_are_reconstructible_in_safe_topology() {
    let record = registered_controls_record();
    let prepared =
        block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
            .unwrap();

    let agent = control(&prepared, "plan", "agent_configuration");
    assert_eq!(identifier(agent, "capability.000"), "change.plan");
    assert_eq!(
        identifier(agent, "allowedTool.001"),
        "tests.execute_targeted"
    );
    assert_eq!(
        identifier(agent, "prohibitedAction.000"),
        "production.deploy"
    );
    assert_eq!(
        decode_reference(identifier(agent, "inputSchema")),
        "schema://RepositoryMap@1"
    );
    assert_eq!(
        decode_reference(identifier(agent, "directiveRef")),
        "contract://planning-instructions@2"
    );
    assert!(agent.digests().is_empty());
    let referenced_agent = control(&prepared, "map_repository", "agent_configuration");
    assert_eq!(identifier(referenced_agent, "mode"), "ref");
    assert_eq!(
        decode_reference(identifier(referenced_agent, "agentRef")),
        "project/security-reviewer@3"
    );

    let model = control(&prepared, "plan", "node_model");
    assert_eq!(identifier(model, "profile"), "software_execution");
    assert_eq!(identifier(model, "independent.000"), "implement");
    let input = control(&prepared, "plan", "input_contract");
    assert_eq!(
        decode_reference(identifier(input, "schema")),
        "schema://PlanningRequest@1"
    );
    assert_eq!(identifier(input, "bindingKey.000"), "request");
    assert_eq!(
        decode_reference(identifier(input, "bindingValue.000")),
        "context://task/request"
    );
    assert!(input.digests().is_empty());
    let output = control(&prepared, "plan", "output_contract");
    assert_eq!(
        decode_reference(identifier(output, "publishAs")),
        format!("artifact://sha256/{}", "c".repeat(64))
    );

    let context = control(&prepared, "plan", "node_context");
    assert_eq!(context.integers().values().copied().max(), Some(24000));
    assert_eq!(context.flags().len(), 6);
    let permissions = control(&prepared, "plan", "node_permissions");
    assert_eq!(identifier(permissions, "capability.001"), "network.request");
    assert_eq!(identifier(permissions, "duration.001"), "call");
    let isolation = control(&prepared, "plan", "node_isolation");
    assert_eq!(identifier(isolation, "minimum"), "tier_2");
    assert_eq!(isolation.integers().len(), 4);
    let retry = control(&prepared, "plan", "node_retry");
    assert_eq!(identifier(retry, "retryOn.001"), "tool_timeout");
    assert_eq!(retry.integers().len(), 5);
    let resources = control(&prepared, "plan", "node_resources");
    assert_eq!(resources.integers().len(), 3);
    let memory = control(&prepared, "plan", "node_memory");
    assert_eq!(
        memory
            .flags()
            .iter()
            .find(|(key, _)| key.as_str() == "writeCandidates")
            .map(|(_, value)| *value),
        Some(false)
    );

    let edge = prepared.version().topology().edges().first().unwrap();
    assert_eq!(
        decode_reference(
            edge.bindings()
                .iter()
                .find(|(key, _)| key.as_str() == "artifact")
                .unwrap()
                .1
                .as_str()
        ),
        format!("artifact://sha256/{}", "a".repeat(64))
    );
    assert_eq!(
        decode_reference(
            edge.bindings()
                .iter()
                .find(|(key, _)| key.as_str() == "request")
                .unwrap()
                .1
                .as_str()
        ),
        "context://task/request"
    );
    let condition = edge.condition().unwrap();
    assert_eq!(
        decode_reference(identifier(condition, "schema")),
        "schema://PlanTransition@1"
    );
    assert!(condition.digests().is_empty());

    let wire = serde_json::to_string(prepared.version()).unwrap();
    for raw in [
        "schema://PlanningRequest@1",
        "contract://planning-instructions@2",
        "context-policy://blind-review@1",
    ] {
        assert!(!wire.contains(raw));
    }
}

#[test]
fn registered_structural_mutations_change_both_safe_hashes() {
    let record = registered_controls_record();
    let scope = scope(&record.graph.metadata.execution_id);
    let baseline = block_on(externalizer().prepare(scope.clone(), &record)).unwrap();

    for mutation in ["capability", "retry", "reference"] {
        let mut changed = record.clone();
        let node = changed.graph.spec.nodes.get_mut("plan").unwrap();
        match mutation {
            "capability" => {
                node.properties.get_mut("agent").unwrap()["ephemeral"]["capabilities"] =
                    serde_json::json!(["change.plan", "diff.inspect"]);
            }
            "retry" => {
                node.properties.get_mut("retry").unwrap()["maxBackoffSeconds"] =
                    serde_json::json!(121)
            }
            "reference" => {
                node.properties.get_mut("context").unwrap()["policyRef"] =
                    serde_json::json!("context-policy://blind-review@2");
            }
            _ => unreachable!(),
        }
        refresh_record(&mut changed);
        let prepared = block_on(externalizer().prepare(scope.clone(), &changed)).unwrap();
        assert_ne!(
            baseline.version().topology_hash(),
            prepared.version().topology_hash(),
            "{mutation}"
        );
        assert_ne!(
            baseline.version().semantic_hash(),
            prepared.version().semantic_hash(),
            "{mutation}"
        );
    }
}

#[test]
fn registered_control_order_and_re_encryption_do_not_change_identity() {
    let first = registered_controls_record();
    let mut second = first.clone();
    let plan = second.graph.spec.nodes.get_mut("plan").unwrap();
    let original = plan.properties.remove("context").unwrap();
    let object = original.as_object().unwrap();
    let mut reversed = serde_json::Map::new();
    for (key, value) in object.iter().rev() {
        reversed.insert(key.clone(), value.clone());
    }
    plan.properties
        .insert("context".into(), serde_json::Value::Object(reversed));
    refresh_record(&mut second);

    let scope = scope(&first.graph.metadata.execution_id);
    let first_prepared = block_on(externalizer().prepare(scope.clone(), &first)).unwrap();
    let retry_prepared = block_on(externalizer().prepare(scope.clone(), &first)).unwrap();
    let reordered_prepared = block_on(externalizer().prepare(scope, &second)).unwrap();
    assert_eq!(first_prepared.version(), retry_prepared.version());
    assert_eq!(first_prepared.version(), reordered_prepared.version());
}

#[test]
fn nominal_instruction_selector_is_not_encoded_as_a_reference() {
    let mut record = registered_controls_record();
    record
        .graph
        .spec
        .nodes
        .get_mut("plan")
        .unwrap()
        .properties
        .get_mut("agent")
        .unwrap()["ephemeral"]["instructionsRef"] = serde_json::json!("inline-or-artifact");
    refresh_record(&mut record);

    let prepared =
        block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
            .unwrap();
    let agent = control(&prepared, "plan", "agent_configuration");
    assert_eq!(identifier(agent, "directiveRef"), "inline-or-artifact");
    validate_persisted_references(prepared.version()).unwrap();
}

#[test]
fn registered_control_negative_matrix_fails_before_sealing_without_echo() {
    for case in [
        "unknown",
        "wrong_type",
        "float",
        "unsafe_reference",
        "filesystem_reference",
        "empty_path",
        "secret",
        "nested_prose",
        "null",
    ] {
        let mut record = registered_controls_record();
        let node = record.graph.spec.nodes.get_mut("plan").unwrap();
        let canary = match case {
            "unknown" => {
                node.properties.get_mut("retry").unwrap()["futureControl"] =
                    serde_json::json!("unknown-control-canary");
                "unknown-control-canary"
            }
            "wrong_type" => {
                node.properties.get_mut("retry").unwrap()["maxAttempts"] =
                    serde_json::json!("three");
                "three"
            }
            "float" => {
                node.properties.get_mut("resources").unwrap()["cpu"] = serde_json::json!(1.5);
                "1.5"
            }
            "unsafe_reference" => {
                node.properties.get_mut("context").unwrap()["policyRef"] =
                    serde_json::json!("https://unsafe.example/control");
                "https://unsafe.example/control"
            }
            "filesystem_reference" => {
                node.properties.get_mut("context").unwrap()["policyRef"] =
                    serde_json::json!("C:private");
                "C:private"
            }
            "empty_path" => {
                node.properties.get_mut("isolation").unwrap()["filesystem"]["writablePaths"] =
                    serde_json::json!([]);
                "empty-path"
            }
            "secret" => {
                node.properties.get_mut("model").unwrap()["profile"] =
                    serde_json::json!("ghp_0123456789abcdefghijklmnop");
                "ghp_0123456789abcdefghijklmnop"
            }
            "nested_prose" => {
                node.properties.get_mut("context").unwrap()["summary"] =
                    serde_json::json!({"text": "human-prose-canary"});
                "human-prose-canary"
            }
            "null" => {
                node.properties.get_mut("memory").unwrap()["defaultTtlDays"] =
                    serde_json::Value::Null;
                "null-canary"
            }
            _ => unreachable!(),
        };
        refresh_record(&mut record);
        assert_rejected_before_sealing(&record, scope(&record.graph.metadata.execution_id), canary);
    }
}

#[test]
fn every_registered_object_rejects_unknown_children_before_sealing() {
    for object in [
        "agent",
        "agent_model",
        "agent_context",
        "agent_evidence",
        "agent_memory",
        "node_model",
        "input",
        "output",
        "context",
        "permission",
        "isolation",
        "retry",
        "resources",
        "memory",
    ] {
        let mut record = registered_controls_record();
        let node = record.graph.spec.nodes.get_mut("plan").unwrap();
        let unknown = serde_json::json!("unknown_control_canary");
        match object {
            "agent" => node.properties.get_mut("agent").unwrap()["ephemeral"]["future"] = unknown,
            "agent_model" => {
                node.properties.get_mut("agent").unwrap()["ephemeral"]["modelRequirements"]["future"] =
                    unknown
            }
            "agent_context" => {
                node.properties.get_mut("agent").unwrap()["ephemeral"]["contextStrategy"]["future"] =
                    unknown
            }
            "agent_evidence" => {
                node.properties.get_mut("agent").unwrap()["ephemeral"]["evidenceRequirements"][0]
                    ["future"] = unknown
            }
            "agent_memory" => {
                node.properties.get_mut("agent").unwrap()["ephemeral"]["memoryPolicy"]["future"] =
                    unknown
            }
            "node_model" => node.properties.get_mut("model").unwrap()["future"] = unknown,
            "input" => node.properties.get_mut("input").unwrap()["future"] = unknown,
            "output" => node.properties.get_mut("output").unwrap()["future"] = unknown,
            "context" => node.properties.get_mut("context").unwrap()["future"] = unknown,
            "permission" => node.properties.get_mut("permissions").unwrap()[1]["future"] = unknown,
            "isolation" => node.properties.get_mut("isolation").unwrap()["future"] = unknown,
            "retry" => node.properties.get_mut("retry").unwrap()["future"] = unknown,
            "resources" => node.properties.get_mut("resources").unwrap()["future"] = unknown,
            "memory" => node.properties.get_mut("memory").unwrap()["future"] = unknown,
            _ => unreachable!(),
        }
        refresh_record(&mut record);
        assert_rejected_before_sealing(
            &record,
            scope(&record.graph.metadata.execution_id),
            "unknown_control_canary",
        );
    }
}

#[test]
fn registered_control_item_limit_fails_before_sealing() {
    let mut record = registered_controls_record();
    record
        .graph
        .spec
        .nodes
        .get_mut("plan")
        .unwrap()
        .properties
        .get_mut("retry")
        .unwrap()["retryOn"] = serde_json::json!(
        (0..65)
            .map(|index| format!("retry_{index}"))
            .collect::<Vec<_>>()
    );
    refresh_record(&mut record);
    let calls = Arc::new(AtomicUsize::new(0));
    let externalizer = SealingGraphExternalizer::new(CountingFailureSealer(Arc::clone(&calls)));

    let error = block_on(externalizer.prepare(scope(&record.graph.metadata.execution_id), &record))
        .unwrap_err();

    assert_eq!(error.code(), "GHE006_LIMIT_EXCEEDED");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn overlong_registered_reference_fails_before_sealing() {
    let mut record = registered_controls_record();
    record
        .graph
        .spec
        .nodes
        .get_mut("plan")
        .unwrap()
        .properties
        .get_mut("context")
        .unwrap()["policyRef"] = serde_json::json!(format!("environment://{}", "a".repeat(100)));
    refresh_record(&mut record);
    let calls = Arc::new(AtomicUsize::new(0));
    let externalizer = SealingGraphExternalizer::new(CountingFailureSealer(Arc::clone(&calls)));

    let error = block_on(externalizer.prepare(scope(&record.graph.metadata.execution_id), &record))
        .unwrap_err();

    assert_eq!(error.code(), "GHE006_LIMIT_EXCEEDED");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn reference_bypass_matrix_fails_before_sealing_without_echo() {
    let cases = [
        "Cargo.lock",
        "./schema.json",
        "../schema.json",
        "/etc/passwd",
        "C:relative\\secret",
        "C:\\absolute\\secret",
        "artifact:///sha256/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "artifact://sha256/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        "artifact://planning/result",
        "schema://file://contract",
        "schema://",
        "environment://",
        "environment://production/apiKey",
        "environment://production/privateKey",
        "environment://production/accessKey",
        "environment://production/databaseUrl",
        "environment://production/connectionString",
        "environment://production_API_KEY",
        "environment://production-private-key",
        "environment://production_access-key",
        "environment://production-database-url",
        "environment://production_connection-string",
        "environment://production?token=x",
        "environment://user@production",
        "environment://%2e%2e",
        "SCHEMA://PlanningRequest@1",
        "schema://PlanningRequest@1#fragment",
        "schema://PlanningRequest@1?query=x",
    ];

    for value in cases {
        let mut record = registered_controls_record();
        record
            .graph
            .spec
            .nodes
            .get_mut("plan")
            .unwrap()
            .properties
            .get_mut("context")
            .unwrap()["policyRef"] = serde_json::json!(value);
        refresh_record(&mut record);
        assert_rejected_before_sealing(&record, scope(&record.graph.metadata.execution_id), value);
    }
}

#[test]
fn schema_identity_and_dialect_are_semantic_but_graph_annotations_are_not() {
    let original = version("software-feature.yaml").to_record();
    let mut identified = original.clone();
    let input = identified
        .graph
        .spec
        .nodes
        .get_mut("tests")
        .unwrap()
        .properties
        .get_mut("input")
        .unwrap()
        .as_object_mut()
        .unwrap();
    input.insert(
        "$schema".into(),
        serde_json::json!("https://json-schema.org/draft/2020-12/schema"),
    );
    input.insert(
        "$id".into(),
        serde_json::json!("https://p50.dev/schemas/test-input.schema.json"),
    );
    identified.graph.metadata.properties.insert(
        "annotations".into(),
        serde_json::json!({"layout": {"x": 99, "y": 42}}),
    );
    refresh_record(&mut identified);

    let baseline =
        block_on(externalizer().prepare(scope(&original.graph.metadata.execution_id), &original))
            .unwrap();
    let prepared = block_on(
        externalizer().prepare(scope(&identified.graph.metadata.execution_id), &identified),
    )
    .unwrap();

    assert_ne!(
        baseline.version().topology_hash(),
        prepared.version().topology_hash()
    );
    assert_ne!(
        baseline.version().semantic_hash(),
        prepared.version().semantic_hash()
    );
    let contract = control(&prepared, "tests", "input_contract");
    assert_eq!(
        decode_reference(identifier(contract, "schemaDialect")),
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(
        decode_reference(identifier(contract, "schemaId")),
        "https://p50.dev/schemas/test-input.schema.json"
    );

    let mut annotation_only = original.clone();
    annotation_only.graph.metadata.properties.insert(
        "annotations".into(),
        serde_json::json!({"layout": {"x": 99, "y": 42}}),
    );
    refresh_record(&mut annotation_only);
    let annotation_projection = block_on(externalizer().prepare(
        scope(&annotation_only.graph.metadata.execution_id),
        &annotation_only,
    ))
    .unwrap();
    assert_eq!(baseline.version(), annotation_projection.version());
}

#[test]
fn persisted_reference_validator_rejects_malformed_and_unexpected_encodings() {
    let record = registered_controls_record();
    let prepared =
        block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
            .unwrap();
    validate_persisted_references(prepared.version()).unwrap();

    let mut malformed = serde_json::to_value(prepared.version()).unwrap();
    malformed["topology"]["nodes"]["plan"]["controls"][1]["identifiers"]["schema"] =
        serde_json::json!("refv1:A");
    let malformed: graphhelm_protocols::PersistedGraphVersion =
        serde_json::from_value(malformed).unwrap();
    assert!(validate_persisted_references(&malformed).is_err());

    let encoded_schema = identifier(control(&prepared, "plan", "input_contract"), "schema");
    let mut unexpected = serde_json::to_value(prepared.version()).unwrap();
    unexpected["topology"]["nodes"]["plan"]["controls"][0]["identifiers"]["mode"] =
        serde_json::json!(encoded_schema);
    let unexpected: graphhelm_protocols::PersistedGraphVersion =
        serde_json::from_value(unexpected).unwrap();
    assert!(validate_persisted_references(&unexpected).is_err());
}

#[test]
fn combined_control_map_limit_fails_before_sealing() {
    let mut record = registered_controls_record();
    let ephemeral = record
        .graph
        .spec
        .nodes
        .get_mut("plan")
        .unwrap()
        .properties
        .get_mut("agent")
        .unwrap()
        .get_mut("ephemeral")
        .unwrap();
    for key in ["capabilities", "allowedTools", "prohibitedActions"] {
        ephemeral[key] = serde_json::json!(
            (0..43)
                .map(|index| format!("registered.{key}.{index}"))
                .collect::<Vec<_>>()
        );
    }
    refresh_record(&mut record);
    let calls = Arc::new(AtomicUsize::new(0));
    let externalizer = SealingGraphExternalizer::new(CountingFailureSealer(Arc::clone(&calls)));

    let error = block_on(externalizer.prepare(scope(&record.graph.metadata.execution_id), &record))
        .unwrap_err();

    assert_eq!(error.code(), "GHE006_LIMIT_EXCEEDED");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn registered_control_integer_bound_fails_before_sealing() {
    let mut record = registered_controls_record();
    record
        .graph
        .spec
        .nodes
        .get_mut("plan")
        .unwrap()
        .properties
        .get_mut("retry")
        .unwrap()["maxAttempts"] = serde_json::json!(9_007_199_254_740_992_u64);
    refresh_record(&mut record);
    let calls = Arc::new(AtomicUsize::new(0));
    let externalizer = SealingGraphExternalizer::new(CountingFailureSealer(Arc::clone(&calls)));

    let error = block_on(externalizer.prepare(scope(&record.graph.metadata.execution_id), &record))
        .unwrap_err();

    assert_eq!(error.code(), "GHE006_LIMIT_EXCEEDED");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

fn assert_rejected_before_sealing(
    version: &GraphVersionRecord,
    scope: RepositoryScope,
    canary: &str,
) {
    let calls = Arc::new(AtomicUsize::new(0));
    let externalizer = SealingGraphExternalizer::new(CountingFailureSealer(Arc::clone(&calls)));

    let error = block_on(externalizer.prepare(scope, version)).unwrap_err();

    assert_eq!(error.code(), "GHE009_EXTERNALIZATION_FAILED");
    assert!(!error.to_string().contains(canary));
    assert!(!format!("{error:?}").contains(canary));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

fn rehashed_projection(mut wire: serde_json::Value) -> graphhelm_protocols::PersistedGraphVersion {
    let provisional: graphhelm_protocols::PersistedGraphVersion =
        serde_json::from_value(wire.clone()).unwrap();
    let hashes = persisted_hashes(provisional.topology(), provisional.content_slots()).unwrap();
    wire["topologyHash"] = serde_json::json!(hashes.topology_hash().as_str());
    wire["semanticHash"] = serde_json::json!(hashes.semantic_hash().as_str());
    serde_json::from_value(wire).unwrap()
}

fn projection_control_mut<'a>(
    wire: &'a mut serde_json::Value,
    control_type: &str,
) -> &'a mut serde_json::Value {
    wire["topology"]["nodes"]["plan"]["controls"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|control| control["controlType"] == control_type)
        .unwrap()
}

#[test]
fn document_context_include_accepts_only_the_document_reference_domain() {
    let valid = "document://architecture/auth";
    let mut baseline_record = version("software-feature.yaml").to_record();
    baseline_record
        .graph
        .spec
        .nodes
        .get_mut("plan")
        .unwrap()
        .properties
        .insert(
            "context".into(),
            serde_json::json!({"include": [{"type": "document", "ref": valid}]}),
        );
    refresh_record(&mut baseline_record);
    let prepared = block_on(externalizer().prepare(
        scope(&baseline_record.graph.metadata.execution_id),
        &baseline_record,
    ))
    .unwrap();
    validate_persisted_projection(prepared.version()).unwrap();
    let baseline = serde_json::to_value(prepared.version()).unwrap();

    for wrong_domain in [
        "artifact://architecture-auth",
        "context://architecture/auth",
        "schema://Architecture@1",
        "environment://staging",
    ] {
        let mut record = baseline_record.clone();
        record
            .graph
            .spec
            .nodes
            .get_mut("plan")
            .unwrap()
            .properties
            .get_mut("context")
            .unwrap()["include"][0]["ref"] = serde_json::json!(wrong_domain);
        refresh_record(&mut record);
        let calls = Arc::new(AtomicUsize::new(0));
        let externalizer = SealingGraphExternalizer::new(CountingFailureSealer(Arc::clone(&calls)));
        let error =
            block_on(externalizer.prepare(scope(&record.graph.metadata.execution_id), &record))
                .unwrap_err();
        assert_eq!(
            error.code(),
            "GHE009_EXTERNALIZATION_FAILED",
            "{wrong_domain}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0, "{wrong_domain}");

        let mut wire = baseline.clone();
        projection_control_mut(&mut wire, "node_context")["identifiers"]["includeRef.000"] =
            serde_json::json!(encode_persisted_reference(wrong_domain).unwrap().as_str());
        assert!(
            validate_persisted_projection(&rehashed_projection(wire)).is_err(),
            "{wrong_domain}"
        );
    }
}

#[test]
fn context_nested_presence_flags_define_exact_nonempty_family_images() {
    let families = [
        (
            serde_json::json!({"freshness": {"maxAgeDays": 30}}),
            "freshnessPresent",
            "integers",
            "freshnessMaxAgeDays",
            "integers",
            "initialUnits",
            serde_json::json!(1),
        ),
        (
            serde_json::json!({"budget": {"initialTokens": 12000}}),
            "budgetPresent",
            "integers",
            "initialUnits",
            "integers",
            "freshnessMaxAgeDays",
            serde_json::json!(1),
        ),
        (
            serde_json::json!({"expansion": {"allowed": true}}),
            "expansionPresent",
            "flags",
            "expansionAllowed",
            "integers",
            "freshnessMaxAgeDays",
            serde_json::json!(1),
        ),
    ];

    for (authoring, presence, child_map, child, foreign_map, foreign, foreign_value) in families {
        let mut record = version("software-feature.yaml").to_record();
        record
            .graph
            .spec
            .nodes
            .get_mut("plan")
            .unwrap()
            .properties
            .insert("context".into(), authoring);
        refresh_record(&mut record);
        let prepared =
            block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
                .unwrap();
        validate_persisted_projection(prepared.version()).unwrap();
        let baseline = serde_json::to_value(prepared.version()).unwrap();

        let mut false_flag = baseline.clone();
        projection_control_mut(&mut false_flag, "node_context")["flags"][presence] =
            serde_json::json!(false);
        assert!(
            validate_persisted_projection(&rehashed_projection(false_flag)).is_err(),
            "{presence}: false"
        );

        let mut empty = baseline.clone();
        projection_control_mut(&mut empty, "node_context")[child_map]
            .as_object_mut()
            .unwrap()
            .remove(child);
        assert!(
            validate_persisted_projection(&rehashed_projection(empty)).is_err(),
            "{presence}: empty"
        );

        let mut orphan = baseline.clone();
        projection_control_mut(&mut orphan, "node_context")["flags"]
            .as_object_mut()
            .unwrap()
            .remove(presence);
        assert!(
            validate_persisted_projection(&rehashed_projection(orphan)).is_err(),
            "{presence}: orphan"
        );

        let mut crossed = baseline;
        projection_control_mut(&mut crossed, "node_context")[foreign_map][foreign] = foreign_value;
        assert!(
            validate_persisted_projection(&rehashed_projection(crossed)).is_err(),
            "{presence}: crossed"
        );
    }
}

#[test]
fn isolation_nested_presence_flags_define_exact_nonempty_family_images() {
    let families = [
        (
            serde_json::json!({"filesystem": {"mode": "execution_worktree"}}),
            "filesystemPresent",
            "filesystemMode",
            "networkMode",
            "allowlist",
        ),
        (
            serde_json::json!({"network": {"mode": "allowlist"}}),
            "networkPresent",
            "networkMode",
            "brokerMode",
            "broker_only",
        ),
        (
            serde_json::json!({"secrets": {"mode": "broker_only"}}),
            "brokerPresent",
            "brokerMode",
            "filesystemMode",
            "execution_worktree",
        ),
    ];

    for (authoring, presence, child, foreign, foreign_value) in families {
        let mut record = version("software-feature.yaml").to_record();
        record
            .graph
            .spec
            .nodes
            .get_mut("plan")
            .unwrap()
            .properties
            .insert("isolation".into(), authoring);
        refresh_record(&mut record);
        let prepared =
            block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
                .unwrap();
        validate_persisted_projection(prepared.version()).unwrap();
        let baseline = serde_json::to_value(prepared.version()).unwrap();

        let mut false_flag = baseline.clone();
        projection_control_mut(&mut false_flag, "node_isolation")["flags"][presence] =
            serde_json::json!(false);
        assert!(
            validate_persisted_projection(&rehashed_projection(false_flag)).is_err(),
            "{presence}: false"
        );

        let mut empty = baseline.clone();
        projection_control_mut(&mut empty, "node_isolation")["identifiers"]
            .as_object_mut()
            .unwrap()
            .remove(child);
        assert!(
            validate_persisted_projection(&rehashed_projection(empty)).is_err(),
            "{presence}: empty"
        );

        let mut orphan = baseline.clone();
        projection_control_mut(&mut orphan, "node_isolation")["flags"]
            .as_object_mut()
            .unwrap()
            .remove(presence);
        assert!(
            validate_persisted_projection(&rehashed_projection(orphan)).is_err(),
            "{presence}: orphan"
        );

        let mut crossed = baseline;
        projection_control_mut(&mut crossed, "node_isolation")["identifiers"][foreign] =
            serde_json::json!(foreign_value);
        assert!(
            validate_persisted_projection(&rehashed_projection(crossed)).is_err(),
            "{presence}: crossed"
        );
    }
}

#[test]
fn zero_count_nested_children_still_require_their_matching_presence_flags() {
    let cases = [
        (
            "context",
            serde_json::json!({"freshness": {"requireRevalidationFor": []}}),
            "node_context",
            "freshnessPresent",
            "revalidateCount",
        ),
        (
            "isolation",
            serde_json::json!({"network": {"allowlist": []}}),
            "node_isolation",
            "networkPresent",
            "networkAllowCount",
        ),
    ];

    for (property, authoring, control_type, presence_key, count_key) in cases {
        let mut record = version("software-feature.yaml").to_record();
        record
            .graph
            .spec
            .nodes
            .get_mut("plan")
            .unwrap()
            .properties
            .insert(property.into(), authoring);
        refresh_record(&mut record);
        let prepared =
            block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
                .unwrap();
        validate_persisted_projection(prepared.version()).unwrap();
        let mut wire = serde_json::to_value(prepared.version()).unwrap();
        let control = projection_control_mut(&mut wire, control_type);
        assert_eq!(control["integers"][count_key], serde_json::json!(0));
        assert_eq!(control["flags"][presence_key], serde_json::json!(true));
        control["flags"]
            .as_object_mut()
            .unwrap()
            .remove(presence_key);
        assert!(
            validate_persisted_projection(&rehashed_projection(wire)).is_err(),
            "{control_type}.{presence_key} must follow field existence even at count zero"
        );
    }
}

fn assert_replay_rejects_all_slot_profile_mutations(
    version: &graphhelm_protocols::PersistedGraphVersion,
    label: &str,
) {
    let baseline = serde_json::to_value(version).unwrap();
    for index in 0..version.content_slots().len() {
        let mut sensitivity = baseline.clone();
        let current = sensitivity["contentSlots"][index]["sensitivity"]
            .as_str()
            .unwrap();
        sensitivity["contentSlots"][index]["sensitivity"] =
            serde_json::json!(if current == "public" {
                "internal"
            } else {
                "public"
            });
        assert!(
            validate_persisted_projection(&rehashed_projection(sensitivity)).is_err(),
            "{label} slot {index} sensitivity"
        );

        let mut required = baseline.clone();
        let current = required["contentSlots"][index]["requiredForExecution"]
            .as_bool()
            .unwrap();
        required["contentSlots"][index]["requiredForExecution"] = serde_json::json!(!current);
        assert!(
            validate_persisted_projection(&rehashed_projection(required)).is_err(),
            "{label} slot {index} requiredForExecution"
        );
    }
}

#[test]
fn controlled_authoring_cycle_persists_its_positive_loop_limit() {
    let mut record = version("software-feature.yaml").to_record();
    record
        .graph
        .spec
        .nodes
        .get_mut("docs")
        .unwrap()
        .properties
        .insert("loop".into(), serde_json::json!({"maxIterations": 2}));
    let cycle: GraphEdge = serde_json::from_value(serde_json::json!({
        "id": "docs-remediation-loop", "from": "docs", "to": "docs", "type": "control"
    }))
    .unwrap();
    record.graph.spec.edges.push(cycle);
    refresh_record(&mut record);

    let prepared =
        block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
            .unwrap();
    let loop_control = control(&prepared, "docs", "node_loop");
    assert_eq!(integer(loop_control, "maxIterations"), 2);
    validate_persisted_projection(prepared.version()).unwrap();
}

#[test]
fn edge_condition_slots_preserve_declared_ordinals_when_fields_are_absent() {
    let mut only_false = registered_controls_record();
    only_false.graph.spec.edges[0].condition = None;
    only_false.graph.spec.edges[0].on_false = Some(serde_json::json!({"routeTo": "docs"}));
    refresh_record(&mut only_false);
    let prepared = block_on(
        externalizer().prepare(scope(&only_false.graph.metadata.execution_id), &only_false),
    )
    .unwrap();
    let condition = prepared.version().topology().edges()[0]
        .condition()
        .unwrap();
    assert!(
        condition
            .identifiers()
            .keys()
            .all(|key| key.as_str() != "slot0")
    );
    let slot1 = identifier(condition, "slot1");
    assert!(prepared.version().content_slots().iter().any(|slot| {
        slot.slot_id().as_str() == slot1
            && slot.owner_kind() == ContentOwnerKind::Edge
            && slot.ordinal() == 1
    }));

    let mut both = registered_controls_record();
    both.graph.spec.edges[0].on_false = Some(serde_json::json!({"routeTo": "docs"}));
    refresh_record(&mut both);
    let prepared =
        block_on(externalizer().prepare(scope(&both.graph.metadata.execution_id), &both)).unwrap();
    let condition = prepared.version().topology().edges()[0]
        .condition()
        .unwrap();
    assert_ne!(
        identifier(condition, "slot0"),
        identifier(condition, "slot1")
    );
    validate_persisted_projection(prepared.version()).unwrap();

    let mut compacted = serde_json::to_value(prepared.version()).unwrap();
    let identifiers = compacted["topology"]["edges"][0]["condition"]["identifiers"]
        .as_object_mut()
        .unwrap();
    let on_false = identifiers.remove("slot1").unwrap();
    identifiers.insert("slot0".into(), on_false);
    assert!(validate_persisted_projection(&rehashed_projection(compacted)).is_err());

    let mut swapped = serde_json::to_value(prepared.version()).unwrap();
    let identifiers = swapped["topology"]["edges"][0]["condition"]["identifiers"]
        .as_object_mut()
        .unwrap();
    let first = identifiers["slot0"].clone();
    let second = identifiers["slot1"].clone();
    identifiers.insert("slot0".into(), second);
    identifiers.insert("slot1".into(), first);
    assert!(validate_persisted_projection(&rehashed_projection(swapped)).is_err());
}

#[test]
fn logical_artifact_locator_is_encoded_without_fabricating_registration() {
    let mut record = registered_controls_record();
    record
        .graph
        .spec
        .nodes
        .get_mut("plan")
        .unwrap()
        .properties
        .get_mut("output")
        .unwrap()["publishAs"] = serde_json::json!("artifact://implementation.diff");
    refresh_record(&mut record);
    let prepared =
        block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
            .unwrap();
    assert_eq!(
        decode_reference(identifier(
            control(&prepared, "plan", "output_contract"),
            "publishAs"
        )),
        "artifact://implementation.diff"
    );
    assert_eq!(
        prepared.evidence().len(),
        prepared.version().content_slots().len()
    );
}

#[test]
fn replay_requires_complete_correlated_groups_and_exact_agent_modes() {
    let record = node_kind_registry_record();
    let prepared =
        block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
            .unwrap();
    let baseline = serde_json::to_value(prepared.version()).unwrap();

    let mut half_parameter = baseline.clone();
    let controls = half_parameter["topology"]["nodes"]["subgraph"]["controls"]
        .as_array_mut()
        .unwrap();
    let control = controls
        .iter_mut()
        .find(|control| control["controlType"] == "subgraph_configuration")
        .unwrap();
    control["identifiers"]
        .as_object_mut()
        .unwrap()
        .remove("parameterValue.000");
    assert!(validate_persisted_projection(&rehashed_projection(half_parameter)).is_err());

    let mut orphan_minimum = baseline.clone();
    let controls = orphan_minimum["topology"]["nodes"]["agent"]["controls"]
        .as_array_mut()
        .unwrap();
    let completion = controls
        .iter_mut()
        .find(|control| control["controlType"] == "node_completion")
        .unwrap();
    completion["identifiers"]
        .as_object_mut()
        .unwrap()
        .remove("requiresEvidenceType.001");
    assert!(validate_persisted_projection(&rehashed_projection(orphan_minimum)).is_err());

    let mut missing_ephemeral_schema = baseline.clone();
    let controls = missing_ephemeral_schema["topology"]["nodes"]["agent"]["controls"]
        .as_array_mut()
        .unwrap();
    let agent = controls
        .iter_mut()
        .find(|control| control["controlType"] == "agent_configuration")
        .unwrap();
    agent["identifiers"]
        .as_object_mut()
        .unwrap()
        .remove("inputSchema");
    assert!(validate_persisted_projection(&rehashed_projection(missing_ephemeral_schema)).is_err());

    let mut hybrid = baseline;
    let controls = hybrid["topology"]["nodes"]["agent"]["controls"]
        .as_array_mut()
        .unwrap();
    let agent = controls
        .iter_mut()
        .find(|control| control["controlType"] == "agent_configuration")
        .unwrap();
    agent["identifiers"]["agentRef"] = serde_json::json!(
        graphhelm_graph::encode_persisted_reference("project/security-reviewer@3").unwrap()
    );
    assert!(validate_persisted_projection(&rehashed_projection(hybrid)).is_err());
}

#[test]
fn invalid_loop_and_agent_modes_fail_before_the_first_sealer_call() {
    let mut bad_loop = registered_controls_record();
    bad_loop
        .graph
        .spec
        .nodes
        .get_mut("plan")
        .unwrap()
        .properties
        .insert("loop".into(), serde_json::json!({"maxIterations": 0}));
    refresh_record(&mut bad_loop);

    let mut hybrid_ref = registered_controls_record();
    hybrid_ref
        .graph
        .spec
        .nodes
        .get_mut("map_repository")
        .unwrap()
        .properties
        .insert(
            "agent".into(),
            serde_json::json!({"ref": "project/security-reviewer@3", "capabilities": []}),
        );
    refresh_record(&mut hybrid_ref);

    let mut missing_ephemeral_schema = registered_controls_record();
    missing_ephemeral_schema
        .graph
        .spec
        .nodes
        .get_mut("plan")
        .unwrap()
        .properties
        .get_mut("agent")
        .unwrap()["ephemeral"]
        .as_object_mut()
        .unwrap()
        .remove("inputSchema");
    refresh_record(&mut missing_ephemeral_schema);

    for record in [bad_loop, hybrid_ref, missing_ephemeral_schema] {
        let calls = Arc::new(AtomicUsize::new(0));
        let externalizer = SealingGraphExternalizer::new(CountingFailureSealer(Arc::clone(&calls)));
        assert!(
            block_on(externalizer.prepare(scope(&record.graph.metadata.execution_id), &record))
                .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn normative_dot_bindings_round_trip_as_bounded_safe_values() {
    let mut record = registered_controls_record();
    record
        .graph
        .spec
        .nodes
        .get_mut("plan")
        .unwrap()
        .properties
        .get_mut("input")
        .unwrap()["bindings"]["request"] = serde_json::json!("outputs.implement.patch");
    record.graph.spec.edges[0]
        .bindings
        .insert("worker".into(), "nodes.implement.output".into());
    refresh_record(&mut record);

    let prepared =
        block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
            .unwrap();

    assert_eq!(
        identifier(
            control(&prepared, "plan", "input_contract"),
            "bindingValue.000"
        ),
        "outputs.implement.patch"
    );
    assert!(
        prepared.version().topology().edges()[0]
            .bindings()
            .values()
            .any(|value| value.as_str() == "nodes.implement.output")
    );
    validate_persisted_projection(prepared.version()).unwrap();
}

#[test]
fn invalid_dot_bindings_fail_closed_before_sealing_and_on_replay() {
    let cases = [
        "outputs..patch",
        "nodes.worker.result",
        "nodes.worker.output..patch",
        "outputs.worker...",
        "outputs.worker.patch/secret",
        "outputs.worker.apiKey",
        "foreign.worker.patch",
        "outputs.a.b.c.d.e.f.g.h",
        "outputs.worker.aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ];
    for value in cases {
        let mut record = registered_controls_record();
        record
            .graph
            .spec
            .nodes
            .get_mut("plan")
            .unwrap()
            .properties
            .get_mut("input")
            .unwrap()["bindings"]["request"] = serde_json::json!(value);
        refresh_record(&mut record);
        let calls = Arc::new(AtomicUsize::new(0));
        let externalizer = SealingGraphExternalizer::new(CountingFailureSealer(Arc::clone(&calls)));
        let error =
            block_on(externalizer.prepare(scope(&record.graph.metadata.execution_id), &record))
                .unwrap_err();
        assert!(
            matches!(
                error.code(),
                "GHE009_EXTERNALIZATION_FAILED" | "GHE006_LIMIT_EXCEEDED"
            ),
            "{value}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0, "{value}");
    }

    let mut record = registered_controls_record();
    record.graph.spec.edges[0]
        .bindings
        .insert("worker".into(), "nodes.implement.output".into());
    refresh_record(&mut record);
    let prepared =
        block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
            .unwrap();
    for value in cases {
        let mut wire = serde_json::to_value(prepared.version()).unwrap();
        wire["topology"]["edges"][0]["bindings"]["worker"] = serde_json::json!(value);
        let Ok(provisional) =
            serde_json::from_value::<graphhelm_protocols::PersistedGraphVersion>(wire.clone())
        else {
            continue;
        };
        let hashes = persisted_hashes(provisional.topology(), provisional.content_slots()).unwrap();
        wire["topologyHash"] = serde_json::json!(hashes.topology_hash().as_str());
        wire["semanticHash"] = serde_json::json!(hashes.semantic_hash().as_str());
        let mutated = serde_json::from_value(wire).unwrap();
        assert!(validate_persisted_projection(&mutated).is_err(), "{value}");
    }
}

#[test]
fn target_reference_is_owned_only_by_deploy_nodes() {
    let record = node_kind_registry_record();
    for node_id in record
        .graph
        .spec
        .nodes
        .keys()
        .filter(|id| id.as_str() != "deploy")
    {
        let mut changed = record.clone();
        changed
            .graph
            .spec
            .nodes
            .get_mut(node_id)
            .unwrap()
            .properties
            .insert(
                "targetRef".into(),
                serde_json::json!("environment://staging"),
            );
        refresh_record(&mut changed);
        assert!(
            block_on(externalizer().prepare(scope(&changed.graph.metadata.execution_id), &changed))
                .is_err(),
            "{node_id}"
        );
    }

    block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record)).unwrap();
}

#[test]
fn policy_shapes_are_injective_and_preserve_each_text_slot() {
    let mut collision = version("software-feature.yaml").to_record();
    collision.graph.spec.policies = vec![serde_json::json!({
        "ref": "policy://workspace/security-baseline@2",
        "inlineConstraint": {"deny": ["production.deploy"]}
    })];
    refresh_record(&mut collision);
    assert!(
        block_on(externalizer().prepare(scope(&collision.graph.metadata.execution_id), &collision))
            .is_err()
    );

    let mut record = version("software-feature.yaml").to_record();
    record.graph.spec.policies = vec![serde_json::json!({
        "inlineConstraint": {
            "deny": ["production.deploy"],
            "ruleText": "rule text one",
            "explanation": "explanation two"
        }
    })];
    refresh_record(&mut record);
    let prepared =
        block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
            .unwrap();
    let policy = &prepared.version().topology().policies()[0];
    assert_ne!(
        identifier(policy, "ruleTextSlot"),
        identifier(policy, "explanationSlot")
    );
    assert_eq!(
        prepared
            .version()
            .content_slots()
            .iter()
            .filter(|slot| slot.owner_kind() == ContentOwnerKind::Policy)
            .count(),
        2
    );

    for (field, changed_ordinal, unchanged_ordinal) in
        [("ruleText", 0u32, 1u32), ("explanation", 1u32, 0u32)]
    {
        let mut changed = record.clone();
        changed.graph.spec.policies[0]["inlineConstraint"][field] =
            serde_json::json!(format!("changed {field}"));
        refresh_record(&mut changed);
        let changed =
            block_on(externalizer().prepare(scope(&changed.graph.metadata.execution_id), &changed))
                .unwrap();
        assert_eq!(
            prepared.version().topology_hash(),
            changed.version().topology_hash(),
            "{field}"
        );
        assert_ne!(
            prepared.version().semantic_hash(),
            changed.version().semantic_hash(),
            "{field}"
        );
        let digest = |version: &graphhelm_protocols::PersistedGraphVersion, ordinal| {
            version
                .content_slots()
                .iter()
                .find(|slot| {
                    slot.owner_kind() == ContentOwnerKind::Policy && slot.ordinal() == ordinal
                })
                .unwrap()
                .content_sha256()
                .clone()
        };
        assert_ne!(
            digest(prepared.version(), changed_ordinal),
            digest(changed.version(), changed_ordinal)
        );
        assert_eq!(
            digest(prepared.version(), unchanged_ordinal),
            digest(changed.version(), unchanged_ordinal)
        );
    }
}

#[test]
fn replay_rejects_policy_text_slot_swaps_and_ref_inline_collisions() {
    let mut record = version("software-feature.yaml").to_record();
    record.graph.spec.policies = vec![serde_json::json!({
        "inlineConstraint": {
            "deny": ["production.deploy"],
            "ruleText": "rule text one",
            "explanation": "explanation two"
        }
    })];
    refresh_record(&mut record);
    let prepared =
        block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
            .unwrap();

    let mut swapped = serde_json::to_value(prepared.version()).unwrap();
    let rule = swapped["topology"]["policies"][0]["identifiers"]["ruleTextSlot"].clone();
    let explanation = swapped["topology"]["policies"][0]["identifiers"]["explanationSlot"].clone();
    swapped["topology"]["policies"][0]["identifiers"]["ruleTextSlot"] = explanation;
    swapped["topology"]["policies"][0]["identifiers"]["explanationSlot"] = rule;
    assert!(validate_persisted_projection(&rehashed_projection(swapped)).is_err());

    let mut collision = serde_json::to_value(prepared.version()).unwrap();
    collision["topology"]["policies"][0]["identifiers"]["mode"] = serde_json::json!("ref");
    collision["topology"]["policies"][0]["identifiers"]["policyRef"] = serde_json::json!(
        graphhelm_graph::encode_persisted_reference("policy://workspace/security-baseline@2")
            .unwrap()
    );
    assert!(validate_persisted_projection(&rehashed_projection(collision)).is_err());
}

#[test]
fn replay_rejects_proper_subsets_of_the_governor_projection_image() {
    let record = version("software-feature.yaml").to_record();
    let prepared =
        block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
            .unwrap();
    let baseline = serde_json::to_value(prepared.version()).unwrap();

    let mut mutations = Vec::new();
    let mut without_slots = baseline.clone();
    without_slots["contentSlots"] = serde_json::json!([]);
    for node in without_slots["topology"]["nodes"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        node["contentSlotIds"] = serde_json::json!([]);
    }
    mutations.push(without_slots);

    for (owner_kind, field_kind) in [
        ("graph", "display_name"),
        ("node", "display_name"),
        ("node", "objective"),
    ] {
        let mut value = baseline.clone();
        let slots = value["contentSlots"].as_array_mut().unwrap();
        let removed = slots
            .iter()
            .position(|slot| slot["ownerKind"] == owner_kind && slot["fieldKind"] == field_kind)
            .unwrap();
        let removed_id = slots.remove(removed)["slotId"].as_str().unwrap().to_owned();
        for node in value["topology"]["nodes"]
            .as_object_mut()
            .unwrap()
            .values_mut()
        {
            node["contentSlotIds"]
                .as_array_mut()
                .unwrap()
                .retain(|slot| slot.as_str() != Some(&removed_id));
        }
        mutations.push(value);
    }

    {
        let (node_type, required_control) = ("agent", "agent_configuration");
        let mut value = baseline.clone();
        let node = value["topology"]["nodes"]
            .as_object_mut()
            .unwrap()
            .values_mut()
            .find(|node| node["nodeType"] == node_type)
            .unwrap();
        node["controls"]
            .as_array_mut()
            .unwrap()
            .retain(|control| control["controlType"] != required_control);
        mutations.push(value);
    }

    let gate_record = version("research-to-publish.yaml").to_record();
    let gate_prepared = block_on(externalizer().prepare(
        scope(&gate_record.graph.metadata.execution_id),
        &gate_record,
    ))
    .unwrap();
    let mut without_gate_completion = serde_json::to_value(gate_prepared.version()).unwrap();
    let gate = without_gate_completion["topology"]["nodes"]
        .as_object_mut()
        .unwrap()
        .values_mut()
        .find(|node| node["nodeType"] == "gate")
        .unwrap();
    gate["controls"]
        .as_array_mut()
        .unwrap()
        .retain(|control| control["controlType"] != "node_completion");
    mutations.push(without_gate_completion);

    let mut legacy_completion = baseline.clone();
    let terminal =
        legacy_completion["topology"]["completion"]["identifiers"]["terminal.000"].clone();
    legacy_completion["topology"]["completion"] = serde_json::json!({
        "controlType": "terminal_nodes",
        "identifiers": {"terminalNode": terminal},
        "digests": {},
        "integers": {},
        "flags": {"allowWaivers": false}
    });
    mutations.push(legacy_completion);

    for value in mutations {
        assert!(
            validate_persisted_projection(&rehashed_projection(value)).is_err(),
            "proper subset was accepted"
        );
    }
}

#[test]
fn replay_derives_every_content_slot_profile_from_its_typed_position() {
    for example in [
        "software-feature.yaml",
        "manual-override-deploy.yaml",
        "research-to-publish.yaml",
    ] {
        let mut record = version(example).to_record();
        let externalizer = externalizer();
        let prepared = if record.graph.metadata.version == 1 {
            block_on(externalizer.prepare(scope(&record.graph.metadata.execution_id), &record))
        } else {
            let predecessor_number = record.graph.metadata.version - 1;
            record.predecessor = Some(GraphVersionRef {
                number: predecessor_number,
                content_hash: record.content_hash.clone(),
            });
            block_on(externalizer.prepare_with_predecessor(
                scope(&record.graph.metadata.execution_id),
                &record,
                safe_predecessor(predecessor_number, 'a'),
            ))
        }
        .unwrap_or_else(|error| panic!("{example}: {error:?}"));
        assert_replay_rejects_all_slot_profile_mutations(prepared.version(), example);
    }

    let mut synthetic = registered_controls_record();
    synthetic.graph.metadata.properties.insert(
        "description".into(),
        serde_json::json!("registered graph description"),
    );
    synthetic
        .graph
        .spec
        .nodes
        .get_mut("plan")
        .unwrap()
        .properties
        .insert(
            "description".into(),
            serde_json::json!("registered node description"),
        );
    synthetic.graph.spec.policies = vec![serde_json::json!({
        "inlineConstraint": {
            "deny": ["production.deploy"],
            "ruleText": "registered rule text",
            "explanation": "registered explanation"
        }
    })];
    refresh_record(&mut synthetic);
    let prepared =
        block_on(externalizer().prepare(scope(&synthetic.graph.metadata.execution_id), &synthetic))
            .unwrap();
    assert_replay_rejects_all_slot_profile_mutations(prepared.version(), "synthetic-all-content");
}

#[test]
fn policy_manual_override_and_inline_enforcement_are_mutually_exclusive() {
    let mut collision = version("software-feature.yaml").to_record();
    collision.graph.spec.policies = vec![serde_json::json!({
        "inlineConstraint": {
            "deny": ["production.deploy"],
            "manualOverride": {
                "bypassedRequirements": ["independent_review"],
                "acknowledgedRisks": ["unreviewed_change"],
                "resultLabel": "waived"
            }
        }
    })];
    refresh_record(&mut collision);
    assert!(
        block_on(
            externalizer().prepare(scope(&collision.graph.metadata.execution_id), &collision,)
        )
        .is_err()
    );

    let mut record = version("software-feature.yaml").to_record();
    record.graph.spec.policies = vec![serde_json::json!({
        "inlineConstraint": {
            "manualOverride": {
                "bypassedRequirements": ["independent_review"],
                "acknowledgedRisks": ["unreviewed_change"],
                "resultLabel": "waived"
            }
        }
    })];
    refresh_record(&mut record);
    let prepared =
        block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
            .unwrap();
    let mut value = serde_json::to_value(prepared.version()).unwrap();
    value["topology"]["policies"][0]["integers"]["denyCount"] = serde_json::json!(1);
    value["topology"]["policies"][0]["identifiers"]["deny.000"] =
        serde_json::json!("production.deploy");
    assert!(validate_persisted_projection(&rehashed_projection(value)).is_err());

    record.graph.spec.policies = vec![serde_json::json!({
        "inlineConstraint": {"deny": ["production.deploy"]}
    })];
    refresh_record(&mut record);
    let prepared =
        block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
            .unwrap();
    let mut value = serde_json::to_value(prepared.version()).unwrap();
    value["topology"]["policies"][0]["integers"]["bypassedRequirementCount"] = serde_json::json!(1);
    value["topology"]["policies"][0]["identifiers"]["bypassedRequirement.000"] =
        serde_json::json!("independent_review");
    assert!(validate_persisted_projection(&rehashed_projection(value)).is_err());
}

#[test]
fn manual_override_requires_complete_audit_fields_before_sealing() {
    let invalid = [
        serde_json::json!({
            "bypassedRequirements": ["independent_review"],
            "resultLabel": "waived"
        }),
        serde_json::json!({
            "acknowledgedRisks": ["unreviewed_change"],
            "resultLabel": "waived"
        }),
        serde_json::json!({
            "bypassedRequirements": ["independent_review"],
            "acknowledgedRisks": ["unreviewed_change"]
        }),
        serde_json::json!({
            "bypassedRequirements": ["independent_review"],
            "acknowledgedRisks": ["unreviewed_change"],
            "resultLabel": ""
        }),
        serde_json::json!({
            "bypassedRequirements": [],
            "acknowledgedRisks": ["unreviewed_change"],
            "resultLabel": "waived"
        }),
        serde_json::json!({
            "bypassedRequirements": ["independent_review"],
            "acknowledgedRisks": [],
            "resultLabel": "waived"
        }),
    ];

    for manual_override in invalid {
        let mut record = version("software-feature.yaml").to_record();
        record.graph.spec.policies = vec![serde_json::json!({
            "inlineConstraint": {"manualOverride": manual_override}
        })];
        refresh_record(&mut record);
        let calls = Arc::new(AtomicUsize::new(0));
        let externalizer = SealingGraphExternalizer::new(CountingFailureSealer(Arc::clone(&calls)));

        assert!(
            block_on(externalizer.prepare(scope(&record.graph.metadata.execution_id), &record))
                .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    let mut record = version("software-feature.yaml").to_record();
    record.graph.spec.policies = vec![serde_json::json!({
        "inlineConstraint": {"manualOverride": {
            "bypassedRequirements": ["independent_review"],
            "acknowledgedRisks": ["unreviewed_change"],
            "resultLabel": "waived"
        }}
    })];
    refresh_record(&mut record);
    block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record)).unwrap();
}

#[test]
fn replay_rejects_partial_manual_override_controls_after_rehash() {
    let mut record = version("software-feature.yaml").to_record();
    record.graph.spec.policies = vec![serde_json::json!({
        "inlineConstraint": {"manualOverride": {
            "bypassedRequirements": ["independent_review"],
            "acknowledgedRisks": ["unreviewed_change"],
            "resultLabel": "waived"
        }}
    })];
    refresh_record(&mut record);
    let prepared =
        block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
            .unwrap();
    let complete = serde_json::to_value(prepared.version()).unwrap();

    for mutation in [
        "only_bypassed",
        "only_risks",
        "zero_bypassed",
        "zero_risks",
        "missing_label",
    ] {
        let mut value = complete.clone();
        let control = &mut value["topology"]["policies"][0];
        match mutation {
            "only_bypassed" => {
                control["integers"]
                    .as_object_mut()
                    .unwrap()
                    .remove("acknowledgedRiskCount");
                control["identifiers"]
                    .as_object_mut()
                    .unwrap()
                    .remove("acknowledgedRisk.000");
            }
            "only_risks" => {
                control["integers"]
                    .as_object_mut()
                    .unwrap()
                    .remove("bypassedRequirementCount");
                control["identifiers"]
                    .as_object_mut()
                    .unwrap()
                    .remove("bypassedRequirement.000");
                control["identifiers"]
                    .as_object_mut()
                    .unwrap()
                    .remove("bypassedRequirement.001");
            }
            "missing_label" => {
                control["identifiers"]
                    .as_object_mut()
                    .unwrap()
                    .remove("resultLabel");
            }
            "zero_bypassed" => {
                control["integers"]["bypassedRequirementCount"] = serde_json::json!(0);
                control["identifiers"]
                    .as_object_mut()
                    .unwrap()
                    .remove("bypassedRequirement.000");
            }
            "zero_risks" => {
                control["integers"]["acknowledgedRiskCount"] = serde_json::json!(0);
                control["identifiers"]
                    .as_object_mut()
                    .unwrap()
                    .remove("acknowledgedRisk.000");
            }
            _ => unreachable!(),
        }
        assert!(
            validate_persisted_projection(&rehashed_projection(value)).is_err(),
            "{mutation}"
        );
    }

    let mut empty_label = complete;
    empty_label["topology"]["policies"][0]["identifiers"]["resultLabel"] = serde_json::json!("");
    assert!(
        serde_json::from_value::<graphhelm_protocols::PersistedGraphVersion>(empty_label).is_err()
    );
    validate_persisted_projection(prepared.version()).unwrap();
}

#[test]
fn annotation_only_schema_containers_fail_before_sealing() {
    for annotation in [
        "title",
        "description",
        "examples",
        "$comment",
        "default",
        "deprecated",
        "readOnly",
        "writeOnly",
    ] {
        let mut record = version("software-feature.yaml").to_record();
        let annotation_value = match annotation {
            "examples" => serde_json::json!(["x"]),
            "deprecated" | "readOnly" | "writeOnly" => serde_json::json!(true),
            _ => serde_json::json!("x"),
        };
        record
            .graph
            .spec
            .nodes
            .get_mut("plan")
            .unwrap()
            .properties
            .insert(
                "input".into(),
                serde_json::json!({annotation: annotation_value}),
            );
        refresh_record(&mut record);
        let calls = Arc::new(AtomicUsize::new(0));
        let externalizer = SealingGraphExternalizer::new(CountingFailureSealer(Arc::clone(&calls)));
        let error =
            block_on(externalizer.prepare(scope(&record.graph.metadata.execution_id), &record))
                .unwrap_err();
        assert_eq!(
            error.code(),
            "GHE009_EXTERNALIZATION_FAILED",
            "{annotation}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0, "{annotation}");
    }
}

#[test]
fn every_registered_json_schema_annotation_is_non_semantic_at_any_depth() {
    let mut baseline = registered_controls_record();
    baseline
        .graph
        .spec
        .nodes
        .get_mut("plan")
        .unwrap()
        .properties
        .get_mut("input")
        .unwrap()["schema"] = serde_json::json!({
        "type": "object",
        "properties": {"request": {"type": "string"}}
    });
    refresh_record(&mut baseline);
    let expected =
        block_on(externalizer().prepare(scope(&baseline.graph.metadata.execution_id), &baseline))
            .unwrap();

    for annotation in [
        "title",
        "description",
        "examples",
        "$comment",
        "default",
        "deprecated",
        "readOnly",
        "writeOnly",
    ] {
        for nested in [false, true] {
            let mut changed = baseline.clone();
            let schema = &mut changed
                .graph
                .spec
                .nodes
                .get_mut("plan")
                .unwrap()
                .properties
                .get_mut("input")
                .unwrap()["schema"];
            let target = if nested {
                &mut schema["properties"]["request"]
            } else {
                schema
            };
            target[annotation] = match annotation {
                "examples" => serde_json::json!(["ignored"]),
                "deprecated" | "readOnly" | "writeOnly" => serde_json::json!(true),
                _ => serde_json::json!("ignored"),
            };
            refresh_record(&mut changed);
            let prepared = block_on(
                externalizer().prepare(scope(&changed.graph.metadata.execution_id), &changed),
            )
            .unwrap();
            assert_eq!(
                expected.version(),
                prepared.version(),
                "{annotation} nested={nested}"
            );
        }
    }
}

#[test]
fn legacy_definitions_members_are_schema_positions_for_annotation_identity() {
    let prepare = |schema: serde_json::Value| {
        let mut record = registered_controls_record();
        record
            .graph
            .spec
            .nodes
            .get_mut("plan")
            .unwrap()
            .properties
            .get_mut("input")
            .unwrap()["schema"] = schema;
        refresh_record(&mut record);
        block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
            .unwrap()
    };

    let baseline = prepare(serde_json::json!({
        "definitions": {"legacy": {"type": "string"}}
    }));
    let annotated = prepare(serde_json::json!({
        "definitions": {
            "legacy": {
                "type": "string",
                "title": "ignored",
                "default": "ignored",
                "deprecated": true
            }
        }
    }));
    let structural = prepare(serde_json::json!({
        "definitions": {"legacy": {"type": "integer"}}
    }));

    assert_eq!(baseline.version(), annotated.version());
    assert_ne!(baseline.version(), structural.version());
}

#[test]
fn schema_identity_preserves_schema_map_names_and_literal_annotation_lookalikes() {
    let cases = [
        (
            "property named default",
            serde_json::json!({
                "type": "object",
                "properties": {"default": {"type": "string"}}
            }),
            serde_json::json!({"type": "object", "properties": {}}),
        ),
        (
            "$defs member named title",
            serde_json::json!({
                "type": "object",
                "$defs": {"title": {"type": "string"}}
            }),
            serde_json::json!({"type": "object", "$defs": {}}),
        ),
        (
            "definitions member named examples",
            serde_json::json!({
                "type": "object",
                "definitions": {"examples": {"type": "integer"}}
            }),
            serde_json::json!({"type": "object", "definitions": {}}),
        ),
        (
            "patternProperties member named deprecated",
            serde_json::json!({
                "type": "object",
                "patternProperties": {"deprecated": {"type": "boolean"}}
            }),
            serde_json::json!({"type": "object", "patternProperties": {}}),
        ),
        (
            "dependentSchemas member named description",
            serde_json::json!({
                "type": "object",
                "dependentSchemas": {"description": {"required": ["dependency"]}}
            }),
            serde_json::json!({"type": "object", "dependentSchemas": {}}),
        ),
        (
            "dependencies member named writeOnly",
            serde_json::json!({
                "type": "object",
                "dependencies": {"writeOnly": {"required": ["dependency"]}}
            }),
            serde_json::json!({"type": "object", "dependencies": {}}),
        ),
        (
            "const literal member named default",
            serde_json::json!({"const": {"default": "literal-a"}}),
            serde_json::json!({"const": {}}),
        ),
        (
            "enum literal member named title",
            serde_json::json!({"enum": [{"title": "literal-b"}]}),
            serde_json::json!({"enum": [{}]}),
        ),
    ];

    for (name, left_schema, right_schema) in cases {
        let prepare = |schema: serde_json::Value| {
            let mut record = registered_controls_record();
            record
                .graph
                .spec
                .nodes
                .get_mut("plan")
                .unwrap()
                .properties
                .get_mut("input")
                .unwrap()["schema"] = schema;
            refresh_record(&mut record);
            block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
                .unwrap()
        };
        let left = prepare(left_schema);
        let right = prepare(right_schema);
        assert_ne!(
            control(&left, "plan", "input_contract").digests(),
            control(&right, "plan", "input_contract").digests(),
            "{name}"
        );
        assert_ne!(
            left.version().semantic_hash(),
            right.version().semantic_hash(),
            "{name}"
        );
    }
}

#[test]
fn inline_schema_shapes_are_validated_before_structural_identity() {
    let prepare = |schema: serde_json::Value| {
        let mut record = registered_controls_record();
        record
            .graph
            .spec
            .nodes
            .get_mut("plan")
            .unwrap()
            .properties
            .get_mut("input")
            .unwrap()["schema"] = schema;
        refresh_record(&mut record);
        let calls = Arc::new(AtomicUsize::new(0));
        let externalizer = SealingGraphExternalizer::new(CountingFailureSealer(Arc::clone(&calls)));
        let result =
            block_on(externalizer.prepare(scope(&record.graph.metadata.execution_id), &record));
        (result, calls.load(Ordering::SeqCst))
    };

    for schema in [
        serde_json::json!({"not": []}),
        serde_json::json!({"properties": {"x": []}}),
        serde_json::json!({"allOf": [{"type": "string"}, 7]}),
        serde_json::json!({"dependentSchemas": []}),
        serde_json::json!({"dependentSchemas": {"x": "not-a-schema"}}),
    ] {
        let (result, calls) = prepare(schema);
        let error = result.unwrap_err();
        assert_eq!(error.code(), "GHE009_EXTERNALIZATION_FAILED");
        assert_eq!(calls, 0);
    }

    for schema in [
        serde_json::json!(true),
        serde_json::json!(false),
        serde_json::json!({"properties": {"object": {"type": "string"}, "boolean": true}}),
        serde_json::json!({"allOf": [{"type": "string"}, false]}),
        serde_json::json!({"items": {"type": "string"}}),
        serde_json::json!({"x-custom-literal": {"schema": {"required": ["x"]}, "boolean": false, "names": ["x", "y"]}}),
    ] {
        let mut record = registered_controls_record();
        record
            .graph
            .spec
            .nodes
            .get_mut("plan")
            .unwrap()
            .properties
            .get_mut("input")
            .unwrap()["schema"] = schema;
        refresh_record(&mut record);
        assert!(
            block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
                .is_ok()
        );
    }
}

#[test]
fn normative_inline_schema_compiler_rejects_malformed_values_before_sealing() {
    let prepare = |schema: serde_json::Value| {
        let mut record = registered_controls_record();
        record
            .graph
            .spec
            .nodes
            .get_mut("plan")
            .unwrap()
            .properties
            .get_mut("input")
            .unwrap()["schema"] = schema;
        refresh_record(&mut record);
        let calls = Arc::new(AtomicUsize::new(0));
        let externalizer = SealingGraphExternalizer::new(CountingFailureSealer(Arc::clone(&calls)));
        let result =
            block_on(externalizer.prepare(scope(&record.graph.metadata.execution_id), &record));
        (result, calls.load(Ordering::SeqCst))
    };

    let malformed = [
        ("scalar type", serde_json::json!({"type": 7})),
        ("unknown type", serde_json::json!({"type": "integerish"})),
        ("empty type array", serde_json::json!({"type": []})),
        (
            "duplicate type",
            serde_json::json!({"type": ["string", "string"]}),
        ),
        (
            "non-string type member",
            serde_json::json!({"type": ["string", 7]}),
        ),
        (
            "title type",
            serde_json::json!({"type": "string", "title": false}),
        ),
        (
            "description type",
            serde_json::json!({"type": "string", "description": []}),
        ),
        (
            "comment type",
            serde_json::json!({"type": "string", "$comment": 1}),
        ),
        (
            "examples type",
            serde_json::json!({"type": "string", "examples": {}}),
        ),
        (
            "deprecated type",
            serde_json::json!({"type": "string", "deprecated": "yes"}),
        ),
        (
            "readOnly type",
            serde_json::json!({"type": "string", "readOnly": 1}),
        ),
        (
            "writeOnly type",
            serde_json::json!({"type": "string", "writeOnly": null}),
        ),
        ("empty allOf", serde_json::json!({"allOf": []})),
        ("empty anyOf", serde_json::json!({"anyOf": []})),
        ("empty oneOf", serde_json::json!({"oneOf": []})),
        (
            "duplicate required",
            serde_json::json!({"required": ["x", "x"]}),
        ),
        (
            "duplicate dependency",
            serde_json::json!({"dependentRequired": {"x": ["y", "y"]}}),
        ),
        (
            "invalid pattern",
            serde_json::json!({"pattern": "[unterminated"}),
        ),
        (
            "dangling regex escape",
            serde_json::json!({"pattern": "value\\"}),
        ),
        (
            "invalid hex escape",
            serde_json::json!({"pattern": "\\xGG"}),
        ),
        (
            "reversed quantifier",
            serde_json::json!({"pattern": "a{2,1}"}),
        ),
        ("unmatched group", serde_json::json!({"pattern": "a)"})),
        (
            "invalid property pattern",
            serde_json::json!({"patternProperties": {"(?": true}}),
        ),
        ("negative minLength", serde_json::json!({"minLength": -1})),
        ("fractional maxItems", serde_json::json!({"maxItems": 1.5})),
        (
            "negative minProperties",
            serde_json::json!({"minProperties": -1}),
        ),
        ("zero multipleOf", serde_json::json!({"multipleOf": 0})),
        (
            "negative multipleOf",
            serde_json::json!({"multipleOf": -0.5}),
        ),
        (
            "invalid uniqueItems",
            serde_json::json!({"uniqueItems": "yes"}),
        ),
        ("nonnumeric minimum", serde_json::json!({"minimum": "zero"})),
        ("invalid properties", serde_json::json!({"properties": []})),
        (
            "invalid defs member",
            serde_json::json!({"$defs": {"x": null}}),
        ),
        (
            "invalid prefixItems",
            serde_json::json!({"prefixItems": []}),
        ),
        ("invalid ref type", serde_json::json!({"$ref": 7})),
        ("invalid id type", serde_json::json!({"$id": false})),
        ("invalid anchor type", serde_json::json!({"$anchor": []})),
        (
            "invalid ref grammar",
            serde_json::json!({"$ref": "bad<ref"}),
        ),
        (
            "bad anchor grammar",
            serde_json::json!({"$anchor": "bad anchor"}),
        ),
        (
            "invalid vocabulary",
            serde_json::json!({"$vocabulary": {"https://example.test/vocab": "required"}}),
        ),
    ];

    for (name, schema) in malformed {
        let (result, calls) = prepare(schema);
        let error = result.expect_err(name);
        assert_eq!(error.code(), "GHE009_EXTERNALIZATION_FAILED", "{name}");
        assert_eq!(calls, 0, "{name}");
        assert!(!format!("{error:?} {error}").contains(name));
    }
}

#[test]
fn normative_inline_schema_compiler_accepts_edge_cases_and_preserves_custom_literals() {
    let prepare = |schema: serde_json::Value| {
        let mut record = registered_controls_record();
        record
            .graph
            .spec
            .nodes
            .get_mut("plan")
            .unwrap()
            .properties
            .get_mut("input")
            .unwrap()["schema"] = schema;
        refresh_record(&mut record);
        block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
            .unwrap()
    };

    for schema in [
        serde_json::json!(true),
        serde_json::json!(false),
        serde_json::json!({"enum": []}),
        serde_json::json!({"required": []}),
        serde_json::json!({"required": [""]}),
        serde_json::json!({"dependentRequired": {"x": []}}),
        serde_json::json!({"dependencies": {"x": []}}),
        serde_json::json!({"format": ""}),
        serde_json::json!({"$ref": ""}),
        serde_json::json!({"$vocabulary": {}}),
        serde_json::json!({"minLength": 1.0}),
        serde_json::json!({
            "$id": "https://p50.dev/schemas/inline.schema.json",
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$anchor": "root_contract",
            "$dynamicAnchor": "dynamic_contract",
            "$ref": "#/$defs/value",
            "$dynamicRef": "#dynamic_contract",
            "$vocabulary": {"https://json-schema.org/draft/2020-12/vocab/core": true},
            "type": ["string", "null"],
            "minLength": 1,
            "maxLength": 8,
            "pattern": "^(?:[a-z]+?){1,8}$",
            "format": "date-time",
            "$defs": {"value": {"type": "string"}}
        }),
        serde_json::json!({
            "$id": "https://p50.dev/schemas/all-keywords.schema.json",
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$vocabulary": {"https://json-schema.org/draft/2020-12/vocab/core": true},
            "$anchor": "root",
            "$dynamicAnchor": "dynamic",
            "$ref": "#/$defs/value",
            "$dynamicRef": "#dynamic",
            "$defs": {"value": {"type": "string"}},
            "definitions": {"legacy": false},
            "title": "title",
            "description": "description",
            "$comment": "comment",
            "examples": ["example"],
            "default": {"literal": true},
            "deprecated": false,
            "readOnly": false,
            "writeOnly": false,
            "type": ["object", "null"],
            "enum": [{"case": 1}, {"case": 2}],
            "const": {"literal": "value"},
            "multipleOf": 0.5,
            "minimum": -10,
            "exclusiveMinimum": -11,
            "maximum": 10,
            "exclusiveMaximum": 11,
            "minLength": 0,
            "maxLength": 64,
            "pattern": "^(?:[a-z]+?){1,8}$",
            "format": "date-time",
            "contentEncoding": "base64",
            "contentMediaType": "application/json",
            "contentSchema": true,
            "minItems": 0,
            "maxItems": 8,
            "uniqueItems": true,
            "minContains": 0,
            "maxContains": 2,
            "prefixItems": [true],
            "items": false,
            "contains": {"type": "string"},
            "additionalItems": true,
            "unevaluatedItems": false,
            "minProperties": 0,
            "maxProperties": 8,
            "required": ["value"],
            "properties": {"value": {"type": "string"}},
            "patternProperties": {"^x-": true},
            "additionalProperties": false,
            "unevaluatedProperties": true,
            "propertyNames": {"type": "string"},
            "dependentRequired": {"value": ["other"]},
            "dependentSchemas": {"value": true},
            "dependencies": {"legacyNames": ["value"], "legacySchema": false},
            "allOf": [true],
            "anyOf": [true],
            "oneOf": [true],
            "not": false,
            "if": true,
            "then": true,
            "else": false
        }),
        // Unsatisfiable constraints remain valid JSON Schemas. This boundary
        // validates keyword grammar, not satisfiability.
        serde_json::json!({
            "minimum": 10,
            "exclusiveMinimum": 12,
            "maximum": 5,
            "minLength": 4,
            "maxLength": 2,
            "minItems": 3,
            "maxItems": 1,
            "minProperties": 2,
            "maxProperties": 0
        }),
    ] {
        prepare(schema);
    }

    let left = prepare(serde_json::json!({
        "type": "object",
        "x-graphhelm-literal": {"title": "literal-a", "nested": [true, 1]}
    }));
    let right = prepare(serde_json::json!({
        "type": "object",
        "x-graphhelm-literal": {"title": "literal-b", "nested": [true, 1]}
    }));
    assert_ne!(
        control(&left, "plan", "input_contract").digests(),
        control(&right, "plan", "input_contract").digests()
    );
    assert_ne!(
        left.version().semantic_hash(),
        right.version().semantic_hash()
    );
}

#[test]
fn mutation_id_is_excluded_from_safe_projection_identity_and_sealing() {
    let baseline_record = version("software-feature.yaml").to_record();
    let baseline = block_on(externalizer().prepare(
        scope(&baseline_record.graph.metadata.execution_id),
        &baseline_record,
    ))
    .unwrap();

    for mutation_id in ["mutation-one", "mutation-two"] {
        let mut changed = baseline_record.clone();
        changed
            .graph
            .metadata
            .properties
            .insert("mutationId".into(), serde_json::json!(mutation_id));
        refresh_record(&mut changed);
        let prepared =
            block_on(externalizer().prepare(scope(&changed.graph.metadata.execution_id), &changed))
                .unwrap();
        assert_eq!(baseline.version(), prepared.version());
        assert_eq!(baseline.evidence().len(), prepared.evidence().len());
    }
}

#[test]
fn replay_rejects_a_rehashed_slot_bound_to_the_wrong_typed_field() {
    let record = version("software-feature.yaml").to_record();
    let prepared =
        block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
            .unwrap();
    let mut wire = serde_json::to_value(prepared.version()).unwrap();
    let wrong_slot = wire["contentSlots"]
        .as_array()
        .unwrap()
        .iter()
        .find(|slot| {
            slot["ownerId"] == "tests" && slot["fieldKind"] == "objective" && slot["ordinal"] == 0
        })
        .unwrap()["slotId"]
        .clone();
    let controls = wire["topology"]["nodes"]["tests"]["controls"]
        .as_array_mut()
        .unwrap();
    let completion = controls
        .iter_mut()
        .find(|control| control["controlType"] == "node_completion")
        .unwrap();
    completion["identifiers"]["requiresSlot.000"] = wrong_slot;
    let mutated = rehashed_projection(wire);

    assert_eq!(
        validate_persisted_projection(&mutated).unwrap_err(),
        graphhelm_graph::GraphError::InvalidProjection
    );
}

#[test]
fn official_graphs_prepare_safe_projection_without_authoring_plaintext() {
    let canaries = [
        (
            "software-feature.yaml",
            [
                "Implementar feature com validação adaptativa",
                "Localizar componentes, dependências e testes relacionados.",
                "Mapear o escopo técnico da mudança.",
                "Produza um mapa com locations e evidências.",
            ],
        ),
        (
            "research-to-publish.yaml",
            [
                "Pesquisa, proposta de valor e publicação",
                "Identificar alternativas, posicionamento e evidências.",
                "Pesquisar concorrentes com fontes verificáveis.",
                "Diferencie fato, inferência e opinião.",
            ],
        ),
        (
            "manual-override-deploy.yaml",
            [
                "Deploy com override manual",
                "Produzir build implantável.",
                "Implementar a mudança.",
                "Produza patch e build artifact.",
            ],
        ),
    ];

    for (name, values) in canaries {
        let mut version = version(name).to_record();
        let externalizer = externalizer();
        let prepared = if version.graph.metadata.version == 1 {
            block_on(externalizer.prepare(scope(&version.graph.metadata.execution_id), &version))
        } else {
            let predecessor_number = version.graph.metadata.version - 1;
            version.predecessor = Some(GraphVersionRef {
                number: predecessor_number,
                content_hash: version.content_hash.clone(),
            });
            block_on(externalizer.prepare_with_predecessor(
                scope(&version.graph.metadata.execution_id),
                &version,
                safe_predecessor(predecessor_number, 'a'),
            ))
        }
        .unwrap_or_else(|error| panic!("{name}: {error:?}"));
        let bytes = serde_json::to_vec(prepared.version()).unwrap();
        let expected_hashes = match name {
            // RE-RECORDED DELIBERATELY. `PersistedNode` now carries the operator's declared
            // `timeoutSeconds`, so the persisted topology of any graph that declares one is a
            // different document and hashes differently. Both values move together because
            // `persisted_hashes` derives the semantic hash from the topology; the AUTHORING
            // graph is untouched and means exactly what it meant before.
            //
            // The evidence that this is the declared change and not an accident: of the three
            // canaries, only this one moved. `research-to-publish.yaml` and
            // `manual-override-deploy.yaml` declare no `timeoutSeconds` (measured: zero
            // occurrences) and both still hash to the values frozen before this change.
            "software-feature.yaml" => (
                "sha256:804511a9be8aa778291e3740c96bba7a27bb706b31cb56100c46fd3fae27929b",
                "sha256:4eaa88979d43364ac37e0bfe0bf70bdd08d90486f7bc78024ffcb47951ac5c97",
            ),
            "research-to-publish.yaml" => (
                "sha256:5a71610a5a01f0f39f855276fbb3ff6a818cb83f5950ec626e0b686711ed50a1",
                "sha256:2564d1a96bcddfe8b02c1dabc48598f41f01b505299a8a48407dfaab9db6d772",
            ),
            "manual-override-deploy.yaml" => (
                "sha256:2c83cf1296d8142e6c8861d34bd4c0d5735de04642bf7e4b713587be729c0559",
                "sha256:54f7d21c9d1d2a326ebfa02215aed380f0a18c8a0006333d92e133d69d8d9a16",
            ),
            _ => unreachable!(),
        };
        assert_eq!(prepared.version.topology_hash().as_str(), expected_hashes.0);
        assert_eq!(prepared.version.semantic_hash().as_str(), expected_hashes.1);
        let surfaces = format!("{prepared:?}");
        for value in values {
            assert!(
                !bytes
                    .windows(value.len())
                    .any(|window| window == value.as_bytes()),
                "{name}: {value}"
            );
            assert!(!surfaces.contains(value), "{name}: {value}");
        }
        assert_eq!(
            prepared.version.content_slots().len(),
            prepared.evidence_refs.len()
        );
        assert_eq!(prepared.evidence.len(), prepared.evidence_refs.len());
    }
}

#[test]
fn re_encryption_changes_ciphertext_but_not_persisted_identity() {
    let version = version("software-feature.yaml").to_record();
    let externalizer = SealingGraphExternalizer::new(DeterministicRewrappingSealer {
        inner: EvidenceProtector::new(FixedKeyProvider),
        generation: AtomicUsize::new(0),
    });
    let first =
        block_on(externalizer.prepare(scope(&version.graph.metadata.execution_id), &version))
            .unwrap();
    let second =
        block_on(externalizer.prepare(scope(&version.graph.metadata.execution_id), &version))
            .unwrap();

    assert_eq!(first.version(), second.version());
    assert_ne!(first.evidence()[0].nonce(), second.evidence()[0].nonce());
    assert_ne!(
        first.evidence()[0].ciphertext(),
        second.evidence()[0].ciphertext()
    );
    assert_eq!(
        first
            .evidence_refs()
            .iter()
            .map(EvidenceReference::evidence_id)
            .collect::<Vec<_>>(),
        second
            .evidence_refs()
            .iter()
            .map(EvidenceReference::evidence_id)
            .collect::<Vec<_>>()
    );
}

fn evidence_id_for<'a>(
    preparation: &'a graphhelm_governor::ProjectionPreparation,
    owner_kind: ContentOwnerKind,
    owner_id: &str,
    field_kind: ContentFieldKind,
) -> &'a EvidenceId {
    preparation
        .version()
        .content_slots()
        .iter()
        .find(|slot| {
            slot.owner_kind() == owner_kind
                && slot.owner_id().as_str() == owner_id
                && slot.field_kind() == field_kind
                && slot.ordinal() == 0
        })
        .unwrap()
        .evidence_id()
}

#[test]
fn evidence_identity_is_bound_to_content_version_and_repository_scope() {
    let original = version("software-feature.yaml");
    let original_record = original.to_record();
    let original_scope = scope(&original_record.graph.metadata.execution_id);
    let first = block_on(externalizer().prepare(original_scope.clone(), &original_record)).unwrap();

    let mut changed_content = original_record.clone();
    changed_content
        .graph
        .spec
        .nodes
        .get_mut("plan")
        .unwrap()
        .objective = "publication identity content change".into();
    refresh_record(&mut changed_content);
    let content_publication =
        block_on(externalizer().prepare(original_scope.clone(), &changed_content)).unwrap();

    let mut successor_graph = original.graph().clone();
    successor_graph.metadata.version = original.number() + 1;
    let successor = GraphVersion::publish(
        successor_graph,
        Some(GraphVersionRef {
            number: original.number(),
            content_hash: original.content_hash().clone(),
        }),
        Actor::new(ActorType::Owner, "owner-test"),
        Utc.with_ymd_and_hms(2026, 8, 9, 12, 1, 0).unwrap(),
    )
    .unwrap();
    let successor_record = successor.to_record();
    let version_publication = block_on(
        externalizer().prepare_with_predecessor(
            original_scope.clone(),
            &successor_record,
            PersistedGraphVersionRef::new(
                first.version().number(),
                first.version().semantic_hash().clone(),
            )
            .unwrap(),
        ),
    )
    .unwrap();

    let alternate_scope = RepositoryScope::new(
        WorkspaceId::parse("workspace-test").unwrap(),
        ProjectId::parse("project-alternate").unwrap(),
        Some(ExecutionId::parse(&original_record.graph.metadata.execution_id).unwrap()),
    );
    let scoped_publication =
        block_on(externalizer().prepare(alternate_scope, &original_record)).unwrap();

    let objective = |prepared| {
        evidence_id_for(
            prepared,
            ContentOwnerKind::Node,
            "plan",
            ContentFieldKind::Objective,
        )
    };
    assert_ne!(objective(&first), objective(&content_publication));
    assert_ne!(objective(&first), objective(&version_publication));
    assert_ne!(objective(&first), objective(&scoped_publication));

    let retry = block_on(externalizer().prepare(original_scope, &original_record)).unwrap();
    assert_eq!(objective(&first), objective(&retry));
    assert_eq!(
        first.version().content_slots(),
        retry.version().content_slots()
    );
}

struct DeterministicRewrappingSealer {
    inner: EvidenceProtector<FixedKeyProvider>,
    generation: AtomicUsize,
}

impl EvidenceSealer for DeterministicRewrappingSealer {
    fn seal<'a>(
        &'a self,
        scope: RepositoryScope,
        input: EvidenceInput,
    ) -> RepositoryFuture<'a, Result<SealedEvidence, EvidenceError>> {
        Box::pin(async move {
            let sealed = self.inner.seal(scope.clone(), input).await?;
            let generation = self.generation.fetch_add(1, Ordering::SeqCst);
            let marker = u8::try_from(generation % 200 + 20).unwrap();
            let nonce = vec![marker; 24];
            let ciphertext = vec![marker; sealed.ciphertext().len()];
            let reference = EvidenceReference::new(
                sealed.reference().evidence_id().clone(),
                sealed.reference().content_sha256().clone(),
                raw_content_sha256(&ciphertext).map_err(|_| EvidenceError::Invalid)?,
            );
            SealedEvidence::new(
                reference,
                scope,
                sealed.media_type().as_str(),
                sealed.sensitivity(),
                sealed.retention_class(),
                sealed.algorithm(),
                nonce,
                ciphertext,
                sealed.wrapped_key().clone(),
            )
        })
    }
}

#[test]
fn content_change_updates_only_semantic_identity() {
    let original = version("software-feature.yaml").to_record();
    let mut changed = original.clone();
    changed.graph.spec.nodes.get_mut("plan").unwrap().objective =
        "A distinct objective canary".into();
    changed.semantic = graphhelm_graph::canonicalize(&changed.graph).unwrap().value;
    changed.content_hash = graphhelm_graph::semantic_hash(&changed.graph).unwrap();

    let first =
        block_on(externalizer().prepare(scope(&original.graph.metadata.execution_id), &original))
            .unwrap();
    let second =
        block_on(externalizer().prepare(scope(&changed.graph.metadata.execution_id), &changed))
            .unwrap();

    assert_eq!(
        first.version().topology_hash(),
        second.version().topology_hash()
    );
    assert_ne!(
        first.version().semantic_hash(),
        second.version().semantic_hash()
    );
}

#[test]
fn completion_expression_changes_semantic_but_not_topology_identity() {
    let original = node_kind_registry_record();
    let mut changed = original.clone();
    changed
        .graph
        .spec
        .nodes
        .get_mut("agent")
        .unwrap()
        .properties
        .get_mut("completion")
        .unwrap()["requires"][2]["expression"] = serde_json::json!("output.confidence >= 0.9");
    refresh_record(&mut changed);

    let scope = scope(&original.graph.metadata.execution_id);
    let first = block_on(externalizer().prepare(scope.clone(), &original)).unwrap();
    let second = block_on(externalizer().prepare(scope, &changed)).unwrap();

    assert_eq!(
        first.version().topology_hash(),
        second.version().topology_hash()
    );
    assert_ne!(
        first.version().semantic_hash(),
        second.version().semantic_hash()
    );
}

#[test]
fn unregistered_nested_free_form_values_fail_without_echo() {
    let mut version = version("software-feature.yaml").to_record();
    version
        .graph
        .spec
        .nodes
        .get_mut("plan")
        .unwrap()
        .properties
        .insert(
            "model".into(),
            serde_json::json!({"nested": {"prompt": "NESTED-PROMPT-CANARY"}}),
        );
    version.semantic = graphhelm_graph::canonicalize(&version.graph).unwrap().value;
    version.content_hash = graphhelm_graph::semantic_hash(&version.graph).unwrap();

    let error =
        block_on(externalizer().prepare(scope(&version.graph.metadata.execution_id), &version))
            .unwrap_err();
    assert_eq!(error.code(), "GHE009_EXTERNALIZATION_FAILED");
    assert!(!error.to_string().contains("NESTED-PROMPT-CANARY"));
    assert!(!format!("{error:?}").contains("NESTED-PROMPT-CANARY"));
}

#[test]
fn secret_shaped_values_across_durable_surfaces_fail_before_sealing_without_echo() {
    let label_canaries = vec![
        format!("GhP_{}", "A".repeat(36)),
        format!("sK_pRoJ_{}", "A".repeat(32)),
        format!("AKIA{}", "A".repeat(16)),
        format!("aSiA{}", "A".repeat(16)),
        format!("glpat-{}", "A".repeat(20)),
        format!(
            "xoxb-{}-{}-{}",
            "1".repeat(12),
            "2".repeat(12),
            "A".repeat(40)
        ),
        format!(
            "{}.{}.{}",
            "eyJhbGciOiJIUzI1NiJ9",
            "eyJzdWIiOiJkdXJhYmxlIn0",
            "A".repeat(24)
        ),
        format!("AUTHORIZATION_BEARER_{}", "A".repeat(24)),
    ];
    for canary in &label_canaries {
        let mut version = version("software-feature.yaml").to_record();
        version
            .graph
            .metadata
            .labels
            .insert("release-channel".into(), canary.clone());
        refresh_record(&mut version);
        assert_rejected_before_sealing(
            &version,
            scope(&version.graph.metadata.execution_id),
            canary,
        );
    }

    let authoring_canaries = vec![
        format!(
            "{}{}{}",
            "-----BEGIN PRIVATE KEY-----",
            "A".repeat(8),
            "-----END PRIVATE KEY-----"
        ),
        format!("{}{}", "-----BeGiN_RsA-PrIvAtE_KeY-----", "A".repeat(8)),
        "secret://production/runtime-token".to_owned(),
        "EnViRoNmEnT://production/AWS_SECRET_ACCESS_KEY".to_owned(),
        format!("Authorization: Basic {}", "A".repeat(24)),
    ];
    for canary in &authoring_canaries {
        let mut version = version("software-feature.yaml").to_record();
        version.graph.spec.nodes.get_mut("plan").unwrap().objective = canary.clone();
        refresh_record(&mut version);
        assert_rejected_before_sealing(
            &version,
            scope(&version.graph.metadata.execution_id),
            canary,
        );
    }

    let binding_canary = format!(
        "xoxp-{}-{}-{}",
        "1".repeat(12),
        "2".repeat(12),
        "A".repeat(40)
    );
    let mut binding_record = version("software-feature.yaml").to_record();
    binding_record.graph.spec.edges[0]
        .bindings
        .insert("channel".into(), binding_canary.clone());
    refresh_record(&mut binding_record);
    assert_rejected_before_sealing(
        &binding_record,
        scope(&binding_record.graph.metadata.execution_id),
        &binding_canary,
    );

    let nested_canary = format!("github_pat_{}", "A".repeat(76));
    let mut nested_record = version("software-feature.yaml").to_record();
    nested_record
        .graph
        .spec
        .nodes
        .get_mut("plan")
        .unwrap()
        .properties
        .insert(
            "ui".into(),
            serde_json::json!({"nested": {"value": &nested_canary}}),
        );
    refresh_record(&mut nested_record);
    assert_rejected_before_sealing(
        &nested_record,
        scope(&nested_record.graph.metadata.execution_id),
        &nested_canary,
    );
}

#[test]
fn secret_shaped_structural_and_scope_identifiers_fail_before_sealing_without_echo() {
    let graph_id_canary = format!("ghp_{}", "A".repeat(36));
    let mut graph_id_record = version("software-feature.yaml").to_record();
    graph_id_record.graph.metadata.id = graph_id_canary.clone();
    refresh_record(&mut graph_id_record);
    assert_rejected_before_sealing(
        &graph_id_record,
        scope(&graph_id_record.graph.metadata.execution_id),
        &graph_id_canary,
    );

    let actor_canary = format!("sk-{}", "A".repeat(32));
    let mut actor_record = version("software-feature.yaml").to_record();
    actor_record.created_by = Actor::new(ActorType::Owner, actor_canary.clone());
    assert_rejected_before_sealing(
        &actor_record,
        scope(&actor_record.graph.metadata.execution_id),
        &actor_canary,
    );

    let scope_canary = format!("glpat-{}", "A".repeat(20));
    let scope_record = version("software-feature.yaml").to_record();
    let unsafe_scope = RepositoryScope::new(
        WorkspaceId::parse(scope_canary.clone()).unwrap(),
        ProjectId::parse("project-test").unwrap(),
        Some(ExecutionId::parse(&scope_record.graph.metadata.execution_id).unwrap()),
    );
    assert_rejected_before_sealing(&scope_record, unsafe_scope, &scope_canary);
}

#[test]
fn annotation_names_outside_registered_schema_fragments_fail_without_echo() {
    let cases = [
        "description",
        "title",
        "examples",
        "source",
        "$comment",
        "Title",
        "source_path",
        "$Comment",
    ];

    for key in cases {
        let mut version = version("software-feature.yaml").to_record();
        let ephemeral = version
            .graph
            .spec
            .nodes
            .get_mut("plan")
            .unwrap()
            .properties
            .get_mut("agent")
            .unwrap()["ephemeral"]
            .as_object_mut()
            .unwrap();
        ephemeral.insert(
            key.into(),
            serde_json::json!({"nested": "ANNOTATION-CANARY"}),
        );
        refresh_record(&mut version);

        let error =
            block_on(externalizer().prepare(scope(&version.graph.metadata.execution_id), &version))
                .unwrap_err();

        assert_eq!(error.code(), "GHE009_EXTERNALIZATION_FAILED");
        assert!(!error.to_string().contains("ANNOTATION-CANARY"));
        assert!(!format!("{error:?}").contains("ANNOTATION-CANARY"));
    }
}

#[test]
fn registered_content_and_schema_annotations_never_reach_safe_surfaces() {
    let mut version = version("software-feature.yaml").to_record();
    version.graph.metadata.name = "GRAPH-NAME-CANARY".into();
    version.graph.metadata.properties.insert(
        "sourcePath".into(),
        serde_json::json!("C:\\Users\\canary\\graph.yaml"),
    );
    version.graph.spec.policies.push(serde_json::json!({
        "inlineConstraint": {
            "deny": ["production.deploy"],
            "reason": "registered_policy_reason",
            "ruleText": "POLICY-TEXT-CANARY"
        }
    }));
    let node = version.graph.spec.nodes.get_mut("plan").unwrap();
    node.objective = "NODE-OBJECTIVE-CANARY".into();
    node.properties.insert(
        "completion".into(),
        serde_json::json!({"requires": [{"expression": "NODE-COMPLETION-CANARY"}]}),
    );
    node.properties.insert(
        "input".into(),
        serde_json::json!({
            "schema": "schema://INPUT-SCHEMA-CANARY@1",
            "title": "SCHEMA-TITLE-CANARY",
            "description": "SCHEMA-DESCRIPTION-CANARY",
            "examples": ["SCHEMA-EXAMPLE-CANARY"]
        }),
    );
    let ephemeral = node.properties.get_mut("agent").unwrap()["ephemeral"]
        .as_object_mut()
        .unwrap();
    ephemeral.insert("purpose".into(), serde_json::json!("AGENT-PURPOSE-CANARY"));
    ephemeral.insert(
        "instructions".into(),
        serde_json::json!("AGENT-INSTRUCTIONS-CANARY"),
    );
    ephemeral.insert(
        "completionContract".into(),
        serde_json::json!({"requires": ["AGENT-COMPLETION-CANARY"]}),
    );
    version.semantic = graphhelm_graph::canonicalize(&version.graph).unwrap().value;
    version.content_hash = graphhelm_graph::semantic_hash(&version.graph).unwrap();

    let prepared =
        block_on(externalizer().prepare(scope(&version.graph.metadata.execution_id), &version))
            .unwrap();
    let version_bytes = serde_json::to_vec(prepared.version()).unwrap();
    let debug = format!("{prepared:?}");
    let canaries = [
        "GRAPH-NAME-CANARY",
        "C:\\Users\\canary\\graph.yaml",
        "POLICY-TEXT-CANARY",
        "NODE-OBJECTIVE-CANARY",
        "NODE-COMPLETION-CANARY",
        "schema://INPUT-SCHEMA-CANARY@1",
        "SCHEMA-TITLE-CANARY",
        "SCHEMA-DESCRIPTION-CANARY",
        "SCHEMA-EXAMPLE-CANARY",
        "AGENT-PURPOSE-CANARY",
        "AGENT-INSTRUCTIONS-CANARY",
        "AGENT-COMPLETION-CANARY",
    ];
    for canary in canaries {
        assert!(
            !version_bytes
                .windows(canary.len())
                .any(|window| window == canary.as_bytes())
        );
        assert!(!debug.contains(canary));
        assert!(prepared.evidence().iter().all(|evidence| {
            !evidence
                .ciphertext()
                .windows(canary.len())
                .any(|window| window == canary.as_bytes())
        }));
    }
}

#[test]
fn ui_source_and_removable_schema_annotations_do_not_change_safe_identity() {
    let original = version("software-feature.yaml").to_record();
    let mut annotated = original.clone();
    annotated.graph.metadata.properties.insert(
        "sourcePath".into(),
        serde_json::json!("C:\\Users\\canary\\graph.yaml"),
    );
    let node = annotated.graph.spec.nodes.get_mut("tests").unwrap();
    node.properties
        .insert("ui".into(), serde_json::json!({"x": 999, "y": -42}));
    node.properties.insert(
        "input".into(),
        serde_json::json!({
            "schema": "schema://ImplementationResult@1",
            "$comment": "ignored input comment",
            "title": "ignored title",
            "description": "ignored description",
            "examples": ["ignored example"]
        }),
    );
    node.properties.insert(
        "output".into(),
        serde_json::json!({
            "schema": "schema://TestReport@1",
            "$comment": "ignored output comment",
            "title": "ignored output title",
            "description": "ignored output description",
            "examples": ["ignored output example"]
        }),
    );
    annotated.semantic = graphhelm_graph::canonicalize(&annotated.graph)
        .unwrap()
        .value;
    annotated.content_hash = graphhelm_graph::semantic_hash(&annotated.graph).unwrap();

    let first =
        block_on(externalizer().prepare(scope(&original.graph.metadata.execution_id), &original))
            .unwrap();
    let second =
        block_on(externalizer().prepare(scope(&annotated.graph.metadata.execution_id), &annotated))
            .unwrap();

    assert_eq!(first.version(), second.version());
}

#[test]
fn schema_like_annotations_in_registered_content_are_externalized_not_silently_stripped() {
    let original = version("software-feature.yaml").to_record();
    let mut annotated = original.clone();
    annotated
        .graph
        .spec
        .nodes
        .get_mut("plan")
        .unwrap()
        .properties
        .get_mut("agent")
        .unwrap()["ephemeral"]["completionContract"]["$comment"] =
        serde_json::json!("COMPLETION-ANNOTATION-CANARY");
    refresh_record(&mut annotated);

    let first =
        block_on(externalizer().prepare(scope(&original.graph.metadata.execution_id), &original))
            .unwrap();
    let second =
        block_on(externalizer().prepare(scope(&annotated.graph.metadata.execution_id), &annotated))
            .unwrap();

    assert_eq!(
        first.version().topology_hash(),
        second.version().topology_hash()
    );
    assert_ne!(
        first.version().semantic_hash(),
        second.version().semantic_hash()
    );
    assert!(
        !serde_json::to_string(second.version())
            .unwrap()
            .contains("COMPLETION-ANNOTATION-CANARY")
    );
}

#[test]
fn payload_schema_reference_remains_reconstructible_not_an_annotation_context() {
    let original = version("software-feature.yaml").to_record();
    let mut payload_bound = original.clone();
    payload_bound.graph.spec.edges[0].payload_schema = Some("schema://DurablePayload@1".into());
    refresh_record(&mut payload_bound);

    let first =
        block_on(externalizer().prepare(scope(&original.graph.metadata.execution_id), &original))
            .unwrap();
    let second = block_on(externalizer().prepare(
        scope(&payload_bound.graph.metadata.execution_id),
        &payload_bound,
    ))
    .unwrap();

    assert_ne!(
        first.version().topology_hash(),
        second.version().topology_hash()
    );
    assert_ne!(
        first.version().semantic_hash(),
        second.version().semantic_hash()
    );
    assert!(
        !serde_json::to_string(second.version())
            .unwrap()
            .contains("schema://DurablePayload@1")
    );
    let condition = second.version().topology().edges()[0].condition().unwrap();
    assert_eq!(
        decode_reference(identifier(condition, "schema")),
        "schema://DurablePayload@1"
    );
    assert!(condition.digests().is_empty());
}

#[test]
fn content_scan_depth_limit_fails_before_sealing() {
    let mut version = version("software-feature.yaml").to_record();
    let mut nested = serde_json::json!("leaf");
    for _ in 0..70 {
        nested = serde_json::json!({"nested": nested});
    }
    version.graph.spec.completion = serde_json::json!({"requires": nested});
    refresh_record(&mut version);
    let calls = Arc::new(AtomicUsize::new(0));
    let externalizer = SealingGraphExternalizer::new(CountingFailureSealer(Arc::clone(&calls)));

    let error =
        block_on(externalizer.prepare(scope(&version.graph.metadata.execution_id), &version))
            .unwrap_err();

    assert_eq!(error.code(), "GHE006_LIMIT_EXCEEDED");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn authoring_value_preflight_runs_before_graph_version_reconstruction() {
    let mut record = version("software-feature.yaml").to_record();
    let mut nested = serde_json::json!("leaf");
    for _ in 0..70 {
        nested = serde_json::json!({"nested": nested});
    }
    record.graph.spec.completion = serde_json::json!({"requires": nested});
    let calls = Arc::new(AtomicUsize::new(0));
    let externalizer = SealingGraphExternalizer::new(CountingFailureSealer(Arc::clone(&calls)));

    let error = block_on(externalizer.prepare(scope(&record.graph.metadata.execution_id), &record))
        .unwrap_err();

    assert_eq!(error.code(), "GHE006_LIMIT_EXCEEDED");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn semantic_value_preflight_runs_before_graph_version_reconstruction() {
    let cases = [
        {
            let mut nested = serde_json::json!(null);
            for _ in 0..=64 {
                nested = serde_json::json!([nested]);
            }
            nested
        },
        serde_json::Value::Array((0..131_072).map(|_| serde_json::Value::Null).collect()),
        serde_json::Value::String("x".repeat(64 * 1024 * 1024 + 1)),
    ];

    for semantic in cases {
        let mut record = version("software-feature.yaml").to_record();
        record.semantic = semantic;
        let calls = Arc::new(AtomicUsize::new(0));
        let externalizer = SealingGraphExternalizer::new(CountingFailureSealer(Arc::clone(&calls)));

        let error =
            block_on(externalizer.prepare(scope(&record.graph.metadata.execution_id), &record))
                .unwrap_err();

        assert_eq!(error.code(), "GHE006_LIMIT_EXCEEDED");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn authoring_rejects_reference_families_outside_the_binding_domain_before_sealing() {
    for reference in [
        "schema://Input@1",
        "policy://project/security@1",
        "builtin/repository.read@1",
        "deploy://docker-compose@1",
        "evaluator://quality@1",
        "project/security-reviewer@3",
        "rules://classifier@1",
        "graph-template://release@1",
        "contract://completion@1",
        "document://architecture/auth.md",
    ] {
        for position in ["input", "output", "edge"] {
            let mut record = version("software-feature.yaml").to_record();
            if position == "edge" {
                record.graph.spec.edges[0]
                    .bindings
                    .insert("request".into(), reference.into());
            } else {
                record
                    .graph
                    .spec
                    .nodes
                    .get_mut("plan")
                    .unwrap()
                    .properties
                    .insert(
                        position.into(),
                        serde_json::json!({"bindings": {"request": reference}}),
                    );
            }
            refresh_record(&mut record);
            let calls = Arc::new(AtomicUsize::new(0));
            let externalizer =
                SealingGraphExternalizer::new(CountingFailureSealer(Arc::clone(&calls)));

            let error =
                block_on(externalizer.prepare(scope(&record.graph.metadata.execution_id), &record))
                    .unwrap_err();

            assert_eq!(
                error.code(),
                "GHE009_EXTERNALIZATION_FAILED",
                "{position}: {reference}"
            );
            assert_eq!(calls.load(Ordering::SeqCst), 0, "{position}: {reference}");
        }
    }
}

#[test]
fn replay_applies_the_binding_domain_to_input_output_and_edge_positions() {
    let mut record = registered_controls_record();
    let plan = record.graph.spec.nodes.get_mut("plan").unwrap();
    plan.properties.get_mut("input").unwrap()["bindings"]["request"] =
        serde_json::json!("context://task/request");
    plan.properties.get_mut("output").unwrap()["bindings"]["plan"] =
        serde_json::json!("artifact://implementation.diff");
    refresh_record(&mut record);
    let prepared =
        block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
            .unwrap();
    let baseline = serde_json::to_value(prepared.version()).unwrap();
    let invalid = encode_persisted_reference("schema://Input@1").unwrap();

    for position in ["input", "output", "edge"] {
        let mut wire = baseline.clone();
        if position == "edge" {
            wire["topology"]["edges"][0]["bindings"]["request"] =
                serde_json::json!(invalid.as_str());
        } else {
            let control_type = format!("{position}_contract");
            let control = wire["topology"]["nodes"]["plan"]["controls"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|control| control["controlType"] == control_type)
                .unwrap();
            control["identifiers"]["bindingValue.000"] = serde_json::json!(invalid.as_str());
        }
        assert!(
            validate_persisted_projection(&rehashed_projection(wire)).is_err(),
            "{position}"
        );
    }
}

#[test]
fn authoring_and_replay_close_both_isolation_tier_positions() {
    for tier in ["tier_0", "tier_1", "tier_2", "tier_3"] {
        let mut record = registered_controls_record();
        record
            .graph
            .spec
            .nodes
            .get_mut("plan")
            .unwrap()
            .properties
            .get_mut("isolation")
            .unwrap()["minimum"] = serde_json::json!(tier);
        record
            .graph
            .spec
            .nodes
            .get_mut("plan")
            .unwrap()
            .properties
            .get_mut("agent")
            .unwrap()["ephemeral"]["isolationMinimum"] = serde_json::json!(tier);
        refresh_record(&mut record);
        let prepared =
            block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
                .unwrap();
        validate_persisted_projection(prepared.version()).unwrap();
    }

    for (control_type, key) in [
        ("node_isolation", "minimum"),
        ("agent_configuration", "isolation"),
    ] {
        let mut record = registered_controls_record();
        if control_type == "node_isolation" {
            record
                .graph
                .spec
                .nodes
                .get_mut("plan")
                .unwrap()
                .properties
                .get_mut("isolation")
                .unwrap()["minimum"] = serde_json::json!("tier_4");
        } else {
            record
                .graph
                .spec
                .nodes
                .get_mut("plan")
                .unwrap()
                .properties
                .get_mut("agent")
                .unwrap()["ephemeral"]["isolationMinimum"] = serde_json::json!("TIER_2");
        }
        refresh_record(&mut record);
        let calls = Arc::new(AtomicUsize::new(0));
        let externalizer = SealingGraphExternalizer::new(CountingFailureSealer(Arc::clone(&calls)));
        let error =
            block_on(externalizer.prepare(scope(&record.graph.metadata.execution_id), &record))
                .unwrap_err();
        assert_eq!(
            error.code(),
            "GHE009_EXTERNALIZATION_FAILED",
            "{control_type}.{key}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    let record = registered_controls_record();
    let prepared =
        block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
            .unwrap();
    let baseline = serde_json::to_value(prepared.version()).unwrap();
    for (control_type, key, invalid) in [
        ("node_isolation", "minimum", "tier_4"),
        ("agent_configuration", "isolation", "TIER_2"),
    ] {
        let mut wire = baseline.clone();
        let controls = wire["topology"]["nodes"]["plan"]["controls"]
            .as_array_mut()
            .unwrap();
        let control = controls
            .iter_mut()
            .find(|control| control["controlType"] == control_type)
            .unwrap();
        control["identifiers"][key] = serde_json::json!(invalid);
        assert!(
            validate_persisted_projection(&rehashed_projection(wire)).is_err(),
            "{control_type}.{key}"
        );
    }
}

#[test]
fn structural_jws_header_is_rejected_before_the_first_sealer_call() {
    let mut record = version("software-feature.yaml").to_record();
    record.graph.spec.nodes.get_mut("plan").unwrap().objective =
        "IHsiYWxnIjoiSFMyNTYiLCJ0eXAiOiJKV1QifQ.cGF5bG9hZA.c2lnbmF0dXJl".into();
    refresh_record(&mut record);
    let calls = Arc::new(AtomicUsize::new(0));
    let externalizer = SealingGraphExternalizer::new(CountingFailureSealer(Arc::clone(&calls)));

    let error = block_on(externalizer.prepare(scope(&record.graph.metadata.execution_id), &record))
        .unwrap_err();

    assert_eq!(error.code(), "GHE009_EXTERNALIZATION_FAILED");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn governor_derives_sorted_sink_terminals_when_completion_omits_them() {
    let mut record = version("software-feature.yaml").to_record();
    record
        .graph
        .spec
        .completion
        .as_object_mut()
        .unwrap()
        .remove("terminalNodes");
    refresh_record(&mut record);

    let prepared =
        block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
            .unwrap();
    let completion = prepared.version().topology().completion();
    let terminals = completion
        .identifiers()
        .iter()
        .filter(|(key, _)| key.as_str().starts_with("terminal."))
        .map(|(_, value)| value.as_str())
        .collect::<Vec<_>>();

    assert!(!terminals.is_empty());
    assert!(terminals.windows(2).all(|pair| pair[0] < pair[1]));

    let mut explicit = record.clone();
    explicit.graph.spec.completion["terminalNodes"] = serde_json::json!(terminals);
    refresh_record(&mut explicit);
    let explicit =
        block_on(externalizer().prepare(scope(&explicit.graph.metadata.execution_id), &explicit))
            .unwrap();
    assert_eq!(
        prepared.version().topology_hash(),
        explicit.version().topology_hash()
    );
    assert_eq!(
        prepared.version().semantic_hash(),
        explicit.version().semantic_hash()
    );
}

#[test]
fn inline_contract_schema_is_retained_only_as_a_structural_digest() {
    let mut first = registered_controls_record();
    first
        .graph
        .spec
        .nodes
        .get_mut("plan")
        .unwrap()
        .properties
        .get_mut("input")
        .unwrap()["schema"] = serde_json::json!({
        "type": "object",
        "required": ["request"],
        "properties": {"request": {"type": "string", "description": "removed"}},
        "title": "removed"
    });
    refresh_record(&mut first);
    let mut second = first.clone();
    second
        .graph
        .spec
        .nodes
        .get_mut("plan")
        .unwrap()
        .properties
        .get_mut("input")
        .unwrap()["schema"]["title"] = serde_json::json!("also removed");
    refresh_record(&mut second);

    let first = block_on(externalizer().prepare(scope(&first.graph.metadata.execution_id), &first))
        .unwrap();
    let second =
        block_on(externalizer().prepare(scope(&second.graph.metadata.execution_id), &second))
            .unwrap();
    let first_contract = control(&first, "plan", "input_contract");
    let second_contract = control(&second, "plan", "input_contract");
    assert!(
        !first_contract
            .identifiers()
            .keys()
            .any(|key| key.as_str() == "schema")
    );
    assert_eq!(first_contract.digests(), second_contract.digests());
    assert_eq!(first_contract.digests().len(), 1);

    let mut changed = registered_controls_record();
    changed
        .graph
        .spec
        .nodes
        .get_mut("plan")
        .unwrap()
        .properties
        .get_mut("input")
        .unwrap()["schema"] = serde_json::json!({"type":"object","required":["different"]});
    refresh_record(&mut changed);
    let changed =
        block_on(externalizer().prepare(scope(&changed.graph.metadata.execution_id), &changed))
            .unwrap();
    assert_ne!(
        first_contract.digests(),
        control(&changed, "plan", "input_contract").digests()
    );
}

#[test]
fn annotation_only_and_unsafe_inline_schemas_fail_before_sealing_without_echo() {
    for schema in [
        serde_json::json!({"title":"annotation only","description":"removed"}),
        serde_json::json!({"type":"string","const":"secret://placeholder-value"}),
    ] {
        let mut record = registered_controls_record();
        record
            .graph
            .spec
            .nodes
            .get_mut("plan")
            .unwrap()
            .properties
            .get_mut("input")
            .unwrap()["schema"] = schema;
        refresh_record(&mut record);
        let calls = Arc::new(AtomicUsize::new(0));
        let externalizer = SealingGraphExternalizer::new(CountingFailureSealer(Arc::clone(&calls)));
        let error =
            block_on(externalizer.prepare(scope(&record.graph.metadata.execution_id), &record))
                .unwrap_err();
        assert_eq!(error.code(), "GHE009_EXTERNALIZATION_FAILED");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(!format!("{error:?} {error}").contains("placeholder-value"));
    }
}

#[test]
fn permission_qualifiers_without_capability_fail_before_sealing() {
    let mut record = registered_controls_record();
    record
        .graph
        .spec
        .nodes
        .get_mut("plan")
        .unwrap()
        .properties
        .insert(
            "permissions".into(),
            serde_json::json!([{"duration":"call","scope":{"allowlist":["api.example.com"]}}]),
        );
    refresh_record(&mut record);
    let calls = Arc::new(AtomicUsize::new(0));
    let externalizer = SealingGraphExternalizer::new(CountingFailureSealer(Arc::clone(&calls)));
    let error = block_on(externalizer.prepare(scope(&record.graph.metadata.execution_id), &record))
        .unwrap_err();
    assert!(matches!(
        error.code(),
        "GHE009_EXTERNALIZATION_FAILED" | "GHE005_INTEGRITY_FAILURE"
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn globally_valid_reference_in_the_wrong_registered_position_fails_before_sealing() {
    let mut record = registered_controls_record();
    record
        .graph
        .spec
        .nodes
        .get_mut("plan")
        .unwrap()
        .properties
        .get_mut("input")
        .unwrap()["schema"] = serde_json::json!("policy://workspace/security-baseline@2");
    refresh_record(&mut record);
    let calls = Arc::new(AtomicUsize::new(0));
    let externalizer = SealingGraphExternalizer::new(CountingFailureSealer(Arc::clone(&calls)));
    let error = block_on(externalizer.prepare(scope(&record.graph.metadata.execution_id), &record))
        .unwrap_err();
    assert!(matches!(
        error.code(),
        "GHE009_EXTERNALIZATION_FAILED" | "GHE005_INTEGRITY_FAILURE"
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn invalid_deploy_compensation_fails_before_sealing() {
    for target in [None, Some("deploy"), Some("missing-node"), Some("agent")] {
        let mut record = node_kind_registry_record();
        let effects = record
            .graph
            .spec
            .nodes
            .get_mut("deploy")
            .unwrap()
            .properties
            .get_mut("effects")
            .unwrap()
            .as_object_mut()
            .unwrap();
        match target {
            Some(target) => {
                effects.insert("compensationNode".into(), serde_json::json!(target));
            }
            None => {
                effects.remove("compensationNode");
            }
        }
        refresh_record(&mut record);
        let calls = Arc::new(AtomicUsize::new(0));
        let externalizer = SealingGraphExternalizer::new(CountingFailureSealer(Arc::clone(&calls)));
        let error =
            block_on(externalizer.prepare(scope(&record.graph.metadata.execution_id), &record))
                .unwrap_err();
        assert!(matches!(
            error.code(),
            "GHE009_EXTERNALIZATION_FAILED" | "GHE005_INTEGRITY_FAILURE"
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    let valid = node_kind_registry_record();
    block_on(externalizer().prepare(scope(&valid.graph.metadata.execution_id), &valid)).unwrap();
}

#[test]
fn exceeded_authoring_budgets_fail_before_sealing() {
    for budget in ["nodes", "depth", "retries"] {
        let mut record = node_kind_registry_record();
        match budget {
            "nodes" => record.graph.spec.budgets.max_nodes = Some(1),
            "depth" => record.graph.spec.budgets.max_depth = Some(1),
            "retries" => {
                record.graph.spec.budgets.max_retries_per_node = Some(0);
                record
                    .graph
                    .spec
                    .nodes
                    .get_mut("agent")
                    .unwrap()
                    .properties
                    .insert("retry".into(), serde_json::json!({"maxAttempts":2}));
            }
            _ => unreachable!(),
        }
        refresh_record(&mut record);
        let calls = Arc::new(AtomicUsize::new(0));
        let externalizer = SealingGraphExternalizer::new(CountingFailureSealer(Arc::clone(&calls)));
        let error =
            block_on(externalizer.prepare(scope(&record.graph.metadata.execution_id), &record))
                .unwrap_err();
        assert!(
            matches!(
                error.code(),
                "GHE009_EXTERNALIZATION_FAILED" | "GHE005_INTEGRITY_FAILURE"
            ),
            "{budget}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0, "{budget}");
    }
}

struct CountingFailureSealer(Arc<AtomicUsize>);

impl EvidenceSealer for CountingFailureSealer {
    fn seal<'a>(
        &'a self,
        _scope: RepositoryScope,
        _input: EvidenceInput,
    ) -> RepositoryFuture<'a, Result<SealedEvidence, EvidenceError>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Err(EvidenceError::SealingFailed) })
    }
}

#[test]
fn content_limit_is_rejected_before_the_first_sealer_call() {
    let mut version = version("software-feature.yaml").to_record();
    version.graph.spec.nodes.get_mut("plan").unwrap().objective = "x".repeat(16 * 1024 * 1024 + 1);
    version.semantic = graphhelm_graph::canonicalize(&version.graph).unwrap().value;
    version.content_hash = graphhelm_graph::semantic_hash(&version.graph).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let sealer = CountingFailureSealer(Arc::clone(&calls));
    let externalizer = SealingGraphExternalizer::new(sealer);

    let error =
        block_on(externalizer.prepare(scope(&version.graph.metadata.execution_id), &version))
            .unwrap_err();

    assert_eq!(error.code(), "GHE006_LIMIT_EXCEEDED");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

struct CorruptingSealer(EvidenceProtector<FixedKeyProvider>);

impl EvidenceSealer for CorruptingSealer {
    fn seal<'a>(
        &'a self,
        scope: RepositoryScope,
        input: EvidenceInput,
    ) -> RepositoryFuture<'a, Result<SealedEvidence, EvidenceError>> {
        Box::pin(async move {
            let sealed = self.0.seal(scope.clone(), input).await?;
            let reference = EvidenceReference::new(
                EvidenceId::parse("wrong-evidence-id").unwrap(),
                sealed.reference().content_sha256().clone(),
                sealed.reference().ciphertext_sha256().clone(),
            );
            SealedEvidence::new(
                reference,
                scope,
                sealed.media_type().as_str(),
                sealed.sensitivity(),
                sealed.retention_class(),
                sealed.algorithm(),
                sealed.nonce().to_vec(),
                sealed.ciphertext().to_vec(),
                sealed.wrapped_key().clone(),
            )
        })
    }
}

#[derive(Clone, Copy, Debug)]
enum SealedFault {
    EvidenceId,
    Scope,
    MediaType,
    Sensitivity,
    RetentionClass,
    PlaintextLength,
    ContentDigest,
    CiphertextDigest,
    AadDigest,
    KeyHandle,
}

struct FaultInjectingSealer {
    inner: EvidenceProtector<FixedKeyProvider>,
    fault: SealedFault,
}

impl EvidenceSealer for FaultInjectingSealer {
    fn seal<'a>(
        &'a self,
        scope: RepositoryScope,
        input: EvidenceInput,
    ) -> RepositoryFuture<'a, Result<SealedEvidence, EvidenceError>> {
        Box::pin(async move {
            let sealed = self.inner.seal(scope, input).await?;
            let mut reference = sealed.reference().clone();
            let mut returned_scope = sealed.scope().clone();
            let mut media_type = sealed.media_type().as_str();
            let mut sensitivity = sealed.sensitivity();
            let mut retention_class = sealed.retention_class();
            let mut ciphertext = sealed.ciphertext().to_vec();
            let mut wrapped_key = sealed.wrapped_key().clone();

            match self.fault {
                SealedFault::EvidenceId => {
                    reference = EvidenceReference::new(
                        EvidenceId::parse("wrong-evidence-id").unwrap(),
                        reference.content_sha256().clone(),
                        reference.ciphertext_sha256().clone(),
                    );
                }
                SealedFault::Scope => {
                    returned_scope = RepositoryScope::new(
                        WorkspaceId::parse("workspace-foreign").unwrap(),
                        ProjectId::parse("project-foreign").unwrap(),
                        returned_scope.execution_id().cloned(),
                    );
                }
                SealedFault::MediaType => media_type = "text/plain",
                SealedFault::Sensitivity => sensitivity = Sensitivity::Public,
                SealedFault::RetentionClass => retention_class = "ephemeral",
                SealedFault::PlaintextLength => {
                    ciphertext.push(0);
                    reference = EvidenceReference::new(
                        reference.evidence_id().clone(),
                        reference.content_sha256().clone(),
                        raw_content_sha256(&ciphertext).unwrap(),
                    );
                }
                SealedFault::ContentDigest => {
                    reference = EvidenceReference::new(
                        reference.evidence_id().clone(),
                        RawSha256::parse("1".repeat(64)).unwrap(),
                        reference.ciphertext_sha256().clone(),
                    );
                }
                SealedFault::CiphertextDigest => {
                    reference = EvidenceReference::new(
                        reference.evidence_id().clone(),
                        reference.content_sha256().clone(),
                        RawSha256::parse("2".repeat(64)).unwrap(),
                    );
                }
                SealedFault::AadDigest => {
                    wrapped_key = graphhelm_events::WrappedKey::new(
                        wrapped_key.key_id(),
                        wrapped_key.handle(),
                        wrapped_key.algorithm(),
                        wrapped_key.nonce().to_vec(),
                        wrapped_key.ciphertext().to_vec(),
                        RawSha256::parse("3".repeat(64)).unwrap(),
                    )
                    .unwrap();
                }
                SealedFault::KeyHandle => {
                    wrapped_key = graphhelm_events::WrappedKey::new(
                        wrapped_key.key_id(),
                        "wrong-key-handle",
                        wrapped_key.algorithm(),
                        wrapped_key.nonce().to_vec(),
                        wrapped_key.ciphertext().to_vec(),
                        wrapped_key.aad_sha256().clone(),
                    )
                    .unwrap();
                }
            }

            SealedEvidence::new(
                reference,
                returned_scope,
                media_type,
                sensitivity,
                retention_class,
                sealed.algorithm(),
                sealed.nonce().to_vec(),
                ciphertext,
                wrapped_key,
            )
        })
    }
}

#[test]
fn every_sealer_output_field_is_validated_before_preparation_succeeds() {
    let record = version("software-feature.yaml").to_record();
    for fault in [
        SealedFault::EvidenceId,
        SealedFault::Scope,
        SealedFault::MediaType,
        SealedFault::Sensitivity,
        SealedFault::RetentionClass,
        SealedFault::PlaintextLength,
        SealedFault::ContentDigest,
        SealedFault::CiphertextDigest,
        SealedFault::AadDigest,
        SealedFault::KeyHandle,
    ] {
        let externalizer = SealingGraphExternalizer::new(FaultInjectingSealer {
            inner: EvidenceProtector::new(FixedKeyProvider),
            fault,
        });

        let error =
            block_on(externalizer.prepare(scope(&record.graph.metadata.execution_id), &record))
                .unwrap_err();

        assert_eq!(error.code(), "GHE005_INTEGRITY_FAILURE", "{fault:?}");
        let public = format!("{error:?} {error}");
        assert!(!public.contains("wrong-key-handle"));
        assert!(!public.contains("wrong-evidence-id"));
        assert!(!public.contains("workspace-foreign"));
    }
}

#[test]
fn maximum_predecessor_version_is_rejected_by_externalizer_without_panicking() {
    let mut record = version("software-feature.yaml").to_record();
    record.predecessor = Some(GraphVersionRef {
        number: u64::MAX,
        content_hash: record.content_hash.clone(),
    });

    let error =
        block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
            .unwrap_err();

    assert_eq!(error.code(), "GHE005_INTEGRITY_FAILURE");
}

#[test]
fn projection_reference_failure_leaves_the_base_candidate_unchanged() {
    let base = version("software-feature.yaml");
    let before = serde_json::to_vec(base.graph()).unwrap();
    let draft = GraphDraft {
        id: "draft-corrupt-reference".into(),
        expected_version: base.number(),
        expected_hash: base.content_hash().clone(),
        operations: Vec::new(),
        manual_override: None,
    };
    let externalizer =
        SealingGraphExternalizer::new(CorruptingSealer(EvidenceProtector::new(FixedKeyProvider)));
    let services = PublicationPreparationServices {
        scope: scope(&base.graph().metadata.execution_id),
        actor: Actor::new(ActorType::Owner, "owner-test"),
        clock: &FixedClock,
        externalizer: &externalizer,
    };

    let error = block_on(prepare_draft_publication(&base, &draft, &services)).unwrap_err();

    assert_eq!(error.code(), "GHE005_INTEGRITY_FAILURE");
    assert_eq!(serde_json::to_vec(base.graph()).unwrap(), before);
}

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 9, 13, 0, 0).unwrap()
    }
}

#[test]
fn governor_prepares_an_isolated_candidate_without_publishing_state() {
    let base = version("software-feature.yaml");
    let before = serde_json::to_vec(base.graph()).unwrap();
    let draft = GraphDraft {
        id: "draft-safe-preparation".into(),
        expected_version: base.number(),
        expected_hash: base.content_hash().clone(),
        operations: Vec::new(),
        manual_override: None,
    };
    let externalizer = externalizer();
    let base_projection = block_on(externalizer.prepare(
        scope(&base.graph().metadata.execution_id),
        &base.to_record(),
    ))
    .unwrap();
    let actor = Actor::new(ActorType::Owner, "owner-test");
    let scope = scope(&base.graph().metadata.execution_id);
    let services = PublicationPreparationServices {
        scope,
        actor,
        clock: &FixedClock,
        externalizer: &externalizer,
    };

    let prepared = block_on(prepare_draft_publication(&base, &draft, &services)).unwrap();

    assert_eq!(prepared.version().number(), base.number() + 1);
    assert_eq!(
        prepared.version().predecessor().unwrap().number(),
        base.number()
    );
    assert_eq!(
        prepared.version().predecessor().unwrap().semantic_hash(),
        base_projection.version().semantic_hash()
    );
    assert_eq!(serde_json::to_vec(base.graph()).unwrap(), before);
}

fn publication_services<'a>(
    base: &GraphVersion,
    externalizer: &'a dyn GraphExternalizer,
) -> PublicationPreparationServices<'a> {
    PublicationPreparationServices {
        scope: scope(&base.graph().metadata.execution_id),
        actor: Actor::new(ActorType::Owner, "owner-test"),
        clock: &FixedClock,
        externalizer,
    }
}

fn publication_draft(base: &GraphVersion, operations: Vec<DraftOperation>) -> GraphDraft {
    GraphDraft {
        id: "draft-preflight".into(),
        expected_version: base.number(),
        expected_hash: base.content_hash().clone(),
        operations,
        manual_override: None,
    }
}

fn complete_publication_override(actor: Actor) -> ManualOverride {
    ManualOverride {
        actor,
        reason: "accepted review bypass".into(),
        waived_requirements: vec!["tests".into()],
        acknowledged_risks: vec!["unreviewed change".into()],
        scope: WaiverScope::Execution,
    }
}

#[derive(Default)]
struct RecordingPublicationObserver(Mutex<Vec<PublicationStage>>);

impl PublicationPreparationObserver for RecordingPublicationObserver {
    fn reached(&self, stage: PublicationStage) {
        self.0.lock().unwrap().push(stage);
    }
}

fn assert_preflight_limit_stages(
    base: &GraphVersion,
    operations: Vec<DraftOperation>,
    expected_stages: &[PublicationStage],
) {
    let before = serde_json::to_vec(base.graph()).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let externalizer = SealingGraphExternalizer::new(CountingFailureSealer(Arc::clone(&calls)));
    let services = publication_services(base, &externalizer);
    let observer = RecordingPublicationObserver::default();
    let error = block_on(prepare_draft_publication_observed(
        base,
        &publication_draft(base, operations),
        &services,
        Some(&observer),
    ))
    .unwrap_err();
    assert_eq!(error.code(), "GHE006_LIMIT_EXCEEDED");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(*observer.0.lock().unwrap(), expected_stages);
    assert_eq!(serde_json::to_vec(base.graph()).unwrap(), before);
}

fn assert_preflight_limit(base: &GraphVersion, operations: Vec<DraftOperation>) {
    assert_preflight_limit_stages(base, operations, &[]);
}

fn assert_audit_preflight_error(
    base: &GraphVersion,
    draft: &GraphDraft,
    actor: Actor,
    expected_code: &str,
) {
    let before = serde_json::to_vec(base.graph()).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let externalizer = SealingGraphExternalizer::new(CountingFailureSealer(Arc::clone(&calls)));
    let mut services = publication_services(base, &externalizer);
    services.actor = actor;
    let observer = RecordingPublicationObserver::default();
    let error = block_on(prepare_draft_publication_observed(
        base,
        draft,
        &services,
        Some(&observer),
    ))
    .unwrap_err();
    assert_eq!(error.code(), expected_code);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(observer.0.lock().unwrap().is_empty());
    assert_eq!(serde_json::to_vec(base.graph()).unwrap(), before);
}

#[test]
fn draft_audit_preflight_validates_nominal_draft_and_actor_ids_before_clone() {
    let base = version("software-feature.yaml");
    for (draft_id, actor, code) in [
        (
            "bad id".into(),
            Actor::new(ActorType::Owner, "owner-test"),
            "GHE009_EXTERNALIZATION_FAILED",
        ),
        (
            "x".repeat(129),
            Actor::new(ActorType::Owner, "owner-test"),
            "GHE006_LIMIT_EXCEEDED",
        ),
        (
            "draft-audit".into(),
            Actor::new(ActorType::Owner, "bad actor"),
            "GHE009_EXTERNALIZATION_FAILED",
        ),
        (
            "draft-audit".into(),
            Actor::new(ActorType::Owner, "x".repeat(257)),
            "GHE006_LIMIT_EXCEEDED",
        ),
    ] {
        let mut draft = publication_draft(&base, Vec::new());
        draft.id = draft_id;
        assert_audit_preflight_error(&base, &draft, actor, code);
    }
}

#[test]
fn draft_audit_preflight_requires_the_authoritative_owner_actor() {
    let base = version("software-feature.yaml");
    for (authoritative, override_actor, code) in [
        (
            Actor::new(ActorType::Owner, "owner-test"),
            Actor::new(ActorType::Human, "owner-test"),
            "GHE009_EXTERNALIZATION_FAILED",
        ),
        (
            Actor::new(ActorType::Owner, "owner-test"),
            Actor::new(ActorType::Owner, "other-owner"),
            "GHE009_EXTERNALIZATION_FAILED",
        ),
        (
            Actor::new(ActorType::Owner, "owner-test"),
            Actor::new(ActorType::Owner, "bad actor"),
            "GHE009_EXTERNALIZATION_FAILED",
        ),
        (
            Actor::new(ActorType::Owner, "owner-test"),
            Actor::new(ActorType::Owner, "x".repeat(257)),
            "GHE006_LIMIT_EXCEEDED",
        ),
        (
            Actor::new(ActorType::Human, "owner-test"),
            Actor::new(ActorType::Human, "owner-test"),
            "GHE009_EXTERNALIZATION_FAILED",
        ),
    ] {
        let mut draft = publication_draft(&base, Vec::new());
        draft.manual_override = Some(complete_publication_override(override_actor));
        assert_audit_preflight_error(&base, &draft, authoritative, code);
    }
}

#[test]
fn manual_override_wire_requires_a_reason() {
    let base = version("software-feature.yaml");
    let actor = Actor::new(ActorType::Owner, "owner-test");
    let mut value = serde_json::to_value(publication_draft(&base, Vec::new())).unwrap();
    value["manualOverride"] = serde_json::to_value(complete_publication_override(actor)).unwrap();
    value["manualOverride"]
        .as_object_mut()
        .unwrap()
        .remove("reason");

    assert!(serde_json::from_value::<GraphDraft>(value).is_err());
}

#[test]
fn draft_audit_preflight_bounds_reason_risks_and_waived_requirements_before_clone() {
    let base = version("software-feature.yaml");
    let actor = Actor::new(ActorType::Owner, "owner-test");
    let invalid = [
        (
            {
                let mut value = complete_publication_override(actor.clone());
                value.reason.clear();
                value
            },
            "GHE009_EXTERNALIZATION_FAILED",
        ),
        (
            {
                let mut value = complete_publication_override(actor.clone());
                value.reason = "r".repeat(2049);
                value
            },
            "GHE006_LIMIT_EXCEEDED",
        ),
        (
            {
                let mut value = complete_publication_override(actor.clone());
                value.acknowledged_risks.clear();
                value
            },
            "GHE009_EXTERNALIZATION_FAILED",
        ),
        (
            {
                let mut value = complete_publication_override(actor.clone());
                value.acknowledged_risks = vec!["risk".into(); 65];
                value
            },
            "GHE006_LIMIT_EXCEEDED",
        ),
        (
            {
                let mut value = complete_publication_override(actor.clone());
                value.acknowledged_risks = vec![String::new()];
                value
            },
            "GHE009_EXTERNALIZATION_FAILED",
        ),
        (
            {
                let mut value = complete_publication_override(actor.clone());
                value.acknowledged_risks = vec!["r".repeat(513)];
                value
            },
            "GHE006_LIMIT_EXCEEDED",
        ),
        (
            {
                let mut value = complete_publication_override(actor.clone());
                value.waived_requirements.clear();
                value
            },
            "GHE009_EXTERNALIZATION_FAILED",
        ),
        (
            {
                let mut value = complete_publication_override(actor.clone());
                value.waived_requirements = vec!["review".into(); 65];
                value
            },
            "GHE006_LIMIT_EXCEEDED",
        ),
        (
            {
                let mut value = complete_publication_override(actor.clone());
                value.waived_requirements = vec!["bad requirement".into()];
                value
            },
            "GHE009_EXTERNALIZATION_FAILED",
        ),
        (
            {
                let mut value = complete_publication_override(actor.clone());
                value.waived_requirements = vec!["r".repeat(129)];
                value
            },
            "GHE006_LIMIT_EXCEEDED",
        ),
    ];
    for (manual_override, code) in invalid {
        let mut draft = publication_draft(&base, Vec::new());
        draft.manual_override = Some(manual_override);
        assert_audit_preflight_error(&base, &draft, actor.clone(), code);
    }
}

#[test]
fn draft_audit_preflight_shares_aggregate_accounting_with_operations() {
    let mut record = version("software-feature.yaml").to_record();
    record.graph.spec.budgets.max_mutations = None;
    refresh_record(&mut record);
    let base = GraphVersion::from_record(record).unwrap();
    let operations = (0..5)
        .map(|index| DraftOperation::RemoveEdge {
            id: format!("{}-{index}", "x".repeat(13_420_000)),
        })
        .collect();
    let actor = Actor::new(ActorType::Owner, "owner-test");
    let mut draft = publication_draft(&base, operations);
    let mut manual_override = complete_publication_override(actor.clone());
    manual_override.reason = "r".repeat(2048);
    manual_override.acknowledged_risks = vec!["k".repeat(512); 64];
    manual_override.waived_requirements = (0..64)
        .map(|index| format!("requirement-{index}"))
        .collect();
    draft.manual_override = Some(manual_override);
    assert_audit_preflight_error(&base, &draft, actor, "GHE006_LIMIT_EXCEEDED");
}

#[test]
fn complete_owner_override_preserves_publication_order_and_audit_input() {
    let base = version("software-feature.yaml");
    let actor = Actor::new(ActorType::Owner, "owner-test");
    let mut operations: Vec<_> = base
        .graph()
        .spec
        .edges
        .iter()
        .filter(|edge| edge.from == "tests" || edge.to == "tests")
        .map(|edge| DraftOperation::RemoveEdge {
            id: edge.id.clone(),
        })
        .collect();
    operations.push(DraftOperation::RemoveNode { id: "tests".into() });
    let mut draft = publication_draft(&base, operations);
    draft.manual_override = Some(complete_publication_override(actor.clone()));
    let audit_before = draft.manual_override.clone();
    let externalizer = externalizer();
    let services = publication_services(&base, &externalizer);
    let observer = RecordingPublicationObserver::default();

    let prepared = block_on(prepare_draft_publication_observed(
        &base,
        &draft,
        &services,
        Some(&observer),
    ))
    .unwrap();

    assert_eq!(draft.manual_override, audit_before);
    assert!(!prepared.evidence().is_empty());
    assert_eq!(
        *observer.0.lock().unwrap(),
        [
            PublicationStage::CandidateClone,
            PublicationStage::CandidateSerialization,
            PublicationStage::CandidateLint,
            PublicationStage::CandidatePolicy,
            PublicationStage::Externalization,
        ]
    );
}

#[test]
fn draft_preflight_rejects_operation_count_above_hard_bound() {
    let mut record = version("software-feature.yaml").to_record();
    record.graph.spec.budgets.max_mutations = None;
    refresh_record(&mut record);
    let base = GraphVersion::from_record(record).unwrap();
    let operations = (0..4097)
        .map(|index| DraftOperation::RemoveEdge {
            id: format!("missing-{index}"),
        })
        .collect();
    assert_preflight_limit(&base, operations);
}

#[test]
fn draft_preflight_rejects_operation_count_above_graph_budget() {
    let base = version("software-feature.yaml");
    let operations = (0..7)
        .map(|index| DraftOperation::RemoveEdge {
            id: format!("missing-{index}"),
        })
        .collect();
    assert_preflight_limit(&base, operations);
}

#[test]
fn draft_preflight_rejects_deep_and_wide_patch_values() {
    let base = version("software-feature.yaml");
    let mut deep = serde_json::json!(null);
    for _ in 0..65 {
        deep = serde_json::json!([deep]);
    }
    assert_preflight_limit(
        &base,
        vec![DraftOperation::PatchNode {
            id: "implement".into(),
            patch: deep,
        }],
    );

    let wide = serde_json::Value::Array((0..131_073).map(|_| serde_json::Value::Null).collect());
    assert_preflight_limit(
        &base,
        vec![DraftOperation::PatchNode {
            id: "implement".into(),
            patch: wide,
        }],
    );
}

#[test]
fn draft_preflight_rejects_oversized_add_node_and_add_edge_fields() {
    let base = version("software-feature.yaml");
    let mut node = base.graph().spec.nodes.values().next().unwrap().clone();
    node.name = "n".repeat(16 * 1024 * 1024 + 1);
    assert_preflight_limit(
        &base,
        vec![DraftOperation::AddNode {
            id: "new-node".into(),
            node,
        }],
    );

    let mut aggregate_node = base.graph().spec.nodes.values().next().unwrap().clone();
    let chunk = serde_json::Value::String("p".repeat(13_421_760));
    for index in 0..5 {
        aggregate_node
            .properties
            .insert(format!("aggregate{index}"), chunk.clone());
    }
    assert_preflight_limit(
        &base,
        vec![DraftOperation::AddNode {
            id: "aggregate-node".into(),
            node: aggregate_node,
        }],
    );

    let mut edge = base.graph().spec.edges[0].clone();
    edge.id = "new-edge".into();
    edge.bindings
        .insert("input".into(), "b".repeat(16 * 1024 * 1024 + 1));
    assert_preflight_limit(&base, vec![DraftOperation::AddEdge { edge }]);
}

#[test]
fn publication_stage_order_is_preserved_for_within_limit_inputs() {
    let base = version("software-feature.yaml");
    let externalizer = externalizer();
    let services = publication_services(&base, &externalizer);
    let observer = RecordingPublicationObserver::default();
    block_on(prepare_draft_publication_observed(
        &base,
        &publication_draft(&base, Vec::new()),
        &services,
        Some(&observer),
    ))
    .unwrap();
    assert_eq!(
        *observer.0.lock().unwrap(),
        [
            PublicationStage::CandidateClone,
            PublicationStage::CandidateSerialization,
            PublicationStage::CandidateLint,
            PublicationStage::CandidatePolicy,
            PublicationStage::Externalization,
        ]
    );
}

#[test]
fn draft_preflight_uses_one_aggregate_counter_across_operations() {
    let base = version("software-feature.yaml");
    let operations = (0..6)
        .map(|index| DraftOperation::RemoveEdge {
            id: format!("{}-{index}", "x".repeat(12 * 1024 * 1024)),
        })
        .collect();
    assert_preflight_limit(&base, operations);
}

#[test]
fn candidate_preflight_runs_after_application_before_externalization() {
    let mut record = version("software-feature.yaml").to_record();
    record.graph.spec.budgets.max_mutations = None;
    refresh_record(&mut record);
    let base = GraphVersion::from_record(record).unwrap();
    let template = base.graph().spec.nodes.values().next().unwrap().clone();
    let operations = (0..=(1024 - base.graph().spec.nodes.len()))
        .map(|index| DraftOperation::AddNode {
            id: format!("candidate-node-{index:04}"),
            node: template.clone(),
        })
        .collect();
    assert_preflight_limit_stages(&base, operations, &[PublicationStage::CandidateClone]);
}

#[test]
fn base_and_disjoint_patch_share_one_aggregate_preflight_before_clone() {
    let mut record = version("software-feature.yaml").to_record();
    record
        .graph
        .spec
        .nodes
        .get_mut("plan")
        .unwrap()
        .properties
        .insert(
            "baseBudget".into(),
            serde_json::Value::Array((0..66_000).map(|_| serde_json::Value::Null).collect()),
        );
    refresh_record(&mut record);
    let base = GraphVersion::from_record(record).unwrap();
    let patch = serde_json::json!({
        "draftBudget": serde_json::Value::Array(
            (0..66_000).map(|_| serde_json::Value::Null).collect()
        )
    });

    assert_preflight_limit_stages(
        &base,
        vec![DraftOperation::PatchNode {
            id: "implement".into(),
            patch,
        }],
        &[],
    );
}

#[test]
fn oversized_base_preflight_runs_before_candidate_clone() {
    let template = version("software-feature.yaml");
    let mut graph = template.graph().clone();
    let mut node = graph.spec.nodes.values().next().unwrap().clone();
    node.properties = (0..128)
        .map(|index| (format!("property-{index:03}"), serde_json::Value::Null))
        .collect();
    graph.spec.nodes.clear();
    graph.spec.edges.clear();
    graph.spec.entrypoints = vec!["node-0000".into()];
    for index in 0..1024 {
        graph
            .spec
            .nodes
            .insert(format!("node-{index:04}"), node.clone());
    }
    let base = GraphVersion::publish(
        graph,
        None,
        Actor::new(ActorType::Owner, "owner-test"),
        Utc.with_ymd_and_hms(2026, 8, 9, 12, 0, 0).unwrap(),
    )
    .unwrap();
    assert_preflight_limit_stages(&base, Vec::new(), &[]);
}

#[test]
fn draft_preflight_counts_operation_node_edge_patch_and_map_structure_exactly() {
    let mut record = version("software-feature.yaml").to_record();
    record.graph.spec.budgets.max_mutations = None;
    refresh_record(&mut record);
    let base = GraphVersion::from_record(record).unwrap();
    let template = base.graph().spec.nodes.values().next().unwrap().clone();

    let add_nodes = (0..4096)
        .map(|index| {
            let mut node = template.clone();
            node.properties = (0..13)
                .map(|property| (format!("property-{property:02}"), serde_json::Value::Null))
                .collect();
            DraftOperation::AddNode {
                id: format!("bulk-node-{index:04}"),
                node,
            }
        })
        .collect();
    assert_preflight_limit_stages(&base, add_nodes, &[]);

    // Eight properties put each typed structural category immediately across
    // the aggregate boundary: removing the operation object, node object,
    // property-map container, or one fixed scalar inventory makes this pass.
    let near_limit_nodes = (0..4096)
        .map(|index| {
            let mut node = template.clone();
            node.properties = (0..8)
                .map(|property| (format!("near-{property:02}"), serde_json::Value::Null))
                .collect();
            DraftOperation::AddNode {
                id: format!("near-node-{index:04}"),
                node,
            }
        })
        .collect();
    assert_preflight_limit_stages(&base, near_limit_nodes, &[]);

    let add_edges = (0..4096)
        .map(|index| {
            let mut edge = base.graph().spec.edges[0].clone();
            edge.id = format!("bulk-edge-{index:04}");
            edge.bindings = (0..13)
                .map(|binding| {
                    (
                        format!("binding-{binding:02}"),
                        "outputs.plan.result".into(),
                    )
                })
                .collect();
            DraftOperation::AddEdge { edge }
        })
        .collect();
    assert_preflight_limit_stages(&base, add_edges, &[]);

    let patch = serde_json::Value::Object(
        (0..25)
            .map(|field| (format!("field-{field:02}"), serde_json::Value::Null))
            .collect(),
    );
    let patches = (0..4096)
        .map(|index| DraftOperation::PatchNode {
            id: format!("missing-node-{index:04}"),
            patch: patch.clone(),
        })
        .collect();
    assert_preflight_limit_stages(&base, patches, &[]);
}

#[test]
fn base_preflight_counts_nested_object_keys_before_clone_serde_and_sealing() {
    let base_with_probe = |probe: serde_json::Value| {
        let mut record = version("software-feature.yaml").to_record();
        record
            .graph
            .spec
            .nodes
            .get_mut("plan")
            .unwrap()
            .properties
            .insert("preflightProbe".into(), probe);
        refresh_record(&mut record);
        GraphVersion::from_record(record).unwrap()
    };

    let near_value_ceiling = serde_json::Value::Object(
        (0..65_536)
            .map(|index| (format!("key-{index:05}"), serde_json::Value::Null))
            .collect(),
    );
    assert_preflight_limit_stages(&base_with_probe(near_value_ceiling), Vec::new(), &[]);

    let oversized_key = serde_json::Value::Object(
        std::iter::once(("k".repeat(16 * 1024 * 1024 + 1), serde_json::Value::Null)).collect(),
    );
    assert_preflight_limit_stages(&base_with_probe(oversized_key), Vec::new(), &[]);
}

fn direct_successor_record() -> GraphVersionRecord {
    let mut record = version("software-feature.yaml").to_record();
    record.graph.metadata.version = 2;
    record.graph.metadata.based_on = Some(record.graph.metadata.id.clone());
    record.predecessor = Some(GraphVersionRef {
        number: 1,
        content_hash: record.content_hash.clone(),
    });
    refresh_record(&mut record);
    record
}

#[test]
fn direct_externalizer_rejects_the_complete_record_before_clone_or_sealing() {
    let mut record = version("software-feature.yaml").to_record();
    let template = record.graph.spec.nodes.values().next().unwrap().clone();
    record.graph.spec.entrypoints = vec!["node-000".into()];
    record.graph.spec.edges.clear();
    record.graph.spec.nodes = (0..500)
        .map(|index| {
            let mut node = template.clone();
            node.properties = (0..128)
                .map(|property| (format!("property-{property:03}"), serde_json::Value::Null))
                .collect();
            (format!("node-{index:03}"), node)
        })
        .collect();
    // The direct boundary must reject the typed graph half before attempting
    // to clone/canonicalize this deliberately stale semantic half.
    record.semantic = serde_json::Value::Null;

    let calls = Arc::new(AtomicUsize::new(0));
    let crossed_preflight = Arc::new(AtomicUsize::new(0));
    let externalizer = SealingGraphExternalizer::new(CountingFailureSealer(Arc::clone(&calls)));
    let observer = {
        let crossed_preflight = Arc::clone(&crossed_preflight);
        move || {
            crossed_preflight.fetch_add(1, Ordering::SeqCst);
        }
    };
    let error = block_on(externalizer.prepare_observed(
        scope(&record.graph.metadata.execution_id),
        &record,
        &observer,
    ))
    .unwrap_err();

    assert_eq!(error.code(), "GHE006_LIMIT_EXCEEDED");
    assert_eq!(crossed_preflight.load(Ordering::SeqCst), 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

fn safe_predecessor(number: u64, digit: char) -> PersistedGraphVersionRef {
    PersistedGraphVersionRef::new(
        number,
        WireHash::parse(format!("sha256:{}", digit.to_string().repeat(64))).unwrap(),
    )
    .unwrap()
}

#[test]
fn direct_externalizer_rejects_relational_topology_before_sealing() {
    let baseline = version("software-feature.yaml").to_record();
    let mut cases = Vec::new();

    let mut duplicate_edge = baseline.clone();
    duplicate_edge.graph.spec.edges[1].id = duplicate_edge.graph.spec.edges[0].id.clone();
    refresh_record(&mut duplicate_edge);
    cases.push(duplicate_edge);

    let mut missing_entrypoint = baseline.clone();
    missing_entrypoint.graph.spec.entrypoints = vec!["missing-node-canary".into()];
    refresh_record(&mut missing_entrypoint);
    cases.push(missing_entrypoint);

    let mut missing_source = baseline.clone();
    missing_source.graph.spec.edges[0].from = "missing-source-canary".into();
    refresh_record(&mut missing_source);
    cases.push(missing_source);

    let mut missing_target = baseline;
    missing_target.graph.spec.edges[0].to = "missing-target-canary".into();
    refresh_record(&mut missing_target);
    cases.push(missing_target);

    for record in cases {
        let calls = Arc::new(AtomicUsize::new(0));
        let externalizer = SealingGraphExternalizer::new(CountingFailureSealer(Arc::clone(&calls)));
        let error =
            block_on(externalizer.prepare(scope(&record.graph.metadata.execution_id), &record))
                .unwrap_err();

        assert_eq!(error.code(), "GHE005_INTEGRITY_FAILURE");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let public = format!("{error:?} {error}");
        assert!(!public.contains("missing-source-canary"));
        assert!(!public.contains("missing-target-canary"));
        assert!(!public.contains("missing-node-canary"));
    }
}

#[test]
fn direct_externalizer_requires_explicit_safe_predecessor_lineage() {
    let genesis = version("software-feature.yaml").to_record();
    let successor = direct_successor_record();
    let safe = safe_predecessor(1, 'a');

    let error = block_on(externalizer().prepare_with_predecessor(
        scope(&genesis.graph.metadata.execution_id),
        &genesis,
        safe.clone(),
    ))
    .unwrap_err();
    assert_eq!(error.code(), "GHE005_INTEGRITY_FAILURE");

    let error =
        block_on(externalizer().prepare(scope(&successor.graph.metadata.execution_id), &successor))
            .unwrap_err();
    assert_eq!(error.code(), "GHE005_INTEGRITY_FAILURE");

    let error = block_on(externalizer().prepare_with_predecessor(
        scope(&successor.graph.metadata.execution_id),
        &successor,
        safe_predecessor(2, 'b'),
    ))
    .unwrap_err();
    assert_eq!(error.code(), "GHE005_INTEGRITY_FAILURE");
}

#[test]
fn direct_externalizer_serializes_only_the_explicit_safe_predecessor_hash() {
    let mut successor = direct_successor_record();
    successor.predecessor.as_mut().unwrap().content_hash =
        graphhelm_protocols::SemanticHash::new(format!("sha256:{}", "f".repeat(64)));
    let safe = safe_predecessor(1, 'a');

    let prepared = block_on(externalizer().prepare_with_predecessor(
        scope(&successor.graph.metadata.execution_id),
        &successor,
        safe.clone(),
    ))
    .unwrap();

    assert_eq!(prepared.version().predecessor(), Some(&safe));
    assert_ne!(
        prepared
            .version()
            .predecessor()
            .unwrap()
            .semantic_hash()
            .as_str(),
        successor
            .predecessor
            .as_ref()
            .unwrap()
            .content_hash
            .as_str()
    );
}

// #1049: a node is the TASK, so more than one agent may work it. The crew must cross the
// documented Graph DSL -> GraphVersion journey, or the shape is only representable on paper.
fn crew_record(primary: bool, crew: serde_json::Value) -> GraphVersionRecord {
    let mut record = version("software-feature.yaml").to_record();
    let node = record.graph.spec.nodes.get_mut("map_repository").unwrap();
    if primary {
        node.properties.insert(
            "agent".into(),
            serde_json::json!({"ref": "project/security-reviewer@3"}),
        );
    } else {
        node.properties.remove("agent");
    }
    node.properties.insert("agents".into(), crew);
    refresh_record(&mut record);
    record
}

#[test]
fn a_crew_beside_the_primary_is_recorded_as_its_own_control() {
    let record = crew_record(
        true,
        serde_json::json!([
            {"ref": "project/reviewer@1"},
            {"ref": "project/scribe@2"}
        ]),
    );
    let prepared =
        block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
            .unwrap();

    let primary = control(&prepared, "map_repository", "agent_configuration");
    assert_eq!(identifier(primary, "mode"), "ref");
    assert_eq!(
        decode_reference(identifier(primary, "agentRef")),
        "project/security-reviewer@3"
    );
    let crew = control(&prepared, "map_repository", "node_agents");
    assert_eq!(integer(crew, "agentRefCount"), 2);
    assert_eq!(
        decode_reference(identifier(crew, "agentRef.000")),
        "project/reviewer@1"
    );
    assert_eq!(
        decode_reference(identifier(crew, "agentRef.001")),
        "project/scribe@2"
    );
    assert!(flag(crew, "present"));
}

// The crew never invents a primary, and never stands in for one. `type == "agent"` still
// requires `agent` - that rule lives in the node schema's `allOf`, which this change leaves
// byte-for-byte as it was, and the Governor validates the authored graph against that schema
// before it projects anything. So a primary-less agent node is refused for the reason it was
// refused at 1.0.0, and the crew control is never fabricated from the singular.
#[test]
fn a_crew_does_not_stand_in_for_the_primary() {
    let record = crew_record(false, serde_json::json!([{"ref": "project/reviewer@1"}]));
    assert_eq!(
        block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
            .unwrap_err(),
        graphhelm_governor::GovernorError::InvalidAuthoring,
        "an agent node with no primary worker crossed publication"
    );

    let primary_only = version("software-feature.yaml").to_record();
    let prepared = block_on(externalizer().prepare(
        scope(&primary_only.graph.metadata.execution_id),
        &primary_only,
    ))
    .unwrap();
    assert!(
        prepared
            .version()
            .topology()
            .nodes()
            .iter()
            .all(|(_, node)| node
                .controls()
                .iter()
                .all(|control| control.control_type().as_str() != "node_agents")),
        "a graph that authors no crew must carry no crew control"
    );
}

// Every refusal the authored crew carries, refused BEFORE sealing and without echoing input.
#[test]
fn a_malformed_or_foreign_crew_fails_before_sealing() {
    for (case, primary, crew) in [
        ("empty", true, serde_json::json!([])),
        ("scalar", true, serde_json::json!("project/reviewer@1")),
        (
            "scalar member",
            true,
            serde_json::json!(["project/reviewer@1"]),
        ),
        (
            "member without a ref",
            true,
            serde_json::json!([{"ephemeral": {"purpose": "help"}}]),
        ),
        (
            "member with a stray key",
            true,
            serde_json::json!([{"ref": "project/reviewer@1", "mode": "ref"}]),
        ),
    ] {
        let record = crew_record(primary, crew);
        let error =
            block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
                .unwrap_err();
        assert_eq!(
            error,
            graphhelm_governor::GovernorError::InvalidAuthoring,
            "{case} was accepted"
        );
    }

    let mut record = crew_record(true, serde_json::json!([{"ref": "project/reviewer@1"}]));
    let node = record.graph.spec.nodes.get_mut("tests").unwrap();
    assert_eq!(node.node_type.as_str(), "tool");
    node.properties.insert(
        "agents".into(),
        serde_json::json!([{"ref": "project/reviewer@1"}]),
    );
    refresh_record(&mut record);
    assert_eq!(
        block_on(externalizer().prepare(scope(&record.graph.metadata.execution_id), &record))
            .unwrap_err(),
        graphhelm_governor::GovernorError::InvalidAuthoring,
        "a crew on a tool node was accepted"
    );
}
