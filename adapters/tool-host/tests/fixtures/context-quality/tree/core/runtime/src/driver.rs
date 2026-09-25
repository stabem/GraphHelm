//! The append half of the async driver (Task 6; the loop arrives in Task 8). Mirrors 04f's
//! `record_outcome` — reread, `apply_transition`, `NewEvent` — with two additions: the
//! outcome's free-form material seals BEFORE anything appends (a sealing failure appends
//! nothing — the signal command's 04f fail-closed rule generalized to node work, §6.2), and a
//! tool outcome served from the read cache appends its `ReuseDecision` beside the outcome in
//! the same atomic batch — the producer the 05c amendment promised.

use graphhelm_events::{
    EventRepositoryError, EvidenceError, EvidenceSealer, LocalEventRepository, PreparedAppend,
    ReplayError, SealedEvidence, replay,
};
use graphhelm_execution::{TransitionRequest, apply_transition};
use graphhelm_protocols::{
    DiagnosticComponent, DiagnosticDomainPath, EventEnvelope, EventKind, EvidenceId, IdGenerator,
    NewEvent, NodeOutcomeRecorded, NodeState, OpaqueId, PersistedActor, PersistedDiagnostic,
    RepositoryScope, ReuseDecision, ReusePlane, Sensitivity, Severity,
};

use crate::context_accounting::{
    ACCOUNTING_RECEIPT_MEDIA_TYPE, AccountingReceiptError, ExecutionAccountingReceipt,
};
use crate::evidence::seal_work;
use crate::executor::WorkOutcome;

