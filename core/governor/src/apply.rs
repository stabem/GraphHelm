use graphhelm_events::{EventRepository, EventRepositoryError, PreparedAppend, RepositoryFuture};
use graphhelm_graph::{GraphError, GraphVersion, preflight_execution_graph, raw_content_sha256};
use graphhelm_policy::evaluate_transition;
use graphhelm_protocols::{
    Actor, ActorId, ActorType, Clock, DiagnosticComponent, DiagnosticDomainPath, DraftApplied,
    DraftProposed, DraftRejected, EventEnvelope, EventKind, GraphDraft, GraphVersionPublished,
    GraphVersionRef, IdGenerator, ManualOverride, ObligationStatus, OpaqueId, PersistedActor,
    PersistedActorType, PersistedDiagnostic, PersistedGraphVersionRef, PersistedObligationStatus,
    PolicyObligation, PolicyObligationEvaluated, PolicyReport, PolicyWaiver, PolicyWaiverCreated,
    RepositoryScope, SafeCode, Sensitivity, Severity, WireHash,
};
use thiserror::Error;

use crate::{
    GovernorError, GraphExternalizer, PublicationPreparationServices,
    candidate::apply_operations,
    externalize::{projected_version_for, safe_semantic_hash_for},
    publish::{preflight_publication_inputs, prepare_published_version},
};

pub struct ApplyServices<'a> {
    pub event_repository: &'a dyn EventRepository,
    pub scope: RepositoryScope,
    pub stream_id: OpaqueId,
    pub actor: Actor,
    pub clock: &'a dyn Clock,
    pub ids: &'a dyn IdGenerator,
    pub externalizer: &'a dyn GraphExternalizer,
}

#[derive(Clone)]
pub struct ApplyResult {
    pub version: GraphVersion,
    pub waivers: Vec<PolicyWaiver>,
    pub events: Vec<EventEnvelope>,
    pub policy_report: PolicyReport,
}

impl std::fmt::Debug for ApplyResult {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ApplyResult")
            .field("version_number", &self.version.number())
            .field("waiver_count", &self.waivers.len())
            .field("event_count", &self.events.len())
            .field("policy_result_status", &self.policy_report.result_status)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Error)]
pub enum ApplyError {
    #[error("draft expected graph version is stale")]
    StaleVersion,
    #[error("draft expected semantic hash is stale")]
    StaleHash,
    #[error("draft operation is invalid")]
    InvalidOperation,
    #[error("candidate is structurally impossible")]
    StructuralImpossible,
    #[error("candidate requires a complete owner override")]
    OverrideRequired,
    #[error("generated waiver failed schema validation")]
    InvalidWaiver,
    #[error(transparent)]
    Repository(#[from] EventRepositoryError),
    #[error(transparent)]
    Governor(#[from] GovernorError),
    #[error(transparent)]
    Graph(#[from] GraphError),
}

impl ApplyError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::StaleVersion => "GHD001_STALE_VERSION",
            Self::StaleHash => "GHD002_STALE_HASH",
            Self::InvalidOperation => "GHD003_OPERATION_INVALID",
            Self::StructuralImpossible | Self::InvalidWaiver => "GHP001_STRUCTURAL_IMPOSSIBILITY",
            Self::OverrideRequired => "GHP002_OVERRIDE_REQUIRED",
            Self::Repository(error) => error.code(),
            Self::Governor(error) => error.code(),
            Self::Graph(_) => "GHD003_OPERATION_INVALID",
        }
    }

    #[must_use]
    pub const fn is_io(&self) -> bool {
        matches!(
            self,
            Self::Repository(
                EventRepositoryError::Storage | EventRepositoryError::StorageAt { .. }
            )
        )
    }
}

