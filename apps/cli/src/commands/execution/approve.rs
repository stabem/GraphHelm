use std::{collections::BTreeMap, path::Path};

use graphhelm_events::{EvidenceOpener, EvidenceRead};
use graphhelm_governor::{
    ApplyServices, ExecutionDraftAcceptance, SealingGraphExternalizer, apply_draft_with_acceptance,
    recover_verified_authoring_snapshot,
};
use graphhelm_graph::raw_content_sha256;
use graphhelm_protocols::{
    Actor, ActorType, DraftOperation, EventKind, NodeOutcome, NodeState, OpaqueId, PersistedActor,
};

use super::{
    Failure, RecordedOutcome, execution_state, finish, idempotency_key, load_projection,
    node_state_label, owner_actor, record_outcome_with_key, render, replay_projection,
    repository_failure,
};
use crate::commands::{SystemClock, UuidIds, event_store};
use crate::output::Outcome;

const COMMAND: &str = "execution.approve";

/// The owner approves a `Ghost` or `Blocked` node, readying it — for a `Blocked` node with
/// `last_outcome == Interrupted`, this *is* the triage act `resume_preconditions` waits for.
///
/// Does not auto-drive afterwards: D-020's rule that nothing auto-starts out of a manual
/// intervention. The owner runs `resume` next (an already-`start`ed execution quiesces again on
/// its next `resume` too).
///
/// Calls `execute` with the owner actor and a fresh per-invocation idempotency key, exactly as
/// before Milestone 05a Task 3 — byte-identical CLI behaviour.
pub fn run(
    events: &Path,
    execution: Option<&str>,
    node: &str,
    keyring: Option<&Path>,
    key_id: Option<&str>,
    assigned_actor: Option<&str>,
    proposal_digest: Option<&str>,
) -> Outcome {
    finish(
        COMMAND,
        match (keyring, key_id) {
            (Some(keyring), Some(key_id)) => execute_governed(
                events,
                execution,
                node,
                keyring,
                key_id,
                assigned_actor,
                proposal_digest,
            ),
            (None, None) => execute(
                events,
                execution,
                node,
                owner_actor(),
                idempotency_key("node-outcome"),
            ),
            _ => Err(execution_state(
                "--keyring and --key-id must be supplied together",
                "/keyring",
            )),
        },
        |value| value,
    )
}

