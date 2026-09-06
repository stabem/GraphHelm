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
const EXEMPTIONS: &[(&str, &str)] = &[("api_http.rs", "fn raw_request(")];

const HELPERS: &[&str] = &[
    "fn raw_request(",
    "fn split_url(",
    "fn parse_response(",
    "struct RawResponse ",
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
