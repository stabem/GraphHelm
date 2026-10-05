//! ADR-040 (#290): the wire vocabulary of a node's delegation declaration and of the
//! `delegation_chosen` event.
//!
//! These types live here, not in `graphhelm-policy`, because the policy crate depends on this
//! one: the node schema, the event payload and the policy's `choose` must name the SAME closed
//! sets, and only the lower crate can own them without a dependency cycle.
//! `graphhelm_policy::delegation` re-exports them under its own names (`SubagentKind`, `Tier`,
//! `Effort`), so the policy and the wire cannot drift.

use serde::{Deserialize, Serialize};

/// What a node asks a subagent to do. The closed set `node.schema.json`'s
/// `delegation.kind` and `event-envelope.schema.json`'s `delegationChosen.kind` carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentKind {
    Explorer,
    Implementer,
    Reviewer,
    Verifier,
}

impl SubagentKind {
    /// The wire spelling, as the schema enumerates it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Explorer => "explorer",
            Self::Implementer => "implementer",
            Self::Reviewer => "reviewer",
            Self::Verifier => "verifier",
        }
    }
}

/// Model size class. Ordered: `Small < Standard < Large`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelegationTier {
    Small,
    Standard,
    Large,
}

/// Reasoning budget. Ordered: `Low < Medium < High`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelegationEffort {
    Low,
    Medium,
    High,
}

/// Which delegation policy produced a recorded choice. Closed: today the Runtime has exactly one
/// built-in policy (`routed`), and a choice that cannot say which policy made it cannot be
/// re-derived from the journal once a second policy exists.
///
/// `Default` is `Routed` ONLY so a `delegation_chosen` written before this field existed (between
/// #295 and its review follow-up) still reads: it can only have come from `routed`, the one policy
/// there has ever been. Writers always emit the field.
#[derive(
    Default, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum DelegationPolicyId {
    #[default]
    Routed,
}

/// A node's `delegation` block: which kind of subagent takes it. Closed (`deny_unknown_fields`),
/// mirroring the schema's `additionalProperties: false`, because a typo here must be a refusal and
/// never a silent "no delegation".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeDelegation {
    pub kind: SubagentKind,
}

/// ADR-040: the deterministic delegation choice the Runtime made when it dispatched one node.
///
/// Recording, not enforcement: `tier` says what the policy chose; the model route actually used
/// is still what `agent_presence_declared` declares.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DelegationChosen {
    pub node_id: crate::OpaqueId,
    /// The policy that produced this choice. Always written; optional on read (absent reads as
    /// `routed`) because `delegation_chosen` already landed without it, and making it required
    /// would be a breaking change to a landed schema (`baseline_origin`).
    #[serde(default)]
    pub policy: DelegationPolicyId,
    pub kind: SubagentKind,
    pub tier: DelegationTier,
    pub effort: DelegationEffort,
    /// True when tier or effort differs from the kind's starting rule because of red checks.
    pub escalated: bool,
    /// Failed mechanical checks already recorded for this node in this execution at dispatch.
    pub red_checks: u32,
}

/// ADR-041: why a delegated node got the subagent `subagent_reused` names. Closed: every value is
/// a mechanical outcome of the reuse key, the authorship rule or the measured bound, never a
/// judgement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentBasis {
    /// An already-briefed subagent took the node: every key member matched and the measured
    /// tokens were under the receiving budget. `fromNodeId` names its previous node.
    Reused,
    /// Fresh: no earlier subagent in this execution matched the whole reuse key (same graph
    /// version, an allowlisted kind pair, its previous node finished).
    NoEligibleSubagent,
    /// Fresh: the node is a `reviewer` or `verifier` and an earlier subagent authored work in
    /// this execution, so reusing it could make an author review its own result.
    AuthorUnderReview,
    /// Fresh: a candidate matched the key, but its token use or the receiving budget could not be
    /// measured (a counter is `unavailable`), and an estimate is never substituted (D-043).
    BoundUnavailable,
    /// Fresh: a candidate matched the key, but its measured tokens reached the receiving budget.
    BoundExceeded,
}

/// ADR-041: which subagent instance took one delegated node, appended right after the node's
/// `delegation_chosen`. A fresh subagent has no `fromNodeId`; a reused one names the node it took
/// before. The execution and repository scope members of the reuse key are the envelope's own.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubagentReused {
    pub node_id: crate::OpaqueId,
    /// Opaque, Runtime-minted at the node's dispatch when the subagent is fresh; carried over
    /// unchanged when it is reused.
    pub subagent_id: crate::OpaqueId,
    /// The receiving node's delegation kind.
    pub kind: SubagentKind,
    /// The graph version the execution runs (`execution_started.graphVersion`).
    pub graph_version: u64,
    pub basis: SubagentBasis,
    /// The node the reused subagent took before. Present exactly when `basis` is `reused`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::persistence::deserialize_optional_non_null"
    )]
    pub from_node_id: Option<crate::OpaqueId>,
    /// The kind of `fromNodeId`, so the kind pair is readable from this event alone.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::persistence::deserialize_optional_non_null"
    )]
    pub from_kind: Option<SubagentKind>,
    /// Measured provider-reported input plus output tokens of the candidate subagent in this
    /// execution. Absent when not measured.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::persistence::deserialize_optional_non_null"
    )]
    pub tokens_used: Option<u64>,
    /// The receiving capsule's `tokenBudget.allocated` the bound was checked against. Absent
    /// when not known.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::persistence::deserialize_optional_non_null"
    )]
    pub tokens_allocated: Option<u64>,
}
