//! Black-box conformance for the wake doorbell's serve-side ring (05g Task 2): a live
//! serve, a real named pipe armed by the test as the sleeper, and appends made through the
//! HTTP API as the triggers. The invariant under test: **a ring implies a durable trigger**
//! — the byte may only arrive after the append that caused it is readable in the store —
//! and a consumed lease never rings twice.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(windows)]
use sha2::{Digest, Sha256};

struct ServerGuard {
    child: Child,
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn token_path(events: &Path) -> PathBuf {
    let mut name = events.file_name().map_or_else(
        || std::ffi::OsString::from("events"),
        std::ffi::OsStr::to_os_string,
    );
    name.push(".token");
    events.with_file_name(name)
}

fn serve(events: &Path) -> (ServerGuard, String, String) {
    serve_with_env(events, &[])
}

fn serve_with_env(events: &Path, env: &[(&str, &str)]) -> (ServerGuard, String, String) {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
    command
        .args([
            "serve",
            "--events",
            events.to_str().unwrap(),
            "--bind",
            "127.0.0.1:0",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for (key, value) in env {
        command.env(key, value);
    }
    let mut child = command.spawn().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    let started: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(started["command"], "serve.started", "{started}");
    let address = started["data"]["address"].as_str().unwrap().to_owned();
    let token = std::fs::read_to_string(token_path(events))
        .unwrap()
        .trim()
        .to_owned();
    (ServerGuard { child }, address, token)
}

/// A bearer-authenticated GET, parsed — the read half of the same raw-socket client the
/// mutations use.
fn get_json(address: &str, token: &str, path: &str) -> serde_json::Value {
    use std::io::{Read, Write};
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nAuthorization: Bearer {token}\r\n\r\n"
    );
    let mut stream = std::net::TcpStream::connect(address).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut reply = Vec::new();
    let _ = stream.read_to_end(&mut reply);
    let text = String::from_utf8_lossy(&reply);
    text.split("\r\n\r\n")
        .nth(1)
        .and_then(|body| serde_json::from_str(body.trim()).ok())
        .unwrap_or(serde_json::Value::Null)
}

fn post_json(
    address: &str,
    token: &str,
    path: &str,
    key: &str,
    body: &serde_json::Value,
) -> (u16, serde_json::Value) {
    use std::io::{Read, Write};
    let payload = serde_json::to_vec(body).unwrap();
    let mut request = format!(
        "POST {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nAuthorization: Bearer {token}\r\nIdempotency-Key: {key}\r\nX-GraphHelm-Actor: agent-wake-test\r\nX-GraphHelm-Actor-Type: agent\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        payload.len()
    );
    let mut stream = std::net::TcpStream::connect(address).unwrap();
    request.push_str("");
    stream.write_all(request.as_bytes()).unwrap();
    stream.write_all(&payload).unwrap();
    let mut reply = Vec::new();
    let _ = stream.read_to_end(&mut reply);
    let text = String::from_utf8_lossy(&reply);
    let status: u16 = text
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or(0);
    let value = text
        .split("\r\n\r\n")
        .nth(1)
        .and_then(|body| serde_json::from_str(body.trim()).ok())
        .unwrap_or(serde_json::Value::Null);
    (status, value)
}

const SEQUENCE_CONFLICT_RETRY_LIMIT: usize = 4;
const SEQUENCE_POST_DEADLINE: Duration = Duration::from_secs(20);
const SEQUENCE_POST_IO_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_SEQUENCE_POST_RESPONSE_BYTES: u64 = 64 * 1024;

#[derive(Clone, Copy)]
struct SequencePostRequest<'a> {
    address: &'a str,
    token: &'a str,
    path: &'a str,
    key: &'a str,
    body: &'a serde_json::Value,
    if_match: Option<u64>,
    deadline: Instant,
}

#[derive(Debug, PartialEq, Eq)]
struct SequencePostTrace {
    status: u16,
    code: Option<String>,
    path: Option<String>,
    current_head: Option<u64>,
}

#[derive(Debug)]
struct SequenceRetryError {
    message: String,
    trace: Vec<SequencePostTrace>,
    phase: &'static str,
    byte_count: usize,
}

#[derive(Debug)]
struct SequencePostOutcome {
    status: u16,
    reply: serde_json::Value,
    retries: usize,
    trace: Vec<SequencePostTrace>,
}

#[derive(Debug, PartialEq, Eq)]
enum SequencePostDecision {
    Accepted,
    Retry { current_head: u64 },
}

#[derive(Debug, PartialEq, Eq)]
struct SequenceReadFailure {
    byte_count: usize,
    message: String,
}

impl SequenceReadFailure {
    fn new(byte_count: usize, message: impl Into<String>) -> Self {
        Self {
            byte_count,
            message: message.into(),
        }
    }
}

#[derive(Debug)]
struct SequenceTransportError {
    phase: &'static str,
    byte_count: usize,
    message: String,
}

impl SequenceTransportError {
    fn new(phase: &'static str, byte_count: usize, message: impl Into<String>) -> Self {
        Self {
            phase,
            byte_count,
            message: message.into(),
        }
    }
}

impl From<String> for SequenceTransportError {
    fn from(message: String) -> Self {
        Self::new("transport", 0, message)
    }
}

fn sequence_io_timeout_at(deadline: Instant, now: Instant) -> Result<Duration, String> {
    if now >= deadline {
        return Err("the sequence-conflict request deadline expired".to_owned());
    }
    Ok((deadline - now).min(SEQUENCE_POST_IO_TIMEOUT))
}

fn sequence_io_timeout(deadline: Instant) -> Result<Duration, String> {
    sequence_io_timeout_at(deadline, Instant::now())
}

fn write_until_sequence_deadline(
    stream: &mut std::net::TcpStream,
    bytes: &[u8],
    deadline: Instant,
) -> Result<(), String> {
    use std::io::Write;
    let mut offset = 0;
    while offset < bytes.len() {
        let timeout = sequence_io_timeout(deadline)?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(|error| format!("cannot set bounded write timeout: {error}"))?;
        let written = stream
            .write(&bytes[offset..])
            .map_err(|error| format!("bounded HTTP write failed: {error}"))?;
        if written == 0 {
            return Err("bounded HTTP write made no progress".to_owned());
        }
        offset += written;
    }
    Ok(())
}

fn read_response_until_sequence_deadline<F, N>(
    deadline: Instant,
    mut read: F,
    mut now: N,
) -> Result<Vec<u8>, SequenceReadFailure>
where
    F: FnMut(Duration, &mut [u8]) -> std::io::Result<usize>,
    N: FnMut() -> Instant,
{
    // The two-second socket timeout is only a retryable wait slice. The one absolute request
    // deadline remains authoritative across every TimedOut/WouldBlock result.
    let mut reply = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let timeout = sequence_io_timeout_at(deadline, now())
            .map_err(|error| SequenceReadFailure::new(reply.len(), error))?;
        let read = match read(timeout, &mut chunk) {
            Ok(read) => {
                if now() >= deadline {
                    return Err(SequenceReadFailure::new(
                        reply.len(),
                        "the sequence-conflict request deadline expired",
                    ));
                }
                read
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                continue;
            }
            Err(error) => {
                return Err(SequenceReadFailure::new(
                    reply.len(),
                    format!("bounded HTTP read failed: {error}"),
                ));
            }
        };
        if read > chunk.len() {
            return Err(SequenceReadFailure::new(
                reply.len(),
                "bounded HTTP reader reported more bytes than its buffer",
            ));
        }
        if read == 0 {
            break;
        }
        if reply.len() as u64 + read as u64 > MAX_SEQUENCE_POST_RESPONSE_BYTES {
            return Err(SequenceReadFailure::new(
                reply.len().saturating_add(read),
                format!("HTTP response exceeded {MAX_SEQUENCE_POST_RESPONSE_BYTES} bytes"),
            ));
        }
        reply.extend_from_slice(&chunk[..read]);
    }
    Ok(reply)
}

fn parse_bounded_http_response(reply: &[u8]) -> Result<(u16, serde_json::Value), String> {
    let separator = reply
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| "HTTP response had no header/body separator".to_owned())?;
    let header = std::str::from_utf8(&reply[..separator])
        .map_err(|error| format!("HTTP response headers were not UTF-8: {error}"))?;
    let status = header
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| "HTTP response had no valid status".to_owned())?;
    let body = std::str::from_utf8(&reply[separator + 4..])
        .map_err(|error| format!("HTTP response body was not UTF-8: {error}"))?;
    let value = serde_json::from_str(body.trim())
        .map_err(|error| format!("HTTP response body was not JSON: {error}"))?;
    Ok((status, value))
}

/// The race fixture's only retry client. Every connect, write, and read is bounded by the
/// one deadline; the ordinary `post_json` above remains the deliberately small client used by
/// the other conformance tests.
fn post_json_bounded(
    request: SequencePostRequest<'_>,
) -> Result<(u16, serde_json::Value), SequenceTransportError> {
    use std::io::Read;
    let payload = serde_json::to_vec(request.body)
        .map_err(|error| SequenceTransportError::new("request-encode", 0, error.to_string()))?;
    let socket = request
        .address
        .parse::<std::net::SocketAddr>()
        .map_err(|error| {
            SequenceTransportError::new(
                "connect",
                0,
                format!("invalid serve address {}: {error}", request.address),
            )
        })?;
    let connect_timeout = sequence_io_timeout(request.deadline)
        .map_err(|error| SequenceTransportError::new("connect", 0, error))?;
    let mut stream = std::net::TcpStream::connect_timeout(&socket, connect_timeout)
        .map_err(|error| SequenceTransportError::new("connect", 0, error.to_string()))?;
    let if_match_header = request
        .if_match
        .map_or_else(String::new, |head| format!("If-Match: {head}\r\n"));
    let header = format!(
        "POST {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nAuthorization: Bearer {}\r\nIdempotency-Key: {}\r\nX-GraphHelm-Actor: agent-wake-test\r\nX-GraphHelm-Actor-Type: agent\r\n{}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        request.path,
        request.address,
        request.token,
        request.key,
        if_match_header,
        payload.len()
    );
    write_until_sequence_deadline(&mut stream, header.as_bytes(), request.deadline)
        .map_err(|error| SequenceTransportError::new("request-write", 0, error))?;
    write_until_sequence_deadline(&mut stream, &payload, request.deadline)
        .map_err(|error| SequenceTransportError::new("request-write", 0, error))?;

    let reply = read_response_until_sequence_deadline(
        request.deadline,
        |timeout, chunk| {
            stream.set_read_timeout(Some(timeout))?;
            stream.read(chunk)
        },
        Instant::now,
    )
    .map_err(|error| {
        SequenceTransportError::new("response-read", error.byte_count, error.message)
    })?;
    parse_bounded_http_response(&reply)
        .map_err(|error| SequenceTransportError::new("response-parse", reply.len(), error))
}

