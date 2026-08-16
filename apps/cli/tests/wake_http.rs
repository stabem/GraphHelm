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
    let mut child = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args([
            "serve",
            "--events",
            events.to_str().unwrap(),
            "--bind",
            "127.0.0.1:0",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
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

/// The sleeper half: creates the platform rendezvous for `rendezvous_id` (the Task 0/1
/// derivation: a fixed local prefix plus the opaque id) and returns a handle whose
/// `wait(timeout)` blocks for the ring, returning the bytes received.
#[cfg(windows)]
struct Sleeper {
    handle: std::thread::JoinHandle<(Vec<u8>, Vec<String>)>,
}

#[cfg(windows)]
impl Sleeper {
    /// `events`: on the INSTANT the byte arrives, the sleeper snapshots the store's event
    /// kinds — the honest detector for "a ring implies a durable trigger" (checking after
    /// the HTTP response returns would be blind to an early ring).
    fn arm(rendezvous_id: &str, events: &Path) -> Self {
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
        Self { handle }
    }

    fn wait(self) -> (Vec<u8>, Vec<String>) {
        self.handle.join().unwrap()
    }
}

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
    let armed_at = head(&events);
    let sleeper = Sleeper::arm("rvz-ring-1", &events);
    arm_lease(&events, "exec-wake-ring", "rvz-ring-1", armed_at);
    let (_guard, address, token) = serve(&events);

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
    assert_eq!(bytes.len(), 1, "exactly one content-free byte crossed");
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
fn a_burned_lease_never_rings_twice_and_no_ring_without_a_fresh_append() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    start_execution(&events, directory.path(), "exec-wake-burn");
    let armed_at = head(&events);
    let sleeper = Sleeper::arm("rvz-burn-1", &events);
    arm_lease(&events, "exec-wake-burn", "rvz-burn-1", armed_at);
    let (_guard, address, token) = serve(&events);

    // First trigger rings and burns.
    let (status, _reply) = post_json(
        &address,
        &token,
        "/v1/executions/exec-wake-burn/signal",
        "wake-burn-1",
        &signal_body("signal-burn-1", &directory.path().join("ev1.json")),
    );
    assert_eq!(status, 200);
    assert_eq!(sleeper.wait().0.len(), 1, "the first trigger rings");

    // Re-arm the PIPE but not the lease: a second trigger must NOT ring (lease burned).
    let second = Sleeper::arm("rvz-burn-1", &events);
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

/// Spawns `graphhelm wake-wait` and returns the child (the sidecar CREATES the rendezvous).
#[cfg(windows)]
fn spawn_wake_wait(rendezvous_id: &str, timeout_seconds: &str) -> Child {
    Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args([
            "wake-wait",
            "--rendezvous-id",
            rendezvous_id,
            "--timeout",
            timeout_seconds,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
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
    let child = spawn_wake_wait("rvz-sidecar-1", "20");
    // Give the sidecar a moment to create the rendezvous, then ring with SENTINEL bytes.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match ring_pipe("rvz-sidecar-1", b"SENTINEL-HOSTILE-PAYLOAD") {
            Ok(()) => break,
            Err(_) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => panic!("the rendezvous never appeared: {error}"),
        }
    }
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
    let child = spawn_wake_wait("rvz-sidecar-timeout", "1");
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(3), "timeout exits 3: {output:?}");

    let output = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args([
            "wake-wait",
            "--rendezvous-id",
            "NOT A VALID ID!!",
            "--timeout",
            "1",
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(2),
        "a bad id refuses: {output:?}"
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
struct CountingProxy {
    address: String,
    connections: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

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

    // The sidecar first: it CREATES the rendezvous the serve will ring.
    let rendezvous = "rdv-choreo-a";
    let mut sidecar = spawn_wake_wait(rendezvous, "30");
    std::thread::sleep(std::time::Duration::from_millis(400));

    // A arms ITSELF through the proxy (this is A's last request before sleep).
    let replies = mcp_via(
        &proxy.address,
        &token,
        &initialize_lines(vec![serde_json::json!({"jsonrpc": "2.0", "id": 2,
            "method": "tools/call", "params": {"name": "wake_arm",
            "arguments": {"executionId": execution, "rendezvousId": rendezvous}}})]),
    );
    let armed = &replies[1]["result"];
    assert_eq!(armed["isError"], false, "{replies:?}");
    let armed_cursor: u64 =
        serde_json::from_str::<serde_json::Value>(armed["content"][0]["text"].as_str().unwrap())
            .unwrap()["data"]["armedCursor"]
            .as_u64()
            .unwrap();

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
    let sidecar_end = sidecar.wait().unwrap();
    assert!(
        sidecar_end.success(),
        "the sidecar must exit 0 on the ring: {sidecar_end:?}"
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
    arm_lease(&events, execution, "rdv-deadman", 1);
    drop(guard); // the serve dies; nothing will ever ring.
    let _ = (base, token);

    let mut sidecar = spawn_wake_wait("rdv-deadman", "2");
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
