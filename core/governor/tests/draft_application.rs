use std::{
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    thread,
};

use chrono::{TimeZone, Utc};
use graphhelm_events::{
    ActiveVersion, AuthenticateRequest, AuthenticationTag, EventPage, EventRepository,
    EventRepositoryError, EvidenceProtector, KeyError, KeyProvider, KeyProviderMetadata,
    LocalEventRepository, LocalFailpoint, PreparedAppend, RepositoryFuture, RevocationReceipt,
    RevokeKeyRequest, SecretBytes, VerifyAuthenticationRequest, WrapKeyRequest, WrappedKey,
};
use graphhelm_governor::{
    ApplyServices, GovernorError, GraphExternalizer, ProjectionPreparation,
    SealingGraphExternalizer, apply_draft,
};
use graphhelm_graph::{GraphVersion, raw_content_sha256};
use graphhelm_protocols::{
    Actor, ActorId, ActorType, ArtifactId, Clock, DraftOperation, DraftRejected, EventEnvelope,
    EventKind, EvidenceId, ExecutionId, GraphDraft, GraphImported, GraphSourceKind,
    GraphVersionPublished, GraphVersionRecord, IdGenerator, NewEvent, OpaqueId, PersistedActor,
    PersistedActorType, PersistedGraphVersion, PersistedGraphVersionRef, ProjectId, RawSha256,
    RepositoryScope, SafeCode, SemanticHash, Sensitivity, WorkspaceId,
};

fn block_on<F: Future>(future: F) -> F::Output {
    struct ThreadWaker(thread::Thread);
    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match Pin::as_mut(&mut future).poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => thread::park(),
        }
    }
}

struct FixedKeyProvider;
impl KeyProvider for FixedKeyProvider {
    fn metadata<'a>(&'a self) -> RepositoryFuture<'a, Result<KeyProviderMetadata, KeyError>> {
        Box::pin(async { KeyProviderMetadata::new("fixed-key", "fixed", "1.0.0", 0) })
    }
    fn wrap<'a>(
        &'a self,
        request: WrapKeyRequest,
    ) -> RepositoryFuture<'a, Result<WrappedKey, KeyError>> {
        Box::pin(async move {
            WrappedKey::new(
                "fixed-key",
                request.handle(),
                "xchacha20poly1305",
                vec![1; 24],
                vec![2; 48],
                raw_content_sha256(request.aad()).unwrap(),
            )
        })
    }
    fn unwrap<'a>(&'a self, _: WrappedKey) -> RepositoryFuture<'a, Result<SecretBytes, KeyError>> {
        Box::pin(async { Err(KeyError::Unavailable) })
    }
    fn revoke<'a>(
        &'a self,
        _: RevokeKeyRequest,
    ) -> RepositoryFuture<'a, Result<RevocationReceipt, KeyError>> {
        Box::pin(async { Err(KeyError::Unavailable) })
    }
    fn authenticate<'a>(
        &'a self,
        _: AuthenticateRequest,
    ) -> RepositoryFuture<'a, Result<AuthenticationTag, KeyError>> {
        Box::pin(async { Err(KeyError::Unavailable) })
    }
    fn verify<'a>(
        &'a self,
        _: VerifyAuthenticationRequest,
    ) -> RepositoryFuture<'a, Result<(), KeyError>> {
        Box::pin(async { Err(KeyError::Unavailable) })
    }
}

struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 10, 12, 0, 0).unwrap()
    }
}

struct DivergentExternalizer {
    inner: SealingGraphExternalizer<EvidenceProtector<FixedKeyProvider>>,
}

struct CountingExternalizer {
    calls: Arc<AtomicUsize>,
    inner: SealingGraphExternalizer<EvidenceProtector<FixedKeyProvider>>,
}

impl GraphExternalizer for CountingExternalizer {
    fn prepare<'a>(
        &'a self,
        scope: RepositoryScope,
        version: &'a GraphVersionRecord,
    ) -> RepositoryFuture<'a, Result<ProjectionPreparation, GovernorError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.prepare(scope, version)
    }

    fn prepare_with_predecessor<'a>(
        &'a self,
        scope: RepositoryScope,
        version: &'a GraphVersionRecord,
        predecessor: PersistedGraphVersionRef,
    ) -> RepositoryFuture<'a, Result<ProjectionPreparation, GovernorError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner
            .prepare_with_predecessor(scope, version, predecessor)
    }
}

impl DivergentExternalizer {
    fn diverge(mut prepared: ProjectionPreparation) -> ProjectionPreparation {
        let version = prepared.version();
        prepared.version = PersistedGraphVersion::new(
            version.number(),
            version.predecessor().cloned(),
            version.topology().clone(),
            version.topology_hash().clone(),
            version.semantic_hash().clone(),
            version.content_slots().to_vec(),
            PersistedActor::new(
                PersistedActorType::System,
                ActorId::parse("system-divergent-adapter").unwrap(),
            ),
            version.created_at().clone(),
        )
        .unwrap();
        prepared
    }
}

