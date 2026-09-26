//! #1063: the resume briefing - what a harness that was not there needs in order to pick an
//! execution up from the store alone. ONE typed value, derived from the projection, the attention
//! answer and the event history, rendered identically by `execution briefing`,
//! `GET /v1/executions/{id}/briefing` and the MCP `briefing` tool.
//!
//! Nothing here reads a clock, measures an age or recomputes attention: `answer` is the verdict
//! the surface already computed and the briefing COPIES its reasons, so the two can never
//! disagree. Every instant a reader might want is a SEQUENCE - `decisions[*].sequence`,
//! `asOfSequence` - because a sequence is a fact of the log and an elapsed duration is not
//! (the same rule `render()` documents for `lastEventAt`).
//!
//! The decision digest is the part no existing view carried. Approvals, waivers, budget
//! amendments, mode changes, pauses, claims and clearances all exist as separate events, and a
//! second harness reading the raw tail had to fold "who decided what" by hand. Here they are
//! folded ONCE, in sequence order, each with the actor the envelope recorded - never an actor
//! the payload claims, because the envelope's is the one the store authenticated.

use std::collections::BTreeMap;

use graphhelm_events::{ClearanceOutcome, ExecutionProjection};
use graphhelm_protocols::{
    ClaimAttestationMode, ClearanceVerifier, DeclaredExecutor, EventEnvelope, EventKind,
    ExecutionMode, NodeOutcome, NodeOutcomeReason, NodeState, PersistedActor, SimulationStatus,
    WireHash,
};
use serde::{Deserialize, Serialize};

use crate::attention::{Attention, AttentionReason, NodeUnevaluated, Unevaluated};
use crate::transition::is_terminal;

/// The whole briefing, as every surface publishes it under `data`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Briefing {
    /// The graph document's `metadata.name`, as declared at start. `None` for a history written
    /// before the declaration carried it - absent, never invented.
    pub name: Option<String>,
    /// The operator's request in their own words (the first entrypoint's objective), as
    /// declared at start.
    pub objective: Option<String>,
    /// What `start` was going to run the nodes with.
    pub executor: Option<DeclaredExecutor>,
    /// The hash of the graph the run started from, so a reader can verify the file
    /// `resume --file` needs before trusting it.
    pub graph_hash: Option<WireHash>,
    pub graph_version: Option<u64>,
    /// Every decision in the history, by sequence, with the actor the envelope recorded.
    pub decisions: Vec<Decision>,
    /// Every node in a terminal state, with how it got there.
    pub work_done: Vec<WorkItem>,
    /// A COPY of the attention answer's reasons - never recomputed here.
    pub pending: Vec<AttentionReason>,
    /// A copy of what the attention answer could not judge, for the same reason.
    pub unevaluated: Vec<Unevaluated>,
    pub next_step: NextStep,
    /// The last sequence this briefing was folded from: 0 for an empty history.
    pub as_of_sequence: u64,
}

/// One decision somebody made about this execution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    pub sequence: u64,
    /// `{type, id}` as the envelope records it.
    pub actor: PersistedActor,
    pub kind: DecisionKind,
    /// The node the decision was about, when it was about one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    /// One line, built only from journal data - never provider prose or a path.
    pub detail: String,
}

/// The closed set of decision kinds the digest folds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionKind {
    Approval,
    Waiver,
    BudgetAmended,
    ModeChanged,
    Paused,
    Resumed,
    Claim,
    Clearance,
    /// A clearance withheld: `completion_rejected`, or a `completion_cleared` whose replay
    /// outcome the fold recorded as `Refused` (a machine-replay hash mismatch).
    Rejection,
    /// A claim the command layer would not accept (`completion_refused`): stale rendezvous,
    /// duplicate completion, evidence budget unmet. The node stays parked; the code says why.
    Refusal,
    MutationAccepted,
    Cancelled,
}

/// One node that reached a terminal state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkItem {
    pub node: String,
    pub state: NodeState,
    pub attempts: u32,
    pub last_outcome: Option<NodeOutcome>,
    /// The reason recorded on the node's LAST outcome, when one was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<NodeOutcomeReason>,
}

/// What a harness picking this run up should do first, derived from the projection and the
/// attention answer and from nothing else.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum NextStep {
    /// The execution is paused: run `resume` with the graph file it started from. `command` is a
    /// template - the graph path and the store path are the caller's, and only the execution id
    /// is a fact of the log.
    ResumeHeld { command: String, nodes: Vec<String> },
    /// A named node is waiting on an operator, and this is the verb that answers it. `claimSeq`
    /// is present only for `clear`: the sequence of the open claim the countersignature names.
    Answer {
        node: String,
        remedy: AnswerRemedy,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        claim_seq: Option<u64>,
    },
    /// Something needs you and no single verb answers it: a node that failed for good, a run
    /// that says `running` while nothing can move, a wake lease burned by a writer that is not
    /// this server. The reason travels whole, so the harness reads the node's events and
    /// evidence (or checks who is writing to the store) and then decides - typically `cancel`,
    /// or a corrected graph. Never `nothing` while `pending` names a need: a briefing that lists
    /// a need and no action is the hand-off this feature exists to prevent.
    Diagnose { reason: AttentionReason },
    /// Nodes are ready or queued and nobody is needed: the next `resume` (or the running
    /// driver) dispatches them.
    Dispatch { nodes: Vec<String> },
    /// The run is over; the status says how.
    Finished { status: SimulationStatus },
    /// Nothing is held, nothing is waiting on you, nothing is dispatchable.
    Nothing,
}

/// The operator verb that answers a pending reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnswerRemedy {
    /// `approve`: a blocked or interrupted node needs an owner's decision.
    Approve,
    /// `claim`: a node parked on input needs its work claimed with evidence.
    Claim,
    /// `clear`: the node's claim is already open and quarantined; what it needs is the
    /// countersignature, not a second claim (which the door refuses as `duplicate_completion`).
    Clear,
    /// `amend_budget`: a node's silence cannot be judged until a bound is declared. ONLY for
    /// `Unevaluated::Node { reason: NoDeclaredBudget }` - never for a `SilentNode`, whose age
    /// already EXCEEDS its bound; telling the next harness to raise that bound is the purchased
    /// calm `Verdict::CalmedByAmendment` exists to expose.
    AmendBudget,
}