/// Prepares safe Evidence and publishes the candidate plus audit events in one repository append.
pub fn apply_draft<'a>(
    base: &'a GraphVersion,
    draft: &'a GraphDraft,
    services: &'a ApplyServices<'a>,
) -> RepositoryFuture<'a, Result<ApplyResult, ApplyError>> {
    Box::pin(async move {
        // This gate intentionally precedes repository reads, candidate apply and Evidence sealing.
        base.number()
            .checked_add(1)
            .ok_or(ApplyError::StructuralImpossible)?;
        preflight_publication_inputs(base, draft, &services.actor)?;
        let request_identity = governor_request_identity(base, draft, services)?;
        let proposed_key = governor_event_key(&request_identity, b"proposed")?;
        if let Some(events) = services.event_repository.committed_events_for_idempotency(
            &services.scope,
            services.stream_id.as_str(),
            &proposed_key,
        )? {
            return recover_committed_apply(base, draft, services, events);
        }
        let expected_sequence = next_sequence(services)?;
        let base_safe_hash = safe_semantic_hash_for(&services.scope, &base.to_record())?;
        let Some(active) = services
            .event_repository
            .active_version(&services.scope, services.stream_id.as_str())?
        else {
            return Err(ApplyError::StaleVersion);
        };
        if active.number != base.number() {
            return Err(ApplyError::StaleVersion);
        }
        if active.semantic_hash != base_safe_hash.as_str() {
            return Err(ApplyError::StaleHash);
        }
        if draft.expected_version != base.number() {
            return reject(
                services,
                draft,
                &[],
                expected_sequence,
                &request_identity,
                ApplyError::StaleVersion,
            );
        }
        if &draft.expected_hash != base.content_hash() {
            return reject(
                services,
                draft,
                &[],
                expected_sequence,
                &request_identity,
                ApplyError::StaleHash,
            );
        }

        let mut candidate = base.graph().clone();
        if apply_operations(&mut candidate, &draft.operations).is_err() {
            return reject(
                services,
                draft,
                &[],
                expected_sequence,
                &request_identity,
                ApplyError::InvalidOperation,
            );
        }
        candidate.metadata.version = base
            .number()
            .checked_add(1)
            .ok_or(ApplyError::StructuralImpossible)?;
        candidate.metadata.based_on = Some(base.graph().metadata.id.clone());

        preflight_execution_graph(&candidate).map_err(|error| match error {
            graphhelm_graph::DurableContentError::LimitExceeded => {
                ApplyError::Governor(GovernorError::LimitExceeded)
            }
            graphhelm_graph::DurableContentError::Unsafe => ApplyError::InvalidOperation,
        })?;

        let raw = serde_json::to_value(&candidate).map_err(|_| ApplyError::InvalidOperation)?;
        if !graphhelm_schema::validate_graph_value(&raw, "governor-candidate").is_empty() {
            return reject(
                services,
                draft,
                &[],
                expected_sequence,
                &request_identity,
                ApplyError::StructuralImpossible,
            );
        }
        let manual_override = authoritative_override(draft, &services.actor);
        let policy_report = evaluate_transition(base, &candidate, manual_override);
        if policy_report
            .obligations
            .iter()
            .any(|item| item.status == ObligationStatus::Impossible)
        {
            return reject(
                services,
                draft,
                &policy_report.obligations,
                expected_sequence,
                &request_identity,
                ApplyError::StructuralImpossible,
            );
        }
        if policy_report
            .obligations
            .iter()
            .any(|item| item.status == ObligationStatus::Unsatisfied)
        {
            return reject(
                services,
                draft,
                &policy_report.obligations,
                expected_sequence,
                &request_identity,
                ApplyError::OverrideRequired,
            );
        }

        let waivers = create_waivers(&candidate, manual_override, &policy_report, services)?;
        let version = GraphVersion::publish(
            candidate,
            Some(GraphVersionRef {
                number: base.number(),
                content_hash: base.content_hash().clone(),
            }),
            services.actor.clone(),
            services.clock.now(),
        )?;
        let preparation_services = PublicationPreparationServices {
            scope: services.scope.clone(),
            actor: services.actor.clone(),
            clock: services.clock,
            externalizer: services.externalizer,
        };
        let preparation = prepare_published_version(base, &version, &preparation_services).await?;
        let predecessor = PersistedGraphVersionRef::new(base.number(), base_safe_hash.clone())
            .map_err(|_| ApplyError::Governor(GovernorError::InvalidProjection))?;
        let expected_projection =
            projected_version_for(&services.scope, &version.to_record(), Some(predecessor))?;
        if preparation.version() != &expected_projection {
            return Err(ApplyError::Governor(GovernorError::InvalidProjection));
        }

        let actor = persisted_actor(&services.actor)?;
        let mut pending = vec![proposed(draft, &actor, &request_identity)?];
        pending.extend(obligation_events(
            draft,
            &policy_report.obligations,
            &actor,
            &request_identity,
        )?);
        for waiver in &waivers {
            pending.push(new_event(
                governor_event_key(
                    &request_identity,
                    format!("waiver:{}", waiver.requirement).as_bytes(),
                )?,
                &actor,
                EventKind::PolicyWaiverCreated(PolicyWaiverCreated {
                    waiver: waiver.clone(),
                }),
                vec![],
            )?);
        }
        pending.push(new_event(
            governor_event_key(&request_identity, b"published")?,
            &actor,
            EventKind::GraphVersionPublished(Box::new(GraphVersionPublished {
                version: preparation.version.clone(),
            })),
            preparation.evidence_refs.clone(),
        )?);
        pending.push(new_event(
            governor_event_key(&request_identity, b"applied")?,
            &actor,
            EventKind::DraftApplied(DraftApplied {
                draft_id: opaque(&draft.id)?,
                graph_version: version.number(),
                graph_hash: preparation.version.semantic_hash().clone(),
            }),
            vec![],
        )?);
        let request = PreparedAppend::new(
            services.scope.clone(),
            services.stream_id.clone(),
            expected_sequence,
            pending,
            preparation.evidence,
            vec![],
        )?;
        let events = services.event_repository.append_atomic(&request)?;
        Ok(ApplyResult {
            version,
            waivers,
            events,
            policy_report,
        })
    })
}

