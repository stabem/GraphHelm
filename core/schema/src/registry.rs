use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use graphhelm_protocols::Diagnostic;
use jsonschema::{Draft, FancyRegex, PatternOptions, Registry, Retrieve, Uri, Validator};
use std::fmt;

const RESTORE_PLAN_SCHEMA: &str = include_str!("../../../schemas/restore-plan.schema.json");
const ACTIVATION_RECEIPT_SCHEMA: &str =
    include_str!("../../../schemas/activation-receipt.schema.json");
const PLAN_SCHEMA: &str = include_str!("../../../schemas/adoption-plan.schema.json");
const RECEIPT_SCHEMA: &str = include_str!("../../../schemas/adoption-receipt.schema.json");
const JOURNAL_SCHEMA: &str = include_str!("../../../schemas/adoption-journal.schema.json");
const GRAPH_SCHEMA: &str = include_str!("../../../schemas/graph.schema.json");
const NODE_SCHEMA: &str = include_str!("../../../schemas/node.schema.json");
const EDGE_SCHEMA: &str = include_str!("../../../schemas/edge.schema.json");
const AGENT_SCHEMA: &str = include_str!("../../../schemas/agent.schema.json");
const EXTENSION_SCHEMA: &str = include_str!("../../../schemas/extension.schema.json");
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
const EXTENSION_ID: &str = "https://p50.dev/schemas/extension.schema.json";
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
const MAX_INLINE_SCHEMA_APPLICATOR_DEPTH: usize = 16;
const MAX_INLINE_SCHEMA_APPLICATOR_BRANCHES: usize = 4 * 1024;
const MAX_INLINE_INSTANCE_VALUES: usize = 32 * 1024;
const MAX_VALIDATION_INSTANCE_VALUES: usize = 128 * 1024;
const MAX_VALIDATION_TEXT_BYTES: usize = 32 * 1024 * 1024;
const MAX_VALIDATION_DIAGNOSTICS: usize = 256;
const MAX_VALIDATION_WORK_UNITS: usize = 32 * 1024 * 1024;
const MAX_SCHEMA_REFERENCE_NODES: usize = 128 * 1024;
const REGEX_BACKTRACK_LIMIT: usize = 100_000;
const REGEX_COMPILED_SIZE_LIMIT: usize = 1024 * 1024;
const REGEX_DFA_SIZE_LIMIT: usize = 2 * 1024 * 1024;
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
    resolved_schema_work_scores(&registry, &[(INLINE_SCHEMA_ID, schema)])?;
    build_inline_validator_with(schema, &registry, RejectExternalResources)
}

/// Validates one bounded value against an untrusted inline Draft 2020-12 schema.
///
/// Schema compilation remains fully offline. Diagnostics contain only stable
/// constraint prose and JSON Pointers; offending values are never echoed.
pub fn validate_inline_value(
    schema: &serde_json::Value,
    value: &serde_json::Value,
    source: &str,
) -> Result<Vec<Diagnostic>, InlineSchemaError> {
    schema_validation_work_score(schema)?;
    let Some(instance_work_units) =
        instance_validation_work_units(value, MAX_INLINE_INSTANCE_VALUES, MAX_FILE_BYTES)
    else {
        return Err(InlineSchemaError::LimitExceeded);
    };
    if serde_json::to_vec(value).map_or(true, |bytes| bytes.len() > MAX_FILE_BYTES) {
        return Err(InlineSchemaError::LimitExceeded);
    }
    let registry = prepare_inline_registry_with(schema, RejectExternalResources)?;
    let work_scores = resolved_schema_work_scores(&registry, &[(INLINE_SCHEMA_ID, schema)])?;
    let work_score = work_scores[INLINE_SCHEMA_ID];
    if !validation_work_is_bounded(
        work_score.reachable_work_units,
        work_score.expanded_work_units,
        instance_work_units,
    ) {
        return Err(InlineSchemaError::LimitExceeded);
    }
    let validator = jsonschema::options()
        .with_draft(Draft::Draft202012)
        .with_base_uri(INLINE_SCHEMA_ID)
        .with_registry(&registry)
        .with_retriever(RejectExternalResources)
        .with_pattern_options(bounded_pattern_options())
        .should_validate_formats(true)
        .build(schema)
        .map_err(|_| InlineSchemaError::Invalid)?;
    Ok(collect_validation_diagnostics(&validator, value, source))
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
        .with_pattern_options(bounded_pattern_options())
        .should_validate_formats(true)
        .build(schema)
        .map(|_| ())
        .map_err(|_| InlineSchemaError::Invalid)
}

