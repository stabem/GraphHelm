//! #1325: one `graphhelm mcp --discover` process follows whichever Runtime serves its port.
//!
//! Two event stores, two tokens, one fixed loopback port. Runtime A serves first, is killed, and
//! Runtime B takes the same port. The SAME bridge process keeps answering, because it reads the
//! discovery record B published and confirms B's `/health` instance before sending B's token. The
//! control: a bridge pinned to A's `--token-file` is refused by B, so the two stores really do hold
//! different tokens. A stale record (nothing on the port, or a record another process did not
//! write) is refused with a diagnostic that names the remedy, and no token ever reaches output.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

struct Killed(Child);
impl Drop for Killed {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A loopback port for a Runtime this test starts: bound to learn a free number, then released so
/// the child can bind it.
fn free_port() -> u16 {
    first_unused(|| {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        (listener, port)
    })
}

/// The ports this test binary has handed out, across its tests and threads.
static HANDED_OUT: std::sync::Mutex<std::collections::BTreeSet<u16>> =
    std::sync::Mutex::new(std::collections::BTreeSet::new());

/// The port `free_port` hands out: the first candidate `offer` yields (each with the listener
/// that holds it) that this binary has not handed out before. A refused candidate's listener is
/// kept until a port is chosen, so the OS cannot offer that number again in the meantime.
fn first_unused<L>(mut offer: impl FnMut() -> (L, u16)) -> u16 {
    let mut refused = Vec::new();
    loop {
        let (listener, port) = offer();
        if HANDED_OUT.lock().unwrap().insert(port) {
            return port;
        }
        refused.push(listener);
    }
}

/// #549: the operating system may offer a port it has just seen released, so two `free_port`
/// calls in one test could get the same number (seen once on a busy machine: 53068 twice, and
/// `project_discovery_routes_two_projects_to_distinct_ports_and_tokens` failed at its own
/// `assert_ne!`). A tight loop does not show it (3000 bind-and-release calls here gave 3000
/// distinct ports), so the offers are scripted: whatever the OS offers, a number is handed out
/// once. Cost: microseconds, no socket.
#[test]
fn a_port_the_os_offers_again_is_never_handed_out_twice() {
    // Ports 1 and 2 are far below the dynamic range, so no real `free_port` call took them.
    let mut offers = [1_u16, 1, 1, 2].into_iter();
    let mut offer = || ((), offers.next().expect("an offer the helper should not need"));
    assert_eq!(first_unused(&mut offer), 1);
    assert_eq!(first_unused(&mut offer), 2, "port 1 was handed out twice");
}

fn health(port: u16) -> Option<serde_json::Value> {
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    write!(
        stream,
        "GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut text = String::new();
    std::io::Read::read_to_string(&mut stream, &mut text).ok()?;
    let body = text.split("\r\n\r\n").nth(1)?;
    serde_json::from_str(body).ok()
}

/// Starts `serve` on `port` against `events`, publishing into `registry`, and waits until the
/// record names the instance `/health` reports.
fn serve(events: &Path, port: u16, registry: &Path) -> Killed {
    serve_with_project(events, port, registry, None)
}

fn serve_with_project(events: &Path, port: u16, registry: &Path, project: Option<&Path>) -> Killed {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
    command
        .args(["serve", "--events", events.to_str().unwrap(), "--bind"])
        .arg(format!("127.0.0.1:{port}"));
    if let Some(project) = project {
        command.args(["--project", project.to_str().unwrap()]);
    }
    let child = command
        .env("GRAPHHELM_RUNTIME_DIR", registry)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let guard = Killed(child);
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let record = std::fs::read(registry.join(format!("{port}.json")))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
        if let (Some(record), Some(health)) = (record, health(port))
            && record["instance"] == health["data"]["instance"]
            && record["events"]
                .as_str()
                .is_some_and(|e| e.ends_with(events.file_name().unwrap().to_str().unwrap()))
        {
            return guard;
        }
        assert!(Instant::now() < deadline, "serve never published on {port}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn stop(mut runtime: Killed, port: u16) {
    let _ = runtime.0.kill();
    let _ = runtime.0.wait();
    let deadline = Instant::now() + Duration::from_secs(30);
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
        assert!(Instant::now() < deadline, "port {port} never freed");
        std::thread::sleep(Duration::from_millis(100));
    }
}

struct Bridge {
    _child: Killed,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    transcript: String,
    next: u64,
}

impl Bridge {
    fn start(port: u16, registry: &Path, token_args: &[&str]) -> Self {
        let child = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
            .args(["mcp", "--url"])
            .arg(format!("http://127.0.0.1:{port}"))
            .args(token_args)
            .args(["--actor", "discovery-test"])
            .env("GRAPHHELM_RUNTIME_DIR", registry)
            .env_remove("GRAPHHELM_API_TOKEN")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Self::from_child(child)
    }

    fn start_project(project: &Path, registry: &Path) -> Self {
        let child = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
            .args(["mcp", "--discover", "--project"])
            .arg(project)
            .args(["--actor", "project-discovery-test"])
            .env("GRAPHHELM_RUNTIME_DIR", registry)
            .env_remove("GRAPHHELM_API_TOKEN")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Self::from_child(child)
    }

    fn start_in_project_cwd(project: &Path, registry: &Path) -> Self {
        let child = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
            .args(["mcp", "--discover", "--actor", "project-discovery-test"])
            .current_dir(project)
            .env("GRAPHHELM_RUNTIME_DIR", registry)
            .env_remove("GRAPHHELM_API_TOKEN")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Self::from_child(child)
    }

    fn from_child(mut child: Child) -> Self {
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut bridge = Self {
            _child: Killed(child),
            stdin,
            stdout,
            transcript: String::new(),
            next: 1,
        };
        bridge.call(
            "initialize",
            serde_json::json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}}),
        );
        writeln!(
            bridge.stdin,
            "{}",
            serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
        )
        .unwrap();
        bridge
    }

    fn call(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
        let id = self.next;
        self.next += 1;
        writeln!(
            self.stdin,
            "{}",
            serde_json::json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
        )
        .unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        self.transcript.push_str(&line);
        serde_json::from_str(&line).unwrap_or_else(|_| panic!("not JSON-RPC: {line}"))
    }

    /// The `list` tool: an authenticated read, so a wrong token is a 401 and a right one is not.
    fn list(&mut self) -> String {
        let reply = self.call(
            "tools/call",
            serde_json::json!({"name": "list", "arguments": {}}),
        );
        reply.to_string()
    }
}

