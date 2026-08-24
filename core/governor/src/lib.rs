//! Transactional graph governance.

mod apply;
mod candidate;
mod externalize;
mod inflight;
mod materialize;
mod memory;
mod publish;

pub use apply::{ApplyError, ApplyResult, ApplyServices, apply_draft};
pub use candidate::{DraftAnalysis, analyze_draft};
pub use externalize::{
    GovernorError, GraphExternalizer, ProjectionPreparation, SealingGraphExternalizer,
};
pub use inflight::{
    AdmittedSignal, GovernanceError, MutationDecision, RejectionReason, admit_signal,
    decide_mutation, override_with_waiver,
};
pub use materialize::{
    ExecutableGraphMaterializer, MaterializationError, MaterializedContent, MaterializedGraph,
    MaterializedValue,
};
pub use memory::{
    CaptureOptIn, CaptureTouch, EvidenceBinding, MemoryCandidate, MemoryField, MemoryOrigin,
    MemoryRecord, MemoryRefusal, MemoryRefusalCode, MemoryState, MemoryTransition, PublicationStep,
    admit_memory_candidate, apply_transition, bind_evidence, capture_memory,
    check_dependency_freshness, publication_steps, republish, validate_candidate,
};
pub use publish::{
    PublicationPreparationObserver, PublicationPreparationServices, PublicationStage,
    prepare_draft_publication, prepare_draft_publication_observed,
};
