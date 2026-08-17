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
    EventKind, EvidenceId, IdGenerator, NewEvent, NodeOutcomeRecorded, NodeState, OpaqueId,
    PersistedActor, RepositoryScope, ReuseDecision, ReusePlane, Sensitivity,
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

    // Evidence before append: a sealing failure returns here, store untouched.
    let sealed_work =
        seal_work(sealer, scope, execution_id, node, attempts, &work.sealables).await?;

    let mut events = vec![NewEvent::new(
        mint_key(ids, "node-outcome")?,
        actor.clone(),
        Sensitivity::Internal,
        EventKind::NodeOutcomeRecorded(NodeOutcomeRecorded {
            execution_id: execution_id.clone(),
            node_id: node_id.clone(),
            outcome: work.outcome,
            next_state,
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

fn mint_key(ids: &dyn IdGenerator, prefix: &'static str) -> Result<OpaqueId, DriverError> {
    OpaqueId::parse(ids.next_id(prefix)).map_err(|_| DriverError::Identity)
}

// ---------------------------------------------------------------------------------------------
// Task 8: the loop — concurrent WORK, serialized WRITES.
// ---------------------------------------------------------------------------------------------

use std::collections::BTreeSet;
use std::sync::Arc;

use graphhelm_events::ExecutionProjection;
use graphhelm_execution::{dispatch_plan, ready_set};
use graphhelm_protocols::{
    EventKind as WireEventKind, ExecutionCompleted, ExecutionPaused, GraphSpec, NodeOutcome,
    SimulationStatus,
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
    }
}

/// One serialized plain append (completion, pause) through `spawn_blocking`.
async fn append_plain(
    store_open: &StoreOpen,
    ids: &Arc<dyn IdGenerator>,
    scope: &RepositoryScope,
    stream: &OpaqueId,
    actor: &PersistedActor,
    prefix: &'static str,
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
        let request = PreparedAppend::new(
            scope.clone(),
            stream.clone(),
            next_sequence,
            vec![NewEvent::new(
                mint_key(ids.as_ref(), prefix)?,
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
) -> Result<NodeWork, crate::executor::ExecutorRefusal> {
    let graph_node = spec
        .nodes
        .get(node)
        .ok_or(crate::executor::ExecutorRefusal::Unsupported)?;
    let kind = crate::classify::work_kind(&graph_node.node_type)?;
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
                (crate::prompt::assemble(graph_node)?, None, None, None)
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
            // CURRENT suite digest. No digest configured means no way to verify — refuse,
            // never gate on unverifiable immunity. A stale receipt refuses identically.
            let current = gate_context
                .current_suite_digest
                .ok_or(crate::executor::ExecutorRefusal::Uncertified)?;
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
    Ok(NodeWork {
        execution_id: execution_id.to_string(),
        node_id: node.to_owned(),
        attempt,
        prompt,
        kind,
        tool_call,
        gate_check,
        judge,
    })
}

/// What the certified-or-not-at-all precondition reads: the fold's receipts and the
/// digest of the pathogen suite THIS build carries (computed by the binary that runs
/// gates — `core` never depends on `tools`, so the digest arrives as configuration).
struct GateContext<'a> {
    certifications: &'a std::collections::BTreeMap<String, String>,
    current_suite_digest: Option<&'a str>,
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
    mut cancel: tokio::sync::watch::Receiver<bool>,
    current_suite_digest: Option<String>,
) -> Result<ExecutionProjection, DriverError> {
    let mut in_flight: tokio::task::JoinSet<(String, Option<WorkOutcome>)> =
        tokio::task::JoinSet::new();
    let mut in_flight_nodes: BTreeSet<String> = BTreeSet::new();
    let mut refused: BTreeSet<String> = BTreeSet::new();
    let max_parallel = match spec.budgets.max_parallel_model_calls {
        None => 1_usize,
        Some(value) => usize::try_from(value).unwrap_or(usize::MAX),
    };

    let cancelled = loop {
        if *cancel.borrow() {
            break true;
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
        let ready =
            ready_set(&spec, &projection.node_states).map_err(|_| DriverError::Transition)?;
        let retry_pending = projection
            .node_states
            .iter()
            .filter(|(_, state)| **state == NodeState::Queued)
            .map(|(node, _)| node.clone());
        let candidates: BTreeSet<String> = ready
            .into_iter()
            .chain(retry_pending)
            .filter(|node| !in_flight_nodes.contains(node) && !refused.contains(node))
            .collect();
        let plan = dispatch_plan(
            &candidates,
            &projection.node_attempts,
            in_flight_nodes.len(),
            max_parallel,
        )
        .map_err(|_| DriverError::Transition)?;

        if plan.is_empty() && in_flight.is_empty() {
            break false;
        }

        for node in &plan {
            let attempt = projection.node_attempts.get(node).copied().unwrap_or(0);
            let gate_context = GateContext {
                certifications: &projection.gate_certifications,
                current_suite_digest: current_suite_digest.as_deref(),
            };
            let work = match build_work(&spec, &execution_id, node, attempt, &gate_context) {
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
            let current = reread_async(&store_open, &scope, &stream)
                .await?
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
                if changed.is_ok() && *cancel.borrow() {
                    break true;
                }
            }
        }
    };

    if cancelled {
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
        append_plain(
            &store_open,
            &ids,
            &scope,
            &stream,
            &actor,
            "execution-paused",
            WireEventKind::ExecutionPaused(ExecutionPaused {
                execution_id: execution_id.clone(),
            }),
        )
        .await?;
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
