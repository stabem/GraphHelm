//! Black-box: `--actor` becomes optional, falls back to `GRAPHHELM_ACTOR`, and a session with
//! neither is refused rather than defaulted (#1058).
//!
//! Mirrors `mcp_url.rs`/`mcp_capability.rs`'s own convention: `apps/cli` is bin-only (no
//! `[lib]`), so an integration test can only drive the built binary as a subprocess and read
//! its stdout envelope back. Every case here is refused before any protocol byte (`build_client`
//! runs before the stdio loop starts), so no server needs to exist and `--url` can point at a
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

/// Drives the built `graphhelm mcp` binary with the given args and no stdin input (an empty
/// stdin is enough for every case here: each one is refused inside `build_client`, before the
/// stdio loop ever reads a line), inheriting the test process's own environment.
fn mcp_cmd(args: &[&str]) -> CliOutput {
    mcp_cmd_env(args, &[])
}

/// Same as `mcp_cmd`, plus explicit environment overrides applied to the child only.
/// `("NAME", None)` means "ensure `NAME` is UNSET for the child" -- required to test the
/// no-flag-no-env case honestly, since the parent's own `GRAPHHELM_ACTOR` (if any, e.g. from a
/// developer's shell or CI) would otherwise leak into the child and mask the refusal.
/// `("NAME", Some(value))` sets it.
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
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    let last_line = stdout
        .lines()
        .last()
        .unwrap_or_else(|| panic!("expected a GHCLI envelope on stdout, got: {stdout:?}"));
    serde_json::from_str(last_line)
        .unwrap_or_else(|err| panic!("expected a GHCLI envelope, got {last_line:?}: {err}"))
}

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