/// Build the briefing. Pure: the same three inputs always produce the same value.
#[must_use]
pub fn briefing_view(
    projection: &ExecutionProjection,
    answer: &Attention,
    history: &[EventEnvelope],
) -> Briefing {
    let declared = projection.declared_form.as_ref();
    let started = history.iter().find_map(|event| match &event.kind {
        EventKind::ExecutionStarted(payload) => Some(payload),
        _ => None,
    });

    Briefing {
        name: declared.and_then(|form| form.name.clone()),
        objective: declared.and_then(|form| form.objective.clone()),
        executor: declared.and_then(|form| form.executor),
        graph_hash: started.map(|payload| payload.graph_hash.clone()),
        graph_version: started.map(|payload| payload.graph_version),
        decisions: decisions(projection, history),
        work_done: work_done(projection, history),
        pending: answer.reasons().to_vec(),
        unevaluated: answer.silence_unevaluated().to_vec(),
        next_step: next_step(projection, answer),
        as_of_sequence: history.last().map_or(0, |event| event.sequence),
    }
}

/// Walk the history once, in the order it was written, and name every decision.
fn decisions(projection: &ExecutionProjection, history: &[EventEnvelope]) -> Vec<Decision> {
    // A clearance names a CLAIM sequence, not a node; the node is on the claim it answers.
    let claimed_node: BTreeMap<u64, String> = history
        .iter()
        .filter_map(|event| match &event.kind {
            EventKind::CompletionClaimed(payload) => {
                Some((event.sequence, payload.node.as_str().to_owned()))
            }
            _ => None,
        })
        .collect();

    // The state each node was in BEFORE the event being read, folded from the outcomes so far.
    // An `Approved` outcome is the owner's decision only when it readied a node the door lets
    // `approve` touch - `Blocked` or `Ghost`. The driver records the same outcome for its own
    // pre-dispatch hops (`Draft` -> `Ready`), and those are nobody's decision.
    let mut state_before: BTreeMap<&str, NodeState> = BTreeMap::new();

    history
        .iter()
        .filter_map(|event| {
            let (kind, node, detail) = match &event.kind {
                EventKind::GhostNodeProposed(payload) => {
                    state_before.insert(payload.node_id.as_str(), NodeState::Ghost);
                    return None;
                }
                EventKind::NodeOutcomeRecorded(payload) => {
                    let previous =
                        state_before.insert(payload.node_id.as_str(), payload.next_state);
                    if payload.outcome != NodeOutcome::Approved
                        || !matches!(previous, Some(NodeState::Blocked | NodeState::Ghost))
                    {
                        return None;
                    }
                    let node = payload.node_id.as_str().to_owned();
                    let detail = format!("{node} approved -> {}", payload.next_state.wire_name());
                    (DecisionKind::Approval, Some(node), detail)
                }
                EventKind::PolicyWaiverCreated(payload) => {
                    let waiver = &payload.waiver;
                    let mut detail = format!("waiver {} for {}", waiver.id, waiver.requirement);
                    if let Some(reason) = &waiver.reason {
                        detail.push_str(": ");
                        detail.push_str(reason);
                    }
                    (DecisionKind::Waiver, None, detail)
                }
                EventKind::ExecutionFormAmended(payload) => {
                    let budgets: Vec<String> = payload
                        .node_timeout_seconds
                        .iter()
                        .map(|(node, seconds)| format!("{}={seconds}s", node.as_str()))
                        .collect();
                    let detail = format!(
                        "budgets {} (computed at #{})",
                        budgets.join(", "),
                        payload.computed_at_sequence
                    );
                    (DecisionKind::BudgetAmended, None, detail)
                }
                EventKind::ExecutionModeChanged(payload) => {
                    let detail = match payload.previous_mode {
                        Some(previous) => {
                            format!("{} -> {}", mode_label(previous), mode_label(payload.mode))
                        }
                        None => format!("-> {}", mode_label(payload.mode)),
                    };
                    (DecisionKind::ModeChanged, None, detail)
                }
                EventKind::ExecutionPaused(_) => {
                    (DecisionKind::Paused, None, "execution paused".to_owned())
                }
                EventKind::ExecutionResumed(_) => {
                    (DecisionKind::Resumed, None, "execution resumed".to_owned())
                }
                EventKind::CompletionClaimed(payload) => {
                    let node = payload.node.as_str().to_owned();
                    let detail = format!(
                        "claim answering wait #{} with {} evidence item(s), {}",
                        payload.completes_wait_seq,
                        payload.evidence.len(),
                        attestation_label(payload.attestation.mode)
                    );
                    (DecisionKind::Claim, Some(node), detail)
                }
                EventKind::CompletionCleared(payload) => {
                    // The event says a clearance was OFFERED; the fold says whether it took.
                    // A machine replay whose manifest hash does not match the journaled
                    // evidence digest is recorded as `Refused` and leaves the node parked, and
                    // a briefing that called that "cleared" would hand off a false history.
                    let (kind, detail) = match projection.clearances.get(&payload.claim_seq) {
                        Some(ClearanceOutcome::Refused { reason_code }) => (
                            DecisionKind::Rejection,
                            format!(
                                "claim #{} clearance by {} refused: {}",
                                payload.claim_seq,
                                verifier_label(&payload.verifier),
                                reason_code.as_str()
                            ),
                        ),
                        Some(ClearanceOutcome::Cleared) | None => (
                            DecisionKind::Clearance,
                            format!(
                                "claim #{} cleared by {}",
                                payload.claim_seq,
                                verifier_label(&payload.verifier)
                            ),
                        ),
                    };
                    (kind, claimed_node.get(&payload.claim_seq).cloned(), detail)
                }
                EventKind::CompletionRefused(payload) => {
                    let detail = format!(
                        "claim for wait #{} refused: {}",
                        payload.claimed_wait_seq,
                        payload.reason_code.as_str()
                    );
                    (
                        DecisionKind::Refusal,
                        Some(payload.node.as_str().to_owned()),
                        detail,
                    )
                }
                EventKind::CompletionRejected(payload) => {
                    let detail = format!(
                        "claim #{} rejected by {}: {}",
                        payload.claim_seq,
                        verifier_label(&payload.verifier),
                        payload.reason_code.as_str()
                    );
                    (
                        DecisionKind::Rejection,
                        claimed_node.get(&payload.claim_seq).cloned(),
                        detail,
                    )
                }
                EventKind::MutationAccepted(payload) => {
                    let detail = format!(
                        "draft {} accepted under {}; graph version {}",
                        payload.draft_id.as_str(),
                        mode_label(payload.mode),
                        payload.graph_version
                    );
                    (DecisionKind::MutationAccepted, None, detail)
                }
                EventKind::ExecutionCompleted(payload)
                    if payload.status == SimulationStatus::Cancelled =>
                {
                    (
                        DecisionKind::Cancelled,
                        None,
                        "execution cancelled".to_owned(),
                    )
                }
                _ => return None,
            };
            Some(Decision {
                sequence: event.sequence,
                actor: event.actor.clone(),
                kind,
                node,
                detail,
            })
        })
        .collect()
}