fn bounded_pattern_options() -> PatternOptions<FancyRegex> {
    PatternOptions::fancy_regex()
        .backtrack_limit(REGEX_BACKTRACK_LIMIT)
        .size_limit(REGEX_COMPILED_SIZE_LIMIT)
        .dfa_size_limit(REGEX_DFA_SIZE_LIMIT)
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct SchemaGraphNodeKey {
    address: usize,
    base_uri: String,
}

struct SchemaGraphNode {
    local_work_units: usize,
    edges: Vec<SchemaGraphNodeKey>,
}

#[derive(Clone, Copy)]
struct SchemaWorkScore {
    reachable_work_units: usize,
    expanded_work_units: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SchemaReferenceGraphError {
    Invalid,
    LimitExceeded,
}

impl From<SchemaReferenceGraphError> for InlineSchemaError {
    fn from(error: SchemaReferenceGraphError) -> Self {
        match error {
            SchemaReferenceGraphError::Invalid => Self::Invalid,
            SchemaReferenceGraphError::LimitExceeded => Self::LimitExceeded,
        }
    }
}

fn schema_graph_node_key(schema: &serde_json::Value, base_uri: &Uri<String>) -> SchemaGraphNodeKey {
    SchemaGraphNodeKey {
        address: std::ptr::from_ref(schema).addr(),
        base_uri: base_uri.as_str().to_owned(),
    }
}

fn schema_node_local_work_units(
    schema: &serde_json::Value,
    subschemas: &[&serde_json::Value],
) -> Result<usize, SchemaReferenceGraphError> {
    let child_addresses: BTreeSet<_> = subschemas
        .iter()
        .map(|child| std::ptr::from_ref(*child).addr())
        .collect();
    let root_address = std::ptr::from_ref(schema).addr();
    let mut work_units = 0usize;
    let mut stack = vec![schema];
    while let Some(value) = stack.pop() {
        let address = std::ptr::from_ref(value).addr();
        if address != root_address && child_addresses.contains(&address) {
            continue;
        }
        work_units = work_units
            .checked_add(1)
            .ok_or(SchemaReferenceGraphError::LimitExceeded)?;
        match value {
            serde_json::Value::Array(items) => stack.extend(items),
            serde_json::Value::Object(members) => stack.extend(members.values()),
            _ => {}
        }
    }
    if let Some(schema_object) = schema.as_object() {
        for (keyword, value) in schema_object {
            work_units = work_units
                .checked_add(
                    schema_keyword_work_units(keyword, value)
                        .map_err(|_| SchemaReferenceGraphError::LimitExceeded)?,
                )
                .ok_or(SchemaReferenceGraphError::LimitExceeded)?;
            if keyword == "$ref" {
                work_units = work_units
                    .checked_add(1)
                    .ok_or(SchemaReferenceGraphError::LimitExceeded)?;
            }
        }
    }
    if work_units > MAX_VALIDATION_WORK_UNITS {
        return Err(SchemaReferenceGraphError::LimitExceeded);
    }
    Ok(work_units)
}

fn resolved_schema_work_scores<'a>(
    registry: &'a Registry<'a>,
    roots: &[(&'a str, &'a serde_json::Value)],
) -> Result<BTreeMap<String, SchemaWorkScore>, SchemaReferenceGraphError> {
    let mut root_keys = BTreeMap::new();
    let mut pending = Vec::with_capacity(roots.len());
    for &(root_uri, schema) in roots {
        let base_uri =
            jsonschema::uri::from_str(root_uri).map_err(|_| SchemaReferenceGraphError::Invalid)?;
        let resolver = registry
            .resolver(base_uri)
            .in_subresource(Draft::Draft202012.create_resource_ref(schema))
            .map_err(|_| SchemaReferenceGraphError::Invalid)?;
        let key = schema_graph_node_key(schema, resolver.base_uri().as_ref());
        root_keys.insert(root_uri.to_owned(), key);
        pending.push((schema, resolver, Draft::Draft202012));
    }

    let mut graph = BTreeMap::new();
    while let Some((schema, resolver, draft)) = pending.pop() {
        let key = schema_graph_node_key(schema, resolver.base_uri().as_ref());
        if graph.contains_key(&key) {
            continue;
        }
        if graph.len() >= MAX_SCHEMA_REFERENCE_NODES {
            return Err(SchemaReferenceGraphError::LimitExceeded);
        }

        let subschemas: Vec<_> = draft.subresources_of(schema).collect();
        let local_work_units = schema_node_local_work_units(schema, &subschemas)?;
        let mut edges = Vec::with_capacity(subschemas.len().saturating_add(1));

        if let Some(schema_object) = schema.as_object() {
            if schema_object.contains_key("$recursiveRef") {
                return Err(SchemaReferenceGraphError::Invalid);
            }
            for keyword in ["$ref", "$dynamicRef"] {
                let Some(reference) = schema_object
                    .get(keyword)
                    .and_then(serde_json::Value::as_str)
                else {
                    continue;
                };
                let resolved = resolver
                    .lookup(reference)
                    .map_err(|_| SchemaReferenceGraphError::Invalid)?;
                let (target, target_resolver, target_draft) = resolved.into_inner();
                let target_key = schema_graph_node_key(target, target_resolver.base_uri().as_ref());
                // Draft 2020-12 dynamic recursion can point an anchor back to the schema that
                // declares it. The validator's work is then bounded by the already-counted
                // instance tree, so do not turn this normative fixed point into schema expansion.
                // Keep ordinary non-empty `$ref` recursion fail-closed: unlike `$dynamicRef`, it
                // is not the accepted dynamic-recursion contract exercised by GraphHelm schemas.
                let is_bounded_self_reference = target_key == key
                    && ((keyword == "$ref" && reference.is_empty()) || keyword == "$dynamicRef");
                if !is_bounded_self_reference {
                    edges.push(target_key);
                    pending.push((target, target_resolver, target_draft));
                }
            }
        }

        for child in subschemas {
            let child_draft = draft.detect(child);
            let child_resolver = resolver
                .in_subresource(child_draft.create_resource_ref(child))
                .map_err(|_| SchemaReferenceGraphError::Invalid)?;
            let child_key = schema_graph_node_key(child, child_resolver.base_uri().as_ref());
            edges.push(child_key);
            pending.push((child, child_resolver, child_draft));
        }
        graph.insert(
            key,
            SchemaGraphNode {
                local_work_units,
                edges,
            },
        );
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum VisitState {
        Visiting,
        Done,
    }

    let mut states = BTreeMap::new();
    let mut costs = BTreeMap::new();
    for root_key in root_keys.values() {
        if costs.contains_key(root_key) {
            continue;
        }
        states.insert(root_key.clone(), VisitState::Visiting);
        let mut stack = vec![(root_key.clone(), 0usize)];
        while let Some((key, next_edge)) = stack.last_mut() {
            let node = graph.get(key).ok_or(SchemaReferenceGraphError::Invalid)?;
            if let Some(child) = node.edges.get(*next_edge) {
                *next_edge += 1;
                match states.get(child) {
                    Some(VisitState::Visiting) => {
                        return Err(SchemaReferenceGraphError::LimitExceeded);
                    }
                    Some(VisitState::Done) => {}
                    None => {
                        states.insert(child.clone(), VisitState::Visiting);
                        stack.push((child.clone(), 0));
                    }
                }
                continue;
            }

            let mut expanded_work_units = node.local_work_units;
            for child in &node.edges {
                expanded_work_units = expanded_work_units
                    .checked_add(*costs.get(child).ok_or(SchemaReferenceGraphError::Invalid)?)
                    .ok_or(SchemaReferenceGraphError::LimitExceeded)?;
                if expanded_work_units > MAX_VALIDATION_WORK_UNITS {
                    return Err(SchemaReferenceGraphError::LimitExceeded);
                }
            }
            costs.insert(key.clone(), expanded_work_units);
            states.insert(key.clone(), VisitState::Done);
            stack.pop();
        }
    }

    root_keys
        .into_iter()
        .map(|(root_uri, key)| {
            let expanded_work_units = *costs.get(&key).ok_or(SchemaReferenceGraphError::Invalid)?;
            let mut reachable_work_units = 0usize;
            let mut visited = BTreeSet::new();
            let mut pending = vec![key];
            while let Some(node_key) = pending.pop() {
                if !visited.insert(node_key.clone()) {
                    continue;
                }
                let node = graph
                    .get(&node_key)
                    .ok_or(SchemaReferenceGraphError::Invalid)?;
                reachable_work_units = reachable_work_units
                    .checked_add(node.local_work_units)
                    .ok_or(SchemaReferenceGraphError::LimitExceeded)?;
                if reachable_work_units > MAX_VALIDATION_WORK_UNITS {
                    return Err(SchemaReferenceGraphError::LimitExceeded);
                }
                pending.extend(node.edges.iter().cloned());
            }
            Ok((
                root_uri,
                SchemaWorkScore {
                    reachable_work_units,
                    expanded_work_units,
                },
            ))
        })
        .collect()
}

enum InlineFrame<'a> {
    Array {
        items: std::slice::Iter<'a, serde_json::Value>,
        child_depth: usize,
        applicator_depth: usize,
    },
    Object {
        members: serde_json::map::Iter<'a>,
        child_depth: usize,
        applicator_depth: usize,
    },
}

fn preflight_inline_schema(schema: &serde_json::Value) -> Result<(), InlineSchemaError> {
    preflight_inline_schema_with_peak(schema).map(|_| ())
}

fn preflight_inline_schema_with_peak(
    schema: &serde_json::Value,
) -> Result<usize, InlineSchemaError> {
    preflight_schema_with_peak(schema, MAX_INLINE_SCHEMA_BYTES)
}

fn preflight_schema_with_peak(
    schema: &serde_json::Value,
    max_serialized_bytes: usize,
) -> Result<usize, InlineSchemaError> {
    preflight_schema(schema, max_serialized_bytes).map(|preflight| preflight.peak_pending_frames)
}

struct SchemaPreflight {
    peak_pending_frames: usize,
    base_work_units: usize,
    reference_count: usize,
}

impl SchemaPreflight {
    fn validation_work_units(&self) -> Result<usize, InlineSchemaError> {
        self.base_work_units
            .checked_add(self.reference_count)
            .ok_or(InlineSchemaError::LimitExceeded)
    }
}

pub(crate) fn schema_validation_work_score(
    schema: &serde_json::Value,
) -> Result<usize, InlineSchemaError> {
    preflight_schema(schema, MAX_INLINE_SCHEMA_BYTES)?.validation_work_units()
}

fn preflight_schema(
    schema: &serde_json::Value,
    max_serialized_bytes: usize,
) -> Result<SchemaPreflight, InlineSchemaError> {
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
        applicator_depth: usize,
    ) -> Result<(), InlineSchemaError> {
        let child_depth = depth
            .checked_add(1)
            .ok_or(InlineSchemaError::LimitExceeded)?;
        match value {
            serde_json::Value::Array(items) if !items.is_empty() => {
                stack.push(InlineFrame::Array {
                    items: items.iter(),
                    child_depth,
                    applicator_depth,
                });
            }
            serde_json::Value::Object(object) if !object.is_empty() => {
                stack.push(InlineFrame::Object {
                    members: object.iter(),
                    child_depth,
                    applicator_depth,
                });
            }
            _ => {}
        }
        Ok(())
    }

    let mut values = 0usize;
    let mut key_bytes = 0usize;
    let mut applicator_branches = 0usize;
    let mut keyword_work_units = 0usize;
    let mut reference_count = 0usize;
    count_value(0, &mut values)?;
    let mut stack = Vec::with_capacity(MAX_INLINE_SCHEMA_DEPTH + 1);
    push_children(&mut stack, schema, 0, 0)?;
    let mut peak = stack.len();

    while let Some(frame) = stack.last_mut() {
        let next = match frame {
            InlineFrame::Array {
                items,
                child_depth,
                applicator_depth,
            } => items
                .next()
                .map(|value| (None, value, *child_depth, *applicator_depth)),
            InlineFrame::Object {
                members,
                child_depth,
                applicator_depth,
            } => members
                .next()
                .map(|(key, value)| (Some(key.as_str()), value, *child_depth, *applicator_depth)),
        };
        let Some((key, value, depth, applicator_depth)) = next else {
            stack.pop();
            continue;
        };
        let child_applicator_depth = if key.is_some_and(schema_applicator_keyword) {
            applicator_depth
                .checked_add(1)
                .ok_or(InlineSchemaError::LimitExceeded)?
        } else {
            applicator_depth
        };
        if child_applicator_depth > MAX_INLINE_SCHEMA_APPLICATOR_DEPTH {
            return Err(InlineSchemaError::LimitExceeded);
        }
        if let Some(branches) = key.and_then(|key| schema_applicator_branches(key, value)) {
            applicator_branches = applicator_branches
                .checked_add(branches)
                .ok_or(InlineSchemaError::LimitExceeded)?;
            if applicator_branches > MAX_INLINE_SCHEMA_APPLICATOR_BRANCHES {
                return Err(InlineSchemaError::LimitExceeded);
            }
        }
        if let Some(key) = key {
            keyword_work_units = keyword_work_units
                .checked_add(schema_keyword_work_units(key, value)?)
                .ok_or(InlineSchemaError::LimitExceeded)?;
            if matches!(key, "$ref" | "$dynamicRef") && value.is_string() {
                reference_count = reference_count
                    .checked_add(1)
                    .ok_or(InlineSchemaError::LimitExceeded)?;
            }
            key_bytes = key_bytes
                .checked_add(key.len())
                .ok_or(InlineSchemaError::LimitExceeded)?;
            if key_bytes > MAX_INLINE_SCHEMA_KEY_BYTES {
                return Err(InlineSchemaError::LimitExceeded);
            }
        }
        count_value(depth, &mut values)?;
        push_children(&mut stack, value, depth, child_applicator_depth)?;
        peak = peak.max(stack.len());
    }

    struct BoundedWriter {
        bytes: usize,
        max_bytes: usize,
    }
    impl std::io::Write for BoundedWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.bytes = self
                .bytes
                .checked_add(bytes.len())
                .ok_or_else(|| std::io::Error::other("inline schema limit"))?;
            if self.bytes > self.max_bytes {
                return Err(std::io::Error::other("inline schema limit"));
            }
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(
        &mut BoundedWriter {
            bytes: 0,
            max_bytes: max_serialized_bytes,
        },
        schema,
    )
    .map_err(|_| InlineSchemaError::LimitExceeded)?;
    let base_work_units = values
        .checked_add(keyword_work_units)
        .ok_or(InlineSchemaError::LimitExceeded)?;
    Ok(SchemaPreflight {
        peak_pending_frames: peak,
        base_work_units,
        reference_count,
    })
}

