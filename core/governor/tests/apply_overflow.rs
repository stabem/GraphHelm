use std::{
    future::Future,
    path::Path,
    pin::pin,
    task::{Context, Poll, Waker},
};

use chrono::{TimeZone, Utc};
use graphhelm_events::{
    ActiveVersion, EventPage, EventRepository, EventRepositoryError, PreparedAppend,
    RepositoryFuture,
};
use graphhelm_governor::{
    ApplyServices, GovernorError, GraphExternalizer, ProjectionPreparation, apply_draft,
};
use graphhelm_graph::GraphVersion;
use graphhelm_protocols::{
    Actor, ActorType, ArtifactId, Clock, DraftOperation, EvidenceId, GraphDraft,
    GraphVersionRecord, IdGenerator, OpaqueId, PersistedGraphVersionRef, ProjectId,
    RepositoryScope, WorkspaceId,
};

fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    let mut future = pin!(future);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("overflow gate unexpectedly yielded"),
    }
}

struct PanicRepository;

impl EventRepository for PanicRepository {
    fn append_atomic(
        &self,
        _request: &PreparedAppend,
    ) -> Result<Vec<graphhelm_protocols::EventEnvelope>, EventRepositoryError> {
        panic!("overflow gate reached repository append")
    }

    fn read_stream(
        &self,
        _scope: &RepositoryScope,
        _stream_id: &str,
        _limit: usize,
        _cursor: Option<&str>,
    ) -> Result<EventPage, EventRepositoryError> {
        panic!("overflow gate reached repository read")
    }
    fn read_replay_stream(
        &self,
        _: &RepositoryScope,
        _: &str,
    ) -> Result<Vec<graphhelm_protocols::EventEnvelope>, EventRepositoryError> {
        panic!("overflow gate reached repository replay")
    }
    fn next_sequence(&self, _: &RepositoryScope, _: &str) -> Result<u64, EventRepositoryError> {
        panic!("overflow gate reached next sequence")
    }

    fn evidence_exists(
        &self,
        _scope: &RepositoryScope,
        _evidence_id: &EvidenceId,
    ) -> Result<bool, EventRepositoryError> {
        panic!("overflow gate reached evidence lookup")
    }

    fn artifact_exists(
        &self,
        _scope: &RepositoryScope,
        _artifact_id: &ArtifactId,
    ) -> Result<bool, EventRepositoryError> {
        panic!("overflow gate reached artifact lookup")
    }

    fn active_version(
        &self,
        _scope: &RepositoryScope,
        _stream_id: &str,
    ) -> Result<Option<ActiveVersion>, EventRepositoryError> {
        panic!("overflow gate reached active marker lookup")
    }

    fn committed_events_for_idempotency(
        &self,
        _: &RepositoryScope,
        _: &str,
        _: &OpaqueId,
    ) -> Result<Option<Vec<graphhelm_protocols::EventEnvelope>>, EventRepositoryError> {
        panic!("overflow gate reached committed request lookup")
    }
}

struct PanicExternalizer;

impl GraphExternalizer for PanicExternalizer {
    fn prepare<'a>(
        &'a self,
        _scope: RepositoryScope,
        _version: &'a GraphVersionRecord,
    ) -> RepositoryFuture<'a, Result<ProjectionPreparation, GovernorError>> {
        panic!("overflow gate reached externalizer")
    }

    fn prepare_with_predecessor<'a>(
        &'a self,
        _scope: RepositoryScope,
        _version: &'a GraphVersionRecord,
        _predecessor: PersistedGraphVersionRef,
    ) -> RepositoryFuture<'a, Result<ProjectionPreparation, GovernorError>> {
        panic!("overflow gate reached externalizer")
    }
}

struct DivergentActiveRepository;

