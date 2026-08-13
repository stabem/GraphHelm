//! Deterministic graph semantics.

mod canonical;
mod lint;
mod persistence;
mod version;

pub use canonical::{CanonicalGraph, GraphError, canonicalize, semantic_hash};
pub use lint::{LintReport, lint};
pub use persistence::{
    ContentSlotProfile, DurableContentError, MAX_DRAFT_OPERATIONS, PersistedReferenceDomain,
    PersistenceHashes, PersistencePreflight, canonical_content_bytes, decode_persisted_reference,
    derive_content_slot_id, derive_content_slot_profile, derive_publication_evidence_id,
    encode_persisted_reference, is_valid_isolation_tier, parse_persisted_binding,
    parse_persisted_binding_reference_node, parse_persisted_nominal_identifier, persisted_hashes,
    persisted_node_control_order, persisted_reference_domain, preflight_execution_graph,
    preflight_graph_version_record_values, preflight_persistence_values, raw_content_sha256,
    validate_context_include_reference, validate_durable_content, validate_evidence_bijection,
    validate_persisted_control_references, validate_persisted_projection,
    validate_persisted_references, validate_publication_evidence_ids,
};
pub use version::GraphVersion;