fn schema_applicator_keyword(key: &str) -> bool {
    matches!(
        key,
        "allOf"
            | "anyOf"
            | "oneOf"
            | "not"
            | "if"
            | "then"
            | "else"
            | "patternProperties"
            | "dependentSchemas"
            | "items"
            | "prefixItems"
            | "properties"
            | "additionalProperties"
            | "unevaluatedProperties"
            | "unevaluatedItems"
            | "contains"
            | "propertyNames"
    )
}

fn schema_applicator_branches(key: &str, value: &serde_json::Value) -> Option<usize> {
    match key {
        "allOf" | "anyOf" | "oneOf" | "prefixItems" => value.as_array().map(Vec::len),
        "patternProperties" | "dependentSchemas" | "properties" => {
            value.as_object().map(serde_json::Map::len)
        }
        "not"
        | "if"
        | "then"
        | "else"
        | "items"
        | "additionalProperties"
        | "unevaluatedProperties"
        | "unevaluatedItems"
        | "contains"
        | "propertyNames" => Some(1),
        _ => None,
    }
}

fn schema_keyword_work_units(
    key: &str,
    value: &serde_json::Value,
) -> Result<usize, InlineSchemaError> {
    let branch_work = schema_applicator_branches(key, value).unwrap_or(0);
    let regex_work = match key {
        "pattern" => value.as_str().map_or(1, regex_pattern_work_units),
        "patternProperties" => value.as_object().map_or(Ok(1), |patterns| {
            patterns.keys().try_fold(0usize, |total, pattern| {
                total
                    .checked_add(regex_pattern_work_units(pattern))
                    .ok_or(InlineSchemaError::LimitExceeded)
            })
        })?,
        _ => 0,
    };
    branch_work
        .checked_add(regex_work)
        .ok_or(InlineSchemaError::LimitExceeded)
}

