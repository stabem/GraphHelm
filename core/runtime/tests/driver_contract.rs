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
            program_allowlist: ["git".to_owned()].into_iter().collect(),
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
            verified_executable: None,
            contained_session: None,
            commit: None,
            landed_ref: None,
            recovered_workspace: false,
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
        context: String::new(),
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
        tool_failure_semantics: Default::default(),
        tool_call: None,
        gate_check: None,
        judge: None,
        context: None,
    }
}

fn tool_work() -> NodeWork {
    NodeWork {
        execution_id: "exec-1".to_owned(),
        node_id: "tests".to_owned(),
        attempt: 1,
        prompt: prompt(),
        kind: NodeWorkKind::Tool,
        tool_failure_semantics: Default::default(),
        tool_call: Some(ToolCall::Shell(ShellAction {
            program: "git".to_owned(),
            arguments: vec!["status".to_owned()],
        })),
        gate_check: None,
        judge: None,
        context: None,
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
        gates: Arc::new(NoGates),
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
        gates: Arc::new(NoGates),
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
            NodeOutcome::TerminalFailure,
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
            NodeOutcome::TerminalFailure,
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
fn permanent_and_cause_ambiguous_host_codes_are_terminal_without_lost_error_kind() {
    for code in [
        "GHTOOL001_SPAWN",
        "GHTOOL002_ENV",
        "GHTOOL003_PREPARE",
        "GHTOOL004_CONFIG",
        "GHTOOL005_ESCAPE",
        "GHTOOL006_TIER",
        "GHTOOL007_EXIT_UNKNOWN",
        "GHTOOL999_FUTURE",
    ] {
        let executor = executor(
            Ok(reply("unused")),
            ToolDisposition::HostError {
                code: code.to_owned(),
            },
        );
        let outcome = block_on(executor.execute(&tool_work())).unwrap();
        assert_eq!(outcome.outcome, NodeOutcome::TerminalFailure, "{code}");
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
    ActorId, Clock, EventKind, ExecutionCompleted, ExecutionId, ExecutionMode, ExecutionStarted,
    IdGenerator, NewEvent, OpaqueId, PersistedActor, PersistedActorType, ProjectId, RawSha256,
    RepositoryScope, ReuseKeyComponent, ReuseOutcome, Sensitivity, WireHash, WorkspaceId,
};
use graphhelm_runtime::context_accounting::{ACCOUNTING_RECEIPT_MEDIA_TYPE, MODEL_USAGE_PRODUCER};
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
    event_by(key, driver_actor(), kind)
}

fn event_by(key: &str, actor: PersistedActor, kind: EventKind) -> NewEvent {
    NewEvent::new(
        OpaqueId::parse(key).unwrap(),
        actor,
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
    running_node_repository_started_by(directory, driver_actor())
}

fn running_node_repository_started_by(
    directory: &std::path::Path,
    start_actor: PersistedActor,
) -> (LocalEventRepository, OpaqueId) {
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
    append(vec![event_by(
        "boot-started",
        start_actor,
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

struct RefusingEvidenceSealer;

impl graphhelm_events::EvidenceSealer for RefusingEvidenceSealer {
    fn seal<'a>(
        &'a self,
        _scope: RepositoryScope,
        _input: graphhelm_events::EvidenceInput,
    ) -> RepositoryFuture<
        'a,
        Result<graphhelm_events::SealedEvidence, graphhelm_events::EvidenceError>,
    > {
        Box::pin(async { Err(graphhelm_events::EvidenceError::Unavailable) })
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
    let expected: Vec<String> = ["reply", "stdout", "accounting-receipt"]
        .iter()
        .map(|suffix| format!("exec-{execution_id}-{NODE}-a{attempt}-{suffix}"))
        .collect();
    let refs = &envelope.evidence_refs;
    assert_eq!(
        refs.len(),
        3,
        "work material and its accounting receipt are referenced"
    );
    // The local store exposes availability, not a sealed read (`EvidenceRepository` is the
    // Postgres adapter's surface — reported discrepancy); the driver returns what it sealed,
    // so the open round-trip runs against the exact appended items.
    let originals = succeeded_work(None);
    for (expected_id, original) in expected.iter().take(2).zip(&originals.sealables) {
        let reference = refs
            .iter()
            .find(|reference| reference.evidence_id().as_str() == expected_id)
            .expect("the original sealable reference exists");
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
fn a_maximum_length_start_actor_gets_a_bounded_stable_binding_identity() {
    let directory = tempfile::tempdir().unwrap();
    let actor_id = "a".repeat(256);
    let start_actor = PersistedActor::new(
        PersistedActorType::System,
        ActorId::parse(&actor_id).unwrap(),
    );
    let (repository, execution_id) =
        running_node_repository_started_by(directory.path(), start_actor);
    let scope = driver_scope();
    let protector = EvidenceProtector::new(InMemoryKeyProvider::default());

    let recorded = block_on(record_outcome_with_evidence(
        &repository,
        &protector,
        &SequenceIds::default(),
        &scope,
        &OpaqueId::parse(DRIVER_STREAM).unwrap(),
        &execution_id,
        &driver_actor(),
        NODE,
        &succeeded_work(None),
    ))
    .expect("a valid ActorId must not make accounting fail");
    let receipt = recorded
        .sealed
        .iter()
        .find(|item| item.media_type().as_str() == ACCOUNTING_RECEIPT_MEDIA_TYPE)
        .unwrap();
    let opened = block_on(graphhelm_events::EvidenceOpener::open(
        &protector, scope, receipt,
    ))
    .unwrap();
    let json: serde_json::Value = opened.expose(|bytes| serde_json::from_slice(bytes).unwrap());
    assert_eq!(json["executionBinding"]["producerActor"]["type"], "system");
    assert_eq!(json["executionBinding"]["producerActor"]["id"], actor_id);
    assert!(json["executionBinding"].get("snapshots").is_none());
}

#[test]
fn measured_model_usage_receipt_round_trips_as_the_same_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let (repository, execution_id) = running_node_repository(directory.path());
    let scope = driver_scope();
    let stream = OpaqueId::parse(DRIVER_STREAM).unwrap();
    let protector = EvidenceProtector::new(InMemoryKeyProvider::default());
    let ids = SequenceIds::default();
    let executor = executor(
        Ok(reply("PRIVATE-REPLY-CONTENT")),
        ToolDisposition::Completed { exit_code: 0 },
    );
    let work = block_on(executor.execute(&cognitive_work())).unwrap();
    let history = repository
        .read_replay_stream(&scope, DRIVER_STREAM)
        .unwrap();
    let started = history
        .iter()
        .find(|envelope| matches!(envelope.kind, EventKind::ExecutionStarted(_)))
        .unwrap();
    let expected_event_id = started.event_id.to_string();
    let expected_event_hash = started.event_hash.to_string();

    let recorded = block_on(record_outcome_with_evidence(
        &repository,
        &protector,
        &ids,
        &scope,
        &stream,
        &execution_id,
        &driver_actor(),
        NODE,
        &work,
    ))
    .unwrap();

    let receipt = recorded
        .sealed
        .iter()
        .find(|item| {
            item.reference()
                .evidence_id()
                .as_str()
                .ends_with("-accounting-receipt")
        })
        .expect("the real work summary must become accounting evidence");
    assert_eq!(receipt.media_type().as_str(), ACCOUNTING_RECEIPT_MEDIA_TYPE);
    assert!(
        repository
            .evidence_exists(&scope, receipt.reference().evidence_id())
            .unwrap(),
        "the receipt reference must reload from the repository boundary"
    );
    let opened = block_on(graphhelm_events::EvidenceOpener::open(
        &protector, scope, receipt,
    ))
    .unwrap();
    let bytes = opened.expose(|bytes| bytes.to_vec());
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        json["executionBinding"]["bindingKind"],
        "execution_started_event"
    );
    assert_eq!(
        json["executionBinding"]["schemaId"],
        "https://p50.dev/schemas/event-envelope.schema.json"
    );
    assert_eq!(json["executionBinding"]["eventId"], expected_event_id);
    assert_eq!(json["executionBinding"]["eventHash"], expected_event_hash);
    assert_eq!(
        json["executionBinding"]["scope"]["executionId"],
        execution_id.as_str()
    );
    assert_eq!(json["executionBinding"]["producerActor"]["type"], "system");
    assert_eq!(
        json["executionBinding"]["producerActor"]["id"],
        "system-driver"
    );
    assert!(json["executionBinding"].get("artifactId").is_none());
    assert!(json["executionBinding"].get("snapshots").is_none());
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("\"value\":12"));
    assert!(text.contains("\"value\":5"));
    assert!(text.contains("\"name\":\"provider_reported_input_tokens\",\"value\":12"));
    assert!(text.contains("\"name\":\"provider_total_input_tokens\",\"value\":null"));
    assert!(text.contains("\"name\":\"compiled_input_tokens\",\"value\":null"));
    assert!(text.contains("\"provenance\":\"measured\""));
    assert!(text.contains(&format!("\"producer\":\"{MODEL_USAGE_PRODUCER}\"")));
    assert!(!text.contains("PRIVATE-REPLY-CONTENT"));
}

#[test]
fn an_unsafe_provider_token_count_refuses_before_sealing_or_append() {
    let directory = tempfile::tempdir().unwrap();
    let (repository, execution_id) = running_node_repository(directory.path());
    let scope = driver_scope();
    let before = repository.next_sequence(&scope, DRIVER_STREAM).unwrap();
    let work = WorkOutcome {
        outcome: NodeOutcome::Succeeded,
        reason: None,
        summary: WorkSummary {
            input_tokens: Some(9_007_199_254_740_992),
            output_tokens: Some(5),
            exit_code: None,
        },
        sealables: Vec::new(),
        reuse: None,
        gate_verdict: None,
    };

    let result = block_on(record_outcome_with_evidence(
        &repository,
        &EvidenceProtector::new(InMemoryKeyProvider::default()),
        &SequenceIds::default(),
        &scope,
        &OpaqueId::parse(DRIVER_STREAM).unwrap(),
        &execution_id,
        &driver_actor(),
        NODE,
        &work,
    ));

    assert!(result.is_err());
    assert_eq!(
        repository.next_sequence(&scope, DRIVER_STREAM).unwrap(),
        before
    );
}

#[test]
fn a_no_observation_fixture_outcome_does_not_require_evidence_sealing() {
    let directory = tempfile::tempdir().unwrap();
    let cli_start_actor = PersistedActor::new(
        PersistedActorType::System,
        ActorId::parse("system-cli").unwrap(),
    );
    let (repository, execution_id) =
        running_node_repository_started_by(directory.path(), cli_start_actor);
    let scope = driver_scope();
    let work = WorkOutcome {
        outcome: NodeOutcome::Succeeded,
        reason: None,
        sealables: Vec::new(),
        summary: WorkSummary {
            input_tokens: None,
            output_tokens: None,
            exit_code: None,
        },
        reuse: None,
        gate_verdict: None,
    };

    let recorded = block_on(record_outcome_with_evidence(
        &repository,
        &RefusingEvidenceSealer,
        &SequenceIds::default(),
        &scope,
        &OpaqueId::parse(DRIVER_STREAM).unwrap(),
        &execution_id,
        &driver_actor(),
        NODE,
        &work,
    ))
    .expect("a fixture with nothing to account must not call the refusing sealer");

    assert!(recorded.sealed.is_empty());
    let history = repository
        .read_replay_stream(&scope, DRIVER_STREAM)
        .unwrap();
    assert!(history.last().unwrap().evidence_refs.is_empty());
}

#[test]
fn sealed_work_without_provider_usage_still_emits_an_unavailable_receipt() {
    let directory = tempfile::tempdir().unwrap();
    let (repository, execution_id) = running_node_repository(directory.path());
    let scope = driver_scope();
    let protector = EvidenceProtector::new(InMemoryKeyProvider::default());
    let work = WorkOutcome {
        outcome: NodeOutcome::Succeeded,
        reason: None,
        sealables: vec![Sealable {
            local_ref_suffix: "reply",
            media_type: "application/json",
            bytes: br#"{"text":"REAL-WORK-WITHOUT-USAGE"}"#.to_vec(),
        }],
        summary: WorkSummary {
            input_tokens: None,
            output_tokens: None,
            exit_code: None,
        },
        reuse: None,
        gate_verdict: None,
    };

    let recorded = block_on(record_outcome_with_evidence(
        &repository,
        &protector,
        &SequenceIds::default(),
        &scope,
        &OpaqueId::parse(DRIVER_STREAM).unwrap(),
        &execution_id,
        &driver_actor(),
        NODE,
        &work,
    ))
    .expect("real sealed work must keep its accounting receipt even without provider usage");

    let receipt = recorded
        .sealed
        .iter()
        .find(|item| item.media_type().as_str() == ACCOUNTING_RECEIPT_MEDIA_TYPE)
        .expect("the unavailable receipt is still durable Evidence");
    let opened = block_on(graphhelm_events::EvidenceOpener::open(
        &protector, scope, receipt,
    ))
    .unwrap();
    let json: serde_json::Value = opened.expose(|bytes| serde_json::from_slice(bytes).unwrap());
    for name in ["provider_reported_input_tokens", "output_tokens"] {
        let field = json["fields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|field| field["name"] == name)
            .unwrap();
        assert_eq!(field["value"], serde_json::Value::Null);
        assert_eq!(field["provenance"], "unavailable");
    }
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
use graphhelm_runtime::driver::{ImmediateCancelRequest, StoreOpen, drive_to_quiescence_async};

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

fn tool_graph_node(failure_semantics: Option<&str>, arguments: Vec<String>) -> GraphNode {
    let mut properties = std::collections::BTreeMap::new();
    properties.insert(
        "tool".to_owned(),
        serde_json::json!({"call": {"tool": "tests", "arguments": arguments}}),
    );
    if let Some(failure_semantics) = failure_semantics {
        let (field, cause) = match failure_semantics {
            "verdict_bearing" => ("doNotRetryOn", "tool_exited_non_zero"),
            "retry_eligible" => ("retryOn", "tool_exited_non_zero"),
            other => panic!("unknown failure semantics: {other}"),
        };
        properties.insert(
            "retry".to_owned(),
            serde_json::Value::Object(
                [(field.to_owned(), serde_json::json!([cause]))]
                    .into_iter()
                    .collect(),
            ),
        );
    }
    GraphNode {
        node_type: NodeType::Tool,
        name: "fixture process".to_owned(),
        objective: "exercise retry semantics against a real child process".to_owned(),
        optionality: Optionality::Required,
        properties,
    }
}

fn conflicting_retry_tool_graph_node() -> GraphNode {
    let mut node = tool_graph_node(None, Vec::new());
    node.properties.insert(
        "retry".to_owned(),
        serde_json::json!({
            "retryOn": ["tool_exited_non_zero"],
            "doNotRetryOn": ["tool_exited_non_zero"]
        }),
    );
    node
}

fn malformed_conflicting_retry_tool_graph_node() -> GraphNode {
    let mut node = tool_graph_node(None, Vec::new());
    node.properties.insert(
        "retry".to_owned(),
        serde_json::json!({
            "retryOn": ["tool_exited_non_zero"],
            "doNotRetryOn": ["tool_exited_non_zero", 1]
        }),
    );
    node
}

fn malformed_retry_tool_graph_node() -> GraphNode {
    let mut node = tool_graph_node(None, Vec::new());
    node.properties.insert(
        "retry".to_owned(),
        serde_json::json!({
            "retryOn": [1]
        }),
    );
    node
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

#[test]
fn retry_fixture_process() {
    let Some(mode) = std::env::var_os("GH_RETRY_FIXTURE_MODE") else {
        return;
    };
    match mode.to_string_lossy().as_ref() {
        "fail_then_pass" => {
            let marker = std::path::PathBuf::from(
                std::env::var_os("GH_RETRY_FIXTURE_MARKER").expect("fixture marker"),
            );
            if !marker.exists() {
                std::fs::write(marker, b"first attempt was red").unwrap();
                panic!("the first real process attempt is deliberately red");
            }
        }
        other => panic!("unknown retry fixture mode: {other}"),
    }
}

struct ProcessFixtureToolPort {
    marker: std::path::PathBuf,
    calls: AtomicUsize,
}

const PROCESS_FIXTURE_WAIT_POLLS: usize = 3_000;

struct ProcessFixtureChild {
    child: Option<std::process::Child>,
    reaped: Option<Arc<std::sync::atomic::AtomicBool>>,
}

impl ProcessFixtureChild {
    fn new(child: std::process::Child) -> Self {
        Self {
            child: Some(child),
            reaped: None,
        }
    }

    fn with_reap_receipt(
        child: std::process::Child,
        reaped: Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        Self {
            child: Some(child),
            reaped: Some(reaped),
        }
    }

    fn wait_with_poll_limit(&mut self, poll_limit: usize) -> std::process::ExitStatus {
        for _ in 0..poll_limit {
            match self
                .child
                .as_mut()
                .expect("fixture child present")
                .try_wait()
            {
                Ok(Some(status)) => {
                    self.child.take();
                    if let Some(reaped) = &self.reaped {
                        reaped.store(true, Ordering::SeqCst);
                    }
                    return status;
                }
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(10)),
                Err(error) => panic!("inspect retry fixture child: {error}"),
            }
        }
        panic!("retry fixture child exceeded its bounded wait");
    }
}

impl Drop for ProcessFixtureChild {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        let _ = child.kill();
        let reaped = child.wait().is_ok();
        if let Some(receipt) = &self.reaped {
            receipt.store(reaped, Ordering::SeqCst);
        }
    }
}

impl ToolPort for ProcessFixtureToolPort {
    fn invoke<'a>(
        &'a self,
        _call: &'a ToolCall,
        lease: &'a ToolLease,
        _actor: &'a str,
    ) -> Pin<Box<dyn Future<Output = ToolPortResult> + Send + 'a>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "retry_fixture_process", "--nocapture"])
            .env("GH_RETRY_FIXTURE_MODE", "fail_then_pass")
            .env("GH_RETRY_FIXTURE_MARKER", &self.marker)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn retry fixture child");
        let mut child = ProcessFixtureChild::new(child);
        let status = child.wait_with_poll_limit(PROCESS_FIXTURE_WAIT_POLLS);
        let disposition = ToolDisposition::Completed {
            exit_code: status.code().unwrap_or(1),
        };
        let record = ToolCallRecord {
            tool: "tests".to_owned(),
            action: "run".to_owned(),
            actor: "agent-runtime".to_owned(),
            program_allowlist: lease.programs.clone(),
            tier: IsolationTier::Tier1,
            disposition,
            stdout_sha256: digest_hex(b""),
            stdout_bytes: 0,
            stderr_sha256: digest_hex(b""),
            stderr_bytes: 0,
            truncated: false,
            reused: false,
            verified_executable: None,
            contained_session: None,
            commit: None,
            landed_ref: None,
            recovered_workspace: false,
        };
        Box::pin(async move {
            ToolPortResult {
                record,
                streams: ToolStreams {
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                },
                reuse: None,
            }
        })
    }
}

struct DeterministicTimeoutToolPort {
    calls: AtomicUsize,
}

impl ToolPort for DeterministicTimeoutToolPort {
    fn invoke<'a>(
        &'a self,
        _call: &'a ToolCall,
        lease: &'a ToolLease,
        _actor: &'a str,
    ) -> Pin<Box<dyn Future<Output = ToolPortResult> + Send + 'a>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            ToolPortResult {
                record: ToolCallRecord {
                    tool: "tests".to_owned(),
                    action: "run".to_owned(),
                    actor: "agent-runtime".to_owned(),
                    program_allowlist: lease.programs.clone(),
                    tier: IsolationTier::Tier1,
                    disposition: ToolDisposition::TimedOut,
                    stdout_sha256: digest_hex(b""),
                    stdout_bytes: 0,
                    stderr_sha256: digest_hex(b""),
                    stderr_bytes: 0,
                    truncated: false,
                    reused: false,
                    verified_executable: None,
                    contained_session: None,
                    commit: None,
                    landed_ref: None,
                    recovered_workspace: false,
                },
                streams: ToolStreams {
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                },
                reuse: None,
            }
        })
    }
}