impl EventRepository for DivergentActiveRepository {
    fn append_atomic(
        &self,
        _: &PreparedAppend,
    ) -> Result<Vec<graphhelm_protocols::EventEnvelope>, EventRepositoryError> {
        panic!("stale hash gate reached repository append")
    }
    fn read_stream(
        &self,
        _: &RepositoryScope,
        _: &str,
        _: usize,
        _: Option<&str>,
    ) -> Result<EventPage, EventRepositoryError> {
        Ok(EventPage {
            events: vec![],
            next_cursor: None,
            head: None,
        })
    }
    fn read_replay_stream(
        &self,
        _: &RepositoryScope,
        _: &str,
    ) -> Result<Vec<graphhelm_protocols::EventEnvelope>, EventRepositoryError> {
        Ok(vec![])
    }
    fn next_sequence(&self, _: &RepositoryScope, _: &str) -> Result<u64, EventRepositoryError> {
        Ok(1)
    }
    fn evidence_exists(
        &self,
        _: &RepositoryScope,
        _: &EvidenceId,
    ) -> Result<bool, EventRepositoryError> {
        panic!("stale hash gate reached evidence lookup")
    }
    fn artifact_exists(
        &self,
        _: &RepositoryScope,
        _: &ArtifactId,
    ) -> Result<bool, EventRepositoryError> {
        panic!("stale hash gate reached artifact lookup")
    }
    fn active_version(
        &self,
        _: &RepositoryScope,
        _: &str,
    ) -> Result<Option<ActiveVersion>, EventRepositoryError> {
        Ok(Some(ActiveVersion {
            number: 1,
            semantic_hash: format!("sha256:{}", "f".repeat(64)),
            sequence: 1,
            event_hash: format!("sha256:{}", "e".repeat(64)),
        }))
    }

    fn committed_events_for_idempotency(
        &self,
        _: &RepositoryScope,
        _: &str,
        _: &OpaqueId,
    ) -> Result<Option<Vec<graphhelm_protocols::EventEnvelope>>, EventRepositoryError> {
        Ok(None)
    }
}

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 10, 12, 0, 0).unwrap()
    }
}

struct PanicIds;

impl IdGenerator for PanicIds {
    fn next_id(&self, _prefix: &'static str) -> String {
        panic!("overflow gate reached identifier allocation")
    }
}

#[test]
fn maximum_graph_version_is_rejected_before_any_repository_or_sealing_effect() {
    let mut graph = graphhelm_schema::load_graph(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/graphs/software-feature.yaml"),
    )
    .unwrap()
    .graph;
    graph.metadata.version = u64::MAX;
    let base = GraphVersion::publish(
        graph,
        None,
        Actor::new(ActorType::Owner, "owner-local"),
        Utc.with_ymd_and_hms(2026, 8, 10, 11, 0, 0).unwrap(),
    )
    .unwrap();
    let draft = GraphDraft {
        id: "draft-overflow".into(),
        expected_version: base.number(),
        expected_hash: base.content_hash().clone(),
        operations: vec![DraftOperation::RemoveNode {
            id: "does-not-matter".into(),
        }],
        manual_override: None,
    };
    let services = ApplyServices {
        event_repository: &PanicRepository,
        scope: RepositoryScope::new(
            WorkspaceId::parse("workspace-test").unwrap(),
            ProjectId::parse("project-test").unwrap(),
            None,
        ),
        stream_id: OpaqueId::parse("stream-test").unwrap(),
        actor: Actor::new(ActorType::Owner, "owner-local"),
        clock: &FixedClock,
        ids: &PanicIds,
        externalizer: &PanicExternalizer,
    };

    let error = block_on(apply_draft(&base, &draft, &services)).unwrap_err();
    assert_eq!(error.code(), "GHP001_STRUCTURAL_IMPOSSIBILITY");
}

#[test]
fn same_number_divergent_active_hash_fails_before_externalization_or_append() {
    let graph = graphhelm_schema::load_graph(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/graphs/software-feature.yaml"),
    )
    .unwrap()
    .graph;
    let base = GraphVersion::publish(
        graph,
        None,
        Actor::new(ActorType::Owner, "owner-local"),
        Utc.with_ymd_and_hms(2026, 8, 10, 11, 0, 0).unwrap(),
    )
    .unwrap();
    let draft = GraphDraft {
        id: "draft-stale-hash".into(),
        expected_version: base.number(),
        expected_hash: base.content_hash().clone(),
        operations: vec![DraftOperation::RemoveNode {
            id: "does-not-matter".into(),
        }],
        manual_override: None,
    };
    let services = ApplyServices {
        event_repository: &DivergentActiveRepository,
        scope: RepositoryScope::new(
            WorkspaceId::parse("workspace-test").unwrap(),
            ProjectId::parse("project-test").unwrap(),
            Some(graphhelm_protocols::ExecutionId::parse("exec_feature").unwrap()),
        ),
        stream_id: OpaqueId::parse("stream-test").unwrap(),
        actor: Actor::new(ActorType::Owner, "owner-local"),
        clock: &FixedClock,
        ids: &PanicIds,
        externalizer: &PanicExternalizer,
    };
    let error = block_on(apply_draft(&base, &draft, &services)).unwrap_err();
    assert_eq!(error.code(), "GHD002_STALE_HASH");
}