fn regex_pattern_work_units(pattern: &str) -> usize {
    pattern.len().div_ceil(64).max(1)
}

/// A schema registry compiled solely from explicitly supplied in-memory resources.
pub struct OfflineSchemaSet {
    validators: BTreeMap<String, CompiledOfflineSchema>,
}

struct CompiledOfflineSchema {
    validator: Validator,
    validation_work_units: usize,
    expanded_work_units: usize,
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
        self.validate_batch_attempted(value).1
    }

    /// Validates a physical batch and reports separately whether the validator ran.
    ///
    /// The store needs the distinction: a stored journal line the governor declined to validate
    /// is not evidence that the line is corrupt, and reporting it as corruption sends an operator
    /// down a recovery path the bytes do not deserve. See [`OfflineSchemaSet::validate_attempted`].
    #[must_use]
    pub fn validate_batch_attempted(
        &self,
        value: &serde_json::Value,
    ) -> (ValidationAttempt, Vec<Diagnostic>) {
        self.schemas
            .validate_attempted(PHYSICAL_BATCH_ID, value, "repository-physical-batch")
    }
}

static REPOSITORY_SCHEMAS: OnceLock<Result<RepositorySchemaSet, Vec<Diagnostic>>> = OnceLock::new();

/// Returns the process-wide repository registry, compiled once from checked-in resources.
pub fn repository_schema_set() -> Result<&'static RepositorySchemaSet, &'static [Diagnostic]> {
    REPOSITORY_SCHEMAS
        .get_or_init(compile_embedded_registry)
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
            let preflight = preflight_schema(&document, MAX_FILE_BYTES).map_err(|_| {
                vec![compile_error_at(
                    "schema resource exceeds deterministic complexity limits",
                    &format!("{resource_path}/complexity"),
                )]
            })?;
            preflight.validation_work_units().map_err(|_| {
                vec![compile_error_at(
                    "schema resource exceeds deterministic complexity limits",
                    &format!("{resource_path}/complexity"),
                )]
            })?;
            resource_bytes = resource_bytes.saturating_add(bytes.len());
            if resource_bytes > MAX_RESOURCE_BYTES {
                return Err(vec![compile_error(
                    "schema resources exceed maximum aggregate size",
                )]);
            }
            let Some(id) = document
                .get("$id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
            else {
                return Err(vec![compile_error(
                    "schema resource must declare a string $id",
                )]);
            };
            if schemas.insert(id.clone(), document).is_some() {
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
        let roots: Vec<_> = schemas
            .iter()
            .map(|(id, schema)| (id.as_str(), schema))
            .collect();
        let work_scores = resolved_schema_work_scores(&registry, &roots).map_err(|_| {
            vec![compile_error_at(
                "schema reference graph is recursive or exceeds deterministic limits",
                "/resources/referenceGraph",
            )]
        })?;

        let mut validators = BTreeMap::new();
        for (id, schema) in &schemas {
            let validator = jsonschema::options()
                .with_draft(Draft::Draft202012)
                .with_registry(&registry)
                .with_retriever(retriever.clone())
                .with_pattern_options(bounded_pattern_options())
                .should_validate_formats(true)
                .build(schema)
                .map_err(|_| vec![compile_error("offline root schema is invalid")])?;
            validators.insert(
                id.clone(),
                CompiledOfflineSchema {
                    validator,
                    validation_work_units: work_scores[id].reachable_work_units,
                    expanded_work_units: work_scores[id].expanded_work_units,
                },
            );
        }
        Ok(Self { validators })
    }

    /// Validates a document using a root schema already compiled into this set.
    ///
    /// The returned diagnostics do NOT say whether the validator ran. A caller that must tell
    /// "this document breaks the schema" from "this document was never checked" wants
    /// [`Self::validate_attempted`], which is where this one gets its answer.
    #[must_use]
    pub fn validate(
        &self,
        schema_id: &str,
        document: &serde_json::Value,
        source: &str,
    ) -> Vec<Diagnostic> {
        self.validate_attempted(schema_id, document, source).1
    }

    /// Validates a document and reports separately WHETHER the validator ran.
    ///
    /// `validate` answers one question with two meanings: an error-severity `GHS002_SCHEMA`
    /// diagnostic is returned both when the document violates the schema and when the resource
    /// governor declined to validate it at all. Those are opposite facts about the document --
    /// one says it is wrong, the other says nothing about it -- and they have opposite recovery
    /// paths for a caller holding stored bytes: repair or quarantine the document, versus present
    /// it in smaller pieces. Distinguishing them by MESSAGE TEXT would make a reworded string a
    /// silent behaviour change, so the distinction is returned as a type.
    ///
    /// This is the only place either answer is produced: `validate` is this function with the
    /// attempt discarded, so the two can never disagree about the same document.
    #[must_use]
    pub fn validate_attempted(
        &self,
        schema_id: &str,
        document: &serde_json::Value,
        source: &str,
    ) -> (ValidationAttempt, Vec<Diagnostic>) {
        let Some(schema) = self.validators.get(schema_id) else {
            return (
                ValidationAttempt::Refused(ValidationRefusal::UnregisteredRoot),
                vec![Diagnostic::error(
                    "GHS002_SCHEMA",
                    "root schema is not registered in the offline schema set",
                    "/",
                    source,
                )],
            );
        };
        let Some(instance_work_units) = instance_validation_work_units(
            document,
            MAX_VALIDATION_INSTANCE_VALUES,
            MAX_VALIDATION_TEXT_BYTES,
        ) else {
            return (
                ValidationAttempt::Refused(ValidationRefusal::InstanceComplexity),
                vec![Diagnostic::error(
                    "GHS002_SCHEMA",
                    "document exceeds deterministic validation complexity limits",
                    "/",
                    source,
                )],
            );
        };
        if !validation_work_is_bounded(
            schema.validation_work_units,
            schema.expanded_work_units,
            instance_work_units,
        ) {
            return (
                ValidationAttempt::Refused(ValidationRefusal::ValidationWork),
                vec![Diagnostic::error(
                    "GHS002_SCHEMA",
                    "document exceeds deterministic validation work limits",
                    "/",
                    source,
                )],
            );
        }
        (
            ValidationAttempt::Ran,
            collect_validation_diagnostics(&schema.validator, document, source),
        )
    }
}

/// Whether the validator ran over a document, as opposed to how the document fared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationAttempt {
    /// The validator ran. Accompanying diagnostics describe the document; empty means it conforms.
    Ran,
    /// The validator did NOT run. Accompanying diagnostics describe the refusal, not the document,
    /// and say nothing about whether it conforms.
    Refused(ValidationRefusal),
}

