use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    thread,
};

use chrono::{TimeZone, Utc};
use graphhelm_events::{
    AsyncEventRepository, AuthenticatedCheckpoint, EventPage, EventRepositoryError,
    EvidenceAvailability, IntegrityReport, LocalEventRepository, PreparedAppend,
    ProjectionGeneration, ProjectionRebuildRequest, ProjectionRebuilder, ProjectionRepository,
    ProjectionWatermark, ReadStart, ReadStreamRequest, ReplayError, RepositoryFuture, StreamHead,
    VerifyRangeRequest, compute_event_hash,
};
use graphhelm_protocols::{
    ActorId, Clock, EventKind, ExecutionId, IdGenerator, NewEvent, NodeState, NodeStateChanged,
    OpaqueId, PersistedActor, PersistedActorType, ProjectId, RepositoryScope, Sensitivity,
    SimulationCompleted, SimulationStarted, SimulationStatus, WireHash, WorkspaceId,
};

fn scope() -> RepositoryScope {
    RepositoryScope::new(
        WorkspaceId::parse("workspace-1").unwrap(),
        ProjectId::parse("project-1").unwrap(),
        Some(ExecutionId::parse("execution-1").unwrap()),
    )
}

struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap()
    }
}

#[derive(Default)]
struct Ids(AtomicU64);
impl IdGenerator for Ids {
    fn next_id(&self, prefix: &'static str) -> String {
        format!("{prefix}-{}", self.0.fetch_add(1, Ordering::SeqCst) + 1)
    }
}

fn event(key: &str, kind: EventKind) -> NewEvent {
    NewEvent::new(
        OpaqueId::parse(key).unwrap(),
        PersistedActor::new(
            PersistedActorType::System,
            ActorId::parse("system-1").unwrap(),
        ),
        Sensitivity::Internal,
        kind,
        vec![],
        vec![],
    )
}

fn block_on<F: Future>(future: F) -> F::Output {
    struct ThreadWaker(thread::Thread);
    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match Pin::as_mut(&mut future).poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => thread::park(),
        }
    }
}

struct StaticEvents(Vec<graphhelm_protocols::EventEnvelope>);
impl AsyncEventRepository for StaticEvents {
    fn append_atomic<'a>(
        &'a self,
        _: PreparedAppend,
    ) -> RepositoryFuture<'a, Result<Vec<graphhelm_protocols::EventEnvelope>, EventRepositoryError>>
    {
        Box::pin(async { Err(EventRepositoryError::Storage) })
    }
    fn read_stream<'a>(
        &'a self,
        request: ReadStreamRequest,
    ) -> RepositoryFuture<'a, Result<EventPage, EventRepositoryError>> {
        Box::pin(async move {
            let after = match request.start() {
                ReadStart::Beginning => 0,
                ReadStart::After { sequence, .. } => *sequence,
                ReadStart::Cursor(_) => return Err(EventRepositoryError::Invalid),
            };
            let events = self
                .0
                .iter()
                .filter(|event| event.sequence > after)
                .take(request.limit() as usize)
                .cloned()
                .collect();
            Ok(EventPage {
                events,
                next_cursor: None,
                head: head(&self.0),
            })
        })
    }
    fn stream_head<'a>(
        &'a self,
        _: RepositoryScope,
        _: String,
    ) -> RepositoryFuture<'a, Result<Option<StreamHead>, EventRepositoryError>> {
        Box::pin(async move { Ok(head(&self.0)) })
    }
    fn verify_range<'a>(
        &'a self,
        _: VerifyRangeRequest,
    ) -> RepositoryFuture<'a, Result<IntegrityReport, EventRepositoryError>> {
        Box::pin(async { Err(EventRepositoryError::Storage) })
    }
    fn append_checkpoint<'a>(
        &'a self,
        _: AuthenticatedCheckpoint,
    ) -> RepositoryFuture<'a, Result<AuthenticatedCheckpoint, EventRepositoryError>> {
        Box::pin(async { Err(EventRepositoryError::Storage) })
    }
    fn latest_checkpoint<'a>(
        &'a self,
        _: RepositoryScope,
        _: String,
    ) -> RepositoryFuture<'a, Result<Option<AuthenticatedCheckpoint>, EventRepositoryError>> {
        Box::pin(async { Err(EventRepositoryError::Storage) })
    }
}

