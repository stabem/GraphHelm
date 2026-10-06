//! #313 image evidence over HTTP and MCP: the owner reads each sealed image back as raw bytes
//! under the four locked-down headers; refusals over HTTP append nothing; a scoped agent can attach
//! but cannot read; and the CLI `--attach` and MCP `signal` paths seal the same PNG identically.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::TimeZone;
use graphhelm_protocols::{EventEnvelope, EventKind};
use serde_json::{Value, json};

const EVENTS_KEY: &str = "0101010101010101010101010101010101010101010101010101010101010101";
const KEY_ID: &str = "owner-key";
const RUN: &str = "image-evidence-http";
const AGENT_CREDENTIAL: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3];
const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 4, 5];
const WEBP: &[u8] = b"RIFF\x04\0\0\0WEBPVP8 ";
const SVG: &[u8] = b"<svg xmlns='http://www.w3.org/2000/svg'/>";

fn graphhelm() -> Command {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
    command.env("GRAPHHELM_EVENTS_KEY", EVENTS_KEY);
    command
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn attachment(media_type: &str, bytes: &[u8]) -> Value {
    json!({"mediaType": media_type, "base64": base64(bytes)})
}

struct FixedClock;
impl graphhelm_protocols::Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc.with_ymd_and_hms(2026, 10, 6, 12, 0, 0).unwrap()
    }
}
struct ReadOnlyIds;
impl graphhelm_protocols::IdGenerator for ReadOnlyIds {
    fn next_id(&self, _: &'static str) -> String {
        panic!("this reader must not append")
    }
}

fn history(events: &Path) -> Vec<EventEnvelope> {
    let store = graphhelm_events::LocalEventRepository::open(
        events,
        Arc::new(FixedClock),
        Arc::new(ReadOnlyIds),
    )
    .unwrap();
    let stream = store
        .list_streams()
        .unwrap()
        .into_iter()
        .find(|stream| stream.stream_id.as_str() == RUN)
        .unwrap();
    store
        .read_replay_stream(&stream.scope, &stream.stream_id)
        .unwrap()
}

/// `(evidenceId, contentSha256)` of every ref on the signal whose envelope id is `signal-<id>`.
fn signal_refs(events: &Path, id: &str) -> Vec<(String, String)> {
    let envelope = format!("signal-{id}");
    history(events)
        .into_iter()
        .find(|event| {
            matches!(event.kind, EventKind::SignalRecorded(_))
                && event
                    .evidence_refs
                    .iter()
                    .any(|reference| reference.evidence_id().as_str() == envelope)
        })
        .unwrap()
        .evidence_refs
        .iter()
        .map(|reference| {
            (
                reference.evidence_id().as_str().to_owned(),
                reference.content_sha256().as_str().to_owned(),
            )
        })
        .collect()
}

struct ServerGuard(Child);
impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Harness {
    scratch: tempfile::TempDir,
    events: PathBuf,
    keyring: PathBuf,
    server: Option<ServerGuard>,
    base: String,
    token: String,
}

