//! The port executor's contract: classification is closed, outcomes are honest (delegated to
//! the 05b taxonomy, never a second mapping), free-form material seals and never rides a
//! summary, and an empty reply retries rather than parking or lying.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use graphhelm_gateway::call::{ModelCall, ModelReply, Usage};
use graphhelm_gateway::taxonomy::GatewayError;
use graphhelm_protocols::{NodeOutcome, NodeType};
use graphhelm_runtime::classify::{NodeWorkKind, work_kind};
use graphhelm_runtime::executor::{AsyncNodeExecutor, ExecutorRefusal, NodeWork, PortExecutor};
use graphhelm_runtime::ports::{ModelPort, ToolPort, ToolPortResult, ToolStreams};
use graphhelm_runtime::prompt::AssembledPrompt;
use graphhelm_tool_broker::call::{ShellAction, ToolCall};
use graphhelm_tool_broker::effect::IsolationTier;
use graphhelm_tool_broker::lease::{Capability, ToolLease};
use graphhelm_tool_broker::record::{ToolCallRecord, ToolDisposition, digest_hex};

fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a runtime")
        .block_on(future)
}

/// A scripted model port: returns the configured result and counts calls.
struct FakeModelPort {
    result: Result<ModelReply, GatewayError>,
    calls: AtomicUsize,
}

impl ModelPort for FakeModelPort {
    fn call<'a>(
        &'a self,
        _route_id: &'a str,
        _call: &'a ModelCall,
    ) -> Pin<Box<dyn Future<Output = Result<ModelReply, GatewayError>> + Send + 'a>> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let result = self.result.clone();
        Box::pin(async move { result })
    }
}

/// A scripted tool port: returns the configured record and streams.
struct FakeToolPort {
    disposition: ToolDisposition,
    reuse: Option<graphhelm_runtime::ports::ReuseSummary>,
}

impl ToolPort for FakeToolPort {
    fn invoke<'a>(
        &'a self,
        _call: &'a ToolCall,
        _lease: &'a ToolLease,
        _actor: &'a str,
    ) -> Pin<Box<dyn Future<Output = ToolPortResult> + Send + 'a>> {
        let record = ToolCallRecord {
            tool: "shell".to_owned(),
            action: "run".to_owned(),
            actor: "agent-runtime".to_owned(),
            tier: IsolationTier::Tier1,
            disposition: self.disposition.clone(),
            stdout_sha256: digest_hex(b"TOOL-STDOUT-SENTINEL"),
            stdout_bytes: 20,
            stderr_sha256: digest_hex(b""),
            stderr_bytes: 0,
            truncated: false,
            reused: self
                .reuse
                .as_ref()
                .is_some_and(|summary| summary.decision == graphhelm_protocols::ReuseOutcome::Hit),
        };
        let reuse = self.reuse.clone();
        Box::pin(async move {
            ToolPortResult {
                record,
                streams: ToolStreams {
                    stdout: b"TOOL-STDOUT-SENTINEL".to_vec(),
                    stderr: Vec::new(),
                },
                reuse,
            }
        })
    }
}

fn prompt() -> AssembledPrompt {
    AssembledPrompt {
        system: "Purpose: test\n".to_owned(),
        task: "do the thing".to_owned(),
        sha256: "0".repeat(64),
    }
}

fn cognitive_work() -> NodeWork {
    NodeWork {
        execution_id: "exec-1".to_owned(),
        node_id: "implement".to_owned(),
        attempt: 1,
        prompt: prompt(),
        kind: NodeWorkKind::Cognitive,
        tool_call: None,
        gate_check: None,
        judge: None,
    }
}

fn tool_work() -> NodeWork {
    NodeWork {
        execution_id: "exec-1".to_owned(),
        node_id: "tests".to_owned(),
        attempt: 1,
        prompt: prompt(),
        kind: NodeWorkKind::Tool,
        tool_call: Some(ToolCall::Shell(ShellAction {
            program: "git".to_owned(),
            arguments: vec!["status".to_owned()],
        })),
        gate_check: None,
        judge: None,
    }
}

fn lease() -> ToolLease {
    ToolLease {
        actor: "agent-runtime".to_owned(),
        capabilities: [Capability::ShellExecute].into_iter().collect(),
        programs: ["git".to_owned()].into_iter().collect(),
    }
}

fn executor(model: Result<ModelReply, GatewayError>, disposition: ToolDisposition) -> PortExecutor {
    PortExecutor {
        model: Arc::new(FakeModelPort {
            result: model,
            calls: AtomicUsize::new(0),
        }),
        tools: Arc::new(FakeToolPort {
            disposition,
            reuse: None,
        }),
        route_id: "claude_subscription".to_owned(),
        lease: lease(),
        actor: "agent-runtime".to_owned(),
    }
}

fn executor_with_reuse(
    disposition: ToolDisposition,
    reuse: Option<graphhelm_runtime::ports::ReuseSummary>,
) -> PortExecutor {
    PortExecutor {
        model: Arc::new(FakeModelPort {
            result: Ok(reply("unused")),
            calls: AtomicUsize::new(0),
        }),
        tools: Arc::new(FakeToolPort { disposition, reuse }),
        route_id: "claude_subscription".to_owned(),
        lease: lease(),
        actor: "agent-runtime".to_owned(),
    }
}

