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
    /// Everything else the node declares, kept as authored.
    ///
    /// TRAP, NAMED HERE BECAUSE THIS IS WHERE SOMEONE WOULD SPRING IT (#160): promoting one of
    /// these keys to a typed field looks like a pure improvement and is not. The governor's
    /// content externalizer iterates THIS MAP (`collect_node_properties`, and for the completion
    /// block `collect_completion_content` in core/governor/src/externalize.rs) — a key consumed
    /// into a named field disappears from here, and the externalizer simply stops seeing it.
    /// For `completion` that would silently stop collecting node completion contracts, which
    /// three checked-in example graphs rely on.
    ///
    /// It is not unguarded: `node_completion_contract_content_is_externalized`-style coverage in
    /// core/governor/tests/safe_publication.rs inserts a `completion` block, externalizes, and
    /// asserts the collected `requiresArtifact.000` reference — so the regression fails a test
    /// rather than shipping. This comment exists so the failure is UNDERSTOOD when it happens
    /// instead of looking like an unrelated break in a crate you were not editing.
    ///
    /// M11's customs budgets are therefore typed on READ (a helper deserializes
    /// `completion.customs`), never by moving the key out of this map.
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

/// The deadline this node declares, in seconds, or `None` when it declares none.
///
/// One definition on purpose. The governor reads it to persist the node, and the CLI reads it
/// to record the declared form at start; two readings of one rule is how the first divergence
/// becomes invisible. `as_u64` is the whole type check — a negative, fractional, string or
/// null declaration yields `None` here rather than a number, and absence stays absence and
/// never becomes zero, which would make every node look permanently overdue.
#[must_use]
pub fn declared_timeout_seconds(node: &GraphNode) -> Option<u64> {
    node.properties
        .get("timeoutSeconds")
        .and_then(serde_json::Value::as_u64)
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

/// M11 #160: the customs declaration a node may carry, read out of `completion.customs`.
///
/// STRICT ON PURPOSE, and strict ONLY HERE: `deny_unknown_fields` means a typo in a budget name
/// is refused rather than silently ignored, because a budget nobody notices is missing reads
/// exactly like a stage with infinite patience — the 0/4 failure this milestone exists to end.
/// The surrounding `completion` block stays the permissive placeholder it has always been:
/// `requires`/`forbids` have three checked-in graphs and a governor consumer, and tightening them
/// is a different decision by a different lane.
///
/// WHY `proof_kinds` AND NOT `requires_evidence` (#182, decided before the name could publish):
/// `completion.requires[].evidence` ALREADY EXISTS in this same block and means something else —
/// a predicate over the node's OUTPUT, read by `build_completion_control`. A sibling called
/// `requires_evidence` would have put two near-homographs one nesting level apart, meaning
/// different things, in the same block; the next author reads one and uses the other, and a diff
/// is exactly where a near-identical name does its damage. `proof_kinds` shares no token, no root
/// and no reading with it, so the collision stops existing rather than being documented.
///
/// The rejected alternative was `submission_kinds`, and the reason it lost is worth keeping: it
/// names the ACT, while `proof_kinds` names the THING. A test report is not a species of
/// submission that happens to be evidence — it is a species of evidence that happens to arrive by
/// submission. A name describing the transport is a name of PLACE in disguise, and place-names
/// collide again the moment someone quotes them alone.
///
/// The relatives may still unify one day. Nested like this that is a local refactor; as sibling
/// top-level keys it would have been a schema migration. The separation is the current shape, not
/// doctrine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeCustoms {
    /// The kinds of work-evidence a completion claim must present. Fewer than declared is refused
    /// (`EvidenceBudgetUnmet`); extra kinds are accepted and marked unverified — logged, never
    /// counted as stronger proof.
    ///
    /// "PROOF" HERE MEANS THE ARTEFACTS THAT SHOW THE WORK HAPPENED — a test report, a diff, a
    /// log. It is NOT cryptographic proof, and nothing in this field is signature-verified.
    /// Signature verification, where it exists, lives on the clearance side
    /// (`ClearanceVerifier::Countersign`), and a reader who takes "proof" as cryptographic will
    /// go looking for verification in the wrong half of the pipeline.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub proof_kinds: Vec<String>,
    pub budgets: CustomsBudgets,
}

/// How long each customs stage may park, in seconds, declared by the node.
///
/// Durations on the wire, instants in the projection: the fold adds one to the `occurred_at` of
/// the event that ENTERED the stage. That is the M09 `matures_in_seconds` discipline — one clock,
/// the envelope's — generalized so that no stage is exempt from the sweep.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CustomsBudgets {
    /// REQUIRED: how long an UN-CLAIMED wait may sit. Required because the un-claimed wait is
    /// precisely the state that parked forever with nothing watching it; a customs declaration
    /// that could omit this would reproduce the hole with more ceremony.
    pub wait_within_seconds: u64,
    /// REQUIRED: how long a claim may await clearance. The quarantine must not be able to park.
    pub clearance_within_seconds: u64,
    /// OPTIONAL: absent means dead-letter occupancy raises no time exception — the DLQ is the
    /// exception state itself, and a second timer on it is escalation policy rather than a
    /// default anyone chose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dlq_within_seconds: Option<u64>,
}

impl GraphNode {
    /// Reads this node's customs declaration, if it has one.
    ///
    /// Typed ON READ rather than as a struct field, and that is not a style choice: the governor's
    /// content externalizer iterates `properties`, so promoting `completion` to a named field
    /// would consume the key out of that map and silently stop node completion contracts from
    /// being collected. See the trap named at `properties`.
    ///
    /// # Errors
    /// Returns the deserialization error when a `customs` block is present but malformed — a
    /// missing or misspelled budget is a refusal, never a default.
    pub fn customs(&self) -> Result<Option<NodeCustoms>, serde_json::Error> {
        let Some(customs) = self
            .properties
            .get("completion")
            .and_then(|completion| completion.get("customs"))
        else {
            return Ok(None);
        };
        serde_json::from_value(customs.clone()).map(Some)
    }
}

/// Node kinds accepted by the checked-in v1 wire schema.
///
/// `PartialOrd`/`Ord` exist so a caller can key a `BTreeMap` by node type (M08 Task 1: the
/// per-type silence budget the surface injects). The alternative — a `Vec` of pairs — would
/// admit two entries for the same type and make "which one wins" an undeclared rule, which
/// is the defect family this milestone spent a day burying. A map makes the duplicate
/// unrepresentable, so the compiler guarantees what a comment would have had to promise.
///
/// The derived order follows DECLARATION ORDER and exists ONLY for keying. Nothing may
/// depend on it for iteration or display: reordering the variants below must stay a
/// cosmetic edit, not a silent behaviour change.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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
