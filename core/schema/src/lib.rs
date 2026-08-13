//! Offline schema validation.

mod document;
mod registry;

pub use document::{LoadedGraph, load_graph};
pub use registry::{
    InlineSchemaError, OfflineSchemaSet, RepositorySchemaSet, compile_inline_schema,
    repository_schema_set, validate_event_envelope, validate_graph_value, validate_physical_batch,
    validate_waiver,
};
