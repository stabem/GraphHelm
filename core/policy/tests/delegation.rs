use graphhelm_policy::delegation::{DelegationPolicy, Effort, SubagentKind, Tier, choose};

const KINDS: [SubagentKind; 4] = [
    SubagentKind::Explorer,
    SubagentKind::Implementer,
    SubagentKind::Reviewer,
    SubagentKind::Verifier,
];

#[test]
fn routed_policy_explores_small_and_implements_standard() {
    let policy = DelegationPolicy::routed();
    let explore = choose(&policy, SubagentKind::Explorer, 0);
    assert_eq!((explore.tier, explore.escalated), (Tier::Small, false));
    let implement = choose(&policy, SubagentKind::Implementer, 0);
    assert_eq!(
        (implement.tier, implement.escalated),
        (Tier::Standard, false)
    );
}

#[test]
fn each_red_check_raises_one_tier_and_never_past_large_or_downward() {
    let policy = DelegationPolicy::routed();
    for kind in KINDS {
        let mut previous = choose(&policy, kind, 0).tier;
        for red in 1..6 {
            let choice = choose(&policy, kind, red);
            assert!(
                choice.tier >= previous,
                "{kind:?} went cheaper after a red check"
            );
            assert!(choice.tier <= Tier::Large);
            assert_eq!(choice.effort, Effort::High);
            previous = choice.tier;
        }
        assert_eq!(previous, Tier::Large);
    }
    assert_eq!(
        choose(&policy, SubagentKind::Explorer, 1).tier,
        Tier::Standard
    );
}

#[test]
fn escalation_off_keeps_the_declared_rule() {
    let mut policy = DelegationPolicy::routed();
    policy.escalate_on_red_check = false;
    let choice = choose(&policy, SubagentKind::Implementer, 3);
    assert_eq!(
        (choice.tier, choice.effort),
        (Tier::Standard, Effort::Medium)
    );
    assert!(!choice.escalated);
}

#[test]
fn policy_is_data_and_refuses_unknown_fields() {
    let policy = DelegationPolicy::routed();
    let text = serde_json::to_string(&policy).unwrap();
    let back: DelegationPolicy = serde_json::from_str(&text).unwrap();
    assert_eq!(back, policy);
    let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
    value["model"] = serde_json::json!("x");
    assert!(serde_json::from_value::<DelegationPolicy>(value).is_err());
    let mut bad_tier: serde_json::Value = serde_json::from_str(&text).unwrap();
    bad_tier["explorer"]["tier"] = serde_json::json!("huge");
    assert!(serde_json::from_value::<DelegationPolicy>(bad_tier).is_err());
}