fn reply(text: &str) -> ModelReply {
    ModelReply {
        text: text.to_owned(),
        usage: Usage {
            input_tokens: Some(12),
            output_tokens: Some(5),
        },
    }
}

#[test]
fn node_types_classify_cognitive_or_tool_and_nothing_else_dispatches() {
    for cognitive in [
        NodeType::Agent,
        NodeType::Planner,
        NodeType::Classifier,
        NodeType::Evaluator,
    ] {
        assert_eq!(work_kind(&cognitive), Ok(NodeWorkKind::Cognitive));
    }
    assert_eq!(work_kind(&NodeType::Tool), Ok(NodeWorkKind::Tool));
    // M06: Gate left this list — it classifies as deterministic GateCheck work now; the
    // whole-table pin lives in gate_nodes.rs.
    assert_eq!(work_kind(&NodeType::Gate), Ok(NodeWorkKind::GateCheck));
    // A refusal is a refusal — never laundered into an outcome the fold would record: the
    // driver simply does not dispatch what the executor refuses.
    for unsupported in [
        NodeType::Fork,
        NodeType::Join,
        NodeType::HumanDecision,
        NodeType::Timer,
        NodeType::Trigger,
        NodeType::Subgraph,
        NodeType::Materializer,
        NodeType::Deploy,
        NodeType::Rollback,
        NodeType::ArtifactTransform,
    ] {
        assert_eq!(work_kind(&unsupported), Err(ExecutorRefusal::Unsupported));
    }
}

#[test]
fn a_model_reply_is_succeeded_with_sealed_reply_evidence() {
    let executor = executor(
        Ok(reply("REPLY-SENTINEL")),
        ToolDisposition::Completed { exit_code: 0 },
    );
    let outcome = block_on(executor.execute(&cognitive_work())).unwrap();
    assert_eq!(outcome.outcome, NodeOutcome::Succeeded);
    let sealed = outcome
        .sealables
        .iter()
        .find(|sealable| sealable.local_ref_suffix == "reply")
        .expect("a sealed reply");
    assert_eq!(sealed.media_type, "application/json");
    assert!(
        String::from_utf8_lossy(&sealed.bytes).contains("REPLY-SENTINEL"),
        "the reply content seals"
    );
    assert_eq!(outcome.summary.input_tokens, Some(12));
    assert_eq!(outcome.summary.output_tokens, Some(5));
    let summary_json = serde_json::to_string(&outcome.summary).unwrap();
    assert!(
        !summary_json.contains("REPLY-SENTINEL"),
        "free-form content must never ride the summary"
    );
}

#[test]
fn gateway_errors_map_through_outcome_for_error_verbatim() {
    // The §12 wait rule is already pinned by 05b's outcome_for_error; the executor DELEGATES
    // to it rather than maintaining a second mapping (the sabotage proves the delegation).
    for (error, expected) in [
        (GatewayError::QuotaExhausted, NodeOutcome::NeedsCapacity),
        (GatewayError::RateLimited, NodeOutcome::NeedsCapacity),
        (GatewayError::Timeout, NodeOutcome::RetryableFailure),
        (GatewayError::PolicyDenied, NodeOutcome::TerminalFailure),
        (GatewayError::Cancelled, NodeOutcome::Cancelled),
    ] {
        let executor = executor(Err(error), ToolDisposition::Completed { exit_code: 0 });
        let outcome = block_on(executor.execute(&cognitive_work())).unwrap();
        assert_eq!(outcome.outcome, expected, "{error:?}");
    }
}

#[test]
fn a_tool_record_maps_by_disposition_and_seals_record_plus_streams() {
    for (disposition, expected) in [
        (
            ToolDisposition::Completed { exit_code: 0 },
            NodeOutcome::Succeeded,
        ),
        (
            ToolDisposition::Completed { exit_code: 101 },
            NodeOutcome::RetryableFailure,
        ),
        (ToolDisposition::TimedOut, NodeOutcome::RetryableFailure),
        (
            ToolDisposition::Denied {
                rule: "program_denied".to_owned(),
            },
            // A lease refusal will not heal by retrying the same call.
            NodeOutcome::TerminalFailure,
        ),
        (
            ToolDisposition::HostError {
                code: "GHTOOL003_PREPARE".to_owned(),
            },
            NodeOutcome::RetryableFailure,
        ),
    ] {
        let executor = executor(Ok(reply("unused")), disposition.clone());
        let outcome = block_on(executor.execute(&tool_work())).unwrap();
        assert_eq!(outcome.outcome, expected, "{disposition:?}");
        let suffixes: Vec<&str> = outcome
            .sealables
            .iter()
            .map(|sealable| sealable.local_ref_suffix)
            .collect();
        assert!(suffixes.contains(&"record"), "the record seals");
        assert!(suffixes.contains(&"stdout"), "stdout seals");
        assert!(suffixes.contains(&"stderr"), "stderr seals");
        let record = outcome
            .sealables
            .iter()
            .find(|sealable| sealable.local_ref_suffix == "record")
            .unwrap();
        assert_eq!(record.media_type, "application/json");
    }
}

#[test]
fn an_empty_reply_is_a_retryable_failure_never_a_success_and_never_a_park() {
    // Revised by the plan review: NeedsInput would park the node waiting for input nothing in
    // 05d can deliver — an operational dead end. An empty reply from a live provider is a
    // provider defect: retryable, bounded by the attempt machinery, landing in Blocked for an
    // owner decision when persistent. Never Succeeded either way.
    let executor = executor(Ok(reply("")), ToolDisposition::Completed { exit_code: 0 });
    let outcome = block_on(executor.execute(&cognitive_work())).unwrap();
    assert_eq!(outcome.outcome, NodeOutcome::RetryableFailure);
}

