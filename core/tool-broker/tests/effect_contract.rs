//! The tier rule's public contract: the two supported effects map to their tiers, everything
//! else is a typed refusal, and the tier wire names never drift from the authoring vocabulary
//! that `core/graph` closed.

use graphhelm_tool_broker::effect::{IsolationTier, ToolEffect, required_tier};

#[test]
fn read_only_is_tier_0_and_reversible_write_is_tier_1() {
    assert_eq!(
        required_tier(ToolEffect::ReadOnly),
        Ok(IsolationTier::Tier0)
    );
    assert_eq!(
        required_tier(ToolEffect::ReversibleWrite),
        Ok(IsolationTier::Tier1)
    );
}

#[test]
fn every_effect_beyond_this_slice_is_a_typed_refusal_not_a_guess() {
    use graphhelm_tool_broker::effect::EffectUnsupported;
    for effect in [
        ToolEffect::IrreversibleWrite,
        ToolEffect::ExternalSideEffect,
        ToolEffect::ProductionEffect,
        ToolEffect::SecretUse,
        ToolEffect::NetworkEgress,
    ] {
        assert_eq!(required_tier(effect), Err(EffectUnsupported { effect }));
    }
}

#[test]
fn tier_wire_names_agree_with_the_authoring_vocabulary() {
    // The authoring contract closed the tier vocabulary in core/graph; this crate must never
    // drift from it. Alignment by test, not by dependency: graphhelm-graph is dev-only.
    for tier in [
        IsolationTier::Tier0,
        IsolationTier::Tier1,
        IsolationTier::Tier2,
        IsolationTier::Tier3,
    ] {
        let name = serde_json::to_value(tier).unwrap();
        let name = name.as_str().unwrap();
        assert!(
            graphhelm_graph::is_valid_isolation_tier(name),
            "{name} drifted"
        );
    }
}