fn sequence_post_trace(status: u16, reply: &serde_json::Value) -> SequencePostTrace {
    let diagnostic = reply
        .get("diagnostics")
        .and_then(serde_json::Value::as_array)
        .and_then(|items| items.first());
    SequencePostTrace {
        status,
        code: diagnostic
            .and_then(|item| item.get("code"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        path: diagnostic
            .and_then(|item| item.get("path"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        current_head: reply
            .get("data")
            .and_then(|data| data.get("currentHead"))
            .and_then(serde_json::Value::as_u64),
    }
}

fn classify_sequence_post(
    status: u16,
    reply: &serde_json::Value,
) -> Result<SequencePostDecision, String> {
    if status == 200 {
        return if reply["ok"].as_bool() == Some(true) {
            Ok(SequencePostDecision::Accepted)
        } else {
            Err("200 response was not an accepted signal mutation".to_owned())
        };
    }
    if status != 409 {
        return Err(format!(
            "signal mutation returned unexpected HTTP status {status}"
        ));
    }
    let diagnostic = reply["diagnostics"]
        .as_array()
        .and_then(|items| items.first());
    let code = diagnostic.and_then(|item| item["code"].as_str());
    let path = diagnostic.and_then(|item| item["path"].as_str());
    let current_head = reply["data"]["currentHead"].as_u64();
    if reply["ok"].as_bool() != Some(false)
        || code != Some("GHE001_SEQUENCE_CONFLICT")
        || !matches!(path, Some("/") | Some("/ifMatch"))
        || current_head.is_none()
    {
        return Err(format!(
            "409 response was not a retryable GHE001_SEQUENCE_CONFLICT at / or /ifMatch (ok={:?}, code={code:?}, path={path:?}, currentHead={current_head:?})",
            reply["ok"].as_bool()
        ));
    }
    Ok(SequencePostDecision::Retry {
        current_head: current_head.expect("checked above"),
    })
}

/// One retry state machine drives both the live socket and the deterministic tests. The
/// transport receives the complete request identity on every pass, making key/body/If-Match
/// preservation observable without duplicating this loop in a test-only script.
fn run_sequence_retry<'a, F, E>(
    address: &'a str,
    token: &'a str,
    path: &'a str,
    key: &'a str,
    body: &'a serde_json::Value,
    mut transport: F,
) -> Result<SequencePostOutcome, SequenceRetryError>
where
    F: FnMut(SequencePostRequest<'a>) -> Result<(u16, serde_json::Value), E>,
    E: Into<SequenceTransportError>,
{
    let started = Instant::now();
    let deadline = started + SEQUENCE_POST_DEADLINE;
    let mut if_match = None;
    let mut trace = Vec::new();
    for attempt in 0..=SEQUENCE_CONFLICT_RETRY_LIMIT {
        let request = SequencePostRequest {
            address,
            token,
            path,
            key,
            body,
            if_match,
            deadline,
        };
        let (status, reply) = match transport(request) {
            Ok(response) => response,
            Err(error) => {
                let SequenceTransportError {
                    phase,
                    byte_count,
                    message,
                } = error.into();
                return Err(SequenceRetryError {
                    message: format!("transport failed on attempt {}: {}", attempt + 1, message),
                    trace,
                    phase,
                    byte_count,
                });
            }
        };
        trace.push(sequence_post_trace(status, &reply));
        match classify_sequence_post(status, &reply) {
            Ok(SequencePostDecision::Accepted) => {
                return Ok(SequencePostOutcome {
                    status,
                    reply,
                    retries: attempt,
                    trace,
                });
            }
            Ok(SequencePostDecision::Retry { current_head }) => {
                if attempt == SEQUENCE_CONFLICT_RETRY_LIMIT {
                    return Err(SequenceRetryError {
                        message: format!(
                            "sequence retry budget exhausted after {SEQUENCE_CONFLICT_RETRY_LIMIT} retries"
                        ),
                        trace,
                        phase: "sequence-retry",
                        byte_count: 0,
                    });
                }
                if_match = Some(current_head);
            }
            Err(error) => {
                return Err(SequenceRetryError {
                    message: format!("attempt {} was refused: {error}", attempt + 1),
                    trace,
                    phase: "response-classify",
                    byte_count: 0,
                });
            }
        }
    }
    unreachable!("the bounded sequence retry loop always returns or reports an error")
}

fn post_signal_with_sequence_retry(
    address: &str,
    token: &str,
    path: &str,
    key: &str,
    body: &serde_json::Value,
) -> Result<SequencePostOutcome, SequenceRetryError> {
    run_sequence_retry(address, token, path, key, body, post_json_bounded)
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn write_json(directory: &Path, name: &str, value: &serde_json::Value) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
    path
}

fn signal_body(id: &str, evidence_out: &Path) -> serde_json::Value {
    serde_json::json!({
        "signal": {
            "id": id,
            "source": {"type": "node", "id": "implementation"},
            "type": "no_progress",
            "severity": "high",
            "description": "wake trigger",
            "evidence": ["exec-1"],
            "emittedAt": "2026-08-16T00:00:00Z"
        },
        "evidenceOut": evidence_out.to_str().unwrap(),
    })
}

fn sequence_conflict_reply(current_head: u64, code: &str, path: &str) -> serde_json::Value {
    serde_json::json!({
        "ok": false,
        "command": "execution.signal",
        "data": {"currentHead": current_head},
        "diagnostics": [{
            "code": code,
            "path": path,
            "severity": "error",
            "message": "test conflict"
        }]
    })
}

fn accepted_signal_reply() -> serde_json::Value {
    serde_json::json!({"ok": true, "command": "execution.signal"})
}

#[test]
fn bounded_response_reader_preserves_partial_bytes_across_retryable_socket_reads() {
    use std::collections::VecDeque;
    use std::io::ErrorKind;

    enum Step {
        Timeout,
        WouldBlock,
        Bytes(&'static [u8]),
        Eof,
    }

    let start = Instant::now();
    let deadline = start + Duration::from_secs(20);
    let mut steps = VecDeque::from([
        Step::Timeout,
        Step::Bytes(b"he"),
        Step::WouldBlock,
        Step::Bytes(b"llo"),
        Step::Eof,
    ]);
    let mut clocks = VecDeque::from([
        start,
        start + Duration::from_secs(1),
        start + Duration::from_secs(1),
        start + Duration::from_secs(2),
        start + Duration::from_secs(3),
        start + Duration::from_secs(3),
        start + Duration::from_secs(4),
        start + Duration::from_secs(4),
    ]);
    let mut observed_timeouts = Vec::new();
    let reply = read_response_until_sequence_deadline(
        deadline,
        |timeout, buffer| {
            observed_timeouts.push(timeout);
            match steps.pop_front().expect("the scripted reader has a step") {
                Step::Timeout => Err(std::io::Error::new(ErrorKind::TimedOut, "scripted timeout")),
                Step::WouldBlock => Err(std::io::Error::new(
                    ErrorKind::WouldBlock,
                    "scripted would-block",
                )),
                Step::Bytes(bytes) => {
                    buffer[..bytes.len()].copy_from_slice(bytes);
                    Ok(bytes.len())
                }
                Step::Eof => Ok(0),
            }
        },
        || clocks.pop_front().expect("the scripted clock has a value"),
    )
    .expect("retryable socket reads preserve the response");

    assert_eq!(reply, b"hello");
    assert_eq!(observed_timeouts, vec![Duration::from_secs(2); 5]);
}

#[test]
fn bounded_response_reader_keeps_one_absolute_deadline_after_partial_retries() {
    use std::collections::VecDeque;
    use std::io::ErrorKind;

    enum Step {
        Timeout,
        WouldBlock,
        Bytes(&'static [u8]),
    }

    let start = Instant::now();
    let deadline = start + Duration::from_secs(20);
    let mut steps = VecDeque::from([
        Step::Timeout,
        Step::Bytes(b"ab"),
        Step::WouldBlock,
        Step::Bytes(b"cd"),
        Step::Timeout,
        Step::Timeout,
        Step::Timeout,
        Step::Timeout,
        Step::Timeout,
        Step::Timeout,
    ]);
    let mut clocks = VecDeque::from([
        start,
        start + Duration::from_secs(2),
        start + Duration::from_secs(2),
        start + Duration::from_secs(4),
        start + Duration::from_secs(6),
        start + Duration::from_secs(6),
        start + Duration::from_secs(8),
        start + Duration::from_secs(10),
        start + Duration::from_secs(12),
        start + Duration::from_secs(14),
        start + Duration::from_secs(16),
        start + Duration::from_secs(18),
        deadline,
    ]);
    let mut observed_timeouts = Vec::new();
    let failure = read_response_until_sequence_deadline(
        deadline,
        |timeout, buffer| {
            observed_timeouts.push(timeout);
            match steps.pop_front().expect("the scripted reader has a step") {
                Step::Timeout => Err(std::io::Error::new(ErrorKind::TimedOut, "scripted timeout")),
                Step::WouldBlock => Err(std::io::Error::new(
                    ErrorKind::WouldBlock,
                    "scripted would-block",
                )),
                Step::Bytes(bytes) => {
                    buffer[..bytes.len()].copy_from_slice(bytes);
                    Ok(bytes.len())
                }
            }
        },
        || clocks.pop_front().expect("the scripted clock has a value"),
    )
    .expect_err("the absolute deadline eventually expires");

    assert_eq!(failure.byte_count, 4);
    assert!(failure.message.contains("deadline expired"));
    assert_eq!(observed_timeouts.len(), 10);
    assert!(
        observed_timeouts
            .iter()
            .all(|timeout| *timeout == Duration::from_secs(2))
    );
}

#[test]
fn bounded_response_reader_rejects_hard_errors_with_the_partial_byte_count() {
    use std::collections::VecDeque;

    let mut steps = VecDeque::from([true, false]);
    let start = Instant::now();
    let failure = read_response_until_sequence_deadline(
        start + Duration::from_secs(20),
        |_timeout, buffer| {
            if steps.pop_front().expect("the scripted reader has a step") {
                buffer[..4].copy_from_slice(b"part");
                Ok(4)
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionReset,
                    "scripted hard error",
                ))
            }
        },
        || start,
    )
    .expect_err("a hard socket error is terminal");

    assert_eq!(failure.byte_count, 4);
    assert!(failure.message.contains("bounded HTTP read failed"));
}

#[test]
fn bounded_response_reader_rejects_an_oversize_response() {
    let start = Instant::now();
    let failure = read_response_until_sequence_deadline(
        start + Duration::from_secs(20),
        |_timeout, buffer| {
            buffer.fill(b'x');
            Ok(buffer.len())
        },
        || start,
    )
    .expect_err("the response byte cap is enforced before append");

    assert_eq!(
        failure.byte_count,
        MAX_SEQUENCE_POST_RESPONSE_BYTES as usize + 8192
    );
    assert!(failure.message.contains("response exceeded"));
}

#[test]
fn bounded_response_reader_rejects_a_successful_read_after_the_deadline() {
    use std::collections::VecDeque;

    let start = Instant::now();
    let deadline = start + Duration::from_secs(20);
    let mut clocks = VecDeque::from([start, deadline]);
    let failure = read_response_until_sequence_deadline(
        deadline,
        |_timeout, buffer| {
            buffer[..4].copy_from_slice(b"late");
            Ok(4)
        },
        || clocks.pop_front().expect("the scripted clock has a value"),
    )
    .expect_err("a read completing at the deadline is not a successful response");

    assert_eq!(failure.byte_count, 0);
    assert!(failure.message.contains("deadline expired"));
}

#[test]
fn bounded_response_reader_caps_the_final_retry_slice_to_remaining_deadline() {
    use std::collections::VecDeque;
    use std::io::ErrorKind;

    let start = Instant::now();
    let deadline = start + Duration::from_secs(20);
    let mut clocks = VecDeque::from([start, start + Duration::from_millis(19_500), deadline]);
    let mut observed_timeouts = Vec::new();
    let mut read_calls = 0;
    let failure = read_response_until_sequence_deadline(
        deadline,
        |timeout, _buffer| {
            observed_timeouts.push(timeout);
            read_calls += 1;
            Err(std::io::Error::new(ErrorKind::TimedOut, "scripted timeout"))
        },
        || clocks.pop_front().expect("the scripted clock has a value"),
    )
    .expect_err("the original absolute deadline remains terminal");

    assert_eq!(failure.byte_count, 0);
    assert!(failure.message.contains("deadline expired"));
    assert_eq!(read_calls, 2);
    assert_eq!(
        observed_timeouts,
        vec![Duration::from_secs(2), Duration::from_millis(500)]
    );
}

#[test]
fn bounded_http_response_parser_rejects_an_incomplete_eof() {
    let error =
        parse_bounded_http_response(b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\n{\"ok\":true")
            .expect_err("EOF before the JSON body is complete");

    assert!(error.contains("body was not JSON"));
}

#[test]
fn sequence_transport_error_is_terminal_without_resending_the_post() {
    let body = serde_json::json!({"private": "request-body"});
    let mut calls = 0;
    let error = run_sequence_retry(
        "127.0.0.1:40000",
        "bearer-secret",
        "/v1/executions/exec-wake-race/signal",
        "same-request-key",
        &body,
        |_request| {
            calls += 1;
            Err(SequenceTransportError::new(
                "response-read",
                4,
                "connection reset",
            ))
        },
    )
    .expect_err("a transport failure must not be retried as a sequence conflict");

    assert_eq!(calls, 1);
    assert_eq!(error.phase, "response-read");
    assert_eq!(error.byte_count, 4);
    assert!(error.message.contains("transport failed on attempt 1"));
}

#[test]
fn sequence_conflict_retry_requires_exact_conflict_then_success_and_preserves_request() {
    use std::collections::VecDeque;

    let body = serde_json::json!({
        "signal": {
            "id": "signal-race-script",
            "source": {"type": "node", "id": "implementation"},
            "type": "no_progress",
            "severity": "high",
            "description": "private body",
            "evidence": ["exec-1"]
        },
        "evidenceOut": "private-evidence-path"
    });
    let mut replies = VecDeque::from([
        (
            409,
            sequence_conflict_reply(12, "GHE001_SEQUENCE_CONFLICT", "/"),
        ),
        (200, accepted_signal_reply()),
    ]);
    let mut requests = Vec::new();
    let outcome = run_sequence_retry(
        "127.0.0.1:40000",
        "bearer-secret",
        "/v1/executions/exec-wake-race/signal",
        "same-request-key",
        &body,
        |request| {
            requests.push((
                request.path.to_owned(),
                request.key.to_owned(),
                request.token.to_owned(),
                request.body.clone(),
                request.if_match,
                request.deadline,
            ));
            replies
                .pop_front()
                .ok_or_else(|| "script ran out of responses".to_owned())
        },
    )
    .expect("an exact sequence conflict is retryable");

    assert_eq!(outcome.status, 200);
    assert_eq!(outcome.retries, 1);
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].0, requests[1].0);
    assert_eq!(requests[0].1, "same-request-key");
    assert_eq!(requests[1].1, "same-request-key");
    assert_eq!(requests[0].2, "bearer-secret");
    assert_eq!(requests[1].2, "bearer-secret");
    assert_eq!(requests[0].3, body);
    assert_eq!(requests[1].3, body);
    assert_eq!(requests[0].4, None);
    assert_eq!(requests[1].4, Some(12));
    assert_eq!(
        requests[0].5, requests[1].5,
        "a 409 retry keeps the original absolute deadline"
    );
    assert_eq!(outcome.trace.len(), 2);
    assert_eq!(outcome.trace[0].status, 409);
    assert_eq!(
        outcome.trace[0].code.as_deref(),
        Some("GHE001_SEQUENCE_CONFLICT")
    );
    assert_eq!(outcome.trace[0].path.as_deref(), Some("/"));
    assert_eq!(outcome.trace[0].current_head, Some(12));
    assert_eq!(outcome.trace[1].status, 200);
    assert_eq!(outcome.trace[1].code, None);
    assert_eq!(outcome.trace[1].path, None);
    assert_eq!(outcome.trace[1].current_head, None);
    let trace_debug = format!("{:?}", outcome.trace);
    assert!(!trace_debug.contains("bearer-secret"));
    assert!(!trace_debug.contains("private-evidence-path"));
}

#[test]
fn sequence_conflict_retry_accepts_if_match_conflict_and_captures_its_head() {
    use std::collections::VecDeque;

    let body = serde_json::json!({"signal": {"id": "signal-if-match"}});
    let mut replies = VecDeque::from([
        (
            409,
            sequence_conflict_reply(27, "GHE001_SEQUENCE_CONFLICT", "/ifMatch"),
        ),
        (200, accepted_signal_reply()),
    ]);
    let mut if_matches = Vec::new();
    let outcome = run_sequence_retry(
        "127.0.0.1:40000",
        "bearer-secret",
        "/v1/executions/exec-wake-race/signal",
        "if-match-request-key",
        &body,
        |request| {
            if_matches.push(request.if_match);
            replies
                .pop_front()
                .ok_or_else(|| "script ran out of responses".to_owned())
        },
    )
    .expect("an If-Match sequence conflict is retryable");

    assert_eq!(outcome.retries, 1);
    assert_eq!(if_matches, [None, Some(27)]);
    assert_eq!(
        outcome.trace[0].code.as_deref(),
        Some("GHE001_SEQUENCE_CONFLICT")
    );
    assert_eq!(outcome.trace[0].path.as_deref(), Some("/ifMatch"));
    assert_eq!(outcome.trace[0].current_head, Some(27));
}

#[test]
fn sequence_conflict_retry_rejects_wrong_code_500_and_repeated_conflicts() {
    use std::collections::VecDeque;

    let body = serde_json::json!({"private": "request-body"});
    let run = |script: Vec<(u16, serde_json::Value)>| {
        let mut replies = VecDeque::from(script);
        run_sequence_retry(
            "127.0.0.1:40000",
            "bearer-secret",
            "/v1/executions/exec-wake-race/signal",
            "same-request-key",
            &body,
            |request| {
                assert_eq!(request.key, "same-request-key");
                assert_eq!(request.body, &body);
                replies
                    .pop_front()
                    .ok_or_else(|| "script ran out of responses".to_owned())
            },
        )
    };

    let wrong_code = run(vec![(
        409,
        sequence_conflict_reply(12, "GHE003_IDEMPOTENCY_CONFLICT", "/"),
    )])
    .expect_err("a different 409 diagnostic is not retryable");
    assert_eq!(wrong_code.trace.len(), 1);
    assert_eq!(wrong_code.trace[0].status, 409);
    assert_eq!(
        wrong_code.trace[0].code.as_deref(),
        Some("GHE003_IDEMPOTENCY_CONFLICT")
    );

    let wrong_path = run(vec![(
        409,
        sequence_conflict_reply(12, "GHE001_SEQUENCE_CONFLICT", "/other"),
    )])
    .expect_err("a sequence conflict from an unknown path is not retryable");
    assert!(wrong_path.message.contains("path=Some(\"/other\")"));
    assert!(wrong_path.message.contains("currentHead=Some(12)"));

    let server_error =
        run(vec![(500, accepted_signal_reply())]).expect_err("500 is never an accepted mutation");
    assert_eq!(server_error.trace.len(), 1);
    assert_eq!(server_error.trace[0].status, 500);

    let repeated = (0..=SEQUENCE_CONFLICT_RETRY_LIMIT)
        .map(|head| {
            (
                409,
                sequence_conflict_reply(head as u64, "GHE001_SEQUENCE_CONFLICT", "/"),
            )
        })
        .collect::<Vec<_>>();
    let exhausted = run(repeated).expect_err("repeated conflicts must fail closed");
    assert_eq!(exhausted.trace.len(), SEQUENCE_CONFLICT_RETRY_LIMIT + 1);
    assert!(exhausted.message.contains("retry budget"));
    assert!(
        exhausted
            .trace
            .iter()
            .enumerate()
            .all(|(head, trace)| trace.status == 409 && trace.current_head == Some(head as u64))
    );
}

fn start_execution(events: &Path, directory: &Path, execution: &str) {
    let fixtures = write_json(
        directory,
        "fixtures.json",
        &serde_json::json!({"nodeOutcomes": {"implementation": "failure", "deploy": "success"}}),
    );
    let graph = root().join("examples/graphs/manual-override-deploy.yaml");
    let output = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args([
            "execution",
            "start",
            "--file",
            graph.to_str().unwrap(),
            "--events",
            events.to_str().unwrap(),
            "--fixtures",
            fixtures.to_str().unwrap(),
            "--mode",
            "supervised",
            "--execution",
            execution,
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
}

// --- store-side helpers (open/use/drop between requests; the serve does the same) ---

struct WallClock;
impl graphhelm_protocols::Clock for WallClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
    }
}
#[derive(Default)]
struct Ids(std::sync::atomic::AtomicU64);
impl graphhelm_protocols::IdGenerator for Ids {
    fn next_id(&self, prefix: &'static str) -> String {
        format!(
            "{prefix}-wake-{}",
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1
        )
    }
}

fn open_store(events: &Path) -> graphhelm_events::LocalEventRepository {
    graphhelm_events::LocalEventRepository::open(
        events,
        Arc::new(WallClock),
        Arc::new(Ids::default()),
    )
    .unwrap()
}

/// Arms a lease by direct store append (the API surface for arming is Task 3; the ring is
/// this task's subject and must work from the fold alone).
fn arm_lease(events: &Path, execution: &str, rendezvous_id: &str, cursor: u64) {
    arm_lease_bounded(events, execution, rendezvous_id, cursor, None);
}

fn arm_lease_bounded(
    events: &Path,
    execution: &str,
    rendezvous_id: &str,
    cursor: u64,
    matures_in_seconds: Option<u64>,
) {
    let store = open_store(events);
    let (stream, _events) = store.read_unique_replay_stream().unwrap();
    let next = store
        .next_sequence(&stream.scope, &stream.stream_id)
        .unwrap();
    let request = graphhelm_events::PreparedAppend::new(
        stream.scope.clone(),
        graphhelm_protocols::OpaqueId::parse(stream.stream_id.clone()).unwrap(),
        next,
        vec![graphhelm_protocols::NewEvent::new(
            graphhelm_protocols::OpaqueId::parse(format!("arm-{rendezvous_id}")).unwrap(),
            graphhelm_protocols::PersistedActor::new(
                graphhelm_protocols::PersistedActorType::Agent,
                graphhelm_protocols::ActorId::parse("agent-sleeper").unwrap(),
            ),
            graphhelm_protocols::Sensitivity::Internal,
            graphhelm_protocols::EventKind::WakeLease(graphhelm_protocols::WakeLease {
                execution_id: graphhelm_protocols::OpaqueId::parse(execution).unwrap(),
                session_id: graphhelm_protocols::OpaqueId::parse("session-sleeper-1").unwrap(),
                cursor,
                rendezvous_id: graphhelm_protocols::OpaqueId::parse(rendezvous_id).unwrap(),
                matures_in_seconds,
            }),
            vec![],
            vec![],
        )],
        vec![],
        vec![],
    )
    .unwrap();
    store.append_atomic(&request).unwrap();
}

fn head(events: &Path) -> u64 {
    let store = open_store(events);
    let (_stream, history) = store.read_unique_replay_stream().unwrap();
    history.last().map_or(0, |event| event.sequence)
}

/// The instant the LAST event actually recorded — the base the horizon is computed from.
///
/// Reading it back beats recomputing it here: a test that asserts against its own `now` would
/// pass for an implementation that measured from a different base, and the base is the whole
/// question.
fn last_event_instant(events: &Path) -> chrono::DateTime<chrono::Utc> {
    let store = open_store(events);
    let (_stream, history) = store.read_unique_replay_stream().unwrap();
    *history
        .last()
        .expect("the stream has at least the execution start")
        .occurred_at
        .as_datetime()
}

#[cfg(windows)]
fn kinds_after(events: &Path, sequence: u64) -> Vec<String> {
    let store = open_store(events);
    let (_stream, history) = store.read_unique_replay_stream().unwrap();
    history
        .iter()
        .filter(|event| event.sequence > sequence)
        .map(|event| {
            serde_json::to_value(&event.kind).unwrap()["type"]
                .as_str()
                .unwrap_or("?")
                .to_owned()
        })
        .collect()
}

/// Which of three states the store is in when a ring that was required did not arrive (#514).
///
/// The failure this answers looked like this and said nothing:
///
/// ```text
/// assertion `left == right` failed: the first trigger rings
///   left: 0
///  right: 1
/// ```
///
/// The POST returned 200, so the server ACCEPTED the signal, and then zero bytes crossed. That
/// leaves two mechanisms a reader cannot tell apart from `left: 0` -- the ring fired and the
/// sleeper missed it, or no ring was ever attempted -- and they have opposite causes. Raising the
/// sleeper's wait bound would make the red rarer without making it better, which is the mistake
/// #386 already made once; the artefact that decides it is the store, and it is right there at
/// panic time.
///
/// A SEAM that takes the kinds as a value, so its own cells feed it constructed states rather than
/// racing a 1-in-10 flake into existence. It is deliberately not `#[cfg(windows)]`: the reading is
/// a property of the event kinds, and its cells are worth running on every platform even though
/// only Windows has the rendezvous.
///
/// Three states rather than two. The third -- no `signal_recorded` at all -- is separated because
/// it is a claim about something else entirely: the POST was answered 200 and left no durable
/// trace, which is not a wake defect and must not be reported as one.
///
/// **PRECONDITION, and it is not satisfied everywhere.** The reading distinguishes the two wake
/// states by whether the lease consumption is DURABLE, so it is only sound where phase 3 has had
/// time to land. At the two call sites it is used from, the reading is taken after
/// `Sleeper::wait` returned -- which on the failing path means its own ten-second bound expired
/// first -- so phase 3 had ten seconds and an absent consumption really means no ring.
///
/// `a_designed_phase3_delay_is_absorbed_by_the_receipt_wait` VIOLATES that precondition on
/// purpose: it sets `GRAPHHELM_TEST_WAKE_PHASE3_DELAY_MS=2000`, so at the instant of its ring
/// there is no consumption in the store BY DESIGN. This diagnosis is deliberately not wired into
/// that test's assertion, because there it would read the designed delay as "no ring was
/// attempted" and say so confidently. A diagnosis that is wrong is worse than the bare
/// `left: 0, right: 1` it replaces: that one at least does not send anyone anywhere.
fn ring_absence_reading(kinds: &[String]) -> &'static str {
    if kinds.iter().any(|kind| kind == "wake_lease_consumed") {
        "the lease WAS consumed, so a ring was attempted and the sleeper did not receive it (a rendezvous problem, not a trigger problem)"
    } else if kinds.iter().any(|kind| kind == "signal_recorded") {
        "the signal is durable and the lease was NOT consumed, so no ring was attempted (a trigger problem, not a rendezvous problem)"
    } else {
        "neither the signal nor a lease consumption is in the store, so a POST answered 200 left no durable trace at all -- which is not a wake defect"
    }
}

/// The reading above, taken from the store at the moment an expected ring is missing.
///
/// Called ONLY from the failing branch of an assertion: `assert_eq!` evaluates its format
/// arguments after the comparison fails, so a passing run pays nothing for this.
#[cfg(windows)]
fn missing_ring_diagnosis(events: &Path, since: u64) -> String {
    let kinds = kinds_after(events, since);
    format!(
        "{} -- kinds after sequence {since}: {kinds:?}",
        ring_absence_reading(&kinds)
    )
}

/// The three readings are DIFFERENT, which is the only thing that makes the diagnosis worth
/// printing. A version that returned one sentence for every state would still satisfy an
/// assertion that merely checks a message appears.
#[test]
fn the_ring_absence_reading_separates_the_three_states() {
    let consumed = ring_absence_reading(&[
        "signal_recorded".to_owned(),
        "wake_lease_consumed".to_owned(),
    ]);
    let recorded = ring_absence_reading(&["signal_recorded".to_owned()]);
    let neither = ring_absence_reading(&[]);

    assert!(
        consumed.contains("rendezvous problem"),
        "a consumed lease means the ring was attempted: {consumed}"
    );
    assert!(
        recorded.contains("trigger problem"),
        "a durable signal with no consumption means no ring was attempted: {recorded}"
    );
    assert!(
        neither.contains("no durable trace"),
        "an empty store after an accepted POST is neither of the wake states: {neither}"
    );
    assert!(
        consumed != recorded && recorded != neither && consumed != neither,
        "the three readings must differ, or the diagnosis names a state without distinguishing it"
    );
}

/// A lease consumption decides the reading even when the kinds arrive in the other order, and
/// unrelated kinds do not.
#[test]
fn the_ring_absence_reading_is_about_the_two_kinds_and_not_their_order() {
    let reversed = ring_absence_reading(&[
        "wake_lease_consumed".to_owned(),
        "signal_recorded".to_owned(),
    ]);
    assert!(
        reversed.contains("rendezvous problem"),
        "the consumption decides regardless of order: {reversed}"
    );
    let unrelated =
        ring_absence_reading(&["execution_started".to_owned(), "node_ready".to_owned()]);
    assert!(
        unrelated.contains("no durable trace"),
        "kinds that are neither of the two are not a wake state: {unrelated}"
    );
}

/// The sleeper half: creates the platform rendezvous for `rendezvous_id` (the Task 0/1
/// derivation: a fixed local prefix plus the opaque id) and returns a handle whose
/// `wait(timeout)` blocks for the ring, returning the bytes received.
#[cfg(windows)]
struct Sleeper {
    rendezvous_id: String,
    handle: std::thread::JoinHandle<(Vec<u8>, Vec<String>)>,
}

#[cfg(windows)]
impl Sleeper {
    /// `events`: on the INSTANT the byte arrives, the sleeper snapshots the store's event
    /// kinds — the honest detector for "a ring implies a durable trigger" (checking after
    /// the HTTP response returns would be blind to an early ring).
    fn arm(logical_rendezvous_id: &str, events: &Path) -> Self {
        let rendezvous_id = fixture_scoped_rendezvous_id(logical_rendezvous_id, events);
        let name = format!(r"\\.\pipe\graphhelm-wake-{rendezvous_id}");
        let events = events.to_path_buf();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                use tokio::io::AsyncReadExt;
                let mut server = tokio::net::windows::named_pipe::ServerOptions::new()
                    .first_pipe_instance(true)
                    .max_instances(1)
                    .create(&name)
                    .expect("the sleeper creates its rendezvous");
                ready_tx.send(()).unwrap();
                // Both phases bounded: an absent ringer must yield an empty result, never
                // a hung test (the no-ring cases DEPEND on this timing out).
                match tokio::time::timeout(Duration::from_secs(10), server.connect()).await {
                    Ok(Ok(())) => {}
                    _ => return (Vec::new(), Vec::new()),
                }
                let mut buffer = [0_u8; 8];
                match tokio::time::timeout(Duration::from_secs(10), server.read(&mut buffer)).await
                {
                    Ok(Ok(read)) => {
                        // The instant of the ring: snapshot what is durable RIGHT NOW.
                        let at_ring = kinds_snapshot(&events);
                        (buffer[..read].to_vec(), at_ring)
                    }
                    _ => (Vec::new(), Vec::new()),
                }
            })
        });
        ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        Self {
            rendezvous_id,
            handle,
        }
    }

    /// Creates the fixture-scoped physical rendezvous and persists that exact id in its lease.
    /// Callers provide only the stable logical id, so a new test cannot accidentally arm a
    /// machine-global fixed pipe while another runner owns it.
    fn arm_lease(logical_rendezvous_id: &str, events: &Path, execution: &str, cursor: u64) -> Self {
        let sleeper = Self::arm(logical_rendezvous_id, events);
        arm_lease(events, execution, sleeper.rendezvous_id(), cursor);
        sleeper
    }

    fn rendezvous_id(&self) -> &str {
        &self.rendezvous_id
    }

    fn wait(self) -> (Vec<u8>, Vec<String>) {
        self.handle.join().unwrap()
    }
}

