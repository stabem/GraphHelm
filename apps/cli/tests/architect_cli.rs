//! `graphhelm graph synthesize` — the road from a goal to a document that starts and completes
//! (spec §4, §5 "CLI" cells). The model in every test is the recorded door (`--fixture`): no
//! network, no credentials, no clock.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;

const REFUSED_CODE: &str = "GHCLI026_ARCHITECT_REFUSED";
const ARGUMENT_CODE: &str = "GHCLI001_ARGUMENT_INVALID";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn command() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
}

fn fixtures() -> PathBuf {
    root().join("core/architect/fixtures")
}

/// The goal the first-compile fixture was recorded with: read from the one file the crate test
/// reads too, so exactly one copy of the string exists.
fn first_compile_goal() -> String {
    std::fs::read_to_string(fixtures().join("first-compile/GOAL.txt"))
        .unwrap()
        .trim_end()
        .to_owned()
}

fn envelope(args: &[&str]) -> Value {
    let output = command().args(args).output().unwrap();
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{error}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn write_json(directory: &Path, name: &str, value: &Value) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
    path
}

fn synthesize(goal: &str, out: &Path, fixture: &Path) -> Value {
    envelope(&[
        "graph",
        "synthesize",
        "--goal",
        goal,
        "--out",
        out.to_str().unwrap(),
        "--allow-program",
        "cargo",
        "--fixture",
        fixture.to_str().unwrap(),
    ])
}

/// THE FIRST COMPILE (PRD §25 step 5, made falsifiable): one command, keyless, produces a
/// document that lints clean, starts, and completes.
#[test]
fn a_goal_becomes_a_document_that_starts_and_completes() {
    let directory = tempfile::tempdir().unwrap();
    let out = directory.path().join("first-compile.json");
    let fixture = fixtures().join("first-compile/replies.json");
    let value = synthesize(&first_compile_goal(), &out, &fixture);
    assert_eq!(value["ok"], true, "{value}");
    assert_eq!(value["command"], "graph.synthesize");
    assert_eq!(
        value["data"]["stampedCustoms"],
        serde_json::json!(["build_check", "summarize"])
    );
    assert_eq!(value["data"]["rounds"], 1);
    assert_eq!(value["data"]["out"], out.to_str().unwrap());
    assert_eq!(value["data"]["promptSha256s"].as_array().unwrap().len(), 1);
    assert!(value["data"]["templateSha256"].as_str().unwrap().len() == 64);
    assert!(
        value["data"].get("usage").is_none(),
        "a recording reports no usage"
    );

    // The file IS the document the reply carries, pretty-printed with a trailing newline.
    let written = std::fs::read(&out).unwrap();
    let mut expected = serde_json::to_vec_pretty(&value["data"]["document"]).unwrap();
    expected.push(b'\n');
    assert_eq!(written, expected);

    // lint: zero GHG102 — the #183 property, measured on the file the operator would use
    let lint = envelope(&["graph", "lint", out.to_str().unwrap()]);
    assert_eq!(lint["ok"], true, "{lint}");
    assert!(!lint.to_string().contains("GHG102"), "{lint}");

    // execute it: the same road every authored graph takes
    let events = directory.path().join("events");
    let fixtures = write_json(
        directory.path(),
        "fixtures.json",
        &serde_json::json!({"nodeOutcomes": {"build_check": "success", "summarize": "success"}}),
    );
    let started = envelope(&[
        "execution",
        "start",
        "--file",
        out.to_str().unwrap(),
        "--events",
        events.to_str().unwrap(),
        "--fixtures",
        fixtures.to_str().unwrap(),
        "--mode",
        "supervised",
        "--execution",
        "exec-first-compile",
    ]);
    assert_eq!(started["ok"], true, "{started}");
    assert_eq!(started["data"]["status"], "completed", "{started}");
}

#[test]
fn the_document_is_byte_identical_across_two_runs() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = fixtures().join("first-compile/replies.json");
    let goal = first_compile_goal();
    let first = directory.path().join("one.json");
    let second = directory.path().join("two.json");
    let one = synthesize(&goal, &first, &fixture);
    let two = synthesize(&goal, &second, &fixture);
    assert_eq!(one["ok"], true, "{one}");
    assert_eq!(two["ok"], true, "{two}");
    assert_eq!(
        std::fs::read(&first).unwrap(),
        std::fs::read(&second).unwrap(),
        "two runs, one document"
    );
    assert_eq!(one["data"]["document"], two["data"]["document"]);
}

