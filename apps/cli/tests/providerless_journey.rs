//! Provider-less mode as a declared guarantee (#1064, MVP promise 4 of #302).
//!
//! `docs/product/PROVIDER_LESS_MODE.md` promises that a clean installation with no gateway
//! manifest, no keyring, no credentials and no network runs a complete execution, shows it on the
//! monitor, and exports it. This file is what holds the promise: one journey from the repository
//! root with an empty store directory, every command spawned with the process environment
//! SCRUBBED (no `GRAPHHELM_*` variable can reach the binary), and the documentation's own command
//! lines read back and compared with the ones the journey ran — the same pattern
//! `attention_tag_domain.rs` uses to bind QUICKSTART's table to `verdict_tag`.
//!
//! **What "no network" means here, stated so the guard is not read as more than it is.** The
//! spawn scrubs credentials; it does not deny sockets, and a test that could would need an
//! operating-system sandbox the committed gate does not have. The obligation this journey holds
//! is the one the product can hold: nothing in the flow is GIVEN a manifest, a key, or a route, and
//! the executor the run declares (`executor == "fixture"`, `core/simulation/src/executor.rs`)
//! has no network code path — it answers from a `BTreeMap` by node id. A regression that added a
//! network call to that executor would be a change to that file, and the label would still be
//! honest about what was declared.
//!
//! The harness spawns `graphhelm serve` the way `monitor_http.rs` does (bin-only crate: the spawn
//! is a second copy by design; the request helper is named `monitor_request` so
//! `http_helper_inventory.rs` does not count it as a drifted `raw_request`). Every blocking step
//! — the startup announcement, each socket connect/read/write, the health poll — is bounded by
//! the deadline it advertises, so a wedged server ends this test red instead of hanging the gate.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const DOCUMENT: &str = include_str!("../../../docs/product/PROVIDER_LESS_MODE.md");

/// The sentence every view of a fixture run carries — monitor page, `status --html` snapshot,
/// Studio panel. Asserted as text so a rewording on one surface fails here rather than drifting.
const DEMONSTRATION_SENTENCE: &str = "Demonstration run — started under the fixture executor: outcomes at start were supplied by a fixture file, not produced by a model or a tool.";

/// The fixture the document tells the reader to write: every node succeeds, so the run completes
/// with nothing waiting on a human and the demonstration label is the ONLY thing separating it
/// from a real run. The journey does not write this literal itself — it EXECUTES the document's
/// own setup line (`documented_setup`) and checks that what the line writes is this.
const FIXTURES_JSON: &str = r#"{"nodeOutcomes":{"implementation":"success","deploy":"success"}}"#;

/// The wall-time budget for one server-side step (startup announcement, one request).
const STEP_BUDGET: Duration = Duration::from_secs(15);

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The command list the journey ran, in the words the document must use. Every entry is the
/// argv the test actually spawned, canonicalised by [`Journey::canonical`].
struct Journey {
    tmp: PathBuf,
    ran: Vec<String>,
}

impl Journey {
    fn new(tmp: &Path) -> Self {
        Self {
            tmp: tmp.to_path_buf(),
            ran: Vec::new(),
        }
    }

    /// A `graphhelm` invocation with NO inherited environment beyond what the operating system
    /// needs to start a process. `env_clear` is the proof: a `GRAPHHELM_*` variable set in the
    /// developer's shell (an events key, an API token, a gateway passphrase) cannot leak into the
    /// run and quietly make "no credentials" false. The working directory is the repository root,
    /// which is what the document tells the reader to use — the flow names repository-relative
    /// inputs (`examples/…`, `core/architect/fixtures/…`); only the STORE lives in `<tmp>`.
    fn command(&mut self, args: &[String]) -> Command {
        let mut command = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
        command.env_clear();
        for name in ["PATH", "SYSTEMROOT", "TEMP", "TMP"] {
            if let Ok(value) = std::env::var(name) {
                command.env(name, value);
            }
        }
        command.current_dir(root()).args(args);
        self.ran.push(self.canonical(args));
        command
    }

