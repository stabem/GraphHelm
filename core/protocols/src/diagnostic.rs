use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    EvidenceId, PersistenceError, RawSha256, deserialize_optional_non_null, is_opaque_id,
    is_safe_key,
};

/// Stable diagnostic severity used by CLI and policy gates.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}

/// A deterministic, source-addressed domain diagnostic.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub code: String,
    pub severity: Severity,
    pub message: String,
    pub path: String,
    pub source: String,
}

impl Diagnostic {
    #[must_use]
    pub fn error(
        code: impl Into<String>,
        message: impl Into<String>,
        path: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            severity: Severity::Error,
            message: message.into(),
            path: path.into(),
            source: source.into(),
        }
    }

    #[must_use]
    pub fn warning(
        code: impl Into<String>,
        message: impl Into<String>,
        path: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            severity: Severity::Warning,
            message: message.into(),
            path: path.into(),
            source: source.into(),
        }
    }
}

/// Closed durable component identity for a safe diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticComponent {
    Schema,
    Graph,
    Policy,
    Governor,
    Simulation,
    Events,
    Evidence,
    Artifact,
    Retention,
    Projection,
    Integrity,
    Repository,
}

/// Bounded JSON Pointer to a registered GraphHelm contract field.
///
/// This is a domain location, never a caller-supplied source or filesystem path.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct DiagnosticDomainPath(String);

