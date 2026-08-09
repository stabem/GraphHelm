use std::collections::BTreeMap;

use graphhelm_protocols::Diagnostic;
use jsonschema::{Draft, Registry, Validator};

const GRAPH_SCHEMA: &str = include_str!("../../../schemas/graph.schema.json");
const NODE_SCHEMA: &str = include_str!("../../../schemas/node.schema.json");
const EDGE_SCHEMA: &str = include_str!("../../../schemas/edge.schema.json");
const AGENT_SCHEMA: &str = include_str!("../../../schemas/agent.schema.json");
const WAIVER_SCHEMA: &str = include_str!("../../../schemas/policy-waiver.schema.json");

const GRAPH_ID: &str = "https://p50.dev/schemas/graph.schema.json";
const NODE_ID: &str = "https://p50.dev/schemas/node.schema.json";
const EDGE_ID: &str = "https://p50.dev/schemas/edge.schema.json";
const AGENT_ID: &str = "https://p50.dev/schemas/agent.schema.json";
const WAIVER_ID: &str = "https://p50.dev/schemas/policy-waiver.schema.json";
const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;
const MAX_RESOURCE_BYTES: usize = 32 * 1024 * 1024;
const MAX_SCHEMAS: usize = 256;
const MAX_JSON_DEPTH: usize = 128;

/// A schema registry compiled solely from explicitly supplied in-memory resources.
pub struct OfflineSchemaSet {
    validators: BTreeMap<String, Validator>,
}

impl OfflineSchemaSet {
    /// Compiles all explicitly supplied schemas without filesystem or network retrieval.
    pub fn compile(
        resources: BTreeMap<String, serde_json::Value>,
    ) -> Result<Self, Vec<Diagnostic>> {
        if resources.len() > MAX_SCHEMAS {
            return Err(vec![compile_error(
                "schema set exceeds maximum resource count",
            )]);
        }

        let mut schemas = BTreeMap::new();
        let mut resource_bytes = 0usize;
        for (index, (_, document)) in resources.into_iter().enumerate() {
            let resource_path = format!("/resources/{index}");
            if exceeds_json_depth(&document, 0) {
                return Err(vec![compile_error_at(
                    "schema resource exceeds maximum JSON depth",
                    &format!("{resource_path}/depth"),
                )]);
            }
            let bytes = serde_json::to_vec(&document)
                .map_err(|_| vec![compile_error("schema resource cannot be serialized")])?;
            if bytes.len() > MAX_FILE_BYTES {
                return Err(vec![compile_error_at(
                    "schema resource exceeds maximum size",
                    &format!("{resource_path}/bytes"),
                )]);
            }
            resource_bytes = resource_bytes.saturating_add(bytes.len());
            if resource_bytes > MAX_RESOURCE_BYTES {
                return Err(vec![compile_error(
                    "schema resources exceed maximum aggregate size",
                )]);
            }
            let Some(id) = document.get("$id").and_then(serde_json::Value::as_str) else {
                return Err(vec![compile_error(
                    "schema resource must declare a string $id",
                )]);
            };
            if schemas.insert(id.to_owned(), document).is_some() {
                return Err(vec![compile_error("schema resource $id must be unique")]);
            }
        }

        let mut builder = Registry::new().draft(Draft::Draft202012);
        for (id, schema) in &schemas {
            builder = builder
                .add(id, schema)
                .map_err(|_| vec![compile_error("offline schema registry is invalid")])?;
        }
        let registry = builder
            .prepare()
            .map_err(|_| vec![compile_error("offline schema reference cannot be resolved")])?;

        let mut validators = BTreeMap::new();
        for (id, schema) in &schemas {
            let validator = jsonschema::options()
                .with_draft(Draft::Draft202012)
                .with_registry(&registry)
                .should_validate_formats(true)
                .build(schema)
                .map_err(|_| vec![compile_error("offline root schema is invalid")])?;
            validators.insert(id.clone(), validator);
        }
        Ok(Self { validators })
    }

    /// Validates a document using a root schema already compiled into this set.
    #[must_use]
    pub fn validate(
        &self,
        schema_id: &str,
        document: &serde_json::Value,
        source: &str,
    ) -> Vec<Diagnostic> {
        let Some(validator) = self.validators.get(schema_id) else {
            return vec![Diagnostic::error(
                "GHS002_SCHEMA",
                "root schema is not registered in the offline schema set",
                "/",
                source,
            )];
        };
        let mut diagnostics: Vec<_> = validator
            .iter_errors(document)
            .map(|error| {
                let path = error.instance_path().as_str();
                Diagnostic::error(
                    "GHS002_SCHEMA",
                    validation_message(error.kind()),
                    if path.is_empty() { "/" } else { path },
                    source,
                )
            })
            .collect();
        diagnostics.sort_by(|left, right| {
            (&left.path, &left.code, &left.message).cmp(&(&right.path, &right.code, &right.message))
        });
        diagnostics
    }
}

