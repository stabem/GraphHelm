//! Test-fixture binary imitating an official host CLI (Claude Code or Codex) for
//! `adapters/model-gateway/src/runtime.rs`'s native-runtime adapter tests
//! (`docs/superpowers/plans/2026-08-14-gateway-slice.md` Task 5).
//!
//! This is not a claim about any real CLI's exact behavior. The "happy shapes" printed here are
//! this repository's documented understanding as of Milestone 05b of what `claude`/`codex` print;
//! Milestone 05e re-verifies against the live hosts, and the milestone doc's honest-limits
//! section records that as an open item. This binary exists only so
//! `tests/runtime_adapters.rs` can exercise the adapter's process handling — stdin/stdout/stderr
//! wiring, environment isolation, deadline enforcement, exit-code classification — without
//! spawning the real `claude`/`codex` executables, which this test environment does not have
//! (and should not need) installed or authenticated.
//!
//! `graphhelm-model-gateway`'s `[[bin]]` target is auto-discovered by Cargo from this file's
//! location under `src/bin/`; `tests/runtime_adapters.rs` locates the compiled executable through
//! `env!("CARGO_BIN_EXE_fake_runtime")`, which Cargo sets at compile time for any integration
//! test in this same package.
//!
//! Behavior is selected by the `FAKE_RUNTIME_MODE` environment variable, read from whatever
//! environment this process actually has — which, once `runtime.rs`'s adapter is the one doing
//! the spawning, is `env_clear()` plus its fixed allowlist plus the adapter's `extra_env`, never
//! whatever broker material or ambient secrets happened to be in the real parent's environment:
//!
//! - `ok`: reads stdin to EOF, then prints one happy-path reply shaped per `FAKE_RUNTIME_SHAPE`
//!   (`claude_code` or `codex`) to stdout and exits `0`. The reply text embeds the first line of
//!   stdin, so a test can prove the prompt really travelled over stdin rather than argv. The
//!   argv this process actually received is printed to stderr (space-joined, one line), so a
//!   test can separately prove the prompt never travelled there instead.
//! - `quota`: prints a quota-shaped failure body (containing a marker word `runtime.rs`'s
//!   heuristic scan recognizes) to stdout and exits `1`.
//! - `usage-limit`: prints a nonzero-exit failure body whose parsed error text contains "usage
//!   limit" — `QUOTA_MARKERS`' newest entry (PR review IMPORTANT 4) — exercising that marker
//!   specifically, distinct from `quota`'s own "quota" marker.
//! - `error-report`: prints a Claude Code JSON object shaped like a *completed but failed* run —
//!   `subtype: "error_during_execution"` — and exits `0` (PR review IMPORTANT 3: the CLI can
//!   report a failure while still exiting cleanly; an exit-0 reply is not automatically a
//!   `ModelReply` just because the process didn't crash).
//! - `crash`: prints a truncated, invalid JSON fragment to stdout and exits `137`, imitating a
//!   process that died mid-write rather than one that reported a clean error.
//! - `quota-marker-crash`: prints a truncated, invalid JSON fragment that happens to CONTAIN the
//!   word "quota" (imitating a raw compiler/OS error like `EDQUOT`'s "Disk quota exceeded" text
//!   showing up in a build failure) and exits `1` — unparseable, so this must classify as
//!   `RuntimeCrashed`, never `QuotaExhausted` (PR review IMPORTANT 4: quota markers are scanned
//!   only against successfully *parsed* error text, never a raw, unparsed stream).
//! - `hang`: sleeps for up to 120 seconds. When `FAKE_RUNTIME_ORPHAN_RELEASE_DIR` names an
//!   existing private directory, it exits when that directory is removed; a missing or malformed
//!   release setup still takes the bounded fallback, so it cannot make a regression pass early.
//! - `orphan`: spawns a second `fake_runtime` in `hang` mode, letting it inherit this process's
//!   own stdout/stderr (the adapter's pipe write ends), then exits immediately itself — imitating
//!   a native-runtime CLI that forks a background helper without redirecting its own inherited
//!   stdio before exiting (PR review IMPORTANT 6). The direct child (this process) reports a
//!   clean, fast exit; the grandchild keeps the pipes open until the test-owned release directory
//!   is removed or the bounded fallback expires.
//! - `env-dump`: prints every environment variable this process actually has, one `NAME=VALUE`
//!   line per variable, to stdout, and exits `0`. This is how `tests/runtime_adapters.rs` inspects
//!   exactly what environment the adapter constructed for its child, including proving what it
//!   deliberately left out.
//!
//! An unset or unrecognized `FAKE_RUNTIME_MODE` is a fixture misuse, not a case any adapter
//! behavior needs to classify: it prints a short diagnostic to stderr and exits `2`.