/// Every node in a terminal state, ascending by id (`node_states` is a `BTreeMap`).
fn work_done(projection: &ExecutionProjection, history: &[EventEnvelope]) -> Vec<WorkItem> {
    // The reason lives on the outcome event, not in the fold: the LAST one per node.
    let mut last_reason: BTreeMap<&str, Option<NodeOutcomeReason>> = BTreeMap::new();
    for event in history {
        if let EventKind::NodeOutcomeRecorded(payload) = &event.kind {
            last_reason.insert(payload.node_id.as_str(), payload.reason);
        }
    }
    projection
        .node_states
        .iter()
        .filter(|(_, state)| is_terminal(**state))
        .map(|(node, state)| WorkItem {
            node: node.clone(),
            state: *state,
            attempts: projection.node_attempts.get(node).copied().unwrap_or(0),
            last_outcome: projection.last_outcome.get(node).copied(),
            reason: last_reason.get(node.as_str()).copied().flatten(),
        })
        .collect()
}

fn next_step(projection: &ExecutionProjection, answer: &Attention) -> NextStep {
    match projection.simulation_status.as_ref() {
        Some(
            status @ (SimulationStatus::Completed
            | SimulationStatus::Failed
            | SimulationStatus::Cancelled),
        ) => {
            return NextStep::Finished {
                status: status.clone(),
            };
        }
        Some(SimulationStatus::Paused | SimulationStatus::Running | SimulationStatus::Blocked)
        | None => {}
    }

    // A reason with a verb of its own comes first, PAUSED OR NOT: `approve` and `claim` are
    // legal under a pause (neither checks the run's status), and readying the blocked node
    // before the resume is the shorter path - a resume first would only re-park it.
    let named = answer.reasons().iter().find_map(|reason| match reason {
        AttentionReason::UntriagedInterruption { node } | AttentionReason::BlockedNode { node } => {
            Some((node.clone(), AnswerRemedy::Approve, None))
        }
        // A parked node whose claim is ALREADY open is quarantined testimony awaiting the
        // countersignature: a second claim is refused as `duplicate_completion`, so the verb is
        // `clear`, naming the claim by its sequence. Oldest first, `open_claims` being ordered.
        AttentionReason::WaitingInputNode { node } => Some(
            match projection
                .open_claims
                .iter()
                .find(|(_, claim)| claim.node == *node)
            {
                Some((seq, _)) => (node.clone(), AnswerRemedy::Clear, Some(*seq)),
                None => (node.clone(), AnswerRemedy::Claim, None),
            },
        ),
        // A silent node's age EXCEEDS its declared bound. Raising the bound is not an answer,
        // it is purchased calm; the node needs looking at, so it falls to `Diagnose` below.
        AttentionReason::SilentNode { .. }
        | AttentionReason::FailedNode { .. }
        | AttentionReason::WedgedQuiescence
        | AttentionReason::ForeignWakeConsumption { .. } => None,
    });
    if let Some((node, remedy, claim_seq)) = named {
        return NextStep::Answer {
            node,
            remedy,
            claim_seq,
        };
    }
    // Every reason with no verb of its own still gets an action, and it comes BEFORE the
    // resume: a foreign wake burn persists in `wake_mis_burns` across a pause, and a harness
    // told to resume first would resume without investigating who else writes to this store.
    // `pending` and `next_step` derive from the same answer, so a non-empty `pending` can never
    // sit beside `nothing`.
    if let Some(reason) = answer.reasons().first() {
        return NextStep::Diagnose {
            reason: reason.clone(),
        };
    }
    if projection.simulation_status == Some(SimulationStatus::Paused) {
        // The held nodes are the explicitly `Paused` ones PLUS every declared node the driver
        // never touched: a `start --held` commits the shape and the hold without a single
        // outcome, so its whole graph is absent from `node_states` and is exactly what the
        // resume releases.
        let mut nodes = nodes_in(projection, |state| state == NodeState::Paused);
        nodes.extend(untouched_declared(projection));
        nodes.sort();
        return NextStep::ResumeHeld {
            command: format!(
                "graphhelm execution resume --file <graph> --events <store> --execution {}",
                projection.execution_id.as_deref().unwrap_or("<execution>")
            ),
            nodes,
        };
    }
    let undeclared = || {
        answer
            .silence_unevaluated()
            .iter()
            .find_map(|item| match item {
                Unevaluated::Node {
                    node,
                    reason: NodeUnevaluated::NoDeclaredBudget,
                    ..
                } => Some((node.clone(), AnswerRemedy::AmendBudget)),
                Unevaluated::Node { .. } | Unevaluated::Execution { .. } => None,
            })
    };
    if let Some((node, remedy)) = undeclared() {
        return NextStep::Answer {
            node,
            remedy,
            claim_seq: None,
        };
    }

    // `Draft` is pre-dispatch work the driver approves on its own pass, and a declared node
    // with no state at all IS `Draft` (the fold's own reading of absence): a start whose
    // process died after committing the shape and before the first outcome has its entire
    // graph in that condition, and "nothing" would be the wrong word for it.
    let mut dispatchable = nodes_in(projection, |state| {
        matches!(
            state,
            NodeState::Ready | NodeState::Queued | NodeState::Draft
        )
    });
    dispatchable.extend(untouched_declared(projection));
    dispatchable.sort();
    if dispatchable.is_empty() {
        NextStep::Nothing
    } else {
        NextStep::Dispatch {
            nodes: dispatchable,
        }
    }
}

/// Declared nodes the driver never recorded a state for - `Draft` by absence.
fn untouched_declared(projection: &ExecutionProjection) -> Vec<String> {
    projection
        .declared_form
        .iter()
        .flat_map(|form| form.node_ids.iter())
        .map(|node| node.as_str().to_owned())
        .filter(|node| !projection.node_states.contains_key(node))
        .collect()
}