#[cfg(windows)]
fn serve_before_arming_sleeper(
    events: &Path,
    execution: &str,
    logical_rendezvous_id: &str,
    env: &[(&str, &str)],
) -> (ServerGuard, String, String, Sleeper) {
    // Starting the child can exceed the sleeper's strict ten-second ring budget under workspace
    // load. Make child readiness a setup precondition so that budget measures only the wake path.
    let (guard, address, token) = serve_with_env(events, env);
    let sleeper = Sleeper::arm_lease(logical_rendezvous_id, events, execution, head(events));
    (guard, address, token, sleeper)
}

#[cfg(windows)]
/// Keeps a logical re-arm stable inside one fixture while isolating the machine-global named-pipe
/// namespace from parallel fixtures in this process and from another test-process runner. Hashing
/// the store path avoids exposing the user's temporary path in either the lease or the pipe name.
fn fixture_scoped_rendezvous_id(logical_id: &str, events: &Path) -> String {
    use std::os::windows::ffi::OsStrExt as _;

    let mut digest = Sha256::new();
    digest.update(b"graphhelm-wake-test-fixture-v1\0");
    for unit in events.as_os_str().encode_wide() {
        digest.update(unit.to_le_bytes());
    }
    let fixture = hex::encode(digest.finalize());
    format!(
        "{logical_id}-runner-{}-{}",
        std::process::id(),
        &fixture[..16]
    )
}

#[cfg(windows)]
fn wake_wait_fixture_rendezvous_id(logical_id: &str, events: &Path) -> String {
    fixture_scoped_rendezvous_id(logical_id, events)
}

#[cfg(windows)]
struct HeldRendezvous {
    release: Option<std::sync::mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(windows)]
impl HeldRendezvous {
    fn new(rendezvous_id: &str) -> Self {
        let name = format!(r"\\.\pipe\graphhelm-wake-{rendezvous_id}");
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let _server = tokio::net::windows::named_pipe::ServerOptions::new()
                    .first_pipe_instance(true)
                    .max_instances(1)
                    .create(&name)
                    .expect("the old generation owns its rendezvous");
                ready_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            });
        });
        ready_rx.recv().unwrap();
        Self {
            release: Some(release_tx),
            thread: Some(thread),
        }
    }
}

#[cfg(windows)]
impl Drop for HeldRendezvous {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// How many scans of the pipe namespace the positive control may take before it is a red.
#[cfg(windows)]
const PIPE_ENUMERATION_ATTEMPTS: usize = 20;

/// Scan the named-pipe namespace until a snapshot contains `expected`, at most `attempts`
/// times. A single directory scan of `//./pipe` is not an atomic listing: under a loaded
/// suite it can miss a pipe that is held open for the whole scan (#955). The bound is
/// declared, and exhausting it is a red that names every attempt it made.
fn pipe_snapshot_containing(
    expected: &str,
    attempts: usize,
    mut enumerate: impl FnMut() -> std::io::Result<Vec<String>>,
) -> Result<Vec<String>, String> {
    let mut seen = Vec::new();
    for attempt in 0..attempts {
        match enumerate() {
            Ok(names) => {
                if names.iter().any(|name| name.eq_ignore_ascii_case(expected)) {
                    return Ok(names);
                }
                seen.push(format!(
                    "attempt {attempt}: {} entries, target absent",
                    names.len()
                ));
            }
            Err(error) => seen.push(format!("attempt {attempt}: enumeration failed: {error}")),
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    Err(format!(
        "{expected} not seen in {attempts} scans: [{}]",
        seen.join("; ")
    ))
}

#[test]
fn a_pipe_scan_that_misses_a_held_pipe_is_retried_not_believed() {
    let mut script = vec![
        Ok(vec!["foreign".to_owned()]),
        Err(std::io::Error::other("scan interrupted")),
        Ok(vec!["foreign".to_owned(), "GraphHelm-Wake-Held".to_owned()]),
    ]
    .into_iter();
    let mut calls = 0;
    let snapshot = pipe_snapshot_containing("graphhelm-wake-held", 5, || {
        calls += 1;
        script
            .next()
            .expect("the scan stops once the target is seen")
    });
    assert_eq!(
        snapshot.as_deref().map(<[String]>::len),
        Ok(2),
        "a scan that missed the held pipe once must be retried, not believed: {snapshot:?}"
    );
    assert_eq!(
        calls, 3,
        "the scan stops at the first snapshot that sees the target"
    );

    let exhausted = pipe_snapshot_containing("graphhelm-wake-held", 3, || Ok(Vec::new()));
    let diagnosis = exhausted.expect_err("an absent target stays a red after the bound");
    assert!(
        diagnosis.contains("not seen in 3 scans") && diagnosis.contains("attempt 2"),
        "exhausting the bound names every attempt: {diagnosis}"
    );
}

#[cfg(windows)]
#[test]
fn an_old_fixture_pipe_cannot_satisfy_a_new_wake_wait_observer() {
    let old_directory = tempfile::tempdir().unwrap();
    let new_directory = tempfile::tempdir().unwrap();
    let logical_id = "rvz-wake-wait-generation";
    let old_id = wake_wait_fixture_rendezvous_id(logical_id, &old_directory.path().join("events"));
    let new_id = wake_wait_fixture_rendezvous_id(logical_id, &new_directory.path().join("events"));
    let _old_generation = HeldRendezvous::new(&old_id);

    let old_expected = format!("graphhelm-wake-{old_id}");
    let new_expected = format!("graphhelm-wake-{new_id}");
    let names = pipe_snapshot_containing(&old_expected, PIPE_ENUMERATION_ATTEMPTS, || {
        // The namespace is global and volatile: an unrelated entry may disappear between
        // enumeration and inspection, and a directory scan of the pipe namespace can miss
        // a live entry while other tests create and drop pipes. The held old pipe is the
        // fail-closed positive control; only a snapshot that saw it is evidence about ids.
        std::fs::read_dir("//./pipe").map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
    })
    .unwrap_or_else(|diagnosis| {
        panic!(
            "positive control: the observer must see the old generation's held pipe; {diagnosis}"
        )
    });

    assert!(
        !names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&new_expected)),
        "a pipe held by the old fixture generation must not satisfy the new generation's observer"
    );
}

#[cfg(windows)]
fn kinds_snapshot(events: &Path) -> Vec<String> {
    let store = open_store(events);
    let (_stream, history) = store.read_unique_replay_stream().unwrap();
    history
        .iter()
        .map(|event| {
            serde_json::to_value(&event.kind).unwrap()["type"]
                .as_str()
                .unwrap_or("?")
                .to_owned()
        })
        .collect()
}

#[cfg(windows)]
#[test]
fn an_append_beyond_the_cursor_rings_one_byte_only_after_the_trigger_is_durable() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    start_execution(&events, directory.path(), "exec-wake-ring");
    let (_guard, address, token, sleeper) =
        serve_before_arming_sleeper(&events, "exec-wake-ring", "rvz-ring-1", &[]);

    let before = head(&events);
    let (status, reply) = post_json(
        &address,
        &token,
        "/v1/executions/exec-wake-ring/signal",
        "wake-trigger-1",
        &signal_body("signal-wake-1", &directory.path().join("ev.json")),
    );
    assert_eq!(status, 200, "{reply}");

    // The ring: exactly one byte, and AT THE INSTANT IT ARRIVES the trigger is durable
    // (the sleeper snapshots the store from inside its own read completion).
    let (bytes, at_ring) = sleeper.wait();
    assert_eq!(
        bytes.len(),
        1,
        "exactly one content-free byte crossed -- {}",
        missing_ring_diagnosis(&events, before)
    );
    assert!(
        at_ring.iter().any(|kind| kind == "signal_recorded"),
        "a ring implies a durable trigger — the append must be readable at the instant \
         the byte arrives: {at_ring:?}"
    );
    let _ = before;

    // The consumption lands (two-phase: the true reason is known only after the ring).
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let kinds = kinds_after(&events, before);
        if kinds.iter().any(|kind| kind == "wake_lease_consumed") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the consumption must land: {kinds:?}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(windows)]
#[test]
fn sleeper_arm_isolates_concurrent_fixtures_and_reuses_a_logical_rearm() {
    let first_directory = tempfile::tempdir().unwrap();
    let first_events = first_directory.path().join("events");
    start_execution(
        &first_events,
        first_directory.path(),
        "exec-runner-isolation-a",
    );
    let second_directory = tempfile::tempdir().unwrap();
    let second_events = second_directory.path().join("events");
    start_execution(
        &second_events,
        second_directory.path(),
        "exec-runner-isolation-b",
    );

    let logical_id = "rvz-runner-collision";
    let first = Sleeper::arm(logical_id, &first_events);
    let first_physical_id = first.rendezvous_id().to_owned();
    let second = Sleeper::arm(logical_id, &second_events);
    let second_physical_id = second.rendezvous_id().to_owned();
    assert_ne!(
        first_physical_id, logical_id,
        "the helper must scope the real pipe"
    );
    assert_ne!(
        first_physical_id, second_physical_id,
        "parallel fixtures in one test process must own different physical pipes"
    );
    ring_pipe(&first_physical_id, b"a").unwrap();
    ring_pipe(&second_physical_id, b"b").unwrap();
    assert_eq!(first.wait().0, b"a");
    assert_eq!(second.wait().0, b"b");

    let rearmed = Sleeper::arm(logical_id, &first_events);
    assert_eq!(
        rearmed.rendezvous_id(),
        first_physical_id,
        "a logical re-arm inside one fixture must target the same physical pipe"
    );
    ring_pipe(rearmed.rendezvous_id(), b"c").unwrap();
    assert_eq!(rearmed.wait().0, b"c");
}

/// Windows `ERROR_PIPE_BUSY`. Named because the #413 report identifies the failure by this
/// number, and a bare 231 in an assertion cannot be matched against that report by a reader.
#[cfg(windows)]
const ERROR_PIPE_BUSY: i32 = 231;

/// A client handle held open across the sleeper's own release. Dropping it closes the client
/// FIRST and the runtime second, which is the order the kernel needs to see.
#[cfg(windows)]
struct HeldClient {
    // Never read: both fields exist only to be dropped, and dropping is the whole point. Order
    // is load-bearing -- fields drop in declaration order, so the client closes before the
    // runtime that drives it.
    _client: tokio::net::windows::named_pipe::NamedPipeClient,
    _runtime: tokio::runtime::Runtime,
}

/// Rings the rendezvous and KEEPS the client handle open. `ring_pipe` drops its client before
/// returning, which is exactly the synchronisation the real sidecar does not offer: there the
/// client belongs to the `serve` child process and closes on that process's schedule.
#[cfg(windows)]
fn ring_pipe_and_hold(rendezvous_id: &str, payload: &[u8]) -> HeldClient {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let name = format!(r"\\.\pipe\graphhelm-wake-{rendezvous_id}");
    let payload = payload.to_vec();
    let client = runtime.block_on(async move {
        use tokio::io::AsyncWriteExt;
        let mut client = tokio::net::windows::named_pipe::ClientOptions::new()
            .open(&name)
            .expect("the held client opens the rendezvous");
        client.write_all(&payload).await.unwrap();
        client.flush().await.unwrap();
        client
    });
    HeldClient {
        _client: client,
        _runtime: runtime,
    }
}

/// The MEASUREMENT behind #413(A), and it REFUTES the mechanism it was written to check.
///
/// The ticket credits, as INFERRED, that "a named-pipe instance only disappears when ALL handles
/// close", so a re-arm races the `serve` child's client handle. This puts the system in exactly
/// that state -- server handle dropped, client handle still open -- and finds the opposite: the
/// name is already delisted and a fresh create on it succeeds.
///
/// **Read the direction carefully. This test is GREEN when the rendezvous does NOT survive.** Its
/// name says so, because a test named for the hypothesis while asserting the refutation is a
/// vacuous green wearing the word "proof" -- the mirror of the vacuous red, and just as blind.
/// A failure here means the inference was right after all and the symmetric wait #413(A) asks for
/// is the correct cure; the assertion messages carry that reading rather than leaving it to me.
#[cfg(windows)]
#[test]
fn the_rendezvous_does_not_outlive_the_sleeper_even_while_a_client_holds_it() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    start_execution(&events, directory.path(), "exec-rearm-mechanism");
    let sleeper = Sleeper::arm("rvz-rearm-mechanism", &events);
    let rendezvous_id = sleeper.rendezvous_id().to_owned();

    let held = ring_pipe_and_hold(&rendezvous_id, b"x");
    assert_eq!(
        sleeper.wait().0,
        b"x",
        "the sleeper read the byte, so its own server handle is now dropped"
    );

    // DISCRIMINATOR (#413(A)): two instruments asked the same question at the same instant.
    // Enumeration answers "is the NAME listed"; create answers "is the INSTANCE there". If they
    // disagree, a wait built on enumeration cannot see the condition it is supposed to wait out.
    let listed = pipe_is_present(&rendezvous_id);
    let name = format!(r"\\.\pipe\graphhelm-wake-{rendezvous_id}");
    let created = {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            tokio::net::windows::named_pipe::ServerOptions::new()
                .first_pipe_instance(true)
                .max_instances(1)
                .create(&name)
                .map(|_| ())
                .map_err(|error| (error.raw_os_error(), error.to_string()))
        })
    };
    assert!(
        !listed,
        "the rendezvous {rendezvous_id} is still listed after its server handle closed, so the \
         instance really can outlive the sleeper and #413(A)'s inferred mechanism is live after all"
    );
    assert!(
        created.is_ok(),
        "creating {name} while a client still holds the old instance failed with {created:?}. \
         #413(A) infers exactly this, so if it ever fires the inference is CORRECT and the \
         symmetric wait it asks for is the right cure -- this test is the thing that would \
         have told us, and it must be read before the acceptance item is retired"
    );

    // The client is released only here, AFTER both readings, so neither of them can be
    // explained by the client having quietly gone away first.
    drop(held);
}