impl DiagnosticDomainPath {
    pub fn parse(value: impl Into<String>) -> Result<Self, PersistenceError> {
        let value = value.into();
        if valid_domain_json_pointer(&value) {
            Ok(Self(value))
        } else {
            Err(PersistenceError::new("diagnostic domain path"))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DiagnosticDomainPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl<'de> Deserialize<'de> for DiagnosticDomainPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

/// Persistence-safe diagnostic without free-form prose or filesystem source paths.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedDiagnostic {
    code: String,
    severity: Severity,
    path: DiagnosticDomainPath,
    component: DiagnosticComponent,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_content_sha256: Option<RawSha256>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail_evidence_id: Option<EvidenceId>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawPersistedDiagnostic {
    code: String,
    severity: Severity,
    path: DiagnosticDomainPath,
    component: DiagnosticComponent,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    source_content_sha256: Option<RawSha256>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    detail_evidence_id: Option<EvidenceId>,
}

impl PersistedDiagnostic {
    pub fn new(
        code: String,
        severity: Severity,
        path: DiagnosticDomainPath,
        component: DiagnosticComponent,
        source_content_sha256: Option<RawSha256>,
        detail_evidence_id: Option<EvidenceId>,
    ) -> Result<Self, PersistenceError> {
        if !valid_diagnostic_code(&code) {
            return Err(PersistenceError::new("diagnostic"));
        }
        Ok(Self {
            code,
            severity,
            path,
            component,
            source_content_sha256,
            detail_evidence_id,
        })
    }

    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    #[must_use]
    pub const fn severity(&self) -> &Severity {
        &self.severity
    }

    #[must_use]
    pub const fn path(&self) -> &DiagnosticDomainPath {
        &self.path
    }

    #[must_use]
    pub const fn component(&self) -> DiagnosticComponent {
        self.component
    }

    #[must_use]
    pub const fn source_content_sha256(&self) -> Option<&RawSha256> {
        self.source_content_sha256.as_ref()
    }

    #[must_use]
    pub const fn detail_evidence_id(&self) -> Option<&EvidenceId> {
        self.detail_evidence_id.as_ref()
    }
}

impl TryFrom<RawPersistedDiagnostic> for PersistedDiagnostic {
    type Error = PersistenceError;

    fn try_from(raw: RawPersistedDiagnostic) -> Result<Self, Self::Error> {
        Self::new(
            raw.code,
            raw.severity,
            raw.path,
            raw.component,
            raw.source_content_sha256,
            raw.detail_evidence_id,
        )
    }
}

impl<'de> Deserialize<'de> for PersistedDiagnostic {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        RawPersistedDiagnostic::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

fn valid_diagnostic_code(value: &str) -> bool {
    let bytes = value.as_bytes();
    (2..=64).contains(&bytes.len())
        && bytes[0].is_ascii_uppercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn valid_domain_json_pointer(value: &str) -> bool {
    const MAX_DOMAIN_PATH_DEPTH: usize = 16;

    if value.len() > 512 {
        return false;
    }
    if value.is_empty() {
        return true;
    }
    if !value.starts_with('/') {
        return false;
    }

    let Some(tokens) = value[1..]
        .split('/')
        .map(decode_json_pointer_token)
        .collect::<Option<Vec<_>>>()
    else {
        return false;
    };
    if tokens.is_empty() || tokens.len() > MAX_DOMAIN_PATH_DEPTH {
        return false;
    }

    valid_authoring_graph_path(&tokens)
        || valid_persisted_graph_path(&tokens)
        || valid_event_envelope_path(&tokens)
        || valid_evidence_record_path(&tokens)
        || valid_artifact_reference_path(&tokens)
        || valid_repository_scope_path(&tokens)
        || valid_policy_waiver_path(&tokens)
}

fn valid_authoring_graph_path(tokens: &[String]) -> bool {
    match tokens {
        [root] => matches!(root.as_str(), "apiVersion" | "kind" | "metadata" | "spec"),
        [root, rest @ ..] if root == "metadata" => match rest {
            [field] => matches!(
                field.as_str(),
                "id" | "name" | "executionId" | "version" | "basedOn" | "labels"
            ),
            [field, key] if field == "labels" => is_safe_key(key),
            _ => false,
        },
        [root, rest @ ..] if root == "spec" => valid_authoring_spec(rest),
        _ => false,
    }
}

fn valid_authoring_spec(tokens: &[String]) -> bool {
    match tokens {
        [field] => matches!(
            field.as_str(),
            "entrypoints" | "nodes" | "edges" | "budgets" | "policies" | "completion"
        ),
        [field, index] if field == "entrypoints" => valid_index(index, 1024),
        [field, node_id] if field == "nodes" => is_opaque_id(node_id),
        [field, node_id, rest @ ..] if field == "nodes" && is_opaque_id(node_id) => {
            valid_authoring_node(rest)
        }
        [field, index] if field == "edges" => valid_index(index, 4096),
        [field, index, rest @ ..] if field == "edges" && valid_index(index, 4096) => {
            valid_authoring_edge(rest)
        }
        [field, rest @ ..] if field == "budgets" => valid_budgets(rest),
        [field, index] if field == "policies" => valid_index(index, 64),
        _ => false,
    }
}

fn valid_authoring_node(tokens: &[String]) -> bool {
    matches!(
        tokens,
        [field] if matches!(
            field.as_str(),
            "type"
                | "name"
                | "objective"
                | "description"
                | "optionality"
                | "agent"
                | "model"
                | "input"
                | "output"
                | "context"
                | "permissions"
                | "isolation"
                | "completion"
                | "retry"
                | "timeoutSeconds"
                | "userEditable"
                | "userOverrideAllowed"
                | "resources"
                | "memory"
                | "ui"
        )
    )
}

fn valid_authoring_edge(tokens: &[String]) -> bool {
    match tokens {
        [field] => matches!(
            field.as_str(),
            "id" | "from"
                | "to"
                | "type"
                | "payloadSchema"
                | "condition"
                | "onFalse"
                | "onUnknown"
                | "map"
                | "priority"
        ),
        [field, key] if field == "map" => is_safe_key(key),
        _ => false,
    }
}

fn valid_persisted_graph_path(tokens: &[String]) -> bool {
    match tokens {
        [root] => matches!(
            root.as_str(),
            "number"
                | "predecessor"
                | "topology"
                | "topologyHash"
                | "semanticHash"
                | "contentSlots"
                | "createdBy"
                | "createdAt"
        ),
        [root, rest @ ..] if root == "predecessor" => valid_version_ref(rest),
        [root, rest @ ..] if root == "topology" => valid_topology(rest),
        [root, index] if root == "contentSlots" => valid_index(index, 8192),
        [root, index, rest @ ..] if root == "contentSlots" && valid_index(index, 8192) => {
            valid_content_slot(rest)
        }
        [root, rest @ ..] if root == "createdBy" => valid_actor(rest),
        _ => false,
    }
}

fn valid_version_ref(tokens: &[String]) -> bool {
    matches!(tokens, [field] if matches!(field.as_str(), "number" | "semanticHash"))
}

fn valid_actor(tokens: &[String]) -> bool {
    matches!(tokens, [field] if matches!(field.as_str(), "type" | "id"))
}

fn valid_topology(tokens: &[String]) -> bool {
    match tokens {
        [field] => matches!(
            field.as_str(),
            "apiVersion"
                | "kind"
                | "graphId"
                | "executionId"
                | "labels"
                | "entrypoints"
                | "nodes"
                | "edges"
                | "budgets"
                | "policies"
                | "completion"
        ),
        [field, key] if field == "labels" => is_safe_key(key),
        [field, index] if field == "entrypoints" => valid_index(index, 1024),
        [field, node_id] if field == "nodes" => is_opaque_id(node_id),
        [field, node_id, rest @ ..] if field == "nodes" && is_opaque_id(node_id) => {
            valid_persisted_node(rest)
        }
        [field, index] if field == "edges" => valid_index(index, 4096),
        [field, index, rest @ ..] if field == "edges" && valid_index(index, 4096) => {
            valid_persisted_edge(rest)
        }
        [field, rest @ ..] if field == "budgets" => valid_budgets(rest),
        [field, index] if field == "policies" => valid_index(index, 64),
        [field, index, rest @ ..] if field == "policies" && valid_index(index, 64) => {
            valid_control(rest)
        }
        [field, rest @ ..] if field == "completion" => valid_control(rest),
        _ => false,
    }
}

fn valid_budgets(tokens: &[String]) -> bool {
    matches!(
        tokens,
        [field] if matches!(
            field.as_str(),
            "maxNodes"
                | "maxDepth"
                | "maxMutations"
                | "maxRetriesPerNode"
                | "maxWallClockSeconds"
                | "maxApiCostUsd"
                | "maxParallelModelCalls"
        )
    )
}

fn valid_persisted_node(tokens: &[String]) -> bool {
    match tokens {
        [field] => matches!(
            field.as_str(),
            "nodeType" | "optionality" | "controls" | "contentSlotIds"
        ),
        [field, index] if field == "controls" => valid_index(index, 64),
        [field, index, rest @ ..] if field == "controls" && valid_index(index, 64) => {
            valid_control(rest)
        }
        [field, index] if field == "contentSlotIds" => valid_index(index, 64),
        _ => false,
    }
}

fn valid_persisted_edge(tokens: &[String]) -> bool {
    match tokens {
        [field] => matches!(
            field.as_str(),
            "id" | "from" | "to" | "edgeType" | "priority" | "bindings" | "condition"
        ),
        [field, key] if field == "bindings" => is_safe_key(key),
        [field, rest @ ..] if field == "condition" => valid_control(rest),
        _ => false,
    }
}

fn valid_control(tokens: &[String]) -> bool {
    match tokens {
        [field] => matches!(
            field.as_str(),
            "controlType" | "identifiers" | "digests" | "integers" | "flags"
        ),
        [field, key]
            if matches!(
                field.as_str(),
                "identifiers" | "digests" | "integers" | "flags"
            ) =>
        {
            is_safe_key(key)
        }
        _ => false,
    }
}

fn valid_content_slot(tokens: &[String]) -> bool {
    matches!(
        tokens,
        [field] if matches!(
            field.as_str(),
            "slotId"
                | "ownerKind"
                | "ownerId"
                | "fieldKind"
                | "ordinal"
                | "evidenceId"
                | "contentSha256"
                | "sensitivity"
                | "requiredForExecution"
        )
    )
}

fn valid_event_envelope_path(tokens: &[String]) -> bool {
    match tokens {
        [root] => matches!(
            root.as_str(),
            "schemaVersion"
                | "eventId"
                | "scope"
                | "streamId"
                | "sequence"
                | "occurredAt"
                | "idempotencyKey"
                | "actor"
                | "sensitivity"
                | "kind"
                | "evidenceRefs"
                | "artifactRefs"
                | "previousHash"
                | "eventHash"
        ),
        [root, rest @ ..] if root == "scope" => valid_repository_scope_fields(rest),
        [root, rest @ ..] if root == "actor" => valid_actor(rest),
        [root, index] if root == "evidenceRefs" => valid_index(index, 8192),
        [root, index, rest @ ..] if root == "evidenceRefs" && valid_index(index, 8192) => {
            valid_evidence_reference_fields(rest)
        }
        [root, index] if root == "artifactRefs" => valid_index(index, 64),
        [root, index, rest @ ..] if root == "artifactRefs" && valid_index(index, 64) => {
            valid_artifact_reference_fields(rest)
        }
        [root, rest @ ..] if root == "kind" => valid_event_kind(rest),
        _ => false,
    }
}

fn valid_event_kind(tokens: &[String]) -> bool {
    match tokens {
        [field] => matches!(field.as_str(), "type" | "data"),
        [field, rest @ ..] if field == "data" => valid_event_data(rest),
        _ => false,
    }
}

fn valid_event_data(tokens: &[String]) -> bool {
    const SCALAR_FIELDS: &[&str] = &[
        "sourceSha256",
        "sourceKind",
        "draftId",
        "expectedVersion",
        "expectedHash",
        "operationCount",
        "reasonCode",
        "detailEvidenceId",
        "graphVersion",
        "graphHash",
        "requirementId",
        "status",
        "overrideable",
        "simulationId",
        "nodeId",
        "previousState",
        "nextState",
        "streamId",
        "sequence",
        "eventHash",
        "repositoryFormat",
        "operationId",
        "evidenceId",
        "keyHandleId",
        "retentionPolicyId",
        "retentionPolicyVersion",
        "authority",
        "priorState",
        "state",
        "requestedAt",
        "ciphertextSha256",
        "providerReceiptId",
        "providerEpoch",
        "completedAt",
        "deletedAt",
        "holdId",
        "changedAt",
    ];

    match tokens {
        [field] => {
            SCALAR_FIELDS.contains(&field.as_str())
                || matches!(
                    field.as_str(),
                    "diagnostics" | "version" | "evidenceIds" | "waiver" | "authenticationTag"
                )
        }
        [field, index] if field == "diagnostics" => valid_index(index, 64),
        [field, index, rest @ ..] if field == "diagnostics" && valid_index(index, 64) => {
            valid_persisted_diagnostic(rest)
        }
        [field, rest @ ..] if field == "version" => valid_persisted_graph_path(rest),
        [field, index] if field == "evidenceIds" => valid_index(index, 64),
        [field, rest @ ..] if field == "waiver" => valid_policy_waiver_fields(rest),
        [field, rest @ ..] if field == "authenticationTag" => valid_authentication_tag(rest),
        _ => false,
    }
}

fn valid_persisted_diagnostic(tokens: &[String]) -> bool {
    matches!(
        tokens,
        [field] if matches!(
            field.as_str(),
            "code"
                | "severity"
                | "path"
                | "component"
                | "sourceContentSha256"
                | "detailEvidenceId"
        )
    )
}

fn valid_authentication_tag(tokens: &[String]) -> bool {
    matches!(tokens, [field] if matches!(field.as_str(), "keyId" | "algorithm" | "tagSha256"))
}

fn valid_evidence_record_path(tokens: &[String]) -> bool {
    match tokens {
        [root] => matches!(
            root.as_str(),
            "evidenceId"
                | "scope"
                | "mediaType"
                | "sensitivity"
                | "cipherAlgorithm"
                | "cipherVersion"
                | "contentSha256"
                | "ciphertextSha256"
                | "nonce"
                | "wrappedKey"
                | "byteLength"
                | "createdAt"
                | "retentionClass"
                | "availability"
        ),
        [root, rest @ ..] if root == "scope" => valid_repository_scope_fields(rest),
        [root, rest @ ..] if root == "wrappedKey" => valid_wrapped_key(rest),
        _ => false,
    }
}

fn valid_wrapped_key(tokens: &[String]) -> bool {
    matches!(tokens, [field] if matches!(field.as_str(), "keyId" | "algorithm" | "wrappedDekSha256"))
}

fn valid_artifact_reference_path(tokens: &[String]) -> bool {
    matches!(tokens, [root] if valid_artifact_reference_field(root))
}

fn valid_artifact_reference_fields(tokens: &[String]) -> bool {
    matches!(tokens, [field] if valid_artifact_reference_field(field))
}

fn valid_artifact_reference_field(field: &str) -> bool {
    matches!(
        field,
        "artifactId"
            | "locator"
            | "contentSha256"
            | "mediaType"
            | "byteLength"
            | "sensitivity"
            | "metadataVersion"
    )
}

fn valid_evidence_reference_fields(tokens: &[String]) -> bool {
    matches!(
        tokens,
        [field] if matches!(
            field.as_str(),
            "evidenceId" | "contentSha256" | "ciphertextSha256"
        )
    )
}

fn valid_repository_scope_path(tokens: &[String]) -> bool {
    matches!(tokens, [root] if valid_repository_scope_field(root))
}

fn valid_repository_scope_fields(tokens: &[String]) -> bool {
    matches!(tokens, [field] if valid_repository_scope_field(field))
}

fn valid_repository_scope_field(field: &str) -> bool {
    matches!(field, "workspaceId" | "projectId" | "executionId")
}

fn valid_policy_waiver_path(tokens: &[String]) -> bool {
    match tokens {
        [root] => valid_policy_waiver_field(root),
        [root, index] if root == "acknowledgedRisks" => valid_index(index, 64),
        _ => false,
    }
}

fn valid_policy_waiver_fields(tokens: &[String]) -> bool {
    match tokens {
        [field] => valid_policy_waiver_field(field),
        [field, index] if field == "acknowledgedRisks" => valid_index(index, 64),
        _ => false,
    }
}

fn valid_policy_waiver_field(field: &str) -> bool {
    matches!(
        field,
        "id" | "requirement"
            | "executionId"
            | "graphVersion"
            | "actor"
            | "reason"
            | "acknowledgedRisks"
            | "scope"
            | "createdAt"
            | "expiresAt"
    )
}

fn valid_index(value: &str, upper_bound: usize) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && (value == "0" || !value.starts_with('0'))
        && value
            .parse::<usize>()
            .is_ok_and(|index| index < upper_bound)
}

fn decode_json_pointer_token(value: &str) -> Option<String> {
    let mut decoded = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(character) = chars.next() {
        if character == '~' {
            match chars.next() {
                Some('0') => decoded.push('~'),
                Some('1') => decoded.push('/'),
                _ => return None,
            }
        } else {
            if character == '/' {
                return None;
            }
            decoded.push(character);
        }
    }
    if decoded.is_empty() {
        None
    } else {
        Some(decoded)
    }
}