// ---------------------------------------------------------------------------------------------
// Task 6: evidence-before-append — outcomes, their sealed material, and the reuse ledger land
// in one atomic append, or nothing lands at all.
// ---------------------------------------------------------------------------------------------

use std::collections::BTreeMap as StdBTreeMap;
use std::sync::Mutex;
use std::sync::atomic::AtomicU64;

use chrono::{TimeZone, Utc};
use graphhelm_events::{
    AuthenticateRequest, AuthenticationTag, EvidenceProtector, KeyError, KeyProvider,
    KeyProviderMetadata, LocalEventRepository, PreparedAppend, RepositoryFuture, RevocationReceipt,
    RevokeKeyRequest, SecretBytes, VerifyAuthenticationRequest, WrapKeyRequest, WrappedKey, replay,
};
use graphhelm_protocols::{
    ActorId, Clock, EventKind, ExecutionId, ExecutionMode, ExecutionStarted, IdGenerator, NewEvent,
    OpaqueId, PersistedActor, PersistedActorType, ProjectId, RawSha256, RepositoryScope,
    ReuseKeyComponent, ReuseOutcome, Sensitivity, WireHash, WorkspaceId,
};
use graphhelm_runtime::driver::record_outcome_with_evidence;
use graphhelm_runtime::executor::{Sealable, WorkOutcome, WorkSummary};
use graphhelm_runtime::ports::ReuseSummary;

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
    use sha2::Digest as _;
    RawSha256::parse(hex::encode(sha2::Sha256::digest(bytes))).unwrap()
}

