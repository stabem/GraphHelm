use std::path::Path;

use chrono::{TimeZone, Utc};
use graphhelm_graph::{GraphError, GraphVersion};
use graphhelm_protocols::{Actor, ActorType, GraphVersionRef};

fn load() -> graphhelm_protocols::ExecutionGraph {
    graphhelm_schema::load_graph(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/graphs/software-feature.yaml"),
    )
    .unwrap()
    .graph
}

fn actor() -> Actor {
    Actor::new(ActorType::Owner, "owner-local")
}

#[test]
fn publishing_successor_does_not_mutate_predecessor() {
    let first = GraphVersion::publish(
        load(),
        None,
        actor(),
        Utc.with_ymd_and_hms(2026, 8, 8, 12, 0, 0).unwrap(),
    )
    .unwrap();
    let before = serde_json::to_value(first.graph()).unwrap();
    let mut successor_graph = first.graph().clone();
    successor_graph.metadata.version += 1;
    let second = GraphVersion::publish(
        successor_graph,
        Some(GraphVersionRef {
            number: first.number(),
            content_hash: first.content_hash().clone(),
        }),
        actor(),
        Utc.with_ymd_and_hms(2026, 8, 8, 12, 1, 0).unwrap(),
    )
    .unwrap();

    assert_eq!(serde_json::to_value(first.graph()).unwrap(), before);
    assert_eq!(second.number(), first.number() + 1);
    assert_eq!(
        second.predecessor().unwrap().content_hash,
        *first.content_hash()
    );
}

#[test]
fn record_round_trip_recomputes_and_checks_semantic_hash() {
    let version = GraphVersion::publish(
        load(),
        None,
        actor(),
        Utc.with_ymd_and_hms(2026, 8, 8, 12, 0, 0).unwrap(),
    )
    .unwrap();
    let record = version.to_record();
    let rebuilt = GraphVersion::from_record(record.clone()).unwrap();
    assert_eq!(rebuilt.to_record(), record);

    let mut tampered = record;
    tampered.content_hash = graphhelm_protocols::SemanticHash::new("sha256:bad");
    assert!(GraphVersion::from_record(tampered).is_err());
}

#[test]
fn maximum_predecessor_version_is_rejected_without_overflow() {
    let mut graph = load();
    graph.metadata.version = u64::MAX;

    let error = GraphVersion::publish(
        graph,
        Some(GraphVersionRef {
            number: u64::MAX,
            content_hash: graphhelm_protocols::SemanticHash::new(format!(
                "sha256:{}",
                "0".repeat(64)
            )),
        }),
        actor(),
        Utc.with_ymd_and_hms(2026, 8, 8, 12, 0, 0).unwrap(),
    )
    .unwrap_err();

    assert_eq!(error, GraphError::InvalidPredecessor);
}
