//! Deterministic Dreams shadow validation and advisory recording.

use graphhelm_events::{
    DreamShadowAppend, EventRepository, EventRepositoryError, prepare_dream_shadow,
};
use graphhelm_protocols::{
    DreamCategory, DreamCriticVerdict, DreamOutcome, DreamRefusalCode, DreamShadowRecorded,
    DreamTaskRequest, DreamTrigger, MAX_DREAM_EVIDENCE, MAX_DREAM_INPUT_BYTES, OpaqueId, RawSha256,
    RepositoryScope, SafeCode,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DreamShadowRequest {
    pub run_id: OpaqueId,
    pub scope: RepositoryScope,
    pub trigger: DreamTrigger,
    pub category: DreamCategory,
    pub input_bytes: usize,
    pub input_sha256: RawSha256,
    pub evidence_sha256: Vec<RawSha256>,
    pub planner_id: OpaqueId,
    pub critic_id: OpaqueId,
    pub critic_verdict: DreamCriticVerdict,
    pub write_critical: bool,
    pub finding_sha256: Option<RawSha256>,
    pub task_id: Option<OpaqueId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DreamRefusal {
    pub code: DreamRefusalCode,
}

impl DreamRefusal {
    #[must_use]
    pub const fn code(&self) -> DreamRefusalCode {
        self.code
    }
}

fn validate(
    request: &DreamShadowRequest,
    expected_scope: &RepositoryScope,
) -> Result<(), DreamRefusal> {
    if request.input_bytes == 0 {
        return Err(DreamRefusal {
            code: DreamRefusalCode::InputEmpty,
        });
    }
    if request.input_bytes > MAX_DREAM_INPUT_BYTES {
        return Err(DreamRefusal {
            code: DreamRefusalCode::InputTooLarge,
        });
    }
    if request.evidence_sha256.is_empty() {
        return Err(DreamRefusal {
            code: DreamRefusalCode::MissingEvidence,
        });
    }
    if request.evidence_sha256.len() > MAX_DREAM_EVIDENCE {
        return Err(DreamRefusal {
            code: DreamRefusalCode::InputTooLarge,
        });
    }
    if &request.scope != expected_scope {
        return Err(DreamRefusal {
            code: DreamRefusalCode::ScopeMismatch,
        });
    }
    if request.write_critical {
        return Err(DreamRefusal {
            code: DreamRefusalCode::WriteCritical,
        });
    }
    if request.planner_id == request.critic_id {
        return Err(DreamRefusal {
            code: DreamRefusalCode::CriticNotIndependent,
        });
    }
    if request.category == DreamCategory::CodeFinding
        && (request.task_id.is_none() || request.finding_sha256.is_none())
    {
        return Err(DreamRefusal {
            code: DreamRefusalCode::TaskMappingMissing,
        });
    }
    Ok(())
}

#[must_use]
pub fn evaluate_dream_shadow(
    request: &DreamShadowRequest,
    expected_scope: &RepositoryScope,
) -> DreamShadowRecorded {
    let validation_code = validate(request, expected_scope)
        .err()
        .map(|error| error.code);
    let critic_verdict = if validation_code.is_some() {
        DreamCriticVerdict::Rejected
    } else {
        request.critic_verdict
    };
    let validation_code = validation_code.or_else(|| {
        (critic_verdict == DreamCriticVerdict::Rejected).then_some(DreamRefusalCode::CriticRejected)
    });
    let outcome = if validation_code.is_none() && critic_verdict == DreamCriticVerdict::Accepted {
        DreamOutcome::AdvisoryProposal
    } else {
        DreamOutcome::Discarded
    };
    let task_request = (outcome == DreamOutcome::AdvisoryProposal
        && request.category == DreamCategory::CodeFinding)
        .then(|| DreamTaskRequest {
            task_id: request
                .task_id
                .clone()
                .expect("validated code finding has a task id"),
            dream_run_id: request.run_id.clone(),
            scope: request.scope.clone(),
            finding_sha256: request
                .finding_sha256
                .clone()
                .expect("validated code finding has a finding digest"),
            evidence_sha256: request.evidence_sha256.clone(),
            origin: SafeCode::parse("dream_generated").expect("constant task origin is valid"),
        });
    DreamShadowRecorded {
        run_id: request.run_id.clone(),
        scope: request.scope.clone(),
        trigger: request.trigger,
        category: request.category,
        input_sha256: request.input_sha256.clone(),
        evidence_sha256: request.evidence_sha256.clone(),
        outcome,
        validation_code,
        planner_id: request.planner_id.clone(),
        critic_id: request.critic_id.clone(),
        critic_verdict,
        task_request,
    }
}

pub fn record_dream_shadow(
    repository: &dyn EventRepository,
    scope: RepositoryScope,
    stream_id: OpaqueId,
    expected_next_sequence: u64,
    idempotency_key: OpaqueId,
    actor: graphhelm_protocols::PersistedActor,
    result: DreamShadowRecorded,
) -> Result<Vec<graphhelm_protocols::EventEnvelope>, EventRepositoryError> {
    let prepared = prepare_dream_shadow(DreamShadowAppend::new(
        scope,
        stream_id,
        expected_next_sequence,
        idempotency_key,
        actor,
        result,
    ))?;
    repository.append_atomic(&prepared)
}