fn drive_tool_fixture(
    port: Arc<dyn ToolPort>,
    failure_semantics: Option<&str>,
) -> graphhelm_events::ExecutionProjection {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    std::fs::create_dir_all(&events).unwrap();
    let execution_id = started_repository(&events);
    let spec = spec_with(
        vec![("stage", tool_graph_node(failure_semantics, Vec::new()))],
        vec![],
        1,
    );
    let executor = Arc::new(PortExecutor {
        model: Arc::new(FakeModelPort {
            result: Ok(reply("unused")),
            calls: AtomicUsize::new(0),
        }),
        tools: port.clone(),
        route_id: "claude_subscription".to_owned(),
        lease: ToolLease {
            actor: "agent-runtime".to_owned(),
            capabilities: [Capability::TestsExecute].into_iter().collect(),
            programs: std::collections::BTreeSet::new(),
        },
        actor: "agent-runtime".to_owned(),
        gates: Arc::new(NoGates),
    });
    let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(None::<ImmediateCancelRequest>);
    multi_thread_runtime()
        .block_on(drive_to_quiescence_async(
            opener(events),
            Arc::new(EvidenceProtector::new(InMemoryKeyProvider::default())),
            Arc::new(SequenceIds::default()),
            driver_scope(),
            OpaqueId::parse(DRIVER_STREAM).unwrap(),
            execution_id,
            spec,
            executor,
            driver_actor(),
            std::collections::BTreeSet::new(),
            driver_actor(),
            cancel_rx,
            None,
            None,
        ))
        .unwrap()
}

