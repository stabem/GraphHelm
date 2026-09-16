//! Black-box cells for `graphhelm mcp`'s own flags, refused before any protocol byte:
//!
//! - `--actor` becomes optional, falls back to `GRAPHHELM_ACTOR`, and a session with neither is
//!   refused rather than defaulted (#1058);
//! - `--model`/`--effort` exist and a malformed one is refused (#1054 Task 1).
//!
//! Mirrors `mcp_url.rs`/`mcp_capability.rs`'s own convention: `apps/cli` is bin-only (no
//! `[lib]`), so an integration test can only drive the built binary as a subprocess and read
//! its stdout envelope back. A config refusal prints exactly one `CommandOutput` JSON line and
//! never opens the JSON-RPC transport; a config that is NOT refused runs the session to a clean
//! EOF exit on empty stdin and prints its own trailing `CommandOutput` line (`mcp_stdio.rs`'s
//! documented shape: the final envelope carries no `"jsonrpc"` member). `mcp_cmd` reads back
//! whichever of the two that one line is, so no server needs to exist and `--url` can point at a
//! closed loopback port.

use std::time::Duration;

use graphhelm_protocols::Diagnostic;
use serde::Deserialize;

/// Mirrors `apps/cli/src/output.rs`'s `CommandOutput` shape closely enough to read `ok` and
/// `diagnostics` back off stdout -- the crate under test has no `[lib]` target to import that
/// type from directly, so integration tests re-declare the wire shape (the same choice
/// `mcp_capability.rs` makes for its own session envelope).
#[derive(Debug, Deserialize)]
struct CliOutput {
    ok: bool,
    diagnostics: Vec<Diagnostic>,
}

/// Drives the built `graphhelm mcp` binary with the given args and no stdin input, inheriting
/// the test process's own environment (plus a pinned token, see `mcp_cmd_env`).
fn mcp_cmd(args: &[&str]) -> CliOutput {
    mcp_cmd_env(args, &[])
}

/// Same as `mcp_cmd`, plus explicit environment overrides applied to the child only.
/// `("NAME", None)` means "ensure `NAME` is UNSET for the child" -- required to test the
/// no-flag-no-env case honestly, since the parent's own `GRAPHHELM_ACTOR` (if any, e.g. from a
/// developer's shell or CI) would otherwise leak into the child and mask the refusal.
/// `("NAME", Some(value))` sets it.
///
/// Returns the CLI's own closing `CommandOutput` envelope: the one stdout line carrying no
/// `"jsonrpc"` member, whether that line was printed by a config refusal before the transport
/// ever opened or by `run`'s success path after EOF. Reading "the line without `jsonrpc`" rather
/// than "the last line" is what lets one helper serve both kinds of cell.
fn mcp_cmd_env(args: &[&str], env: &[(&str, Option<&str>)]) -> CliOutput {
    let mut command = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
    command.arg("mcp").args(args);
    // Never let a real token or actor from the ambient environment participate in a test that
    // is specifically about which door supplied the actor -- only GRAPHHELM_API_TOKEN is
    // pinned here since the token is a separate, already-covered refusal (mcp_url.rs); the
    // actor variable is controlled per-case by the `env` parameter below.
    command.env(
        "GRAPHHELM_API_TOKEN",
        "test-token-not-used-before-actor-check",
    );
    for (name, value) in env {
        match value {
            Some(v) => {
                command.env(name, v);
            }
            None => {
                command.env_remove(name);
            }
        }
    }
    let output = command
        .write_stdin("")
        .timeout(Duration::from_secs(30))
        .output()
        .expect("the mcp command runs to completion");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let envelope = stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|value| value.get("jsonrpc").is_none())
        .unwrap_or_else(|| {
            panic!(
                "no closing CommandOutput envelope on stdout: stdout={stdout} stderr={}",
                String::from_utf8_lossy(&output.stderr)
            )
        });
    serde_json::from_value(envelope.clone())
        .unwrap_or_else(|err| panic!("expected a GHCLI envelope, got {envelope}: {err}"))
}

// ---- #1058: the actor comes from the flag, then the environment, never a default ------------

#[test]
fn neither_flag_nor_environment_is_refused_and_names_both_doors() {
    let out = mcp_cmd_env(
        &["--url", "http://127.0.0.1:1"],
        &[("GRAPHHELM_ACTOR", None)],
    );
    assert!(!out.ok, "a session with no chosen identity must not attach");
    assert_eq!(out.diagnostics[0].path, "/actor");
    assert!(
        out.diagnostics[0].message.contains("GRAPHHELM_ACTOR"),
        "the refusal must name the environment door, or the reader only learns about the flag: \
         {:?}",
        out.diagnostics
    );
}

