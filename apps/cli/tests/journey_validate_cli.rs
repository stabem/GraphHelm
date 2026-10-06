//! #328: `graphhelm journey validate` checks journey contracts before anything records against
//! them: exit 0 clean, 2 findings (with the finding named), 3 when the input is unusable.

use std::path::Path;
use std::process::{Command, Output};

use serde_json::{Value, json};

fn graphhelm() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
}

fn step(id: &str, screen: &str, scope: &str) -> Value {
    json!({
        "stepId": id,
        "actorId": "shopper",
        "semanticAction": {
            "kind": "navigate",
            "target": {"strategy": "visible_text", "value": "Cart", "geometryClaim": false}
        },
        "expectedStates": ["stable"],
        "failureContract": {
            "timeoutSeconds": 30,
            "visibleError": "Cart did not open",
            "safeStop": "Stay on the page",
            "recoveryAction": null,
            "prohibitedSideEffects": []
        },
        "screen": {"screenId": screen, "title": screen, "scopePaths": [scope]}
    })
}

fn contract(id: &str) -> Value {
    json!({
        "contractId": id,
        "version": 1,
        "title": "Cart",
        "taskScope": "Open the cart and pay",
        "actors": [{"actorId": "shopper", "name": "Shopper", "goal": "Pay"}],
        "preconditions": [],
        "steps": [step("open-cart", "cart", "src/cart.tsx"), step("pay", "checkout", "src/pay")],
        "promises": [{
            "promiseId": "cart-visible",
            "stepId": "open-cart",
            "statement": "The cart shows its items",
            "requiredFact": "content_rendered",
            "requiredEvidenceKinds": ["visual_capture"],
            "requiredObserverCapability": "browser",
            "statesToObserve": ["stable"],
            "maxEvidenceAgeSeconds": 3600
        }],
        "riskSignals": [],
        "outOfScope": []
    })
}

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src/pay")).unwrap();
    std::fs::write(dir.path().join("src/cart.tsx"), "export {}").unwrap();
    std::fs::create_dir_all(dir.path().join(".graphhelm/journeys")).unwrap();
    dir
}

