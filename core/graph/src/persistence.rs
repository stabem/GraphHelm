use std::collections::{BTreeMap, BTreeSet};

use graphhelm_protocols::{
    ContentFieldKind, ContentOwnerKind, ContentSlot, EvidenceId, EvidenceReference,
    GraphVersionRecord, OpaqueId, PersistedControl, PersistedGraphVersion, PersistedTopology,
    RawSha256, RepositoryScope, SafeValue, Sensitivity, WireHash,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{GraphError, canonical::sort_value};

const REFERENCE_PREFIX: &str = "refv1:";
const MAX_REFERENCE_BYTES: usize = 91;
const BASE64_URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
const MAX_CONTENT_SCAN_DEPTH: usize = 64;
const MAX_CONTENT_SCAN_VALUES: usize = 131_072;
const MAX_CONTENT_SCAN_BYTES: usize = 64 * 1024 * 1024;
const MAX_RECORD_STRING_BYTES: usize = 16 * 1024 * 1024;
const MAX_JWS_HEADER_SEGMENT_BYTES: usize = 16 * 1024;
pub const MAX_DRAFT_OPERATIONS: usize = 4096;

/// Allocation-bounded accounting shared by untrusted draft and graph preflights.
#[derive(Debug, Default)]
pub struct PersistencePreflight {
    value_count: usize,
    bytes: usize,
}

impl PersistencePreflight {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            value_count: 0,
            bytes: 0,
        }
    }

    pub fn account_string(&mut self, value: &str) -> Result<(), DurableContentError> {
        account_persistence_string(value, &mut self.value_count, &mut self.bytes)
    }

    pub fn account_collection(
        &self,
        length: usize,
        maximum: usize,
    ) -> Result<(), DurableContentError> {
        if length > maximum {
            Err(DurableContentError::LimitExceeded)
        } else {
            Ok(())
        }
    }

    /// Accounts one structural map/list container and enforces its independent cardinality cap.
    pub fn account_container(
        &mut self,
        length: usize,
        maximum: usize,
    ) -> Result<(), DurableContentError> {
        self.account_collection(length, maximum)?;
        self.account_scalar()
    }

    /// Accounts one non-string scalar without inventing serialized bytes.
    pub fn account_scalar(&mut self) -> Result<(), DurableContentError> {
        self.value_count = self
            .value_count
            .checked_add(1)
            .ok_or(DurableContentError::LimitExceeded)?;
        self.check()
    }

    pub fn account_value(&mut self, value: &Value) -> Result<(), DurableContentError> {
        let mut max_work_stack = 0;
        preflight_persistence_values_incremental(
            std::iter::once(value),
            &mut self.value_count,
            &mut self.bytes,
            &mut max_work_stack,
        )
    }

    fn check(&self) -> Result<(), DurableContentError> {
        if self.bytes > MAX_CONTENT_SCAN_BYTES || self.value_count > MAX_CONTENT_SCAN_VALUES {
            Err(DurableContentError::LimitExceeded)
        } else {
            Ok(())
        }
    }
}

/// Bounds a borrowed candidate before recursive serialization, lint, or policy work.
pub fn preflight_execution_graph(
    graph: &graphhelm_protocols::ExecutionGraph,
) -> Result<(), DurableContentError> {
    preflight_execution_graph_usage(graph).map(|_| ())
}

fn preflight_execution_graph_usage(
    graph: &graphhelm_protocols::ExecutionGraph,
) -> Result<PersistencePreflight, DurableContentError> {
    let mut usage = PersistencePreflight::new();
    account_execution_graph_usage(&mut usage, graph)?;
    Ok(usage)
}

fn account_execution_graph_usage(
    usage: &mut PersistencePreflight,
    graph: &graphhelm_protocols::ExecutionGraph,
) -> Result<(), DurableContentError> {
    usage.account_container(4, 4)?;
    for key in ["apiVersion", "kind", "metadata", "spec"] {
        usage.account_string(key)?;
    }
    for value in [graph.api_version.as_str(), graph.kind.as_str()] {
        usage.account_string(value)?;
    }

    let metadata_fields = 6 + graph.metadata.properties.len();
    usage.account_container(metadata_fields, 135)?;
    for key in ["id", "name", "executionId", "version", "basedOn", "labels"] {
        usage.account_string(key)?;
    }
    usage.account_scalar()?;
    usage.account_container(graph.metadata.labels.len(), 128)?;
    usage.account_container(graph.metadata.properties.len(), 128)?;
    for value in [
        graph.metadata.id.as_str(),
        graph.metadata.name.as_str(),
        graph.metadata.execution_id.as_str(),
    ] {
        usage.account_string(value)?;
    }
    if let Some(value) = graph.metadata.based_on.as_deref() {
        usage.account_string(value)?;
    } else {
        usage.account_scalar()?;
    }
    for (key, value) in &graph.metadata.labels {
        usage.account_string(key)?;
        usage.account_string(value)?;
    }
    for (key, value) in &graph.metadata.properties {
        usage.account_string(key)?;
        usage.account_value(value)?;
    }

    usage.account_container(6, 6)?;
    for key in [
        "entrypoints",
        "nodes",
        "edges",
        "budgets",
        "policies",
        "completion",
    ] {
        usage.account_string(key)?;
    }
    usage.account_container(graph.spec.entrypoints.len(), 64)?;
    usage.account_container(graph.spec.nodes.len(), 1024)?;
    usage.account_container(graph.spec.edges.len(), 4096)?;
    usage.account_container(graph.spec.policies.len(), 64)?;
    for entrypoint in &graph.spec.entrypoints {
        usage.account_string(entrypoint)?;
    }
    for (id, node) in &graph.spec.nodes {
        usage.account_string(id)?;
        usage.account_container(4 + node.properties.len(), 132)?;
        for key in ["type", "name", "objective", "optionality"] {
            usage.account_string(key)?;
        }
        usage.account_container(node.properties.len(), 128)?;
        usage.account_string(node.node_type.as_str())?;
        usage.account_string(&node.name)?;
        usage.account_string(&node.objective)?;
        usage.account_string(match node.optionality {
            graphhelm_protocols::Optionality::Required => "required",
            graphhelm_protocols::Optionality::Recommended => "recommended",
            graphhelm_protocols::Optionality::Optional => "optional",
        })?;
        for (key, value) in &node.properties {
            usage.account_string(key)?;
            usage.account_value(value)?;
        }
    }
    for edge in &graph.spec.edges {
        let optional_fields = usize::from(edge.payload_schema.is_some())
            + usize::from(edge.condition.is_some())
            + usize::from(edge.on_false.is_some())
            + usize::from(edge.on_unknown.is_some())
            + usize::from(edge.priority.is_some());
        usage.account_container(5 + optional_fields, 10)?;
        for key in ["id", "from", "to", "type", "map"] {
            usage.account_string(key)?;
        }
        usage.account_container(edge.bindings.len(), 128)?;
        for value in [&edge.id, &edge.from, &edge.to] {
            usage.account_string(value)?;
        }
        usage.account_string(match edge.edge_type {
            graphhelm_protocols::EdgeType::Control => "control",
            graphhelm_protocols::EdgeType::Data => "data",
            graphhelm_protocols::EdgeType::Evidence => "evidence",
            graphhelm_protocols::EdgeType::Event => "event",
            graphhelm_protocols::EdgeType::Failure => "failure",
            graphhelm_protocols::EdgeType::Compensation => "compensation",
            graphhelm_protocols::EdgeType::HumanApproval => "human_approval",
        })?;
        if let Some(value) = &edge.payload_schema {
            usage.account_string("payloadSchema")?;
            usage.account_string(value)?;
        }
        for (key, value) in &edge.bindings {
            usage.account_string(key)?;
            usage.account_string(value)?;
        }
        if let Some(value) = &edge.condition {
            usage.account_string("condition")?;
            usage.account_value(value)?;
        }
        if let Some(value) = &edge.on_false {
            usage.account_string("onFalse")?;
            usage.account_value(value)?;
        }
        if let Some(value) = &edge.on_unknown {
            usage.account_string("onUnknown")?;
            usage.account_string(match value {
                graphhelm_protocols::UnknownConditionBehavior::Pause => "pause",
                graphhelm_protocols::UnknownConditionBehavior::Fail => "fail",
                graphhelm_protocols::UnknownConditionBehavior::Skip => "skip",
                graphhelm_protocols::UnknownConditionBehavior::Route => "route",
            })?;
        }
        if edge.priority.is_some() {
            usage.account_string("priority")?;
            usage.account_scalar()?;
        }
    }

    let budget_fields = usize::from(graph.spec.budgets.max_nodes.is_some())
        + usize::from(graph.spec.budgets.max_depth.is_some())
        + usize::from(graph.spec.budgets.max_mutations.is_some())
        + usize::from(graph.spec.budgets.max_retries_per_node.is_some())
        + usize::from(graph.spec.budgets.max_wall_clock_seconds.is_some())
        + usize::from(graph.spec.budgets.max_api_cost_usd.is_some())
        + usize::from(graph.spec.budgets.max_parallel_model_calls.is_some());
    usage.account_container(budget_fields, 7)?;
    for (key, present) in [
        ("maxNodes", graph.spec.budgets.max_nodes.is_some()),
        ("maxDepth", graph.spec.budgets.max_depth.is_some()),
        ("maxMutations", graph.spec.budgets.max_mutations.is_some()),
        (
            "maxRetriesPerNode",
            graph.spec.budgets.max_retries_per_node.is_some(),
        ),
        (
            "maxWallClockSeconds",
            graph.spec.budgets.max_wall_clock_seconds.is_some(),
        ),
        (
            "maxApiCostUsd",
            graph.spec.budgets.max_api_cost_usd.is_some(),
        ),
        (
            "maxParallelModelCalls",
            graph.spec.budgets.max_parallel_model_calls.is_some(),
        ),
    ] {
        if present {
            usage.account_string(key)?;
            usage.account_scalar()?;
        }
    }
    for policy in &graph.spec.policies {
        usage.account_value(policy)?;
    }
    usage.account_value(&graph.spec.completion)?;
    Ok(())
}

/// Derives the only valid durable content-slot identity for one typed position.
///
/// The encoding is domain separated and length-prefixed so producers and replay
/// validators share one bounded, unambiguous identity function.
pub fn derive_content_slot_id(
    owner_kind: ContentOwnerKind,
    owner_id: &OpaqueId,
    field_kind: ContentFieldKind,
    ordinal: u32,
) -> Result<OpaqueId, GraphError> {
    let mut bytes = Vec::with_capacity(256);
    push_slot_position_part(&mut bytes, b"graphhelm-content-slot-position-v1")?;
    push_slot_position_part(&mut bytes, content_owner_kind_bytes(owner_kind))?;
    push_slot_position_part(&mut bytes, owner_id.as_str().as_bytes())?;
    push_slot_position_part(&mut bytes, content_field_kind_bytes(field_kind))?;
    bytes.extend_from_slice(&ordinal.to_be_bytes());
    let digest = raw_content_sha256(&bytes)?;
    OpaqueId::parse(format!("slot-{}", digest.as_str())).map_err(|_| GraphError::InvalidProjection)
}

/// Derives the publication-scoped Evidence identity for one exact typed slot.
pub fn derive_publication_evidence_id(
    scope: &RepositoryScope,
    version_number: u64,
    semantic_hash: &WireHash,
    slot: &ContentSlot,
) -> Result<EvidenceId, GraphError> {
    let mut bytes = Vec::with_capacity(512);
    push_slot_position_part(&mut bytes, b"graphhelm-publication-evidence-identity-v1")?;
    push_slot_position_part(&mut bytes, scope.workspace_id().as_str().as_bytes())?;
    push_slot_position_part(&mut bytes, scope.project_id().as_str().as_bytes())?;
    match scope.execution_id() {
        Some(execution_id) => {
            bytes.push(1);
            push_slot_position_part(&mut bytes, execution_id.as_str().as_bytes())?;
        }
        None => bytes.push(0),
    }
    bytes.extend_from_slice(&version_number.to_be_bytes());
    push_slot_position_part(&mut bytes, semantic_hash.as_str().as_bytes())?;
    push_slot_position_part(&mut bytes, slot.slot_id().as_str().as_bytes())?;
    push_slot_position_part(&mut bytes, content_owner_kind_bytes(slot.owner_kind()))?;
    push_slot_position_part(&mut bytes, slot.owner_id().as_str().as_bytes())?;
    push_slot_position_part(&mut bytes, content_field_kind_bytes(slot.field_kind()))?;
    bytes.extend_from_slice(&slot.ordinal().to_be_bytes());
    let digest = raw_content_sha256(&bytes)?;
    EvidenceId::parse(format!("evidence-{}", digest.as_str()))
        .map_err(|_| GraphError::InvalidProjection)
}

/// Verifies every slot uses its exact publication-scoped Evidence identity.
pub fn validate_publication_evidence_ids(
    scope: &RepositoryScope,
    version: &PersistedGraphVersion,
) -> Result<(), GraphError> {
    for slot in version.content_slots() {
        if derive_publication_evidence_id(scope, version.number(), version.semantic_hash(), slot)?
            != *slot.evidence_id()
        {
            return Err(GraphError::InvalidProjection);
        }
    }
    Ok(())
}

/// The only durable sensitivity and execution-availability profile for a typed content position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContentSlotProfile {
    sensitivity: Sensitivity,
    required_for_execution: bool,
}

impl ContentSlotProfile {
    pub const fn sensitivity(self) -> Sensitivity {
        self.sensitivity
    }

    pub const fn required_for_execution(self) -> bool {
        self.required_for_execution
    }
}

/// Derives the only valid durable profile without trusting serialized slot metadata.
pub fn derive_content_slot_profile(
    owner_kind: ContentOwnerKind,
    _owner_id: &OpaqueId,
    field_kind: ContentFieldKind,
    ordinal: u32,
) -> Result<ContentSlotProfile, GraphError> {
    let (sensitivity, required_for_execution) = match (owner_kind, field_kind, ordinal) {
        (ContentOwnerKind::Graph, ContentFieldKind::DisplayName, 0)
        | (ContentOwnerKind::Graph, ContentFieldKind::Description, 0)
        | (ContentOwnerKind::Node, ContentFieldKind::DisplayName, 0)
        | (ContentOwnerKind::Node, ContentFieldKind::Description, 0) => {
            (Sensitivity::Internal, false)
        }
        (ContentOwnerKind::Node, ContentFieldKind::Objective, 0)
        | (ContentOwnerKind::Node, ContentFieldKind::Instructions, 0)
        | (ContentOwnerKind::Agent, ContentFieldKind::Purpose, 0)
        | (ContentOwnerKind::Agent, ContentFieldKind::Instructions, 0)
        | (ContentOwnerKind::Agent, ContentFieldKind::CompletionContract, 0)
        | (ContentOwnerKind::Edge, ContentFieldKind::PolicyText, 0..=1) => {
            (Sensitivity::Restricted, true)
        }
        (ContentOwnerKind::Node, ContentFieldKind::CompletionContract, _)
        | (ContentOwnerKind::Policy, ContentFieldKind::PolicyText, _)
        | (ContentOwnerKind::Node, ContentFieldKind::ContextPath, _)
        | (ContentOwnerKind::Node, ContentFieldKind::PermissionPath, _)
        | (ContentOwnerKind::Node, ContentFieldKind::IsolationPath, _) => {
            (Sensitivity::Restricted, true)
        }
        _ => return Err(GraphError::InvalidProjection),
    };
    Ok(ContentSlotProfile {
        sensitivity,
        required_for_execution,
    })
}

fn push_slot_position_part(output: &mut Vec<u8>, value: &[u8]) -> Result<(), GraphError> {
    let length = u32::try_from(value.len()).map_err(|_| GraphError::InvalidProjection)?;
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(value);
    Ok(())
}

const fn content_owner_kind_bytes(owner_kind: ContentOwnerKind) -> &'static [u8] {
    match owner_kind {
        ContentOwnerKind::Graph => b"graph".as_slice(),
        ContentOwnerKind::Node => b"node".as_slice(),
        ContentOwnerKind::Agent => b"agent".as_slice(),
        ContentOwnerKind::Edge => b"edge".as_slice(),
        ContentOwnerKind::Policy => b"policy".as_slice(),
        ContentOwnerKind::Diagnostic => b"diagnostic".as_slice(),
    }
}

const fn content_field_kind_bytes(field_kind: ContentFieldKind) -> &'static [u8] {
    match field_kind {
        ContentFieldKind::DisplayName => b"display_name".as_slice(),
        ContentFieldKind::Description => b"description".as_slice(),
        ContentFieldKind::Objective => b"objective".as_slice(),
        ContentFieldKind::Purpose => b"purpose".as_slice(),
        ContentFieldKind::Instructions => b"instructions".as_slice(),
        ContentFieldKind::CompletionContract => b"completion_contract".as_slice(),
        ContentFieldKind::PolicyText => b"policy_text".as_slice(),
        ContentFieldKind::DiagnosticDetail => b"diagnostic_detail".as_slice(),
        ContentFieldKind::ContextPath => b"context_path".as_slice(),
        ContentFieldKind::PermissionPath => b"permission_path".as_slice(),
        ContentFieldKind::IsolationPath => b"isolation_path".as_slice(),
    }
}

/// Redacted result of the shared durable-content gate.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum DurableContentError {
    #[error("durable content is not safe")]
    Unsafe,
    #[error("durable content exceeds a deterministic limit")]
    LimitExceeded,
}

/// Applies the same bounded content gate used by authoring and replay.
pub fn validate_durable_content(
    value: &Value,
    additional_strings: &[&str],
) -> Result<(), DurableContentError> {
    preflight_json_structure(value)?;
    let mut scanner = DurableContentScanner::default();
    scanner.scan_json(value, 0)?;
    for value in additional_strings {
        scanner.scan_text(value)?;
    }
    Ok(())
}

/// Iteratively bounds untrusted authoring values before recursive canonicalization.
pub fn preflight_persistence_values<'a>(
    values: impl IntoIterator<Item = &'a Value>,
) -> Result<(), DurableContentError> {
    preflight_persistence_values_with_usage(values, 0, 0)
}

fn preflight_persistence_values_with_usage<'a>(
    values: impl IntoIterator<Item = &'a Value>,
    mut value_count: usize,
    mut bytes: usize,
) -> Result<(), DurableContentError> {
    let mut max_work_stack = 0;
    preflight_persistence_values_incremental(
        values,
        &mut value_count,
        &mut bytes,
        &mut max_work_stack,
    )
}

