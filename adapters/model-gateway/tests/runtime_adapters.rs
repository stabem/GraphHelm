//! Fake-runtime subprocess contract tests for the native-runtime adapter (`src/runtime.rs`).
//!
//! Mirrors `tests/byok_adapters.rs`'s spirit (a from-scratch fixture the adapter is exercised
//! against) but over a real subprocess instead of a fake HTTP server: every test here spawns
//! `src/bin/fake_runtime.rs`, compiled by Cargo alongside this test binary and located through
//! `env!("CARGO_BIN_EXE_fake_runtime")` (gateway-slice plan, Task 5).

use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use graphhelm_gateway::call::ModelCall;
use graphhelm_gateway::manifest::RouteManifest;
use graphhelm_gateway::taxonomy::{GatewayError, outcome_for_error};
use graphhelm_model_gateway::runtime::RuntimeAdapter;
use graphhelm_protocols::NodeOutcome;

const ORPHAN_RELEASE_DIR_ENV: &str = "FAKE_RUNTIME_ORPHAN_RELEASE_DIR";

struct OrphanFixtureGuard {
    release_dir: Option<tempfile::TempDir>,
}

impl OrphanFixtureGuard {
    fn new() -> Self {
        Self {
            release_dir: Some(tempfile::tempdir().expect("create orphan release directory")),
        }
    }

    fn env(&self) -> (String, String) {
        let path = self
            .release_dir
            .as_ref()
            .expect("orphan release directory is still owned")
            .path()
            .to_str()
            .expect("orphan release directory path must be valid UTF-8")
            .to_owned();
        (ORPHAN_RELEASE_DIR_ENV.to_owned(), path)
    }

    fn release(&mut self) {
        let path = self
            .release_dir
            .as_ref()
            .expect("orphan release directory is still owned")
            .path()
            .to_owned();
        match fs::remove_dir(&path) {
            Ok(()) => {
                self.release_dir = None;
            }
            Err(error) => panic!(
                "failed to release orphan fixture directory {}: {error}",
                path.display()
            ),
        }
    }
}

impl Drop for OrphanFixtureGuard {
    fn drop(&mut self) {
        let Some(tempdir) = self.release_dir.take() else {
            return;
        };
        let path: PathBuf = tempdir.path().to_owned();
        if let Err(error) = fs::remove_dir(&path)
            && error.kind() != io::ErrorKind::NotFound
        {
            eprintln!(
                "failed to release orphan fixture directory during drop {}: {error}",
                path.display()
            );
        }
    }
}

/// Path to the compiled `fake_runtime` fixture — set by Cargo at compile time for any
/// integration test in this package (`graphhelm-model-gateway` has both a `[lib]` and this
/// `[[bin]]` target).
fn fake_runtime_path() -> &'static str {
    env!("CARGO_BIN_EXE_fake_runtime")
}

fn call(prompt: &str) -> ModelCall {
    ModelCall {
        prompt: prompt.to_owned(),
        max_tokens: 64,
    }
}

/// A validated single-route `native_runtime` manifest pointed at `fake_runtime`. `id`/`profiles`
/// are fixed, arbitrary values — nothing in this file cares about them beyond satisfying
/// `native_runtime`'s structural requirements (`core/gateway/src/manifest.rs`). `timeout_seconds`
/// defaults unless `timeout_seconds` is `Some`.
fn native_manifest(runtime: &str, timeout_seconds: Option<u64>) -> RouteManifest {
    let provider = if runtime == "codex" {
        "openai"
    } else {
        "anthropic"
    };
    let mut route = serde_json::json!({
        "id": "test_native_route",
        "provider": provider,
        "transport": "native_runtime",
        "runtime": runtime,
        "authentication": "account_subscription",
        "billingMode": "subscription_quota",
        "command": { "program": fake_runtime_path(), "args": ["--fixture-arg"] },
        "profiles": ["software_execution"],
        "enabled": true
    });
    if let Some(seconds) = timeout_seconds {
        route["timeoutSeconds"] = seconds.into();
    }
    let json = serde_json::json!({
        "manifestVersion": 1,
        "routes": [route]
    });
    RouteManifest::from_json(&json.to_string())
        .unwrap_or_else(|error| panic!("fixture manifest must be valid: {error}"))
}

