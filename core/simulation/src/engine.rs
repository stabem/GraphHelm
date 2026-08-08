use std::collections::{BTreeMap, BTreeSet, VecDeque};

use chrono::{DateTime, Utc};
use graphhelm_events::{EventStore, EventStoreError};
use graphhelm_graph::GraphVersion;
use graphhelm_protocols::{
    Clock, Diagnostic, EventEnvelope, EventKind, FixtureOutcome, GraphEdge, IdGenerator, NewEvent,
    NodeState, NodeStateChanged, SimulationCompleted, SimulationStarted, SimulationStatus,
    UnknownConditionBehavior,
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
    pub event_store: &'a dyn EventStore,
    pub stream_id: &'a str,
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
    EventStore(#[from] EventStoreError),
}

impl SimulationError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::EventStore(error) => error.code(),
        }
    }
}

/// Simulates state transitions without invoking tools, models, networks, or graph payloads.
pub fn simulate(
    graph: &GraphVersion,
    fixtures: &SimulationFixtures,
    services: &SimulationServices<'_>,
) -> Result<SimulationResult, SimulationError> {
    let simulation_id = services.ids.next_id("simulation");
    let started_at = services.clock.now();
    let mut pending: BTreeSet<_> = graph.graph().spec.nodes.keys().cloned().collect();
    let mut queue: VecDeque<_> = graph.graph().spec.entrypoints.iter().cloned().collect();
    let mut states = BTreeMap::new();
    let mut transitions = Vec::new();
    let mut diagnostics = Vec::new();
    let mut status = SimulationStatus::Running;
    let mut pending_events = vec![NewEvent {
        idempotency_key: format!("{simulation_id}:started"),
        kind: EventKind::SimulationStarted(SimulationStarted {}),
    }];
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
            );
            transition(
                &simulation_id,
                &node_id,
                NodeState::Running,
                &mut states,
                &mut transitions,
                &mut pending_events,
            );
            let outcome = fixtures
                .node_outcomes
                .get(&node_id)
                .cloned()
                .unwrap_or(FixtureOutcome::Success);
            match outcome {
                FixtureOutcome::Success => transition(
                    &simulation_id,
                    &node_id,
                    NodeState::Succeeded,
                    &mut states,
                    &mut transitions,
                    &mut pending_events,
                ),
                FixtureOutcome::Failure => {
                    transition(
                        &simulation_id,
                        &node_id,
                        NodeState::Failed,
                        &mut states,
                        &mut transitions,
                        &mut pending_events,
                    );
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
                    );
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

    pending_events.push(NewEvent {
        idempotency_key: format!("{simulation_id}:completed"),
        kind: EventKind::SimulationCompleted(SimulationCompleted {
            status: status.clone(),
        }),
    });
    let next = services
        .event_store
        .read_stream(services.stream_id)?
        .last()
        .map_or(1, |event| event.sequence + 1);
    let events = services
        .event_store
        .append_batch(services.stream_id, next, &pending_events)?;
    Ok(SimulationResult {
        simulation_id,
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
    simulation_id: &str,
    node_id: &str,
    next: NodeState,
    states: &mut BTreeMap<String, NodeState>,
    transitions: &mut Vec<(String, NodeState)>,
    events: &mut Vec<NewEvent>,
) {
    let previous = states.insert(node_id.to_owned(), next.clone());
    transitions.push((node_id.to_owned(), next.clone()));
    events.push(NewEvent {
        idempotency_key: format!("{simulation_id}:{node_id}:{}", transitions.len()),
        kind: EventKind::NodeStateChanged(NodeStateChanged {
            node_id: node_id.to_owned(),
            from: previous,
            to: next,
        }),
    });
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