#[test]
fn project_discovery_routes_two_projects_to_distinct_ports_and_tokens() {
    let dir = tempfile::tempdir().unwrap();
    let registry = dir.path().join("registry");
    let project_a = dir.path().join("project-a");
    let project_b = dir.path().join("project-b");
    let events_a = project_a.join(".graphhelm").join("events");
    let events_b = project_b.join(".graphhelm").join("events");
    std::fs::create_dir_all(&events_a).unwrap();
    std::fs::create_dir_all(&events_b).unwrap();
    let port_a = free_port();
    let port_b = free_port();
    assert_ne!(port_a, port_b);

    let _runtime_a = serve_with_project(&events_a, port_a, &registry, Some(&project_a));
    let _runtime_b = serve_with_project(&events_b, port_b, &registry, Some(&project_b));
    let token_a = std::fs::read_to_string(events_a.with_extension("token")).unwrap();
    let token_b = std::fs::read_to_string(events_b.with_extension("token")).unwrap();
    assert_ne!(token_a, token_b, "each project keeps its own bearer token");

    // The single user-scope registration has no project path; the host's project cwd binds each
    // MCP process automatically. Project-local init entries may instead pass `--project`.
    let mut bridge_a = Bridge::start_in_project_cwd(&project_a, &registry);
    let mut bridge_b = Bridge::start_in_project_cwd(&project_b, &registry);
    let reply_a = bridge_a.list();
    let reply_b = bridge_b.list();
    assert!(
        authorized(&reply_a),
        "project A's Runtime must answer: {reply_a}"
    );
    assert!(
        authorized(&reply_b),
        "project B's Runtime must answer: {reply_b}"
    );
    assert!(!bridge_a.transcript.contains(token_a.trim()));
    assert!(!bridge_b.transcript.contains(token_b.trim()));
}