pub(super) fn authoritative_override<'a>(
    draft: &'a GraphDraft,
    actor: &Actor,
) -> Option<&'a ManualOverride> {
    draft
        .manual_override
        .as_ref()
        .filter(|request| actor.is_owner() && request.actor == *actor)
}

fn create_waivers(
    candidate: &graphhelm_protocols::ExecutionGraph,
    manual_override: Option<&ManualOverride>,
    report: &PolicyReport,
    services: &ApplyServices<'_>,
) -> Result<Vec<PolicyWaiver>, ApplyError> {
    let Some(request) = manual_override else {
        return Ok(Vec::new());
    };
    let mut waivers = Vec::new();
    for obligation in report
        .obligations
        .iter()
        .filter(|item| item.status == ObligationStatus::Waived)
    {
        let waiver = PolicyWaiver {
            id: services.ids.next_id("waiver"),
            requirement: obligation.requirement.clone(),
            execution_id: candidate.metadata.execution_id.clone(),
            graph_version: candidate.metadata.version,
            actor: services.actor.id.clone(),
            reason: Some(request.reason.clone()),
            acknowledged_risks: request.acknowledged_risks.clone(),
            scope: request.scope.clone(),
            created_at: services.clock.now(),
            expires_at: None,
        };
        let raw = serde_json::to_value(&waiver).map_err(|_| ApplyError::InvalidWaiver)?;
        if !graphhelm_schema::validate_waiver(&raw, "generated-waiver").is_empty() {
            return Err(ApplyError::InvalidWaiver);
        }
        waivers.push(waiver);
    }
    Ok(waivers)
}

fn reject<T>(
    services: &ApplyServices<'_>,
    draft: &GraphDraft,
    obligations: &[PolicyObligation],
    expected_sequence: u64,
    request_identity: &WireHash,
    error: ApplyError,
) -> Result<T, ApplyError> {
    let actor = persisted_actor(&services.actor)?;
    let mut pending = vec![proposed(draft, &actor, request_identity)?];
    pending.extend(obligation_events(
        draft,
        obligations,
        &actor,
        request_identity,
    )?);
    pending.push(new_event(
        governor_event_key(
            request_identity,
            format!("rejected:{}", reason_code(&error)).as_bytes(),
        )?,
        &actor,
        EventKind::DraftRejected(DraftRejected {
            draft_id: opaque(&draft.id)?,
            reason_code: SafeCode::parse(reason_code(&error))
                .map_err(|_| ApplyError::InvalidOperation)?,
            diagnostics: vec![safe_diagnostic(error.code())?],
            detail_evidence_id: None,
        }),
        vec![],
    )?);
    let request = PreparedAppend::new(
        services.scope.clone(),
        services.stream_id.clone(),
        expected_sequence,
        pending,
        vec![],
        vec![],
    )?;
    services.event_repository.append_atomic(&request)?;
    Err(error)
}

fn proposed(
    draft: &GraphDraft,
    actor: &PersistedActor,
    request_identity: &WireHash,
) -> Result<graphhelm_protocols::NewEvent, ApplyError> {
    new_event(
        governor_event_key(request_identity, b"proposed")?,
        actor,
        EventKind::DraftProposed(DraftProposed {
            draft_id: opaque(&draft.id)?,
            expected_version: draft.expected_version,
            expected_hash: WireHash::parse(draft.expected_hash.as_str())
                .map_err(|_| ApplyError::InvalidOperation)?,
            operation_count: u16::try_from(draft.operations.len())
                .map_err(|_| ApplyError::InvalidOperation)?,
        }),
        vec![],
    )
}

