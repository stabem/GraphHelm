//! Black-box conformance for the monitor surface (Milestone 05f): browser-shaped requests
//! over raw `TcpStream` against a live `graphhelm serve` — cookie bootstrap, read-only
//! structure (405 to every mutating verb), CSP on every 200. The harness mirrors
//! `api_http.rs`'s spawn pattern (bin-only crate: second copy by design).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
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
    wait_for_health(&address);
    (ServerGuard { child }, address, token)
}

fn wait_for_health(address: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let (status, _headers, _body) = request(address, "GET", "/health", &[]);
        if status == 200 {
            return;
        }
        assert!(Instant::now() < deadline, "the server never became healthy");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// One raw HTTP request; returns (status, raw header block, body).
fn request(
    address: &str,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
) -> (u16, String, String) {
    let Ok(mut stream) = TcpStream::connect(address) else {
        return (0, String::new(), String::new());
    };
    let mut text = format!("{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n");
    for (name, value) in headers {
        text.push_str(&format!("{name}: {value}\r\n"));
    }
    text.push_str("\r\n");
    if stream.write_all(text.as_bytes()).is_err() {
        return (0, String::new(), String::new());
    }
    let mut reply = Vec::new();
    let _ = stream.read_to_end(&mut reply);
    let reply = String::from_utf8_lossy(&reply).into_owned();
    let status: u16 = reply
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or(0);
    let mut parts = reply.splitn(2, "\r\n\r\n");
    let headers = parts.next().unwrap_or_default().to_owned();
    let body = parts.next().unwrap_or_default().to_owned();
    (status, headers, body)
}

fn header_value(headers: &str, name: &str) -> Option<String> {
    headers
        .lines()
        .find(|line| {
            line.to_ascii_lowercase()
                .starts_with(&format!("{}:", name.to_ascii_lowercase()))
        })
        .and_then(|line| line.split_once(':'))
        .map(|(_, value)| value.trim().to_owned())
}

fn write_json(directory: &Path, name: &str, value: &serde_json::Value) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
    path
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Starts one execution through the CLI so the monitor has something to render.
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

#[test]
fn the_bootstrap_url_sets_a_cookie_and_redirects_clean() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    start_execution(&events, directory.path(), "exec-monitor-boot");
    let (_guard, address, token) = serve(&events);

    // Bootstrap: token in the query → 303 to the clean URL, cookie set, token nowhere.
    let (status, headers, body) = request(
        &address,
        "GET",
        &format!("/monitor/exec-monitor-boot?token={token}"),
        &[],
    );
    assert_eq!(status, 303, "{headers}\n{body}");
    let location = header_value(&headers, "Location").expect("a Location header");
    assert_eq!(location, "/monitor/exec-monitor-boot");
    assert!(
        !location.contains(&token),
        "the token never rides the Location"
    );
    let cookie = header_value(&headers, "Set-Cookie").expect("a Set-Cookie header");
    assert!(cookie.contains("HttpOnly"), "{cookie}");
    assert!(cookie.contains("SameSite=Strict"), "{cookie}");
    assert!(cookie.contains("Path=/monitor"), "{cookie}");
    assert!(
        !body.contains(&token),
        "the token never appears in a page byte"
    );

    // Following with the cookie → 200 text/html.
    let cookie_pair = cookie.split(';').next().unwrap().trim().to_owned();
    let (status, headers, body) = request(
        &address,
        "GET",
        "/monitor/exec-monitor-boot",
        &[("Cookie", &cookie_pair)],
    );
    assert_eq!(status, 200, "{headers}\n{body}");
    assert!(
        header_value(&headers, "Content-Type")
            .unwrap_or_default()
            .contains("text/html"),
        "{headers}"
    );
    assert!(
        !body.contains(&token),
        "the token never appears in a page byte"
    );

    // A WRONG token on bootstrap → 401 and NO cookie.
    let (status, headers, _body) = request(
        &address,
        "GET",
        "/monitor/exec-monitor-boot?token=0000000000000000000000000000000000000000000000000000000000000000",
        &[],
    );
    assert_eq!(status, 401);
    assert!(
        header_value(&headers, "Set-Cookie").is_none(),
        "no cookie on refusal"
    );

    // The clean URL without a cookie → 401.
    let (status, _headers, _body) = request(&address, "GET", "/monitor/exec-monitor-boot", &[]);
    assert_eq!(status, 401);

    // The index lists the execution as a link, under the same cookie.
    let (status, _headers, body) =
        request(&address, "GET", "/monitor", &[("Cookie", &cookie_pair)]);
    assert_eq!(status, 200);
    assert!(
        body.contains("/monitor/exec-monitor-boot"),
        "the index links the execution: {body}"
    );
}

