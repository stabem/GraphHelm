//! Fake-server contract tests for the BYOK Anthropic/OpenAI adapters (`src/byok.rs`) over the
//! production [`UreqTransport`].
//!
//! Mirrors `apps/cli/tests/api_http.rs`'s pattern: a `TcpListener` on `127.0.0.1:0`, one thread per
//! test that accepts exactly one connection, reads the request the adapter actually sent, and
//! replies with a canned HTTP/1.1 response. `apps/cli` is bin-only (no `[lib]` target) and this is
//! a different crate entirely, so the helpers below are a from-scratch, minimal copy of that same
//! approach rather than a shared import. The manifest's cleartext-loopback-only rule
//! (`core/gateway/src/manifest.rs`) is exactly why `http://127.0.0.1:<port>` is a legal `baseUrl`
//! here: `UreqTransport` speaks http for these tests, https in production.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use graphhelm_events::SecretBytes;
use graphhelm_gateway::call::ModelCall;
use graphhelm_gateway::manifest::RouteManifest;
use graphhelm_gateway::taxonomy::GatewayError;
use graphhelm_model_gateway::byok::ByokAdapter;
use graphhelm_model_gateway::transport::{TransportRequest, UreqTransport};

/// A validated single-route `native_runtime` manifest — used only by
/// [`a_native_runtime_route_is_rejected_not_panicked_on`], which needs a structurally valid
/// route of the *wrong* transport kind to hand to [`ByokAdapter`].
fn build_native_runtime_manifest() -> RouteManifest {
    let json = serde_json::json!({
        "manifestVersion": 1,
        "routes": [{
            "id": "test_native_route",
            "provider": "anthropic",
            "transport": "native_runtime",
            "runtime": "claude_code",
            "authentication": "account_subscription",
            "billingMode": "subscription_quota",
            "command": { "program": "claude", "args": [] },
            "profiles": ["balanced_reasoning"],
            "enabled": true
        }]
    });
    RouteManifest::from_json(&json.to_string())
        .unwrap_or_else(|error| panic!("fixture manifest must be valid: {error}"))
}

const SENTINEL: &str = "sk-ant-SENTINEL-0123456789abcdef";

fn sentinel_key() -> SecretBytes {
    SecretBytes::new(SENTINEL.as_bytes().to_vec())
}

fn call(prompt: &str, max_tokens: u32) -> ModelCall {
    ModelCall {
        prompt: prompt.to_owned(),
        max_tokens,
    }
}

/// A validated single-route manifest pointed at a fake server's loopback `base_url`, for the given
/// `provider`. `id`/`model`/`credentialRef`/`profiles` are fixed, arbitrary values — nothing in
/// this file cares about them beyond satisfying `direct_api`'s structural requirements
/// (`core/gateway/src/manifest.rs`).
fn build_manifest(base_url: &str, provider: &str) -> RouteManifest {
    let json = serde_json::json!({
        "manifestVersion": 1,
        "routes": [{
            "id": "test_route",
            "provider": provider,
            "transport": "direct_api",
            "authentication": "api_key",
            "billingMode": "per_token",
            "baseUrl": base_url,
            "model": "test-model",
            "credentialRef": "secret_test",
            "profiles": ["balanced_reasoning"],
            "enabled": true
        }]
    });
    RouteManifest::from_json(&json.to_string())
        .unwrap_or_else(|error| panic!("fixture manifest must be valid: {error}"))
}

/// One HTTP request captured by [`fake_server`], for assertions.
struct CapturedRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
}

fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

/// Starts a `TcpListener` on an OS-assigned loopback port, spawns a thread that accepts exactly
/// one connection, reads one HTTP/1.1 request off it in full (headers, then `Content-Length`
/// bytes of body), replies with `status`/`body`, then sends the captured request down the
/// returned channel. Returns the bound `http://127.0.0.1:<port>` base URL and that channel.
fn fake_server(status: u16, body: &'static str) -> (String, mpsc::Receiver<CapturedRequest>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();

        let captured = read_request(&mut stream);

        let response_body = body.as_bytes();
        let head = format!(
            "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response_body.len()
        );
        stream.write_all(head.as_bytes()).unwrap();
        stream.write_all(response_body).unwrap();
        stream.flush().unwrap();

        let _ = sender.send(captured);
    });

    (format!("http://{addr}"), receiver)
}