fn obligation_events(
    draft: &GraphDraft,
    obligations: &[PolicyObligation],
    actor: &PersistedActor,
    request_identity: &WireHash,
) -> Result<Vec<graphhelm_protocols::NewEvent>, ApplyError> {
    obligations
        .iter()
        .map(|obligation| {
            let status = match obligation.status {
                ObligationStatus::Satisfied => PersistedObligationStatus::Satisfied,
                ObligationStatus::Unsatisfied => PersistedObligationStatus::Unsatisfied,
                ObligationStatus::Waived => PersistedObligationStatus::Waived,
                ObligationStatus::Impossible => PersistedObligationStatus::Impossible,
            };
            new_event(
                governor_event_key(
                    request_identity,
                    format!("obligation:{}", obligation.requirement).as_bytes(),
                )?,
                actor,
                EventKind::PolicyObligationEvaluated(PolicyObligationEvaluated {
                    draft_id: opaque(&draft.id)?,
                    requirement_id: opaque(&obligation.requirement)?,
                    status,
                    // Foundation policy evidence labels are authoring assertions, not
                    // committed Evidence identities. Only real sealed identities may
                    // cross this persistence boundary.
                    evidence_ids: vec![],
                    reason_code: SafeCode::parse(match obligation.status {
                        ObligationStatus::Satisfied => "satisfied",
                        ObligationStatus::Unsatisfied => "unsatisfied",
                        ObligationStatus::Waived => "waived",
                        ObligationStatus::Impossible => "impossible",
                    })
                    .map_err(|_| ApplyError::InvalidOperation)?,
                    overrideable: obligation.overrideable,
                }),
                vec![],
            )
        })
        .collect()
}

fn new_event(
    idempotency_key: OpaqueId,
    actor: &PersistedActor,
    kind: EventKind,
    evidence_refs: Vec<graphhelm_protocols::EvidenceReference>,
) -> Result<graphhelm_protocols::NewEvent, ApplyError> {
    Ok(graphhelm_protocols::NewEvent::new(
        idempotency_key,
        actor.clone(),
        Sensitivity::Internal,
        kind,
        evidence_refs,
        vec![],
    ))
}

fn persisted_actor(actor: &Actor) -> Result<PersistedActor, ApplyError> {
    let actor_type = match actor.actor_type {
        ActorType::Owner => PersistedActorType::Owner,
        ActorType::Human => PersistedActorType::Human,
        ActorType::Agent => PersistedActorType::Agent,
        ActorType::System => PersistedActorType::System,
    };
    Ok(PersistedActor::new(
        actor_type,
        ActorId::parse(&actor.id).map_err(|_| ApplyError::InvalidOperation)?,
    ))
}

fn governor_request_identity(
    base: &GraphVersion,
    draft: &GraphDraft,
    services: &ApplyServices<'_>,
) -> Result<WireHash, ApplyError> {
    let draft_bytes = serde_json::to_vec(draft).map_err(|_| ApplyError::InvalidOperation)?;
    let mut material = Vec::with_capacity(draft_bytes.len().saturating_add(512));
    for part in [
        b"graphhelm-governor-request-v1".as_slice(),
        services.scope.workspace_id().as_str().as_bytes(),
        services.scope.project_id().as_str().as_bytes(),
        services
            .scope
            .execution_id()
            .map_or(b"".as_slice(), |value| value.as_str().as_bytes()),
        services.stream_id.as_str().as_bytes(),
        base.content_hash().as_str().as_bytes(),
        services.actor.id.as_bytes(),
        match services.actor.actor_type {
            ActorType::Owner => b"owner".as_slice(),
            ActorType::Human => b"human".as_slice(),
            ActorType::Agent => b"agent".as_slice(),
            ActorType::System => b"system".as_slice(),
        },
        &draft_bytes,
    ] {
        push_identity_part(&mut material, part)?;
    }
    material.extend_from_slice(&base.number().to_be_bytes());
    let digest = raw_content_sha256(&material).map_err(|_| ApplyError::InvalidOperation)?;
    WireHash::parse(format!("sha256:{}", digest.as_str())).map_err(|_| ApplyError::InvalidOperation)
}

