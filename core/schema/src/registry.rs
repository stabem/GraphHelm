use std::collections::BTreeMap;
use std::sync::OnceLock;

use graphhelm_protocols::Diagnostic;
use jsonschema::{Draft, Registry, Retrieve, Uri, Validator};
use std::fmt;

const GRAPH_SCHEMA: &str = include_str!("../../../schemas/graph.schema.json");
const NODE_SCHEMA: &str = include_str!("../../../schemas/node.schema.json");
const EDGE_SCHEMA: &str = include_str!("../../../schemas/edge.schema.json");
const AGENT_SCHEMA: &str = include_str!("../../../schemas/agent.schema.json");
const WAIVER_SCHEMA: &str = include_str!("../../../schemas/policy-waiver.schema.json");
const EVENT_SCHEMA: &str = include_str!("../../../schemas/event-envelope.schema.json");
const SCOPE_SCHEMA: &str = include_str!("../../../schemas/repository-scope.schema.json");
const SENSITIVITY_SCHEMA: &str = include_str!("../../../schemas/sensitivity.schema.json");
const ARTIFACT_SCHEMA: &str = include_str!("../../../schemas/artifact-reference.schema.json");
const PERSISTED_GRAPH_SCHEMA: &str =
    include_str!("../../../schemas/persisted-graph-version.schema.json");
const PHYSICAL_BATCH_SCHEMA: &str = r#"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "$id":"urn:graphhelm:repository-v1:physical-batch",
  "type":"object",
  "required":["formatVersion","requestDigest","scope","streamId","expectedNextSequence","checksum","evidenceIds","artifacts","events"],
  "properties":{
    "formatVersion":{"const":"1.0.0"},
    "requestDigest":{"type":"string","pattern":"^sha256:[0-9a-f]{64}$"},
    "scope":{"$ref":"https://p50.dev/schemas/repository-scope.schema.json"},
    "streamId":{"type":"string","pattern":"^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$"},
    "expectedNextSequence":{"type":"integer","minimum":1,"maximum":9007199254740991},
    "checksum":{"type":"string","pattern":"^sha256:[0-9a-f]{64}$"},
    "evidenceIds":{"type":"array","maxItems":10000,"uniqueItems":true,"items":{"type":"string","pattern":"^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$"}},
    "artifacts":{"type":"array","maxItems":64,"items":{"type":"object","required":["reference","producerStreamId","producerIdempotencyKey"],"properties":{"reference":{"$ref":"https://p50.dev/schemas/artifact-reference.schema.json"},"producerStreamId":{"type":"string","pattern":"^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$"},"producerIdempotencyKey":{"type":"string","pattern":"^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$"}},"unevaluatedProperties":false}},
    "events":{"type":"array","minItems":1,"maxItems":10000,"items":{"$ref":"https://p50.dev/schemas/event-envelope.schema.json"}}
  },
  "unevaluatedProperties":false
}"#;

const GRAPH_ID: &str = "https://p50.dev/schemas/graph.schema.json";
const NODE_ID: &str = "https://p50.dev/schemas/node.schema.json";
const EDGE_ID: &str = "https://p50.dev/schemas/edge.schema.json";
const AGENT_ID: &str = "https://p50.dev/schemas/agent.schema.json";
const WAIVER_ID: &str = "https://p50.dev/schemas/policy-waiver.schema.json";
const EVENT_ID: &str = "https://p50.dev/schemas/event-envelope.schema.json";
const SCOPE_ID: &str = "https://p50.dev/schemas/repository-scope.schema.json";
const SENSITIVITY_ID: &str = "https://p50.dev/schemas/sensitivity.schema.json";
const ARTIFACT_ID: &str = "https://p50.dev/schemas/artifact-reference.schema.json";
const PERSISTED_GRAPH_ID: &str = "https://p50.dev/schemas/persisted-graph-version.schema.json";
const PHYSICAL_BATCH_ID: &str = "urn:graphhelm:repository-v1:physical-batch";
const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;
const MAX_RESOURCE_BYTES: usize = 32 * 1024 * 1024;
const MAX_SCHEMAS: usize = 256;
const MAX_JSON_DEPTH: usize = 128;
const MAX_INLINE_SCHEMA_BYTES: usize = 1024 * 1024;
const MAX_INLINE_SCHEMA_DEPTH: usize = 64;
const MAX_INLINE_SCHEMA_VALUES: usize = 32 * 1024;
const MAX_INLINE_SCHEMA_KEY_BYTES: usize = 512 * 1024;
const INLINE_SCHEMA_ID: &str = "urn:graphhelm:inline-schema";

