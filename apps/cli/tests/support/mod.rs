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
use std::net::TcpStream;
use std::time::Duration;

/// The read/write budget for a test request. 15 seconds is what three of the four copies already
/// carried; `resume_atomicity.rs` carried 5 with nothing saying why, and a shorter budget there
/// only makes a slow listener look like a failure sooner.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

pub struct RawResponse {
    pub status: u16,
    pub body: String,
}

pub fn raw_request(url: &str, token: Option<&str>) -> std::io::Result<RawResponse> {
    let (host, port, path) = split_url(url);
    let mut stream = TcpStream::connect((host.as_str(), port))?;
    stream.set_read_timeout(Some(REQUEST_TIMEOUT))?;
    stream.set_write_timeout(Some(REQUEST_TIMEOUT))?;

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