enum PersistenceFrame<'a> {
    Array {
        values: std::slice::Iter<'a, Value>,
        child_depth: usize,
    },
    Object {
        values: serde_json::map::Iter<'a>,
        child_depth: usize,
    },
}

impl<'a> PersistenceFrame<'a> {
    fn next(&mut self) -> Option<(&'a Value, usize, Option<&'a str>)> {
        match self {
            Self::Array {
                values,
                child_depth,
            } => values.next().map(|value| (value, *child_depth, None)),
            Self::Object {
                values,
                child_depth,
            } => values
                .next()
                .map(|(key, value)| (value, *child_depth, Some(key.as_str()))),
        }
    }
}

fn preflight_persistence_values_incremental<'a>(
    values: impl IntoIterator<Item = &'a Value>,
    value_count: &mut usize,
    bytes: &mut usize,
    max_work_stack: &mut usize,
) -> Result<(), DurableContentError> {
    for root in values {
        let mut frames = Vec::with_capacity(MAX_CONTENT_SCAN_DEPTH);
        let mut current = Some((root, 0usize));
        loop {
            if let Some((value, depth)) = current.take() {
                if depth > MAX_CONTENT_SCAN_DEPTH {
                    return Err(DurableContentError::LimitExceeded);
                }
                match value {
                    Value::String(value) => {
                        account_persistence_string(value, value_count, bytes)?;
                    }
                    Value::Array(items) if !items.is_empty() => {
                        account_persistence_scalar(value_count, *bytes)?;
                        if depth >= MAX_CONTENT_SCAN_DEPTH {
                            return Err(DurableContentError::LimitExceeded);
                        }
                        frames.push(PersistenceFrame::Array {
                            values: items.iter(),
                            child_depth: depth + 1,
                        });
                        *max_work_stack = (*max_work_stack).max(frames.len());
                    }
                    Value::Object(object) if !object.is_empty() => {
                        account_persistence_scalar(value_count, *bytes)?;
                        if depth >= MAX_CONTENT_SCAN_DEPTH {
                            return Err(DurableContentError::LimitExceeded);
                        }
                        frames.push(PersistenceFrame::Object {
                            values: object.iter(),
                            child_depth: depth + 1,
                        });
                        *max_work_stack = (*max_work_stack).max(frames.len());
                    }
                    Value::Array(_)
                    | Value::Object(_)
                    | Value::Null
                    | Value::Bool(_)
                    | Value::Number(_) => account_persistence_scalar(value_count, *bytes)?,
                }
            }

            while let Some(frame) = frames.last_mut() {
                if let Some((value, depth, key)) = frame.next() {
                    if let Some(key) = key {
                        account_persistence_string(key, value_count, bytes)?;
                    }
                    current = Some((value, depth));
                    break;
                }
                frames.pop();
            }
            if current.is_none() && frames.is_empty() {
                break;
            }
        }
    }
    Ok(())
}

fn account_persistence_string(
    value: &str,
    value_count: &mut usize,
    bytes: &mut usize,
) -> Result<(), DurableContentError> {
    if value.len() > MAX_RECORD_STRING_BYTES {
        return Err(DurableContentError::LimitExceeded);
    }
    *bytes = bytes
        .checked_add(value.len())
        .ok_or(DurableContentError::LimitExceeded)?;
    *value_count = value_count
        .checked_add(1)
        .ok_or(DurableContentError::LimitExceeded)?;
    check_persistence_usage(*value_count, *bytes)
}

fn account_persistence_scalar(
    value_count: &mut usize,
    bytes: usize,
) -> Result<(), DurableContentError> {
    *value_count = value_count
        .checked_add(1)
        .ok_or(DurableContentError::LimitExceeded)?;
    check_persistence_usage(*value_count, bytes)
}

fn check_persistence_usage(value_count: usize, bytes: usize) -> Result<(), DurableContentError> {
    if value_count > MAX_CONTENT_SCAN_VALUES || bytes > MAX_CONTENT_SCAN_BYTES {
        Err(DurableContentError::LimitExceeded)
    } else {
        Ok(())
    }
}

#[cfg(test)]
fn preflight_persistence_values_with_metrics<'a>(
    values: impl IntoIterator<Item = &'a Value>,
) -> (Result<(), DurableContentError>, usize) {
    let mut value_count = 0;
    let mut bytes = 0;
    let mut max_work_stack = 0;
    let result = preflight_persistence_values_incremental(
        values,
        &mut value_count,
        &mut bytes,
        &mut max_work_stack,
    );
    (result, max_work_stack)
}

/// Inventories the complete serialized authoring record before cloning it.
pub fn preflight_graph_version_record_values(
    record: &GraphVersionRecord,
) -> Result<(), DurableContentError> {
    preflight_graph_version_record_usage(record).map(|_| ())
}

fn preflight_graph_version_record_usage(
    record: &GraphVersionRecord,
) -> Result<PersistencePreflight, DurableContentError> {
    let mut usage = PersistencePreflight::new();
    usage.account_container(6, 6)?;
    for key in [
        "graph",
        "predecessor",
        "semantic",
        "contentHash",
        "createdBy",
        "createdAt",
    ] {
        usage.account_string(key)?;
    }

    account_execution_graph_usage(&mut usage, &record.graph)?;
    if let Some(predecessor) = &record.predecessor {
        usage.account_container(2, 2)?;
        usage.account_string("number")?;
        usage.account_scalar()?;
        usage.account_string("contentHash")?;
        usage.account_string(predecessor.content_hash.as_str())?;
    } else {
        usage.account_scalar()?;
    }
    usage.account_value(&record.semantic)?;
    usage.account_string(record.content_hash.as_str())?;

    usage.account_container(2, 2)?;
    usage.account_string("type")?;
    usage.account_string(match record.created_by.actor_type {
        graphhelm_protocols::ActorType::Owner => "owner",
        graphhelm_protocols::ActorType::Human => "human",
        graphhelm_protocols::ActorType::Agent => "agent",
        graphhelm_protocols::ActorType::System => "system",
    })?;
    usage.account_string("id")?;
    usage.account_string(&record.created_by.id)?;

    let created_at = record
        .created_at
        .to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true);
    usage.account_string(&created_at)?;
    Ok(usage)
}

fn preflight_json_structure(value: &Value) -> Result<(), DurableContentError> {
    preflight_persistence_values(std::iter::once(value))
}

#[derive(Default)]
struct DurableContentScanner {
    values: usize,
    bytes: usize,
}

impl DurableContentScanner {
    fn scan_json(&mut self, value: &Value, depth: usize) -> Result<(), DurableContentError> {
        if depth > MAX_CONTENT_SCAN_DEPTH {
            return Err(DurableContentError::LimitExceeded);
        }
        self.values = self
            .values
            .checked_add(1)
            .ok_or(DurableContentError::LimitExceeded)?;
        if self.values > MAX_CONTENT_SCAN_VALUES {
            return Err(DurableContentError::LimitExceeded);
        }
        match value {
            Value::String(value) => self.scan_text(value),
            Value::Array(values) => {
                for value in values {
                    self.scan_json(value, depth + 1)?;
                }
                Ok(())
            }
            Value::Object(values) => {
                for (key, value) in values {
                    self.scan_text(key)?;
                    self.scan_json(value, depth + 1)?;
                }
                Ok(())
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => Ok(()),
        }
    }

    fn scan_text(&mut self, value: &str) -> Result<(), DurableContentError> {
        self.bytes = self
            .bytes
            .checked_add(value.len())
            .ok_or(DurableContentError::LimitExceeded)?;
        if self.bytes > MAX_CONTENT_SCAN_BYTES {
            return Err(DurableContentError::LimitExceeded);
        }
        let mut operations = 0;
        if is_secret_shaped_counted(value, &mut operations) {
            return Err(DurableContentError::Unsafe);
        }
        Ok(())
    }
}

fn is_secret_shaped_counted(text: &str, operations: &mut usize) -> bool {
    // Compact JWS segments are case-sensitive, so structural detection must run
    // over the original bytes before the case-folded scans below.
    if contains_jwt_linear(text.as_bytes(), operations) {
        return true;
    }
    let lower = text
        .bytes()
        .map(|byte| {
            *operations = operations.saturating_add(1);
            byte.to_ascii_lowercase()
        })
        .collect::<Vec<_>>();
    [
        (b"ghp_".as_slice(), 16),
        (b"ghp-".as_slice(), 16),
        (b"github_pat_".as_slice(), 16),
        (b"github-pat-".as_slice(), 16),
        (b"sk-".as_slice(), 20),
        (b"sk_".as_slice(), 20),
        (b"akia".as_slice(), 16),
        (b"asia".as_slice(), 16),
        (b"aida".as_slice(), 16),
        (b"aroa".as_slice(), 16),
        (b"aipa".as_slice(), 16),
        (b"anpa".as_slice(), 16),
        (b"anva".as_slice(), 16),
        (b"glpat-".as_slice(), 16),
        (b"glpat_".as_slice(), 16),
        (b"xoxb-".as_slice(), 20),
        (b"xoxb_".as_slice(), 20),
        (b"xoxp-".as_slice(), 20),
        (b"xoxp_".as_slice(), 20),
        (b"xoxa-".as_slice(), 20),
        (b"xoxa_".as_slice(), 20),
        (b"xoxr-".as_slice(), 20),
        (b"xoxr_".as_slice(), 20),
        (b"xoxs-".as_slice(), 20),
        (b"xoxs_".as_slice(), 20),
    ]
    .into_iter()
    .any(|(prefix, tail)| contains_prefixed_secret(&lower, prefix, tail, operations))
        || contains_authorization_secret(&lower, operations)
        || contains_environment_secret_uri(&lower, operations)
        || contains_reference_secret_name(&lower, operations)
        || contains_compact_pem(&lower, operations)
        || [
            b"secret://".as_slice(),
            b"env://".as_slice(),
            b"vault://".as_slice(),
            b"credential://".as_slice(),
        ]
        .into_iter()
        .any(|marker| contains_bytes(&lower, marker, operations))
}

fn contains_prefixed_secret(
    text: &[u8],
    prefix: &[u8],
    minimum_tail: usize,
    operations: &mut usize,
) -> bool {
    if text.len() < prefix.len() + minimum_tail {
        return false;
    }
    for start in 0..=text.len() - prefix.len() {
        if !matches_at(text, start, prefix, operations) {
            continue;
        }
        let mut tail = 0;
        while tail < minimum_tail {
            *operations = operations.saturating_add(1);
            let Some(byte) = text.get(start + prefix.len() + tail) else {
                break;
            };
            if !(byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')) {
                break;
            }
            tail += 1;
        }
        if tail == minimum_tail {
            return true;
        }
    }
    false
}

fn contains_jwt_linear(text: &[u8], operations: &mut usize) -> bool {
    let mut previous: Option<(usize, bool)> = None;
    let mut before_previous: Option<(usize, bool)> = None;
    let mut cursor = 0;
    let mut joined = false;
    while cursor < text.len() {
        *operations = operations.saturating_add(1);
        if !is_base64url(text[cursor]) {
            joined = text[cursor] == b'.';
            if !joined {
                previous = None;
                before_previous = None;
            }
            cursor += 1;
            continue;
        }
        let start = cursor;
        while cursor < text.len() && is_base64url(text[cursor]) {
            *operations = operations.saturating_add(1);
            cursor += 1;
        }
        let segment_bytes = &text[start..cursor];
        let jwt_start = structural_jws_header(segment_bytes, operations);
        let segment = (cursor - start, jwt_start);
        if joined
            && before_previous.is_some_and(|(_, eligible)| eligible)
            && previous.is_some_and(|(length, _)| length >= 8)
            && segment.0 >= 8
        {
            return true;
        }
        if joined {
            before_previous = previous;
        } else {
            before_previous = None;
        }
        previous = Some(segment);
        joined = cursor < text.len() && text[cursor] == b'.';
    }
    false
}

fn structural_jws_header(segment: &[u8], operations: &mut usize) -> bool {
    *operations = operations.saturating_add(segment.len().saturating_mul(3));
    if segment.is_empty() {
        return false;
    }
    // An otherwise compact-token-shaped candidate with an oversized header is
    // rejected conservatively without allocating in proportion to the header.
    if segment.len() > MAX_JWS_HEADER_SEGMENT_BYTES {
        return true;
    }
    let Ok(encoded) = std::str::from_utf8(segment) else {
        return false;
    };
    let Ok(decoded) = decode_base64url(encoded) else {
        return false;
    };
    let Ok(Value::Object(header)) = serde_json::from_slice::<Value>(&decoded) else {
        return false;
    };
    header
        .get("alg")
        .and_then(Value::as_str)
        .is_some_and(|algorithm| !algorithm.is_empty())
}

const fn is_base64url(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}

fn contains_authorization_secret(text: &[u8], operations: &mut usize) -> bool {
    let mut cursor = 0;
    while cursor < text.len() {
        let (scheme_start, next) = if matches_at(text, cursor, b"authorization", operations) {
            let next = skip_separators(text, cursor + b"authorization".len(), operations);
            (next, next)
        } else {
            (cursor, cursor + 1)
        };
        let scheme_len = if matches_at(text, scheme_start, b"bearer", operations) {
            6
        } else if matches_at(text, scheme_start, b"basic", operations) {
            5
        } else {
            cursor = next;
            continue;
        };
        let tail = skip_separators(text, scheme_start + scheme_len, operations);
        if tail > scheme_start + scheme_len && has_secret_tail(text, tail, 8, operations) {
            return true;
        }
        cursor = next.max(scheme_start + scheme_len);
    }
    false
}

fn skip_separators(text: &[u8], mut cursor: usize, operations: &mut usize) -> usize {
    while let Some(byte) = text.get(cursor) {
        *operations = operations.saturating_add(1);
        if !(byte.is_ascii_whitespace() || matches!(byte, b':' | b'=' | b'_' | b'-')) {
            break;
        }
        cursor += 1;
    }
    cursor
}

fn has_secret_tail(text: &[u8], start: usize, minimum: usize, operations: &mut usize) -> bool {
    (0..minimum).all(|offset| {
        *operations = operations.saturating_add(1);
        text.get(start + offset).is_some_and(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'+' | b'/')
        })
    })
}

fn contains_environment_secret_uri(text: &[u8], operations: &mut usize) -> bool {
    let marker = b"environment://";
    let mut cursor = 0;
    while cursor + marker.len() <= text.len() {
        if !matches_at(text, cursor, marker, operations) {
            cursor += 1;
            continue;
        }
        let start = cursor + marker.len();
        let mut end = start;
        while end < text.len()
            && !text[end].is_ascii_whitespace()
            && !matches!(text[end], b'\'' | b'"' | b',' | b';' | b')' | b']' | b'}')
        {
            *operations = operations.saturating_add(1);
            end += 1;
        }
        if compact_contains_secret_name(&text[start..end], operations) {
            return true;
        }
        cursor = end.max(cursor + 1);
    }
    false
}

fn compact_contains_secret_name(text: &[u8], operations: &mut usize) -> bool {
    let compact = text
        .iter()
        .filter_map(|byte| {
            *operations = operations.saturating_add(1);
            byte.is_ascii_alphanumeric().then_some(*byte)
        })
        .collect::<Vec<_>>();
    [
        b"secret".as_slice(),
        b"password".as_slice(),
        b"passwd".as_slice(),
        b"credential".as_slice(),
        b"token".as_slice(),
        b"privatekey".as_slice(),
        b"accesskey".as_slice(),
        b"apikey".as_slice(),
        b"databaseurl".as_slice(),
        b"connectionstring".as_slice(),
    ]
    .into_iter()
    .any(|marker| contains_bytes(&compact, marker, operations))
}

fn contains_reference_secret_name(text: &[u8], operations: &mut usize) -> bool {
    if !contains_bytes(text, b"://", operations)
        && !text.starts_with(b"project/")
        && !text.starts_with(b"builtin/")
    {
        return false;
    }
    compact_contains_secret_name(text, operations)
}

fn contains_compact_pem(text: &[u8], operations: &mut usize) -> bool {
    let markers = [
        b"beginprivatekey".as_slice(),
        b"beginrsaprivatekey".as_slice(),
        b"beginecprivatekey".as_slice(),
        b"beginopensshprivatekey".as_slice(),
    ];
    let mut matched = [0_usize; 4];
    for byte in text.iter().copied() {
        *operations = operations.saturating_add(1);
        if !byte.is_ascii_alphanumeric() {
            continue;
        }
        for (index, marker) in markers.iter().enumerate() {
            *operations = operations.saturating_add(1);
            let state = &mut matched[index];
            if byte == marker[*state] {
                *state += 1;
                if *state == marker.len() {
                    return true;
                }
            } else {
                *state = usize::from(byte == marker[0]);
            }
        }
    }
    false
}

fn contains_bytes(text: &[u8], needle: &[u8], operations: &mut usize) -> bool {
    if needle.len() > text.len() {
        return false;
    }
    (0..=text.len() - needle.len()).any(|start| matches_at(text, start, needle, operations))
}

fn matches_at(text: &[u8], start: usize, needle: &[u8], operations: &mut usize) -> bool {
    needle.iter().enumerate().all(|(offset, expected)| {
        *operations = operations.saturating_add(1);
        text.get(start + offset) == Some(expected)
    })
}

#[cfg(test)]
fn durable_content_scan_operation_count(text: &str) -> usize {
    let mut operations = 0;
    let _ = is_secret_shaped_counted(text, &mut operations);
    operations
}

/// Encodes one registered non-secret reference into canonical bounded wire-safe form.
pub fn encode_persisted_reference(value: &str) -> Result<SafeValue, GraphError> {
    if value.len() > MAX_REFERENCE_BYTES || !is_registered_reference(value) {
        return Err(GraphError::InvalidProjection);
    }
    let mut encoded = String::with_capacity(REFERENCE_PREFIX.len() + value.len().div_ceil(3) * 4);
    encoded.push_str(REFERENCE_PREFIX);
    encode_base64url(value.as_bytes(), &mut encoded);
    SafeValue::parse(encoded).map_err(|_| GraphError::InvalidProjection)
}