#[derive(Clone, Copy)]
struct RejectExternalResources;

#[derive(Debug)]
struct ExternalResourceRejected;

impl fmt::Display for ExternalResourceRejected {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("external schema retrieval is disabled")
    }
}

impl std::error::Error for ExternalResourceRejected {}

impl Retrieve for RejectExternalResources {
    fn retrieve(
        &self,
        _: &Uri<String>,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        Err(Box::new(ExternalResourceRejected))
    }
}

/// Redacted result of compiling an untrusted inline Draft 2020-12 schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlineSchemaError {
    /// The schema exceeded a deterministic pre-compilation bound.
    LimitExceeded,
    /// The schema is not a valid, fully offline Draft 2020-12 schema.
    Invalid,
}

impl fmt::Display for InlineSchemaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::LimitExceeded => "inline schema exceeds a deterministic limit",
            Self::Invalid => "inline schema is invalid",
        })
    }
}

impl std::error::Error for InlineSchemaError {}

/// Compiles an untrusted inline schema as Draft 2020-12 with no ambient resources.
///
/// Local fragments and in-document resources are available. Any reference that
/// would require filesystem, network, or caller-provided retrieval fails closed.
/// After deterministic preflight, the pinned compiler is used only through its
/// fallible APIs. Unexpected dependency panics remain a library invariant; this
/// boundary does not install or mutate a process-global panic hook.
pub fn compile_inline_schema(schema: &serde_json::Value) -> Result<(), InlineSchemaError> {
    preflight_inline_schema(schema)?;
    let registry = prepare_inline_registry_with(schema, RejectExternalResources)?;
    build_inline_validator_with(schema, &registry, RejectExternalResources)
}

fn prepare_inline_registry_with<R: Retrieve + 'static>(
    schema: &serde_json::Value,
    retriever: R,
) -> Result<Registry<'_>, InlineSchemaError> {
    Registry::new()
        .draft(Draft::Draft202012)
        .retriever(retriever)
        .add(INLINE_SCHEMA_ID, schema)
        .map_err(|_| InlineSchemaError::Invalid)?
        .prepare()
        .map_err(|_| InlineSchemaError::Invalid)
}

fn build_inline_validator_with<R: Retrieve + 'static>(
    schema: &serde_json::Value,
    registry: &Registry<'_>,
    retriever: R,
) -> Result<(), InlineSchemaError> {
    jsonschema::options()
        .with_draft(Draft::Draft202012)
        .with_base_uri(INLINE_SCHEMA_ID)
        .with_registry(registry)
        .with_retriever(retriever)
        .should_validate_formats(true)
        .build(schema)
        .map(|_| ())
        .map_err(|_| InlineSchemaError::Invalid)
}

enum InlineFrame<'a> {
    Array {
        items: std::slice::Iter<'a, serde_json::Value>,
        child_depth: usize,
    },
    Object {
        members: serde_json::map::Iter<'a>,
        child_depth: usize,
    },
}

fn preflight_inline_schema(schema: &serde_json::Value) -> Result<(), InlineSchemaError> {
    preflight_inline_schema_with_peak(schema).map(|_| ())
}