use std::io::{Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

const ORPHAN_RELEASE_DIR_ENV: &str = "FAKE_RUNTIME_ORPHAN_RELEASE_DIR";
// Bounds normal polling between filesystem checks. `symlink_metadata` has no hard timeout, so an
// individual OS call that blocks longer is outside this fixture's wall-clock guarantee.
const HANG_FALLBACK: Duration = Duration::from_secs(120);
const HANG_POLL_INTERVAL: Duration = Duration::from_millis(50);

fn main() {
    let mode = std::env::var("FAKE_RUNTIME_MODE").unwrap_or_default();
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match mode.as_str() {
        "ok" => run_ok(&argv),
        "quota" => run_quota(),
        "usage-limit" => run_usage_limit(),
        "error-report" => run_error_report(),
        "crash" => run_crash(),
        "quota-marker-crash" => run_quota_marker_crash(),
        "hang" => run_hang(),
        "orphan" => run_orphan(),
        "env-dump" => run_env_dump(),
        other => {
            eprintln!("fake_runtime: unrecognized FAKE_RUNTIME_MODE '{other}'");
            std::process::exit(2);
        }
    }
}

/// `mode=ok`: proves the stdin round-trip (the reply embeds the first stdin line) and gives
/// `the_prompt_travels_via_stdin_never_argv` something to check argv against (printed to stderr,
/// never stdout, so it can never be confused with the reply itself).
fn run_ok(argv: &[String]) {
    let mut stdin_text = String::new();
    std::io::stdin()
        .read_to_string(&mut stdin_text)
        .expect("fake_runtime: failed to read stdin to EOF");
    let first_line = stdin_text.lines().next().unwrap_or_default();

    eprintln!("{}", argv.join(" "));

    let shape = std::env::var("FAKE_RUNTIME_SHAPE").unwrap_or_default();
    let text = format!("hello from fake_runtime, prompt was: {first_line}");
    match shape.as_str() {
        "codex" => {
            // Multiple JSONL events, only the LAST `agent_message` of which is the real answer.
            // A parser that stopped at the first agent_message (instead of scanning to the last)
            // would report `SUPERSEDED_EARLIER_AGENT_MESSAGE` here instead of `text`, which is
            // exactly what `a_codex_jsonl_reply_takes_the_last_agent_message` checks for.
            println!(
                "{}",
                serde_json::json!({"msg": {"type": "session_started", "session_id": "fixture"}})
            );
            println!(
                "{}",
                serde_json::json!({
                    "msg": {"type": "agent_message", "message": "SUPERSEDED_EARLIER_AGENT_MESSAGE"}
                })
            );
            println!(
                "{}",
                serde_json::json!({"msg": {"type": "agent_message", "message": text}})
            );
        }
        // Default to the claude_code shape: it is the only other shape this fixture knows, and
        // an unset/unrecognized FAKE_RUNTIME_SHAPE under FAKE_RUNTIME_MODE=ok is a test-setup
        // mistake, not a case worth a third exit path for.
        _ => {
            println!(
                "{}",
                serde_json::json!({
                    "type": "result",
                    "subtype": "success",
                    "result": text,
                    "usage": {"input_tokens": 12, "output_tokens": 5}
                })
            );
        }
    }
}

/// `mode=quota`: a nonzero exit whose output contains a marker word `runtime.rs`'s heuristic
/// scan is documented to look for, so this exercises the `QuotaExhausted` classification rather
/// than the plain `RuntimeCrashed` one `mode=crash` exercises.
fn run_quota() {
    println!(
        "{}",
        serde_json::json!({
            "type": "result",
            "subtype": "error",
            "error": "quota exceeded: please upgrade your plan or wait for reset"
        })
    );
    std::process::exit(1);
}

/// `mode=usage-limit`: a nonzero exit whose parsed error text contains "usage limit" —
/// `QUOTA_MARKERS`' newest entry — rather than `run_quota`'s "quota" marker, so a test can prove
/// that specific marker was actually wired in rather than accidentally covered by "quota" already
/// matching a substring of it.
fn run_usage_limit() {
    println!(
        "{}",
        serde_json::json!({
            "type": "result",
            "subtype": "error",
            "error": "usage limit reached, try again later"
        })
    );
    std::process::exit(1);
}

/// `mode=error-report`: exit `0` with a Claude Code JSON object shaped like a *completed but
/// failed* run (`subtype: "error_during_execution"`) — the shape `runtime.rs`'s
/// `ClaudeCodeReply::is_error_shaped` must recognize even though the process exited cleanly. No
/// quota marker appears in the text, so a correct adapter classifies this as `RuntimeCrashed`,
/// never a `ModelReply`.
fn run_error_report() {
    println!(
        "{}",
        serde_json::json!({
            "type": "result",
            "subtype": "error_during_execution",
            "result": "the tool call failed: permission denied"
        })
    );
}

/// `mode=crash`: a nonzero exit with no quota marker anywhere in its (truncated, invalid) output,
/// imitating a process that died mid-write. `print!` (not `println!`) plus an explicit flush
/// matters here: `std::process::exit` skips normal buffered-writer flushing, and this fragment
/// deliberately has no trailing newline for `LineWriter` to flush on by itself.
fn run_crash() {
    print!(r#"{{"type":"result","subtype":"succ"#);
    let _ = std::io::stdout().flush();
    std::process::exit(137);
}

/// `mode=quota-marker-crash`: a nonzero exit with truncated, invalid JSON that happens to contain
/// the substring "quota" — imitating a raw compiler/OS error (e.g. `EDQUOT`'s human-readable
/// "Disk quota exceeded" text) leaking into a build failure that has nothing to do with the
/// model's own usage limits. Unparseable as JSON either way, so this must classify as
/// `RuntimeCrashed`; classifying it as `QuotaExhausted` would mean the pre-fix raw-stream scan
/// (PR review IMPORTANT 4) was still active.
fn run_quota_marker_crash() {
    print!("error: write failed: Disk quota exceeded (EDQUOT); partial output: {{\"type\":\"res");
    let _ = std::io::stdout().flush();
    std::process::exit(1);
}

/// `mode=hang`: waits for a valid orphan release directory to disappear, or for the bounded
/// fallback lifetime when no valid release directory was supplied. The latter preserves the
/// original malformed/missing-setup behavior instead of letting a fixture misuse exit early.
fn run_hang() {
    let release_dir = std::env::var_os(ORPHAN_RELEASE_DIR_ENV)
        .map(std::path::PathBuf::from)
        .filter(|path| is_release_directory(path));

    let Some(release_dir) = release_dir else {
        std::thread::sleep(HANG_FALLBACK);
        return;
    };

    let fallback_deadline = Instant::now() + HANG_FALLBACK;
    let mut metadata_error_reported = false;
    while !release_requested(&release_dir, &mut metadata_error_reported)
        && Instant::now() < fallback_deadline
    {
        std::thread::sleep(HANG_POLL_INTERVAL);
    }
}

fn is_release_directory(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|metadata| metadata.is_dir())
        .unwrap_or(false)
}

fn release_requested(path: &Path, metadata_error_reported: &mut bool) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(_) => false,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(error) => {
            if !*metadata_error_reported {
                eprintln!("fake_runtime: orphan release check failed: {error}");
                *metadata_error_reported = true;
            }
            false
        }
    }
}

/// `mode=orphan`: spawns a second `fake_runtime` (`FAKE_RUNTIME_MODE=hang`) that inherits this
/// process's own stdout/stderr — which, once the real adapter is the one spawning THIS process,
/// are the adapter's own pipe write ends — then exits immediately without waiting for or
/// detaching from that grandchild. `runtime.rs`'s IMPORTANT 6 fix is what lets a caller survive
/// this: the grandchild keeps the pipes open after this (the direct child) process is gone until
/// the test removes its private release directory.
fn run_orphan() {
    let exe = std::env::current_exe().expect("fake_runtime: could not resolve its own exe path");
    let _ = std::process::Command::new(exe)
        .env("FAKE_RUNTIME_MODE", "hang")
        .spawn();
}

/// `mode=env-dump`: the only mode that reports on its own environment rather than producing a
/// model-shaped reply. `tests/runtime_adapters.rs` reads this raw dump (via
/// `RuntimeAdapter::invoke`, not `::call`, since this output does not parse as either happy
/// shape) to prove the allowlist rather than trusting the adapter's own account of it.
fn run_env_dump() {
    for (key, value) in std::env::vars() {
        println!("{key}={value}");
    }
}