#[test]
fn project_discovery_refuses_missing_or_ambiguous_project_bindings() {
    let dir = tempfile::tempdir().unwrap();
    let registry = dir.path().join("registry");
    let missing = dir.path().join("missing-project");
    std::fs::create_dir_all(&missing).unwrap();
    let mut missing_bridge = Bridge::start_project(&missing, &registry);
    let missing_reply = missing_bridge.list();
    assert!(
        missing_reply.contains("no Runtime has registered this project"),
        "an unregistered project must fail closed: {missing_reply}"
    );

    let project = dir.path().join("duplicate-project");
    std::fs::create_dir_all(&project).unwrap();
    let events_a = dir.path().join("store-a").join("events");
    let events_b = dir.path().join("store-b").join("events");
    std::fs::create_dir_all(events_a.parent().unwrap()).unwrap();
    std::fs::create_dir_all(events_b.parent().unwrap()).unwrap();
    let runtime_a = serve_with_project(&events_a, free_port(), &registry, Some(&project));
    let runtime_b = serve_with_project(&events_b, free_port(), &registry, Some(&project));
    let mut duplicate_bridge = Bridge::start_project(&project, &registry);
    let duplicate_reply = duplicate_bridge.list();
    assert!(
        duplicate_reply.contains("more than one live Runtime is registered"),
        "two live stores for one project must not be guessed between: {duplicate_reply}"
    );
    drop((runtime_a, runtime_b));
}

fn authorized(reply: &str) -> bool {
    !reply.contains("GHCLI007")
        && !reply.contains("\"isError\":true")
        && !reply.contains("discovery")
}

#[test]
fn one_discovering_bridge_follows_a_runtime_swap_on_the_same_port() {
    let dir = tempfile::tempdir().unwrap();
    let registry = dir.path().join("registry");
    let events_a = dir.path().join("a").join("events");
    let events_b = dir.path().join("b").join("events");
    std::fs::create_dir_all(events_a.parent().unwrap()).unwrap();
    std::fs::create_dir_all(events_b.parent().unwrap()).unwrap();
    let port = free_port();

    let runtime_a = serve(&events_a, port, &registry);
    let mut bridge = Bridge::start(port, &registry, &["--discover"]);
    let first = bridge.list();
    assert!(
        authorized(&first),
        "A must answer the discovering bridge: {first}"
    );

    stop(runtime_a, port);
    let runtime_b = serve(&events_b, port, &registry);
    let token_a = std::fs::read_to_string(dir.path().join("a").join("events.token")).unwrap();
    let token_b = std::fs::read_to_string(dir.path().join("b").join("events.token")).unwrap();
    assert_ne!(token_a, token_b, "two stores, two tokens");

    // The same process, after the swap: the cached token A draws B's 401, the record is read
    // again, B's instance is confirmed, and the one retry carries token B.
    let second = bridge.list();
    assert!(
        authorized(&second),
        "B must answer the SAME bridge: {second}"
    );

    // Control: a bridge pinned to A's token file is refused by B.
    let token_file_a = dir.path().join("a").join("events.token");
    let mut pinned = Bridge::start(
        port,
        &registry,
        &["--token-file", token_file_a.to_str().unwrap()],
    );
    let refused = pinned.list();
    assert!(
        refused.contains("GHCLI007"),
        "the pinned control must be unauthorized: {refused}"
    );

    // A record the live Runtime did not write is not believed.
    let record_path = registry.join(format!("{port}.json"));
    let mut record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&record_path).unwrap()).unwrap();
    let real = record.clone();
    record["instance"] = serde_json::json!("0".repeat(32));
    std::fs::write(&record_path, serde_json::to_vec(&record).unwrap()).unwrap();
    let mut forged = Bridge::start(port, &registry, &["--discover"]);
    let stale = forged.list();
    assert!(stale.contains("not the one that published"), "{stale}");
    std::fs::write(&record_path, serde_json::to_vec(&real).unwrap()).unwrap();

    // Nothing on the port: fail closed, naming the remedy.
    stop(runtime_b, port);
    let mut orphan = Bridge::start(port, &registry, &["--discover"]);
    let down = orphan.list();
    assert!(down.contains("no Runtime answers"), "{down}");

    for transcript in [&bridge.transcript, &forged.transcript, &orphan.transcript] {
        assert!(!transcript.contains(token_a.trim()) && !transcript.contains(token_b.trim()));
    }
}