struct AdvancingEvents {
    events: Vec<graphhelm_protocols::EventEnvelope>,
    head_calls: AtomicU64,
}
impl AsyncEventRepository for AdvancingEvents {
    fn append_atomic<'a>(
        &'a self,
        _: PreparedAppend,
    ) -> RepositoryFuture<'a, Result<Vec<graphhelm_protocols::EventEnvelope>, EventRepositoryError>>
    {
        Box::pin(async { Err(EventRepositoryError::Storage) })
    }
    fn read_stream<'a>(
        &'a self,
        request: ReadStreamRequest,
    ) -> RepositoryFuture<'a, Result<EventPage, EventRepositoryError>> {
        Box::pin(async move {
            let visible = if self.head_calls.load(Ordering::SeqCst) <= 1 {
                1
            } else {
                self.events.len()
            };
            let after = match request.start() {
                ReadStart::Beginning => 0,
                ReadStart::After { sequence, .. } => *sequence,
                ReadStart::Cursor(_) => return Err(EventRepositoryError::Invalid),
            };
            let events = self.events[..visible]
                .iter()
                .filter(|event| event.sequence > after)
                .take(request.limit() as usize)
                .cloned()
                .collect();
            Ok(EventPage {
                events,
                next_cursor: None,
                head: head(&self.events[..visible]),
            })
        })
    }
    fn stream_head<'a>(
        &'a self,
        _: RepositoryScope,
        _: String,
    ) -> RepositoryFuture<'a, Result<Option<StreamHead>, EventRepositoryError>> {
        Box::pin(async move {
            let call = self.head_calls.fetch_add(1, Ordering::SeqCst);
            let visible = if call == 0 { 1 } else { self.events.len() };
            Ok(head(&self.events[..visible]))
        })
    }
    fn verify_range<'a>(
        &'a self,
        _: VerifyRangeRequest,
    ) -> RepositoryFuture<'a, Result<IntegrityReport, EventRepositoryError>> {
        Box::pin(async { Err(EventRepositoryError::Storage) })
    }
    fn append_checkpoint<'a>(
        &'a self,
        _: AuthenticatedCheckpoint,
    ) -> RepositoryFuture<'a, Result<AuthenticatedCheckpoint, EventRepositoryError>> {
        Box::pin(async { Err(EventRepositoryError::Storage) })
    }
    fn latest_checkpoint<'a>(
        &'a self,
        _: RepositoryScope,
        _: String,
    ) -> RepositoryFuture<'a, Result<Option<AuthenticatedCheckpoint>, EventRepositoryError>> {
        Box::pin(async { Err(EventRepositoryError::Storage) })
    }
}

fn head(events: &[graphhelm_protocols::EventEnvelope]) -> Option<StreamHead> {
    events.last().map(|last| StreamHead {
        next_sequence: last.sequence + 1,
        last_event_hash: last.event_hash.clone(),
    })
}

#[derive(Default)]
struct MemoryProjections {
    saved: Mutex<Option<ProjectionGeneration>>,
    active: Mutex<Option<ProjectionGeneration>>,
    fail_swap: bool,
    save_calls: AtomicU64,
}
impl ProjectionRepository for MemoryProjections {
    fn load_generation<'a>(
        &'a self,
        _: &'a ProjectionRebuildRequest,
    ) -> RepositoryFuture<'a, Result<Option<ProjectionGeneration>, EventRepositoryError>> {
        Box::pin(async move { Ok(self.saved.lock().unwrap().clone()) })
    }
    fn save_generation<'a>(
        &'a self,
        value: ProjectionGeneration,
    ) -> RepositoryFuture<'a, Result<(), EventRepositoryError>> {
        Box::pin(async move {
            self.save_calls.fetch_add(1, Ordering::SeqCst);
            *self.saved.lock().unwrap() = Some(value);
            Ok(())
        })
    }
    fn load_active<'a>(
        &'a self,
        _: RepositoryScope,
        _: String,
        _: String,
        _: u32,
    ) -> RepositoryFuture<'a, Result<Option<ProjectionGeneration>, EventRepositoryError>> {
        Box::pin(async move { Ok(self.active.lock().unwrap().clone()) })
    }
    fn swap_active<'a>(
        &'a self,
        value: ProjectionGeneration,
        _: Option<StreamHead>,
    ) -> RepositoryFuture<'a, Result<(), EventRepositoryError>> {
        Box::pin(async move {
            if self.fail_swap {
                Err(EventRepositoryError::SequenceConflict)
            } else if self.saved.lock().unwrap().is_none() {
                Err(EventRepositoryError::Integrity)
            } else {
                *self.active.lock().unwrap() = Some(value);
                Ok(())
            }
        })
    }
}