/// The other half of the discriminator, and the one that names the real precondition.
///
/// The test above shows a lingering CLIENT does not block a create. This one shows what does: a
/// live SERVER on the same name. `ERROR_PIPE_BUSY` is therefore evidence of two servers sharing a
/// name at the same instant -- two runners, two fixtures, or two concurrent arms -- and never of
/// a handle that has not finished closing. That is why the cure is identity separation (the
/// per-runner, per-fixture id) and not a wait for an instance to drain.
#[cfg(windows)]
#[test]
fn a_second_server_on_a_live_name_is_what_reports_pipe_busy() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    start_execution(&events, directory.path(), "exec-rearm-busy");
    let sleeper = Sleeper::arm("rvz-rearm-busy", &events);
    let rendezvous_id = sleeper.rendezvous_id().to_owned();

    // Positive control first: while that sleeper is ALIVE, a second server on its exact name is
    // refused, and refused with the specific code the #413 report carried.
    let name = format!(r"\\.\pipe\graphhelm-wake-{rendezvous_id}");
    let refusal = {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            tokio::net::windows::named_pipe::ServerOptions::new()
                .first_pipe_instance(true)
                .max_instances(1)
                .create(&name)
                .map(|_| ())
                .map_err(|error| error.raw_os_error())
        })
    };
    assert_eq!(
        refusal,
        Err(Some(ERROR_PIPE_BUSY)),
        "a second server on the live name {name} was not refused with ERROR_PIPE_BUSY, so this \
         test no longer reproduces the condition #413 reported and cannot speak for its cause"
    );

    // Release the subject the honest way, so the fixture leaves nothing behind.
    ring_pipe(&rendezvous_id, b"z").unwrap();
    assert_eq!(sleeper.wait().0, b"z");
}

#[cfg(windows)]
#[test]
fn a_burned_lease_never_rings_twice_and_no_ring_without_a_fresh_append() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    start_execution(&events, directory.path(), "exec-wake-burn");
    let logical_rendezvous_id = "rvz-burn-1";
    let (_guard, address, token, sleeper) =
        serve_before_arming_sleeper(&events, "exec-wake-burn", logical_rendezvous_id, &[]);
    // Read BEFORE the POST, because the diagnosis below is about what the signal added and a
    // sequence taken afterwards would include it (#514).
    let before_first_signal = head(&events);

    // First trigger rings and burns.
    let (status, _reply) = post_json(
        &address,
        &token,
        "/v1/executions/exec-wake-burn/signal",
        "wake-burn-1",
        &signal_body("signal-burn-1", &directory.path().join("ev1.json")),
    );
    assert_eq!(status, 200);
    assert_eq!(
        sleeper.wait().0.len(),
        1,
        "the first trigger rings -- {}",
        missing_ring_diagnosis(&events, before_first_signal)
    );

    // Re-arm the exact runner-scoped PIPE but not the lease: a second trigger must NOT ring
    // (lease burned). A concurrent runner has a different physical name; both arms here do not.
    let second = Sleeper::arm(logical_rendezvous_id, &events);
    let (status, _reply) = post_json(
        &address,
        &token,
        "/v1/executions/exec-wake-burn/signal",
        "wake-burn-2",
        &signal_body("signal-burn-2", &directory.path().join("ev2.json")),
    );
    assert_eq!(status, 200);
    assert_eq!(
        second.wait().0,
        Vec::<u8>::new(),
        "a burned lease never rings twice"
    );
}

#[cfg(windows)]
#[test]
fn a_missing_rendezvous_consumes_the_lease_without_a_serve_error() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    start_execution(&events, directory.path(), "exec-wake-stale");
    let armed_at = head(&events);
    // Lease armed, but NO pipe exists (the sleeper died).
    arm_lease(&events, "exec-wake-stale", "rvz-stale-1", armed_at);
    let (_guard, address, token) = serve(&events);

    let before = head(&events);
    let (status, reply) = post_json(
        &address,
        &token,
        "/v1/executions/exec-wake-stale/signal",
        "wake-stale-1",
        &signal_body("signal-stale-1", &directory.path().join("ev.json")),
    );
    assert_eq!(
        status, 200,
        "a wake failure must never fail the append: {reply}"
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let kinds = kinds_after(&events, before);
        if kinds.iter().any(|kind| kind == "wake_lease_consumed") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the stale lease must be consumed as honest cleanup: {kinds:?}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

// ---------------------------------------------------------------------------------------------
// Task 4: the sidecar — `graphhelm wake-wait` blocks for free and content never crosses.
// ---------------------------------------------------------------------------------------------

#[cfg(windows)]
const TEST_WAKE_CAUSAL_TRANSCRIPT_ENV: &str = "GRAPHHELM_TEST_WAKE_CAUSAL_TRANSCRIPT";
#[cfg(windows)]
const TEST_WAKE_CAUSAL_TRANSCRIPT_MAX_BYTES: usize = 4 * 1024;

/// Spawns `graphhelm wake-wait` and returns the child (the sidecar CREATES the rendezvous).
#[cfg(windows)]
/// The waiter no longer takes a rendezvous or a deadline from its caller: both come from the
/// lease this session armed. So the harness arms one, and the test's "timeout" is now the
/// bound the sleeper DECLARED — which is the point of the step.
fn spawn_wake_wait(
    events: &Path,
    execution: &str,
    session: &str,
    logical_rendezvous_id: &str,
) -> Child {
    spawn_wake_wait_with_env(events, execution, session, logical_rendezvous_id, &[])
}

#[cfg(windows)]
fn spawn_wake_wait_with_env(
    events: &Path,
    execution: &str,
    session: &str,
    logical_rendezvous_id: &str,
    env: &[(&str, &str)],
) -> Child {
    let expected = wake_wait_fixture_rendezvous_id(logical_rendezvous_id, events);
    let store = open_store(events);
    let (stream, history) = store.read_unique_replay_stream().unwrap();
    let projection = graphhelm_events::replay(&stream.scope, &stream.stream_id, &history).unwrap();
    let lease = projection
        .wake_leases
        .get(session)
        .unwrap_or_else(|| panic!("{session} owns no wake lease before its waiter starts"));
    assert_eq!(
        lease.rendezvous_id, expected,
        "wake-wait fixtures must persist their fixture-scoped physical rendezvous before spawn"
    );
    drop(store);

    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
    command
        .args([
            "wake-wait",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            execution,
            "--session-id",
            session,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.env_remove(TEST_WAKE_CAUSAL_TRANSCRIPT_ENV);
    for (key, value) in env {
        command.env(key, value);
    }
    command.spawn().unwrap()
}

/// Owns a wake-wait child while setup and ringing can still panic. `Child` does not terminate its
/// process on drop, so every failure before the final output collection must explicitly kill and
/// reap the sidecar or it can retain the test binary and event files.
#[cfg(windows)]
type WakeWaitReapResult = (
    std::io::Result<()>,
    std::io::Result<std::process::ExitStatus>,
);

#[cfg(windows)]
struct WakeWaitChildGuard {
    child: Option<Child>,
    reaped: Option<std::sync::mpsc::Sender<WakeWaitReapResult>>,
}

#[cfg(windows)]
impl WakeWaitChildGuard {
    fn new(child: Child) -> Self {
        Self {
            child: Some(child),
            reaped: None,
        }
    }

    fn child_mut(&mut self) -> &mut Child {
        self.child.as_mut().expect("the wake-wait child is owned")
    }

    fn wait_with_output(mut self) -> std::io::Result<std::process::Output> {
        use std::io::Read as _;

        let child = self.child_mut();
        let status = child.wait()?;
        let mut stdout = Vec::new();
        if let Some(mut pipe) = child.stdout.take() {
            pipe.read_to_end(&mut stdout)?;
        }
        let mut stderr = Vec::new();
        if let Some(mut pipe) = child.stderr.take() {
            pipe.read_to_end(&mut stderr)?;
        }
        self.child.take();
        Ok(std::process::Output {
            status,
            stdout,
            stderr,
        })
    }

    fn with_reap_observer(
        child: Child,
        reaped: std::sync::mpsc::Sender<WakeWaitReapResult>,
    ) -> Self {
        Self {
            child: Some(child),
            reaped: Some(reaped),
        }
    }
}

#[cfg(windows)]
impl Drop for WakeWaitChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let kill_result = child.kill();
            let wait_result = child.wait();
            if let Some(reaped) = self.reaped.take() {
                let _ = reaped.send((kill_result, wait_result));
            }
        }
    }
}

#[cfg(windows)]
#[test]
fn wake_wait_child_guard_kills_and_reaps_on_early_exit() {
    let child = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 60",
        ])
        .spawn()
        .unwrap();
    let (reaped_tx, reaped_rx) = std::sync::mpsc::channel();
    let guard = WakeWaitChildGuard::with_reap_observer(child, reaped_tx);

    let cleanup = std::thread::spawn(move || drop(guard));

    let (kill_result, wait_result) = reaped_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("dropping the guard kills and reaps within the cleanup bound");
    kill_result.expect("dropping the guard successfully kills the live child");
    let status = wait_result.expect("dropping the guard successfully reaps the child");
    assert!(
        !status.success(),
        "the live child was killed before reaping"
    );
    cleanup.join().expect("the cleanup worker does not panic");
}

/// Reaps a sidecar and returns its exit status together with everything it wrote to
/// stderr. The stderr pipe was created at spawn and then DISCARDED, so an exit-2 refusal
/// (the sidecar's own diagnosis) was invisible and masqueraded as whatever assertion
/// failed downstream. Reading after `wait` cannot deadlock here: the sidecar's stderr is
/// at most a refusal line, far below the pipe buffer.
#[cfg(windows)]
fn reap_with_diagnosis(child: &mut Child) -> (std::process::ExitStatus, String) {
    let status = child.wait().unwrap();
    let mut reported = String::new();
    if let Some(mut pipe) = child.stdout.take() {
        use std::io::Read as _;
        let _ = pipe.read_to_string(&mut reported);
    }
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        use std::io::Read as _;
        let _ = pipe.read_to_string(&mut stderr);
    }
    if !stderr.trim().is_empty() {
        reported.push_str(" | stderr: ");
        reported.push_str(stderr.trim());
    }
    (status, reported)
}

/// Rings the sidecar's rendezvous with the given bytes (a HOSTILE ringer may write more
/// than one and none of it may surface).
#[cfg(windows)]
fn ring_pipe(rendezvous_id: &str, payload: &[u8]) -> std::io::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let name = format!(r"\\.\pipe\graphhelm-wake-{rendezvous_id}");
    let payload = payload.to_vec();
    runtime.block_on(async move {
        use tokio::io::AsyncWriteExt;
        let mut client = tokio::net::windows::named_pipe::ClientOptions::new().open(&name)?;
        client.write_all(&payload).await
    })
}

#[cfg(windows)]
#[test]
fn wake_wait_exits_zero_on_ring_and_no_hostile_byte_reaches_stdout() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    start_execution(&events, directory.path(), "exec-sidecar-ring");
    let logical_rendezvous_id = "rvz-sidecar-1";
    let rendezvous_id = wake_wait_fixture_rendezvous_id(logical_rendezvous_id, &events);
    arm_lease_bounded(
        &events,
        "exec-sidecar-ring",
        &rendezvous_id,
        1,
        Some(RACING_WAKE_LEASE_SECONDS),
    );
    let child = spawn_wake_wait(
        &events,
        "exec-sidecar-ring",
        "session-sleeper-1",
        logical_rendezvous_id,
    );
    let mut child = WakeWaitChildGuard::new(child);
    // Observe the real rendezvous while also proving that its child is still alive. The shared
    // hang catcher is deliberately below the lease, so a loaded but healthy start is not retried
    // into success and a dead child fails with its own diagnosis.
    wait_for_pipe(child.child_mut(), &rendezvous_id);
    ring_pipe(&rendezvous_id, b"SENTINEL-HOSTILE-PAYLOAD")
        .expect("the observed live rendezvous accepts the hostile ring");
    let started = Instant::now();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0), "ring exits 0: {output:?}");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the exit follows the ring promptly"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stdout.contains("SENTINEL") && !stderr.contains("SENTINEL"),
        "content never crosses the sidecar: {stdout} {stderr}"
    );
}

#[cfg(windows)]
#[test]
fn wake_wait_exits_three_on_timeout_and_two_on_a_bad_id() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    start_execution(&events, directory.path(), "exec-sidecar-timeout");
    let logical_rendezvous_id = "rvz-sidecar-timeout";
    let rendezvous_id = wake_wait_fixture_rendezvous_id(logical_rendezvous_id, &events);
    // The bound is DECLARED on the lease now, so exit 3 can only mean the deadline the sleeper
    // itself set. It stopped being "the number I happened to type ran out".
    arm_lease_bounded(&events, "exec-sidecar-timeout", &rendezvous_id, 1, Some(1));
    let child = spawn_wake_wait(
        &events,
        "exec-sidecar-timeout",
        "session-sleeper-1",
        logical_rendezvous_id,
    );
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(3), "timeout exits 3: {output:?}");

    // The unusable case is no longer a malformed id from the caller — the id comes from the
    // lease. It is a session with no lease of its own, which refuses rather than waiting on
    // whatever it found.
    let output = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args([
            "wake-wait",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            "exec-sidecar-timeout",
            "--session-id",
            "session-nobody",
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(2),
        "a session with no lease refuses: {output:?}"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("GHCLI017"),
        "the refusal carries GHCLI017: {output:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// Task 5: the choreography proof — two sessions, one ring, ZERO polls (measured, not claimed).
// ---------------------------------------------------------------------------------------------

/// A counting TCP proxy in front of the serve: EVERY byte session A sends to the API goes
/// through here, and the connection counter is the measurement the zero-polling assertion
/// reads. The waker (B) talks to the serve directly — only the sleeper is under watch.
#[cfg(windows)]
struct CountingProxy {
    address: String,
    connections: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

#[cfg(windows)]
fn counting_proxy(upstream: String) -> CountingProxy {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    let connections = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let counter = connections.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let upstream = upstream.clone();
            std::thread::spawn(move || {
                let Ok(server) = std::net::TcpStream::connect(&upstream) else {
                    return;
                };
                let client = stream;
                let mut client_reader = client.try_clone().unwrap();
                let mut server_writer = server.try_clone().unwrap();
                let mut server_reader = server;
                let mut client_writer = client;
                let up = std::thread::spawn(move || {
                    let _ = std::io::copy(&mut client_reader, &mut server_writer);
                });
                let _ = std::io::copy(&mut server_reader, &mut client_writer);
                let _ = up.join();
            });
        }
    });
    CountingProxy {
        address,
        connections,
    }
}

/// One MCP session for the sleeper, pointed AT THE PROXY — every API byte it ever sends is
/// counted. Returns the protocol replies.
#[cfg(windows)]
fn mcp_via(
    proxy_address: &str,
    token: &str,
    lines: &[serde_json::Value],
) -> Vec<serde_json::Value> {
    let mut input = String::new();
    for line in lines {
        input.push_str(&line.to_string());
        input.push('\n');
    }
    let output = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args([
            "mcp",
            "--url",
            &format!("http://{proxy_address}"),
            "--actor",
            "agent-sleeper",
        ])
        .env("GRAPHHELM_API_TOKEN", token)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child.stdin.take().unwrap().write_all(input.as_bytes())?;
            child.wait_with_output()
        })
        .unwrap();
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|value| value.get("jsonrpc").is_some())
        .collect()
}

#[cfg(windows)]
fn initialize_lines(calls: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
    let mut lines = vec![
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": "2025-06-18", "capabilities": {},
                       "clientInfo": {"name": "choreo", "version": "0"}}}),
        serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    ];
    lines.extend(calls);
    lines
}