impl GraphExternalizer for DivergentExternalizer {
    fn prepare<'a>(
        &'a self,
        scope: RepositoryScope,
        version: &'a GraphVersionRecord,
    ) -> RepositoryFuture<'a, Result<ProjectionPreparation, GovernorError>> {
        Box::pin(async move { self.inner.prepare(scope, version).await.map(Self::diverge) })
    }

    fn prepare_with_predecessor<'a>(
        &'a self,
        scope: RepositoryScope,
        version: &'a GraphVersionRecord,
        predecessor: PersistedGraphVersionRef,
    ) -> RepositoryFuture<'a, Result<ProjectionPreparation, GovernorError>> {
        Box::pin(async move {
            self.inner
                .prepare_with_predecessor(scope, version, predecessor)
                .await
                .map(Self::diverge)
        })
    }
}

#[derive(Default)]
struct AdvancingClock(AtomicU64);
impl Clock for AdvancingClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        let seconds = self.0.fetch_add(1, Ordering::SeqCst);
        Utc.with_ymd_and_hms(2026, 8, 10, 12, 0, seconds as u32)
            .unwrap()
    }
}
#[derive(Default)]
struct Ids(AtomicU64);
impl IdGenerator for Ids {
    fn next_id(&self, prefix: &'static str) -> String {
        format!("{prefix}-{}", self.0.fetch_add(1, Ordering::SeqCst) + 1)
    }
}

struct TestDirectory(PathBuf);
impl TestDirectory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "graphhelm-governor-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

struct CommittedBatchRepository {
    events: Vec<EventEnvelope>,
}

impl EventRepository for CommittedBatchRepository {
    fn append_atomic(
        &self,
        _: &PreparedAppend,
    ) -> Result<Vec<EventEnvelope>, EventRepositoryError> {
        panic!("committed recovery must not append")
    }

    fn read_stream(
        &self,
        _: &RepositoryScope,
        _: &str,
        _: usize,
        _: Option<&str>,
    ) -> Result<EventPage, EventRepositoryError> {
        panic!("committed recovery must not read pages")
    }

    fn read_replay_stream(
        &self,
        _: &RepositoryScope,
        _: &str,
    ) -> Result<Vec<EventEnvelope>, EventRepositoryError> {
        panic!("committed recovery must not replay")
    }

    fn next_sequence(&self, _: &RepositoryScope, _: &str) -> Result<u64, EventRepositoryError> {
        panic!("committed recovery must not read sequence")
    }

    fn evidence_exists(
        &self,
        _: &RepositoryScope,
        _: &EvidenceId,
    ) -> Result<bool, EventRepositoryError> {
        panic!("committed recovery must not read evidence")
    }

    fn artifact_exists(
        &self,
        _: &RepositoryScope,
        _: &ArtifactId,
    ) -> Result<bool, EventRepositoryError> {
        panic!("committed recovery must not read artifacts")
    }

    fn active_version(
        &self,
        _: &RepositoryScope,
        _: &str,
    ) -> Result<Option<ActiveVersion>, EventRepositoryError> {
        panic!("committed recovery must not read active version")
    }

    fn committed_events_for_idempotency(
        &self,
        _: &RepositoryScope,
        _: &str,
        _: &OpaqueId,
    ) -> Result<Option<Vec<EventEnvelope>>, EventRepositoryError> {
        Ok(Some(self.events.clone()))
    }
}
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn base() -> GraphVersion {
    let graph = graphhelm_schema::load_graph(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/graphs/software-feature.yaml"),
    )
    .unwrap()
    .graph;
    GraphVersion::publish(
        graph,
        None,
        Actor::new(ActorType::Owner, "owner-local"),
        Utc.with_ymd_and_hms(2026, 8, 10, 11, 0, 0).unwrap(),
    )
    .unwrap()
}
fn scope(base: &GraphVersion) -> RepositoryScope {
    RepositoryScope::new(
        WorkspaceId::parse("workspace-test").unwrap(),
        ProjectId::parse("project-test").unwrap(),
        Some(ExecutionId::parse(&base.graph().metadata.execution_id).unwrap()),
    )
}
fn draft(base: &GraphVersion) -> GraphDraft {
    let mut node = base.graph().spec.nodes["docs"].clone();
    node.name = "Archive evidence".into();
    GraphDraft {
        id: "draft-1".into(),
        expected_version: base.number(),
        expected_hash: base.content_hash().clone(),
        operations: vec![DraftOperation::AddNode {
            id: "archive".into(),
            node,
        }],
        manual_override: None,
    }
}

