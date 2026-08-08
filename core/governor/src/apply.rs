use graphhelm_events::{EventStore, EventStoreError};
use graphhelm_graph::{GraphError, GraphVersion};
use graphhelm_policy::evaluate_transition;
use graphhelm_protocols::{
    Actor, Clock, DraftApplied, DraftProposed, DraftRejected, EventEnvelope, EventKind, GraphDraft,
    GraphVersionPublished, GraphVersionRef, IdGenerator, NewEvent, ObligationStatus,
    PolicyObligation, PolicyObligationEvaluated, PolicyReport, PolicyWaiver, PolicyWaiverCreated,
};
use thiserror::Error;

use crate::candidate::apply_operations;

pub struct ApplyServices<'a> {
    pub event_store: &'a dyn EventStore,
    pub stream_id: &'a str,
    pub actor: Actor,
    pub clock: &'a dyn Clock,
    pub ids: &'a dyn IdGenerator,
}

#[derive(Clone, Debug)]
pub struct ApplyResult {
    pub version: GraphVersion,
    pub waivers: Vec<PolicyWaiver>,
    pub events: Vec<EventEnvelope>,
    pub policy_report: PolicyReport,
}

#[derive(Debug, Error)]
pub enum ApplyError {
    #[error("draft expected graph version is stale")]
    StaleVersion,
    #[error("draft expected semantic hash is stale")]
    StaleHash,
    #[error("draft operation is invalid: {0}")]
    InvalidOperation(String),
    #[error("candidate is structurally impossible")]
    StructuralImpossible,
    #[error("candidate requires a complete owner override")]
    OverrideRequired,
    #[error("generated waiver failed schema validation")]
    InvalidWaiver,
    #[error(transparent)]
    EventStore(#[from] EventStoreError),
    #[error(transparent)]
    Graph(#[from] GraphError),
}

impl ApplyError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::StaleVersion => "GHD001_STALE_VERSION",
            Self::StaleHash => "GHD002_STALE_HASH",
            Self::InvalidOperation(_) => "GHD003_OPERATION_INVALID",
            Self::StructuralImpossible | Self::InvalidWaiver => "GHP001_STRUCTURAL_IMPOSSIBILITY",
            Self::OverrideRequired => "GHP002_OVERRIDE_REQUIRED",
            Self::EventStore(error) => error.code(),
            Self::Graph(_) => "GHD003_OPERATION_INVALID",
        }
    }

    #[must_use]
    pub const fn is_io(&self) -> bool {
        matches!(self, Self::EventStore(EventStoreError::Io(_)))
    }
}

pub fn apply_draft(
    base: &GraphVersion,
    draft: &GraphDraft,
    services: &ApplyServices<'_>,
) -> Result<ApplyResult, ApplyError> {
    let existing = services.event_store.read_stream(services.stream_id)?;
    let expected_sequence = existing.last().map_or(1, |event| event.sequence + 1);
    if let Some(active) = existing.iter().rev().find_map(|event| match &event.kind {
        EventKind::GraphVersionPublished(payload) => Some(&payload.version),
        _ => None,
    }) {
        if active.graph.metadata.version != base.number() {
            return reject(
                services,
                draft,
                &[],
                expected_sequence,
                ApplyError::StaleVersion,
            );
        }
        if &active.content_hash != base.content_hash() {
            return reject(
                services,
                draft,
                &[],
                expected_sequence,
                ApplyError::StaleHash,
            );
        }
    }
    if draft.expected_version != base.number() {
        return reject(
            services,
            draft,
            &[],
            expected_sequence,
            ApplyError::StaleVersion,
        );
    }
    if &draft.expected_hash != base.content_hash() {
        return reject(
            services,
            draft,
            &[],
            expected_sequence,
            ApplyError::StaleHash,
        );
    }

    let mut candidate = base.graph().clone();
    if let Err(message) = apply_operations(&mut candidate, &draft.operations) {
        return reject(
            services,
            draft,
            &[],
            expected_sequence,
            ApplyError::InvalidOperation(message),
        );
    }
    candidate.metadata.version = base.number() + 1;
    candidate.metadata.based_on = Some(base.graph().metadata.id.clone());

    let raw = serde_json::to_value(&candidate)
        .map_err(|error| ApplyError::InvalidOperation(error.to_string()))?;
    if !graphhelm_schema::validate_graph_value(&raw, &draft.id).is_empty() {
        return reject(
            services,
            draft,
            &[],
            expected_sequence,
            ApplyError::StructuralImpossible,
        );
    }
    let manual_override = draft
        .manual_override
        .as_ref()
        .filter(|request| services.actor.is_owner() && request.actor == services.actor);
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
    let mut pending = vec![proposed(draft)];
    pending.extend(obligation_events(draft, &policy_report.obligations));
    pending.extend(waivers.iter().map(|waiver| NewEvent {
        idempotency_key: format!("{}:waiver:{}", draft.id, waiver.requirement),
        kind: EventKind::PolicyWaiverCreated(PolicyWaiverCreated {
            waiver: waiver.clone(),
        }),
    }));
    pending.push(NewEvent {
        idempotency_key: format!("{}:published", draft.id),
        kind: EventKind::GraphVersionPublished(Box::new(GraphVersionPublished {
            version: version.to_record(),
        })),
    });
    pending.push(NewEvent {
        idempotency_key: format!("{}:applied", draft.id),
        kind: EventKind::DraftApplied(DraftApplied {
            draft_id: draft.id.clone(),
            graph_version: version.number(),
        }),
    });
    let events =
        services
            .event_store
            .append_batch(services.stream_id, expected_sequence, &pending)?;
    Ok(ApplyResult {
        version,
        waivers,
        events,
        policy_report,
    })
}

fn create_waivers(
    candidate: &graphhelm_protocols::ExecutionGraph,
    manual_override: Option<&graphhelm_protocols::ManualOverride>,
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
    obligations: &[graphhelm_protocols::PolicyObligation],
    expected_sequence: u64,
    error: ApplyError,
) -> Result<T, ApplyError> {
    let mut pending = vec![proposed(draft)];
    pending.extend(obligation_events(draft, obligations));
    pending.push(NewEvent {
        idempotency_key: format!("{}:rejected:{}", draft.id, error.code()),
        kind: EventKind::DraftRejected(DraftRejected {
            draft_id: draft.id.clone(),
            reason: error.to_string(),
            diagnostics: Vec::new(),
        }),
    });
    services
        .event_store
        .append_batch(services.stream_id, expected_sequence, &pending)?;
    Err(error)
}

fn proposed(draft: &GraphDraft) -> NewEvent {
    NewEvent {
        idempotency_key: format!("{}:proposed", draft.id),
        kind: EventKind::DraftProposed(DraftProposed {
            draft_id: draft.id.clone(),
            expected_version: draft.expected_version,
            expected_hash: draft.expected_hash.clone(),
            operation_count: draft.operations.len(),
        }),
    }
}

fn obligation_events(draft: &GraphDraft, obligations: &[PolicyObligation]) -> Vec<NewEvent> {
    obligations
        .iter()
        .map(|obligation| NewEvent {
            idempotency_key: format!("{}:obligation:{}", draft.id, obligation.requirement),
            kind: EventKind::PolicyObligationEvaluated(PolicyObligationEvaluated {
                draft_id: draft.id.clone(),
                obligation: obligation.clone(),
            }),
        })
        .collect()
}
