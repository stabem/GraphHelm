//! #179: the CLI's HTTP test client has ONE definition per helper, and every exemption is named
//! here rather than discovered later.
//!
//! **What this guards, and why a comment could not.** `raw_request`, `split_url`, `parse_response`
//! and `RawResponse` were defined independently in five test files. Measured before this guard
//! existed, they had already drifted: three distinct `split_url` bodies, two `parse_response`
//! bodies, and a request timeout of 5 seconds in one file and 15 in three others with nothing
//! saying why. Every one of those differences was in the WORDING of a panic message, so the suites
//! agreed on behaviour by luck rather than by construction — and the next copy-paste had no reason
//! to notice.
//!
//! **The exemption is the point.** `api_http.rs` keeps its own `raw_request` because it bounds the
//! connect (`connect_with_retry` plus a hang guard) after measuring that an unbounded connect
//! under a full accept backlog retransmits for ~21 seconds and outlives the caller's own deadline.
//! That is a different function, not a stale copy. A guard that simply banned duplicates would
//! force it to be deleted or hoisted; this one requires it to be LISTED, so the fleet reads
//! "deliberate" instead of "drifted".
//!
//! **This is a sweep, not an `include_str!`.** The files are read at run time from
//! `CARGO_MANIFEST_DIR`, so a helper added to a file this test never heard of is still counted
//! (#204: a source-reading guard that bakes its subject in at compile time can pass against text
//! that no longer exists).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The one file allowed to define a helper that `support/mod.rs` also defines, and the one helper
/// it may define. A CLOSED list: adding to it is an edit a reviewer sees.
///
/// #1313 (plan wave E, HTTP test harness) widened the guard to the ten helper names the plan
/// counted. `token_path`, `read_token`, `wait_for_health`, `get_json`, `repository_root` and
/// `owner_token` live in `support/mod.rs`; every copy left below is a DIFFERENT function under
/// the same name, with the reason on its group. The second test fails on an exemption whose
/// copy is gone, so moving a copy also strikes it here.
const EXEMPTIONS: &[(&str, &str)] = &[
    // Bounded connect (connect_with_retry + hang guard); see the module doc.
    ("api_http.rs", "fn raw_request("),
    // api_http's own client: its health wait and GET go through the hardened `raw_request`.
    ("api_http.rs", "fn wait_for_health("),
    ("api_http.rs", "fn get_json("),
    // Bare `host:port` forms (a raw TcpStream / the file's own `request`), not a URL.
    ("mcp_stdio.rs", "fn wait_for_health("),
    ("monitor_http.rs", "fn wait_for_health("),
    // Bare address, mandatory Bearer token, Null on a non-JSON body.
    ("wake_http.rs", "fn get_json("),
    // `post_json`: six signatures (fixed idempotency/actor headers, bare addresses, chunked-body
    // trimming, a `Server` handle, different timeouts); each file's own request shape.
    ("api_http.rs", "fn post_json("),
    ("context_journey.rs", "fn post_json("),
    ("development_context_budget.rs", "fn post_json("),
    ("documents_http.rs", "fn post_json("),
    ("gate_http.rs", "fn post_json("),
    ("mcp_stdio.rs", "fn post_json("),
    ("resume_atomicity.rs", "fn post_json("),
    ("resume_project_default.rs", "fn post_json("),
    ("runtime_http.rs", "fn post_json("),
    ("wake_http.rs", "fn post_json("),
    // `serve`: each starts the server its suite needs (env, keyring, extra flags, journeys,
    // project registries) and returns its own guard type; several are methods.
    ("api_http.rs", "fn serve("),
    ("documents_http.rs", "fn serve("),
    ("journey_flow_surfaces.rs", "fn serve("),
    ("journeys_surfaces.rs", "fn serve("),
    ("mcp_discovery.rs", "fn serve("),
    ("mcp_stdio.rs", "fn serve("),
    ("monitor_http.rs", "fn serve("),
    ("providerless_journey.rs", "fn serve("),
    ("runtime_http.rs", "fn serve("),
    // serve_handover.rs: a fixed port and an inherited stdout the successor reuses (#1523).
    ("serve_handover.rs", "fn serve("),
    ("signal_image_evidence_http.rs", "fn serve("),
    ("wake_http.rs", "fn serve("),
    ("workspace_cli.rs", "fn serve("),
    // `cli`: six shapes (envelope or `data`, exit code or assert, secret env, `Command` builders).
    ("amend_budget.rs", "fn cli("),
    ("api_http.rs", "fn cli("),
    ("documents_http.rs", "fn cli("),
    ("journey_explore_browser.rs", "fn cli("),
    ("journey_flow_surfaces.rs", "fn cli("),
    ("journey_live_browser.rs", "fn cli("),
    ("journey_live_cli.rs", "fn cli("),
    ("journey_preview_browser.rs", "fn cli("),
    ("journey_preview_cli.rs", "fn cli("),
    ("journey_replay_browser.rs", "fn cli("),
    ("journey_replay_cli.rs", "fn cli("),
    ("journeys_surfaces.rs", "fn cli("),
    ("mcp_capability.rs", "fn cli("),
    ("resume_atomicity.rs", "fn cli("),
    // `credential_set`: architect_cli and gateway_cli take a provider/value and use assert_cmd;
    // the four `--usable-by` copies each seal with their own gateway key and stdin secret, so
    // one helper would add two arguments to 24 call sites to save about 25 lines (#1313).
    ("architect_cli.rs", "fn credential_set("),
    ("gateway_cli.rs", "fn credential_set("),
    ("context_journey.rs", "fn credential_set("),
    ("gate_http.rs", "fn credential_set("),
    ("resume_project_default.rs", "fn credential_set("),
    ("runtime_http.rs", "fn credential_set("),
    // `repository_root` without `canonicalize` (`join("../..")`, `ancestors().nth(2)`): these
    // suites compare against non-canonical paths, which the canonical form would prefix.
    ("event_store_local_backup.rs", "fn repository_root("),
    ("extension_cli.rs", "fn repository_root("),
    ("schema_cli.rs", "fn repository_root("),
    ("shipped_package_coverage.rs", "fn repository_root("),
    ("test_support_has_no_dead_helpers.rs", "fn repository_root("),
];