/// The in-memory key provider `core/events/tests/evidence_crypto.rs` established as the test
/// bootstrap: wrap stores the key, unwrap returns it, authenticate digests honestly.
#[derive(Default)]
struct InMemoryKeyProvider {
    keys: Mutex<StdBTreeMap<String, Vec<u8>>>,
    fail_wrap: bool,
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
            if self.fail_wrap {
                return Err(KeyError::Unavailable);
            }
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

const DRIVER_STREAM: &str = "stream-driver-test";
const NODE: &str = "implement";

fn driver_scope() -> RepositoryScope {
    RepositoryScope::new(
        WorkspaceId::parse("workspace-driver").unwrap(),
        ProjectId::parse("project-driver").unwrap(),
        Some(ExecutionId::parse("execution-driver").unwrap()),
    )
}

fn driver_actor() -> PersistedActor {
    PersistedActor::new(
        PersistedActorType::System,
        ActorId::parse("system-driver").unwrap(),
    )
}

fn plain_event(key: &str, kind: EventKind) -> NewEvent {
    NewEvent::new(
        OpaqueId::parse(key).unwrap(),
        driver_actor(),
        Sensitivity::Internal,
        kind,
        vec![],
        vec![],
    )
}

/// A repository whose stream holds a started execution with `NODE` already `Running`:
/// Approved (`Draft -> Ready`), Started (`-> Queued`), Started (`-> Running`) — each through
/// the projection so the bootstrap can never disagree with `apply_transition`.
fn running_node_repository(directory: &std::path::Path) -> (LocalEventRepository, OpaqueId) {
    let repository = LocalEventRepository::open(
        directory,
        Arc::new(FixedClock),
        Arc::new(SequenceIds::default()),
    )
    .unwrap();
    let execution_id = OpaqueId::parse("execution-driver").unwrap();
    let scope = driver_scope();
    let stream = OpaqueId::parse(DRIVER_STREAM).unwrap();
    let mut sequence = 1;
    let mut append = |events: Vec<NewEvent>| {
        let request = PreparedAppend::new(
            scope.clone(),
            stream.clone(),
            sequence,
            events,
            vec![],
            vec![],
        )
        .unwrap();
        sequence += u64::try_from(repository.append_atomic(&request).unwrap().len()).unwrap();
    };
    append(vec![plain_event(
        "boot-started",
        EventKind::ExecutionStarted(ExecutionStarted {
            execution_id: execution_id.clone(),
            graph_version: 1,
            graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
            mode: ExecutionMode::Autopilot,
        }),
    )]);
    for (key, outcome) in [
        ("boot-approve", NodeOutcome::Approved),
        ("boot-queue", NodeOutcome::Started),
        ("boot-run", NodeOutcome::Started),
    ] {
        let history = repository
            .read_replay_stream(&scope, DRIVER_STREAM)
            .unwrap();
        let projection = replay(&scope, DRIVER_STREAM, &history).unwrap();
        let current = projection
            .node_states
            .get(NODE)
            .copied()
            .unwrap_or(graphhelm_protocols::NodeState::Draft);
        let next_state =
            graphhelm_execution::apply_transition(&graphhelm_execution::TransitionRequest {
                current,
                outcome,
                attempts: projection.node_attempts.get(NODE).copied().unwrap_or(0),
                identical_outcomes: projection.identical_outcomes_for(NODE, outcome),
            })
            .unwrap();
        append(vec![plain_event(
            key,
            EventKind::NodeOutcomeRecorded(graphhelm_protocols::NodeOutcomeRecorded {
                execution_id: execution_id.clone(),
                node_id: OpaqueId::parse(NODE).unwrap(),
                outcome,
                next_state,
                reason: None,
            }),
        )]);
    }
    (repository, execution_id)
}

fn succeeded_work(reuse: Option<ReuseSummary>) -> WorkOutcome {
    WorkOutcome {
        outcome: NodeOutcome::Succeeded,
        reason: None,
        sealables: vec![
            Sealable {
                local_ref_suffix: "reply",
                media_type: "application/json",
                bytes: br#"{"text":"REPLY-SENTINEL"}"#.to_vec(),
            },
            Sealable {
                local_ref_suffix: "stdout",
                media_type: "text/plain",
                bytes: b"STREAM-SENTINEL".to_vec(),
            },
        ],
        summary: WorkSummary {
            input_tokens: Some(12),
            output_tokens: Some(5),
            exit_code: None,
        },
        reuse,
        gate_verdict: None,
    }
}

fn hit_summary() -> ReuseSummary {
    ReuseSummary {
        decision: ReuseOutcome::Hit,
        forced_reason: None,
        freshness_class: None,
        key_components: vec![
            ReuseKeyComponent::ToolVersion,
            ReuseKeyComponent::CanonicalInput,
            ReuseKeyComponent::LeaseScope,
            ReuseKeyComponent::SourceSnapshot,
        ],
        key_digest: WireHash::parse(format!("sha256:{}", "b".repeat(64))).unwrap(),
        evidence_ref: None,
        provenance_erased: false,
    }
}

#[test]
fn an_outcome_and_its_evidence_land_in_one_atomic_append() {
    let directory = tempfile::tempdir().unwrap();
    let (repository, execution_id) = running_node_repository(directory.path());
    let scope = driver_scope();
    let stream = OpaqueId::parse(DRIVER_STREAM).unwrap();
    let protector = EvidenceProtector::new(InMemoryKeyProvider::default());
    let ids = SequenceIds::default();

    let history = repository
        .read_replay_stream(&scope, DRIVER_STREAM)
        .unwrap();
    let attempt = replay(&scope, DRIVER_STREAM, &history)
        .unwrap()
        .node_attempts
        .get(NODE)
        .copied()
        .unwrap_or(0);

    let recorded_outcome = block_on(record_outcome_with_evidence(
        &repository,
        &protector,
        &ids,
        &scope,
        &stream,
        &execution_id,
        &driver_actor(),
        NODE,
        &succeeded_work(None),
    ))
    .unwrap();
    assert_eq!(
        recorded_outcome.next_state,
        graphhelm_protocols::NodeState::Succeeded
    );

    let history = repository
        .read_replay_stream(&scope, DRIVER_STREAM)
        .unwrap();
    let envelope = history.last().unwrap();
    let EventKind::NodeOutcomeRecorded(recorded) = &envelope.kind else {
        panic!("the last event is the recorded outcome");
    };
    assert_eq!(recorded.outcome, NodeOutcome::Succeeded);

    // Deterministically derived, attempt-scoped references — retries never collide.
    let expected: Vec<String> = ["reply", "stdout"]
        .iter()
        .map(|suffix| format!("exec-{execution_id}-{NODE}-a{attempt}-{suffix}"))
        .collect();
    let refs = &envelope.evidence_refs;
    assert_eq!(refs.len(), 2, "one reference per sealable");
    // The local store exposes availability, not a sealed read (`EvidenceRepository` is the
    // Postgres adapter's surface — reported discrepancy); the driver returns what it sealed,
    // so the open round-trip runs against the exact appended items.
    let originals = succeeded_work(None);
    for ((reference, expected_id), original) in refs.iter().zip(&expected).zip(&originals.sealables)
    {
        assert_eq!(reference.evidence_id().as_str(), expected_id);
        assert!(
            repository
                .evidence_exists(&scope, reference.evidence_id())
                .unwrap(),
            "sealed evidence is available for {expected_id}"
        );
        let sealed = recorded_outcome
            .sealed
            .iter()
            .find(|item| item.reference().evidence_id() == reference.evidence_id())
            .expect("the driver returns each sealed item");
        let opened = block_on(graphhelm_events::EvidenceOpener::open(
            &protector,
            scope.clone(),
            sealed,
        ))
        .unwrap();
        assert!(
            opened.expose(|bytes| bytes == original.bytes.as_slice()),
            "round-trips the original bytes"
        );
    }

    // No sealable byte rides the event: the envelope serializes clean of both sentinels.
    let serialized = serde_json::to_string(envelope).unwrap();
    assert!(!serialized.contains("REPLY-SENTINEL"));
    assert!(!serialized.contains("STREAM-SENTINEL"));
}

#[test]
fn a_sealing_failure_appends_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let (repository, execution_id) = running_node_repository(directory.path());
    let scope = driver_scope();
    let stream = OpaqueId::parse(DRIVER_STREAM).unwrap();
    let protector = EvidenceProtector::new(InMemoryKeyProvider {
        fail_wrap: true,
        ..Default::default()
    });
    let ids = SequenceIds::default();

    let before = repository.next_sequence(&scope, DRIVER_STREAM).unwrap();
    let result = block_on(record_outcome_with_evidence(
        &repository,
        &protector,
        &ids,
        &scope,
        &stream,
        &execution_id,
        &driver_actor(),
        NODE,
        &succeeded_work(None),
    ));
    assert!(result.is_err(), "a sealing failure is a driver error");
    let after = repository.next_sequence(&scope, DRIVER_STREAM).unwrap();
    assert_eq!(
        before, after,
        "evidence-before-append: nothing was appended"
    );
}