/// Why the append half refused. Every variant is a refusal to record, never a partial record.
#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    #[error("the outcome's evidence could not be sealed; nothing was appended")]
    Sealing(#[from] EvidenceError),
    #[error("the event repository refused the operation")]
    Repository(#[from] EventRepositoryError),
    #[error("the stream's history does not replay")]
    Replay(#[from] ReplayError),
    #[error("the node's current state does not allow this outcome")]
    Transition,
    #[error("an identifier is not wire-safe")]
    Identity,
    #[error("the execution accounting receipt could not be built; nothing was appended")]
    Accounting(#[from] AccountingReceiptError),
}

/// What one recorded outcome left behind: the transition's result and the sealed items the
/// append carried (the local store exposes availability, not a sealed read — callers that
/// need to open what was just sealed hold it here).
pub struct RecordedOutcome {
    pub next_state: NodeState,
    pub sealed: Vec<SealedEvidence>,
}

/// Records one node outcome with its evidence in one atomic append.
///
/// Order is the invariant: reread the projection, decide the transition, seal every sealable
/// — and only then append. The appended `node_outcome_recorded` names exactly the sealed
/// references; when the outcome carries a reuse summary, a `ReuseDecision` event rides the
/// same `PreparedAppend`, so the ledger can never record a decision whose outcome vanished
/// (or the reverse).
///
/// # Errors
/// [`DriverError`] — on any error, the store is untouched.
#[allow(clippy::too_many_arguments)] // the 04f record_outcome surface plus the sealing seam
pub async fn record_outcome_with_evidence(
    store: &LocalEventRepository,
    sealer: &dyn EvidenceSealer,
    ids: &dyn IdGenerator,
    scope: &RepositoryScope,
    stream: &OpaqueId,
    execution_id: &OpaqueId,
    actor: &PersistedActor,
    node: &str,
    work: &WorkOutcome,
) -> Result<RecordedOutcome, DriverError> {
    let history = store.read_replay_stream(scope, stream.as_str())?;
    let execution_start = execution_start_event(&history, execution_id)?;
    let projection = replay(scope, stream.as_str(), &history)?;
    let current = projection
        .node_states
        .get(node)
        .copied()
        .unwrap_or(NodeState::Draft);
    let attempts = projection.node_attempts.get(node).copied().unwrap_or(0);
    let next_state = apply_transition(&TransitionRequest {
        current,
        outcome: work.outcome,
        attempts,
        identical_outcomes: projection.identical_outcomes_for(node, work.outcome),
    })
    .map_err(|_| DriverError::Transition)?;
    let node_id = OpaqueId::parse(node).map_err(|_| DriverError::Identity)?;

    // Evidence before append: a sealing failure returns here, store untouched. An attempt result also
    // carries the immutable accounting artifact built from the executor's real summary. Lifecycle
    // hops do not: they did not execute a port, and sharing their attempt-scoped evidence identity
    // with the eventual work result would create a false duplicate.
    let receipt = if is_accounted_attempt(work) {
        let receipt =
            ExecutionAccountingReceipt::from_work_summary(execution_start, &work.summary)?;
        Some(crate::executor::Sealable {
            local_ref_suffix: "accounting-receipt",
            media_type: ACCOUNTING_RECEIPT_MEDIA_TYPE,
            bytes: receipt.stable_bytes()?,
        })
    } else {
        None
    };
    let mut sealed_work =
        seal_work(sealer, scope, execution_id, node, attempts, &work.sealables).await?;
    if let Some(receipt) = receipt {
        let mut sealed_receipt =
            seal_work(sealer, scope, execution_id, node, attempts, &[receipt]).await?;
        sealed_work.evidence.append(&mut sealed_receipt.evidence);
        sealed_work
            .references
            .append(&mut sealed_receipt.references);
    }

    let mut events = vec![NewEvent::new(
        mint_key(ids, "node-outcome")?,
        actor.clone(),
        Sensitivity::Internal,
        EventKind::NodeOutcomeRecorded(NodeOutcomeRecorded {
            execution_id: execution_id.clone(),
            node_id: node_id.clone(),
            outcome: work.outcome,
            next_state,
            reason: work.reason,
        }),
        sealed_work.references.clone(),
        vec![],
    )];

    // The 05c amendment's producer obligation: a reuse-consulting outcome appends its ledger
    // entry in the same batch. The decision points at the sealed record when the summary does
    // not already carry a reference of its own.
    if let Some(summary) = &work.reuse {
        let record_reference = sealed_work
            .references
            .iter()
            .find(|reference| reference.evidence_id().as_str().ends_with("-record"))
            .map(|reference| reference.evidence_id().clone());
        let evidence_ref: Option<EvidenceId> = summary.evidence_ref.clone().or(record_reference);
        events.push(NewEvent::new(
            mint_key(ids, "reuse-decision")?,
            actor.clone(),
            Sensitivity::Internal,
            EventKind::ReuseDecision(ReuseDecision {
                execution_id: execution_id.clone(),
                node_id: Some(node_id),
                plane: ReusePlane::ToolBroker,
                decision: summary.decision,
                forced_reason: summary.forced_reason,
                freshness_class: summary.freshness_class,
                key_components: summary.key_components.clone(),
                key_digest: summary.key_digest.clone(),
                evidence_ref,
                provenance_erased: summary.provenance_erased,
            }),
            vec![],
            vec![],
        ));
    }

    // The M06 Task 4 writer obligation: a gate outcome appends its verdict in the SAME
    // batch — the graph can never route on a verdict the ledger does not carry, and the
    // ledger can never carry a verdict whose outcome vanished.
    if let Some(verdict) = &work.gate_verdict {
        let gate_id = OpaqueId::parse(&verdict.gate_id).map_err(|_| DriverError::Identity)?;
        let verdict_node = OpaqueId::parse(node).map_err(|_| DriverError::Identity)?;
        events.push(NewEvent::new(
            mint_key(ids, "gate-verdict")?,
            actor.clone(),
            Sensitivity::Internal,
            EventKind::GateVerdict(graphhelm_protocols::GateVerdict {
                execution_id: execution_id.clone(),
                node_id: verdict_node,
                gate_id,
                passed: verdict.passed,
                findings: verdict.findings.clone(),
            }),
            vec![],
            vec![],
        ));
    }

    let next_sequence = store.next_sequence(scope, stream.as_str())?;
    let request = PreparedAppend::new(
        scope.clone(),
        stream.clone(),
        next_sequence,
        events,
        sealed_work.evidence.clone(),
        vec![],
    )?;
    store.append_atomic(&request)?;
    Ok(RecordedOutcome {
        next_state,
        sealed: sealed_work.evidence,
    })
}

/// Whether this outcome carries any observation made by an executor.
///
/// The fixture bridge's deliberate shape is empty sealables plus an entirely absent summary,
/// reuse decision, and gate verdict. It represents no provider or tool attempt, so manufacturing
/// an "unavailable provider" receipt would invent work and would require a fixture-only server to
/// provision Evidence keys for bytes that describe nothing observed. Real port outcomes always
/// carry either sealable material or an observed summary value; lifecycle hops are excluded by
/// outcome even if a future caller accidentally attaches one.
fn is_accounted_attempt(work: &WorkOutcome) -> bool {
    !matches!(
        work.outcome,
        graphhelm_protocols::NodeOutcome::Started
            | graphhelm_protocols::NodeOutcome::Approved
            | graphhelm_protocols::NodeOutcome::Waived
            | graphhelm_protocols::NodeOutcome::Skipped
            | graphhelm_protocols::NodeOutcome::Invalidated
            | graphhelm_protocols::NodeOutcome::Paused
            | graphhelm_protocols::NodeOutcome::Interrupted
    ) && (!work.sealables.is_empty()
        || work.summary.input_tokens.is_some()
        || work.summary.output_tokens.is_some()
        || work.summary.exit_code.is_some()
        || work.reuse.is_some()
        || work.gate_verdict.is_some())
}

/// Return the exact persisted `execution_started` envelope, not an ID copied from the caller.
/// Its journal ID, hash, scope and full actor become the closed execution binding. No generic
/// artifact snapshots are synthesized because an event is not a repository index.
fn execution_start_event<'a>(
    history: &'a [EventEnvelope],
    execution_id: &OpaqueId,
) -> Result<&'a EventEnvelope, DriverError> {
    history
        .iter()
        .find(|envelope| {
            matches!(
                &envelope.kind,
                EventKind::ExecutionStarted(started) if &started.execution_id == execution_id
            )
        })
        .ok_or(DriverError::Identity)
}

fn mint_key(ids: &dyn IdGenerator, prefix: &'static str) -> Result<OpaqueId, DriverError> {
    OpaqueId::parse(ids.next_id(prefix)).map_err(|_| DriverError::Identity)
}

// ---------------------------------------------------------------------------------------------
// Task 8: the loop — concurrent WORK, serialized WRITES.
// ---------------------------------------------------------------------------------------------

use std::collections::BTreeSet;
use std::sync::Arc;

use graphhelm_events::ExecutionProjection;
use graphhelm_execution::{dispatch_candidates, dispatch_plan};
use graphhelm_protocols::{
    EventKind as WireEventKind, ExecutionCompleted, ExecutionPaused, GraphSpec,
    GraphValidationFailed, NodeOutcome, SimulationStatus,
};

use crate::executor::{AsyncNodeExecutor, NodeWork, WorkSummary};

/// Opens a fresh store handle per touch. The local store's `open` takes a blocking
/// OS-exclusive lock for the handle's whole lifetime, so the driver opens, uses, and drops
/// inside `spawn_blocking` — the lock never parks an async worker thread, and the OS lock
/// stays the cross-process authority (CLI concurrency is unaffected).
pub type StoreOpen =
    Arc<dyn Fn() -> Result<LocalEventRepository, EventRepositoryError> + Send + Sync>;

/// One serialized write through `spawn_blocking`: open, record, drop. The sealing future is
/// CPU-only, so `Handle::block_on` inside the blocking thread is legal and cheap.
#[allow(clippy::too_many_arguments)]
async fn write_outcome(
    store_open: &StoreOpen,
    sealer: &Arc<dyn EvidenceSealer>,
    ids: &Arc<dyn IdGenerator>,
    scope: &RepositoryScope,
    stream: &OpaqueId,
    execution_id: &OpaqueId,
    actor: &PersistedActor,
    node: String,
    work: WorkOutcome,
) -> Result<NodeState, DriverError> {
    let store_open = store_open.clone();
    let sealer = sealer.clone();
    let ids = ids.clone();
    let scope = scope.clone();
    let stream = stream.clone();
    let execution_id = execution_id.clone();
    let actor = actor.clone();
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let store = store_open()?;
        let recorded = handle.block_on(record_outcome_with_evidence(
            &store,
            sealer.as_ref(),
            ids.as_ref(),
            &scope,
            &stream,
            &execution_id,
            &actor,
            &node,
            &work,
        ))?;
        Ok(recorded.next_state)
    })
    .await
    .expect("the writer task is never cancelled")
}