const HELPERS: &[&str] = &[
    "fn raw_request(",
    "fn split_url(",
    "fn parse_response(",
    "struct RawResponse ",
    // #1313: the plan row's helper names.
    "fn token_path(",
    "fn read_token(",
    "fn wait_for_health(",
    "fn get_json(",
    "fn post_json(",
    "fn serve(",
    "fn cli(",
    "fn credential_set(",
    "fn repository_root(",
    "fn owner_token(",
];

fn tests_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests")
}

/// Every `.rs` file directly under `tests/`, plus the support module. Comments are NOT stripped:
/// these needles are declarations, and a declaration inside a comment is not one — the count is
/// over lines that start the declaration, which a `///` line cannot do.
fn sources() -> BTreeMap<String, String> {
    let mut found = BTreeMap::new();
    let directory = tests_directory();
    for entry in std::fs::read_dir(&directory).expect("the tests directory is readable") {
        let path = entry.expect("the directory entry is readable").path();
        if path.extension().is_some_and(|extension| extension == "rs") {
            let name = path
                .file_name()
                .expect("a file has a name")
                .to_string_lossy()
                .into_owned();
            found.insert(
                name,
                std::fs::read_to_string(&path).expect("the source is readable"),
            );
        }
    }
    let support = directory.join("support").join("mod.rs");
    found.insert(
        "support/mod.rs".to_owned(),
        std::fs::read_to_string(&support).expect("the support module is readable"),
    );
    found
}

fn declarations(text: &str, needle: &str) -> usize {
    text.lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with("//")
                && (trimmed.starts_with(needle) || trimmed.starts_with(&format!("pub {needle}")))
        })
        .count()
}

#[test]
fn each_http_test_helper_has_one_definition_and_every_exemption_is_named() {
    let sources = sources();
    assert!(
        sources.len() > 10,
        "ARRANGEMENT: the sweep read {} files under {} -- the scan is wrong, not the tree",
        sources.len(),
        tests_directory().display()
    );
    assert!(
        sources.contains_key("support/mod.rs"),
        "ARRANGEMENT: the support module was not read, so every count below would be a false zero"
    );

    let mut unexpected: Vec<String> = Vec::new();
    for helper in HELPERS {
        for (file, text) in &sources {
            let count = declarations(text, helper);
            if count == 0 {
                continue;
            }
            if file == "support/mod.rs" {
                assert_eq!(
                    count, 1,
                    "the support module defines `{helper}` {count} times; one definition is the whole point"
                );
                continue;
            }
            if file == "http_helper_inventory.rs" {
                continue;
            }
            if EXEMPTIONS.contains(&(file.as_str(), helper)) {
                assert_eq!(
                    count, 1,
                    "{file} is exempt for `{helper}` once, and defines it {count} times"
                );
                continue;
            }
            unexpected.push(format!("{file} defines `{helper}` ({count}x)"));
        }
    }

    assert!(
        unexpected.is_empty(),
        "these helpers are defined outside `tests/support/mod.rs` and are not on the exemption \
         list: {unexpected:?}. Take them from `support` instead, or add the file to EXEMPTIONS \
         with the reason in its own doc comment -- as `api_http.rs` does for its bounded connect."
    );
}

/// The exemption list must name only files that exist and helpers that are still there: a stale
/// exemption is a licence nobody is using, and it would hide the next real copy in that file.
#[test]
fn every_exemption_is_still_load_bearing() {
    let sources = sources();
    for (file, helper) in EXEMPTIONS {
        let text = sources
            .get(*file)
            .unwrap_or_else(|| panic!("the exempt file {file} is gone; drop it from EXEMPTIONS"));
        assert_eq!(
            declarations(text, helper),
            1,
            "{file} no longer defines `{helper}`, so its exemption is stale and would cover a \
             future copy nobody decided on"
        );
    }
}