#[test]
fn a_goal_needing_a_program_outside_the_allowlist_is_refused_and_names_the_program() {
    let directory = tempfile::tempdir().unwrap();
    let out = directory.path().join("refused.json");
    let fixture = fixtures().join("sabotage/program-outside-catalog.json");
    let value = synthesize(&first_compile_goal(), &out, &fixture);
    assert_eq!(value["ok"], false, "{value}");
    assert_eq!(value["command"], "graph.synthesize");
    let diagnostics = value["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 1, "{value}");
    assert_eq!(diagnostics[0]["code"], REFUSED_CODE);
    assert_eq!(diagnostics[0]["path"], "/goal");
    let message = diagnostics[0]["message"].as_str().unwrap();
    let refusal: Value = serde_json::from_str(message).unwrap_or_else(|error| {
        panic!("the message is the refusal as compact JSON: {error}: {message}")
    });
    assert_eq!(refusal["kind"], "capabilityMissing");
    assert_eq!(refusal["node"], "build_check");
    assert_eq!(refusal["program"], "python");
    assert!(!out.exists(), "a refusal writes nothing");
}

#[test]
fn an_existing_out_path_is_never_overwritten() {
    let directory = tempfile::tempdir().unwrap();
    let out = directory.path().join("taken.json");
    std::fs::write(&out, b"precious\n").unwrap();
    let fixture = fixtures().join("first-compile/replies.json");
    let value = synthesize(&first_compile_goal(), &out, &fixture);
    assert_eq!(value["ok"], false, "{value}");
    assert_eq!(value["diagnostics"][0]["code"], ARGUMENT_CODE);
    assert_eq!(value["diagnostics"][0]["path"], "/out");
    assert_eq!(std::fs::read(&out).unwrap(), b"precious\n");
}

#[test]
fn a_fixture_without_the_prompt_prints_the_hash_to_record() {
    let directory = tempfile::tempdir().unwrap();
    let out = directory.path().join("unrecorded.json");
    let empty = write_json(
        directory.path(),
        "empty.json",
        &serde_json::json!({"replies": {}}),
    );
    let value = synthesize(&first_compile_goal(), &out, &empty);
    assert_eq!(value["ok"], false, "{value}");
    assert_eq!(value["diagnostics"][0]["code"], REFUSED_CODE);
    let message = value["diagnostics"][0]["message"].as_str().unwrap();
    let refusal: Value = serde_json::from_str(message).unwrap();
    assert_eq!(refusal["kind"], "fixtureMissing");
    // camelCase on the wire, like every other key of the envelope (`rename_all_fields`).
    let hash = refusal["promptSha256"].as_str().unwrap();
    assert!(
        refusal.get("prompt_sha256").is_none(),
        "the snake_case spelling must not survive: {refusal}"
    );
    assert_eq!(hash.len(), 64, "{hash}");
    assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()), "{hash}");
    assert!(!out.exists());
}

/// #1123: `--drafts 2` without a judge is the compiler's own `InvalidProfile` refusal
/// (`GHCLI026`, exit 2), never a CLI pre-emption — the refusal text says a judge is what is
/// missing, and nothing is written.
#[test]
fn more_than_one_draft_without_a_judge_is_refused_by_the_compiler() {
    let directory = tempfile::tempdir().unwrap();
    let out = directory.path().join("drafts.json");
    let fixture = fixtures().join("first-compile/replies.json");
    let goal = first_compile_goal();
    let output = command()
        .args([
            "graph",
            "synthesize",
            "--goal",
            &goal,
            "--out",
            out.to_str().unwrap(),
            "--allow-program",
            "cargo",
            "--fixture",
            fixture.to_str().unwrap(),
            "--drafts",
            "2",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], false, "{value}");
    assert_eq!(value["diagnostics"][0]["code"], REFUSED_CODE);
    assert_eq!(value["diagnostics"][0]["path"], "/goal");
    let message = value["diagnostics"][0]["message"].as_str().unwrap();
    let refusal: Value = serde_json::from_str(message).unwrap();
    assert_eq!(refusal["kind"], "invalidProfile", "{refusal}");
    assert!(
        refusal["message"].as_str().unwrap().contains("judge"),
        "the refusal names what is missing: {refusal}"
    );
    assert!(!out.exists(), "a refusal writes nothing");
}