fn write(project: &Path, value: &Value, name: &str) -> std::path::PathBuf {
    let path = project.join(".graphhelm/journeys").join(name);
    std::fs::write(&path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    path
}

fn validate(project: &Path, extra: &[&std::ffi::OsStr]) -> (Output, Value) {
    let output = graphhelm()
        .args(["--json", "journey", "validate", "--project"])
        .arg(project)
        .args(extra)
        .output()
        .unwrap();
    let reply = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    (output, reply)
}

fn codes(reply: &Value) -> Vec<String> {
    reply["data"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|file| file["findings"].as_array().unwrap().clone())
        .map(|finding| finding["code"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn a_good_contract_is_clean() {
    let dir = project();
    let file = write(dir.path(), &contract("cart"), "cart.json");
    let (output, reply) = validate(dir.path(), &[file.as_os_str()]);
    assert_eq!(output.status.code(), Some(0), "{reply}");
    assert_eq!(reply["command"], "journey.validate");
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["data"]["checked"], 1);
    assert_eq!(reply["data"]["findings"], 0);
}

#[test]
fn a_missing_scope_path_is_a_finding() {
    let dir = project();
    let mut value = contract("cart");
    value["steps"][1]["screen"]["scopePaths"] = json!(["src/gone.tsx"]);
    let file = write(dir.path(), &value, "cart.json");
    let (output, reply) = validate(dir.path(), &[file.as_os_str()]);
    assert_eq!(output.status.code(), Some(2), "{reply}");
    assert_eq!(codes(&reply), ["scope_path_missing"]);
    assert_eq!(
        reply["data"]["files"][0]["findings"][0]["pointer"],
        "/steps/1/screen/scopePaths/0"
    );
    assert_eq!(
        reply["diagnostics"][0]["code"],
        "GHCLI033_JOURNEY_CONTRACT_INVALID"
    );
}

#[test]
fn an_id_the_schema_allows_but_journeys_refuse_is_a_finding() {
    let dir = project();
    let mut value = contract("cart");
    // `:` and `/` pass the schema's id pattern but not the journey id rule.
    value["steps"][0]["stepId"] = json!("cart:open");
    value["promises"][0]["stepId"] = json!("cart:open");
    let file = write(dir.path(), &value, "cart.json");
    let (output, reply) = validate(dir.path(), &[file.as_os_str()]);
    assert_eq!(output.status.code(), Some(2), "{reply}");
    assert_eq!(codes(&reply), ["invalid_id"]);
}

#[test]
fn a_schema_error_and_consistency_errors_are_findings() {
    let dir = project();
    let mut value = contract("cart");
    value["steps"][0]["expectedStates"] = json!(["happy"]);
    value["steps"][1]["actorId"] = json!("ghost");
    value["steps"][1]["stepId"] = json!("open-cart");
    value["steps"][1]["screen"]["screenId"] = json!("cart");
    let file = write(dir.path(), &value, "wrong-name.json");
    let (output, reply) = validate(dir.path(), &[file.as_os_str()]);
    assert_eq!(output.status.code(), Some(2), "{reply}");
    let found = codes(&reply);
    for expected in [
        "schema_invalid",
        "contract_id_mismatch",
        "duplicate_step",
        "unknown_actor",
        "screen_inconsistent",
    ] {
        assert!(
            found.iter().any(|code| code == expected),
            "{expected} in {found:?}"
        );
    }
}

#[test]
fn all_reads_every_contract_in_the_project() {
    let dir = project();
    write(dir.path(), &contract("cart"), "cart.json");
    write(dir.path(), &json!({"contractId": "broken"}), "broken.json");
    std::fs::write(
        dir.path().join(".graphhelm/journeys/notes.json"),
        "{not json",
    )
    .unwrap();
    let (output, reply) = validate(dir.path(), &["--all".as_ref()]);
    assert_eq!(output.status.code(), Some(2), "{reply}");
    assert_eq!(reply["data"]["checked"], 3);
    let files = reply["data"]["files"].as_array().unwrap();
    let ok: Vec<bool> = files.iter().map(|file| file["ok"] == true).collect();
    assert_eq!(ok, [false, true, false], "{reply}");
    assert_eq!(files[2]["findings"][0]["code"], "not_json");
}

#[test]
fn unusable_input_exits_3() {
    let dir = project();
    let (output, _) = validate(dir.path(), &[]);
    assert_eq!(output.status.code(), Some(3));
    let missing = dir.path().join("nope.json");
    let (output, _) = validate(dir.path(), &[missing.as_os_str()]);
    assert_eq!(output.status.code(), Some(3));
    let bare = tempfile::tempdir().unwrap();
    let (output, reply) = validate(bare.path(), &["--all".as_ref()]);
    assert_eq!(output.status.code(), Some(3), "{reply}");
}

/// #329 review: `Path::join` discards the project for an absolute or drive path, so such an entry
/// must be a finding, never probed and never silently skipped.
#[test]
fn a_scope_path_that_leaves_the_project_is_a_finding() {
    for bad in ["C:/Windows", "c:x", "/etc", "src/../..", "src\\cart.tsx"] {
        let dir = project();
        let mut value = contract("cart");
        value["steps"][0]["screen"]["scopePaths"] = json!([bad]);
        let file = write(dir.path(), &value, "cart.json");
        let (output, reply) = validate(dir.path(), &[file.as_os_str()]);
        assert_eq!(output.status.code(), Some(2), "{bad}: {reply}");
        let found = codes(&reply);
        assert!(
            found
                .iter()
                .any(|code| code == "scope_path_outside_project"),
            "{bad}: {found:?}"
        );
    }
}
