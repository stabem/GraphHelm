//! Append-only event storage and replay.

mod jsonl;
mod projection;
mod store;

pub use jsonl::JsonlEventStore;
pub use projection::{ExecutionProjection, ReplayError, replay};
pub use store::{EventStore, EventStoreError};
