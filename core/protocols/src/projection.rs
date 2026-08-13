use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    EdgeType, EvidenceId, ExecutionId, NodeType, OpaqueId, Optionality, PersistedActor,
    PersistedTimestamp, PersistenceError, RawSha256, SafeKey, SafeValue, Sensitivity, WireHash,
    deserialize_optional_non_null, deserialize_required_nullable, valid_positive_safe_integer,
    valid_safe_integer,
};

const MAX_MAP_ITEMS: usize = 128;

/// Immutable predecessor identity for a persisted graph version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedGraphVersionRef {
    number: u64,
    semantic_hash: WireHash,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawPersistedGraphVersionRef {
    number: u64,
    semantic_hash: WireHash,
}

impl PersistedGraphVersionRef {
    pub fn new(number: u64, semantic_hash: WireHash) -> Result<Self, PersistenceError> {
        if !valid_positive_safe_integer(number) {
            return Err(PersistenceError::new("predecessor number"));
        }
        Ok(Self {
            number,
            semantic_hash,
        })
    }

    #[must_use]
    pub const fn number(&self) -> u64 {
        self.number
    }

    #[must_use]
    pub const fn semantic_hash(&self) -> &WireHash {
        &self.semantic_hash
    }
}

impl<'de> Deserialize<'de> for PersistedGraphVersionRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawPersistedGraphVersionRef::deserialize(deserializer)?;
        Self::new(raw.number, raw.semantic_hash).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedControl {
    control_type: SafeValue,
    identifiers: BTreeMap<SafeKey, SafeValue>,
    digests: BTreeMap<SafeKey, RawSha256>,
    integers: BTreeMap<SafeKey, i64>,
    flags: BTreeMap<SafeKey, bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawPersistedControl {
    control_type: SafeValue,
    identifiers: BTreeMap<SafeKey, SafeValue>,
    digests: BTreeMap<SafeKey, RawSha256>,
    integers: BTreeMap<SafeKey, i64>,
    flags: BTreeMap<SafeKey, bool>,
}

impl PersistedControl {
    pub fn new(
        control_type: SafeValue,
        identifiers: BTreeMap<SafeKey, SafeValue>,
        digests: BTreeMap<SafeKey, RawSha256>,
        integers: BTreeMap<SafeKey, i64>,
        flags: BTreeMap<SafeKey, bool>,
    ) -> Result<Self, PersistenceError> {
        if [
            identifiers.len(),
            digests.len(),
            integers.len(),
            flags.len(),
        ]
        .into_iter()
        .any(|count| count > MAX_MAP_ITEMS)
            || integers.values().any(|value| !valid_safe_integer(*value))
        {
            return Err(PersistenceError::new("control"));
        }
        Ok(Self {
            control_type,
            identifiers,
            digests,
            integers,
            flags,
        })
    }

    #[must_use]
    pub const fn control_type(&self) -> &SafeValue {
        &self.control_type
    }

    #[must_use]
    pub const fn identifiers(&self) -> &BTreeMap<SafeKey, SafeValue> {
        &self.identifiers
    }

    #[must_use]
    pub const fn digests(&self) -> &BTreeMap<SafeKey, RawSha256> {
        &self.digests
    }

    #[must_use]
    pub const fn integers(&self) -> &BTreeMap<SafeKey, i64> {
        &self.integers
    }

    #[must_use]
    pub const fn flags(&self) -> &BTreeMap<SafeKey, bool> {
        &self.flags
    }
}

impl TryFrom<RawPersistedControl> for PersistedControl {
    type Error = PersistenceError;

    fn try_from(raw: RawPersistedControl) -> Result<Self, Self::Error> {
        Self::new(
            raw.control_type,
            raw.identifiers,
            raw.digests,
            raw.integers,
            raw.flags,
        )
    }
}

impl<'de> Deserialize<'de> for PersistedControl {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        RawPersistedControl::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

/// Schema-bounded resource budgets retained in the safe topology.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedBudgets {
    #[serde(skip_serializing_if = "Option::is_none")]
    max_nodes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_depth: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_mutations: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_retries_per_node: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_wall_clock_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_api_cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_parallel_model_calls: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawPersistedBudgets {
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    max_nodes: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    max_depth: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    max_mutations: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    max_retries_per_node: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    max_wall_clock_seconds: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    max_api_cost_usd: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    max_parallel_model_calls: Option<u64>,
}

impl PersistedBudgets {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        max_nodes: Option<u64>,
        max_depth: Option<u64>,
        max_mutations: Option<u64>,
        max_retries_per_node: Option<u64>,
        max_wall_clock_seconds: Option<u64>,
        max_api_cost_usd: Option<f64>,
        max_parallel_model_calls: Option<u64>,
    ) -> Result<Self, PersistenceError> {
        let bounded_nonzero =
            |value: Option<u64>, max| value.is_none_or(|value| (1..=max).contains(&value));
        if !bounded_nonzero(max_nodes, 100_000)
            || !bounded_nonzero(max_depth, 100_000)
            || max_mutations.is_some_and(|value| value > 100_000)
            || max_retries_per_node.is_some_and(|value| value > 100_000)
            || !bounded_nonzero(max_wall_clock_seconds, 315_576_000)
            || max_api_cost_usd.is_some_and(|value| !(0.0..=1_000_000_000.0).contains(&value))
            || !bounded_nonzero(max_parallel_model_calls, 100_000)
        {
            return Err(PersistenceError::new("budgets"));
        }
        Ok(Self {
            max_nodes,
            max_depth,
            max_mutations,
            max_retries_per_node,
            max_wall_clock_seconds,
            max_api_cost_usd,
            max_parallel_model_calls,
        })
    }

    #[must_use]
    pub const fn max_nodes(&self) -> Option<u64> {
        self.max_nodes
    }

    #[must_use]
    pub const fn max_depth(&self) -> Option<u64> {
        self.max_depth
    }

    #[must_use]
    pub const fn max_mutations(&self) -> Option<u64> {
        self.max_mutations
    }

    #[must_use]
    pub const fn max_retries_per_node(&self) -> Option<u64> {
        self.max_retries_per_node
    }

    #[must_use]
    pub const fn max_wall_clock_seconds(&self) -> Option<u64> {
        self.max_wall_clock_seconds
    }

    #[must_use]
    pub const fn max_api_cost_usd(&self) -> Option<f64> {
        self.max_api_cost_usd
    }

    #[must_use]
    pub const fn max_parallel_model_calls(&self) -> Option<u64> {
        self.max_parallel_model_calls
    }
}

impl TryFrom<RawPersistedBudgets> for PersistedBudgets {
    type Error = PersistenceError;

    fn try_from(raw: RawPersistedBudgets) -> Result<Self, Self::Error> {
        Self::new(
            raw.max_nodes,
            raw.max_depth,
            raw.max_mutations,
            raw.max_retries_per_node,
            raw.max_wall_clock_seconds,
            raw.max_api_cost_usd,
            raw.max_parallel_model_calls,
        )
    }
}

impl<'de> Deserialize<'de> for PersistedBudgets {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        RawPersistedBudgets::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedNode {
    node_type: NodeType,
    optionality: Optionality,
    controls: Vec<PersistedControl>,
    content_slot_ids: Vec<OpaqueId>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawPersistedNode {
    node_type: NodeType,
    optionality: Optionality,
    controls: Vec<PersistedControl>,
    content_slot_ids: Vec<OpaqueId>,
}

impl PersistedNode {
    pub fn new(
        node_type: NodeType,
        optionality: Optionality,
        controls: Vec<PersistedControl>,
        content_slot_ids: Vec<OpaqueId>,
    ) -> Result<Self, PersistenceError> {
        if controls.len() > 64 || content_slot_ids.len() > 64 || !all_unique(&content_slot_ids) {
            return Err(PersistenceError::new("node"));
        }
        Ok(Self {
            node_type,
            optionality,
            controls,
            content_slot_ids,
        })
    }

    #[must_use]
    pub const fn node_type(&self) -> &NodeType {
        &self.node_type
    }

    #[must_use]
    pub const fn optionality(&self) -> &Optionality {
        &self.optionality
    }

    #[must_use]
    pub fn controls(&self) -> &[PersistedControl] {
        &self.controls
    }

    #[must_use]
    pub fn content_slot_ids(&self) -> &[OpaqueId] {
        &self.content_slot_ids
    }
}

impl TryFrom<RawPersistedNode> for PersistedNode {
    type Error = PersistenceError;

    fn try_from(raw: RawPersistedNode) -> Result<Self, Self::Error> {
        Self::new(
            raw.node_type,
            raw.optionality,
            raw.controls,
            raw.content_slot_ids,
        )
    }
}

impl<'de> Deserialize<'de> for PersistedNode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        RawPersistedNode::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedEdge {
    id: OpaqueId,
    from: OpaqueId,
    to: OpaqueId,
    edge_type: EdgeType,
    priority: Option<i64>,
    bindings: BTreeMap<SafeKey, SafeValue>,
    condition: Option<PersistedControl>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawPersistedEdge {
    id: OpaqueId,
    from: OpaqueId,
    to: OpaqueId,
    edge_type: EdgeType,
    #[serde(deserialize_with = "deserialize_required_nullable")]
    priority: Option<i64>,
    bindings: BTreeMap<SafeKey, SafeValue>,
    #[serde(deserialize_with = "deserialize_required_nullable")]
    condition: Option<PersistedControl>,
}

impl PersistedEdge {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: OpaqueId,
        from: OpaqueId,
        to: OpaqueId,
        edge_type: EdgeType,
        priority: Option<i64>,
        bindings: BTreeMap<SafeKey, SafeValue>,
        condition: Option<PersistedControl>,
    ) -> Result<Self, PersistenceError> {
        if priority.is_some_and(|value| !valid_safe_integer(value))
            || bindings.len() > MAX_MAP_ITEMS
        {
            return Err(PersistenceError::new("edge"));
        }
        Ok(Self {
            id,
            from,
            to,
            edge_type,
            priority,
            bindings,
            condition,
        })
    }

    #[must_use]
    pub const fn id(&self) -> &OpaqueId {
        &self.id
    }

    #[must_use]
    pub const fn from(&self) -> &OpaqueId {
        &self.from
    }

    #[must_use]
    pub const fn to(&self) -> &OpaqueId {
        &self.to
    }

    #[must_use]
    pub const fn edge_type(&self) -> &EdgeType {
        &self.edge_type
    }

    #[must_use]
    pub const fn priority(&self) -> Option<i64> {
        self.priority
    }

    #[must_use]
    pub const fn bindings(&self) -> &BTreeMap<SafeKey, SafeValue> {
        &self.bindings
    }

    #[must_use]
    pub const fn condition(&self) -> Option<&PersistedControl> {
        self.condition.as_ref()
    }
}

impl TryFrom<RawPersistedEdge> for PersistedEdge {
    type Error = PersistenceError;

    fn try_from(raw: RawPersistedEdge) -> Result<Self, Self::Error> {
        Self::new(
            raw.id,
            raw.from,
            raw.to,
            raw.edge_type,
            raw.priority,
            raw.bindings,
            raw.condition,
        )
    }
}

impl<'de> Deserialize<'de> for PersistedEdge {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        RawPersistedEdge::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedTopology {
    api_version: &'static str,
    kind: &'static str,
    graph_id: OpaqueId,
    execution_id: ExecutionId,
    labels: BTreeMap<SafeKey, SafeValue>,
    entrypoints: Vec<OpaqueId>,
    nodes: BTreeMap<OpaqueId, PersistedNode>,
    edges: Vec<PersistedEdge>,
    budgets: PersistedBudgets,
    policies: Vec<PersistedControl>,
    completion: PersistedControl,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawPersistedTopology {
    api_version: String,
    kind: String,
    graph_id: OpaqueId,
    execution_id: ExecutionId,
    labels: BTreeMap<SafeKey, SafeValue>,
    entrypoints: Vec<OpaqueId>,
    nodes: BTreeMap<OpaqueId, PersistedNode>,
    edges: Vec<PersistedEdge>,
    budgets: PersistedBudgets,
    policies: Vec<PersistedControl>,
    completion: PersistedControl,
}

impl PersistedTopology {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        graph_id: OpaqueId,
        execution_id: ExecutionId,
        labels: BTreeMap<SafeKey, SafeValue>,
        entrypoints: Vec<OpaqueId>,
        nodes: BTreeMap<OpaqueId, PersistedNode>,
        edges: Vec<PersistedEdge>,
        budgets: PersistedBudgets,
        policies: Vec<PersistedControl>,
        completion: PersistedControl,
    ) -> Result<Self, PersistenceError> {
        if labels.len() > MAX_MAP_ITEMS
            || entrypoints.is_empty()
            || entrypoints.len() > 1024
            || !all_unique(&entrypoints)
            || nodes.is_empty()
            || nodes.len() > 1024
            || edges.len() > 4096
            || policies.len() > 64
        {
            return Err(PersistenceError::new("topology"));
        }
        Ok(Self {
            api_version: "p50.dev/graph/v1",
            kind: "ExecutionGraph",
            graph_id,
            execution_id,
            labels,
            entrypoints,
            nodes,
            edges,
            budgets,
            policies,
            completion,
        })
    }

    #[must_use]
    pub const fn api_version(&self) -> &'static str {
        self.api_version
    }

    #[must_use]
    pub const fn kind(&self) -> &'static str {
        self.kind
    }

    #[must_use]
    pub const fn graph_id(&self) -> &OpaqueId {
        &self.graph_id
    }

    #[must_use]
    pub const fn execution_id(&self) -> &ExecutionId {
        &self.execution_id
    }

    #[must_use]
    pub const fn labels(&self) -> &BTreeMap<SafeKey, SafeValue> {
        &self.labels
    }

    #[must_use]
    pub fn entrypoints(&self) -> &[OpaqueId] {
        &self.entrypoints
    }

    #[must_use]
    pub const fn nodes(&self) -> &BTreeMap<OpaqueId, PersistedNode> {
        &self.nodes
    }

    #[must_use]
    pub fn edges(&self) -> &[PersistedEdge] {
        &self.edges
    }

    #[must_use]
    pub const fn budgets(&self) -> &PersistedBudgets {
        &self.budgets
    }

    #[must_use]
    pub fn policies(&self) -> &[PersistedControl] {
        &self.policies
    }

    #[must_use]
    pub const fn completion(&self) -> &PersistedControl {
        &self.completion
    }
}

impl TryFrom<RawPersistedTopology> for PersistedTopology {
    type Error = PersistenceError;

    fn try_from(raw: RawPersistedTopology) -> Result<Self, Self::Error> {
        if raw.api_version != "p50.dev/graph/v1" || raw.kind != "ExecutionGraph" {
            return Err(PersistenceError::new("topology discriminator"));
        }
        Self::new(
            raw.graph_id,
            raw.execution_id,
            raw.labels,
            raw.entrypoints,
            raw.nodes,
            raw.edges,
            raw.budgets,
            raw.policies,
            raw.completion,
        )
    }
}

impl<'de> Deserialize<'de> for PersistedTopology {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        RawPersistedTopology::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentOwnerKind {
    Graph,
    Node,
    Agent,
    Edge,
    Policy,
    Diagnostic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentFieldKind {
    DisplayName,
    Description,
    Objective,
    Purpose,
    Instructions,
    CompletionContract,
    PolicyText,
    DiagnosticDetail,
    ContextPath,
    PermissionPath,
    IsolationPath,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContentSlot {
    slot_id: OpaqueId,
    owner_kind: ContentOwnerKind,
    owner_id: OpaqueId,
    field_kind: ContentFieldKind,
    ordinal: u32,
    evidence_id: EvidenceId,
    content_sha256: RawSha256,
    sensitivity: Sensitivity,
    required_for_execution: bool,
}

impl ContentSlot {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn new(
        slot_id: OpaqueId,
        owner_kind: ContentOwnerKind,
        owner_id: OpaqueId,
        field_kind: ContentFieldKind,
        ordinal: u32,
        evidence_id: EvidenceId,
        content_sha256: RawSha256,
        sensitivity: Sensitivity,
        required_for_execution: bool,
    ) -> Self {
        Self {
            slot_id,
            owner_kind,
            owner_id,
            field_kind,
            ordinal,
            evidence_id,
            content_sha256,
            sensitivity,
            required_for_execution,
        }
    }

    #[must_use]
    pub const fn ordinal(&self) -> u32 {
        self.ordinal
    }

    #[must_use]
    pub const fn slot_id(&self) -> &OpaqueId {
        &self.slot_id
    }

    #[must_use]
    pub const fn owner_kind(&self) -> ContentOwnerKind {
        self.owner_kind
    }

    #[must_use]
    pub const fn owner_id(&self) -> &OpaqueId {
        &self.owner_id
    }

    #[must_use]
    pub const fn field_kind(&self) -> ContentFieldKind {
        self.field_kind
    }

    #[must_use]
    pub const fn evidence_id(&self) -> &EvidenceId {
        &self.evidence_id
    }

    #[must_use]
    pub const fn content_sha256(&self) -> &RawSha256 {
        &self.content_sha256
    }

    #[must_use]
    pub const fn sensitivity(&self) -> Sensitivity {
        self.sensitivity
    }

    #[must_use]
    pub const fn required_for_execution(&self) -> bool {
        self.required_for_execution
    }

    fn position(&self) -> (ContentOwnerKind, &OpaqueId, ContentFieldKind, u32) {
        (
            self.owner_kind,
            &self.owner_id,
            self.field_kind,
            self.ordinal,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedGraphVersion {
    number: u64,
    predecessor: Option<PersistedGraphVersionRef>,
    topology: PersistedTopology,
    topology_hash: WireHash,
    semantic_hash: WireHash,
    content_slots: Vec<ContentSlot>,
    created_by: PersistedActor,
    created_at: PersistedTimestamp,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawPersistedGraphVersion {
    number: u64,
    #[serde(deserialize_with = "deserialize_required_nullable")]
    predecessor: Option<PersistedGraphVersionRef>,
    topology: PersistedTopology,
    topology_hash: WireHash,
    semantic_hash: WireHash,
    content_slots: Vec<ContentSlot>,
    created_by: PersistedActor,
    created_at: PersistedTimestamp,
}

impl PersistedGraphVersion {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        number: u64,
        predecessor: Option<PersistedGraphVersionRef>,
        topology: PersistedTopology,
        topology_hash: WireHash,
        semantic_hash: WireHash,
        content_slots: Vec<ContentSlot>,
        created_by: PersistedActor,
        created_at: PersistedTimestamp,
    ) -> Result<Self, PersistenceError> {
        if !valid_positive_safe_integer(number)
            || predecessor
                .as_ref()
                .is_some_and(|reference| reference.number.checked_add(1) != Some(number))
            || content_slots.len() > 8192
            || content_slots
                .windows(2)
                .any(|pair| pair[0].position() >= pair[1].position())
            || content_slots
                .iter()
                .map(|slot| &slot.slot_id)
                .collect::<BTreeSet<_>>()
                .len()
                != content_slots.len()
            || content_slots
                .iter()
                .map(|slot| &slot.evidence_id)
                .collect::<BTreeSet<_>>()
                .len()
                != content_slots.len()
        {
            return Err(PersistenceError::new("graph version"));
        }
        Ok(Self {
            number,
            predecessor,
            topology,
            topology_hash,
            semantic_hash,
            content_slots,
            created_by,
            created_at,
        })
    }

    #[must_use]
    pub const fn number(&self) -> u64 {
        self.number
    }

    #[must_use]
    pub const fn predecessor(&self) -> Option<&PersistedGraphVersionRef> {
        self.predecessor.as_ref()
    }

    #[must_use]
    pub const fn topology(&self) -> &PersistedTopology {
        &self.topology
    }

    #[must_use]
    pub const fn topology_hash(&self) -> &WireHash {
        &self.topology_hash
    }

    #[must_use]
    pub const fn semantic_hash(&self) -> &WireHash {
        &self.semantic_hash
    }

    #[must_use]
    pub fn content_slots(&self) -> &[ContentSlot] {
        &self.content_slots
    }

    #[must_use]
    pub const fn created_by(&self) -> &PersistedActor {
        &self.created_by
    }

    #[must_use]
    pub const fn created_at(&self) -> &PersistedTimestamp {
        &self.created_at
    }
}

impl TryFrom<RawPersistedGraphVersion> for PersistedGraphVersion {
    type Error = PersistenceError;

    fn try_from(raw: RawPersistedGraphVersion) -> Result<Self, Self::Error> {
        Self::new(
            raw.number,
            raw.predecessor,
            raw.topology,
            raw.topology_hash,
            raw.semantic_hash,
            raw.content_slots,
            raw.created_by,
            raw.created_at,
        )
    }
}

impl<'de> Deserialize<'de> for PersistedGraphVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        RawPersistedGraphVersion::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

fn all_unique<T: Ord + Clone>(items: &[T]) -> bool {
    items.iter().cloned().collect::<BTreeSet<_>>().len() == items.len()
}