/// §5 as a measured fact: sleeper A arms through the counting proxy and blocks a REAL
/// `wake-wait`; waker B appends a signal talking to the serve DIRECTLY; A's sidecar exits 0;
/// the proxy counted ZERO connections from A in the arm→ring window (no polling anywhere —
/// the sidecar has no URL, no token, and the measurement proves the design rather than
/// trusting it); woken, A re-reads its own log THROUGH the proxy and sees B's event.
#[cfg(windows)]
#[test]
fn a_sleeper_wakes_on_a_peer_append_with_zero_requests_in_the_window() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let execution = "exec-wake-choreo";
    start_execution(&events, directory.path(), execution);
    let (_guard, base, token) = serve(&events);
    let proxy = counting_proxy(base.clone());

    // A arms ITSELF through the proxy FIRST — the waiter now takes its rendezvous and its
    // deadline from its own lease, so the lease has to exist before it can wait on one. Its
    // one read of the store is local and is not an API request, so the measurement below is
    // unchanged: zero requests cross the proxy between sleep and ring.
    let logical_rendezvous = "rdv-choreo-a";
    let rendezvous = wake_wait_fixture_rendezvous_id(logical_rendezvous, &events);
    let replies = mcp_via(
        &proxy.address,
        &token,
        &initialize_lines(vec![serde_json::json!({"jsonrpc": "2.0", "id": 2,
            "method": "tools/call", "params": {"name": "wake_arm",
            "arguments": {"executionId": execution, "rendezvousId": rendezvous,
                          "maturesInSeconds": 30}}})]),
    );
    let armed = &replies[1]["result"];
    assert_eq!(armed["isError"], false, "{replies:?}");
    let armed_reply: serde_json::Value =
        serde_json::from_str(armed["content"][0]["text"].as_str().unwrap()).unwrap();
    let armed_cursor: u64 = armed_reply["data"]["armedCursor"].as_u64().unwrap();
    // The MCP session id is a per-PROCESS nonce (sleeper-only by design, 05g): a later MCP
    // session is a different identity, so a woken sleeper reads its OWN alarm through the
    // API with the id its arm reply handed back — which is exactly what the factory's own
    // agents do.
    let armed_session = armed_reply["data"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();

    // The sidecar waits on THAT session's lease — the one the arm reply named. It reads the
    // store once, locally, and drops the handle before blocking.
    let mut sidecar = spawn_wake_wait(&events, execution, &armed_session, logical_rendezvous);
    // WAIT for the rendezvous to exist rather than sleeping and hoping. The sidecar used to be
    // started before the arming, so it always won the race by construction; now it needs the
    // lease first, and a fixed sleep would be a guess about a cold binary's start-up on a
    // contended machine. It flaked once here before this loop existed -- a fixed sleep is a
    // timing assumption wearing the clothes of a step.
    let appeared = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let expected = format!("graphhelm-wake-{rendezvous}");
    while !std::fs::read_dir("//./pipe").is_ok_and(|entries| {
        entries.filter_map(Result::ok).any(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.eq_ignore_ascii_case(&expected))
        })
    }) {
        // A dead child can never create the pipe: report ITS diagnosis immediately instead
        // of burning the 10 s bound and blaming the pipe. The diagnosis arrives on STDOUT, not
        // stderr -- see `reported_refusal`, which measured this rather than assuming it.
        if let Some(status) = sidecar.try_wait().unwrap() {
            panic!(
                "the sidecar is no longer running and its rendezvous is not present: {status:?}{}",
                reported_refusal(&mut sidecar)
            );
        }
        assert!(
            std::time::Instant::now() < appeared,
            "the sidecar never created its rendezvous"
        );
        std::thread::sleep(std::time::Duration::from_millis(25));
    }

    // The window opens: whatever the proxy has seen so far was the arming.
    let at_sleep = proxy.connections.load(std::sync::atomic::Ordering::SeqCst);

    // B (the waker) appends a signal DIRECTLY at the serve — B is not under measurement.
    let evidence_out = directory.path().join("choreo-evidence.json");
    let (status, reply) = post_json(
        &base,
        &token,
        &format!("/v1/executions/{execution}/signal"),
        "choreo-signal-1",
        &signal_body("signal-choreo-1", &evidence_out),
    );
    assert_eq!(status, 200, "{reply}");

    // The ring: the sidecar exits 0, promptly.
    let started = std::time::Instant::now();
    let (sidecar_end, sidecar_diagnosis) = reap_with_diagnosis(&mut sidecar);
    assert!(
        sidecar_end.success(),
        "the sidecar must exit 0 on the ring: {sidecar_end:?}: {sidecar_diagnosis}"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "the wake is prompt, not a timeout in disguise"
    );

    // THE assertion: zero connections from A between arm and ring — measured.
    let at_wake = proxy.connections.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        at_wake, at_sleep,
        "the sleeper placed ZERO requests while asleep — no polling anywhere, measured"
    );

    // Woken, A re-reads its OWN log from its cursor and sees B's event, attributed.
    let replies = mcp_via(
        &proxy.address,
        &token,
        &initialize_lines(vec![serde_json::json!({"jsonrpc": "2.0", "id": 3,
            "method": "tools/call", "params": {"name": "events",
            "arguments": {"executionId": execution, "after": armed_cursor,
                          "limit": 1000}}})]),
    );
    let tail = &replies[1]["result"];
    assert_eq!(tail["isError"], false, "{replies:?}");
    let envelope: serde_json::Value =
        serde_json::from_str(tail["content"][0]["text"].as_str().unwrap()).unwrap();
    let signal_from_waker = envelope["data"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|event| {
            event["kind"]["type"] == "signal_recorded" && event["actor"]["id"] == "agent-wake-test"
        });
    assert!(
        signal_from_waker,
        "the woken sleeper reads the waker's event from its own cursor: {envelope}"
    );

    // M07 F4: and the sleeper can now ask its OWN alarm what happened. Before this, a
    // woken session saw only `live: false` — indistinguishable from "I never armed" — so
    // it could not tell a ring from a stale burn without reading raw history. The receipt
    // answers in the sleeper's own words: "it rang, at #N".
    //
    // The consumption is two-phase BY DESIGN (serve/wake.rs module doc): the byte may
    // arrive before the consume append is durable, so the receipt is EVENTUALLY visible,
    // not instantly. Wait for the condition — the receipt existing — with a bound, the
    // same shape as the consumption wait above (:376-387). Asserting immediately was a
    // timing assumption wearing the clothes of a step: it failed 9/10 standalone as
    // "the lease burned on the ring" with live:true, lastConsumed:null. (Ringing only
    // AFTER the durable append would make the receipt instant — that is a product
    // decision about wake latency vs receipt strength, routed to the owner separately.)
    let deadline = Instant::now() + Duration::from_secs(10);
    let answer = loop {
        let answer = get_json(
            &base,
            &token,
            &format!("/v1/executions/{execution}/wake-lease?sessionId={armed_session}"),
        );
        if !answer["data"]["lastConsumed"].is_null() {
            break answer;
        }
        assert!(
            Instant::now() < deadline,
            "the receipt must land — the ring already happened (sidecar exited 0), so a \
             receipt that never appears means the consume append was lost: {answer}"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    // (No "must parse" assert here: the loop only breaks on a non-null lastConsumed, so a
    // null answer can no longer reach this line — a guard that cannot fail measures nothing.)
    let data = &answer["data"];
    assert_eq!(
        data["live"], false,
        "the lease burned on the ring: {answer}"
    );
    assert_eq!(
        data["lastConsumed"]["reason"], "rung",
        "the alarm says it RANG — not merely that it is no longer armed: {answer}"
    );
    let rang_at = data["lastConsumed"]["atSequence"]
        .as_u64()
        .expect("the receipt carries the sequence it burned at");
    assert!(
        rang_at > armed_cursor,
        "the ring landed after the arm ({rang_at} > {armed_cursor}): {answer}"
    );
    assert!(
        data["head"].as_u64().expect("head") >= rang_at,
        "the head makes the cursor readable: armed at #{armed_cursor}, rang at #{rang_at}: {answer}"
    );
    // M07 Task 6, from the blind judge's re-judgement: the doorbell rings on CONTENT only
    // (`serve/wake.rs` skips wake bookkeeping), so publishing the raw head alone let the
    // judge read `cursor:13, head:14, lastConsumed:null` and conclude a ring had been lost.
    // It had not — the #14 was the arm's own `wake_lease` event. `contentHead` is the number
    // that actually answers "will I be woken", so both are reported and the arm's own
    // bookkeeping can never masquerade as progress.
    let content_head = data["contentHead"]
        .as_u64()
        .expect("the doorbell's own head is reported");
    assert!(
        content_head <= data["head"].as_u64().expect("head"),
        "content head never exceeds the stream head: {answer}"
    );
}

/// The same distinction with nothing but bookkeeping in flight: arming appends a
/// `wake_lease` event, so the raw head moves while the doorbell's head does not. An
/// operator comparing cursor to head would predict a ring that will never come.
#[test]
fn arming_moves_the_stream_head_but_never_the_doorbells_head() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    start_execution(&events, directory.path(), "exec-content-head");
    let armed_at = head(&events);
    arm_lease(&events, "exec-content-head", "rvz-content-head", armed_at);
    let (_guard, address, token) = serve(&events);

    let answer = get_json(
        &address,
        &token,
        "/v1/executions/exec-content-head/wake-lease?sessionId=session-sleeper-1",
    );
    let data = &answer["data"];
    assert_eq!(
        data["live"], true,
        "this guard is about the LIVE reply — a not-live answer would prove nothing: {answer}"
    );
    let head_now = data["head"].as_u64().expect("head");
    let content_head = data["contentHead"].as_u64().expect("content head");
    assert!(
        head_now > armed_at,
        "the arm's own event moved the stream head: {answer}"
    );
    assert!(
        content_head <= armed_at,
        "but the doorbell's head did not move, so no ring is pending: {answer}"
    );
    assert_eq!(
        data["lastConsumed"],
        serde_json::Value::Null,
        "nothing was consumed, and the surface must not imply otherwise: {answer}"
    );
}

/// The degradation path: the serve dies before anyone appends — the sidecar's timeout fires
/// (exit 3, rotina, not failure) and the sleeper falls back to a plain CLI read of its own
/// store: slow, never wrong (constraint 3: the wake is an accelerator, never a correction).
#[cfg(windows)]
#[test]
fn a_dead_serve_degrades_to_timeout_and_a_plain_read_never_to_wrong() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let execution = "exec-wake-deadman";
    start_execution(&events, directory.path(), execution);
    let (guard, base, token) = serve(&events);
    let logical_rendezvous_id = "rdv-deadman";
    let rendezvous_id = wake_wait_fixture_rendezvous_id(logical_rendezvous_id, &events);
    arm_lease_bounded(&events, execution, &rendezvous_id, 1, Some(2));
    drop(guard); // the serve dies; nothing will ever ring.
    let _ = (base, token);

    let mut sidecar = spawn_wake_wait(
        &events,
        execution,
        "session-sleeper-1",
        logical_rendezvous_id,
    );
    let end = sidecar.wait().unwrap();
    assert_eq!(
        end.code(),
        Some(3),
        "timeout is routine, not failure: {end:?}"
    );

    // The plain read still tells the truth from the store itself.
    let output = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args([
            "execution",
            "status",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            execution,
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["data"]["executionId"], execution, "{value}");
}

// ---------------------------------------------------------------------------------------------
// Hotfix #55: concurrent sweeps must never double-consume a lease. Found live on the
// factory pair store (two sweeps raced read->append; the second consumption had no live
// lease and the fold refused the WHOLE stream on every later replay — the archived
// evidence was a local, never-committed copy under the retired `.factory/`).
// ---------------------------------------------------------------------------------------------

/// Two mutations fired at the same instant (a real barrier, not luck) while ONE lease is
/// live with a dead rendezvous: both sweeps race the read->consume window. Repeated
/// rounds; after every round the stream must still REPLAY (the fold's integrity guard is
/// the oracle) and the armed lease must have been consumed exactly once.
#[test]
fn concurrent_sweeps_never_double_consume_a_lease() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let execution = "exec-wake-race";
    start_execution(&events, directory.path(), execution);
    let (_guard, base, token) = serve(&events);

    for round in 0..15 {
        // Arm with NO pipe: the stale path makes the ring instantaneous, which is the
        // tightest race window. Pipe-first ordering is irrelevant here on purpose.
        arm_lease(&events, execution, &format!("rdv-race-{round}"), 1);
        // Which arming this round IS — read back from the store, never derived by
        // arithmetic (a guard whose expected value can be derived without doing the
        // work is not a guard).
        let arming = head(&events);

        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let posts: Vec<_> = (0..2)
            .map(|lane| {
                let barrier = barrier.clone();
                let base = base.clone();
                let token = token.clone();
                let evidence = directory
                    .path()
                    .join(format!("race-evidence-{round}-{lane}.json"));
                let path = format!("/v1/executions/{execution}/signal");
                let key = format!("race-{round}-{lane}");
                let body = signal_body(&format!("signal-race-{round}-{lane}"), &evidence);
                std::thread::spawn(move || {
                    let started = Instant::now();
                    barrier.wait();
                    let result = post_signal_with_sequence_retry(&base, &token, &path, &key, &body);
                    (started.elapsed(), result)
                })
            })
            .collect();
        let joined: Vec<_> = posts
            .into_iter()
            .enumerate()
            .map(|(lane, post)| match post.join() {
                Ok((elapsed, result)) => (lane, Some(elapsed), result),
                Err(_) => (
                    lane,
                    None,
                    Err(SequenceRetryError {
                        message: "request thread panicked before returning a result".to_owned(),
                        trace: Vec::new(),
                        phase: "thread-join",
                        byte_count: 0,
                    }),
                ),
            })
            .collect();
        for (lane, elapsed, result) in joined {
            let outcome = match result {
                Ok(outcome) => outcome,
                Err(error) => {
                    let elapsed_ms = elapsed.map_or_else(
                        || "unknown".to_owned(),
                        |elapsed| elapsed.as_millis().to_string(),
                    );
                    panic!(
                        "round {round} lane {lane} phase={} elapsed_ms={} byte_count={} signal request failed: {}",
                        error.phase, elapsed_ms, error.byte_count, error.message
                    );
                }
            };
            assert_eq!(
                outcome.status, 200,
                "round {round} lane {lane} mutation status after bounded sequence retries"
            );
            assert_eq!(
                outcome.reply["ok"], true,
                "round {round} lane {lane} final response did not certify the signal append"
            );
            assert!(
                outcome.retries <= SEQUENCE_CONFLICT_RETRY_LIMIT,
                "the retry count is bounded"
            );
            assert!(
                outcome.trace.len() <= SEQUENCE_CONFLICT_RETRY_LIMIT + 1,
                "the attempt trace is bounded"
            );
        }

        // Both request identities must remain durable. This catches a fixture that treats the
        // first 409 as success, retries with a changed key/body, or overwrites the first signal.
        let store = open_store(&events);
        let (_stream, history) = store.read_unique_replay_stream().unwrap();
        let expected_ids = [
            format!("signal-race-{round}-0"),
            format!("signal-race-{round}-1"),
        ];
        let records: Vec<_> = history
            .iter()
            .filter_map(|envelope| match &envelope.kind {
                graphhelm_protocols::EventKind::SignalRecorded(record)
                    if record.execution_id.as_str() == execution
                        && expected_ids
                            .iter()
                            .any(|id| id.as_str() == record.signal_id.as_str()) =>
                {
                    Some((envelope, record))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            records.len(),
            2,
            "both concurrent signal writes are retained"
        );
        for lane in 0..2 {
            let signal_id = format!("signal-race-{round}-{lane}");
            let (envelope, record) = records
                .iter()
                .copied()
                .find(|(_, record)| record.signal_id.as_str() == signal_id)
                .unwrap_or_else(|| panic!("missing durable signal {signal_id}"));
            assert_eq!(
                envelope.actor.actor_type(),
                graphhelm_protocols::PersistedActorType::Agent
            );
            assert_eq!(envelope.actor.id().as_str(), "agent-wake-test");
            assert!(
                envelope
                    .idempotency_key
                    .as_str()
                    .starts_with(&format!("race-{round}-{lane}-record-")),
                "the derived key preserves the same request key: {}",
                envelope.idempotency_key.as_str()
            );
            assert_eq!(
                record.source_kind,
                graphhelm_protocols::SignalSourceKind::Node
            );
            assert_eq!(record.source_id.as_str(), "implementation");
            assert_eq!(record.kind, "no_progress");
            assert_eq!(record.severity, graphhelm_protocols::SignalSeverity::High);
            assert!(
                envelope.evidence_refs.is_empty(),
                "the unsealed HTTP fixture keeps evidence in its operator file"
            );
            let evidence = directory
                .path()
                .join(format!("race-evidence-{round}-{lane}.json"));
            assert!(
                evidence.exists(),
                "the request's evidence output is retained"
            );
            let evidence_bytes = std::fs::read(&evidence).unwrap();
            let evidence_value: serde_json::Value =
                serde_json::from_slice(&evidence_bytes).unwrap();
            assert_eq!(
                evidence_value,
                signal_body(&signal_id, &evidence)["signal"],
                "each retry preserves the exact signal body in its evidence file"
            );
            let evidence_hash = graphhelm_graph::raw_content_sha256(&evidence_bytes).unwrap();
            assert!(
                evidence_hash.as_str() == record.envelope_sha256.as_str(),
                "the durable signal remains bound to its exact operator evidence"
            );
        }

        // #118: wait for the CONDITION — this round's arming consumed — not the schedule.
        // The 600ms sleep this replaces was a timing assumption wearing a step's clothes,
        // and it could not fail when the sweep consumed NOTHING: the close doc measured
        // this guard green with the recorder deleted, because its oracles (`ok == true`
        // per round, `consumed <= armed` overall) are satisfied by a component that never
        // writes. The bounded wait is the presence half that was missing: a recorder that
        // consumes nothing now fails HERE, at the first round, by name.
        let settle = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if consumption_ledger(&events)
                .iter()
                .any(|(victim, _, _)| *victim == arming)
            {
                break;
            }
            assert!(
                std::time::Instant::now() < settle,
                "round {round}: the arming at #{arming} was never consumed — a sweep that \
                 consumes nothing is exactly what the old sleep-plus-aggregate oracle \
                 could not see"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        // #118: per-round identity at receipt grain. EXACTLY one consumption took this
        // round's arming, and that consumption NAMES it (captured == victim) — the #74
        // discriminator asserted per consumption instead of trusted. The aggregate
        // inequality below cannot see a compensating redistribution (round N consumed
        // twice, round M never: 15 <= 15 still holds — two failures that cancel inside a
        // satisfied aggregate); per-arming counts catch both ends independently.
        let mine: Vec<(u64, u64, Option<u64>)> = consumption_ledger(&events)
            .into_iter()
            .filter(|(victim, _, _)| *victim == arming)
            .collect();
        assert_eq!(
            mine.len(),
            1,
            "round {round}: the arming at #{arming} is consumed EXACTLY once: {mine:?}"
        );
        assert_eq!(
            mine[0].2,
            Some(arming),
            "round {round}: the consumption at #{} names the arming it took: {mine:?}",
            mine[0].1
        );

        // The oracle: the stream still replays, and this round's lease was consumed
        // exactly once. A double-consume poisons every future replay — the live failure.
        let output = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
            .args([
                "execution",
                "status",
                "--events",
                events.to_str().unwrap(),
                "--execution",
                execution,
            ])
            .output()
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        if value["ok"] != true {
            // The journal IS the evidence: two `wake_lease_consumed` for one `wake_lease`
            // at consecutive sequences confirms the consume race; a storage-shaped refusal
            // instead acquits it. The tempdir dies with the test, so dump it in the panic.
            let journal = std::fs::read_to_string(events.join("journal.jsonl"))
                .unwrap_or_else(|error| format!("<journal unreadable: {error}>"));
            panic!(
                "round {round}: the stream must still replay — a refused replay means a \
                 double-consume landed: {value}\n\
                 --- journal.jsonl of the failing run ---\n{journal}"
            );
        }
    }

    // Belt over the whole run: count consumptions per arming in the raw journal.
    let journal = std::fs::read_to_string(events.join("journal.jsonl")).expect("journal readable");
    let mut armed = 0_usize;
    let mut consumed = 0_usize;
    for line in journal.lines().filter(|line| !line.trim().is_empty()) {
        let batch: serde_json::Value = serde_json::from_str(line).unwrap();
        for event in batch["events"].as_array().into_iter().flatten() {
            match event["kind"]["type"].as_str() {
                Some("wake_lease") => armed += 1,
                Some("wake_lease_consumed") => consumed += 1,
                _ => {}
            }
        }
    }
    assert!(
        consumed <= armed,
        "never more consumptions than armings ({consumed} > {armed})"
    );

    // #118: the finer belt — per-ARMING identity over the whole run, from the same
    // journal. Every one of the fifteen armings has exactly one ledger entry and every
    // entry names its victim. The aggregate above is kept (it costs nothing and still
    // owns the illegal-double world via the fold), but the headline moved here: this is
    // the assertion the recorder-dead and key-smear worlds cannot pass, and the one a
    // compensating redistribution cannot cancel inside.
    let ledger = consumption_ledger(&events);
    let mut per_arming: std::collections::BTreeMap<u64, usize> = std::collections::BTreeMap::new();
    for (victim, consumed_at, captured) in &ledger {
        *per_arming.entry(*victim).or_insert(0) += 1;
        assert_eq!(
            *captured,
            Some(*victim),
            "the consumption at #{consumed_at} names the arming it took"
        );
    }
    assert_eq!(
        per_arming.len(),
        15,
        "fifteen armings, fifteen victims in the ledger: {per_arming:?}"
    );
    for (arming, count) in &per_arming {
        assert_eq!(
            *count, 1,
            "the arming at #{arming} was consumed exactly once: {per_arming:?}"
        );
    }
}

/// #118's instrument: an ordered walk of the raw journal pairing each consumption with
/// the arming it took — the fold's own victim rule (a burn takes whatever lease is live
/// when it lands), applied test-side to the bytes on disk. Returns
/// (victim_arming, consumption_sequence, captured_arming) in journal order.
///
/// The walk exists because the projection's receipt maps are LAST-PER-SESSION (the #88
/// named cause, main 20fbf9e's precedent in-tree): an arming-scoped question walks the
/// log the maps cannot erase. Reimplemented here rather than shared with the product's
/// walk (`wake_wait.rs`) per this workspace's no-shared-lib convention for test binaries.
fn consumption_ledger(events: &Path) -> Vec<(u64, u64, Option<u64>)> {
    let journal = std::fs::read_to_string(events.join("journal.jsonl")).expect("journal readable");
    let mut live: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    let mut ledger = Vec::new();
    for line in journal.lines().filter(|line| !line.trim().is_empty()) {
        // A line that fails to parse is a TORN final line — an append in flight under
        // the 50ms poll this walk serves. SKIP it: it is complete on the next poll, so
        // skipping costs nothing, while unwrapping would make the anti-flake instrument
        // its own flake — and a JSON parse panic reads as "the test is broken", which is
        // how assertions get deleted instead of investigated (C's #118 strike). With
        // torn lines skipped, the victim `expect` below keeps its "cannot happen"
        // meaning: a complete, replayable journal cannot consume an unarmed lease.
        let Ok(batch) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        for event in batch["events"].as_array().into_iter().flatten() {
            let sequence = event["sequence"].as_u64().expect("envelope sequence");
            match event["kind"]["type"].as_str() {
                Some("wake_lease") => {
                    let session = event["kind"]["data"]["sessionId"]
                        .as_str()
                        .expect("wake_lease carries a sessionId")
                        .to_owned();
                    live.insert(session, sequence);
                }
                Some("wake_lease_consumed") => {
                    let session = event["kind"]["data"]["sessionId"]
                        .as_str()
                        .expect("wake_lease_consumed carries a sessionId");
                    let victim = live
                        .remove(session)
                        .expect("a replayable journal cannot consume an unarmed lease");
                    let captured = event["kind"]["data"]["capturedArming"].as_u64();
                    ledger.push((victim, sequence, captured));
                }
                _ => {}
            }
        }
    }
    ledger
}

/// M08, from the judge's finding: `wake_arm` answered with a number the ring never
/// compares, so a client could not predict from the reply whether it would be woken. The
/// reply must now name the doorbell's own head — and, because a client will echo it back
/// as its next cursor, re-arming with it must be a FIXED POINT: no free ring, and the next
/// content event still wakes.
///
/// State that makes the question exist: an execution with content on the stream, then an
/// arm, then one more content append.
#[test]
fn arming_reports_the_head_the_doorbell_compares_and_re_arming_with_it_is_a_fixed_point() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let execution = "exec-arm-contract";
    start_execution(&events, directory.path(), execution);
    let content = head(&events);
    arm_lease(&events, execution, "rvz-contract", content);
    let (_guard, address, token) = serve(&events);

    let armed = get_json(
        &address,
        &token,
        &format!("/v1/executions/{execution}/wake-lease?sessionId=session-sleeper-1"),
    );
    let reported = armed["data"]["contentHead"]
        .as_u64()
        .expect("the read reports the doorbell's head");

    // Re-arm with exactly what the surface reports: the fixed-point property a client
    // needs in order to echo the reply back without arming itself past the ring.
    let (status, reply) = post_json(
        &address,
        &token,
        &format!("/v1/executions/{execution}/wake-lease"),
        "arm-contract-echo",
        &serde_json::json!({
            "sessionId": "session-sleeper-1",
            "rendezvousId": "rvz-contract",
            "cursor": reported,
        }),
    );
    assert_eq!(status, 200, "{reply}");
    assert_eq!(
        reply["data"]["contentHead"].as_u64(),
        Some(reported),
        "arming answers with the SAME doorbell head it was armed against — a client that \
         echoes the reply back must land where it already was: {reply}"
    );
    assert_eq!(
        reply["data"]["armedCursor"].as_u64(),
        Some(reported),
        "the echoed cursor is honoured verbatim: {reply}"
    );

    let after = get_json(
        &address,
        &token,
        &format!("/v1/executions/{execution}/wake-lease?sessionId=session-sleeper-1"),
    );
    assert_eq!(
        after["data"]["live"], true,
        "re-arming at the doorbell's own head must NOT burn the lease — a free ring would \
         wake an operator who was told nothing happened: {after}"
    );
}

/// M08 judge, finding 2: `lastEventAt` advanced purely because of the observer's own
/// `wake_arm`, while no node made any progress. The field an operator reads to decide
/// whether anything is happening was being BUMPED BY THE ACT OF MONITORING — the
/// head-versus-contentHead defect, wearing a clock.
///
/// The store state that makes the question exist: a real execution with real content, then
/// wake bookkeeping and NOTHING else. If arming moved the clock, an operator watching a
/// wedged run would see it look alive precisely because they were watching it.
#[test]
fn arming_a_lease_never_moves_the_execution_clock() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let execution = "exec-observer-clock";
    start_execution(&events, directory.path(), execution);
    let (_guard, address, token) = serve(&events);

    let before = get_json(&address, &token, &format!("/v1/executions/{execution}"));
    let before_stamp = before["data"]["lastEventAt"].clone();
    assert!(
        before_stamp.is_string(),
        "the fixture must have content to timestamp: {before:?}"
    );

    arm_lease(&events, execution, "rdv-observer", 1);

    let after = get_json(&address, &token, &format!("/v1/executions/{execution}"));
    assert_eq!(
        after["data"]["lastEventAt"], before_stamp,
        "watching is not progress: a wake lease is a reader announcing that it intends to \
         listen, and it must never make a wedged run look alive"
    );
}

