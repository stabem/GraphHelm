//! Drive-to-quiescence over the pure pieces.
//!
//! `core/execution/tests/execution_lifecycle.rs` is the reference for the exact sequencing this
//! file reproduces against the real store: two `Started` hops from `Ready`, one from a
//! retry-pending `Queued` node (`core/events/tests/execution_projection.rs`'s `precondition`
//! comment: "a real scheduler cycles a retried node back out to the queue and redispatches it
//! before the next failure, which is scheduler behaviour these tests have no need to
//! reproduce" — this is that scheduler), `classify_progress` consulted before every dispatch, and
//! every `next_state` produced by the real `apply_transition`, never invented.
//!
//! The driver has no other source of truth: every loop iteration re-reads the projection by
//! replaying the stream through the store, exactly like the lifecycle test's `Journal::projection`.

use std::collections::BTreeSet;

use graphhelm_events::{ExecutionProjection, LocalEventRepository, PreparedAppend};
use graphhelm_execution::{
    NodeExecutor, TransitionRequest, apply_transition, classify_progress, dispatch_candidates,
    dispatch_plan,
};
use graphhelm_protocols::{
    EventKind, ExecutionCompleted, GraphSpec, IdGenerator, NewEvent, NodeOutcome,
    NodeOutcomeReason, NodeOutcomeRecorded, NodeState, OpaqueId, PersistedActor, RepositoryScope,
    Sensitivity, SimulationStatus,
};
use graphhelm_simulation::{FixtureExecutor, SimulationFixtures};

use super::{Failure, RecordedOutcome, execution_state, replay_failure, repository_failure};
use crate::commands::UuidIds;

/// Drives a started execution to quiescence: completion, blocked, waiting, or paused. Used by
/// `start` and (in a later task) `resume`.
pub(super) fn drive_to_quiescence(
    store: &LocalEventRepository,
    scope: &RepositoryScope,
    stream: &str,
    spec: &GraphSpec,
    fixtures: &SimulationFixtures,
    actor: &PersistedActor,
) -> Result<ExecutionProjection, Failure> {
    let stream_id = OpaqueId::parse(stream)
        .map_err(|_| execution_state("the stream identifier is not wire-safe", "/execution"))?;
    let execution_id_ref = scope.execution_id().ok_or_else(|| {
        execution_state(
            "the driver requires an execution-scoped repository",
            "/execution",
        )
    })?;
    let execution_id = OpaqueId::parse(execution_id_ref.as_str())
        .map_err(|_| execution_state("the execution identifier is not wire-safe", "/execution"))?;
    let executor = FixtureExecutor::new(fixtures.clone());

    loop {
        approve_untouched(store, scope, &stream_id, &execution_id, spec, actor)?;

        let projection = reread(store, scope, stream)?;
        // The ready set plus the retry-pending `Queued` nodes, both edge-gated by the ONE rule in
        // `graphhelm_execution::ready`. This used to be an inline union here — `ready_set` chained
        // with a bare `state == Queued` filter — and that filter is the #80 defect: a node sitting
        // `Queued` at a pass boundary was redispatched without anyone asking whether its
        // dependencies still held.
        //
        // The sync CLI driver and the async runtime driver each had their OWN copy of this union.
        // Fixing one left the other open, which is exactly the drift the extraction exists to
        // prevent, and it is why both now call the same function instead of agreeing by hand.
        let candidates: BTreeSet<String> = dispatch_candidates(spec, &projection.node_states)
            .map_err(|_| {
                execution_state(
                    "more nodes are ready than the execution may dispatch at once",
                    "/execution/readySet",
                )
            })?;

        let running = projection
            .node_states
            .values()
            .filter(|state| **state == NodeState::Running)
            .count();
        let max_parallel = match spec.budgets.max_parallel_model_calls {
            None => 1_usize,
            Some(value) => usize::try_from(value).unwrap_or(usize::MAX),
        };
        let plan = dispatch_plan(
            &candidates,
            &projection.node_attempts,
            running,
            max_parallel,
        )
        .map_err(|_| {
            execution_state(
                "the execution has zero parallelism and can never progress",
                "/execution/dispatch",
            )
        })?;

        if plan.is_empty() {
            break;
        }

        for node in &plan {
            let projection = reread(store, scope, stream)?;
            let current = projection
                .node_states
                .get(node)
                .copied()
                .unwrap_or(NodeState::Draft);
            // The verdict is consulted before every dispatch, but it never skips the real
            // executor call and never invents an outcome. For `NoProgress` the executor is
            // attempt-invariant, so calling it costs nothing and records the truth; for
            // `AttemptsExhausted` the counter is blind to *why* the attempts were spent — a
            // healthy node repeatedly interrupted would be recorded as failing, a fabrication —
            // and after an owner approval the node deserves a real answer, or approval becomes a
            // permanent dead end. Blocking, when due, comes from `apply_transition`'s own counter
            // condition acting on the executor's genuine outcome, exactly as the composed
            // lifecycle test does it.
            let _advisory = classify_progress(&projection, node, NodeOutcome::RetryableFailure);
            dispatch_hops(
                store,
                scope,
                &stream_id,
                &execution_id,
                actor,
                node,
                current,
            )?;
            let outcome = executor.execute(node, 0).map_err(|_| {
                execution_state(
                    "the executor rejected a legal dispatch",
                    "/execution/dispatch",
                )
            })?;
            record_outcome(
                store,
                scope,
                &stream_id,
                &execution_id,
                actor,
                node,
                // The outcome came from a fixture, so the fixture IS the cause (M07 F3):
                // recording it keeps a simulated red from being triaged as a real one.
                if outcome == NodeOutcome::Succeeded {
                    RecordedOutcome::uncaused(outcome)
                } else {
                    RecordedOutcome::caused(outcome, NodeOutcomeReason::FixtureScripted)
                },
            )?;
        }
    }

    complete_if_quiesced(store, scope, &stream_id, &execution_id, spec, actor)?;
    reread(store, scope, stream)
}

