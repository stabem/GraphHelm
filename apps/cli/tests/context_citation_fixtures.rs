//! Walks every citation case in the shared package and runs it through the real verifier.
//!
//! **Scope provenance.** This file and
//! `extensions/builtin/graphhelm-development-contracts/schemas/context-citation-case.schema.json`
//! entered #222's strict file list by **scope amendment 5**, in the issue body. The amendment's
//! own text was committed as the scope amendment 5 (#222) note.
//!
//! **Why fixtures at all, when unit tests already cover this.** L's gate 3 on #222 requires the
//! citation-spoofing threat to exist as a *fixture*, not only as a Rust test — a case in the shared
//! package is readable by every lane and by the schema tooling, where a unit test is readable by
//! whoever opens that file. The gate names two shapes and singles out the second: an ID that is
//! not in the capsule, and an ID **from a different capsule**, which is the scope-bleed version and
//! the one a naive "does this ID parse" check passes.
//!
//! **Why the cases declare items and not IDs.** Item identity is derived from content. A fixture
//! carrying a hard-coded `item-…` string would assert against a number nothing computed, and would
//! go on passing after the derivation changed — the guard would then be certifying its own stale
//! copy of the answer. The walker derives every ID through the same `item_id` production callers
//! use, so a change to the derivation moves the fixtures with it or reddens them.

use std::path::{Path, PathBuf};

use graphhelm_runtime::context_compiler::{item_id, verify_citations};

fn package_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../extensions/builtin/graphhelm-development-contracts")
}

fn case_schema() -> serde_json::Value {
    let path = package_root().join("schemas/context-citation-case.schema.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "arrangement check: the citation-case schema must be readable at {} ({e})",
            path.display()
        )
    });
    serde_json::from_str(&text).expect("the citation-case schema must be valid JSON")
}

fn cases() -> Vec<(PathBuf, serde_json::Value)> {
    let dir = package_root().join("fixtures/context");
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) => panic!(
            "found no citation cases: {} could not be read ({e}).\n\n\
             This is a failure, not an empty pass. A sweep that returns nothing looks exactly like \
             a sweep whose every case succeeded, and the assertions below would all hold \
             vacuously over an empty list.",
            dir.display()
        ),
    };

    let mut out = Vec::new();
    for entry in entries {
        let path = entry.expect("a directory entry must be readable").path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("case {} could not be read ({e})", path.display()));
        let value = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("case {} is not valid JSON ({e})", path.display()));
        out.push((path, value));
    }
    out
}

/// Derive the item IDs for one capsule's declared items.
fn ids_for(capsule_id: &str, capsule: &serde_json::Value) -> Vec<String> {
    let version = capsule["version"]
        .as_u64()
        .expect("version must be an integer") as u32;
    capsule["items"]
        .as_array()
        .expect("items must be an array")
        .iter()
        .enumerate()
        .map(|(position, item)| {
            item_id(
                capsule_id,
                version,
                item["section"].as_str().expect("section must be a string"),
                position,
                item["text"].as_str().expect("text must be a string"),
            )
        })
        .collect()
}

