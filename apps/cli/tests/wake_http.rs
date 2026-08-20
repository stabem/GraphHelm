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
/// The waiter no longer takes a rendezvous or a deadline from its caller: both come from the
/// lease this session armed. So the harness arms one, and the test's "timeout" is now the
/// bound the sleeper DECLARED — which is the point of the step.
fn spawn_wake_wait(events: &Path, execution: &str, session: &str) -> Child {
    Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
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
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

/// Reaps a sidecar and returns its exit status together with everything it wrote to
/// stderr. The stderr pipe was created at spawn and then DISCARDED, so an exit-2 refusal
/// (the sidecar's own diagnosis) was invisible and masqueraded as whatever assertion
/// failed downstream. Reading after `wait` cannot deadlock here: the sidecar's stderr is
/// at most a refusal line, far below the pipe buffer.
#[cfg(windows)]
fn reap_with_stderr(child: &mut Child) -> (std::process::ExitStatus, String) {
    let status = child.wait().unwrap();
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        use std::io::Read as _;
        let _ = pipe.read_to_string(&mut stderr);
    }
    (status, stderr)
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
    arm_lease_bounded(&events, "exec-sidecar-ring", "rvz-sidecar-1", 1, Some(20));
    let child = spawn_wake_wait(&events, "exec-sidecar-ring", "session-sleeper-1");
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
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    start_execution(&events, directory.path(), "exec-sidecar-timeout");
    // The bound is DECLARED on the lease now, so exit 3 can only mean the deadline the sleeper
    // itself set. It stopped being "the number I happened to type ran out".
    arm_lease_bounded(
        &events,
        "exec-sidecar-timeout",
        "rvz-sidecar-timeout",
        1,
        Some(1),
    );
    let child = spawn_wake_wait(&events, "exec-sidecar-timeout", "session-sleeper-1");
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

    // A arms ITSELF through the proxy FIRST — the waiter now takes its rendezvous and its
    // deadline from its own lease, so the lease has to exist before it can wait on one. Its
    // one read of the store is local and is not an API request, so the measurement below is
    // unchanged: zero requests cross the proxy between sleep and ring.
    let rendezvous = "rdv-choreo-a";
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
    let mut sidecar = spawn_wake_wait(&events, execution, &armed_session);
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
        // of burning the 10 s bound and blaming the pipe. Exit 2 (refusal) lives on stderr.
        if let Some(status) = sidecar.try_wait().unwrap() {
            let mut stderr = String::new();
            if let Some(mut pipe) = sidecar.stderr.take() {
                use std::io::Read as _;
                let _ = pipe.read_to_string(&mut stderr);
            }
            panic!("the sidecar died before creating its rendezvous: {status:?}: {stderr}");
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
    let (sidecar_end, sidecar_stderr) = reap_with_stderr(&mut sidecar);
    assert!(
        sidecar_end.success(),
        "the sidecar must exit 0 on the ring: {sidecar_end:?}: {sidecar_stderr}"
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
        "the head makes the cursor readable: armed at #{armed_cursor}, rang at #{rang_at}:          {answer}"
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
    arm_lease_bounded(&events, execution, "rdv-deadman", 1, Some(2));
    drop(guard); // the serve dies; nothing will ever ring.
    let _ = (base, token);

    let mut sidecar = spawn_wake_wait(&events, execution, "session-sleeper-1");
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
// evidence lives in .factory/archive/pair-events-corrupted-2026-08-16, preserved intact).
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
                std::thread::spawn(move || {
                    barrier.wait();
                    post_json(
                        &base,
                        &token,
                        &format!("/v1/executions/{execution}/signal"),
                        &format!("race-{round}-{lane}"),
                        &signal_body(&format!("signal-race-{round}-{lane}"), &evidence),
                    )
                })
            })
            .collect();
        for post in posts {
            let (status, reply) = post.join().unwrap();
            assert_eq!(status, 200, "the mutation itself always lands: {reply}");
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
    let armed_at = head(&events);
    let sleeper = Sleeper::arm("rvz-delay-1", &events);
    arm_lease(&events, execution, "rvz-delay-1", armed_at);
    let (_guard, base, token) =
        serve_with_env(&events, &[("GRAPHHELM_TEST_WAKE_PHASE3_DELAY_MS", "2000")]);

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

/// Waits (bounded) for the sidecar's rendezvous to exist — the same condition-wait the
/// choreography test uses; a fixed sleep would be a timing assumption wearing a step's
/// clothes.
#[cfg(windows)]
fn wait_for_pipe(rendezvous_id: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let expected = format!("graphhelm-wake-{rendezvous_id}");
    while !std::fs::read_dir("//./pipe").is_ok_and(|entries| {
        entries.filter_map(Result::ok).any(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.eq_ignore_ascii_case(&expected))
        })
    }) {
        assert!(
            Instant::now() < deadline,
            "the sidecar never created its rendezvous {expected}"
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
    arm_lease_bounded(&events, execution, "rvz-88-missed", head(&events), Some(3));
    let armed_at = head(&events);
    let child = spawn_wake_wait(&events, execution, "session-sleeper-1");
    wait_for_pipe("rvz-88-missed");
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
    let execution = "exec-88-silent";
    start_execution(&events, directory.path(), execution);
    arm_lease_bounded(&events, execution, "rvz-88-silent", head(&events), Some(1));
    let started = Instant::now();
    let child = spawn_wake_wait(&events, execution, "session-sleeper-1");

    let (code, value) = wake_wait_result(child);
    // The deadline never moves for the receipt read: one open, no waiting. The bound is
    // deliberately loose against machine load (declared bound 1s + spawn + one store
    // open), but a wait-loop smuggled into the final read blows straight through it —
    // this is the blade sabotage s4 falls on, not a race to win.
    assert!(
        started.elapsed() < Duration::from_secs(6),
        "the deadline answer arrives promptly — the receipt read never waits: {:?}",
        started.elapsed()
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
    arm_lease_bounded(&events, execution, "rvz-88-rearm-a", head(&events), Some(3));
    let first_arming = head(&events);
    let child = spawn_wake_wait(&events, execution, "session-sleeper-1");
    wait_for_pipe("rvz-88-rearm-a");
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
    arm_lease(&events, execution, "rvz-88-rearm-b", head(&events));
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
    arm_lease(&events, execution, "rvz-88-stale-a", head(&events));
    let old_arming = head(&events);
    consume_lease(
        &events,
        execution,
        "session-sleeper-1",
        graphhelm_protocols::WakeConsumeReason::Rung,
        Some(old_arming),
    );
    arm_lease_bounded(&events, execution, "rvz-88-stale-b", head(&events), Some(2));
    let child = spawn_wake_wait(&events, execution, "session-sleeper-1");

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
    arm_lease_bounded(&events, execution, "rvz-88-unread", head(&events), Some(3));
    let child = spawn_wake_wait(&events, execution, "session-sleeper-1");
    wait_for_pipe("rvz-88-unread");
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
    arm_lease(&events, execution, "rvz-88-mis-a", head(&events));
    let captured_arming = head(&events);
    arm_lease_bounded(&events, execution, "rvz-88-mis-b", head(&events), Some(3));
    let live_arming = head(&events);
    assert!(captured_arming < live_arming);
    let child = spawn_wake_wait(&events, execution, "session-sleeper-1");
    wait_for_pipe("rvz-88-mis-b");
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
    arm_lease_bounded(&events, execution, "rvz-88-cycle-a", head(&events), Some(4));
    let first_arming = head(&events);
    let child = spawn_wake_wait(&events, execution, "session-sleeper-1");
    wait_for_pipe("rvz-88-cycle-a");
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
    arm_lease(&events, execution, "rvz-88-cycle-b", head(&events));
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
    arm_lease_bounded(
        &events,
        execution,
        "rvz-88-stale-alive",
        head(&events),
        Some(3),
    );
    let armed_at = head(&events);
    let child = spawn_wake_wait(&events, execution, "session-sleeper-1");
    wait_for_pipe("rvz-88-stale-alive");
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
