use std::collections::{BTreeMap, BTreeSet};

use graphhelm_protocols::Diagnostic;
use semver::Version;
use serde::Serialize;
use serde_json::Value;

use crate::{
    MAX_FILE_BYTES, MAX_RESOURCE_BYTES, MAX_SCHEMAS, SchemaDigest, schema_digest, sort_diagnostics,
};

/// The stable, versioned inventory of schemas in a release.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SchemaCatalog {
    pub format_version: u32,
    pub release_version: Version,
    pub schemas: BTreeMap<String, CatalogEntry>,
}

/// The catalog metadata that attests to one schema resource.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogEntry {
    pub id: String,
    pub document_version: Version,
    pub path: String,
    pub sha256: SchemaDigest,
}

/// Explicit in-memory catalog and schema resources, supplied by an I/O adapter.
#[derive(Clone, Debug, PartialEq)]
pub struct CatalogResources {
    pub catalog_source: String,
    pub catalog: SchemaCatalog,
    pub schemas: BTreeMap<String, Value>,
}

/// A deterministic catalog integrity result.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogReport {
    pub ok: bool,
    pub diagnostics: Vec<Diagnostic>,
}

/// Validates catalog identity, bounded resources, and canonical schema digests.
#[must_use]
pub fn validate_catalog(resources: &CatalogResources) -> CatalogReport {
    let mut diagnostics = validate_catalog_package(&resources.catalog_source, &resources.catalog);
    let source = diagnostic_source(&resources.catalog_source);

    match serde_json::to_vec(&resources.catalog) {
        Ok(bytes) if bytes.len() > MAX_FILE_BYTES => invalid(
            &mut diagnostics,
            "catalog exceeds maximum size",
            "/",
            source,
        ),
        Err(_) => invalid(
            &mut diagnostics,
            "catalog cannot be serialized",
            "/",
            source,
        ),
        Ok(_) => {}
    }

    if resources.catalog.format_version != 1 {
        invalid(
            &mut diagnostics,
            "catalog formatVersion must equal 1",
            "/formatVersion",
            source,
        );
    }
    if resources.catalog.schemas.len() > MAX_SCHEMAS {
        invalid(
            &mut diagnostics,
            "catalog exceeds maximum schema count",
            "/schemas",
            source,
        );
    }

    let mut identities = BTreeSet::new();
    let mut resource_bytes = 0usize;
    for (name, entry) in &resources.catalog.schemas {
        let entry_path = format!("/schemas/{}", escape_pointer(name));
        if !is_schema_name(name) {
            invalid(
                &mut diagnostics,
                "schema name is invalid",
                &entry_path,
                source,
            );
        }
        if !is_schema_path(&entry.path) {
            invalid(
                &mut diagnostics,
                "schema path must be normalized and below schemas/",
                &format!("{entry_path}/path"),
                source,
            );
        }
        let expected_id = format!("https://p50.dev/schemas/{name}.schema.json");
        if entry.id != expected_id {
            invalid(
                &mut diagnostics,
                "schema id does not match the catalog key",
                &format!("{entry_path}/id"),
                source,
            );
        }
        if !identities.insert(&entry.id) {
            invalid(
                &mut diagnostics,
                "schema id must be unique",
                &format!("{entry_path}/id"),
                source,
            );
        }

        let Some(document) = resources.schemas.get(name) else {
            invalid(
                &mut diagnostics,
                "catalog entry has no loaded schema resource",
                &entry_path,
                source,
            );
            continue;
        };
        let resource_unusable = match serde_json::to_vec(document) {
            Ok(bytes) if bytes.len() > MAX_FILE_BYTES => {
                invalid(
                    &mut diagnostics,
                    "schema resource exceeds maximum size",
                    &format!("{entry_path}/bytes"),
                    source,
                );
                true
            }
            Ok(bytes) => {
                resource_bytes = resource_bytes.saturating_add(bytes.len());
                if resource_bytes > MAX_RESOURCE_BYTES {
                    invalid(
                        &mut diagnostics,
                        "schema resources exceed maximum aggregate size",
                        "/schemas",
                        source,
                    );
                }
                false
            }
            Err(_) => {
                invalid(
                    &mut diagnostics,
                    "schema resource cannot be serialized",
                    &entry_path,
                    source,
                );
                true
            }
        };
        if resource_unusable {
            continue;
        }

        if document.get("$id").and_then(Value::as_str) != Some(entry.id.as_str()) {
            invalid(
                &mut diagnostics,
                "schema $id does not match catalog entry",
                &format!("{entry_path}/id"),
                source,
            );
        }
        let document_version = document
            .get("x-graphhelm-schema-version")
            .and_then(Value::as_str)
            .and_then(|value| Version::parse(value).ok());
        if document_version.as_ref() != Some(&entry.document_version) {
            invalid(
                &mut diagnostics,
                "schema version does not match catalog entry",
                &format!("{entry_path}/x-graphhelm-schema-version"),
                source,
            );
        }
        match schema_digest(document) {
            Ok(digest) if digest != entry.sha256 => diagnostics.push(Diagnostic::error(
                "GHC002_HASH_MISMATCH",
                "canonical schema digest does not match catalog entry",
                format!("{entry_path}/sha256"),
                source,
            )),
            Ok(_) => {}
            Err(error) => invalid(&mut diagnostics, &error.to_string(), &entry_path, source),
        }
    }

    for name in resources.schemas.keys() {
        if !resources.catalog.schemas.contains_key(name) {
            invalid(
                &mut diagnostics,
                "loaded schema resource has no catalog entry",
                &format!("/schemas/{}", escape_pointer(name)),
                source,
            );
        }
    }

    sort_diagnostics(&mut diagnostics);
    CatalogReport {
        ok: diagnostics.is_empty(),
        diagnostics,
    }
}