/// Decodes and revalidates one canonical `refv1` value without exposing malformed input.
pub fn decode_persisted_reference(encoded: &str) -> Result<String, GraphError> {
    if encoded.len() > 128 {
        return Err(GraphError::InvalidProjection);
    }
    let payload = encoded
        .strip_prefix(REFERENCE_PREFIX)
        .ok_or(GraphError::InvalidProjection)?;
    if payload.is_empty() || payload.len() % 4 == 1 || payload.contains('=') {
        return Err(GraphError::InvalidProjection);
    }
    let bytes = decode_base64url(payload)?;
    let decoded = String::from_utf8(bytes).map_err(|_| GraphError::InvalidProjection)?;
    if decoded.len() > MAX_REFERENCE_BYTES || !is_registered_reference(&decoded) {
        return Err(GraphError::InvalidProjection);
    }
    let canonical = encode_persisted_reference(&decoded)?;
    if canonical.as_str() != encoded {
        return Err(GraphError::InvalidProjection);
    }
    Ok(decoded)
}

/// Parses a bounded nominal selector that carries no URI or filesystem semantics.
pub fn parse_persisted_nominal_identifier(value: &str) -> Result<SafeValue, GraphError> {
    let bytes = value.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 64
        || !bytes[0].is_ascii_alphanumeric()
        || !bytes.iter().all(|byte| {
            byte.is_ascii_digit() || byte.is_ascii_lowercase() || matches!(byte, b'_' | b'-')
        })
    {
        return Err(GraphError::InvalidProjection);
    }
    SafeValue::parse(value).map_err(|_| GraphError::InvalidProjection)
}

/// Parses one closed, bounded authoring binding expression retained inline.
///
/// URI-like bindings use `refv1`; raw bindings are limited to the normative
/// `outputs.<node>.<field...>` and `nodes.<node>.output[.<field...>]` forms.
pub fn parse_persisted_binding(value: &str) -> Result<SafeValue, GraphError> {
    parse_persisted_binding_reference_node(value)?;
    SafeValue::parse(value).map_err(|_| GraphError::InvalidProjection)
}

