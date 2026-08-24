//! Offline schema validation.

mod bounded_value;
mod document;
mod extension;
mod registry;

pub use document::{LoadedExtension, LoadedGraph, load_extension, load_graph};
pub use extension::{
    __surface_allowlists_for_testing, ValidatedExtensionPackage, validate_extension_package,
};
pub use registry::{
    InlineSchemaError, OfflineSchemaSet, RepositorySchemaSet, compile_inline_schema,
    repository_schema_set, validate_agent_value, validate_event_envelope, validate_extension_value,
    validate_graph_value, validate_inline_value, validate_physical_batch, validate_waiver,
};