/// A hop or interruption: an outcome with nothing to seal.
fn bare(outcome: NodeOutcome) -> WorkOutcome {
    WorkOutcome {
        outcome,
        sealables: Vec::new(),
        summary: WorkSummary {
            input_tokens: None,
            output_tokens: None,
            exit_code: None,
        },
        reuse: None,
        gate_verdict: None,
        // Lifecycle hops (`Approved`, `Started`) have nothing to explain, and `Interrupted`
        // IS its own cause — the triage rule reads that outcome directly
        // (`last_outcome == Interrupted`), so a restated reason here would be noise, which
        // is the other half of what M07 F3 is about.
        reason: None,
    }
}

/// One serialized plain append (completion, pause) through `spawn_blocking`.
///
/// `idempotency_key`, when supplied, is used verbatim instead of minting a fresh one (#681: the
/// immediate-stop caller's own derived key, so a retried request can be recognised as a retry by
/// `serve`'s idempotency layer). `None` keeps the original behavior -- `mint_key(ids, prefix)` --
/// for the other caller of this function, which has no caller-supplied key to attribute to.
#[allow(clippy::too_many_arguments)] // one caller-supplied key, added to an already-wide append surface (#681)
async fn append_plain(
    store_open: &StoreOpen,
    ids: &Arc<dyn IdGenerator>,
    scope: &RepositoryScope,
    stream: &OpaqueId,
    actor: &PersistedActor,
    prefix: &'static str,
    idempotency_key: Option<OpaqueId>,
    kind: WireEventKind,
) -> Result<(), DriverError> {
    let store_open = store_open.clone();
    let ids = ids.clone();
    let scope = scope.clone();
    let stream = stream.clone();
    let actor = actor.clone();
    tokio::task::spawn_blocking(move || {
        let store = store_open()?;
        let next_sequence = store.next_sequence(&scope, stream.as_str())?;
        let key = match idempotency_key {
            Some(key) => key,
            None => mint_key(ids.as_ref(), prefix)?,
        };
        let request = PreparedAppend::new(
            scope.clone(),
            stream.clone(),
            next_sequence,
            vec![NewEvent::new(
                key,
                actor,
                Sensitivity::Internal,
                kind,
                vec![],
                vec![],
            )],
            vec![],
            vec![],
        )?;
        store.append_atomic(&request)?;
        Ok(())
    })
    .await
    .expect("the writer task is never cancelled")
}

const RETRY_CAUSE_CONFLICT_CODE: &str = "GHG015_RETRY_CAUSE_CONFLICT";
const RETRY_POLICY_TYPED_CODE: &str = "GHS003_TYPED";

/// Record ADR-030's invalid-policy refusal and terminal execution settlement in one batch.
/// No node lifecycle event may precede this pair: contradictory policy is rejected before work.
async fn append_retry_policy_refusal(
    store_open: &StoreOpen,
    ids: &Arc<dyn IdGenerator>,
    scope: &RepositoryScope,
    stream: &OpaqueId,
    execution_id: &OpaqueId,
    actor: &PersistedActor,
    diagnostics: Vec<PersistedDiagnostic>,
) -> Result<(), DriverError> {
    let store_open = store_open.clone();
    let ids = ids.clone();
    let scope = scope.clone();
    let stream = stream.clone();
    let execution_id = execution_id.clone();
    let actor = actor.clone();
    tokio::task::spawn_blocking(move || {
        let store = store_open()?;
        let next_sequence = store.next_sequence(&scope, stream.as_str())?;
        let events = vec![
            NewEvent::new(
                mint_key(ids.as_ref(), "retry-policy-refused")?,
                actor.clone(),
                Sensitivity::Internal,
                WireEventKind::GraphValidationFailed(GraphValidationFailed { diagnostics }),
                vec![],
                vec![],
            ),
            NewEvent::new(
                mint_key(ids.as_ref(), "execution-completed")?,
                actor,
                Sensitivity::Internal,
                WireEventKind::ExecutionCompleted(ExecutionCompleted {
                    execution_id,
                    status: SimulationStatus::Failed,
                }),
                vec![],
                vec![],
            ),
        ];
        let request = PreparedAppend::new(
            scope.clone(),
            stream.clone(),
            next_sequence,
            events,
            vec![],
            vec![],
        )?;
        store.append_atomic(&request)?;
        Ok(())
    })
    .await
    .expect("the writer task is never cancelled")
}

/// One serialized projection reread through `spawn_blocking`.
async fn reread_async(
    store_open: &StoreOpen,
    scope: &RepositoryScope,
    stream: &OpaqueId,
) -> Result<ExecutionProjection, DriverError> {
    let store_open = store_open.clone();
    let scope = scope.clone();
    let stream = stream.clone();
    tokio::task::spawn_blocking(move || {
        let store = store_open()?;
        let history = store.read_replay_stream(&scope, stream.as_str())?;
        Ok(replay(&scope, stream.as_str(), &history)?)
    })
    .await
    .expect("the writer task is never cancelled")
}

