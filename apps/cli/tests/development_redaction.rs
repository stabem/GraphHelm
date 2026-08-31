//! Proves two of #223's acceptance criteria for the `development.*` surface that had no test
//! before this file (#394): JSON output has a stable key set, and no output leaks a filesystem
//! path outside the repo or a backtrace fragment. The existing redaction coverage
//! (`api_http.rs:1939,2272,2351`) is about extension-manifest bytes, an unrelated feature --
//! `git grep -c "\"development\." apps/cli/tests/api_http.rs` is 0, and the parity guard's own
//! HTTP probe (`development_surface_parity.rs`) only asserts `status != 404 && status != 405`,
//! never body content.

use std::collections::BTreeSet;
use std::process::Command;

use serde_json::Value;

const DEVELOPMENT_OPERATIONS: &[&str] = &[
    "resolve-contract",
    "memory-status",
    "present",
    "compile-context",
    "memory-propose",
    "accounting",
];

fn raw_output(subcommand: &str) -> Vec<u8> {
    Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["development", subcommand])
        .output()
        .unwrap_or_else(|error| panic!("the built binary runs `development {subcommand}`: {error}"))
        .stdout
}

fn run(subcommand: &str) -> Value {
    let stdout = raw_output(subcommand);
    serde_json::from_slice(&stdout).unwrap_or_else(|error| {
        panic!(
            "{subcommand}: stdout was not the JSON envelope ({error}): {:?}",
            String::from_utf8_lossy(&stdout)
        )
    })
}

/// Every operation's own top-level `data` key set, as measured against the running binary today.
///
/// **Not derived from the operation itself** -- a set built by re-deriving it from the same code
/// path it is meant to freeze would defeat the guard, the same way `envelope["data"] == policy`
/// in `development_cli.rs` only freezes `memory-status` (by comparing against the independently
/// loaded shipped YAML) and leaves the other five operations with no shape check at all. This is
/// a plain, hand-written expectation for exactly that reason.
fn expected_top_level_keys(subcommand: &str) -> &'static [&'static str] {
    match subcommand {
        "resolve-contract" => &["record", "requirements"],
        // Updated by #220/ADR-032's two-axis memory lifecycle (#548, `68d8890`): the shipped
        // `memory-transition.yaml` this command echoes verbatim replaced its one-axis shape
        // (`states`/`transitions`/`allowed`) with two independent axes plus the supersession
        // relationship's own reason set. The new set is correct, not merely different --
        // `development_cli.rs`'s own equality-against-the-shipped-policy check already proves
        // the CLI's answer matches the YAML byte-for-byte; this freeze just fell behind it.
        "memory-status" => &[
            "allowedPublicationTransitions",
            "policyVersion",
            "publicationStates",
            "publicationTransitions",
            "semanticStates",
            "supersessionReasons",
        ],
        "present" => &["text"],
        "compile-context" => &["digest"],
        "memory-propose" => &["admitted"],
        "accounting" => &["totalTokens"],
        other => panic!(
            "{other}: no expected key set recorded here -- add one in the same change that adds \
             the operation rather than leaving it unchecked"
        ),
    }
}

/// The key set of every `development.*` operation's `data` object must match what is recorded
/// above. A field silently gained or lost is exactly the kind of change #223's "JSON output is
/// stable" criterion exists to catch, and nothing before this test asserted it for five of the
/// six operations (`memory-status` is the one exception, covered incidentally by
/// `development_cli.rs`'s exact-equality check against the shipped policy).
#[test]
fn every_development_cli_output_has_its_declared_key_set() {
    for subcommand in DEVELOPMENT_OPERATIONS {
        let envelope = run(subcommand);
        assert_eq!(
            envelope["ok"], true,
            "{subcommand}: command did not succeed: {envelope}"
        );
        let data = envelope["data"]
            .as_object()
            .unwrap_or_else(|| panic!("{subcommand}: `data` is not a JSON object: {envelope}"));
        let actual: BTreeSet<&str> = data.keys().map(String::as_str).collect();
        let expected: BTreeSet<&str> = expected_top_level_keys(subcommand)
            .iter()
            .copied()
            .collect();
        assert_eq!(
            actual, expected,
            "{subcommand}: `data`'s key set changed -- used to report {expected:?}, now reports \
             {actual:?}. A new or removed field is a real change; update the expected set in the \
             SAME change that adds or removes it, and name why."
        );
    }
}

/// Substrings that must never appear in a `development.*` response: a filesystem path rooted at
/// a user's home directory (Windows and Unix shapes), or a Rust panic/backtrace fragment. Checked
/// against the RAW stdout bytes, not a parsed field -- a leak can land anywhere a string gets
/// spliced into the response, not only in a field this test happens to inspect structurally.
///
/// Armed and proven by #394's own sabotage-and-revert: with a literal `C:\Users\test\...
/// panicked at ...` fragment appended to `run_present`'s `text` field, this test fails at this
/// exact assertion (documented in the PR), and the full existing `development_*`/`mcp_stdio`/
/// `api_http` suite stays green under the same sabotage -- proving the gap #394 was filed for.
const HOME_PATH_MARKERS: &[&str] = &["Users\\", "Users/", "/home/"];
const BACKTRACE_MARKERS: &[&str] = &["panicked at", "stack backtrace", "RUST_BACKTRACE"];

#[test]
fn no_development_cli_output_leaks_a_home_path_or_a_backtrace() {
    for subcommand in DEVELOPMENT_OPERATIONS {
        let raw = String::from_utf8_lossy(&raw_output(subcommand)).into_owned();
        for marker in HOME_PATH_MARKERS.iter().chain(BACKTRACE_MARKERS) {
            assert!(
                !raw.contains(marker),
                "{subcommand}: output contains {marker:?} -- a development.* response must never \
                 carry a filesystem path outside the repo or a backtrace fragment: {raw}"
            );
        }
    }
}