fn ok_env(shape: &str) -> Vec<(String, String)> {
    vec![
        ("FAKE_RUNTIME_MODE".to_owned(), "ok".to_owned()),
        ("FAKE_RUNTIME_SHAPE".to_owned(), shape.to_owned()),
    ]
}

fn mode_env(mode: &str) -> Vec<(String, String)> {
    vec![("FAKE_RUNTIME_MODE".to_owned(), mode.to_owned())]
}

/// A validated single-route `direct_api` manifest — used only by
/// [`a_direct_api_route_is_rejected_not_panicked_on`], which needs a structurally valid route of
/// the *wrong* transport kind to hand to [`RuntimeAdapter`] (MEDIUM 12 — the mirror image of
/// `tests/byok_adapters.rs`'s `a_native_runtime_route_is_rejected_not_panicked_on`).
fn build_direct_api_manifest() -> RouteManifest {
    let json = serde_json::json!({
        "manifestVersion": 1,
        "routes": [{
            "id": "test_direct_route",
            "provider": "anthropic",
            "transport": "direct_api",
            "authentication": "api_key",
            "billingMode": "per_token",
            "baseUrl": "https://api.anthropic.com",
            "model": "test-model",
            "credentialRef": "secret_test",
            "profiles": ["balanced_reasoning"],
            "enabled": true
        }]
    });
    RouteManifest::from_json(&json.to_string())
        .unwrap_or_else(|error| panic!("fixture manifest must be valid: {error}"))
}

#[test]
fn a_claude_shaped_reply_parses_text_and_usage() {
    let manifest = native_manifest("claude_code", None);
    let route = &manifest.routes()[0];
    let adapter = RuntimeAdapter::new(route, ok_env("claude_code"));

    let reply = adapter
        .call(&call("distinctive-claude-prompt"))
        .unwrap_or_else(|error| panic!("expected success: {error}"));

    assert!(
        reply.text.contains("distinctive-claude-prompt"),
        "reply must embed the first stdin line: {}",
        reply.text
    );
    assert_eq!(reply.usage.input_tokens, Some(12));
    assert_eq!(reply.usage.output_tokens, Some(5));
}

#[test]
fn a_codex_jsonl_reply_takes_the_last_agent_message() {
    let manifest = native_manifest("codex", None);
    let route = &manifest.routes()[0];
    let adapter = RuntimeAdapter::new(route, ok_env("codex"));

    let reply = adapter
        .call(&call("distinctive-codex-prompt"))
        .unwrap_or_else(|error| panic!("expected success: {error}"));

    assert!(
        reply.text.contains("distinctive-codex-prompt"),
        "reply must take the LAST agent_message, which embeds the prompt: {}",
        reply.text
    );
    assert!(
        !reply.text.contains("SUPERSEDED_EARLIER_AGENT_MESSAGE"),
        "reply must not take the earlier, superseded agent_message: {}",
        reply.text
    );
    // Codex's shape never reports usage this milestone knows how to read (§11.2: never invent).
    assert_eq!(reply.usage.input_tokens, None);
    assert_eq!(reply.usage.output_tokens, None);
}

#[test]
fn a_legacy_codex_reply_ignores_unrelated_typed_noise() {
    let manifest = native_manifest("codex", None);
    let mut env = ok_env("codex");
    env[0].1 = "legacy-codex-typed-noise".to_owned();
    let adapter = RuntimeAdapter::new(&manifest.routes()[0], env);
    let reply = adapter
        .call(&call("typed-noise-prompt"))
        .expect("legacy reply");
    assert!(reply.text.contains("typed-noise-prompt"));
    assert!(!reply.text.contains("SUPERSEDED_EARLIER_AGENT_MESSAGE"));
    assert_eq!(reply.usage.input_tokens, None);
    assert_eq!(reply.usage.output_tokens, None);
}

