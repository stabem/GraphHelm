use graphhelm_protocols::{EventEnvelope, NewEvent};
use thiserror::Error;

/// Append-only event store failures with stable diagnostic codes.
#[derive(Debug, Error)]
pub enum EventStoreError {
    #[error("expected next sequence {expected}, but stream requires {actual}")]
    SequenceConflict { expected: u64, actual: u64 },
    #[error("committed event batch is corrupt: {0}")]
    CorruptBatch(String),
    #[error("event store I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

impl EventStoreError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::SequenceConflict { .. } => "GHE001_SEQUENCE_CONFLICT",
            Self::CorruptBatch(_) => "GHE002_CORRUPT_BATCH",
            Self::Io(_) => "GHE002_CORRUPT_BATCH",
        }
    }
}

/// Ordered atomic append and stream read boundary.
pub trait EventStore: Send + Sync {
    fn append_batch(
        &self,
        stream_id: &str,
        expected_next_sequence: u64,
        events: &[NewEvent],
    ) -> Result<Vec<EventEnvelope>, EventStoreError>;

    fn read_stream(&self, stream_id: &str) -> Result<Vec<EventEnvelope>, EventStoreError>;
}