#[test]
fn empty_generation_binds_scope_stream_name_version_and_generation() {
    let generation =
        ProjectionGeneration::new(scope(), "stream-1".into(), "execution".into(), 1, 7).unwrap();

    let watermark = generation.watermark();
    assert_eq!(watermark.scope(), &scope());
    assert_eq!(watermark.stream_id(), "stream-1");
    assert_eq!(watermark.projection_name(), "execution");
    assert_eq!(watermark.projection_version(), 1);
    assert_eq!(watermark.generation(), 7);
    assert_eq!(watermark.last_sequence(), 0);
    assert!(watermark.last_event_hash().is_none());
}

#[test]
fn watermarks_reject_old_projection_formats_and_zero_generations() {
    assert_eq!(
        ProjectionWatermark::new(
            scope(),
            "stream-1".into(),
            "execution".into(),
            0,
            1,
            0,
            None
        ),
        Err(ReplayError::Corrupt)
    );
    assert_eq!(
        ProjectionWatermark::new(
            scope(),
            "stream-1".into(),
            "execution".into(),
            1,
            0,
            0,
            None
        ),
        Err(ReplayError::Corrupt)
    );
    assert_eq!(
        ProjectionWatermark::new(
            scope(),
            "stream-1".into(),
            "execution".into(),
            u32::MAX,
            1,
            0,
            None
        ),
        Err(ReplayError::LimitExceeded)
    );
    assert_eq!(
        ProjectionWatermark::new(
            scope(),
            "stream-1".into(),
            "execution".into(),
            1,
            9_007_199_254_740_992,
            0,
            None
        ),
        Err(ReplayError::LimitExceeded)
    );
    assert!(serde_json::from_str::<EvidenceAvailability>("\"forged_success\"").is_err());
}

#[test]
fn generation_resumes_at_page_boundaries_without_plaintext() {
    let directory = tempfile::tempdir().unwrap();
    let repository = LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    )
    .unwrap();
    let simulation_id = OpaqueId::parse("simulation-1").unwrap();
    let events = repository
        .append_atomic(
            &PreparedAppend::new(
                scope(),
                OpaqueId::parse("stream-1").unwrap(),
                1,
                vec![
                    event(
                        "event-1",
                        EventKind::SimulationStarted(SimulationStarted {
                            simulation_id: simulation_id.clone(),
                            graph_version: 1,
                            graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64)))
                                .unwrap(),
                        }),
                    ),
                    event(
                        "event-2",
                        EventKind::NodeStateChanged(NodeStateChanged {
                            simulation_id: simulation_id.clone(),
                            node_id: OpaqueId::parse("node-1").unwrap(),
                            previous_state: None,
                            next_state: NodeState::Succeeded,
                        }),
                    ),
                    event(
                        "event-3",
                        EventKind::SimulationCompleted(SimulationCompleted {
                            simulation_id,
                            status: SimulationStatus::Completed,
                        }),
                    ),
                ],
                vec![],
                vec![],
            )
            .unwrap(),
        )
        .unwrap();
    let mut generation =
        ProjectionGeneration::new(scope(), "stream-1".into(), "execution".into(), 1, 9).unwrap();
    generation.apply_page(&events[..1]).unwrap();
    let encoded = serde_json::to_vec(&generation).unwrap();
    assert!(!String::from_utf8_lossy(&encoded).contains("plaintext"));
    let mut resumed: ProjectionGeneration = serde_json::from_slice(&encoded).unwrap();
    resumed.apply_page(&events[1..]).unwrap();
    assert_eq!(resumed.watermark().last_sequence(), 3);
    assert_eq!(
        resumed.projection().simulation_status,
        Some(SimulationStatus::Completed)
    );
    assert_eq!(
        resumed.projection().node_states.get("node-1"),
        Some(&NodeState::Succeeded)
    );
}