/// Builds the work unit from the node's contract, or refuses — the driver does not dispatch
/// what the executor would refuse, and it never invents a prompt or a call.
fn build_work(
    spec: &GraphSpec,
    execution_id: &OpaqueId,
    node: &str,
    attempt: u32,
    gate_context: &GateContext<'_>,
    // #1065: the capsule compiled for this node BEFORE this call, off the reactor (the search
    // and reads are blocking filesystem work). `None` for every kind that runs without one.
    context: Option<crate::context::CompiledContext>,
) -> Result<NodeWork, crate::executor::ExecutorRefusal> {
    let graph_node = spec
        .nodes
        .get(node)
        .ok_or(crate::executor::ExecutorRefusal::Unsupported)?;
    let kind = crate::classify::work_kind(&graph_node.node_type)?;
    let mut context_summary = None;
    let (prompt, tool_call, gate_check, judge) = match kind {
        crate::classify::NodeWorkKind::Cognitive => {
            // The blind-judge specialization (M06 Task 5): an Evaluator whose contract
            // carries a `judge` block assembles from the judge's OWN diet — story and
            // surface, nothing else. A malformed judge block (unknown fields INCLUDED —
            // deny_unknown_fields is the blindness rule at this boundary) is
            // unassemblable, never silently degraded to a plain prompt. Every other
            // cognitive node assembles byte-identically to 05d.
            if graph_node.node_type == graphhelm_protocols::NodeType::Evaluator
                && let Some(block) = graph_node.properties.get("judge")
            {
                let judge: crate::judge::JudgeWork = serde_json::from_value(block.clone())
                    .map_err(|_| crate::executor::ExecutorRefusal::Unassemblable)?;
                (crate::judge::assemble(&judge), None, None, Some(judge))
            } else {
                // #1065: the capsule is the prompt's third field and enters its digest, so the
                // sealed record names exactly what the model was shown. No ports, no capsule:
                // the field is empty and the summary absent, which is today's behaviour said
                // out loud rather than assumed.
                let (text, summary) = match context {
                    Some(compiled) => (compiled.text, Some(compiled.summary)),
                    None => (String::new(), None),
                };
                context_summary = summary;
                (
                    crate::prompt::assemble_with_context(graph_node, &text)?,
                    None,
                    None,
                    None,
                )
            }
        }
        crate::classify::NodeWorkKind::Tool => {
            // The decided call comes from the node's own contract; the executor must not
            // invent one, and neither may the driver.
            let call = graph_node
                .properties
                .get("tool")
                .and_then(|tool| tool.get("call"))
                .cloned()
                .and_then(|value| serde_json::from_value(value).ok())
                .ok_or(crate::executor::ExecutorRefusal::Unassemblable)?;
            (crate::prompt::tool_placeholder(), Some(call), None, None)
        }
        crate::classify::NodeWorkKind::GateCheck => {
            let check: crate::executor::GateCheckWork = graph_node
                .properties
                .get("gate")
                .and_then(|gate| gate.get("check"))
                .cloned()
                .and_then(|value| serde_json::from_value(value).ok())
                .ok_or(crate::executor::ExecutorRefusal::Unassemblable)?;
            // Certified or not at all: the fold's receipt for THIS gate must match the
            // CURRENT digest of THIS gate's own suite. No registry configured, or no entry
            // for the gate the node named, means no way to verify — refuse, never gate on
            // unverifiable immunity. A stale receipt refuses identically.
            let current = gate_context
                .gates
                .and_then(|gates| gates.suite_digest(&check.gate_id))
                .ok_or(crate::executor::ExecutorRefusal::Uncertified)?;
            let current = current.as_str();
            let certified = gate_context
                .certifications
                .get(&check.gate_id)
                .is_some_and(|receipt| receipt == current);
            if !certified {
                return Err(crate::executor::ExecutorRefusal::Uncertified);
            }
            (crate::prompt::tool_placeholder(), None, Some(check), None)
        }
    };
    let tool_failure_semantics = if kind == crate::classify::NodeWorkKind::Tool {
        tool_failure_semantics(graph_node)?
    } else {
        crate::executor::ToolFailureSemantics::default()
    };
    Ok(NodeWork {
        execution_id: execution_id.to_string(),
        node_id: node.to_owned(),
        attempt,
        prompt,
        kind,
        tool_failure_semantics,
        tool_call,
        gate_check,
        judge,
        context: context_summary,
    })
}

/// Whether a node runs with a context capsule (#1065): plain cognitive work only. Tool and gate
/// work have no prompt, and the blind judge assembles from its own diet by design.
fn wants_context(node: &graphhelm_protocols::GraphNode) -> bool {
    matches!(
        crate::classify::work_kind(&node.node_type),
        Ok(crate::classify::NodeWorkKind::Cognitive)
    ) && !(node.node_type == graphhelm_protocols::NodeType::Evaluator
        && node.properties.contains_key("judge"))
}

/// Compile one node's capsule off the reactor: the search walks the workspace and the reads
/// touch disk, so the work runs in `spawn_blocking` exactly like every store touch above. The
/// summary is recorded on the ports' ledger here, at the one place that holds both.
async fn compile_context_async(
    ports: &crate::context::ContextPorts,
    execution_id: &OpaqueId,
    node: &str,
    attempt: u32,
    graph_node: &graphhelm_protocols::GraphNode,
) -> Result<crate::context::CompiledContext, crate::executor::ExecutorRefusal> {
    let owned_ports = ports.clone();
    let execution_id = execution_id.to_string();
    let node_id = node.to_owned();
    let graph_node = graph_node.clone();
    let compiled = tokio::task::spawn_blocking(move || {
        crate::context::compile_for_node(
            &owned_ports,
            &execution_id,
            &node_id,
            attempt,
            &graph_node,
        )
    })
    .await
    .expect("the context compile task is never cancelled")?;
    ports.ledger.record(node, compiled.summary.clone());
    Ok(compiled)
}