    fn json(&mut self, args: &[String]) -> serde_json::Value {
        let output = self.command(args).output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        assert!(
            output.status.success(),
            "{args:?} failed: {stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_str(stdout.trim()).unwrap_or_else(|error| panic!("{error}: {stdout}"))
    }

    /// One argv as the document writes it: the temp directory becomes `<tmp>`, separators are
    /// forward slashes, and an argument with a space is double-quoted.
    fn canonical(&self, args: &[String]) -> String {
        let tmp = self.tmp.to_string_lossy().replace('\\', "/");
        let words: Vec<String> = std::iter::once("graphhelm".to_owned())
            .chain(args.iter().map(|arg| {
                let arg = arg.replace('\\', "/").replace(&tmp, "<tmp>");
                if arg.contains(' ') {
                    format!("\"{arg}\"")
                } else {
                    arg
                }
            }))
            .collect();
        words.join(" ")
    }
}

fn strings(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_owned()).collect()
}

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

/// `serve --events <dir> --bind 127.0.0.1:0` and NOTHING else: no manifest, no broker, no
/// keyring, no key id, no allowlist. The server this starts is fixture-only by construction.
///
/// The startup announcement is read on its own thread and awaited through a channel with a
/// timeout, because a `read_line` on the child's stdout has no deadline of its own: a server
/// that stays alive but never prints `serve.started` would otherwise hold this test — and the
/// gate around it — forever.
fn serve(journey: &mut Journey, events: &Path) -> (ServerGuard, String, String) {
    let args = strings(&[
        "serve",
        "--events",
        events.to_str().unwrap(),
        "--bind",
        "127.0.0.1:0",
    ]);
    let mut child = journey
        .command(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + STEP_BUDGET;
    let stdout = child.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let read = BufReader::new(stdout).read_line(&mut line);
        let _ = sender.send(read.map(|count| (count, line)));
    });
    let mut guard = ServerGuard { child };
    let announced = match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()))
    {
        Ok(Ok((count, line))) if count > 0 => line,
        other => {
            let mut stderr_text = String::new();
            let _ = guard.child.kill();
            if let Some(mut stderr) = guard.child.stderr.take() {
                let _ = stderr.read_to_string(&mut stderr_text);
            }
            panic!(
                "`graphhelm serve` did not announce itself within {STEP_BUDGET:?} ({other:?}); stderr:\n{stderr_text}"
            );
        }
    };
    let started: serde_json::Value = serde_json::from_str(announced.trim()).unwrap();
    assert_eq!(started["command"], "serve.started", "{started}");
    let address = started["data"]["address"].as_str().unwrap().to_owned();
    let token = std::fs::read_to_string(token_path(events))
        .unwrap()
        .trim()
        .to_owned();
    loop {
        let (status, _headers, _body) = monitor_request(&address, "/health", &[], deadline);
        if status == 200 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the server never became healthy within {STEP_BUDGET:?}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    (guard, address, token)
}

/// One raw GET whose connect, write and read are each bounded by what remains of `deadline`;
/// returns (status, raw header block, body), with status 0 when the budget ran out.
fn monitor_request(
    address: &str,
    path: &str,
    headers: &[(&str, &str)],
    deadline: Instant,
) -> (u16, String, String) {
    let remaining = || deadline.saturating_duration_since(Instant::now());
    let Ok(socket) = address.parse::<SocketAddr>() else {
        return (0, String::new(), String::new());
    };
    let Ok(mut stream) =
        TcpStream::connect_timeout(&socket, remaining().max(Duration::from_millis(1)))
    else {
        return (0, String::new(), String::new());
    };
    let budget = remaining().max(Duration::from_millis(1));
    let _ = stream.set_read_timeout(Some(budget));
    let _ = stream.set_write_timeout(Some(budget));
    let mut text = format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n");
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

/// The goal the first-compile fixture was recorded with, read from the one file that holds it.
fn first_compile_goal() -> String {
    std::fs::read_to_string(root().join("core/architect/fixtures/first-compile/GOAL.txt"))
        .unwrap()
        .trim_end()
        .to_owned()
}

/// The lines of the document's fenced blocks, with `\` (bash) and `` ` `` (PowerShell)
/// continuations joined, in order.
fn fenced_lines() -> Vec<String> {
    let mut lines = Vec::new();
    let mut in_fence = false;
    let mut pending = String::new();
    for line in DOCUMENT.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if !in_fence {
            continue;
        }
        let trimmed = line.trim();
        let (text, continues) = match trimmed.strip_suffix('\\').or(trimmed.strip_suffix('`')) {
            Some(text) => (text.trim_end(), true),
            None => (trimmed, false),
        };
        if !pending.is_empty() {
            pending.push(' ');
        }
        pending.push_str(text);
        if !continues {
            lines.push(std::mem::take(&mut pending));
        }
    }
    lines
}