#[test]
fn resumed_generation_rejects_a_replayed_page() {
    let directory = tempfile::tempdir().unwrap();
    let repository = LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    )
    .unwrap();
    let events = repository
        .append_atomic(
            &PreparedAppend::new(
                scope(),
                OpaqueId::parse("stream-1").unwrap(),
                1,
                vec![event(
                    "event-1",
                    EventKind::SimulationStarted(SimulationStarted {
                        simulation_id: OpaqueId::parse("simulation-1").unwrap(),
                        graph_version: 1,
                        graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
                    }),
                )],
                vec![],
                vec![],
            )
            .unwrap(),
        )
        .unwrap();
    let mut generation =
        ProjectionGeneration::new(scope(), "stream-1".into(), "execution".into(), 1, 1).unwrap();
    generation.apply_page(&events).unwrap();
    assert_eq!(generation.apply_page(&events), Err(ReplayError::Corrupt));
}

#[test]
fn rebuild_requests_bound_page_size_and_generation_identity() {
    assert!(
        ProjectionRebuildRequest::new(scope(), "stream-1".into(), "execution".into(), 1, 4, 100)
            .is_ok()
    );
    assert_eq!(
        ProjectionRebuildRequest::new(scope(), "stream-1".into(), "execution".into(), 1, 4, 0),
        Err(ReplayError::LimitExceeded),
    );
    assert_eq!(
        ProjectionRebuildRequest::new(scope(), "stream-1".into(), "execution".into(), 1, 0, 100),
        Err(ReplayError::Corrupt),
    );
}

#[test]
fn rebuild_pages_to_the_exact_source_head_and_swaps_once_complete() {
    let directory = tempfile::tempdir().unwrap();
    let local = LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    )
    .unwrap();
    let simulation_id = OpaqueId::parse("simulation-1").unwrap();
    let events = local
        .append_atomic(
            &PreparedAppend::new(
                scope(),
                OpaqueId::parse("stream-1").unwrap(),
                1,
                vec![
                    event(
                        "event-1",
                        EventKind::SimulationStarted(SimulationStarted {
                            simulation_id: simulation_id.clone(),
                            graph_version: 1,
                            graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64)))
                                .unwrap(),
                        }),
                    ),
                    event(
                        "event-2",
                        EventKind::SimulationCompleted(SimulationCompleted {
                            simulation_id,
                            status: SimulationStatus::Completed,
                        }),
                    ),
                ],
                vec![],
                vec![],
            )
            .unwrap(),
        )
        .unwrap();
    let projections = Arc::new(MemoryProjections::default());
    let result = block_on(
        ProjectionRebuilder::new(Arc::new(StaticEvents(events)), projections.clone()).rebuild(
            ProjectionRebuildRequest::new(scope(), "stream-1".into(), "execution".into(), 1, 2, 1)
                .unwrap(),
        ),
    )
    .unwrap();
    assert_eq!(result.watermark().last_sequence(), 2);
    assert_eq!(
        projections
            .active
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .watermark(),
        result.watermark()
    );
}

#[test]
fn swap_failure_preserves_the_old_active_generation() {
    let old =
        ProjectionGeneration::new(scope(), "stream-1".into(), "execution".into(), 1, 1).unwrap();
    let projections = Arc::new(MemoryProjections {
        saved: Mutex::new(None),
        active: Mutex::new(Some(old.clone())),
        fail_swap: true,
        save_calls: AtomicU64::new(0),
    });
    let result = block_on(
        ProjectionRebuilder::new(Arc::new(StaticEvents(vec![])), projections.clone()).rebuild(
            ProjectionRebuildRequest::new(scope(), "stream-1".into(), "execution".into(), 1, 2, 1)
                .unwrap(),
        ),
    );
    assert!(matches!(
        result,
        Err(EventRepositoryError::SequenceConflict)
    ));
    assert_eq!(
        projections
            .active
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .watermark(),
        old.watermark()
    );
}

#[test]
fn empty_stream_generation_is_persisted_before_activation() {
    let projections = Arc::new(MemoryProjections::default());
    let result = block_on(
        ProjectionRebuilder::new(Arc::new(StaticEvents(vec![])), projections.clone()).rebuild(
            ProjectionRebuildRequest::new(scope(), "stream-1".into(), "execution".into(), 1, 2, 1)
                .unwrap(),
        ),
    );
    assert!(result.is_ok());
    assert!(projections.saved.lock().unwrap().is_some());
    assert!(projections.active.lock().unwrap().is_some());
}