#[test]
fn a_reused_tool_record_produces_a_reuse_decision_beside_its_outcome() {
    let directory = tempfile::tempdir().unwrap();
    let (repository, execution_id) = running_node_repository(directory.path());
    let scope = driver_scope();
    let stream = OpaqueId::parse(DRIVER_STREAM).unwrap();
    let protector = EvidenceProtector::new(InMemoryKeyProvider::default());
    let ids = SequenceIds::default();

    // The port propagates the host's summary: a fake ToolPort scripted as a hit produces a
    // WorkOutcome that carries it (the Task 5 executor is the propagation path).
    let executor = executor_with_reuse(
        ToolDisposition::Completed { exit_code: 0 },
        Some(hit_summary()),
    );
    let work_outcome = block_on(executor.execute(&tool_work())).unwrap();
    assert!(
        work_outcome.reuse.is_some(),
        "the executor propagates reuse"
    );

    block_on(record_outcome_with_evidence(
        &repository,
        &protector,
        &ids,
        &scope,
        &stream,
        &execution_id,
        &driver_actor(),
        NODE,
        &work_outcome,
    ))
    .unwrap();

    let history = repository
        .read_replay_stream(&scope, DRIVER_STREAM)
        .unwrap();
    let decisions: Vec<_> = history
        .iter()
        .filter_map(|envelope| match &envelope.kind {
            EventKind::ReuseDecision(decision) => Some(decision),
            _ => None,
        })
        .collect();
    assert_eq!(decisions.len(), 1, "exactly one ReuseDecision for the hit");
    let decision = decisions[0];
    assert_eq!(decision.decision, ReuseOutcome::Hit);
    assert_eq!(
        decision
            .node_id
            .as_ref()
            .map(graphhelm_protocols::OpaqueId::as_str),
        Some(NODE),
        "the executor's decision names its node"
    );
    assert_eq!(decision.execution_id, execution_id);
    assert!(
        decision
            .evidence_ref
            .as_ref()
            .is_some_and(|id| id.as_str().ends_with("-record")),
        "the decision points at the sealed record"
    );

    // Replay-stable: the fold's explicit no-op ledger arm accepts the event twice over.
    let once = replay(&scope, DRIVER_STREAM, &history).unwrap();
    let twice = replay(&scope, DRIVER_STREAM, &history).unwrap();
    assert_eq!(once, twice);
}

#[test]
fn a_fresh_tool_run_records_a_miss_decision() {
    let directory = tempfile::tempdir().unwrap();
    let (repository, execution_id) = running_node_repository(directory.path());
    let scope = driver_scope();
    let stream = OpaqueId::parse(DRIVER_STREAM).unwrap();
    let protector = EvidenceProtector::new(InMemoryKeyProvider::default());
    let ids = SequenceIds::default();

    let miss = ReuseSummary {
        decision: ReuseOutcome::Miss,
        ..hit_summary()
    };
    let executor = executor_with_reuse(ToolDisposition::Completed { exit_code: 0 }, Some(miss));
    let work_outcome = block_on(executor.execute(&tool_work())).unwrap();

    block_on(record_outcome_with_evidence(
        &repository,
        &protector,
        &ids,
        &scope,
        &stream,
        &execution_id,
        &driver_actor(),
        NODE,
        &work_outcome,
    ))
    .unwrap();

    let history = repository
        .read_replay_stream(&scope, DRIVER_STREAM)
        .unwrap();
    let decisions: Vec<_> = history
        .iter()
        .filter_map(|envelope| match &envelope.kind {
            EventKind::ReuseDecision(decision) => Some(decision),
            _ => None,
        })
        .collect();
    assert_eq!(decisions.len(), 1, "the ledger records both directions");
    assert_eq!(decisions[0].decision, ReuseOutcome::Miss);
}

// ---------------------------------------------------------------------------------------------
// Task 8: the async driver — 04f sequencing reproduced, bounded concurrency, immediate-stop.
// ---------------------------------------------------------------------------------------------

use graphhelm_execution::{ResumeError, resume_preconditions};
use graphhelm_protocols::{
    EdgeType, GraphBudgets, GraphEdge, GraphNode, GraphSpec, NodeState, Optionality,
};
use graphhelm_runtime::driver::{StoreOpen, drive_to_quiescence_async};

fn agent_graph_node(objective: &str) -> GraphNode {
    let mut properties = std::collections::BTreeMap::new();
    properties.insert(
        "agent".to_owned(),
        serde_json::json!({"ephemeral": {"purpose": "p", "instructions": "i"}}),
    );
    GraphNode {
        node_type: NodeType::Agent,
        name: "n".to_owned(),
        objective: objective.to_owned(),
        optionality: Optionality::Required,
        properties,
    }
}

