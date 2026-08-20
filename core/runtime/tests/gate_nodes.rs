//! M06 Task 4: Gate nodes execute — certified or not at all.
//!
//! The classification table is pinned WHOLE: exactly one refusal arm dies in this
//! milestone (Gate), and the other fifteen node types keep their 05d behavior
//! byte-identical. A Gate node runs only under a live certification against the CURRENT
//! pathogen-suite digest; its verdict is appended beside the node outcome in the same
//! durable batch, and a failing verdict is the outcome the graph routes on.

use graphhelm_protocols::NodeType;
use graphhelm_runtime::classify::{NodeWorkKind, work_kind};

#[test]
fn the_classification_table_is_pinned_whole() {
    use NodeType::{
        Agent, ArtifactTransform, Classifier, Deploy, Evaluator, Fork, Gate, HumanDecision, Join,
        Materializer, Planner, Rollback, Subgraph, Timer, Tool, Trigger,
    };
    // The 05d cognitive four, byte-identical.
    for cognitive in [Agent, Planner, Classifier, Evaluator] {
        assert_eq!(work_kind(&cognitive), Ok(NodeWorkKind::Cognitive));
    }
    // The 05c/05d tool path, byte-identical.
    assert_eq!(work_kind(&Tool), Ok(NodeWorkKind::Tool));
    // The ONE arm this milestone opens: deterministic gate work, no model port.
    assert_eq!(work_kind(&Gate), Ok(NodeWorkKind::GateCheck));
    // Everything else still refuses — the exhaustive-match posture unchanged.
    for refused in [
        Fork,
        Join,
        HumanDecision,
        Timer,
        Trigger,
        Subgraph,
        Materializer,
        Deploy,
        Rollback,
        ArtifactTransform,
    ] {
        assert!(
            work_kind(&refused).is_err(),
            "{refused:?} must still refuse"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// The driver half: a certified gate runs and its verdict lands beside the outcome in the
// same batch; an uncertified (or stale-certified) gate never runs at all.
// ---------------------------------------------------------------------------------------------

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{TimeZone, Utc};
use graphhelm_events::{
    AuthenticateRequest, AuthenticationTag, EvidenceProtector, KeyError, KeyProvider,
    KeyProviderMetadata, LocalEventRepository, PreparedAppend, RepositoryFuture, RevocationReceipt,
    RevokeKeyRequest, SecretBytes, VerifyAuthenticationRequest, WrapKeyRequest, WrappedKey,
};
use graphhelm_protocols::{
    ActorId, Clock, EventKind, ExecutionId, ExecutionMode, ExecutionStarted, GateCertified,
    GraphBudgets, GraphNode, GraphSpec, IdGenerator, NewEvent, NodeState, OpaqueId, Optionality,
    PersistedActor, PersistedActorType, ProjectId, RawSha256, RepositoryScope, Sensitivity,
    WireHash, WorkspaceId,
};
use graphhelm_runtime::driver::{StoreOpen, drive_to_quiescence_async};
use graphhelm_runtime::executor::PortExecutor;
use graphhelm_tool_broker::lease::ToolLease;

fn empty_lease() -> ToolLease {
    ToolLease {
        actor: "agent-gate".to_owned(),
        capabilities: std::collections::BTreeSet::new(),
        programs: std::collections::BTreeSet::new(),
    }
}

struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 16, 12, 0, 0).unwrap()
    }
}

#[derive(Default)]
struct SequenceIds(AtomicU64);
impl IdGenerator for SequenceIds {
    fn next_id(&self, prefix: &'static str) -> String {
        format!("{prefix}-{}", self.0.fetch_add(1, Ordering::SeqCst) + 1)
    }
}

fn digest32(bytes: &[u8]) -> RawSha256 {
    use sha2::{Digest, Sha256};
    RawSha256::parse(hex::encode(Sha256::digest(bytes))).unwrap()
}

#[derive(Default)]
struct InMemoryKeyProvider {
    keys: Mutex<BTreeMap<String, Vec<u8>>>,
}

