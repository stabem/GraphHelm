use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{TimeZone, Utc};
use graphhelm_events::{EventStore, EventStoreError};
use graphhelm_governor::{ApplyServices, apply_draft};
use graphhelm_graph::GraphVersion;
use graphhelm_protocols::{
    Actor, ActorType, Clock, DraftOperation, EventEnvelope, EventKind, GraphDraft, IdGenerator,
    ManualOverride, NewEvent, NodeType, SemanticHash, WaiverScope,
};

#[derive(Default)]
struct RecordingStore(Mutex<Vec<EventEnvelope>>);

impl RecordingStore {
    fn events(&self) -> Vec<EventEnvelope> {
        self.0.lock().unwrap().clone()
    }
}

impl EventStore for RecordingStore {
    fn append_batch(
        &self,
        stream_id: &str,
        expected_next_sequence: u64,
        events: &[NewEvent],
    ) -> Result<Vec<EventEnvelope>, EventStoreError> {
        let mut stored = self.0.lock().unwrap();
        let actual = stored.len() as u64 + 1;
        if actual != expected_next_sequence {
            return Err(EventStoreError::SequenceConflict {
                expected: expected_next_sequence,
                actual,
            });
        }
        let written: Vec<_> = events
            .iter()
            .enumerate()
            .map(|(offset, event)| EventEnvelope {
                id: format!("event-{}", expected_next_sequence + offset as u64),
                stream_id: stream_id.into(),
                sequence: expected_next_sequence + offset as u64,
                occurred_at: Utc.with_ymd_and_hms(2026, 8, 8, 12, 0, 0).unwrap(),
                idempotency_key: event.idempotency_key.clone(),
                kind: event.kind.clone(),
            })
            .collect();
        stored.extend(written.clone());
        Ok(written)
    }

    fn read_stream(&self, stream_id: &str) -> Result<Vec<EventEnvelope>, EventStoreError> {
        Ok(self
            .events()
            .into_iter()
            .filter(|event| event.stream_id == stream_id)
            .collect())
    }
}

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

fn base() -> GraphVersion {
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

fn services<'a>(
    store: &'a RecordingStore,
    clock: &'a FixedClock,
    ids: &'a SequenceIds,
) -> ApplyServices<'a> {
    ApplyServices {
        event_store: store,
        stream_id: "exec-feature",
        actor: Actor::new(ActorType::Owner, "owner-local"),
        clock,
        ids,
    }
}

fn draft(base: &GraphVersion, operations: Vec<DraftOperation>) -> GraphDraft {
    GraphDraft {
        id: "draft-1".into(),
        expected_version: base.number(),
        expected_hash: base.content_hash().clone(),
        operations,
        manual_override: None,
    }
}

#[test]
fn valid_draft_publishes_n_plus_one_and_preserves_n() {
    let base = base();
    let before = serde_json::to_value(base.graph()).unwrap();
    let mut node = base.graph().spec.nodes["docs"].clone();
    node.name = "Archive evidence".into();
    let draft = draft(
        &base,
        vec![DraftOperation::AddNode {
            id: "archive".into(),
            node,
        }],
    );
    let store = RecordingStore::default();
    let result = apply_draft(
        &base,
        &draft,
        &services(&store, &FixedClock, &SequenceIds::default()),
    )
    .unwrap();

    assert_eq!(result.version.number(), base.number() + 1);
    assert_eq!(serde_json::to_value(base.graph()).unwrap(), before);
    assert!(result.version.graph().spec.nodes.contains_key("archive"));
    assert!(result.events.iter().any(|event| {
        matches!(
            event.kind,
            EventKind::GraphVersionPublished(_) | EventKind::DraftApplied(_)
        )
    }));
}

#[test]
fn stale_hash_fails_without_publishing_successor() {
    let base = base();
    let mut draft = draft(&base, Vec::new());
    draft.expected_hash = SemanticHash::new("sha256:stale");
    let store = RecordingStore::default();
    let error = apply_draft(
        &base,
        &draft,
        &services(&store, &FixedClock, &SequenceIds::default()),
    )
    .unwrap_err();

    assert_eq!(error.code(), "GHD002_STALE_HASH");
    assert!(
        store
            .events()
            .iter()
            .any(|event| matches!(event.kind, EventKind::DraftRejected(_)))
    );
    assert!(!store.events().iter().any(|event| {
        matches!(
            event.kind,
            EventKind::GraphVersionPublished(_) | EventKind::DraftApplied(_)
        )
    }));
}

