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
    /// The same failure as `Integrity`, carrying WHICH check raised it.
    ///
    /// `Integrity` is raised from 205 sites across this workspace and the postgres adapter, and it
    /// carries no discriminator — so a live failure produces one message for two hundred possible
    /// causes, and no amount of re-running separates them. That is what made #311 undiagnosable
    /// through the events read path, and it is the same shape #261 fixed one layer up.
    ///
    /// TRANSITIONAL, and saying so is the point: the end state is a site tag on `Integrity` itself,
    /// which is a 205-site mechanical change across two backends and belongs to its own decision.
    /// This variant carries the tag for the sites that have been converted, so the conversion can
    /// proceed incrementally instead of as one large diff nobody can review.
    ///
    /// Wire behaviour is deliberately IDENTICAL: same `GHE005_INTEGRITY_FAILURE` code. Only the
    /// human-readable message gains the site, so no consumer keying on the code can break.
    ///
    /// `&'static str` and not `String`, and this is load-bearing rather than a micro-optimisation:
    /// the type makes it IMPOSSIBLE for a caller's input to reach this message. A site tag is a
    /// compile-time literal naming a check in this file; if it could carry a runtime value, the
    /// next person to convert a site could put a path, an id, or an operator's payload into an
    /// error that crosses the wire. Copy this form when converting the remaining sites.
    ///
    /// AND THE TAG TEXT IS NOT MACHINE-CHECKABLE. No guard can assert that a literal DESCRIBES the
    /// check it labels - that is the one part only human review carries. Measured base rate on the
    /// first batch: 1 of 4 tags was wrong, and wrong confidently (`journal-identity` on a check
    /// that tests for an unterminated tail). A wrong tag is worse than no tag: absence says
    /// nothing, while a wrong one sends the reader after a different fault with a different remedy.
    /// Convert in small batches and review the TEXT, never sweep.
    #[error("repository integrity verification failed at {0}")]
    IntegrityAt(&'static str),
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
            Self::Integrity | Self::IntegrityAt(_) => "GHE005_INTEGRITY_FAILURE",
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

#[cfg(test)]
mod tests {
    use super::EventRepositoryError;

    /// The whole point of `IntegrityAt`: the site must reach the reader.
    ///
    /// `repository_failure` in the CLI builds the wire message with `error.to_string()`, so this
    /// Display IS the HTTP body. If the site stops appearing here, a live integrity failure goes
    /// back to naming two hundred possible causes with one sentence - which is the state that made
    /// #311 undiagnosable in the first place.
    #[test]
    fn an_integrity_failure_names_the_check_that_raised_it() {
        let error = EventRepositoryError::IntegrityAt("open:root-identity-after-lock");
        let rendered = error.to_string();
        assert!(
            rendered.contains("open:root-identity-after-lock"),
            "the site must survive into the message that reaches the wire: {rendered}"
        );
    }

    /// The wire CODE must not move. Consumers key on the code, not the prose, so gaining a site
    /// must be additive: same code, richer message. A change here breaks anyone matching GHE005.
    #[test]
    fn gaining_a_site_does_not_move_the_wire_code() {
        assert_eq!(
            EventRepositoryError::IntegrityAt("anything").code(),
            EventRepositoryError::Integrity.code(),
            "IntegrityAt is the same failure with more detail, not a different one"
        );
        assert_eq!(
            EventRepositoryError::IntegrityAt("anything").code(),
            "GHE005_INTEGRITY_FAILURE"
        );
    }
}