fn spec_with(nodes: Vec<(&str, GraphNode)>, edges: Vec<(&str, &str)>, parallel: u64) -> GraphSpec {
    let mut spec = GraphSpec {
        entrypoints: nodes
            .first()
            .map(|(id, _)| vec![(*id).to_owned()])
            .unwrap_or_default(),
        nodes: nodes
            .into_iter()
            .map(|(id, node)| (id.to_owned(), node))
            .collect(),
        edges: Vec::new(),
        budgets: GraphBudgets {
            max_parallel_model_calls: Some(parallel),
            ..GraphBudgets::default()
        },
        policies: Vec::new(),
        completion: serde_json::Value::Null,
    };
    for (from, to) in edges {
        spec.edges.push(GraphEdge {
            id: format!("{from}-to-{to}"),
            from: from.to_owned(),
            to: to.to_owned(),
            edge_type: EdgeType::Control,
            payload_schema: None,
            condition: None,
            on_false: None,
            on_unknown: None,
            bindings: std::collections::BTreeMap::new(),
            priority: None,
        });
    }
    spec
}

/// A fresh store holding only `ExecutionStarted` — the async driver approves and dispatches
/// everything else itself, exactly as 04f does.
fn started_repository(directory: &std::path::Path) -> OpaqueId {
    let repository = LocalEventRepository::open(
        directory,
        Arc::new(FixedClock),
        Arc::new(SequenceIds::default()),
    )
    .unwrap();
    let execution_id = OpaqueId::parse("execution-driver").unwrap();
    let request = PreparedAppend::new(
        driver_scope(),
        OpaqueId::parse(DRIVER_STREAM).unwrap(),
        1,
        vec![plain_event(
            "async-started",
            EventKind::ExecutionStarted(ExecutionStarted {
                execution_id: execution_id.clone(),
                graph_version: 1,
                graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
                mode: ExecutionMode::Autopilot,
            }),
        )],
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

/// A gated model port for the concurrency test: counts in-flight calls, records the maximum,
/// and resolves only when the test releases it — wall-clock free.
struct GatedModelPort {
    current: AtomicUsize,
    max_seen: AtomicUsize,
    total: AtomicUsize,
    release: tokio::sync::Semaphore,
}

impl GatedModelPort {
    fn new() -> Self {
        Self {
            current: AtomicUsize::new(0),
            max_seen: AtomicUsize::new(0),
            total: AtomicUsize::new(0),
            release: tokio::sync::Semaphore::new(0),
        }
    }
}

impl ModelPort for GatedModelPort {
    fn call<'a>(
        &'a self,
        _route_id: &'a str,
        _call: &'a ModelCall,
    ) -> Pin<Box<dyn Future<Output = Result<ModelReply, GatewayError>> + Send + 'a>> {
        Box::pin(async move {
            let now = self.current.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_seen.fetch_max(now, Ordering::SeqCst);
            self.total.fetch_add(1, Ordering::SeqCst);
            let permit = self.release.acquire().await.expect("release semaphore");
            permit.forget();
            self.current.fetch_sub(1, Ordering::SeqCst);
            Ok(reply("GATED-REPLY"))
        })
    }
}

/// A model port that never resolves until cancelled; `cancel_all` releases it and records
/// that the hook ran.
struct HangingModelPort {
    called: AtomicUsize,
    cancelled: std::sync::atomic::AtomicBool,
    wake: tokio::sync::Notify,
}

impl HangingModelPort {
    fn new() -> Self {
        Self {
            called: AtomicUsize::new(0),
            cancelled: std::sync::atomic::AtomicBool::new(false),
            wake: tokio::sync::Notify::new(),
        }
    }
}

impl ModelPort for HangingModelPort {
    fn call<'a>(
        &'a self,
        _route_id: &'a str,
        _call: &'a ModelCall,
    ) -> Pin<Box<dyn Future<Output = Result<ModelReply, GatewayError>> + Send + 'a>> {
        Box::pin(async move {
            self.called.fetch_add(1, Ordering::SeqCst);
            self.wake.notified().await;
            Ok(reply("TOO-LATE"))
        })
    }

    fn cancel_all(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.wake.notify_waiters();
    }
}

/// A tool port that spawns a real sleeping child and kills it on cancel — the pid-liveness
/// half of the immediate-stop contract.
struct KillingToolPort {
    child: std::sync::Mutex<Option<std::process::Child>>,
    killed: std::sync::atomic::AtomicBool,
    wake: tokio::sync::Notify,
}

impl KillingToolPort {
    fn new() -> Self {
        Self {
            child: std::sync::Mutex::new(None),
            killed: std::sync::atomic::AtomicBool::new(false),
            wake: tokio::sync::Notify::new(),
        }
    }

    fn spawn_sleeper() -> std::process::Child {
        if cfg!(windows) {
            std::process::Command::new("ping")
                .args(["-n", "60", "127.0.0.1"])
                .stdout(std::process::Stdio::null())
                .spawn()
                .expect("a sleeping child")
        } else {
            std::process::Command::new("sleep")
                .arg("60")
                .spawn()
                .expect("a sleeping child")
        }
    }
}

impl ToolPort for KillingToolPort {
    fn invoke<'a>(
        &'a self,
        _call: &'a ToolCall,
        _lease: &'a ToolLease,
        _actor: &'a str,
    ) -> Pin<Box<dyn Future<Output = ToolPortResult> + Send + 'a>> {
        Box::pin(async move {
            *self.child.lock().unwrap() = Some(Self::spawn_sleeper());
            self.wake.notified().await;
            unreachable!("the invoke future is aborted on cancel, never completed");
        })
    }

    fn cancel_all(&self) {
        if let Some(child) = self.child.lock().unwrap().as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.killed.store(true, Ordering::SeqCst);
    }
}