#[test]
fn bypass_creates_schema_valid_waiver_and_auditable_status() {
    let mut raw = base().graph().clone();
    raw.spec.nodes.get_mut("review").unwrap().node_type = NodeType::Gate;
    let base = GraphVersion::publish(
        raw,
        None,
        Actor::new(ActorType::Owner, "owner-local"),
        Utc.with_ymd_and_hms(2026, 8, 8, 11, 0, 0).unwrap(),
    )
    .unwrap();
    let review_edges: Vec<_> = base
        .graph()
        .spec
        .edges
        .iter()
        .filter(|edge| edge.from == "review" || edge.to == "review")
        .map(|edge| DraftOperation::RemoveEdge {
            id: edge.id.clone(),
        })
        .collect();
    let mut operations = review_edges;
    operations.push(DraftOperation::RemoveNode {
        id: "review".into(),
    });
    let mut draft = draft(&base, operations);
    draft.manual_override = Some(ManualOverride {
        actor: Actor::new(ActorType::Owner, "owner-local"),
        reason: "accepted review bypass".into(),
        waived_requirements: vec!["review".into()],
        acknowledged_risks: vec!["unreviewed change".into()],
        scope: WaiverScope::Execution,
    });
    let store = RecordingStore::default();
    let result = apply_draft(
        &base,
        &draft,
        &services(&store, &FixedClock, &SequenceIds::default()),
    )
    .unwrap();

    assert_eq!(result.policy_report.result_status, "completed_with_waivers");
    assert_eq!(result.waivers.len(), 1);
    let value = serde_json::to_value(&result.waivers[0]).unwrap();
    assert!(graphhelm_schema::validate_waiver(&value, "generated").is_empty());
}

#[test]
fn structurally_impossible_deploy_is_rejected_despite_owner_override() {
    let base = base();
    let mut draft = draft(
        &base,
        vec![DraftOperation::PatchNode {
            id: "implement".into(),
            patch: serde_json::json!({"type": "deploy", "targetRef": null}),
        }],
    );
    draft.manual_override = Some(ManualOverride {
        actor: Actor::new(ActorType::Owner, "owner-local"),
        reason: "attempted emergency deploy".into(),
        waived_requirements: vec!["deploy_target".into()],
        acknowledged_risks: vec!["unknown destination".into()],
        scope: WaiverScope::Execution,
    });
    let store = RecordingStore::default();
    let error = apply_draft(
        &base,
        &draft,
        &services(&store, &FixedClock, &SequenceIds::default()),
    )
    .unwrap_err();

    assert_eq!(error.code(), "GHP001_STRUCTURAL_IMPOSSIBILITY");
    assert!(
        store
            .events()
            .iter()
            .any(|event| matches!(event.kind, EventKind::DraftRejected(_)))
    );
    assert!(!store.events().iter().any(|event| {
        matches!(
            event.kind,
            EventKind::PolicyWaiverCreated(_)
                | EventKind::GraphVersionPublished(_)
                | EventKind::DraftApplied(_)
        )
    }));
}

#[test]
fn invalid_operation_is_atomic_and_only_records_rejection() {
    let base = base();
    let before = serde_json::to_value(base.graph()).unwrap();
    let draft = draft(
        &base,
        vec![DraftOperation::RemoveNode {
            id: "does-not-exist".into(),
        }],
    );
    let store = RecordingStore::default();
    let error = apply_draft(
        &base,
        &draft,
        &services(&store, &FixedClock, &SequenceIds::default()),
    )
    .unwrap_err();

    assert_eq!(error.code(), "GHD003_OPERATION_INVALID");
    assert_eq!(serde_json::to_value(base.graph()).unwrap(), before);
    assert_eq!(
        store
            .events()
            .iter()
            .filter(|event| matches!(event.kind, EventKind::DraftRejected(_)))
            .count(),
        1
    );
}

