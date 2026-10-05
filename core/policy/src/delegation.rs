//! #137: deterministic delegation choice for one node dispatch.
//!
//! The loop asks "which kind of subagent, at which tier and effort, takes this node?". The answer
//! is a pure function of a declared [`DelegationPolicy`] and what the loop already knows about the
//! node (its role and how many mechanical checks it has failed). No model is consulted: a policy
//! is data, so a hillclimb changes the policy file, not this code, and the same inputs always
//! produce the same [`DelegationChoice`].
//!
//! Escalation is one-way and capped: each red check moves the tier up one step, never above
//! `large`, and a cheaper tier is never chosen after a failure. Keel itself is NOT delegated here;
//! whichever tier produced a diff, the same mechanical checks judge it.

use serde::{Deserialize, Serialize};

/// What the node asks a subagent to do. Owned by `graphhelm-protocols` (ADR-040) so the node
/// schema, the `delegation_chosen` event and this policy name one closed set.
pub use graphhelm_protocols::SubagentKind;

/// Model size class. Ordered: `Small < Standard < Large`. Owned by `graphhelm-protocols`.
pub use graphhelm_protocols::DelegationTier as Tier;

/// Reasoning budget. Ordered: `Low < Medium < High`. Owned by `graphhelm-protocols`.
pub use graphhelm_protocols::DelegationEffort as Effort;

/// Raises `tier` by `steps`, capped at `Large`.
fn raised(tier: Tier, steps: u32) -> Tier {
    let index: u32 = match tier {
        Tier::Small => 0,
        Tier::Standard => 1,
        Tier::Large => 2,
    };
    match index.saturating_add(steps).min(2) {
        0 => Tier::Small,
        1 => Tier::Standard,
        _ => Tier::Large,
    }
}

/// The starting point for one subagent kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DelegationRule {
    pub tier: Tier,
    pub effort: Effort,
}

/// A declared delegation policy: one rule per kind, plus whether red checks escalate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DelegationPolicy {
    pub explorer: DelegationRule,
    pub implementer: DelegationRule,
    pub reviewer: DelegationRule,
    pub verifier: DelegationRule,
    /// When true, each failed mechanical check raises the tier one step (capped at `large`) and
    /// sets effort to `high`.
    pub escalate_on_red_check: bool,
}

impl DelegationPolicy {
    /// The first policy #137 names: explore small, implement standard, escalate after red.
    #[must_use]
    pub const fn routed() -> Self {
        Self {
            explorer: DelegationRule {
                tier: Tier::Small,
                effort: Effort::Low,
            },
            implementer: DelegationRule {
                tier: Tier::Standard,
                effort: Effort::Medium,
            },
            reviewer: DelegationRule {
                tier: Tier::Standard,
                effort: Effort::High,
            },
            verifier: DelegationRule {
                tier: Tier::Small,
                effort: Effort::Medium,
            },
            escalate_on_red_check: true,
        }
    }

    const fn rule(&self, kind: SubagentKind) -> DelegationRule {
        match kind {
            SubagentKind::Explorer => self.explorer,
            SubagentKind::Implementer => self.implementer,
            SubagentKind::Reviewer => self.reviewer,
            SubagentKind::Verifier => self.verifier,
        }
    }
}

/// The decision for one dispatch, ready to be recorded on the node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DelegationChoice {
    pub kind: SubagentKind,
    pub tier: Tier,
    pub effort: Effort,
    /// True when the tier or effort differs from the kind's starting rule because of red checks.
    pub escalated: bool,
}

/// Choose who takes a node. `red_checks` is how many mechanical checks this node has failed so far.
#[must_use]
pub fn choose(policy: &DelegationPolicy, kind: SubagentKind, red_checks: u32) -> DelegationChoice {
    let rule = policy.rule(kind);
    if !policy.escalate_on_red_check || red_checks == 0 {
        return DelegationChoice {
            kind,
            tier: rule.tier,
            effort: rule.effort,
            escalated: false,
        };
    }
    let tier = raised(rule.tier, red_checks);
    let effort = Effort::High;
    DelegationChoice {
        kind,
        tier,
        effort,
        escalated: tier != rule.tier || effort != rule.effort,
    }
}
