use std::collections::BTreeSet;

use graphhelm_protocols::Diagnostic;
use semver::Version;
use serde::Serialize;
use serde_json::Value;

use crate::{
    CatalogResources, SchemaDigest, canonical_json, reference::resolved_schema_references,
    validate_catalog,
};

/// A deterministic, generated-only view of one catalog schema.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalSchemaView {
    pub name: String,
    pub schema_id: String,
    pub catalog_format_version: u32,
    pub catalog_release_version: Version,
    pub document_version: Version,
    pub sha256: SchemaDigest,
    pub dependencies: Vec<String>,
    pub schema: Value,
}

/// Generates a payload-safe canonical schema view from explicit in-memory resources.
pub fn canonical_view(
    resources: &CatalogResources,
    schema: &str,
) -> Result<CanonicalSchemaView, Vec<Diagnostic>> {
    let report = validate_catalog(resources);
    if !report.ok {
        return Err(redact_catalog_diagnostics(report.diagnostics));
    }
    let Some(entry) = resources.catalog.schemas.get(schema) else {
        return Err(vec![Diagnostic::error(
            "GHC001_CATALOG_INVALID",
            "schema is absent from catalog resources",
            "/schemas",
            "schema-evolution",
        )]);
    };
    let Some(document) = resources.schemas.get(schema) else {
        return Err(vec![Diagnostic::error(
            "GHC001_CATALOG_INVALID",
            "schema resource is absent from catalog resources",
            "/schemas",
            "schema-evolution",
        )]);
    };
    let references =
        resolved_schema_references(resources, schema, document).map_err(|pointers| {
            pointers
                .into_iter()
                .map(|pointer| {
                    Diagnostic::error(
                        "GHC001_CATALOG_INVALID",
                        "schema reference is unresolved",
                        pointer,
                        "schema-evolution",
                    )
                })
                .collect::<Vec<_>>()
        })?;
    let canonical = canonical_json(document).map_err(|error| vec![error.diagnostic()])?;
    let normalized = serde_json::from_slice(&canonical).map_err(|_| {
        vec![Diagnostic::error(
            "GHC001_CATALOG_INVALID",
            "canonical schema JSON cannot be decoded",
            "/schemas",
            "schema-evolution",
        )]
    })?;

    Ok(CanonicalSchemaView {
        name: schema.to_owned(),
        schema_id: entry.id.clone(),
        catalog_format_version: resources.catalog.format_version,
        catalog_release_version: resources.catalog.release_version.clone(),
        document_version: entry.document_version.clone(),
        sha256: entry.sha256.clone(),
        dependencies: references
            .into_iter()
            .filter_map(|reference| reference.split('#').next().map(str::to_owned))
            .filter(|reference| reference != &entry.id)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        schema: normalized,
    })
}

fn redact_catalog_diagnostics(mut diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
    for diagnostic in &mut diagnostics {
        diagnostic.source = "schema-view".into();
    }
    diagnostics
}
