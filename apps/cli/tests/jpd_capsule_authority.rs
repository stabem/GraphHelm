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

fn schema_id(root: &Path, schema: &str) -> String {
    load_json(&root.join("schemas").join(schema))["$id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn advisory_evaluation() -> Value {
    serde_json::json!({
        "evaluationId": "evaluation/issue-210/advisory",
        "capsuleId": "capsule/issue-210/browser-install-check",
        "evaluatedAt": "2026-08-22T15:30:00Z",
        "authority": {
            "status": "candidate",
            "effect": "advisory_only",
            "capsuleValidation": {
                "status": "capability_missing",
                "code": "SKILL_CAPSULE_VALIDATOR_MISSING",
                "missingCapability": "jpd.registered-deterministic-skill-capsule-validator",
                "reason": "No registered deterministic Skill Capsule validator is available."
            }
        },
        "classification": {
            "status": "capability_missing",
            "refusal": {
                "code": "SKILL_EVALUATOR_MISSING",
                "missingCapability": "jpd.registered-deterministic-evaluator",
                "reason": "No registered deterministic skill evaluator is available."
            }
        },
        "runSamples": [{
            "journeyRunId": "run/issue-210/001",
            "result": "unresolved_failure",
            "evidenceCoverage": 0.5,
            "tokenCost": 100,
            "defectsBefore": 1,
            "defectsAfter": 1,
            "evidenceFresh": true,
            "firstFailurePreserved": true
        }],
        "aggregates": {
            "distinctRuns": 1,
            "evidenceCoverage": 0.5,
            "errorReduction": 0.0,
            "tokenOverheadRatio": 1.0,
            "generalizationScore": 0.0,
            "allEvidenceFresh": true
        },
        "counterexamples": [],
        "disagreements": [],
        "agentAgreementIsEvidence": false,
        "promotionDecision": {
            "status": "advisory",
            "eligible": false,
            "reasons": ["A registered deterministic evaluator is unavailable."],
            "requiredAction": "register_evaluator",
            "proposal": null
        }
    })
}

#[test]
fn capsule_capability_permissions_are_closed_and_effect_coherent() {
    let root = package_root();
    let schemas = package_schemas(&root);
    let capsule_schema = schema_id(&root, "skill-capsule.schema.json");
    let capsule = load_json(&root.join("fixtures/positive/non-promotable-skill-capsule.json"));

    assert!(
        schemas
            .validate(&capsule_schema, &capsule, "jpd-capsule")
            .is_empty(),
        "a capsule must accept explicit permissions for every required capability"
    );

    let mut unknown_permission = capsule.clone();
    unknown_permission["requiredCapabilities"][0]["permissions"] =
        serde_json::json!(["filesystem.everything"]);
    assert!(
        !schemas
            .validate(&capsule_schema, &unknown_permission, "jpd-capsule")
            .is_empty(),
        "capsule capabilities must reject permissions outside the closed vocabulary"
    );

    let mut missing_runtime_read = capsule.clone();
    missing_runtime_read["requiredCapabilities"][1]["permissions"] =
        serde_json::json!(["network.loopback", "token.reference.read"]);
    assert!(
        !schemas
            .validate(&capsule_schema, &missing_runtime_read, "jpd-capsule")
            .is_empty(),
        "runtime_read effects must require runtime.read permission"
    );

    let mut mutation_permission_on_read_only = capsule;
    mutation_permission_on_read_only["requiredCapabilities"][0]["permissions"] =
        serde_json::json!(["package.read", "runtime.write"]);
    assert!(
        !schemas
            .validate(
                &capsule_schema,
                &mutation_permission_on_read_only,
                "jpd-capsule",
            )
            .is_empty(),
        "a read-only capability cannot smuggle a mutation permission"
    );
}

#[test]
fn capsule_v0_1_rejects_self_promotion_and_publication() {
    let root = package_root();
    let schemas = package_schemas(&root);
    let capsule_schema = schema_id(&root, "skill-capsule.schema.json");
    let capsule = load_json(&root.join("fixtures/positive/non-promotable-skill-capsule.json"));
    assert!(
        schemas
            .validate(&capsule_schema, &capsule, "jpd-capsule")
            .is_empty()
    );

    let mut proposed = capsule.clone();
    proposed["status"] = serde_json::json!("promotion_proposed");
    proposed["promotion"] = serde_json::json!({
        "eligible": true,
        "distinctSuccessfulRuns": 3,
        "minimumDistinctRuns": 3,
        "evidenceFresh": true,
        "tokenBudgetWithinLimit": true,
        "unresolvedSevereClaims": 0,
        "reasons": []
    });
    assert!(
        !schemas
            .validate(&capsule_schema, &proposed, "jpd-capsule")
            .is_empty(),
        "the task-local synthesizer cannot author a promotion_proposed Project Skill"
    );

    let mut project_scoped = capsule.clone();
    project_scoped["scope"] = serde_json::json!("project");
    assert!(
        !schemas
            .validate(&capsule_schema, &project_scoped, "jpd-capsule")
            .is_empty(),
        "the v0.1 capsule wire cannot claim Project Skill scope"
    );

    let mut published = capsule;
    published["scope"] = serde_json::json!("project");
    published["status"] = serde_json::json!("published");
    published["promotion"] = serde_json::json!({
        "eligible": true,
        "distinctSuccessfulRuns": 3,
        "minimumDistinctRuns": 3,
        "evidenceFresh": true,
        "tokenBudgetWithinLimit": true,
        "unresolvedSevereClaims": 0,
        "reasons": []
    });
    published["governance"]["publication"] = serde_json::json!({
        "proposalId": "proposal/issue-210/forged",
        "graphVersion": 2,
        "actorId": "agent/skill-synthesizer",
        "publishedAt": "2026-08-22T16:00:00Z"
    });
    assert!(
        !schemas
            .validate(&capsule_schema, &published, "jpd-capsule")
            .is_empty(),
        "the task-local synthesizer cannot self-publish a Project Skill"
    );
}

#[test]
fn skill_evaluation_v0_1_rejects_forged_evaluator_and_eligibility() {
    let root = package_root();
    let schemas = package_schemas(&root);
    let evaluation_schema = schema_id(&root, "skill-evaluation.schema.json");
    let evaluation = advisory_evaluation();
    assert!(
        schemas
            .validate(&evaluation_schema, &evaluation, "jpd-evaluation")
            .is_empty(),
        "the data-only package needs an honest advisory evaluation shape"
    );

    let mut forged = evaluation;
    forged["classification"] = serde_json::json!({
        "status": "evaluated",
        "evaluator": {
            "evaluatorId": "agent/self-asserted-evaluator",
            "evaluatorVersion": "1.0.0",
            "inputDigest": format!("sha256:{}", "a".repeat(64)),
            "receipt": {
                "evidenceId": "evidence.self-asserted-evaluation",
                "contentSha256": "b".repeat(64),
                "ciphertextSha256": "c".repeat(64)
            }
        }
    });
    forged["promotionDecision"] = serde_json::json!({
        "status": "evaluated",
        "eligible": true,
        "reasons": [],
        "requiredAction": "governor_review",
        "proposal": {
            "proposalId": "proposal/issue-210/forged",
            "targetScope": "project",
            "requestedAuthority": "graph_governor"
        }
    });
    assert!(
        !schemas
            .validate(&evaluation_schema, &forged, "jpd-evaluation")
            .is_empty(),
        "an agent-authored receipt cannot make the v0.1 evaluation eligible"
    );
}
