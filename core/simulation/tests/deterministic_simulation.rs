use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{TimeZone, Utc};
use graphhelm_events::{LocalEventRepository, replay};
use graphhelm_graph::GraphVersion;
use graphhelm_protocols::{
    Actor, ActorId, ActorType, Clock, ExecutionId, IdGenerator, OpaqueId, PersistedActor,
    PersistedActorType, ProjectId, RepositoryScope, SimulationStatus, WorkspaceId,
};
use graphhelm_simulation::{SimulationFixtures, SimulationServices, simulate};

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 8, 12, 0, 0).unwrap()
    }
}

#[derive(Default)]
struct SequenceIds(AtomicU64);

static IDS: SequenceIds = SequenceIds(AtomicU64::new(0));

impl IdGenerator for SequenceIds {
    fn next_id(&self, prefix: &'static str) -> String {
        format!("{prefix}-{}", self.0.fetch_add(1, Ordering::SeqCst) + 1)
    }
}

fn version() -> GraphVersion {
    let graph = graphhelm_schema::load_graph(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/graphs/software-feature.yaml"),
    )
    .unwrap()
    .graph;
    GraphVersion::publish(
        graph,
        None,
        Actor::new(ActorType::Owner, "owner-local"),
        Utc.with_ymd_and_hms(2026, 8, 8, 11, 0, 0).unwrap(),
    )
    .unwrap()
}

fn store(path: &std::path::Path) -> LocalEventRepository {
    LocalEventRepository::open(path, Arc::new(FixedClock), Arc::new(SequenceIds::default()))
        .unwrap()
}

fn scope(stream: &str) -> RepositoryScope {
    RepositoryScope::new(
        WorkspaceId::parse("workspace-test").unwrap(),
        ProjectId::parse("project-test").unwrap(),
        Some(ExecutionId::parse(stream).unwrap()),
    )
}

fn services<'a>(store: &'a LocalEventRepository, stream: &str) -> SimulationServices<'a> {
    SimulationServices {
        event_repository: store,
        scope: scope(stream),
        stream_id: OpaqueId::parse(stream).unwrap(),
        actor: PersistedActor::new(
            PersistedActorType::System,
            ActorId::parse("system-simulation").unwrap(),
        ),
        clock: &FixedClock,
        ids: &IDS,
    }
}

#[test]
fn same_graph_and_fixtures_emit_same_ordered_transition_kinds() {
    let first_dir = tempfile::tempdir().unwrap();
    let second_dir = tempfile::tempdir().unwrap();
    let first_store = store(&first_dir.path().join("events.jsonl"));
    let second_store = store(&second_dir.path().join("events.jsonl"));
    let fixtures = SimulationFixtures::default();
    let first = simulate(&version(), &fixtures, &services(&first_store, "exec-1")).unwrap();
    let second = simulate(&version(), &fixtures, &services(&second_store, "exec-1")).unwrap();

    assert_eq!(first.transition_trace(), second.transition_trace());
    assert_eq!(first.status, SimulationStatus::Completed);
}

#[test]
fn maximum_wire_ids_produce_bounded_distinct_idempotency_keys() {
    struct MaximumId;
    impl IdGenerator for MaximumId {
        fn next_id(&self, _: &'static str) -> String {
            "s".repeat(128)
        }
    }

    let base = version();
    let mut graph = base.graph().clone();
    let node_id = "n".repeat(128);
    let node = graph.spec.nodes["plan"].clone();
    graph.spec.nodes.clear();
    graph.spec.nodes.insert(node_id.clone(), node);
    graph.spec.entrypoints = vec![node_id.clone()];
    graph.spec.edges.clear();
    graph.spec.completion = serde_json::json!({"terminalNodes": [node_id]});
    let graph = GraphVersion::publish(
        graph,
        None,
        Actor::new(ActorType::Owner, "owner-local"),
        Utc.with_ymd_and_hms(2026, 8, 8, 11, 0, 0).unwrap(),
    )
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let repository = store(&directory.path().join("max-wire-ids"));
    let services = SimulationServices {
        event_repository: &repository,
        scope: scope("exec-max-wire-ids"),
        stream_id: OpaqueId::parse("exec-max-wire-ids").unwrap(),
        actor: PersistedActor::new(
            PersistedActorType::System,
            ActorId::parse("system-simulation").unwrap(),
        ),
        clock: &FixedClock,
        ids: &MaximumId,
    };

    let result = simulate(&graph, &SimulationFixtures::default(), &services).unwrap();
    let keys = result
        .events
        .iter()
        .map(|event| event.idempotency_key.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(keys.len(), result.events.len());
    assert!(keys.iter().all(|key| key.len() <= 128));
    assert!(
        keys.iter()
            .all(|key| !key.contains('n') && !key.contains('s'))
    );
}

#[test]
fn unknown_condition_pauses_instead_of_guessing() {
    let base = version();
    let mut graph = base.graph().clone();
    graph.spec.edges[0].condition = Some(serde_json::json!("provider.result == maybe"));
    let graph = GraphVersion::publish(
        graph,
        None,
        Actor::new(ActorType::Owner, "owner-local"),
        Utc.with_ymd_and_hms(2026, 8, 8, 11, 0, 0).unwrap(),
    )
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let loop_store = store(&directory.path().join("events.jsonl"));
    let result = simulate(
        &graph,
        &SimulationFixtures::default(),
        &services(&loop_store, "exec-unknown"),
    )
    .unwrap();

    assert_eq!(result.status, SimulationStatus::Paused);
    assert_eq!(result.diagnostics[0].code, "GHSIM001_UNKNOWN_CONDITION");
}

#[test]
fn fresh_store_replay_matches_simulation_projection() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("events.jsonl");
    let first = store(&path);
    let result = simulate(
        &version(),
        &SimulationFixtures::default(),
        &services(&first, "exec-replay"),
    )
    .unwrap();
    drop(first);
    let events = store(&path)
        .read_stream(&scope("exec-replay"), "exec-replay", 1000, None)
        .unwrap()
        .events;
    let projection = replay(&scope("exec-replay"), "exec-replay", &events).unwrap();

    assert_eq!(projection.node_states, result.node_states);
    assert_eq!(projection.simulation_status, Some(result.status));
}

