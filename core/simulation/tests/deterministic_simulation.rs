use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{TimeZone, Utc};
use graphhelm_events::{EventStore, JsonlEventStore, replay};
use graphhelm_graph::GraphVersion;
use graphhelm_protocols::{Actor, ActorType, Clock, IdGenerator, SimulationStatus};
use graphhelm_simulation::{SimulationFixtures, SimulationServices, simulate};

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 8, 12, 0, 0).unwrap()
    }
}

#[derive(Default)]
struct SequenceIds(AtomicU64);

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

fn store(path: &std::path::Path) -> JsonlEventStore {
    JsonlEventStore::new(path, Arc::new(FixedClock), Arc::new(SequenceIds::default()))
}

#[test]
fn same_graph_and_fixtures_emit_same_ordered_transition_kinds() {
    let first_dir = tempfile::tempdir().unwrap();
    let second_dir = tempfile::tempdir().unwrap();
    let first_store = store(&first_dir.path().join("events.jsonl"));
    let second_store = store(&second_dir.path().join("events.jsonl"));
    let fixtures = SimulationFixtures::default();
    let first = simulate(
        &version(),
        &fixtures,
        &SimulationServices {
            event_store: &first_store,
            stream_id: "exec-1",
            clock: &FixedClock,
            ids: &SequenceIds::default(),
        },
    )
    .unwrap();
    let second = simulate(
        &version(),
        &fixtures,
        &SimulationServices {
            event_store: &second_store,
            stream_id: "exec-1",
            clock: &FixedClock,
            ids: &SequenceIds::default(),
        },
    )
    .unwrap();

    assert_eq!(first.transition_trace(), second.transition_trace());
    assert_eq!(first.status, SimulationStatus::Completed);
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
        &SimulationServices {
            event_store: &loop_store,
            stream_id: "exec-unknown",
            clock: &FixedClock,
            ids: &SequenceIds::default(),
        },
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
        &SimulationServices {
            event_store: &first,
            stream_id: "exec-replay",
            clock: &FixedClock,
            ids: &SequenceIds::default(),
        },
    )
    .unwrap();
    drop(first);
    let events = store(&path).read_stream("exec-replay").unwrap();
    let projection = replay(&events).unwrap();

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
        &SimulationServices {
            event_store: &independent_store,
            stream_id: "exec-loop",
            clock: &FixedClock,
            ids: &SequenceIds::default(),
        },
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
        &SimulationServices {
            event_store: &independent_loop_store,
            stream_id: "exec-independent-loops",
            clock: &FixedClock,
            ids: &SequenceIds::default(),
        },
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
        &SimulationServices {
            event_store: &huge_store,
            stream_id: "exec-huge-loop",
            clock: &FixedClock,
            ids: &SequenceIds::default(),
        },
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
        &SimulationServices {
            event_store: &event_store,
            stream_id: "exec-acyclic-loop-metadata",
            clock: &FixedClock,
            ids: &SequenceIds::default(),
        },
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