#[test]
fn a_current_codex_jsonl_reply_requires_terminal_completion_and_reports_usage() {
    let manifest = native_manifest("codex", None);
    let route = &manifest.routes()[0];
    let adapter = RuntimeAdapter::new(route, mode_env("current-codex-ok"));

    let reply = adapter
        .call(&call("distinctive-current-codex-prompt"))
        .unwrap_or_else(|error| panic!("expected success: {error}"));

    assert!(
        reply.text.contains("distinctive-current-codex-prompt"),
        "reply must use the completed current-format agent message: {}",
        reply.text
    );
    assert_eq!(reply.usage.input_tokens, Some(12));
    assert_eq!(reply.usage.output_tokens, Some(3));
}

#[test]
fn a_current_codex_retry_recovers_only_with_terminal_completion_and_zero_exit() {
    let manifest = native_manifest("codex", None);
    let route = &manifest.routes()[0];
    let adapter = RuntimeAdapter::new(route, mode_env("current-codex-retry-ok"));
    let reply = adapter
        .call(&call("retry recovery prompt"))
        .expect("the completed retry must supply the reply");
    assert!(reply.text.contains("retry recovery prompt"));
    assert_eq!(reply.usage.input_tokens, Some(12));
    assert_eq!(reply.usage.output_tokens, Some(3));

    let adapter = RuntimeAdapter::new(route, mode_env("current-codex-retry-nonzero"));
    assert_eq!(
        adapter.call(&call("retry recovery prompt")).unwrap_err(),
        GatewayError::RuntimeCrashed
    );
}

#[test]
fn a_current_codex_top_level_quota_error_is_classified_on_nonzero_exit() {
    let manifest = native_manifest("codex", None);
    let route = &manifest.routes()[0];
    let adapter = RuntimeAdapter::new(route, mode_env("current-codex-quota"));

    let error = adapter.call(&call("hi")).unwrap_err();
    assert_eq!(error, GatewayError::QuotaExhausted);
    assert_eq!(outcome_for_error(error), NodeOutcome::NeedsCapacity);
}

#[test]
fn a_current_codex_auth_failure_is_runtime_crashed() {
    let manifest = native_manifest("codex", None);
    let route = &manifest.routes()[0];
    let adapter = RuntimeAdapter::new(route, mode_env("current-codex-auth"));

    let error = adapter.call(&call("hi")).unwrap_err();
    assert_eq!(error, GatewayError::RuntimeCrashed);
}

#[test]
fn corrupt_output_after_a_failed_turn_does_not_pause_subscription_capacity() {
    let manifest = native_manifest("codex", None);
    let adapter = RuntimeAdapter::new(
        &manifest.routes()[0],
        mode_env("current-codex-quota-then-corruption"),
    );
    let error = adapter.call(&call("hi")).unwrap_err();
    assert_eq!(error, GatewayError::RuntimeCrashed);
    assert_ne!(outcome_for_error(error), NodeOutcome::NeedsCapacity);
}

#[test]
fn invalid_item_lifecycle_payload_does_not_pause_subscription_capacity() {
    let manifest = native_manifest("codex", None);
    let adapter = RuntimeAdapter::new(
        &manifest.routes()[0],
        mode_env("current-codex-invalid-lifecycle"),
    );
    let error = adapter.call(&call("hi")).unwrap_err();
    assert_eq!(error, GatewayError::RuntimeCrashed);
    assert_ne!(outcome_for_error(error), NodeOutcome::NeedsCapacity);
}

#[test]
fn a_current_codex_failed_turn_is_failure_even_on_zero_exit() {
    let manifest = native_manifest("codex", None);
    let route = &manifest.routes()[0];
    let adapter = RuntimeAdapter::new(route, mode_env("current-codex-failed-zero"));

    let error = adapter.call(&call("hi")).unwrap_err();
    assert_eq!(error, GatewayError::RuntimeCrashed);
}