/// Validates a persisted binding and returns its referenced topology node, if any.
pub fn parse_persisted_binding_reference_node(value: &str) -> Result<Option<&str>, GraphError> {
    if value.starts_with(REFERENCE_PREFIX) {
        let decoded = decode_persisted_reference(value)?;
        if !is_binding_reference(&decoded) {
            return Err(GraphError::InvalidProjection);
        }
        return Ok(None);
    }
    if value.len() > 128 {
        return Err(GraphError::InvalidProjection);
    }
    let parts = value.split('.').collect::<Vec<_>>();
    let lower = value
        .bytes()
        .map(|byte| byte.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let mut operations = 0;
    if !(3..=8).contains(&parts.len())
        || !matches!(parts[0], "outputs" | "nodes")
        || !valid_binding_owner(parts[1])
        || parts[2..].iter().any(|part| !valid_binding_token(part))
        || (parts[0] == "nodes" && parts[2] != "output")
        || compact_contains_secret_name(&lower, &mut operations)
    {
        return Err(GraphError::InvalidProjection);
    }
    Ok(Some(parts[1]))
}

fn is_binding_reference(value: &str) -> bool {
    is_artifact_reference(value)
        || slash_reference(value, "context://", 1, 4)
        || environment_reference(value)
}

/// Returns whether a value is one of the four closed authoring isolation tiers.
#[must_use]
pub fn is_valid_isolation_tier(value: &str) -> bool {
    matches!(value, "tier_0" | "tier_1" | "tier_2" | "tier_3")
}

fn valid_binding_owner(value: &str) -> bool {
    OpaqueId::parse(value).is_ok() && valid_binding_token(value)
}

fn valid_binding_token(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

/// Validates every registered durable reference position and rejects encodings elsewhere.
pub fn validate_persisted_references(version: &PersistedGraphVersion) -> Result<(), GraphError> {
    let topology = version.topology();
    for value in topology.labels().values() {
        reject_unexpected_reference(value)?;
    }
    for node in topology.nodes().values() {
        for control in node.controls() {
            validate_persisted_control_references(control)?;
        }
    }
    for edge in topology.edges() {
        for value in edge.bindings().values() {
            require_binding(value)?;
        }
        if let Some(condition) = edge.condition() {
            validate_persisted_control_references(condition)?;
        }
    }
    for policy in topology.policies() {
        validate_persisted_control_references(policy)?;
    }
    validate_persisted_control_references(topology.completion())
}

fn validate_projection_grammar(version: &PersistedGraphVersion) -> Result<(), GraphError> {
    let topology = version.topology();
    for node in topology.nodes().values() {
        let mut seen = BTreeSet::new();
        let mut last_order = None;
        for control in node.controls() {
            let order = persisted_node_control_order(control.control_type().as_str())
                .ok_or(GraphError::InvalidProjection)?;
            if last_order.is_some_and(|previous| previous >= order)
                || !seen.insert(control.control_type().as_str())
                || !control_allowed_for_node(
                    control.control_type().as_str(),
                    node.node_type().as_str(),
                )
            {
                return Err(GraphError::InvalidProjection);
            }
            last_order = Some(order);
            validate_control_shape(control)?;
            if control.control_type().as_str() == "node_configuration"
                && identifier(control, "targetRef").is_some()
                && node.node_type().as_str() != "deploy"
            {
                return Err(GraphError::InvalidProjection);
            }
        }
    }
    for edge in topology.edges() {
        if let Some(condition) = edge.condition() {
            if condition.control_type().as_str() != "edge_condition" {
                return Err(GraphError::InvalidProjection);
            }
            validate_control_shape(condition)?;
        }
    }
    for policy in topology.policies() {
        if policy.control_type().as_str() != "policy_control" {
            return Err(GraphError::InvalidProjection);
        }
        validate_control_shape(policy)?;
    }
    if topology.completion().control_type().as_str() != "graph_completion" {
        return Err(GraphError::InvalidProjection);
    }
    validate_control_shape(topology.completion())
}

fn control_allowed_for_node(control_type: &str, node_type: &str) -> bool {
    match control_type {
        "agent_configuration"
        | "agent_model_requirements"
        | "agent_context_strategy"
        | "agent_evidence_requirements"
        | "agent_memory_policy"
        | "node_agents" => node_type == "agent",
        "tool_configuration" => node_type == "tool",
        "classifier_configuration" => node_type == "classifier",
        "gate_configuration" => node_type == "gate",
        "fork_configuration" => node_type == "fork",
        "join_configuration" => node_type == "join",
        "human_decision" => node_type == "human_decision",
        "subgraph_configuration" => node_type == "subgraph",
        "materializer_configuration" => node_type == "materializer",
        "deploy_configuration" => node_type == "deploy",
        "rollback_configuration" => node_type == "rollback",
        "node_common" | "input_contract" | "output_contract" | "node_model" | "node_context"
        | "node_permissions" | "node_isolation" | "node_retry" | "node_resources"
        | "node_memory" | "node_completion" | "node_configuration" | "node_loop" => true,
        _ => false,
    }
}

/// Returns the single canonical position for every registered node control.
pub fn persisted_node_control_order(control_type: &str) -> Option<u8> {
    Some(match control_type {
        "agent_configuration" => 0,
        "agent_context_strategy" => 1,
        "agent_evidence_requirements" => 2,
        "agent_memory_policy" => 3,
        "agent_model_requirements" => 4,
        "node_agents" => 5,
        "input_contract" => 10,
        "output_contract" => 11,
        "node_model" => 20,
        "node_context" => 21,
        "node_permissions" => 22,
        "node_isolation" => 23,
        "node_retry" => 24,
        "node_resources" => 25,
        "node_memory" => 26,
        "node_completion" => 27,
        "node_common" => 28,
        "node_loop" => 29,
        "tool_configuration"
        | "classifier_configuration"
        | "gate_configuration"
        | "fork_configuration"
        | "join_configuration"
        | "human_decision"
        | "subgraph_configuration"
        | "materializer_configuration"
        | "deploy_configuration"
        | "rollback_configuration" => 30,
        "node_configuration" => 31,
        _ => return None,
    })
}

fn validate_control_shape(control: &PersistedControl) -> Result<(), GraphError> {
    let control_type = control.control_type().as_str();
    let contract = matches!(control_type, "input_contract" | "output_contract");
    if control
        .digests()
        .keys()
        .any(|key| !contract || key.as_str() != "schema")
        || control
            .identifiers()
            .keys()
            .any(|key| !identifier_key_allowed(control_type, key.as_str()))
        || control
            .integers()
            .keys()
            .any(|key| !integer_key_allowed(control_type, key.as_str()))
        || control
            .flags()
            .keys()
            .any(|key| !flag_key_allowed(control_type, key.as_str()))
        || control.identifiers().iter().any(|(key, value)| {
            !identifier_value_allowed(control_type, key.as_str(), value.as_str())
        })
    {
        return Err(GraphError::InvalidProjection);
    }
    if control_type == "edge_condition" {
        if control.identifiers().is_empty()
            || !control.integers().is_empty()
            || !control.flags().is_empty()
        {
            return Err(GraphError::InvalidProjection);
        }
    } else if control_type == "terminal_nodes" {
        if control.identifiers().len() != 1
            || !control.identifiers().contains_key(
                &graphhelm_protocols::SafeKey::parse("terminalNode")
                    .map_err(|_| GraphError::InvalidProjection)?,
            )
            || !control.integers().is_empty()
            || control.flags().len() != 1
            || control.flags().values().next() != Some(&false)
        {
            return Err(GraphError::InvalidProjection);
        }
    } else if control
        .flags()
        .iter()
        .find(|(key, _)| key.as_str() == "present")
        .map(|(_, value)| *value)
        != Some(true)
    {
        return Err(GraphError::InvalidProjection);
    }
    if !matches!(
        control_type,
        "edge_condition" | "terminal_nodes" | "input_contract" | "output_contract"
    ) && control.identifiers().is_empty()
        && control.integers().is_empty()
        && control.flags().len() == 1
    {
        return Err(GraphError::InvalidProjection);
    }
    validate_required_discriminators(control)?;
    validate_control_counts(control)?;
    validate_correlated_control_groups(control)?;
    validate_nested_presence_families(control)?;
    if contract {
        let schema_identifier = control
            .identifiers()
            .keys()
            .any(|key| key.as_str() == "schema");
        let schema_digest = control.digests().keys().any(|key| key.as_str() == "schema");
        if schema_identifier && schema_digest {
            return Err(GraphError::InvalidProjection);
        }
        let has_semantic_identifier = control.identifiers().keys().any(|key| {
            matches!(
                key.as_str(),
                "schema" | "schemaId" | "schemaDialect" | "publishAs"
            ) || indexed_key(key.as_str(), "bindingKey.")
                || indexed_key(key.as_str(), "bindingValue.")
        });
        let has_bindings = control
            .integers()
            .iter()
            .any(|(key, value)| key.as_str() == "bindingCount" && *value > 0);
        if !has_semantic_identifier && !schema_digest && !has_bindings {
            return Err(GraphError::InvalidProjection);
        }
    }
    if control_type == "node_loop"
        && !control
            .integers()
            .iter()
            .any(|(key, value)| key.as_str() == "maxIterations" && *value > 0)
    {
        return Err(GraphError::InvalidProjection);
    }
    if control_type == "node_permissions" {
        validate_permission_allowlist_counts(control)?;
    }
    if matches!(
        control_type,
        "node_context" | "node_permissions" | "node_isolation"
    ) {
        validate_path_control_shape(control)?;
    }
    Ok(())
}

fn nested_path_count(control: &PersistedControl, prefix: &str, parent: usize) -> Option<usize> {
    let expected = format!("{prefix}.{parent:03}Count");
    control
        .integers()
        .iter()
        .find(|(key, _)| key.as_str() == expected)
        .and_then(|(_, value)| usize::try_from(*value).ok())
}

fn double_indexed_suffix(key: &str, prefix: &str) -> Option<(usize, usize)> {
    let (parent, child) = key.strip_prefix(prefix)?.split_once('.')?;
    if parent.len() != 3
        || child.len() != 3
        || !parent.bytes().all(|byte| byte.is_ascii_digit())
        || !child.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    Some((parent.parse().ok()?, child.parse().ok()?))
}

fn validate_nested_slots(
    control: &PersistedControl,
    count_prefix: &str,
    slot_prefix: &str,
    parent: usize,
    required: bool,
) -> Result<(), GraphError> {
    let count = nested_path_count(control, count_prefix, parent);
    let indices = control
        .identifiers()
        .keys()
        .filter_map(|key| double_indexed_suffix(key.as_str(), slot_prefix))
        .filter_map(|(candidate, child)| (candidate == parent).then_some(child))
        .collect::<BTreeSet<_>>();
    match count {
        Some(count) if (1..=64).contains(&count) => {
            if indices.len() != count || (0..count).any(|index| !indices.contains(&index)) {
                return Err(GraphError::InvalidProjection);
            }
        }
        None if !required && indices.is_empty() => {}
        _ => return Err(GraphError::InvalidProjection),
    }
    Ok(())
}

fn reject_nested_slots(
    control: &PersistedControl,
    count_prefix: &str,
    slot_prefix: &str,
    parent: usize,
) -> Result<(), GraphError> {
    if nested_path_count(control, count_prefix, parent).is_some()
        || control.identifiers().keys().any(|key| {
            double_indexed_suffix(key.as_str(), slot_prefix)
                .is_some_and(|(candidate, _)| candidate == parent)
        })
    {
        return Err(GraphError::InvalidProjection);
    }
    Ok(())
}

fn validate_path_control_shape(control: &PersistedControl) -> Result<(), GraphError> {
    match control.control_type().as_str() {
        "node_context" => {
            let count = control
                .integers()
                .iter()
                .find(|(key, _)| key.as_str() == "includeCount")
                .map(|(_, value)| {
                    usize::try_from(*value).map_err(|_| GraphError::InvalidProjection)
                })
                .transpose()?;
            let has_indexed_include = control.identifiers().keys().any(|key| {
                indexed_any(
                    key.as_str(),
                    &["includeType.", "includeRef.", "includeNode."],
                ) || double_indexed_key(key.as_str(), "scopeSlot.")
            }) || control
                .integers()
                .keys()
                .any(|key| nested_count_key(key.as_str(), "scope."));
            let Some(count) = count else {
                if has_indexed_include {
                    return Err(GraphError::InvalidProjection);
                }
                return Ok(());
            };
            if !(1..=32).contains(&count) {
                return Err(GraphError::InvalidProjection);
            }
            for index in 0..count {
                let kind = identifier(control, &format!("includeType.{index:03}"))
                    .ok_or(GraphError::InvalidProjection)?;
                let has_ref = identifier(control, &format!("includeRef.{index:03}")).is_some();
                let has_node = identifier(control, &format!("includeNode.{index:03}")).is_some();
                match kind {
                    "project_kernel" if !has_ref && !has_node => {
                        reject_nested_slots(control, "scope", "scopeSlot.", index)?;
                    }
                    "document" if has_ref && !has_node => {
                        let encoded = identifier(control, &format!("includeRef.{index:03}"))
                            .ok_or(GraphError::InvalidProjection)?;
                        let raw = decode_persisted_reference(encoded)?;
                        validate_context_include_reference(kind, &raw)?;
                        reject_nested_slots(control, "scope", "scopeSlot.", index)?;
                    }
                    "dependency_output" if !has_ref && has_node => {
                        reject_nested_slots(control, "scope", "scopeSlot.", index)?;
                    }
                    "source_scope" if !has_ref && !has_node => {
                        validate_nested_slots(control, "scope", "scopeSlot.", index, true)?
                    }
                    _ => return Err(GraphError::InvalidProjection),
                }
            }
        }
        "node_permissions" => {
            let count = control
                .integers()
                .iter()
                .find(|(key, _)| key.as_str() == "permissionCount")
                .and_then(|(_, value)| usize::try_from(*value).ok())
                .ok_or(GraphError::InvalidProjection)?;
            for index in 0..count {
                validate_nested_slots(control, "scope", "scopeSlot.", index, false)?;
            }
        }
        "node_isolation" => {
            let raw_count = control
                .integers()
                .iter()
                .find(|(key, _)| key.as_str() == "writeScopeCount")
                .map(|(_, value)| *value);
            let count = raw_count
                .map(|value| usize::try_from(value).map_err(|_| GraphError::InvalidProjection))
                .transpose()?;
            let indices = control
                .identifiers()
                .keys()
                .filter_map(|key| indexed_suffix(key.as_str(), "writeScopeSlot."))
                .collect::<BTreeSet<_>>();
            match count {
                Some(count) if (1..=64).contains(&count) => {
                    if indices.len() != count || (0..count).any(|index| !indices.contains(&index)) {
                        return Err(GraphError::InvalidProjection);
                    }
                }
                None if indices.is_empty() => {}
                _ => return Err(GraphError::InvalidProjection),
            }
        }
        _ => return Err(GraphError::InvalidProjection),
    }
    Ok(())
}

fn has_identifier(control: &PersistedControl, key: &str) -> bool {
    control
        .identifiers()
        .keys()
        .any(|candidate| candidate.as_str() == key)
}

fn has_integer(control: &PersistedControl, key: &str) -> bool {
    control
        .integers()
        .keys()
        .any(|candidate| candidate.as_str() == key)
}

fn has_flag(control: &PersistedControl, key: &str) -> bool {
    control
        .flags()
        .keys()
        .any(|candidate| candidate.as_str() == key)
}

fn require_presence_equivalence(
    control: &PersistedControl,
    presence_key: &str,
    has_child: bool,
) -> Result<(), GraphError> {
    let presence = control
        .flags()
        .iter()
        .find(|(key, _)| key.as_str() == presence_key)
        .map(|(_, value)| *value);
    match (presence, has_child) {
        (Some(true), true) | (None, false) => Ok(()),
        _ => Err(GraphError::InvalidProjection),
    }
}

fn validate_nested_presence_families(control: &PersistedControl) -> Result<(), GraphError> {
    match control.control_type().as_str() {
        "node_context" => {
            require_presence_equivalence(
                control,
                "freshnessPresent",
                has_integer(control, "freshnessMaxAgeDays")
                    || has_integer(control, "revalidateCount")
                    || control
                        .identifiers()
                        .keys()
                        .any(|key| indexed_key(key.as_str(), "revalidate.")),
            )?;
            require_presence_equivalence(
                control,
                "budgetPresent",
                has_integer(control, "initialUnits") || has_integer(control, "maxUnits"),
            )?;
            require_presence_equivalence(
                control,
                "expansionPresent",
                has_flag(control, "expansionAllowed")
                    || has_flag(control, "expansionRequiresReason"),
            )?;
        }
        "node_isolation" => {
            require_presence_equivalence(
                control,
                "filesystemPresent",
                has_identifier(control, "filesystemMode")
                    || has_integer(control, "writeScopeCount")
                    || control
                        .identifiers()
                        .keys()
                        .any(|key| indexed_key(key.as_str(), "writeScopeSlot.")),
            )?;
            require_presence_equivalence(
                control,
                "networkPresent",
                has_identifier(control, "networkMode")
                    || has_integer(control, "networkAllowCount")
                    || control
                        .identifiers()
                        .keys()
                        .any(|key| indexed_key(key.as_str(), "networkAllow.")),
            )?;
            require_presence_equivalence(
                control,
                "brokerPresent",
                has_identifier(control, "brokerMode"),
            )?;
        }
        _ => {}
    }
    Ok(())
}

fn identifier_key_allowed(control_type: &str, key: &str) -> bool {
    match control_type {
        "agent_configuration" => {
            matches!(
                key,
                "mode" | "agentRef" | "inputSchema" | "resultSchema" | "directiveRef" | "isolation"
            ) || indexed_any(key, &["capability.", "allowedTool.", "prohibitedAction."])
        }
        "node_agents" => indexed_key(key, "agentRef."),
        "agent_model_requirements" | "node_model" => {
            matches!(key, "profile" | "routePolicy") || indexed_key(key, "independent.")
        }
        "agent_context_strategy" => indexed_key(key, "includeScope."),
        "agent_evidence_requirements" => indexed_any(key, &["requirement.", "type.", "ref."]),
        "agent_memory_policy" | "node_memory" => {
            key == "policyRef" || indexed_key(key, "includeScope.")
        }
        "input_contract" | "output_contract" => {
            matches!(key, "schema" | "schemaId" | "schemaDialect" | "publishAs")
                || indexed_any(key, &["bindingKey.", "bindingValue."])
        }
        "node_context" => {
            matches!(key, "policyRef" | "conflicts")
                || indexed_any(
                    key,
                    &[
                        "includeType.",
                        "includeRef.",
                        "includeNode.",
                        "exclude.",
                        "revalidate.",
                    ],
                )
                || double_indexed_key(key, "scopeSlot.")
        }
        "node_permissions" => {
            indexed_any(key, &["capability.", "duration."])
                || double_indexed_key(key, "allow.")
                || double_indexed_key(key, "scopeSlot.")
        }
        "node_isolation" => {
            matches!(
                key,
                "minimum" | "filesystemMode" | "networkMode" | "brokerMode"
            ) || indexed_key(key, "networkAllow.")
                || indexed_key(key, "writeScopeSlot.")
        }
        "node_retry" => {
            key == "backoff" || indexed_any(key, &["retryOn.", "noRetry.", "beforeRetry."])
        }
        "node_resources" => false,
        "node_common" => matches!(key, "onCancel" | "onFailure") || indexed_key(key, "tag."),
        "tool_configuration" => matches!(key, "toolRef" | "action"),
        "classifier_configuration" => matches!(key, "method" | "profile" | "rulesRef"),
        "gate_configuration" => {
            matches!(key, "passWhen" | "failureRoute" | "overrideResult")
                || indexed_any(key, &["requirement.", "evaluator.", "overrideRole."])
        }
        "fork_configuration" => key == "strategy",
        "join_configuration" => matches!(key, "strategy" | "mergeMethod" | "resultSchema"),
        "human_decision" => {
            matches!(key, "directiveSlot" | "timeoutAction") || indexed_key(key, "option.")
        }
        "subgraph_configuration" => {
            key == "graphRef"
                || indexed_any(key, &["parameterKey.", "parameterValue.", "exposedResult."])
        }
        "materializer_configuration" => matches!(key, "target" | "strategy"),
        "deploy_configuration" => {
            matches!(key, "adapterRef" | "compensationNode") || indexed_key(key, "precondition.")
        }
        "rollback_configuration" => key == "adapterRef",
        "node_completion" => {
            key == "contractRef"
                || indexed_any(
                    key,
                    &[
                        // M11 #160: the customs evidence family. Three separate closed
                        // vocabularies govern this control type — identifiers here, integers
                        // below, and count groups in `validate_control_counts` — and a key
                        // missing from ANY of them fails as `InvalidProjection` at sealing,
                        // with nothing naming the key that was rejected.
                        "customsProof.",
                        "requires.",
                        "forbids.",
                        "requiresArtifact.",
                        "forbidsArtifact.",
                        "requiresEvidenceType.",
                        "forbidsEvidenceType.",
                        "requiresSlot.",
                        "forbidsSlot.",
                    ],
                )
        }
        "node_configuration" => key == "targetRef",
        "node_loop" => false,
        "edge_condition" => matches!(key, "schema" | "unknownBehavior" | "slot0" | "slot1"),
        "policy_control" => {
            matches!(
                key,
                "mode"
                    | "policyRef"
                    | "reasonCode"
                    | "resultLabel"
                    | "ruleTextSlot"
                    | "explanationSlot"
            ) || indexed_any(key, &["deny.", "bypassedRequirement.", "acknowledgedRisk."])
        }
        "graph_completion" => {
            matches!(key, "statusFull" | "statusWaived")
                || indexed_any(key, &["terminal.", "requirement."])
        }
        "terminal_nodes" => key == "terminalNode",
        _ => false,
    }
}

fn integer_key_allowed(control_type: &str, key: &str) -> bool {
    match control_type {
        "agent_configuration" => matches!(
            key,
            "capabilityCount" | "allowedToolCount" | "prohibitedActionCount"
        ),
        "node_agents" => key == "agentRefCount",
        "agent_model_requirements" | "node_model" => key == "independentCount",
        "agent_context_strategy" => matches!(key, "includeScopeCount" | "maxUnits"),
        "agent_evidence_requirements" => key == "requirementCount" || indexed_key(key, "min."),
        "agent_memory_policy" | "node_memory" => {
            matches!(key, "defaultTtlDays" | "includeScopeCount")
        }
        "input_contract" | "output_contract" => key == "bindingCount",
        "node_context" => {
            matches!(
                key,
                "includeCount"
                    | "excludeCount"
                    | "revalidateCount"
                    | "freshnessMaxAgeDays"
                    | "initialUnits"
                    | "maxUnits"
            ) || nested_count_key(key, "scope.")
        }
        "node_permissions" => {
            key == "permissionCount"
                || nested_count_key(key, "allow.")
                || nested_count_key(key, "scope.")
        }
        "node_isolation" => matches!(
            key,
            "networkAllowCount"
                | "writeScopeCount"
                | "isolationCpu"
                | "isolationMemoryMb"
                | "isolationDiskMb"
        ),
        "node_retry" => matches!(
            key,
            "maxAttempts"
                | "maxBackoffSeconds"
                | "retryOnCount"
                | "noRetryCount"
                | "beforeRetryCount"
        ),
        "node_resources" => matches!(key, "cpu" | "memoryMb" | "diskMb"),
        "node_common" => key == "tagCount",
        "gate_configuration" => matches!(
            key,
            "requirementCount" | "evaluatorCount" | "overrideRoleCount"
        ),
        "human_decision" => matches!(key, "optionCount" | "timeoutSeconds"),
        "subgraph_configuration" => matches!(key, "parameterCount" | "exposedResultCount"),
        "deploy_configuration" => key == "preconditionCount",
        "node_completion" => {
            matches!(key, "requiresCount" | "forbidsCount" | "customsProofCount")
                || indexed_any(key, &["requiresEvidenceMin.", "forbidsEvidenceMin."])
        }
        "node_configuration" => key == "timeoutSeconds",
        "node_loop" => key == "maxIterations",
        "policy_control" => matches!(
            key,
            "denyCount" | "bypassedRequirementCount" | "acknowledgedRiskCount"
        ),
        "graph_completion" => matches!(key, "terminalCount" | "requirementCount"),
        _ => false,
    }
}

fn flag_key_allowed(control_type: &str, key: &str) -> bool {
    if key == "present" && !matches!(control_type, "edge_condition" | "terminal_nodes") {
        return true;
    }
    match control_type {
        "agent_memory_policy" | "node_memory" => key == "writeCandidates",
        "node_context" => matches!(
            key,
            "freshnessPresent"
                | "budgetPresent"
                | "expansionPresent"
                | "expansionAllowed"
                | "expansionRequiresReason"
        ),
        "node_isolation" => matches!(
            key,
            "filesystemPresent" | "networkPresent" | "brokerPresent"
        ),
        "deploy_configuration" => matches!(key, "reversible" | "compensationRequired"),
        "node_completion" => indexed_any(key, &["requiresSchemaValid.", "forbidsSchemaValid."]),
        "node_configuration" => matches!(key, "userEditable" | "userOverrideAllowed"),
        "graph_completion" => key == "allowWaivers",
        "terminal_nodes" => key == "allowWaivers",
        _ => false,
    }
}

fn identifier_value_allowed(control_type: &str, key: &str, value: &str) -> bool {
    if matches!(
        (control_type, key),
        ("node_isolation", "minimum") | ("agent_configuration", "isolation")
    ) {
        return is_valid_isolation_tier(value);
    }
    let allowed = match (control_type, key) {
        ("agent_configuration", "mode") => Some(&["ref", "ephemeral"][..]),
        ("tool_configuration", "action") => Some(&["execute"][..]),
        ("classifier_configuration", "method") => Some(&["hybrid"][..]),
        ("gate_configuration", "passWhen") => Some(&["all"][..]),
        ("fork_configuration", "strategy") => Some(&["all"][..]),
        ("join_configuration", "strategy") => Some(
            &[
                "all_completed",
                "all_succeeded",
                "any_succeeded",
                "quorum",
                "first_valid",
                "custom_evaluator",
            ][..],
        ),
        ("human_decision", "timeoutAction") => Some(&["pause"][..]),
        ("materializer_configuration", "strategy") => Some(&["evidence_backed_patch"][..]),
        ("edge_condition", "unknownBehavior") => Some(&["pause", "fail", "skip", "route"][..]),
        ("policy_control", "mode") => Some(&["ref", "inline", "manual_override"][..]),
        _ => None,
    };
    allowed.is_none_or(|allowed| allowed.contains(&value))
}

fn validate_required_discriminators(control: &PersistedControl) -> Result<(), GraphError> {
    if control.control_type().as_str() == "agent_configuration" {
        let mode = identifier(control, "mode").ok_or(GraphError::InvalidProjection)?;
        let exact_flags = control.flags().len() == 1
            && control
                .flags()
                .iter()
                .next()
                .is_some_and(|(key, value)| key.as_str() == "present" && *value);
        let valid = match mode {
            "ref" => {
                exact_flags
                    && control.identifiers().len() == 2
                    && identifier(control, "agentRef").is_some()
                    && control.integers().is_empty()
            }
            "ephemeral" => {
                exact_flags
                    && identifier(control, "agentRef").is_none()
                    && identifier(control, "inputSchema").is_some()
                    && identifier(control, "resultSchema").is_some()
                    && control
                        .integers()
                        .iter()
                        .any(|(key, count)| key.as_str() == "capabilityCount" && *count > 0)
            }
            _ => false,
        };
        if !valid {
            return Err(GraphError::InvalidProjection);
        }
    }
    if control.control_type().as_str() == "policy_control" {
        validate_exact_policy_mode(control)?;
    }
    Ok(())
}

fn validate_correlated_control_groups(control: &PersistedControl) -> Result<(), GraphError> {
    let exact = |count_key: &str, prefixes: &[&str]| -> Result<(), GraphError> {
        let count = control
            .integers()
            .iter()
            .find(|(key, _)| key.as_str() == count_key)
            .map(|(_, value)| usize::try_from(*value).map_err(|_| GraphError::InvalidProjection))
            .transpose()?;
        let Some(count) = count else {
            return Ok(());
        };
        for prefix in prefixes {
            let indices = control
                .identifiers()
                .keys()
                .chain(control.integers().keys())
                .chain(control.flags().keys())
                .filter_map(|key| indexed_suffix(key.as_str(), prefix))
                .collect::<BTreeSet<_>>();
            if indices.len() != count || (0..count).any(|index| !indices.contains(&index)) {
                return Err(GraphError::InvalidProjection);
            }
        }
        Ok(())
    };
    match control.control_type().as_str() {
        "input_contract" | "output_contract" => {
            exact("bindingCount", &["bindingKey.", "bindingValue."])?
        }
        "subgraph_configuration" => exact("parameterCount", &["parameterKey.", "parameterValue."])?,
        "node_permissions" => exact("permissionCount", &["capability."])?,
        "node_context" => exact("includeCount", &["includeType."])?,
        "agent_evidence_requirements" => {
            validate_one_of_positions(control, "requirementCount", &["requirement.", "type."])?;
            validate_optional_position_owner(control, "ref.", "type.")?;
            validate_optional_position_owner(control, "min.", "type.")?;
        }
        "node_completion" => {
            for (count, prefixes) in [
                (
                    "requiresCount",
                    &[
                        "requires.",
                        "requiresArtifact.",
                        "requiresEvidenceType.",
                        "requiresSchemaValid.",
                        "requiresSlot.",
                    ][..],
                ),
                (
                    "forbidsCount",
                    &[
                        "forbids.",
                        "forbidsArtifact.",
                        "forbidsEvidenceType.",
                        "forbidsSchemaValid.",
                        "forbidsSlot.",
                    ][..],
                ),
            ] {
                validate_one_of_positions(control, count, prefixes)?;
            }
            validate_optional_position_owner(
                control,
                "requiresEvidenceMin.",
                "requiresEvidenceType.",
            )?;
            validate_optional_position_owner(
                control,
                "forbidsEvidenceMin.",
                "forbidsEvidenceType.",
            )?;
        }
        _ => {}
    }
    Ok(())
}

fn control_indices(control: &PersistedControl, prefix: &str) -> BTreeSet<usize> {
    control
        .identifiers()
        .keys()
        .chain(control.integers().keys())
        .chain(control.flags().keys())
        .filter_map(|key| indexed_suffix(key.as_str(), prefix))
        .collect()
}

fn validate_one_of_positions(
    control: &PersistedControl,
    count_key: &str,
    prefixes: &[&str],
) -> Result<(), GraphError> {
    let count = control
        .integers()
        .iter()
        .find(|(key, _)| key.as_str() == count_key)
        .map(|(_, value)| usize::try_from(*value).map_err(|_| GraphError::InvalidProjection))
        .transpose()?;
    let Some(count) = count else {
        return Ok(());
    };
    let sets = prefixes
        .iter()
        .map(|prefix| control_indices(control, prefix))
        .collect::<Vec<_>>();
    for index in 0..count {
        if sets
            .iter()
            .filter(|indices| indices.contains(&index))
            .count()
            != 1
        {
            return Err(GraphError::InvalidProjection);
        }
    }
    Ok(())
}

fn validate_optional_position_owner(
    control: &PersistedControl,
    optional_prefix: &str,
    owner_prefix: &str,
) -> Result<(), GraphError> {
    let optional = control_indices(control, optional_prefix);
    let owner = control_indices(control, owner_prefix);
    if optional.iter().any(|index| !owner.contains(index)) {
        return Err(GraphError::InvalidProjection);
    }
    Ok(())
}

fn validate_exact_policy_mode(control: &PersistedControl) -> Result<(), GraphError> {
    let mode = identifier(control, "mode").ok_or(GraphError::InvalidProjection)?;
    let identifiers_valid = |fixed: &[&str], prefixes: &[&str]| {
        control.identifiers().keys().all(|key| {
            fixed.contains(&key.as_str())
                || prefixes
                    .iter()
                    .any(|prefix| indexed_key(key.as_str(), prefix))
        })
    };
    let integers_exact = |keys: &[&str]| {
        control.integers().len() == keys.len()
            && keys.iter().all(|expected| {
                control
                    .integers()
                    .keys()
                    .any(|key| key.as_str() == *expected)
            })
    };
    if control.flags().len() != 1
        || control
            .flags()
            .iter()
            .next()
            .is_none_or(|(key, value)| key.as_str() != "present" || !value)
    {
        return Err(GraphError::InvalidProjection);
    }
    match mode {
        "ref"
            if identifiers_valid(&["mode", "policyRef"], &[])
                && control.identifiers().len() == 2
                && control.integers().is_empty() =>
        {
            Ok(())
        }
        "inline"
            if identifiers_valid(
                &["mode", "reasonCode", "ruleTextSlot", "explanationSlot"],
                &["deny."],
            ) && integers_exact(&["denyCount"])
                && control
                    .integers()
                    .values()
                    .next()
                    .is_some_and(|count| *count > 0) =>
        {
            Ok(())
        }
        "manual_override"
            if identifiers_valid(
                &["mode", "resultLabel"],
                &["bypassedRequirement.", "acknowledgedRisk."],
            ) && integers_exact(&["bypassedRequirementCount", "acknowledgedRiskCount"])
                && identifier(control, "resultLabel").is_some()
                && control.integers().values().all(|count| *count > 0) =>
        {
            Ok(())
        }
        _ => Err(GraphError::InvalidProjection),
    }
}

fn validate_control_counts(control: &PersistedControl) -> Result<(), GraphError> {
    let groups: &[(&str, &[&str])] = match control.control_type().as_str() {
        "agent_configuration" => &[
            ("capabilityCount", &["capability."]),
            ("allowedToolCount", &["allowedTool."]),
            ("prohibitedActionCount", &["prohibitedAction."]),
        ],
        "node_agents" => &[("agentRefCount", &["agentRef."])],
        "agent_model_requirements" | "node_model" => &[("independentCount", &["independent."])],
        "agent_context_strategy" => &[("includeScopeCount", &["includeScope."])],
        "agent_evidence_requirements" => &[(
            "requirementCount",
            &["requirement.", "type.", "ref.", "min."],
        )],
        "agent_memory_policy" | "node_memory" => &[("includeScopeCount", &["includeScope."])],
        "input_contract" | "output_contract" => {
            &[("bindingCount", &["bindingKey.", "bindingValue."])]
        }
        "node_context" => &[
            (
                "includeCount",
                &["includeType.", "includeRef.", "includeNode."],
            ),
            ("excludeCount", &["exclude."]),
            ("revalidateCount", &["revalidate."]),
        ],
        "node_permissions" => &[],
        "node_isolation" => &[("networkAllowCount", &["networkAllow."])],
        "node_retry" => &[
            ("retryOnCount", &["retryOn."]),
            ("noRetryCount", &["noRetry."]),
            ("beforeRetryCount", &["beforeRetry."]),
        ],
        "node_common" => &[("tagCount", &["tag."])],
        "gate_configuration" => &[
            ("requirementCount", &["requirement."]),
            ("evaluatorCount", &["evaluator."]),
            ("overrideRoleCount", &["overrideRole."]),
        ],
        "human_decision" => &[("optionCount", &["option."])],
        "subgraph_configuration" => &[
            ("parameterCount", &["parameterKey.", "parameterValue."]),
            ("exposedResultCount", &["exposedResult."]),
        ],
        "deploy_configuration" => &[("preconditionCount", &["precondition."])],
        "node_completion" => &[
            // M11 #160: the customs evidence requirement is an indexed family like the others,
            // so it needs its declared count here or `validate_control_counts` refuses the whole
            // projection. Naming this table is the point: the node-completion control's key
            // vocabulary is CLOSED, and a governor arm that emits an undeclared prefix produces
            // `InvalidProjection` at sealing rather than a message about the key it dislikes.
            ("customsProofCount", &["customsProof."]),
            (
                "requiresCount",
                &[
                    "requires.",
                    "requiresArtifact.",
                    "requiresEvidenceType.",
                    "requiresEvidenceMin.",
                    "requiresSchemaValid.",
                    "requiresSlot.",
                ],
            ),
            (
                "forbidsCount",
                &[
                    "forbids.",
                    "forbidsArtifact.",
                    "forbidsEvidenceType.",
                    "forbidsEvidenceMin.",
                    "forbidsSchemaValid.",
                    "forbidsSlot.",
                ],
            ),
        ],
        "policy_control" => &[
            ("denyCount", &["deny."]),
            ("bypassedRequirementCount", &["bypassedRequirement."]),
            ("acknowledgedRiskCount", &["acknowledgedRisk."]),
        ],
        "graph_completion" => &[
            ("terminalCount", &["terminal."]),
            ("requirementCount", &["requirement."]),
        ],
        _ => &[],
    };
    for (count_key, prefixes) in groups {
        validate_count_group(control, count_key, prefixes)?;
    }
    Ok(())
}

fn validate_count_group(
    control: &PersistedControl,
    count_key: &str,
    prefixes: &[&str],
) -> Result<(), GraphError> {
    let count = control
        .integers()
        .iter()
        .find(|(key, _)| key.as_str() == count_key)
        .map(|(_, value)| *value);
    let mut indices = BTreeSet::new();
    for key in control
        .identifiers()
        .keys()
        .chain(control.integers().keys())
        .chain(control.flags().keys())
    {
        for prefix in prefixes {
            if let Some(index) = indexed_suffix(key.as_str(), prefix) {
                indices.insert(index);
            }
        }
    }
    match count {
        None if indices.is_empty() => Ok(()),
        Some(count) if (0..=64).contains(&count) => {
            let count = usize::try_from(count).map_err(|_| GraphError::InvalidProjection)?;
            if indices.iter().any(|index| *index >= count)
                || (0..count).any(|index| !indices.contains(&index))
            {
                Err(GraphError::InvalidProjection)
            } else {
                Ok(())
            }
        }
        _ => Err(GraphError::InvalidProjection),
    }
}

fn validate_permission_allowlist_counts(control: &PersistedControl) -> Result<(), GraphError> {
    let permission_count = control
        .integers()
        .iter()
        .find(|(key, _)| key.as_str() == "permissionCount")
        .and_then(|(_, value)| usize::try_from(*value).ok())
        .ok_or(GraphError::InvalidProjection)?;
    if permission_count > 32 {
        return Err(GraphError::InvalidProjection);
    }
    let mut represented = BTreeSet::new();
    for key in control.identifiers().keys() {
        for prefix in ["capability.", "duration."] {
            if let Some(index) = indexed_suffix(key.as_str(), prefix) {
                if index >= permission_count {
                    return Err(GraphError::InvalidProjection);
                }
                represented.insert(index);
            }
        }
    }
    for (key, value) in control
        .integers()
        .iter()
        .filter(|(key, _)| nested_count_key(key.as_str(), "allow."))
    {
        let permission = key
            .as_str()
            .strip_prefix("allow.")
            .and_then(|value| value.strip_suffix("Count"))
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or(GraphError::InvalidProjection)?;
        let count = usize::try_from(*value).map_err(|_| GraphError::InvalidProjection)?;
        if permission >= permission_count || count > 64 {
            return Err(GraphError::InvalidProjection);
        }
        represented.insert(permission);
        let prefix = format!("allow.{permission:03}.");
        let indices = control
            .identifiers()
            .keys()
            .filter_map(|key| indexed_suffix(key.as_str(), &prefix))
            .collect::<BTreeSet<_>>();
        if indices.len() != count || (0..count).any(|index| !indices.contains(&index)) {
            return Err(GraphError::InvalidProjection);
        }
    }
    for key in control
        .identifiers()
        .keys()
        .filter(|key| double_indexed_key(key.as_str(), "allow."))
    {
        let permission = key
            .as_str()
            .strip_prefix("allow.")
            .and_then(|value| value.split_once('.'))
            .and_then(|(value, _)| value.parse::<usize>().ok())
            .ok_or(GraphError::InvalidProjection)?;
        let count_key = format!("allow.{permission:03}Count");
        if permission >= permission_count
            || !control
                .integers()
                .keys()
                .any(|key| key.as_str() == count_key)
        {
            return Err(GraphError::InvalidProjection);
        }
    }
    if (0..permission_count).any(|index| !represented.contains(&index)) {
        return Err(GraphError::InvalidProjection);
    }
    Ok(())
}

fn identifier<'a>(control: &'a PersistedControl, key: &str) -> Option<&'a str> {
    control
        .identifiers()
        .iter()
        .find(|(candidate, _)| candidate.as_str() == key)
        .map(|(_, value)| value.as_str())
}

