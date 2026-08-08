use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::Actor;

/// The schema-valid graph document exchanged at GraphHelm boundaries.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionGraph {
    pub api_version: String,
    pub kind: String,
    pub metadata: GraphMetadata,
    pub spec: GraphSpec,
}

/// Graph identity and version metadata. Unknown fields survive a round trip.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphMetadata {
    pub id: String,
    pub name: String,
    pub execution_id: String,
    pub version: u64,
    #[serde(default)]
    pub based_on: Option<String>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    #[serde(flatten)]
    pub properties: BTreeMap<String, serde_json::Value>,
}

/// The executable graph topology and its deterministic controls.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphSpec {
    pub entrypoints: Vec<String>,
    pub nodes: BTreeMap<String, GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub budgets: GraphBudgets,
    #[serde(default)]
    pub policies: Vec<serde_json::Value>,
    pub completion: serde_json::Value,
}

/// Static resource limits enforced by the semantic linter.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphBudgets {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_nodes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_depth: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_mutations: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retries_per_node: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_wall_clock_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_api_cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_parallel_model_calls: Option<u64>,
}

/// A node's stable fields plus forward-compatible schema-permitted properties.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GraphNode {
    #[serde(rename = "type")]
    pub node_type: NodeType,
    pub name: String,
    pub objective: String,
    pub optionality: Optionality,
    #[serde(flatten)]
    pub properties: BTreeMap<String, serde_json::Value>,
}

/// Node kinds accepted by the checked-in v1 wire schema.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    Agent,
    Tool,
    Classifier,
    Planner,
    Gate,
    Evaluator,
    Fork,
    Join,
    HumanDecision,
    Timer,
    Trigger,
    Subgraph,
    Materializer,
    Deploy,
    Rollback,
    ArtifactTransform,
}

impl NodeType {
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Tool => "tool",
            Self::Classifier => "classifier",
            Self::Planner => "planner",
            Self::Gate => "gate",
            Self::Evaluator => "evaluator",
            Self::Fork => "fork",
            Self::Join => "join",
            Self::HumanDecision => "human_decision",
            Self::Timer => "timer",
            Self::Trigger => "trigger",
            Self::Subgraph => "subgraph",
            Self::Materializer => "materializer",
            Self::Deploy => "deploy",
            Self::Rollback => "rollback",
            Self::ArtifactTransform => "artifact_transform",
        }
    }
}

/// Whether a node is mandatory or may be bypassed through policy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Optionality {
    Required,
    Recommended,
    Optional,
}

/// A directed, typed graph edge.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdge {
    pub id: String,
    pub from: String,
    pub to: String,
    #[serde(rename = "type")]
    pub edge_type: EdgeType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_schema: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_false: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_unknown: Option<UnknownConditionBehavior>,
    #[serde(default, rename = "map")]
    pub bindings: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeType {
    Control,
    Data,
    Evidence,
    Event,
    Failure,
    Compensation,
    HumanApproval,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnknownConditionBehavior {
    Pause,
    Fail,
    Skip,
    Route,
}

/// Lowercase, algorithm-prefixed digest of a graph's semantic projection.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SemanticHash(String);

impl SemanticHash {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SemanticHash {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Immutable predecessor reference carried between graph versions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphVersionRef {
    pub number: u64,
    pub content_hash: SemanticHash,
}

/// Complete persistence record for an immutable graph version.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphVersionRecord {
    pub graph: ExecutionGraph,
    pub predecessor: Option<GraphVersionRef>,
    pub semantic: serde_json::Value,
    pub content_hash: SemanticHash,
    pub created_by: Actor,
    pub created_at: DateTime<Utc>,
}
