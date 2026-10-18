//! The un-hardened HTTP client the CLI's integration tests use to speak to a `graphhelm serve`
//! listener, in ONE place (#179).
//!
//! **Why five copies existed, and why the reason no longer holds.** `runtime_http.rs`'s header
//! says the harness was copied because *"integration test binaries in this workspace do not share
//! code across files (`apps/cli` is bin-only, no `[lib]` target)"*. That is true of the mechanism
//! it names — a bin-only crate exports nothing to `use` — and it is not true of the one used here:
//! a module under `tests/` is compiled INTO each test binary that declares it, no library
//! involved. `adapters/postgres-event-store/tests/support/mod.rs` has done exactly this for seven
//! binaries since M08.
//!
//! **What is shared and what is not.** The four un-hardened callers had the same algorithm and
//! differed only in the wording of their `expect`/`Error::other` messages — measured body by body
//! before this file existed. The most informative wording won, and one `.unwrap()` on the port
//! became a named `expect`. `api_http.rs` keeps its OWN `raw_request`: it bounds the connect with
//! `connect_with_retry` and a hang guard because an unbounded connect under a full accept backlog
//! retransmits for ~21s and outlives the caller's own deadline. That is a different function, not
//! a drifted copy, and hoisting it here would push a retry policy onto suites that never asked for
//! one — which is the failure mode this file is supposed to prevent, not commit.
//!
//! `#![allow(dead_code)]` is load-bearing exactly as it is in the postgres support module: each
//! test binary uses a subset of these helpers, so `rustc` — which sees one binary at a time —
//! reports the rest as dead in that binary while they are live in another.
#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

pub mod time_scale;

/// The read/write budget for a test request. 15 seconds is what three of the four copies already
/// carried; `resume_atomicity.rs` carried 5 with nothing saying why, and a shorter budget there
/// only makes a slow listener look like a failure sooner.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

pub struct RawResponse {
    pub status: u16,
    pub body: String,
}

pub fn raw_request(url: &str, token: Option<&str>) -> std::io::Result<RawResponse> {
    raw_request_within(url, token, time_scale::scaled(REQUEST_TIMEOUT))
}

/// `raw_request` with its budget named: the most each of the connect, a write and a read may
/// take. #549: a request to a slow server failed a whole-package run with `os error 10060`, which
/// reads like a failed CONNECT and is what Windows reports for a READ that outlasts its timeout.
/// The connect had no bound at all. Now all three share one budget, the budget scales with
/// `GRAPHHELM_TEST_TIME_SCALE` like every other hang catcher, and a cell in `runtime_http.rs`
/// pins what running out of it looks like.
pub fn raw_request_within(
    url: &str,
    token: Option<&str>,
    budget: Duration,
) -> std::io::Result<RawResponse> {
    let (host, port, path) = split_url(url);
    let address = (host.as_str(), port)
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| std::io::Error::other(format!("no address resolved for {host}:{port}")))?;
    let mut stream = TcpStream::connect_timeout(&address, budget)?;
    stream.set_read_timeout(Some(budget))?;
    stream.set_write_timeout(Some(budget))?;

    let mut request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
    if let Some(token) = token {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes())?;

    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;
    parse_response(&String::from_utf8_lossy(&raw))
}

pub fn split_url(url: &str) -> (String, u16, String) {
    let rest = url
        .strip_prefix("http://")
        .expect("test helper URLs are always http://host:port/path");
    let (authority, path) = match rest.split_once('/') {
        Some((authority, path)) => (authority, format!("/{path}")),
        None => (rest, "/".to_owned()),
    };
    let (host, port) = authority
        .split_once(':')
        .expect("test helper URLs always carry an explicit port");
    (
        host.to_owned(),
        port.parse().expect("port must be numeric"),
        path,
    )
}

pub fn parse_response(text: &str) -> std::io::Result<RawResponse> {
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| std::io::Error::other("malformed HTTP response: no header/body split"))?;
    let status_line = head
        .lines()
        .next()
        .ok_or_else(|| std::io::Error::other("malformed HTTP response: no status line"))?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| std::io::Error::other("malformed HTTP response: no status code"))?;
    Ok(RawResponse {
        status,
        body: body.to_owned(),
    })
}