fn describe(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("<unnamed>")
        .to_owned()
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **a
/// `verify_citations` that resolves a citation against any known item rather than against the
/// capsule under test** — the scope-bleed case then reports `Accepted`, because the ID it cites is
/// a real ID of a real item, just one belonging to a different capsule.
///
/// Also fails on the cheaper mistakes: dropping either refusal code, emitting one for the wrong
/// cause, or accepting a result that cited nothing at all.
#[test]
fn every_citation_case_produces_exactly_the_refusal_codes_it_declares() {
    let schema = case_schema();
    let cases = cases();

    assert!(
        cases.len() >= 3,
        "expected at least the three cases gate 3 requires — a required item nobody cited, an ID \
         nothing hashes to, and an ID belonging to another capsule — and found {}: {:?}. A short \
         sweep passes for the cases it did not run.",
        cases.len(),
        cases.iter().map(|(p, _)| describe(p)).collect::<Vec<_>>()
    );

    for (path, case) in &cases {
        let name = describe(path);

        // `validate_inline_value` returns `Ok` when the VALIDATOR ran, not when the DOCUMENT
        // passed: the verdict is the diagnostics vector inside it. Reading the `Result` alone would
        // wave every invalid case straight through, and the case would then run against the
        // verifier with a shape nobody checked.
        let diagnostics =
            graphhelm_schema::validate_inline_value(&schema, case, "context-citation-case")
                .expect("the citation-case schema compiles offline");
        assert!(
            diagnostics.is_empty(),
            "case {name} does not validate against its own schema: {diagnostics:?}"
        );

        let capsules = case["capsules"]
            .as_object()
            .expect("capsules must be an object");
        let under_test = case["capsuleUnderTest"]
            .as_str()
            .expect("capsuleUnderTest must be a string");
        let capsule = capsules.get(under_test).unwrap_or_else(|| {
            panic!(
                "case {name} names `{under_test}` as the capsule under test and does not declare it"
            )
        });

        let capsule_item_ids = ids_for(under_test, capsule);
        let version = capsule["version"].as_u64().unwrap() as u32;

        let required_item_ids: Vec<String> = case["requiredItems"]
            .as_array()
            .expect("requiredItems must be an array")
            .iter()
            .enumerate()
            .map(|(position, item)| {
                item_id(
                    under_test,
                    version,
                    item["section"].as_str().unwrap(),
                    position,
                    item["text"].as_str().unwrap(),
                )
            })
            .collect();

        let cited_item_ids: Vec<String> = case["citations"]
            .as_array()
            .expect("citations must be an array")
            .iter()
            .map(|citation| {
                if let Some(literal) = citation["literal"].as_str() {
                    return literal.to_owned();
                }
                let owner = citation["capsule"]
                    .as_str()
                    .expect("citation needs a capsule");
                let owner_capsule = capsules.get(owner).unwrap_or_else(|| {
                    panic!("case {name} cites capsule `{owner}` and does not declare it")
                });
                item_id(
                    owner,
                    owner_capsule["version"].as_u64().unwrap() as u32,
                    citation["section"].as_str().unwrap(),
                    0,
                    citation["text"].as_str().unwrap(),
                )
            })
            .collect();

        let verdict = verify_citations(&capsule_item_ids, &required_item_ids, &cited_item_ids);
        let mut produced: Vec<String> = verdict
            .refusal_codes()
            .iter()
            .map(|code| code.wire_name().to_owned())
            .collect();
        produced.sort();

        let mut expected: Vec<String> = case["expectedRefusalCodes"]
            .as_array()
            .expect("expectedRefusalCodes must be an array")
            .iter()
            .map(|code| code.as_str().expect("a code must be a string").to_owned())
            .collect();
        expected.sort();

        assert_eq!(
            produced,
            expected,
            "case {name} declares {expected:?} and the verifier produced {produced:?}.\n\
             The case says why it exists: {}",
            case["why"].as_str().unwrap_or("<no reason recorded>")
        );
    }
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **adding a case
/// file whose `expectedRefusalCodes` is empty**, or one whose declared capsule holds no items.
///
/// Without this the sweep above is satisfiable by three cases that assert nothing. Each of them
/// would validate, run, and agree with an empty expectation — three green rows standing for no
/// coverage at all, which is the failure a count of cases cannot see.
#[test]
fn no_citation_case_is_an_empty_assertion() {
    for (path, case) in &cases() {
        let name = describe(path);
        assert!(
            !case["expectedRefusalCodes"]
                .as_array()
                .map(|a| a.is_empty())
                .unwrap_or(true),
            "case {name} expects no refusal at all. A case that asserts an empty outcome is a row \
             in the report and nothing else; if the intent is to cover the accepted path, say so \
             in a case that also names the items it cited."
        );
        let items = case["capsules"][case["capsuleUnderTest"].as_str().unwrap_or("")]["items"]
            .as_array()
            .map(|a| a.len())
            .unwrap_or(0);
        assert!(
            items > 0,
            "case {name} declares a capsule under test with no items, so every citation is \
             unresolved by construction and the case cannot distinguish the two causes"
        );
    }
}