/// Validates the catalog-source package binding without reading schema resources.
#[must_use]
pub fn validate_catalog_package(catalog_source: &str, catalog: &SchemaCatalog) -> Vec<Diagnostic> {
    let source = diagnostic_source(catalog_source);
    let package_directory = if catalog_source == "schemas/catalog.json" {
        Some("schemas".to_owned())
    } else {
        let segments = catalog_source.split('/').collect::<Vec<_>>();
        if let ["schemas", "releases", directory_version, "catalog.json"] = segments.as_slice() {
            match Version::parse(directory_version) {
                Ok(version)
                    if version.pre.is_empty()
                        && version.build.is_empty()
                        && version.to_string() == *directory_version =>
                {
                    if version != catalog.release_version {
                        let mut diagnostics = Vec::new();
                        invalid(
                            &mut diagnostics,
                            "release catalog directory must match releaseVersion",
                            "/releaseVersion",
                            source,
                        );
                        return diagnostics;
                    }
                    Some(format!("schemas/releases/{directory_version}"))
                }
                _ => None,
            }
        } else {
            None
        }
    };

    let Some(package_directory) = package_directory else {
        let mut diagnostics = Vec::new();
        invalid(
            &mut diagnostics,
            "catalog source is not a supported schema package",
            "/",
            source,
        );
        return diagnostics;
    };

    let mut diagnostics = Vec::new();
    for (name, entry) in &catalog.schemas {
        if is_schema_path(&entry.path)
            && entry.path != format!("{package_directory}/{name}.schema.json")
        {
            invalid(
                &mut diagnostics,
                "schema path must belong to the catalog package",
                &format!("/schemas/{}/path", escape_pointer(name)),
                source,
            );
        }
    }
    sort_diagnostics(&mut diagnostics);
    diagnostics
}

fn invalid(diagnostics: &mut Vec<Diagnostic>, message: &str, path: &str, source: &str) {
    diagnostics.push(Diagnostic::error(
        "GHC001_CATALOG_INVALID",
        message,
        path,
        source,
    ));
}

fn is_schema_name(value: &str) -> bool {
    let mut characters = value.bytes();
    matches!(characters.next(), Some(byte) if byte.is_ascii_lowercase())
        && characters.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn is_schema_path(value: &str) -> bool {
    value.starts_with("schemas/")
        && !value.contains('\\')
        && value
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

fn diagnostic_source(value: &str) -> &str {
    let normalized_relative = !value.is_empty()
        && !value.starts_with('/')
        && !value.contains('\\')
        && !value.contains(':')
        && value
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..");
    if normalized_relative {
        value
    } else {
        "schema-evolution"
    }
}

fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}