fn drive_real_process_fixture(
    failure_semantics: Option<&str>,
) -> (graphhelm_events::ExecutionProjection, usize) {
    let directory = tempfile::tempdir().unwrap();
    let port = Arc::new(ProcessFixtureToolPort {
        marker: directory.path().join("retry-marker"),
        calls: AtomicUsize::new(0),
    });
    let projection = drive_tool_fixture(port.clone(), failure_semantics);
    (projection, port.calls.load(Ordering::SeqCst))
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
        gates: Arc::new(NoGates),
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
fn a_verdict_bearing_real_process_cannot_retry_from_red_into_green() {
    let (projection, calls) = drive_real_process_fixture(Some("verdict_bearing"));
    assert_eq!(projection.node_states["stage"], NodeState::Failed);
    assert_eq!(calls, 1, "the honest red verdict is never redispatched");
}

#[test]
fn an_undeclared_tool_defaults_to_verdict_bearing() {
    let (projection, calls) = drive_real_process_fixture(None);
    assert_eq!(projection.node_states["stage"], NodeState::Failed);
    assert_eq!(calls, 1, "absence uses the safe verdict-bearing default");
}

#[test]
fn conflicting_retry_policy_refuses_before_every_node_effect_and_settles_failed() {
    let directory = tempfile::tempdir().unwrap();
    let execution_id = started_repository(directory.path());
    let port = Arc::new(DeterministicTimeoutToolPort {
        calls: AtomicUsize::new(0),
    });
    let spec = spec_with(
        vec![
            ("conflict", conflicting_retry_tool_graph_node()),
            ("effect-capable", tool_graph_node(None, Vec::new())),
        ],
        vec![],
        2,
    );
    let executor = Arc::new(PortExecutor {
        model: Arc::new(FakeModelPort {
            result: Ok(reply("unused")),
            calls: AtomicUsize::new(0),
        }),
        tools: port.clone(),
        route_id: "claude_subscription".to_owned(),
        lease: ToolLease {
            actor: "agent-runtime".to_owned(),
            capabilities: [Capability::TestsExecute].into_iter().collect(),
            programs: std::collections::BTreeSet::new(),
        },
        actor: "agent-runtime".to_owned(),
        gates: Arc::new(NoGates),
    });
    let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(None::<ImmediateCancelRequest>);
    let projection = multi_thread_runtime()
        .block_on(drive_to_quiescence_async(
            opener(directory.path().to_path_buf()),
            Arc::new(EvidenceProtector::new(InMemoryKeyProvider::default())),
            Arc::new(SequenceIds::default()),
            driver_scope(),
            OpaqueId::parse(DRIVER_STREAM).unwrap(),
            execution_id,
            spec,
            executor,
            driver_actor(),
            std::collections::BTreeSet::new(),
            driver_actor(),
            cancel_rx,
            None,
            None,
        ))
        .unwrap();

    assert_eq!(port.calls.load(Ordering::SeqCst), 0, "no tool may run");
    assert!(
        projection.node_states.is_empty(),
        "preflight refusal must precede even Draft -> Ready approval"
    );
    assert_eq!(
        projection.simulation_status,
        Some(graphhelm_protocols::SimulationStatus::Failed)
    );

    let repository = opener(directory.path().to_path_buf())().unwrap();
    let history = repository
        .read_replay_stream(&driver_scope(), DRIVER_STREAM)
        .unwrap();
    assert!(
        !history
            .iter()
            .any(|event| matches!(event.kind, EventKind::NodeOutcomeRecorded(_))),
        "a refusal before effects cannot record a node lifecycle hop"
    );
    let diagnostics = history
        .iter()
        .find_map(|event| match &event.kind {
            EventKind::GraphValidationFailed(failed) => Some(&failed.diagnostics),
            _ => None,
        })
        .expect("stable refusal diagnostics are journaled");
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code(), "GHG015_RETRY_CAUSE_CONFLICT");
    assert_eq!(diagnostics[0].path().as_str(), "/spec/nodes/conflict/retry");
    assert_eq!(
        diagnostics[0].component(),
        graphhelm_protocols::DiagnosticComponent::Graph
    );
    assert_eq!(
        diagnostics[0].severity(),
        &graphhelm_protocols::Severity::Error
    );
    assert_eq!(history.len(), 3, "start + atomic refusal/settlement pair");
    assert!(matches!(
        history.last().map(|event| &event.kind),
        Some(EventKind::ExecutionCompleted(ExecutionCompleted {
            status: graphhelm_protocols::SimulationStatus::Failed,
            ..
        }))
    ));
}