// -------------------------------------------------------------------------------------------
// M09 decision B, step 2: arming DECLARES the horizon.
//
// Until now a lease said "wake me for anything after #N" and nothing about how long quiet may
// last. The horizon is computed ONCE here, at the only moment someone is provably awake and
// consenting, and it is an absolute instant so no reader ever has to add a duration to a clock
// of its own -- the two-clocks defect this decision exists to remove.
//
// The assertion is against the LEASE EVENT'S OWN recorded instant plus the declared seconds,
// not against a number this test computed from its own clock. A test that says "roughly now
// plus 300" passes for an implementation that used the wrong base, and the base is the thing
// in question.
// -------------------------------------------------------------------------------------------

/// The armed horizon is the arming event's own instant plus the seconds the sleeper declared.
#[test]
fn arming_with_a_declared_bound_stores_that_instant_on_the_lease() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    start_execution(&events, directory.path(), "exec-wake-horizon");
    let (_guard, address, token) = serve(&events);

    let (status, reply) = post_json(
        &address,
        &token,
        "/v1/executions/exec-wake-horizon/wake-lease",
        "wake-horizon-1",
        &serde_json::json!({
            "sessionId": "session-sleeper-1",
            "rendezvousId": "rvz-horizon-1",
            "maturesInSeconds": 300,
        }),
    );
    assert_eq!(status, 200, "{reply}");

    let armed_at = last_event_instant(&events);
    let expected = (armed_at + chrono::Duration::seconds(300))
        .to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true);

    // The write's own reply, and then a READ: a horizon that exists only in the response is
    // the F2 defect of the previous milestone, where the button worked and its effect was
    // discarded on the next read.
    assert_eq!(
        reply["data"]["maturesAt"], expected,
        "the arming reply names the horizon it stored: {reply}"
    );
    let answer = get_json(
        &address,
        &token,
        "/v1/executions/exec-wake-horizon/wake-lease?sessionId=session-sleeper-1",
    );
    assert_eq!(
        answer["data"]["maturesAt"], expected,
        "the horizon is on the lease the next reader folds, not only in the write's reply: \
         {answer}"
    );
}

/// Absence stays absence: arming without declaring a bound promises nothing, and no horizon is
/// invented for it. Sabotage: default the missing bound to any number at all.
#[test]
fn arming_without_a_declared_bound_promises_no_horizon() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    start_execution(&events, directory.path(), "exec-wake-nohorizon");
    let (_guard, address, token) = serve(&events);

    let (status, reply) = post_json(
        &address,
        &token,
        "/v1/executions/exec-wake-nohorizon/wake-lease",
        "wake-nohorizon-1",
        &serde_json::json!({
            "sessionId": "session-sleeper-1",
            "rendezvousId": "rvz-nohorizon-1",
        }),
    );
    assert_eq!(status, 200, "{reply}");
    assert!(
        reply["data"]["maturesAt"].is_null(),
        "no bound declared means no horizon, and none is invented: {reply}"
    );
    let answer = get_json(
        &address,
        &token,
        "/v1/executions/exec-wake-nohorizon/wake-lease?sessionId=session-sleeper-1",
    );
    assert!(
        answer["data"]["maturesAt"].is_null(),
        "the read agrees that nothing was promised: {answer}"
    );
}

/// The loose end of the bound, which is the dangerous one.
///
/// Zero was already refused. A trillion seconds was not: it produced a horizon in the year
/// 33715 and the surface answered with a DATE, which reads as a promise while meaning never —
/// absence laundered into calm through arithmetic. Worse, `u64::MAX` overflowed the conversion
/// and silently yielded NO horizon at all, so an operator who declared a bound got none and was
/// told nothing. The ceiling is the one its neighbours already use for a declared duration.
#[test]
fn a_bound_nobody_will_live_to_see_is_refused_rather_than_promised() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    start_execution(&events, directory.path(), "exec-wake-absurd");
    let (_guard, address, token) = serve(&events);

    for (label, seconds) in [
        ("a trillion seconds", 1_000_000_000_000_u64),
        ("the largest number there is", u64::MAX),
        ("one second past ten years", 315_576_001),
    ] {
        let (status, reply) = post_json(
            &address,
            &token,
            "/v1/executions/exec-wake-absurd/wake-lease",
            &format!("wake-absurd-{seconds}"),
            &serde_json::json!({
                "sessionId": "session-sleeper-1",
                "rendezvousId": "rvz-absurd-1",
                "maturesInSeconds": seconds,
            }),
        );
        assert_eq!(status, 400, "{label} must be refused, not stored: {reply}");
        assert_eq!(
            reply["diagnostics"][0]["path"], "/maturesInSeconds",
            "the refusal names the field the operator must change: {reply}"
        );
    }

    // And nothing was armed by the attempts.
    let answer = get_json(
        &address,
        &token,
        "/v1/executions/exec-wake-absurd/wake-lease?sessionId=session-sleeper-1",
    );
    assert_eq!(
        answer["data"]["live"], false,
        "a refused bound arms nothing: {answer}"
    );
}

// -------------------------------------------------------------------------------------------
// M09 decision B, step 3: the waiter reads its OWN lease and nothing else.
//
// `wake-wait` took a rendezvous id and a timeout FROM THE CALLER. Two numbers answered "how
// long before I give up" -- the one the sleeper declared at arming, and the one it happened to
// pass on the command line -- and nothing tied them together. That is the F4 family on the
// sleep surface: whenever two numbers answer one question, one of them is lying at some point.
//
// Now there is one. The waiter opens the store, reads the lease belonging to ITS OWN session,
// LETS THE HANDLE GO, and only then blocks. Letting go matters: the repository holds an
// OS-level exclusive lock for the handle's lifetime, so a waiter that held it would lock every
// concurrent process out for the whole night -- the exact window the product is supposed to
// keep working.
// -------------------------------------------------------------------------------------------

fn wake_wait(events: &Path, execution: &str, session: &str) -> (i32, serde_json::Value) {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args([
            "wake-wait",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            execution,
            "--session-id",
            session,
        ])
        .output()
        .unwrap();
    let value = serde_json::from_slice(&output.stdout).unwrap_or(serde_json::Value::Null);
    (output.status.code().unwrap_or(-1), value)
}

/// B10: a horizon already past when the wait begins must answer AT ONCE.
///
/// Blocking here would mean the one case where everything has already gone wrong is the one
/// case the tool sits quiet through. The lease is armed with a one-second bound and the wait
/// starts after it, so the deadline is behind us before the first instruction runs.
#[test]
fn a_horizon_already_past_answers_immediately_rather_than_waiting() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    start_execution(&events, directory.path(), "exec-wake-past");
    let (_guard, address, token) = serve(&events);
    let (status, reply) = post_json(
        &address,
        &token,
        "/v1/executions/exec-wake-past/wake-lease",
        "wake-past-1",
        &serde_json::json!({
            "sessionId": "session-sleeper-1",
            "rendezvousId": "rvz-past-1",
            "maturesInSeconds": 1,
        }),
    );
    assert_eq!(status, 200, "{reply}");
    std::thread::sleep(std::time::Duration::from_millis(1200));

    let (code, answer) = wake_wait(&events, "exec-wake-past", "session-sleeper-1");
    assert_eq!(code, 3, "the declared deadline passed: {answer}");
    assert_eq!(
        answer["data"]["matured"], true,
        "the answer says the DECLARED deadline passed, not merely that nothing rang: {answer}"
    );
    // The assertion that actually measures "at once". An elapsed-time bound cannot: process
    // start-up costs more than the one-second wait a broken implementation would perform, so
    // a generous threshold passes for the bug and a tight one fails for the fixture. The
    // answer says whether it waited at all.
    assert_eq!(
        answer["data"]["alreadyPast"], true,
        "the deadline was behind us before the wait began, and the answer says so: {answer}"
    );
}

/// B11: a waiter may only wait on the lease of its own session.
///
/// Accepting whatever lease happened to be in the store is the waiter reading more than its
/// own -- the property this step exists to keep. Refusal names the session, and it is a
/// refusal rather than an indefinite wait, because waiting forever on nothing is the silent
/// failure this milestone is about.
#[test]
fn a_waiter_with_no_lease_of_its_own_refuses_instead_of_waiting() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    start_execution(&events, directory.path(), "exec-wake-other");
    let (_guard, address, token) = serve(&events);
    let (status, reply) = post_json(
        &address,
        &token,
        "/v1/executions/exec-wake-other/wake-lease",
        "wake-other-1",
        &serde_json::json!({
            "sessionId": "session-sleeper-1",
            "rendezvousId": "rvz-other-1",
            "maturesInSeconds": 300,
        }),
    );
    assert_eq!(status, 200, "{reply}");

    let (code, answer) = wake_wait(&events, "exec-wake-other", "session-somebody-else");
    assert_eq!(
        code, 2,
        "a waiter with no lease of its own refuses: {answer}"
    );
    assert_eq!(answer["ok"], false, "{answer}");
    assert!(
        answer["diagnostics"][0]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("session-somebody-else"),
        "the refusal names the session that has no lease: {answer}"
    );
}

/// A lease armed with no bound promises nothing, so waiting on it is refused rather than
/// silently becoming a wait with no end. Absence stays absence on this surface too.
#[test]
fn waiting_on_a_lease_that_declared_no_bound_is_refused() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    start_execution(&events, directory.path(), "exec-wake-unbounded");
    let (_guard, address, token) = serve(&events);
    let (status, reply) = post_json(
        &address,
        &token,
        "/v1/executions/exec-wake-unbounded/wake-lease",
        "wake-unbounded-1",
        &serde_json::json!({
            "sessionId": "session-sleeper-1",
            "rendezvousId": "rvz-unbounded-1",
        }),
    );
    assert_eq!(status, 200, "{reply}");

    let (code, answer) = wake_wait(&events, "exec-wake-unbounded", "session-sleeper-1");
    assert_eq!(
        code, 2,
        "no bound declared, so no wait is promised: {answer}"
    );
    assert_eq!(answer["ok"], false, "{answer}");
}

// -------------------------------------------------------------------------------------------
// M09 decision B, step 4: shortening a horizon is ALLOWED and SAID, never refused in silence.
//
// A waiter reads its lease once and then blocks, so re-arming cannot reach it. The two
// directions of that gap are not the same failure. Lengthening means the sleeper wakes EARLY
// -- a false alarm, annoying and safe. Shortening means it wakes LATE, missing the deadline
// someone set precisely because they thought it more urgent, which is the silent broken
// promise this milestone exists to remove.
//
// Refusing the shortening was the first answer and it was wrong: shortening is not a mistake,
// and "never invent" and "always refuse" are different rules. What is wrong is failing in
// silence. So the arm accepts it and SAYS so -- as a named field, because a sentence would
// repeat the exit-code gap the quickstart documents: a client must be able to decide without
// reading prose.
// -------------------------------------------------------------------------------------------

/// Shortening is accepted and named, with both instants, so a client can act without parsing
/// English.
#[test]
fn shortening_a_horizon_is_accepted_and_named_with_both_instants() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    start_execution(&events, directory.path(), "exec-wake-shorter");
    let (_guard, address, token) = serve(&events);

    let arm = |key: &str, seconds: u64| {
        post_json(
            &address,
            &token,
            "/v1/executions/exec-wake-shorter/wake-lease",
            key,
            &serde_json::json!({
                "sessionId": "session-sleeper-1",
                "rendezvousId": "rvz-shorter-1",
                "maturesInSeconds": seconds,
            }),
        )
    };

    let (status, first) = arm("wake-shorter-1", 3600);
    assert_eq!(status, 200, "{first}");
    assert!(
        first["data"]["horizonShortened"].is_null(),
        "the first arming shortens nothing: {first}"
    );
    let was = first["data"]["maturesAt"].as_str().unwrap().to_owned();

    let (status, shorter) = arm("wake-shorter-2", 60);
    assert_eq!(
        status, 200,
        "shortening is accepted, not refused: {shorter}"
    );
    let notice = &shorter["data"]["horizonShortened"];
    assert_eq!(
        notice["from"], was,
        "the notice names the horizon that was replaced: {shorter}"
    );
    assert_eq!(
        notice["to"], shorter["data"]["maturesAt"],
        "and the one that replaced it: {shorter}"
    );
    assert_eq!(
        notice["remedy"], "restart_wait",
        "a client must be able to decide from a field, not from prose: {shorter}"
    );
}

/// The other direction stays quiet, because it fails toward waking early — which is safe.
/// Sabotage: notify on any change at all. This falls, and it matters: a notice that fires for
/// the harmless direction trains the reader to ignore the dangerous one.
#[test]
fn lengthening_a_horizon_says_nothing_because_it_fails_safe() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    start_execution(&events, directory.path(), "exec-wake-longer");
    let (_guard, address, token) = serve(&events);

    let arm = |key: &str, seconds: u64| {
        post_json(
            &address,
            &token,
            "/v1/executions/exec-wake-longer/wake-lease",
            key,
            &serde_json::json!({
                "sessionId": "session-sleeper-1",
                "rendezvousId": "rvz-longer-1",
                "maturesInSeconds": seconds,
            }),
        )
    };

    let (status, first) = arm("wake-longer-1", 60);
    assert_eq!(status, 200, "{first}");
    let (status, longer) = arm("wake-longer-2", 3600);
    assert_eq!(status, 200, "{longer}");
    assert!(
        longer["data"]["horizonShortened"].is_null(),
        "waking early is safe, so nothing is said: {longer}"
    );
}