/// Like [`build_manifest`] but with an explicit `timeoutSeconds`, for
/// [`a_body_read_stall_past_the_route_timeout_is_timeout_not_provider_unavailable`] (MEDIUM 13),
/// which needs a short route deadline to observe firing mid-body-read within a test's patience.
fn build_manifest_with_timeout(
    base_url: &str,
    provider: &str,
    timeout_seconds: u64,
) -> RouteManifest {
    let json = serde_json::json!({
        "manifestVersion": 1,
        "routes": [{
            "id": "test_route",
            "provider": provider,
            "transport": "direct_api",
            "authentication": "api_key",
            "billingMode": "per_token",
            "baseUrl": base_url,
            "model": "test-model",
            "credentialRef": "secret_test",
            "profiles": ["balanced_reasoning"],
            "enabled": true,
            "timeoutSeconds": timeout_seconds
        }]
    });
    RouteManifest::from_json(&json.to_string())
        .unwrap_or_else(|error| panic!("fixture manifest must be valid: {error}"))
}

/// Accepts one connection, reads the request in full, sends response HEADERS only (promising a
/// body it never sends), then sleeps far longer than any route timeout a test configures —
/// imitating a server that accepts the request and starts replying, then stalls mid-body. Used
/// only by [`a_body_read_stall_past_the_route_timeout_is_timeout_not_provider_unavailable`]
/// (MEDIUM 13) to prove a stall specifically during the *body* read (as opposed to before any
/// response arrives at all, which the existing timeout coverage does not distinguish from) is
/// still classified as `GatewayError::Timeout`, not `ProviderUnavailable`.
fn fake_stalling_body_server(status: u16) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let _captured = read_request(&mut stream);

        let head = format!(
            "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: 4096\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(head.as_bytes()).unwrap();
        stream.flush().unwrap();
        // Never writes the body. The test's own 1s route timeout must fire long before this.
        thread::sleep(Duration::from_secs(20));
    });

    format!("http://{addr}")
}

/// Like [`fake_server`], but the reply also carries a `Location` header — used only by
/// [`a_redirect_is_never_followed_and_the_credential_never_reaches_the_redirect_target`] to prove
/// [`UreqTransport`] never follows it.
fn fake_redirect_server(status: u16, location: &str) -> (String, mpsc::Receiver<CapturedRequest>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (sender, receiver) = mpsc::channel();
    let location = location.to_owned();

    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();

        let captured = read_request(&mut stream);

        let head = format!(
            "HTTP/1.1 {status} X\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(head.as_bytes()).unwrap();
        stream.flush().unwrap();

        let _ = sender.send(captured);
    });

    (format!("http://{addr}"), receiver)
}