fn reread(
    store: &LocalEventRepository,
    scope: &RepositoryScope,
    stream: &str,
) -> Result<ExecutionProjection, Failure> {
    let history = store
        .read_replay_stream(scope, stream)
        .map_err(|error| repository_failure(&error))?;
    graphhelm_events::replay(scope, stream, &history).map_err(|error| replay_failure(&error))
}

fn append_event(
    store: &LocalEventRepository,
    scope: &RepositoryScope,
    stream: &OpaqueId,
    event: NewEvent,
) -> Result<(), Failure> {
    let next_sequence = store
        .next_sequence(scope, stream.as_str())
        .map_err(|error| repository_failure(&error))?;
    let request = PreparedAppend::new(
        scope.clone(),
        stream.clone(),
        next_sequence,
        vec![event],
        vec![],
        vec![],
    )
    .map_err(|error| repository_failure(&error))?;
    store
        .append_atomic(&request)
        .map_err(|error| repository_failure(&error))?;
    Ok(())
}

/// Appends one `node_outcome_recorded`, with `next_state` always computed by the real
/// `apply_transition` against the freshly replayed projection — the driver's only source of
/// truth for `current`, `attempts` and `identical_outcomes`.
fn record_outcome(
    store: &LocalEventRepository,
    scope: &RepositoryScope,
    stream: &OpaqueId,
    execution_id: &OpaqueId,
    actor: &PersistedActor,
    node: &str,
    recorded: RecordedOutcome,
) -> Result<NodeState, Failure> {
    let RecordedOutcome { outcome, reason } = recorded;
    let projection = reread(store, scope, stream.as_str())?;
    let current = projection
        .node_states
        .get(node)
        .copied()
        .unwrap_or(NodeState::Draft);
    let attempts = projection.node_attempts.get(node).copied().unwrap_or(0);
    let identical_outcomes = projection.identical_outcomes_for(node, outcome);
    let next_state = apply_transition(&TransitionRequest {
        current,
        outcome,
        attempts,
        identical_outcomes,
    })
    .map_err(|_| {
        execution_state(
            "the driver attempted an outcome the node's current state does not allow",
            "/execution/transition",
        )
    })?;
    let node_id = OpaqueId::parse(node)
        .map_err(|_| execution_state("node identifier is not wire-safe", "/execution"))?;
    append_event(
        store,
        scope,
        stream,
        NewEvent::new(
            idempotency_key("node-outcome"),
            actor.clone(),
            Sensitivity::Internal,
            EventKind::NodeOutcomeRecorded(NodeOutcomeRecorded {
                execution_id: execution_id.clone(),
                node_id,
                outcome,
                next_state,
                reason,
            }),
            vec![],
            vec![],
        ),
    )?;
    Ok(next_state)
}