/// Approves a sealed graph proposal and immediately records the owner decision. Assignment is
/// optional and is appended only after the approval has replayed successfully. Every input used to
/// apply the draft comes from authenticated sealed evidence or the verified active snapshot.
pub(crate) fn execute_governed(
    events: &Path,
    execution: Option<&str>,
    node: &str,
    keyring: &Path,
    key_id: &str,
    assigned_actor: Option<&str>,
    proposal_digest: Option<&str>,
) -> Result<serde_json::Value, Failure> {
    let keyring = super::signal::SignalKeyring {
        directory: keyring.to_owned(),
        key_id: key_id.to_owned(),
    };
    let opener = super::signal::open_sealer(&keyring)?;
    let store = event_store(events).map_err(|error| repository_failure(&error))?;
    let (scope, stream, projection) = load_projection(&store, execution)?;
    if !matches!(
        projection.mode,
        Some(
            graphhelm_protocols::ExecutionMode::Supervised
                | graphhelm_protocols::ExecutionMode::Manual
        )
    ) {
        return Err(execution_state(
            "sealed graph proposals require a supervised or manual execution",
            "/mode",
        ));
    }
    let active = projection.current_graph.as_ref().ok_or_else(|| {
        execution_state("the execution has no published graph snapshot", "/graph")
    })?;
    let snapshot_ref = projection
        .authoring_snapshots
        .get(&active.number())
        .ok_or_else(|| execution_state("the authoring snapshot is unavailable", "/graph"))?;
    let snapshot = match store
        .sealed_evidence(&scope, snapshot_ref.evidence_id())
        .map_err(|error| repository_failure(&error))?
    {
        EvidenceRead::Available(value) => value,
        EvidenceRead::Unavailable(_) => {
            return Err(execution_state(
                "the authoring snapshot is unavailable",
                "/graph",
            ));
        }
    };
    for slot in active.content_slots() {
        match store
            .sealed_evidence(&scope, slot.evidence_id())
            .map_err(|error| repository_failure(&error))?
        {
            EvidenceRead::Available(value) => {
                let plaintext =
                    block_on_local(opener.open(scope.clone(), &value)).map_err(|_| {
                        execution_state(
                            "the active graph content failed authentication; approval is refused",
                            "/graph/content",
                        )
                    })?;
                let digest = plaintext.expose(raw_content_sha256).map_err(|_| {
                    execution_state("the active graph content is invalid", "/graph/content")
                })?;
                if digest != *slot.content_sha256() {
                    return Err(execution_state(
                        "the active graph content does not match its digest; approval is refused",
                        "/graph/content",
                    ));
                }
            }
            EvidenceRead::Unavailable(_) => {
                return Err(execution_state(
                    "the active graph has unavailable content; approval is refused",
                    "/graph/content",
                ));
            }
        }
    }
    let authoring = block_on_local(recover_verified_authoring_snapshot(
        &opener,
        scope.clone(),
        active,
        &snapshot,
    ))
    .map_err(|_| execution_state("the authoring snapshot failed integrity checks", "/graph"))?;

    let history = store
        .read_replay_stream(&scope, &stream)
        .map_err(|error| repository_failure(&error))?;
    let requested_digest = proposal_digest
        .map(|value| {
            graphhelm_protocols::RawSha256::parse(value)
                .map_err(|_| execution_state("the proposal digest is invalid", "/proposalDigest"))
        })
        .transpose()?
        .ok_or_else(|| {
            execution_state(
                "proposal-digest is required for governed approval",
                "/proposalDigest",
            )
        })?;
    let mut candidates = Vec::new();
    for event in history.iter().rev() {
        let EventKind::DraftProposed(payload) = &event.kind else {
            continue;
        };
        let Some(digest) = payload.proposal_sha256.as_ref() else {
            continue;
        };
        let Some(reference) = event.evidence_refs.first() else {
            continue;
        };
        let sealed = match store
            .sealed_evidence(&scope, reference.evidence_id())
            .map_err(|error| repository_failure(&error))?
        {
            EvidenceRead::Available(value) => value,
            EvidenceRead::Unavailable(_) => continue,
        };
        let plaintext = block_on_local(opener.open(scope.clone(), &sealed))
            .map_err(|_| execution_state("the sealed proposal is unavailable", "/proposal"))?;
        let envelope: serde_json::Value =
            plaintext
                .expose(|bytes| serde_json::from_slice(bytes))
                .map_err(|_| execution_state("the sealed proposal is invalid", "/proposal"))?;
        let typed = graphhelm_execution::TypedSignal::parse(&envelope)
            .map_err(|_| execution_state("the sealed proposal is invalid", "/proposal"))?;
        let Some(draft) = typed.proposal().cloned() else {
            continue;
        };
        let calculated = raw_content_sha256(
            &serde_json::to_vec(&draft)
                .map_err(|_| execution_state("the sealed proposal is invalid", "/proposal"))?,
        )
        .map_err(|_| execution_state("the proposal digest is invalid", "/proposal"))?;
        if &calculated == digest
            && draft.id == payload.draft_id.as_str()
            && requested_digest == *digest
            && draft.operations.iter().any(
                |operation| matches!(operation, DraftOperation::AddNode { id, .. } if id == node),
            )
        {
            candidates.push(draft);
        }
    }
    let draft = match candidates.as_slice() {
        [draft] => draft,
        [] => {
            return Err(execution_state(
                "no recoverable sealed proposal matches this node and digest",
                "/proposal",
            ));
        }
        _ => {
            return Err(execution_state(
                "multiple sealed proposals match this node; provide proposal-digest",
                "/proposalDigest",
            ));
        }
    };
    let ids = UuidIds;
    let clock = SystemClock;
    let externalizer = SealingGraphExternalizer::new(opener);
    let assigned_actor = assigned_actor.ok_or_else(|| {
        execution_state(
            "actor-id is required so the approved node has an authenticated owner",
            "/actorId",
        )
    })?;
    let assigned_id = graphhelm_protocols::ActorId::parse(assigned_actor)
        .map_err(|_| execution_state("the assigned actor identifier is invalid", "/actorId"))?;
    let mut assignments = BTreeMap::new();
    assignments.insert(
        OpaqueId::parse(node)
            .map_err(|_| execution_state("the node identifier is invalid", "/node"))?,
        PersistedActor::new(graphhelm_protocols::PersistedActorType::Agent, assigned_id),
    );
    let acceptance = ExecutionDraftAcceptance {
        draft_id: OpaqueId::parse(&draft.id)
            .map_err(|_| execution_state("the proposal identifier is invalid", "/proposal"))?,
        mode: projection
            .mode
            .ok_or_else(|| execution_state("the execution has no mode", "/mode"))?,
        proposal_sha256: requested_digest,
        assignments,
    };
    let apply = block_on_local(apply_draft_with_acceptance(
        &graphhelm_graph::GraphVersion::from_record(authoring)
            .map_err(|_| execution_state("the authoring snapshot is invalid", "/graph"))?,
        draft,
        &ApplyServices {
            event_repository: &store,
            scope: scope.clone(),
            stream_id: OpaqueId::parse(&stream)
                .map_err(|_| execution_state("the execution stream is invalid", "/execution"))?,
            actor: Actor::new(ActorType::Owner, "owner-cli"),
            clock: &clock,
            ids: &ids,
            externalizer: &externalizer,
        },
        Some(&acceptance),
    ))
    .map_err(|_| execution_state("the Governor rejected the proposal", "/proposal"))?;
    if !apply.events.iter().any(|event| {
        matches!(&event.kind, EventKind::GhostNodeProposed(payload) if payload.node_id.as_str() == node)
    }) {
        return Err(execution_state(
            "the Governor result did not contain the requested proposal and Ghost",
            "/proposal",
        ));
    }
    if !apply.events.iter().any(|event| {
        matches!(&event.kind, EventKind::NodeAssigned(payload) if payload.node_id.as_str() == node)
    }) || !apply.events.iter().any(|event| {
        matches!(&event.kind, EventKind::NodeOutcomeRecorded(payload) if payload.node_id.as_str() == node && payload.next_state == NodeState::Ready)
    }) {
        return Err(execution_state(
            "the Governor result did not contain the requested assignment and approval",
            "/proposal",
        ));
    }
    let final_projection = replay_projection(&store, &scope, &stream)?;
    Ok(render(
        &final_projection,
        &graphhelm_execution::AttentionInputs::for_surface(
            &final_projection,
            BTreeMap::new(),
            None,
        ),
        &super::Liveness::default(),
        None,
    ))
}