#[test]
fn an_ephemeral_port_publishes_no_record() {
    let dir = tempfile::tempdir().unwrap();
    let registry = dir.path().join("registry");
    let child = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["serve", "--events"])
        .arg(dir.path().join("events"))
        .args(["--bind", "127.0.0.1:0"])
        .env("GRAPHHELM_RUNTIME_DIR", &registry)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut guard = Killed(child);
    let mut line = String::new();
    BufReader::new(guard.0.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert!(line.contains("serve.started"), "{line}");
    assert!(!registry.exists(), "port 0 must not publish");
}

#[test]
fn project_discovery_uses_the_runtime_assigned_loopback_port() {
    let dir = tempfile::tempdir().unwrap();
    let registry = dir.path().join("registry");
    let project = dir.path().join("ephemeral-project");
    let events = project.join(".graphhelm").join("events");
    std::fs::create_dir_all(&events).unwrap();

    let child = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["serve", "--events"])
        .arg(&events)
        .args(["--bind", "127.0.0.1:0", "--project"])
        .arg(&project)
        .env("GRAPHHELM_RUNTIME_DIR", &registry)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _runtime = Killed(child);

    let deadline = Instant::now() + Duration::from_secs(60);
    let (record, project_id) = loop {
        let found = std::fs::read_dir(registry.join("projects"))
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .flat_map(|entry| {
                std::fs::read_dir(entry.path())
                    .into_iter()
                    .flatten()
                    .flatten()
            })
            .find_map(|entry| {
                let bytes = std::fs::read(entry.path()).ok()?;
                let record: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
                Some((
                    record,
                    entry.path().parent()?.file_name()?.to_str()?.to_owned(),
                ))
            });
        if let Some(found) = found {
            break found;
        }
        assert!(
            Instant::now() < deadline,
            "serve never published a project record"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    let url = record["url"].as_str().unwrap();
    let port = url.rsplit(':').next().unwrap().parse::<u16>().unwrap();
    assert_ne!(port, 0, "the record contains the OS-assigned port");
    let health = health(port).expect("the assigned port answers health");
    assert_eq!(health["data"]["instance"], record["instance"]);
    assert_eq!(health["data"]["projectId"], project_id);

    let mut bridge = Bridge::start_project(&project, &registry);
    let reply = bridge.list();
    assert!(
        authorized(&reply),
        "the discovered Runtime must answer: {reply}"
    );
}

#[test]
fn discover_and_token_file_together_are_refused() {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args([
            "mcp",
            "--url",
            "http://127.0.0.1:1",
            "--discover",
            "--token-file",
            "x",
            "--actor",
            "a",
        ])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
}

/// #380: discovery hands an MCP session the Runtime's agent session token, never the owner
/// token. Observed by breaking the owner token file after `serve` has read it: a bridge that
/// still discovers the owner token cannot authenticate, one that discovers the agent token can.
#[test]
fn project_discovery_hands_out_the_agent_token_not_the_owner_token() {
    let dir = tempfile::tempdir().unwrap();
    let registry = dir.path().join("registry");
    let project = dir.path().join("project");
    let events = project.join(".graphhelm").join("events");
    std::fs::create_dir_all(&events).unwrap();
    let port = free_port();
    let _runtime = serve_with_project(&events, port, &registry, Some(&project));
    assert!(
        events.with_extension("agent.token").is_file(),
        "serve mints the agent session token beside the owner token"
    );
    std::fs::write(events.with_extension("token"), "not-a-token").unwrap();

    let mut bridge = Bridge::start_project(&project, &registry);
    let reply = bridge.list();
    assert!(
        authorized(&reply),
        "discovery must use the agent token: {reply}"
    );
}

/// #380: an owner-typed MCP session is never discovered; it names the owner token explicitly.
#[test]
fn discover_with_an_owner_actor_type_is_refused() {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["mcp", "--discover", "--actor", "a", "--actor-type", "owner"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("--token-file"),
        "the refusal names the owner door: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}
