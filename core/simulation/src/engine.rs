use std::collections::{BTreeMap, BTreeSet, VecDeque};

use chrono::{DateTime, Utc};
use graphhelm_events::{EventRepository, EventRepositoryError, PreparedAppend};
use graphhelm_graph::{GraphVersion, raw_content_sha256};
use graphhelm_protocols::{
    Clock, Diagnostic, EventEnvelope, EventKind, FixtureOutcome, GraphEdge, IdGenerator, NewEvent,
    NodeState, NodeStateChanged, OpaqueId, PersistedActor, RepositoryScope, Sensitivity,
    SimulationCompleted, SimulationStarted, SimulationStatus, UnknownConditionBehavior, WireHash,
};
use thiserror::Error;

use crate::SimulationFixtures;

const MAX_LOOP_ITERATIONS: u64 = 100;
const MAX_SIMULATION_TRANSITIONS: usize = 5_000;

struct ControlledLoop {
    nodes: BTreeSet<String>,
    limit: u64,
    completed: u64,
}

pub struct SimulationServices<'a> {
    pub event_repository: &'a dyn EventRepository,
    pub scope: RepositoryScope,
    pub stream_id: OpaqueId,
    pub actor: PersistedActor,
    pub clock: &'a dyn Clock,
    pub ids: &'a dyn IdGenerator,
}

/// Durable effect-free simulation output.
#[derive(Clone, Debug)]
pub struct SimulationResult {
    pub simulation_id: String,
    pub started_at: DateTime<Utc>,
    pub status: SimulationStatus,
    pub node_states: BTreeMap<String, NodeState>,
    pub diagnostics: Vec<Diagnostic>,
    pub events: Vec<EventEnvelope>,
    transitions: Vec<(String, NodeState)>,
}

impl SimulationResult {
    #[must_use]
    pub fn transition_trace(&self) -> &[(String, NodeState)] {
        &self.transitions
    }
}

#[derive(Debug, Error)]
pub enum SimulationError {
    #[error(transparent)]
    Repository(#[from] EventRepositoryError),
}

impl SimulationError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Repository(error) => error.code(),
        }
    }
}

