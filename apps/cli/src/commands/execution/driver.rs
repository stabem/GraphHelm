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

use super::{Failure, RecordedOutcome, execution_state, replay_failure, repository_failure};
use crate::commands::UuidIds;

/// The nodes a `resume` held and the actor whose decision releasing them is.
///
/// One parameter rather than two loose ones: they are meaningless apart — a set with no actor
/// cannot be attributed, and an actor with no set releases nothing — and clippy's argument-count
/// lint was right that the pair wanted a name.
pub(super) struct Release<'a> {
    pub(super) nodes: &'a BTreeSet<String>,
    /// The OWNER's, deliberately not the `actor` the drive runs under: releasing work the owner
    /// paused is the owner's act, every ordinary hop is machinery.
    pub(super) actor: &'a PersistedActor,
}

/// Drives a started execution to quiescence: completion, blocked, waiting, or paused. Used by
/// `start` and (in a later task) `resume`.
///
/// #123 adds the RELEASE half. `resume` no longer force-`Started`s the nodes a pause held; it
/// hands them here as `release`, and this loop lets each one go at the first pass where its edges
/// actually allow it. Timing is the whole point: a resume decides BEFORE the drive, and the edges
/// it would test are satisfied only AFTER — so the evaluation has to live in the loop that
/// repeats. `release_actor` is the OWNER's, deliberately different from `actor`: releasing work
/// the owner paused is the owner's act (D-019), while every ordinary hop below stays machinery.
/// Two actors, threaded rather than swapped — passing the owner's wholesale would silently make
/// every retry of every unrelated node read as an owner decision.
pub(super) fn drive_to_quiescence(
    store: &LocalEventRepository,
    scope: &RepositoryScope,
    stream: &str,
    spec: &GraphSpec,
    // INJECTED, not constructed here (#536). The loop used to build its own `FixtureExecutor`
    // from the fixtures, which left no way to make anything happen BETWEEN two dispatches -- and
    // that is the only place an ordinary pause can land. A test cannot reach this function's
    // inside; giving it the executor is the smallest change that makes the window constructible.
    // Both callers pass exactly what was built here before.
    executor: &dyn NodeExecutor,
    actor: &PersistedActor,
    release: &Release<'_>,
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

    // #1184 review (pass B): REFUSED BEFORE ANY NODE RUNS, and the same condition under the same
    // code the async driver uses -- both ask `graphhelm_execution::unreadable_customs_nodes`, so
    // the two drivers cannot come to disagree about which graphs are refusable. By the time the
    // park decision below runs, the node's work has already happened and a block that cannot be
    // read has no honest answer left.
    if let Some(node) = graphhelm_execution::unreadable_customs_nodes(spec).first() {
        return Err(execution_state(
            "a node declares a completion.customs block that cannot be read",
            &format!("/spec/nodes/{node}/completion"),
        ));
    }

    loop {
        approve_untouched(store, scope, &stream_id, &execution_id, spec, actor)?;

        let projection = reread(store, scope, stream)?;

        // THE RELEASE, before candidates are computed so a released node is dispatchable on this
        // same pass. Three conditions, and the third is not belt-and-braces: an ORDINARY (non
        // -immediate) pause does NOT stop an in-flight drive — the serve route signals the
        // driver's cancel channel only when `mode == "immediate"` (`serve/routes.rs:579`), and
        // this sync loop has no channel at all. So without re-reading the aggregate we would
        // release a node the operator paused while this drive was still running.
        let mut released_any = false;
        for node in release.nodes {
            let held =
                projection.node_states.get(node.as_str()).copied() == Some(NodeState::Paused);
            let execution_paused = projection.simulation_status == Some(SimulationStatus::Paused);
            if held
                && !execution_paused
                && graphhelm_execution::edges_satisfied(spec, &projection.node_states, node)
            {
                record_outcome(
                    store,
                    scope,
                    &stream_id,
                    &execution_id,
                    release.actor,
                    node,
                    RecordedOutcome::uncaused(NodeOutcome::Started),
                )?;
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
            reread(store, scope, stream)?
        } else {
            projection
        };

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
        let max_parallel = graphhelm_execution::parallel_limit(&spec.budgets);
        // #536: AN ORDINARY PAUSE STOPS DISPATCH, here as in the async driver (#124/#534). The
        // pause route appends `execution_paused` and signals nothing, and this loop has no cancel
        // channel at all -- so the only way it can learn is by asking the projection it re-reads
        // every pass.
        //
        // An EMPTY PLAN rather than a new exit: `plan.is_empty()` below is the quiescence exit
        // this loop already had. Nothing is interrupted and no event is appended; those belong to
        // `pause {"mode":"immediate"}`, which this driver never serves anyway.
        let execution_paused = projection.simulation_status == Some(SimulationStatus::Paused);
        let plan = if execution_paused {
            Vec::new()
        } else {
            dispatch_plan(
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
            })?
        };

        if plan.is_empty() {
            break;
        }

        for node in &plan {
            let projection = reread(store, scope, stream)?;
            // ASKED AGAIN AT THE POINT OF DISPATCH, because the plan above was computed from an
            // earlier read and a pause appended since then is invisible to it. Node state cannot
            // stand in: an ordinary pause moves NO node state, so this node is still `Ready` and
            // would sail through the gate below with the pause already in the log.
            //
            // The break needs the plan gate above to end the pass -- this only skips the rest of
            // one plan. Both are load-bearing; the async driver's equivalent pair spins forever
            // when the plan gate alone is removed.
            // NARROWED, NOT ELIMINATED, and the residual is worth stating rather than leaving for
            // the next reader to find. A pause appended between this read and the dispatch append
            // below is still missed: `append_hop` reads `store.next_sequence` immediately before
            // appending (`driver.rs:257`), so its expected sequence is taken AFTER the pause and
            // the append succeeds.
            //
            // Closing it does NOT need a primitive the codebase lacks -- the store already refuses
            // a stale expected sequence with `SequenceConflict`
            // (`core/events/src/local.rs:877-879`). It needs the sequence CAPTURED HERE and
            // threaded to the append, so that anything landing in between is refused. That is a
            // behaviour change beyond this fix: it would turn EVERY concurrent append into a
            // refused dispatch, not only a pause, and the retry policy for that refusal is a
            // decision nobody has taken.
            if projection.simulation_status == Some(SimulationStatus::Paused) {
                break;
            }
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
            // #1184 review (pass B): THE PARK, on this driver too. It was added to the async
            // driver alone, so a customs node started through the CLI reached `Succeeded` and the
            // execution completed with the declaration inert -- on exactly the path an operator
            // uses by hand. The predicate is the shared one; a change to it moves both drivers.
            //
            // ONLY a success is converted, for the reason the async side gives: parking a node
            // that failed would ask a person to attest to work that did not happen, and would
            // take the node out of the retry path that owns it.
            let outcome = if outcome == NodeOutcome::Succeeded
                && graphhelm_execution::completion_is_gated_in(spec, node)
            {
                NodeOutcome::NeedsInput
            } else {
                outcome
            };
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
            .is_some_and(|state| graphhelm_execution::is_terminal(*state))
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

#[cfg(test)]
mod tests {
    use super::*;
    use graphhelm_protocols::{
        ExecutionId, ExecutionMode, ExecutionPaused, ExecutionStarted, GraphBudgets, GraphNode,
        NodeType, Optionality, ProjectId, Sensitivity, WireHash, WorkspaceId,
    };
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    const WORKSPACE: &str = "workspace-1";
    const PROJECT: &str = "project-1";
    const EXECUTION: &str = "exec-sync-pause";

    fn scope() -> RepositoryScope {
        RepositoryScope::new(
            WorkspaceId::parse(WORKSPACE).unwrap(),
            ProjectId::parse(PROJECT).unwrap(),
            Some(ExecutionId::parse(EXECUTION).unwrap()),
        )
    }

    fn node(objective: &str) -> GraphNode {
        GraphNode {
            node_type: NodeType::Agent,
            name: "n".to_owned(),
            objective: objective.to_owned(),
            optionality: Optionality::Required,
            properties: BTreeMap::new(),
        }
    }

    /// Two INDEPENDENT nodes. `parallel` decides which window the cell exercises: at 1 the pause
    /// lands between two passes, at 2 it lands between two dispatches of the SAME pass.
    fn spec(parallel: u64) -> GraphSpec {
        GraphSpec {
            entrypoints: vec!["first".to_owned(), "second".to_owned()],
            nodes: [("first", node("a")), ("second", node("b"))]
                .into_iter()
                .map(|(id, n)| (id.to_owned(), n))
                .collect(),
            edges: Vec::new(),
            budgets: GraphBudgets {
                max_parallel_model_calls: Some(parallel),
                ..GraphBudgets::default()
            },
            policies: Vec::new(),
            completion: serde_json::Value::Null,
        }
    }

    fn started_store(events: &Path) -> LocalEventRepository {
        let store = crate::commands::event_store(events).unwrap();
        let request = PreparedAppend::new(
            scope(),
            OpaqueId::parse(EXECUTION).unwrap(),
            1,
            vec![NewEvent::new(
                OpaqueId::parse("sync-pause-started").unwrap(),
                super::super::system_actor(),
                Sensitivity::Internal,
                EventKind::ExecutionStarted(ExecutionStarted {
                    execution_id: OpaqueId::parse(EXECUTION).unwrap(),
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
        .unwrap();
        store.append_atomic(&request).unwrap();
        store
    }

    /// THE ARRANGEMENT #536 asked for, and it needs no threads.
    ///
    /// An ordinary pause is an APPEND and nothing else — the route appends `execution_paused` and
    /// returns, signalling nothing. So the pause does not have to arrive from outside the drive:
    /// the executor can append it, at a moment the test picks exactly. That removes the wall the
    /// issue recorded (nothing on the sync path can be held open) by removing the need to hold
    /// anything open at all.
    struct PauseOnFirstDispatch {
        calls: AtomicUsize,
        events: PathBuf,
    }

    impl NodeExecutor for PauseOnFirstDispatch {
        fn execute(
            &self,
            _node_id: &str,
            _attempt: u32,
        ) -> Result<NodeOutcome, graphhelm_execution::ExecutionError> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                let store = crate::commands::event_store(&self.events).unwrap();
                let head = store.read_replay_stream(&scope(), EXECUTION).unwrap().len() as u64;
                let request = PreparedAppend::new(
                    scope(),
                    OpaqueId::parse(EXECUTION).unwrap(),
                    head + 1,
                    vec![NewEvent::new(
                        OpaqueId::parse("sync-ordinary-pause").unwrap(),
                        super::super::system_actor(),
                        Sensitivity::Internal,
                        EventKind::ExecutionPaused(ExecutionPaused {
                            execution_id: OpaqueId::parse(EXECUTION).unwrap(),
                        }),
                        vec![],
                        vec![],
                    )],
                    vec![],
                    vec![],
                )
                .unwrap();
                store.append_atomic(&request).expect(
                    "HARNESS-BROKE: the pause never landed, so the cell never posed its question",
                );
            }
            Ok(NodeOutcome::Succeeded)
        }
    }

    fn drive_with_pause(parallel: u64) -> usize {
        let directory = tempfile::tempdir().unwrap();
        let events = directory.path().join("events");
        let store = started_store(&events);
        let executor = PauseOnFirstDispatch {
            calls: AtomicUsize::new(0),
            events: events.clone(),
        };
        let nothing_to_release = BTreeSet::new();
        let outcome = drive_to_quiescence(
            &store,
            &scope(),
            EXECUTION,
            &spec(parallel),
            &executor,
            &super::super::system_actor(),
            &Release {
                nodes: &nothing_to_release,
                actor: &super::super::system_actor(),
            },
        );
        // `Failure` carries no `Debug`, so an `unwrap` would not compile and a bare `is_ok` would
        // hide which drive broke. Name the parallelism instead.
        assert!(
            outcome.is_ok(),
            "the drive at max_parallel={parallel} failed"
        );
        executor.calls.load(Ordering::SeqCst)
    }

    /// #536 window 1 — the pause lands BETWEEN PASSES.
    ///
    /// `max_parallel = 1`, so each pass plans one node. The first dispatch appends the pause; the
    /// pass after it must not plan the second. THE PRODUCTION CHANGE THAT MAKES THIS FAIL:
    /// computing the plan without asking `simulation_status`.
    #[test]
    fn an_ordinary_pause_between_passes_stops_the_sync_drive() {
        assert_eq!(
            drive_with_pause(1),
            1,
            "the sync drive planned another node after execution_paused was readable"
        );
    }

    /// #536 window 2 — the pause lands BETWEEN TWO DISPATCHES OF ONE PASS.
    ///
    /// `max_parallel = 2`, so a single plan holds both nodes and the pause arrives after the plan
    /// was computed. Only a check at the point of dispatch can see it; the plan gate cannot, and
    /// node state cannot either — an ordinary pause moves no node state, so `second` is still
    /// `Ready` when its turn comes. THE PRODUCTION CHANGE THAT MAKES THIS FAIL: checking only
    /// before the plan.
    #[test]
    fn an_ordinary_pause_mid_plan_stops_the_sync_drive() {
        assert_eq!(
            drive_with_pause(2),
            1,
            "the sync drive dispatched the rest of a plan computed before the pause"
        );
    }
}
