use std::collections::HashMap;

use graphhelm_gateway::eligibility::{Requirements, eligible_routes};
use graphhelm_gateway::manifest::{RouteManifest, WorkProfile};
use graphhelm_gateway::taxonomy::{GatewayError, RouteHealth, outcome_for_error};
use graphhelm_protocols::NodeOutcome;

const EVERY_ERROR: &[GatewayError] = &[
    GatewayError::AuthRequired,
    GatewayError::AuthRevoked,
    GatewayError::QuotaExhausted,
    GatewayError::RateLimited,
    GatewayError::ProviderUnavailable,
    GatewayError::ModelRemoved,
    GatewayError::ContextTooLarge,
    GatewayError::MalformedOutput,
    GatewayError::ToolDenied,
    GatewayError::RuntimeCrashed,
    GatewayError::UnsupportedCapability,
    GatewayError::PolicyDenied,
    GatewayError::Cancelled,
    GatewayError::Timeout,
];

#[test]
fn the_mapping_is_total_and_lands_only_in_failure_shaped_outcomes() {
    // §12: capacity exhaustion pauses; nothing in the taxonomy may ever map to a
    // success-shaped or owner-decision-shaped outcome.
    for error in EVERY_ERROR {
        let outcome = outcome_for_error(*error);
        assert!(
            matches!(
                outcome,
                NodeOutcome::NeedsCapacity
                    | NodeOutcome::RetryableFailure
                    | NodeOutcome::TerminalFailure
                    | NodeOutcome::Cancelled
            ),
            "{error:?} escaped the failure codomain as {outcome:?}"
        );
    }
}

#[test]
fn capacity_class_errors_park_the_node_and_only_them() {
    use GatewayError as E;
    use NodeOutcome as O;
    // §12 step 1 names "limit/throttle/auth failure" as the pause triggers; wait, no
    // automatic paid fallback.
    for e in [
        E::QuotaExhausted,
        E::RateLimited,
        E::AuthRequired,
        E::AuthRevoked,
    ] {
        assert_eq!(outcome_for_error(e), O::NeedsCapacity);
    }
    for e in [
        E::ProviderUnavailable,
        E::Timeout,
        E::RuntimeCrashed,
        E::MalformedOutput,
    ] {
        assert_eq!(outcome_for_error(e), O::RetryableFailure);
    }
    for e in [
        E::ContextTooLarge,
        E::ModelRemoved,
        E::UnsupportedCapability,
        E::PolicyDenied,
        E::ToolDenied,
    ] {
        assert_eq!(outcome_for_error(e), O::TerminalFailure);
    }
    assert_eq!(outcome_for_error(E::Cancelled), O::Cancelled);
}

#[test]
fn health_for_error_updates_the_route_state_per_class() {
    use graphhelm_gateway::taxonomy::health_for_error;
    assert_eq!(
        health_for_error(GatewayError::QuotaExhausted),
        Some(RouteHealth::WaitingReset)
    );
    assert_eq!(
        health_for_error(GatewayError::AuthRequired),
        Some(RouteHealth::AuthRequired)
    );
    assert_eq!(
        health_for_error(GatewayError::AuthRevoked),
        Some(RouteHealth::AuthRequired)
    );
    assert_eq!(
        health_for_error(GatewayError::ProviderUnavailable),
        Some(RouteHealth::Degraded)
    );
    // A per-call condition says nothing about the route.
    assert_eq!(health_for_error(GatewayError::ContextTooLarge), None);
    assert_eq!(health_for_error(GatewayError::Cancelled), None);
}

/// Five routes, each varying one axis away from the two "good" routes `good_direct` and
/// `good_subscription`: `disabled_direct` is turned off, `unhealthy_direct` is marked unhealthy in
/// the health map below, `wrong_profile_direct` serves a different work profile than requested.
/// `good_direct` and `good_subscription` differ only in billing mode, which the second half of
/// `eligibility_filters_disabled_unhealthy_wrong_profile_and_wrong_billing` exercises.
fn eligibility_manifest_json() -> serde_json::Value {
    serde_json::json!({
        "manifestVersion": 1,
        "routes": [
            {
                "id": "good_direct",
                "provider": "anthropic",
                "transport": "direct_api",
                "authentication": "api_key",
                "billingMode": "per_token",
                "baseUrl": "https://api.anthropic.com",
                "model": "claude-sonnet-5",
                "credentialRef": "secret_a",
                "profiles": ["critical_reasoning"],
                "enabled": true
            },
            {
                "id": "disabled_direct",
                "provider": "anthropic",
                "transport": "direct_api",
                "authentication": "api_key",
                "billingMode": "per_token",
                "baseUrl": "https://api.anthropic.com",
                "model": "claude-sonnet-5",
                "credentialRef": "secret_b",
                "profiles": ["critical_reasoning"],
                "enabled": false
            },
            {
                "id": "unhealthy_direct",
                "provider": "anthropic",
                "transport": "direct_api",
                "authentication": "api_key",
                "billingMode": "per_token",
                "baseUrl": "https://api.anthropic.com",
                "model": "claude-sonnet-5",
                "credentialRef": "secret_c",
                "profiles": ["critical_reasoning"],
                "enabled": true
            },
            {
                "id": "wrong_profile_direct",
                "provider": "anthropic",
                "transport": "direct_api",
                "authentication": "api_key",
                "billingMode": "per_token",
                "baseUrl": "https://api.anthropic.com",
                "model": "claude-sonnet-5",
                "credentialRef": "secret_d",
                "profiles": ["fast_classification"],
                "enabled": true
            },
            {
                "id": "good_subscription",
                "provider": "anthropic",
                "transport": "native_runtime",
                "runtime": "claude_code",
                "authentication": "account_subscription",
                "billingMode": "subscription_quota",
                "command": { "program": "claude", "args": [] },
                "profiles": ["critical_reasoning"],
                "enabled": true
            }
        ]
    })
}

#[test]
fn eligibility_filters_disabled_unhealthy_wrong_profile_and_wrong_billing() {
    let manifest = RouteManifest::from_json(&eligibility_manifest_json().to_string()).unwrap();

    // Health comes from a caller-supplied map (id -> RouteHealth) — the pure crate holds no
    // registry of its own. Available and Degraded both pass; everything else is excluded (§8.2).
    let health = HashMap::from([
        ("good_direct".to_string(), RouteHealth::Available),
        ("disabled_direct".to_string(), RouteHealth::Available),
        ("unhealthy_direct".to_string(), RouteHealth::Unavailable),
        ("wrong_profile_direct".to_string(), RouteHealth::Available),
        ("good_subscription".to_string(), RouteHealth::Degraded),
    ]);

    // subscription_only: false admits both billing modes, so only the disabled, unhealthy and
    // wrong-profile routes fall out. The survivors come back in manifest order — scoring within
    // the eligible set (§8.3) is deferred.
    let open = eligible_routes(
        &manifest,
        &health,
        &Requirements {
            profile: WorkProfile::CriticalReasoning,
            subscription_only: false,
        },
    );
    assert_eq!(
        open.iter().map(|route| route.id()).collect::<Vec<_>>(),
        vec!["good_direct", "good_subscription"]
    );

    // subscription_only: true additionally excludes the PerToken route (§19 user control): only
    // the subscription route remains eligible.
    let subscription_only = eligible_routes(
        &manifest,
        &health,
        &Requirements {
            profile: WorkProfile::CriticalReasoning,
            subscription_only: true,
        },
    );
    assert_eq!(
        subscription_only
            .iter()
            .map(|route| route.id())
            .collect::<Vec<_>>(),
        vec!["good_subscription"]
    );
}