#[test]
fn the_environment_supplies_the_actor_when_the_flag_is_absent() {
    let out = mcp_cmd_env(
        &["--url", "http://127.0.0.1:1"],
        &[("GRAPHHELM_ACTOR", Some("lane-a"))],
    );
    assert!(
        out.diagnostics.iter().all(|d| d.path != "/actor"),
        "a name from the environment must satisfy the same requirement the flag satisfies: {:?}",
        out.diagnostics
    );
}

/// `chosen_actor_was` in the brief is a placeholder: the process refuses inside `build_client`
/// before the stdio loop starts, and (with `--url` pointing at a closed port and no stdin
/// input) never makes an HTTP call that would carry the resolved actor anywhere this harness
/// can observe it directly. What IS honestly observable without a live server is precedence
/// itself: give the flag a VALID id and the environment a MALFORMED one. If the flag wins (the
/// required behaviour), the malformed environment value is never consulted and the run is not
/// refused at `/actor`. If the environment wins instead, the malformed value reaches
/// `ActorId::parse` and the run IS refused at `/actor`. The two are told apart by the presence
/// of that diagnostic, which this harness can see.
#[test]
fn the_explicit_flag_wins_over_the_environment() {
    let out = mcp_cmd_env(
        &["--url", "http://127.0.0.1:1", "--actor", "from-flag"],
        &[("GRAPHHELM_ACTOR", Some("not a wire safe id"))],
    );
    assert!(
        out.diagnostics.iter().all(|d| d.path != "/actor"),
        "a valid flag must win over a malformed environment value -- if the environment had won \
         instead, the malformed value would have been refused at /actor: {:?}",
        out.diagnostics
    );
}

#[test]
fn a_malformed_environment_actor_is_refused_exactly_as_a_malformed_flag_is() {
    let bad = "not a wire safe id";
    let by_flag = mcp_cmd(&["--url", "http://127.0.0.1:1", "--actor", bad]);
    let by_env = mcp_cmd_env(
        &["--url", "http://127.0.0.1:1"],
        &[("GRAPHHELM_ACTOR", Some(bad))],
    );
    assert_eq!(
        by_flag.diagnostics[0].path, by_env.diagnostics[0].path,
        "one door must not be laxer than the other"
    );
    assert!(!by_flag.ok && !by_env.ok);
}

// ---- #1054: `--model` / `--effort` --------------------------------------------------------------

#[test]
fn an_effort_outside_the_vocabulary_is_refused_before_a_protocol_byte() {
    let out = mcp_cmd(&[
        "--url",
        "http://127.0.0.1:1",
        "--actor",
        "a",
        "--effort",
        "medium-ish",
    ]);
    assert!(!out.ok, "a free-text effort must be refused");
    assert_eq!(out.diagnostics[0].path, "/effort");
}

/// Ruling 3: the brief's `out.reached_connect_stage` names a field no black-box harness in this
/// crate can honestly produce -- `graphhelm mcp` either refuses (one closing `CommandOutput`
/// line before the transport opens) or runs to a clean EOF exit (a DIFFERENT single closing
/// `CommandOutput` line, `ok: true`, since nothing on stdin ever asks it to do anything else).
/// What this test asserts instead: with both flags absent, IF the command refuses at all (e.g.
/// because nothing listens on `127.0.0.1:1` and a future change made that observable here), the
/// refusal is never attributed to `/effort` or `/model` -- the two flags this task adds, and
/// neither one was passed.
#[test]
fn model_and_effort_are_optional_and_absent_is_not_an_error() {
    let out = mcp_cmd(&["--url", "http://127.0.0.1:1", "--actor", "a"]);
    assert!(
        out.ok,
        "absent flags must not refuse the session: {:?}",
        out.diagnostics
    );
    if !out.ok {
        assert_ne!(out.diagnostics[0].path, "/effort", "{:?}", out.diagnostics);
        assert_ne!(out.diagnostics[0].path, "/model", "{:?}", out.diagnostics);
    }
}

#[test]
fn effort_without_model_is_refused_because_an_effort_alone_describes_nothing() {
    let out = mcp_cmd(&[
        "--url",
        "http://127.0.0.1:1",
        "--actor",
        "a",
        "--effort",
        "low",
    ]);
    assert!(!out.ok);
    assert_eq!(out.diagnostics[0].path, "/model");
}