#[test]
fn the_monitor_surface_answers_405_to_every_mutating_verb() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    start_execution(&events, directory.path(), "exec-monitor-verbs");
    let (_guard, address, token) = serve(&events);

    let (_status, headers, _body) = request(
        &address,
        "GET",
        &format!("/monitor/exec-monitor-verbs?token={token}"),
        &[],
    );
    let cookie_pair = header_value(&headers, "Set-Cookie")
        .expect("bootstrap sets the cookie")
        .split(';')
        .next()
        .unwrap()
        .trim()
        .to_owned();

    // Every mutating verb, both paths, even WITH a valid cookie → 405: the read-only
    // structure is the router's, not the handler's (D-040 as physics).
    for verb in ["POST", "PUT", "DELETE", "PATCH"] {
        for path in ["/monitor", "/monitor/exec-monitor-verbs"] {
            let (status, _headers, _body) =
                request(&address, verb, path, &[("Cookie", &cookie_pair)]);
            assert_eq!(status, 405, "{verb} {path} must be 405");
        }
    }

    // The CSP header rides every 200.
    for path in ["/monitor", "/monitor/exec-monitor-verbs"] {
        let (status, headers, _body) = request(&address, "GET", path, &[("Cookie", &cookie_pair)]);
        assert_eq!(status, 200, "{path}");
        assert_eq!(
            header_value(&headers, "Content-Security-Policy").as_deref(),
            Some("default-src 'none'; style-src 'unsafe-inline'"),
            "{path} carries the CSP"
        );
    }
}

/// FIX-1 (A's Task 2 review, proven live): axum's Path extractor percent-DECODES the
/// segment, so `/monitor/a%0D%0Ab?token=<real>` used to reach HeaderValue::from_str with
/// CR/LF in the id and PANIC the handler — a URL-reachable panic, connection dropped with
/// no reply. The fix: a header value that cannot be built is a 400, never a panic.
#[test]
fn a_header_hostile_id_is_a_400_never_a_panic() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    start_execution(&events, directory.path(), "exec-monitor-hostile");
    let (_guard, address, token) = serve(&events);

    let (status, _headers, _body) = request(
        &address,
        "GET",
        &format!("/monitor/a%0D%0Ab?token={token}"),
        &[],
    );
    assert_eq!(
        status, 400,
        "a CR/LF id is refused with a reply, never a dropped connection"
    );

    // The server is still alive and the well-formed bootstrap still works after the attack.
    let (status, headers, _body) = request(
        &address,
        "GET",
        &format!("/monitor/exec-monitor-hostile?token={token}"),
        &[],
    );
    assert_eq!(status, 303);
    assert!(header_value(&headers, "Set-Cookie").is_some());
}