fn governor_event_key(request_identity: &WireHash, purpose: &[u8]) -> Result<OpaqueId, ApplyError> {
    let mut material = Vec::with_capacity(256);
    push_identity_part(&mut material, b"graphhelm-governor-event-key-v1")?;
    push_identity_part(&mut material, request_identity.as_str().as_bytes())?;
    push_identity_part(&mut material, purpose)?;
    let digest = raw_content_sha256(&material).map_err(|_| ApplyError::InvalidOperation)?;
    OpaqueId::parse(format!("gov-{}", digest.as_str())).map_err(|_| ApplyError::InvalidOperation)
}

fn push_identity_part(output: &mut Vec<u8>, part: &[u8]) -> Result<(), ApplyError> {
    let length = u32::try_from(part.len()).map_err(|_| ApplyError::InvalidOperation)?;
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(part);
    Ok(())
}

enum CommittedTerminal {
    Rejected(DraftRejected),
    Applied {
        published: Box<GraphVersionPublished>,
        applied: DraftApplied,
    },
}

struct CommittedOutcome {
    obligations: Vec<PolicyObligationEvaluated>,
    waivers: Vec<PolicyWaiver>,
    terminal: CommittedTerminal,
}

fn invalid_committed_outcome<T>() -> Result<T, ApplyError> {
    Err(ApplyError::Governor(GovernorError::InvalidProjection))
}

fn envelope_matches_new_event(
    event: &EventEnvelope,
    expected: &graphhelm_protocols::NewEvent,
) -> bool {
    event.idempotency_key == expected.idempotency_key
        && event.actor == expected.actor
        && event.sensitivity == expected.sensitivity
        && event.kind == expected.kind
        && event.evidence_refs == expected.evidence_refs
        && event.artifact_refs == expected.artifact_refs
}

fn validate_committed_outcome_grammar(
    draft: &GraphDraft,
    services: &ApplyServices<'_>,
    request_identity: &WireHash,
    events: &[EventEnvelope],
) -> Result<CommittedOutcome, ApplyError> {
    let actor = persisted_actor(&services.actor)?;
    let expected_proposed = proposed(draft, &actor, request_identity)?;
    let Some(first) = events.first() else {
        return invalid_committed_outcome();
    };
    if !envelope_matches_new_event(first, &expected_proposed)
        || events.iter().any(|event| {
            event.scope != services.scope
                || event.stream_id != services.stream_id
                || event.actor != actor
                || event.sensitivity != Sensitivity::Internal
        })
    {
        return invalid_committed_outcome();
    }

    let mut cursor = 1;
    let mut obligations = Vec::new();
    while let Some(event) = events.get(cursor) {
        let EventKind::PolicyObligationEvaluated(payload) = &event.kind else {
            break;
        };
        let expected_key = governor_event_key(
            request_identity,
            format!("obligation:{}", payload.requirement_id.as_str()).as_bytes(),
        )?;
        let expected_reason = match payload.status {
            PersistedObligationStatus::Satisfied => "satisfied",
            PersistedObligationStatus::Unsatisfied => "unsatisfied",
            PersistedObligationStatus::Waived => "waived",
            PersistedObligationStatus::Impossible => "impossible",
        };
        if payload.draft_id.as_str() != draft.id
            || payload.reason_code.as_str() != expected_reason
            || !payload.evidence_ids.is_empty()
            || event.idempotency_key != expected_key
            || !event.evidence_refs.is_empty()
            || !event.artifact_refs.is_empty()
            || obligations.iter().any(|prior: &PolicyObligationEvaluated| {
                prior.requirement_id == payload.requirement_id
            })
        {
            return invalid_committed_outcome();
        }
        obligations.push(payload.clone());
        cursor += 1;
    }

    let mut waivers = Vec::new();
    while let Some(event) = events.get(cursor) {
        let EventKind::PolicyWaiverCreated(payload) = &event.kind else {
            break;
        };
        let expected_key = governor_event_key(
            request_identity,
            format!("waiver:{}", payload.waiver.requirement).as_bytes(),
        )?;
        if event.idempotency_key != expected_key
            || !event.evidence_refs.is_empty()
            || !event.artifact_refs.is_empty()
            || waivers
                .iter()
                .any(|prior: &PolicyWaiver| prior.requirement == payload.waiver.requirement)
        {
            return invalid_committed_outcome();
        }
        waivers.push(payload.waiver.clone());
        cursor += 1;
    }

    let remaining = &events[cursor..];
    let terminal = match remaining {
        [rejected] => {
            let EventKind::DraftRejected(payload) = &rejected.kind else {
                return invalid_committed_outcome();
            };
            let expected_key = governor_event_key(
                request_identity,
                format!("rejected:{}", payload.reason_code.as_str()).as_bytes(),
            )?;
            if rejected.idempotency_key != expected_key
                || !rejected.evidence_refs.is_empty()
                || !rejected.artifact_refs.is_empty()
                || !waivers.is_empty()
            {
                return invalid_committed_outcome();
            }
            CommittedTerminal::Rejected(payload.clone())
        }
        [published, applied] => {
            let (
                EventKind::GraphVersionPublished(published_payload),
                EventKind::DraftApplied(applied_payload),
            ) = (&published.kind, &applied.kind)
            else {
                return invalid_committed_outcome();
            };
            if published.idempotency_key != governor_event_key(request_identity, b"published")?
                || applied.idempotency_key != governor_event_key(request_identity, b"applied")?
                || !applied.evidence_refs.is_empty()
                || !published.artifact_refs.is_empty()
                || !applied.artifact_refs.is_empty()
            {
                return invalid_committed_outcome();
            }
            CommittedTerminal::Applied {
                published: published_payload.clone(),
                applied: applied_payload.clone(),
            }
        }
        _ => return invalid_committed_outcome(),
    };
    Ok(CommittedOutcome {
        obligations,
        waivers,
        terminal,
    })
}