/// #1123 (spec D8): a library and a judge that answers `reuse` fill a template and never ask
/// the draft model — the draft recording is EMPTY and the run still succeeds with `rounds == 0`
/// and the document written to `--out`.
#[test]
fn a_reused_template_never_asks_the_draft_model_and_writes_the_document() {
    let directory = tempfile::tempdir().unwrap();
    let out = directory.path().join("reused.json");
    let empty = write_json(
        directory.path(),
        "empty.json",
        &serde_json::json!({"replies": {}}),
    );
    let judge = fixtures().join("judge/reuse-reuse.json");
    let library = fixtures().join("library");
    let goal = first_compile_goal();
    let output = command()
        .args([
            "graph",
            "synthesize",
            "--goal",
            &goal,
            "--out",
            out.to_str().unwrap(),
            "--allow-program",
            "cargo",
            "--fixture",
            empty.to_str().unwrap(),
            "--judge-fixture",
            judge.to_str().unwrap(),
            "--library",
            library.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], true, "{value}");
    assert_eq!(value["data"]["rounds"], 0, "{value}");
    assert_eq!(value["data"]["reuse"]["road"], "reuse", "{value}");
    assert_eq!(value["data"]["reuse"]["template"], "build-and-summarize");
    assert_eq!(value["data"]["promptSha256s"], serde_json::json!([]));
    let written: Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(
        written, value["data"]["document"],
        "--out is the reply's document"
    );
    assert_eq!(
        written["spec"]["nodes"]["build_check"]["tool"]["call"]["program"],
        "cargo"
    );
}

// ---------------------------------------------------------------------------------------------
// #1123 / #1127: `--judge-route`, the judge door that spends a leased credential.
//
// The road is the gateway one end to end: `--judge-route` requires `--manifest` (clap), and
// `--manifest` beside `--fixture` is refused by `model_source`, so a judge over a route can only
// ride beside a DRAFT over a route. Both routes point at one fake provider on loopback that
// answers `/v1/messages` with the recorded first-compile draft and `/v1/systemone` with the
// recorded below-threshold judge reply, and records every request it saw — method, path,
// headers, body — so the assertions can say where each leased key travelled.
// ---------------------------------------------------------------------------------------------

const GATEWAY_INVALID_CODE: &str = "GHCLI009_GATEWAY_INVALID";
/// The judge route's credential. Must appear in exactly one place: the `Authorization` header of
/// the one `POST /v1/systemone`.
const JUDGE_SENTINEL: &str = "ts-JUDGE-SENTINEL-fedcba9876543210";
/// The draft route's credential, planted so the draft lease succeeds and so the two keys can be
/// told apart on the wire.
const DRAFT_SENTINEL: &str = "sk-ant-DRAFT-SENTINEL-0123456789abcdef";

/// 64 lowercase hexadecimal characters — a well-formed `GRAPHHELM_GATEWAY_KEY`.
fn passphrase() -> String {
    "0123456789abcdef".repeat(4)
}

/// One HTTP request captured by [`fake_provider`], for assertions.
struct CapturedRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: String,
}

fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

/// A fake provider on an OS-assigned loopback port: every connection is read as one HTTP/1.1
/// request (headers, then `Content-Length` bytes of body), answered by path — `/v1/messages`
/// with `draft_body`, `/v1/systemone` with `judge_body`, anything else with 404 — and pushed
/// onto the returned log. The listener thread lives for the test process.
fn fake_provider(
    draft_body: String,
    judge_body: String,
) -> (String, Arc<Mutex<Vec<CapturedRequest>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else {
                return;
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let captured = read_request(&mut stream);
            let (status, body) = match captured.path.as_str() {
                "/v1/messages" => (200, draft_body.as_str()),
                "/v1/systemone" => (200, judge_body.as_str()),
                _ => (404, "{}"),
            };
            let head = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(body.as_bytes());
            let _ = stream.flush();
            log.lock().unwrap().push(captured);
        }
    });
    (format!("http://{address}"), seen)
}

