use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use graphhelm_schema::OfflineSchemaSet;
use serde_json::{Value, json};

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn package_root() -> PathBuf {
    repository_root().join("extensions/builtin/graphhelm-jpd")
}

fn load_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn package_schemas(root: &Path) -> OfflineSchemaSet {
    let schema_directory = root.join("schemas");
    let mut paths = fs::read_dir(&schema_directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    paths.sort();
    let documents = paths
        .into_iter()
        .map(|path| {
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            (relative, load_json(&path))
        })
        .collect::<BTreeMap<_, _>>();
    OfflineSchemaSet::compile(documents).unwrap()
}

fn retry_schema_id(root: &Path) -> String {
    load_json(&root.join("schemas/retry-chain.schema.json"))["$id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn recovered_chain(root: &Path) -> Value {
    load_json(&root.join("fixtures/positive/recovered-retry-chain.json"))
}

fn prepend_unexplained_failure(chain: &mut Value) {
    let root_id = chain["rootAttemptId"].clone();
    let retries = chain["retries"].as_array_mut().unwrap();
    let mut explained_success = retries.remove(0);
    let success_id = explained_success["attemptId"].clone();

    let mut unexplained_failure = explained_success.clone();
    unexplained_failure["attemptId"] = json!("attempt/issue-210/unexplained");
    unexplained_failure["ordinal"] = json!(2);
    unexplained_failure["retryOf"] = root_id;
    unexplained_failure["causeTag"] = json!("unexplained");
    unexplained_failure["result"] = json!("failed");
    unexplained_failure["startedAt"] = json!("2026-08-22T12:03:00Z");
    unexplained_failure["endedAt"] = json!("2026-08-22T12:03:04Z");
    unexplained_failure["evidenceDelta"]["materialChange"] = json!(false);

    explained_success["ordinal"] = json!(3);
    explained_success["retryOf"] = unexplained_failure["attemptId"].clone();
    retries.push(unexplained_failure);
    retries.push(explained_success);
    chain["successfulAttemptId"] = success_id;
}

fn append_terminal_failure(chain: &mut Value) {
    let retries = chain["retries"].as_array_mut().unwrap();
    let previous_attempt = retries.last().unwrap().clone();
    let mut terminal_failure = previous_attempt.clone();
    terminal_failure["attemptId"] = json!("attempt/issue-210/terminal-failure");
    terminal_failure["ordinal"] = json!(previous_attempt["ordinal"].as_u64().unwrap() + 1);
    terminal_failure["retryOf"] = previous_attempt["attemptId"].clone();
    terminal_failure["result"] = json!("failed");
    terminal_failure["startedAt"] = json!("2026-08-22T12:04:00Z");
    terminal_failure["endedAt"] = json!("2026-08-22T12:04:04Z");
    terminal_failure["evidenceDelta"]["materialChange"] = json!(false);
    retries.push(terminal_failure);
}

#[test]
fn success_classifications_require_the_latest_retry_to_succeed() {
    let root = package_root();
    let schemas = package_schemas(&root);

    let mut recovered = recovered_chain(&root);
    append_terminal_failure(&mut recovered);
    assert!(
        !schemas
            .validate(&retry_schema_id(&root), &recovered, "jpd-retry-test")
            .is_empty(),
        "recovered_success must reject a terminal failed retry"
    );

    let mut flaky = recovered_chain(&root);
    flaky["retries"][0]["evidenceDelta"]["materialChange"] = json!(false);
    flaky["outcomeClass"] = json!("flaky_pass");
    flaky["classificationBasis"] = json!([
        "later_attempt_succeeded",
        "explained_cause",
        "first_failure_preserved"
    ]);
    append_terminal_failure(&mut flaky);
    assert!(
        !schemas
            .validate(&retry_schema_id(&root), &flaky, "jpd-retry-test")
            .is_empty(),
        "flaky_pass must reject a terminal failed retry"
    );
}

#[test]
fn recovered_success_rejects_an_earlier_unexplained_retry() {
    let root = package_root();
    let schemas = package_schemas(&root);
    let mut chain = recovered_chain(&root);
    prepend_unexplained_failure(&mut chain);

    let diagnostics = schemas.validate(&retry_schema_id(&root), &chain, "jpd-retry-test");
    assert!(
        !diagnostics.is_empty(),
        "first-match classification must choose flaky_pass when any retry is unexplained"
    );
}

#[test]
fn flaky_pass_rejects_an_explained_material_success_without_unexplained_retries() {
    let root = package_root();
    let schemas = package_schemas(&root);
    let mut chain = recovered_chain(&root);
    chain["outcomeClass"] = json!("flaky_pass");

    let diagnostics = schemas.validate(&retry_schema_id(&root), &chain, "jpd-retry-test");
    assert!(
        !diagnostics.is_empty(),
        "an explained successful retry with material evidence is recovered_success, not flaky_pass"
    );
}

#[test]
fn flaky_pass_accepts_an_unexplained_retry_before_an_explained_material_success() {
    let root = package_root();
    let schemas = package_schemas(&root);
    let mut chain = recovered_chain(&root);
    prepend_unexplained_failure(&mut chain);
    chain["outcomeClass"] = json!("flaky_pass");
    chain["classificationBasis"] = json!([
        "later_attempt_succeeded",
        "unexplained_cause",
        "first_failure_preserved"
    ]);

    let diagnostics = schemas.validate(&retry_schema_id(&root), &chain, "jpd-retry-test");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn first_pass_success_has_one_exact_classification_basis() {
    let root = package_root();
    let schemas = package_schemas(&root);
    let mut chain = recovered_chain(&root);
    chain["rootAttempt"]["result"] = json!("succeeded");
    chain["retries"] = json!([]);
    chain["firstFailure"] = Value::Null;
    chain["successfulAttemptId"] = chain["rootAttemptId"].clone();
    chain["outcomeClass"] = json!("first_pass_success");
    chain["classificationBasis"] = json!(["root_succeeded_without_retry"]);

    let diagnostics = schemas.validate(&retry_schema_id(&root), &chain, "jpd-retry-test");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");

    chain["classificationBasis"] =
        json!(["root_succeeded_without_retry", "later_attempt_succeeded"]);
    assert!(
        !schemas
            .validate(&retry_schema_id(&root), &chain, "jpd-retry-test")
            .is_empty(),
        "first_pass_success must not claim a later attempt"
    );
}

#[test]
fn recovered_success_has_one_exact_classification_basis() {
    let root = package_root();
    let schemas = package_schemas(&root);
    let mut chain = recovered_chain(&root);
    chain["classificationBasis"] = json!(["root_succeeded_without_retry"]);

    assert!(
        !schemas
            .validate(&retry_schema_id(&root), &chain, "jpd-retry-test")
            .is_empty(),
        "recovered_success must bind the later explained material recovery basis"
    );
}

#[test]
fn flaky_pass_basis_tracks_the_first_matching_reason() {
    let root = package_root();
    let schemas = package_schemas(&root);

    let mut unexplained = recovered_chain(&root);
    prepend_unexplained_failure(&mut unexplained);
    unexplained["outcomeClass"] = json!("flaky_pass");
    unexplained["classificationBasis"] = json!([
        "later_attempt_succeeded",
        "explained_cause",
        "first_failure_preserved"
    ]);
    assert!(
        !schemas
            .validate(&retry_schema_id(&root), &unexplained, "jpd-retry-test")
            .is_empty(),
        "an unexplained retry must be named as the first flaky reason"
    );

    let mut no_material = recovered_chain(&root);
    no_material["retries"][0]["evidenceDelta"]["materialChange"] = json!(false);
    no_material["outcomeClass"] = json!("flaky_pass");
    no_material["classificationBasis"] = json!([
        "later_attempt_succeeded",
        "unexplained_cause",
        "first_failure_preserved"
    ]);
    assert!(
        !schemas
            .validate(&retry_schema_id(&root), &no_material, "jpd-retry-test")
            .is_empty(),
        "an explained no-material retry must not claim an unexplained cause"
    );
}

#[test]
fn unresolved_failure_has_one_exact_classification_basis() {
    let root = package_root();
    let schemas = package_schemas(&root);
    let mut chain = recovered_chain(&root);
    chain["retries"][0]["result"] = json!("failed");
    chain["successfulAttemptId"] = Value::Null;
    chain["outcomeClass"] = json!("unresolved_failure");
    chain["classificationBasis"] = json!(["no_attempt_succeeded", "first_failure_preserved"]);

    let diagnostics = schemas.validate(&retry_schema_id(&root), &chain, "jpd-retry-test");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");

    chain["classificationBasis"] = json!(["root_succeeded_without_retry"]);
    assert!(
        !schemas
            .validate(&retry_schema_id(&root), &chain, "jpd-retry-test")
            .is_empty(),
        "unresolved_failure must bind no success and the preserved first failure"
    );
}

#[test]
fn first_match_policy_does_not_refuse_lower_priority_overlap() {
    let root = package_root();
    let schemas = package_schemas(&root);
    let policy: Value = serde_yaml_ng::from_slice(
        &fs::read(root.join("evaluators/retry-classification-policy.yaml")).unwrap(),
    )
    .unwrap();
    let policy_schema_id = load_json(&root.join("schemas/retry-classification-policy.schema.json"))
        ["$id"]
        .as_str()
        .unwrap()
        .to_owned();
    let diagnostics = schemas.validate(&policy_schema_id, &policy, "jpd-retry-test");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(policy["spec"]["evaluationOrder"], json!("first_match"));
    assert!(
        !policy["spec"]["refusal"]["when"]
            .as_str()
            .unwrap()
            .contains("exactly one rule"),
        "first_match may intentionally overlap lower-priority rules"
    );
}

#[test]
fn flaky_pass_accepts_an_explained_success_without_material_evidence() {
    let root = package_root();
    let schemas = package_schemas(&root);
    let mut chain = recovered_chain(&root);
    chain["retries"][0]["evidenceDelta"]["materialChange"] = json!(false);
    chain["outcomeClass"] = json!("flaky_pass");
    chain["classificationBasis"] = json!([
        "later_attempt_succeeded",
        "explained_cause",
        "first_failure_preserved"
    ]);

    let diagnostics = schemas.validate(&retry_schema_id(&root), &chain, "jpd-retry-test");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}
