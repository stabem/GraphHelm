//! Deterministic graph semantics.

mod canonical;
mod lint;
mod version;

pub use canonical::{CanonicalGraph, GraphError, canonicalize, semantic_hash};
pub use lint::{LintReport, lint};
pub use version::GraphVersion;