fn block_on_local<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("the local approval runtime must be constructible")
        .block_on(future)
}

/// Widened from private to `pub(crate)` (Milestone 05a Task 3), gaining `actor` and `key` as
/// explicit parameters — the mechanical widening the plan's file table names, plus the one
/// additional parameter the idempotent-retry semantics require (see `signal::execute`'s identical
/// note). No other logic changed.
pub(crate) fn execute(
    events: &Path,
    execution: Option<&str>,
    node: &str,
    actor: PersistedActor,
    key: OpaqueId,
) -> Result<serde_json::Value, Failure> {
    let store = event_store(events).map_err(|error| repository_failure(&error))?;
    let (scope, stream, projection) = load_projection(&store, execution)?;

    let execution_id = projection
        .execution_id
        .clone()
        .ok_or_else(|| execution_state("no execution has started on this stream", "/execution"))?;

    // Absent from `node_states` reads as `Draft`, the same convention `ready_set` and the driver
    // use — indistinguishable, from the projection alone, from a real node nobody has approved
    // yet, and `Draft` is correctly refused by the same check below.
    let state = projection
        .node_states
        .get(node)
        .copied()
        .unwrap_or(NodeState::Draft);
    if !matches!(state, NodeState::Ghost | NodeState::Blocked) {
        return Err(execution_state(
            &format!("node is {}, not ghost or blocked", node_state_label(state)),
            "/node",
        ));
    }

    let stream_id = OpaqueId::parse(&stream)
        .map_err(|_| execution_state("the stream identifier is not wire-safe", "/execution"))?;
    let execution_id = OpaqueId::parse(&execution_id)
        .map_err(|_| execution_state("the execution identifier is not wire-safe", "/execution"))?;
    record_outcome_with_key(
        &store,
        &scope,
        &stream_id,
        &execution_id,
        &actor,
        node,
        RecordedOutcome::uncaused(NodeOutcome::Approved),
        key,
    )?;

    let projection = replay_projection(&store, &scope, &stream)?;
    Ok(render(
        &projection,
        // Nothing measured here on purpose: this command reports the mutation it just made,
        // not a liveness reading. The seam turns "not measured" into `silenceUnevaluated`
        // rather than into calm, so the omission is stated instead of implied.
        // But the BUDGET is not a measurement. It is declared in the graph this projection
        // already holds, and `default()` asserted there was none -- so `attention` took the
        // `(None, measured)` arm and answered `NoDeclaredBudget` with the remedy "declare a
        // budget for this node", to an operator who had declared one (G's measurement on
        // #1013). `for_surface` derives it from the projection; the empty map is the AGE,
        // which really is unmeasured here, and that lands on the honest `(Some, None)` arm:
        // `NotMeasured`, remedy `Unavailable { SurfaceMeasuredNoAge }`.
        &graphhelm_execution::AttentionInputs::for_surface(
            &projection,
            std::collections::BTreeMap::new(),
            None,
        ),
        // Same posture for the instants: a mutation reply publishes null rather than a
        // stillness it never looked for.
        &super::Liveness::default(),
        None,
    ))
}