#[test]
fn legacy_codex_nonzero_exit_classifies_the_last_error_when_it_is_quota() {
    let manifest = native_manifest("codex", None);
    let route = &manifest.routes()[0];
    let adapter = RuntimeAdapter::new(route, mode_env("legacy-codex-errors-quota-last"));

    let error = adapter.call(&call("hi")).unwrap_err();
    assert_eq!(error, GatewayError::QuotaExhausted);
}

#[test]
fn legacy_codex_nonzero_exit_classifies_the_last_error_when_it_is_not_quota() {
    let manifest = native_manifest("codex", None);
    let route = &manifest.routes()[0];
    let adapter = RuntimeAdapter::new(route, mode_env("legacy-codex-errors-nonquota-last"));

    let error = adapter.call(&call("hi")).unwrap_err();
    assert_eq!(error, GatewayError::RuntimeCrashed);
}

#[test]
fn legacy_codex_zero_exit_ignores_an_error_before_a_valid_message() {
    let manifest = native_manifest("codex", None);
    let route = &manifest.routes()[0];
    let adapter = RuntimeAdapter::new(route, mode_env("legacy-codex-error-then-valid"));

    let reply = adapter
        .call(&call("hi"))
        .unwrap_or_else(|error| panic!("expected legacy success: {error}"));
    assert_eq!(reply.text, "legacy recovered");
}

#[test]
fn a_truncated_codex_stream_cannot_hide_a_failure_after_completion() {
    let manifest = native_manifest("codex", None);
    let adapter = RuntimeAdapter::new(
        &manifest.routes()[0],
        mode_env("current-codex-hidden-failure"),
    );
    assert_eq!(
        adapter.call(&call("hi")).unwrap_err(),
        GatewayError::MalformedOutput
    );
    let adapter = RuntimeAdapter::new(
        &manifest.routes()[0],
        mode_env("current-codex-hidden-failure-nonzero"),
    );
    assert_eq!(
        adapter.call(&call("hi")).unwrap_err(),
        GatewayError::RuntimeCrashed
    );
}

#[test]
fn a_complete_codex_stream_at_the_capture_limit_is_accepted() {
    let manifest = native_manifest("codex", None);
    let adapter = RuntimeAdapter::new(&manifest.routes()[0], mode_env("current-codex-exact-cap"));
    assert_eq!(
        adapter
            .call(&call("hi"))
            .expect("complete bounded stream")
            .text,
        "bounded reply"
    );
}

#[test]
fn runtime_output_capture_is_bounded_while_the_child_is_fully_drained() {
    let manifest = native_manifest("codex", None);
    let route = &manifest.routes()[0];
    let adapter = RuntimeAdapter::new(route, mode_env("oversized-output"));

    let invocation = adapter
        .invoke("hi")
        .unwrap_or_else(|error| panic!("expected raw invocation: {error}"));
    assert!(invocation.status.success());
    assert_eq!(invocation.stdout.len(), 16 * 1024 * 1024);
    assert!(!invocation.stdout_complete);
}

#[test]
fn quota_exhaustion_maps_to_needs_capacity() {
    let manifest = native_manifest("claude_code", None);
    let route = &manifest.routes()[0];
    let adapter = RuntimeAdapter::new(
        route,
        vec![("FAKE_RUNTIME_MODE".to_owned(), "quota".to_owned())],
    );

    let error = adapter.call(&call("hi")).unwrap_err();
    assert_eq!(error, GatewayError::QuotaExhausted);
    assert_eq!(outcome_for_error(error), NodeOutcome::NeedsCapacity);
}