fn read_request(stream: &mut TcpStream) -> CapturedRequest {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        if let Some(header_end) = find_header_end(&buffer)
            && buffer.len() - (header_end + 4) >= parse_content_length(&buffer[..header_end])
        {
            break;
        }
        let read = stream.read(&mut chunk).unwrap();
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    let header_end = find_header_end(&buffer).expect("the request must have a header/body split");
    let head = String::from_utf8_lossy(&buffer[..header_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut parts = lines.next().unwrap_or_default().split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let path = parts.next().unwrap_or_default().to_owned();
    let headers = lines
        .filter(|line| !line.is_empty())
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_owned(), value.trim().to_owned()))
        .collect();
    let content_length = parse_content_length(&buffer[..header_end]);
    let body_start = header_end + 4;
    let body =
        String::from_utf8_lossy(&buffer[body_start..body_start + content_length]).into_owned();
    CapturedRequest {
        method,
        path,
        headers,
        body,
    }
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn parse_content_length(head: &[u8]) -> usize {
    String::from_utf8_lossy(head)
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0)
}

/// The first-compile recording's one reply, wrapped as the Anthropic Messages body the BYOK
/// adapter parses: the gateway draft door then hands the compiler the exact text the fixture
/// door would.
fn recorded_draft_as_anthropic_body() -> String {
    let recording: Value = serde_json::from_slice(
        &std::fs::read(fixtures().join("first-compile/replies.json")).unwrap(),
    )
    .unwrap();
    let replies = recording["replies"].as_object().unwrap();
    assert_eq!(
        replies.len(),
        1,
        "the first compile is one round: {recording}"
    );
    let text = replies.values().next().unwrap().as_str().unwrap();
    serde_json::json!({
        "content": [{"type": "text", "text": text}],
        "usage": {"input_tokens": 1, "output_tokens": 1}
    })
    .to_string()
}

/// The below-threshold recording's one reply, served verbatim: `kind` = `tool` for `summarize`
/// at 0.60 lands that node in `judgments.unresolved`.
fn recorded_judge_reply_body() -> String {
    let recording: Value = serde_json::from_slice(
        &std::fs::read(fixtures().join("judge/nodes-below-threshold.json")).unwrap(),
    )
    .unwrap();
    let answers = recording["answers"].as_object().unwrap();
    assert_eq!(answers.len(), 1, "one request, one reply: {recording}");
    answers.values().next().unwrap().to_string()
}

/// `draft`: a chat route (`anthropic`, `direct_api`). `judge`: the one shape `--judge-route`
/// accepts (`typesafe`, `direct_api`). `dormant`: that shape, disabled.
fn judge_manifest_value(base_url: &str) -> Value {
    serde_json::json!({
        "manifestVersion": 1,
        "routes": [
            {
                "id": "draft",
                "provider": "anthropic",
                "transport": "direct_api",
                "authentication": "api_key",
                "billingMode": "per_token",
                "baseUrl": base_url,
                "model": "claude-sonnet-5",
                "credentialRef": "cred_anthropic",
                "profiles": ["critical_reasoning"],
                "enabled": true
            },
            {
                "id": "judge",
                "provider": "typesafe",
                "transport": "direct_api",
                "authentication": "api_key",
                "billingMode": "per_token",
                "baseUrl": base_url,
                "model": "jev-latest",
                "credentialRef": "secret_typesafe",
                "profiles": ["balanced_reasoning"],
                "enabled": true
            },
            {
                "id": "dormant",
                "provider": "typesafe",
                "transport": "direct_api",
                "authentication": "api_key",
                "billingMode": "per_token",
                "baseUrl": base_url,
                "model": "jev-latest",
                "credentialRef": "secret_typesafe",
                "profiles": ["balanced_reasoning"],
                "enabled": false
            }
        ]
    })
}