#[test]
fn controlled_cycle_runs_exactly_to_its_iteration_bound() {
    let base = version();
    let mut graph = base.graph().clone();
    graph
        .spec
        .nodes
        .get_mut("plan")
        .unwrap()
        .properties
        .insert("loop".into(), serde_json::json!({"maxIterations": 2}));
    graph.spec.edges.push(graphhelm_protocols::GraphEdge {
        id: "docs-back-to-plan".into(),
        from: "docs".into(),
        to: "plan".into(),
        edge_type: graphhelm_protocols::EdgeType::Control,
        payload_schema: None,
        condition: None,
        on_false: None,
        on_unknown: None,
        bindings: Default::default(),
        priority: None,
    });
    let graph = GraphVersion::publish(
        graph,
        None,
        Actor::new(ActorType::Owner, "owner-local"),
        Utc.with_ymd_and_hms(2026, 8, 8, 11, 0, 0).unwrap(),
    )
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let independent_store = store(&directory.path().join("events.jsonl"));
    let result = simulate(
        &graph,
        &SimulationFixtures::default(),
        &services(&independent_store, "exec-loop"),
    )
    .unwrap();

    assert_eq!(result.status, SimulationStatus::Completed);
    assert_eq!(
        result
            .transition_trace()
            .iter()
            .filter(|(node, state)| {
                node == "docs" && *state == graphhelm_protocols::NodeState::Succeeded
            })
            .count(),
        2
    );
}

#[test]
fn independent_loops_keep_separate_bounds_and_huge_limits_block() {
    let base = version();
    let mut graph = base.graph().clone();
    for (id, limit) in [("loop-a", 2_u64), ("loop-b", 3_u64)] {
        let mut node = graph.spec.nodes["docs"].clone();
        node.name = id.into();
        node.properties
            .insert("loop".into(), serde_json::json!({"maxIterations": limit}));
        graph.spec.nodes.insert(id.into(), node);
        graph.spec.entrypoints.push(id.into());
        graph.spec.edges.push(graphhelm_protocols::GraphEdge {
            id: format!("{id}-self"),
            from: id.into(),
            to: id.into(),
            edge_type: graphhelm_protocols::EdgeType::Control,
            payload_schema: None,
            condition: None,
            on_false: None,
            on_unknown: None,
            bindings: Default::default(),
            priority: None,
        });
    }
    graph.spec.completion = serde_json::json!({"terminalNodes": ["docs", "loop-a", "loop-b"]});
    let published = GraphVersion::publish(
        graph.clone(),
        None,
        Actor::new(ActorType::Owner, "owner-local"),
        Utc.with_ymd_and_hms(2026, 8, 8, 11, 0, 0).unwrap(),
    )
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let independent_loop_store = store(&directory.path().join("events.jsonl"));
    let result = simulate(
        &published,
        &SimulationFixtures::default(),
        &services(&independent_loop_store, "exec-independent-loops"),
    )
    .unwrap();
    for (id, expected) in [("loop-a", 2), ("loop-b", 3)] {
        assert_eq!(
            result
                .transition_trace()
                .iter()
                .filter(|(node, state)| node == id
                    && *state == graphhelm_protocols::NodeState::Succeeded)
                .count(),
            expected
        );
    }

    graph
        .spec
        .nodes
        .get_mut("loop-a")
        .unwrap()
        .properties
        .insert(
            "loop".into(),
            serde_json::json!({"maxIterations": u64::MAX}),
        );
    let huge = GraphVersion::publish(
        graph,
        None,
        Actor::new(ActorType::Owner, "owner-local"),
        Utc.with_ymd_and_hms(2026, 8, 8, 11, 0, 0).unwrap(),
    )
    .unwrap();
    let huge_store = store(&directory.path().join("huge-events.jsonl"));
    let blocked = simulate(
        &huge,
        &SimulationFixtures::default(),
        &services(&huge_store, "exec-huge-loop"),
    )
    .unwrap();
    assert_eq!(blocked.status, SimulationStatus::Blocked);
    assert_eq!(blocked.diagnostics[0].code, "GHSIM002_STEP_LIMIT_EXCEEDED");
}

#[test]
fn loop_metadata_on_an_acyclic_node_does_not_repeat_it() {
    let base = version();
    let mut graph = base.graph().clone();
    graph
        .spec
        .nodes
        .get_mut("docs")
        .unwrap()
        .properties
        .insert("loop".into(), serde_json::json!({"maxIterations": 5}));
    let graph = GraphVersion::publish(
        graph,
        None,
        Actor::new(ActorType::Owner, "owner-local"),
        Utc.with_ymd_and_hms(2026, 8, 8, 11, 0, 0).unwrap(),
    )
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let event_store = store(&directory.path().join("events.jsonl"));
    let result = simulate(
        &graph,
        &SimulationFixtures::default(),
        &services(&event_store, "exec-acyclic-loop-metadata"),
    )
    .unwrap();
    assert_eq!(
        result
            .transition_trace()
            .iter()
            .filter(|(node, state)| {
                node == "docs" && *state == graphhelm_protocols::NodeState::Succeeded
            })
            .count(),
        1
    );
}
