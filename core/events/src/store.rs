use thiserror::Error;

/// Stable, redacted local repository failures.
#[derive(Debug, Error)]
pub enum EventRepositoryError {
    #[error("repository sequence conflicts with the append request")]
    SequenceConflict,
    #[error("repository idempotency key conflicts with committed input")]
    IdempotencyConflict,
    #[error("repository integrity verification failed")]
    Integrity,
    /// A stored batch failed its own checksum, as distinct from a broken hash chain.
    #[error("repository batch checksum does not match its contents")]
    CorruptBatch,
    /// A projection generation cannot resume from the watermark it was asked to continue.
    #[error("projection watermark does not match the requested generation")]
    WatermarkMismatch,
    #[error("repository input exceeds a deterministic limit")]
    LimitExceeded,
    #[error("repository format is unsupported")]
    UnsupportedFormat,
    #[error("repository input is invalid")]
    Invalid,
    #[error("repository content is not safe for persistence")]
    UnsafePersistence,
    #[error("repository replay requires an explicit scope and stream selection")]
    StreamSelectionRequired,
    #[error("repository storage operation failed")]
    Storage,
}

impl EventRepositoryError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::SequenceConflict => "GHE001_SEQUENCE_CONFLICT",
            Self::IdempotencyConflict => "GHE003_IDEMPOTENCY_CONFLICT",
            Self::Integrity => "GHE005_INTEGRITY_FAILURE",
            Self::CorruptBatch => "GHE002_CORRUPT_BATCH",
            Self::WatermarkMismatch => "GHPROJ001_WATERMARK_MISMATCH",
            Self::LimitExceeded => "GHE006_LIMIT_EXCEEDED",
            Self::UnsupportedFormat => "GHE007_UNSUPPORTED_FORMAT",
            Self::Invalid => "GHE004_INVALID_EVENT",
            Self::UnsafePersistence => "GHE009_EXTERNALIZATION_FAILED",
            Self::StreamSelectionRequired => "GHE010_STREAM_SELECTION_REQUIRED",
            Self::Storage => "GHE008_STORAGE_FAILURE",
        }
    }
}

impl From<std::io::Error> for EventRepositoryError {
    fn from(_: std::io::Error) -> Self {
        Self::Storage
    }
}