/// `gateway credential set`, the value on stdin, exactly as `gateway_cli.rs` plants one.
fn credential_set(
    broker: &Path,
    keyring: &Path,
    reference: &str,
    provider: &str,
    usable_by: &str,
    value: &str,
) {
    let output = command()
        .args([
            "gateway",
            "credential",
            "set",
            "--broker",
            broker.to_str().unwrap(),
            "--keyring",
            keyring.to_str().unwrap(),
            "--key-id",
            "test-key",
            "--ref",
            reference,
            "--provider",
            provider,
            "--usable-by",
            usable_by,
        ])
        .env("GRAPHHELM_GATEWAY_KEY", passphrase())
        .write_stdin(value)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The whole bench: the fake provider, the manifest naming it, the broker and keyring with both
/// credentials planted. Returns the temp directory, the manifest path and the request log.
fn judge_route_bench() -> (tempfile::TempDir, PathBuf, Arc<Mutex<Vec<CapturedRequest>>>) {
    let directory = tempfile::tempdir().unwrap();
    let (base_url, seen) = fake_provider(
        recorded_draft_as_anthropic_body(),
        recorded_judge_reply_body(),
    );
    let manifest = write_json(
        directory.path(),
        "manifest.json",
        &judge_manifest_value(&base_url),
    );
    let broker = directory.path().join("broker");
    let keyring = directory.path().join("keyring");
    std::fs::create_dir_all(&keyring).unwrap();
    credential_set(
        &broker,
        &keyring,
        "cred_anthropic",
        "anthropic",
        "draft",
        DRAFT_SENTINEL,
    );
    credential_set(
        &broker,
        &keyring,
        "secret_typesafe",
        "typesafe",
        "judge",
        JUDGE_SENTINEL,
    );
    (directory, manifest, seen)
}

/// `graph synthesize` over the gateway draft route `draft`, with `judge_flags` appended, the
/// broker coordinates of [`judge_route_bench`] and the passphrase in the environment.
fn synthesize_over_routes(
    directory: &Path,
    manifest: &Path,
    out: &Path,
    judge_flags: &[&str],
) -> std::process::Output {
    let goal = first_compile_goal();
    let broker = directory.join("broker");
    let keyring = directory.join("keyring");
    command()
        .args([
            "graph",
            "synthesize",
            "--goal",
            &goal,
            "--out",
            out.to_str().unwrap(),
            "--allow-program",
            "cargo",
            "--manifest",
            manifest.to_str().unwrap(),
            "--route",
            "draft",
            "--broker",
            broker.to_str().unwrap(),
            "--keyring",
            keyring.to_str().unwrap(),
            "--key-id",
            "test-key",
        ])
        .args(judge_flags)
        .env("GRAPHHELM_GATEWAY_KEY", passphrase())
        .output()
        .unwrap()
}

fn assert_no_leak(output: &std::process::Output, out: &Path) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    for sentinel in [JUDGE_SENTINEL, DRAFT_SENTINEL] {
        assert!(!stdout.contains(sentinel), "stdout leaks a key: {stdout}");
        assert!(!stderr.contains(sentinel), "stderr leaks a key: {stderr}");
        if let Ok(written) = std::fs::read(out) {
            assert!(
                !String::from_utf8_lossy(&written).contains(sentinel),
                "--out leaks a key"
            );
        }
    }
}