/// #72 S7, the green half: the phase-3 delay seam makes "eventually" a CHOSEN number
/// (2s here), and the receipt wait absorbs it deterministically — the guard waits for
/// the condition, not the schedule. The red half (same delay, wait removed -> fails
/// every time) is a sabotage run recorded in the issue's evidence, not committed code.
/// The final assert is this test's own blade: if the seam's env plumbing ever dies, the
/// receipt arrives instantly and the >=1.5s check falls — a delay hook nobody can
/// trigger would otherwise pass this test while measuring nothing.
#[cfg(windows)]
#[test]
fn a_designed_phase3_delay_is_absorbed_by_the_receipt_wait() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let execution = "exec-wake-delay";
    start_execution(&events, directory.path(), execution);
    let (_guard, base, token, sleeper) = serve_before_arming_sleeper(
        &events,
        execution,
        "rvz-delay-1",
        &[("GRAPHHELM_TEST_WAKE_PHASE3_DELAY_MS", "2000")],
    );

    let evidence_out = directory.path().join("delay-evidence.json");
    let (status, reply) = post_json(
        &base,
        &token,
        &format!("/v1/executions/{execution}/signal"),
        "delay-signal-1",
        &signal_body("signal-delay-1", &evidence_out),
    );
    assert_eq!(status, 200, "{reply}");

    // Two-phase by design: the byte crosses BEFORE the (deliberately delayed) durable
    // consume append.
    let (bytes, _at_ring) = sleeper.wait();
    assert_eq!(bytes.len(), 1, "exactly one content-free byte crossed");
    let rung_at = Instant::now();

    let deadline = Instant::now() + Duration::from_secs(10);
    let answer = loop {
        let answer = get_json(
            &base,
            &token,
            &format!("/v1/executions/{execution}/wake-lease?sessionId=session-sleeper-1"),
        );
        if !answer["data"]["lastConsumed"].is_null() {
            break answer;
        }
        assert!(
            Instant::now() < deadline,
            "the receipt must land despite the designed delay: {answer}"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(
        answer["data"]["lastConsumed"]["reason"], "rung",
        "the receipt names the ring, delay or no delay: {answer}"
    );
    assert!(
        rung_at.elapsed() >= Duration::from_millis(1500),
        "the seam actually delayed phase 3 — a receipt this early means the delay hook \
         is dead and this test is measuring nothing: {:?}",
        rung_at.elapsed()
    );
}

// ---------------------------------------------------------------------------------------------
// #88: the timeout answer consults the receipt — the deadline stops flattening "you were rung
// and the byte died" into "nothing happened". Guards at receipt grain: exact reason AND exact
// sequence, never presence. `receiptReadAt` is asserted FIRST in every guard: it proves the
// deadline read HAPPENED, so no guard can pass vacuously against the old shape (a missing
// `lastConsumed` key and a null one are indistinguishable to a JSON index — the marker is not).
// ---------------------------------------------------------------------------------------------

/// Burns a lease by direct store append, naming the arming it captured (None models a
/// consumption from before `captured_arming` existed). Returns the consumption's sequence.
#[cfg(windows)]
fn consume_lease(
    events: &Path,
    execution: &str,
    session: &str,
    reason: graphhelm_protocols::WakeConsumeReason,
    captured_arming: Option<u64>,
) -> u64 {
    let store = open_store(events);
    let (stream, _events) = store.read_unique_replay_stream().unwrap();
    let next = store
        .next_sequence(&stream.scope, &stream.stream_id)
        .unwrap();
    let request = graphhelm_events::PreparedAppend::new(
        stream.scope.clone(),
        graphhelm_protocols::OpaqueId::parse(stream.stream_id.clone()).unwrap(),
        next,
        vec![graphhelm_protocols::NewEvent::new(
            graphhelm_protocols::OpaqueId::parse(format!("consume-{next}")).unwrap(),
            graphhelm_protocols::PersistedActor::new(
                graphhelm_protocols::PersistedActorType::System,
                graphhelm_protocols::ActorId::parse("system-wake").unwrap(),
            ),
            graphhelm_protocols::Sensitivity::Internal,
            graphhelm_protocols::EventKind::WakeLeaseConsumed(
                graphhelm_protocols::WakeLeaseConsumed {
                    execution_id: graphhelm_protocols::OpaqueId::parse(execution).unwrap(),
                    session_id: graphhelm_protocols::OpaqueId::parse(session).unwrap(),
                    reason,
                    captured_arming,
                },
            ),
            vec![],
            vec![],
        )],
        vec![],
        vec![],
    )
    .unwrap();
    store.append_atomic(&request).unwrap();
    next
}

/// Arms a lease for a DECOY session — a sequence-spacer between a fixture's arm and its
/// consume. Exists because sabotage s2 (liveness-instead-of-receipt, synthesizing
/// `(rung, armed+1)`) survived G1 and G7: their consume sat ADJACENT to the arm, so the
/// guessed sequence was coincidentally right. One unrelated event between the two makes
/// `atSequence` unguessable by adjacency for ANY guessing implementation; a decoy-session
/// wake_lease is the cheapest event the store accepts standalone, and the walk under test
/// skips other sessions by construction.
#[cfg(windows)]
fn arm_decoy(events: &Path, execution: &str, rendezvous_id: &str) {
    let store = open_store(events);
    let (stream, _events) = store.read_unique_replay_stream().unwrap();
    let next = store
        .next_sequence(&stream.scope, &stream.stream_id)
        .unwrap();
    let request = graphhelm_events::PreparedAppend::new(
        stream.scope.clone(),
        graphhelm_protocols::OpaqueId::parse(stream.stream_id.clone()).unwrap(),
        next,
        vec![graphhelm_protocols::NewEvent::new(
            graphhelm_protocols::OpaqueId::parse(format!("decoy-{rendezvous_id}")).unwrap(),
            graphhelm_protocols::PersistedActor::new(
                graphhelm_protocols::PersistedActorType::Agent,
                graphhelm_protocols::ActorId::parse("agent-decoy").unwrap(),
            ),
            graphhelm_protocols::Sensitivity::Internal,
            graphhelm_protocols::EventKind::WakeLease(graphhelm_protocols::WakeLease {
                execution_id: graphhelm_protocols::OpaqueId::parse(execution).unwrap(),
                session_id: graphhelm_protocols::OpaqueId::parse("session-decoy-1").unwrap(),
                cursor: 1,
                rendezvous_id: graphhelm_protocols::OpaqueId::parse(rendezvous_id).unwrap(),
                matures_in_seconds: None,
            }),
            vec![],
            vec![],
        )],
        vec![],
        vec![],
    )
    .unwrap();
    store.append_atomic(&request).unwrap();
}

/// The sidecar's own diagnosis, as a suffix that disappears when there is not one.
///
/// It arrives on STDOUT as the refusal envelope, NOT on stderr. Measured, not assumed: `wake-wait`
/// on an execution with no live lease exits 2, writes **0 bytes** to stderr, and puts
/// `GHCLI017_WAKE_INVALID` on stdout. Both waits in this file previously read stderr and appended
/// an empty string, so every dead-sidecar failure named the exit class and silently dropped the
/// reason. Reading stdout is only sound once the child has been reaped -- which is the sole
/// condition under which this is called.
#[cfg(windows)]
fn reported_refusal(child: &mut Child) -> String {
    let mut reported = String::new();
    if let Some(mut pipe) = child.stdout.take() {
        use std::io::Read as _;
        let _ = pipe.read_to_string(&mut reported);
    }
    let reported = reported.trim();
    if reported.is_empty() {
        String::new()
    } else {
        format!(": {reported}")
    }
}

/// Waits (bounded) for the sidecar's rendezvous to exist — the same condition-wait the
/// choreography test uses; a fixed sleep would be a timing assumption wearing a step's
/// clothes. Both waits now share `reported_refusal`, so the claim of sameness this comment
/// has always made is true of the DIAGNOSIS too, not only of the polling shape.
///
/// This is a 30-second hang catcher, not a supported startup deadline. A healthy Windows start can
/// take longer than five seconds while an eight-second lease is still alive, especially when the
/// test binary starts several sidecars together. The lease must outlive this catcher so a failure
/// here still diagnoses startup rather than an already-mature lease (#413).
///
/// The idle benchmark below is evidence for the original bug: parallel sidecar startup consumed
/// most of the old short lease before the scenario began. It is not evidence that five seconds is
/// a safe maximum on every healthy Windows machine.
#[cfg(windows)]
const PIPE_STARTUP_HANG_CATCHER_SECONDS: u64 = 30;
/// #1053: 60 -> 40, and the CATCHER ABOVE IS DELIBERATELY UNCHANGED.
///
/// These six racing tests are a pure wait: the sidecar sits until the lease deadline, reads its
/// receipt once, and exits, so each test costs almost exactly this constant. Measured with
/// `cargo nextest run -p graphhelm-cli --test wake_http`, which reports per-test wall time:
///
///   61.524 / 61.440 / 61.369 / 61.276 / 61.217 / 61.206 s
///
/// Against a 60-second lease that is **~1.3 s for everything else the scenario does** -- spawning
/// the sidecar, `wait_for_pipe`, arming the decoy, consuming the lease, reading the result. The
/// startup this file's catcher guards is a fraction of that 1.3 s, so 30 s is roughly a 23x margin
/// on an idle box and the lease was carrying 20 s nobody was using.
///
/// WHAT DID NOT CHANGE, AND WHY THAT IS THE POINT. #413 was not about the lease being long; it was
/// about parallel sidecar startup eating a lease that was too SHORT to cover startup at all. The
/// defence against that is the invariant `lease > catcher`, and lowering only the lease keeps it
/// with 10 s to spare while leaving the catcher's own 30 s worst-case budget untouched. Lowering
/// the catcher as well would have traded the actual safety margin for another 10 s of gate time,
/// which is the trade #413 already paid for once.
///
/// THE INVARIANT IS ALREADY GUARDED, and this change narrows the margin it allows rather than
/// removing it. `the_pipe_wait_is_bounded_below_the_leases_it_races` reads this very file, collects
/// every lease bound armed by a test that then calls `wait_for_pipe`, and asserts BOTH
/// `pipe_bound >= 30` and `pipe_bound < tightest` -- with a refusal first if it extracted nothing,
/// so it cannot pass vacuously. At 60 the slack was 30 s; at 40 it is 10 s. The guard's own
/// message is what a future reader gets if someone takes the last 10 s.
#[cfg(windows)]
const RACING_WAKE_LEASE_SECONDS: u64 = 40;

#[cfg(windows)]
/// Is this rendezvous currently an instance in the machine's pipe namespace?
///
/// ONE definition, deliberately: `wait_for_pipe` waits for this to become true and
/// `wait_for_pipe_to_vanish` waits for it to become false. Two spellings of the same question
/// could disagree about case, prefix, or enumeration failure, and then the two waits would be
/// waiting on different things while reading as symmetric.
#[cfg(windows)]
fn pipe_is_present(rendezvous_id: &str) -> bool {
    let expected = format!("graphhelm-wake-{rendezvous_id}");
    std::fs::read_dir("//./pipe").is_ok_and(|entries| {
        entries.filter_map(Result::ok).any(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.eq_ignore_ascii_case(&expected))
        })
    })
}

#[cfg(windows)]
fn wait_for_pipe(child: &mut Child, rendezvous_id: &str) {
    let deadline = Instant::now() + Duration::from_secs(PIPE_STARTUP_HANG_CATCHER_SECONDS);
    let expected = format!("graphhelm-wake-{rendezvous_id}");
    while !pipe_is_present(rendezvous_id) {
        // The child is asked BEFORE the deadline is judged, because a dead sidecar and a slow one
        // are different failures and only this call can tell them apart. Without it both spend the
        // full hang-catcher window and report the same sentence -- true of the pipe, useless about the
        // cause, while the exit status sat unread the whole time (#386).
        if let Some(status) = child.try_wait().expect("the sidecar handle is readable") {
            // Says only what this position can KNOW: the child is gone and the name is absent.
            // Not "exited before creating it" -- measured under load, six failures carried
            // `exit code: 3` with `"timedOut":true`, meaning the sidecar HAD created the pipe,
            // waited its whole lease, exited, and took the pipe with it. Only the envelope
            // separates that from a sidecar that died early, so it is printed, not summarised.
            panic!(
                "the sidecar is no longer running and {expected} is not present: {status}{}",
                reported_refusal(child)
            );
        }
        // Reached only while the child is ALIVE, so this is now what it says it is: the rendezvous
        // did not appear in time. It must keep failing -- a wait that cannot fail is not a wait.
        assert!(
            Instant::now() < deadline,
            "the sidecar is still running but never created its rendezvous {expected}"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Reaps a wake-wait child and parses the one JSON line it prints: (exit code, envelope).
#[cfg(windows)]
fn wake_wait_result(child: Child) -> (Option<i32>, serde_json::Value) {
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value = stdout
        .lines()
        .find_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .unwrap_or_else(|| panic!("no JSON line on stdout: {output:?}"));
    (output.status.code(), value)
}

/// G1 (#88): the missed ring. The lease is burned as `rung` while the waiter sleeps and no
/// byte ever crosses; the deadline answer must say so — exact reason, exact sequence — while
/// `rung:false` keeps the byte claim honest and the exit code stays 3 (the fallback-read
/// contract is unchanged; the receipt tells the host the read will find something).
#[cfg(windows)]
#[test]
fn a_burned_but_unrung_lease_names_its_missed_ring_at_the_deadline() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let execution = "exec-88-missed";
    start_execution(&events, directory.path(), execution);
    let logical_rendezvous_id = "rvz-88-missed";
    let rendezvous_id = wake_wait_fixture_rendezvous_id(logical_rendezvous_id, &events);
    arm_lease_bounded(
        &events,
        execution,
        &rendezvous_id,
        head(&events),
        Some(RACING_WAKE_LEASE_SECONDS),
    );
    let armed_at = head(&events);
    let mut child = spawn_wake_wait(
        &events,
        execution,
        "session-sleeper-1",
        logical_rendezvous_id,
    );
    wait_for_pipe(&mut child, &rendezvous_id);
    // Sequence-spacer: the burn must NOT sit adjacent to the arm, or its sequence is
    // guessable by `armed + 1` (sabotage s2 proved a guessing implementation survives
    // an adjacent fixture).
    arm_decoy(&events, execution, "rvz-88-decoy-missed");
    let consumed_at = consume_lease(
        &events,
        execution,
        "session-sleeper-1",
        graphhelm_protocols::WakeConsumeReason::Rung,
        Some(armed_at),
    );

    let (code, value) = wake_wait_result(child);
    assert_eq!(code, Some(3), "a missed ring is still a timeout: {value}");
    let data = &value["data"];
    assert_eq!(
        data["receiptReadAt"], "deadline-once",
        "the deadline read happened, and says when it looked: {data}"
    );
    assert_eq!(
        data["rung"], false,
        "no byte crossed and none is claimed: {data}"
    );
    assert_eq!(
        data["lastConsumed"]["reason"], "rung",
        "the receipt names the ring the byte lost: {data}"
    );
    assert_eq!(
        data["lastConsumed"]["atSequence"], consumed_at,
        "the exact burn, not merely 'a burn': {data}"
    );
    assert_eq!(data["missedRing"], true, "{data}");
    assert_eq!(
        data["laterArmingLive"], false,
        "nobody re-armed, and the answer must not imply otherwise: {data}"
    );
    assert!(
        data.get("misBurn").is_none(),
        "an honest burn is not a mis-burn: {data}"
    );
}

/// G2 (#88): genuine silence. Nothing happened, and the answer says so at the same grain the
/// missed-ring case uses — G1 is this guard's positive control (the same machinery
/// demonstrably CAN report a receipt, so a dead deadline-read cannot fake this pair green in
/// both directions).
#[cfg(windows)]
#[test]
fn a_silent_deadline_reports_a_silent_receipt_not_just_silence() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let causal_transcript = directory.path().join("wake-wait-causal-transcript");
    let execution = "exec-88-silent";
    start_execution(&events, directory.path(), execution);
    let logical_rendezvous_id = "rvz-88-silent";
    let rendezvous_id = wake_wait_fixture_rendezvous_id(logical_rendezvous_id, &events);
    arm_lease_bounded(&events, execution, &rendezvous_id, head(&events), Some(1));
    let child = spawn_wake_wait_with_env(
        &events,
        execution,
        "session-sleeper-1",
        logical_rendezvous_id,
        &[(
            TEST_WAKE_CAUSAL_TRANSCRIPT_ENV,
            causal_transcript.to_str().unwrap(),
        )],
    );

    let (code, value) = wake_wait_result(child);
    // Post-child contention observer: the zero-capacity channel forces one bounded
    // scheduler rendezvous after the child is complete, without using wall-clock timing.
    let (post_child_contention_tx, post_child_contention_rx) = std::sync::mpsc::sync_channel(0);
    let post_child_contention_observer =
        std::thread::spawn(move || post_child_contention_tx.send(()).unwrap());
    post_child_contention_rx.recv().unwrap();
    post_child_contention_observer.join().unwrap();
    let transcript = std::fs::read_to_string(&causal_transcript)
        .unwrap_or_else(|error| panic!("CAUSAL_TRANSCRIPT_OBSERVER_MISSING: {error}"));
    let entries = transcript.lines().collect::<Vec<_>>();
    assert_eq!(
        entries
            .iter()
            .filter(|entry| **entry == "deadline-transition")
            .count(),
        1,
        "CAUSAL_TRACE_DEADLINE_COUNT: {entries:?}"
    );
    assert_eq!(
        entries
            .iter()
            .filter(|entry| **entry == "receipt-read-attempt")
            .count(),
        1,
        "CAUSAL_TRACE_RECEIPT_READ_ATTEMPT_COUNT: {entries:?}"
    );
    assert_eq!(
        entries
            .iter()
            .filter(|entry| **entry == "outcome-publication")
            .count(),
        1,
        "CAUSAL_TRACE_PUBLICATION_COUNT: {entries:?}"
    );
    assert_eq!(
        entries,
        [
            "deadline-transition",
            "receipt-read-attempt",
            "outcome-publication"
        ],
        "CAUSAL_TRACE_ORDER: the deadline, its one receipt read, and publication must be causally ordered"
    );
    assert_eq!(code, Some(3), "{value}");
    let data = &value["data"];
    assert_eq!(
        data["receiptReadAt"], "deadline-once",
        "silence is only reportable if the read happened: {data}"
    );
    assert!(
        data["lastConsumed"].is_null(),
        "no receipt for this arming: {data}"
    );
    assert_eq!(data["missedRing"], false, "{data}");
    assert_eq!(data["laterArmingLive"], false, "{data}");
}

#[cfg(windows)]
#[test]
fn causal_transcript_refuses_marks_past_its_byte_bound() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let causal_transcript = directory.path().join("wake-wait-causal-transcript-at-cap");
    let execution = "exec-88-transcript-cap";
    start_execution(&events, directory.path(), execution);
    let logical_rendezvous_id = "rvz-88-transcript-cap";
    let rendezvous_id = wake_wait_fixture_rendezvous_id(logical_rendezvous_id, &events);
    arm_lease_bounded(&events, execution, &rendezvous_id, head(&events), Some(1));
    let at_cap = vec![b'x'; TEST_WAKE_CAUSAL_TRANSCRIPT_MAX_BYTES];
    std::fs::write(&causal_transcript, &at_cap).unwrap();
    let child = spawn_wake_wait_with_env(
        &events,
        execution,
        "session-sleeper-1",
        logical_rendezvous_id,
        &[(
            TEST_WAKE_CAUSAL_TRANSCRIPT_ENV,
            causal_transcript.to_str().unwrap(),
        )],
    );

    let (code, value) = wake_wait_result(child);
    assert_eq!(code, Some(3), "{value}");
    assert_eq!(
        std::fs::read(&causal_transcript).unwrap(),
        at_cap,
        "the opt-in observer must never grow beyond its named byte bound"
    );
}