fn preflight_inline_schema_with_peak(
    schema: &serde_json::Value,
) -> Result<usize, InlineSchemaError> {
    fn count_value(depth: usize, values: &mut usize) -> Result<(), InlineSchemaError> {
        if depth > MAX_INLINE_SCHEMA_DEPTH {
            return Err(InlineSchemaError::LimitExceeded);
        }
        *values = values
            .checked_add(1)
            .ok_or(InlineSchemaError::LimitExceeded)?;
        if *values > MAX_INLINE_SCHEMA_VALUES {
            return Err(InlineSchemaError::LimitExceeded);
        }
        Ok(())
    }

    fn push_children<'a>(
        stack: &mut Vec<InlineFrame<'a>>,
        value: &'a serde_json::Value,
        depth: usize,
    ) -> Result<(), InlineSchemaError> {
        let child_depth = depth
            .checked_add(1)
            .ok_or(InlineSchemaError::LimitExceeded)?;
        match value {
            serde_json::Value::Array(items) if !items.is_empty() => {
                stack.push(InlineFrame::Array {
                    items: items.iter(),
                    child_depth,
                });
            }
            serde_json::Value::Object(object) if !object.is_empty() => {
                stack.push(InlineFrame::Object {
                    members: object.iter(),
                    child_depth,
                });
            }
            _ => {}
        }
        Ok(())
    }

    let mut values = 0usize;
    let mut key_bytes = 0usize;
    count_value(0, &mut values)?;
    let mut stack = Vec::with_capacity(MAX_INLINE_SCHEMA_DEPTH + 1);
    push_children(&mut stack, schema, 0)?;
    let mut peak = stack.len();

    while let Some(frame) = stack.last_mut() {
        let next = match frame {
            InlineFrame::Array { items, child_depth } => {
                items.next().map(|value| (None, value, *child_depth))
            }
            InlineFrame::Object {
                members,
                child_depth,
            } => members
                .next()
                .map(|(key, value)| (Some(key.as_str()), value, *child_depth)),
        };
        let Some((key, value, depth)) = next else {
            stack.pop();
            continue;
        };
        if let Some(key) = key {
            key_bytes = key_bytes
                .checked_add(key.len())
                .ok_or(InlineSchemaError::LimitExceeded)?;
            if key_bytes > MAX_INLINE_SCHEMA_KEY_BYTES {
                return Err(InlineSchemaError::LimitExceeded);
            }
        }
        count_value(depth, &mut values)?;
        push_children(&mut stack, value, depth)?;
        peak = peak.max(stack.len());
    }

    struct BoundedWriter {
        bytes: usize,
    }
    impl std::io::Write for BoundedWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.bytes = self
                .bytes
                .checked_add(bytes.len())
                .ok_or_else(|| std::io::Error::other("inline schema limit"))?;
            if self.bytes > MAX_INLINE_SCHEMA_BYTES {
                return Err(std::io::Error::other("inline schema limit"));
            }
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(&mut BoundedWriter { bytes: 0 }, schema)
        .map_err(|_| InlineSchemaError::LimitExceeded)?;
    Ok(peak)
}

/// A schema registry compiled solely from explicitly supplied in-memory resources.
pub struct OfflineSchemaSet {
    validators: BTreeMap<String, Validator>,
}

/// Strict checked-in schemas used by repository append, load and public replay.
pub struct RepositorySchemaSet {
    schemas: OfflineSchemaSet,
}

impl RepositorySchemaSet {
    #[must_use]
    pub fn validate_event(&self, value: &serde_json::Value) -> Vec<Diagnostic> {
        self.schemas.validate(EVENT_ID, value, "event-envelope")
    }

    #[must_use]
    pub fn validate_batch(&self, value: &serde_json::Value) -> Vec<Diagnostic> {
        self.schemas
            .validate(PHYSICAL_BATCH_ID, value, "repository-physical-batch")
    }
}

static REPOSITORY_SCHEMAS: OnceLock<Result<RepositorySchemaSet, Vec<Diagnostic>>> = OnceLock::new();

/// Returns the process-wide repository registry, compiled once from checked-in resources.
pub fn repository_schema_set() -> Result<&'static RepositorySchemaSet, &'static [Diagnostic]> {
    REPOSITORY_SCHEMAS
        .get_or_init(|| {
            #[cfg(test)]
            EMBEDDED_COMPILE_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            OfflineSchemaSet::compile(embedded_resources())
                .map(|schemas| RepositorySchemaSet { schemas })
        })
        .as_ref()
        .map_err(Vec::as_slice)
}

