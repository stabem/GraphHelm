//! The non-browser observer (#927): a journey flow whose acts are `http` requests or `cli`
//! commands is played without a browser. Each act yields one observation (an HTTP status and
//! body, or an exit code and stdout) and the screen it reaches is checked against it. The preview
//! runner records the verdict exactly as it records a browser step, so `keel test validation`
//! and `keel test sabotage` read backend journeys unchanged.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde_json::Value;

/// How long one act may take; a hung request or command is a failed act, never a wait for ever.
const ACT_LIMIT: Duration = Duration::from_secs(30);
/// The most of a response or stdout an observation keeps.
const MAX_BODY: u64 = 1024 * 1024;
const METHODS: &[&str] = &["GET", "HEAD", "OPTIONS", "POST", "PUT", "PATCH", "DELETE"];
const HTTP_EXPECTS: &[&str] = &["status", "json", "text"];
const CLI_EXPECTS: &[&str] = &["exit", "stdout"];

/// What one act observed.
#[derive(Debug, Default)]
pub(super) struct Observation {
    pub(super) status: Option<u16>,
    pub(super) exit: Option<i32>,
    pub(super) body: String,
}

fn acts(flow: &Value) -> impl Iterator<Item = &Value> {
    flow["edges"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|edge| edge["acts"].as_array().into_iter().flatten())
}

fn backend_kind(act: &Value) -> bool {
    matches!(act["kind"].as_str(), Some("http" | "cli"))
}

/// A flow is a backend flow when any act is `http` or `cli`.
pub(super) fn is_backend(flow: &Value) -> bool {
    acts(flow).any(backend_kind)
}

/// A backend flow with an `http` act needs the app at `base`; a `cli`-only flow does not.
pub(super) fn needs_app(flow: &Value) -> bool {
    acts(flow).any(|act| act["kind"] == "http")
}

/// Semantic findings for a backend flow: `(code, pointer, message)`.
pub(super) fn findings(flow: &Value) -> Vec<(&'static str, String, &'static str)> {
    let mut found = Vec::new();
    if !is_backend(flow) {
        return found;
    }
    for (i, edge) in flow["edges"].as_array().into_iter().flatten().enumerate() {
        for (a, act) in edge["acts"].as_array().into_iter().flatten().enumerate() {
            let pointer = format!("/edges/{i}/acts/{a}");
            let name = act["name"].as_str().unwrap_or_default();
            let valid = match act["kind"].as_str() {
                Some("http") => {
                    METHODS.contains(&act["role"].as_str().unwrap_or_default())
                        && name.starts_with('/')
                        && !name.starts_with("//")
                }
                Some("cli") => act["role"] == "exec" && !name.trim().is_empty(),
                _ => {
                    found.push((
                        "flow.observer_mixed",
                        pointer,
                        "a flow with http or cli acts cannot also play browser acts",
                    ));
                    continue;
                }
            };
            if !valid {
                found.push((
                    "flow.backend_act_invalid",
                    pointer,
                    "http needs a method role and a /path name; cli needs role exec and argv",
                ));
            }
        }
    }
    for (i, screen) in flow["screens"].as_array().into_iter().flatten().enumerate() {
        for (e, expect) in screen["expect"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let role = expect["role"].as_str().unwrap_or_default();
            if !HTTP_EXPECTS.contains(&role) && !CLI_EXPECTS.contains(&role) {
                found.push((
                    "flow.backend_expect_invalid",
                    format!("/screens/{i}/expect/{e}/role"),
                    "a backend expectation is status, json, text, exit or stdout",
                ));
            }
        }
    }
    found
}