impl KeyProvider for InMemoryKeyProvider {
    fn metadata<'a>(&'a self) -> RepositoryFuture<'a, Result<KeyProviderMetadata, KeyError>> {
        Box::pin(async { KeyProviderMetadata::new("test-key", "test-provider", "1.0.0", 0) })
    }
    fn wrap<'a>(
        &'a self,
        request: WrapKeyRequest,
    ) -> RepositoryFuture<'a, Result<WrappedKey, KeyError>> {
        Box::pin(async move {
            let handle = request.handle().to_owned();
            let key = request.plaintext_key().expose(|bytes| bytes.to_vec());
            self.keys.lock().unwrap().insert(handle.clone(), key);
            WrappedKey::new(
                "test-key",
                handle,
                "xchacha20poly1305",
                vec![7; 24],
                vec![11; 48],
                digest32(request.aad()),
            )
        })
    }
    fn unwrap<'a>(
        &'a self,
        wrapped: WrappedKey,
    ) -> RepositoryFuture<'a, Result<SecretBytes, KeyError>> {
        Box::pin(async move {
            self.keys
                .lock()
                .unwrap()
                .get(wrapped.handle())
                .cloned()
                .map(SecretBytes::new)
                .ok_or(KeyError::Unavailable)
        })
    }
    fn revoke<'a>(
        &'a self,
        request: RevokeKeyRequest,
    ) -> RepositoryFuture<'a, Result<RevocationReceipt, KeyError>> {
        Box::pin(async move {
            RevocationReceipt::new(
                request.handle(),
                request.idempotency_key(),
                1,
                AuthenticationTag::new("test-key", "hmac-sha256", vec![3; 32])?,
            )
        })
    }
    fn authenticate<'a>(
        &'a self,
        _request: AuthenticateRequest,
    ) -> RepositoryFuture<'a, Result<AuthenticationTag, KeyError>> {
        Box::pin(async { AuthenticationTag::new("test-key", "hmac-sha256", vec![3; 32]) })
    }
    fn verify<'a>(
        &'a self,
        _request: VerifyAuthenticationRequest,
    ) -> RepositoryFuture<'a, Result<(), KeyError>> {
        Box::pin(async { Ok(()) })
    }
}

const GATE_STREAM: &str = "stream-gate-test";
const SUITE_DIGEST: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const STALE_DIGEST: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn gate_scope() -> RepositoryScope {
    RepositoryScope::new(
        WorkspaceId::parse("workspace-gate").unwrap(),
        ProjectId::parse("project-gate").unwrap(),
        Some(ExecutionId::parse("execution-gate").unwrap()),
    )
}

fn gate_actor() -> PersistedActor {
    PersistedActor::new(
        PersistedActorType::System,
        ActorId::parse("system-gate").unwrap(),
    )
}

fn plain_event(key: &str, kind: EventKind) -> NewEvent {
    NewEvent::new(
        OpaqueId::parse(key).unwrap(),
        gate_actor(),
        Sensitivity::Internal,
        kind,
        vec![],
        vec![],
    )
}

/// A started execution; when `certified` carries a digest, a `GateCertified` receipt for
/// `gate-geometry` against that digest is in the stream too.
fn started_store(directory: &std::path::Path, certified: Option<&str>) -> OpaqueId {
    let repository = LocalEventRepository::open(
        directory,
        Arc::new(FixedClock),
        Arc::new(SequenceIds::default()),
    )
    .unwrap();
    let execution_id = OpaqueId::parse("execution-gate").unwrap();
    let mut events = vec![plain_event(
        "gate-started",
        EventKind::ExecutionStarted(ExecutionStarted {
            execution_id: execution_id.clone(),
            graph_version: 1,
            graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
            mode: ExecutionMode::Autopilot,
        }),
    )];
    if let Some(digest) = certified {
        events.push(plain_event(
            "gate-certified",
            EventKind::GateCertified(GateCertified {
                execution_id: execution_id.clone(),
                gate_id: OpaqueId::parse("gate-geometry").unwrap(),
                suite_digest: WireHash::parse(digest).unwrap(),
                specimens: 10,
            }),
        ));
    }
    let request = PreparedAppend::new(
        gate_scope(),
        OpaqueId::parse(GATE_STREAM).unwrap(),
        1,
        events,
        vec![],
        vec![],
    )
    .unwrap();
    repository.append_atomic(&request).unwrap();
    execution_id
}