fn validation_message(kind: &jsonschema::error::ValidationErrorKind) -> &'static str {
    use jsonschema::error::ValidationErrorKind;

    match kind {
        ValidationErrorKind::AdditionalItems { .. } => {
            "document contains items that are not allowed"
        }
        ValidationErrorKind::AdditionalProperties { .. }
        | ValidationErrorKind::UnevaluatedProperties { .. } => {
            "document contains properties that are not allowed"
        }
        ValidationErrorKind::AnyOf { .. } => "document does not satisfy any allowed schema",
        ValidationErrorKind::BacktrackLimitExceeded { .. }
        | ValidationErrorKind::RegexEngineFailure { .. }
        | ValidationErrorKind::Pattern { .. } => "document does not satisfy the pattern constraint",
        ValidationErrorKind::Constant { .. } => "document does not match the required constant",
        ValidationErrorKind::Contains => "document does not contain a required matching item",
        ValidationErrorKind::ContentEncoding { .. } | ValidationErrorKind::FromUtf8 { .. } => {
            "document does not satisfy the content encoding constraint"
        }
        ValidationErrorKind::ContentMediaType { .. } => {
            "document does not satisfy the content media type constraint"
        }
        ValidationErrorKind::Custom { .. } => "document does not satisfy a schema constraint",
        ValidationErrorKind::Enum { .. } => "document is not one of the allowed values",
        ValidationErrorKind::ExclusiveMaximum { .. } => "document exceeds the exclusive maximum",
        ValidationErrorKind::ExclusiveMinimum { .. } => "document is below the exclusive minimum",
        ValidationErrorKind::FalseSchema => "document is rejected by the schema",
        ValidationErrorKind::Format { .. } => "document has an invalid format",
        ValidationErrorKind::MaxItems { .. } => "document exceeds the maximum item count",
        ValidationErrorKind::Maximum { .. } => "document exceeds the maximum value",
        ValidationErrorKind::MaxLength { .. } => "document exceeds the maximum length",
        ValidationErrorKind::MaxProperties { .. } => "document exceeds the maximum property count",
        ValidationErrorKind::MinItems { .. } => "document is below the minimum item count",
        ValidationErrorKind::Minimum { .. } => "document is below the minimum value",
        ValidationErrorKind::MinLength { .. } => "document is below the minimum length",
        ValidationErrorKind::MinProperties { .. } => "document is below the minimum property count",
        ValidationErrorKind::MultipleOf { .. } => {
            "document does not satisfy the multipleOf constraint"
        }
        ValidationErrorKind::Not { .. } => "document matches a prohibited schema",
        ValidationErrorKind::OneOfMultipleValid { .. } => {
            "document satisfies more than one exclusive schema"
        }
        ValidationErrorKind::OneOfNotValid { .. } => {
            "document does not satisfy exactly one required schema"
        }
        ValidationErrorKind::PropertyNames { .. } => "document contains an invalid property name",
        ValidationErrorKind::Required { .. } => "document is missing a required property",
        ValidationErrorKind::Type { .. } => "document has an invalid type",
        ValidationErrorKind::UnevaluatedItems { .. } => {
            "document contains unevaluated items that are not allowed"
        }
        ValidationErrorKind::UniqueItems => "document contains duplicate array items",
        ValidationErrorKind::Referencing(_) => "document does not satisfy a schema reference",
    }
}

/// Validates a raw graph using only embedded checked-in schema resources.
#[must_use]
pub fn validate_graph_value(value: &serde_json::Value, source: &str) -> Vec<Diagnostic> {
    validate_embedded(GRAPH_ID, value, source)
}

/// Validates a policy waiver using the embedded checked-in waiver schema.
#[must_use]
pub fn validate_waiver(value: &serde_json::Value, source: &str) -> Vec<Diagnostic> {
    validate_embedded(WAIVER_ID, value, source)
}

fn validate_embedded(schema_id: &str, value: &serde_json::Value, source: &str) -> Vec<Diagnostic> {
    match OfflineSchemaSet::compile(embedded_resources()) {
        Ok(set) => set.validate(schema_id, value, source),
        Err(_) => vec![Diagnostic::error(
            "GHS002_SCHEMA",
            "embedded schema registry is invalid",
            "/",
            source,
        )],
    }
}

fn embedded_resources() -> BTreeMap<String, serde_json::Value> {
    BTreeMap::from([
        (GRAPH_ID.into(), parse_schema(GRAPH_SCHEMA)),
        (NODE_ID.into(), parse_schema(NODE_SCHEMA)),
        (EDGE_ID.into(), parse_schema(EDGE_SCHEMA)),
        (AGENT_ID.into(), parse_schema(AGENT_SCHEMA)),
        (WAIVER_ID.into(), parse_schema(WAIVER_SCHEMA)),
    ])
}

fn compile_error(message: &str) -> Diagnostic {
    Diagnostic::error("GHS002_SCHEMA", message, "/", "offline-schema-set")
}

fn compile_error_at(message: &str, path: &str) -> Diagnostic {
    Diagnostic::error("GHS002_SCHEMA", message, path, "offline-schema-set")
}

fn exceeds_json_depth(value: &serde_json::Value, depth: usize) -> bool {
    if depth > MAX_JSON_DEPTH {
        return true;
    }
    match value {
        serde_json::Value::Array(values) => values
            .iter()
            .any(|item| exceeds_json_depth(item, depth + 1)),
        serde_json::Value::Object(values) => values
            .values()
            .any(|item| exceeds_json_depth(item, depth + 1)),
        _ => false,
    }
}

fn parse_schema(source: &str) -> serde_json::Value {
    serde_json::from_str(source).expect("checked-in JSON schemas must parse")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unresolved_reference_fails_during_local_registry_preparation() {
        let resources = BTreeMap::from([(
            "test".into(),
            serde_json::json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "$id": "https://p50.dev/schemas/test-local-only.json",
                "$ref": "https://p50.dev/schemas/never-registered.json"
            }),
        )]);
        assert!(OfflineSchemaSet::compile(resources).is_err());
    }
}