#[test]
fn malformed_conflicting_retry_policy_fails_closed_before_every_node_effect() {
    let directory = tempfile::tempdir().unwrap();
    let execution_id = started_repository(directory.path());
    let port = Arc::new(DeterministicTimeoutToolPort {
        calls: AtomicUsize::new(0),
    });
    let spec = spec_with(
        vec![
            ("conflict", malformed_conflicting_retry_tool_graph_node()),
            ("effect-capable", tool_graph_node(None, Vec::new())),
            ("malformed-only", malformed_retry_tool_graph_node()),
        ],
        vec![],
        2,
    );
    let executor = Arc::new(PortExecutor {
        model: Arc::new(FakeModelPort {
            result: Ok(reply("unused")),
            calls: AtomicUsize::new(0),
        }),
        tools: port.clone(),
        route_id: "claude_subscription".to_owned(),
        lease: ToolLease {
            actor: "agent-runtime".to_owned(),
            capabilities: [Capability::TestsExecute].into_iter().collect(),
            programs: std::collections::BTreeSet::new(),
        },
        actor: "agent-runtime".to_owned(),
        gates: Arc::new(NoGates),
    });
    let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(None::<ImmediateCancelRequest>);
    let projection = multi_thread_runtime()
        .block_on(drive_to_quiescence_async(
            opener(directory.path().to_path_buf()),
            Arc::new(EvidenceProtector::new(InMemoryKeyProvider::default())),
            Arc::new(SequenceIds::default()),
            driver_scope(),
            OpaqueId::parse(DRIVER_STREAM).unwrap(),
            execution_id,
            spec,
            executor,
            driver_actor(),
            std::collections::BTreeSet::new(),
            driver_actor(),
            cancel_rx,
            None,
            None,
        ))
        .unwrap();

    assert_eq!(port.calls.load(Ordering::SeqCst), 0, "no tool may run");
    assert!(projection.node_states.is_empty());
    assert_eq!(
        projection.simulation_status,
        Some(graphhelm_protocols::SimulationStatus::Failed)
    );

    let repository = opener(directory.path().to_path_buf())().unwrap();
    let history = repository
        .read_replay_stream(&driver_scope(), DRIVER_STREAM)
        .unwrap();
    assert!(
        !history
            .iter()
            .any(|event| matches!(event.kind, EventKind::NodeOutcomeRecorded(_)))
    );
    let diagnostics = history
        .iter()
        .find_map(|event| match &event.kind {
            EventKind::GraphValidationFailed(failed) => Some(&failed.diagnostics),
            _ => None,
        })
        .expect("stable refusal diagnostics are journaled");
    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code(), diagnostic.path().as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("GHG015_RETRY_CAUSE_CONFLICT", "/spec/nodes/conflict/retry"),
            ("GHS003_TYPED", "/spec/nodes/conflict/retry"),
            ("GHS003_TYPED", "/spec/nodes/malformed-only/retry")
        ]
    );
    assert_eq!(history.len(), 3, "start + atomic refusal/settlement pair");
    assert!(matches!(
        history.last().map(|event| &event.kind),
        Some(EventKind::ExecutionCompleted(ExecutionCompleted {
            status: graphhelm_protocols::SimulationStatus::Failed,
            ..
        }))
    ));
}

#[test]
fn a_stuck_retry_fixture_child_is_killed_and_reaped_at_the_wait_bound() {
    let reaped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let child = KillingToolPort::spawn_sleeper();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe({
        let reaped = reaped.clone();
        move || {
            let mut child = ProcessFixtureChild::with_reap_receipt(child, reaped);
            child.wait_with_poll_limit(1);
        }
    }));
    assert!(result.is_err(), "the harness limit must trip for a sleeper");
    assert!(
        reaped.load(Ordering::SeqCst),
        "the timeout path kills and waits for the child before unwinding"
    );
}

#[test]
fn an_explicit_retry_eligible_real_process_may_retry_nonzero_into_green() {
    let (projection, calls) = drive_real_process_fixture(Some("retry_eligible"));
    assert_eq!(projection.node_states["stage"], NodeState::Succeeded);
    assert_eq!(calls, 2, "the explicit declaration permits one redispatch");
}

#[test]
fn a_verdict_bearing_timeout_stays_retryable_harness_failure() {
    let port = Arc::new(DeterministicTimeoutToolPort {
        calls: AtomicUsize::new(0),
    });
    let projection = drive_tool_fixture(port.clone(), Some("verdict_bearing"));
    let calls = port.calls.load(Ordering::SeqCst);
    assert_eq!(projection.node_states["stage"], NodeState::Blocked);
    assert!(calls > 1, "a timeout is not laundered into a verdict");
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
    let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(None::<ImmediateCancelRequest>);

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
            // #123: this drive releases nothing, so the set is empty and the
            // releasing actor is never consulted.
            std::collections::BTreeSet::new(),
            driver_actor(),
            cancel_rx,
            None,
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
    let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(None::<ImmediateCancelRequest>);

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
            // #123: this drive releases nothing, so the set is empty and the
            // releasing actor is never consulted.
            std::collections::BTreeSet::new(),
            driver_actor(),
            cancel_rx,
            None,
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
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(None::<ImmediateCancelRequest>);

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
            // #123: this drive releases nothing, so the set is empty and the
            // releasing actor is never consulted.
            std::collections::BTreeSet::new(),
            driver_actor(),
            cancel_rx,
            None,
            None,
        ));
        while model.called.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
        cancel_tx
            .send(Some(ImmediateCancelRequest {
                actor: driver_actor(),
                idempotency_key: OpaqueId::parse("driver-contract-test-cancel").unwrap(),
            }))
            .unwrap();
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
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(None::<ImmediateCancelRequest>);

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
            // #123: this drive releases nothing, so the set is empty and the
            // releasing actor is never consulted.
            std::collections::BTreeSet::new(),
            driver_actor(),
            cancel_rx,
            None,
            None,
        ));
        while tools.child.lock().unwrap().is_none() {
            tokio::task::yield_now().await;
        }
        cancel_tx
            .send(Some(ImmediateCancelRequest {
                actor: driver_actor(),
                idempotency_key: OpaqueId::parse("driver-contract-test-cancel").unwrap(),
            }))
            .unwrap();
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

/// A model port that hangs its FIRST call and answers every later one immediately.
///
/// `HangingModelPort` hangs every call, which is right for immediate-stop (the drive is killed
/// while the node is held) and wrong here: an ordinary pause must let the drive REACH its next
/// dispatch decision, and a port that hangs forever would make this test hang rather than fail.
/// A red that hangs is not a red -- nobody can tell it from a slow machine.
struct HangsOnceModelPort {
    called: AtomicUsize,
    /// A SEMAPHORE, not a `Notify`, and the difference is a hang.
    ///
    /// `called` is published by `fetch_add` BEFORE the wait registers, so a test that sees the
    /// counter move and releases immediately can land in the gap. `notify_waiters` keeps no permit
    /// for a waiter that has not arrived yet, so that release would be dropped and the call would
    /// wait forever — a CI hang with no assertion to read, which is the failure mode this file
    /// already argues against in `HangsOnceModelPort`'s own reason for existing.
    ///
    /// A permit added to a semaphore is RETAINED. Release before the wait and the wait returns at
    /// once; release after and it wakes normally. The race stops being a race.
    wake: tokio::sync::Semaphore,
}

impl HangsOnceModelPort {
    fn new() -> Self {
        Self {
            called: AtomicUsize::new(0),
            wake: tokio::sync::Semaphore::new(0),
        }
    }

    fn release(&self) {
        self.wake.add_permits(1);
    }
}

impl ModelPort for HangsOnceModelPort {
    fn call<'a>(
        &'a self,
        _route_id: &'a str,
        _call: &'a ModelCall,
    ) -> Pin<Box<dyn Future<Output = Result<ModelReply, GatewayError>> + Send + 'a>> {
        Box::pin(async move {
            let first = self.called.fetch_add(1, Ordering::SeqCst) == 0;
            if first {
                let permit = self
                    .wake
                    .acquire()
                    .await
                    .expect("the semaphore is never closed");
                permit.forget();
            }
            Ok(reply("DONE"))
        })
    }
}

