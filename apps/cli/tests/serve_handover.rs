//! #1523 (D-059): `serve --handover-install <root>` moves a live Runtime onto the binary
//! `graphhelm update` installs, keeping the key in memory, and fails closed. Every run uses a temp
//! store, a temp install root and a throwaway key; nothing here reaches a real Runtime.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::Value;

const KEY_HEX: &str = "0101010101010101010101010101010101010101010101010101010101010101";

struct Fixture {
    _dir: tempfile::TempDir,
    events: PathBuf,
    keyring: PathBuf,
    install: PathBuf,
    shutdown: PathBuf,
    port: u16,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let events = dir.path().join("events");
        let keyring = dir.path().join("keyring");
        std::fs::create_dir_all(&keyring).unwrap();
        graphhelm_sealed_key_provider::SealedKeyProvider::create(
            &keyring,
            "signal-key",
            graphhelm_events::SecretBytes::new(vec![1; 32]),
        )
        .unwrap();
        let install = dir.path().join("install");
        std::fs::create_dir_all(install.join("bin")).unwrap();
        std::fs::write(install.join(".graphhelm-installed-head"), "old-head\n").unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        Self {
            shutdown: dir.path().join("shutdown"),
            _dir: dir,
            events,
            keyring,
            install,
            port,
        }
    }

    fn installed_binary(&self) -> PathBuf {
        self.install.join("bin").join(if cfg!(windows) {
            "graphhelm.exe"
        } else {
            "graphhelm"
        })
    }

    fn stamp(&self, head: &str) {
        std::fs::write(self.install.join(".graphhelm-installed-head"), head).unwrap();
    }

    fn stderr(&self) -> String {
        std::fs::read_to_string(self.events.with_extension("stderr")).unwrap_or_default()
    }

    fn token(&self) -> String {
        let mut name = self.events.file_name().unwrap().to_os_string();
        name.push(".token");
        std::fs::read_to_string(self.events.with_file_name(name)).unwrap()
    }

    /// Starts the old Runtime; its stdout lines (and its successor's, which inherits the pipe)
    /// arrive on the returned channel.
    fn serve(&self) -> (Child, mpsc::Receiver<String>) {
        let mut child = graphhelm_process::command(assert_cmd::cargo::cargo_bin!("graphhelm"))
            .args(["serve", "--events"])
            .arg(&self.events)
            .args(["--bind", &format!("127.0.0.1:{}", self.port)])
            .arg("--keyring")
            .arg(&self.keyring)
            .args(["--key-id", "signal-key", "--handover-install"])
            .arg(&self.install)
            .env("GRAPHHELM_EVENTS_KEY", KEY_HEX)
            .env("GRAPHHELM_TEST_SHUTDOWN_FILE", &self.shutdown)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(std::fs::File::create(self.events.with_extension("stderr")).unwrap())
            .spawn()
            .unwrap();
        let (sender, lines) = mpsc::channel();
        let stdout = child.stdout.take().unwrap();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let _ = sender.send(line);
            }
        });
        let first = lines.recv_timeout(Duration::from_secs(120)).unwrap();
        assert!(first.contains("serve.started"), "{first}");
        (child, lines)
    }

    fn get(&self, path: &str, token: Option<&str>) -> Option<(u16, Value)> {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).ok()?;
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .ok()?;
        let auth = token.map_or_else(String::new, |token| {
            format!("Authorization: Bearer {token}\r\n")
        });
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{auth}Connection: close\r\n\r\n"
        )
        .ok()?;
        let mut raw = String::new();
        stream.read_to_string(&mut raw).ok()?;
        let status = raw.split(' ').nth(1)?.parse().ok()?;
        let body = raw.split_once("\r\n\r\n")?.1;
        // Responses may be chunked; the JSON object is between the first `{` and the last `}`.
        let json = &body[body.find('{')?..=body.rfind('}')?];
        Some((status, serde_json::from_str(json).ok()?))
    }

    fn serving_pid(&self) -> Option<u64> {
        self.get("/health", None)?.1["data"]["currentRun"]["pid"].as_u64()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Stops whichever Runtime is serving, old or successor: both inherited the seam.
        let _ = std::fs::write(&self.shutdown, "");
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline && self.serving_pid().is_some() {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

fn exited_within(child: &mut Child, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if child.try_wait().unwrap().is_some() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

fn start_execution(events: &Path, directory: &Path) -> Value {
    let fixtures = directory.join("fixtures.json");
    std::fs::write(
        &fixtures,
        r#"{"nodeOutcomes":{"implementation":"success","deploy":"success"}}"#,
    )
    .unwrap();
    let graph = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/graphs/manual-override-deploy.yaml");
    let output = graphhelm_process::command(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["execution", "start", "--file"])
        .arg(graph)
        .arg("--events")
        .arg(events)
        .arg("--fixtures")
        .arg(&fixtures)
        .args(["--mode", "supervised", "--execution", "handover-exec"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice::<Value>(&output.stdout).unwrap()["data"]["status"].clone()
}

#[test]
fn a_new_install_takes_over_the_port_and_the_old_runtime_exits() {
    let fixture = Fixture::new();
    let before = start_execution(&fixture.events, fixture.keyring.parent().unwrap());
    std::fs::copy(
        assert_cmd::cargo::cargo_bin!("graphhelm"),
        fixture.installed_binary(),
    )
    .unwrap();
    let (mut old, lines) = fixture.serve();
    assert_eq!(fixture.serving_pid(), Some(u64::from(old.id())));

    fixture.stamp("new-head\n");
    let deadline = Instant::now() + Duration::from_secs(120);
    let successor = loop {
        match fixture.serving_pid() {
            Some(pid) if pid != u64::from(old.id()) => break pid,
            _ if Instant::now() > deadline => panic!("no successor took over the port"),
            _ => std::thread::sleep(Duration::from_millis(100)),
        }
    };
    assert!(
        exited_within(&mut old, Duration::from_secs(30)),
        "the old Runtime kept running"
    );
    let announced = std::iter::from_fn(|| lines.recv_timeout(Duration::from_secs(5)).ok())
        .any(|line| line.contains("serve.started"));
    assert!(
        announced,
        "the successor announced itself on the inherited stdout"
    );

    let (status, health) = fixture.get("/health", None).unwrap();
    assert_eq!(status, 200);
    assert_eq!(health["data"]["previousExit"]["state"], "clean");
    assert_eq!(health["data"]["previousExit"]["pid"], u64::from(old.id()));
    assert_eq!(health["data"]["currentRun"]["pid"], successor);
    let token = fixture.token();
    let (status, execution) = fixture
        .get("/v1/executions/handover-exec", Some(token.trim()))
        .unwrap();
    assert_eq!(status, 200, "{execution}");
    assert_eq!(
        execution["data"]["status"], before,
        "the events survived the handover"
    );
}

#[test]
fn a_new_install_that_cannot_start_leaves_the_old_runtime_serving() {
    let fixture = Fixture::new();
    let (mut old, _lines) = fixture.serve();
    let pid = u64::from(old.id());

    // Not an executable at all.
    std::fs::write(fixture.installed_binary(), b"not a program").unwrap();
    fixture.stamp("broken-head\n");
    std::thread::sleep(Duration::from_secs(6));
    assert!(
        old.try_wait().unwrap().is_none(),
        "the old Runtime exited: {}",
        fixture.stderr()
    );
    assert_eq!(fixture.serving_pid(), Some(pid));

    // A real binary whose startup fails: the keyring it must open is gone.
    std::fs::remove_file(fixture.installed_binary()).unwrap();
    std::fs::copy(
        assert_cmd::cargo::cargo_bin!("graphhelm"),
        fixture.installed_binary(),
    )
    .unwrap();
    std::fs::remove_dir_all(&fixture.keyring).unwrap();
    fixture.stamp("unhealthy-head\n");
    std::thread::sleep(Duration::from_secs(10));
    assert!(
        old.try_wait().unwrap().is_none(),
        "the old Runtime exited: {}",
        fixture.stderr()
    );
    assert_eq!(fixture.serving_pid(), Some(pid));
}