#[test]
fn a_crashed_runtime_is_runtime_crashed_not_malformed() {
    let manifest = native_manifest("claude_code", None);
    let route = &manifest.routes()[0];
    let adapter = RuntimeAdapter::new(
        route,
        vec![("FAKE_RUNTIME_MODE".to_owned(), "crash".to_owned())],
    );

    let error = adapter.call(&call("hi")).unwrap_err();
    assert_eq!(error, GatewayError::RuntimeCrashed);
}

#[test]
fn a_hung_runtime_is_killed_within_the_deadline() {
    // A short, test-local deadline — the fixture's bounded fallback is 120s, so only the
    // adapter's own kill-at-deadline logic can make this test finish quickly.
    let manifest = native_manifest("claude_code", Some(2));
    let route = &manifest.routes()[0];
    let adapter = RuntimeAdapter::new(
        route,
        vec![("FAKE_RUNTIME_MODE".to_owned(), "hang".to_owned())],
    );

    let started = Instant::now();
    let error = adapter.call(&call("hi")).unwrap_err();
    let elapsed = started.elapsed();

    assert_eq!(error, GatewayError::Timeout);
    // Well under the fixture's 120s fallback, and comfortably under the suite's own patience — if
    // `kill()` had not actually terminated the child, `Child::wait()` inside `invoke` would have
    // blocked until the fixture exited on its own, which is the real proof "the child is
    // actually dead": this call could not have returned quickly otherwise.
    assert!(
        elapsed < Duration::from_secs(30),
        "expected the hung child to be killed well within 30s, took {elapsed:?}"
    );
}

/// Regression test for the stdin-write deadlock: writing the prompt on the calling thread,
/// before the deadline loop even starts, blocks that write call until the child drains its
/// stdin — against `FAKE_RUNTIME_MODE=hang` (which never reads stdin at all) a prompt bigger
/// than the OS pipe buffer fills it and hangs the write forever, since the deadline logic that
/// would otherwise kill the child never gets a chance to run. 1 MiB is comfortably over any OS
/// pipe buffer (typically 64 KiB or less on both Windows and Unix). Observed directly against
/// the pre-fix code: this test hung indefinitely (confirmed with a wrapped harness timeout,
/// since the fixture's own 120s fallback and the absence of any deadline-loop entry means nothing
/// in-process would ever kill it) — see the fix's commit message for the exact command used.
#[test]
fn a_prompt_larger_than_the_pipe_buffer_still_hits_the_deadline_against_a_hung_child() {
    let huge_prompt = "x".repeat(1024 * 1024); // 1 MiB.
    let manifest = native_manifest("claude_code", Some(2));
    let route = &manifest.routes()[0];
    let adapter = RuntimeAdapter::new(
        route,
        vec![("FAKE_RUNTIME_MODE".to_owned(), "hang".to_owned())],
    );

    let started = Instant::now();
    let error = adapter.call(&call(&huge_prompt)).unwrap_err();
    let elapsed = started.elapsed();

    assert_eq!(error, GatewayError::Timeout);
    assert!(
        elapsed < Duration::from_secs(30),
        "expected the hung child to be killed well within 30s even with an oversized prompt, \
         took {elapsed:?} — a stdin write blocking ahead of the deadline loop would hang this \
         test instead of returning at all"
    );
}

#[test]
fn the_prompt_travels_via_stdin_never_argv() {
    let manifest = native_manifest("claude_code", None);
    let route = &manifest.routes()[0];
    let adapter = RuntimeAdapter::new(route, ok_env("claude_code"));

    let distinctive_prompt = "xyzzy-stdin-only-marker-8f3c1a9e";
    let invocation = adapter
        .invoke(distinctive_prompt)
        .unwrap_or_else(|error| panic!("expected success: {error}"));

    assert!(invocation.status.success());
    let stdout_text = String::from_utf8_lossy(&invocation.stdout);
    assert!(
        stdout_text.contains(distinctive_prompt),
        "reply must round-trip the prompt via stdin: {stdout_text}"
    );
    let stderr_text = String::from_utf8_lossy(&invocation.stderr);
    assert!(
        stderr_text.contains("--fixture-arg"),
        "the configured argv must reach the runtime: {stderr_text}"
    );
    assert!(
        !stderr_text.contains(distinctive_prompt),
        "argv (echoed on stderr by the fixture) must never carry the prompt: {stderr_text}"
    );
}

