//! The self-validation journey and its fixtures (#226, task-010).
//!
//! These guards hold the journey graph and its fixture list against each other. The graph declares
//! what each node must present; the fixtures declare which branch each case drives and what the
//! journey must answer. Neither is authoritative alone: a fixture naming a node the graph does not
//! define, or a sabotage path that does not exist, is a case that silently never runs — and a case
//! that never runs reads exactly like a case that passed.
//!
//! The referential checks below exist because that integrity was previously asserted by a
//! throwaway script. A check nothing consumes produces the right value and gates nothing.
//!
//! Loading goes through `graphhelm_schema::load_graph` rather than a schema set assembled here:
//! it parses, validates against the checked-in schemas, and types the result, so this file tests
//! the graph by the same door the runtime uses instead of by a second one built to agree.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use graphhelm_schema::load_graph;
use serde_json::Value;

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn graph_path() -> PathBuf {
    repository_root().join("extensions/builtin/graphhelm-development-contracts/graphs/development-contracts-self-validation.yaml")
}

fn load_json(path: &Path) -> Value {
    serde_json::from_slice(
        &fs::read(path).unwrap_or_else(|error| panic!("unreadable at {}: {error}", path.display())),
    )
    .unwrap_or_else(|error| panic!("not JSON at {}: {error}", path.display()))
}

fn cases() -> Vec<Value> {
    let path = repository_root().join("extensions/builtin/graphhelm-development-contracts/graphs/development-contracts-self-validation.fixtures.json");
    let fixtures = load_json(&path);
    let cases = fixtures["cases"]
        .as_array()
        .expect("the fixtures file carries a cases array")
        .clone();
    assert!(
        !cases.is_empty(),
        "the fixtures file carries no cases, so every guard below would pass vacuously"
    );
    cases
}

fn case_id(case: &Value) -> String {
    case["id"]
        .as_str()
        .expect("every case has an id")
        .to_owned()
}

/// The journey graph loads, validates and types.
///
/// `load_graph` is the runtime's own door: it parses the YAML, validates against the checked-in
/// schemas, and types the result into `ExecutionGraph`. Passing here means more than "the JSON
/// shape is legal".
#[test]
fn the_journey_graph_loads_and_validates() {
    let loaded = load_graph(&graph_path()).unwrap_or_else(|diagnostics| {
        panic!("the self-validation graph is invalid: {diagnostics:?}")
    });
    assert_eq!(
        loaded.raw["kind"].as_str(),
        Some("ExecutionGraph"),
        "the loaded document is not an ExecutionGraph"
    );
}

/// A deliberately broken copy is REFUSED by the same call that accepts the real one.
///
/// Without this, the guard above passes identically against a loader that accepts everything, and
/// "the graph is valid" would mean only "the loader said nothing".
#[test]
fn the_graph_loader_refuses_a_broken_copy() {
    let mut broken: Value = load_graph(&graph_path()).expect("the real graph loads").raw;
    broken["spec"]["entrypoints"] = Value::Array(Vec::new());

    let directory = tempfile::tempdir().expect("a temporary directory");
    let path = directory.path().join("broken.json");
    fs::write(&path, serde_json::to_vec(&broken).expect("serialisable")).expect("write");

    let result = load_graph(&path);
    assert!(
        result.is_err(),
        "a graph with no entrypoints was accepted: the loader is not discriminating"
    );
}

/// Every fixture case terminates at a node the graph actually defines.
#[test]
fn every_case_terminates_at_a_node_the_graph_defines() {
    let loaded = load_graph(&graph_path()).expect("the graph loads");
    let nodes = loaded.raw["spec"]["nodes"]
        .as_object()
        .expect("the graph declares nodes")
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();

    for case in cases() {
        let terminal = case["terminalNode"]
            .as_str()
            .unwrap_or_else(|| panic!("{} has no terminalNode", case_id(&case)));
        assert!(
            nodes.contains(terminal),
            "case {} terminates at `{terminal}`, which the graph does not define",
            case_id(&case)
        );
    }
}