const TOOL_EXITED_NON_ZERO_CAUSE: &str = "tool_exited_non_zero";

fn tool_failure_semantics(
    node: &graphhelm_protocols::GraphNode,
) -> Result<crate::executor::ToolFailureSemantics, crate::executor::ExecutorRefusal> {
    let Some(retry) = node.properties.get("retry") else {
        return Ok(Default::default());
    };
    let retry = retry
        .as_object()
        .ok_or(crate::executor::ExecutorRefusal::Unassemblable)?;
    let retry_on = retry_causes(retry, "retryOn")?;
    let do_not_retry_on = retry_causes(retry, "doNotRetryOn")?;
    let retry_declared = retry_on
        .iter()
        .any(|cause| cause == TOOL_EXITED_NON_ZERO_CAUSE);
    let terminal_declared = do_not_retry_on
        .iter()
        .any(|cause| cause == TOOL_EXITED_NON_ZERO_CAUSE);
    match (retry_declared, terminal_declared) {
        (true, true) => Err(crate::executor::ExecutorRefusal::Unassemblable),
        (true, false) => Ok(crate::executor::ToolFailureSemantics::RetryEligible),
        (false, _) => Ok(crate::executor::ToolFailureSemantics::VerdictBearing),
    }
}

fn retry_causes(
    retry: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<Vec<String>, crate::executor::ExecutorRefusal> {
    retry
        .get(key)
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|_| crate::executor::ExecutorRefusal::Unassemblable)
        .map(Option::unwrap_or_default)
}

fn retry_policy_conflict_diagnostics(
    spec: &GraphSpec,
) -> Result<Vec<PersistedDiagnostic>, DriverError> {
    let mut diagnostics = Vec::new();
    for (node_id, node) in &spec.nodes {
        let Some(retry) = node.properties.get("retry") else {
            continue;
        };
        let (retry_on, do_not_retry_on, malformed) = if let Some(retry) = retry.as_object() {
            let (retry_on, retry_on_malformed) = preflight_retry_causes(retry.get("retryOn"));
            let (do_not_retry_on, do_not_retry_on_malformed) =
                preflight_retry_causes(retry.get("doNotRetryOn"));
            (
                retry_on,
                do_not_retry_on,
                retry_on_malformed || do_not_retry_on_malformed,
            )
        } else {
            (BTreeSet::new(), BTreeSet::new(), true)
        };
        let node_pointer = node_id.replace('~', "~0").replace('/', "~1");
        let path = DiagnosticDomainPath::parse(format!("/spec/nodes/{node_pointer}/retry"))
            .map_err(|_| DriverError::Identity)?;
        if retry_on.iter().any(|cause| do_not_retry_on.contains(cause)) {
            diagnostics.push(retry_policy_diagnostic(
                RETRY_CAUSE_CONFLICT_CODE,
                path.clone(),
                DiagnosticComponent::Graph,
            )?);
        }
        if malformed {
            diagnostics.push(retry_policy_diagnostic(
                RETRY_POLICY_TYPED_CODE,
                path,
                DiagnosticComponent::Schema,
            )?);
        }
    }
    Ok(diagnostics)
}

fn preflight_retry_causes(value: Option<&serde_json::Value>) -> (BTreeSet<&str>, bool) {
    let Some(value) = value else {
        return (BTreeSet::new(), false);
    };
    let Some(values) = value.as_array() else {
        return (BTreeSet::new(), true);
    };
    let mut causes = BTreeSet::new();
    let mut malformed = false;
    for value in values {
        if let Some(cause) = value.as_str() {
            causes.insert(cause);
        } else {
            malformed = true;
        }
    }
    (causes, malformed)
}

fn retry_policy_diagnostic(
    code: &str,
    path: DiagnosticDomainPath,
    component: DiagnosticComponent,
) -> Result<PersistedDiagnostic, DriverError> {
    PersistedDiagnostic::new(
        code.to_owned(),
        Severity::Error,
        path,
        component,
        None,
        None,
    )
    .map_err(|_| DriverError::Identity)
}

/// What the certified-or-not-at-all precondition reads: the fold's receipts and the gate
/// registry THIS build carries (owned by the binary that runs gates — `core` never depends
/// on `tools`, so the suites and their digests arrive as configuration).
///
/// The registry is asked PER GATE (#668). It used to be one digest — geometry's — compared
/// against every gate's receipt, so `gate-retry-lineage` and `gate-journey-contract` could
/// be certified by `quality certify` and were still refused as uncertified at dispatch.
struct GateContext<'a> {
    certifications: &'a std::collections::BTreeMap<String, String>,
    gates: Option<&'a dyn crate::ports::GateRegistryPort>,
}

/// Who asked for an immediate stop, and under what idempotency key — carried ON the cancel
/// channel itself rather than read from `actor` above, so the `execution-paused` this loop
/// appends on cancel attributes to the REQUESTING caller and carries THEIR idempotency key, not
/// the drive's own identity (#681: before this, the append always used `actor`, so the ledger
/// recorded the drive's own actor — usually `system-runtime` — for a stop a named caller asked
/// for, and a retried immediate-pause request could never be recognised as a retry because no
/// committed event ever carried its key).
#[derive(Clone)]
pub struct ImmediateCancelRequest {
    pub actor: PersistedActor,
    pub idempotency_key: OpaqueId,
}