/// G3 (#88): the three-worlds split. Burned-and-missed PLUS a later live re-arm — the world
/// where waking the host into "re-arm" would double-arm, so it gets its own field rather
/// than flattening into W2. The burn's sequence sits strictly between the two armings'.
#[cfg(windows)]
#[test]
fn a_ring_missed_and_a_re_arm_are_reported_as_different_worlds() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let execution = "exec-88-rearm";
    start_execution(&events, directory.path(), execution);
    let first_logical_rendezvous_id = "rvz-88-rearm-a";
    let first_rendezvous_id = wake_wait_fixture_rendezvous_id(first_logical_rendezvous_id, &events);
    arm_lease_bounded(
        &events,
        execution,
        &first_rendezvous_id,
        head(&events),
        Some(RACING_WAKE_LEASE_SECONDS),
    );
    let first_arming = head(&events);
    let mut child = spawn_wake_wait(
        &events,
        execution,
        "session-sleeper-1",
        first_logical_rendezvous_id,
    );
    wait_for_pipe(&mut child, &first_rendezvous_id);
    let consumed_at = consume_lease(
        &events,
        execution,
        "session-sleeper-1",
        graphhelm_protocols::WakeConsumeReason::Rung,
        Some(first_arming),
    );
    // The host re-arms while the first waiter still sleeps (a distinct rendezvous id keeps
    // the fixture's idempotency keys apart; the field under test is attribution by ARMING
    // SEQUENCE, which #74 established precisely because rendezvous ids repeat).
    let second_rendezvous_id = wake_wait_fixture_rendezvous_id("rvz-88-rearm-b", &events);
    arm_lease(&events, execution, &second_rendezvous_id, head(&events));
    let second_arming = head(&events);

    let (code, value) = wake_wait_result(child);
    assert_eq!(code, Some(3), "{value}");
    let data = &value["data"];
    assert_eq!(data["receiptReadAt"], "deadline-once", "{data}");
    assert_eq!(data["missedRing"], true, "the ring was missed: {data}");
    assert_eq!(
        data["laterArmingLive"], true,
        "and someone already re-armed — different world, different move: {data}"
    );
    let at = data["lastConsumed"]["atSequence"].as_u64().unwrap();
    assert_eq!(at, consumed_at, "{data}");
    assert!(
        first_arming < at && at < second_arming,
        "the burn sits between the armings ({first_arming} < {at} < {second_arming}): {data}"
    );
}

/// G4 (#88): a previous cycle's receipt never claims a new waiting. The session's history
/// carries a full arm+burn cycle from before; the fresh arming times out in silence and the
/// answer must be W1 — attribution is by arming sequence, not by "any receipt exists".
#[cfg(windows)]
#[test]
fn a_previous_cycles_receipt_never_claims_a_new_waiting() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let execution = "exec-88-stale";
    start_execution(&events, directory.path(), execution);
    let old_rendezvous_id = wake_wait_fixture_rendezvous_id("rvz-88-stale-a", &events);
    arm_lease(&events, execution, &old_rendezvous_id, head(&events));
    let old_arming = head(&events);
    consume_lease(
        &events,
        execution,
        "session-sleeper-1",
        graphhelm_protocols::WakeConsumeReason::Rung,
        Some(old_arming),
    );
    let logical_rendezvous_id = "rvz-88-stale-b";
    let rendezvous_id = wake_wait_fixture_rendezvous_id(logical_rendezvous_id, &events);
    arm_lease_bounded(&events, execution, &rendezvous_id, head(&events), Some(2));
    let child = spawn_wake_wait(
        &events,
        execution,
        "session-sleeper-1",
        logical_rendezvous_id,
    );

    let (code, value) = wake_wait_result(child);
    assert_eq!(code, Some(3), "{value}");
    let data = &value["data"];
    assert_eq!(data["receiptReadAt"], "deadline-once", "{data}");
    assert!(
        data["lastConsumed"].is_null(),
        "the old cycle's burn is not this waiting's news: {data}"
    );
    assert_eq!(data["missedRing"], false, "{data}");
}

/// G5 (#88): an unreadable store at the deadline stays a TIMEOUT (exit 3, never a refusal —
/// the wait's verdict was already made and a failed diagnostic read must not rewrite it),
/// and says "unreadable" as its own value — an unreadable store and a silent receipt are
/// different worlds. The deletion succeeding at all doubles as proof the sidecar dropped its
/// store handle before blocking, which is the documented discipline.
#[cfg(windows)]
#[test]
fn an_unreadable_store_at_the_deadline_stays_a_timeout_and_says_unreadable() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let execution = "exec-88-unreadable";
    start_execution(&events, directory.path(), execution);
    let logical_rendezvous_id = "rvz-88-unread";
    let rendezvous_id = wake_wait_fixture_rendezvous_id(logical_rendezvous_id, &events);
    arm_lease_bounded(
        &events,
        execution,
        &rendezvous_id,
        head(&events),
        Some(RACING_WAKE_LEASE_SECONDS),
    );
    let mut child = spawn_wake_wait(
        &events,
        execution,
        "session-sleeper-1",
        logical_rendezvous_id,
    );
    wait_for_pipe(&mut child, &rendezvous_id);
    std::fs::remove_dir_all(&events)
        .expect("the sidecar dropped its handle before blocking, so the store is deletable");

    let (code, value) = wake_wait_result(child);
    assert_eq!(
        code,
        Some(3),
        "a timeout with a broken diagnostic read is still a timeout: {value}"
    );
    let data = &value["data"];
    assert_eq!(data["receiptReadAt"], "deadline-once", "{data}");
    assert_eq!(
        data["lastConsumed"], "unreadable",
        "unreadable is its own value, never null: {data}"
    );
    assert_eq!(data["missedRing"], false, "{data}");
}

/// G6 (#88): a mis-aimed burn — a consumption that destroyed THIS arming's lease while
/// naming a different one — reports BOTH facts: `lastConsumed` carries the burn (fold
/// parity: the fold records a receipt for the victim session even on a mis-burn) and
/// `misBurn` names the arming it was actually aimed at, while `missedRing` stays false —
/// the ring was never meant for this arming, and calling it missed would send the host
/// hunting for content that was addressed to a dead capture. The #74 stranded-sleeper
/// case, seen from the waiter's side, with nothing flattened.
#[cfg(windows)]
#[test]
fn a_mis_aimed_burn_is_reported_as_the_folds_own_diagnosis() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let execution = "exec-88-misburn";
    start_execution(&events, directory.path(), execution);
    let captured_rendezvous_id = wake_wait_fixture_rendezvous_id("rvz-88-mis-a", &events);
    arm_lease(&events, execution, &captured_rendezvous_id, head(&events));
    let captured_arming = head(&events);
    let logical_rendezvous_id = "rvz-88-mis-b";
    let rendezvous_id = wake_wait_fixture_rendezvous_id(logical_rendezvous_id, &events);
    arm_lease_bounded(
        &events,
        execution,
        &rendezvous_id,
        head(&events),
        Some(RACING_WAKE_LEASE_SECONDS),
    );
    let live_arming = head(&events);
    assert!(captured_arming < live_arming);
    let mut child = spawn_wake_wait(
        &events,
        execution,
        "session-sleeper-1",
        logical_rendezvous_id,
    );
    wait_for_pipe(&mut child, &rendezvous_id);
    let burned_at = consume_lease(
        &events,
        execution,
        "session-sleeper-1",
        graphhelm_protocols::WakeConsumeReason::Rung,
        Some(captured_arming),
    );

    let (code, value) = wake_wait_result(child);
    assert_eq!(code, Some(3), "{value}");
    let data = &value["data"];
    assert_eq!(data["receiptReadAt"], "deadline-once", "{data}");
    assert_eq!(
        data["lastConsumed"]["atSequence"], burned_at,
        "the burn that took this lease is reported, mis-aimed or not (fold parity): {data}"
    );
    // The WHOLE triple is pinned (C's strike 1): `reason: "rung"` right next to
    // `missedRing: false` is the exact combination a consumer will misread, so the guard
    // owns it — the reason is the burn's own, faithfully reported, while missedRing speaks
    // only for rings aimed at THIS arming; `misBurn` below is what reconciles the two.
    assert_eq!(
        data["lastConsumed"]["reason"], "rung",
        "the burn's own reason is reported faithfully even though the ring was never \
         this arming's: {data}"
    );
    assert_eq!(
        data["missedRing"], false,
        "the ring was aimed at a dead capture, never at this arming: {data}"
    );
    assert_eq!(
        data["misBurn"]["atSequence"], burned_at,
        "the mis-aim is named alongside the burn: {data}"
    );
    assert_eq!(
        data["misBurn"]["capturedArming"], captured_arming,
        "and it names the arming the burn actually captured: {data}"
    );
}

/// G7 (#88, from C's review finding): the projection's receipt map keeps only the LAST
/// consumption per session, so a full burn/re-arm/burn cycle inside one wait would erase
/// the first arming's receipt — and a deadline answer read from that map would collapse
/// the first waiter's missed ring into silence (false W1). The answer must come from the
/// LOG, which forgets nothing: after a second complete cycle, the first arming's waiter
/// still reports ITS OWN burn, exactly.
#[cfg(windows)]
#[test]
fn a_second_cycles_burn_never_erases_the_first_armings_receipt() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let execution = "exec-88-twocycle";
    start_execution(&events, directory.path(), execution);
    let first_logical_rendezvous_id = "rvz-88-cycle-a";
    let first_rendezvous_id = wake_wait_fixture_rendezvous_id(first_logical_rendezvous_id, &events);
    arm_lease_bounded(
        &events,
        execution,
        &first_rendezvous_id,
        head(&events),
        Some(RACING_WAKE_LEASE_SECONDS),
    );
    let first_arming = head(&events);
    let mut child = spawn_wake_wait(
        &events,
        execution,
        "session-sleeper-1",
        first_logical_rendezvous_id,
    );
    wait_for_pipe(&mut child, &first_rendezvous_id);
    // Sequence-spacer, same reason as G1's: an adjacent burn is guessable by armed+1.
    arm_decoy(&events, execution, "rvz-88-decoy-cycle");
    // Cycle one: MY burn, honest, rung — the byte never crosses.
    let my_burn = consume_lease(
        &events,
        execution,
        "session-sleeper-1",
        graphhelm_protocols::WakeConsumeReason::Rung,
        Some(first_arming),
    );
    // Cycle two, complete, while the first waiter still sleeps: re-arm and burn THAT.
    // After this, wake_last_consumed[session] holds the SECOND burn only.
    let second_rendezvous_id = wake_wait_fixture_rendezvous_id("rvz-88-cycle-b", &events);
    arm_lease(&events, execution, &second_rendezvous_id, head(&events));
    let second_arming = head(&events);
    consume_lease(
        &events,
        execution,
        "session-sleeper-1",
        graphhelm_protocols::WakeConsumeReason::Rung,
        Some(second_arming),
    );

    let (code, value) = wake_wait_result(child);
    assert_eq!(code, Some(3), "{value}");
    let data = &value["data"];
    assert_eq!(data["receiptReadAt"], "deadline-once", "{data}");
    assert_eq!(
        data["lastConsumed"]["atSequence"], my_burn,
        "the FIRST arming's own burn — not the second cycle's, not silence: {data}"
    );
    assert_eq!(
        data["missedRing"], true,
        "a rung burn of this arming stays a missed ring no matter how many cycles \
         followed it: {data}"
    );
    assert_eq!(
        data["laterArmingLive"], false,
        "the second arming was itself burned, so nothing is live: {data}"
    );
    assert!(
        data.get("misBurn").is_none(),
        "both burns were honestly aimed: {data}"
    );
}

/// G8 (#88, row 4 from C's totality check): the fourth state — this arming's lease burned
/// `stale_rendezvous`, honestly aimed, while the waiter lived. Neither silence (something
/// happened to you) nor a missed ring (nobody rang you) nor a mis-aim (the sweep aimed at
/// YOU and judged your rendezvous dead) — reachable today when a ring lands before the
/// sidecar's pipe exists. No boolean names it; its identification rule is the reason
/// strike 1 made readable: `lastConsumed.reason == "stale_rendezvous"` with `misBurn`
/// absent. This guard pins that triple so row 4 is a named world, not whatever falls out.
#[cfg(windows)]
#[test]
fn a_lease_burned_stale_while_its_waiter_lived_names_the_rejection() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let execution = "exec-88-stale-alive";
    start_execution(&events, directory.path(), execution);
    let logical_rendezvous_id = "rvz-88-stale-alive";
    let rendezvous_id = wake_wait_fixture_rendezvous_id(logical_rendezvous_id, &events);
    arm_lease_bounded(
        &events,
        execution,
        &rendezvous_id,
        head(&events),
        Some(RACING_WAKE_LEASE_SECONDS),
    );
    let armed_at = head(&events);
    let mut child = spawn_wake_wait(
        &events,
        execution,
        "session-sleeper-1",
        logical_rendezvous_id,
    );
    wait_for_pipe(&mut child, &rendezvous_id);
    let burned_at = consume_lease(
        &events,
        execution,
        "session-sleeper-1",
        graphhelm_protocols::WakeConsumeReason::StaleRendezvous,
        Some(armed_at),
    );

    let (code, value) = wake_wait_result(child);
    assert_eq!(code, Some(3), "{value}");
    let data = &value["data"];
    assert_eq!(data["receiptReadAt"], "deadline-once", "{data}");
    assert_eq!(
        data["lastConsumed"]["reason"], "stale_rendezvous",
        "the rejection is named in the burn's own words: {data}"
    );
    assert_eq!(data["lastConsumed"]["atSequence"], burned_at, "{data}");
    assert_eq!(
        data["missedRing"], false,
        "a stale burn is not a missed ring — no ring ever carried content for it: {data}"
    );
    assert!(
        data.get("misBurn").is_none(),
        "the sweep aimed at this arming; being judged stale is not a mis-aim: {data}"
    );
    assert_eq!(data["laterArmingLive"], false, "{data}");
}

/// The sidecar's startup budget must fit INSIDE the lease it is racing (#413 B).
///
/// `cargo test` runs this binary's tests in parallel, so an ordinary run spawns as many sidecars at
/// once as there are tests. This idle-machine benchmark explains the original failure: even without
/// outside load, parallel startup consumed most of the old short lease before the scenario began.
/// It does not establish a supported startup deadline. Each sample used a 600 s lease so nothing
/// could expire mid-measurement and panicked if any child died before its rendezvous:
///
/// ```text
/// spawn -> rendezvous visible      min      p50      p90      max
///   1  sequential                   120      313     1013     1268 ms
///  12  concurrent                   351      396      421      433 ms
///  28  concurrent (this binary)    1766     1840     1926     1953 ms
/// ```
///
/// The maturity clock starts at the ARMING APPEND, before the spawn. So the window the scenario
/// actually gets is `lease - startup`, and at the binary's own width that leaves a `Some(2)` lease
/// between fifty and two hundred and thirty milliseconds -- on a machine where nothing else is
/// running. The scaling is superlinear (2.3x the width, 4.5x the time), so a colder cache or an
/// antivirus pass over fresh binaries closes it.
///
/// The guard holds both requirements: the startup hang catcher allows at least 30 seconds for a
/// healthy slow Windows start, and the tightest lease it races remains strictly longer. Otherwise
/// the test either rejects a healthy start or fails downstream after the lease has already matured.
#[cfg(windows)]
#[test]
fn the_pipe_wait_is_bounded_below_the_leases_it_races() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/wake_http.rs"),
    )
    .expect("this test file is readable");

    let pipe_bound = PIPE_STARTUP_HANG_CATCHER_SECONDS;

    // Every lease bound armed by a test that then waits for a rendezvous.
    let mut raced: Vec<u64> = Vec::new();
    for block in source.split("\nfn ").skip(1) {
        if block.starts_with("the_pipe_wait_is_bounded_below_the_leases_it_races") {
            continue;
        }
        if !block.contains("wait_for_pipe(") {
            continue;
        }
        for (index, _) in block.match_indices("arm_lease_bounded(") {
            let call = &block[index..];
            if let Some(some) = call.split_once("Some(")
                && let Some(bound) = some.1.split(')').next()
            {
                let seconds = match bound.trim() {
                    "RACING_WAKE_LEASE_SECONDS" => RACING_WAKE_LEASE_SECONDS,
                    digits => digits
                        .parse::<u64>()
                        .expect("a racing lease bound is a known constant or literal seconds"),
                };
                raced.push(seconds);
            }
        }
    }

    assert!(
        !raced.is_empty(),
        "no test was found that arms a bounded lease and then waits for a rendezvous, so the \
         comparison below would pass vacuously -- the extraction is broken, not the code"
    );
    let tightest = *raced.iter().min().expect("non-empty");
    assert!(
        pipe_bound >= 30,
        "the pipe wait must allow a healthy slow Windows start: {pipe_bound} < 30"
    );
    assert!(
        pipe_bound < tightest,
        "the pipe wait allows {pipe_bound}s while the tightest lease it races matures in \
         {tightest}s. A wait longer than the lease cannot report a slow sidecar: by the time it \
         gives up, the lease has been mature for {}s and the failure lands downstream saying \
         nothing about startup. Lease bounds seen: {raced:?}",
        pipe_bound - tightest
    );
}