/// One or two `Started` hops to reach `Running`, depending on whether `current` is a fresh
/// `Ready` node (two hops: `Ready -> Queued -> Running`) or a retry-pending `Queued` node the
/// scheduler is redispatching (one hop: `Queued -> Running`).
fn dispatch_hops(
    store: &LocalEventRepository,
    scope: &RepositoryScope,
    stream: &OpaqueId,
    execution_id: &OpaqueId,
    actor: &PersistedActor,
    node: &str,
    current: NodeState,
) -> Result<(), Failure> {
    if current == NodeState::Ready {
        record_outcome(
            store,
            scope,
            stream,
            execution_id,
            actor,
            node,
            RecordedOutcome::uncaused(NodeOutcome::Started),
        )?;
    }
    record_outcome(
        store,
        scope,
        stream,
        execution_id,
        actor,
        node,
        RecordedOutcome::uncaused(NodeOutcome::Started),
    )?;
    Ok(())
}

/// Approves every node still `Draft` to `Ready` — the legal route (`apply_transition`'s
/// `(Draft | Linting, Approved) -> Ready` arm).
fn approve_untouched(
    store: &LocalEventRepository,
    scope: &RepositoryScope,
    stream: &OpaqueId,
    execution_id: &OpaqueId,
    spec: &GraphSpec,
    actor: &PersistedActor,
) -> Result<(), Failure> {
    let projection = reread(store, scope, stream.as_str())?;
    for node in spec.nodes.keys() {
        let state = projection
            .node_states
            .get(node)
            .copied()
            .unwrap_or(NodeState::Draft);
        if state == NodeState::Draft {
            record_outcome(
                store,
                scope,
                stream,
                execution_id,
                actor,
                node,
                RecordedOutcome::uncaused(NodeOutcome::Approved),
            )?;
        }
    }
    Ok(())
}

/// A node in one of these states will never be revisited by this driver; the execution is done
/// with it either way.
fn is_terminal(state: NodeState) -> bool {
    matches!(
        state,
        NodeState::Succeeded
            | NodeState::Failed
            | NodeState::Waived
            | NodeState::Skipped
            | NodeState::Cancelled
    )
}

/// A terminal state that counts toward overall success rather than failure.
fn is_success_like(state: NodeState) -> bool {
    matches!(
        state,
        NodeState::Succeeded | NodeState::Waived | NodeState::Skipped
    )
}

/// If every node is terminal and the aggregate status is not already terminal, appends
/// `execution_completed` — `Completed` when every node succeeded, was waived or was skipped,
/// `Failed` otherwise.
fn complete_if_quiesced(
    store: &LocalEventRepository,
    scope: &RepositoryScope,
    stream: &OpaqueId,
    execution_id: &OpaqueId,
    spec: &GraphSpec,
    actor: &PersistedActor,
) -> Result<(), Failure> {
    let projection = reread(store, scope, stream.as_str())?;
    let already_terminal = matches!(
        projection.simulation_status,
        Some(SimulationStatus::Completed | SimulationStatus::Failed | SimulationStatus::Cancelled)
    );
    if already_terminal {
        return Ok(());
    }
    let all_terminal = spec.nodes.keys().all(|node| {
        projection
            .node_states
            .get(node)
            .is_some_and(|state| is_terminal(*state))
    });
    if !all_terminal {
        return Ok(());
    }
    let status = if spec.nodes.keys().all(|node| {
        projection
            .node_states
            .get(node)
            .is_some_and(|state| is_success_like(*state))
    }) {
        SimulationStatus::Completed
    } else {
        SimulationStatus::Failed
    };
    append_event(
        store,
        scope,
        stream,
        NewEvent::new(
            idempotency_key("execution-completed"),
            actor.clone(),
            Sensitivity::Internal,
            EventKind::ExecutionCompleted(ExecutionCompleted {
                execution_id: execution_id.clone(),
                status,
            }),
            vec![],
            vec![],
        ),
    )
}

fn idempotency_key(prefix: &'static str) -> OpaqueId {
    OpaqueId::parse(UuidIds.next_id(prefix)).expect("uuid-derived id is wire-safe")
}