/// `--judge-route judge`: the judge's key is leased from the broker and travels ONLY as the
/// `Authorization: Bearer` header of the one `POST /v1/systemone`; the draft's key travels only
/// as `x-api-key` of the one `POST /v1/messages`; neither reaches stdout, stderr or `--out`.
/// The compiled result is the recorded doors' result — same document, same judgments
/// (`summarize` unresolved), same template hash — and `usage` is the one field that differs,
/// because the gateway doors report what the provider said and the recordings report nothing.
#[test]
fn the_judge_route_spends_the_leased_key_only_in_the_authorization_header() {
    let (directory, manifest, seen) = judge_route_bench();
    let out = directory.path().join("routed.json");
    let output = synthesize_over_routes(
        directory.path(),
        &manifest,
        &out,
        &["--judge-route", "judge"],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_no_leak(&output, &out);
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], true, "{value}");
    assert_eq!(
        value["data"]["judgments"]["unresolved"],
        serde_json::json!(["summarize"]),
        "{value}"
    );
    assert_eq!(value["data"]["rounds"], 1, "{value}");

    let seen = seen.lock().unwrap();
    let judge_calls: Vec<&CapturedRequest> = seen
        .iter()
        .filter(|request| request.path == "/v1/systemone")
        .collect();
    let draft_calls: Vec<&CapturedRequest> = seen
        .iter()
        .filter(|request| request.path == "/v1/messages")
        .collect();
    assert_eq!(seen.len(), 2, "one draft, one judgment, nothing else");
    assert_eq!(judge_calls.len(), 1);
    assert_eq!(draft_calls.len(), 1);
    let judge_call = judge_calls[0];
    assert_eq!(judge_call.method, "POST");
    assert_eq!(
        header_value(&judge_call.headers, "authorization"),
        Some(format!("Bearer {JUDGE_SENTINEL}").as_str()),
        "the leased judge key is the bearer"
    );
    assert!(
        !judge_call.body.contains(JUDGE_SENTINEL) && !judge_call.body.contains(DRAFT_SENTINEL),
        "no key in a body: {}",
        judge_call.body
    );
    let judge_body: Value = serde_json::from_str(&judge_call.body).unwrap();
    assert_eq!(judge_body["model"], "jev-latest", "{judge_body}");
    let draft_call = draft_calls[0];
    assert_eq!(draft_call.method, "POST");
    assert_eq!(
        header_value(&draft_call.headers, "x-api-key"),
        Some(DRAFT_SENTINEL)
    );
    assert!(
        !draft_call
            .headers
            .iter()
            .any(|(_, value)| value.contains(JUDGE_SENTINEL)),
        "the judge key never reaches the draft provider"
    );
    assert!(
        !judge_call
            .headers
            .iter()
            .any(|(_, value)| value.contains(DRAFT_SENTINEL)),
        "the draft key never reaches the judge provider"
    );

    // The same goal through the two recorded doors: one result on every door, `usage` aside.
    let fixture_out = directory.path().join("recorded.json");
    let recorded = envelope(&[
        "graph",
        "synthesize",
        "--goal",
        &first_compile_goal(),
        "--out",
        fixture_out.to_str().unwrap(),
        "--allow-program",
        "cargo",
        "--fixture",
        fixtures()
            .join("first-compile/replies.json")
            .to_str()
            .unwrap(),
        "--judge-fixture",
        fixtures()
            .join("judge/nodes-below-threshold.json")
            .to_str()
            .unwrap(),
    ]);
    assert_eq!(recorded["ok"], true, "{recorded}");
    for field in [
        "document",
        "judgments",
        "templateSha256",
        "promptSha256s",
        "rationale",
        "stampedCustoms",
        "rounds",
    ] {
        assert_eq!(
            value["data"][field], recorded["data"][field],
            "{field} differs between the route door and the recorded door"
        );
    }
    assert_ne!(
        value["data"]["usage"], recorded["data"]["usage"],
        "the gateway doors report the provider's usage; the recordings report none"
    );
    assert_eq!(
        std::fs::read(&out).unwrap(),
        std::fs::read(&fixture_out).unwrap(),
        "--out is the same file on both doors"
    );
}