/// Simulates state transitions without invoking tools, models, networks, or graph payloads.
pub fn simulate(
    graph: &GraphVersion,
    fixtures: &SimulationFixtures,
    services: &SimulationServices<'_>,
) -> Result<SimulationResult, SimulationError> {
    let simulation_id = OpaqueId::parse(services.ids.next_id("simulation"))
        .map_err(|_| SimulationError::Repository(EventRepositoryError::Invalid))?;
    let started_at = services.clock.now();
    let mut pending: BTreeSet<_> = graph.graph().spec.nodes.keys().cloned().collect();
    let mut queue: VecDeque<_> = graph.graph().spec.entrypoints.iter().cloned().collect();
    let mut states = BTreeMap::new();
    let mut transitions = Vec::new();
    let mut diagnostics = Vec::new();
    let mut status = SimulationStatus::Running;
    let graph_hash = WireHash::parse(graph.content_hash().as_str())
        .map_err(|_| SimulationError::Repository(EventRepositoryError::Invalid))?;
    let mut pending_events = vec![simulation_event(
        simulation_idempotency_key(b"started", &simulation_id, None, 0)?,
        services,
        EventKind::SimulationStarted(SimulationStarted {
            simulation_id: simulation_id.clone(),
            graph_version: graph.number(),
            graph_hash,
        }),
    )];
    let mut controlled_loops = controlled_loops(graph);
    let invalid_limit = controlled_loops
        .iter()
        .any(|component| component.limit > MAX_LOOP_ITERATIONS);
    let estimated = controlled_loops.iter().try_fold(
        graph.graph().spec.nodes.len().saturating_mul(3),
        |total, component| {
            usize::try_from(component.limit)
                .ok()
                .and_then(|limit| component.nodes.len().checked_mul(limit))
                .and_then(|steps| steps.checked_mul(3))
                .and_then(|steps| total.checked_add(steps))
        },
    );
    if invalid_limit || estimated.is_none_or(|steps| steps > MAX_SIMULATION_TRANSITIONS) {
        diagnostics.push(Diagnostic::error(
            "GHSIM002_STEP_LIMIT_EXCEEDED",
            "controlled loop exceeds the deterministic simulation step limit",
            "/spec/nodes",
            graph.graph().metadata.id.clone(),
        ));
        status = SimulationStatus::Blocked;
    }
    let transition_limit = estimated
        .unwrap_or(MAX_SIMULATION_TRANSITIONS)
        .min(MAX_SIMULATION_TRANSITIONS);

    while status == SimulationStatus::Running {
        while let Some(node_id) = queue.pop_front() {
            if !pending.remove(&node_id) {
                continue;
            }
            transition(
                &simulation_id,
                &node_id,
                NodeState::Queued,
                &mut states,
                &mut transitions,
                &mut pending_events,
                services,
            )?;
            transition(
                &simulation_id,
                &node_id,
                NodeState::Running,
                &mut states,
                &mut transitions,
                &mut pending_events,
                services,
            )?;
            let outcome = fixtures
                .node_outcomes
                .get(&node_id)
                .cloned()
                .unwrap_or(FixtureOutcome::Success);
            match outcome {
                FixtureOutcome::Success => {
                    transition(
                        &simulation_id,
                        &node_id,
                        NodeState::Succeeded,
                        &mut states,
                        &mut transitions,
                        &mut pending_events,
                        services,
                    )?;
                }
                FixtureOutcome::Failure => {
                    transition(
                        &simulation_id,
                        &node_id,
                        NodeState::Failed,
                        &mut states,
                        &mut transitions,
                        &mut pending_events,
                        services,
                    )?;
                    status = SimulationStatus::Failed;
                    break;
                }
                FixtureOutcome::Unknown => {
                    transition(
                        &simulation_id,
                        &node_id,
                        NodeState::Paused,
                        &mut states,
                        &mut transitions,
                        &mut pending_events,
                        services,
                    )?;
                    diagnostics.push(Diagnostic::error(
                        "GHSIM001_UNKNOWN_CONDITION",
                        "fixture outcome is unknown",
                        format!("/spec/nodes/{node_id}"),
                        graph.graph().metadata.id.clone(),
                    ));
                    status = SimulationStatus::Paused;
                    break;
                }
            }
            if transitions.len() > transition_limit {
                diagnostics.push(Diagnostic::error(
                    "GHSIM002_STEP_LIMIT_EXCEEDED",
                    "simulation exceeded its deterministic transition limit",
                    "/spec/nodes",
                    graph.graph().metadata.id.clone(),
                ));
                status = SimulationStatus::Blocked;
                break;
            }
        }
        if status != SimulationStatus::Running {
            break;
        }

        let mut discovered = Vec::new();
        for node_id in &pending {
            match readiness(graph, node_id, &states, fixtures) {
                Readiness::Ready => discovered.push(node_id.clone()),
                Readiness::Waiting => {}
                Readiness::Unknown(edge_index) => {
                    diagnostics.push(Diagnostic::error(
                        "GHSIM001_UNKNOWN_CONDITION",
                        "condition is outside the deterministic simulation subset",
                        format!("/spec/edges/{edge_index}/condition"),
                        graph.graph().metadata.id.clone(),
                    ));
                    status = SimulationStatus::Paused;
                    break;
                }
                Readiness::Failed => {
                    status = SimulationStatus::Failed;
                    break;
                }
            }
        }
        if status != SimulationStatus::Running {
            break;
        }
        if discovered.is_empty() {
            if let Some(component) = controlled_loops.iter_mut().find(|component| {
                component.completed == 0
                    && component
                        .nodes
                        .iter()
                        .all(|node| !states.contains_key(node))
            }) {
                queue.extend(component.nodes.iter().take(1).cloned());
                component.completed = 1;
                status = SimulationStatus::Running;
            } else if terminals_succeeded(graph, &states) {
                for component in &mut controlled_loops {
                    if component.completed == 0
                        && component
                            .nodes
                            .iter()
                            .all(|node| states.get(node) == Some(&NodeState::Succeeded))
                    {
                        component.completed = 1;
                    }
                }
                if let Some(component) = controlled_loops
                    .iter_mut()
                    .find(|component| component.completed < component.limit)
                {
                    pending.extend(component.nodes.iter().cloned());
                    queue.extend(component.nodes.iter().take(1).cloned());
                    component.completed += 1;
                    status = SimulationStatus::Running;
                } else {
                    status = SimulationStatus::Completed;
                }
            } else {
                status = SimulationStatus::Blocked;
            }
        } else {
            queue.extend(discovered);
        }
    }

    pending_events.push(simulation_event(
        simulation_idempotency_key(
            b"completed",
            &simulation_id,
            None,
            u64::try_from(transitions.len())
                .map_err(|_| SimulationError::Repository(EventRepositoryError::LimitExceeded))?,
        )?,
        services,
        EventKind::SimulationCompleted(SimulationCompleted {
            simulation_id: simulation_id.clone(),
            status: status.clone(),
        }),
    ));
    let next = next_sequence(services)?;
    let request = PreparedAppend::new(
        services.scope.clone(),
        services.stream_id.clone(),
        next,
        pending_events,
        vec![],
        vec![],
    )?;
    let events = services.event_repository.append_atomic(&request)?;
    Ok(SimulationResult {
        simulation_id: simulation_id.to_string(),
        started_at,
        status,
        node_states: states,
        diagnostics,
        events,
        transitions,
    })
}