/// Task 4: the negative proof — the store cannot be moved through the monitor. Every file
/// byte under the events directory is hashed before and after hammering every monitor
/// route with every verb, with and without the cookie, with garbage bodies; the store must
/// be bit-identical and the status JSON unchanged. (Expected green if Tasks 2-3 are honest
/// — observed and recorded either way; the sabotage is the red proof.)
#[test]
fn hammering_the_monitor_never_changes_a_byte_of_the_store() {
    use sha2::Digest;

    fn store_fingerprint(events: &Path) -> Vec<(String, String)> {
        fn walk(dir: &Path, base: &Path, out: &mut Vec<(String, String)>) {
            let mut entries: Vec<_> = std::fs::read_dir(dir)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect();
            entries.sort();
            for path in entries {
                if path.is_dir() {
                    walk(&path, base, out);
                } else {
                    let bytes = std::fs::read(&path).unwrap();
                    out.push((
                        path.strip_prefix(base)
                            .unwrap()
                            .to_string_lossy()
                            .into_owned(),
                        hex::encode(sha2::Sha256::digest(&bytes)),
                    ));
                }
            }
        }
        let mut out = Vec::new();
        walk(events, events, &mut out);
        out
    }

    fn status_json(events: &Path, execution: &str) -> serde_json::Value {
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
        serde_json::from_slice(&output.stdout).unwrap()
    }

    /// A raw request WITH a body (the plain `request` helper sends none).
    fn request_with_body(
        address: &str,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> u16 {
        let Ok(mut stream) = TcpStream::connect(address) else {
            return 0;
        };
        let mut text = format!(
            "{method} {path} HTTP/1.1
Host: {address}
Connection: close
"
        );
        for (name, value) in headers {
            text.push_str(&format!(
                "{name}: {value}
"
            ));
        }
        text.push_str(&format!(
            "Content-Length: {}

{body}",
            body.len()
        ));
        if stream.write_all(text.as_bytes()).is_err() {
            return 0;
        }
        let mut reply = Vec::new();
        let _ = stream.read_to_end(&mut reply);
        String::from_utf8_lossy(&reply)
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse().ok())
            .unwrap_or(0)
    }

    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    start_execution(&events, directory.path(), "exec-monitor-hammer");
    let (_guard, address, token) = serve(&events);

    // Bootstrap once so the hammer can also fire authenticated requests.
    let (_status, headers, _body) = request(
        &address,
        "GET",
        &format!("/monitor/exec-monitor-hammer?token={token}"),
        &[],
    );
    let cookie_pair = header_value(&headers, "Set-Cookie")
        .expect("the bootstrap cookie")
        .split(';')
        .next()
        .unwrap()
        .trim()
        .to_owned();

    let before_bytes = store_fingerprint(&events);
    let before_status = status_json(&events, "exec-monitor-hammer");

    // The enumerated surface: both known paths, plus a probe list proving the router
    // exposes nothing else under /monitor (each probe must be 404/405, never 200).
    let paths = ["/monitor", "/monitor/exec-monitor-hammer"];
    let probes = [
        "/monitor/exec-monitor-hammer/approve",
        "/monitor/exec-monitor-hammer/pause",
        "/monitor/exec-monitor-hammer/retry",
        "/monitor/exec-monitor-hammer/cancel",
        "/monitor/api",
    ];
    for probe in probes {
        let (status, _headers, _body) =
            request(&address, "GET", probe, &[("Cookie", &cookie_pair)]);
        assert!(
            status == 404 || status == 401,
            "{probe} must not exist as a monitor surface (got {status})"
        );
    }

    for path in paths {
        for verb in ["GET", "HEAD", "POST", "PUT", "DELETE", "PATCH"] {
            for cookie in [None, Some(cookie_pair.as_str())] {
                let mut headers: Vec<(&str, &str)> = Vec::new();
                if let Some(value) = cookie {
                    headers.push(("Cookie", value));
                }
                let _ = request(&address, verb, path, &headers);
                let _ = request_with_body(
                    &address,
                    verb,
                    path,
                    &headers,
                    "{\"node\":\"implementation\",\"outcome\":\"approved\"}",
                );
            }
        }
    }

    let after_bytes = store_fingerprint(&events);
    let after_status = status_json(&events, "exec-monitor-hammer");
    assert_eq!(
        before_bytes, after_bytes,
        "the store must be bit-identical after the hammer"
    );
    assert_eq!(before_status, after_status, "the status JSON is unchanged");
}