/// #1137: A RECORDED DRAFT PAIRS WITH A REAL JUDGE — the combination `serve/routes.rs` has
/// always accepted (`fixture` + `judgeRoute`) and `model_source` refused, so the same capability
/// existed over HTTP and not from a terminal. `GRAPH_ARCHITECT.md` §10.8 recorded that asymmetry
/// instead of repairing it, and the cell above drafts through a fake gateway route only because
/// of it.
///
/// **The discriminator is the request count, not the exit code.** A run that quietly fell back to
/// the gateway for its draft would also exit 0 and also produce a document; what says the FIXTURE
/// served the draft is that the provider saw ONE request and it was the judge's. `/v1/messages`
/// is never called and the draft credential is never leased.
///
/// The document is then pinned against the both-doors-recorded run byte for byte, so "the fixture
/// served the draft" is a claim about the OUTPUT and not only about the traffic.
#[test]
fn a_recorded_draft_pairs_with_a_real_judge_route_and_asks_the_provider_only_to_judge() {
    let (directory, manifest, seen) = judge_route_bench();
    let out = directory.path().join("paired.json");
    let broker = directory.path().join("broker");
    let keyring = directory.path().join("keyring");
    let output = command()
        .args([
            "graph",
            "synthesize",
            "--goal",
            &first_compile_goal(),
            "--out",
            out.to_str().unwrap(),
            "--allow-program",
            "cargo",
            // The draft door: a recording. No `--route`.
            "--fixture",
            fixtures()
                .join("first-compile/replies.json")
                .to_str()
                .unwrap(),
            // The judge door: the real thing, over the manifest this run also supplies.
            "--manifest",
            manifest.to_str().unwrap(),
            "--judge-route",
            "judge",
            "--broker",
            broker.to_str().unwrap(),
            "--keyring",
            keyring.to_str().unwrap(),
            "--key-id",
            "test-key",
        ])
        .env("GRAPHHELM_GATEWAY_KEY", passphrase())
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_no_leak(&output, &out);
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], true, "{value}");

    {
        let seen = seen.lock().unwrap();
        assert_eq!(
            seen.len(),
            1,
            "the recorded draft must cost the provider nothing: {:?}",
            seen.iter().map(|request| &request.path).collect::<Vec<_>>()
        );
        assert_eq!(seen[0].path, "/v1/systemone", "the one call is the judge's");
        assert_eq!(
            header_value(&seen[0].headers, "authorization"),
            Some(format!("Bearer {JUDGE_SENTINEL}").as_str())
        );
        assert!(
            !seen[0]
                .headers
                .iter()
                .any(|(_, value)| value.contains(DRAFT_SENTINEL)),
            "the draft credential is never leased on this run"
        );
    }

    // The same goal through BOTH recorded doors: the pairing must produce the same document, and
    // this is what makes the request-count assertion above a claim about the result rather than
    // only about the traffic.
    let recorded_out = directory.path().join("both-recorded.json");
    let recorded = envelope(&[
        "graph",
        "synthesize",
        "--goal",
        &first_compile_goal(),
        "--out",
        recorded_out.to_str().unwrap(),
        "--allow-program",
        "cargo",
        "--fixture",
        fixtures()
            .join("first-compile/replies.json")
            .to_str()
            .unwrap(),
        "--judge-fixture",
        fixtures()
            .join("judge/nodes-below-threshold.json")
            .to_str()
            .unwrap(),
    ]);
    assert_eq!(recorded["ok"], true, "{recorded}");
    assert_eq!(
        std::fs::read(&out).unwrap(),
        std::fs::read(&recorded_out).unwrap(),
        "--out must be byte-equal to the both-recorded run"
    );
}

/// #1137: what is still refused, and the two arms ask different questions.
///
/// `--fixture` with `--route` is two DRAFT doors and stays refused. `--fixture` with `--manifest`
/// and no judge door is refused because the manifest would then serve nothing — and without this
/// arm the change would read as "`--manifest` beside `--fixture` is simply ignored", which is a
/// silent no-op for a typo rather than an answer to one.
#[test]
fn a_fixture_still_refuses_a_draft_route_and_a_manifest_that_serves_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = fixtures().join("first-compile/replies.json");
    let manifest = directory.path().join("manifest.json");
    std::fs::write(&manifest, "{}").unwrap();

    let two_draft_doors = envelope(&[
        "graph",
        "synthesize",
        "--goal",
        &first_compile_goal(),
        "--out",
        directory.path().join("a.json").to_str().unwrap(),
        "--allow-program",
        "cargo",
        "--fixture",
        fixture.to_str().unwrap(),
        "--manifest",
        manifest.to_str().unwrap(),
        "--route",
        "draft",
        "--judge-route",
        "judge",
    ]);
    assert_eq!(two_draft_doors["ok"], false, "{two_draft_doors}");
    assert_eq!(
        two_draft_doors["diagnostics"][0]["path"], "/fixture",
        "{two_draft_doors}"
    );

    let manifest_serves_nothing = envelope(&[
        "graph",
        "synthesize",
        "--goal",
        &first_compile_goal(),
        "--out",
        directory.path().join("b.json").to_str().unwrap(),
        "--allow-program",
        "cargo",
        "--fixture",
        fixture.to_str().unwrap(),
        "--manifest",
        manifest.to_str().unwrap(),
    ]);
    assert_eq!(
        manifest_serves_nothing["ok"], false,
        "{manifest_serves_nothing}"
    );
    assert_eq!(
        manifest_serves_nothing["diagnostics"][0]["path"], "/judgeRoute",
        "{manifest_serves_nothing}"
    );
}