/// Why validation was declined before the validator saw the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationRefusal {
    /// The root schema id is not in this set. A build or wiring fault, not a property of the
    /// document: the same document would validate against a correctly assembled set.
    UnregisteredRoot,
    /// The document has more values, or more text, than the instance walk will traverse.
    InstanceComplexity,
    /// The document and schema together exceed the bounded validation work budget.
    ValidationWork,
}

fn collect_validation_diagnostics(
    validator: &Validator,
    document: &serde_json::Value,
    source: &str,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::with_capacity(MAX_VALIDATION_DIAGNOSTICS + 1);
    let mut omitted = false;
    for validation_error in validator.iter_errors(document) {
        if diagnostics.len() == MAX_VALIDATION_DIAGNOSTICS {
            omitted = true;
            break;
        }
        let path = validation_error.instance_path().as_str();
        diagnostics.push(Diagnostic::error(
            "GHS002_SCHEMA",
            validation_message(validation_error.kind()),
            if path.is_empty() { "/" } else { path },
            source,
        ));
    }
    if omitted {
        diagnostics.push(Diagnostic::error(
            "GHS002_SCHEMA",
            "schema validation diagnostic limit reached; further failures omitted",
            "/",
            source,
        ));
    }
    diagnostics.sort_by(|left, right| {
        (&left.path, &left.code, &left.message).cmp(&(&right.path, &right.code, &right.message))
    });
    diagnostics
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

/// Validates a raw extension manifest using only embedded checked-in schema resources.
#[must_use]
pub fn validate_extension_value(value: &serde_json::Value, source: &str) -> Vec<Diagnostic> {
    validate_embedded(EXTENSION_ID, value, source)
}

/// Validates a raw agent definition using only embedded checked-in schema resources.
#[must_use]
pub fn validate_agent_value(value: &serde_json::Value, source: &str) -> Vec<Diagnostic> {
    validate_embedded(AGENT_ID, value, source)
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
    match repository_schema_set() {
        Ok(set) => set.schemas.validate(schema_id, value, source),
        Err(_) => embedded_registry_error(source),
    }
}

fn compile_embedded_registry() -> Result<RepositorySchemaSet, Vec<Diagnostic>> {
    OfflineSchemaSet::compile(embedded_resources()).map(|schemas| RepositorySchemaSet { schemas })
}

#[cfg(test)]
static EMBEDDED_COMPILE_COUNT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(test)]
static EMBEDDED_OBSERVER_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
fn embedded_compile_count() -> usize {
    EMBEDDED_COMPILE_COUNT.load(std::sync::atomic::Ordering::SeqCst)
}