/// #124 — AN ORDINARY PAUSE MUST STOP THE DRIVE, and today it does not.
///
/// The defect is stated in main's own source: *"an ORDINARY (non-immediate) pause does NOT stop an
/// in-flight drive -- the serve route signals the driver's cancel channel only when
/// `mode == "immediate"`"*. `#123` added a guard that reads `simulation_status` each pass, but it
/// guards RELEASE only; dispatch never consults it. So the stream records `execution_paused` at
/// sequence T and node outcomes at T+1, T+2 -- the log-that-lies class at the exact moment of
/// operator intervention.
///
/// ARRANGEMENT, and the reason for each part. Two INDEPENDENT nodes with `max_parallel = 1`, so
/// exactly one is in flight and the other is a dispatch the drive has not made yet. The pause is
/// appended out of band, which is precisely what the ordinary pause route does (it appends and
/// returns; it signals nothing). Only then is the held node released, so the drive must make its
/// next dispatch decision with `Paused` already readable.
///
/// WHAT THIS ASSERTS AND WHAT IT DOES NOT. It asserts the drive stops DISPATCHING. It does not
/// assert the in-flight node is killed -- that is immediate-stop's promise, and collapsing the two
/// verbs into one is the exit #124 explicitly declined. The distinction survives: immediate
/// interrupts, ordinary declines to start more.
///
/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL once the fix lands: removing the paused check from
/// the loop head, or moving it below the dispatch that follows.
#[test]
fn an_ordinary_pause_stops_the_drive_before_it_dispatches_more_work() {
    let directory = tempfile::tempdir().unwrap();
    let execution_id = started_repository(directory.path());
    let spec = spec_with(
        vec![
            ("first", agent_graph_node("held")),
            ("second", agent_graph_node("held")),
        ],
        vec![],
        1,
    );
    let model = Arc::new(HangsOnceModelPort::new());
    let executor = Arc::new(port_executor_with(
        model.clone(),
        Arc::new(FakeToolPort {
            disposition: ToolDisposition::Completed { exit_code: 0 },
            reuse: None,
        }),
    ));
    let protector = Arc::new(EvidenceProtector::new(InMemoryKeyProvider::default()));
    let ids = Arc::new(SequenceIds::default());
    let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(None::<ImmediateCancelRequest>);

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
            std::collections::BTreeSet::new(),
            driver_actor(),
            cancel_rx,
            None,
            None,
        ));

        while model.called.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }

        // The ordinary pause: an append and nothing else. No channel, no signal -- the route's
        // whole behaviour, reproduced.
        append_execution_paused(directory.path(), &execution_id);

        // Release the held node ONLY now, so the next dispatch decision is made with the pause
        // already in the log. Releasing first would let the drive finish before it could read it,
        // and the test would pass without ever posing the question.
        model.release();
        driver.await.unwrap().unwrap()
    });

    assert_eq!(
        model.called.load(Ordering::SeqCst),
        1,
        "the drive dispatched more work AFTER execution_paused was readable: the pause is \
         recorded and the machine keeps going, which is what an operator reading `paused` cannot \
         see. Node states: {:?}",
        projection.node_states
    );
    assert!(
        !matches!(
            projection.node_states.get("second"),
            Some(NodeState::Running | NodeState::Succeeded | NodeState::Failed)
        ),
        "`second` was never dispatched, so it cannot have run: {:?}",
        projection.node_states
    );
}

/// Appends `execution_paused` the way the ordinary pause route does: straight onto the stream,
/// with no signal to anything. Retries on a sequence conflict because a live drive is appending
/// too -- a conflict here is contention, not a verdict.
fn append_execution_paused(directory: &std::path::Path, execution_id: &OpaqueId) {
    let repository = LocalEventRepository::open(
        directory,
        Arc::new(FixedClock),
        Arc::new(SequenceIds::default()),
    )
    .unwrap();
    let scope = driver_scope();
    for attempt in 0..64 {
        let head = repository
            .read_replay_stream(&scope, DRIVER_STREAM)
            .unwrap()
            .len() as u64;
        let request = PreparedAppend::new(
            scope.clone(),
            OpaqueId::parse(DRIVER_STREAM).unwrap(),
            head + 1,
            vec![plain_event(
                &format!("ordinary-pause-{attempt}"),
                EventKind::ExecutionPaused(graphhelm_protocols::ExecutionPaused {
                    execution_id: execution_id.clone(),
                }),
            )],
            vec![],
            vec![],
        )
        .unwrap();
        if repository.append_atomic(&request).is_ok() {
            return;
        }
    }
    panic!("HARNESS-BROKE: execution_paused never landed, so the test never posed its question");
}

/// No gate runs in this file, and that is a property worth stating rather than a gap: a
/// registry that answers `None` to every id refuses every gate node, which is what a driver
/// exercising cognitive and tool work should do if a gate node ever appears here by accident.
struct NoGates;

impl graphhelm_runtime::ports::GateRegistryPort for NoGates {
    fn suite_digest(&self, _gate_id: &str) -> Option<String> {
        None
    }

    fn evaluate(
        &self,
        _gate_id: &str,
        _evidence: &serde_json::Value,
    ) -> Option<graphhelm_runtime::ports::GateEvaluation> {
        None
    }
}

// ---------------------------------------------------------------------------------------------
// #1065 (Codex P1 on #1078): an immediate stop that arrives while a node's context is being
// compiled — the search or a read blocking on the filesystem — must not let that node, or any
// node after it in the plan, be dispatched. The compile is raced against the cancel channel.
// ---------------------------------------------------------------------------------------------

/// A search that BLOCKS until the test releases it, and says when it was entered.
struct BlockingSearch {
    entered: std::sync::atomic::AtomicBool,
    release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
}

impl graphhelm_runtime::ports::BoundedSourceSearch for BlockingSearch {
    fn search(
        &self,
        _terms: &[String],
        _bounds: &graphhelm_runtime::ports::SourceSearchBounds,
    ) -> Result<Vec<String>, graphhelm_runtime::ports::SourceSearchError> {
        self.entered.store(true, Ordering::SeqCst);
        let _ = self.release.lock().unwrap().recv();
        Ok(vec!["src/lib.rs".to_owned()])
    }
}

/// An execution-tree port whose scan blocks until released and records the token it was handed.
struct BlockingTree {
    entered: std::sync::atomic::AtomicBool,
    release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
    token: std::sync::Mutex<Option<graphhelm_runtime::ports::ScanCancel>>,
}

impl graphhelm_runtime::ports::ExecutionTreePort for BlockingTree {
    fn with_tree(
        &self,
        cancel: &graphhelm_runtime::ports::ScanCancel,
        _compile: &mut dyn FnMut(
            &dyn graphhelm_runtime::ports::BoundedSourceSearch,
            &dyn graphhelm_runtime::ports::BoundedSourceReader,
        ),
    ) -> graphhelm_runtime::ports::ExecutionTreeAccess {
        *self.token.lock().unwrap() = Some(cancel.clone());
        self.entered.store(true, Ordering::SeqCst);
        let _ = self.release.lock().unwrap().recv();
        graphhelm_runtime::ports::ExecutionTreeAccess::Unavailable
    }
}

/// Codex P1 on #1092: when immediate cancellation wins over a compile that is scanning the
/// execution tree, the abandoned scan is TOLD — its token is set by the time the drive returns,
/// while the scan is still blocked — so it can let go of the tree the drive's release needs.
#[test]
fn an_abandoned_execution_tree_scan_is_told_it_was_cancelled() {
    let directory = tempfile::tempdir().unwrap();
    let execution_id = started_repository(directory.path());
    let spec = spec_with(
        vec![("first", agent_graph_node("search the tree"))],
        vec![],
        2,
    );
    let model = Arc::new(HangingModelPort::new());
    let executor = Arc::new(port_executor_with(
        model.clone(),
        Arc::new(FakeToolPort {
            disposition: ToolDisposition::Completed { exit_code: 0 },
            reuse: None,
        }),
    ));
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(None::<ImmediateCancelRequest>);
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let tree = Arc::new(BlockingTree {
        entered: std::sync::atomic::AtomicBool::new(false),
        release: std::sync::Mutex::new(release_rx),
        token: std::sync::Mutex::new(None),
    });
    let ports = graphhelm_runtime::context::ContextPorts {
        search: Arc::new(StaticSearch(vec!["src/tree.rs".to_owned()])),
        reader: Arc::new(NeverReader),
        ledger: graphhelm_runtime::context::ContextLedger::new(),
        execution_tree: Some(tree.clone()),
    };
    let runtime = multi_thread_runtime();
    runtime.block_on(async {
        let driver = tokio::spawn(drive_to_quiescence_async(
            opener(directory.path().to_path_buf()),
            Arc::new(EvidenceProtector::new(InMemoryKeyProvider::default())),
            Arc::new(SequenceIds::default()),
            driver_scope(),
            OpaqueId::parse(DRIVER_STREAM).unwrap(),
            execution_id.clone(),
            spec,
            executor,
            driver_actor(),
            std::collections::BTreeSet::new(),
            driver_actor(),
            cancel_rx,
            None,
            Some(ports),
        ));
        while !tree.entered.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
        let token = tree.token.lock().unwrap().clone().unwrap();
        assert!(!token.is_cancelled(), "a running compile is not cancelled");
        cancel_tx
            .send(Some(ImmediateCancelRequest {
                actor: driver_actor(),
                idempotency_key: OpaqueId::parse("driver-contract-cancel-tree-scan").unwrap(),
            }))
            .unwrap();
        driver.await.unwrap().unwrap();
        assert!(
            token.is_cancelled(),
            "the drive returned, the scan is still blocked, and it has been told to stop"
        );
        release_tx.send(()).unwrap();
    });
    assert_eq!(model.called.load(Ordering::SeqCst), 0);
}

struct NeverReader;

impl graphhelm_runtime::ports::BoundedSourceReader for NeverReader {
    fn read_prefix(
        &self,
        _relative_path: &str,
        _max_bytes: u64,
    ) -> Result<graphhelm_runtime::ports::SourceExcerpt, graphhelm_runtime::ports::SourceReadError>
    {
        Err(graphhelm_runtime::ports::SourceReadError::Unreadable)
    }
}

