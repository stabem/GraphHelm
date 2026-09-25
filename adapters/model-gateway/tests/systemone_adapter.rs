//! Fake-server contract tests for the System One adapter (`src/systemone.rs`) over the production
//! [`UreqTransport`].
//!
//! The `TcpListener` helpers below are a copy of `tests/byok_adapters.rs`'s (they are file-private
//! there and this crate has no shared test lib), extended with the request `body` so the
//! documented `POST /v1/systemone` JSON the adapter actually sends can be asserted on. The
//! manifest's cleartext-loopback-only rule (`core/gateway/src/manifest.rs`) is why
//! `http://127.0.0.1:<port>` is a legal `baseUrl` here.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use graphhelm_events::SecretBytes;
use graphhelm_gateway::call::ModelCall;
use graphhelm_gateway::judgment::{Answer, JEV_LATEST, JudgeRequest, Question};
use graphhelm_gateway::manifest::RouteManifest;
use graphhelm_gateway::taxonomy::GatewayError;
use graphhelm_model_gateway::byok::ByokAdapter;
use graphhelm_model_gateway::systemone::SystemOneAdapter;
use graphhelm_model_gateway::transport::UreqTransport;

const SENTINEL: &str = "sk-ant-SENTINEL-0123456789abcdef";

fn sentinel_key() -> SecretBytes {
    SecretBytes::new(SENTINEL.as_bytes().to_vec())
}

/// A validated single-route `direct_api` manifest pointed at a fake server's loopback `base_url`,
/// for the given `provider`.
fn build_manifest(base_url: &str, provider: &str) -> RouteManifest {
    let json = serde_json::json!({
        "manifestVersion": 1,
        "routes": [{
            "id": "judge",
            "provider": provider,
            "transport": "direct_api",
            "authentication": "api_key",
            "billingMode": "per_token",
            "baseUrl": base_url,
            "model": "jev-latest",
            "credentialRef": "secret_typesafe",
            "profiles": ["balanced_reasoning"],
            "enabled": true
        }]
    });
    RouteManifest::from_json(&json.to_string())
        .unwrap_or_else(|error| panic!("fixture manifest must be valid: {error}"))
}

fn request() -> JudgeRequest {
    JudgeRequest {
        state: serde_json::json!("Help! My payouts have been failing for 3 days."),
        model: JEV_LATEST.to_owned(),
        questions: BTreeMap::from([(
            "is_urgent".to_owned(),
            Question::Noul {
                instructions: "Does this convey urgency?".to_owned(),
                criteria: None,
            },
        )]),
    }
}

const OK_BODY: &str = r#"{"model":"jev-latest","answers":{"is_urgent":{"type":"noul","noul":0.92}},"usage":{"input_tokens":312,"output_tokens":48}}"#;

/// One HTTP request captured by [`fake_server`], for assertions.
struct CapturedRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: String,
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
    let content_length = parse_content_length(&buffer[..header_end]);
    let body_start = header_end + 4;
    let body =
        String::from_utf8_lossy(&buffer[body_start..body_start + content_length]).into_owned();

    CapturedRequest {
        method,
        path,
        headers,
        body,
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

#[test]
fn a_success_posts_the_documented_body_with_a_bearer_and_parses_the_answers() {
    let (base_url, captured) = fake_server(200, OK_BODY);
    let manifest = build_manifest(&base_url, "typesafe");
    let adapter = SystemOneAdapter::new(&manifest.routes()[0], Arc::new(UreqTransport::new()));
    let reply = adapter.call(&sentinel_key(), &request()).unwrap();
    assert_eq!(reply.answers["is_urgent"], Answer::Noul { noul: 0.92 });
    assert_eq!(reply.usage.input_tokens, Some(312));

    let seen = captured.recv().unwrap();
    assert_eq!(seen.method, "POST");
    assert_eq!(seen.path, "/v1/systemone");
    assert_eq!(
        header_value(&seen.headers, "authorization"),
        Some(format!("Bearer {SENTINEL}").as_str())
    );
    assert_eq!(
        header_value(&seen.headers, "content-type"),
        Some("application/json")
    );
    let body: serde_json::Value = serde_json::from_str(&seen.body).unwrap();
    assert_eq!(body, serde_json::to_value(request()).unwrap());
    assert!(
        !seen.body.contains(SENTINEL),
        "the key travels in the header only"
    );
}

#[test]
fn every_documented_status_maps_to_its_taxonomy_error() {
    for (status, expected) in [
        (401, GatewayError::AuthRequired),
        (403, GatewayError::PolicyDenied),
        (422, GatewayError::MalformedOutput),
        (429, GatewayError::RateLimited),
        (529, GatewayError::ProviderUnavailable),
        (503, GatewayError::ProviderUnavailable),
    ] {
        let (base_url, _captured) = fake_server(status, r#"{"error":"x"}"#);
        let manifest = build_manifest(&base_url, "typesafe");
        let adapter = SystemOneAdapter::new(&manifest.routes()[0], Arc::new(UreqTransport::new()));
        assert_eq!(
            adapter.call(&sentinel_key(), &request()).unwrap_err(),
            expected,
            "status {status}"
        );
    }
}

#[test]
fn a_success_whose_body_is_not_a_reply_is_malformed_output() {
    let (base_url, _captured) = fake_server(
        200,
        r#"{"model":"jev-latest","answers":{"is_urgent":{"type":"essay"}},"usage":{}}"#,
    );
    let manifest = build_manifest(&base_url, "typesafe");
    let adapter = SystemOneAdapter::new(&manifest.routes()[0], Arc::new(UreqTransport::new()));
    assert_eq!(
        adapter.call(&sentinel_key(), &request()).unwrap_err(),
        GatewayError::MalformedOutput
    );
}

#[test]
fn a_chat_provider_on_the_judge_door_is_unsupported_and_sends_nothing() {
    let (base_url, captured) = fake_server(200, OK_BODY);
    let manifest = build_manifest(&base_url, "anthropic");
    let adapter = SystemOneAdapter::new(&manifest.routes()[0], Arc::new(UreqTransport::new()));
    assert_eq!(
        adapter.call(&sentinel_key(), &request()).unwrap_err(),
        GatewayError::UnsupportedCapability
    );
    assert!(
        captured.recv_timeout(Duration::from_millis(200)).is_err(),
        "no request must reach the server"
    );
}

#[test]
fn a_typesafe_route_on_the_draft_door_is_unsupported_and_sends_nothing() {
    let (base_url, captured) = fake_server(200, OK_BODY);
    let manifest = build_manifest(&base_url, "typesafe");
    let adapter = ByokAdapter::new(&manifest.routes()[0], Arc::new(UreqTransport::new()));
    let call = ModelCall {
        prompt: "draft a graph".to_owned(),
        max_tokens: 16,
    };
    assert_eq!(
        adapter.call(&sentinel_key(), &call).unwrap_err(),
        GatewayError::UnsupportedCapability
    );
    assert!(captured.recv_timeout(Duration::from_millis(200)).is_err());
}