fn recover_committed_apply(
    base: &GraphVersion,
    draft: &GraphDraft,
    services: &ApplyServices<'_>,
    events: Vec<EventEnvelope>,
) -> Result<ApplyResult, ApplyError> {
    let request_identity = governor_request_identity(base, draft, services)?;
    let outcome = validate_committed_outcome_grammar(draft, services, &request_identity, &events)?;
    if let CommittedTerminal::Rejected(rejection) = &outcome.terminal {
        return Err(recover_committed_rejection(
            base,
            draft,
            services,
            &outcome.obligations,
            rejection,
        )?);
    }
    let mut candidate = base.graph().clone();
    apply_operations(&mut candidate, &draft.operations)
        .map_err(|_| ApplyError::InvalidOperation)?;
    candidate.metadata.version = base
        .number()
        .checked_add(1)
        .ok_or(ApplyError::StructuralImpossible)?;
    candidate.metadata.based_on = Some(base.graph().metadata.id.clone());
    preflight_execution_graph(&candidate).map_err(|error| match error {
        graphhelm_graph::DurableContentError::LimitExceeded => {
            ApplyError::Governor(GovernorError::LimitExceeded)
        }
        graphhelm_graph::DurableContentError::Unsafe => ApplyError::InvalidOperation,
    })?;
    let raw = serde_json::to_value(&candidate).map_err(|_| ApplyError::InvalidOperation)?;
    if !graphhelm_schema::validate_graph_value(&raw, "governor-candidate").is_empty() {
        return Err(ApplyError::StructuralImpossible);
    }
    let manual_override = authoritative_override(draft, &services.actor);
    let policy_report = evaluate_transition(base, &candidate, manual_override);
    if policy_report.obligations.iter().any(|obligation| {
        matches!(
            obligation.status,
            ObligationStatus::Impossible | ObligationStatus::Unsatisfied
        )
    }) {
        return Err(ApplyError::Governor(GovernorError::InvalidProjection));
    }
    let expected_obligations = obligation_events(
        draft,
        &policy_report.obligations,
        &persisted_actor(&services.actor)?,
        &request_identity,
    )?;
    let expected_obligations = expected_obligations
        .into_iter()
        .filter_map(|event| match event.kind {
            EventKind::PolicyObligationEvaluated(payload) => Some(payload),
            _ => None,
        })
        .collect::<Vec<_>>();
    if outcome.obligations != expected_obligations {
        return invalid_committed_outcome();
    }
    validate_committed_waivers(
        draft,
        &candidate,
        &services.actor,
        &policy_report,
        &outcome.waivers,
    )?;
    let CommittedTerminal::Applied { published, applied } = &outcome.terminal else {
        return invalid_committed_outcome();
    };
    let version = GraphVersion::publish(
        candidate,
        Some(GraphVersionRef {
            number: base.number(),
            content_hash: base.content_hash().clone(),
        }),
        services.actor.clone(),
        *published.version.created_at().as_datetime(),
    )?;
    let base_safe_hash = safe_semantic_hash_for(&services.scope, &base.to_record())?;
    let predecessor = PersistedGraphVersionRef::new(base.number(), base_safe_hash)
        .map_err(|_| ApplyError::Governor(GovernorError::InvalidProjection))?;
    let expected_projection =
        projected_version_for(&services.scope, &version.to_record(), Some(predecessor))?;
    if published.version != expected_projection {
        return Err(ApplyError::Governor(GovernorError::InvalidProjection));
    }
    if applied.draft_id.as_str() != draft.id
        || applied.graph_version != version.number()
        || applied.graph_hash != *published.version.semantic_hash()
    {
        return Err(ApplyError::Governor(GovernorError::InvalidProjection));
    }
    Ok(ApplyResult {
        version,
        waivers: outcome.waivers,
        events,
        policy_report,
    })
}

