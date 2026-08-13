use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use chrono::{TimeZone, Utc};
use graphhelm_events::{LocalEventRepository, PreparedAppend, replay};
use graphhelm_protocols::{
    ActorId, Clock, EventKind, ExecutionId, IdGenerator, NewEvent, NodeState, NodeStateChanged,
    OpaqueId, PersistedActor, PersistedActorType, ProjectId, RepositoryScope, Sensitivity,
    SimulationCompleted, SimulationStarted, SimulationStatus, WireHash, WorkspaceId,
};
use sha2::{Digest, Sha256};

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
        WorkspaceId::parse("workspace-test").unwrap(),
        ProjectId::parse("project-test").unwrap(),
        Some(ExecutionId::parse("execution-test").unwrap()),
    )
}
fn actor() -> PersistedActor {
    PersistedActor::new(
        PersistedActorType::System,
        ActorId::parse("system-test").unwrap(),
    )
}
fn event(key: &str, kind: EventKind) -> NewEvent {
    NewEvent::new(
        OpaqueId::parse(key).unwrap(),
        actor(),
        Sensitivity::Internal,
        kind,
        vec![],
        vec![],
    )
}

fn canonical(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(canonical).collect())
        }
        serde_json::Value::Object(values) => {
            let mut entries = values.into_iter().collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            serde_json::Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, canonical(value)))
                    .collect(),
            )
        }
        scalar => scalar,
    }
}

fn rehash(event: &mut graphhelm_protocols::EventEnvelope) {
    let mut value = serde_json::to_value(&*event).unwrap();
    value.as_object_mut().unwrap().remove("eventHash");
    let mut bytes = b"graphhelm-event-hash-v1\0".to_vec();
    bytes.extend_from_slice(event.previous_hash.as_str().as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&serde_json::to_vec(&canonical(value)).unwrap());
    event.event_hash = graphhelm_protocols::EventHash::parse(format!(
        "sha256:{}",
        hex::encode(Sha256::digest(bytes))
    ))
    .unwrap();
}

#[test]
fn replay_reconstructs_node_state_and_simulation_status_from_verified_events() {
    let directory = tempfile::tempdir().unwrap();
    let repository = LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    )
    .unwrap();
    let simulation_id = OpaqueId::parse("simulation-1").unwrap();
    let request = PreparedAppend::new(
        scope(),
        OpaqueId::parse("stream-test").unwrap(),
        1,
        vec![
            event(
                "started",
                EventKind::SimulationStarted(SimulationStarted {
                    simulation_id: simulation_id.clone(),
                    graph_version: 1,
                    graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
                }),
            ),
            event(
                "changed",
                EventKind::NodeStateChanged(NodeStateChanged {
                    simulation_id: simulation_id.clone(),
                    node_id: OpaqueId::parse("map-repository").unwrap(),
                    previous_state: None,
                    next_state: NodeState::Succeeded,
                }),
            ),
            event(
                "completed",
                EventKind::SimulationCompleted(SimulationCompleted {
                    simulation_id,
                    status: SimulationStatus::Completed,
                }),
            ),
        ],
        vec![],
        vec![],
    )
    .unwrap();
    let events = repository.append_atomic(&request).unwrap();
    let projection = replay(&scope(), "stream-test", &events).unwrap();
    assert_eq!(
        projection.node_states["map-repository"],
        NodeState::Succeeded
    );
    assert_eq!(
        projection.simulation_status,
        Some(SimulationStatus::Completed)
    );
}

#[test]
fn replay_rejects_non_contiguous_sequences() {
    let directory = tempfile::tempdir().unwrap();
    let repository = LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    )
    .unwrap();
    let request = PreparedAppend::new(
        scope(),
        OpaqueId::parse("stream-test").unwrap(),
        1,
        vec![
            event(
                "started",
                EventKind::SimulationStarted(SimulationStarted {
                    simulation_id: OpaqueId::parse("simulation-1").unwrap(),
                    graph_version: 1,
                    graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
                }),
            ),
            event(
                "completed",
                EventKind::SimulationCompleted(SimulationCompleted {
                    simulation_id: OpaqueId::parse("simulation-1").unwrap(),
                    status: SimulationStatus::Completed,
                }),
            ),
        ],
        vec![],
        vec![],
    )
    .unwrap();
    let mut events = repository.append_atomic(&request).unwrap();
    events[0].sequence = 2;
    assert_eq!(
        replay(&scope(), "stream-test", &events).unwrap_err().code(),
        "GHE005_INTEGRITY_FAILURE"
    );
}

#[test]
fn public_replay_rejects_rehashed_scope_and_schema_tampering() {
    let directory = tempfile::tempdir().unwrap();
    let repository = LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    )
    .unwrap();
    let request = PreparedAppend::new(
        scope(),
        OpaqueId::parse("stream-test").unwrap(),
        1,
        vec![
            event(
                "started",
                EventKind::SimulationStarted(SimulationStarted {
                    simulation_id: OpaqueId::parse("simulation-1").unwrap(),
                    graph_version: 1,
                    graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
                }),
            ),
            event(
                "completed",
                EventKind::SimulationCompleted(SimulationCompleted {
                    simulation_id: OpaqueId::parse("simulation-1").unwrap(),
                    status: SimulationStatus::Completed,
                }),
            ),
        ],
        vec![],
        vec![],
    )
    .unwrap();
    let events = repository.append_atomic(&request).unwrap();

    let mut wrong_hash = events.clone();
    wrong_hash[1].event_hash =
        graphhelm_protocols::EventHash::parse(format!("sha256:{}", "f".repeat(64))).unwrap();
    assert!(replay(&scope(), "stream-test", &wrong_hash).is_err());

    let mut wrong_scope = events.clone();
    wrong_scope[0].scope = RepositoryScope::new(
        WorkspaceId::parse("workspace-test").unwrap(),
        ProjectId::parse("project-test").unwrap(),
        Some(ExecutionId::parse("execution-test").unwrap()),
    );
    wrong_scope[1].scope = RepositoryScope::new(
        WorkspaceId::parse("workspace-other").unwrap(),
        ProjectId::parse("project-test").unwrap(),
        Some(ExecutionId::parse("execution-test").unwrap()),
    );
    rehash(&mut wrong_scope[1]);
    assert!(replay(&scope(), "stream-test", &wrong_scope).is_err());

    let mut invalid_schema = events;
    if let EventKind::SimulationStarted(payload) = &mut invalid_schema[0].kind {
        payload.graph_version = 0;
    } else {
        panic!("fixture kind changed");
    }
    rehash(&mut invalid_schema[0]);
    invalid_schema[1].previous_hash = invalid_schema[0].event_hash.clone();
    rehash(&mut invalid_schema[1]);
    assert!(replay(&scope(), "stream-test", &invalid_schema).is_err());
}