fn nodes_in(projection: &ExecutionProjection, wanted: impl Fn(NodeState) -> bool) -> Vec<String> {
    projection
        .node_states
        .iter()
        .filter(|(_, state)| wanted(**state))
        .map(|(node, _)| node.clone())
        .collect()
}

const fn mode_label(mode: ExecutionMode) -> &'static str {
    match mode {
        ExecutionMode::Autopilot => "autopilot",
        ExecutionMode::Supervised => "supervised",
        ExecutionMode::Manual => "manual",
    }
}

const fn attestation_label(mode: ClaimAttestationMode) -> &'static str {
    match mode {
        ClaimAttestationMode::OperatorAttested => "operator_attested",
        ClaimAttestationMode::MachineVerified => "machine_verified",
    }
}

fn verifier_label(verifier: &ClearanceVerifier) -> String {
    match verifier {
        ClearanceVerifier::MachineReplay { .. } => "machine_replay".to_owned(),
        ClearanceVerifier::Countersign { identity, .. } => {
            format!("countersign {}", identity.as_str())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attention::{AttentionInputs, attention};
    use chrono::{TimeZone, Utc};
    use graphhelm_protocols::{
        ActorId, ClaimAttestation, CompletionClaimed, CompletionCleared, CompletionRefused,
        EventHash, ExecutionCompleted, ExecutionFormDeclared, ExecutionId, ExecutionPaused,
        ExecutionStarted, NewEvent, NodeOutcomeRecorded, OpaqueId, PersistedActorType,
        PersistedTimestamp, ProjectId, RepositoryScope, Sensitivity, WorkspaceId,
    };

    const GENESIS: &str = "sha256:35c8ab0717bef1684ad07efcf3bedd4648c778a2c944cbd2c7e6a4802e2237b3";
    const EXECUTION: &str = "exec-briefing";

    fn actor(actor_type: PersistedActorType, id: &str) -> PersistedActor {
        PersistedActor::new(actor_type, ActorId::parse(id.to_owned()).unwrap())
    }

    /// An envelope at a fixed instant: the briefing never reads `occurred_at`, and the tests
    /// prove it by giving every event the same one.
    fn envelope(sequence: u64, actor: PersistedActor, kind: EventKind) -> EventEnvelope {
        EventEnvelope::new(
            OpaqueId::parse(format!("event-{sequence}")).unwrap(),
            RepositoryScope::new(
                WorkspaceId::parse("workspace-1").unwrap(),
                ProjectId::parse("project-1").unwrap(),
                Some(ExecutionId::parse(EXECUTION).unwrap()),
            ),
            OpaqueId::parse(EXECUTION).unwrap(),
            sequence,
            PersistedTimestamp::from_datetime(Utc.with_ymd_and_hms(2026, 9, 13, 9, 0, 0).unwrap())
                .unwrap(),
            NewEvent::new(
                OpaqueId::parse(format!("request-{sequence}")).unwrap(),
                actor,
                Sensitivity::Internal,
                kind,
                vec![],
                vec![],
            ),
            EventHash::parse(GENESIS).unwrap(),
            EventHash::parse(GENESIS).unwrap(),
        )
    }

    fn hash(fill: &str) -> WireHash {
        WireHash::parse(format!("sha256:{}", fill.repeat(64))).unwrap()
    }

    fn started(sequence: u64) -> EventEnvelope {
        envelope(
            sequence,
            actor(PersistedActorType::System, "system-cli"),
            EventKind::ExecutionStarted(ExecutionStarted {
                execution_id: OpaqueId::parse(EXECUTION).unwrap(),
                graph_version: 13,
                graph_hash: hash("b"),
                mode: ExecutionMode::Supervised,
            }),
        )
    }

    fn outcome(
        sequence: u64,
        actor: PersistedActor,
        node: &str,
        outcome: NodeOutcome,
        next_state: NodeState,
        reason: Option<NodeOutcomeReason>,
    ) -> EventEnvelope {
        envelope(
            sequence,
            actor,
            EventKind::NodeOutcomeRecorded(NodeOutcomeRecorded {
                executor: None,
                execution_id: OpaqueId::parse(EXECUTION).unwrap(),
                node_id: OpaqueId::parse(node).unwrap(),
                outcome,
                next_state,
                reason,
            }),
        )
    }

    /// The story the CLI and MCP cells drive: start, `implementation` fails and blocks,
    /// `claude-code` approves it, then pauses the run.
    fn approved_then_paused() -> (ExecutionProjection, Vec<EventEnvelope>) {
        let owner = actor(PersistedActorType::Agent, "claude-code");
        let history = vec![
            started(1),
            outcome(
                2,
                actor(PersistedActorType::System, "system-cli"),
                "implementation",
                NodeOutcome::TerminalFailure,
                NodeState::Blocked,
                Some(NodeOutcomeReason::FixtureScripted),
            ),
            outcome(
                3,
                owner.clone(),
                "implementation",
                NodeOutcome::Approved,
                NodeState::Ready,
                None,
            ),
            envelope(
                4,
                owner,
                EventKind::ExecutionPaused(ExecutionPaused {
                    execution_id: OpaqueId::parse(EXECUTION).unwrap(),
                }),
            ),
        ];
        let mut projection = ExecutionProjection {
            execution_id: Some(EXECUTION.to_owned()),
            mode: Some(ExecutionMode::Supervised),
            simulation_status: Some(SimulationStatus::Paused),
            ..ExecutionProjection::default()
        };
        projection.declared_form = Some(ExecutionFormDeclared {
            node_descriptors: Default::default(),
            execution_id: OpaqueId::parse(EXECUTION).unwrap(),
            node_ids: vec![
                OpaqueId::parse("implementation").unwrap(),
                OpaqueId::parse("deploy").unwrap(),
            ],
            node_timeout_seconds: BTreeMap::new(),
            name: Some("Deploy com override manual".to_owned()),
            objective: Some("Produzir build implantável.".to_owned()),
            executor: Some(DeclaredExecutor::Fixture),
            // These fixtures are about the declared form's OTHER fields; a graph that declares no
            // customs produces an empty map, which is what keeps each cell asking its own question.
            node_customs_budgets: std::collections::BTreeMap::new(),
        });
        projection
            .node_states
            .insert("implementation".to_owned(), NodeState::Paused);
        projection
            .node_states
            .insert("deploy".to_owned(), NodeState::Paused);
        projection
            .node_attempts
            .insert("implementation".to_owned(), 1);
        projection
            .last_outcome
            .insert("implementation".to_owned(), NodeOutcome::Approved);
        (projection, history)
    }

    #[test]
    fn an_empty_history_briefs_nothing_and_says_so() {
        let projection = ExecutionProjection::default();
        let answer = attention(&projection, &AttentionInputs::default());
        let briefing = briefing_view(&projection, &answer, &[]);
        assert_eq!(briefing.name, None);
        assert_eq!(briefing.objective, None);
        assert_eq!(briefing.executor, None);
        assert_eq!(briefing.graph_hash, None);
        assert!(briefing.decisions.is_empty());
        assert!(briefing.work_done.is_empty());
        assert_eq!(briefing.next_step, NextStep::Nothing);
        assert_eq!(briefing.as_of_sequence, 0);
    }

    #[test]
    fn the_decisions_are_folded_in_sequence_order_with_the_envelopes_actor() {
        let (projection, history) = approved_then_paused();
        let answer = attention(&projection, &AttentionInputs::default());
        let briefing = briefing_view(&projection, &answer, &history);

        assert_eq!(briefing.name.as_deref(), Some("Deploy com override manual"));
        assert_eq!(
            briefing.objective.as_deref(),
            Some("Produzir build implantável.")
        );
        assert_eq!(briefing.executor, Some(DeclaredExecutor::Fixture));
        assert_eq!(briefing.graph_hash, Some(hash("b")));
        assert_eq!(briefing.graph_version, Some(13));
        assert_eq!(briefing.as_of_sequence, 4);

        let kinds: Vec<(u64, DecisionKind, Option<&str>)> = briefing
            .decisions
            .iter()
            .map(|decision| (decision.sequence, decision.kind, decision.node.as_deref()))
            .collect();
        assert_eq!(
            kinds,
            vec![
                (3, DecisionKind::Approval, Some("implementation")),
                (4, DecisionKind::Paused, None),
            ],
            "the failure at #2 is an outcome, not a decision; the start is not one either"
        );
        for decision in &briefing.decisions {
            assert_eq!(decision.actor.id().as_str(), "claude-code");
            assert_eq!(decision.actor.actor_type(), PersistedActorType::Agent);
        }
        assert_eq!(
            briefing.decisions[0].detail,
            "implementation approved -> ready"
        );
    }

    /// The driver records `Approved` for its own `Draft -> Ready` hop under the system actor.
    /// That is a hop, not a decision: only an approval that readied a BLOCKED or GHOST node is
    /// somebody's call, and the digest folds the node's prior state to tell them apart.
    #[test]
    fn the_drivers_own_pre_dispatch_approval_is_not_a_decision() {
        let system = actor(PersistedActorType::System, "system-cli");
        let history = vec![
            started(1),
            outcome(
                2,
                system.clone(),
                "deploy",
                NodeOutcome::Approved,
                NodeState::Ready,
                None,
            ),
            outcome(
                3,
                system.clone(),
                "implementation",
                NodeOutcome::TerminalFailure,
                NodeState::Blocked,
                Some(NodeOutcomeReason::FixtureScripted),
            ),
            outcome(
                4,
                system,
                "implementation",
                NodeOutcome::Approved,
                NodeState::Ready,
                None,
            ),
        ];
        let projection = ExecutionProjection {
            execution_id: Some(EXECUTION.to_owned()),
            ..ExecutionProjection::default()
        };
        let answer = attention(&projection, &AttentionInputs::default());
        let briefing = briefing_view(&projection, &answer, &history);
        let kinds: Vec<(u64, Option<&str>)> = briefing
            .decisions
            .iter()
            .map(|decision| (decision.sequence, decision.node.as_deref()))
            .collect();
        assert_eq!(
            kinds,
            vec![(4, Some("implementation"))],
            "#2 readied a node nobody had blocked; #4 readied a blocked one"
        );
    }

    #[test]
    fn a_paused_run_names_resume_and_the_nodes_it_holds() {
        let (projection, history) = approved_then_paused();
        let answer = attention(&projection, &AttentionInputs::default());
        let briefing = briefing_view(&projection, &answer, &history);
        assert_eq!(
            briefing.next_step,
            NextStep::ResumeHeld {
                command: format!(
                    "graphhelm execution resume --file <graph> --events <store> --execution {EXECUTION}"
                ),
                nodes: vec!["deploy".to_owned(), "implementation".to_owned()],
            }
        );
        assert!(
            briefing.work_done.is_empty(),
            "a paused node is not work done"
        );
    }

    #[test]
    fn pending_is_a_copy_of_the_answer_and_a_blocked_node_answers_with_approve() {
        let history = vec![
            started(1),
            outcome(
                2,
                actor(PersistedActorType::System, "system-cli"),
                "implementation",
                NodeOutcome::TerminalFailure,
                NodeState::Blocked,
                Some(NodeOutcomeReason::FixtureScripted),
            ),
        ];
        let mut projection = ExecutionProjection {
            execution_id: Some(EXECUTION.to_owned()),
            ..ExecutionProjection::default()
        };
        projection
            .node_states
            .insert("implementation".to_owned(), NodeState::Blocked);
        projection
            .node_states
            .insert("deploy".to_owned(), NodeState::Ready);
        let answer = attention(&projection, &AttentionInputs::default());
        assert!(!answer.reasons().is_empty(), "the story needs a reason");

        let briefing = briefing_view(&projection, &answer, &history);
        assert_eq!(briefing.pending, answer.reasons().to_vec());
        assert_eq!(
            briefing.next_step,
            NextStep::Answer {
                node: "implementation".to_owned(),
                remedy: AnswerRemedy::Approve,
                claim_seq: None,
            },
            "the operator's verb comes before the dispatchable `deploy`"
        );
    }

    /// The reasons no verb answers - a node failed for good, a wedged run, a foreign wake burn -
    /// still get an action. `pending` and `next_step` come from the same answer, so a briefing
    /// listing a need beside `nothing` is unrepresentable rather than merely untested.
    #[test]
    fn a_reason_without_a_verb_is_a_diagnose_step_never_nothing() {
        let history = vec![started(1)];
        let mut projection = ExecutionProjection {
            execution_id: Some(EXECUTION.to_owned()),
            ..ExecutionProjection::default()
        };
        projection
            .node_states
            .insert("implementation".to_owned(), NodeState::Failed);
        projection
            .node_states
            .insert("deploy".to_owned(), NodeState::Ready);
        let answer = attention(&projection, &AttentionInputs::default());
        assert_eq!(
            answer.reasons(),
            &[AttentionReason::FailedNode {
                node: "implementation".to_owned()
            }],
            "the story needs exactly the verb-less reason"
        );
        let briefing = briefing_view(&projection, &answer, &history);
        assert_eq!(
            briefing.next_step,
            NextStep::Diagnose {
                reason: AttentionReason::FailedNode {
                    node: "implementation".to_owned()
                }
            },
            "a failed node outranks the dispatchable `deploy`, as it does in `pending`"
        );
        assert_eq!(
            serde_json::to_value(&briefing.next_step).unwrap(),
            serde_json::json!({
                "kind": "diagnose",
                "reason": {"kind": "failed_node", "node": "implementation"}
            })
        );

        // The wedge: status says running, nothing moves, no node is named.
        let mut wedged = ExecutionProjection {
            execution_id: Some(EXECUTION.to_owned()),
            simulation_status: Some(SimulationStatus::Running),
            ..ExecutionProjection::default()
        };
        wedged
            .node_states
            .insert("implementation".to_owned(), NodeState::Succeeded);
        wedged
            .node_states
            .insert("deploy".to_owned(), NodeState::Draft);
        wedged
            .node_states
            .insert("ship".to_owned(), NodeState::Ghost);
        let answer = attention(&wedged, &AttentionInputs::default());
        let briefing = briefing_view(&wedged, &answer, &history);
        assert!(
            briefing.pending.is_empty() || !matches!(briefing.next_step, NextStep::Nothing),
            "whatever the answer names, it is never a need beside `nothing`: {briefing:?}"
        );
    }

    /// A SILENT node - running past its own declared bound - is a `diagnose`, never an
    /// `amend_budget`: raising the bound is the purchased calm `CalmedByAmendment` exposes.
    /// `amend_budget` stays the answer only for a node with NO bound (`NoDeclaredBudget`).
    #[test]
    fn a_silent_node_is_diagnosed_and_only_an_undeclared_budget_is_amended() {
        let history = vec![started(1)];
        let mut projection = ExecutionProjection {
            execution_id: Some(EXECUTION.to_owned()),
            simulation_status: Some(SimulationStatus::Running),
            ..ExecutionProjection::default()
        };
        projection
            .node_states
            .insert("judge".to_owned(), NodeState::Running);
        projection.node_attempts.insert("judge".to_owned(), 1);

        let over_budget = AttentionInputs {
            node_silence_seconds: BTreeMap::from([("judge".to_owned(), 900)]),
            silence_budget_seconds: BTreeMap::from([("judge".to_owned(), 300)]),
            at_sequence: Some(1),
        };
        let answer = attention(&projection, &over_budget);
        assert_eq!(
            answer.reasons(),
            &[AttentionReason::SilentNode {
                node: "judge".to_owned()
            }]
        );
        let briefing = briefing_view(&projection, &answer, &history);
        assert_eq!(
            briefing.next_step,
            NextStep::Diagnose {
                reason: AttentionReason::SilentNode {
                    node: "judge".to_owned()
                }
            },
            "900s of silence under a 300s bound is not answered by a bigger bound"
        );

        let undeclared = AttentionInputs {
            node_silence_seconds: BTreeMap::from([("judge".to_owned(), 900)]),
            silence_budget_seconds: BTreeMap::new(),
            at_sequence: Some(1),
        };
        let answer = attention(&projection, &undeclared);
        assert!(answer.reasons().is_empty(), "{answer:?}");
        let briefing = briefing_view(&projection, &answer, &history);
        assert_eq!(
            briefing.next_step,
            NextStep::Answer {
                node: "judge".to_owned(),
                remedy: AnswerRemedy::AmendBudget,
                claim_seq: None,
            },
            "no bound declared: declaring one IS the answer"
        );
    }

    /// Under a pause, an answerable reason comes BEFORE the resume: `approve` is legal while
    /// paused, and resuming first would only re-park the blocked node. With nothing answerable
    /// the held run still says `resume_held` (`a_paused_run_names_resume_and_the_nodes_it_holds`).
    #[test]
    fn a_paused_run_with_a_blocked_node_answers_before_it_resumes() {
        let (mut projection, history) = approved_then_paused();
        projection
            .node_states
            .insert("deploy".to_owned(), NodeState::Blocked);
        let answer = attention(&projection, &AttentionInputs::default());
        assert_eq!(
            answer.reasons(),
            &[AttentionReason::BlockedNode {
                node: "deploy".to_owned()
            }]
        );
        let briefing = briefing_view(&projection, &answer, &history);
        assert_eq!(
            briefing.next_step,
            NextStep::Answer {
                node: "deploy".to_owned(),
                remedy: AnswerRemedy::Approve,
                claim_seq: None,
            }
        );
    }

    fn claimed(sequence: u64, actor: PersistedActor, node: &str, wait: u64) -> EventEnvelope {
        envelope(
            sequence,
            actor,
            EventKind::CompletionClaimed(CompletionClaimed {
                execution_id: OpaqueId::parse(EXECUTION).unwrap(),
                node: OpaqueId::parse(node).unwrap(),
                completes_wait_seq: wait,
                evidence: vec![],
                attestation: ClaimAttestation {
                    asserter: OpaqueId::parse("agent-claimer").unwrap(),
                    mode: ClaimAttestationMode::OperatorAttested,
                },
            }),
        )
    }

    /// `completion_cleared` says a clearance was OFFERED; the fold says whether it took. A
    /// machine replay with the wrong manifest hash replays as `Refused` and leaves the node
    /// parked, so the digest calls it a rejection with the fold's code - never "cleared".
    #[test]
    fn a_cleared_event_the_fold_refused_is_a_rejection_not_a_clearance() {
        let auditor = actor(PersistedActorType::Agent, "agent-auditor");
        let history = vec![
            started(1),
            claimed(
                3,
                actor(PersistedActorType::Agent, "agent-claimer"),
                "implementation",
                2,
            ),
            envelope(
                4,
                auditor,
                EventKind::CompletionCleared(CompletionCleared {
                    execution_id: OpaqueId::parse(EXECUTION).unwrap(),
                    claim_seq: 3,
                    verifier: ClearanceVerifier::MachineReplay {
                        manifest_hash: hash("d"),
                    },
                }),
            ),
        ];
        let mut projection = ExecutionProjection {
            execution_id: Some(EXECUTION.to_owned()),
            ..ExecutionProjection::default()
        };
        projection.clearances.insert(
            3,
            ClearanceOutcome::Refused {
                reason_code: graphhelm_protocols::SafeCode::parse("hash_mismatch").unwrap(),
            },
        );
        let answer = attention(&projection, &AttentionInputs::default());
        let briefing = briefing_view(&projection, &answer, &history);
        let kinds: Vec<(u64, DecisionKind)> = briefing
            .decisions
            .iter()
            .map(|decision| (decision.sequence, decision.kind))
            .collect();
        assert_eq!(
            kinds,
            vec![(3, DecisionKind::Claim), (4, DecisionKind::Rejection)]
        );
        assert_eq!(
            briefing.decisions[1].node.as_deref(),
            Some("implementation")
        );
        assert_eq!(
            briefing.decisions[1].detail,
            "claim #3 clearance by machine_replay refused: hash_mismatch"
        );

        // The control: the fold recorded `Cleared`, and the digest says so.
        projection.clearances.insert(3, ClearanceOutcome::Cleared);
        let briefing = briefing_view(&projection, &answer, &history);
        assert_eq!(briefing.decisions[1].kind, DecisionKind::Clearance);
        assert_eq!(
            briefing.decisions[1].detail,
            "claim #3 cleared by machine_replay"
        );
    }

    /// A claim the door refused (`completion_refused`) is a decision the next harness must
    /// see - with its actor and the registry code - or it repeats the same invalid claim.
    #[test]
    fn a_refused_claim_is_in_the_digest_with_its_code() {
        let claimer = actor(PersistedActorType::Agent, "agent-claimer");
        let history = vec![
            started(1),
            envelope(
                3,
                claimer,
                EventKind::CompletionRefused(CompletionRefused {
                    execution_id: OpaqueId::parse(EXECUTION).unwrap(),
                    node: OpaqueId::parse("implementation").unwrap(),
                    claimed_wait_seq: 2,
                    reason_code: graphhelm_protocols::SafeCode::parse("wait_superseded").unwrap(),
                }),
            ),
        ];
        let projection = ExecutionProjection {
            execution_id: Some(EXECUTION.to_owned()),
            ..ExecutionProjection::default()
        };
        let answer = attention(&projection, &AttentionInputs::default());
        let briefing = briefing_view(&projection, &answer, &history);
        assert_eq!(briefing.decisions.len(), 1);
        let refusal = &briefing.decisions[0];
        assert_eq!(refusal.kind, DecisionKind::Refusal);
        assert_eq!(refusal.node.as_deref(), Some("implementation"));
        assert_eq!(refusal.actor.id().as_str(), "agent-claimer");
        assert_eq!(refusal.detail, "claim for wait #2 refused: wait_superseded");
    }

    /// A parked node whose claim is already OPEN needs the countersignature, not a second
    /// claim (the door refuses that as `duplicate_completion`): the step is `clear`, naming
    /// the claim by its sequence. Without an open claim the step is `claim`.
    #[test]
    fn an_open_claim_is_answered_by_clear_naming_its_sequence() {
        let history = vec![started(1)];
        let mut projection = ExecutionProjection {
            execution_id: Some(EXECUTION.to_owned()),
            ..ExecutionProjection::default()
        };
        projection
            .node_states
            .insert("implementation".to_owned(), NodeState::WaitingInput);
        let answer = attention(&projection, &AttentionInputs::default());
        assert_eq!(
            answer.reasons(),
            &[AttentionReason::WaitingInputNode {
                node: "implementation".to_owned()
            }]
        );
        let briefing = briefing_view(&projection, &answer, &history);
        assert_eq!(
            briefing.next_step,
            NextStep::Answer {
                node: "implementation".to_owned(),
                remedy: AnswerRemedy::Claim,
                claim_seq: None,
            }
        );

        projection.open_claims.insert(
            7,
            graphhelm_events::OpenClaim {
                node: "implementation".to_owned(),
                completes_wait_seq: 4,
                stage_entered_at: 7,
                deadline: None,
                evidence_digest: hash("c"),
            },
        );
        let briefing = briefing_view(&projection, &answer, &history);
        assert_eq!(
            briefing.next_step,
            NextStep::Answer {
                node: "implementation".to_owned(),
                remedy: AnswerRemedy::Clear,
                claim_seq: Some(7),
            }
        );
        assert_eq!(
            serde_json::to_value(&briefing.next_step).unwrap(),
            serde_json::json!({
                "kind": "answer", "node": "implementation", "remedy": "clear", "claimSeq": 7
            })
        );
    }

    /// A start whose process died after committing the shape and before the first outcome:
    /// every declared node is absent from `node_states` (`Draft` by absence), and the whole
    /// graph is the work to dispatch - not "nothing".
    #[test]
    fn declared_nodes_the_driver_never_touched_are_the_work_to_dispatch() {
        let history = vec![started(1)];
        let mut projection = ExecutionProjection {
            execution_id: Some(EXECUTION.to_owned()),
            ..ExecutionProjection::default()
        };
        projection.declared_form = Some(ExecutionFormDeclared {
            node_descriptors: Default::default(),
            execution_id: OpaqueId::parse(EXECUTION).unwrap(),
            node_ids: vec![
                OpaqueId::parse("implementation").unwrap(),
                OpaqueId::parse("deploy").unwrap(),
            ],
            node_timeout_seconds: BTreeMap::new(),
            name: None,
            objective: None,
            executor: None,
            // These fixtures are about the declared form's OTHER fields; a graph that declares no
            // customs produces an empty map, which is what keeps each cell asking its own question.
            node_customs_budgets: std::collections::BTreeMap::new(),
        });
        let answer = attention(&projection, &AttentionInputs::default());
        let briefing = briefing_view(&projection, &answer, &history);
        assert_eq!(
            briefing.next_step,
            NextStep::Dispatch {
                nodes: vec!["deploy".to_owned(), "implementation".to_owned()],
            }
        );
    }

    /// `start --held` commits the shape and the hold without a single outcome, so the held
    /// nodes are the declared ones - the resume releases exactly them.
    #[test]
    fn a_held_start_names_every_declared_node_as_held() {
        let owner = actor(PersistedActorType::Owner, "owner-cli");
        let history = vec![
            started(1),
            envelope(
                2,
                owner,
                EventKind::ExecutionPaused(ExecutionPaused {
                    execution_id: OpaqueId::parse(EXECUTION).unwrap(),
                }),
            ),
        ];
        let mut projection = ExecutionProjection {
            execution_id: Some(EXECUTION.to_owned()),
            simulation_status: Some(SimulationStatus::Paused),
            ..ExecutionProjection::default()
        };
        projection.declared_form = Some(ExecutionFormDeclared {
            node_descriptors: Default::default(),
            execution_id: OpaqueId::parse(EXECUTION).unwrap(),
            node_ids: vec![
                OpaqueId::parse("implementation").unwrap(),
                OpaqueId::parse("deploy").unwrap(),
            ],
            node_timeout_seconds: BTreeMap::new(),
            name: None,
            objective: None,
            executor: None,
            // These fixtures are about the declared form's OTHER fields; a graph that declares no
            // customs produces an empty map, which is what keeps each cell asking its own question.
            node_customs_budgets: std::collections::BTreeMap::new(),
        });
        let answer = attention(&projection, &AttentionInputs::default());
        let briefing = briefing_view(&projection, &answer, &history);
        let NextStep::ResumeHeld { nodes, .. } = &briefing.next_step else {
            panic!("a held start resumes: {:?}", briefing.next_step);
        };
        assert_eq!(nodes, &["deploy".to_owned(), "implementation".to_owned()]);
    }

    /// A foreign wake burn persists across a pause. Resuming first would resume without asking
    /// who else writes to this store, so the hazard is diagnosed BEFORE the resume is offered.
    #[test]
    fn a_hazard_under_a_pause_is_diagnosed_before_the_resume() {
        let (mut projection, history) = approved_then_paused();
        projection.wake_mis_burns.insert(
            "session-other".to_owned(),
            graphhelm_events::WakeMisBurn {
                at_sequence: 4,
                captured_arming: 2,
                live_arming: 3,
            },
        );
        let answer = attention(&projection, &AttentionInputs::default());
        assert_eq!(
            answer.reasons(),
            &[AttentionReason::ForeignWakeConsumption {
                session: "session-other".to_owned()
            }]
        );
        let briefing = briefing_view(&projection, &answer, &history);
        assert_eq!(
            briefing.next_step,
            NextStep::Diagnose {
                reason: AttentionReason::ForeignWakeConsumption {
                    session: "session-other".to_owned()
                }
            }
        );
    }

    #[test]
    fn work_done_carries_the_terminal_nodes_with_their_last_recorded_reason() {
        let history = vec![
            started(1),
            outcome(
                2,
                actor(PersistedActorType::System, "system-cli"),
                "implementation",
                NodeOutcome::Succeeded,
                NodeState::Succeeded,
                None,
            ),
            outcome(
                3,
                actor(PersistedActorType::System, "system-cli"),
                "deploy",
                NodeOutcome::TerminalFailure,
                NodeState::Failed,
                Some(NodeOutcomeReason::ToolExitedNonZero),
            ),
            envelope(
                4,
                actor(PersistedActorType::System, "system-cli"),
                EventKind::ExecutionCompleted(ExecutionCompleted {
                    execution_id: OpaqueId::parse(EXECUTION).unwrap(),
                    status: SimulationStatus::Failed,
                }),
            ),
        ];
        let mut projection = ExecutionProjection {
            execution_id: Some(EXECUTION.to_owned()),
            simulation_status: Some(SimulationStatus::Failed),
            ..ExecutionProjection::default()
        };
        projection
            .node_states
            .insert("implementation".to_owned(), NodeState::Succeeded);
        projection
            .node_states
            .insert("deploy".to_owned(), NodeState::Failed);
        projection
            .node_attempts
            .insert("implementation".to_owned(), 1);
        projection.node_attempts.insert("deploy".to_owned(), 2);
        projection
            .last_outcome
            .insert("implementation".to_owned(), NodeOutcome::Succeeded);
        projection
            .last_outcome
            .insert("deploy".to_owned(), NodeOutcome::TerminalFailure);
        let answer = attention(&projection, &AttentionInputs::default());

        let briefing = briefing_view(&projection, &answer, &history);
        assert_eq!(
            briefing.work_done,
            vec![
                WorkItem {
                    node: "deploy".to_owned(),
                    state: NodeState::Failed,
                    attempts: 2,
                    last_outcome: Some(NodeOutcome::TerminalFailure),
                    reason: Some(NodeOutcomeReason::ToolExitedNonZero),
                },
                WorkItem {
                    node: "implementation".to_owned(),
                    state: NodeState::Succeeded,
                    attempts: 1,
                    last_outcome: Some(NodeOutcome::Succeeded),
                    reason: None,
                },
            ]
        );
        assert_eq!(
            briefing.next_step,
            NextStep::Finished {
                status: SimulationStatus::Failed
            }
        );
        assert!(
            briefing.decisions.is_empty(),
            "a failure recorded by the driver is not a decision"
        );
    }

    #[test]
    fn a_cancellation_is_a_decision_and_the_wire_shape_is_camel_case_and_tagged() {
        let owner = actor(PersistedActorType::Owner, "owner-cli");
        let history = vec![
            started(1),
            envelope(
                2,
                owner,
                EventKind::ExecutionCompleted(ExecutionCompleted {
                    execution_id: OpaqueId::parse(EXECUTION).unwrap(),
                    status: SimulationStatus::Cancelled,
                }),
            ),
        ];
        let projection = ExecutionProjection {
            execution_id: Some(EXECUTION.to_owned()),
            simulation_status: Some(SimulationStatus::Cancelled),
            ..ExecutionProjection::default()
        };
        let answer = attention(&projection, &AttentionInputs::default());
        let briefing = briefing_view(&projection, &answer, &history);
        assert_eq!(briefing.decisions.len(), 1);
        assert_eq!(briefing.decisions[0].kind, DecisionKind::Cancelled);

        let value = serde_json::to_value(&briefing).unwrap();
        let top: Vec<&String> = value.as_object().unwrap().keys().collect();
        assert_eq!(
            top,
            [
                "asOfSequence",
                "decisions",
                "executor",
                "graphHash",
                "graphVersion",
                "name",
                "nextStep",
                "objective",
                "pending",
                "unevaluated",
                "workDone",
            ]
        );
        assert_eq!(
            value["decisions"][0],
            serde_json::json!({
                "sequence": 2,
                "actor": {"type": "owner", "id": "owner-cli"},
                "kind": "cancelled",
                "detail": "execution cancelled",
            }),
            "no `node: null`: an absent node is omitted"
        );
        assert_eq!(
            value["nextStep"],
            serde_json::json!({"kind": "finished", "status": "cancelled"})
        );
        let back: Briefing = serde_json::from_value(value).unwrap();
        assert_eq!(back, briefing);
    }
}