/// Every sabotage artifact a case points at is present on disk.
///
/// The fixtures reference the corpus by path rather than restating it, so the corpus stays the one
/// source of each attack. That only holds while the paths resolve.
#[test]
fn every_referenced_sabotage_artifact_exists() {
    let root = repository_root();
    let mut checked = 0_usize;
    for case in cases() {
        for key in ["journey", "evidence", "ground", "control"] {
            let Some(relative) = case[key].as_str() else {
                continue;
            };
            assert!(
                root.join(relative).is_file(),
                "case {} points `{key}` at {relative}, which is not a file",
                case_id(&case)
            );
            checked += 1;
        }
    }
    assert!(
        checked > 0,
        "no case referenced any artifact: this guard would pass without looking at anything"
    );

    // The honest cases must carry a journey fixture of their own, not inline data. They are the
    // half that proves a refusal means something, so they are artifacts a reader can open, on the
    // same footing as the sabotage entries they are there to balance.
    let honest = cases()
        .into_iter()
        .filter(|case| case["expect"]["outcome"].as_str() == Some("certified"))
        .collect::<Vec<_>>();
    assert!(
        honest.len() >= 2,
        "fewer than two certifying cases: one per capture branch is the floor"
    );
    for case in honest {
        assert!(
            case["journey"].as_str().is_some(),
            "certifying case {} carries no journey fixture",
            case_id(&case)
        );
    }
}

/// Both capture branches are driven, and each has a case that CERTIFIES.
///
/// The honest cases are what stop the refusal cases from being satisfied by an implementation that
/// refuses everything. A corpus of only-refusals cannot tell a working gate from a closed door.
#[test]
fn both_capture_branches_are_driven_and_both_can_certify() {
    let mut driven = BTreeSet::new();
    let mut certified = BTreeSet::new();
    for case in cases() {
        let branch = case["branch"]
            .as_str()
            .unwrap_or_else(|| panic!("{} has no branch", case_id(&case)))
            .to_owned();
        driven.insert(branch.clone());
        if case["expect"]["outcome"].as_str() == Some("certified") {
            certified.insert(branch);
        }
    }
    let expected = BTreeSet::from(["capture_off".to_owned(), "capture_on".to_owned()]);
    assert_eq!(
        driven, expected,
        "both capture branches must be driven by the fixture set"
    );
    assert_eq!(
        certified, expected,
        "each capture branch needs a case that CERTIFIES, or the refusals prove nothing"
    );
}

/// No two refusal cases share a named reason.
///
/// Distinct sabotages must be refused for distinct causes. If two cases accept the same reason, an
/// implementation that refuses for the wrong cause satisfies both, and the corpus stops
/// distinguishing the defects it was built to separate.
#[test]
fn refusal_cases_name_distinct_reasons() {
    let mut reasons = Vec::new();
    for case in cases() {
        if case["expect"]["outcome"].as_str() != Some("refused") {
            continue;
        }
        let reason = case["expect"]["namedReason"]
            .as_str()
            .unwrap_or_else(|| panic!("refusal case {} names no reason", case_id(&case)))
            .to_owned();
        reasons.push((case_id(&case), reason));
    }
    assert!(
        reasons.len() >= 2,
        "fewer than two refusal cases: this guard would pass vacuously"
    );
    let distinct = reasons
        .iter()
        .map(|(_, reason)| reason.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        distinct.len(),
        reasons.len(),
        "two refusal cases share a named reason, so neither can fail alone: {reasons:?}"
    );
}

/// The secret-capture case forbids echoing the value it refuses.
///
/// Refusing while logging the secret fails the half that matters most, and the memory screen makes
/// the same argument at its own refusal site: the refusal is itself a persisted event.
#[test]
fn the_secret_case_forbids_echoing_what_it_refuses() {
    let case = cases()
        .into_iter()
        .find(|case| case["expect"]["namedReason"].as_str() == Some("secret_detected"))
        .expect("the corpus carries a secret-capture case");
    let forbidden = case["expect"]["mustNotEcho"]
        .as_array()
        .expect("the secret case declares mustNotEcho");
    assert!(
        !forbidden.is_empty(),
        "mustNotEcho is empty, so `refused` alone would satisfy the case"
    );
}