#[test]
fn rebuild_catches_a_source_head_that_advances_before_swap() {
    let directory = tempfile::tempdir().unwrap();
    let local = LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    )
    .unwrap();
    let simulation_id = OpaqueId::parse("simulation-1").unwrap();
    let events = local
        .append_atomic(
            &PreparedAppend::new(
                scope(),
                OpaqueId::parse("stream-1").unwrap(),
                1,
                vec![
                    event(
                        "event-1",
                        EventKind::SimulationStarted(SimulationStarted {
                            simulation_id: simulation_id.clone(),
                            graph_version: 1,
                            graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64)))
                                .unwrap(),
                        }),
                    ),
                    event(
                        "event-2",
                        EventKind::NodeStateChanged(NodeStateChanged {
                            simulation_id: simulation_id.clone(),
                            node_id: OpaqueId::parse("node-1").unwrap(),
                            previous_state: None,
                            next_state: NodeState::Succeeded,
                        }),
                    ),
                    event(
                        "event-3",
                        EventKind::SimulationCompleted(SimulationCompleted {
                            simulation_id,
                            status: SimulationStatus::Completed,
                        }),
                    ),
                ],
                vec![],
                vec![],
            )
            .unwrap(),
        )
        .unwrap();
    let projections = Arc::new(MemoryProjections::default());
    let result = block_on(
        ProjectionRebuilder::new(
            Arc::new(AdvancingEvents {
                events,
                head_calls: AtomicU64::new(0),
            }),
            projections.clone(),
        )
        .rebuild(
            ProjectionRebuildRequest::new(scope(), "stream-1".into(), "execution".into(), 1, 2, 1)
                .unwrap(),
        ),
    )
    .unwrap();
    assert_eq!(result.watermark().last_sequence(), 3);
    assert_eq!(projections.save_calls.load(Ordering::SeqCst), 2);
}