fn indexed_any(key: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|prefix| indexed_key(key, prefix))
}

fn indexed_suffix(key: &str, prefix: &str) -> Option<usize> {
    key.strip_prefix(prefix)
        .filter(|suffix| suffix.len() == 3 && suffix.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|suffix| suffix.parse().ok())
}

fn double_indexed_key(key: &str, prefix: &str) -> bool {
    key.strip_prefix(prefix)
        .and_then(|value| value.split_once('.'))
        .is_some_and(|(first, second)| {
            first.len() == 3
                && second.len() == 3
                && first.bytes().all(|byte| byte.is_ascii_digit())
                && second.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn nested_count_key(key: &str, prefix: &str) -> bool {
    key.strip_prefix(prefix)
        .and_then(|value| value.strip_suffix("Count"))
        .is_some_and(|index| index.len() == 3 && index.bytes().all(|byte| byte.is_ascii_digit()))
}

/// Validates the complete pure safe-projection contract after deserialization.
///
/// This performs only deterministic in-memory checks. It neither reconstructs
/// an authoring graph nor reads Evidence, files, network resources, or models.
pub fn validate_persisted_projection(version: &PersistedGraphVersion) -> Result<(), GraphError> {
    match (version.number(), version.predecessor()) {
        (1, None) => {}
        (number, Some(predecessor)) if predecessor.number().checked_add(1) == Some(number) => {}
        _ => return Err(GraphError::InvalidProjection),
    }
    validate_projection_grammar(version)?;
    validate_persisted_references(version)?;
    validate_projection_content(version)?;
    let hashes = persisted_hashes(version.topology(), version.content_slots())?;
    if hashes.topology_hash() != version.topology_hash()
        || hashes.semantic_hash() != version.semantic_hash()
    {
        return Err(GraphError::InvalidProjection);
    }

    let topology = version.topology();
    if topology
        .entrypoints()
        .iter()
        .any(|entrypoint| !topology.nodes().contains_key(entrypoint))
    {
        return Err(GraphError::InvalidProjection);
    }
    let validated_topology = validate_edge_identity_and_endpoints(topology)?;
    validate_control_node_relations(validated_topology.topology)?;
    validate_persisted_topology(&validated_topology)?;
    let edge_ids = validated_topology.edge_ids;

    let mut node_slots = topology
        .nodes()
        .keys()
        .map(|node_id| (node_id, Vec::<&OpaqueId>::new()))
        .collect::<BTreeMap<_, _>>();
    for slot in version.content_slots() {
        if !valid_slot_field(slot.owner_kind(), slot.field_kind()) {
            return Err(GraphError::InvalidProjection);
        }
        if &derive_content_slot_id(
            slot.owner_kind(),
            slot.owner_id(),
            slot.field_kind(),
            slot.ordinal(),
        )? != slot.slot_id()
        {
            return Err(GraphError::InvalidProjection);
        }
        let profile = derive_content_slot_profile(
            slot.owner_kind(),
            slot.owner_id(),
            slot.field_kind(),
            slot.ordinal(),
        )?;
        if slot.sensitivity() != profile.sensitivity()
            || slot.required_for_execution() != profile.required_for_execution()
        {
            return Err(GraphError::InvalidProjection);
        }
        match slot.owner_kind() {
            ContentOwnerKind::Graph | ContentOwnerKind::Policy => {
                if slot.owner_id() != topology.graph_id() {
                    return Err(GraphError::InvalidProjection);
                }
            }
            ContentOwnerKind::Node | ContentOwnerKind::Agent => {
                let Some(node) = topology.nodes().get(slot.owner_id()) else {
                    return Err(GraphError::InvalidProjection);
                };
                if slot.owner_kind() == ContentOwnerKind::Agent
                    && node.node_type().as_str() != "agent"
                {
                    return Err(GraphError::InvalidProjection);
                }
                let Some(owned) = node_slots.get_mut(slot.owner_id()) else {
                    return Err(GraphError::InvalidProjection);
                };
                owned.push(slot.slot_id());
            }
            ContentOwnerKind::Edge => {
                if !edge_ids.contains(slot.owner_id()) {
                    return Err(GraphError::InvalidProjection);
                }
            }
            ContentOwnerKind::Diagnostic => return Err(GraphError::InvalidProjection),
        }
    }

    if topology.nodes().iter().any(|(node_id, node)| {
        let expected = &node_slots[node_id];
        expected.len() != node.content_slot_ids().len()
            || expected
                .iter()
                .zip(node.content_slot_ids())
                .any(|(left, right)| *left != right)
    }) {
        return Err(GraphError::InvalidProjection);
    }
    validate_minimum_projection_image(version)?;
    validate_exact_slot_bindings(version)?;
    Ok(())
}

struct ValidatedTopology<'a> {
    topology: &'a PersistedTopology,
    edge_ids: BTreeSet<&'a OpaqueId>,
}

fn validate_edge_identity_and_endpoints<'a>(
    topology: &'a PersistedTopology,
) -> Result<ValidatedTopology<'a>, GraphError> {
    let edge_ids = topology
        .edges()
        .iter()
        .map(|edge| edge.id())
        .collect::<BTreeSet<_>>();
    if edge_ids.len() != topology.edges().len()
        || topology.edges().iter().any(|edge| {
            !topology.nodes().contains_key(edge.from()) || !topology.nodes().contains_key(edge.to())
        })
    {
        return Err(GraphError::InvalidProjection);
    }
    Ok(ValidatedTopology { topology, edge_ids })
}

fn validate_minimum_projection_image(version: &PersistedGraphVersion) -> Result<(), GraphError> {
    let topology = version.topology();
    let has_slot = |owner_kind, owner_id: &OpaqueId, field_kind, ordinal| {
        version.content_slots().iter().any(|slot| {
            slot.owner_kind() == owner_kind
                && slot.owner_id() == owner_id
                && slot.field_kind() == field_kind
                && slot.ordinal() == ordinal
        })
    };
    if !has_slot(
        ContentOwnerKind::Graph,
        topology.graph_id(),
        ContentFieldKind::DisplayName,
        0,
    ) {
        return Err(GraphError::InvalidProjection);
    }
    for (node_id, node) in topology.nodes() {
        if !has_slot(
            ContentOwnerKind::Node,
            node_id,
            ContentFieldKind::DisplayName,
            0,
        ) || !has_slot(
            ContentOwnerKind::Node,
            node_id,
            ContentFieldKind::Objective,
            0,
        ) {
            return Err(GraphError::InvalidProjection);
        }
        let control = |control_type: &str| {
            node.controls()
                .iter()
                .find(|control| control.control_type().as_str() == control_type)
        };
        if node.node_type().as_str() == "agent" {
            let agent = control("agent_configuration").ok_or(GraphError::InvalidProjection)?;
            let mode = identifier(agent, "mode").ok_or(GraphError::InvalidProjection)?;
            let agent_slots = version
                .content_slots()
                .iter()
                .filter(|slot| {
                    slot.owner_kind() == ContentOwnerKind::Agent && slot.owner_id() == node_id
                })
                .collect::<Vec<_>>();
            let expected_instructions =
                mode == "ephemeral" && identifier(agent, "directiveRef").is_none();
            let exact_agent_slot = |field_kind, expected| {
                agent_slots
                    .iter()
                    .filter(|slot| slot.field_kind() == field_kind && slot.ordinal() == 0)
                    .count()
                    == usize::from(expected)
            };
            if !matches!(mode, "ref" | "ephemeral")
                || !exact_agent_slot(ContentFieldKind::Purpose, mode == "ephemeral")
                || !exact_agent_slot(ContentFieldKind::CompletionContract, mode == "ephemeral")
                || !exact_agent_slot(ContentFieldKind::Instructions, expected_instructions)
                || agent_slots.len()
                    != usize::from(mode == "ephemeral") * 2 + usize::from(expected_instructions)
            {
                return Err(GraphError::InvalidProjection);
            }
        }
        if node.node_type().as_str() == "gate" && control("node_completion").is_none() {
            return Err(GraphError::InvalidProjection);
        }
    }
    Ok(())
}

fn validate_exact_slot_bindings(version: &PersistedGraphVersion) -> Result<(), GraphError> {
    let topology = version.topology();
    let slots = version
        .content_slots()
        .iter()
        .map(|slot| (slot.slot_id().as_str(), slot))
        .collect::<BTreeMap<_, _>>();
    let mut bound = BTreeSet::new();

    for (node_id, node) in topology.nodes() {
        for control in node.controls() {
            match control.control_type().as_str() {
                "human_decision" => {
                    if let Some(slot_id) = identifier(control, "directiveSlot") {
                        bind_exact_slot(
                            &slots,
                            &mut bound,
                            slot_id,
                            ContentOwnerKind::Node,
                            node_id,
                            ContentFieldKind::Instructions,
                            0,
                        )?;
                    }
                }
                "node_completion" => {
                    let requires_count = control
                        .integers()
                        .iter()
                        .find(|(key, _)| key.as_str() == "requiresCount")
                        .and_then(|(_, value)| u32::try_from(*value).ok())
                        .unwrap_or(0);
                    for (key, value) in control.identifiers() {
                        let ordinal = if let Some(index) =
                            indexed_suffix(key.as_str(), "requiresSlot.")
                        {
                            Some(u32::try_from(index).map_err(|_| GraphError::InvalidProjection)?)
                        } else if let Some(index) = indexed_suffix(key.as_str(), "forbidsSlot.") {
                            Some(
                                requires_count
                                    .checked_add(
                                        u32::try_from(index)
                                            .map_err(|_| GraphError::InvalidProjection)?,
                                    )
                                    .ok_or(GraphError::InvalidProjection)?,
                            )
                        } else {
                            None
                        };
                        if let Some(ordinal) = ordinal {
                            bind_exact_slot(
                                &slots,
                                &mut bound,
                                value.as_str(),
                                ContentOwnerKind::Node,
                                node_id,
                                ContentFieldKind::CompletionContract,
                                ordinal,
                            )?;
                        }
                    }
                }
                "node_context" => {
                    for (key, value) in control.identifiers() {
                        if let Some((parent, child)) =
                            double_indexed_suffix(key.as_str(), "scopeSlot.")
                        {
                            bind_exact_slot(
                                &slots,
                                &mut bound,
                                value.as_str(),
                                ContentOwnerKind::Node,
                                node_id,
                                ContentFieldKind::ContextPath,
                                nested_slot_ordinal(parent, child)?,
                            )?;
                        }
                    }
                }
                "node_permissions" => {
                    for (key, value) in control.identifiers() {
                        if let Some((parent, child)) =
                            double_indexed_suffix(key.as_str(), "scopeSlot.")
                        {
                            bind_exact_slot(
                                &slots,
                                &mut bound,
                                value.as_str(),
                                ContentOwnerKind::Node,
                                node_id,
                                ContentFieldKind::PermissionPath,
                                nested_slot_ordinal(parent, child)?,
                            )?;
                        }
                    }
                }
                "node_isolation" => {
                    for (key, value) in control.identifiers() {
                        if let Some(index) = indexed_suffix(key.as_str(), "writeScopeSlot.") {
                            bind_exact_slot(
                                &slots,
                                &mut bound,
                                value.as_str(),
                                ContentOwnerKind::Node,
                                node_id,
                                ContentFieldKind::IsolationPath,
                                u32::try_from(index).map_err(|_| GraphError::InvalidProjection)?,
                            )?;
                        }
                    }
                }
                _ => {}
            }
        }
    }
    for edge in topology.edges() {
        if let Some(condition) = edge.condition() {
            for ordinal in 0..=1u32 {
                if let Some(slot_id) = identifier(condition, &format!("slot{ordinal}")) {
                    bind_exact_slot(
                        &slots,
                        &mut bound,
                        slot_id,
                        ContentOwnerKind::Edge,
                        edge.id(),
                        ContentFieldKind::PolicyText,
                        ordinal,
                    )?;
                }
            }
        }
    }

    for (policy_index, policy) in topology.policies().iter().enumerate() {
        for (key, offset) in [("ruleTextSlot", 0usize), ("explanationSlot", 1usize)] {
            if let Some(slot_id) = identifier(policy, key) {
                let ordinal = policy_index
                    .checked_mul(2)
                    .and_then(|value| value.checked_add(offset))
                    .and_then(|value| u32::try_from(value).ok())
                    .ok_or(GraphError::InvalidProjection)?;
                bind_exact_slot(
                    &slots,
                    &mut bound,
                    slot_id,
                    ContentOwnerKind::Policy,
                    topology.graph_id(),
                    ContentFieldKind::PolicyText,
                    ordinal,
                )?;
            }
        }
    }

    if version.content_slots().iter().any(|slot| {
        matches!(
            slot.owner_kind(),
            ContentOwnerKind::Edge | ContentOwnerKind::Policy
        ) && !bound.contains(slot.slot_id().as_str())
            || (slot.owner_kind() == ContentOwnerKind::Node
                && matches!(
                    slot.field_kind(),
                    ContentFieldKind::Instructions
                        | ContentFieldKind::CompletionContract
                        | ContentFieldKind::ContextPath
                        | ContentFieldKind::PermissionPath
                        | ContentFieldKind::IsolationPath
                )
                && !bound.contains(slot.slot_id().as_str()))
    }) {
        return Err(GraphError::InvalidProjection);
    }
    Ok(())
}

