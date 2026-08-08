use std::collections::BTreeMap;

use chrono::{TimeZone, Utc};
use graphhelm_events::replay;
use graphhelm_graph::GraphVersion;
use graphhelm_protocols::{
    Actor, ActorType, EventEnvelope, EventKind, GraphVersionPublished, NodeState, NodeStateChanged,
    SimulationCompleted, SimulationStatus,
};

fn envelope(sequence: u64, key: &str, kind: EventKind) -> EventEnvelope {
    EventEnvelope {
        id: format!("event-{sequence}"),
        stream_id: "exec-1".into(),
        sequence,
        occurred_at: Utc
            .with_ymd_and_hms(2026, 8, 8, 12, 0, sequence as u32)
            .unwrap(),
        idempotency_key: key.into(),
        kind,
    }
}

#[test]
fn replay_reconstructs_published_graph_node_states_and_simulation_status() {
    let graph = graphhelm_schema::load_graph(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/graphs/software-feature.yaml"),
    )
    .unwrap()
    .graph;
    let version = GraphVersion::publish(
        graph,
        None,
        Actor::new(ActorType::Owner, "owner-local"),
        Utc.with_ymd_and_hms(2026, 8, 8, 12, 0, 0).unwrap(),
    )
    .unwrap();
    let events = vec![
        envelope(
            1,
            "publish-1",
            EventKind::GraphVersionPublished(Box::new(GraphVersionPublished {
                version: version.to_record(),
            })),
        ),
        envelope(
            2,
            "node-1",
            EventKind::NodeStateChanged(NodeStateChanged {
                node_id: "map_repository".into(),
                from: None,
                to: NodeState::Succeeded,
            }),
        ),
        envelope(
            3,
            "complete-1",
            EventKind::SimulationCompleted(SimulationCompleted {
                status: SimulationStatus::Completed,
            }),
        ),
    ];

    let projection = replay(&events).unwrap();
    assert_eq!(projection.current_graph.unwrap().graph.metadata.version, 1);
    assert_eq!(
        projection.node_states,
        BTreeMap::from([("map_repository".into(), NodeState::Succeeded)])
    );
    assert_eq!(
        projection.simulation_status,
        Some(SimulationStatus::Completed)
    );
}

#[test]
fn replay_rejects_non_contiguous_sequences() {
    let events = vec![envelope(2, "late", EventKind::simulation_started())];
    assert_eq!(replay(&events).unwrap_err().code(), "GHE002_CORRUPT_BATCH");
}