fn validate_committed_waivers(
    draft: &GraphDraft,
    candidate: &graphhelm_protocols::ExecutionGraph,
    actor: &Actor,
    policy_report: &PolicyReport,
    waivers: &[PolicyWaiver],
) -> Result<(), ApplyError> {
    let waived = policy_report
        .obligations
        .iter()
        .filter(|obligation| obligation.status == ObligationStatus::Waived)
        .collect::<Vec<_>>();
    let Some(manual_override) = authoritative_override(draft, actor) else {
        return if waived.is_empty() && waivers.is_empty() {
            Ok(())
        } else {
            invalid_committed_outcome()
        };
    };
    if waived.len() != waivers.len()
        || waived.iter().zip(waivers).any(|(obligation, waiver)| {
            waiver.requirement != obligation.requirement
                || waiver.execution_id != candidate.metadata.execution_id
                || waiver.graph_version != candidate.metadata.version
                || waiver.actor != actor.id
                || waiver.reason.as_deref() != Some(manual_override.reason.as_str())
                || waiver.acknowledged_risks != manual_override.acknowledged_risks
                || waiver.scope != manual_override.scope
                || waiver.expires_at.is_some()
        })
    {
        return invalid_committed_outcome();
    }
    Ok(())
}

fn expected_rejection_obligations(
    base: &GraphVersion,
    draft: &GraphDraft,
    services: &ApplyServices<'_>,
    error: &ApplyError,
) -> Result<Vec<PolicyObligation>, ApplyError> {
    match error {
        ApplyError::StaleVersion => {
            if draft.expected_version == base.number() {
                return invalid_committed_outcome();
            }
            return Ok(Vec::new());
        }
        ApplyError::StaleHash => {
            if draft.expected_version != base.number()
                || draft.expected_hash == *base.content_hash()
            {
                return invalid_committed_outcome();
            }
            return Ok(Vec::new());
        }
        _ => {}
    }
    if draft.expected_version != base.number() || draft.expected_hash != *base.content_hash() {
        return invalid_committed_outcome();
    }
    let mut candidate = base.graph().clone();
    if apply_operations(&mut candidate, &draft.operations).is_err() {
        return if matches!(error, ApplyError::InvalidOperation) {
            Ok(Vec::new())
        } else {
            invalid_committed_outcome()
        };
    }
    if matches!(error, ApplyError::InvalidOperation) {
        return invalid_committed_outcome();
    }
    candidate.metadata.version = base
        .number()
        .checked_add(1)
        .ok_or(ApplyError::StructuralImpossible)?;
    candidate.metadata.based_on = Some(base.graph().metadata.id.clone());
    if preflight_execution_graph(&candidate).is_err() {
        return invalid_committed_outcome();
    }
    let raw = serde_json::to_value(&candidate).map_err(|_| ApplyError::InvalidOperation)?;
    if !graphhelm_schema::validate_graph_value(&raw, "governor-candidate").is_empty() {
        return if matches!(error, ApplyError::StructuralImpossible) {
            Ok(Vec::new())
        } else {
            invalid_committed_outcome()
        };
    }
    let policy_report = evaluate_transition(
        base,
        &candidate,
        authoritative_override(draft, &services.actor),
    );
    let expected_error = if policy_report
        .obligations
        .iter()
        .any(|item| item.status == ObligationStatus::Impossible)
    {
        ApplyError::StructuralImpossible
    } else if policy_report
        .obligations
        .iter()
        .any(|item| item.status == ObligationStatus::Unsatisfied)
    {
        ApplyError::OverrideRequired
    } else {
        return invalid_committed_outcome();
    };
    if expected_error.code() != error.code() {
        return invalid_committed_outcome();
    }
    Ok(policy_report.obligations)
}

