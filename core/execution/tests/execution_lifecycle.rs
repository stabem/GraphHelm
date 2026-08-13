//! The composed lifecycle, replayed.
//!
//! This is the first time every pure piece shipped since 04a runs together: `ready_set`
//! proposes, `dispatch_plan` bounds, `FixtureExecutor` executes, `apply_transition` decides, the
//! `graphhelm_events` fold records, `recovery_plan` and `resume_preconditions` gate the
//! lifecycle. The driver here is test-only — a loop inside this file — and shipping the
//! production driver remains 04f's job; nothing here widens any crate's public API.
//!
//! Four acts, all folded onto one growing `Vec<EventEnvelope>` (via [`Journal`]) so the final
//! replay covers the whole story: run, pause, and (on a second, fresh history) crash and
//! recover, then complete and replay.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use chrono::{TimeZone, Utc};
use graphhelm_events::{
    ExecutionProjection, LocalEventRepository, PreparedAppend, ProjectionGeneration, replay,
};
use graphhelm_execution::{
    MAX_NODE_ATTEMPTS, NodeExecutor, Progress, ResumeError, TransitionRequest, apply_transition,
    classify_progress, dispatch_plan, ready_set, recovery_plan, resume_preconditions,
};
use graphhelm_protocols::{
    ActorId, Clock, EdgeType, EventEnvelope, EventKind, ExecutionCompleted, ExecutionId,
    ExecutionMode, ExecutionPaused, ExecutionResumed, ExecutionStarted, FixtureOutcome,
    GraphBudgets, GraphEdge, GraphNode, GraphSpec, IdGenerator, NewEvent, NodeOutcome,
    NodeOutcomeRecorded, NodeState, NodeType, OpaqueId, Optionality, PersistedActor,
    PersistedActorType, ProjectId, RepositoryScope, Sensitivity, SimulationStatus, WireHash,
    WorkspaceId,
};
use graphhelm_simulation::{FixtureExecutor, SimulationFixtures};

const STREAM: &str = "stream-lifecycle-test";

struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 10, 12, 0, 0).unwrap()
    }
}

#[derive(Default)]
struct Ids(AtomicU64);
impl IdGenerator for Ids {
    fn next_id(&self, prefix: &'static str) -> String {
        format!("{prefix}-{}", self.0.fetch_add(1, Ordering::SeqCst) + 1)
    }
}

fn scope() -> RepositoryScope {
    RepositoryScope::new(
        WorkspaceId::parse("workspace-lifecycle").unwrap(),
        ProjectId::parse("project-lifecycle").unwrap(),
        Some(ExecutionId::parse("execution-lifecycle").unwrap()),
    )
}

fn actor() -> PersistedActor {
    PersistedActor::new(
        PersistedActorType::System,
        ActorId::parse("system-lifecycle").unwrap(),
    )
}

fn event(key: impl Into<String>, kind: EventKind) -> NewEvent {
    NewEvent::new(
        OpaqueId::parse(key.into()).unwrap(),
        actor(),
        Sensitivity::Internal,
        kind,
        vec![],
        vec![],
    )
}

/// A growing, single stream backed by the real repository. Every act appends here (through
/// [`Journal::append`]) so the final replay covers the whole composed story, exactly as
/// `core/events/tests/execution_projection.rs`'s `append` helper does for one batch at a time.
struct Journal {
    _directory: tempfile::TempDir,
    repository: LocalEventRepository,
    events: Vec<EventEnvelope>,
}

impl Journal {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let repository = LocalEventRepository::open(
            directory.path(),
            Arc::new(FixedClock),
            Arc::new(Ids::default()),
        )
        .unwrap();
        Self {
            _directory: directory,
            repository,
            events: Vec::new(),
        }
    }

    fn append(&mut self, new_events: Vec<NewEvent>) {
        let expected_next_sequence = u64::try_from(self.events.len()).unwrap() + 1;
        let request = PreparedAppend::new(
            scope(),
            OpaqueId::parse(STREAM).unwrap(),
            expected_next_sequence,
            new_events,
            vec![],
            vec![],
        )
        .unwrap();
        let appended = self.repository.append_atomic(&request).unwrap();
        self.events.extend(appended);
    }

    fn projection(&self) -> ExecutionProjection {
        replay(&scope(), STREAM, &self.events).unwrap()
    }
}

