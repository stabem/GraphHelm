//! `development compile-context` must consult the token budget before it compiles (#393).
//!
//! `core/runtime` already decides this. `context_compiler::fit_within_budget` refuses with the
//! allocated code `context_budget_insufficient` when the REQUIRED context cannot fit, and it is
//! covered by its own tests. **Nothing on any surface calls it.** Measured at base `9e90a29`:
//! `compile_plan`, `compile_plan_against`, `compile_plan_within` and `fit_within_budget` have
//! zero call sites under `apps/`, and `fit_within_budget` has no production caller anywhere --
//! its only non-test occurrence in `core/` is its own definition.
//!
//! What the CLI calls instead is `compile_capsule`, which is a pure serializer: it takes an id, a
//! version and sections, and returns bytes. It has no budget parameter and cannot refuse. So the
//! command compiles a capsule without ever asking whether it fits, and the check that would have
//! asked sits one module over, tested and unreachable.
//!
//! **The production change this file catches is the one that looks like a fix and is not:**
//! giving the command an input and then trimming the required context to make it fit. That
//! returns a capsule, under budget, with every number healthy and without the evidence the caller
//! was required to see -- `fit_within_budget`'s own doc names this as the whole point of the
//! asymmetry. A silent trim is invisible; a refusal is not. So the assertion below is on the
//! REFUSAL, never on the size of what came back.

use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// One required section, and a budget it cannot fit in.
///
/// The two numbers are computed rather than written down, and that is deliberate. A hardcoded
/// pair invites the next reader to adjust one of them until the test passes, which is the edit
/// that turns this cell vacuous without anyone noticing: if the required content ever fit inside
/// the budget, `fit_within_budget` would return `Fits` and the command would be RIGHT to succeed.
/// The test would then be green over the exact behaviour it exists to forbid.
///
/// The literal deliberately does NOT state its own length. A first draft of this line ended
/// `"at sixty-four bytes."` and was sixty-FIVE -- a false claim written into the fixture of a
/// test about false claims. Every length below is `REQUIRED.len()`, so there is one producer of
/// that number and nothing to drift.
const REQUIRED: &str = "the evidence the caller was required to see.";
const BUDGET: usize = 8;

