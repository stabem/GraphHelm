//! Native-runtime adapters: spawn an official CLI (Claude Code or Codex) as a subprocess and
//! speak to it over stdio, never a network call the gateway places itself.
//!
//! `docs/models/UNIVERSAL_MODEL_GATEWAY.md` §6.3/§13 describes native-runtime routes; this module
//! implements the Milestone 05b subset of it
//! (`docs/superpowers/plans/2026-08-14-gateway-slice.md` Task 5): one [`RuntimeAdapter`] per
//! `native_runtime` [`ModelRoute`], spawning `route.command.program` with `route.command.args`
//! under an environment that structurally cannot carry gateway/broker material, sending the
//! prompt over stdin (never argv, never an environment variable), bounding the wait with the
//! route's deadline, and mapping the outcome onto the closed [`GatewayError`] taxonomy
//! `core/gateway` already owns.
//!
//! §6.3's separation is the whole point of this module: a native runtime owns its own
//! authentication (its own login, its own config directory, its own token store) and the gateway
//! never touches it — [`RouteManifest::from_json`](graphhelm_gateway::manifest::RouteManifest::from_json)
//! already refuses a `native_runtime` route that carries `credentialRef`, and this module closes
//! the other half of that promise: the *process environment* the spawned CLI actually runs under
//! is built from nothing (`env_clear`) plus a fixed, published allowlist of the ordinary
//! interpreter/OS variables a CLI needs to find its own config and auth, plus whatever the caller
//! explicitly passes as `extra_env` — never a copy of the gateway's own ambient environment.

use std::io::{Read, Write};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use graphhelm_gateway::call::{ModelCall, ModelReply, Usage};
use graphhelm_gateway::manifest::{ModelRoute, RuntimeKind, Transport};
use graphhelm_gateway::taxonomy::GatewayError;
use serde::Deserialize;

use crate::env;

/// How often [`RuntimeAdapter::invoke`] polls `Child::try_wait` while waiting for the child to
/// exit or the route's deadline to expire. `std::process::Child` has no blocking "wait with
/// timeout" in `std` (that requires either a platform-specific wait primitive or a crate this
/// workspace does not depend on); polling is the plan's own prescribed shape for this loop.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Bound on how much of a pipe's bytes [`spawn_capped_reader`] retains. The reader thread keeps
/// draining the pipe past this bound (so a verbose child, or one whose grandchild keeps writing,
/// never backs up and stalls on its own `write`), but bytes beyond it are discarded rather than
/// retained — the same "cap, don't buffer unboundedly" posture
/// `core/gateway/src/manifest.rs::MAX_MANIFEST_BYTES` and its neighbors take elsewhere in this
/// codebase.
const MAX_CAPTURED_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

/// Case-insensitive substrings that, when found in *parsed* error text (never raw, unparsed
/// stdout/stderr — see [`classify_error_text`] and PR review IMPORTANT 4), are this milestone's
/// heuristic signal that a runtime failure was capacity exhaustion rather than an ordinary crash.
///
/// This is explicitly a heuristic, not a wire contract: neither official CLI this milestone
/// targets publishes a stable, versioned "you are out of quota" exit shape for the gateway to key
/// off instead, so this module does the same kind of best-effort text sniffing §17 tolerates
/// elsewhere and no more — a marker match is treated as [`GatewayError::QuotaExhausted`]
/// (parking the route, §12), and its absence falls through to the more conservative
/// [`GatewayError::RuntimeCrashed`] (merely retried). Revisit once Milestone 05e's live-host
/// verification either confirms a firmer signal or documents that none exists.
const QUOTA_MARKERS: &[&str] = &["quota", "rate limit", "usage limit"];