fn port_executor_with(model: Arc<dyn ModelPort>, tools: Arc<dyn ToolPort>) -> PortExecutor {
    PortExecutor {
        model,
        tools,
        route_id: "claude_subscription".to_owned(),
        lease: lease(),
        actor: "agent-runtime".to_owned(),
    }
}

fn multi_thread_runtime() -> tokio::runtime::Runtime {
    // Multi-thread by design: the store's blocking OS lock runs on spawn_blocking threads,
    // and a current-thread runtime plus a blocking store call is a self-deadlock.
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("a runtime")
}

#[test]
fn the_async_driver_reproduces_the_04f_sequencing_on_a_happy_chain() {
    let directory = tempfile::tempdir().unwrap();
    let execution_id = started_repository(directory.path());
    let spec = spec_with(
        vec![
            ("a", agent_graph_node("first")),
            ("b", agent_graph_node("second")),
        ],
        vec![("a", "b")],
        1,
    );
    let model: Arc<FakeModelPort> = Arc::new(FakeModelPort {
        result: Ok(reply("CHAIN-REPLY")),
        calls: AtomicUsize::new(0),
    });
    let executor = Arc::new(port_executor_with(
        model,
        Arc::new(FakeToolPort {
            disposition: ToolDisposition::Completed { exit_code: 0 },
            reuse: None,
        }),
    ));
    let protector = Arc::new(EvidenceProtector::new(InMemoryKeyProvider::default()));
    let ids = Arc::new(SequenceIds::default());
    let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

    let runtime = multi_thread_runtime();
    let projection = runtime
        .block_on(drive_to_quiescence_async(
            opener(directory.path().to_path_buf()),
            protector.clone(),
            ids,
            driver_scope(),
            OpaqueId::parse(DRIVER_STREAM).unwrap(),
            execution_id.clone(),
            spec,
            executor,
            driver_actor(),
            cancel_rx,
            None,
        ))
        .unwrap();
    assert_eq!(
        projection.simulation_status,
        Some(graphhelm_protocols::SimulationStatus::Completed)
    );

    // The 04f stream shape: approvals, two Started hops per dispatch, outcomes with
    // next_state from apply_transition, completion.
    let store = opener(directory.path().to_path_buf())().unwrap();
    let history = store
        .read_replay_stream(&driver_scope(), DRIVER_STREAM)
        .unwrap();
    let story: Vec<String> = history
        .iter()
        .map(|envelope| match &envelope.kind {
            EventKind::ExecutionStarted(_) => "started".to_owned(),
            EventKind::NodeOutcomeRecorded(record) => format!(
                "{}:{:?}->{:?}",
                record.node_id.as_str(),
                record.outcome,
                record.next_state
            ),
            EventKind::ExecutionCompleted(_) => "completed".to_owned(),
            other => format!("unexpected:{other:?}"),
        })
        .collect();
    assert_eq!(
        story,
        vec![
            "started",
            "a:Approved->Ready",
            "b:Approved->Ready",
            "a:Started->Queued",
            "a:Started->Running",
            "a:Succeeded->Succeeded",
            "b:Started->Queued",
            "b:Started->Running",
            "b:Succeeded->Succeeded",
            "completed",
        ],
        "the async driver must reproduce the 04f sequencing exactly"
    );

    // The acceptance's replay clause: the full history replays identically twice.
    let once = replay(&driver_scope(), DRIVER_STREAM, &history).unwrap();
    let twice = replay(&driver_scope(), DRIVER_STREAM, &history).unwrap();
    assert_eq!(once, twice);
}

#[test]
fn max_parallel_dispatches_concurrently_and_respects_the_bound() {
    let directory = tempfile::tempdir().unwrap();
    let execution_id = started_repository(directory.path());
    // One root, three independent children, bound 2.
    let spec = spec_with(
        vec![
            ("root", agent_graph_node("root")),
            ("child-a", agent_graph_node("a")),
            ("child-b", agent_graph_node("b")),
            ("child-c", agent_graph_node("c")),
        ],
        vec![
            ("root", "child-a"),
            ("root", "child-b"),
            ("root", "child-c"),
        ],
        2,
    );
    let model = Arc::new(GatedModelPort::new());
    let executor = Arc::new(port_executor_with(
        model.clone(),
        Arc::new(FakeToolPort {
            disposition: ToolDisposition::Completed { exit_code: 0 },
            reuse: None,
        }),
    ));
    let protector = Arc::new(EvidenceProtector::new(InMemoryKeyProvider::default()));
    let ids = Arc::new(SequenceIds::default());
    let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

    let runtime = multi_thread_runtime();
    runtime.block_on(async {
        let driver = tokio::spawn(drive_to_quiescence_async(
            opener(directory.path().to_path_buf()),
            protector.clone(),
            ids,
            driver_scope(),
            OpaqueId::parse(DRIVER_STREAM).unwrap(),
            execution_id.clone(),
            spec,
            executor,
            driver_actor(),
            cancel_rx,
            None,
        ));
        // Gate on the port's own counters, never on sleeps: the root runs alone, then the
        // three children contend for the bound of 2.
        model.release.add_permits(1); // let the root through
        while model.current.load(Ordering::SeqCst) < 2 {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            model.current.load(Ordering::SeqCst),
            2,
            "the bound holds at the moment of saturation"
        );
        model.release.add_permits(3); // release the children
        let projection = driver.await.unwrap().unwrap();
        assert_eq!(
            projection.simulation_status,
            Some(graphhelm_protocols::SimulationStatus::Completed)
        );
        assert_eq!(model.total.load(Ordering::SeqCst), 4, "root + 3 children");
        assert!(
            model.max_seen.load(Ordering::SeqCst) <= 2,
            "never more than max_parallel in flight; saw {}",
            model.max_seen.load(Ordering::SeqCst)
        );
    });
}