#[test]
fn immediate_stop_during_context_compilation_dispatches_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let execution_id = started_repository(directory.path());
    let spec = spec_with(
        vec![
            ("first", agent_graph_node("search the tree")),
            ("second", agent_graph_node("also search the tree")),
        ],
        vec![],
        2,
    );
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
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(None::<ImmediateCancelRequest>);
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let search = Arc::new(BlockingSearch {
        entered: std::sync::atomic::AtomicBool::new(false),
        release: std::sync::Mutex::new(release_rx),
    });
    let ports = graphhelm_runtime::context::ContextPorts {
        search: search.clone(),
        reader: Arc::new(NeverReader),
        ledger: graphhelm_runtime::context::ContextLedger::new(),
        execution_tree: None,
    };

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
            std::collections::BTreeSet::new(),
            driver_actor(),
            cancel_rx,
            None,
            Some(ports),
        ));
        while !search.entered.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
        cancel_tx
            .send(Some(ImmediateCancelRequest {
                actor: driver_actor(),
                idempotency_key: OpaqueId::parse("driver-contract-cancel-during-compile").unwrap(),
            }))
            .unwrap();
        let projection = driver.await.unwrap().unwrap();
        // Released only AFTER the drive returned: the drive did not wait for the search.
        release_tx.send(()).unwrap();
        projection
    });

    assert_eq!(
        model.called.load(Ordering::SeqCst),
        0,
        "no model call was ever made"
    );
    for node in ["first", "second"] {
        assert_ne!(
            projection.node_states.get(node),
            Some(&NodeState::Running),
            "{node} was never dispatched"
        );
        assert_ne!(
            projection.node_states.get(node),
            Some(&NodeState::Blocked),
            "{node} was never interrupted, because it never started"
        );
    }
    assert_eq!(
        projection.simulation_status,
        Some(graphhelm_protocols::SimulationStatus::Paused)
    );
    let store = opener(directory.path().to_path_buf())().unwrap();
    let history = store
        .read_replay_stream(&driver_scope(), DRIVER_STREAM)
        .unwrap();
    assert!(
        !history.iter().any(|envelope| matches!(
            &envelope.kind,
            EventKind::NodeOutcomeRecorded(record) if record.outcome == NodeOutcome::Started
        )),
        "no dispatch hop was written after the stop was requested"
    );
}

// ---------------------------------------------------------------------------------------------
// #1065 review: a declared `context.budgetBytes` that cannot be read refuses the EXECUTION in
// preflight, journaled like a retry-policy conflict — never a node silently skipped while the
// execution stays `running`.
// ---------------------------------------------------------------------------------------------

fn agent_graph_node_with_budget(objective: &str, budget: serde_json::Value) -> GraphNode {
    let mut node = agent_graph_node(objective);
    node.properties.insert(
        "context".to_owned(),
        serde_json::json!({ "budgetBytes": budget }),
    );
    node
}

#[test]
fn an_unreadable_declared_context_budget_refuses_the_execution_before_any_node_effect() {
    let directory = tempfile::tempdir().unwrap();
    let execution_id = started_repository(directory.path());
    let model = Arc::new(FakeModelPort {
        result: Ok(reply("unused")),
        calls: AtomicUsize::new(0),
    });
    let spec = spec_with(
        vec![
            (
                "typo",
                agent_graph_node_with_budget("search", serde_json::json!(0)),
            ),
            (
                "fine",
                agent_graph_node_with_budget("search", serde_json::json!(4096)),
            ),
            ("plain", agent_graph_node("search")),
        ],
        vec![],
        2,
    );
    let executor = Arc::new(port_executor_with(
        model.clone(),
        Arc::new(FakeToolPort {
            disposition: ToolDisposition::Completed { exit_code: 0 },
            reuse: None,
        }),
    ));
    let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(None::<ImmediateCancelRequest>);
    // #1086: the budget is read only by a drive that compiles context, so the refusal is asked
    // of a drive WITH ports (`a_portless_drive_runs_…` below is the other side).
    let ports = graphhelm_runtime::context::ContextPorts {
        search: Arc::new(StaticSearch(vec!["src/tree.rs".to_owned()])),
        reader: Arc::new(StaticReader(b"fn search() {}\n")),
        ledger: graphhelm_runtime::context::ContextLedger::new(),
        execution_tree: None,
    };
    let projection = multi_thread_runtime()
        .block_on(drive_to_quiescence_async(
            opener(directory.path().to_path_buf()),
            Arc::new(EvidenceProtector::new(InMemoryKeyProvider::default())),
            Arc::new(SequenceIds::default()),
            driver_scope(),
            OpaqueId::parse(DRIVER_STREAM).unwrap(),
            execution_id,
            spec,
            executor,
            driver_actor(),
            std::collections::BTreeSet::new(),
            driver_actor(),
            cancel_rx,
            None,
            Some(ports),
        ))
        .unwrap();

    assert_eq!(model.calls.load(Ordering::SeqCst), 0, "no model may run");
    assert!(
        projection.node_states.is_empty(),
        "preflight refusal must precede even Draft -> Ready approval"
    );
    assert_eq!(
        projection.simulation_status,
        Some(graphhelm_protocols::SimulationStatus::Failed),
        "the execution is refused, never left running with the node skipped"
    );

    let repository = opener(directory.path().to_path_buf())().unwrap();
    let history = repository
        .read_replay_stream(&driver_scope(), DRIVER_STREAM)
        .unwrap();
    assert!(
        !history
            .iter()
            .any(|event| matches!(event.kind, EventKind::NodeOutcomeRecorded(_))),
        "a refusal before effects cannot record a node lifecycle hop"
    );
    let diagnostics = history
        .iter()
        .find_map(|event| match &event.kind {
            EventKind::GraphValidationFailed(failed) => Some(&failed.diagnostics),
            _ => None,
        })
        .expect("stable refusal diagnostics are journaled");
    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code(), diagnostic.path().as_str()))
            .collect::<Vec<_>>(),
        vec![("GHG016_CONTEXT_BUDGET_INVALID", "/spec/nodes/typo/context")],
        "only the unreadable declaration is named; a valid one and an absent one are not"
    );
    assert_eq!(
        diagnostics[0].component(),
        graphhelm_protocols::DiagnosticComponent::Graph
    );
    assert_eq!(history.len(), 3, "start + atomic refusal/settlement pair");
    assert!(matches!(
        history.last().map(|event| &event.kind),
        Some(EventKind::ExecutionCompleted(ExecutionCompleted {
            status: graphhelm_protocols::SimulationStatus::Failed,
            ..
        }))
    ));
}

/// #1086 item 13: `GHG016` is asked only of nodes that RECEIVE context — plain cognitive nodes
/// on a drive that has context ports. A graph that ran on a portless drive before #1065 (a
/// fixture drive, a tools-only server) never read its `context.budgetBytes` and keeps running.
#[test]
fn a_portless_drive_runs_a_graph_whose_context_budget_it_never_reads() {
    let directory = tempfile::tempdir().unwrap();
    let execution_id = started_repository(directory.path());
    let model = Arc::new(FakeModelPort {
        result: Ok(reply("done")),
        calls: AtomicUsize::new(0),
    });
    let spec = spec_with(
        vec![(
            "typo",
            agent_graph_node_with_budget("search", serde_json::json!(0)),
        )],
        vec![],
        2,
    );
    let executor = Arc::new(port_executor_with(
        model.clone(),
        Arc::new(FakeToolPort {
            disposition: ToolDisposition::Completed { exit_code: 0 },
            reuse: None,
        }),
    ));
    let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(None::<ImmediateCancelRequest>);
    let projection = multi_thread_runtime()
        .block_on(drive_to_quiescence_async(
            opener(directory.path().to_path_buf()),
            Arc::new(EvidenceProtector::new(InMemoryKeyProvider::default())),
            Arc::new(SequenceIds::default()),
            driver_scope(),
            OpaqueId::parse(DRIVER_STREAM).unwrap(),
            execution_id,
            spec,
            executor,
            driver_actor(),
            std::collections::BTreeSet::new(),
            driver_actor(),
            cancel_rx,
            None,
            None,
        ))
        .unwrap();
    assert_eq!(model.calls.load(Ordering::SeqCst), 1, "the node ran");
    assert_eq!(
        projection.node_states.get("typo"),
        Some(&NodeState::Succeeded)
    );
}

// ---------------------------------------------------------------------------------------------
// #1065 review: the ledger says what a node RAN with. A capsule compiled for a node that is then
// refused at assembly (or held back by a pause) is never recorded — only dispatched nodes are.
// ---------------------------------------------------------------------------------------------

struct StaticSearch(Vec<String>);

impl graphhelm_runtime::ports::BoundedSourceSearch for StaticSearch {
    fn search(
        &self,
        _terms: &[String],
        _bounds: &graphhelm_runtime::ports::SourceSearchBounds,
    ) -> Result<Vec<String>, graphhelm_runtime::ports::SourceSearchError> {
        Ok(self.0.clone())
    }
}