fn nested_slot_ordinal(parent: usize, child: usize) -> Result<u32, GraphError> {
    parent
        .checked_mul(64)
        .and_then(|value| value.checked_add(child))
        .and_then(|value| u32::try_from(value).ok())
        .ok_or(GraphError::InvalidProjection)
}

#[allow(clippy::too_many_arguments)]
fn bind_exact_slot(
    slots: &BTreeMap<&str, &ContentSlot>,
    bound: &mut BTreeSet<String>,
    slot_id: &str,
    owner_kind: ContentOwnerKind,
    owner_id: &OpaqueId,
    field_kind: ContentFieldKind,
    ordinal: u32,
) -> Result<(), GraphError> {
    let slot = slots.get(slot_id).ok_or(GraphError::InvalidProjection)?;
    if slot.owner_kind() != owner_kind
        || slot.owner_id() != owner_id
        || slot.field_kind() != field_kind
        || slot.ordinal() != ordinal
        || !bound.insert(slot_id.to_owned())
    {
        return Err(GraphError::InvalidProjection);
    }
    Ok(())
}

fn validate_projection_content(version: &PersistedGraphVersion) -> Result<(), GraphError> {
    let mut scanner = DurableContentScanner::default();
    let topology = version.topology();
    for value in [
        topology.graph_id().as_str(),
        topology.execution_id().as_str(),
        version.created_by().id().as_str(),
    ] {
        scanner
            .scan_text(value)
            .map_err(|_| GraphError::InvalidProjection)?;
    }
    for (key, value) in topology.labels() {
        scanner
            .scan_text(key.as_str())
            .map_err(|_| GraphError::InvalidProjection)?;
        scanner
            .scan_text(value.as_str())
            .map_err(|_| GraphError::InvalidProjection)?;
    }
    for entrypoint in topology.entrypoints() {
        scanner
            .scan_text(entrypoint.as_str())
            .map_err(|_| GraphError::InvalidProjection)?;
    }
    for (node_id, node) in topology.nodes() {
        scanner
            .scan_text(node_id.as_str())
            .map_err(|_| GraphError::InvalidProjection)?;
        for slot_id in node.content_slot_ids() {
            scanner
                .scan_text(slot_id.as_str())
                .map_err(|_| GraphError::InvalidProjection)?;
        }
        for control in node.controls() {
            scan_control_content(control, &mut scanner)?;
        }
    }
    for edge in topology.edges() {
        for value in [edge.id().as_str(), edge.from().as_str(), edge.to().as_str()] {
            scanner
                .scan_text(value)
                .map_err(|_| GraphError::InvalidProjection)?;
        }
        for (key, value) in edge.bindings() {
            scanner
                .scan_text(key.as_str())
                .map_err(|_| GraphError::InvalidProjection)?;
            scanner
                .scan_text(value.as_str())
                .map_err(|_| GraphError::InvalidProjection)?;
            if value.as_str().starts_with(REFERENCE_PREFIX) {
                let decoded = decode_persisted_reference(value.as_str())?;
                scanner
                    .scan_text(&decoded)
                    .map_err(|_| GraphError::InvalidProjection)?;
            }
        }
        if let Some(condition) = edge.condition() {
            scan_control_content(condition, &mut scanner)?;
        }
    }
    for policy in topology.policies() {
        scan_control_content(policy, &mut scanner)?;
    }
    scan_control_content(topology.completion(), &mut scanner)?;
    for slot in version.content_slots() {
        for value in [
            slot.slot_id().as_str(),
            slot.owner_id().as_str(),
            slot.evidence_id().as_str(),
        ] {
            scanner
                .scan_text(value)
                .map_err(|_| GraphError::InvalidProjection)?;
        }
    }
    Ok(())
}

fn scan_control_content(
    control: &PersistedControl,
    scanner: &mut DurableContentScanner,
) -> Result<(), GraphError> {
    scanner
        .scan_text(control.control_type().as_str())
        .map_err(|_| GraphError::InvalidProjection)?;
    for (key, value) in control.identifiers() {
        scanner
            .scan_text(key.as_str())
            .map_err(|_| GraphError::InvalidProjection)?;
        scanner
            .scan_text(value.as_str())
            .map_err(|_| GraphError::InvalidProjection)?;
        if value.as_str().starts_with(REFERENCE_PREFIX) {
            let decoded = decode_persisted_reference(value.as_str())?;
            scanner
                .scan_text(&decoded)
                .map_err(|_| GraphError::InvalidProjection)?;
        }
    }
    for key in control
        .digests()
        .keys()
        .chain(control.integers().keys())
        .chain(control.flags().keys())
    {
        scanner
            .scan_text(key.as_str())
            .map_err(|_| GraphError::InvalidProjection)?;
    }
    Ok(())
}

fn validate_control_node_relations(topology: &PersistedTopology) -> Result<(), GraphError> {
    for (node_id, node) in topology.nodes() {
        for control in node.controls() {
            let relational_key = match control.control_type().as_str() {
                "gate_configuration" => Some("failureRoute"),
                "deploy_configuration" => Some("compensationNode"),
                _ => None,
            };
            if let Some(key) = relational_key
                && control
                    .identifiers()
                    .iter()
                    .find(|(candidate, _)| candidate.as_str() == key)
                    .is_some_and(|(_, value)| !topology_contains_node(topology, value.as_str()))
            {
                return Err(GraphError::InvalidProjection);
            }
        }
        for control in node.controls().iter().filter(|control| {
            matches!(
                control.control_type().as_str(),
                "input_contract" | "output_contract"
            )
        }) {
            for (key, value) in control.identifiers() {
                if indexed_reference_key(key.as_str(), "bindingValue.")
                    && parse_persisted_binding_reference_node(value.as_str())?
                        .is_some_and(|id| !topology_contains_node(topology, id))
                {
                    return Err(GraphError::InvalidProjection);
                }
            }
        }
        for control in node.controls() {
            match control.control_type().as_str() {
                "node_model" | "agent_model_requirements" => {
                    for (key, value) in control.identifiers() {
                        if indexed_suffix(key.as_str(), "independent.").is_some()
                            && (value.as_str() == node_id.as_str()
                                || !topology_contains_node(topology, value.as_str()))
                        {
                            return Err(GraphError::InvalidProjection);
                        }
                    }
                }
                "node_context" => {
                    for (key, value) in control.identifiers() {
                        if indexed_suffix(key.as_str(), "includeNode.").is_some()
                            && (value.as_str() == node_id.as_str()
                                || !topology_contains_node(topology, value.as_str()))
                        {
                            return Err(GraphError::InvalidProjection);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    for edge in topology.edges() {
        for value in edge.bindings().values() {
            if parse_persisted_binding_reference_node(value.as_str())?
                .is_some_and(|id| !topology_contains_node(topology, id))
            {
                return Err(GraphError::InvalidProjection);
            }
        }
    }

    for (key, value) in topology.completion().identifiers() {
        if (key.as_str() == "terminalNode" || indexed_reference_key(key.as_str(), "terminal."))
            && !topology_contains_node(topology, value.as_str())
        {
            return Err(GraphError::InvalidProjection);
        }
    }
    Ok(())
}

fn validate_persisted_topology(validated: &ValidatedTopology<'_>) -> Result<(), GraphError> {
    let topology = validated.topology;
    let mut outgoing: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut incoming: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for edge in topology.edges() {
        outgoing
            .entry(edge.from().as_str())
            .or_default()
            .push(edge.to().as_str());
        incoming
            .entry(edge.to().as_str())
            .or_default()
            .push(edge.from().as_str());
    }
    for edges in outgoing.values_mut().chain(incoming.values_mut()) {
        edges.sort_unstable();
    }

    for node in topology.nodes().values() {
        if node.node_type().as_str() == "deploy" {
            let target = node
                .controls()
                .iter()
                .find(|control| control.control_type().as_str() == "node_configuration")
                .and_then(|control| identifier(control, "targetRef"));
            if target.is_none() {
                return Err(GraphError::InvalidProjection);
            }
            if let Some(effects) = node
                .controls()
                .iter()
                .find(|control| control.control_type().as_str() == "deploy_configuration")
            {
                let requires_compensation = effects.flags().iter().any(|(key, value)| {
                    matches!(key.as_str(), "reversible" | "compensationRequired") && *value
                });
                if requires_compensation {
                    let compensation = identifier(effects, "compensationNode")
                        .ok_or(GraphError::InvalidProjection)?;
                    let target = topology
                        .nodes()
                        .iter()
                        .find(|(id, _)| id.as_str() == compensation)
                        .map(|(_, node)| node)
                        .ok_or(GraphError::InvalidProjection)?;
                    if target.node_type().as_str() != "rollback" {
                        return Err(GraphError::InvalidProjection);
                    }
                }
            }
        }
    }

    let terminals = topology
        .completion()
        .identifiers()
        .iter()
        .filter(|(key, _)| indexed_reference_key(key.as_str(), "terminal."))
        .map(|(_, value)| value.as_str())
        .collect::<Vec<_>>();
    if terminals.is_empty()
        || terminals.iter().copied().collect::<BTreeSet<_>>().len() != terminals.len()
        || terminals
            .iter()
            .any(|id| !topology_contains_node(topology, id))
    {
        return Err(GraphError::InvalidProjection);
    }

    let reachable = traverse_topology(
        topology.entrypoints().iter().map(|id| id.as_str()),
        &outgoing,
    );
    let can_finish = traverse_topology(terminals.iter().copied(), &incoming);
    if reachable.iter().any(|node| !can_finish.contains(node)) {
        return Err(GraphError::InvalidProjection);
    }

    let components = persisted_components(topology, &outgoing, &incoming);
    for component in &components {
        let start = component[0];
        let cyclic = component.len() > 1
            || outgoing
                .get(start)
                .is_some_and(|next| next.contains(&start));
        if cyclic
            && !component.iter().any(|id| {
                topology
                    .nodes()
                    .iter()
                    .find(|(node_id, _)| node_id.as_str() == *id)
                    .is_some_and(|(_, node)| {
                        node.controls().iter().any(|control| {
                            control.control_type().as_str() == "node_loop"
                                && control.integers().iter().any(|(key, value)| {
                                    key.as_str() == "maxIterations" && *value > 0
                                })
                        })
                    })
            })
        {
            return Err(GraphError::InvalidProjection);
        }
    }
    validate_persisted_budgets(topology, &components)?;
    Ok(())
}

fn persisted_components<'a>(
    topology: &'a PersistedTopology,
    outgoing: &BTreeMap<&'a str, Vec<&'a str>>,
    incoming: &BTreeMap<&'a str, Vec<&'a str>>,
) -> Vec<Vec<&'a str>> {
    let mut visited = BTreeSet::new();
    let mut order = Vec::new();
    for start in topology.nodes().keys().map(|id| id.as_str()) {
        if visited.contains(start) {
            continue;
        }
        let mut stack = vec![(start, false)];
        while let Some((node, expanded)) = stack.pop() {
            if expanded {
                order.push(node);
            } else if visited.insert(node) {
                stack.push((node, true));
                for next in outgoing.get(node).into_iter().flatten().rev() {
                    if !visited.contains(next) {
                        stack.push((next, false));
                    }
                }
            }
        }
    }
    visited.clear();
    let mut components = Vec::new();
    while let Some(start) = order.pop() {
        if visited.contains(start) {
            continue;
        }
        let mut component = Vec::new();
        let mut stack = vec![start];
        while let Some(node) = stack.pop() {
            if visited.insert(node) {
                component.push(node);
                stack.extend(incoming.get(node).into_iter().flatten().copied());
            }
        }
        component.sort_unstable();
        components.push(component);
    }
    components
}

fn validate_persisted_budgets(
    topology: &PersistedTopology,
    components: &[Vec<&str>],
) -> Result<(), GraphError> {
    if topology.budgets().max_nodes().is_some_and(|limit| {
        u64::try_from(topology.nodes().len()).map_or(true, |count| count > limit)
    }) {
        return Err(GraphError::InvalidProjection);
    }

    if let Some(limit) = topology.budgets().max_depth() {
        let mut component_by_node = BTreeMap::new();
        for (index, component) in components.iter().enumerate() {
            for node in component {
                component_by_node.insert(*node, index);
            }
        }
        let mut indegree = vec![0usize; components.len()];
        let mut edges = vec![BTreeSet::new(); components.len()];
        for edge in topology.edges() {
            let from = *component_by_node
                .get(edge.from().as_str())
                .ok_or(GraphError::InvalidProjection)?;
            let to = *component_by_node
                .get(edge.to().as_str())
                .ok_or(GraphError::InvalidProjection)?;
            if from != to && edges[from].insert(to) {
                indegree[to] += 1;
            }
        }
        let mut queue = std::collections::VecDeque::from_iter(
            indegree
                .iter()
                .enumerate()
                .filter_map(|(index, degree)| (*degree == 0).then_some(index)),
        );
        let mut depths = vec![1u64; components.len()];
        let mut maximum = u64::from(!components.is_empty());
        while let Some(component) = queue.pop_front() {
            maximum = maximum.max(depths[component]);
            for target in edges[component].iter().copied() {
                depths[target] = depths[target].max(depths[component].saturating_add(1));
                indegree[target] -= 1;
                if indegree[target] == 0 {
                    queue.push_back(target);
                }
            }
        }
        if maximum > limit {
            return Err(GraphError::InvalidProjection);
        }
    }

    if let Some(retries) = topology.budgets().max_retries_per_node() {
        let maximum_attempts = retries
            .checked_add(1)
            .ok_or(GraphError::InvalidProjection)?;
        for node in topology.nodes().values() {
            let attempts = node
                .controls()
                .iter()
                .find(|control| control.control_type().as_str() == "node_retry")
                .and_then(|control| {
                    control
                        .integers()
                        .iter()
                        .find(|(key, _)| key.as_str() == "maxAttempts")
                        .map(|(_, value)| *value)
                });
            if attempts.is_some_and(|attempts| {
                u64::try_from(attempts).map_or(true, |attempts| attempts > maximum_attempts)
            }) {
                return Err(GraphError::InvalidProjection);
            }
        }
    }
    Ok(())
}

fn traverse_topology<'a>(
    starts: impl IntoIterator<Item = &'a str>,
    edges: &BTreeMap<&'a str, Vec<&'a str>>,
) -> BTreeSet<&'a str> {
    let mut visited = BTreeSet::new();
    let mut queue = std::collections::VecDeque::from_iter(starts);
    while let Some(node) = queue.pop_front() {
        if visited.insert(node) {
            queue.extend(edges.get(node).into_iter().flatten().copied());
        }
    }
    visited
}

fn topology_contains_node(topology: &PersistedTopology, node_id: &str) -> bool {
    topology
        .nodes()
        .keys()
        .any(|candidate| candidate.as_str() == node_id)
}

#[cfg(test)]
mod replay_accounting_tests {
    use super::*;

    #[test]
    fn budget_component_lookup_is_fallible_even_behind_the_endpoint_gate() {
        let mut value: Value = serde_json::from_str(include_str!(
            "../../../conformance/schemas/valid/persisted-graph-version.json"
        ))
        .unwrap();
        value["topology"]["edges"] = json!([{
            "id":"foreign-edge", "from":"foreign-endpoint", "to":"start",
            "edgeType":"control", "priority":null, "bindings":{}, "condition":null
        }]);
        let version: PersistedGraphVersion = serde_json::from_value(value).unwrap();
        let components = vec![vec!["start"]];
        let result = std::panic::catch_unwind(|| {
            validate_persisted_budgets(version.topology(), &components)
        });
        assert_eq!(result.unwrap(), Err(GraphError::InvalidProjection));
    }
}

const fn valid_slot_field(owner: ContentOwnerKind, field: ContentFieldKind) -> bool {
    match owner {
        ContentOwnerKind::Graph => matches!(
            field,
            ContentFieldKind::DisplayName | ContentFieldKind::Description
        ),
        ContentOwnerKind::Node => matches!(
            field,
            ContentFieldKind::DisplayName
                | ContentFieldKind::Description
                | ContentFieldKind::Objective
                | ContentFieldKind::Instructions
                | ContentFieldKind::CompletionContract
                | ContentFieldKind::ContextPath
                | ContentFieldKind::PermissionPath
                | ContentFieldKind::IsolationPath
        ),
        ContentOwnerKind::Agent => matches!(
            field,
            ContentFieldKind::Purpose
                | ContentFieldKind::Instructions
                | ContentFieldKind::CompletionContract
        ),
        ContentOwnerKind::Edge | ContentOwnerKind::Policy => {
            matches!(field, ContentFieldKind::PolicyText)
        }
        ContentOwnerKind::Diagnostic => false,
    }
}

/// Closed semantic family for one registered durable reference position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PersistedReferenceDomain {
    Agent,
    Artifact,
    CompletionRequirement,
    ContextInclude,
    ContextPolicy,
    Contract,
    DeployAdapter,
    Directive,
    Environment,
    Evaluator,
    GraphTemplate,
    Policy,
    Rules,
    Schema,
    SchemaDialect,
    SchemaIdentity,
    Tool,
    Document,
}

/// Returns the one closed reference family registered for a control position.
pub fn persisted_reference_domain(
    control_type: &str,
    key: &str,
) -> Option<PersistedReferenceDomain> {
    use PersistedReferenceDomain as Domain;
    match control_type {
        "agent_configuration" => match key {
            "agentRef" => Some(Domain::Agent),
            "inputSchema" | "resultSchema" => Some(Domain::Schema),
            "directiveRef" => Some(Domain::Directive),
            _ => None,
        },
        "node_agents" if indexed_reference_key(key, "agentRef.") => Some(Domain::Agent),
        "agent_evidence_requirements" if indexed_reference_key(key, "ref.") => {
            Some(Domain::Artifact)
        }
        "input_contract" | "output_contract" => match key {
            "schema" => Some(Domain::Schema),
            "schemaId" => Some(Domain::SchemaIdentity),
            "schemaDialect" => Some(Domain::SchemaDialect),
            "publishAs" => Some(Domain::Artifact),
            _ => None,
        },
        "agent_context_strategy" | "node_context" if key == "policyRef" => {
            Some(Domain::ContextPolicy)
        }
        "node_context" if indexed_reference_key(key, "includeRef.") => Some(Domain::ContextInclude),
        "agent_memory_policy" | "node_memory" if key == "policyRef" => Some(Domain::ContextPolicy),
        "node_configuration" if key == "targetRef" => Some(Domain::Environment),
        "edge_condition" if key == "schema" => Some(Domain::Schema),
        "tool_configuration" if key == "toolRef" => Some(Domain::Tool),
        "classifier_configuration" if key == "rulesRef" => Some(Domain::Rules),
        "gate_configuration" if indexed_reference_key(key, "evaluator.") => Some(Domain::Evaluator),
        "join_configuration" if key == "resultSchema" => Some(Domain::Schema),
        "subgraph_configuration" if key == "graphRef" => Some(Domain::GraphTemplate),
        "subgraph_configuration" if indexed_reference_key(key, "parameterValue.") => {
            Some(Domain::Artifact)
        }
        "materializer_configuration" if key == "target" => Some(Domain::Document),
        "deploy_configuration" | "rollback_configuration" if key == "adapterRef" => {
            Some(Domain::DeployAdapter)
        }
        "deploy_configuration" if indexed_reference_key(key, "precondition.") => {
            Some(Domain::Artifact)
        }
        "node_completion" if key == "contractRef" => Some(Domain::Contract),
        "node_completion"
            if indexed_reference_key(key, "requiresArtifact.")
                || indexed_reference_key(key, "forbidsArtifact.") =>
        {
            Some(Domain::Artifact)
        }
        "policy_control" if key == "policyRef" => Some(Domain::Policy),
        "graph_completion" if indexed_reference_key(key, "requirement.") => {
            Some(Domain::CompletionRequirement)
        }
        _ => None,
    }
}

/// Validates the raw reference carried by one context-include discriminant.
///
/// Foundation permits a reference only for the `document` include image, and
/// that position is closed to the Document domain.
pub fn validate_context_include_reference(
    include_type: &str,
    value: &str,
) -> Result<(), GraphError> {
    if include_type == "document"
        && reference_matches_domain(value, PersistedReferenceDomain::Document)
    {
        Ok(())
    } else {
        Err(GraphError::InvalidProjection)
    }
}

/// Validates every encoded reference using the same closed registry as replay.
pub fn validate_persisted_control_references(control: &PersistedControl) -> Result<(), GraphError> {
    reject_unexpected_reference(control.control_type())?;
    for (key, value) in control.identifiers() {
        if matches!(
            control.control_type().as_str(),
            "input_contract" | "output_contract"
        ) && indexed_reference_key(key.as_str(), "bindingValue.")
        {
            require_binding(value)?;
        } else if let Some(domain) =
            persisted_reference_domain(control.control_type().as_str(), key.as_str())
        {
            if domain == PersistedReferenceDomain::Directive
                && !value.as_str().starts_with(REFERENCE_PREFIX)
            {
                parse_persisted_nominal_identifier(value.as_str())?;
            } else {
                let raw = decode_persisted_reference(value.as_str())?;
                if !reference_matches_domain(&raw, domain) {
                    return Err(GraphError::InvalidProjection);
                }
            }
        } else {
            reject_unexpected_reference(value)?;
        }
    }
    Ok(())
}

fn reference_matches_domain(value: &str, domain: PersistedReferenceDomain) -> bool {
    use PersistedReferenceDomain as Domain;
    match domain {
        Domain::Agent => slash_reference(value, "project/", 1, 2),
        Domain::Artifact => is_artifact_reference(value),
        Domain::CompletionRequirement => {
            is_artifact_reference(value) || slash_reference(value, "document://", 1, 4)
        }
        Domain::ContextInclude => {
            is_artifact_reference(value)
                || slash_reference(value, "context://", 1, 4)
                || slash_reference(value, "document://", 1, 4)
        }
        Domain::ContextPolicy => reference_with_token(value, "context-policy://", true),
        Domain::Contract => reference_with_token(value, "contract://", true),
        Domain::DeployAdapter => reference_with_token(value, "deploy://", true),
        Domain::Directive => {
            is_artifact_reference(value) || reference_with_token(value, "contract://", true)
        }
        Domain::Environment => environment_reference(value),
        Domain::Evaluator => reference_with_token(value, "evaluator://", true),
        Domain::GraphTemplate => reference_with_token(value, "graph-template://", true),
        Domain::Policy => slash_reference(value, "policy://", 2, 3),
        Domain::Rules => reference_with_token(value, "rules://", true),
        Domain::Schema => reference_with_token(value, "schema://", false),
        Domain::SchemaDialect => value == "https://json-schema.org/draft/2020-12/schema",
        Domain::SchemaIdentity => value
            .strip_prefix("https://p50.dev/schemas/")
            .and_then(|tail| tail.strip_suffix(".schema.json"))
            .is_some_and(|name| nominal_token(name, false)),
        Domain::Tool => slash_reference(value, "builtin/", 1, 2),
        Domain::Document => slash_reference(value, "document://", 1, 4),
    }
}

fn indexed_reference_key(value: &str, prefix: &str) -> bool {
    value
        .strip_prefix(prefix)
        .is_some_and(|index| index.len() == 3 && index.bytes().all(|byte| byte.is_ascii_digit()))
}

fn indexed_key(value: &str, prefix: &str) -> bool {
    indexed_suffix(value, prefix).is_some()
}

fn require_binding(value: &SafeValue) -> Result<(), GraphError> {
    parse_persisted_binding(value.as_str()).map(drop)
}

fn reject_unexpected_reference(value: &SafeValue) -> Result<(), GraphError> {
    if value.as_str().starts_with(REFERENCE_PREFIX) {
        return Err(GraphError::InvalidProjection);
    }
    Ok(())
}

fn encode_base64url(bytes: &[u8], output: &mut String) {
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        output.push(char::from(BASE64_URL[usize::from(first >> 2)]));
        let second_index = ((first & 0x03) << 4) | chunk.get(1).copied().unwrap_or(0) >> 4;
        output.push(char::from(BASE64_URL[usize::from(second_index)]));
        if let Some(second) = chunk.get(1).copied() {
            let third_index = ((second & 0x0f) << 2) | chunk.get(2).copied().unwrap_or(0) >> 6;
            output.push(char::from(BASE64_URL[usize::from(third_index)]));
        }
        if let Some(third) = chunk.get(2).copied() {
            output.push(char::from(BASE64_URL[usize::from(third & 0x3f)]));
        }
    }
}

