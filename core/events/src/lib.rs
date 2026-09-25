//! Append-only event storage and replay.

mod artifact;
mod budget;
mod canonical;
mod customs;
mod evidence;
mod integrity;
mod jsonl;
mod key;
mod limits;
mod local;
mod memory;
mod projection;
mod repository;
mod retention;
mod store;
mod sweep;

pub use artifact::{ArtifactRegistration, ArtifactRegistrationError, validate_artifact_reference};
pub use budget::{READ_BUDGET_CHECK_INTERVAL, ReadBudget, ReadBudgetExceeded};
pub use customs::{ClaimError, ClaimOutcome, ClaimRequest, ClearError, claim, clear, refusal};
pub use evidence::{
    EvidenceError, EvidenceInput, EvidenceOpener, EvidenceProtector, EvidenceSealer,
    MAX_EVIDENCE_ITEMS_PER_BATCH, SealedEvidence, SecretBytes,
};
pub use integrity::{
    ActiveGraphIdentity, CommittedArtifact, compute_event_hash, is_graph_successor,
    request_digest as prepared_append_digest, validate_artifact_relations, validate_envelope,
    validate_envelope_content, validate_evidence_references, validate_graph_lineage,
    validate_prepared_append, validate_sealed_evidence,
};
pub use key::{
    AuthenticateRequest, AuthenticationTag, KeyError, KeyProvider, KeyProviderMetadata,
    RepositoryFuture, RevocationReceipt, RevokeKeyRequest, VerifyAuthenticationRequest,
    WrapKeyRequest, WrappedKey,
};
pub use limits::STATUS_READ_BUDGET_MILLIS;
pub use local::{
    LocalEventRepository, LocalFailpoint, LocalRepositoryInspection, ReadLockHeld,
    journal_line_roundtrips, with_repository_read_lock,
};
pub use memory::{
    MemoryAdmissionRefusalAppend, MemoryPublicationTransitionAppend, MemoryRecordSupersededAppend,
    prepare_memory_admission_refusal, prepare_memory_publication_transition,
    prepare_memory_record_superseded,
};
pub use projection::{
    ClearanceOutcome, CustomsScan, CustomsStage, EvidenceAvailability, ExecutionProjection,
    MAX_PROJECTION_NODES, MemoryAdmissionRefusalReceipt, MemoryRecordProjection, OpenClaim,
    OpenWait, OverdueStage, ProjectionGeneration, ProjectionRebuildRequest, ProjectionRebuilder,
    ProjectionRepository, ProjectionWatermark, ReplayError, WakeMisBurn, claim_evidence_digest,
    overdue_at, project_customs_stage, replay, replay_within,
};
pub use repository::{
    ActiveVersion, ArtifactCatalog, AsyncEventRepository, AuthenticatedCheckpoint, EventPage,
    EventRepository, EvidenceRead, EvidenceRepository, EvidenceUnavailableReason, IntegrityReport,
    PreparedAppend, ReadStart, ReadStreamRequest, RepositoryStream, StreamHead, VerifyRangeRequest,
};
pub use retention::{
    CleanupReceipt, CleanupRequest, EvidencePriorAvailability, FinalizedRetention, LegalHoldChange,
    LegalHoldReceipt, PreparedRetention, PreparedRetentionTarget, RetentionAuthority,
    RetentionBlockReason, RetentionClock, RetentionError, RetentionPlan, RetentionPlanTarget,
    RetentionPolicy, RetentionPrepareOutcome, RetentionRepository, RetentionRequest,
    RetentionService, RetentionTarget, cleanup_request_digest, finalized_authentication_bytes,
    legal_hold_authentication_bytes, prepared_authentication_bytes,
    provider_revocation_idempotency_key, retention_authority_authentication_bytes,
    retention_request_digest, revocation_receipt_authentication_bytes,
};
pub use store::EventRepositoryError;
pub use sweep::sweep;