#[test]
fn every_nonpublication_event_kind_has_a_safe_generation_handler() {
    let raw = "b".repeat(64);
    let hash = format!("sha256:{}", "a".repeat(64));
    let diagnostic = serde_json::json!({"code":"GHP001_SAFE","severity":"error","path":"/topology","component":"governor"});
    let variants = vec![
        (
            serde_json::json!({"type":"graph_imported","data":{"sourceSha256":raw,"sourceKind":"graph_document"}}),
            false,
        ),
        (
            serde_json::json!({"type":"graph_validation_failed","data":{"diagnostics":[diagnostic.clone()]}}),
            false,
        ),
        (
            serde_json::json!({"type":"draft_proposed","data":{"draftId":"draft-1","expectedVersion":1,"expectedHash":hash,"operationCount":1}}),
            false,
        ),
        (
            serde_json::json!({"type":"draft_rejected","data":{"draftId":"draft-1","reasonCode":"policy_failed","diagnostics":[diagnostic]}}),
            false,
        ),
        (
            serde_json::json!({"type":"draft_applied","data":{"draftId":"draft-1","graphVersion":2,"graphHash":hash}}),
            false,
        ),
        (
            serde_json::json!({"type":"policy_obligation_evaluated","data":{"draftId":"draft-1","requirementId":"review","status":"satisfied","evidenceIds":[],"reasonCode":"satisfied","overrideable":true}}),
            false,
        ),
        (
            serde_json::json!({"type":"policy_waiver_created","data":{"waiver":{"id":"waiver-1","requirement":"review","executionId":"execution-1","graphVersion":1,"actor":"owner-1","acknowledgedRisks":["accepted-risk"],"scope":"execution","createdAt":"2026-08-09T00:00:00Z","expiresAt":null}}}),
            false,
        ),
        (
            serde_json::json!({"type":"simulation_started","data":{"simulationId":"simulation-1","graphVersion":1,"graphHash":hash}}),
            false,
        ),
        (
            serde_json::json!({"type":"node_state_changed","data":{"simulationId":"simulation-1","nodeId":"start","previousState":null,"nextState":"running"}}),
            false,
        ),
        (
            serde_json::json!({"type":"simulation_completed","data":{"simulationId":"simulation-1","status":"completed"}}),
            false,
        ),
        (
            serde_json::json!({"type":"integrity_checkpoint_created","data":{"streamId":"stream-1","sequence":1,"eventHash":hash,"repositoryFormat":"1.0.0","authenticationTag":{"keyId":"key-1","algorithm":"hmac-sha256","tagSha256":raw}}}),
            true,
        ),
        (
            serde_json::json!({"type":"evidence_erasure_requested","data":{"evidenceScope":{"workspaceId":"workspace-1","projectId":"project-1","executionId":"execution-1"},"operationId":"operation-1","evidenceId":"evidence-1","keyHandleId":"handle-1","retentionPolicyId":"retention-1","retentionPolicyVersion":"1.0.0","authority":"authority-1","reasonCode":"expired","priorState":"available","state":"erasure_pending","requestedAt":"2026-08-09T00:00:00Z"}}),
            true,
        ),
        (
            serde_json::json!({"type":"evidence_erasure_completed","data":{"evidenceScope":{"workspaceId":"workspace-1","projectId":"project-1","executionId":"execution-1"},"operationId":"operation-1","evidenceId":"evidence-1","keyHandleId":"handle-1","retentionPolicyId":"retention-1","retentionPolicyVersion":"1.0.0","authority":"authority-1","reasonCode":"expired","ciphertextSha256":raw,"providerReceiptId":"receipt-1","providerEpoch":1,"priorState":"erasure_pending","state":"erased","requestedAt":"2026-08-09T00:00:00Z","completedAt":"2026-08-09T00:01:00Z"}}),
            true,
        ),
        (
            serde_json::json!({"type":"evidence_ciphertext_deleted","data":{"evidenceScope":{"workspaceId":"workspace-1","projectId":"project-1","executionId":"execution-1"},"operationId":"operation-1","evidenceId":"evidence-1","ciphertextSha256":raw,"deletedAt":"2026-08-09T00:02:00Z"}}),
            true,
        ),
        (
            serde_json::json!({"type":"evidence_legal_hold_changed","data":{"evidenceScope":{"workspaceId":"workspace-1","projectId":"project-1","executionId":"execution-1"},"holdId":"hold-1","evidenceId":"evidence-1","authority":"authority-1","reasonCode":"investigation","state":"placed","changedAt":"2026-08-09T00:03:00Z"}}),
            true,
        ),
    ];
    assert_eq!(variants.len() + 1, 16); // publication has dedicated lineage/bijection tests.
    for (kind, project_level) in variants {
        let event_scope = if project_level {
            RepositoryScope::new(
                WorkspaceId::parse("workspace-1").unwrap(),
                ProjectId::parse("project-1").unwrap(),
                None,
            )
        } else {
            RepositoryScope::new(
                WorkspaceId::parse("workspace-1").unwrap(),
                ProjectId::parse("project-1").unwrap(),
                Some(ExecutionId::parse("execution-1").unwrap()),
            )
        };
        let mut envelope: graphhelm_protocols::EventEnvelope = serde_json::from_value(serde_json::json!({
            "schemaVersion":"1.0.0","eventId":"event-1","scope":event_scope,"streamId":"stream-1","sequence":1,
            "occurredAt":"2026-08-09T00:00:00Z","idempotencyKey":"request-1","actor":{"type":"system","id":"system-1"},
            "sensitivity":"internal","kind":kind,"evidenceRefs":[],"artifactRefs":[],
            "previousHash":"sha256:35c8ab0717bef1684ad07efcf3bedd4648c778a2c944cbd2c7e6a4802e2237b3",
            "eventHash":format!("sha256:{}", "0".repeat(64))
        })).unwrap();
        envelope.event_hash = graphhelm_protocols::EventHash::parse(
            compute_event_hash(&envelope, envelope.previous_hash.as_str()).unwrap(),
        )
        .unwrap();
        let mut generation =
            ProjectionGeneration::new(event_scope, "stream-1".into(), "execution".into(), 1, 1)
                .unwrap();
        generation.apply_page(&[envelope]).unwrap();
    }
}

#[test]
fn generation_rejects_an_independently_corrupted_envelope() {
    let directory = tempfile::tempdir().unwrap();
    let repository = LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    )
    .unwrap();
    let mut events = repository
        .append_atomic(
            &PreparedAppend::new(
                scope(),
                OpaqueId::parse("stream-1").unwrap(),
                1,
                vec![event(
                    "event-1",
                    EventKind::SimulationStarted(SimulationStarted {
                        simulation_id: OpaqueId::parse("simulation-1").unwrap(),
                        graph_version: 1,
                        graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
                    }),
                )],
                vec![],
                vec![],
            )
            .unwrap(),
        )
        .unwrap();
    events[0].event_hash =
        graphhelm_protocols::EventHash::parse(format!("sha256:{}", "f".repeat(64))).unwrap();
    let mut generation =
        ProjectionGeneration::new(scope(), "stream-1".into(), "execution".into(), 1, 1).unwrap();
    assert_eq!(generation.apply_page(&events), Err(ReplayError::Corrupt));
}
