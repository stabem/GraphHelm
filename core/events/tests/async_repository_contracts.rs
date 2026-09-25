use std::sync::Arc;

use graphhelm_events::{
    ArtifactCatalog, AsyncEventRepository, EvidenceRepository, ReadStart, ReadStreamRequest,
    VerifyRangeRequest,
};

#[test]
fn active_graph_identity_is_bounded_before_checkpoint_authentication() {
    assert!(
        graphhelm_events::ActiveGraphIdentity::new(1, format!("sha256:{}", "a".repeat(64))).is_ok()
    );
    assert!(
        graphhelm_events::ActiveGraphIdentity::new(0, format!("sha256:{}", "a".repeat(64)))
            .is_err()
    );
    assert!(
        graphhelm_events::ActiveGraphIdentity::new(
            9_007_199_254_740_992,
            format!("sha256:{}", "a".repeat(64))
        )
        .is_err()
    );
    assert!(graphhelm_events::ActiveGraphIdentity::new(1, "sha256:xyz").is_err());
}
use graphhelm_protocols::{ProjectId, RepositoryScope, WorkspaceId};

fn scope() -> RepositoryScope {
    RepositoryScope::new(
        WorkspaceId::parse("workspace-contract").unwrap(),
        ProjectId::parse("project-contract").unwrap(),
        None,
    )
}

#[test]
fn async_repository_contracts_are_object_safe() {
    fn accept_events(_: Option<Arc<dyn AsyncEventRepository>>) {}
    fn accept_evidence(_: Option<Arc<dyn EvidenceRepository>>) {}
    fn accept_artifacts(_: Option<Arc<dyn ArtifactCatalog>>) {}

    accept_events(None);
    accept_evidence(None);
    accept_artifacts(None);
}

#[test]
fn owned_async_requests_reject_unbounded_values_at_construction() {
    assert!(ReadStreamRequest::new(scope(), "stream".into(), ReadStart::Beginning, 1).is_ok());
    assert!(ReadStreamRequest::new(scope(), "stream".into(), ReadStart::Beginning, 1_001).is_err());
    assert!(
        ReadStreamRequest::new(
            scope(),
            "stream".into(),
            ReadStart::Cursor("x".repeat(4 * 1024 + 1)),
            1,
        )
        .is_err()
    );
    assert!(VerifyRangeRequest::new(scope(), "stream".into(), 1, 100_000).is_ok());
    assert!(VerifyRangeRequest::new(scope(), "stream".into(), 100_001, 1).is_ok());
    assert!(VerifyRangeRequest::new(scope(), "stream".into(), 1, 100_001).is_err());
}
