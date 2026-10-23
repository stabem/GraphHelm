//! #1962: `serve --supervise` brings a Runtime that died back on its own, reports the restart in
//! `/health` `previousExit`, and stops (without restarting) after a clean stop. Every run uses a
//! temp store, its own free port and a throwaway key; nothing here reaches a real Runtime.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

const KEY_HEX: &str = "0101010101010101010101010101010101010101010101010101010101010101";

struct Fixture {
    _dir: tempfile::TempDir,
    events: PathBuf,
    keyring: PathBuf,
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
            port,
        }
    }

    fn supervise(&self) -> Child {
        graphhelm_process::command(assert_cmd::cargo::cargo_bin!("graphhelm"))
            .args(["serve", "--supervise", "--events"])
            .arg(&self.events)
            .args(["--bind", &format!("127.0.0.1:{}", self.port)])
            .arg("--keyring")
            .arg(&self.keyring)
            .args(["--key-id", "signal-key"])
            .env("GRAPHHELM_EVENTS_KEY", KEY_HEX)
            .env("GRAPHHELM_TEST_SHUTDOWN_FILE", &self.shutdown)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(self.events.with_extension("stderr")).unwrap())
            .spawn()
            .unwrap()
    }

    fn stderr(&self) -> String {
        std::fs::read_to_string(self.events.with_extension("stderr")).unwrap_or_default()
    }

    fn health(&self) -> Option<Value> {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).ok()?;
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .ok()?;
        write!(
            stream,
            "GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        )
        .ok()?;
        let mut raw = String::new();
        stream.read_to_string(&mut raw).ok()?;
        if raw.split(' ').nth(1)? != "200" {
            return None;
        }
        let body = raw.split_once("\r\n\r\n")?.1;
        let json = &body[body.find('{')?..=body.rfind('}')?];
        serde_json::from_str::<Value>(json)
            .ok()
            .map(|v| v["data"].clone())
    }

    /// The pid of a healthy Runtime other than `not`, within `limit`.
    fn serving_other_than(&self, not: Option<u64>, limit: Duration) -> Option<(u64, Value)> {
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline {
            if let Some(data) = self.health() {
                if let Some(pid) = data["currentRun"]["pid"].as_u64() {
                    if Some(pid) != not {
                        return Some((pid, data));
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        None
    }
}

fn kill_pid(pid: u64) {
    let status = if cfg!(windows) {
        graphhelm_process::command("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
    } else {
        graphhelm_process::command("kill")
            .args(["-9", &pid.to_string()])
            .status()
    };
    assert!(status.unwrap().success(), "could not kill pid {pid}");
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

// The issue's promise end to end: a killed Runtime (an abort leaves no clean record either)
// comes back healthy on the same port by itself, the new run says a supervisor restarted it, and
// a clean stop ends the supervisor instead of starting a third run.
#[test]
fn a_killed_runtime_comes_back_and_a_clean_stop_does_not_restart() {
    let fixture = Fixture::new();
    let mut supervisor = fixture.supervise();
    let Some((first, data)) = fixture.serving_other_than(None, Duration::from_secs(120)) else {
        let _ = supervisor.kill();
        panic!(
            "the supervised Runtime never answered: {}",
            fixture.stderr()
        );
    };
    assert_ne!(
        first,
        u64::from(supervisor.id()),
        "the supervisor must not serve itself"
    );
    assert!(data["previousExit"].is_null(), "{data}");

    kill_pid(first);
    let Some((second, data)) = fixture.serving_other_than(Some(first), Duration::from_secs(60))
    else {
        let _ = supervisor.kill();
        panic!("the Runtime did not come back: {}", fixture.stderr());
    };
    assert_ne!(second, first);
    assert_eq!(data["previousExit"]["state"], "vanished", "{data}");
    assert_eq!(data["previousExit"]["pid"], first, "{data}");
    assert_eq!(
        data["previousExit"]["supervisorRestart"]["count"], 1,
        "{data}"
    );

    std::fs::write(&fixture.shutdown, "").unwrap();
    let stopped = exited_within(&mut supervisor, Duration::from_secs(30));
    if !stopped {
        let _ = supervisor.kill();
    }
    assert!(
        stopped,
        "a clean stop must end the supervisor: {}",
        fixture.stderr()
    );
    assert!(supervisor.wait().unwrap().success(), "{}", fixture.stderr());
    assert!(
        fixture.health().is_none(),
        "nothing may serve after a clean stop"
    );
    assert_eq!(
        fixture
            .stderr()
            .matches("runtime.supervise the Runtime exited unexpectedly")
            .count(),
        1,
        "{}",
        fixture.stderr()
    );
}