struct StaticReader(&'static [u8]);

impl graphhelm_runtime::ports::BoundedSourceReader for StaticReader {
    fn read_prefix(
        &self,
        _relative_path: &str,
        max_bytes: u64,
    ) -> Result<graphhelm_runtime::ports::SourceExcerpt, graphhelm_runtime::ports::SourceReadError>
    {
        let take = usize::try_from(max_bytes)
            .unwrap_or(usize::MAX)
            .min(self.0.len());
        Ok(graphhelm_runtime::ports::SourceExcerpt {
            bytes: self.0[..take].to_vec(),
            file_len: self.0.len() as u64,
        })
    }
}

#[test]
fn the_context_ledger_records_only_nodes_that_were_dispatched() {
    let directory = tempfile::tempdir().unwrap();
    let execution_id = started_repository(directory.path());
    // `refused` is an Agent with NO `agent` block: it wants a capsule (plain cognitive work), the
    // capsule compiles, and assembly then refuses it as unassemblable. `ran` is a proper node.
    let refused = GraphNode {
        node_type: NodeType::Agent,
        name: "n".to_owned(),
        objective: "search the tree".to_owned(),
        optionality: Optionality::Required,
        properties: std::collections::BTreeMap::new(),
    };
    let spec = spec_with(
        vec![
            ("ran", agent_graph_node("search the tree")),
            ("refused", refused),
        ],
        vec![],
        2,
    );
    let model = Arc::new(FakeModelPort {
        result: Ok(reply("done")),
        calls: AtomicUsize::new(0),
    });
    let executor = Arc::new(port_executor_with(
        model.clone(),
        Arc::new(FakeToolPort {
            disposition: ToolDisposition::Completed { exit_code: 0 },
            reuse: None,
        }),
    ));
    let ports = graphhelm_runtime::context::ContextPorts {
        search: Arc::new(StaticSearch(vec!["src/tree.rs".to_owned()])),
        reader: Arc::new(StaticReader(b"fn search_the_tree() {}\n")),
        ledger: graphhelm_runtime::context::ContextLedger::new(),
        execution_tree: None,
    };
    let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(None::<ImmediateCancelRequest>);
    let projection = multi_thread_runtime()
        .block_on(drive_to_quiescence_async(
            opener(directory.path().to_path_buf()),
            Arc::new(EvidenceProtector::new(InMemoryKeyProvider::default())),
            Arc::new(SequenceIds::default()),
            driver_scope(),
            OpaqueId::parse(DRIVER_STREAM).unwrap(),
            execution_id,
            spec,
            executor,
            driver_actor(),
            std::collections::BTreeSet::new(),
            driver_actor(),
            cancel_rx,
            None,
            Some(ports.clone()),
        ))
        .unwrap();

    assert_eq!(
        model.calls.load(Ordering::SeqCst),
        1,
        "only `ran` reached the model"
    );
    assert_eq!(
        projection.node_states.get("ran"),
        Some(&NodeState::Succeeded)
    );
    assert_ne!(
        projection.node_states.get("refused"),
        Some(&NodeState::Running),
        "the refused node was never dispatched"
    );
    let ledger = ports.ledger.snapshot();
    assert_eq!(
        ledger.keys().cloned().collect::<Vec<_>>(),
        vec!["ran".to_owned()],
        "the ledger names exactly the nodes that ran with a capsule: {ledger:?}"
    );
    assert_eq!(ledger["ran"].sources, ["src/tree.rs"]);
}

// ---------------------------------------------------------------------------------------------
// #1184: a node can wait for a person.
//
// The customs pipeline was built from the fold outwards -- the park transition
// (`(Running, NeedsInput) -> WaitingInput`), the open-wait map, the `needs_you` attention
// reason, `claim`, and the `CompletionCleared` arm that writes `Succeeded` back. The one thing
// missing was the PARK: the real executor is deliberately built never to return `NeedsInput`
// (`serve/mod.rs` says so verbatim), so on every real drive a node that declared `proofKinds`
// went straight to `succeeded` and its declaration was inert. These cells pin the link.
// ---------------------------------------------------------------------------------------------

fn agent_graph_node_with_customs(objective: &str, customs: serde_json::Value) -> GraphNode {
    let mut node = agent_graph_node(objective);
    node.properties.insert(
        "completion".to_owned(),
        serde_json::json!({ "customs": customs }),
    );
    node
}

fn declared_customs(proof_kinds: serde_json::Value) -> serde_json::Value {
    let mut customs = serde_json::json!({
        "budgets": { "waitWithinSeconds": 3600, "clearanceWithinSeconds": 3600 }
    });
    if let Some(kinds) = proof_kinds.as_array() {
        customs["proofKinds"] = serde_json::Value::Array(kinds.clone());
    }
    customs
}

/// Drives one spec to quiescence with a model that always answers the same way, and hands back
/// the projection plus the model's call count.
fn drive_customs_spec(
    directory: &std::path::Path,
    execution_id: OpaqueId,
    spec: GraphSpec,
    model_result: Result<ModelReply, GatewayError>,
) -> (graphhelm_events::ExecutionProjection, usize) {
    let model = Arc::new(FakeModelPort {
        result: model_result,
        calls: AtomicUsize::new(0),
    });
    let executor = Arc::new(port_executor_with(
        model.clone(),
        Arc::new(FakeToolPort {
            disposition: ToolDisposition::Completed { exit_code: 0 },
            reuse: None,
        }),
    ));
    let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(None::<ImmediateCancelRequest>);
    let projection = multi_thread_runtime()
        .block_on(drive_to_quiescence_async(
            opener(directory.to_path_buf()),
            Arc::new(EvidenceProtector::new(InMemoryKeyProvider::default())),
            Arc::new(SequenceIds::default()),
            driver_scope(),
            OpaqueId::parse(DRIVER_STREAM).unwrap(),
            execution_id,
            spec,
            executor,
            driver_actor(),
            std::collections::BTreeSet::new(),
            driver_actor(),
            cancel_rx,
            None,
            None,
        ))
        .unwrap();
    let calls = model.calls.load(Ordering::SeqCst);
    (projection, calls)
}

/// THE CELL. Subject and control in ONE drive, so the difference between them cannot be
/// explained by the store, the executor, the clock or the pass: two nodes, same shape, same
/// reply, and the only thing that differs is whether the node's own declaration names a proof
/// kind. `gated` parks; `plain` completes.
#[test]
fn a_node_that_declares_proof_kinds_parks_instead_of_completing_itself() {
    let directory = tempfile::tempdir().unwrap();
    let execution_id = started_repository(directory.path());
    let spec = spec_with(
        vec![
            (
                "gated",
                agent_graph_node_with_customs(
                    "search",
                    declared_customs(serde_json::json!(["test_report"])),
                ),
            ),
            ("plain", agent_graph_node("search")),
        ],
        vec![],
        2,
    );
    let (projection, calls) =
        drive_customs_spec(directory.path(), execution_id, spec, Ok(reply("done")));

    // The park happens AFTER the work, never instead of it: proof kinds are evidence that the
    // work HAPPENED, so a gate that skipped the work would be asking for proof of nothing.
    assert_eq!(calls, 2, "both nodes ran their work");
    assert_eq!(
        projection.node_states.get("gated"),
        Some(&NodeState::WaitingInput),
        "a declared proof kind parks the node instead of completing it"
    );
    assert_eq!(
        projection.node_states.get("plain"),
        Some(&NodeState::Succeeded),
        "the control node in the SAME drive completes as before"
    );
    // The wait is open and the timeline says so -- the two structures `claim` reads. Without
    // both, a claim against this park is refused as `unknown_wait` and the whole pipeline is
    // unreachable, so asserting only the node state would pass on a park nobody can answer.
    assert!(
        projection.open_waits.contains_key("gated"),
        "the park must open a wait a claim can answer"
    );
    assert!(
        projection.customs_scans.get("gated").is_some_and(|scans| {
            scans
                .iter()
                .any(|scan| scan.stage == graphhelm_events::CustomsStage::Parked)
        }),
        "the park must appear on the node's customs timeline"
    );
    assert!(
        !projection.open_waits.contains_key("plain"),
        "the control node opens no wait"
    );
}

/// The attention surface is the whole point: a park nobody can SEE is a hang. This is the
/// reading the Studio badge renders.
#[test]
fn a_parked_node_reports_needs_you() {
    let directory = tempfile::tempdir().unwrap();
    let execution_id = started_repository(directory.path());
    let spec = spec_with(
        vec![(
            "gated",
            agent_graph_node_with_customs(
                "search",
                declared_customs(serde_json::json!(["test_report"])),
            ),
        )],
        vec![],
        1,
    );
    let (projection, _) =
        drive_customs_spec(directory.path(), execution_id, spec, Ok(reply("done")));
    let attention = graphhelm_execution::attention(
        &projection,
        &graphhelm_execution::AttentionInputs::default(),
    );
    assert!(
        attention.reasons().iter().any(|reason| matches!(
            reason,
            graphhelm_execution::AttentionReason::WaitingInputNode { node } if node == "gated"
        )),
        "the parked node must be named as the thing waiting on a person: {attention:?}"
    );
}

/// An empty `proofKinds` is the field's own DEFAULT, so treating it as a gate would park every
/// node that declared budgets and nothing else -- a requirement its author never wrote.
#[test]
fn a_customs_block_naming_no_proof_kind_declares_no_gate() {
    let directory = tempfile::tempdir().unwrap();
    let execution_id = started_repository(directory.path());
    let spec = spec_with(
        vec![
            (
                "absent",
                agent_graph_node_with_customs("search", declared_customs(serde_json::json!(null))),
            ),
            (
                "empty",
                agent_graph_node_with_customs("search", declared_customs(serde_json::json!([]))),
            ),
        ],
        vec![],
        2,
    );
    let (projection, _) =
        drive_customs_spec(directory.path(), execution_id, spec, Ok(reply("done")));
    assert_eq!(
        projection.node_states.get("absent"),
        Some(&NodeState::Succeeded),
        "a customs block with no proofKinds field is not a gate"
    );
    assert_eq!(
        projection.node_states.get("empty"),
        Some(&NodeState::Succeeded),
        "an explicitly empty proofKinds list is not a gate"
    );
}

/// Only a SUCCESS is converted. Parking a node that failed would ask a person to attest to work
/// that did not happen, and would take the node out of the retry path that owns it.
#[test]
fn a_node_that_failed_keeps_its_own_outcome_however_it_declared_customs() {
    let directory = tempfile::tempdir().unwrap();
    let execution_id = started_repository(directory.path());
    let spec = spec_with(
        vec![(
            "gated",
            agent_graph_node_with_customs(
                "search",
                declared_customs(serde_json::json!(["test_report"])),
            ),
        )],
        vec![],
        1,
    );
    let (projection, calls) = drive_customs_spec(
        directory.path(),
        execution_id,
        spec,
        Err(GatewayError::ProviderUnavailable),
    );
    assert!(calls >= 1, "the node was dispatched");
    assert_ne!(
        projection.node_states.get("gated"),
        Some(&NodeState::WaitingInput),
        "a failure must not be laundered into a wait for a person"
    );
    assert!(
        !projection.open_waits.contains_key("gated"),
        "a failed node opens no wait"
    );
}

/// The park carries the work's own record through UNCHANGED. The sealed reply is the material a
/// claimant cites as the proof this park waits for; a park that discarded it would be asking for
/// evidence it had just thrown away.
#[test]
fn a_parked_node_keeps_the_evidence_its_work_produced() {
    let directory = tempfile::tempdir().unwrap();
    let execution_id = started_repository(directory.path());
    let spec = spec_with(
        vec![(
            "gated",
            agent_graph_node_with_customs(
                "search",
                declared_customs(serde_json::json!(["test_report"])),
            ),
        )],
        vec![],
        1,
    );
    let (_projection, _) = drive_customs_spec(
        directory.path(),
        execution_id,
        spec,
        Ok(reply("the answer")),
    );
    let repository = opener(directory.path().to_path_buf())().unwrap();
    let history = repository
        .read_replay_stream(&driver_scope(), DRIVER_STREAM)
        .unwrap();
    let parked = history
        .iter()
        .find(|event| match &event.kind {
            EventKind::NodeOutcomeRecorded(recorded) => recorded.outcome == NodeOutcome::NeedsInput,
            _ => false,
        })
        .expect("the park is journaled as a node outcome");
    assert!(
        !parked.evidence_refs.is_empty(),
        "the sealed work travels with the park, not only with a completion"
    );
}

/// A `completion.customs` block that does not deserialize refuses the EXECUTION, before any node
/// effect -- the same shape `GHG016` uses, for the same reason: by the time the park decision
/// runs the node's work has already happened, and there is no honest answer left.
#[test]
fn an_unreadable_customs_declaration_refuses_the_execution_before_any_node_effect() {
    let directory = tempfile::tempdir().unwrap();
    let execution_id = started_repository(directory.path());
    let spec = spec_with(
        vec![
            (
                "typo",
                // `waitWithin` is not `waitWithinSeconds`, and `deny_unknown_fields` plus a
                // missing required field makes this unreadable rather than partly readable.
                agent_graph_node_with_customs(
                    "search",
                    serde_json::json!({
                        "proofKinds": ["test_report"],
                        "budgets": { "waitWithin": 3600, "clearanceWithinSeconds": 3600 }
                    }),
                ),
            ),
            (
                "fine",
                agent_graph_node_with_customs(
                    "search",
                    declared_customs(serde_json::json!(["test_report"])),
                ),
            ),
            ("plain", agent_graph_node("search")),
        ],
        vec![],
        2,
    );
    let (projection, calls) =
        drive_customs_spec(directory.path(), execution_id, spec, Ok(reply("unused")));

    assert_eq!(calls, 0, "no model may run");
    assert!(
        projection.node_states.is_empty(),
        "preflight refusal must precede even Draft -> Ready approval"
    );
    assert_eq!(
        projection.simulation_status,
        Some(graphhelm_protocols::SimulationStatus::Failed)
    );
    let repository = opener(directory.path().to_path_buf())().unwrap();
    let history = repository
        .read_replay_stream(&driver_scope(), DRIVER_STREAM)
        .unwrap();
    let diagnostics = history
        .iter()
        .find_map(|event| match &event.kind {
            EventKind::GraphValidationFailed(failed) => Some(&failed.diagnostics),
            _ => None,
        })
        .expect("stable refusal diagnostics are journaled");
    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code(), diagnostic.path().as_str()))
            .collect::<Vec<_>>(),
        vec![(
            "GHG017_CUSTOMS_DECLARATION_INVALID",
            "/spec/nodes/typo/completion"
        )],
        "only the unreadable declaration is named; a valid one and an absent one are not"
    );
}