fn recover_committed_rejection(
    base: &GraphVersion,
    draft: &GraphDraft,
    services: &ApplyServices<'_>,
    committed_obligations: &[PolicyObligationEvaluated],
    rejection: &DraftRejected,
) -> Result<ApplyError, ApplyError> {
    if rejection.draft_id.as_str() != draft.id {
        return Err(ApplyError::Governor(GovernorError::InvalidProjection));
    }
    let error = match rejection.reason_code.as_str() {
        "stale_version" => ApplyError::StaleVersion,
        "stale_hash" => ApplyError::StaleHash,
        "invalid_operation" => ApplyError::InvalidOperation,
        "structural_impossible" => ApplyError::StructuralImpossible,
        "override_required" => ApplyError::OverrideRequired,
        "invalid_waiver" => ApplyError::InvalidWaiver,
        _ => return Err(ApplyError::Governor(GovernorError::InvalidProjection)),
    };
    if rejection.diagnostics.len() != 1
        || rejection.diagnostics[0].code() != error.code()
        || rejection.detail_evidence_id.is_some()
    {
        return Err(ApplyError::Governor(GovernorError::InvalidProjection));
    }
    let expected_obligations = expected_rejection_obligations(base, draft, services, &error)?;
    let request_identity = governor_request_identity(base, draft, services)?;
    let expected_obligations = obligation_events(
        draft,
        &expected_obligations,
        &persisted_actor(&services.actor)?,
        &request_identity,
    )?
    .into_iter()
    .filter_map(|event| match event.kind {
        EventKind::PolicyObligationEvaluated(payload) => Some(payload),
        _ => None,
    })
    .collect::<Vec<_>>();
    if committed_obligations != expected_obligations {
        return invalid_committed_outcome();
    }
    Ok(error)
}

fn opaque(value: &str) -> Result<OpaqueId, ApplyError> {
    OpaqueId::parse(value).map_err(|_| ApplyError::InvalidOperation)
}

fn safe_diagnostic(code: &str) -> Result<PersistedDiagnostic, ApplyError> {
    PersistedDiagnostic::new(
        code.to_owned(),
        Severity::Error,
        DiagnosticDomainPath::parse("").map_err(|_| ApplyError::InvalidOperation)?,
        DiagnosticComponent::Governor,
        None,
        None,
    )
    .map_err(|_| ApplyError::InvalidOperation)
}

const fn reason_code(error: &ApplyError) -> &'static str {
    match error {
        ApplyError::StaleVersion => "stale_version",
        ApplyError::StaleHash => "stale_hash",
        ApplyError::InvalidOperation => "invalid_operation",
        ApplyError::StructuralImpossible => "structural_impossible",
        ApplyError::OverrideRequired => "override_required",
        ApplyError::InvalidWaiver => "invalid_waiver",
        ApplyError::Repository(_) => "repository_failure",
        ApplyError::Governor(_) => "projection_failure",
        ApplyError::Graph(_) => "graph_failure",
    }
}

fn next_sequence(services: &ApplyServices<'_>) -> Result<u64, ApplyError> {
    services
        .event_repository
        .next_sequence(&services.scope, services.stream_id.as_str())
        .map_err(ApplyError::Repository)
}

#[cfg(test)]
mod storage_at_witnesses {
    //! #824: `is_io` was widened to accept `StorageAt`; this is the cell that reddens if it is
    //! narrowed back. A predicate with no witness is a comment.
    use super::*;

    #[test]
    fn is_io_accepts_a_storage_failure_that_names_its_cause() {
        let carried = ApplyError::Repository(EventRepositoryError::StorageAt {
            site: "witness",
            os: Some(33),
        });
        assert!(carried.is_io(), "a StorageAt is still an io failure");
        let bare = ApplyError::Repository(EventRepositoryError::Storage);
        assert!(bare.is_io(), "CONTROL: the bare variant still is");
        // CONTROL in the other direction: the predicate is not `true` for everything.
        let integrity = ApplyError::Repository(EventRepositoryError::Integrity);
        assert!(!integrity.is_io(), "an integrity failure is not io");
    }
}