fn controlled_loops(graph: &GraphVersion) -> Vec<ControlledLoop> {
    let execution = graph.graph();
    let mut components: Vec<ControlledLoop> = Vec::new();
    for (controller, node) in &execution.spec.nodes {
        let Some(limit) = node
            .properties
            .get("loop")
            .and_then(|value| value.get("maxIterations"))
            .and_then(serde_json::Value::as_u64)
            .filter(|limit| *limit > 0)
        else {
            continue;
        };
        let forward = reachable(execution, controller, false);
        let reverse = reachable(execution, controller, true);
        let nodes: BTreeSet<_> = forward.intersection(&reverse).cloned().collect();
        let self_loop = execution
            .spec
            .edges
            .iter()
            .any(|edge| edge.from == *controller && edge.to == *controller);
        if nodes.len() == 1 && !self_loop {
            continue;
        }
        if let Some(existing) = components.iter_mut().find(|item| item.nodes == nodes) {
            existing.limit = existing.limit.max(limit);
        } else {
            components.push(ControlledLoop {
                nodes,
                limit,
                completed: 0,
            });
        }
    }
    components.sort_by(|left, right| left.nodes.iter().next().cmp(&right.nodes.iter().next()));
    components
}

fn reachable(
    graph: &graphhelm_protocols::ExecutionGraph,
    start: &str,
    reverse: bool,
) -> BTreeSet<String> {
    let mut visited = BTreeSet::new();
    let mut stack = vec![start.to_owned()];
    while let Some(node) = stack.pop() {
        if !visited.insert(node.clone()) {
            continue;
        }
        for edge in &graph.spec.edges {
            let next = if reverse && edge.to == node {
                Some(&edge.from)
            } else if !reverse && edge.from == node {
                Some(&edge.to)
            } else {
                None
            };
            if let Some(next) = next {
                stack.push(next.clone());
            }
        }
    }
    visited
}

fn transition(
    simulation_id: &OpaqueId,
    node_id: &str,
    next: NodeState,
    states: &mut BTreeMap<String, NodeState>,
    transitions: &mut Vec<(String, NodeState)>,
    events: &mut Vec<NewEvent>,
    services: &SimulationServices<'_>,
) -> Result<(), SimulationError> {
    let previous = states.insert(node_id.to_owned(), next);
    transitions.push((node_id.to_owned(), next));
    let ordinal = u64::try_from(transitions.len())
        .map_err(|_| SimulationError::Repository(EventRepositoryError::LimitExceeded))?;
    let idempotency_key =
        simulation_idempotency_key(b"transition", simulation_id, Some(node_id), ordinal)?;
    let node_id = OpaqueId::parse(node_id)
        .map_err(|_| SimulationError::Repository(EventRepositoryError::Invalid))?;
    events.push(simulation_event(
        idempotency_key,
        services,
        EventKind::NodeStateChanged(NodeStateChanged {
            simulation_id: simulation_id.clone(),
            node_id,
            previous_state: previous,
            next_state: next,
        }),
    ));
    Ok(())
}

