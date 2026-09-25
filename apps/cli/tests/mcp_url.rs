//! Black-box: the MCP command refuses an unusable base URL before any protocol byte (#214).
//!
//! **What this half can and cannot assert, stated rather than left to the word "black-box".**
//! `apps/cli` is a binary-only crate with no `[lib]`, so an integration test can drive the process
//! and nothing else — the composition table is unit-tested inside `commands/mcp/url.rs`, next to
//! the code, in the same pattern `is_loopback_url` already uses.
//!
//! And the refusals are ordered. `build_client` checks loopback first, so a base with a bad scheme,
//! or with no authority, is refused by *that* check and never reaches the URL contract. Asserting
//! those here would be asserting a message this change did not write — a case passing for a
//! neighbouring reason. The base shapes that pass loopback and reach the contract are the ones with
//! a fragment, and that is what this file covers end to end.
//!
//! Cross-platform because it asserts on the process's own output. Nothing is bound, nothing is
//! dialled, and no server has to exist: the refusal happens before any byte leaves.

use std::time::Duration;

fn refuse_output(url: &str) -> (bool, String) {
    let mut command = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
    let output = command
        .arg("mcp")
        .arg("--url")
        .arg(url)
        .arg("--actor")
        .arg("chat-under-test")
        .env(
            "GRAPHHELM_API_TOKEN",
            "test-token-not-used-because-the-url-is-refused-first",
        )
        .write_stdin("")
        .timeout(Duration::from_secs(30))
        .output()
        .expect("the mcp command runs to completion");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.success(), combined)
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **accepting a
/// base that carries a fragment**, or repairing one by stripping it.
///
/// A fragment never reaches a server. A base carrying one describes a request that cannot happen,
/// and the operator who wrote it believes otherwise until something tells them. Refusing at startup
/// is what tells them; a per-request failure would read as an intermittent fault instead.
#[test]
fn a_base_carrying_a_fragment_is_refused_before_any_protocol_byte() {
    let (succeeded, output) = refuse_output("http://127.0.0.1:8080/#fragment");
    assert!(
        !succeeded,
        "the command accepted a base with a fragment. Output was:\n{output}"
    );
    assert!(
        output.contains("fragment"),
        "the refusal must name what is wrong with the base, or the operator has to guess which of \
         their flags is at fault. Output was:\n{output}"
    );
}

/// The positive control for the case above, and it is not optional.
///
/// A command that refused every `--url` would satisfy the fragment case perfectly. This one shows
/// the loopback base the built-in package configures is still admitted far enough to ask for a
/// token — the next refusal in `build_client`'s fail-closed order — which is as far as a test
/// without a running Runtime can see.
#[test]
fn the_ordinary_loopback_base_is_not_refused_for_its_url() {
    let mut command = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
    let output = command
        .arg("mcp")
        .arg("--url")
        .arg("http://127.0.0.1:8080")
        .arg("--actor")
        .arg("chat-under-test")
        .env_remove("GRAPHHELM_API_TOKEN")
        .write_stdin("")
        .timeout(Duration::from_secs(30))
        .output()
        .expect("the mcp command runs to completion");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        !combined.contains("fragment") && !combined.contains("not an absolute URL"),
        "an ordinary loopback base must not be refused by the URL contract. Output was:\n{combined}"
    );
    assert!(
        combined.contains("token"),
        "the run should have reached the token check, which is the refusal after the URL in \
         build_client's fail-closed order. Output was:\n{combined}"
    );
}
