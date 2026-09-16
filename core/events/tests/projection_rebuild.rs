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
        // #569 SLICE A — the seven variants whose fold arm reads no prior projection state, so a
        // fixture is one event and nothing else. Three of them (`gate_verdict`, `reuse_decision`,
        // `sweep_performed`) are literal no-ops in the fold, and covering a no-op is not busywork:
        // this test asserts the generation handler is SAFE, and "safe" is exactly the claim a
        // no-op makes. The remaining classes (a positioned event, and one needing a predecessor
        // arranged) are deliberately NOT here — their fixtures cost an arrangement, and mixing the
        // two costs in one change is how a cheap slice grows a hard half nobody reviewed.
        (
            serde_json::json!({"type":"gate_verdict","data":{"executionId":"execution-1","nodeId":"node-1","gateId":"gate-1","passed":true,"findings":[]}}),
            false,
        ),
        (
            serde_json::json!({"type":"gate_certified","data":{"executionId":"execution-1","gateId":"gate-1","suiteDigest":hash,"specimens":1}}),
            false,
        ),
        (
            serde_json::json!({"type":"reuse_decision","data":{"executionId":"execution-1","plane":"tool_broker","decision":"miss","keyComponents":["tool_version","canonical_input"],"keyDigest":hash,"provenanceErased":false}}),
            false,
        ),
        (
            serde_json::json!({"type":"sweep_performed","data":{"executionId":"execution-1","asOf":"2026-08-09T00:04:00Z","caller":"tick"}}),
            false,
        ),
        (
            serde_json::json!({"type":"wake_lease","data":{"executionId":"execution-1","sessionId":"session-1","cursor":1,"rendezvousId":"rendezvous-1"}}),
            false,
        ),
        (
            serde_json::json!({"type":"execution_form_amended","data":{"executionId":"execution-1","computedAtSequence":1,"nodeTimeoutSeconds":{},"observedSilenceSeconds":{}}}),
            false,
        ),
        (
            // PROJECT-LEVEL, and the payload says so: this is the only one of the seven whose
            // data carries no `executionId`. Filed under an execution-scoped generation it fails
            // `Corrupt` before the fold arm is ever reached -- the arm itself cannot produce that
            // error, it only counts and records (`projection.rs:1224-1235`, LimitExceeded is its
            // only failure). Measured, after guessing wrong: the scope level is part of the
            // fixture, and the payload is what declares it.
            serde_json::json!({"type":"memory_admission_refused","data":{"code":"scope_mismatch","local":"content","bytes":1}}),
            true,
        ),
        (
            // Same shape as `memory_admission_refused` above: project-level, no prelude needed --
            // the fold arm reads no prior projection state, only checks the keyed-map bound and
            // inserts/updates one entry (`projection.rs`'s `MemoryPublicationTransitioned` arm),
            // `LimitExceeded` its only failure.
            serde_json::json!({"type":"memory_publication_transitioned","data":{"recordId":"record-1","transition":"publish","resultingState":"published"}}),
            true,
        ),
        (
            // Same class, TWO records instead of one: project-level, no prelude -- the fold arm
            // reads no prior projection state for either record, it creates or updates each
            // (`projection.rs`'s `MemoryRecordSuperseded` arm), bounded the same way.
            serde_json::json!({"type":"memory_record_superseded","data":{"predecessorId":"record-1","successorId":"record-2","reason":"contradicted","predecessorNewSemanticState":"contradicted"}}),
            true,
        ),
    ];
    // No length literal (#190): the old `assert_eq!(variants.len() + 1, 16)` compared this
    // vec's own length to a hand-written number, so it could never fail regardless of how many
    // real EventKind variants exist or how many of them this vec actually lists — the same
    // shape already fixed in `conformance.rs` and `persistence_wire.rs` (#160/#167).
    //
    // This test does NOT gain 28 new fixtures here. Inventing schema-correct JSON for events
    // this file has never exercised risks wrong-but-plausible coverage — worse than the gap it
    // would claim to close. Instead: the gap is PINNED, by name, same shape as #478's tripwire.
    // A new EventKind variant, or the accidental loss of an existing fixture, changes this
    // test's own comparison and turns it red — nothing can silently drift again.

    // #569 C1 — FIXTURES THAT NEED A PRELUDE, and the reason is one line in the fold.
    //
    // 18 of the 28 pinned variants open their arm with
    // `projection.execution_id.as_deref() != Some(payload.execution_id.as_str()) => Corrupt`, and
    // `projection.execution_id` is set by exactly ONE event: `ExecutionStarted`
    // (`core/events/src/projection.rs:1321-1325`). A fresh generation holds `None`, so every one of
    // those 18 refuses as a single event no matter how correct its payload is.
    //
    // That is NOT what my own classification on #569 said. It grouped by "does the arm read prior
    // projection state", which put `execution_paused` (no read) far from `completion_cleared`
    // (deep read) — while the property that actually decides whether a fixture is one event or
    // several is the execution-id guard, which both of them have. The axis was wrong, and slice A
    // passed only because none of its seven carry that guard.
    //
    // A page, not an event, is therefore the unit here. `apply_page` already takes a slice; what
    // was missing was a fixture shape that uses it.
    let pages: Vec<Vec<serde_json::Value>> = vec![
        // The clearance registry: register then revoke, both against a known execution.
        vec![
            serde_json::json!({"type":"execution_started","data":{"executionId":"execution-1","graphVersion":3,"graphHash":hash,"mode":"supervised"}}),
            serde_json::json!({"type":"clearance_identity_registered","data":{"executionId":"execution-1","identity":"auditor-a","keyFingerprint":hash}}),
            serde_json::json!({"type":"clearance_identity_revoked","data":{"executionId":"execution-1","identity":"auditor-a"}}),
        ],
        // The customs chain, cleared. Each step exists because the next one refuses without it:
        // the wait is opened by a `waiting_input` outcome (`projection.rs:1382`), the claim only
        // enters `open_claims` when it ANSWERS that open wait, and the clearance refuses `Corrupt`
        // unless it finds that claim. Sequence numbers are the page positions, which is what
        // `completesWaitSeq` and `claimSeq` name.
        vec![
            serde_json::json!({"type":"execution_started","data":{"executionId":"execution-1","graphVersion":3,"graphHash":hash,"mode":"supervised"}}),
            serde_json::json!({"type":"node_outcome_recorded","data":{"executionId":"execution-1","nodeId":"implementation","outcome":"needs_input","nextState":"waiting_input"}}),
            serde_json::json!({"type":"completion_claimed","data":{"executionId":"execution-1","node":"implementation","completesWaitSeq":2,"evidence":[{"kind":"patch","contentHash":hash,"size":2048}],"attestation":{"asserter":"agent-claimer","mode":"operator_attested"}}}),
            serde_json::json!({"type":"completion_cleared","data":{"executionId":"execution-1","claimSeq":3,"verifier":{"type":"machineReplay","manifestHash":hash}}}),
        ],
        // The same chain, rejected instead of cleared: one arm, one fixture, and the claim is spent
        // either way.
        vec![
            serde_json::json!({"type":"execution_started","data":{"executionId":"execution-1","graphVersion":3,"graphHash":hash,"mode":"supervised"}}),
            serde_json::json!({"type":"node_outcome_recorded","data":{"executionId":"execution-1","nodeId":"implementation","outcome":"needs_input","nextState":"waiting_input"}}),
            serde_json::json!({"type":"completion_claimed","data":{"executionId":"execution-1","node":"implementation","completesWaitSeq":2,"evidence":[{"kind":"patch","contentHash":hash,"size":2048}],"attestation":{"asserter":"agent-claimer","mode":"operator_attested"}}}),
            serde_json::json!({"type":"completion_rejected","data":{"executionId":"execution-1","claimSeq":3,"verifier":{"type":"countersign","identity":"reviewer-1","keyFingerprint":hash},"reasonCode":"evidence_did_not_replay"}}),
        ],
        // The remaining pinned variants, re-sliced BY THE GUARD rather than by my first map's
        // classes. Each of these carries the execution-id guard and, as far as the fold is
        // concerned, needs nothing beyond the execution it names.
        vec![
            serde_json::json!({"type":"execution_started","data":{"executionId":"execution-1","graphVersion":3,"graphHash":hash,"mode":"supervised"}}),
            serde_json::json!({"type":"completion_refused","data":{"executionId":"execution-1","node":"implementation","claimedWaitSeq":4,"reasonCode":"wait_superseded"}}),
        ],
        vec![
            serde_json::json!({"type":"execution_started","data":{"executionId":"execution-1","graphVersion":3,"graphHash":hash,"mode":"supervised"}}),
            serde_json::json!({"type":"dlq_routed","data":{"executionId":"execution-1","nodeId":"implementation","episodeSequence":4,"reason":"stalled"}}),
        ],
        vec![
            serde_json::json!({"type":"execution_started","data":{"executionId":"execution-1","graphVersion":3,"graphHash":hash,"mode":"supervised"}}),
            serde_json::json!({"type":"dlq_redrive","data":{"executionId":"execution-1","nodeId":"implementation","dlqEpisodeSequence":5}}),
        ],
        vec![
            serde_json::json!({"type":"execution_started","data":{"executionId":"execution-1","graphVersion":3,"graphHash":hash,"mode":"supervised"}}),
            serde_json::json!({"type":"dlq_returned","data":{"executionId":"execution-1","nodeId":"implementation","dlqEpisodeSequence":5,"waitWithinSeconds":600}}),
        ],
        vec![
            serde_json::json!({"type":"execution_started","data":{"executionId":"execution-1","graphVersion":3,"graphHash":hash,"mode":"supervised"}}),
            serde_json::json!({"type":"execution_completed","data":{"executionId":"execution-1","status":"completed"}}),
        ],
        vec![
            serde_json::json!({"type":"execution_started","data":{"executionId":"execution-1","graphVersion":3,"graphHash":hash,"mode":"supervised"}}),
            serde_json::json!({"type":"execution_form_declared","data":{"executionId":"execution-1","nodeIds":["start"],"nodeTimeoutSeconds":{"start":900}}}),
        ],
        vec![
            serde_json::json!({"type":"execution_started","data":{"executionId":"execution-1","graphVersion":3,"graphHash":hash,"mode":"supervised"}}),
            // `previousMode` is "supervised", not null: the arm refuses unless it EQUALS the mode
            // the projection already holds, and the prelude started this execution supervised. The
            // house's own wire example carries null because it was written for a stream with no
            // prior mode -- a payload correct in one arrangement and Corrupt in this one.
            serde_json::json!({"type":"execution_mode_changed","data":{"executionId":"execution-1","previousMode":"supervised","mode":"manual"}}),
        ],
        vec![
            serde_json::json!({"type":"execution_started","data":{"executionId":"execution-1","graphVersion":3,"graphHash":hash,"mode":"supervised"}}),
            serde_json::json!({"type":"execution_paused","data":{"executionId":"execution-1"}}),
        ],
        vec![
            serde_json::json!({"type":"execution_started","data":{"executionId":"execution-1","graphVersion":3,"graphHash":hash,"mode":"supervised"}}),
            // Resumed needs something PAUSED to resume: the arm refuses unless
            // `simulation_status` is already `Paused`. Two preludes, because two different
            // requirements stack.
            serde_json::json!({"type":"execution_paused","data":{"executionId":"execution-1"}}),
            serde_json::json!({"type":"execution_resumed","data":{"executionId":"execution-1"}}),
        ],
        vec![
            serde_json::json!({"type":"execution_started","data":{"executionId":"execution-1","graphVersion":3,"graphHash":hash,"mode":"supervised"}}),
            serde_json::json!({"type":"ghost_node_proposed","data":{"executionId":"execution-1","nodeId":"ghost-a","draftId":"draft-1"}}),
        ],
        vec![
            serde_json::json!({"type":"execution_started","data":{"executionId":"execution-1","graphVersion":3,"graphHash":hash,"mode":"supervised"}}),
            // `mode` must EQUAL the projection's, not merely be a legal mode: the arm compares
            // them. The prelude starts supervised, so this says supervised. The house's wire
            // example says autopilot, which is correct there and Corrupt here -- the same trap as
            // `previousMode` above, and the reason a payload cannot be lifted between arrangements.
            serde_json::json!({"type":"mutation_accepted","data":{"executionId":"execution-1","draftId":"draft-1","mode":"supervised","graphVersion":4}}),
        ],
        vec![
            serde_json::json!({"type":"execution_started","data":{"executionId":"execution-1","graphVersion":3,"graphHash":hash,"mode":"supervised"}}),
            serde_json::json!({"type":"overdue_exception","data":{"executionId":"execution-1","nodeId":"implementation","episodeSequence":4,"stage":"claimed","deadline":"2026-08-09T00:00:00Z"}}),
        ],
        vec![
            serde_json::json!({"type":"execution_started","data":{"executionId":"execution-1","graphVersion":3,"graphHash":hash,"mode":"supervised"}}),
            serde_json::json!({"type":"signal_recorded","data":{"executionId":"execution-1","signalId":"signal-1","sourceKind":"node","sourceId":"node-a","kind":"unexpected_dependency","severity":"high","envelopeSha256":raw}}),
        ],
        // `wake_lease_consumed` carries NO execution-id guard, so it needs no `execution_started`
        // -- it needs the LEASE it consumes. Different prelude, same rule: the page is whatever the
        // arm requires of the world before it.
        vec![
            serde_json::json!({"type":"wake_lease","data":{"executionId":"execution-1","sessionId":"session-1","cursor":1,"rendezvousId":"rendezvous-1"}}),
            serde_json::json!({"type":"wake_lease_consumed","data":{"executionId":"execution-1","sessionId":"session-1","reason":"rung","capturedArming":1}}),
        ],
        // #1054: `agent_presence_declared` needs NO prelude at all, and that absence is the
        // fixture's content rather than an omission. The fold arm reads no projection state,
        // checks no execution id and has no precondition -- a declaration is a fact about a
        // session, so there is no world it can be inconsistent with. Every neighbour above needed
        // a prelude because its arm demands one; this one would be answering a requirement that
        // does not exist.
        vec![
            serde_json::json!({"type":"agent_presence_declared","data":{"actorId":"agent-planner","actorType":"agent","model":"claude-opus-5","effort":"high"}}),
        ],
    ];
    let covered: std::collections::BTreeSet<&str> = variants
        .iter()
        .map(|(json, _)| json["type"].as_str().expect("fixture carries a type tag"))
        .chain(
            pages
                .iter()
                .flatten()
                .map(|json| json["type"].as_str().expect("fixture carries a type tag")),
        )
        .collect();
    let non_publication: std::collections::BTreeSet<&str> = EventKind::EVERY_WIRE_NAME
        .iter()
        .copied()
        .filter(|name| *name != "graph_version_published")
        .collect();
    // (1) Every fixture above names a real, current, non-publication variant — catches a typo'd
    // or retired "type" tag in this vec, which the diff in (2) alone would not distinguish from
    // a genuinely uncovered variant.
    let covered_but_unreal: Vec<&&str> = covered.difference(&non_publication).collect();
    assert!(
        covered_but_unreal.is_empty(),
        "fixture(s) above name a type tag EventKind does not currently produce: {covered_but_unreal:?}"
    );
    // (2) THE DEBT IS PAID, AND THE TRIPWIRE STAYS. #569 pinned 28 non-publication variants with
    // zero generation-handler coverage; all 28 now have a fixture, so the list is EMPTY rather
    // than deleted.
    //
    // Empty is not the same as gone, and that is the point. The two assertions below still run:
    // a NEW EventKind variant arriving with no fixture fails the first, and a name added here
    // without need fails the second. Deleting the const would remove the first check with it and
    // let the next variant land uncovered and silent -- which is exactly the drift #190 found
    // hiding behind a vacuous length literal.
    //
    // If a variant ever needs to be pinned again, add it here WITH the reason. Declared debt is a
    // legitimate state; undeclared debt is what this test exists to make impossible.
    const UNCOVERED_PIN: &[&str] = &[];
    let pinned: std::collections::BTreeSet<&str> = UNCOVERED_PIN.iter().copied().collect();
    let actually_uncovered: std::collections::BTreeSet<&str> =
        non_publication.difference(&covered).copied().collect();
    // Two directions, two questions, two messages (D, review on #571): a single "these sets
    // differ" comparison prints both 28-entry sides in full and offers a menu of unequal-cost
    // fixes, so the reader defaults to the cheap one (add a name to the pin) even when that is
    // the wrong branch. Splitting by direction forces the actual question each drift shape asks.
    let newly_uncovered: Vec<&&str> = actually_uncovered.difference(&pinned).collect();
    assert!(
        newly_uncovered.is_empty(),
        "these non-publication variant(s) are uncovered and NOT in UNCOVERED_PIN — a new \
         EventKind variant arrived (it compiles because the enum's exhaustive matches force \
         handling, but this test still needs it CLASSIFIED): either write a fixture now, or add \
         it to UNCOVERED_PIN as new declared debt. Do not leave it unclassified: {newly_uncovered:?}"
    );
    let stale_pin_entries: Vec<&&str> = pinned.difference(&actually_uncovered).collect();
    assert!(
        stale_pin_entries.is_empty(),
        "these UNCOVERED_PIN name(s) are no longer uncovered — either a fixture now exists for \
         them (remove the name from UNCOVERED_PIN in the SAME commit as the fixture, per the \
         comment above) or the variant no longer exists at all: {stale_pin_entries:?}"
    );
    for (kind, project_level) in variants {
        // The tag, kept for the failure message below. Measured need, not decoration: a fixture of
        // mine failed here as a bare `Corrupt` with nothing naming WHICH of the fixtures produced
        // it, and finding out cost a throwaway probe run. A loop over N fixtures that reports only
        // the error is a true statement about the wrong grain.
        let failing_kind = kind["type"]
            .as_str()
            .expect("fixture carries a type tag")
            .to_owned();
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
        generation
            .apply_page(&[envelope])
            .unwrap_or_else(|error| panic!("{failing_kind} is not safe to apply: {error:?}"));
    }

    // The page loop. One generation per page, envelopes chained: sequence is the position, and
    // each `previousHash` is the hash of the one before, because the fold recomputes and compares
    // them. A page is applied whole, so the prelude and the variant it exists for are the same
    // replay -- which is the only way the guard above can be satisfied.
    for page in &pages {
        let event_scope = RepositoryScope::new(
            WorkspaceId::parse("workspace-1").unwrap(),
            ProjectId::parse("project-1").unwrap(),
            Some(ExecutionId::parse("execution-1").unwrap()),
        );
        let mut previous =
            "sha256:35c8ab0717bef1684ad07efcf3bedd4648c778a2c944cbd2c7e6a4802e2237b3".to_owned();
        let mut envelopes = Vec::new();
        for (index, kind) in page.iter().enumerate() {
            let sequence = index as u64 + 1;
            let mut envelope: graphhelm_protocols::EventEnvelope = serde_json::from_value(serde_json::json!({
                "schemaVersion":"1.0.0","eventId":format!("event-{sequence}"),"scope":event_scope,"streamId":"stream-1","sequence":sequence,
                "occurredAt":"2026-08-09T00:00:00Z","idempotencyKey":format!("request-{sequence}"),"actor":{"type":"system","id":"system-1"},
                "sensitivity":"internal","kind":kind,"evidenceRefs":[],"artifactRefs":[],
                "previousHash":previous,
                "eventHash":format!("sha256:{}", "0".repeat(64))
            })).unwrap();
            envelope.event_hash = graphhelm_protocols::EventHash::parse(
                compute_event_hash(&envelope, envelope.previous_hash.as_str()).unwrap(),
            )
            .unwrap();
            previous = envelope.event_hash.as_str().to_owned();
            envelopes.push(envelope);
        }
        // The page's LAST tag names it: that is the variant the page exists to cover, and the one
        // a reader looks for when this fails.
        let covers = page
            .last()
            .and_then(|json| json["type"].as_str())
            .expect("a page carries at least one tagged event")
            .to_owned();
        let mut generation =
            ProjectionGeneration::new(event_scope, "stream-1".into(), "execution".into(), 1, 1)
                .unwrap();
        generation.apply_page(&envelopes).unwrap_or_else(|error| {
            panic!("the page for {covers} is not safe to apply: {error:?}")
        });
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