fn embedded_resources() -> BTreeMap<String, serde_json::Value> {
    // This is the single resource-construction site used by both the historical per-call
    // compiler and the process-wide initializer. Keeping the observer here means a regression
    // that reintroduces the old direct compile cannot hide behind the cached initializer count.
    #[cfg(test)]
    EMBEDDED_COMPILE_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    BTreeMap::from([
        (
            "https://p50.dev/schemas/restore-plan.schema.json".into(),
            serde_json::from_str(RESTORE_PLAN_SCHEMA).expect("embedded restore schema"),
        ),
        (
            "https://p50.dev/schemas/activation-receipt.schema.json".into(),
            serde_json::from_str(ACTIVATION_RECEIPT_SCHEMA)
                .expect("embedded activation receipt schema"),
        ),
        (
            "https://p50.dev/schemas/adoption-plan.schema.json".into(),
            parse_schema(PLAN_SCHEMA),
        ),
        (
            "https://p50.dev/schemas/adoption-receipt.schema.json".into(),
            parse_schema(RECEIPT_SCHEMA),
        ),
        (
            "https://p50.dev/schemas/adoption-journal.schema.json".into(),
            parse_schema(JOURNAL_SCHEMA),
        ),
        (GRAPH_ID.into(), parse_schema(GRAPH_SCHEMA)),
        (NODE_ID.into(), parse_schema(NODE_SCHEMA)),
        (EDGE_ID.into(), parse_schema(EDGE_SCHEMA)),
        (AGENT_ID.into(), parse_schema(AGENT_SCHEMA)),
        (EXTENSION_ID.into(), parse_schema(EXTENSION_SCHEMA)),
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

fn instance_validation_work_units(
    value: &serde_json::Value,
    max_values: usize,
    max_text_bytes: usize,
) -> Option<usize> {
    fn walk(
        value: &serde_json::Value,
        depth: usize,
        values: &mut usize,
        work_units: &mut usize,
        text_bytes: &mut usize,
        max_values: usize,
        max_text_bytes: usize,
    ) -> bool {
        if depth > MAX_JSON_DEPTH {
            return true;
        }
        let Some(next_values) = values.checked_add(1) else {
            return true;
        };
        *values = next_values;
        if *values > max_values {
            return true;
        }
        let Some(next_work_units) = work_units.checked_add(1) else {
            return true;
        };
        *work_units = next_work_units;
        match value {
            serde_json::Value::String(value) => {
                let Some(next_text_bytes) = text_bytes.checked_add(value.len()) else {
                    return true;
                };
                *text_bytes = next_text_bytes;
                *text_bytes > max_text_bytes
            }
            serde_json::Value::Array(values_in_array) => {
                for item in values_in_array {
                    let Some(next_work_units) = work_units.checked_add(1) else {
                        return true;
                    };
                    *work_units = next_work_units;
                    if walk(
                        item,
                        depth + 1,
                        values,
                        work_units,
                        text_bytes,
                        max_values,
                        max_text_bytes,
                    ) {
                        return true;
                    }
                }
                false
            }
            serde_json::Value::Object(values_in_object) => {
                for (key, item) in values_in_object {
                    let Some(next_work_units) = work_units.checked_add(1) else {
                        return true;
                    };
                    *work_units = next_work_units;
                    let Some(next_text_bytes) = text_bytes.checked_add(key.len()) else {
                        return true;
                    };
                    *text_bytes = next_text_bytes;
                    if *text_bytes > max_text_bytes
                        || walk(
                            item,
                            depth + 1,
                            values,
                            work_units,
                            text_bytes,
                            max_values,
                            max_text_bytes,
                        )
                    {
                        return true;
                    }
                }
                false
            }
            _ => false,
        }
    }

    let mut values = 0;
    let mut work_units = 0;
    let mut text_bytes = 0;
    if walk(
        value,
        0,
        &mut values,
        &mut work_units,
        &mut text_bytes,
        max_values,
        max_text_bytes,
    ) {
        None
    } else {
        Some(work_units)
    }
}

fn validation_work_is_bounded(
    schema_work_units: usize,
    expanded_work_units: usize,
    instance_work_units: usize,
) -> bool {
    // Reference expansion is a one-time evaluation cost. Only the deduplicated
    // reachable schema can scale with every bounded instance value; multiplying
    // the expanded graph by the whole document rejects valid repeated records.
    schema_work_units
        .checked_mul(instance_work_units)
        .and_then(|instance_work| instance_work.checked_add(expanded_work_units))
        .is_some_and(|work_units| work_units <= MAX_VALIDATION_WORK_UNITS)
}

fn parse_schema(source: &str) -> serde_json::Value {
    serde_json::from_str(source).expect("checked-in JSON schemas must parse")
}

pub fn validate_adoption_plan(value: &serde_json::Value) -> Vec<Diagnostic> {
    validate_embedded(
        "https://p50.dev/schemas/adoption-plan.schema.json",
        value,
        "adoption-plan",
    )
}
pub fn validate_activation_receipt(value: &serde_json::Value) -> Vec<Diagnostic> {
    validate_embedded(
        "https://p50.dev/schemas/activation-receipt.schema.json",
        value,
        "activation-receipt",
    )
}
pub fn validate_adoption_receipt(value: &serde_json::Value) -> Vec<Diagnostic> {
    validate_embedded(
        "https://p50.dev/schemas/adoption-receipt.schema.json",
        value,
        "adoption-receipt",
    )
}
pub fn validate_adoption_journal(value: &serde_json::Value) -> Vec<Diagnostic> {
    validate_embedded(
        "https://p50.dev/schemas/adoption-journal.schema.json",
        value,
        "adoption-journal",
    )
}
pub fn validate_restore_plan(value: &serde_json::Value) -> Vec<Diagnostic> {
    validate_embedded(
        "https://p50.dev/schemas/restore-plan.schema.json",
        value,
        "restore-plan",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex, MutexGuard};

    fn observer_lock() -> MutexGuard<'static, ()> {
        EMBEDDED_OBSERVER_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn valid_graph() -> serde_json::Value {
        serde_json::json!({
            "apiVersion": "p50.dev/graph/v1",
            "kind": "ExecutionGraph",
            "metadata": {
                "id": "graph-cache",
                "name": "Cache test graph",
                "executionId": "execution-cache",
                "version": 1
            },
            "spec": {
                "entrypoints": ["start"],
                "nodes": {
                    "start": {
                        "type": "tool",
                        "name": "Start",
                        "objective": "start the test graph",
                        "optionality": "required"
                    }
                },
                "edges": [],
                "budgets": {},
                "completion": {"status": "complete"}
            }
        })
    }

    fn valid_extension() -> serde_json::Value {
        serde_json::json!({
            "apiVersion": "p50.dev/v1",
            "kind": "Extension",
            "metadata": {
                "id": "extension-cache",
                "version": "1.0.0",
                "publisher": "graphhelm-tests"
            },
            "spec": {
                "type": "tool",
                "capabilities": [],
                "permissions": {},
                "contracts": {},
                "runtime": {"kind": "data", "isolationMinimum": "tier_0"},
                "compatibility": {}
            }
        })
    }

    fn valid_agent() -> serde_json::Value {
        serde_json::json!({
            "purpose": "validate cache behavior",
            "capabilities": ["read"],
            "inputSchema": "input",
            "outputSchema": "output",
            "completionContract": "done",
            "instructions": "Validate the supplied document."
        })
    }

    fn valid_waiver() -> serde_json::Value {
        serde_json::json!({
            "id": "waiver-cache",
            "requirement": "review",
            "executionId": "execution-cache",
            "graphVersion": 1,
            "actor": "owner-local",
            "reason": "accepted test risk",
            "acknowledgedRisks": ["test risk"],
            "scope": "execution",
            "createdAt": "2026-08-08T12:00:00Z",
            "expiresAt": null
        })
    }

    fn over_complex_instance() -> serde_json::Value {
        serde_json::Value::Array(
            std::iter::repeat_n(serde_json::Value::Null, MAX_VALIDATION_INSTANCE_VALUES + 1)
                .collect(),
        )
    }

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
                "$ref": "#item",
                "$defs": {"item": {"$anchor": "item", "type": "string"}}
            }),
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
    fn inline_reference_cycles_are_rejected_before_validation() {
        let schema = serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$defs": {
                "node": {
                    "type": ["array", "null"],
                    "allOf": [
                        {"items": {"$ref": "#/$defs/node"}},
                        {"items": {"$ref": "#/$defs/node"}}
                    ]
                }
            },
            "$ref": "#/$defs/node"
        });
        let mut instance = serde_json::Value::Null;
        for _ in 0..20 {
            instance = serde_json::json!([instance]);
        }

        assert_eq!(
            compile_inline_schema(&schema),
            Err(InlineSchemaError::LimitExceeded)
        );
        assert_eq!(
            validate_inline_value(&schema, &instance, "recursive-inline"),
            Err(InlineSchemaError::LimitExceeded)
        );
    }

    #[test]
    fn inline_acyclic_reference_expansion_has_a_deterministic_limit() {
        let mut definitions = serde_json::Map::new();
        definitions.insert("n24".into(), serde_json::Value::Bool(true));
        for index in (0..24).rev() {
            let next = format!("#/$defs/n{}", index + 1);
            definitions.insert(
                format!("n{index}"),
                serde_json::json!({"allOf": [{"$ref": next}, {"$ref": next}]}),
            );
        }
        let schema = serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$defs": definitions,
            "$ref": "#/$defs/n0"
        });

        assert_eq!(
            compile_inline_schema(&schema),
            Err(InlineSchemaError::LimitExceeded)
        );
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
    fn inline_dynamic_self_reference_is_bounded_and_recursive_ref_is_rejected() {
        let dynamic = serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$dynamicAnchor": "node",
            "$dynamicRef": "#node"
        });
        assert_eq!(compile_inline_schema(&dynamic), Ok(()));
        assert!(validate_inline_value(&dynamic, &serde_json::json!(null), "dynamic").is_ok());

        assert_eq!(
            compile_inline_schema(&serde_json::json!({"$recursiveRef": "#"})),
            Err(InlineSchemaError::Invalid)
        );
    }

    #[test]
    fn inline_value_validation_is_offline_redacted_and_pointer_stable() {
        let schema = serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "required": ["enabled"],
            "properties": {"enabled": {"type": "boolean"}},
            "additionalProperties": false
        });
        let value = serde_json::json!({"enabled": "secret-canary"});
        let diagnostics = validate_inline_value(&schema, &value, "inline-test").unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "GHS002_SCHEMA");
        assert_eq!(diagnostics[0].path, "/enabled");
        assert!(!format!("{diagnostics:?}").contains("secret-canary"));
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
    fn offline_schema_set_rejects_cross_resource_reference_cycles() {
        let resources = BTreeMap::from([
            (
                "a".into(),
                serde_json::json!({
                    "$schema": "https://json-schema.org/draft/2020-12/schema",
                    "$id": "https://p50.dev/schemas/cycle-a.json",
                    "$ref": "https://p50.dev/schemas/cycle-b.json"
                }),
            ),
            (
                "b".into(),
                serde_json::json!({
                    "$schema": "https://json-schema.org/draft/2020-12/schema",
                    "$id": "https://p50.dev/schemas/cycle-b.json",
                    "$ref": "https://p50.dev/schemas/cycle-a.json"
                }),
            ),
        ]);

        let diagnostics = match OfflineSchemaSet::compile(resources) {
            Err(diagnostics) => diagnostics,
            Ok(_) => panic!("cross-resource reference cycle was accepted"),
        };
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].path, "/resources/referenceGraph");
    }

    #[test]
    fn offline_schema_set_charges_referenced_resource_expansion_to_the_root() {
        let mut definitions = serde_json::Map::new();
        definitions.insert("n23".into(), serde_json::Value::Bool(true));
        for index in (0..23).rev() {
            let next = format!("#/$defs/n{}", index + 1);
            definitions.insert(
                format!("n{index}"),
                serde_json::json!({"allOf": [{"$ref": next}, {"$ref": next}]}),
            );
        }
        let resources = BTreeMap::from([
            (
                "root".into(),
                serde_json::json!({
                    "$schema": "https://json-schema.org/draft/2020-12/schema",
                    "$id": "https://p50.dev/schemas/expansion-root.json",
                    "allOf": [
                        {"$ref": "https://p50.dev/schemas/expansion-target.json#/$defs/n0"},
                        {"$ref": "https://p50.dev/schemas/expansion-target.json#/$defs/n0"}
                    ]
                }),
            ),
            (
                "target".into(),
                serde_json::json!({
                    "$schema": "https://json-schema.org/draft/2020-12/schema",
                    "$id": "https://p50.dev/schemas/expansion-target.json",
                    "$defs": definitions,
                    "$ref": "#/$defs/n0"
                }),
            ),
        ]);

        let diagnostics = match OfflineSchemaSet::compile(resources) {
            Err(diagnostics) => diagnostics,
            Ok(_) => panic!("cross-resource reference expansion was accepted"),
        };
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].path, "/resources/referenceGraph");
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
            // Regression: the JPD simulation emits a valid 35-event replay batch.
            "events":vec![event; 35]
        });
        let diagnostics = validate_physical_batch(&batch);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let mut invalid_digest = batch.clone();
        invalid_digest["requestDigest"] = serde_json::json!("not-a-sha256");
        assert!(!validate_physical_batch(&invalid_digest).is_empty());
        let mut extra = batch;
        extra["typedSerdeSentinel"] = serde_json::json!(true);
        assert!(!validate_physical_batch(&extra).is_empty());
    }

    #[test]
    fn embedded_public_validation_uses_shared_registry() {
        let _observer_lock = observer_lock();
        repository_schema_set().expect("checked-in embedded schemas compile");
        let before = embedded_compile_count();
        let diagnostics = validate_graph_value(&serde_json::json!({}), "embedded-cache");
        assert!(diagnostics.iter().any(|item| item.code == "GHS002_SCHEMA"));
        let after_public_validation = embedded_compile_count();

        assert_eq!(
            after_public_validation, before,
            "public validation compiled an embedded registry outside the shared cache"
        );
    }

    #[test]
    fn embedded_public_validators_match_explicit_set_for_valid_invalid_and_complex_values() {
        let _observer_lock = observer_lock();
        let compiled = OfflineSchemaSet::compile(embedded_resources())
            .expect("checked-in embedded schemas compile");
        let mut invalid_graph = valid_graph();
        invalid_graph["kind"] = serde_json::json!("NotAnExecutionGraph");
        let mut invalid_extension = valid_extension();
        invalid_extension["apiVersion"] = serde_json::json!("wrong/v1");
        let mut invalid_agent = valid_agent();
        invalid_agent["capabilities"] = serde_json::json!([]);
        let mut invalid_waiver = valid_waiver();
        invalid_waiver["acknowledgedRisks"] = serde_json::json!([]);

        let cases = [
            (
                GRAPH_ID,
                validate_graph_value as fn(&serde_json::Value, &str) -> Vec<Diagnostic>,
                valid_graph(),
                invalid_graph,
            ),
            (
                EXTENSION_ID,
                validate_extension_value,
                valid_extension(),
                invalid_extension,
            ),
            (AGENT_ID, validate_agent_value, valid_agent(), invalid_agent),
            (WAIVER_ID, validate_waiver, valid_waiver(), invalid_waiver),
        ];

        for (schema_id, validate, valid, invalid) in cases {
            for (case_name, value) in [("valid", valid), ("invalid", invalid)] {
                let source = format!("embedded-cache-{case_name}-{schema_id}");
                let expected = compiled.validate(schema_id, &value, &source);
                let actual = validate(&value, &source);
                assert_eq!(
                    actual, expected,
                    "diagnostics changed for {schema_id} {case_name}"
                );
            }
        }

        let complex = over_complex_instance();
        for (schema_id, validate) in [
            (
                GRAPH_ID,
                validate_graph_value as fn(&serde_json::Value, &str) -> Vec<Diagnostic>,
            ),
            (EXTENSION_ID, validate_extension_value),
            (AGENT_ID, validate_agent_value),
            (WAIVER_ID, validate_waiver),
        ] {
            let source = format!("embedded-cache-complex-{schema_id}");
            let expected = compiled.validate(schema_id, &complex, &source);
            let actual = validate(&complex, &source);
            assert_eq!(
                actual, expected,
                "complex diagnostics changed for {schema_id}"
            );
            assert_eq!(actual.len(), 1);
            assert_eq!(actual[0].code, "GHS002_SCHEMA");
            assert_eq!(actual[0].path, "/");
            assert_eq!(actual[0].source, source);
            assert_eq!(
                actual[0].message,
                "document exceeds deterministic validation complexity limits"
            );
        }
    }

    #[test]
    fn embedded_public_validators_are_concurrent_and_do_not_recompile() {
        let _observer_lock = observer_lock();
        repository_schema_set().expect("checked-in embedded schemas compile");
        let before = embedded_compile_count();
        let workers = (0..8)
            .map(|worker| {
                std::thread::spawn(move || {
                    let graph = validate_graph_value(&valid_graph(), &format!("graph-{worker}"));
                    assert!(graph.is_empty(), "graph diagnostics: {graph:?}");
                    let extension = validate_extension_value(
                        &valid_extension(),
                        &format!("extension-{worker}"),
                    );
                    assert!(extension.is_empty(), "extension diagnostics: {extension:?}");
                    let agent = validate_agent_value(&valid_agent(), &format!("agent-{worker}"));
                    assert!(agent.is_empty(), "agent diagnostics: {agent:?}");
                    let mut waiver = valid_waiver();
                    waiver["acknowledgedRisks"] = serde_json::json!([]);
                    let waiver_diagnostics = validate_waiver(&waiver, &format!("waiver-{worker}"));
                    assert!(
                        waiver_diagnostics
                            .iter()
                            .any(|diagnostic| diagnostic.path == "/acknowledgedRisks")
                    );
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker
                .join()
                .expect("concurrent validation worker panicked");
        }
        let after = embedded_compile_count();
        assert_eq!(
            after, before,
            "concurrent validation recompiled embedded schemas"
        );
    }

    #[test]
    fn arbitrary_schema_compilation_does_not_use_embedded_registry_cache() {
        let _observer_lock = observer_lock();
        repository_schema_set().expect("checked-in embedded schemas compile");
        let before = embedded_compile_count();
        let resources = BTreeMap::from([(
            "arbitrary-root".to_owned(),
            serde_json::json!({
                "$id": "https://p50.dev/schemas/arbitrary-root.json",
                "type": "object"
            }),
        )]);
        let set = OfflineSchemaSet::compile(resources).expect("arbitrary schema compiles");
        assert!(
            set.validate(
                "https://p50.dev/schemas/arbitrary-root.json",
                &serde_json::json!({}),
                "arbitrary"
            )
            .is_empty()
        );
        assert_eq!(embedded_compile_count(), before);
    }

    #[test]
    fn embedded_event_registry_compiles_at_most_once_across_thousands_of_validations() {
        let _observer_lock = observer_lock();
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
    }
}