/// Drives a started execution to quiescence — the 04f loop, async: concurrent executor
/// futures up to `max_parallel_model_calls`, every write serialized through this task (the
/// single writer that keeps `next_sequence` race-free), `tokio::select!` between completions
/// and the cancellation watch.
///
/// On cancel (§12 immediate stop, composed from existing pieces): the ports' cancel hooks run
/// first (a subprocess-backed port kills its children; an HTTP-backed port's bound is the
/// transport timeout), in-flight futures are aborted, each aborted node records `Interrupted`
/// (`(Running, Interrupted) -> Blocked`, the 04e arm — never a silent retry), and
/// `execution_paused` closes the story; recovery is what resume does.
///
/// # Errors
/// [`DriverError`] on any store, replay, sealing, or identity refusal.
#[allow(clippy::too_many_arguments)] // the 04f driver surface plus the async seams
pub async fn drive_to_quiescence_async(
    store_open: StoreOpen,
    sealer: Arc<dyn EvidenceSealer>,
    ids: Arc<dyn IdGenerator>,
    scope: RepositoryScope,
    stream: OpaqueId,
    execution_id: OpaqueId,
    spec: GraphSpec,
    executor: Arc<dyn AsyncNodeExecutor>,
    actor: PersistedActor,
    // #123: the nodes a `resume` held, released here at the first pass where their edges allow.
    // `release_actor` is the OWNER's, deliberately separate from `actor` — releasing work the
    // owner paused is the owner's act, while every ordinary hop below stays machinery.
    release: BTreeSet<String>,
    release_actor: PersistedActor,
    mut cancel: tokio::sync::watch::Receiver<Option<ImmediateCancelRequest>>,
    // #668: the gate registry this binary carries — per-gate suite digests for the
    // certified-or-not-at-all precondition below, and the evaluators the executor dispatches
    // to. `None` refuses every gate node, which is what a binary that registers no gate at
    // all should do.
    gates: Option<Arc<dyn crate::ports::GateRegistryPort>>,
    // #1065: the bounded search and reader over the project root, plus the ledger the caller
    // reads back. `None` is the pre-#1065 drive: no capsule, every context receipt field
    // `unavailable`, the prompt's context field empty.
    context: Option<crate::context::ContextPorts>,
) -> Result<ExecutionProjection, DriverError> {
    let retry_policy_diagnostics = retry_policy_conflict_diagnostics(&spec)?;
    if !retry_policy_diagnostics.is_empty() {
        let projection = reread_async(&store_open, &scope, &stream).await?;
        let already_terminal = matches!(
            projection.simulation_status,
            Some(
                SimulationStatus::Completed
                    | SimulationStatus::Failed
                    | SimulationStatus::Cancelled
            )
        );
        if !already_terminal {
            append_retry_policy_refusal(
                &store_open,
                &ids,
                &scope,
                &stream,
                &execution_id,
                &actor,
                retry_policy_diagnostics,
            )
            .await?;
        }
        return reread_async(&store_open, &scope, &stream).await;
    }

    let mut in_flight: tokio::task::JoinSet<(String, Option<WorkOutcome>)> =
        tokio::task::JoinSet::new();
    let mut in_flight_nodes: BTreeSet<String> = BTreeSet::new();
    let mut refused: BTreeSet<String> = BTreeSet::new();
    let max_parallel = graphhelm_execution::parallel_limit(&spec.budgets);

    // The `Option<ImmediateCancelRequest>` is CAPTURED at the point cancellation is detected, not
    // re-read later at the append site (Codex P1, PR #695 review). Two immediate-pause requests
    // racing the same execution could otherwise let a SECOND `send` overwrite the channel's
    // payload between "cancellation observed" and "payload fetched to attribute the append" --
    // the loop would still correctly detect a request happened, but attribute it to whichever
    // request happened to be sitting in the channel at the LATER read, not the one that actually
    // triggered this stop.
    let cancelled_request: Option<ImmediateCancelRequest> = 'passes: loop {
        if let Some(request) = cancel.borrow().clone() {
            break Some(request);
        }

        // Approve every untouched node — the legal Draft -> Ready route, as 04f does.
        let projection = reread_async(&store_open, &scope, &stream).await?;
        for node in spec.nodes.keys() {
            let state = projection
                .node_states
                .get(node)
                .copied()
                .unwrap_or(NodeState::Draft);
            if state == NodeState::Draft {
                write_outcome(
                    &store_open,
                    &sealer,
                    &ids,
                    &scope,
                    &stream,
                    &execution_id,
                    &actor,
                    node.clone(),
                    bare(NodeOutcome::Approved),
                )
                .await?;
            }
        }

        let projection = reread_async(&store_open, &scope, &stream).await?;

        // #123's RELEASE. Three conditions, and the third is load-bearing rather than defensive:
        // re-reading the aggregate every pass is what lets this section see a pause the operator
        // already recorded AT THIS REREAD, so release does not re-start a node paused before it.
        // NARROWED, NOT ELIMINATED: a pause landing after this reread and before the release
        // append below is the same check-then-act residual #562 documents at the dispatch gate
        // (`apps/cli/src/commands/execution/driver.rs:185-197`) -- not closed here either, and for
        // the same reason: closing it is a behaviour change beyond what this comment describes.
        // (This comment used to claim an ordinary pause does not stop an in-flight drive at
        // all, on the strength of the cancel channel above being signalled only for
        // `mode: immediate` — true of that channel, but no longer true of the drive: #124's own
        // PAIR of gates below in this function stops DISPATCH for an ordinary pause too — the
        // PLAN gate (`execution_paused` checked before `dispatch_plan` is built) and the
        // DISPATCH-POINT gate (the same check re-read per node, just before each dispatch). Their
        // own comment explains why removing either as "covered by the other" is how this becomes
        // a hang. Landed in 2ea6c6a7. This section's own reasoning about RELEASE never depended on
        // that claim, so only the claim moves.)
        // A node paused MID-drive is safe by construction: it is not in `release`, which was fixed
        // when the resume handed it over.
        let mut released_any = false;
        for node in &release {
            let held =
                projection.node_states.get(node.as_str()).copied() == Some(NodeState::Paused);
            let execution_paused = projection.simulation_status == Some(SimulationStatus::Paused);
            if held
                && !execution_paused
                && graphhelm_execution::edges_satisfied(&spec, &projection.node_states, node)
            {
                write_outcome(
                    &store_open,
                    &sealer,
                    &ids,
                    &scope,
                    &stream,
                    &execution_id,
                    &release_actor,
                    node.clone(),
                    bare(NodeOutcome::Started),
                )
                .await?;
                released_any = true;
            }
        }
        // Re-read ONLY when a release actually appended. Unconditional here would put a full
        // O(head) `read_replay_stream` + `replay` on EVERY pass of EVERY execution — the
        // overwhelming majority of which release nothing — which is the exact cost term this fix
        // exists to remove. It would also silently widen behaviour: the loop would start observing
        // concurrent external appends mid-pass, where before it computed candidates from the
        // projection it already held. Neither was asked for.
        let projection = if released_any {
            reread_async(&store_open, &scope, &stream).await?
        } else {
            projection
        };

        // The union (ready + retry-pending) lives in `dispatch_candidates` rather than here, so
        // the edge rule reaches BOTH halves from one implementation. It used to be built inline,
        // with the retry-pending half a bare `state == Queued` filter — which is how a node whose
        // dependencies were unmet could reach dispatch (#80).
        let candidates: BTreeSet<String> = dispatch_candidates(&spec, &projection.node_states)
            .map_err(|_| DriverError::Transition)?
            .into_iter()
            .filter(|node| !in_flight_nodes.contains(node) && !refused.contains(node))
            .collect();
        // #124: AN ORDINARY PAUSE STOPS DISPATCH. The pause route appends `execution_paused` and
        // signals nothing -- the cancel channel is written only for `mode: immediate`
        // (`serve/routes.rs`). So without this the stream records the pause at sequence T and node
        // outcomes at T+1: an operator reads `paused` while the machine keeps going, which is the
        // log-that-lies class at the exact moment of intervention.
        //
        // An EMPTY PLAN rather than a new exit, and the choice is the whole design. Anything
        // already in flight keeps running and is still joined below; when the last one lands, the
        // pass after it finds an empty plan and an empty `in_flight` and leaves through the
        // quiescence exit that was always there. Nothing is aborted, nothing is recorded
        // `Interrupted`, and `execution_paused` is not appended a second time -- all three belong
        // to immediate-stop, and #124 declined to collapse the two verbs.
        //
        // So the distinction survives with its meaning intact: immediate INTERRUPTS work,
        // ordinary DECLINES TO START MORE.
        // Read AFTER the rebind above, never before it. `projection` is replaced when a release
        // appended, so a value taken earlier in this pass describes a store that has since moved --
        // and the whole point of this check is to see an append that arrived late.
        let execution_paused = projection.simulation_status == Some(SimulationStatus::Paused);
        let plan = if execution_paused {
            Vec::new()
        } else {
            dispatch_plan(
                &candidates,
                &projection.node_attempts,
                in_flight_nodes.len(),
                max_parallel,
            )
            .map_err(|_| DriverError::Transition)?
        };

        if plan.is_empty() && in_flight.is_empty() {
            break None;
        }

        for node in &plan {
            let attempt = projection.node_attempts.get(node).copied().unwrap_or(0);
            let gate_context = GateContext {
                certifications: &projection.gate_certifications,
                gates: gates.as_deref(),
            };
            // #1065: bounded retrieval BEFORE assembly, and before the dispatch hop below is
            // written — a refused node (unassemblable) still compiled a capsule it never used,
            // which the ledger records honestly as what was prepared for it.
            // RACED against immediate cancellation, exactly as the executor futures are below
            // (Codex P1 on #1078): the search and the reads are blocking filesystem work whose
            // only bound is the channel's declared ceilings, and an unconditional await here let
            // the rest of the plan compile and dispatch after a stop was requested. The blocking
            // task itself runs to its bound and is then dropped unread; nothing after it is
            // dispatched. A refused budget refuses the node (never a silent default).
            let compiled = match (&context, spec.nodes.get(node)) {
                (Some(ports), Some(graph_node)) if wants_context(graph_node) => {
                    let compile =
                        compile_context_async(ports, &execution_id, node, attempt, graph_node);
                    tokio::pin!(compile);
                    let compiled = tokio::select! {
                        compiled = &mut compile => compiled,
                        changed = cancel.changed() => {
                            if changed.is_ok()
                                && let Some(request) = cancel.borrow().clone()
                            {
                                break 'passes Some(request);
                            }
                            compile.await
                        }
                    };
                    match compiled {
                        Ok(compiled) => Some(compiled),
                        Err(_) => {
                            refused.insert(node.clone());
                            continue;
                        }
                    }
                }
                _ => None,
            };
            let work =
                match build_work(&spec, &execution_id, node, attempt, &gate_context, compiled) {
                    Ok(work) => work,
                    Err(_) => {
                        // A refusal is a refusal: the node is simply never dispatched. It stays
                        // whatever state it is in; the driver moves on.
                        refused.insert(node.clone());
                        continue;
                    }
                };
            // Dispatch hops flow through the same writer BEFORE the executor future starts,
            // so sequencing matches 04f exactly.
            // ASKED AT THE POINT OF DISPATCH, from the projection this point actually holds.
            // The plan was computed from a read taken earlier in the pass; a pause appended since
            // then is invisible to it, and this per-node reread is the only place that can see it.
            // Node state alone cannot stand in: an ordinary pause moves NO node state (it sets
            // `simulation_status` and nothing else), so a planned node is still `Ready` here and
            // sails through the gate below with the pause already in the log. Blindness by
            // construction, not by race.
            //
            // THIS BREAK NEEDS THE PLAN GATE ABOVE TO TERMINATE, and the two are not redundant.
            // Measured by removing each alone: with the plan gate gone this loop SPINS FOREVER --
            // the plan stays non-empty, every node breaks out before dispatch, `in_flight` stays
            // empty, and the `plan.is_empty() && in_flight.is_empty()` exit is never reached.
            // One stops a dispatch mid-plan; the other ends the pass. Removing either as
            // "covered by the other" is how this becomes a hang.
            let at_dispatch = reread_async(&store_open, &scope, &stream).await?;
            if at_dispatch.simulation_status == Some(SimulationStatus::Paused) {
                break;
            }
            let current = at_dispatch
                .node_states
                .get(node)
                .copied()
                .unwrap_or(NodeState::Draft);
            if current == NodeState::Ready {
                write_outcome(
                    &store_open,
                    &sealer,
                    &ids,
                    &scope,
                    &stream,
                    &execution_id,
                    &actor,
                    node.clone(),
                    bare(NodeOutcome::Started),
                )
                .await?;
            }
            write_outcome(
                &store_open,
                &sealer,
                &ids,
                &scope,
                &stream,
                &execution_id,
                &actor,
                node.clone(),
                bare(NodeOutcome::Started),
            )
            .await?;

            let executor = executor.clone();
            let node_name = node.clone();
            in_flight_nodes.insert(node.clone());
            in_flight.spawn(async move {
                let outcome = executor.execute(&work).await.ok();
                (node_name, outcome)
            });
        }

        if in_flight.is_empty() {
            continue;
        }
        tokio::select! {
            joined = in_flight.join_next() => {
                if let Some(Ok((node, outcome))) = joined {
                    in_flight_nodes.remove(&node);
                    match outcome {
                        Some(work) => {
                            write_outcome(
                                &store_open, &sealer, &ids, &scope, &stream,
                                &execution_id, &actor, node, work,
                            )
                            .await?;
                        }
                        None => {
                            // A runtime refusal after a legal dispatch: never dispatch it
                            // again — the executor refused to attempt, so there is no
                            // outcome to invent.
                            refused.insert(node);
                        }
                    }
                }
            }
            changed = cancel.changed() => {
                if changed.is_ok()
                    && let Some(request) = cancel.borrow().clone()
                {
                    break Some(request);
                }
            }
        }
    };

    if let Some(request) = cancelled_request {
        // §12 immediate stop: hooks first (blocking work dies for real), then abort the
        // futures, then record the truth — Interrupted, never a silent retry.
        executor.cancel_all();
        in_flight.abort_all();
        while in_flight.join_next().await.is_some() {}
        for node in std::mem::take(&mut in_flight_nodes) {
            write_outcome(
                &store_open,
                &sealer,
                &ids,
                &scope,
                &stream,
                &execution_id,
                &actor,
                node,
                bare(NodeOutcome::Interrupted),
            )
            .await?;
        }
        // `request` is the SAME value that triggered this branch above -- captured once, at
        // detection, never re-read (#681: attribute and key both come from the caller who asked
        // to pause, not from the drive's own `actor`; the earlier re-read was Codex's P1 finding
        // on this PR -- a second racing request could overwrite the channel between detection
        // and a later fetch).
        //
        // #695, Codex P1: a GRACEFUL pause can commit `execution-paused` while THIS drive is
        // still draining the very node this immediate request is about to interrupt -- graceful
        // holds only `Ready`/`Queued` nodes and appends its own `ExecutionPaused` immediately
        // (`apps/cli/src/commands/execution/pause.rs`), never waiting for what's in flight. The
        // projection's own fold for `ExecutionPaused` accepts the `None`/`Running` -> `Paused`
        // transition exactly once (`core/events/src/projection.rs`); a second one while already
        // `Paused` is `ReplayError::Corrupt`, and every future replay of this stream -- including
        // this very append's own `reread_async` below -- would fail from that point on. Re-read
        // fresh, right before the append, rather than trust the projection this function loaded
        // earlier: that read predates the abort/join work above, which is exactly where a
        // concurrent graceful append has room to land. If it already landed, this request's own
        // escalation is fully expressed by the `Interrupted` outcomes already written above --
        // interrupting the node now, instead of letting it drain, is the whole difference
        // immediate mode promises over graceful, and neither half of that needs a second
        // aggregate event to be true. Skipping the append here, not skipping the abort above, is
        // what keeps the promise without touching the guard that makes a duplicate corrupt.
        let already_paused = reread_async(&store_open, &scope, &stream)
            .await?
            .simulation_status
            == Some(SimulationStatus::Paused);
        if !already_paused {
            append_plain(
                &store_open,
                &ids,
                &scope,
                &stream,
                &request.actor,
                "execution-paused",
                Some(request.idempotency_key),
                WireEventKind::ExecutionPaused(ExecutionPaused {
                    execution_id: execution_id.clone(),
                }),
            )
            .await?;
        }
        return reread_async(&store_open, &scope, &stream).await;
    }

    // Quiescence: complete when every node is terminal, exactly as 04f's
    // complete_if_quiesced decides it.
    let projection = reread_async(&store_open, &scope, &stream).await?;
    let already_terminal = matches!(
        projection.simulation_status,
        Some(SimulationStatus::Completed | SimulationStatus::Failed | SimulationStatus::Cancelled)
    );
    let all_terminal = spec.nodes.keys().all(|node| {
        matches!(
            projection.node_states.get(node),
            Some(
                NodeState::Succeeded
                    | NodeState::Failed
                    | NodeState::Waived
                    | NodeState::Skipped
                    | NodeState::Cancelled
            )
        )
    });
    if all_terminal && !already_terminal {
        let all_success = spec.nodes.keys().all(|node| {
            matches!(
                projection.node_states.get(node),
                Some(NodeState::Succeeded | NodeState::Waived | NodeState::Skipped)
            )
        });
        append_plain(
            &store_open,
            &ids,
            &scope,
            &stream,
            &actor,
            "execution-completed",
            None,
            WireEventKind::ExecutionCompleted(ExecutionCompleted {
                execution_id: execution_id.clone(),
                status: if all_success {
                    SimulationStatus::Completed
                } else {
                    SimulationStatus::Failed
                },
            }),
        )
        .await?;
    }
    reread_async(&store_open, &scope, &stream).await
}
