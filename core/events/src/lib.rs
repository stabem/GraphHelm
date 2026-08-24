//! Append-only event storage and replay.

mod artifact;
mod canonical;
mod evidence;
mod integrity;
mod jsonl;
mod key;
mod limits;
mod local;
mod projection;
mod repository;
mod retention;
mod store;

pub use artifact::{ArtifactRegistration, ArtifactRegistrationError, validate_artifact_reference};
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
pub use local::{LocalEventRepository, LocalFailpoint, journal_line_roundtrips};
pub use projection::{
    ClearanceOutcome, CustomsScan, CustomsStage, EvidenceAvailability, ExecutionProjection,
    MAX_PROJECTION_NODES, OpenClaim, OpenWait, OverdueStage, ProjectionGeneration,
    ProjectionRebuildRequest, ProjectionRebuilder, ProjectionRepository, ProjectionWatermark,
    ReplayError, overdue_at, replay,
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
