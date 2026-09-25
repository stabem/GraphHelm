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
    /// The same failure as `Storage`, carrying WHERE it was raised and WHAT the operating system
    /// said. Same form as [`IntegrityAt`](Self::IntegrityAt), for the same reason (#824).
    ///
    /// `Storage` is produced from 78 sites in `local.rs` and by every `?` on an `io::Result`
    /// through the `From` impl below - and until this variant, ALL of them discarded the cause.
    /// A sharing violation, a lock held by another handle, ENOSPC and a permission denial arrived
    /// as one value with one sentence, so the failure that reddened `runtime_http`'s signal test
    /// under gate load (#824, failure #2) could not be told apart from any other. An instrument
    /// that fails without naming what it saw cannot be diagnosed by re-running it.
    ///
    /// `os` is the RAW OS ERROR CODE, not `io::ErrorKind`: on Windows both a sharing violation
    /// (32) and a lock violation (33) map to `ErrorKind::Uncategorized`, so the kind alone would
    /// erase exactly the distinction this variant exists to keep. A raw code is an integer from
    /// the kernel, never caller input, so it is safe to cross the wire - the `&'static str` site
    /// tag is load-bearing for the same reason `IntegrityAt`'s is: no runtime value can reach it.
    ///
    /// Wire behaviour is IDENTICAL to `Storage`: same `GHE008_STORAGE_FAILURE` code. Only the
    /// human-readable message gains the site and the code, so no consumer keying on the code
    /// can break. TRANSITIONAL like `IntegrityAt`: convert sites in small reviewed batches and
    /// review the TAG TEXT, never sweep - a wrong tag sends the reader after a different fault.
    #[error("repository storage operation failed at {site} (os error {os:?})")]
    StorageAt { site: &'static str, os: Option<i32> },
    /// The read declared a wall-clock budget and the walk outlived it (#750).
    ///
    /// Distinct from `LimitExceeded` on purpose: that one says the input is larger than a
    /// deterministic bound this build refuses to process at all, and it is the same answer on
    /// every machine. This one says a walk that WOULD have completed did not finish inside the
    /// time the caller allowed, which depends on the store, the machine and the build. The
    /// operator's next move differs, so the two must not share a code.
    #[error(
        "read exceeded its {limit_millis} ms budget after walking {walked} events; this store is larger than the read can answer within that budget"
    )]
    ReadBudgetExceeded { walked: u64, limit_millis: i64 },
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
            Self::Storage | Self::StorageAt { .. } => "GHE008_STORAGE_FAILURE",
            Self::ReadBudgetExceeded { .. } => "GHE013_READ_BUDGET_EXCEEDED",
        }
    }
}

impl From<std::io::Error> for EventRepositoryError {
    /// Every `?` on an `io::Result` lands here, so this ONE conversion is where most storage
    /// failures get their cause back (#824). The site tag is honest about what it does not know:
    /// `io` says "propagated by `?`, site not named". A converted `map_err` site names itself.
    fn from(error: std::io::Error) -> Self {
        Self::StorageAt {
            site: "io",
            os: error.raw_os_error(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::EventRepositoryError;

    /// #824: a storage failure NAMES WHAT THE OS SAID, and the wire code does not move.
    ///
    /// The raw code is asserted, not the kind: 32 is Windows ERROR_SHARING_VIOLATION, and the
    /// whole point of carrying the raw value is that `ErrorKind` would have folded it into
    /// `Uncategorized` alongside a lock violation.
    #[test]
    fn an_io_error_propagated_by_question_mark_keeps_its_os_code() {
        let error: EventRepositoryError = std::io::Error::from_raw_os_error(32).into();
        assert_eq!(
            error.code(),
            "GHE008_STORAGE_FAILURE",
            "the wire code must not move"
        );
        let text = error.to_string();
        assert!(text.contains("32"), "the OS code is in the message: {text}");
        assert!(
            text.contains("at io"),
            "the `?` path says it did not name the site: {text}"
        );
    }

    /// CONTROL: the bare variant is untouched, so nothing that matched its text before can drift.
    #[test]
    fn the_bare_storage_variant_is_unchanged() {
        let error = EventRepositoryError::Storage;
        assert_eq!(error.code(), "GHE008_STORAGE_FAILURE");
        assert_eq!(error.to_string(), "repository storage operation failed");
    }

    /// And an error with NO os code (a synthetic io::Error) says so rather than inventing one.
    #[test]
    fn an_io_error_without_an_os_code_reports_none_not_a_sentinel() {
        let error: EventRepositoryError = std::io::Error::other("synthetic").into();
        assert!(error.to_string().contains("None"), "{error}");
    }

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