#[test]
fn rejected_draft_events_do_not_persist_operation_secrets() {
    let base = base();
    let draft = draft(
        &base,
        vec![
            DraftOperation::PatchNode {
                id: "implement".into(),
                patch: serde_json::json!({"apiKey": "TOP-SECRET-DO-NOT-PERSIST"}),
            },
            DraftOperation::RemoveNode {
                id: "does-not-exist".into(),
            },
        ],
    );
    let store = RecordingStore::default();
    apply_draft(
        &base,
        &draft,
        &services(&store, &FixedClock, &SequenceIds::default()),
    )
    .unwrap_err();

    assert!(
        !serde_json::to_string(&store.events())
            .unwrap()
            .contains("TOP-SECRET")
    );
}

#[test]
fn stale_version_has_distinct_code_and_no_successor() {
    let base = base();
    let mut draft = draft(&base, Vec::new());
    draft.expected_version -= 1;
    let store = RecordingStore::default();
    let error = apply_draft(
        &base,
        &draft,
        &services(&store, &FixedClock, &SequenceIds::default()),
    )
    .unwrap_err();
    assert_eq!(error.code(), "GHD001_STALE_VERSION");
    assert!(
        !store
            .events()
            .iter()
            .any(|event| matches!(event.kind, EventKind::GraphVersionPublished(_)))
    );
}

#[test]
fn second_sibling_draft_cannot_publish_competing_n_plus_one() {
    let base = base();
    let store = RecordingStore::default();
    let first = draft(
        &base,
        vec![DraftOperation::AddNode {
            id: "archive-a".into(),
            node: base.graph().spec.nodes["docs"].clone(),
        }],
    );
    apply_draft(
        &base,
        &first,
        &services(&store, &FixedClock, &SequenceIds::default()),
    )
    .unwrap();

    let mut sibling = draft(
        &base,
        vec![DraftOperation::AddNode {
            id: "archive-b".into(),
            node: base.graph().spec.nodes["docs"].clone(),
        }],
    );
    sibling.id = "draft-2".into();
    let error = apply_draft(
        &base,
        &sibling,
        &services(&store, &FixedClock, &SequenceIds::default()),
    )
    .unwrap_err();

    assert_eq!(error.code(), "GHD001_STALE_VERSION");
    assert_eq!(
        store
            .events()
            .iter()
            .filter(|event| matches!(event.kind, EventKind::GraphVersionPublished(_)))
            .count(),
        1
    );
}

#[test]
fn draft_claimed_owner_cannot_override_authoritative_agent_actor() {
    let mut raw = base().graph().clone();
    raw.spec.nodes.get_mut("review").unwrap().node_type = NodeType::Gate;
    let base = GraphVersion::publish(
        raw,
        None,
        Actor::new(ActorType::Owner, "owner-local"),
        Utc.with_ymd_and_hms(2026, 8, 8, 11, 0, 0).unwrap(),
    )
    .unwrap();
    let mut operations: Vec<_> = base
        .graph()
        .spec
        .edges
        .iter()
        .filter(|edge| edge.from == "review" || edge.to == "review")
        .map(|edge| DraftOperation::RemoveEdge {
            id: edge.id.clone(),
        })
        .collect();
    operations.push(DraftOperation::RemoveNode {
        id: "review".into(),
    });
    let mut draft = draft(&base, operations);
    draft.manual_override = Some(ManualOverride {
        actor: Actor::new(ActorType::Owner, "owner-local"),
        reason: "forged proposal authority".into(),
        waived_requirements: vec!["review".into()],
        acknowledged_risks: vec!["unreviewed change".into()],
        scope: WaiverScope::Execution,
    });
    let store = RecordingStore::default();
    let services = ApplyServices {
        event_store: &store,
        stream_id: "exec-feature",
        actor: Actor::new(ActorType::Agent, "agent-untrusted"),
        clock: &FixedClock,
        ids: &SequenceIds::default(),
    };
    let error = apply_draft(&base, &draft, &services).unwrap_err();

    assert_eq!(error.code(), "GHP002_OVERRIDE_REQUIRED");
    assert!(
        !store
            .events()
            .iter()
            .any(|event| matches!(event.kind, EventKind::PolicyWaiverCreated(_)))
    );
}
