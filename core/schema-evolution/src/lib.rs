//! Deterministic schema-evolution primitives.

mod canonical;
mod catalog;
mod compatibility;
mod conformance;
mod limits;
mod migration;
mod reference;
mod release;
mod view;

pub use canonical::{EvolutionError, SchemaDigest, canonical_json, schema_digest};
pub use catalog::{
    CatalogEntry, CatalogReport, CatalogResources, SchemaCatalog, validate_catalog,
    validate_catalog_package,
};
pub use compatibility::{
    CompatibilityChange, CompatibilityClass, CompatibilityReport, SemverImpact, compare_catalogs,
    one_of_branches_are_provably_disjoint,
};
pub use conformance::{
    ConformanceCase, ConformanceCaseMetadata, ConformanceCaseResult, ConformanceExpectation,
    ConformanceReport, ConformanceResources, ConformanceSuite, run_conformance,
};
pub use limits::*;
pub use migration::{
    MigrationCatalogs, MigrationManifest, MigrationResult, PatchOperation, apply_migration,
    plan_migration_chain, validate_migration_manifest,
};
pub use release::{
    CatalogVersionCollision, ChangelogKey, FixturePairKey, MigrationKey, ReleaseEvidence,
    ReleaseReport, catalog_version_collision, enforce_release,
};
pub use view::{CanonicalSchemaView, canonical_view};

use graphhelm_protocols::Diagnostic;

pub fn sort_diagnostics(diagnostics: &mut [Diagnostic]) {
    diagnostics.sort_by(|left, right| {
        (
            &left.source,
            &left.path,
            &left.code,
            &left.severity,
            &left.message,
        )
            .cmp(&(
                &right.source,
                &right.path,
                &right.code,
                &right.severity,
                &right.message,
            ))
    });
}