fn opener(directory: std::path::PathBuf) -> StoreOpen {
    Arc::new(move || {
        LocalEventRepository::open(
            &directory,
            Arc::new(FixedClock),
            Arc::new(SequenceIds::default()),
        )
    })
}

/// A gate node whose contract carries the decided check. `blank` empties the page so the
/// manifest fails; otherwise the delivered surface is coherent and passes clean.
fn gate_graph_node(blank: bool) -> GraphNode {
    let html = if blank {
        String::new()
    } else {
        "<main id=\"panel\">Panel<ul><li>ok</li></ul></main>".to_owned()
    };
    let mut properties = std::collections::BTreeMap::new();
    properties.insert(
        "gate".to_owned(),
        serde_json::json!({"check": {
            "gateId": "gate-geometry",
            "delivered": {
                "claims": [{"feature": "Panel", "elementId": "panel", "artifact": "src/panel.rs"}],
                "html": html,
                "reachableIds": ["panel"],
                "journey": [
                    {"action": "open Panel", "assertion": "Panel is visible", "exercisesErrorPath": false},
                    {"action": "break Panel", "assertion": "Panel refuses", "exercisesErrorPath": true}
                ],
                "tests": [{"name": "panel_works", "passed": true, "assertions": 2}],
                "diff": {"filesTouched": 1, "behaviorLines": 5}
            },
            "manifest": {"required": [{"marker": "id=\"panel\"", "label": "Panel"}]}
        }}),
    );
    GraphNode {
        node_type: NodeType::Gate,
        name: "gate".to_owned(),
        objective: "gate the delivery".to_owned(),
        optionality: Optionality::Required,
        properties,
    }
}

fn gate_spec(blank: bool) -> GraphSpec {
    GraphSpec {
        entrypoints: vec!["quality".to_owned()],
        nodes: [("quality".to_owned(), gate_graph_node(blank))].into(),
        edges: Vec::new(),
        budgets: GraphBudgets {
            max_parallel_model_calls: Some(1),
            ..GraphBudgets::default()
        },
        policies: Vec::new(),
        completion: serde_json::Value::Null,
    }
}

/// The executor under test is the REAL PortExecutor: gate work must never touch either
/// port, so panicking ports prove the no-model-no-tool transport claim for free.
fn port_executor() -> Arc<PortExecutor> {
    struct NoPort;
    impl graphhelm_runtime::ports::ModelPort for NoPort {
        fn call<'a>(
            &'a self,
            _route_id: &'a str,
            _call: &'a graphhelm_gateway::call::ModelCall,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<
                            graphhelm_gateway::call::ModelReply,
                            graphhelm_gateway::taxonomy::GatewayError,
                        >,
                    > + Send
                    + 'a,
            >,
        > {
            panic!("gate work must never call the model port");
        }
    }
    impl graphhelm_runtime::ports::ToolPort for NoPort {
        fn invoke<'a>(
            &'a self,
            _call: &'a graphhelm_tool_broker::call::ToolCall,
            _lease: &'a ToolLease,
            _actor: &'a str,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = graphhelm_runtime::ports::ToolPortResult>
                    + Send
                    + 'a,
            >,
        > {
            panic!("gate work must never call the tool port");
        }
    }
    Arc::new(PortExecutor {
        model: Arc::new(NoPort),
        tools: Arc::new(NoPort),
        route_id: "route-test".to_owned(),
        lease: empty_lease(),
        actor: "agent-gate".to_owned(),
    })
}

fn drive(
    directory: &std::path::Path,
    execution_id: OpaqueId,
    spec: GraphSpec,
    digest: Option<String>,
) -> graphhelm_events::ExecutionProjection {
    let sealer = Arc::new(EvidenceProtector::new(InMemoryKeyProvider::default()));
    let ids = Arc::new(SequenceIds::default());
    let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    runtime
        .block_on(drive_to_quiescence_async(
            opener(directory.to_path_buf()),
            sealer,
            ids,
            gate_scope(),
            OpaqueId::parse(GATE_STREAM).unwrap(),
            execution_id,
            spec,
            port_executor(),
            gate_actor(),
            // #123: this drive releases nothing, so the set is empty and the
            // releasing actor is never consulted.
            std::collections::BTreeSet::new(),
            gate_actor(),
            cancel_rx,
            digest,
        ))
        .unwrap()
}