// -------------------------------------------------------------------------------------------
// Transport guard (MEDIUM 12): the mirror image of `byok_adapters.rs`'s own transport guard.
// -------------------------------------------------------------------------------------------

#[test]
fn a_direct_api_route_is_rejected_not_panicked_on() {
    let manifest = build_direct_api_manifest();
    let route = &manifest.routes()[0];
    let adapter = RuntimeAdapter::new(route, Vec::new());

    let error = adapter.call(&call("hi")).unwrap_err();
    assert_eq!(error, GatewayError::UnsupportedCapability);

    // `invoke` carries the same guard independently (it is `pub`, and reaches
    // `route.command()`'s `.expect()` on its own if unguarded).
    let error = adapter.invoke("hi").unwrap_err();
    assert_eq!(error, GatewayError::UnsupportedCapability);
}

// -------------------------------------------------------------------------------------------
// Exit-0 error reports and parsed-vs-raw quota-marker scanning (IMPORTANT 3 & 4).
// -------------------------------------------------------------------------------------------

/// IMPORTANT 3: `fake_runtime`'s `error-report` mode exits `0` but its JSON reports
/// `subtype: "error_during_execution"` — a completed-but-failed run, not a success. Pre-fix,
/// `parse_claude_code` read only `result`/`usage` and never looked at `subtype`/`is_error` at
/// all, so this exit-0 body would have parsed as an ordinary successful `ModelReply` carrying the
/// error text as if it were the model's own reply.
#[test]
fn an_exit_0_error_report_is_not_a_model_reply() {
    let manifest = native_manifest("claude_code", None);
    let route = &manifest.routes()[0];
    let adapter = RuntimeAdapter::new(
        route,
        vec![("FAKE_RUNTIME_MODE".to_owned(), "error-report".to_owned())],
    );

    let error = adapter.call(&call("hi")).unwrap_err();
    // No quota marker in the fixture's error text ("permission denied") — an explicit error
    // report with no recognizable quota shape is a crash, not a parked-capacity signal.
    assert_eq!(error, GatewayError::RuntimeCrashed);
}

/// IMPORTANT 4 (i): a genuinely SUCCESSFUL reply whose TEXT happens to contain "rate limit" (the
/// prompt is echoed back into the reply by `fake_runtime`'s `ok` mode) must still be an ordinary
/// `ModelReply` — never scanned for quota markers at all, since it never reached the error path.
/// Red pre-fix in spirit: the pre-fix code's raw-stream scan applied to `invocation.stdout`
/// regardless of exit status, which — had the marker scan run before the exit-0 success check, or
/// on any code path that inspected raw stdout on success — would have misclassified this as
/// `QuotaExhausted` instead of returning a reply at all.
#[test]
fn a_successful_reply_whose_text_contains_a_quota_marker_is_still_a_model_reply() {
    let manifest = native_manifest("claude_code", None);
    let route = &manifest.routes()[0];
    let adapter = RuntimeAdapter::new(route, ok_env("claude_code"));

    let reply = adapter
        .call(&call("please rate limit this politely"))
        .unwrap_or_else(|error| panic!("expected success, got {error}"));
    assert!(
        reply.text.contains("rate limit"),
        "the fixture must echo the prompt (containing the marker text) into the reply: {}",
        reply.text
    );
}

/// IMPORTANT 4 (ii): a nonzero exit whose *parsed* error text contains "usage limit" — the newest
/// `QUOTA_MARKERS` entry — maps to `QuotaExhausted`.
#[test]
fn parsed_usage_limit_error_text_maps_to_quota_exhausted() {
    let manifest = native_manifest("claude_code", None);
    let route = &manifest.routes()[0];
    let adapter = RuntimeAdapter::new(
        route,
        vec![("FAKE_RUNTIME_MODE".to_owned(), "usage-limit".to_owned())],
    );

    let error = adapter.call(&call("hi")).unwrap_err();
    assert_eq!(error, GatewayError::QuotaExhausted);
}