impl OfflineSchemaSet {
    /// Compiles all explicitly supplied schemas without filesystem or network retrieval.
    pub fn compile(
        resources: BTreeMap<String, serde_json::Value>,
    ) -> Result<Self, Vec<Diagnostic>> {
        Self::compile_with_retriever(resources, RejectExternalResources)
    }

    fn compile_with_retriever<R: Retrieve + Clone + 'static>(
        resources: BTreeMap<String, serde_json::Value>,
        retriever: R,
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

        let mut builder = Registry::new()
            .draft(Draft::Draft202012)
            .retriever(retriever.clone());
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
                .with_retriever(retriever.clone())
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

/// Validates a production event envelope using the complete checked-in offline registry.
#[must_use]
pub fn validate_event_envelope(value: &serde_json::Value) -> Vec<Diagnostic> {
    repository_schema_set().map_or_else(
        |_| embedded_registry_error("event-envelope"),
        |schemas| schemas.validate_event(value),
    )
}

/// Validates one closed repository-v1 physical batch before typed deserialization.
#[must_use]
pub fn validate_physical_batch(value: &serde_json::Value) -> Vec<Diagnostic> {
    repository_schema_set().map_or_else(
        |_| embedded_registry_error("repository-physical-batch"),
        |schemas| schemas.validate_batch(value),
    )
}

fn embedded_registry_error(source: &str) -> Vec<Diagnostic> {
    vec![Diagnostic::error(
        "GHS002_SCHEMA",
        "embedded schema registry is invalid",
        "/",
        source,
    )]
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

#[cfg(test)]
static EMBEDDED_COMPILE_COUNT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(test)]
fn embedded_compile_count() -> usize {
    EMBEDDED_COMPILE_COUNT.load(std::sync::atomic::Ordering::SeqCst)
}

fn embedded_resources() -> BTreeMap<String, serde_json::Value> {
    BTreeMap::from([
        (GRAPH_ID.into(), parse_schema(GRAPH_SCHEMA)),
        (NODE_ID.into(), parse_schema(NODE_SCHEMA)),
        (EDGE_ID.into(), parse_schema(EDGE_SCHEMA)),
        (AGENT_ID.into(), parse_schema(AGENT_SCHEMA)),
        (WAIVER_ID.into(), parse_schema(WAIVER_SCHEMA)),
        (EVENT_ID.into(), parse_schema(EVENT_SCHEMA)),
        (SCOPE_ID.into(), parse_schema(SCOPE_SCHEMA)),
        (SENSITIVITY_ID.into(), parse_schema(SENSITIVITY_SCHEMA)),
        (ARTIFACT_ID.into(), parse_schema(ARTIFACT_SCHEMA)),
        (
            PERSISTED_GRAPH_ID.into(),
            parse_schema(PERSISTED_GRAPH_SCHEMA),
        ),
        (
            PHYSICAL_BATCH_ID.into(),
            parse_schema(PHYSICAL_BATCH_SCHEMA),
        ),
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
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct RecordingRejecter {
        calls: Arc<Mutex<Vec<String>>>,
    }

    #[derive(Debug)]
    struct RejectedExternalResource;

    impl fmt::Display for RejectedExternalResource {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("external schema retrieval is disabled")
        }
    }

    impl std::error::Error for RejectedExternalResource {}

    impl jsonschema::Retrieve for RecordingRejecter {
        fn retrieve(
            &self,
            uri: &jsonschema::Uri<String>,
        ) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
            self.calls.lock().unwrap().push(uri.as_str().to_owned());
            Err(Box::new(RejectedExternalResource))
        }
    }

    #[test]
    fn inline_draft_2020_12_accepts_normative_edge_cases_and_local_refs() {
        for schema in [
            serde_json::json!(true),
            serde_json::json!({"enum": []}),
            serde_json::json!({"required": []}),
            serde_json::json!({"required": [""]}),
            serde_json::json!({"dependentRequired": {"name": []}}),
            serde_json::json!({"format": ""}),
            serde_json::json!({"$ref": ""}),
            serde_json::json!({"$ref": "#/$defs/item", "$defs": {"item": {"type": "string"}}}),
            serde_json::json!({
                "$id": "https://p50.dev/inline/root",
                "$ref": "child",
                "$defs": {"item": {"$id": "child", "type": "string"}}
            }),
            serde_json::json!({"$vocabulary": {}}),
            serde_json::json!({"pattern": "(?<=a)b"}),
            serde_json::json!({"minLength": 1.0}),
        ] {
            assert_eq!(compile_inline_schema(&schema), Ok(()), "{schema}");
        }
    }

    #[test]
    fn inline_draft_2020_12_rejects_invalid_shapes_and_external_resources() {
        for schema in [
            serde_json::json!({"type": 7}),
            serde_json::json!({"title": 7}),
            serde_json::json!({"properties": []}),
            serde_json::json!({"allOf": []}),
            serde_json::json!({"pattern": "["}),
            serde_json::json!({"$ref": "https://unavailable.invalid/schema.json"}),
            serde_json::json!({"$ref": "missing.json"}),
        ] {
            assert_eq!(
                compile_inline_schema(&schema),
                Err(InlineSchemaError::Invalid)
            );
        }
    }

    #[test]
    fn inline_schema_failures_are_redacted_bounded_and_do_not_panic() {
        let canary = "https://secret.invalid/private-schema-canary";
        let failure = compile_inline_schema(&serde_json::json!({"$ref": canary})).unwrap_err();
        assert_eq!(failure, InlineSchemaError::Invalid);
        assert!(!failure.to_string().contains(canary));
        assert!(!format!("{failure:?}").contains(canary));

        let oversized = serde_json::json!({"const": "x".repeat(MAX_INLINE_SCHEMA_BYTES)});
        assert_eq!(
            compile_inline_schema(&oversized),
            Err(InlineSchemaError::LimitExceeded)
        );

        let mut deep = serde_json::json!(true);
        for _ in 0..=MAX_INLINE_SCHEMA_DEPTH {
            deep = serde_json::json!({"not": deep});
        }
        let result = std::panic::catch_unwind(|| compile_inline_schema(&deep));
        assert_eq!(result.unwrap(), Err(InlineSchemaError::LimitExceeded));

        let too_many_values =
            serde_json::Value::Array(vec![serde_json::Value::Null; MAX_INLINE_SCHEMA_VALUES]);
        assert_eq!(
            preflight_inline_schema(&too_many_values),
            Err(InlineSchemaError::LimitExceeded)
        );

        let mut too_many_key_bytes = serde_json::Map::new();
        for index in 0..8192 {
            too_many_key_bytes.insert(
                format!("{index:05}-{}", "k".repeat(59)),
                serde_json::Value::Null,
            );
        }
        assert_eq!(
            preflight_inline_schema(&serde_json::Value::Object(too_many_key_bytes)),
            Err(InlineSchemaError::LimitExceeded)
        );
    }

    #[test]
    fn inline_preflight_peak_pending_work_is_bounded_by_depth_not_width() {
        let wide = serde_json::Value::Array(vec![serde_json::Value::Null; 16_384]);
        let wide_peak = preflight_inline_schema_with_peak(&wide).unwrap();
        assert!(wide_peak <= 2, "wide peak was {wide_peak}");

        let mut deep = serde_json::json!(null);
        for _ in 0..MAX_INLINE_SCHEMA_DEPTH {
            deep = serde_json::json!([deep]);
        }
        let deep_peak = preflight_inline_schema_with_peak(&deep).unwrap();
        assert!(deep_peak <= MAX_INLINE_SCHEMA_DEPTH + 1);
        assert!(deep_peak > wide_peak);
    }

    #[test]
    fn every_inline_builder_uses_only_the_explicit_rejecting_retriever() {
        for reference in [
            "ssh://private.invalid/schema.json",
            "https://private.invalid/schema.json",
            "file:///C:/private/schema.json",
            "relative/private.json",
        ] {
            let schema = serde_json::json!({
                "$id": "https://p50.dev/inline/root.json",
                "$ref": reference
            });

            let registry_calls = Arc::new(Mutex::new(Vec::new()));
            let registry_result = prepare_inline_registry_with(
                &schema,
                RecordingRejecter {
                    calls: Arc::clone(&registry_calls),
                },
            );
            assert_eq!(registry_result.unwrap_err(), InlineSchemaError::Invalid);
            assert_eq!(registry_calls.lock().unwrap().len(), 1, "{reference}");

            let empty_registry = Registry::new().draft(Draft::Draft202012).prepare().unwrap();
            let validator_calls = Arc::new(Mutex::new(Vec::new()));
            let validator_result = build_inline_validator_with(
                &schema,
                &empty_registry,
                RecordingRejecter {
                    calls: Arc::clone(&validator_calls),
                },
            );
            assert_eq!(validator_result, Err(InlineSchemaError::Invalid));
            assert_eq!(validator_calls.lock().unwrap().len(), 1, "{reference}");

            let public_error = compile_inline_schema(&schema).unwrap_err();
            assert_eq!(public_error, InlineSchemaError::Invalid);
            assert!(!public_error.to_string().contains(reference));
            assert!(!format!("{public_error:?}").contains(reference));
        }
    }

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

    #[test]
    fn offline_schema_set_uses_an_explicit_rejecting_retriever() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let resources = BTreeMap::from([(
            "test".into(),
            serde_json::json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "$id": "https://p50.dev/schemas/test-offline-root.json",
                "$ref": "file:///private/schema.json"
            }),
        )]);
        assert!(
            OfflineSchemaSet::compile_with_retriever(
                resources,
                RecordingRejecter {
                    calls: Arc::clone(&calls),
                },
            )
            .is_err()
        );
        assert_eq!(
            calls.lock().unwrap().as_slice(),
            ["file:///private/schema.json"]
        );
    }

    #[test]
    fn physical_batch_root_is_closed_and_rejects_invalid_digest_before_typed_use() {
        let event = serde_json::json!({
            "schemaVersion":"1.0.0",
            "eventId":"event-1",
            "scope":{"workspaceId":"workspace-1","projectId":"project-1","executionId":"execution-1"},
            "streamId":"stream-1",
            "sequence":1,
            "occurredAt":"2026-08-10T12:00:00Z",
            "idempotencyKey":"request-1",
            "actor":{"type":"system","id":"system-1"},
            "sensitivity":"internal",
            "kind":{"type":"graph_imported","data":{"sourceSha256":"a".repeat(64),"sourceKind":"graph_document"}},
            "evidenceRefs":[],
            "artifactRefs":[],
            "previousHash":format!("sha256:{}", "0".repeat(64)),
            "eventHash":format!("sha256:{}", "1".repeat(64))
        });
        let batch = serde_json::json!({
            "formatVersion":"1.0.0",
            "requestDigest":format!("sha256:{}", "2".repeat(64)),
            "scope":{"workspaceId":"workspace-1","projectId":"project-1","executionId":"execution-1"},
            "streamId":"stream-1",
            "expectedNextSequence":1,
            "checksum":format!("sha256:{}", "3".repeat(64)),
            "evidenceIds":[],
            "artifacts":[],
            "events":[event]
        });
        assert!(validate_physical_batch(&batch).is_empty());
        let mut invalid_digest = batch.clone();
        invalid_digest["requestDigest"] = serde_json::json!("not-a-sha256");
        assert!(!validate_physical_batch(&invalid_digest).is_empty());
        let mut extra = batch;
        extra["typedSerdeSentinel"] = serde_json::json!(true);
        assert!(!validate_physical_batch(&extra).is_empty());
    }

    #[test]
    fn embedded_event_registry_compiles_at_most_once_across_thousands_of_validations() {
        let before = embedded_compile_count();
        for _ in 0..2_048 {
            let _ = validate_event_envelope(&serde_json::json!({}));
            let _ = validate_physical_batch(&serde_json::json!({}));
        }
        let after = embedded_compile_count();
        assert!(
            after.saturating_sub(before) <= 1,
            "compiled {} times",
            after - before
        );
        assert!(after <= 1, "embedded registry compiled {after} times");
    }
}