fn read_request(stream: &mut TcpStream) -> CapturedRequest {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let header_end = find_header_end(&buffer);
        if let Some(header_end) = header_end {
            let content_length = parse_content_length(&buffer[..header_end]);
            if buffer.len() - (header_end + 4) >= content_length {
                break;
            }
        }
        let read = stream.read(&mut chunk).unwrap();
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }

    let header_end = find_header_end(&buffer).expect("the request must have a header/body split");
    let head = String::from_utf8_lossy(&buffer[..header_end]).into_owned();
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let path = parts.next().unwrap_or_default().to_owned();
    let headers = lines
        .filter(|line| !line.is_empty())
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_owned(), value.trim().to_owned()))
        .collect();

    CapturedRequest {
        method,
        path,
        headers,
    }
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn parse_content_length(head: &[u8]) -> usize {
    String::from_utf8_lossy(head)
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------------------------
// Anthropic.
// ---------------------------------------------------------------------------------------------

#[test]
fn anthropic_success_parses_text_and_usage_and_carries_auth_headers() {
    let (base_url, receiver) = fake_server(
        200,
        r#"{"content":[{"type":"text","text":"hello"}],"usage":{"input_tokens":12,"output_tokens":5}}"#,
    );
    let manifest = build_manifest(&base_url, "anthropic");
    let route = &manifest.routes()[0];
    let adapter = ByokAdapter::new(route, Arc::new(UreqTransport::new()));

    let reply = adapter
        .call(&sentinel_key(), &call("hi", 64))
        .unwrap_or_else(|error| panic!("expected success: {error}"));

    assert_eq!(reply.text, "hello");
    assert_eq!(reply.usage.input_tokens, Some(12));
    assert_eq!(reply.usage.output_tokens, Some(5));

    let captured = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(captured.method, "POST");
    assert_eq!(captured.path, "/v1/messages");
    assert_eq!(header_value(&captured.headers, "x-api-key"), Some(SENTINEL));
    assert_eq!(
        header_value(&captured.headers, "anthropic-version"),
        Some("2023-06-01")
    );
}

#[test]
fn anthropic_rate_limit_maps_to_rate_limited() {
    let (base_url, _receiver) = fake_server(
        429,
        r#"{"type":"error","error":{"type":"rate_limit_error","message":"slow down"}}"#,
    );
    let manifest = build_manifest(&base_url, "anthropic");
    let route = &manifest.routes()[0];
    let adapter = ByokAdapter::new(route, Arc::new(UreqTransport::new()));

    let error = adapter.call(&sentinel_key(), &call("hi", 16)).unwrap_err();
    assert_eq!(error, GatewayError::RateLimited);
}

#[test]
fn anthropic_auth_failure_maps_to_auth_required() {
    let (base_url, _receiver) = fake_server(
        401,
        r#"{"type":"error","error":{"type":"authentication_error","message":"bad key"}}"#,
    );
    let manifest = build_manifest(&base_url, "anthropic");
    let route = &manifest.routes()[0];
    let adapter = ByokAdapter::new(route, Arc::new(UreqTransport::new()));

    let error = adapter.call(&sentinel_key(), &call("hi", 16)).unwrap_err();
    assert_eq!(error, GatewayError::AuthRequired);
}

#[test]
fn anthropic_overloaded_maps_to_provider_unavailable() {
    let (base_url, _receiver) = fake_server(
        529,
        r#"{"type":"error","error":{"type":"overloaded_error","message":"overloaded"}}"#,
    );
    let manifest = build_manifest(&base_url, "anthropic");
    let route = &manifest.routes()[0];
    let adapter = ByokAdapter::new(route, Arc::new(UreqTransport::new()));

    let error = adapter.call(&sentinel_key(), &call("hi", 16)).unwrap_err();
    assert_eq!(error, GatewayError::ProviderUnavailable);
}

/// MEDIUM 13: a server that accepts the connection, sends response headers, then stalls before
/// ever sending the body must classify as `GatewayError::Timeout`, not `ProviderUnavailable`.
/// Pre-fix, `UreqTransport::execute` funneled every `body_mut().read_to_vec()` failure through
/// the `Io` arm regardless of whether the underlying cause was itself a timeout — only the
/// INITIAL `.run()` call's `ureq::Error::Timeout` was recognized, so a stall specifically during
/// the body phase reported as `ProviderUnavailable` (retried as an ordinary transport failure)
/// instead of the taxonomy's dedicated `Timeout`.
#[test]
fn a_body_read_stall_past_the_route_timeout_is_timeout_not_provider_unavailable() {
    let base_url = fake_stalling_body_server(200);
    let manifest = build_manifest_with_timeout(&base_url, "anthropic", 1);
    let route = &manifest.routes()[0];
    let adapter = ByokAdapter::new(route, Arc::new(UreqTransport::new()));

    let started = std::time::Instant::now();
    let error = adapter.call(&sentinel_key(), &call("hi", 16)).unwrap_err();
    let elapsed = started.elapsed();

    assert_eq!(
        error,
        GatewayError::Timeout,
        "a body-read stall past the route timeout must be Timeout, not {error:?}"
    );
    assert!(
        elapsed < Duration::from_secs(10),
        "expected the 1s route timeout to fire promptly, took {elapsed:?}"
    );
}

#[test]
fn anthropic_garbage_200_maps_to_malformed_output() {
    let (base_url, _receiver) = fake_server(200, "not json");
    let manifest = build_manifest(&base_url, "anthropic");
    let route = &manifest.routes()[0];
    let adapter = ByokAdapter::new(route, Arc::new(UreqTransport::new()));

    let error = adapter.call(&sentinel_key(), &call("hi", 16)).unwrap_err();
    assert_eq!(error, GatewayError::MalformedOutput);
}

#[test]
fn anthropic_absent_usage_is_none_not_invented() {
    let (base_url, _receiver) = fake_server(200, r#"{"content":[{"type":"text","text":"hello"}]}"#);
    let manifest = build_manifest(&base_url, "anthropic");
    let route = &manifest.routes()[0];
    let adapter = ByokAdapter::new(route, Arc::new(UreqTransport::new()));

    let reply = adapter
        .call(&sentinel_key(), &call("hi", 16))
        .unwrap_or_else(|error| panic!("expected success: {error}"));
    assert_eq!(reply.text, "hello");
    assert_eq!(reply.usage.input_tokens, None);
    assert_eq!(reply.usage.output_tokens, None);
}

/// MEDIUM 10: a real Anthropic response can lead with a `thinking` block (extended thinking)
/// before the `text` block a reply is built from. Pre-fix, `parse_anthropic_success` blindly took
/// `content[0]` — here that would be the thinking block, whose own `text` is absent, misreporting
/// a genuine success as `MalformedOutput`. Post-fix, the FIRST block whose `type` is `"text"` is
/// what the reply is built from, regardless of what precedes it.
#[test]
fn a_thinking_block_before_the_text_block_still_parses() {
    let (base_url, _receiver) = fake_server(
        200,
        r#"{"content":[{"type":"thinking","thinking":"reasoning about the answer..."},{"type":"text","text":"hello"}],"usage":{"input_tokens":12,"output_tokens":5}}"#,
    );
    let manifest = build_manifest(&base_url, "anthropic");
    let route = &manifest.routes()[0];
    let adapter = ByokAdapter::new(route, Arc::new(UreqTransport::new()));

    let reply = adapter
        .call(&sentinel_key(), &call("hi", 16))
        .unwrap_or_else(|error| panic!("expected success (thinking block first): {error}"));
    assert_eq!(reply.text, "hello");
}

/// Beyond the plan's explicit fixture table: the "Both: 403 -> PolicyDenied" fixed rule stated
/// alongside it, exercised once here rather than duplicated for both providers.
#[test]
fn anthropic_policy_denial_maps_to_policy_denied() {
    let (base_url, _receiver) = fake_server(
        403,
        r#"{"type":"error","error":{"type":"permission_error","message":"denied"}}"#,
    );
    let manifest = build_manifest(&base_url, "anthropic");
    let route = &manifest.routes()[0];
    let adapter = ByokAdapter::new(route, Arc::new(UreqTransport::new()));

    let error = adapter.call(&sentinel_key(), &call("hi", 16)).unwrap_err();
    assert_eq!(error, GatewayError::PolicyDenied);
}

/// Beyond the plan's explicit fixture table: the "unrecognized >=500 -> ProviderUnavailable" fixed
/// rule, distinct from 529's own explicit branch.
#[test]
fn anthropic_unrecognized_server_error_maps_to_provider_unavailable() {
    let (base_url, _receiver) = fake_server(
        503,
        r#"{"type":"error","error":{"type":"api_error","message":"unavailable"}}"#,
    );
    let manifest = build_manifest(&base_url, "anthropic");
    let route = &manifest.routes()[0];
    let adapter = ByokAdapter::new(route, Arc::new(UreqTransport::new()));

    let error = adapter.call(&sentinel_key(), &call("hi", 16)).unwrap_err();
    assert_eq!(error, GatewayError::ProviderUnavailable);
}

/// Beyond the plan's explicit fixture table: "any other unmapped status -> MalformedOutput",
/// distinct from the 200-with-garbage-body path (`anthropic_garbage_200_maps_to_malformed_output`)
/// — this exercises the status-mapping catch-all itself, not the success-body parse failure.
#[test]
fn anthropic_unrecognized_status_maps_to_malformed_output() {
    let (base_url, _receiver) = fake_server(418, r#"{"type":"error","error":{"type":"teapot"}}"#);
    let manifest = build_manifest(&base_url, "anthropic");
    let route = &manifest.routes()[0];
    let adapter = ByokAdapter::new(route, Arc::new(UreqTransport::new()));

    let error = adapter.call(&sentinel_key(), &call("hi", 16)).unwrap_err();
    assert_eq!(error, GatewayError::MalformedOutput);
}

// ---------------------------------------------------------------------------------------------
// OpenAI.
// ---------------------------------------------------------------------------------------------

#[test]
fn openai_success_parses_text_and_usage_and_carries_bearer_auth() {
    let (base_url, receiver) = fake_server(
        200,
        r#"{"choices":[{"message":{"role":"assistant","content":"hello"}}],"usage":{"prompt_tokens":12,"completion_tokens":5}}"#,
    );
    let manifest = build_manifest(&base_url, "openai");
    let route = &manifest.routes()[0];
    let adapter = ByokAdapter::new(route, Arc::new(UreqTransport::new()));

    let reply = adapter
        .call(&sentinel_key(), &call("hi", 32))
        .unwrap_or_else(|error| panic!("expected success: {error}"));

    assert_eq!(reply.text, "hello");
    assert_eq!(reply.usage.input_tokens, Some(12));
    assert_eq!(reply.usage.output_tokens, Some(5));

    let captured = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(captured.method, "POST");
    assert_eq!(captured.path, "/v1/chat/completions");
    let expected_auth = format!("Bearer {SENTINEL}");
    assert_eq!(
        header_value(&captured.headers, "authorization"),
        Some(expected_auth.as_str())
    );
}

/// The one genuine disambiguation this milestone's error mapping performs: OpenAI's `429` is
/// ambiguous between hard quota exhaustion and ordinary throttling, distinguished only by the
/// error body (`byok.rs`'s `openai_error_is_insufficient_quota` — the plan's sabotage target).
#[test]
fn openai_quota_exhaustion_is_distinguished_from_plain_rate_limiting() {
    let (quota_base_url, _quota_receiver) = fake_server(
        429,
        r#"{"error":{"type":"insufficient_quota","code":"insufficient_quota"}}"#,
    );
    let quota_manifest = build_manifest(&quota_base_url, "openai");
    let quota_route = &quota_manifest.routes()[0];
    let quota_adapter = ByokAdapter::new(quota_route, Arc::new(UreqTransport::new()));
    let quota_error = quota_adapter
        .call(&sentinel_key(), &call("hi", 16))
        .unwrap_err();
    assert_eq!(quota_error, GatewayError::QuotaExhausted);

    let (rate_base_url, _rate_receiver) =
        fake_server(429, r#"{"error":{"code":"rate_limit_exceeded"}}"#);
    let rate_manifest = build_manifest(&rate_base_url, "openai");
    let rate_route = &rate_manifest.routes()[0];
    let rate_adapter = ByokAdapter::new(rate_route, Arc::new(UreqTransport::new()));
    let rate_error = rate_adapter
        .call(&sentinel_key(), &call("hi", 16))
        .unwrap_err();
    assert_eq!(rate_error, GatewayError::RateLimited);
}

#[test]
fn openai_auth_failure_maps_to_auth_required() {
    let (base_url, _receiver) = fake_server(
        401,
        r#"{"error":{"type":"invalid_request_error","code":"invalid_api_key"}}"#,
    );
    let manifest = build_manifest(&base_url, "openai");
    let route = &manifest.routes()[0];
    let adapter = ByokAdapter::new(route, Arc::new(UreqTransport::new()));

    let error = adapter.call(&sentinel_key(), &call("hi", 16)).unwrap_err();
    assert_eq!(error, GatewayError::AuthRequired);
}

// ---------------------------------------------------------------------------------------------
// Transport-level protections.
// ---------------------------------------------------------------------------------------------

/// `UreqTransport` must never follow a redirect at all (`transport.rs`'s `max_redirects(0)`):
/// `/v1/messages` never legitimately redirects, and ureq's redirect handling strips only
/// `Authorization`/`Cookie`/`Content-Length` from the re-sent request — Anthropic's `x-api-key`
/// survives untouched and would otherwise be re-sent to whatever host a 302 `Location` names.
/// This plants a real redirect chain (fake server 1 answers 302 pointing at fake server 2) and
/// proves fake server 2 receives no connection at all, not merely that its reply is ignored.
#[test]
fn a_redirect_is_never_followed_and_the_credential_never_reaches_the_redirect_target() {
    // The redirect target. If `UreqTransport` ever followed the 302 below, the real request —
    // carrying the real x-api-key header — would land here. It replies 200 only so a version of
    // the transport that *does* follow redirects resolves quickly instead of hanging for the
    // full request timeout; the assertion below is on whether it was contacted at all, not on
    // what it replies.
    let (target_url, target_receiver) = fake_server(
        200,
        r#"{"content":[{"type":"text","text":"should never be reached"}]}"#,
    );
    let redirect_location = format!("{target_url}/v1/messages");

    let (base_url, receiver) = fake_redirect_server(302, &redirect_location);
    let manifest = build_manifest(&base_url, "anthropic");
    let route = &manifest.routes()[0];
    let adapter = ByokAdapter::new(route, Arc::new(UreqTransport::new()));

    let error = adapter.call(&sentinel_key(), &call("hi", 16)).unwrap_err();

    // Sanity: the first server (the one the manifest actually points at) really was hit, real
    // credential header and all — proves this test is not vacuously true.
    let captured = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(header_value(&captured.headers, "x-api-key"), Some(SENTINEL));

    // The redirect target must never have received a connection at all.
    assert!(
        target_receiver
            .recv_timeout(Duration::from_millis(500))
            .is_err(),
        "the redirect target was contacted — a credential-carrying request followed the 302"
    );

    // 302 is not a status any fixed rule maps to something more specific; byok.rs's own
    // catch-all turns any unmapped status into MalformedOutput.
    assert_eq!(error, GatewayError::MalformedOutput);
}

/// `ByokAdapter` is constructed from a plain `&ModelRoute` with nothing at the type level
/// restricting it to `direct_api` routes. Before this guard, handing it a structurally valid
/// `native_runtime` route (which carries no `baseUrl` at all — forbidden by manifest validation)
/// panicked inside `call_anthropic`/`call_openai`'s `base_url().expect(...)` instead of
/// returning an error.
#[test]
fn a_native_runtime_route_is_rejected_not_panicked_on() {
    let manifest = build_native_runtime_manifest();
    let route = &manifest.routes()[0];
    let adapter = ByokAdapter::new(route, Arc::new(UreqTransport::new()));

    let error = adapter.call(&sentinel_key(), &call("hi", 16)).unwrap_err();
    assert_eq!(error, GatewayError::UnsupportedCapability);
}

// ---------------------------------------------------------------------------------------------
// Shared redaction contract.
// ---------------------------------------------------------------------------------------------

/// The credential value must never surface in a formatted [`GatewayError`] (`Display`/`Debug`) or
/// in a [`TransportRequest`]'s `Debug`. `GatewayError` is a fieldless enum (nothing dynamic could
/// leak through it regardless), so the meaningful half of this test is the `TransportRequest`
/// check: it constructs one carrying the real sentinel header value directly, the same shape
/// `byok.rs` builds internally, and asserts the header *name* survives while the *value* does not.
#[test]
fn the_api_key_never_appears_in_errors_or_debug() {
    let (base_url, receiver) = fake_server(200, "not json");
    let manifest = build_manifest(&base_url, "anthropic");
    let route = &manifest.routes()[0];
    let adapter = ByokAdapter::new(route, Arc::new(UreqTransport::new()));

    let error = adapter.call(&sentinel_key(), &call("hi", 16)).unwrap_err();
    assert_eq!(error, GatewayError::MalformedOutput);
    let display = format!("{error}");
    let debug = format!("{error:?}");
    assert!(!display.contains(SENTINEL), "Display leaked: {display}");
    assert!(!debug.contains(SENTINEL), "Debug leaked: {debug}");

    // Sanity: the fake server really did receive the real key (proves this test is not
    // vacuously true because the key never went anywhere).
    let captured = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(header_value(&captured.headers, "x-api-key"), Some(SENTINEL));

    let request = TransportRequest {
        method: "POST",
        url: format!("{base_url}/v1/messages"),
        headers: vec![("x-api-key".to_owned(), SENTINEL.to_owned())],
        body: Vec::new(),
        timeout: Duration::from_secs(5),
    };
    let request_debug = format!("{request:?}");
    assert!(
        !request_debug.contains(SENTINEL),
        "TransportRequest Debug leaked the key: {request_debug}"
    );
    assert!(
        request_debug.contains("x-api-key"),
        "redaction must still name the header: {request_debug}"
    );
}