fn read_kinds(directory: &std::path::Path) -> Vec<String> {
    let repository = LocalEventRepository::open(
        directory,
        Arc::new(FixedClock),
        Arc::new(SequenceIds::default()),
    )
    .unwrap();
    let history = repository
        .read_replay_stream(&gate_scope(), GATE_STREAM)
        .unwrap();
    history
        .iter()
        .map(|event| match &event.kind {
            EventKind::GateVerdict(verdict) => format!(
                "gate_verdict:{}:{}:{}",
                verdict.gate_id.as_str(),
                verdict.passed,
                verdict.findings.len()
            ),
            EventKind::NodeOutcomeRecorded(outcome) => {
                format!("outcome:{:?}:{:?}", outcome.outcome, outcome.next_state)
            }
            other => format!("{:?}", std::mem::discriminant(other)),
        })
        .collect()
}

#[test]
fn a_certified_gate_runs_and_a_failing_verdict_lands_beside_the_terminal_outcome() {
    let directory = tempfile::tempdir().unwrap();
    let execution_id = started_store(directory.path(), Some(SUITE_DIGEST));
    let projection = drive(
        directory.path(),
        execution_id,
        gate_spec(true),
        Some(SUITE_DIGEST.to_owned()),
    );
    assert_eq!(
        projection.node_states.get("quality"),
        Some(&NodeState::Failed),
        "a deterministic failing verdict is terminal — retrying cannot change it"
    );
    let kinds = read_kinds(directory.path());
    let verdict_position = kinds
        .iter()
        .position(|kind| kind.starts_with("gate_verdict:gate-geometry:false:"))
        .unwrap_or_else(|| panic!("the failing verdict is in the ledger: {kinds:?}"));
    assert!(
        kinds[verdict_position]
            .strip_prefix("gate_verdict:gate-geometry:false:")
            .is_some_and(|count| count.parse::<usize>().unwrap() >= 1),
        "a failing verdict always carries findings"
    );
    assert!(
        kinds[verdict_position - 1].starts_with("outcome:TerminalFailure"),
        "the verdict rides the SAME append as the outcome it explains: {kinds:?}"
    );
}

#[test]
fn a_certified_gate_passing_clean_succeeds_with_an_empty_findings_verdict() {
    let directory = tempfile::tempdir().unwrap();
    let execution_id = started_store(directory.path(), Some(SUITE_DIGEST));
    let projection = drive(
        directory.path(),
        execution_id,
        gate_spec(false),
        Some(SUITE_DIGEST.to_owned()),
    );
    assert_eq!(
        projection.node_states.get("quality"),
        Some(&NodeState::Succeeded)
    );
    let kinds = read_kinds(directory.path());
    assert!(
        kinds
            .iter()
            .any(|kind| kind == "gate_verdict:gate-geometry:true:0"),
        "a clean pass verdicts with zero findings: {kinds:?}"
    );
}

#[test]
fn an_uncertified_gate_never_runs_and_never_verdicts() {
    let directory = tempfile::tempdir().unwrap();
    let execution_id = started_store(directory.path(), None);
    let projection = drive(
        directory.path(),
        execution_id,
        gate_spec(true),
        Some(SUITE_DIGEST.to_owned()),
    );
    assert_ne!(
        projection.node_states.get("quality"),
        Some(&NodeState::Succeeded),
        "an uncertified gate must not have gated"
    );
    let kinds = read_kinds(directory.path());
    assert!(
        !kinds.iter().any(|kind| kind.starts_with("gate_verdict")),
        "no certification, no verdict — ever: {kinds:?}"
    );
}

#[test]
fn a_stale_certification_refuses_identically() {
    let directory = tempfile::tempdir().unwrap();
    let execution_id = started_store(directory.path(), Some(STALE_DIGEST));
    let projection = drive(
        directory.path(),
        execution_id,
        gate_spec(true),
        Some(SUITE_DIGEST.to_owned()),
    );
    let kinds = read_kinds(directory.path());
    assert!(
        !kinds.iter().any(|kind| kind.starts_with("gate_verdict")),
        "a receipt against a grown suite is void — recertify or stand down: {kinds:?}"
    );
    assert_ne!(
        projection.node_states.get("quality"),
        Some(&NodeState::Succeeded)
    );
}