/// IMPORTANT 4 (iii): a nonzero exit with unparseable garbage that happens to CONTAIN "quota"
/// (imitating `EDQUOT`'s "Disk quota exceeded" text leaking from an unrelated compiler/OS error)
/// must classify as `RuntimeCrashed`, never `QuotaExhausted` — quota markers are scanned only
/// against successfully parsed error text, never a raw, unparsed stream. Red pre-fix: the
/// original `looks_like_quota_exhaustion` scanned raw `stdout`/`stderr` directly regardless of
/// whether either parsed as JSON, so this exact fixture would have matched "quota" and parked the
/// route's capacity for a failure that has nothing to do with the model provider's own limits.
#[test]
fn unparseable_output_mentioning_quota_is_runtime_crashed_not_quota_exhausted() {
    let manifest = native_manifest("claude_code", None);
    let route = &manifest.routes()[0];
    let adapter = RuntimeAdapter::new(
        route,
        vec![(
            "FAKE_RUNTIME_MODE".to_owned(),
            "quota-marker-crash".to_owned(),
        )],
    );

    let error = adapter.call(&call("hi")).unwrap_err();
    assert_eq!(error, GatewayError::RuntimeCrashed);
}

// -------------------------------------------------------------------------------------------
// Reader-join wedge past the deadline (IMPORTANT 6).
// -------------------------------------------------------------------------------------------

/// The direct child (`fake_runtime`, `orphan` mode) exits almost immediately with status `0` and
/// empty output, but its grandchild (`FAKE_RUNTIME_MODE=hang`) inherits the adapter's own
/// stdout/stderr pipe write ends and stays alive until the test-owned release directory is
/// removed. Pre-fix, `invoke`'s `Exited` branch unconditionally `.join()`ed the reader threads
/// once `try_wait` reported the direct child gone — with no deadline of its own on that join — so
/// this call would have wedged until the fixture's bounded fallback released the pipes.
///
/// Post-fix, the poll loop only reaches its `Exited` branch once BOTH the child has exited AND
/// both readers have observed EOF/an error; since the readers never see EOF here (the grandchild
/// keeps the pipes open), the loop's own deadline is what fires instead, and the classification
/// is `GatewayError::Timeout` — the same outcome a genuinely hung child produces, since from this
/// adapter's perspective the two are indistinguishable (something is still holding the pipes open
/// past the deadline).
#[test]
fn an_orphaned_grandchild_holding_the_pipe_does_not_wedge_past_the_deadline() {
    let mut fixture = OrphanFixtureGuard::new();
    let manifest = native_manifest("claude_code", Some(2));
    let route = &manifest.routes()[0];
    let adapter = RuntimeAdapter::new(
        route,
        vec![
            ("FAKE_RUNTIME_MODE".to_owned(), "orphan".to_owned()),
            fixture.env(),
        ],
    );

    let started = Instant::now();
    let error = adapter.call(&call("hi")).unwrap_err();
    let elapsed = started.elapsed();

    assert_eq!(
        error,
        GatewayError::Timeout,
        "an orphaned grandchild holding the pipe open must classify as Timeout, not wedge \
         forever or silently succeed"
    );
    assert!(
        elapsed < Duration::from_secs(15),
        "expected the adapter to return once its own 2s deadline elapsed, not wait on the \
         orphaned grandchild; took {elapsed:?}"
    );

    // Keep the grandchild and its inherited pipes alive until both original assertions above
    // have run. Drop also attempts this release during a panic; a normal-path failure panics so
    // cleanup cannot silently certify the fixture.
    fixture.release();
}

