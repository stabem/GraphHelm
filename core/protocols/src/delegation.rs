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
    pub kind: SubagentKind,
    pub tier: DelegationTier,
    pub effort: DelegationEffort,
    /// True when tier or effort differs from the kind's starting rule because of red checks.
    pub escalated: bool,
    /// Failed mechanical checks already recorded for this node in this execution at dispatch.
    pub red_checks: u32,
}
