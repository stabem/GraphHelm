//! #1226 cost observer, alone in its own test binary because it reads a process-wide counter.
//!
//! Over a whole drive of a 40-node chain, the events folded into a projection stay within a small
//! multiple of the stream's final length (O(H)). A driver that replays the full stream on every
//! reread folds O(H) per reread and O(H^2) per drive, which this cell refuses.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{TimeZone, Utc};
use graphhelm_events::{
    AuthenticateRequest, AuthenticationTag, EvidenceProtector, KeyError, KeyProvider,
    KeyProviderMetadata, LocalEventRepository, PreparedAppend, RepositoryFuture, RevocationReceipt,
    RevokeKeyRequest, SecretBytes, VerifyAuthenticationRequest, WrapKeyRequest, WrappedKey,
};
use graphhelm_protocols::{
    ActorId, Clock, EdgeType, EventKind, ExecutionId, ExecutionMode, ExecutionStarted,
    GraphBudgets, GraphEdge, GraphNode, GraphSpec, IdGenerator, NewEvent, NodeOutcome, NodeType,
    OpaqueId, Optionality, PersistedActor, PersistedActorType, ProjectId, RawSha256,
    RepositoryScope, Sensitivity, SimulationStatus, WireHash, WorkspaceId,
};
use graphhelm_runtime::driver::{ImmediateCancelRequest, StoreOpen, drive_to_quiescence_async};
use graphhelm_runtime::executor::{
    AsyncNodeExecutor, ExecutorRefusal, NodeWork, WorkOutcome, WorkSummary,
};

const STREAM: &str = "stream-fold-cost";
const NODES: usize = 40;

struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 14, 12, 0, 0).unwrap()
    }
}

#[derive(Default)]
struct Ids(AtomicU64);
impl IdGenerator for Ids {
    fn next_id(&self, prefix: &'static str) -> String {
        format!("{prefix}-{}", self.0.fetch_add(1, Ordering::SeqCst) + 1)
    }
}

fn digest32(bytes: &[u8]) -> RawSha256 {
    use sha2::Digest as _;
    RawSha256::parse(hex::encode(sha2::Sha256::digest(bytes))).unwrap()
}

/// The test key provider `driver_contract.rs` uses: wrap stores, unwrap returns.
#[derive(Default)]
struct Keys(Mutex<BTreeMap<String, Vec<u8>>>);

impl KeyProvider for Keys {
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
            self.0.lock().unwrap().insert(handle.clone(), key);
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
            self.0
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

/// Every node succeeds at once with nothing to seal: the drive's own reads are all that is left.
struct Succeeds;
impl AsyncNodeExecutor for Succeeds {
    fn execute<'a>(
        &'a self,
        _work: &'a NodeWork,
    ) -> Pin<Box<dyn Future<Output = Result<WorkOutcome, ExecutorRefusal>> + Send + 'a>> {
        Box::pin(async {
            Ok(WorkOutcome {
                outcome: NodeOutcome::Succeeded,
                crash_site: None,
                reason: None,
                sealables: Vec::new(),
                summary: WorkSummary {
                    input_tokens: None,
                    output_tokens: None,
                    exit_code: None,
                    provider_usage: None,
                },
                reuse: None,
                gate_verdict: None,
                executor_kind: None,
                model_route_id: None,
            })
        })
    }
}

fn scope() -> RepositoryScope {
    RepositoryScope::new(
        WorkspaceId::parse("workspace-cost").unwrap(),
        ProjectId::parse("project-cost").unwrap(),
        Some(ExecutionId::parse("execution-cost").unwrap()),
    )
}

fn actor() -> PersistedActor {
    PersistedActor::new(
        PersistedActorType::System,
        ActorId::parse("system-cost").unwrap(),
    )
}

fn opener(directory: std::path::PathBuf) -> StoreOpen {
    Arc::new(move || {
        LocalEventRepository::open(&directory, Arc::new(FixedClock), Arc::new(Ids::default()))
    })
}

fn node() -> GraphNode {
    let mut properties = BTreeMap::new();
    properties.insert(
        "agent".to_owned(),
        serde_json::json!({"ephemeral": {"purpose": "p", "instructions": "i"}}),
    );
    GraphNode {
        node_type: NodeType::Agent,
        name: "n".to_owned(),
        objective: "step".to_owned(),
        optionality: Optionality::Required,
        properties,
    }
}

fn chain() -> GraphSpec {
    let ids: Vec<String> = (0..NODES).map(|index| format!("n{index}")).collect();
    GraphSpec {
        entrypoints: vec![ids[0].clone()],
        nodes: ids.iter().map(|id| (id.clone(), node())).collect(),
        edges: ids
            .windows(2)
            .map(|pair| GraphEdge {
                id: format!("{}-to-{}", pair[0], pair[1]),
                from: pair[0].clone(),
                to: pair[1].clone(),
                edge_type: EdgeType::Control,
                payload_schema: None,
                condition: None,
                on_false: None,
                on_unknown: None,
                bindings: BTreeMap::new(),
                priority: None,
            })
            .collect(),
        budgets: GraphBudgets {
            max_parallel_model_calls: Some(1),
            ..GraphBudgets::default()
        },
        policies: Vec::new(),
        completion: serde_json::Value::Null,
    }
}

#[test]
fn a_drive_folds_each_event_a_bounded_number_of_times() {
    let directory = tempfile::tempdir().unwrap();
    let execution_id = OpaqueId::parse("execution-cost").unwrap();
    opener(directory.path().to_path_buf())()
        .unwrap()
        .append_atomic(
            &PreparedAppend::new(
                scope(),
                OpaqueId::parse(STREAM).unwrap(),
                1,
                vec![NewEvent::new(
                    OpaqueId::parse("cost-started").unwrap(),
                    actor(),
                    Sensitivity::Internal,
                    EventKind::ExecutionStarted(ExecutionStarted {
                        execution_id: execution_id.clone(),
                        graph_version: 1,
                        graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
                        mode: ExecutionMode::Autopilot,
                    }),
                    vec![],
                    vec![],
                )],
                vec![],
                vec![],
            )
            .unwrap(),
        )
        .unwrap();
    let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(None::<ImmediateCancelRequest>);
    // Multi-thread: the store's blocking lock runs on spawn_blocking threads.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();

    let before = graphhelm_events::projection_events_folded();
    let projection = runtime
        .block_on(drive_to_quiescence_async(
            opener(directory.path().to_path_buf()),
            Arc::new(EvidenceProtector::new(Keys::default())),
            Arc::new(Ids::default()),
            scope(),
            OpaqueId::parse(STREAM).unwrap(),
            execution_id,
            chain(),
            Arc::new(Succeeds),
            actor(),
            std::collections::BTreeSet::new(),
            actor(),
            cancel_rx,
            None,
            None,
        ))
        .unwrap();
    let folded = graphhelm_events::projection_events_folded() - before;
    assert_eq!(
        projection.simulation_status,
        Some(SimulationStatus::Completed),
        "HARNESS-BROKE: the chain did not complete, so the count measures a different drive"
    );
    let length = opener(directory.path().to_path_buf())()
        .unwrap()
        .read_replay_stream(&scope(), STREAM)
        .unwrap()
        .len() as u64;
    println!("fold-cost nodes={NODES} stream_length={length} events_folded={folded}");
    assert!(
        folded <= 3 * length,
        "the drive folded {folded} events for a {length}-event stream: more than 3x its length \
         means the driver re-folds history per reread"
    );
}