fn agent_node() -> GraphNode {
    GraphNode {
        node_type: NodeType::Agent,
        name: "n".to_owned(),
        objective: "o".to_owned(),
        optionality: Optionality::Required,
        properties: BTreeMap::new(),
    }
}

/// The three-node chain `a -> b -> c`, built the way `core/execution/src/ready.rs`'s tests do.
fn chain_spec() -> GraphSpec {
    let mut spec = GraphSpec {
        entrypoints: vec!["a".to_owned()],
        nodes: BTreeMap::new(),
        edges: Vec::new(),
        budgets: GraphBudgets::default(),
        policies: Vec::new(),
        completion: serde_json::Value::Null,
    };
    for node in ["a", "b", "c"] {
        spec.nodes.insert(node.to_owned(), agent_node());
    }
    for (from, to) in [("a", "b"), ("b", "c")] {
        spec.edges.push(GraphEdge {
            id: format!("{from}-to-{to}"),
            from: from.to_owned(),
            to: to.to_owned(),
            edge_type: EdgeType::Control,
            payload_schema: None,
            condition: None,
            on_false: None,
            on_unknown: None,
            bindings: BTreeMap::new(),
            priority: None,
        });
    }
    spec
}

fn all_success_executor() -> FixtureExecutor {
    let mut fixtures = SimulationFixtures::default();
    for node in ["a", "b", "c"] {
        fixtures
            .node_outcomes
            .insert(node.to_owned(), FixtureOutcome::Success);
    }
    FixtureExecutor::new(fixtures)
}

fn execution_started_event(execution_id: &OpaqueId) -> NewEvent {
    event(
        "execution-started",
        EventKind::ExecutionStarted(ExecutionStarted {
            execution_id: execution_id.clone(),
            graph_version: 1,
            graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
            mode: ExecutionMode::Autopilot,
        }),
    )
}

/// Runs one outcome for `node` through the real `apply_transition`, folds it as
/// `node_outcome_recorded`, and returns the resulting state. `current`, `attempts` and
/// `identical_outcomes` are read from the journal's own replayed projection — the single source
/// of truth a real driver would consult — never hand-tracked locally.
fn record_outcome(
    journal: &mut Journal,
    execution_id: &OpaqueId,
    node: &str,
    outcome: NodeOutcome,
    key: impl Into<String>,
) -> NodeState {
    let projection = journal.projection();
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
    .unwrap();
    journal.append(vec![event(
        key,
        EventKind::NodeOutcomeRecorded(NodeOutcomeRecorded {
            execution_id: execution_id.clone(),
            node_id: OpaqueId::parse(node).unwrap(),
            outcome,
            next_state,
        }),
    )]);
    next_state
}

/// Approves every node still `Draft`, the legal route from `Draft` to `Ready`
/// (`apply_transition`'s `(Draft | Linting, Approved) -> Ready` arm).
fn approve_untouched(journal: &mut Journal, execution_id: &OpaqueId, spec: &GraphSpec) {
    let projection = journal.projection();
    for node in spec.nodes.keys() {
        if projection
            .node_states
            .get(node)
            .copied()
            .unwrap_or(NodeState::Draft)
            == NodeState::Draft
        {
            record_outcome(
                journal,
                execution_id,
                node,
                NodeOutcome::Approved,
                format!("approve-{node}"),
            );
        }
    }
}

/// Drives one dispatched node from `Ready` to a terminal fixture outcome: the two `Started` hops
/// (`Ready -> Queued`, `Queued -> Running`) then the executor's answer, each folded as its own
/// `node_outcome_recorded`.
fn drive_dispatched_node(
    journal: &mut Journal,
    execution_id: &OpaqueId,
    node: &str,
    executor: &FixtureExecutor,
) -> NodeOutcome {
    record_outcome(
        journal,
        execution_id,
        node,
        NodeOutcome::Started,
        format!("{node}-queued"),
    );
    record_outcome(
        journal,
        execution_id,
        node,
        NodeOutcome::Started,
        format!("{node}-running"),
    );
    let outcome = executor.execute(node, 0).unwrap();
    record_outcome(
        journal,
        execution_id,
        node,
        outcome,
        format!("{node}-outcome"),
    );
    outcome
}