/// The unparsed outcome of spawning the configured runtime once: its exit status and captured
/// stdout/stderr, before any [`RuntimeKind`]-specific parsing.
///
/// [`RuntimeAdapter::call`] is built on top of [`RuntimeAdapter::invoke`], which returns this;
/// it is exposed separately because not every invocation produces output that parses as one of
/// the two happy shapes `call` knows how to interpret — most notably `fake_runtime`'s
/// `env-dump` test mode, which exists specifically so a test can inspect exactly what
/// environment the child process received rather than trusting the adapter's own account of it.
#[derive(Debug)]
pub struct RawInvocation {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// One native-runtime adapter bound to a specific `native_runtime` [`ModelRoute`].
///
/// Construction does not itself touch the process table; each [`Self::call`]/[`Self::invoke`]
/// spawns exactly one child process. `extra_env` is the *only* mechanism this adapter has for
/// adding anything to the allowlist described on [`env::ENV_ALLOWLIST`] — production callers pass an
/// empty vector (a real `claude`/`codex` invocation needs nothing beyond the allowlist to
/// authenticate; that is the entire point of native-runtime routes owning their own auth), and
/// tests use it to steer `fake_runtime` (`FAKE_RUNTIME_MODE`/`FAKE_RUNTIME_SHAPE`) without that
/// steering being mistaken for part of the allowlist itself.
pub struct RuntimeAdapter<'a> {
    route: &'a ModelRoute,
    extra_env: Vec<(String, String)>,
}

impl<'a> RuntimeAdapter<'a> {
    #[must_use]
    pub fn new(route: &'a ModelRoute, extra_env: Vec<(String, String)>) -> Self {
        Self { route, extra_env }
    }

    /// Spawns the configured runtime once with `request.prompt` on stdin, waits for it to exit
    /// (or kills it at the route's deadline), and parses its stdout per the route's
    /// [`RuntimeKind`].
    ///
    /// `request.max_tokens` is deliberately not forwarded anywhere: a native-runtime route's own
    /// `command.args` (set once, in the manifest, by whoever configured the route) fully controls
    /// how the spawned CLI is invoked, the same way Task 4's OpenAI adapter deliberately does not
    /// forward it for reasons specific to that wire shape — here there is no per-call channel to
    /// forward it through at all, since the prompt is the only thing that travels per call
    /// (stdin), and neither host CLI's non-interactive invocation shape this milestone targets
    /// takes a per-call output-length argument over stdin alongside the prompt.
    ///
    /// # Errors
    /// See [`GatewayError`]; in particular [`GatewayError::UnsupportedCapability`] if
    /// `self.route` is not a [`Transport::NativeRuntime`] route (PR review MEDIUM 12 — mirrors
    /// `byok.rs`'s `ByokAdapter::call` guard: `RuntimeAdapter::new` takes any `&ModelRoute`,
    /// nothing at the type level narrows it to `native_runtime`, so a structurally *valid* route
    /// can still be the wrong *kind* for this adapter — most concretely a `direct_api` route,
    /// which carries no `runtime`/`command` at all. This function never panics: the transport
    /// check is what makes that true, since without it a `direct_api` route would reach
    /// `self.route.runtime()`'s `.expect()`), [`GatewayError::Timeout`] if the process does not
    /// exit within `route.timeout_seconds()`, and [`GatewayError::MalformedOutput`] if it exits
    /// `0` but its stdout does not parse as its `RuntimeKind`'s happy shape.
    pub fn call(&self, request: &ModelCall) -> Result<ModelReply, GatewayError> {
        if self.route.transport() != Transport::NativeRuntime {
            return Err(GatewayError::UnsupportedCapability);
        }
        let kind = self
            .route
            .runtime()
            .expect("native_runtime routes carry runtime — enforced by manifest validation");
        let invocation = self.invoke(&request.prompt)?;
        interpret(kind, &invocation)
    }