/// Every `graphhelm` command the document shows, bash and PowerShell spellings alike (they
/// canonicalise to the same string, which is the point: one flow, two shells).
fn documented_commands() -> Vec<String> {
    fenced_lines()
        .into_iter()
        .filter(|line| line.starts_with("graphhelm "))
        .collect()
}

/// The document's setup lines — the ones that are not `graphhelm` commands but that the flow
/// needs — as (file name under `<tmp>`, contents). Recognised spellings, one per shell:
///
/// - bash: `echo '<contents>' > <tmp>/<name>`
/// - PowerShell: `'<contents>' | Set-Content -Path <tmp>/<name>`
///
/// A setup line the parser does not understand is a hard failure, never a skip: the review that
/// asked for this found the old scanner silently ignoring every non-`graphhelm` line, which let
/// a malformed setup keep the document "proven".
fn documented_setup() -> Vec<(String, String)> {
    let mut files = Vec::new();
    for line in fenced_lines() {
        if line.starts_with("graphhelm ") || line.starts_with('$') || line.starts_with('{') {
            continue;
        }
        let parsed = if let Some(rest) = line.strip_prefix("echo '") {
            rest.split_once("' > <tmp>/")
        } else if let Some(rest) = line.strip_prefix('\'') {
            rest.split_once("' | Set-Content -Path <tmp>/")
        } else {
            None
        };
        match parsed {
            Some((contents, name)) => files.push((name.trim().to_owned(), contents.to_owned())),
            None => {
                // Prose-shaped lines inside non-command fences (the page excerpt, the journal
                // excerpt) carry no `<tmp>/` target and are not setup; anything that names a
                // `<tmp>/` target and is not understood IS a setup line this parser cannot run.
                assert!(
                    !line.contains("<tmp>/"),
                    "PROVIDER_LESS_MODE.md has a setup line this journey cannot execute: {line}"
                );
            }
        }
    }
    files
}