#[test]
fn analyze_draft_rejects_oversized_override_before_applying_operations() {
    let base = base();
    for (requirements, risks, risk_chars, path) in [
        (65, 2, 4, "/manualOverride/waivedRequirements"),
        (1, 65, 4, "/manualOverride/acknowledgedRisks"),
        (1, 2, 513, "/manualOverride/acknowledgedRisks"),
    ] {
        let mut request = draft(&base);
        // This invalid operation must not be reached before the borrowed override check.
        request.operations = vec![DraftOperation::RemoveNode {
            id: "missing-node".into(),
        }];
        request.manual_override = Some(graphhelm_protocols::ManualOverride {
            actor: Actor::new(ActorType::Owner, "owner-local"),
            reason: "accepted risk".into(),
            waived_requirements: vec!["review".into(); requirements],
            acknowledged_risks: vec!["r".repeat(risk_chars); risks],
            scope: graphhelm_protocols::WaiverScope::Execution,
        });
        let analysis = graphhelm_governor::analyze_draft(&base, &request);
        assert!(analysis.candidate.is_none());
        assert!(analysis.policy_report.is_none());
        assert_eq!(analysis.diagnostics.len(), 1);
        assert_eq!(
            analysis.diagnostics[0].code,
            "GHP001_OVERRIDE_LIMIT_EXCEEDED"
        );
        assert_eq!(analysis.diagnostics[0].source, request.id);
        assert_eq!(analysis.diagnostics[0].path, path);
    }
}

fn seed_base(
    repository: &LocalEventRepository,
    base: &GraphVersion,
    externalizer: &dyn GraphExternalizer,
) {
    let repository_scope = scope(base);
    let preparation =
        block_on(externalizer.prepare(repository_scope.clone(), &base.to_record())).unwrap();
    let actor = preparation.version().created_by().clone();
    repository
        .append_atomic(
            &graphhelm_events::PreparedAppend::new(
                repository_scope,
                OpaqueId::parse("execution-draft").unwrap(),
                1,
                vec![NewEvent::new(
                    OpaqueId::parse("seed-base").unwrap(),
                    actor,
                    Sensitivity::Internal,
                    EventKind::GraphVersionPublished(Box::new(GraphVersionPublished {
                        version: preparation.version,
                    })),
                    preparation.evidence_refs,
                    vec![],
                )],
                preparation.evidence,
                vec![],
            )
            .unwrap(),
        )
        .unwrap();
}

#[test]
fn valid_draft_atomically_publishes_safe_projection_and_evidence() {
    let base = base();
    let directory = TestDirectory::new();
    let repository =
        LocalEventRepository::open(&directory.0, Arc::new(FixedClock), Arc::new(Ids::default()))
            .unwrap();
    let externalizer = SealingGraphExternalizer::new(EvidenceProtector::new(FixedKeyProvider));
    seed_base(&repository, &base, &externalizer);
    let ids = Ids::default();
    let services = ApplyServices {
        event_repository: &repository,
        scope: scope(&base),
        stream_id: OpaqueId::parse("execution-draft").unwrap(),
        actor: Actor::new(ActorType::Owner, "owner-local"),
        clock: &FixedClock,
        ids: &ids,
        externalizer: &externalizer,
    };
    let result = block_on(apply_draft(&base, &draft(&base), &services)).unwrap();
    assert_eq!(result.version.number(), 2);
    let published = result
        .events
        .iter()
        .find_map(|event| match &event.kind {
            EventKind::GraphVersionPublished(value) => Some(value),
            _ => None,
        })
        .unwrap();
    assert!(!published.version.content_slots().is_empty());
    for reference in result.events.iter().flat_map(|event| &event.evidence_refs) {
        assert!(
            repository
                .evidence_exists(&scope(&base), reference.evidence_id())
                .unwrap()
        );
    }
    let active = repository
        .active_version(&scope(&base), "execution-draft")
        .unwrap()
        .unwrap();
    assert_eq!(active.number, 2);
    let published_sequence = result
        .events
        .iter()
        .find(|event| matches!(event.kind, EventKind::GraphVersionPublished(_)))
        .unwrap()
        .sequence;
    assert_eq!(active.sequence, published_sequence);
    let durable = repository_bytes(&directory.0);
    for plaintext in ["Archive evidence", "Software Feature Delivery"] {
        assert!(!durable.contains(plaintext));
    }
    assert!(!format!("{result:?}").contains("Archive evidence"));
}

#[test]
fn unseeded_repository_rejects_a_successor_without_effects() {
    let base = base();
    let directory = TestDirectory::new();
    let repository =
        LocalEventRepository::open(&directory.0, Arc::new(FixedClock), Arc::new(Ids::default()))
            .unwrap();
    let externalizer = SealingGraphExternalizer::new(EvidenceProtector::new(FixedKeyProvider));
    let ids = Ids::default();
    let services = ApplyServices {
        event_repository: &repository,
        scope: scope(&base),
        stream_id: OpaqueId::parse("execution-draft").unwrap(),
        actor: Actor::new(ActorType::Owner, "owner-local"),
        clock: &FixedClock,
        ids: &ids,
        externalizer: &externalizer,
    };
    let before = std::fs::read(directory.0.join("journal.jsonl")).unwrap();

    let error = block_on(apply_draft(&base, &draft(&base), &services)).unwrap_err();
    assert_eq!(error.code(), "GHD001_STALE_VERSION");
    assert_eq!(
        std::fs::read(directory.0.join("journal.jsonl")).unwrap(),
        before
    );
    assert!(
        repository
            .active_version(&scope(&base), "execution-draft")
            .unwrap()
            .is_none()
    );
}