fn decode_base64url(payload: &str) -> Result<Vec<u8>, GraphError> {
    let sextets = payload
        .bytes()
        .map(decode_sextet)
        .collect::<Result<Vec<_>, _>>()?;
    match sextets.len() % 4 {
        2 if sextets[sextets.len() - 1] & 0x0f != 0 => return Err(GraphError::InvalidProjection),
        3 if sextets[sextets.len() - 1] & 0x03 != 0 => return Err(GraphError::InvalidProjection),
        0 | 2 | 3 => {}
        _ => return Err(GraphError::InvalidProjection),
    }
    let mut bytes = Vec::with_capacity(sextets.len() * 3 / 4);
    for chunk in sextets.chunks(4) {
        bytes.push((chunk[0] << 2) | (chunk[1] >> 4));
        if let Some(third) = chunk.get(2).copied() {
            bytes.push((chunk[1] << 4) | (third >> 2));
            if let Some(fourth) = chunk.get(3).copied() {
                bytes.push((third << 6) | fourth);
            }
        }
    }
    Ok(bytes)
}

fn decode_sextet(byte: u8) -> Result<u8, GraphError> {
    match byte {
        b'A'..=b'Z' => Ok(byte - b'A'),
        b'a'..=b'z' => Ok(byte - b'a' + 26),
        b'0'..=b'9' => Ok(byte - b'0' + 52),
        b'-' => Ok(62),
        b'_' => Ok(63),
        _ => Err(GraphError::InvalidProjection),
    }
}

fn is_registered_reference(value: &str) -> bool {
    let mut operations = 0;
    value.is_ascii()
        && !contains_reference_secret_name(value.as_bytes(), &mut operations)
        && (is_artifact_reference(value)
            || exact_schema_identity(value)
            || reference_with_token(value, "schema://", false)
            || reference_with_token(value, "context-policy://", true)
            || reference_with_token(value, "contract://", true)
            || reference_with_token(value, "rules://", true)
            || reference_with_token(value, "graph-template://", true)
            || reference_with_token(value, "evaluator://", true)
            || reference_with_token(value, "deploy://", true)
            || slash_reference(value, "policy://", 2, 3)
            || slash_reference(value, "project/", 1, 2)
            || slash_reference(value, "builtin/", 1, 2)
            || slash_reference(value, "context://", 1, 4)
            || slash_reference(value, "document://", 1, 4)
            || environment_reference(value))
}

fn is_artifact_reference(value: &str) -> bool {
    if value
        .strip_prefix("artifact://sha256/")
        .is_some_and(|digest| {
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        })
    {
        return true;
    }
    value.strip_prefix("artifact://").is_some_and(|token| {
        nominal_token(token, true) && !token.contains('/') && !token.contains(':')
    })
}

fn exact_schema_identity(value: &str) -> bool {
    if value == "https://json-schema.org/draft/2020-12/schema" {
        return true;
    }
    value
        .strip_prefix("https://p50.dev/schemas/")
        .and_then(|tail| tail.strip_suffix(".schema.json"))
        .is_some_and(|name| nominal_token(name, false))
}

fn environment_reference(value: &str) -> bool {
    value.strip_prefix("environment://").is_some_and(|token| {
        let mut operations = 0;
        nominal_token(token, false)
            && !token.contains('@')
            && !compact_contains_secret_name(token.as_bytes(), &mut operations)
    })
}

fn reference_with_token(value: &str, prefix: &str, version_optional: bool) -> bool {
    let Some(tail) = value.strip_prefix(prefix) else {
        return false;
    };
    let (token, version) = split_version(tail);
    nominal_token(token, true)
        && match version {
            Some(version) => numeric_version(version),
            None => version_optional,
        }
}

fn slash_reference(value: &str, prefix: &str, minimum: usize, maximum: usize) -> bool {
    let Some(tail) = value.strip_prefix(prefix) else {
        return false;
    };
    let (path, version) = split_version(tail);
    if version.is_some_and(|version| !numeric_version(version)) {
        return false;
    }
    let mut count = 0;
    for segment in path.split('/') {
        if !nominal_token(segment, true) {
            return false;
        }
        count += 1;
    }
    (minimum..=maximum).contains(&count)
}

fn split_version(value: &str) -> (&str, Option<&str>) {
    match value.rsplit_once('@') {
        Some((token, version)) => (token, Some(version)),
        None => (value, None),
    }
}

fn numeric_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 10
        && value.as_bytes()[0] != b'0'
        && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn nominal_token(value: &str, uppercase: bool) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_alphanumeric()
        && bytes.iter().all(|byte| {
            byte.is_ascii_digit()
                || byte.is_ascii_lowercase()
                || (uppercase && byte.is_ascii_uppercase())
                || matches!(byte, b'.' | b'_' | b'-')
        })
        && !value.contains("..")
        && value != "."
        && value != ".."
}

/// Canonical compact JSON bytes for one registered authoring content value.
pub fn canonical_content_bytes(value: &Value) -> Result<Vec<u8>, GraphError> {
    preflight_json_structure(value).map_err(|_| GraphError::InvalidProjection)?;
    serde_json::to_vec(&sort_value(value.clone())).map_err(|_| GraphError::InvalidProjection)
}

/// Lowercase SHA-256 of already canonical content bytes.
pub fn raw_content_sha256(bytes: &[u8]) -> Result<RawSha256, GraphError> {
    RawSha256::parse(hex::encode(Sha256::digest(bytes))).map_err(|_| GraphError::InvalidProjection)
}

/// The two deterministic identities of a safe persisted graph projection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistenceHashes {
    topology_hash: WireHash,
    semantic_hash: WireHash,
}

impl PersistenceHashes {
    #[must_use]
    pub const fn topology_hash(&self) -> &WireHash {
        &self.topology_hash
    }

    #[must_use]
    pub const fn semantic_hash(&self) -> &WireHash {
        &self.semantic_hash
    }
}

/// Hashes safe topology independently from Evidence encryption metadata.
pub fn persisted_hashes(
    topology: &PersistedTopology,
    slots: &[ContentSlot],
) -> Result<PersistenceHashes, GraphError> {
    let topology_value = topology_identity(topology, slots)?;
    let semantic_value = sort_value(json!({
        "topology": topology_value,
        "contentDigests": slots.iter().map(|slot| json!({
            "ownerKind": slot.owner_kind(),
            "ownerId": slot.owner_id(),
            "fieldKind": slot.field_kind(),
            "ordinal": slot.ordinal(),
            "contentSha256": slot.content_sha256(),
        })).collect::<Vec<_>>()
    }));

    Ok(PersistenceHashes {
        topology_hash: hash_value(&topology_value)?,
        semantic_hash: hash_value(&semantic_value)?,
    })
}

/// Requires an exact ordered one-to-one match between content slots and Evidence references.
pub fn validate_evidence_bijection(
    slots: &[ContentSlot],
    references: &[EvidenceReference],
) -> Result<(), GraphError> {
    if slots.len() > 8192
        || slots.len() != references.len()
        || slots
            .windows(2)
            .any(|pair| slot_position(&pair[0]) >= slot_position(&pair[1]))
        || slots
            .iter()
            .map(ContentSlot::slot_id)
            .collect::<BTreeSet<_>>()
            .len()
            != slots.len()
        || slots
            .iter()
            .map(ContentSlot::evidence_id)
            .collect::<BTreeSet<_>>()
            .len()
            != slots.len()
        || references
            .iter()
            .map(|reference| reference.evidence_id())
            .collect::<BTreeSet<_>>()
            .len()
            != references.len()
        || slots.iter().zip(references).any(|(slot, reference)| {
            slot.evidence_id() != reference.evidence_id()
                || slot.content_sha256() != reference.content_sha256()
        })
    {
        return Err(GraphError::InvalidProjection);
    }
    Ok(())
}

fn slot_position(
    slot: &ContentSlot,
) -> (
    graphhelm_protocols::ContentOwnerKind,
    &graphhelm_protocols::OpaqueId,
    graphhelm_protocols::ContentFieldKind,
    u32,
) {
    (
        slot.owner_kind(),
        slot.owner_id(),
        slot.field_kind(),
        slot.ordinal(),
    )
}

fn topology_identity(
    topology: &PersistedTopology,
    slots: &[ContentSlot],
) -> Result<Value, GraphError> {
    let topology = serde_json::to_value(topology).map_err(|_| GraphError::InvalidProjection)?;
    Ok(sort_value(json!({
        "topology": topology,
        "contentPositions": slots.iter().map(|slot| json!({
            "ownerKind": slot.owner_kind(),
            "ownerId": slot.owner_id(),
            "fieldKind": slot.field_kind(),
            "ordinal": slot.ordinal(),
        })).collect::<Vec<_>>()
    })))
}

