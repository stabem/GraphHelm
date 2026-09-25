//! Offline schema validation.

mod bounded_value;
mod document;
mod extension;
mod registry;

pub use document::{
    LoadedExtension, LoadedGraph, NOT_A_REGULAR_FILE, load_extension, load_graph, load_graph_json,
};
pub use extension::{
    __surface_allowlists_for_testing, ValidatedExtensionPackage, validate_extension_package,
};
pub use registry::{
    InlineSchemaError, OfflineSchemaSet, RepositorySchemaSet, ValidationAttempt, ValidationRefusal,
    compile_inline_schema, repository_schema_set, validate_agent_value, validate_event_envelope,
    validate_extension_value, validate_graph_value, validate_inline_value, validate_physical_batch,
    validate_waiver,
};

pub use registry::validate_activation_receipt;
pub use registry::{validate_adoption_journal, validate_adoption_plan, validate_adoption_receipt};

pub use registry::validate_restore_plan;