/// The whole journey, in the order the document tells it, returning the commands it ran so the
/// document can be held against them.
fn run_journey() -> Journey {
    let directory = tempfile::tempdir().unwrap();
    let tmp = directory.path();
    let mut journey = Journey::new(tmp);
    let events = tmp.join("events");

    // 0. The document's own setup lines, executed: the reader's fixture is the test's fixture.
    let setup = documented_setup();
    assert!(
        setup.iter().any(|(name, _)| name == "fixtures.json"),
        "the document must show how fixtures.json is written: {setup:?}"
    );
    for (name, contents) in &setup {
        std::fs::write(tmp.join(name), contents).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(tmp.join("fixtures.json")).unwrap(),
        FIXTURES_JSON,
        "every documented spelling of the setup writes the same fixture"
    );

    // 1. A complete execution, declared as fixture-driven at start.
    let started = journey.json(&strings(&[
        "execution",
        "start",
        "--file",
        "examples/graphs/provider-less-demo.yaml",
        "--events",
        events.to_str().unwrap(),
        "--fixtures",
        tmp.join("fixtures.json").to_str().unwrap(),
        "--execution",
        "demo",
        "--mode",
        "supervised",
    ]));
    assert_eq!(started["ok"], true, "{started}");
    assert_eq!(started["data"]["status"], "completed", "{started}");
    assert_eq!(
        started["data"]["nodeStateCounts"]["succeeded"], 2,
        "{started}"
    );
    assert_eq!(
        started["data"]["executor"], "fixture",
        "the start reply names the executor it ran under: {started}"
    );

    // The journal itself says which executor the run was declared under — the label is a
    // recorded fact, not something a renderer infers after the fact.
    let journal = std::fs::read_to_string(events.join("journal.jsonl")).unwrap();
    let declared = journal
        .lines()
        .find(|line| line.contains("\"execution_form_declared\""))
        .expect("the journal carries the execution_form_declared event");
    assert!(
        declared.contains("\"executor\":\"fixture\""),
        "execution_form_declared must record the executor: {declared}"
    );

    // 2. The monitor, over a server given nothing but the store and a bind address. The guard
    //    stays alive through steps 3-5: the document says the snapshot, the export and the index
    //    all work while `serve` is up, so that is the condition they are proven under.
    let (guard, address, token) = serve(&mut journey, &events);
    let deadline = Instant::now() + STEP_BUDGET;
    let (status, headers, _body) = monitor_request(
        &address,
        &format!("/monitor/demo?token={token}"),
        &[],
        deadline,
    );
    assert_eq!(
        status, 303,
        "the bootstrap redirects to the clean URL: {headers}"
    );
    let cookie_pair = header_value(&headers, "Set-Cookie")
        .expect("the bootstrap sets the cookie")
        .split(';')
        .next()
        .unwrap()
        .trim()
        .to_owned();
    let (status, _headers, page) = monitor_request(
        &address,
        "/monitor/demo",
        &[("Cookie", &cookie_pair)],
        deadline,
    );
    assert_eq!(status, 200, "{page}");
    assert!(
        page.contains(DEMONSTRATION_SENTENCE),
        "the monitor page labels the fixture run as a demonstration: {page}"
    );
    assert!(page.contains("status: <b>completed</b>"), "{page}");

    // The same run through the API the Studio reads: the field the panel turns into words, and
    // the index row the rail marks.
    let bearer = format!("Bearer {token}");
    let (status, _headers, body) = monitor_request(
        &address,
        "/v1/executions/demo",
        &[("Authorization", &bearer)],
        deadline,
    );
    assert_eq!(status, 200, "{body}");
    let api: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(api["data"]["executor"], "fixture", "{api}");
    assert_eq!(api["data"]["status"], "completed", "{api}");
    let (status, _headers, body) = monitor_request(
        &address,
        "/v1/executions",
        &[("Authorization", &bearer)],
        deadline,
    );
    assert_eq!(status, 200, "{body}");
    let index: serde_json::Value = serde_json::from_str(&body).unwrap();
    let row = index["data"]["executions"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["executionId"] == "demo"))
        .unwrap_or_else(|| panic!("the index lists demo: {index}"));
    assert_eq!(
        row["executor"], "fixture",
        "the index row carries the executor so the rail can mark it: {row}"
    );

    // 3. The frozen snapshot: the same renderer, written to a file, with the server up.
    let snapshot = tmp.join("demo.html");
    let status = journey.json(&strings(&[
        "execution",
        "status",
        "--events",
        events.to_str().unwrap(),
        "--execution",
        "demo",
        "--html",
        snapshot.to_str().unwrap(),
    ]));
    assert_eq!(status["ok"], true, "{status}");
    assert_eq!(status["data"]["executor"], "fixture", "{status}");
    let page = std::fs::read_to_string(&snapshot).unwrap();
    assert!(
        page.contains(DEMONSTRATION_SENTENCE),
        "the snapshot labels the fixture run as a demonstration: {page}"
    );

    // 4. The index from the CLI, the same rows the API served above.
    let listed = journey.json(&strings(&[
        "execution",
        "list",
        "--events",
        events.to_str().unwrap(),
    ]));
    assert_eq!(listed["ok"], true, "{listed}");
    let row = listed["data"]["executions"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["executionId"] == "demo"))
        .unwrap_or_else(|| panic!("execution list shows demo: {listed}"));
    assert_eq!(row["executor"], "fixture", "{row}");

    // 5. The export, with the server still holding the store open: the backup takes the
    //    repository's read lock beside a live writer, which is the claim the document makes.
    let archive = tmp.join("demo-backup.json");
    let backup = journey.json(&strings(&[
        "events",
        "backup",
        "--repository",
        events.to_str().unwrap(),
        "--output",
        archive.to_str().unwrap(),
    ]));
    assert_eq!(backup["ok"], true, "{backup}");
    assert_eq!(
        backup["data"]["lockHeld"], true,
        "the backup held the store's lock while the server was up: {backup}"
    );
    let archived: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&archive).unwrap()).unwrap();
    assert!(archived["archiveVersion"].is_string(), "{archived}");
    assert!(
        archived["journal"]
            .as_str()
            .is_some_and(|journal| journal.contains("\"execution_form_declared\"")),
        "the archive carries the journal, executor declaration included"
    );
    assert!(archived["blobs"].is_object(), "{archived}");
    // The server is still alive after the backup: the proof was taken beside it, not after it.
    let (status, _headers, _body) = monitor_request(&address, "/health", &[], deadline);
    assert_eq!(status, 200, "the server outlived the backup");
    drop(guard);

    // 6. The compiler, against a recorded reply: a graph from a goal with no model reachable.
    let out = tmp.join("synthesized.json");
    let synthesized = journey.json(&strings(&[
        "graph",
        "synthesize",
        "--goal",
        &first_compile_goal(),
        "--fixture",
        "core/architect/fixtures/first-compile/replies.json",
        "--allow-program",
        "cargo",
        "--out",
        out.to_str().unwrap(),
    ]));
    assert_eq!(synthesized["ok"], true, "{synthesized}");
    assert_eq!(synthesized["command"], "graph.synthesize", "{synthesized}");
    assert!(out.is_file(), "the synthesized graph was written");

    journey
}