fn hash_value(value: &Value) -> Result<WireHash, GraphError> {
    let bytes = serde_json::to_vec(value).map_err(|_| GraphError::InvalidProjection)?;
    WireHash::parse(format!("sha256:{}", hex::encode(Sha256::digest(bytes))))
        .map_err(|_| GraphError::InvalidProjection)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::{TimeZone, Utc};
    use graphhelm_protocols::{
        Actor, ActorType, EdgeType, ExecutionGraph, GraphBudgets, GraphEdge, GraphMetadata,
        GraphNode, GraphSpec, GraphVersionRecord, NodeType, Optionality, PersistedControl, SafeKey,
        SafeValue, SemanticHash,
    };

    use super::{
        DurableContentError, MAX_CONTENT_SCAN_DEPTH, MAX_CONTENT_SCAN_VALUES,
        decode_persisted_reference, durable_content_scan_operation_count,
        encode_persisted_reference, parse_persisted_nominal_identifier, preflight_execution_graph,
        preflight_persistence_values_with_metrics,
    };

    #[test]
    fn whole_record_preflight_matches_the_complete_serialized_inventory() {
        fn inventory(value: &serde_json::Value) -> (usize, usize) {
            match value {
                serde_json::Value::Object(object) => {
                    object
                        .iter()
                        .fold((1, 0), |(value_count, bytes), (key, value)| {
                            let (child_count, child_bytes) = inventory(value);
                            (
                                value_count + 1 + child_count,
                                bytes + key.len() + child_bytes,
                            )
                        })
                }
                serde_json::Value::Array(array) => {
                    array.iter().fold((1, 0), |(value_count, bytes), value| {
                        let (child_count, child_bytes) = inventory(value);
                        (value_count + child_count, bytes + child_bytes)
                    })
                }
                serde_json::Value::String(value) => (1, value.len()),
                serde_json::Value::Null
                | serde_json::Value::Bool(_)
                | serde_json::Value::Number(_) => (1, 0),
            }
        }

        let mut record = GraphVersionRecord {
            graph: ExecutionGraph {
                api_version: "p50.dev/v1".into(),
                kind: "ExecutionGraph".into(),
                metadata: GraphMetadata {
                    id: "graph-inventory".into(),
                    name: "Inventory graph".into(),
                    execution_id: "execution-inventory".into(),
                    version: 2,
                    based_on: Some("graph-base".into()),
                    labels: BTreeMap::from([("team".into(), "runtime".into())]),
                    properties: BTreeMap::from([(
                        "extension".into(),
                        serde_json::json!({"nested": [null, true, "value"]}),
                    )]),
                },
                spec: GraphSpec {
                    entrypoints: vec!["node-a".into()],
                    nodes: BTreeMap::from([(
                        "node-a".into(),
                        GraphNode {
                            node_type: NodeType::Agent,
                            name: "Node A".into(),
                            objective: "Inventory".into(),
                            optionality: Optionality::Recommended,
                            properties: BTreeMap::from([(
                                "control".into(),
                                serde_json::json!({"enabled": true}),
                            )]),
                        },
                    )]),
                    edges: vec![GraphEdge {
                        id: "edge-a".into(),
                        from: "node-a".into(),
                        to: "node-a".into(),
                        edge_type: EdgeType::Control,
                        payload_schema: Some("schema://Payload@1".into()),
                        condition: Some(serde_json::json!({"when": true})),
                        on_false: Some(serde_json::Value::Null),
                        on_unknown: Some(graphhelm_protocols::UnknownConditionBehavior::Pause),
                        bindings: BTreeMap::from([("input".into(), "outputs.node-a.value".into())]),
                        priority: Some(1),
                    }],
                    budgets: GraphBudgets {
                        max_nodes: Some(4),
                        max_depth: None,
                        max_mutations: Some(8),
                        max_retries_per_node: None,
                        max_wall_clock_seconds: Some(60),
                        max_api_cost_usd: Some(1.25),
                        max_parallel_model_calls: Some(2),
                    },
                    policies: vec![serde_json::json!({"mode": "strict"})],
                    completion: serde_json::json!({"terminalNodes": ["node-a"]}),
                },
            },
            predecessor: Some(graphhelm_protocols::GraphVersionRef {
                number: 1,
                content_hash: SemanticHash::new(format!("sha256:{}", "1".repeat(64))),
            }),
            semantic: serde_json::json!({"semanticGraph": {"nodes": [null, false]}}),
            content_hash: SemanticHash::new(format!("sha256:{}", "0".repeat(64))),
            created_by: Actor::new(ActorType::Agent, "agent-inventory"),
            created_at: Utc.with_ymd_and_hms(2026, 8, 10, 12, 0, 0).unwrap(),
        };

        let serialized = serde_json::to_value(&record).unwrap();
        assert_eq!(
            serialized["createdBy"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            vec!["id", "type"]
        );
        assert_eq!(
            serialized["createdAt"].as_str().unwrap(),
            record
                .created_at
                .to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)
        );
        let mut expected = inventory(&serialized);
        // Flattened maps have no independent JSON object in the wire image,
        // but remain typed input containers and therefore each consume one
        // structural value under the shared preflight rules.
        expected.0 += 1 + record.graph.spec.nodes.len();
        let usage = super::preflight_graph_version_record_usage(&record).unwrap();
        assert_eq!((usage.value_count, usage.bytes), expected);

        record.predecessor = None;
        record.graph.metadata.based_on = None;
        let edge = &mut record.graph.spec.edges[0];
        edge.payload_schema = None;
        edge.condition = None;
        edge.on_false = None;
        edge.on_unknown = None;
        edge.priority = None;
        let serialized_without_options = serde_json::to_value(&record).unwrap();
        let mut expected_without_options = inventory(&serialized_without_options);
        expected_without_options.0 += 1 + record.graph.spec.nodes.len();
        let usage_without_options = super::preflight_graph_version_record_usage(&record).unwrap();
        assert_eq!(
            (
                usage_without_options.value_count,
                usage_without_options.bytes
            ),
            expected_without_options
        );

        record.semantic = serde_json::Value::Null;
        let base_count = super::preflight_graph_version_record_usage(&record)
            .unwrap()
            .value_count;
        record.semantic = serde_json::Value::Array(
            (0..(MAX_CONTENT_SCAN_VALUES - base_count))
                .map(|_| serde_json::Value::Null)
                .collect(),
        );
        assert!(super::preflight_graph_version_record_usage(&record).is_ok());
        record
            .semantic
            .as_array_mut()
            .unwrap()
            .push(serde_json::Value::Null);
        assert!(matches!(
            super::preflight_graph_version_record_usage(&record),
            Err(DurableContentError::LimitExceeded)
        ));
    }

    #[test]
    fn persistence_preflight_keeps_wide_work_stack_bounded_by_depth() {
        let wide_array = serde_json::Value::Array(
            (0..=MAX_CONTENT_SCAN_VALUES)
                .map(|_| serde_json::Value::Null)
                .collect(),
        );
        let wide_object = serde_json::Value::Object(
            (0..=MAX_CONTENT_SCAN_VALUES)
                .map(|index| (format!("k{index:06}"), serde_json::Value::Null))
                .collect(),
        );

        for value in [&wide_array, &wide_object] {
            let (result, max_work_stack) =
                preflight_persistence_values_with_metrics(std::iter::once(value));
            assert_eq!(result, Err(DurableContentError::LimitExceeded));
            assert!(
                max_work_stack <= MAX_CONTENT_SCAN_DEPTH,
                "wide input retained {max_work_stack} pending frames"
            );
        }
    }

    #[test]
    fn persistence_preflight_counts_nested_values_without_width_scaled_frames() {
        let accepted = serde_json::json!({"outer": [[null, true], {"leaf": "value"}]});
        let (result, max_work_stack) =
            preflight_persistence_values_with_metrics(std::iter::once(&accepted));
        assert_eq!(result, Ok(()));
        assert!(max_work_stack <= 4);

        let mut nested_wide = serde_json::Value::Array(
            (0..=MAX_CONTENT_SCAN_VALUES)
                .map(|_| serde_json::Value::Null)
                .collect(),
        );
        for _ in 0..(MAX_CONTENT_SCAN_DEPTH - 1) {
            nested_wide = serde_json::Value::Array(vec![nested_wide]);
        }
        let (result, max_work_stack) =
            preflight_persistence_values_with_metrics(std::iter::once(&nested_wide));
        assert_eq!(result, Err(DurableContentError::LimitExceeded));
        assert_eq!(max_work_stack, MAX_CONTENT_SCAN_DEPTH);
    }

    #[test]
    fn persistence_preflight_counts_object_keys_and_bounds_each_key() {
        let near_value_ceiling = serde_json::Value::Object(
            (0..65_536)
                .map(|index| (format!("key-{index:05}"), serde_json::Value::Null))
                .collect(),
        );
        assert_eq!(
            super::preflight_persistence_values(std::iter::once(&near_value_ceiling)),
            Err(DurableContentError::LimitExceeded)
        );

        let oversized_key = serde_json::Value::Object(
            std::iter::once((
                "k".repeat(super::MAX_RECORD_STRING_BYTES + 1),
                serde_json::Value::Null,
            ))
            .collect(),
        );
        assert_eq!(
            super::preflight_persistence_values(std::iter::once(&oversized_key)),
            Err(DurableContentError::LimitExceeded)
        );
    }

    #[test]
    fn execution_graph_preflight_accounts_serialized_based_on_null_at_the_boundary() {
        let mut graph = ExecutionGraph {
            api_version: "p50.dev/v1".into(),
            kind: "ExecutionGraph".into(),
            metadata: GraphMetadata {
                id: "graph-boundary".into(),
                name: "Boundary graph".into(),
                execution_id: "execution-boundary".into(),
                version: 1,
                based_on: None,
                labels: BTreeMap::new(),
                properties: BTreeMap::new(),
            },
            spec: GraphSpec {
                entrypoints: Vec::new(),
                nodes: BTreeMap::new(),
                edges: Vec::new(),
                budgets: GraphBudgets::default(),
                policies: Vec::new(),
                completion: serde_json::Value::Null,
            },
        };
        let none_count = super::preflight_execution_graph_usage(&graph)
            .unwrap()
            .value_count;
        let mut with_based_on = graph.clone();
        with_based_on.metadata.based_on = Some("graph-predecessor".into());
        let some_count = super::preflight_execution_graph_usage(&with_based_on)
            .unwrap()
            .value_count;
        assert_eq!(none_count, some_count);

        let filler = MAX_CONTENT_SCAN_VALUES + 1 - some_count;
        graph.spec.completion =
            serde_json::Value::Array((0..filler).map(|_| serde_json::Value::Null).collect());

        assert_eq!(
            preflight_execution_graph(&graph),
            Err(DurableContentError::LimitExceeded)
        );
    }

    #[test]
    fn registered_reference_families_round_trip_canonically() {
        let references = [
            "schema://PlanningRequest@1",
            "context-policy://blind-review@1",
            "contract://planning-instructions@2",
            "rules://security-baseline@1",
            "evaluator://security-report-validator@1",
            "graph-template://security-review@3",
            "deploy://docker-compose@1",
            "policy://workspace/security-baseline@2",
            "environment://staging",
            "project/security-reviewer@3",
            "builtin/repository-reader@1",
            "context://task/request",
            "document://architecture/auth",
            "artifact://sha256/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "artifact://implementation.diff",
            "https://json-schema.org/draft/2020-12/schema",
            "https://p50.dev/schemas/test-input.schema.json",
        ];
        for reference in references {
            let encoded = encode_persisted_reference(reference).unwrap();
            assert_eq!(
                decode_persisted_reference(encoded.as_str()).unwrap(),
                reference
            );
        }
    }

    #[test]
    fn every_registered_reference_position_enforces_its_exact_domain() {
        let cases = [
            (
                "agent_configuration",
                "agentRef",
                "project/security-reviewer@3",
            ),
            (
                "agent_configuration",
                "inputSchema",
                "schema://AgentInput@1",
            ),
            (
                "agent_configuration",
                "resultSchema",
                "schema://AgentResult@1",
            ),
            (
                "agent_configuration",
                "directiveRef",
                "contract://agent-directive@1",
            ),
            (
                "agent_evidence_requirements",
                "ref.000",
                "artifact://review.diff",
            ),
            ("input_contract", "schema", "schema://Input@1"),
            (
                "input_contract",
                "schemaId",
                "https://p50.dev/schemas/input.schema.json",
            ),
            (
                "input_contract",
                "schemaDialect",
                "https://json-schema.org/draft/2020-12/schema",
            ),
            ("output_contract", "publishAs", "artifact://result.json"),
            (
                "node_context",
                "policyRef",
                "context-policy://blind-review@1",
            ),
            (
                "node_context",
                "includeRef.000",
                "document://architecture/auth",
            ),
            (
                "agent_memory_policy",
                "policyRef",
                "context-policy://memory@1",
            ),
            ("node_memory", "policyRef", "context-policy://memory@1"),
            ("node_configuration", "targetRef", "environment://staging"),
            ("edge_condition", "schema", "schema://Condition@1"),
            (
                "tool_configuration",
                "toolRef",
                "builtin/repository-reader@1",
            ),
            (
                "classifier_configuration",
                "rulesRef",
                "rules://classification@1",
            ),
            (
                "gate_configuration",
                "evaluator.000",
                "evaluator://security@1",
            ),
            (
                "join_configuration",
                "resultSchema",
                "schema://JoinResult@1",
            ),
            (
                "subgraph_configuration",
                "graphRef",
                "graph-template://review@1",
            ),
            (
                "subgraph_configuration",
                "parameterValue.000",
                "artifact://scope.json",
            ),
            (
                "materializer_configuration",
                "target",
                "document://architecture/auth",
            ),
            (
                "deploy_configuration",
                "adapterRef",
                "deploy://docker-compose@1",
            ),
            (
                "deploy_configuration",
                "precondition.000",
                "artifact://approval.json",
            ),
            (
                "rollback_configuration",
                "adapterRef",
                "deploy://docker-compose@1",
            ),
            ("node_completion", "contractRef", "contract://completion@1"),
            (
                "policy_control",
                "policyRef",
                "policy://workspace/security@1",
            ),
            (
                "graph_completion",
                "requirement.000",
                "document://summary/report",
            ),
        ];
        for (control_type, key, valid) in cases {
            let mut identifiers = BTreeMap::new();
            identifiers.insert(
                SafeKey::parse(key).unwrap(),
                super::encode_persisted_reference(valid).unwrap(),
            );
            let valid_control = PersistedControl::new(
                SafeValue::parse(control_type).unwrap(),
                identifiers.clone(),
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::new(),
            )
            .unwrap();
            assert!(super::validate_persisted_control_references(&valid_control).is_ok());

            let wrong = if valid == "environment://staging" {
                super::encode_persisted_reference("schema://WrongFamily@1").unwrap()
            } else {
                super::encode_persisted_reference("environment://staging").unwrap()
            };
            identifiers.insert(SafeKey::parse(key).unwrap(), wrong);
            let wrong_control = PersistedControl::new(
                SafeValue::parse(control_type).unwrap(),
                identifiers,
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::new(),
            )
            .unwrap();
            assert!(
                super::validate_persisted_control_references(&wrong_control).is_err(),
                "{control_type}.{key}"
            );
        }
    }

    #[test]
    fn decoder_rejects_noncanonical_or_unregistered_payloads() {
        for encoded in [
            "refv1:",
            "refv1:A",
            "refv1:AA=",
            "refv1:AA+",
            "refv1:_w",
            "refv1:Q2FyZ28ubG9jaw",
        ] {
            assert!(decode_persisted_reference(encoded).is_err(), "{encoded}");
        }
        for raw in [
            "Cargo.lock",
            "artifact://planning/result",
            "artifact://..",
            "artifact://nested:scheme",
            "SCHEMA://PlanningRequest@1",
            "schema://PlanningRequest@0",
            "schema://PlanningRequest@01",
            "schema://Planning..Request@1",
            "schema://file://contract",
            "environment://production/apiKey",
            "environment://production-api-key",
            "environment://production_apikey",
            "environment://production-private-key",
            "environment://production_accesskey",
            "environment://production-database-url",
            "environment://production_connectionstring",
            "schema://runtime-api-key@1",
            "contract://private-key-rotation@1",
            "rules://database-url@1",
            "context-policy://connection-string@1",
        ] {
            assert!(encode_persisted_reference(raw).is_err(), "{raw}");
        }
        assert!(
            decode_persisted_reference("refv1:ZW52aXJvbm1lbnQ6Ly9wcm9kdWN0aW9uLWFwaS1rZXk")
                .is_err()
        );

        let canonical = encode_persisted_reference("environment://qa").unwrap();
        let mut noncanonical = canonical.as_str().as_bytes().to_vec();
        let last = noncanonical.last_mut().unwrap();
        *last = match *last {
            b'A' => b'B',
            b'Q' => b'R',
            other => panic!("unexpected canonical final sextet {other}"),
        };
        let noncanonical = String::from_utf8(noncanonical).unwrap();
        assert!(decode_persisted_reference(&noncanonical).is_err());

        assert_eq!(
            parse_persisted_nominal_identifier("inline-or-artifact")
                .unwrap()
                .as_str(),
            "inline-or-artifact"
        );
        for invalid in ["Cargo.lock", "../selector", "UPPER", "refv1:encoded"] {
            assert!(parse_persisted_nominal_identifier(invalid).is_err());
        }
    }

    #[test]
    fn all_durable_content_detectors_have_a_global_linear_operation_bound() {
        let families = [
            "eyJ".repeat(16_384),
            "ghp-aaaa!".repeat(16_384),
            "authorization-----".repeat(16_384),
            "environment://prod-ap-ke-near-miss/".repeat(4_096),
            "-----BEGIN---PRIVATE---KE-near-miss".repeat(4_096),
            "secret:/near-miss/".repeat(16_384),
        ];
        for text in families {
            let operations = durable_content_scan_operation_count(&text);
            assert!(
                operations <= text.len() * 512,
                "{operations} operations for {} bytes",
                text.len()
            );
        }
    }

    #[test]
    fn structural_jws_headers_are_detected_independent_of_encoding_prefix_or_json_layout() {
        fn compact(header: &str) -> String {
            let mut encoded = String::new();
            super::encode_base64url(header.as_bytes(), &mut encoded);
            format!("{encoded}.cGF5bG9hZA.c2lnbmF0dXJl")
        }

        for header in [
            r#"{"alg":"HS256","typ":"JWT"}"#,
            "  { \"typ\" : \"JWT\", \"alg\" : \"RS256\" }  ",
            r#"{"kid":"fixture-key","extra":true,"alg":"ES256"}"#,
            "\n{\"z\":0,\"alg\":\"EdDSA\",\"a\":1}\t",
        ] {
            let candidate = compact(header);
            assert!(super::validate_durable_content(&serde_json::json!(candidate), &[]).is_err());
        }

        for header in [
            r#"{"typ":"JWT"}"#,
            r#"{"alg":7}"#,
            r#"["alg","HS256"]"#,
            r#"{"algorithm":"HS256"}"#,
        ] {
            let candidate = compact(header);
            assert!(super::validate_durable_content(&serde_json::json!(candidate), &[]).is_ok());
        }

        let separators = format!("{}{}", "._-".repeat(32_768), compact(r#"{"alg":"HS256"}"#));
        let operations = durable_content_scan_operation_count(&separators);
        assert!(operations <= separators.len() * 512);
    }

    #[test]
    fn shared_durable_content_gate_preserves_every_secret_family() {
        let values = [
            format!("GhP_{}", "A".repeat(36)),
            format!("sK_pRoJ_{}", "A".repeat(32)),
            format!("AKIA{}", "A".repeat(16)),
            format!("aSiA{}", "A".repeat(16)),
            format!("glpat-{}", "A".repeat(20)),
            format!(
                "xoxb-{}-{}-{}",
                "1".repeat(12),
                "2".repeat(12),
                "A".repeat(40)
            ),
            format!(
                "{}.{}.{}",
                "eyJhbGciOiJIUzI1NiJ9",
                "eyJzdWIiOiJkdXJhYmxlIn0",
                "A".repeat(24)
            ),
            format!("AUTHORIZATION_BEARER_{}", "A".repeat(24)),
            format!("-----BEGIN PRIVATE KEY-----{}", "A".repeat(8)),
            format!("-----BeGiN_RsA-PrIvAtE_KeY-----{}", "A".repeat(8)),
            "secret://production/runtime-token".to_owned(),
            "EnViRoNmEnT://production/AWS_SECRET_ACCESS_KEY".to_owned(),
            format!("Authorization: Basic {}", "A".repeat(24)),
        ];
        for value in values {
            assert!(
                super::validate_durable_content(&serde_json::json!(value), &[]).is_err(),
                "secret family was not rejected: {value}"
            );
        }
    }
}