#[test]
fn apply_result_is_the_exact_authoring_version_committed_by_the_preparation() {
    let base = base();
    let directory = TestDirectory::new();
    let clock = AdvancingClock::default();
    let repository =
        LocalEventRepository::open(&directory.0, Arc::new(FixedClock), Arc::new(Ids::default()))
            .unwrap();
    let externalizer = SealingGraphExternalizer::new(EvidenceProtector::new(FixedKeyProvider));
    seed_base(&repository, &base, &externalizer);
    let ids = Ids::default();
    let services = ApplyServices {
        event_repository: &repository,
        scope: scope(&base),
        stream_id: OpaqueId::parse("execution-draft").unwrap(),
        actor: Actor::new(ActorType::Owner, "owner-local"),
        clock: &clock,
        ids: &ids,
        externalizer: &externalizer,
    };
    let result = block_on(apply_draft(&base, &draft(&base), &services)).unwrap();
    let published = result
        .events
        .iter()
        .find_map(|event| match &event.kind {
            EventKind::GraphVersionPublished(value) => Some(value),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        &result.version.to_record().created_at,
        published.version.created_at().as_datetime()
    );
}

#[test]
fn divergent_public_externalizer_identity_is_rejected_before_repository_append() {
    let base = base();
    let directory = TestDirectory::new();
    let repository =
        LocalEventRepository::open(&directory.0, Arc::new(FixedClock), Arc::new(Ids::default()))
            .unwrap();
    let externalizer = DivergentExternalizer {
        inner: SealingGraphExternalizer::new(EvidenceProtector::new(FixedKeyProvider)),
    };
    let seed_externalizer = SealingGraphExternalizer::new(EvidenceProtector::new(FixedKeyProvider));
    seed_base(&repository, &base, &seed_externalizer);
    let ids = Ids::default();
    let services = ApplyServices {
        event_repository: &repository,
        scope: scope(&base),
        stream_id: OpaqueId::parse("execution-draft").unwrap(),
        actor: Actor::new(ActorType::Owner, "owner-local"),
        clock: &FixedClock,
        ids: &ids,
        externalizer: &externalizer,
    };
    let before = std::fs::read(directory.0.join("journal.jsonl")).unwrap();
    let error = block_on(apply_draft(&base, &draft(&base), &services)).unwrap_err();
    assert_eq!(error.code(), "GHE005_INTEGRITY_FAILURE");
    assert_eq!(
        std::fs::read(directory.0.join("journal.jsonl")).unwrap(),
        before
    );
}

#[test]
fn apply_draft_rejects_graph_mutation_budget_before_repository_or_externalizer_effects() {
    let base = base();
    let directory = TestDirectory::new();
    let repository =
        LocalEventRepository::open(&directory.0, Arc::new(FixedClock), Arc::new(Ids::default()))
            .unwrap();
    let externalizer = SealingGraphExternalizer::new(EvidenceProtector::new(FixedKeyProvider));
    seed_base(&repository, &base, &externalizer);
    let ids = Ids::default();
    let services = ApplyServices {
        event_repository: &repository,
        scope: scope(&base),
        stream_id: OpaqueId::parse("execution-draft").unwrap(),
        actor: Actor::new(ActorType::Owner, "owner-local"),
        clock: &FixedClock,
        ids: &ids,
        externalizer: &externalizer,
    };
    let mut oversized = draft(&base);
    oversized.operations = (0..7)
        .map(|_| DraftOperation::PatchNode {
            id: "implement".into(),
            patch: serde_json::json!({}),
        })
        .collect();
    let journal_before = std::fs::read(directory.0.join("journal.jsonl")).unwrap();

    let error = block_on(apply_draft(&base, &oversized, &services)).unwrap_err();
    assert_eq!(error.code(), "GHE006_LIMIT_EXCEEDED");
    assert_eq!(
        std::fs::read(directory.0.join("journal.jsonl")).unwrap(),
        journal_before
    );
    assert!(
        repository
            .active_version(&scope(&base), "execution-draft")
            .unwrap()
            .is_some_and(|active| active.number == 1)
    );
}

#[test]
fn apply_draft_rejects_the_post_apply_candidate_before_serde_or_repository_effects() {
    let template = base();
    let mut graph = template.graph().clone();
    graph.spec.budgets.max_mutations = None;
    let base = GraphVersion::publish(
        graph,
        None,
        Actor::new(ActorType::Owner, "owner-local"),
        Utc.with_ymd_and_hms(2026, 8, 10, 11, 0, 0).unwrap(),
    )
    .unwrap();
    let node = base.graph().spec.nodes["docs"].clone();
    let operations = (0..=(1024 - base.graph().spec.nodes.len()))
        .map(|index| DraftOperation::AddNode {
            id: format!("candidate-node-{index:04}"),
            node: node.clone(),
        })
        .collect();
    let oversized = GraphDraft {
        id: "draft-candidate-limit".into(),
        expected_version: base.number(),
        expected_hash: base.content_hash().clone(),
        operations,
        manual_override: None,
    };
    let directory = TestDirectory::new();
    let repository =
        LocalEventRepository::open(&directory.0, Arc::new(FixedClock), Arc::new(Ids::default()))
            .unwrap();
    let seed_externalizer = SealingGraphExternalizer::new(EvidenceProtector::new(FixedKeyProvider));
    seed_base(&repository, &base, &seed_externalizer);
    let calls = Arc::new(AtomicUsize::new(0));
    let externalizer = CountingExternalizer {
        calls: calls.clone(),
        inner: SealingGraphExternalizer::new(EvidenceProtector::new(FixedKeyProvider)),
    };
    let ids = Ids::default();
    let services = ApplyServices {
        event_repository: &repository,
        scope: scope(&base),
        stream_id: OpaqueId::parse("execution-draft").unwrap(),
        actor: Actor::new(ActorType::Owner, "owner-local"),
        clock: &FixedClock,
        ids: &ids,
        externalizer: &externalizer,
    };
    let before = std::fs::read(directory.0.join("journal.jsonl")).unwrap();

    let error = block_on(apply_draft(&base, &oversized, &services)).unwrap_err();
    assert_eq!(error.code(), "GHE006_LIMIT_EXCEEDED");
    assert_eq!(
        std::fs::read(directory.0.join("journal.jsonl")).unwrap(),
        before
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn maximum_wire_draft_id_uses_bounded_internal_event_keys() {
    let base = base();
    let directory = TestDirectory::new();
    let repository =
        LocalEventRepository::open(&directory.0, Arc::new(FixedClock), Arc::new(Ids::default()))
            .unwrap();
    let externalizer = SealingGraphExternalizer::new(EvidenceProtector::new(FixedKeyProvider));
    seed_base(&repository, &base, &externalizer);
    let ids = Ids::default();
    let services = ApplyServices {
        event_repository: &repository,
        scope: scope(&base),
        stream_id: OpaqueId::parse("execution-draft").unwrap(),
        actor: Actor::new(ActorType::Owner, "owner-local"),
        clock: &FixedClock,
        ids: &ids,
        externalizer: &externalizer,
    };
    let mut maximum = draft(&base);
    maximum.id = "d".repeat(128);

    let result = block_on(apply_draft(&base, &maximum, &services)).unwrap();
    assert!(result.events.iter().all(|event| {
        event.idempotency_key.as_str().len() <= 128
            && !event.idempotency_key.as_str().contains(&maximum.id)
    }));
}

#[test]
fn stale_hash_records_only_safe_rejection_without_authoring_content() {
    let base = base();
    let mut proposed = draft(&base);
    proposed.expected_hash = SemanticHash::new(format!("sha256:{}", "f".repeat(64)));
    proposed.operations[0] = DraftOperation::PatchNode {
        id: "implement".into(),
        patch: serde_json::json!({"apiKey": "TOP-SECRET-CANARY"}),
    };
    let directory = TestDirectory::new();
    let repository =
        LocalEventRepository::open(&directory.0, Arc::new(FixedClock), Arc::new(Ids::default()))
            .unwrap();
    let externalizer = SealingGraphExternalizer::new(EvidenceProtector::new(FixedKeyProvider));
    seed_base(&repository, &base, &externalizer);
    let ids = Ids::default();
    let services = ApplyServices {
        event_repository: &repository,
        scope: scope(&base),
        stream_id: OpaqueId::parse("execution-draft").unwrap(),
        actor: Actor::new(ActorType::Owner, "owner-local"),
        clock: &FixedClock,
        ids: &ids,
        externalizer: &externalizer,
    };
    let error = block_on(apply_draft(&base, &proposed, &services)).unwrap_err();
    assert_eq!(error.code(), "GHD002_STALE_HASH");
    let durable = std::fs::read(directory.0.join("journal.jsonl")).unwrap();
    assert!(!String::from_utf8_lossy(&durable).contains("TOP-SECRET-CANARY"));
    assert!(!format!("{error:?} {error}").contains("TOP-SECRET-CANARY"));
}

#[test]
fn exact_retry_of_committed_rejection_preserves_the_original_outcome() {
    let base = base();
    let mut rejected = draft(&base);
    rejected.expected_hash = SemanticHash::new(format!("sha256:{}", "f".repeat(64)));
    let directory = TestDirectory::new();
    let repository =
        LocalEventRepository::open(&directory.0, Arc::new(FixedClock), Arc::new(Ids::default()))
            .unwrap();
    let externalizer = SealingGraphExternalizer::new(EvidenceProtector::new(FixedKeyProvider));
    seed_base(&repository, &base, &externalizer);
    let ids = Ids::default();
    let services = ApplyServices {
        event_repository: &repository,
        scope: scope(&base),
        stream_id: OpaqueId::parse("execution-draft").unwrap(),
        actor: Actor::new(ActorType::Owner, "owner-local"),
        clock: &FixedClock,
        ids: &ids,
        externalizer: &externalizer,
    };

    let first = block_on(apply_draft(&base, &rejected, &services)).unwrap_err();
    assert_eq!(first.code(), "GHD002_STALE_HASH");
    let committed = std::fs::read(directory.0.join("journal.jsonl")).unwrap();
    let second = block_on(apply_draft(&base, &rejected, &services)).unwrap_err();
    assert_eq!(second.code(), first.code());
    assert_eq!(
        std::fs::read(directory.0.join("journal.jsonl")).unwrap(),
        committed
    );
    let rejected_count = repository
        .read_replay_stream(&scope(&base), "execution-draft")
        .unwrap()
        .iter()
        .filter(|event| matches!(event.kind, EventKind::DraftRejected(_)))
        .count();
    assert_eq!(rejected_count, 1);
}

fn assert_committed_batch_is_invalid(
    base: &GraphVersion,
    draft: &GraphDraft,
    externalizer: &dyn GraphExternalizer,
    events: Vec<EventEnvelope>,
) {
    let repository = CommittedBatchRepository { events };
    let ids = Ids::default();
    let services = ApplyServices {
        event_repository: &repository,
        scope: scope(base),
        stream_id: OpaqueId::parse("execution-draft").unwrap(),
        actor: Actor::new(ActorType::Owner, "owner-local"),
        clock: &FixedClock,
        ids: &ids,
        externalizer,
    };
    let error = block_on(apply_draft(base, draft, &services)).unwrap_err();
    assert!(matches!(
        error,
        graphhelm_governor::ApplyError::Governor(GovernorError::InvalidProjection)
    ));
}

fn unrelated_event_kinds() -> Vec<EventKind> {
    let hash = format!("sha256:{}", "a".repeat(64));
    let raw = "a".repeat(64);
    vec![
        EventKind::GraphImported(GraphImported {
            source_sha256: RawSha256::parse(raw.clone()).unwrap(),
            source_kind: GraphSourceKind::Generated,
        }),
        serde_json::from_value(serde_json::json!({
            "type":"graph_validation_failed","data":{"diagnostics":[]}
        }))
        .unwrap(),
        serde_json::from_value(serde_json::json!({
            "type":"simulation_started","data":{"simulationId":"simulation-1","graphVersion":1,"graphHash":hash}
        }))
        .unwrap(),
        serde_json::from_value(serde_json::json!({
            "type":"node_state_changed","data":{"simulationId":"simulation-1","nodeId":"start","previousState":null,"nextState":"running"}
        }))
        .unwrap(),
        serde_json::from_value(serde_json::json!({
            "type":"simulation_completed","data":{"simulationId":"simulation-1","status":"completed"}
        }))
        .unwrap(),
        serde_json::from_value(serde_json::json!({
            "type":"integrity_checkpoint_created","data":{"streamId":"stream-1","sequence":1,"eventHash":hash,"repositoryFormat":"1.0.0","authenticationTag":{"keyId":"key-1","algorithm":"hmac-sha256","tagSha256":raw}}
        }))
        .unwrap(),
        serde_json::from_value(serde_json::json!({
            "type":"evidence_erasure_requested","data":{"evidenceScope":{"workspaceId":"workspace-alpha","projectId":"project-alpha","executionId":"execution-alpha"},"operationId":"operation-1","evidenceId":"evidence-1","keyHandleId":"handle-1","retentionPolicyId":"retention-1","retentionPolicyVersion":"1.0.0","authority":"authority-1","reasonCode":"expired","priorState":"available","state":"erasure_pending","requestedAt":"2026-08-09T00:00:00Z"}
        }))
        .unwrap(),
        serde_json::from_value(serde_json::json!({
            "type":"evidence_erasure_completed","data":{"evidenceScope":{"workspaceId":"workspace-alpha","projectId":"project-alpha","executionId":"execution-alpha"},"operationId":"operation-1","evidenceId":"evidence-1","keyHandleId":"handle-1","retentionPolicyId":"retention-1","retentionPolicyVersion":"1.0.0","authority":"authority-1","reasonCode":"expired","ciphertextSha256":raw,"providerReceiptId":"receipt-1","providerEpoch":1,"priorState":"erasure_pending","state":"erased","requestedAt":"2026-08-09T00:00:00Z","completedAt":"2026-08-09T00:01:00Z"}
        }))
        .unwrap(),
        serde_json::from_value(serde_json::json!({
            "type":"evidence_ciphertext_deleted","data":{"evidenceScope":{"workspaceId":"workspace-alpha","projectId":"project-alpha","executionId":"execution-alpha"},"operationId":"operation-1","evidenceId":"evidence-1","ciphertextSha256":raw,"deletedAt":"2026-08-09T00:02:00Z"}
        }))
        .unwrap(),
        serde_json::from_value(serde_json::json!({
            "type":"evidence_legal_hold_changed","data":{"evidenceScope":{"workspaceId":"workspace-alpha","projectId":"project-alpha","executionId":"execution-alpha"},"holdId":"hold-1","evidenceId":"evidence-1","authority":"authority-1","reasonCode":"investigation","state":"placed","changedAt":"2026-08-09T00:03:00Z"}
        }))
        .unwrap(),
    ]
}

fn foreign_waiver_kind() -> EventKind {
    serde_json::from_value(serde_json::json!({
        "type":"policy_waiver_created",
        "data":{"waiver":{
            "id":"waiver-foreign","requirement":"review","executionId":"exec_feature",
            "graphVersion":2,"actor":"owner-foreign","acknowledgedRisks":["accepted-risk"],
            "scope":"execution","createdAt":"2026-08-09T00:00:00Z","expiresAt":null
        }}
    }))
    .unwrap()
}

#[test]
fn committed_success_rejects_unrelated_and_divergent_events() {
    let base = base();
    let directory = TestDirectory::new();
    let repository =
        LocalEventRepository::open(&directory.0, Arc::new(FixedClock), Arc::new(Ids::default()))
            .unwrap();
    let externalizer = SealingGraphExternalizer::new(EvidenceProtector::new(FixedKeyProvider));
    seed_base(&repository, &base, &externalizer);
    let proposed_draft = draft(&base);
    let ids = Ids::default();
    let services = ApplyServices {
        event_repository: &repository,
        scope: scope(&base),
        stream_id: OpaqueId::parse("execution-draft").unwrap(),
        actor: Actor::new(ActorType::Owner, "owner-local"),
        clock: &FixedClock,
        ids: &ids,
        externalizer: &externalizer,
    };
    let canonical = block_on(apply_draft(&base, &proposed_draft, &services))
        .unwrap()
        .events;

    for kind in unrelated_event_kinds() {
        let mut unrelated = canonical.clone();
        let mut extra = unrelated[0].clone();
        extra.kind = kind;
        unrelated.insert(1, extra);
        assert_committed_batch_is_invalid(&base, &proposed_draft, &externalizer, unrelated);
    }

    let mut divergent_proposed = canonical.clone();
    let EventKind::DraftProposed(payload) = &mut divergent_proposed[0].kind else {
        panic!("canonical governor batch starts with DraftProposed")
    };
    payload.operation_count += 1;
    assert_committed_batch_is_invalid(&base, &proposed_draft, &externalizer, divergent_proposed);

    for duplicate_index in [0, 1, canonical.len() - 2, canonical.len() - 1] {
        let mut duplicate = canonical.clone();
        duplicate.insert(duplicate_index + 1, canonical[duplicate_index].clone());
        assert_committed_batch_is_invalid(&base, &proposed_draft, &externalizer, duplicate);
    }

    let mut divergent_obligation = canonical.clone();
    let EventKind::PolicyObligationEvaluated(payload) = &mut divergent_obligation[1].kind else {
        panic!("canonical governor success contains obligations")
    };
    payload.draft_id = OpaqueId::parse("foreign-draft").unwrap();
    assert_committed_batch_is_invalid(&base, &proposed_draft, &externalizer, divergent_obligation);

    let mut reordered_obligations = canonical.clone();
    reordered_obligations.swap(1, 2);
    assert_committed_batch_is_invalid(&base, &proposed_draft, &externalizer, reordered_obligations);

    let mut foreign_actor = canonical.clone();
    foreign_actor[1].actor = PersistedActor::new(
        PersistedActorType::Owner,
        ActorId::parse("owner-foreign").unwrap(),
    );
    assert_committed_batch_is_invalid(&base, &proposed_draft, &externalizer, foreign_actor);

    let mut foreign_waiver = canonical.clone();
    let mut waiver_event = foreign_waiver[1].clone();
    waiver_event.kind = foreign_waiver_kind();
    foreign_waiver.insert(foreign_waiver.len() - 2, waiver_event.clone());
    assert_committed_batch_is_invalid(&base, &proposed_draft, &externalizer, foreign_waiver);

    let mut reordered_waiver = canonical.clone();
    reordered_waiver.insert(1, waiver_event);
    assert_committed_batch_is_invalid(&base, &proposed_draft, &externalizer, reordered_waiver);

    let mut mixed_terminal = canonical.clone();
    let mut extra_terminal = mixed_terminal.last().unwrap().clone();
    extra_terminal.kind = EventKind::DraftRejected(DraftRejected {
        draft_id: OpaqueId::parse("draft-1").unwrap(),
        reason_code: SafeCode::parse("stale_hash").unwrap(),
        diagnostics: vec![],
        detail_evidence_id: None,
    });
    mixed_terminal.push(extra_terminal);
    assert_committed_batch_is_invalid(&base, &proposed_draft, &externalizer, mixed_terminal);

    let mut reordered = canonical;
    reordered.swap(0, 1);
    assert_committed_batch_is_invalid(&base, &proposed_draft, &externalizer, reordered);
}

#[test]
fn committed_rejection_rejects_unrelated_and_duplicate_events() {
    let base = base();
    let mut rejected = draft(&base);
    rejected.expected_hash = SemanticHash::new(format!("sha256:{}", "f".repeat(64)));
    let directory = TestDirectory::new();
    let repository =
        LocalEventRepository::open(&directory.0, Arc::new(FixedClock), Arc::new(Ids::default()))
            .unwrap();
    let externalizer = SealingGraphExternalizer::new(EvidenceProtector::new(FixedKeyProvider));
    seed_base(&repository, &base, &externalizer);
    let ids = Ids::default();
    let services = ApplyServices {
        event_repository: &repository,
        scope: scope(&base),
        stream_id: OpaqueId::parse("execution-draft").unwrap(),
        actor: Actor::new(ActorType::Owner, "owner-local"),
        clock: &FixedClock,
        ids: &ids,
        externalizer: &externalizer,
    };
    assert_eq!(
        block_on(apply_draft(&base, &rejected, &services))
            .unwrap_err()
            .code(),
        "GHD002_STALE_HASH"
    );
    let canonical = repository
        .read_replay_stream(&scope(&base), "execution-draft")
        .unwrap()
        .into_iter()
        .filter(|event| event.sequence > 1)
        .collect::<Vec<_>>();

    for kind in unrelated_event_kinds() {
        let mut unrelated = canonical.clone();
        let mut extra = unrelated[0].clone();
        extra.kind = kind;
        unrelated.insert(1, extra);
        assert_committed_batch_is_invalid(&base, &rejected, &externalizer, unrelated);
    }

    let mut duplicate = canonical.clone();
    duplicate.push(canonical.last().unwrap().clone());
    assert_committed_batch_is_invalid(&base, &rejected, &externalizer, duplicate);

    let mut duplicate_proposed = canonical.clone();
    duplicate_proposed.insert(1, canonical[0].clone());
    assert_committed_batch_is_invalid(&base, &rejected, &externalizer, duplicate_proposed);

    let mut foreign_actor = canonical.clone();
    foreign_actor[0].actor = PersistedActor::new(
        PersistedActorType::Owner,
        ActorId::parse("owner-foreign").unwrap(),
    );
    assert_committed_batch_is_invalid(&base, &rejected, &externalizer, foreign_actor);

    let mut mixed_terminal = canonical;
    let published = repository
        .read_replay_stream(&scope(&base), "execution-draft")
        .unwrap()
        .into_iter()
        .find(|event| matches!(event.kind, EventKind::GraphVersionPublished(_)))
        .unwrap();
    mixed_terminal.insert(mixed_terminal.len() - 1, published);
    assert_committed_batch_is_invalid(&base, &rejected, &externalizer, mixed_terminal);
}

#[test]
fn journal_synced_publication_recovers_active_marker_and_blocks_sibling_successor() {
    let base = base();
    let directory = TestDirectory::new();
    let externalizer = SealingGraphExternalizer::new(EvidenceProtector::new(FixedKeyProvider));
    let seed_repository =
        LocalEventRepository::open(&directory.0, Arc::new(FixedClock), Arc::new(Ids::default()))
            .unwrap();
    seed_base(&seed_repository, &base, &externalizer);
    drop(seed_repository);
    let repository = LocalEventRepository::open_with_failpoint(
        &directory.0,
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
        LocalFailpoint::ActiveMarker,
    )
    .unwrap();
    let ids = Ids::default();
    let services = ApplyServices {
        event_repository: &repository,
        scope: scope(&base),
        stream_id: OpaqueId::parse("execution-draft").unwrap(),
        actor: Actor::new(ActorType::Owner, "owner-local"),
        clock: &FixedClock,
        ids: &ids,
        externalizer: &externalizer,
    };
    assert!(block_on(apply_draft(&base, &draft(&base), &services)).is_err());
    drop(repository);

    let reopened =
        LocalEventRepository::open(&directory.0, Arc::new(FixedClock), Arc::new(Ids::default()))
            .unwrap();
    let active = reopened
        .active_version(&scope(&base), "execution-draft")
        .unwrap()
        .unwrap();
    assert_eq!(active.number, 2);
    let committed = reopened
        .read_replay_stream(&scope(&base), "execution-draft")
        .unwrap()
        .into_iter()
        .filter(|event| event.sequence > 1)
        .collect::<Vec<_>>();
    let retry_ids = Ids::default();
    let retry_services = ApplyServices {
        event_repository: &reopened,
        scope: scope(&base),
        stream_id: OpaqueId::parse("execution-draft").unwrap(),
        actor: Actor::new(ActorType::Owner, "owner-local"),
        clock: &FixedClock,
        ids: &retry_ids,
        externalizer: &externalizer,
    };
    let result = block_on(apply_draft(&base, &draft(&base), &retry_services)).unwrap();
    assert_eq!(result.events, committed);
    let published = committed
        .iter()
        .find_map(|event| match &event.kind {
            EventKind::GraphVersionPublished(payload) => Some(payload),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        result.version.to_record().created_at,
        *published.version.created_at().as_datetime()
    );

    let mut sibling = draft(&base);
    sibling.id = "draft-sibling".into();
    sibling.operations[0] = DraftOperation::RemoveNode { id: "docs".into() };
    let error = block_on(apply_draft(&base, &sibling, &retry_services)).unwrap_err();
    assert_eq!(error.code(), "GHD001_STALE_VERSION");
}

fn repository_bytes(root: &Path) -> String {
    fn visit(path: &Path, bytes: &mut Vec<u8>) {
        for entry in std::fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(&path, bytes);
            } else {
                bytes.extend(std::fs::read(path).unwrap());
            }
        }
    }
    let mut bytes = Vec::new();
    visit(root, &mut bytes);
    String::from_utf8_lossy(&bytes).into_owned()
}
