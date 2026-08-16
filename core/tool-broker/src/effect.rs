//! The declared tool-effect classes and the effect-to-tier rule.

/// The declared effect classes of AGENTS_SKILLS_PLUGINS.md §11.3, complete so that adding a
/// tool later forces a deliberate classification rather than a default.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolEffect {
    ReadOnly,
    ReversibleWrite,
    IrreversibleWrite,
    ExternalSideEffect,
    ProductionEffect,
    SecretUse,
    NetworkEgress,
}

/// Serialized names match `core/graph::is_valid_isolation_tier`'s closed vocabulary exactly.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, serde::Serialize, serde::Deserialize,
)]
pub enum IsolationTier {
    #[serde(rename = "tier_0")]
    Tier0,
    #[serde(rename = "tier_1")]
    Tier1,
    #[serde(rename = "tier_2")]
    Tier2,
    #[serde(rename = "tier_3")]
    Tier3,
}

/// The typed refusal for every effect class the 05c broker does not execute.
#[derive(Clone, Copy, PartialEq, Eq, Debug, thiserror::Error)]
#[error("the {effect:?} effect is not supported by the 05c broker")]
pub struct EffectUnsupported {
    pub effect: ToolEffect,
}

/// One exhaustive match, no wildcard arm: a new effect variant breaks compilation here on
/// purpose, so its tier is decided, never defaulted. SecretUse is refused structurally — the
/// register's hard constraint is that the workspace never sees a credential, so no tool in this
/// broker may even declare wanting one.
pub const fn required_tier(effect: ToolEffect) -> Result<IsolationTier, EffectUnsupported> {
    match effect {
        ToolEffect::ReadOnly => Ok(IsolationTier::Tier0),
        ToolEffect::ReversibleWrite => Ok(IsolationTier::Tier1),
        ToolEffect::IrreversibleWrite
        | ToolEffect::ExternalSideEffect
        | ToolEffect::ProductionEffect
        | ToolEffect::SecretUse
        | ToolEffect::NetworkEgress => Err(EffectUnsupported { effect }),
    }
}