    /// Spawns the configured runtime once with `prompt` on stdin, applying the deadline and
    /// environment allowlist, and returns its raw exit status and captured stdio without any
    /// [`RuntimeKind`]-specific parsing. [`Self::call`] is the production entry point; this is
    /// exposed for the tests described on [`RawInvocation`].
    ///
    /// # Errors
    /// [`GatewayError::UnsupportedCapability`] under the same non-`native_runtime`-route
    /// condition as [`Self::call`] (this function alone reaches `self.route.command()`'s
    /// `.expect()`, so it needs the same guard). [`GatewayError::Timeout`] if the process does
    /// not exit within `route.timeout_seconds()` (the direct child is killed and reaped before
    /// this returns; see the module-level note on [`spawn_capped_reader`] about why the *reader
    /// threads* are deliberately not waited on past that point — PR review IMPORTANT 6).
    /// [`GatewayError::ProviderUnavailable`] if the process could not be spawned at all (e.g.
    /// `command.program` does not resolve) — the native-runtime analogue of a BYOK transport
    /// never obtaining a response, per `byok.rs`'s own `execute()`. [`GatewayError::RuntimeCrashed`]
    /// in the practically-unreachable case where the OS wait call itself fails, since at that
    /// point this function cannot tell whether the child is even still running.
    pub fn invoke(&self, prompt: &str) -> Result<RawInvocation, GatewayError> {
        if self.route.transport() != Transport::NativeRuntime {
            return Err(GatewayError::UnsupportedCapability);
        }
        let command_spec = self
            .route
            .command()
            .expect("native_runtime routes carry command — enforced by manifest validation");

        let mut command = Command::new(&command_spec.program);
        command
            .args(&command_spec.args)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in env::compose(&self.extra_env) {
            command.env(key, value);
        }

        let mut child = command
            .spawn()
            .map_err(|_| GatewayError::ProviderUnavailable)?;

        // The prompt is written on its own thread — spawned here, before the reader threads and
        // before the deadline loop below ever runs — rather than with a direct blocking write on
        // this (the calling) thread. A previous version of this function wrote here directly, on
        // the assumption that every prompt this milestone sends is a short test/operator string
        // comfortably under a pipe's OS buffer; that assumption does not hold for a real prompt,
        // and a large one against a child that is slow to read (or, per `FAKE_RUNTIME_MODE=hang`,
        // never reads stdin at all) fills the pipe and blocks a direct write forever — on the
        // calling thread, which is also the thread that would otherwise go on to run the deadline
        // loop, so nothing would ever be left to kill the child. Spawning the writer thread ahead
        // of the stdout/stderr readers also closes a write-write deadlock window: without a
        // reader already running, a child that starts flooding stdout while this process is still
        // blocked writing stdin can fill *its* pipe first and stall on its own write, leaving
        // both sides waiting on each other with nothing polling `try_wait` to notice or kill
        // either one. `stdin` is moved into the thread and dropped when it returns, sending EOF
        // the same way the previous inline block's scope-exit drop did — the prompt still travels
        // over stdin only, never argv, never an environment variable
        // (`the_prompt_travels_via_stdin_never_argv`).
        let mut stdin = child.stdin.take().expect("stdin was configured as piped");
        let prompt = prompt.to_owned();
        let stdin_writer = std::thread::spawn(move || {
            // A write failure is not treated as fatal on its own — the wait/parse path below
            // still runs and reports whatever the child actually produced. This now also covers
            // the deadline case: killing a child (below) closes its stdin read end, which turns a
            // still-in-flight write here into a `BrokenPipe` error rather than letting it hang —
            // exactly the unblocking mechanism the deadline path depends on to let this thread be
            // joined at all.
            let _ = stdin.write_all(prompt.as_bytes());
        });

        // Stdout/stderr are drained on background threads concurrently with the poll loop below,
        // into capped, lockable buffers rather than being read to completion and returned as
        // owned `Vec`s from the thread itself (PR review IMPORTANT 6). The distinction matters at
        // the deadline: `Child::kill` only kills the direct child. If that child spawned a
        // grandchild which inherited these same pipe write ends (and never redirected or closed
        // them before its own parent exited — `fake_runtime`'s `orphan` mode imitates exactly
        // this), the pipes stay open, and a reader that only returns once it sees EOF would block
        // forever waiting for a close that may never come — wedging this function long past its
        // own deadline even though the *direct* child is long dead and reaped. Reading into a
        // shared `Arc<Mutex<Vec<u8>>>` lets the poll loop below observe "has this reader seen
        // EOF/an error yet" via [`ReaderState::done`] without ever blocking on the reader thread
        // itself, and lets the timeout path return without joining these threads at all — see
        // that branch's own comment.
        let stdout_pipe = child.stdout.take().expect("stdout was configured as piped");
        let stderr_pipe = child.stderr.take().expect("stderr was configured as piped");
        let stdout_reader = spawn_capped_reader(stdout_pipe);
        let stderr_reader = spawn_capped_reader(stderr_pipe);

        let deadline = Instant::now() + Duration::from_secs(self.route.timeout_seconds());
        let mut exited: Option<ExitStatus> = None;
        let outcome = loop {
            if exited.is_none() {
                match child.try_wait() {
                    Ok(Some(status)) => exited = Some(status),
                    Ok(None) => {}
                    Err(_) => break WaitOutcome::WaitFailed,
                }
            }
            if let Some(status) = exited
                && stdout_reader.done.load(Ordering::Acquire)
                && stderr_reader.done.load(Ordering::Acquire)
            {
                break WaitOutcome::Exited(status);
            }
            if Instant::now() >= deadline {
                break WaitOutcome::TimedOut;
            }
            std::thread::sleep(POLL_INTERVAL);
        };

        match outcome {
            WaitOutcome::Exited(status) => {
                // Both readers already observed EOF/an error (that is what let the loop above
                // reach this branch), so these joins are just cleanup, not a wait.
                let _ = stdin_writer.join();
                let _ = stdout_reader.handle.join();
                let _ = stderr_reader.handle.join();
                Ok(RawInvocation {
                    status,
                    stdout: take_buffer(&stdout_reader.buffer),
                    stderr: take_buffer(&stderr_reader.buffer),
                })
            }
            WaitOutcome::TimedOut => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdin_writer.join();
                // Deliberately NOT joined (IMPORTANT 6): if a grandchild is still holding either
                // pipe's write end open, these reader threads are still blocked in `read` and may
                // never return. Dropping the `JoinHandle`s detaches them instead — they keep
                // running and keep draining (bounded by `MAX_CAPTURED_OUTPUT_BYTES`) in the
                // background for as long as whatever process still holds the pipe survives, but
                // this function does not wait on them. A deliberate leaked-thread trade-off, not
                // an oversight — recorded in the milestone doc's honest-limits section.
                drop(stdout_reader.handle);
                drop(stderr_reader.handle);
                Err(GatewayError::Timeout)
            }
            WaitOutcome::WaitFailed => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdin_writer.join();
                drop(stdout_reader.handle);
                drop(stderr_reader.handle);
                Err(GatewayError::RuntimeCrashed)
            }
        }
    }
}