fn simulation_idempotency_key(
    phase: &[u8],
    simulation_id: &OpaqueId,
    node_id: Option<&str>,
    ordinal: u64,
) -> Result<OpaqueId, SimulationError> {
    fn push_part(material: &mut Vec<u8>, part: &[u8]) -> Result<(), SimulationError> {
        let length = u32::try_from(part.len())
            .map_err(|_| SimulationError::Repository(EventRepositoryError::LimitExceeded))?;
        material.extend_from_slice(&length.to_be_bytes());
        material.extend_from_slice(part);
        Ok(())
    }

    let mut material = Vec::with_capacity(
        64_usize
            .saturating_add(simulation_id.as_str().len())
            .saturating_add(node_id.map_or(0, str::len)),
    );
    push_part(&mut material, b"graphhelm-simulation-idempotency-v1")?;
    push_part(&mut material, phase)?;
    push_part(&mut material, simulation_id.as_str().as_bytes())?;
    push_part(&mut material, node_id.unwrap_or("").as_bytes())?;
    material.extend_from_slice(&ordinal.to_be_bytes());
    let digest = raw_content_sha256(&material)
        .map_err(|_| SimulationError::Repository(EventRepositoryError::Invalid))?;
    OpaqueId::parse(format!("ev-{}", digest.as_str()))
        .map_err(|_| SimulationError::Repository(EventRepositoryError::Invalid))
}

fn simulation_event(
    idempotency_key: OpaqueId,
    services: &SimulationServices<'_>,
    kind: EventKind,
) -> NewEvent {
    NewEvent::new(
        idempotency_key,
        services.actor.clone(),
        Sensitivity::Internal,
        kind,
        vec![],
        vec![],
    )
}

fn next_sequence(services: &SimulationServices<'_>) -> Result<u64, SimulationError> {
    services
        .event_repository
        .next_sequence(&services.scope, services.stream_id.as_str())
        .map_err(SimulationError::Repository)
}

enum Readiness {
    Ready,
    Waiting,
    Unknown(usize),
    Failed,
}

fn readiness(
    graph: &GraphVersion,
    node_id: &str,
    states: &BTreeMap<String, NodeState>,
    fixtures: &SimulationFixtures,
) -> Readiness {
    let incoming: Vec<_> = graph
        .graph()
        .spec
        .edges
        .iter()
        .enumerate()
        .filter(|(_, edge)| edge.to == node_id)
        .collect();
    if incoming.is_empty() {
        return Readiness::Waiting;
    }
    for (index, edge) in incoming {
        if states.get(&edge.from) != Some(&NodeState::Succeeded) {
            return Readiness::Waiting;
        }
        match evaluate_condition(edge, states, fixtures) {
            Some(true) => {}
            Some(false) => return Readiness::Waiting,
            None => {
                return match edge.on_unknown {
                    Some(UnknownConditionBehavior::Fail) => Readiness::Failed,
                    Some(UnknownConditionBehavior::Skip) => Readiness::Waiting,
                    Some(UnknownConditionBehavior::Pause | UnknownConditionBehavior::Route)
                    | None => Readiness::Unknown(index),
                };
            }
        }
    }
    Readiness::Ready
}

fn evaluate_condition(
    edge: &GraphEdge,
    states: &BTreeMap<String, NodeState>,
    fixtures: &SimulationFixtures,
) -> Option<bool> {
    let Some(condition) = &edge.condition else {
        return Some(true);
    };
    if let Some(value) = condition.as_bool() {
        return Some(value);
    }
    let text = condition.as_str()?;
    match text {
        "true" => Some(true),
        "false" => Some(false),
        _ => fixtures.conditions.get(text).copied().or_else(|| {
            let (left, expected) = text.split_once(" == ")?;
            let node = left
                .strip_prefix("nodes.")?
                .strip_suffix(".output.passed")?;
            let passed = states.get(node) == Some(&NodeState::Succeeded);
            match expected {
                "true" => Some(passed),
                "false" => Some(!passed),
                _ => None,
            }
        }),
    }
}