struct Run {
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

/// Invoke the binary WITHOUT panicking on a non-zero exit or on stdout that is not JSON.
///
/// **This tolerance is the point, not laziness.** Today the flags below do not exist, so `clap`
/// rejects the invocation, exits non-zero and writes nothing to stdout. A helper that did
/// `serde_json::from_slice(..).expect(..)` would panic HERE, in the arrangement, and the failure
/// would name a JSON parse error -- a true statement about the wrong thing. The condition this
/// test is about is named by the assertion, so the arrangement must survive long enough for the
/// assertion to be reached and to print what actually happened.
fn run(args: &[&str]) -> Run {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(args)
        .output()
        .expect("the built binary is invocable");
    Run {
        status: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

impl Run {
    /// The whole run, for a failure message. A reader of the red must be able to tell
    /// "the flag does not exist yet" from "the flag exists and the command answered success".
    fn report(&self) -> String {
        format!(
            "exit {:?}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            self.status,
            self.stdout.trim(),
            self.stderr.trim()
        )
    }
}

/// The fixture must actually violate the budget, or the cell below proves nothing.
///
/// A required section that fits is a legitimate `Fits`, and a command that succeeded on it would
/// be correct. Asserting the arithmetic separately means this can never become a test that passes
/// because its own input stopped being a violation.
#[test]
fn the_fixture_really_exceeds_the_budget() {
    assert!(
        REQUIRED.len() > BUDGET,
        "HARNESS-BROKE: the required section is {} bytes and the budget is {BUDGET}, so it FITS. \
         The refusal cell below would then be asserting a refusal that must not happen",
        REQUIRED.len()
    );
}

/// Required context that cannot fit must be REFUSED, on the CLI surface.
///
/// The production change that makes this fail once it passes: routing `compile-context` back to
/// `compile_capsule` without consulting `fit_within_budget`, or trimming `required` to fit. Both
/// return a digest and exit zero, which is what this asserts against.
#[test]
fn compile_context_refuses_required_context_that_cannot_fit() {
    let budget = BUDGET.to_string();
    let outcome = run(&[
        "development",
        "compile-context",
        "--budget",
        &budget,
        "--require",
        REQUIRED,
    ]);

    let envelope: serde_json::Value = serde_json::from_str(&outcome.stdout).unwrap_or_else(|_| {
        panic!(
            "`development compile-context` did not answer the JSON envelope. Required context of \
             {} bytes against a budget of {BUDGET} must be refused, and today the command takes \
             no input at all -- it calls `compile_capsule(\"\", 1, &[])` and cannot refuse.\n{}",
            REQUIRED.len(),
            outcome.report()
        )
    });

    assert_eq!(
        envelope["ok"],
        false,
        "required context of {} bytes was accepted against a budget of {BUDGET}. Either the \
         budget was never consulted, or the required context was trimmed to fit -- and a trim \
         returns a healthy-looking capsule that is missing evidence the caller had to see.\n{}",
        REQUIRED.len(),
        outcome.report()
    );
    assert_eq!(
        envelope["diagnostics"][0]["code"],
        "context_budget_insufficient",
        "the refusal did not carry the allocated code. `fit_within_budget` deliberately does not \
         answer `cardinality_violation` here: nothing is malformed, and the operator's move is to \
         grant more budget rather than to correct the input.\n{}",
        outcome.report()
    );
}

/// The refusal exits with the code ALLOCATED to it, not the generic domain-failure 2.
///
/// `development_refusal_exit_code` computes `20 + ordinal` over `DevelopmentRefusalCode::every()`,
/// and has sat dormant behind `#[cfg_attr(not(test), expect(dead_code))]` since #235 waiting for a
/// caller. This is that caller.
///
/// **The literal is a PREDICTION made before it was measured, and it is load-bearing.**
/// `ContextBudgetInsufficient` is the thirteenth entry in the vocabulary, ordinal 12, so the
/// allocated code is 32. `apps/cli` is bin-only, so an integration test cannot import the mapping
/// and compare against it -- the number has to be written here. That makes this test a witness to
/// the vocabulary's append-only rule: the declaration says codes are "appended, never reordered",
/// because a reorder "is invisible here and a renumbering downstream". A reorder that renumbers
/// this refusal fails HERE, which is the downstream that would otherwise stay silent.
#[test]
fn the_refusal_exits_with_its_allocated_code_not_the_generic_domain_failure() {
    let budget = BUDGET.to_string();
    let outcome = run(&[
        "development",
        "compile-context",
        "--budget",
        &budget,
        "--require",
        REQUIRED,
    ]);

    assert_eq!(
        outcome.status,
        Some(32),
        "the refusal exited {:?}. 2 means it took `Outcome::domain`'s generic domain-failure code \
         and the allocated identity never reached the exit status; anything else means the \
         refusal vocabulary was REORDERED, which the declaration forbids because it renumbers \
         every downstream consumer silently.\n{}",
        outcome.status,
        outcome.report()
    );
}

/// The framing the compiled capsule adds around the items: the capsule id, the version, the
/// section name, the counts and a length prefix per part. The budget bounds the RENDERED capsule
/// (#1065 review), so a budget that fits the required text by one byte no longer fits the
/// capsule it compiles to; this allowance is generous rather than exact — an exact number would
/// be a second producer of the framing size, and the cell is about fitting, not about the size.
const FRAMING_ALLOWANCE: usize = 256;

/// A capsule whose required context fits must still compile, and must NOT refuse.
///
/// Without this, the cheapest way to make the cell above pass is to refuse unconditionally, and
/// every assertion there would stay green while the command became useless. This is the
/// uninformative cell: it is the one that fails if the fix is "always refuse".
#[test]
fn compile_context_still_compiles_when_the_required_context_fits() {
    let budget = (REQUIRED.len() + FRAMING_ALLOWANCE).to_string();
    let outcome = run(&[
        "development",
        "compile-context",
        "--budget",
        &budget,
        "--require",
        REQUIRED,
    ]);

    let envelope: serde_json::Value = serde_json::from_str(&outcome.stdout).unwrap_or_else(|_| {
        panic!(
            "`development compile-context` did not answer the JSON envelope.\n{}",
            outcome.report()
        )
    });

    assert_eq!(
        envelope["ok"],
        true,
        "required context of {} bytes was refused against a budget of {budget}, which it fits. A \
         fix that refuses unconditionally passes the refusal cell above and is still wrong.\n{}",
        REQUIRED.len(),
        outcome.report()
    );
    assert!(
        envelope["data"]["digest"].is_string(),
        "the capsule compiled but reported no digest, so nothing proves the compiler ran.\n{}",
        outcome.report()
    );
}

// ---------------------------------------------------------------------------------------------
// The HTTP surface.
//
// The harness is a copy, and that is the house convention rather than laziness: `apps/cli` is
// bin-only, so integration tests cannot share code through a library target --
// `mcp_capability.rs` states the rule ("each integration test file is self-contained").
// Deliberately minimal next to `api_http.rs`'s `ServerGuard`, for the same reason
// `development_surface_parity.rs` gives: this sends one request and exits, so it needs none of
// that struct's drain-thread machinery.
//
// **Why this surface gets its own cells instead of inheriting the CLI's.** The CLI and HTTP
// paths are two different call sites into the same function, and a route that ignores its body
// answers exactly like a route that read it and found nothing to refuse. The CLI passing proves
// nothing about the route -- that is the parity this issue exists to stop assuming.
// ---------------------------------------------------------------------------------------------

struct Server {
    child: std::process::Child,
    address: String,
    token: String,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_server(directory: &Path) -> Server {
    let events = directory.join("events");
    let mut child = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args([
            "serve",
            "--events",
            events.to_str().expect("the temp path is UTF-8"),
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
    let started: serde_json::Value = loop {
        let mut byte = [0u8; 1];
        match stdout.read(&mut byte) {
            Ok(1) if byte[0] == b'\n' => {
                break serde_json::from_slice(&buffer).unwrap_or_else(|error| {
                    let _ = child.kill();
                    panic!(
                        "the startup line was not JSON ({error}): {:?}",
                        String::from_utf8_lossy(&buffer)
                    )
                });
            }
            Ok(1) => buffer.push(byte[0]),
            _ => {
                assert!(
                    Instant::now() < deadline,
                    "`graphhelm serve` printed no startup line within 30s"
                );
            }
        }
    };
    let address = started["data"]["address"]
        .as_str()
        .unwrap_or_else(|| panic!("startup envelope must carry data.address: {started}"))
        .to_owned();

    let mut name = events
        .file_name()
        .map_or_else(|| std::ffi::OsString::from("events"), ToOwned::to_owned);
    name.push(".token");
    let token_path = events.with_file_name(name);
    let token_deadline = Instant::now() + Duration::from_secs(5);
    let token = loop {
        if let Ok(contents) = std::fs::read_to_string(&token_path)
            && !contents.is_empty()
        {
            break contents;
        }
        assert!(
            Instant::now() < token_deadline,
            "the server never wrote a readable token file at {token_path:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    };

    Server {
        child,
        address,
        token,
    }
}

/// POST a JSON body and return the status code with the parsed envelope.
///
/// Returns BOTH because they can be wrong separately: a route can answer the right status with an
/// empty body, or the right body under a 200.
fn post_json(server: &Server, path: &str, body: &serde_json::Value) -> (u16, serde_json::Value) {
    let (host, port) = server
        .address
        .split_once(':')
        .expect("the startup address always carries an explicit port");
    let mut stream = TcpStream::connect((host, port.parse::<u16>().expect("the port is a number")))
        .expect("the server accepts a connection right after startup");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("read timeout is settable");
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .expect("write timeout is settable");

    let payload = serde_json::to_vec(body).expect("the request body serializes");
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        server.token.trim(),
        payload.len()
    );
    stream
        .write_all(request.as_bytes())
        .expect("the request header is writable");
    stream
        .write_all(&payload)
        .expect("the request body is writable");

    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .expect("the server answers before the timeout");
    let text = String::from_utf8_lossy(&raw);
    let status = text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or_else(|| panic!("malformed HTTP response: {text:?}"));
    let envelope = text.split_once("\r\n\r\n").map_or_else(
        || serde_json::Value::Null,
        |(_, body)| serde_json::from_str(body.trim()).unwrap_or(serde_json::Value::Null),
    );
    (status, envelope)
}

/// Required context that cannot fit must be refused over HTTP too, with the ALLOCATED status.
///
/// 422 is a PREDICTION, made from `development_refusal_http_status` before it was measured:
/// `ContextBudgetInsufficient` sits in the `UNPROCESSABLE_ENTITY` arm, on the stated ground that
/// the request is well-formed and simply cannot be honoured at the budget given.
///
/// That mapping is DELIBERATELY not injective -- several refusals share a status, because a
/// status is a claim with ecosystem-wide meaning and minting one per refusal would be worse than
/// the coarseness it fixes. So the status alone cannot identify the refusal, and the body is
/// asserted alongside it rather than instead of it.
#[test]
fn the_http_route_refuses_required_context_that_cannot_fit() {
    let directory = tempfile::tempdir().expect("a temp directory is creatable");
    let server = start_server(directory.path());
    let (status, envelope) = post_json(
        &server,
        "/v1/development/context",
        &serde_json::json!({"budget": BUDGET, "require": [REQUIRED]}),
    );

    assert_eq!(
        status,
        422,
        "required context of {} bytes was not refused with 422 over HTTP. 200 means the route \
         ignored its request body and called the compiler with the degenerate arguments, which \
         answers exactly like a route that read the body and found nothing to refuse.\n{envelope}",
        REQUIRED.len()
    );
    assert_eq!(
        envelope["diagnostics"][0]["code"], "context_budget_insufficient",
        "the HTTP refusal did not carry the allocated code. The status is deliberately shared \
         between several refusals, so the body is the only place the exact one survives.\n\
         {envelope}"
    );
}

/// The same route must still compile when the required context fits.
///
/// The uninformative cell for this surface: without it, a route that refuses every request passes
/// the cell above.
#[test]
fn the_http_route_still_compiles_when_the_required_context_fits() {
    let directory = tempfile::tempdir().expect("a temp directory is creatable");
    let server = start_server(directory.path());
    let (status, envelope) = post_json(
        &server,
        "/v1/development/context",
        &serde_json::json!({"budget": REQUIRED.len() + FRAMING_ALLOWANCE, "require": [REQUIRED]}),
    );

    assert_eq!(
        status, 200,
        "required context that FITS was refused over HTTP. A route that refuses unconditionally \
         passes the refusal cell above and is still wrong.\n{envelope}"
    );
    assert!(
        envelope["data"]["digest"].is_string(),
        "the route answered 200 without a digest, so nothing proves the compiler ran.\n{envelope}"
    );
}

/// A request with no body at all keeps its existing meaning.
///
/// This is the compatibility cell. `POST /v1/development/context` with an empty object is what
/// the existence-parity guard sends, and what every caller written before this change sends.
/// Reading a body must not turn those into refusals or 400s.
#[test]
fn the_http_route_still_accepts_the_bodyless_request_it_answered_before() {
    let directory = tempfile::tempdir().expect("a temp directory is creatable");
    let server = start_server(directory.path());
    let (status, envelope) = post_json(&server, "/v1/development/context", &serde_json::json!({}));

    assert_eq!(
        status, 200,
        "the argument-free request stopped working. The existence-parity guard sends exactly \
         this, so a change that breaks it breaks a surface contract that predates the \
         budget.\n{envelope}"
    );
    assert!(
        envelope["data"]["digest"].is_string(),
        "the bodyless request answered 200 without a digest.\n{envelope}"
    );
}

// ---------------------------------------------------------------------------------------------
// The MCP surface.
//
// **A tool refusal is a RESULT with `isError: true`, not a JSON-RPC protocol error, and that
// distinction is the whole design here.** The protocol's error channel is for failures of the
// protocol itself -- unknown method, malformed request, bad params -- and every existing
// `HandlerOutcome::Error` in this codebase carries exactly those (`INVALID_REQUEST`,
// `INVALID_PARAMS`, `METHOD_NOT_FOUND`). A budget that cannot fit is not a protocol failure: the
// call was well-formed and the tool ran and said no. Routing it through the error channel would
// tell a chat client the request was broken when it was answered.
//
// So the refusal's identity does not need a new `error.data.code` field to travel here. The
// shared tail of `tools::call` already turns a non-ok envelope into `isError: true` with the
// envelope as the content, and the envelope is where the allocated code already lives.
// ---------------------------------------------------------------------------------------------

/// Call one tool over a real MCP stdio session, against a real running server.
///
/// Returns the `tools/call` reply. Both processes are real: an MCP test that stubs the API cannot
/// see a refusal that only exists because the route produced it.
fn mcp_call(server: &Server, arguments: &serde_json::Value) -> serde_json::Value {
    let mut child = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args([
            "mcp",
            "--url",
            &format!("http://{}", server.address),
            "--actor",
            "agent-chat",
        ])
        .env("GRAPHHELM_API_TOKEN", server.token.trim())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("graphhelm mcp spawns");

    {
        let mut stdin = child.stdin.take().expect("stdin is piped");
        for line in [
            serde_json::json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "context-budget", "version": "0"}
                }
            }),
            serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            serde_json::json!({
                "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": {"name": "compile_context", "arguments": arguments}
            }),
        ] {
            writeln!(stdin, "{line}").expect("the session accepts a request line");
        }
    }

    let output = child
        .wait_with_output()
        .expect("the mcp session exits on EOF");
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|reply| reply["id"] == serde_json::json!(2))
        .unwrap_or_else(|| {
            panic!(
                "no reply to the tools/call carrying id 2.\n--- stdout ---\n{stdout}\n\
                 --- stderr ---\n{}",
                String::from_utf8_lossy(&output.stderr)
            )
        })
}

/// Required context that cannot fit must be refused over MCP too, as a tool error.
#[test]
fn the_mcp_tool_refuses_required_context_that_cannot_fit() {
    let directory = tempfile::tempdir().expect("a temp directory is creatable");
    let server = start_server(directory.path());
    let reply = mcp_call(
        &server,
        &serde_json::json!({"budget": BUDGET, "require": [REQUIRED]}),
    );

    assert!(
        reply["error"].is_null(),
        "the refusal was reported as a JSON-RPC protocol error. The call was well-formed and the \
         tool answered; the protocol error channel is for unknown methods and malformed \
         requests.\n{reply}"
    );
    assert_eq!(
        reply["result"]["isError"],
        serde_json::json!(true),
        "required context of {} bytes was reported as a successful tool call. Most likely the \
         tool still forwards an empty body and never sent the budget at all.\n{reply}",
        REQUIRED.len()
    );

    let text = reply["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    assert!(
        text.contains("context_budget_insufficient"),
        "the tool error did not carry the allocated refusal code. `isError: true` alone says \
         something went wrong and not WHAT, which is the identity this issue exists to \
         deliver.\ntext: {text}\n{reply}"
    );
}

/// The same tool must still succeed when the required context fits.
///
/// The uninformative cell for this surface. It also proves the harness reaches a real server: a
/// session that never connected would report a transport error here, not a digest.
#[test]
fn the_mcp_tool_still_compiles_when_the_required_context_fits() {
    let directory = tempfile::tempdir().expect("a temp directory is creatable");
    let server = start_server(directory.path());
    let reply = mcp_call(
        &server,
        &serde_json::json!({"budget": REQUIRED.len() + FRAMING_ALLOWANCE, "require": [REQUIRED]}),
    );

    assert_eq!(
        reply["result"]["isError"],
        serde_json::json!(false),
        "required context that FITS was reported as a tool error.\n{reply}"
    );
    let text = reply["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    assert!(
        text.contains("digest"),
        "the successful tool call carried no digest, so nothing proves it reached the \
         compiler.\ntext: {text}"
    );
}
