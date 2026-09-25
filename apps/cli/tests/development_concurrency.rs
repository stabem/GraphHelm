//! Proves one of #223's acceptance criteria for the `development.*` HTTP surface that had no test
//! before this file (#395): concurrent calls don't crash or deadlock the server.
//!
//! **What this file does NOT claim.** An earlier version of this file also carried a test for
//! "an unfinished/oversized body cannot block other callers," built on a filed bug (#405) that
//! measured a `TcpStream::connect` timeout when one connection had an incomplete declared body.
//! That finding was retracted by its own author after independent review — see #405's closing
//! comment for the full evidence (H's and N's independent negatives, the structural self-check,
//! and the uncontrolled load variable the three original reproductions shared). Not re-adding
//! that test here — it was locking in a reproduction of a bug that isn't one.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

struct ServerHandle {
    child: std::process::Child,
    address: String,
    token: String,
    _directory: tempfile::TempDir,
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_server() -> ServerHandle {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let mut child = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args([
            "serve",
            "--events",
            events.to_str().unwrap(),
            "--bind",
            "127.0.0.1:0",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("graphhelm serve spawns");

    let mut stdout = child.stdout.take().expect("stdout is piped");
    let mut buffer = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(30);
    let started: Value = loop {
        let mut byte = [0u8; 1];
        match stdout.read(&mut byte) {
            Ok(1) if byte[0] == b'\n' => {
                break serde_json::from_slice(&buffer).unwrap_or_else(|error| {
                    let _ = child.kill();
                    panic!(
                        "the startup line was not valid JSON ({error}): {:?}",
                        String::from_utf8_lossy(&buffer)
                    )
                });
            }
            Ok(1) => buffer.push(byte[0]),
            _ => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    panic!("`graphhelm serve` printed no startup line within 30s");
                }
            }
        }
    };
    assert_eq!(
        started["ok"], true,
        "expected a successful startup: {started}"
    );
    let address = started["data"]["address"]
        .as_str()
        .unwrap_or_else(|| panic!("startup envelope must carry data.address: {started}"))
        .to_owned();

    let token_path = token_file_path(&events);
    let token = read_token(&token_path);

    ServerHandle {
        child,
        address,
        token,
        _directory: directory,
    }
}

fn token_file_path(events: &Path) -> std::path::PathBuf {
    let mut name = events.file_name().map_or_else(
        || std::ffi::OsString::from("events"),
        std::ffi::OsStr::to_os_string,
    );
    name.push(".token");
    events.with_file_name(name)
}

fn read_token(path: &Path) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(contents) = std::fs::read_to_string(path)
            && !contents.is_empty()
        {
            return contents;
        }
        if Instant::now() >= deadline {
            panic!("the server never wrote a readable token file at {path:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// One full request/response round trip: connects, sends `body` in full, reads the whole
/// response, and returns the HTTP status code plus the wall-clock window it occupied.
fn request(
    address: &str,
    method: &str,
    path: &str,
    token: &str,
    body: &[u8],
) -> (u16, Instant, Instant) {
    let start = Instant::now();
    let (host, port) = address
        .split_once(':')
        .expect("startup address always carries an explicit port");
    let mut stream = TcpStream::connect((host, port.parse::<u16>().unwrap()))
        .expect("the server accepts a connection");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    stream.write_all(request.as_bytes()).unwrap();
    stream.write_all(body).unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let end = Instant::now();
    let text = String::from_utf8_lossy(&raw);
    let status_line = text
        .lines()
        .next()
        .unwrap_or_else(|| panic!("empty HTTP response from {method} {path}"));
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or_else(|| panic!("malformed status line: {status_line:?}"));
    (status, start, end)
}

const DEVELOPMENT_HTTP_ROUTES: &[(&str, &str)] = &[
    ("POST", "/v1/development/contract"),
    ("GET", "/v1/development/memory"),
    ("POST", "/v1/development/memory"),
    ("POST", "/v1/development/present"),
    ("POST", "/v1/development/context"),
    ("GET", "/v1/development/accounting"),
];

/// Twenty concurrent callers, cycling through all six `development.*` routes on ONE running
/// server, all succeed. Nothing before this test issued more than one request at a time against
/// these routes (`development_surface_parity.rs`'s probe spawns one server per single request and
/// exits) -- a real coverage gap, not a behavioral regression: proven by the absence, not by a
/// planted defect.
///
/// **Asserts genuine overlap, not just that 20 sequential-looking calls all happened to pass.**
/// Each caller's [start, end) window is recorded; the test fails if no two windows overlap, which
/// would mean the threads never actually ran concurrently (e.g. a scheduler that serialized them
/// so completely the test measured 20 fast sequential calls and called it concurrency).
#[test]
fn concurrent_development_requests_all_succeed() {
    let server = spawn_server();
    let address = server.address.clone();
    let token = server.token.clone();

    let handles: Vec<_> = (0..20)
        .map(|i| {
            let address = address.clone();
            let token = token.clone();
            let (method, path) = DEVELOPMENT_HTTP_ROUTES[i % DEVELOPMENT_HTTP_ROUTES.len()];
            std::thread::spawn(move || request(&address, method, path, &token, b"{}"))
        })
        .collect();

    let mut windows = Vec::with_capacity(20);
    for (i, handle) in handles.into_iter().enumerate() {
        let (status, start, end) = handle.join().unwrap_or_else(|_| {
            panic!("caller {i} panicked instead of returning a status -- the server did not survive concurrent load")
        });
        assert_eq!(
            status,
            200,
            "caller {i} against {:?} got {status}, not 200, under concurrent load",
            DEVELOPMENT_HTTP_ROUTES[i % DEVELOPMENT_HTTP_ROUTES.len()]
        );
        windows.push((start, end));
    }

    let overlaps = windows.iter().enumerate().any(|(i, &(start_i, end_i))| {
        windows
            .iter()
            .enumerate()
            .any(|(j, &(start_j, end_j))| i != j && start_i < end_j && start_j < end_i)
    });
    assert!(
        overlaps,
        "no two of the 20 callers' request windows overlapped -- this measured 20 fast \
         sequential calls, not concurrency"
    );
}