/// THE OWNER'S FLOW, end to end: the node parks, a person claims it with the proof its own
/// declaration asked for, a clearance lands, and the node completes WITHOUT re-running its work.
/// That last part is why this does not go through `(WaitingInput, Started)`: re-dispatching the
/// node would buy the same answer a second time.
#[test]
fn a_cleared_claim_completes_a_parked_node_without_running_its_work_again() {
    let directory = tempfile::tempdir().unwrap();
    let execution_id = started_repository(directory.path());
    let node = agent_graph_node_with_customs(
        "search",
        declared_customs(serde_json::json!(["test_report"])),
    );
    let spec = spec_with(vec![("gated", node)], vec![], 1);
    let (projection, calls) = drive_customs_spec(
        directory.path(),
        execution_id.clone(),
        spec.clone(),
        Ok(reply("done")),
    );
    assert_eq!(calls, 1);
    assert_eq!(
        projection.node_states.get("gated"),
        Some(&NodeState::WaitingInput)
    );

    let repository = opener(directory.path().to_path_buf())().unwrap();
    let evidence = vec![graphhelm_protocols::ClaimEvidence {
        kind: "test_report".to_owned(),
        content_hash: WireHash::parse(format!("sha256:{}", "b".repeat(64))).unwrap(),
        size: 12,
    }];
    let required = vec!["test_report".to_owned()];
    let (claimed, _) = graphhelm_events::claim(
        &repository,
        &driver_scope(),
        DRIVER_STREAM,
        &driver_actor(),
        &OpaqueId::parse("claim-key").unwrap(),
        graphhelm_events::ClaimRequest {
            node: "gated",
            completes_wait_seq: None,
            evidence: evidence.clone(),
            attestation: graphhelm_protocols::ClaimAttestation {
                asserter: OpaqueId::parse("owner").unwrap(),
                mode: graphhelm_protocols::ClaimAttestationMode::OperatorAttested,
            },
            required_proof_kinds: &required,
        },
    )
    .unwrap();
    let graphhelm_events::ClaimOutcome::Claimed { claim_seq, .. } = claimed else {
        panic!("the claim must be accepted: {claimed:?}");
    };

    let (cleared, _) = graphhelm_events::clear(
        &repository,
        &driver_scope(),
        DRIVER_STREAM,
        &driver_actor(),
        &OpaqueId::parse("clear-key").unwrap(),
        claim_seq,
        &graphhelm_events::claim_evidence_digest(&evidence),
    )
    .unwrap();
    assert!(
        matches!(cleared, graphhelm_events::ClearanceOutcome::Cleared),
        "a machine replay against the claim's own digest clears: {cleared:?}"
    );

    // The SECOND drive: the node is already `Succeeded` by the clearance's own fold arm, so
    // nothing re-dispatches it. A model call here would mean the work was bought twice.
    let (after, calls_after) =
        drive_customs_spec(directory.path(), execution_id, spec, Ok(reply("done")));
    assert_eq!(
        after.node_states.get("gated"),
        Some(&NodeState::Succeeded),
        "a cleared claim completes the node"
    );
    assert_eq!(calls_after, 0, "the node's work is never run a second time");
    assert!(
        !after.open_waits.contains_key("gated"),
        "the wait is closed once cleared"
    );
}