/// One composed run: three-node chain, pause, a separate crash-and-recover scenario, resume and
/// completion, then full-history replay — the 04b replay guarantee proved over a history
/// containing every lifecycle event kind.
#[test]
fn the_composed_lifecycle_pauses_recovers_completes_and_replays() {
    let execution_id = OpaqueId::parse("execution-lifecycle").unwrap();
    let spec = chain_spec();
    let executor = all_success_executor();

    // ---- Act 1: run --------------------------------------------------------------------
    let mut journal = Journal::new();
    journal.append(vec![execution_started_event(&execution_id)]);

    loop {
        approve_untouched(&mut journal, &execution_id, &spec);
        let projection = journal.projection();
        let ready = ready_set(&spec, &projection.node_states).unwrap();
        let in_flight = projection
            .node_states
            .values()
            .filter(|state| matches!(state, NodeState::Queued | NodeState::Running))
            .count();
        let plan = dispatch_plan(&ready, in_flight, 1).unwrap();
        assert!(
            !plan.is_empty(),
            "the chain must never stall in this scenario"
        );
        for node in &plan {
            assert_eq!(
                classify_progress(&projection, node, NodeOutcome::Started),
                Progress::Continue,
                "a driver consults classify_progress before every dispatch"
            );
            let outcome = drive_dispatched_node(&mut journal, &execution_id, node, &executor);
            assert_eq!(outcome, NodeOutcome::Succeeded);
        }
        if journal.projection().node_states.get("a") == Some(&NodeState::Succeeded) {
            break;
        }
    }

    let projection = journal.projection();
    assert_eq!(projection.node_states.get("a"), Some(&NodeState::Succeeded));
    // b and c were pre-approved in the same pass that approved a, so both sit Ready even though
    // ready_set never surfaced them yet (b's predecessor a had not succeeded until this instant).
    assert_eq!(projection.node_states.get("b"), Some(&NodeState::Ready));
    assert_eq!(projection.node_states.get("c"), Some(&NodeState::Ready));

    // ---- Act 2: pause -------------------------------------------------------------------
    journal.append(vec![event(
        "execution-paused",
        EventKind::ExecutionPaused(ExecutionPaused {
            execution_id: execution_id.clone(),
        }),
    )]);
    assert_eq!(
        journal.projection().simulation_status,
        Some(SimulationStatus::Paused)
    );

    for node in ["b", "c"] {
        record_outcome(
            &mut journal,
            &execution_id,
            node,
            NodeOutcome::Paused,
            format!("{node}-paused"),
        );
    }
    let projection = journal.projection();
    assert_eq!(projection.node_states.get("b"), Some(&NodeState::Paused));
    assert_eq!(projection.node_states.get("c"), Some(&NodeState::Paused));

    let ready_while_paused = ready_set(&spec, &projection.node_states).unwrap();
    assert!(
        ready_while_paused.is_empty(),
        "a paused node must not be dispatchable"
    );

    // ---- Act 3: crash and recover, on a SECOND, fresh history ----------------------------
    //
    // Finding for 04f: the plan's Task 7 narrates this as "fold the interrupted outcome; assert
    // resume_preconditions refuses while a node is Running" — but by the time the interrupted
    // outcome is folded, the node is Blocked, not Running, so that assertion cannot be checked
    // there. The guard that actually rejects a Running node (`ResumeError::UnrecoveredInterruption`)
    // only fires when `simulation_status` is already `Paused`, and `execution_paused` is legal
    // while a node is still Running (the fold only inspects the aggregate's own status, never node
    // states). So the honest order is: drive to the crash point, pause the aggregate *while the
    // node is still Running* (proving the refusal with its real reason), THEN fold the interrupted
    // outcome to recover it. A second surprise: `resume_preconditions` already returns `Ok(())`
    // immediately after that recovery fold, before any owner approval — the pure gate only checks
    // for `Running` nodes, never `Blocked` ones. Owner-approving the blocked node is necessary for
    // the execution to make forward progress after resume (a `Blocked` node never dispatches on its
    // own), but it is not a precondition `resume_preconditions` itself enforces.
    let mut crash_journal = Journal::new();
    crash_journal.append(vec![execution_started_event(&execution_id)]);
    approve_untouched(&mut crash_journal, &execution_id, &spec);
    let a_outcome = drive_dispatched_node(&mut crash_journal, &execution_id, "a", &executor);
    assert_eq!(a_outcome, NodeOutcome::Succeeded);

    let projection = crash_journal.projection();
    let ready = ready_set(&spec, &projection.node_states).unwrap();
    assert_eq!(ready, ["b".to_owned()].into_iter().collect::<BTreeSet<_>>());

    // Two Started hops, no outcome: this is the crash. b is Running with unknown effects.
    record_outcome(
        &mut crash_journal,
        &execution_id,
        "b",
        NodeOutcome::Started,
        "b-queued",
    );
    record_outcome(
        &mut crash_journal,
        &execution_id,
        "b",
        NodeOutcome::Started,
        "b-running",
    );
    let projection = crash_journal.projection();
    assert_eq!(projection.node_states.get("b"), Some(&NodeState::Running));
    assert_eq!(recovery_plan(&projection), vec!["b".to_owned()]);

    // Pause the aggregate while b is still Running: legal, because the pause guard only checks
    // simulation_status (None | Running), never node states.
    crash_journal.append(vec![event(
        "execution-paused",
        EventKind::ExecutionPaused(ExecutionPaused {
            execution_id: execution_id.clone(),
        }),
    )]);
    let projection = crash_journal.projection();
    assert_eq!(projection.simulation_status, Some(SimulationStatus::Paused));
    assert_eq!(
        resume_preconditions(&projection, None),
        Err(ResumeError::UnrecoveredInterruption),
        "resume_preconditions must refuse while a node is still Running"
    );

    // Recover: fold the interrupted outcome. (Running, Interrupted) -> Blocked is the only legal
    // consequence.
    record_outcome(
        &mut crash_journal,
        &execution_id,
        "b",
        NodeOutcome::Interrupted,
        "b-interrupted",
    );
    let projection = crash_journal.projection();
    assert_eq!(projection.node_states.get("b"), Some(&NodeState::Blocked));
    assert!(recovery_plan(&projection).is_empty());

    // The pure gate already passes here — see the finding above.
    assert_eq!(resume_preconditions(&projection, None), Ok(()));

    // The owner resumes the blocked node so it is schedulable again once the execution resumes.
    record_outcome(
        &mut crash_journal,
        &execution_id,
        "b",
        NodeOutcome::Approved,
        "b-owner-approved",
    );
    let projection = crash_journal.projection();
    assert_eq!(projection.node_states.get("b"), Some(&NodeState::Ready));
    assert_eq!(resume_preconditions(&projection, None), Ok(()));

    // ---- Act 4: complete and replay, back on the MAIN history -----------------------------
    let projection = journal.projection();
    assert_eq!(resume_preconditions(&projection, None), Ok(()));

    journal.append(vec![event(
        "execution-resumed",
        EventKind::ExecutionResumed(ExecutionResumed {
            execution_id: execution_id.clone(),
        }),
    )]);
    assert_eq!(
        journal.projection().simulation_status,
        Some(SimulationStatus::Running)
    );

    // Resuming a Paused node is 04e's business, not the scheduler's — ready_set only ever
    // dispatches a node already in state Ready (see ready.rs's is_dispatchable) — so the driver
    // applies the (Paused, Started) -> Queued hop directly, one node at a time in DAG order, then
    // the ordinary (Queued, Started) -> Running hop and the executor's outcome.
    for node in ["b", "c"] {
        let before = journal.projection();
        assert_eq!(before.node_states.get(node), Some(&NodeState::Paused));
        let outcome = drive_dispatched_node(&mut journal, &execution_id, node, &executor);
        assert_eq!(outcome, NodeOutcome::Succeeded);
    }

    let projection = journal.projection();
    assert_eq!(projection.node_states.get("a"), Some(&NodeState::Succeeded));
    assert_eq!(projection.node_states.get("b"), Some(&NodeState::Succeeded));
    assert_eq!(projection.node_states.get("c"), Some(&NodeState::Succeeded));

    journal.append(vec![event(
        "execution-completed",
        EventKind::ExecutionCompleted(ExecutionCompleted {
            execution_id: execution_id.clone(),
            status: SimulationStatus::Completed,
        }),
    )]);
    assert_eq!(
        journal.projection().simulation_status,
        Some(SimulationStatus::Completed)
    );

    // The 04b replay guarantee, now over a history containing every lifecycle event kind:
    // replaying the whole history twice must be byte-identical...
    let history = journal.events.clone();
    let first = replay(&scope(), STREAM, &history).unwrap();
    let second = replay(&scope(), STREAM, &history).unwrap();
    let first_json = serde_json::to_string(&first).unwrap();
    let second_json = serde_json::to_string(&second).unwrap();
    assert_eq!(first_json, second_json);

    // ...and a disposable generation, resumed from an arbitrary split point via apply_page, must
    // land on exactly the same projection as the direct replay.
    let split_at = history.len() / 2;
    let (first_half, second_half) = history.split_at(split_at);
    let mut generation =
        ProjectionGeneration::new(scope(), STREAM.to_owned(), "execution".to_owned(), 1, 1)
            .unwrap();
    generation.apply_page(first_half).unwrap();
    generation.apply_page(second_half).unwrap();
    let generation_json = serde_json::to_string(generation.projection()).unwrap();
    assert_eq!(first_json, generation_json);
}