fn terminals_succeeded(graph: &GraphVersion, states: &BTreeMap<String, NodeState>) -> bool {
    let terminals = graph
        .graph()
        .spec
        .completion
        .get("terminalNodes")
        .and_then(serde_json::Value::as_array);
    terminals.is_some_and(|items| {
        !items.is_empty()
            && items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .all(|id| states.get(id) == Some(&NodeState::Succeeded))
    })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use chrono::{TimeZone, Utc};
    use graphhelm_events::{ActiveVersion, EventPage};
    use graphhelm_protocols::{
        ActorId, ArtifactId, EvidenceId, ExecutionId, PersistedActorType, ProjectId, WorkspaceId,
    };

    use super::*;

    struct HundredThousandEventRepository(AtomicUsize);
    impl EventRepository for HundredThousandEventRepository {
        fn append_atomic(
            &self,
            _: &PreparedAppend,
        ) -> Result<Vec<EventEnvelope>, EventRepositoryError> {
            unreachable!()
        }
        fn read_stream(
            &self,
            _: &RepositoryScope,
            _: &str,
            _: usize,
            _: Option<&str>,
        ) -> Result<EventPage, EventRepositoryError> {
            panic!("paginated reload used")
        }
        fn read_replay_stream(
            &self,
            _: &RepositoryScope,
            _: &str,
        ) -> Result<Vec<EventEnvelope>, EventRepositoryError> {
            unreachable!()
        }
        fn next_sequence(&self, _: &RepositoryScope, _: &str) -> Result<u64, EventRepositoryError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(100_001)
        }
        fn evidence_exists(
            &self,
            _: &RepositoryScope,
            _: &EvidenceId,
        ) -> Result<bool, EventRepositoryError> {
            unreachable!()
        }
        fn artifact_exists(
            &self,
            _: &RepositoryScope,
            _: &ArtifactId,
        ) -> Result<bool, EventRepositoryError> {
            unreachable!()
        }
        fn active_version(
            &self,
            _: &RepositoryScope,
            _: &str,
        ) -> Result<Option<ActiveVersion>, EventRepositoryError> {
            unreachable!()
        }
        fn committed_events_for_idempotency(
            &self,
            _: &RepositoryScope,
            _: &str,
            _: &OpaqueId,
        ) -> Result<Option<Vec<EventEnvelope>>, EventRepositoryError> {
            unreachable!()
        }
    }
    struct FixedClock;
    impl Clock for FixedClock {
        fn now(&self) -> chrono::DateTime<Utc> {
            Utc.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap()
        }
    }
    struct NoIds;
    impl IdGenerator for NoIds {
        fn next_id(&self, _: &'static str) -> String {
            unreachable!()
        }
    }

    #[test]
    fn hundred_thousand_event_sequence_uses_one_direct_repository_load() {
        let repository = HundredThousandEventRepository(AtomicUsize::new(0));
        let services = SimulationServices {
            event_repository: &repository,
            scope: RepositoryScope::new(
                WorkspaceId::parse("workspace-1").unwrap(),
                ProjectId::parse("project-1").unwrap(),
                Some(ExecutionId::parse("execution-1").unwrap()),
            ),
            stream_id: OpaqueId::parse("stream-1").unwrap(),
            actor: PersistedActor::new(
                PersistedActorType::System,
                ActorId::parse("system-1").unwrap(),
            ),
            clock: &FixedClock,
            ids: &NoIds,
        };
        assert_eq!(next_sequence(&services).unwrap(), 100_001);
        assert_eq!(repository.0.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn derived_idempotency_key_is_stable_and_binds_every_transition_component() {
        let first_simulation = OpaqueId::parse("simulation-a").unwrap();
        let second_simulation = OpaqueId::parse("simulation-b").unwrap();
        let first = simulation_idempotency_key(b"transition", &first_simulation, Some("node-a"), 1)
            .unwrap();
        assert_eq!(
            first,
            simulation_idempotency_key(b"transition", &first_simulation, Some("node-a"), 1)
                .unwrap()
        );
        assert_ne!(
            first,
            simulation_idempotency_key(b"transition", &second_simulation, Some("node-a"), 1)
                .unwrap()
        );
        assert_ne!(
            first,
            simulation_idempotency_key(b"transition", &first_simulation, Some("node-b"), 1)
                .unwrap()
        );
        assert_ne!(
            first,
            simulation_idempotency_key(b"transition", &first_simulation, Some("node-a"), 2)
                .unwrap()
        );
    }
}