#[test]
fn a_clean_installation_with_no_credentials_runs_shows_and_exports_a_demonstration() {
    let journey = run_journey();
    assert!(
        journey.ran.len() >= 6,
        "the journey ran {} command(s): {:?}",
        journey.ran.len(),
        journey.ran
    );
}

/// The control that proves the document scan read something: a document whose fences stopped
/// matching this parser would yield an empty list, and an empty list is a subset of anything.
#[test]
fn the_document_scan_finds_the_commands_it_is_supposed_to_read() {
    let documented = documented_commands();
    assert!(
        documented.len() >= 6,
        "the scan of PROVIDER_LESS_MODE.md found {} command(s): {documented:?}",
        documented.len()
    );
    assert!(
        documented
            .iter()
            .any(|command| command.starts_with("graphhelm execution start ")),
        "the document no longer shows `execution start` in a fenced block: {documented:?}"
    );
    // Both shells are present, and they spell the same commands: a PowerShell reader gets the
    // flow the bash reader gets, not a subset.
    let bash = DOCUMENT.matches("```bash").count();
    let powershell = DOCUMENT.matches("```powershell").count();
    assert!(
        bash >= 1 && powershell >= 1,
        "the document shows the flow in bash ({bash}) and PowerShell ({powershell})"
    );
    let mut unique = documented.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        documented.len(),
        unique.len() * 2,
        "every command appears exactly once per shell, spelled identically once continuations \
         are joined: {documented:#?}"
    );
}

/// The document's commands ARE the journey's commands — both directions, so the document can
/// neither show a command the journey never proved nor hide one it did.
#[test]
fn the_document_shows_exactly_the_commands_the_journey_ran() {
    let journey = run_journey();
    let documented = documented_commands();

    let unproven: Vec<&String> = documented
        .iter()
        .filter(|command| !journey.ran.contains(command))
        .collect();
    assert!(
        unproven.is_empty(),
        "PROVIDER_LESS_MODE.md shows {unproven:#?}, which this journey never ran. The journey \
         ran:\n{}",
        journey.ran.join("\n")
    );

    let unshown: Vec<&String> = journey
        .ran
        .iter()
        .filter(|command| !documented.contains(command))
        .collect();
    assert!(
        unshown.is_empty(),
        "this journey ran {unshown:#?}, which PROVIDER_LESS_MODE.md does not show. The document \
         shows:\n{}",
        documented.join("\n")
    );
}

/// The setup the document tells the reader to run is understood, executed, and writes the
/// fixture the journey needs — in both shells.
#[test]
fn the_document_setup_is_executed_and_writes_the_fixture() {
    let setup = documented_setup();
    let fixture_spellings = setup
        .iter()
        .filter(|(name, _)| name == "fixtures.json")
        .count();
    assert_eq!(
        fixture_spellings, 2,
        "one bash and one PowerShell spelling of the fixture setup: {setup:?}"
    );
    for (name, contents) in &setup {
        assert_eq!(
            contents, FIXTURES_JSON,
            "{name} must be the all-success fixture"
        );
    }
}

/// The label the document promises is the label the journey saw, word for word.
#[test]
fn the_document_quotes_the_demonstration_sentence() {
    assert!(
        DOCUMENT.contains(DEMONSTRATION_SENTENCE),
        "PROVIDER_LESS_MODE.md must quote the demonstration sentence exactly"
    );
}