/// The result of one `try_wait` poll loop: the child exited AND both readers have seen EOF/an
/// error, the deadline elapsed first, or the OS wait call itself failed (see
/// [`RuntimeAdapter::invoke`]'s doc comment).
enum WaitOutcome {
    Exited(ExitStatus),
    TimedOut,
    WaitFailed,
}

/// A capped pipe reader running on its own thread: `buffer` accumulates up to
/// [`MAX_CAPTURED_OUTPUT_BYTES`], `done` flips to `true` once the thread's `read` loop ends (EOF
/// or an error), and `handle` is the thread's `JoinHandle` — joined on the happy path, dropped
/// (detached) on the timeout/wait-failed paths. See [`RuntimeAdapter::invoke`]'s comment at its
/// call site for why `done` (checked without blocking) rather than a blocking `join` is what the
/// poll loop above waits on.
struct CappedReader {
    buffer: Arc<Mutex<Vec<u8>>>,
    done: Arc<AtomicBool>,
    handle: std::thread::JoinHandle<()>,
}

fn spawn_capped_reader(mut pipe: impl Read + Send + 'static) -> CappedReader {
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let done = Arc::new(AtomicBool::new(false));
    let writer_buffer = Arc::clone(&buffer);
    let writer_done = Arc::clone(&done);
    let handle = std::thread::spawn(move || {
        let mut chunk = [0_u8; 8192];
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    let mut guard = writer_buffer
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if guard.len() < MAX_CAPTURED_OUTPUT_BYTES {
                        let take = read.min(MAX_CAPTURED_OUTPUT_BYTES - guard.len());
                        guard.extend_from_slice(&chunk[..take]);
                    }
                }
            }
        }
        writer_done.store(true, Ordering::Release);
    });
    CappedReader {
        buffer,
        done,
        handle,
    }
}