/// Sends one `http` act (or the start screen's GET) to `base` and reads the whole response.
pub(super) fn request(
    base: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> Result<Observation, String> {
    let authority = base
        .split_once("://")
        .map_or(base, |(_, rest)| rest)
        .split('/')
        .next()
        .unwrap_or_default();
    let address = super::journey_live::base_address(base).ok_or("base has no address")?;
    let mut stream = std::net::ToSocketAddrs::to_socket_addrs(&address)
        .map_err(|error| error.to_string())?
        .find_map(|address| TcpStream::connect_timeout(&address, ACT_LIMIT).ok())
        .ok_or("connection refused")?;
    stream
        .set_read_timeout(Some(ACT_LIMIT))
        .and_then(|()| stream.set_write_timeout(Some(ACT_LIMIT)))
        .map_err(|error| error.to_string())?;
    let body = body.unwrap_or_default();
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\nAccept: */*\r\nContent-Length: {}\r\n",
        body.len()
    );
    if !body.is_empty() {
        head.push_str("Content-Type: application/json\r\n");
    }
    head.push_str("\r\n");
    stream
        .write_all(head.as_bytes())
        .and_then(|()| stream.write_all(body.as_bytes()))
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    stream
        .take(MAX_BODY)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    let text = String::from_utf8_lossy(&bytes);
    let (headers, content) = text.split_once("\r\n\r\n").ok_or("malformed response")?;
    let status = headers
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or("malformed status line")?;
    let chunked = headers
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked");
    Ok(Observation {
        status: Some(status),
        exit: None,
        body: if chunked {
            dechunk(content)
        } else {
            content.to_owned()
        },
    })
}

fn dechunk(mut rest: &str) -> String {
    let mut out = String::new();
    while let Some((size, tail)) = rest.split_once("\r\n") {
        let Ok(size) = usize::from_str_radix(size.split(';').next().unwrap_or("").trim(), 16)
        else {
            break;
        };
        if size == 0 || tail.len() < size {
            break;
        }
        out.push_str(&tail[..size]);
        rest = tail[size..].trim_start_matches("\r\n");
    }
    out
}

/// Runs one `cli` act: argv split on whitespace, no shell, in the project, bounded by
/// [`ACT_LIMIT`]. The child is killed at the limit.
pub(super) fn command(project: &Path, argv: &str) -> Result<Observation, String> {
    let mut words = argv.split_whitespace();
    let program = words.next().ok_or("empty argv")?;
    let mut child = graphhelm_process::command(program)
        .args(words)
        .current_dir(project)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    let mut stdout = child.stdout.take().ok_or("no stdout")?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = (&mut stdout).take(MAX_BODY).read_to_end(&mut bytes);
        bytes
    });
    let deadline = Instant::now() + ACT_LIMIT;
    let status = loop {
        match child.try_wait().map_err(|error| error.to_string())? {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("command timed out".into());
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let bytes = reader.join().unwrap_or_default();
    Ok(Observation {
        status: None,
        exit: status.code(),
        body: String::from_utf8_lossy(&bytes).into_owned(),
    })
}

/// Plays one backend act.
pub(super) fn play(project: &Path, base: &str, act: &Value) -> Result<Observation, String> {
    let name = act["name"].as_str().unwrap_or_default();
    match act["kind"].as_str() {
        Some("http") => request(
            base,
            act["role"].as_str().unwrap_or("GET"),
            name,
            act["text"].as_str(),
        ),
        Some("cli") => command(project, name),
        _ => Err("not a backend act".into()),
    }
}

fn json_field<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(value, |node, key| match node {
        Value::Array(items) => items.get(key.parse::<usize>().ok()?),
        _ => node.get(key),
    })
}

/// The first expectation of `screen` the observation does not meet, as `role name`.
pub(super) fn unmet(screen: &Value, seen: &Observation) -> Option<String> {
    screen["expect"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|expect| {
            let name = expect["name"].as_str().unwrap_or_default();
            let met = match expect["role"].as_str().unwrap_or_default() {
                "status" => seen.status.map(|s| s.to_string()).as_deref() == Some(name),
                "exit" => seen.exit.map(|s| s.to_string()).as_deref() == Some(name),
                "text" | "stdout" => seen.body.contains(name),
                "json" => {
                    let (path, want) = name
                        .split_once('=')
                        .map_or((name, None), |(p, v)| (p, Some(v)));
                    serde_json::from_str::<Value>(&seen.body)
                        .ok()
                        .and_then(|body| json_field(&body, path).cloned())
                        .is_some_and(|field| {
                            want.is_none_or(|want| match &field {
                                Value::String(text) => text == want,
                                other => {
                                    serde_json::from_str::<Value>(want).ok().as_ref() == Some(other)
                                }
                            })
                        })
                }
                _ => false,
            };
            !met
        })
        .map(|expect| {
            format!(
                "{} {}",
                expect["role"].as_str().unwrap_or_default(),
                expect["name"].as_str().unwrap_or_default()
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The defect named: a backend expectation that passes on the wrong value (a status prefix,
    /// a JSON field compared as its quoted form, a missing field read as present).
    #[test]
    fn expectations_match_exact_values_and_report_the_first_unmet() {
        let screen = json!({"expect":[{"role":"status","name":"200"},
            {"role":"json","name":"item.tags.1=b"},{"role":"json","name":"count=2"},
            {"role":"text","name":"apple"}]});
        let seen = Observation {
            status: Some(200),
            exit: None,
            body: r#"{"item":{"name":"apple","tags":["a","b"]},"count":2}"#.into(),
        };
        assert_eq!(unmet(&screen, &seen), None);
        let wrong = Observation {
            status: Some(2000),
            ..Default::default()
        };
        assert_eq!(unmet(&screen, &wrong).as_deref(), Some("status 200"));
        let missing = Observation {
            status: Some(200),
            exit: None,
            body: r#"{"item":{"tags":["a","c"]},"count":2}"#.into(),
        };
        assert_eq!(
            unmet(&screen, &missing).as_deref(),
            Some("json item.tags.1=b")
        );
        assert_eq!(dechunk("3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n"), "abcde");
    }
}