#[test]
fn immediate_stop_interrupts_in_flight_work_and_blocks_it() {
    let directory = tempfile::tempdir().unwrap();
    let execution_id = started_repository(directory.path());
    let spec = spec_with(vec![("only", agent_graph_node("held"))], vec![], 1);
    let model = Arc::new(HangingModelPort::new());
    let executor = Arc::new(port_executor_with(
        model.clone(),
        Arc::new(FakeToolPort {
            disposition: ToolDisposition::Completed { exit_code: 0 },
            reuse: None,
        }),
    ));
    let protector = Arc::new(EvidenceProtector::new(InMemoryKeyProvider::default()));
    let ids = Arc::new(SequenceIds::default());
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

    let runtime = multi_thread_runtime();
    let projection = runtime.block_on(async {
        let driver = tokio::spawn(drive_to_quiescence_async(
            opener(directory.path().to_path_buf()),
            protector.clone(),
            ids,
            driver_scope(),
            OpaqueId::parse(DRIVER_STREAM).unwrap(),
            execution_id.clone(),
            spec,
            executor,
            driver_actor(),
            cancel_rx,
            None,
        ));
        while model.called.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
        cancel_tx.send(true).unwrap();
        driver.await.unwrap().unwrap()
    });

    assert!(
        model.cancelled.load(Ordering::SeqCst),
        "the port's cancel hook ran"
    );
    assert_eq!(
        projection.node_states.get("only"),
        Some(&NodeState::Blocked),
        "(Running, Interrupted) -> Blocked, the 04e arm"
    );
    assert_eq!(
        projection.simulation_status,
        Some(graphhelm_protocols::SimulationStatus::Paused),
        "execution_paused follows the interruption"
    );
    // The stream tail: Interrupted -> Blocked, then paused.
    let store = opener(directory.path().to_path_buf())().unwrap();
    let history = store
        .read_replay_stream(&driver_scope(), DRIVER_STREAM)
        .unwrap();
    let interrupted = history.iter().any(|envelope| {
        matches!(
            &envelope.kind,
            EventKind::NodeOutcomeRecorded(record)
                if record.outcome == NodeOutcome::Interrupted
                    && record.next_state == NodeState::Blocked
        )
    });
    assert!(interrupted, "the interruption is recorded, never silent");
    assert!(
        history
            .iter()
            .any(|envelope| matches!(&envelope.kind, EventKind::ExecutionPaused(_))),
        "execution_paused is appended"
    );
    // Resume afterwards refuses until the owner approves the interrupted node.
    assert_eq!(
        resume_preconditions(&projection, None),
        Err(ResumeError::UntriagedInterruption)
    );
}

#[test]
fn a_cancelled_tool_child_is_actually_dead() {
    let directory = tempfile::tempdir().unwrap();
    let execution_id = started_repository(directory.path());
    let mut tool_node = agent_graph_node("run the sleeper");
    tool_node.node_type = NodeType::Tool;
    tool_node.properties.insert(
        "tool".to_owned(),
        serde_json::json!({"call": {"tool": "shell", "program": "git", "arguments": ["status"]}}),
    );
    let spec = spec_with(vec![("tool-node", tool_node)], vec![], 1);
    let tools = Arc::new(KillingToolPort::new());
    let executor = Arc::new(port_executor_with(
        Arc::new(FakeModelPort {
            result: Ok(reply("unused")),
            calls: AtomicUsize::new(0),
        }),
        tools.clone(),
    ));
    let protector = Arc::new(EvidenceProtector::new(InMemoryKeyProvider::default()));
    let ids = Arc::new(SequenceIds::default());
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

    let runtime = multi_thread_runtime();
    let projection = runtime.block_on(async {
        let driver = tokio::spawn(drive_to_quiescence_async(
            opener(directory.path().to_path_buf()),
            protector.clone(),
            ids,
            driver_scope(),
            OpaqueId::parse(DRIVER_STREAM).unwrap(),
            execution_id.clone(),
            spec,
            executor,
            driver_actor(),
            cancel_rx,
            None,
        ));
        while tools.child.lock().unwrap().is_none() {
            tokio::task::yield_now().await;
        }
        cancel_tx.send(true).unwrap();
        driver.await.unwrap().unwrap()
    });

    // The child pid is gone: kill+wait ran inside the cancel hook, which the driver invokes
    // BEFORE recording the interruption (construction order; the killed flag is the hook's
    // own receipt).
    assert!(tools.killed.load(Ordering::SeqCst), "the cancel hook ran");
    let mut guard = tools.child.lock().unwrap();
    let child = guard.as_mut().expect("the child was spawned");
    assert!(
        child.try_wait().expect("liveness is checkable").is_some(),
        "the child process is dead before the outcome is recorded"
    );
    drop(guard);
    assert_eq!(
        projection.node_states.get("tool-node"),
        Some(&NodeState::Blocked),
        "the interrupted tool node is blocked for triage"
    );
}