/// Where `graphhelm serve` writes its bearer token: beside the events directory, as
/// `<events>.token`. Beside, not inside: `LocalEventRepository::open` refuses a store directory
/// that holds an entry outside its own allowlist (#1313: one copy, was eleven).
pub fn token_path(events: &std::path::Path) -> std::path::PathBuf {
    let mut name = events.file_name().map_or_else(
        || std::ffi::OsString::from("events"),
        std::ffi::OsStr::to_os_string,
    );
    name.push(".token");
    events.with_file_name(name)
}

/// The token the server wrote at `path`, polled until the file is non-empty (the server writes
/// it after binding). Returned as written, untrimmed (#1313: one copy, was eight).
pub fn read_token(path: &std::path::Path) -> String {
    let deadline = std::time::Instant::now() + time_scale::scaled(Duration::from_secs(5));
    loop {
        if let Ok(contents) = std::fs::read_to_string(path)
            && !contents.is_empty()
        {
            return contents;
        }
        if std::time::Instant::now() >= deadline {
            panic!("the server never wrote a readable token file at {path:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Polls `GET {base}/health` (base is `http://host:port`) through [`raw_request`] until it
/// answers 200 (#1313: one copy, was five). The bare-address forms in `mcp_stdio.rs` and
/// `monitor_http.rs`, and `api_http.rs`'s hardened one, are different functions and stay.
pub fn wait_for_health(base: &str) {
    let deadline = std::time::Instant::now() + time_scale::scaled(Duration::from_secs(5));
    loop {
        if let Ok(response) = raw_request(&format!("{base}/health"), None)
            && response.status == 200
        {
            return;
        }
        if std::time::Instant::now() >= deadline {
            panic!("the server never answered /health at {base}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// `GET url` through [`raw_request`], the body parsed as JSON; panics naming the URL on a failed
/// request or a non-JSON body (#1313: one copy, was four).
pub fn get_json(url: &str, token: Option<&str>) -> serde_json::Value {
    let response =
        raw_request(url, token).unwrap_or_else(|error| panic!("request to {url} failed: {error}"));
    serde_json::from_str(&response.body).unwrap_or_else(|error| {
        panic!(
            "response body from {url} was not JSON ({error}): {:?}",
            response.body
        )
    })
}

/// The repository root, canonicalized, for naming fixtures that live in-tree (#1313: one copy,
/// was nine). The uncanonicalized forms stay in their files: a suite that compares against
/// non-canonical paths would see a `\\?\` prefix on Windows.
pub fn repository_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// #534: approving is the owner's; `graphhelm init` makes the project's owner store and token.
/// Returns the token's PATH, not its contents (#1313: one copy, was seven).
pub fn owner_token(project: &std::path::Path) -> String {
    // init ignores all of `.graphhelm/`; a real project keeps its flows tracked and its owner
    // store (events, token, keys, owner records) out of git, so the test ignores exactly that.
    let gitignore = project.join(".gitignore");
    let kept = std::fs::read(&gitignore).ok();
    let out = graphhelm_process::command(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["--json", "init", "--project"])
        .arg(project)
        .args(["--harness", "claude-code"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let mut ignore = kept.unwrap_or_default();
    ignore.extend_from_slice(
        b"
.graphhelm/*
!.graphhelm/journeys/
/.mcp.json
",
    );
    std::fs::write(&gitignore, ignore).unwrap();
    project
        .join(".graphhelm/events.token")
        .to_string_lossy()
        .into_owned()
}

/// #1309 card 1c: the owner approves only a flow the preview runner recorded passing on the same
/// digest and scope commit. Tests that approve cannot drive a browser, so this appends the
/// runner's record straight into the project's owner store, as a process that writes the store
/// could (the limit `journey_owner` states). The key is what `journey preview --read` reports. A
/// flow the preview refuses has no key and gets no record; approve then refuses on its findings.
pub fn passing_preview(project: &std::path::Path, id: &str) {
    use graphhelm_protocols::{
        ActorId, EventKind, ExecutionId, NewEvent, OpaqueId, PersistedActor, PersistedActorType,
        ProjectId, RepositoryScope, Sensitivity, SignalRecorded, SignalSeverity, SignalSourceKind,
        WorkspaceId,
    };
    let out = graphhelm_process::command(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["--json", "journey", "preview", id, "--read", "--project"])
        .arg(project)
        .output()
        .unwrap();
    let reply: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let (Some(digest), Some(commit)) = (
        reply["data"]["digest"].as_str(),
        reply["data"]["commit"].as_str(),
    ) else {
        return;
    };
    let events = project.join(".graphhelm/events");
    let store = graphhelm_events::LocalEventRepository::open(
        &events,
        std::sync::Arc::new(PreviewClock),
        std::sync::Arc::new(PreviewIds::default()),
    )
    .unwrap();
    let scope = RepositoryScope::new(
        WorkspaceId::parse("workspace-local").unwrap(),
        ProjectId::parse("project-local").unwrap(),
        Some(ExecutionId::parse("graphhelm-owner").unwrap()),
    );
    if store.next_sequence(&scope, "graphhelm-owner").unwrap() == 1 {
        start_owner_execution(project, &events);
    }
    let signal_id = {
        use sha2::{Digest, Sha256};
        let hash = Sha256::digest(
            format!(
                "previewed
{id}
{digest}
{commit}"
            )
            .as_bytes(),
        );
        format!("journey-previewed-{}", &hex::encode(hash)[..32])
    };
    let next = store.next_sequence(&scope, "graphhelm-owner").unwrap();
    let event = NewEvent::new(
        // A test may record the same preview twice (approve, edit back, approve again).
        OpaqueId::parse(format!("{signal_id}-{next}")).unwrap(),
        PersistedActor::new(
            PersistedActorType::System,
            ActorId::parse("journey-preview-runner").unwrap(),
        ),
        Sensitivity::Internal,
        EventKind::SignalRecorded(SignalRecorded {
            to: None,
            reply_to: None,
            execution_id: OpaqueId::parse("graphhelm-owner").unwrap(),
            signal_id: OpaqueId::parse(signal_id).unwrap(),
            source_kind: SignalSourceKind::Node,
            source_id: OpaqueId::parse("journey-preview-runner").unwrap(),
            kind: "journey_flow_previewed".into(),
            severity: SignalSeverity::Low,
            envelope_sha256: graphhelm_protocols::RawSha256::parse("0".repeat(64)).unwrap(),
            scoped_agent_authenticated: None,
        }),
        vec![],
        vec![],
    );
    let request = graphhelm_events::PreparedAppend::new(
        scope,
        OpaqueId::parse("graphhelm-owner").unwrap(),
        next,
        vec![event],
        vec![],
        vec![],
    )
    .unwrap();
    store.append_atomic(&request).unwrap();
}

/// The reserved owner execution, started as `journey_owner` starts it on first use.
fn start_owner_execution(project: &std::path::Path, events: &std::path::Path) {
    let graph = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/schemas/valid/graph.json");
    let mut graph: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(graph).unwrap()).unwrap();
    graph["metadata"]["executionId"] = "graphhelm-owner".into();
    graph["metadata"]["id"] = "graphhelm-owner-records".into();
    graph["metadata"]["name"] = "Owner records".into();
    let records = project.join(".graphhelm/owner-records");
    std::fs::create_dir_all(&records).unwrap();
    let file = records.join("owner-graph.json");
    std::fs::write(&file, graph.to_string()).unwrap();
    let out = graphhelm_process::command(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["--json", "execution", "start", "--events"])
        .arg(events)
        .args(["--execution", "graphhelm-owner", "--file"])
        .arg(&file)
        .args(["--mode", "supervised"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

struct PreviewClock;
impl graphhelm_protocols::Clock for PreviewClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
    }
}

#[derive(Default)]
struct PreviewIds(std::sync::atomic::AtomicU64);
impl graphhelm_protocols::IdGenerator for PreviewIds {
    fn next_id(&self, prefix: &'static str) -> String {
        format!(
            "{prefix}-preview-{}",
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1
        )
    }
}