fn write(directory: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// A held manual execution in a fresh store, with an owner keyring; the server is not started.
fn prepared() -> Harness {
    let scratch = tempfile::tempdir().unwrap();
    let events = scratch.path().join("runtime-data");
    let keyring = scratch.path().join("keyring");
    std::fs::create_dir(&keyring).unwrap();
    graphhelm_sealed_key_provider::SealedKeyProvider::create(
        &keyring,
        KEY_ID,
        graphhelm_events::SecretBytes::new(vec![1; 32]),
    )
    .unwrap();
    let fixtures = write(scratch.path(), "fixtures.json", br#"{"nodeOutcomes":{}}"#);
    let graph = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/graphs/manual-override-deploy.yaml");
    let output = graphhelm()
        .args(["execution", "start", "--events"])
        .arg(&events)
        .args(["--execution", RUN, "--file"])
        .arg(&graph)
        .arg("--fixtures")
        .arg(&fixtures)
        .args(["--mode", "manual", "--held"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    Harness {
        scratch,
        events,
        keyring,
        server: None,
        base: String::new(),
        token: String::new(),
    }
}

fn token_path(events: &Path) -> PathBuf {
    let mut name = events.file_name().unwrap().to_os_string();
    name.push(".token");
    events.with_file_name(name)
}

impl Harness {
    fn serve(&mut self) {
        let binding = format!("{AGENT_CREDENTIAL}=agent-planner|project-local|{RUN}");
        let mut child = graphhelm()
            .args(["serve", "--events"])
            .arg(&self.events)
            .args(["--bind", "127.0.0.1:0", "--keyring"])
            .arg(&self.keyring)
            .args(["--key-id", KEY_ID])
            .env("GRAPHHELM_AGENT_CREDENTIALS", binding)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let guard = ServerGuard(child);
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        let started: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(started["command"], "serve.started", "{started}");
        let address = started["data"]["address"].as_str().unwrap().to_owned();
        self.token = std::fs::read_to_string(token_path(&self.events))
            .unwrap()
            .trim()
            .to_owned();
        self.base = format!("http://{address}");
        self.server = Some(guard);
        let deadline = Instant::now() + Duration::from_secs(10);
        while request(&self.base, "GET", "/health", None, &[], None)
            .map(|reply| reply.status != 200)
            .unwrap_or(true)
        {
            assert!(Instant::now() < deadline, "the server never became healthy");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn head(&self) -> usize {
        history(&self.events).len()
    }

    fn signal(&self, bearer: &str, actor: (&str, &str), key: &str, body: &Value) -> Reply {
        request(
            &self.base,
            "POST",
            &format!("/v1/executions/{RUN}/signal"),
            Some(bearer),
            &[
                ("Idempotency-Key", key),
                ("X-GraphHelm-Actor", actor.0),
                ("X-GraphHelm-Actor-Type", actor.1),
            ],
            Some(&serde_json::to_vec(body).unwrap()),
        )
        .unwrap()
    }

    fn owner_signal(&self, key: &str, body: &Value) -> Reply {
        self.signal(&self.token, ("owner-observer", "owner"), key, body)
    }

    fn evidence(&self, bearer: &str, evidence_id: &str) -> Reply {
        request(
            &self.base,
            "GET",
            &format!("/v1/executions/{RUN}/evidence/{evidence_id}"),
            Some(bearer),
            &[],
            None,
        )
        .unwrap()
    }
}

struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Reply {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

fn request(
    base: &str,
    method: &str,
    path: &str,
    bearer: Option<&str>,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> std::io::Result<Reply> {
    let address = base.strip_prefix("http://").unwrap();
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n");
    if let Some(bearer) = bearer {
        head.push_str(&format!("Authorization: Bearer {bearer}\r\n"));
    }
    if let Some(body) = body {
        head.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    let mut stream = TcpStream::connect(address)?;
    stream.set_read_timeout(Some(Duration::from_secs(60)))?;
    stream.write_all(head.as_bytes())?;
    if let Some(body) = body {
        stream.write_all(body)?;
    }
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("a header terminator");
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .unwrap();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let mut body = raw[split + 4..].to_vec();
    if headers
        .iter()
        .any(|(key, value)| key == "transfer-encoding" && value.contains("chunked"))
    {
        body = dechunk(&body);
    }
    Ok(Reply {
        status,
        headers,
        body,
    })
}

fn dechunk(mut raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let line_end = raw.windows(2).position(|w| w == b"\r\n").unwrap();
        let size_text = String::from_utf8_lossy(&raw[..line_end]);
        let size = usize::from_str_radix(size_text.split(';').next().unwrap().trim(), 16).unwrap();
        raw = &raw[line_end + 2..];
        if size == 0 {
            return out;
        }
        out.extend_from_slice(&raw[..size]);
        raw = &raw[size + 2..];
    }
}

fn owner_note(id: &str) -> Value {
    json!({"id":id,"source":{"type":"user","id":"owner"},"type":"operator_note","severity":"low",
        "description":"see the screenshot","evidence":["owner"],
        "emittedAt":"2026-10-06T12:00:00Z"})
}

fn node_signal(id: &str) -> Value {
    json!({"id":id,"source":{"type":"node","id":"implementation"},"type":"no_progress",
        "severity":"high","description":"the deploy stage needs a manual review",
        "evidence":["exec-1"],"emittedAt":"2026-10-06T12:00:00Z"})
}

fn assert_image_served(reply: &Reply, media_type: &str, bytes: &[u8]) {
    assert_eq!(
        reply.status,
        200,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    assert_eq!(reply.header("content-type"), Some(media_type));
    assert_eq!(reply.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(
        reply.header("content-security-policy"),
        Some("default-src 'none'")
    );
    assert_eq!(reply.header("cache-control"), Some("private, no-store"));
    assert_eq!(reply.body, bytes);
}

#[test]
fn the_owner_reads_each_image_type_as_raw_bytes_under_locked_headers() {
    let mut harness = prepared();
    harness.serve();
    let expected = [
        ("image/png", PNG),
        ("image/jpeg", JPEG),
        ("image/webp", WEBP),
    ];
    let body = json!({
        "signal": owner_note("shots"),
        "attachments": expected
            .iter()
            .map(|(media_type, bytes)| attachment(media_type, bytes))
            .collect::<Vec<_>>(),
    });
    let reply = harness.owner_signal("shots-1", &body);
    assert_eq!(reply.status, 200, "{}", reply.json());
    let attachments = reply.json()["data"]["attachments"].clone();
    for (index, (media_type, bytes)) in expected.iter().enumerate() {
        let evidence_id = format!("signal-shots-image-{}", index + 1);
        assert_eq!(attachments[index]["evidenceId"], evidence_id);
        assert_eq!(attachments[index]["mediaType"], *media_type);
        assert_image_served(
            &harness.evidence(&harness.token, &evidence_id),
            media_type,
            bytes,
        );
    }

    // The idempotency digest covers the attachments: the same body replays without a second
    // append, and the same key with different images diverges instead of replaying.
    let before = harness.head();
    let replay = harness.owner_signal("shots-1", &body);
    assert_eq!(replay.status, 200, "{}", replay.json());
    // A recognised retry answers with the execution's status (the existing replay contract for
    // every mutation), not a second recording.
    assert_eq!(
        replay.json()["data"]["idempotency"]["recognizedRetry"],
        true
    );
    assert_eq!(replay.json()["data"]["signalId"], "shots");
    assert_eq!(harness.head(), before);
    let mut changed = body.clone();
    changed["attachments"] = json!([attachment("image/png", PNG)]);
    let divergent = harness.owner_signal("shots-1", &changed);
    assert_eq!(divergent.status, 409, "{}", divergent.json());
    assert_eq!(harness.head(), before);
}

#[test]
fn svg_wrong_magic_and_malformed_attachments_are_refused_over_http_without_an_append() {
    let mut harness = prepared();
    harness.serve();
    let cases = [
        (
            "svg",
            json!([attachment("image/svg+xml", SVG)]),
            "/attachments/0/mediaType",
        ),
        (
            "magic",
            json!([attachment("image/png", JPEG)]),
            "/attachments/0",
        ),
        (
            "partial",
            json!([attachment("image/png", PNG), attachment("image/webp", PNG)]),
            "/attachments/1",
        ),
        ("shape", json!({"mediaType": "image/png"}), "/attachments"),
        (
            "padding",
            json!([{"mediaType":"image/png","base64":"iVBORw0KGgo"}]),
            "/attachments/0/base64",
        ),
    ];
    for (id, attachments, pointer) in cases {
        let before = harness.head();
        let reply = harness.owner_signal(
            &format!("refused-{id}"),
            &json!({"signal": owner_note(id), "attachments": attachments}),
        );
        assert_eq!(reply.status, 400, "{id}: {}", reply.json());
        let diagnostic = &reply.json()["diagnostics"][0];
        assert_eq!(diagnostic["code"], "GHCLI003_SIGNAL_INVALID", "{id}");
        assert_eq!(diagnostic["path"], pointer, "{id}");
        assert_eq!(harness.head(), before, "{id} appended");
    }
}

#[test]
fn a_scoped_agent_can_attach_an_image_but_cannot_read_it_back() {
    let mut harness = prepared();
    harness.serve();
    let reply = harness.signal(
        AGENT_CREDENTIAL,
        ("agent-planner", "agent"),
        "agent-shot-1",
        &json!({"signal": node_signal("agent-shot"), "attachments": [attachment("image/png", PNG)]}),
    );
    assert_eq!(reply.status, 200, "{}", reply.json());
    let refs = signal_refs(&harness.events, "agent-shot");
    assert_eq!(refs.len(), 2);
    assert_eq!(refs[1].0, "signal-agent-shot-image-1");

    for evidence_id in ["signal-agent-shot-image-1", "signal-agent-shot"] {
        let denied = harness.evidence(AGENT_CREDENTIAL, evidence_id);
        assert_eq!(denied.status, 401, "{evidence_id}");
        assert!(!denied.body.starts_with(PNG));
    }
    assert_image_served(
        &harness.evidence(&harness.token, "signal-agent-shot-image-1"),
        "image/png",
        PNG,
    );
}

fn mcp(harness: &Harness, arguments: Value) -> Value {
    let token_file = write(
        harness.scratch.path(),
        "mcp-token",
        harness.token.as_bytes(),
    );
    let lines = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2025-06-18","capabilities":{},
            "clientInfo":{"name":"conformance","version":"0"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"signal","arguments":arguments}}),
    ];
    let mut input = String::new();
    for line in &lines {
        input.push_str(&line.to_string());
        input.push('\n');
    }
    let output = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["mcp", "--url", &harness.base, "--token-file"])
        .arg(&token_file)
        .args(["--actor", "agent-chat", "--actor-type", "agent"])
        .write_stdin(input)
        .timeout(Duration::from_secs(60))
        .output()
        .unwrap();
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|value| value["id"] == 2)
        .expect("a reply to the tool call")
}

#[test]
fn the_cli_attach_and_the_mcp_signal_seal_the_same_png_identically() {
    let mut harness = prepared();
    // The CLI writes the store directly, so it runs before the server owns it.
    let png = write(harness.scratch.path(), "shot.bin", PNG);
    let note = write(
        harness.scratch.path(),
        "cli.json",
        &serde_json::to_vec(&owner_note("cli-shot")).unwrap(),
    );
    let output = graphhelm()
        .args(["execution", "signal", "--events"])
        .arg(&harness.events)
        .args(["--execution", RUN, "--signal"])
        .arg(&note)
        .arg("--evidence-out")
        .arg(harness.scratch.path().join("cli-evidence.json"))
        .arg("--keyring")
        .arg(&harness.keyring)
        .args(["--key-id", KEY_ID, "--attach"])
        .arg(&png)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let cli_reply: Value = serde_json::from_slice(&output.stdout).unwrap();
    harness.serve();

    let reply = mcp(
        &harness,
        json!({"executionId": RUN, "signal": node_signal("mcp-shot"),
            "attachments": [attachment("image/png", PNG)]}),
    );
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    let mcp_reply: Value =
        serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();

    let cli_attachment = &cli_reply["data"]["attachments"][0];
    let mcp_attachment = &mcp_reply["data"]["attachments"][0];
    assert_eq!(cli_attachment["mediaType"], "image/png");
    assert_eq!(mcp_attachment["mediaType"], cli_attachment["mediaType"]);
    assert_eq!(mcp_attachment["bytes"], cli_attachment["bytes"]);

    let cli_refs = signal_refs(&harness.events, "cli-shot");
    let mcp_refs = signal_refs(&harness.events, "mcp-shot");
    assert_eq!(cli_refs[1].0, "signal-cli-shot-image-1");
    assert_eq!(mcp_refs[1].0, "signal-mcp-shot-image-1");
    assert_eq!(cli_refs[1].1, mcp_refs[1].1, "contentSha256 differs");

    let cli_served = harness.evidence(&harness.token, "signal-cli-shot-image-1");
    let mcp_served = harness.evidence(&harness.token, "signal-mcp-shot-image-1");
    assert_image_served(&cli_served, "image/png", PNG);
    assert_image_served(&mcp_served, "image/png", PNG);
    assert_eq!(cli_served.body, mcp_served.body);
}

#[test]
fn mcp_refuses_svg_and_wrong_magic_like_the_cli_without_an_append() {
    let mut harness = prepared();
    harness.serve();
    let before = harness.head();

    let svg = mcp(
        &harness,
        json!({"executionId": RUN, "signal": node_signal("mcp-svg"),
            "attachments": [attachment("image/svg+xml", SVG)]}),
    );
    // Refused by the advertised closed schema before any HTTP request.
    assert_eq!(svg["error"]["code"], -32602, "{svg}");
    assert_eq!(harness.head(), before);

    let magic = mcp(
        &harness,
        json!({"executionId": RUN, "signal": node_signal("mcp-magic"),
            "attachments": [attachment("image/png", SVG)]}),
    );
    assert_eq!(magic["result"]["isError"], true, "{magic}");
    let envelope: Value =
        serde_json::from_str(magic["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        envelope["diagnostics"][0]["code"],
        "GHCLI003_SIGNAL_INVALID"
    );
    assert_eq!(harness.head(), before);
}

#[test]
fn the_signal_route_admits_a_body_above_the_global_five_mib_limit() {
    let mut harness = prepared();
    harness.serve();
    let mut big = PNG.to_vec();
    big.resize(6 * 1024 * 1024, 7);
    let reply = harness.owner_signal(
        "big-1",
        &json!({"signal": owner_note("big"), "attachments": [attachment("image/png", &big)]}),
    );
    assert_eq!(reply.status, 200, "{}", reply.json());
    assert_image_served(
        &harness.evidence(&harness.token, "signal-big-image-1"),
        "image/png",
        &big,
    );
}