fn take_buffer(buffer: &Mutex<Vec<u8>>) -> Vec<u8> {
    let mut guard = buffer
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    std::mem::take(&mut *guard)
}

/// Turns one raw invocation into a [`GatewayError`]-classified reply.
///
/// Exit `0`: parses per `kind`. For [`RuntimeKind::ClaudeCode`], a successfully parsed reply
/// whose `subtype`/`is_error` marks it as an error report (PR review IMPORTANT 3 — the CLI can
/// exit `0` while reporting a failed run, e.g. `subtype: "error_during_execution"`) is classified
/// by [`classify_error_text`] on its parsed error text rather than ever treated as a
/// [`ModelReply`]; a genuinely unparseable exit-`0` body is [`GatewayError::MalformedOutput`].
///
/// Nonzero exit: classified by [`classify_error_text`] on whatever *parsed* error text `kind`'s
/// shape yields (PR review IMPORTANT 4 — never a raw-stream scan of stdout/stderr); output that
/// does not parse at all is [`GatewayError::RuntimeCrashed`], never scanned for quota markers —
/// an unrelated raw string like a compiler's `EDQUOT`/"disk quota exceeded" message must not park
/// the route's capacity.
fn interpret(kind: RuntimeKind, invocation: &RawInvocation) -> Result<ModelReply, GatewayError> {
    match kind {
        RuntimeKind::ClaudeCode => interpret_claude_code(invocation),
        RuntimeKind::Codex => interpret_codex(invocation),
    }
}

fn interpret_claude_code(invocation: &RawInvocation) -> Result<ModelReply, GatewayError> {
    let parsed = parse_claude_code_json(&invocation.stdout);

    if invocation.status.success() {
        let reply = parsed.ok_or(GatewayError::MalformedOutput)?;
        if reply.is_error_shaped() {
            return Err(classify_error_text(reply.error_text()));
        }
        return reply
            .into_model_reply()
            .ok_or(GatewayError::MalformedOutput);
    }

    match parsed {
        Some(reply) => Err(classify_error_text(reply.error_text())),
        None => Err(GatewayError::RuntimeCrashed),
    }
}

fn interpret_codex(invocation: &RawInvocation) -> Result<ModelReply, GatewayError> {
    if invocation.status.success() {
        return parse_codex_jsonl(&invocation.stdout).ok_or(GatewayError::MalformedOutput);
    }
    match parsed_codex_error_text(&invocation.stdout) {
        Some(text) => Err(classify_error_text(&text)),
        None => Err(GatewayError::RuntimeCrashed),
    }
}

/// Classifies already-*parsed* error text (never a raw stream — see [`interpret`]'s doc comment)
/// against [`QUOTA_MARKERS`].
fn classify_error_text(text: &str) -> GatewayError {
    if looks_like_quota_exhaustion(text) {
        GatewayError::QuotaExhausted
    } else {
        GatewayError::RuntimeCrashed
    }
}

fn looks_like_quota_exhaustion(text: &str) -> bool {
    let haystack = text.to_lowercase();
    QUOTA_MARKERS.iter().any(|marker| haystack.contains(marker))
}