/// `--judge-route` naming a chat route (`anthropic`, `direct_api`) is `GHCLI009` at
/// `/judgeRoute`, the message naming the one shape accepted — and it costs no lease and no
/// request: the provider sees nothing, not even the draft, because the judge door is checked
/// before the first draft is asked.
#[test]
fn a_judge_route_that_is_a_chat_route_is_refused_by_name_before_any_request() {
    let (directory, manifest, seen) = judge_route_bench();
    let out = directory.path().join("chat.json");
    let output = synthesize_over_routes(
        directory.path(),
        &manifest,
        &out,
        &["--judge-route", "draft"],
    );
    assert!(!output.status.success(), "{output:?}");
    assert_no_leak(&output, &out);
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], false, "{value}");
    assert_eq!(
        value["diagnostics"][0]["code"], GATEWAY_INVALID_CODE,
        "{value}"
    );
    assert_eq!(value["diagnostics"][0]["path"], "/judgeRoute", "{value}");
    let message = value["diagnostics"][0]["message"].as_str().unwrap();
    assert!(
        message.contains("direct_api typesafe"),
        "the refusal names the accepted shape: {message}"
    );
    assert!(
        seen.lock().unwrap().is_empty(),
        "no request may reach the provider"
    );
    assert!(!out.exists(), "a refusal writes nothing");
}

/// A disabled route is refused as disabled, at `/judgeRoute`, with no request placed; a route
/// the manifest does not declare likewise.
#[test]
fn a_disabled_or_unknown_judge_route_is_refused_at_its_pointer() {
    let (directory, manifest, seen) = judge_route_bench();
    for (route, word) in [("dormant", "disabled"), ("absent", "does not name")] {
        let out = directory.path().join(format!("{route}.json"));
        let output =
            synthesize_over_routes(directory.path(), &manifest, &out, &["--judge-route", route]);
        assert!(!output.status.success(), "{route}: {output:?}");
        assert_no_leak(&output, &out);
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            value["diagnostics"][0]["code"], GATEWAY_INVALID_CODE,
            "{value}"
        );
        assert_eq!(value["diagnostics"][0]["path"], "/judgeRoute", "{value}");
        let message = value["diagnostics"][0]["message"].as_str().unwrap();
        assert!(message.contains(word), "{route}: {message}");
        assert!(!out.exists(), "{route}: a refusal writes nothing");
    }
    assert!(
        seen.lock().unwrap().is_empty(),
        "no request may reach the provider"
    );
}

/// clap refuses the pair and the orphan before the command runs: `--judge-route` beside
/// `--judge-fixture` names both flags, `--judge-route` without `--manifest` names the missing
/// one. Exit 2, nothing on stdout, nothing written.
#[test]
fn judge_route_beside_judge_fixture_or_without_manifest_is_a_usage_error() {
    let directory = tempfile::tempdir().unwrap();
    let goal = first_compile_goal();
    let fixture = fixtures().join("first-compile/replies.json");
    let judge = fixtures().join("judge/nodes-below-threshold.json");
    let cases: [(&str, &[&str], &[&str]); 2] = [
        (
            "pair.json",
            &[
                "--manifest",
                "m.json",
                "--route",
                "draft",
                "--judge-route",
                "judge",
                "--judge-fixture",
                judge.to_str().unwrap(),
            ],
            &["--judge-route", "--judge-fixture"],
        ),
        (
            "orphan.json",
            &[
                "--fixture",
                fixture.to_str().unwrap(),
                "--judge-route",
                "judge",
            ],
            &["--judge-route", "--manifest"],
        ),
    ];
    for (name, flags, named) in cases {
        let out = directory.path().join(name);
        let output = command()
            .args([
                "graph",
                "synthesize",
                "--goal",
                &goal,
                "--out",
                out.to_str().unwrap(),
                "--allow-program",
                "cargo",
            ])
            .args(flags)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{name}: {output:?}");
        assert!(
            output.stdout.is_empty(),
            "{name}: a usage error prints no envelope"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        for flag in named {
            assert!(
                stderr.contains(flag),
                "{name}: stderr must name {flag}: {stderr}"
            );
        }
        assert!(!out.exists(), "{name}: nothing is written");
    }
}