// -------------------------------------------------------------------------------------------
// Environment isolation.
// -------------------------------------------------------------------------------------------
//
// Setting real process environment variables from inside a test is racy: `std::env::set_var`
// mutates the whole process's environment table, and `cargo test` runs every test in this binary
// on its own thread by default, including several tests above that concurrently read the real
// environment (`RuntimeAdapter::invoke`'s allowlist copy). To give the two sentinel names a real
// ambient environment to be filtered out of — rather than merely asserting they were never
// present in the first place, which would hold trivially and prove nothing — this test re-execs
// this same compiled test binary as a child process, setting the sentinels only on that child's
// `Command` (which is not racy: it edits the child's environment snapshot, never this process's
// real one), and has the child run the adapter's real spawn path and report what the grandchild
// (`fake_runtime`, mode `env-dump`) actually received, over a temp file rather than parsed stdout
// so libtest's own harness output around it needs no special handling.
const REEXEC_OUTPUT_PATH_ENV: &str = "GRAPHHELM_RUNTIME_TEST_ENV_DUMP_OUTPUT_PATH";
const GATEWAY_KEY_SENTINEL_NAME: &str = "GRAPHHELM_GATEWAY_KEY";
const OTHER_SECRET_SENTINEL_NAME: &str = "A_SENTINEL_SECRET";
const GATEWAY_KEY_SENTINEL_VALUE: &str = "sk-broker-material-must-never-leak";
const OTHER_SECRET_SENTINEL_VALUE: &str = "sentinel-value-must-never-leak";

#[test]
fn the_child_environment_is_an_allowlist_and_never_carries_broker_material() {
    if let Ok(output_path) = std::env::var(REEXEC_OUTPUT_PATH_ENV) {
        // Re-exec child branch: this process's own real environment now genuinely carries the
        // two sentinels (set below, via `Command::env`, by the top-level branch) — proving the
        // adapter's `env_clear()` + allowlist filters them out of the grandchild is the entire
        // point of running this branch in a separate process rather than in-line.
        let manifest = native_manifest("claude_code", None);
        let route = &manifest.routes()[0];
        let adapter = RuntimeAdapter::new(
            route,
            vec![("FAKE_RUNTIME_MODE".to_owned(), "env-dump".to_owned())],
        );
        let invocation = adapter
            .invoke("irrelevant for env-dump")
            .expect("fake_runtime env-dump must spawn and exit 0");
        std::fs::write(&output_path, &invocation.stdout)
            .expect("write the captured dump for the parent test process to read");
        return;
    }

    let dump_file = tempfile::NamedTempFile::new().expect("create a scratch file for the dump");
    let test_binary = std::env::current_exe().expect("current test binary path");
    let status = Command::new(&test_binary)
        .arg("the_child_environment_is_an_allowlist_and_never_carries_broker_material")
        .arg("--exact")
        .env(REEXEC_OUTPUT_PATH_ENV, dump_file.path())
        .env(GATEWAY_KEY_SENTINEL_NAME, GATEWAY_KEY_SENTINEL_VALUE)
        .env(OTHER_SECRET_SENTINEL_NAME, OTHER_SECRET_SENTINEL_VALUE)
        .status()
        .expect("re-exec this test binary for the isolated child run");
    assert!(
        status.success(),
        "the re-exec child test did not run cleanly"
    );

    let dump = std::fs::read_to_string(dump_file.path()).expect("read the captured dump");
    assert!(
        dump.contains("PATH="),
        "allowlisted PATH is missing from the child's environment: {dump}"
    );
    for leaked_name in [GATEWAY_KEY_SENTINEL_NAME, OTHER_SECRET_SENTINEL_NAME] {
        assert!(
            !dump.contains(leaked_name),
            "'{leaked_name}' leaked into the child's environment: {dump}"
        );
    }
    for leaked_value in [GATEWAY_KEY_SENTINEL_VALUE, OTHER_SECRET_SENTINEL_VALUE] {
        assert!(
            !dump.contains(leaked_value),
            "a sentinel value leaked into the child's environment: {dump}"
        );
    }
}
