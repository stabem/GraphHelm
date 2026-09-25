//! M06 Task 4: Gate nodes execute — certified or not at all.
//!
//! The classification table is pinned WHOLE, and #425 made that literal: the pin walks
//! `NodeType::EVERY_VARIANT` rather than a hand list, because the hand list said "whole"
//! while missing `DeadLetter`. Exactly one refusal arm dies in this milestone (Gate); the
//! other SIXTEEN node types keep their 05d behavior byte-identical -- sixteen, not the
//! fifteen this header used to say, because #288 added a seventeenth variant and nothing
//! made the count go red. A Gate node runs only under a live certification against the CURRENT
//! pathogen-suite digest; its verdict is appended beside the node outcome in the same
//! durable batch, and a failing verdict is the outcome the graph routes on.

use graphhelm_protocols::NodeType;
use graphhelm_runtime::classify::{NodeWorkKind, work_kind};
use graphhelm_runtime::executor::ExecutorRefusal;

/// Every `NodeType` the enum has, put through `work_kind`, against a table written out here.
///
/// WHY EVERY VARIANT AND NOT A HAND LIST, measured on #425. This test was already called
/// "pinned whole" and named SIXTEEN of the seventeen variants; `driver_contract.rs` named the
/// same sixteen. `DeadLetter` was in neither. Changing its arm in `classify::work_kind` from
/// `Err(Unsupported)` to `Ok(Cognitive)` -- wrong but perfectly legal, so the compiler has no
/// objection -- was measured GREEN across both files:
///
/// ```text
/// error[E in the build                                        0
/// `running` blocks (both test binaries executed)              2
/// the_classification_table_is_pinned_whole                    ok
/// node_types_classify_cognitive_or_tool_and_nothing_else...   ok
/// ```
///
/// So the sub-claim "no driver dispatches a dead-letter node" rested on a match arm that no
/// assertion read. The exhaustive `match` in `classify` forces a NEW variant to be decided; it
/// does nothing about an EXISTING arm being changed, and that is the half a test has to hold.
///
/// The fix is not "add the missing name" -- that leaves the next variant in the same hole. The
/// pin now walks `NodeType::EVERY_VARIANT`, which the same macro emits as the wire-name list, so
/// a variant cannot be absent from this table without the count assertion below failing by name.
///
/// LOOKUP BY VARIANT, NEVER BY INDEX. `graph.rs` promises that reordering the variants stays a
/// cosmetic edit; pairing this table to the enum positionally would quietly turn that promise
/// into a lie, and the failure would accuse the wrong edit.
#[test]
fn the_classification_table_is_pinned_whole() {
    use NodeType::{
        Agent, ArtifactTransform, Classifier, DeadLetter, Deploy, Evaluator, Fork, Gate,
        HumanDecision, Join, Materializer, Planner, Rollback, Subgraph, Timer, Tool, Trigger,
    };

    let refused = Err(ExecutorRefusal::Unsupported);
    let expected: &[(NodeType, Result<NodeWorkKind, ExecutorRefusal>)] = &[
        // The 05d cognitive four, byte-identical.
        (Agent, Ok(NodeWorkKind::Cognitive)),
        (Planner, Ok(NodeWorkKind::Cognitive)),
        (Classifier, Ok(NodeWorkKind::Cognitive)),
        (Evaluator, Ok(NodeWorkKind::Cognitive)),
        // The 05c/05d tool path, byte-identical.
        (Tool, Ok(NodeWorkKind::Tool)),
        // The ONE arm M06 opens: deterministic gate work, no model port.
        (Gate, Ok(NodeWorkKind::GateCheck)),
        // Everything else refuses. These are "no driver yet" -- work waiting for one.
        (Fork, refused),
        (Join, refused),
        (HumanDecision, refused),
        (Timer, refused),
        (Trigger, refused),
        (Subgraph, refused),
        (Materializer, refused),
        (Deploy, refused),
        (Rollback, refused),
        (ArtifactTransform, refused),
        // #288/#425: the dead-letter node refuses for a STRONGER reason than its neighbours --
        // permanently, not pending. It shares their arm because `Unsupported` is what the
        // executor can act on today. This row is the one the old pin was missing.
        (DeadLetter, refused),
    ];

    // Non-vacuity FIRST: an empty variant list would make the loop below assert nothing while
    // reading as full coverage.
    assert!(
        !NodeType::EVERY_VARIANT.is_empty(),
        "an empty EVERY_VARIANT would satisfy the whole table vacuously"
    );

    // The count is what makes this WHOLE rather than merely long. A new variant lands here
    // before it can land anywhere else.
    assert_eq!(
        NodeType::EVERY_VARIANT.len(),
        expected.len(),
        "every NodeType variant needs a row in this table; the enum has {} and the table \
         has {}. A new variant must declare here whether the driver dispatches it -- that \
         decision is what the 1.0.0 node-type promise is made of, not the fact that \
         `classify` compiles.",
        NodeType::EVERY_VARIANT.len(),
        expected.len()
    );

    for variant in NodeType::EVERY_VARIANT {
        let (_, want) = expected
            .iter()
            .find(|(named, _)| named == variant)
            .unwrap_or_else(|| panic!("{variant:?} has no row in the classification table"));
        assert_eq!(
            &work_kind(variant),
            want,
            "{variant:?} must classify exactly as this table says"
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
    GateFinding, GraphBudgets, GraphNode, GraphSpec, IdGenerator, NewEvent, NodeState, OpaqueId,
    Optionality, PersistedActor, PersistedActorType, ProjectId, RawSha256, RepositoryScope,
    Sensitivity, SignalSeverity, WireHash, WorkspaceId,
};
use graphhelm_runtime::driver::{ImmediateCancelRequest, StoreOpen, drive_to_quiescence_async};
use graphhelm_runtime::executor::PortExecutor;
use graphhelm_runtime::ports::GateEvaluation;
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
/// A SECOND gate's suite digest (#668). Different from geometry's on purpose: a registry whose
/// two entries share a digest cannot tell "certified against its own suite" from "certified
/// against whichever suite the drive happened to be handed".
const ALPHA_DIGEST: &str =
    "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const ALPHA_GATE: &str = "gate-alpha";
const ALPHA_REFUSAL: &str = "alpha refuses every surface, and only alpha says so";

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
    started_store_for(directory, "gate-geometry", certified)
}

/// The same, for an arbitrary gate id -- what the per-gate cells (#668) need: a receipt that
/// names a gate OTHER than geometry, so "certified" and "certified as geometry" stop being the
/// same sentence.
fn started_store_for(
    directory: &std::path::Path,
    gate_id: &str,
    certified: Option<&str>,
) -> OpaqueId {
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
                gate_id: OpaqueId::parse(gate_id).unwrap(),
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
fn gate_graph_node_for(gate_id: &str, blank: bool) -> GraphNode {
    let html = if blank {
        String::new()
    } else {
        "<main id=\"panel\">Panel<ul><li>ok</li></ul></main>".to_owned()
    };
    let mut properties = std::collections::BTreeMap::new();
    properties.insert(
        "gate".to_owned(),
        serde_json::json!({"check": {
            "gateId": gate_id,
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
    gate_spec_for("gate-geometry", blank)
}

fn gate_spec_for(gate_id: &str, blank: bool) -> GraphSpec {
    GraphSpec {
        entrypoints: vec!["quality".to_owned()],
        nodes: [("quality".to_owned(), gate_graph_node_for(gate_id, blank))].into(),
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
fn port_executor(gates: Arc<dyn graphhelm_runtime::ports::GateRegistryPort>) -> Arc<PortExecutor> {
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
        gates,
    })
}

/// The registry these cells run against, in the shape the binary supplies (#668): a lookup from
/// gate id to that gate's own suite digest and its own evaluator.
///
/// Held as data rather than as a hard-coded pair so a cell can state a registry where the two
/// gates DISAGREE -- which is the only arrangement that can tell per-gate dispatch from the
/// single-evaluator behaviour it replaced.
struct TestGates {
    entries: Vec<TestGate>,
}

/// One registry entry: the gate id, the digest of ITS suite, and ITS evaluator.
type TestGate = (String, String, fn(&serde_json::Value) -> GateEvaluation);

impl graphhelm_runtime::ports::GateRegistryPort for TestGates {
    fn suite_digest(&self, gate_id: &str) -> Option<String> {
        self.entries
            .iter()
            .find(|(id, _, _)| id == gate_id)
            .map(|(_, digest, _)| digest.clone())
    }

    fn evaluate(&self, gate_id: &str, evidence: &serde_json::Value) -> Option<GateEvaluation> {
        self.entries
            .iter()
            .find(|(id, _, _)| id == gate_id)
            .map(|(_, _, evaluate)| evaluate(evidence))
    }
}

/// Geometry's real evaluator over the node's evidence -- the composition the runtime used to
/// hard-code, now supplied by whoever wires the registry.
fn geometry_evaluator(evidence: &serde_json::Value) -> GateEvaluation {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct GeometryEvidence {
        delivered: graphhelm_quality::Delivered,
        manifest: graphhelm_quality::ContentManifest,
        #[serde(default)]
        budget: graphhelm_quality::LayoutBudget,
    }
    // The real registry answers `Unreadable` here; these cells hand it evidence that parses, and
    // the unreadable path has its own cell below rather than being folded into this one.
    let Ok(evidence) = serde_json::from_value::<GeometryEvidence>(evidence.clone()) else {
        return GateEvaluation::Unreadable(
            "the fixture does not carry geometry evidence".to_owned(),
        );
    };
    GateEvaluation::Verdict(graphhelm_quality::evaluate_geometry(
        &evidence.delivered,
        &evidence.manifest,
        &evidence.budget,
    ))
}

/// A registry holding geometry alone, at `digest` -- what every pre-#668 cell in this file
/// means by "the current suite".
fn geometry_registry(digest: &str) -> Arc<dyn graphhelm_runtime::ports::GateRegistryPort> {
    Arc::new(TestGates {
        entries: vec![(
            "gate-geometry".to_owned(),
            digest.to_owned(),
            geometry_evaluator,
        )],
    })
}

fn drive(
    directory: &std::path::Path,
    execution_id: OpaqueId,
    spec: GraphSpec,
    gates: Option<Arc<dyn graphhelm_runtime::ports::GateRegistryPort>>,
) -> graphhelm_events::ExecutionProjection {
    let sealer = Arc::new(EvidenceProtector::new(InMemoryKeyProvider::default()));
    let ids = Arc::new(SequenceIds::default());
    let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(None::<ImmediateCancelRequest>);
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
            port_executor(
                gates
                    .clone()
                    .unwrap_or_else(|| geometry_registry(SUITE_DIGEST)),
            ),
            gate_actor(),
            // #123: this drive releases nothing, so the set is empty and the
            // releasing actor is never consulted.
            std::collections::BTreeSet::new(),
            gate_actor(),
            cancel_rx,
            gates,
            None,
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
        Some(geometry_registry(SUITE_DIGEST)),
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
        Some(geometry_registry(SUITE_DIGEST)),
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
        Some(geometry_registry(SUITE_DIGEST)),
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
        Some(geometry_registry(SUITE_DIGEST)),
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

// -------------------------------------------------------------------------------------------
// #668: a registered gate is certified against ITS OWN suite and judged by ITS OWN evaluator.
//
// Both cells below run a node that names `gate-alpha`, a gate the registry holds beside
// geometry. Before per-gate dispatch, the drive carried ONE digest and the executor ran ONE
// evaluator, so this node was refused as uncertified (its receipt was compared against
// geometry's digest) and, had it dispatched, would have been scored by geometry's rules.
// -------------------------------------------------------------------------------------------

/// Alpha's evaluator: refuses everything, with a finding no geometry evaluation can produce.
///
/// Constant rather than derived from the evidence deliberately. The question here is WHICH
/// evaluator ran, and a finding that could only have come from this function answers it; an
/// evaluator that merely scored differently would leave "geometry ran and disagreed" open.
fn alpha_evaluator(_evidence: &serde_json::Value) -> GateEvaluation {
    GateEvaluation::Verdict(vec![GateFinding {
        severity: SignalSeverity::High,
        claim: ALPHA_REFUSAL.to_owned(),
        evidence: vec![],
        remediation: "there is no remedy; alpha exists to be recognised".to_owned(),
    }])
}

/// A gate that can never read what this node carries -- the shape a misspelled key in a
/// `gate.check` block produces once the evidence is carried unparsed (#771, found by L).
fn unreadable_evaluator(_evidence: &serde_json::Value) -> GateEvaluation {
    GateEvaluation::Unreadable("this evaluator reads no evidence at all".to_owned())
}

/// Geometry and alpha, each at its own digest with its own evaluator.
fn two_gate_registry() -> Arc<dyn graphhelm_runtime::ports::GateRegistryPort> {
    Arc::new(TestGates {
        entries: vec![
            (
                "gate-geometry".to_owned(),
                SUITE_DIGEST.to_owned(),
                geometry_evaluator,
            ),
            (
                ALPHA_GATE.to_owned(),
                ALPHA_DIGEST.to_owned(),
                alpha_evaluator,
            ),
        ],
    })
}

/// The DIGEST half: a receipt for alpha's own suite certifies alpha.
///
/// The receipt names `gate-alpha` at `ALPHA_DIGEST`, which is NOT the digest of the geometry
/// entry the same registry carries. A verdict naming alpha is the whole assertion: it can only
/// exist if the precondition looked alpha's own suite up.
#[test]
fn a_gate_is_certified_against_the_digest_of_its_own_suite() {
    let directory = tempfile::tempdir().unwrap();
    let execution_id = started_store_for(directory.path(), ALPHA_GATE, Some(ALPHA_DIGEST));
    drive(
        directory.path(),
        execution_id,
        gate_spec_for(ALPHA_GATE, false),
        Some(two_gate_registry()),
    );
    let kinds = read_kinds(directory.path());
    assert!(
        kinds
            .iter()
            .any(|kind| kind.starts_with("gate_verdict:gate-alpha:")),
        "a gate certified against its own suite must dispatch: {kinds:?}"
    );
}

/// The EVALUATOR half: the gate the node named is the gate that judged it.
///
/// The surface is the CLEAN one -- geometry passes it, and every other cell in this file relies
/// on that. Alpha refuses it. So a passing verdict here does not mean "alpha is lenient", it
/// means geometry answered for a node that asked alpha.
#[test]
fn the_named_gates_own_evaluator_produces_the_verdict() {
    let directory = tempfile::tempdir().unwrap();
    let execution_id = started_store_for(directory.path(), ALPHA_GATE, Some(ALPHA_DIGEST));
    let projection = drive(
        directory.path(),
        execution_id,
        gate_spec_for(ALPHA_GATE, false),
        Some(two_gate_registry()),
    );
    let kinds = read_kinds(directory.path());
    assert!(
        kinds
            .iter()
            .any(|kind| kind == "gate_verdict:gate-alpha:false:1"),
        "alpha refuses every surface; a pass here is geometry answering for alpha: {kinds:?}"
    );
    assert_eq!(
        projection.node_states.get("quality"),
        Some(&NodeState::Failed),
        "alpha's refusal is deterministic, so the node is terminal"
    );
}

/// A gate the registry does not hold never dispatches, even with a receipt in the stream.
///
/// The receipt is genuine and matches nothing this build can run: an operator who certified a
/// gate on a binary that had it, and then ran a binary that does not, must be refused rather
/// than served geometry's opinion under another gate's name.
#[test]
fn an_unregistered_gate_never_dispatches_even_when_certified() {
    let directory = tempfile::tempdir().unwrap();
    let execution_id = started_store_for(directory.path(), ALPHA_GATE, Some(ALPHA_DIGEST));
    let projection = drive(
        directory.path(),
        execution_id,
        gate_spec_for(ALPHA_GATE, false),
        Some(geometry_registry(SUITE_DIGEST)),
    );
    let kinds = read_kinds(directory.path());
    assert!(
        !kinds.iter().any(|kind| kind.starts_with("gate_verdict")),
        "a gate this build cannot run must not verdict: {kinds:?}"
    );
    assert_ne!(
        projection.node_states.get("quality"),
        Some(&NodeState::Succeeded)
    );
}

/// Evidence a gate cannot READ refuses the node and appends NOTHING (#771).
///
/// **Why this is not a taste question about error shapes.** A `GateVerdict` is permanent: the
/// Event Store is append-only and historical evidence is never rewritten. A failing verdict says
/// a delivered surface was examined and refused, so emitting one for a node whose contract could
/// not even be read leaves a High-severity claim about a surface nothing looked at -- and fixing
/// the typo that caused it cannot retract the event. Before per-gate dispatch, a contract that
/// did not deserialize refused as `Unassemblable` and wrote nothing; carrying the evidence
/// unparsed moved that failure into the evaluator, and this cell pins that the OUTCOME CLASS
/// came with it.
///
/// The certification is genuine and the digest matches, so nothing upstream of the evaluator can
/// account for the refusal: the only thing that decides it is what the evaluator answered.
#[test]
fn evidence_a_gate_cannot_read_refuses_the_node_and_appends_no_verdict() {
    let directory = tempfile::tempdir().unwrap();
    let execution_id = started_store_for(directory.path(), ALPHA_GATE, Some(ALPHA_DIGEST));
    let gates: Arc<dyn graphhelm_runtime::ports::GateRegistryPort> = Arc::new(TestGates {
        entries: vec![(
            ALPHA_GATE.to_owned(),
            ALPHA_DIGEST.to_owned(),
            unreadable_evaluator,
        )],
    });
    let projection = drive(
        directory.path(),
        execution_id,
        gate_spec_for(ALPHA_GATE, false),
        Some(gates),
    );
    let kinds = read_kinds(directory.path());
    assert!(
        !kinds.iter().any(|kind| kind.starts_with("gate_verdict")),
        "unreadable evidence must leave NO permanent claim about an unexamined surface: {kinds:?}"
    );
    assert_ne!(
        projection.node_states.get("quality"),
        Some(&NodeState::Succeeded),
        "a node whose gate contract cannot be read must not pass"
    );
}
