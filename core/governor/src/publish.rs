use graphhelm_events::RepositoryFuture;
use graphhelm_graph::{
    DurableContentError, GraphVersion, PersistencePreflight, lint, preflight_execution_graph,
};
use graphhelm_policy::evaluate_transition;
use graphhelm_protocols::{
    Actor, Clock, GraphDraft, GraphVersionRef, ObligationStatus, PersistedGraphVersionRef,
    RepositoryScope,
};

use crate::{
    GovernorError, GraphExternalizer, ProjectionPreparation,
    apply::authoritative_override,
    candidate::{apply_operations, preflight_draft},
    externalize::safe_semantic_hash_for,
};

/// Dependencies for an isolated, side-effect-free publication preparation.
pub struct PublicationPreparationServices<'a> {
    pub scope: RepositoryScope,
    pub actor: Actor,
    pub clock: &'a dyn Clock,
    pub externalizer: &'a dyn GraphExternalizer,
}

/// Observable publication boundaries used by deterministic ordering tests and hosts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicationStage {
    CandidateClone,
    CandidateSerialization,
    CandidateLint,
    CandidatePolicy,
    Externalization,
}

pub trait PublicationPreparationObserver: Sync {
    fn reached(&self, stage: PublicationStage);
}

/// Validates and externalizes a draft candidate without appending or activating state.
pub fn prepare_draft_publication<'a>(
    base: &'a GraphVersion,
    draft: &'a GraphDraft,
    services: &'a PublicationPreparationServices<'a>,
) -> RepositoryFuture<'a, Result<ProjectionPreparation, GovernorError>> {
    prepare_draft_publication_observed(base, draft, services, None)
}

/// Equivalent preparation with an allocation-free stage observer.
pub fn prepare_draft_publication_observed<'a>(
    base: &'a GraphVersion,
    draft: &'a GraphDraft,
    services: &'a PublicationPreparationServices<'a>,
    observer: Option<&'a dyn PublicationPreparationObserver>,
) -> RepositoryFuture<'a, Result<ProjectionPreparation, GovernorError>> {
    Box::pin(async move {
        if draft.expected_version != base.number() || &draft.expected_hash != base.content_hash() {
            return Err(GovernorError::InvalidAuthoring);
        }
        preflight_publication_inputs(base, draft, &services.actor)?;
        observe(observer, PublicationStage::CandidateClone);
        let mut candidate = base.graph().clone();
        apply_operations(&mut candidate, &draft.operations)
            .map_err(|_| GovernorError::InvalidAuthoring)?;
        candidate.metadata.version = base
            .number()
            .checked_add(1)
            .ok_or(GovernorError::InvalidAuthoring)?;
        candidate.metadata.based_on = Some(base.graph().metadata.id.clone());

        preflight_execution_graph(&candidate).map_err(map_preflight_error)?;

        observe(observer, PublicationStage::CandidateSerialization);
        let raw = serde_json::to_value(&candidate).map_err(|_| GovernorError::InvalidAuthoring)?;
        if !graphhelm_schema::validate_graph_value(&raw, "governor-candidate").is_empty() {
            return Err(GovernorError::InvalidAuthoring);
        }
        observe(observer, PublicationStage::CandidateLint);
        if !lint(&candidate, "governor-candidate").errors.is_empty() {
            return Err(GovernorError::InvalidAuthoring);
        }
        let manual_override = authoritative_override(draft, &services.actor);
        observe(observer, PublicationStage::CandidatePolicy);
        let policy = evaluate_transition(base, &candidate, manual_override);
        if policy.obligations.iter().any(|obligation| {
            matches!(
                obligation.status,
                ObligationStatus::Impossible | ObligationStatus::Unsatisfied
            )
        }) {
            return Err(GovernorError::InvalidAuthoring);
        }

        let version = GraphVersion::publish(
            candidate,
            Some(GraphVersionRef {
                number: base.number(),
                content_hash: base.content_hash().clone(),
            }),
            services.actor.clone(),
            services.clock.now(),
        )
        .map_err(|_| GovernorError::InvalidAuthoring)?;
        let base_semantic_hash = safe_semantic_hash_for(&services.scope, &base.to_record())?;
        let predecessor = PersistedGraphVersionRef::new(base.number(), base_semantic_hash)
            .map_err(|_| GovernorError::InvalidProjection)?;
        let record = version.to_record();
        observe(observer, PublicationStage::Externalization);
        services
            .externalizer
            .prepare_with_predecessor(services.scope.clone(), &record, predecessor)
            .await
    })
}

pub(crate) fn preflight_publication_inputs(
    base: &GraphVersion,
    draft: &GraphDraft,
    actor: &Actor,
) -> Result<(), GovernorError> {
    preflight_execution_graph(base.graph()).map_err(map_preflight_error)?;
    let mut aggregate = PersistencePreflight::new();
    aggregate
        .account_value(base.semantic())
        .map_err(map_preflight_error)?;
    preflight_draft(
        draft,
        actor,
        base.graph().spec.budgets.max_mutations,
        &mut aggregate,
    )
    .map_err(map_preflight_error)
}

pub(crate) fn prepare_published_version<'a>(
    base: &'a GraphVersion,
    version: &'a GraphVersion,
    services: &'a PublicationPreparationServices<'a>,
) -> RepositoryFuture<'a, Result<ProjectionPreparation, GovernorError>> {
    Box::pin(async move {
        let base_semantic_hash = safe_semantic_hash_for(&services.scope, &base.to_record())?;
        let predecessor = PersistedGraphVersionRef::new(base.number(), base_semantic_hash)
            .map_err(|_| GovernorError::InvalidProjection)?;
        let record = version.to_record();
        services
            .externalizer
            .prepare_with_predecessor(services.scope.clone(), &record, predecessor)
            .await
    })
}

fn observe(observer: Option<&dyn PublicationPreparationObserver>, stage: PublicationStage) {
    if let Some(observer) = observer {
        observer.reached(stage);
    }
}

const fn map_preflight_error(error: DurableContentError) -> GovernorError {
    match error {
        DurableContentError::LimitExceeded => GovernorError::LimitExceeded,
        DurableContentError::Unsafe => GovernorError::InvalidAuthoring,
    }
}
