//! The live half of the paired driver, proven without a network (#225, #506).
//!
//! `HttpTransport` is a trait, so the WHOLE live path -- prompt built, provider wire shape
//! spoken, usage parsed, `CostField` written -- runs here against a fake transport returning
//! canned Anthropic responses. The only residue a real run adds is the network and the real key,
//! and both sit behind the same two labels the dry run uses (`modelRoute`, `producer`).
//!
//! Usage honesty is the whole point of the mapping cells: a provider that reported no usage
//! yields `unavailable`, never zero and never an estimate -- #222's rule, arriving at the arm.

use std::sync::Arc;

use graphhelm_development_benchmark::{BenchmarkRefusal, live_arm_cost, usage_to_cost};
use graphhelm_events::SecretBytes;
use graphhelm_gateway::manifest::RouteManifest;
use graphhelm_model_gateway::byok::ByokAdapter;
use graphhelm_model_gateway::transport::{
    HttpTransport, TransportError, TransportRequest, TransportResponse,
};
use graphhelm_runtime::context_accounting::CostField;

/// A transport that returns one canned response and records nothing.
struct CannedTransport {
    status: u16,
    body: &'static str,
}

impl HttpTransport for CannedTransport {
    fn execute(&self, _request: &TransportRequest) -> Result<TransportResponse, TransportError> {
        Ok(TransportResponse {
            status: self.status,
            body: self.body.as_bytes().to_vec(),
        })
    }
}

fn route_manifest() -> RouteManifest {
    let json = serde_json::json!({
        "manifestVersion": 2,
        "routes": [{
            "id": "anthropic_prod",
            "provider": "anthropic",
            "transport": "direct_api",
            "authentication": "api_key",
            "billingMode": "per_token",
            "baseUrl": "http://127.0.0.1:9",
            "model": "test-model",
            "credentialRef": "secret_test",
            "profiles": ["balanced_reasoning"],
            "enabled": true
        }]
    });
    RouteManifest::from_json(&json.to_string())
        .unwrap_or_else(|error| panic!("fixture manifest must be valid: {error}"))
}

fn key() -> SecretBytes {
    SecretBytes::new(b"sk-ant-test-key-0123456789abcdef".to_vec())
}

/// POSITIVE CONTROL: a reply carrying usage lands as a MEASURED cost naming the route as its
/// producer -- the label that separates a live number from a fake one.
#[test]
fn a_reply_with_usage_becomes_a_measured_cost_named_by_the_route() {
    let manifest = route_manifest();
    let route = &manifest.routes()[0];
    let transport = Arc::new(CannedTransport {
        status: 200,
        body: r#"{"content":[{"type":"text","text":"the answer"}],
                  "usage":{"input_tokens":1234,"output_tokens":56}}"#,
    });
    let adapter = ByokAdapter::new(route, transport);

    let cost = live_arm_cost(&adapter, &key(), "the prompt", 512, "anthropic_prod")
        .expect("a 200 with usage is a completed call");

    assert_eq!(cost.observed(), Some(1234));
    assert!(cost.is_measured());
    assert_eq!(cost.producer(), Some("anthropic_prod"));
}

/// §11.2 at the arm: a reply WITHOUT usage is unavailable -- never zero, never estimated.
#[test]
fn a_reply_without_usage_is_unavailable_not_zero() {
    let manifest = route_manifest();
    let route = &manifest.routes()[0];
    let transport = Arc::new(CannedTransport {
        status: 200,
        body: r#"{"content":[{"type":"text","text":"the answer"}]}"#,
    });
    let adapter = ByokAdapter::new(route, transport);

    let cost = live_arm_cost(&adapter, &key(), "the prompt", 512, "anthropic_prod")
        .expect("a 200 without usage is still a completed call");

    assert_eq!(cost.observed(), None, "absence stays absence");
    assert!(!cost.is_measured());
}

/// A provider error is a typed refusal naming the failure -- the run does not limp on without
/// the case, because a partial run is a different corpus.
#[test]
fn a_provider_error_refuses_with_the_gateway_taxonomy_word() {
    let manifest = route_manifest();
    let route = &manifest.routes()[0];
    let transport = Arc::new(CannedTransport {
        status: 429,
        body: r#"{"error":{"type":"rate_limit_error"}}"#,
    });
    let adapter = ByokAdapter::new(route, transport);

    let refusal = live_arm_cost(&adapter, &key(), "the prompt", 512, "anthropic_prod")
        .expect_err("a 429 was treated as a completed call");

    match refusal {
        BenchmarkRefusal::Unreadable { detail } => {
            assert!(
                detail.contains("anthropic_prod"),
                "the refusal names the route, got: {detail}"
            );
        }
        other => panic!("the gateway taxonomy did not decide this: {other:?}"),
    }
}

/// The pure mapping, pinned on both branches so the arm cells above cannot drift from it.
#[test]
fn usage_to_cost_maps_presence_and_absence_faithfully() {
    let measured = usage_to_cost(Some(99), "route-x");
    assert_eq!(measured, CostField::measured(99, "route-x"));

    let absent = usage_to_cost(None, "route-x");
    assert_eq!(absent.observed(), None);
    assert!(!absent.is_measured());
}

/// K's hold on #531 (Codex P1 confirmed): the settings string in the receipt must be DERIVED
/// from what the call actually transmits, per provider -- never an asserted literal. A
/// held-equal axis both arms copy from the same constant is a check that cannot fail.
#[test]
fn the_settings_label_is_derived_from_what_the_provider_is_sent() {
    let anthropic = graphhelm_development_benchmark::transmitted_settings("anthropic", 512);
    assert!(
        anthropic.contains("max_tokens=512"),
        "anthropic binds the cap as a required top-level field, so the label carries it: {anthropic}"
    );
    assert!(
        anthropic.contains("temperature=provider-default"),
        "nothing transmits a temperature, and the label must SAY so instead of asserting 0: {anthropic}"
    );

    let openai = graphhelm_development_benchmark::transmitted_settings("openai", 512);
    assert!(
        openai.contains("max_tokens=not-transmitted"),
        "the openai adapter deliberately does not forward the cap (byok.rs), and recording it as \
         bound would claim a control that was not executed: {openai}"
    );

    let fake = graphhelm_development_benchmark::transmitted_settings("fake", 512);
    assert!(fake.contains("input=ceil(bytes/4)"), "got: {fake}");
}