/// Proves the composition can fail: a fixture that always retries drives the transition table's
/// blocking condition on `(Running, RetryableFailure)`, then the owner resumes it exactly like
/// `an_owner_approval_readies_a_blocked_node` pins at the unit level.
///
/// Note for the report: this scenario exhausts via `attempts`, not `identical_outcomes`, even
/// though `classify_progress` and `apply_transition` both judge either. Every retry cycle folds
/// `Started` between failures (the real redispatch a scheduler performs), and `Started` becomes
/// the node's `last_outcome` each time — so `identical_outcomes_for(node, RetryableFailure)`
/// resets to 0 on every cycle and never reaches `MAX_IDENTICAL_OUTCOMES`. Only the `attempts`
/// counter, which survives the redispatch, ever reaches its bound.
#[test]
fn a_failing_node_blocks_and_owner_resumes() {
    let execution_id = OpaqueId::parse("execution-lifecycle").unwrap();
    let mut spec = GraphSpec {
        entrypoints: vec!["a".to_owned()],
        nodes: BTreeMap::new(),
        edges: Vec::new(),
        budgets: GraphBudgets::default(),
        policies: Vec::new(),
        completion: serde_json::Value::Null,
    };
    spec.nodes.insert("a".to_owned(), agent_node());

    let mut fixtures = SimulationFixtures::default();
    fixtures
        .node_outcomes
        .insert("a".to_owned(), FixtureOutcome::Failure);
    let executor = FixtureExecutor::new(fixtures);

    let mut journal = Journal::new();
    journal.append(vec![execution_started_event(&execution_id)]);
    approve_untouched(&mut journal, &execution_id, &spec);

    let mut cycle = 0_u32;
    loop {
        let projection = journal.projection();
        if classify_progress(&projection, "a", NodeOutcome::Started) != Progress::Continue {
            break;
        }
        cycle += 1;
        let current = projection.node_states.get("a").copied().unwrap();
        // Ready needs both Started hops; a retried node is already Queued and needs only one.
        if current == NodeState::Ready {
            record_outcome(
                &mut journal,
                &execution_id,
                "a",
                NodeOutcome::Started,
                format!("a-queued-{cycle}"),
            );
        }
        record_outcome(
            &mut journal,
            &execution_id,
            "a",
            NodeOutcome::Started,
            format!("a-running-{cycle}"),
        );
        let outcome = executor.execute("a", 0).unwrap();
        assert_eq!(outcome, NodeOutcome::RetryableFailure);
        let next = record_outcome(
            &mut journal,
            &execution_id,
            "a",
            outcome,
            format!("a-failure-{cycle}"),
        );
        // (Running, RetryableFailure) re-dispatches to Queued until attempts are exhausted.
        assert!(matches!(next, NodeState::Queued | NodeState::Blocked));
    }

    let projection = journal.projection();
    assert_eq!(projection.node_states.get("a"), Some(&NodeState::Blocked));
    assert_eq!(projection.node_attempts.get("a"), Some(&MAX_NODE_ATTEMPTS));
    assert_eq!(
        classify_progress(&projection, "a", NodeOutcome::Started),
        Progress::AttemptsExhausted
    );

    // The owner's resume path out of Blocked.
    record_outcome(
        &mut journal,
        &execution_id,
        "a",
        NodeOutcome::Approved,
        "a-owner-approved",
    );
    assert_eq!(
        journal.projection().node_states.get("a"),
        Some(&NodeState::Ready)
    );
}
