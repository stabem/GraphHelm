use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use graphhelm_schema::OfflineSchemaSet;
use serde_json::Value;

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

#[test]
fn first_failure_provenance_rule_targets_failure_bearing_outcomes() {
    let package_root = repository_root().join("extensions/builtin/graphhelm-jpd");
    let policy_path = package_root.join("policies/skill-promotion-policy.yaml");
    let policy: Value = serde_yaml_ng::from_slice(&fs::read(policy_path).unwrap()).unwrap();
    let rule = policy["spec"]["denyWhen"]
        .as_array()
        .unwrap()
        .iter()
        .find(|rule| rule["id"] == "first-failure-provenance-missing")
        .unwrap();
    let conditions = rule["predicate"]["whereAll"].as_array().unwrap();

    assert_eq!(
        conditions,
        &[
            serde_json::json!({
                "field": "result",
                "operator": "in",
                "values": [
                    "recovered_success",
                    "flaky_pass",
                    "unresolved_failure"
                ]
            }),
            serde_json::json!({
                "field": "firstFailurePreserved",
                "operator": "equals",
                "value": false
            })
        ],
        "first-pass success has no initial failure to preserve; provenance is required only when a run contains failure history"
    );

    let schema_path = package_root.join("schemas/skill-promotion-policy.schema.json");
    let schema: Value = serde_json::from_slice(&fs::read(&schema_path).unwrap()).unwrap();
    let schema_id = schema["$id"].as_str().unwrap().to_owned();
    let schemas = OfflineSchemaSet::compile(BTreeMap::from([(
        "schemas/skill-promotion-policy.schema.json".to_owned(),
        schema,
    )]))
    .unwrap();
    assert!(
        schemas
            .validate(&schema_id, &policy, "jpd-policy")
            .is_empty(),
        "the normative policy must remain valid against its checked-in schema"
    );
}