/// The Claude Code JSON shape (Task 5 fixture, `src/bin/fake_runtime.rs`): one JSON object
/// carrying an optional `subtype`/`is_error` (present on both success and error reports — PR
/// review IMPORTANT 3) and either a `result` (the reply text on success, or the error text on
/// many error reports) or an `error` field (the error text on the shapes that use it instead —
/// PR review IMPORTANT 4 treats either as this shape's "parsed error text").
#[derive(Deserialize)]
struct ClaudeCodeReply {
    #[serde(default)]
    subtype: Option<String>,
    #[serde(default)]
    is_error: bool,
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    usage: Option<ClaudeCodeUsage>,
}

#[derive(Deserialize)]
struct ClaudeCodeUsage {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
}

impl ClaudeCodeReply {
    /// An explicit error report, not a successful reply: `is_error: true`, or a `subtype` present
    /// and not `"success"`. Absent both fields entirely (the shape `parse_claude_code` accepted
    /// before this review), a parsed reply is treated as successful — unchanged behavior for the
    /// happy path.
    fn is_error_shaped(&self) -> bool {
        self.is_error
            || self
                .subtype
                .as_deref()
                .is_some_and(|subtype| subtype != "success")
    }

    /// The text this shape's error report carries, from whichever of `result`/`error` is present.
    /// Never invented: an error-shaped reply with neither field yields an empty string, which
    /// matches no [`QUOTA_MARKERS`] entry and so classifies as [`GatewayError::RuntimeCrashed`].
    fn error_text(&self) -> &str {
        self.result
            .as_deref()
            .or(self.error.as_deref())
            .unwrap_or_default()
    }

    fn into_model_reply(self) -> Option<ModelReply> {
        let text = self.result?;
        let usage = self.usage.map_or(Usage::default(), |usage| Usage {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
        });
        Some(ModelReply { text, usage })
    }
}

fn parse_claude_code_json(stdout: &[u8]) -> Option<ClaudeCodeReply> {
    serde_json::from_slice(stdout).ok()
}

#[derive(Deserialize)]
struct CodexLine {
    msg: CodexMsg,
}

#[derive(Deserialize)]
struct CodexMsg {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    message: Option<String>,
}

/// The Codex happy shape: JSONL, one event per line, keeping only the last `agent_message` (§11.2:
/// Codex's shape never reports usage this milestone knows how to read, so `usage` is always
/// [`Usage::default`] — absent, never invented — for this `RuntimeKind`). Lines that are not valid
/// JSON, or whose `msg.type` is not `"agent_message"`, are skipped rather than failing the whole
/// parse: a real Codex transcript is expected to contain other event kinds interleaved with the
/// answer.
fn parse_codex_jsonl(stdout: &[u8]) -> Option<ModelReply> {
    let text = std::str::from_utf8(stdout).ok()?;
    let mut last_agent_message = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(parsed) = serde_json::from_str::<CodexLine>(trimmed) else {
            continue;
        };
        if parsed.msg.kind == "agent_message"
            && let Some(message) = parsed.msg.message
        {
            last_agent_message = Some(message);
        }
    }
    last_agent_message.map(|text| ModelReply {
        text,
        usage: Usage::default(),
    })
}

/// Scans nonzero-exit Codex JSONL for an `msg.type == "error"` event's `message` field — the
/// Codex analogue of [`ClaudeCodeReply::error_text`], and the only text this module scans for
/// [`QUOTA_MARKERS`] on a Codex failure (PR review IMPORTANT 4: never raw, unparsed
/// stdout/stderr). A stream with no such event, or no valid JSON at all, has no parsed error
/// text, and the caller falls through to [`GatewayError::RuntimeCrashed`].
fn parsed_codex_error_text(stdout: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(stdout).ok()?;
    let mut last_error_message = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(parsed) = serde_json::from_str::<CodexLine>(trimmed) else {
            continue;
        };
        if parsed.msg.kind == "error"
            && let Some(message) = parsed.msg.message
        {
            last_error_message = Some(message);
        }
    }
    last_error_message
}
