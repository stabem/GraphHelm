use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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

fn scoped_test_waiver(
    verification_id: &str,
    blocker_kind: &str,
    blocker_id: &str,
    reference_id: &str,
    digest_byte: &str,
) -> Value {
    let raw_digest = digest_byte.repeat(32);
    let digest = format!("sha256:{raw_digest}");
    serde_json::json!({
        "waiverRef": {
            "referenceId": reference_id,
            "contentDigest": digest
        },
        "waiverSchema": "https://p50.dev/schemas/policy-waiver.schema.json",
        "reasonPresent": true,
        "coverage": {
            "verificationId": verification_id,
            "blockerKind": blocker_kind,
            "blockerId": blocker_id,
            "blockerDigest": digest
        },
        "validation": {
            "evaluatorId": "graphhelm/policy-waiver-resolver",
            "evaluatorVersion": "1.0.0",
            "inputDigest": digest,
            "evaluatedAt": "2026-08-22T16:19:59Z",
            "effectiveThrough": "2026-08-23T16:20:00Z",
            "activeAtVerification": true,
            "receipt": {
                "evidenceId": format!("evidence.waiver-validation.{blocker_kind}"),
                "contentSha256": raw_digest,
                "ciphertextSha256": digest_byte.repeat(32)
            }
        }
    })
}

fn trust_compatibility_candidate() -> Value {
    serde_json::json!({
        "authority": "candidate",
        "latticeBinding": {
            "latticeId": "graphhelm-jpd/evidence-strength-lattice",
            "latticeVersion": "1.0.0",
            "latticeDigest": "sha256:d1aa1ccfd7fa32be662230098cc1c513fd9edee79c569b2e0ca8aa383c6c5531"
        },
        "relationId": "graphhelm-jpd/trust-compatibility",
        "relationVersion": "1.0.0",
        "decision": "compatible_candidate",
        "inputDigest": "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        "receipt": {
            "evidenceId": "evidence.trust-compatibility-candidate",
            "contentSha256": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
            "ciphertextSha256": "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
        }
    })
}

#[test]
fn built_in_jpd_extension_is_a_closed_digest_bound_package() {
    let output = Command::new(env!("CARGO_BIN_EXE_graphhelm"))
        .args(["extension", "validate"])
        .arg(package_root())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(output.stderr.is_empty());

    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["ok"], true);
    assert_eq!(result["data"]["id"], "graphhelm-jpd");
    assert_eq!(result["data"]["version"], "0.1.0");
    assert_eq!(result["data"]["contributionCount"], 53);
    assert!(
        result["data"]["packageDigest"]
            .as_str()
            .unwrap()
            .starts_with("sha256:")
    );

    let manifest = load_json(&package_root().join("extension.json"));
    let contracts = &manifest["spec"]["contracts"];
    assert_eq!(
        contracts["artifactFlowFormat"],
        "p50.dev/jpd/artifact-flow/v1"
    );
    let entry_families = contracts["entryFamilies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect::<BTreeSet<_>>();
    let flows = contracts["artifactFlows"].as_array().unwrap();
    assert_eq!(flows.len(), 8);
    let flow_families = flows
        .iter()
        .map(|flow| {
            assert!(!flow["inputs"].as_array().unwrap().is_empty());
            assert!(!flow["outputs"].as_array().unwrap().is_empty());
            flow["family"].as_str().unwrap()
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(flow_families, entry_families);
}

#[test]
fn digest_bound_text_resources_pin_lf_checkout_bytes() {
    let root = repository_root();
    let manifest = load_json(&package_root().join("extension.json"));
    let paths = manifest["spec"]["contracts"]["contributions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|contribution| contribution["path"].as_str().unwrap())
        .filter(|path| {
            [".json", ".md", ".yaml", ".yml"]
                .iter()
                .any(|extension| path.ends_with(extension))
        })
        .map(|path| format!("extensions/builtin/graphhelm-jpd/{path}"))
        .collect::<Vec<_>>();

    let output = Command::new("git")
        .current_dir(root)
        .args(["check-attr", "eol", "--"])
        .args(&paths)
        .output()
        .unwrap();
    assert!(output.status.success(), "git check-attr failed");
    assert!(output.stderr.is_empty());

    let attributes = String::from_utf8(output.stdout).unwrap();
    let lines = attributes.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), paths.len());
    for (path, line) in paths.iter().zip(lines) {
        assert_eq!(line, format!("{path}: eol: lf"));
    }
}

#[test]
fn recovery_operator_requires_a_bound_owner_decision_for_immediate_pause_and_cancel() {
    let agent = load_json(&package_root().join("agents/recovery-operator.json"));
    let prohibited = agent["prohibitedActions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect::<BTreeSet<_>>();
    assert!(prohibited.contains("execution.immediate_pause_without_explicit_owner_decision"));
    assert!(prohibited.contains("execution.cancel_without_explicit_owner_decision"));

    let instructions = agent["instructions"].as_str().unwrap();
    assert!(instructions.contains("`tool:pause` with `mode: immediate` or `tool:cancel`"));
    assert!(instructions.contains("bound to the active execution and requested action"));

    let requirements = agent["completionContract"]["requires"].as_array().unwrap();
    assert!(requirements.iter().any(|value| {
        value == "explicit_bound_owner_decision_for_immediate_pause_cancel_or_continue"
    }));
}

#[test]
fn jpd_contract_fixtures_pin_both_acceptance_and_refusal() {
    let root = package_root();
    let schemas = package_schemas(&root);
    let positive = [
        (
            "journey-verification-first-pass.json",
            "journey-verification-result.schema.json",
        ),
        (
            "journey-verification-accepted-with-waiver.json",
            "journey-verification-result.schema.json",
        ),
        (
            "missing-observer-refusal.json",
            "observation-obligation.schema.json",
        ),
        (
            "non-promotable-skill-capsule.json",
            "skill-capsule.schema.json",
        ),
        ("recovered-retry-chain.json", "retry-chain.schema.json"),
        (
            "semantic-journey-defect-claim.json",
            "journey-defect-claim.schema.json",
        ),
    ];
    for (fixture, schema) in positive {
        let value = load_json(&root.join("fixtures/positive").join(fixture));
        let diagnostics = schemas.validate(&schema_id(&root, schema), &value, "jpd-fixture");
        assert!(diagnostics.is_empty(), "{fixture}: {diagnostics:?}");
    }

    let negative = [
        ("invalid-retry-lineage.json", "retry-chain.schema.json"),
        (
            "journey-verification-missing-gate-claimed-success.json",
            "journey-verification-result.schema.json",
        ),
    ];
    for (fixture, schema) in negative {
        let value = load_json(&root.join("fixtures/negative").join(fixture));
        let diagnostics = schemas.validate(&schema_id(&root, schema), &value, "jpd-fixture");
        assert!(!diagnostics.is_empty(), "{fixture} must be refused");
    }

    let verification_schema = schema_id(&root, "journey-verification-result.schema.json");
    let mut uncovered_gate =
        load_json(&root.join("fixtures/positive/journey-verification-accepted-with-waiver.json"));
    uncovered_gate["gate"]
        .as_object_mut()
        .unwrap()
        .remove("waiver");
    assert!(
        !schemas
            .validate(&verification_schema, &uncovered_gate, "jpd-fixture")
            .is_empty(),
        "a waiver on another blocker cannot cover a rejected gate"
    );

    let retry_schema = schema_id(&root, "retry-chain.schema.json");
    let mut missing_classifier =
        load_json(&root.join("fixtures/positive/recovered-retry-chain.json"));
    let retry_input_schema = schema_id(&root, "retry-chain-input.schema.json");
    let retry_lineage_input_schema = schema_id(&root, "retry-lineage-input.schema.json");
    let mut unclassified_input = missing_classifier.clone();
    let unclassified_object = unclassified_input.as_object_mut().unwrap();
    unclassified_object.remove("outcomeClass");
    unclassified_object.remove("classification");
    unclassified_object.remove("classificationBasis");
    assert!(
        schemas
            .validate(&retry_input_schema, &unclassified_input, "jpd-fixture")
            .is_empty(),
        "the evaluator input must validate without any pre-filled classification"
    );
    assert!(
        !schemas
            .validate(&retry_input_schema, &missing_classifier, "jpd-fixture")
            .is_empty(),
        "the evaluator input must reject a self-asserted classification"
    );
    let mut raw_lineage_input = unclassified_input.clone();
    raw_lineage_input
        .as_object_mut()
        .unwrap()
        .remove("lineageValidation");
    assert!(
        schemas
            .validate(
                &retry_lineage_input_schema,
                &raw_lineage_input,
                "jpd-fixture",
            )
            .is_empty(),
        "the lineage evaluator must receive raw input without its own receipt"
    );

    let mut wrong_chain_classifier = missing_classifier.clone();
    wrong_chain_classifier["classification"]["evaluator"]["evaluatorId"] =
        serde_json::json!("unregistered/retry-classifier");
    assert!(
        !schemas
            .validate(&retry_schema, &wrong_chain_classifier, "jpd-fixture")
            .is_empty(),
        "a classified retry chain must name the registered retry classifier"
    );
    let mut recovered_without_success = missing_classifier.clone();
    recovered_without_success["retries"][0]["result"] = serde_json::json!("failed");
    assert!(
        !schemas
            .validate(&retry_schema, &recovered_without_success, "jpd-fixture")
            .is_empty(),
        "recovered_success requires a successful later attempt"
    );
    let mut recovered_without_material_delta = missing_classifier.clone();
    recovered_without_material_delta["retries"][0]["evidenceDelta"]["materialChange"] =
        serde_json::json!(false);
    assert!(
        !schemas
            .validate(
                &retry_schema,
                &recovered_without_material_delta,
                "jpd-fixture",
            )
            .is_empty(),
        "recovered_success requires a material evidence delta"
    );
    let mut chain_with_invalid_lineage = missing_classifier.clone();
    chain_with_invalid_lineage["lineageValidation"]["result"] = serde_json::json!("invalid");
    assert!(
        !schemas
            .validate(&retry_schema, &chain_with_invalid_lineage, "jpd-fixture")
            .is_empty(),
        "classification may consume only a valid lineage result"
    );

    missing_classifier["outcomeClass"] = Value::Null;
    missing_classifier["classificationBasis"] = serde_json::json!([]);
    missing_classifier["classification"] = serde_json::json!({
        "status": "capability_missing",
        "refusal": {
            "code": "RETRY_CLASSIFIER_MISSING",
            "missingCapability": "graphhelm-jpd/retry-classifier",
            "reason": "No registered deterministic retry classifier is available."
        }
    });
    assert!(
        schemas
            .validate(&retry_schema, &missing_classifier, "jpd-fixture")
            .is_empty(),
        "missing evaluator must have an honest schema-valid unresolved representation"
    );
    let mut erased_first_failure = missing_classifier.clone();
    erased_first_failure["firstFailure"] = Value::Null;
    assert!(
        !schemas
            .validate(&retry_schema, &erased_first_failure, "jpd-fixture")
            .is_empty(),
        "a missing classifier can never make a failed root's first failure waivable or erasable"
    );

    let mut unresolved_verification =
        load_json(&root.join("fixtures/positive/journey-verification-first-pass.json"));
    unresolved_verification["retry"]["outcomeClassification"] = Value::Null;
    unresolved_verification["retry"]["classification"] = serde_json::json!({
        "status": "capability_missing",
        "refusal": {
            "code": "RETRY_CLASSIFIER_MISSING",
            "missingCapability": "graphhelm-jpd/retry-classifier",
            "reason": "No registered deterministic retry classifier is available."
        }
    });
    unresolved_verification["proposedResultStatus"] = serde_json::json!("unresolved");
    assert!(
        schemas
            .validate(
                &verification_schema,
                &unresolved_verification,
                "jpd-fixture",
            )
            .is_empty(),
        "a missing retry evaluator must remain unresolved without a waiver"
    );

    let accepted_with_waiver =
        load_json(&root.join("fixtures/positive/journey-verification-accepted-with-waiver.json"));
    let fixture_waiver_refs = [
        &accepted_with_waiver["obligations"][0]["waiver"]["waiverRef"]["referenceId"],
        &accepted_with_waiver["disagreements"][0]["waiver"]["waiverRef"]["referenceId"],
        &accepted_with_waiver["gate"]["waiver"]["waiverRef"]["referenceId"],
    ]
    .into_iter()
    .map(|value| value.as_str().unwrap())
    .collect::<BTreeSet<_>>();
    assert_eq!(
        fixture_waiver_refs.len(),
        3,
        "each fixture blocker must carry a distinct waiver artifact"
    );
    let gate_waiver = accepted_with_waiver["gate"]["waiver"].clone();
    let retry_waiver = scoped_test_waiver(
        "verification/issue-210/first-pass",
        "retry",
        "retry/retry-classifier",
        "policy-waiver.issue-210.retry-classifier",
        "82",
    );
    let mut proven_with_retry_waiver =
        load_json(&root.join("fixtures/positive/journey-verification-first-pass.json"));
    proven_with_retry_waiver["retry"]["waiver"] = retry_waiver.clone();
    assert!(
        !schemas
            .validate(
                &verification_schema,
                &proven_with_retry_waiver,
                "jpd-fixture",
            )
            .is_empty(),
        "a proven result cannot carry any retry waiver"
    );

    unresolved_verification["proposedResultStatus"] = serde_json::json!("accepted_with_waiver");
    assert!(
        !schemas
            .validate(
                &verification_schema,
                &unresolved_verification,
                "jpd-fixture",
            )
            .is_empty(),
        "a waiver on another blocker cannot cover a missing retry classifier"
    );
    unresolved_verification["retry"]["waiver"] = gate_waiver.clone();
    assert!(
        !schemas
            .validate(
                &verification_schema,
                &unresolved_verification,
                "jpd-fixture",
            )
            .is_empty(),
        "a gate-scoped waiver cannot be copied onto a retry blocker"
    );
    unresolved_verification["retry"]["waiver"] = retry_waiver.clone();
    assert!(
        schemas
            .validate(
                &verification_schema,
                &unresolved_verification,
                "jpd-fixture",
            )
            .is_empty(),
        "a retry-local owner waiver may authorize continuation without inventing a classification"
    );

    let mut flaky_verification =
        load_json(&root.join("fixtures/positive/journey-verification-first-pass.json"));
    flaky_verification["retry"]["outcomeClassification"] = serde_json::json!("flaky_pass");
    flaky_verification["retry"]["firstFailurePreserved"] = serde_json::json!(true);
    assert!(
        !schemas
            .validate(&verification_schema, &flaky_verification, "jpd-fixture")
            .is_empty(),
        "a flaky pass can never be JPD-proven"
    );
    flaky_verification["proposedResultStatus"] = serde_json::json!("accepted_with_waiver");
    assert!(
        !schemas
            .validate(&verification_schema, &flaky_verification, "jpd-fixture")
            .is_empty(),
        "a flaky pass needs a retry-local waiver"
    );
    flaky_verification["retry"]["waiver"] = retry_waiver;
    assert!(
        schemas
            .validate(&verification_schema, &flaky_verification, "jpd-fixture")
            .is_empty(),
        "a retry-local waiver preserves flaky_pass while authorizing continuation"
    );

    let mut proven_with_observer_waiver =
        load_json(&root.join("fixtures/positive/journey-verification-first-pass.json"));
    let observer_waiver = scoped_test_waiver(
        "verification/issue-210/first-pass",
        "observer",
        "graphhelm-cli-graph-contract",
        "policy-waiver.issue-210.observer",
        "83",
    );
    proven_with_observer_waiver["bindings"]["observers"][0]["waiver"] = observer_waiver.clone();
    assert!(
        !schemas
            .validate(
                &verification_schema,
                &proven_with_observer_waiver,
                "jpd-fixture",
            )
            .is_empty(),
        "a proven result cannot carry an observer waiver"
    );

    let mut structural_gate_with_waiver = accepted_with_waiver.clone();
    structural_gate_with_waiver["gate"]["failureKind"] =
        serde_json::json!("structural_impossibility");
    assert!(
        !schemas
            .validate(
                &verification_schema,
                &structural_gate_with_waiver,
                "jpd-fixture",
            )
            .is_empty(),
        "structural impossibility can never be waived"
    );

    let mut stale_observer_without_local_waiver = accepted_with_waiver.clone();
    stale_observer_without_local_waiver["bindings"]["observers"][0]["evidenceFresh"] =
        serde_json::json!(false);
    assert!(
        !schemas
            .validate(
                &verification_schema,
                &stale_observer_without_local_waiver,
                "jpd-fixture",
            )
            .is_empty(),
        "a gate waiver cannot silently cover stale observer evidence"
    );
    stale_observer_without_local_waiver["bindings"]["observers"][0]["waiver"] = gate_waiver.clone();
    assert!(
        !schemas
            .validate(
                &verification_schema,
                &stale_observer_without_local_waiver,
                "jpd-fixture",
            )
            .is_empty(),
        "a gate-scoped waiver cannot be copied onto a stale observer"
    );
    stale_observer_without_local_waiver["bindings"]["observers"][0]["waiver"] = observer_waiver;
    assert!(
        schemas
            .validate(
                &verification_schema,
                &stale_observer_without_local_waiver,
                "jpd-fixture",
            )
            .is_empty(),
        "an observer-local waiver may authorize accepted_with_waiver without relabeling proof"
    );

    let mut gate_waiver_on_obligation = accepted_with_waiver.clone();
    gate_waiver_on_obligation["obligations"][0]["waiver"] = gate_waiver.clone();
    assert!(
        !schemas
            .validate(
                &verification_schema,
                &gate_waiver_on_obligation,
                "jpd-fixture",
            )
            .is_empty(),
        "a gate-scoped waiver cannot be copied onto an obligation"
    );

    let mut gate_waiver_on_disagreement = accepted_with_waiver.clone();
    gate_waiver_on_disagreement["disagreements"][0]["waiver"] = gate_waiver;
    assert!(
        !schemas
            .validate(
                &verification_schema,
                &gate_waiver_on_disagreement,
                "jpd-fixture",
            )
            .is_empty(),
        "a gate-scoped waiver cannot be copied onto a disagreement"
    );

    let mut canonical_graph_id =
        load_json(&root.join("fixtures/positive/journey-verification-first-pass.json"));
    canonical_graph_id["bindings"]["graph"]["graphId"] = serde_json::json!("GraphHelm@Project#V1");
    assert!(
        schemas
            .validate(&verification_schema, &canonical_graph_id, "jpd-fixture")
            .is_empty(),
        "graph bindings must accept the canonical opaque Graph ID wire shape"
    );
    canonical_graph_id["bindings"]["graph"]["version"] =
        serde_json::json!(9_007_199_254_740_992_u64);
    assert!(
        !schemas
            .validate(&verification_schema, &canonical_graph_id, "jpd-fixture")
            .is_empty(),
        "graph bindings must reject versions outside the canonical GraphVersion range"
    );

    let mut missing_candidate_authority =
        load_json(&root.join("fixtures/positive/journey-verification-first-pass.json"));
    missing_candidate_authority
        .as_object_mut()
        .unwrap()
        .remove("authority");
    assert!(
        !schemas
            .validate(
                &verification_schema,
                &missing_candidate_authority,
                "jpd-fixture",
            )
            .is_empty(),
        "a JVR must disclose that v0.1.0 produces only an unvalidated candidate"
    );

    let mut forged_validator_authority =
        load_json(&root.join("fixtures/positive/journey-verification-first-pass.json"));
    forged_validator_authority["authority"]["status"] = serde_json::json!("validated");
    assert!(
        !schemas
            .validate(
                &verification_schema,
                &forged_validator_authority,
                "jpd-fixture",
            )
            .is_empty(),
        "the candidate schema must not offer a fake validated authority branch"
    );

    let mut missing_council_binding =
        load_json(&root.join("fixtures/positive/journey-verification-first-pass.json"));
    missing_council_binding["bindings"]
        .as_object_mut()
        .unwrap()
        .remove("council");
    assert!(
        !schemas
            .validate(
                &verification_schema,
                &missing_council_binding,
                "jpd-fixture",
            )
            .is_empty(),
        "a JVR candidate must bind the council result and complete dissent inventory"
    );

    let mut wrong_retry_classifier =
        load_json(&root.join("fixtures/positive/journey-verification-first-pass.json"));
    wrong_retry_classifier["retry"]["classification"]["evaluator"]["evaluatorId"] =
        serde_json::json!("unregistered/retry-classifier");
    assert!(
        !schemas
            .validate(&verification_schema, &wrong_retry_classifier, "jpd-fixture",)
            .is_empty(),
        "an evaluated retry classification must name the registered deterministic evaluator"
    );

    let mut invalid_retry_lineage =
        load_json(&root.join("fixtures/positive/journey-verification-first-pass.json"));
    invalid_retry_lineage["retry"]["lineageValidation"]["result"] = serde_json::json!("invalid");
    assert!(
        !schemas
            .validate(&verification_schema, &invalid_retry_lineage, "jpd-fixture",)
            .is_empty(),
        "a JVR can consume only a retry lineage receipt whose result is valid"
    );

    let mut unsupported_resolution =
        load_json(&root.join("fixtures/positive/journey-verification-first-pass.json"));
    unsupported_resolution["disagreements"] = serde_json::json!([{
        "disagreementId": "disagreement/unsupported",
        "status": "resolved",
        "resolution": "Agents agree."
    }]);
    assert!(
        !schemas
            .validate(&verification_schema, &unsupported_resolution, "jpd-fixture")
            .is_empty(),
        "agent agreement cannot resolve a disagreement without evidence and evaluator authority"
    );
    unsupported_resolution["disagreements"][0]["evidenceRefs"] =
        serde_json::json!([unsupported_resolution["gate"]["evaluator"]["receipt"].clone()]);
    unsupported_resolution["disagreements"][0]["resolutionEvaluation"] =
        unsupported_resolution["gate"]["evaluator"].clone();
    assert!(
        schemas
            .validate(&verification_schema, &unsupported_resolution, "jpd-fixture")
            .is_empty(),
        "digest-bound evidence plus a registered evaluator receipt may resolve a disagreement"
    );

    let mut missing_gate = load_json(
        &root.join("fixtures/negative/journey-verification-missing-gate-claimed-success.json"),
    );
    missing_gate["proposedResultStatus"] = serde_json::json!("unresolved");
    assert!(
        schemas
            .validate(&verification_schema, &missing_gate, "jpd-fixture")
            .is_empty(),
        "a missing gate capability must have an honest unresolved representation"
    );
}

#[test]
fn jpd_authority_and_provenance_cannot_be_self_asserted() {
    let root = package_root();
    let schemas = package_schemas(&root);

    let evaluation_schema = schema_id(&root, "skill-evaluation.schema.json");
    let mut missing_evaluator = serde_json::json!({
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
    });
    assert!(
        schemas
            .validate(&evaluation_schema, &missing_evaluator, "jpd-fixture")
            .is_empty(),
        "missing skill evaluator must have a schema-valid advisory result"
    );
    missing_evaluator["promotionDecision"]["eligible"] = serde_json::json!(true);
    missing_evaluator["promotionDecision"]["reasons"] = serde_json::json!([]);
    missing_evaluator["promotionDecision"]["requiredAction"] = serde_json::json!("governor_review");
    missing_evaluator["promotionDecision"]["proposal"] = serde_json::json!({
        "proposalId": "proposal/issue-210/forged",
        "targetScope": "project",
        "requestedAuthority": "graph_governor"
    });
    assert!(
        !schemas
            .validate(&evaluation_schema, &missing_evaluator, "jpd-fixture")
            .is_empty(),
        "an advisory result cannot self-assert promotion eligibility"
    );

    let observation_schema = schema_id(&root, "observation-obligation.schema.json");
    let mut matched = load_json(&root.join("fixtures/positive/missing-observer-refusal.json"));
    matched["resolution"] = serde_json::json!({
        "status": "matched",
        "catalogBinding": {
            "kind": "observer_catalog",
            "catalogId": "graphhelm-jpd/observer-catalog",
            "catalogVersion": "1.0.0",
            "catalogDigest": format!("sha256:{}", "1".repeat(64))
        },
        "capabilityBinding": {
            "kind": "observer_capability",
            "capabilityId": "browser.semantic-journey",
            "observerId": "graphhelm-browser-user-journey",
            "observerVersion": "1.0.0",
            "capabilityDigest": format!("sha256:{}", "2".repeat(64))
        },
        "configurationDigest": format!("sha256:{}", "3".repeat(64)),
        "environmentDigest": format!("sha256:{}", "4".repeat(64)),
        "observedTrust": "first_party_instrumented",
        "trustCompatibilityCandidate": trust_compatibility_candidate(),
        "capabilityReceipt": {
            "capturedAtUnixSeconds": 1_777_000_000,
            "receipt": {
                "evidenceId": "evidence.observer-capability",
                "contentSha256": "5".repeat(64),
                "ciphertextSha256": "6".repeat(64)
            }
        },
        "matchedEvidence": [
            {
                "evidenceKind": "dom_semantic_snapshot",
                "capturedAtUnixSeconds": 1_777_000_001,
                "receipt": {
                    "evidenceId": "evidence.dom-semantic-snapshot",
                    "contentSha256": "7".repeat(64),
                    "ciphertextSha256": "8".repeat(64)
                }
            },
            {
                "evidenceKind": "accessibility_tree",
                "capturedAtUnixSeconds": 1_777_000_002,
                "receipt": {
                    "evidenceId": "evidence.accessibility-tree",
                    "contentSha256": "9".repeat(64),
                    "ciphertextSha256": "a".repeat(64)
                }
            }
        ],
        "freshnessEvaluation": {
            "clock": "unix_seconds",
            "evaluatedAtUnixSeconds": 1_777_000_005,
            "maximumObservedAgeSeconds": 5,
            "requiredMaxAgeSeconds": 30,
            "requiredAbsenceWindowSeconds": null,
            "absenceWindow": null,
            "result": "satisfied"
        },
        "evidenceMatchEvaluation": {
            "evaluatorId": "graphhelm-jpd/evidence-matcher",
            "evaluatorVersion": "1.0.0",
            "evaluationSemantics": "registered_deterministic",
            "inputDigest": format!("sha256:{}", "b".repeat(64)),
            "receipt": {
                "evidenceId": "evidence.evidence-match-evaluation",
                "contentSha256": "c".repeat(64),
                "ciphertextSha256": "d".repeat(64)
            }
        }
    });
    assert!(
        schemas
            .validate(&observation_schema, &matched, "jpd-fixture")
            .is_empty(),
        "an evidence match with two distinct receipts must validate"
    );
    let mut partial_match = matched.clone();
    partial_match["resolution"]["matchedEvidence"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert!(
        !schemas
            .validate(&observation_schema, &partial_match, "jpd-fixture")
            .is_empty(),
        "matched evidence must cover every evidence kind required by the obligation"
    );
    matched["resolution"]
        .as_object_mut()
        .unwrap()
        .remove("evidenceMatchEvaluation");
    assert!(
        !schemas
            .validate(&observation_schema, &matched, "jpd-fixture")
            .is_empty(),
        "an observer receipt alone cannot self-assert evidence adequacy"
    );

    let capsule_schema = schema_id(&root, "skill-capsule.schema.json");
    let capsule = load_json(&root.join("fixtures/positive/non-promotable-skill-capsule.json"));
    let mut canonical_capsule_graph_id = capsule.clone();
    canonical_capsule_graph_id["source"]["bindings"]["graph"]["graphId"] =
        serde_json::json!("GraphHelm@Project#V1");
    assert!(
        schemas
            .validate(&capsule_schema, &canonical_capsule_graph_id, "jpd-fixture",)
            .is_empty(),
        "capsule source bindings must accept the canonical opaque Graph ID wire shape"
    );
    canonical_capsule_graph_id["source"]["bindings"]["graph"]["version"] =
        serde_json::json!(9_007_199_254_740_992_u64);
    assert!(
        !schemas
            .validate(&capsule_schema, &canonical_capsule_graph_id, "jpd-fixture")
            .is_empty(),
        "capsule graph bindings must reject versions outside the canonical GraphVersion range"
    );
    let mut unbound_capsule = capsule.clone();
    unbound_capsule["source"]
        .as_object_mut()
        .unwrap()
        .remove("sourceJourney");
    assert!(
        !schemas
            .validate(&capsule_schema, &unbound_capsule, "jpd-fixture")
            .is_empty(),
        "a generated capsule must remain bound to its source journey"
    );
    let mut unversioned_capability = capsule;
    unversioned_capability["requiredCapabilities"][0]
        .as_object_mut()
        .unwrap()
        .remove("capabilityVersion");
    assert!(
        !schemas
            .validate(&capsule_schema, &unversioned_capability, "jpd-fixture")
            .is_empty(),
        "a generated capsule cannot request an unversioned capability"
    );

    let defect_schema = schema_id(&root, "journey-defect-claim.schema.json");
    let mut contradictory_review =
        load_json(&root.join("fixtures/positive/semantic-journey-defect-claim.json"));
    contradictory_review["review"]["result"] = serde_json::json!("falsified");
    assert!(
        !schemas
            .validate(&defect_schema, &contradictory_review, "jpd-fixture")
            .is_empty(),
        "the independent review result must match the defect lifecycle status"
    );
}

#[test]
fn jpd_council_result_binds_complete_claim_and_dissent_inventories() {
    let root = package_root();
    let schemas = package_schemas(&root);
    let council_schema = schema_id(&root, "council-result.schema.json");
    let verification_schema = schema_id(&root, "journey-verification-result.schema.json");

    let executed =
        load_json(&root.join("fixtures/positive/journey-verification-accepted-with-waiver.json"));
    let council_result = executed["bindings"]["council"]["result"].clone();
    assert!(
        schemas
            .validate(&council_schema, &council_result, "jpd-fixture")
            .is_empty(),
        "the embedded executed council result must satisfy its typed candidate contract"
    );
    assert!(
        schemas
            .validate(&verification_schema, &executed, "jpd-fixture")
            .is_empty(),
        "an executed JVR must bind the complete council result"
    );

    let mut missing_claim_completeness = executed.clone();
    missing_claim_completeness["bindings"]["council"]["result"]["claims"]
        .as_object_mut()
        .unwrap()
        .remove("completeness");
    assert!(
        !schemas
            .validate(
                &verification_schema,
                &missing_claim_completeness,
                "jpd-fixture",
            )
            .is_empty(),
        "an executed JVR cannot omit the claim inventory completeness binding"
    );

    let mut missing_dissent_completeness = executed.clone();
    missing_dissent_completeness["bindings"]["council"]["result"]["dissent"]
        .as_object_mut()
        .unwrap()
        .remove("completeness");
    assert!(
        !schemas
            .validate(
                &verification_schema,
                &missing_dissent_completeness,
                "jpd-fixture",
            )
            .is_empty(),
        "an executed JVR cannot omit the dissent inventory completeness binding"
    );

    let mut missing_embedded_result = executed;
    missing_embedded_result["bindings"]["council"]
        .as_object_mut()
        .unwrap()
        .remove("result");
    assert!(
        !schemas
            .validate(
                &verification_schema,
                &missing_embedded_result,
                "jpd-fixture",
            )
            .is_empty(),
        "an executed JVR cannot cite inventory digests without the bound typed result"
    );

    for digest_name in ["claimInventoryDigest", "dissentInventoryDigest"] {
        let mut missing_inventory_digest = load_json(
            &root.join("fixtures/positive/journey-verification-accepted-with-waiver.json"),
        );
        missing_inventory_digest["bindings"]["council"]
            .as_object_mut()
            .unwrap()
            .remove(digest_name);
        assert!(
            !schemas
                .validate(
                    &verification_schema,
                    &missing_inventory_digest,
                    "jpd-fixture",
                )
                .is_empty(),
            "an executed JVR must bind {digest_name}"
        );
    }

    let mut missing_binding_validation =
        load_json(&root.join("fixtures/positive/journey-verification-accepted-with-waiver.json"));
    missing_binding_validation["bindings"]["council"]
        .as_object_mut()
        .unwrap()
        .remove("bindingValidation");
    assert!(
        !schemas
            .validate(
                &verification_schema,
                &missing_binding_validation,
                "jpd-fixture",
            )
            .is_empty(),
        "schema-only binding must disclose that digest recomputation is unavailable"
    );
}

#[test]
fn jpd_direct_tier_council_binding_is_closed_and_unambiguous() {
    let root = package_root();
    let schemas = package_schemas(&root);
    let verification_schema = schema_id(&root, "journey-verification-result.schema.json");
    let direct = load_json(&root.join("fixtures/positive/journey-verification-first-pass.json"));
    assert!(
        schemas
            .validate(&verification_schema, &direct, "jpd-fixture")
            .is_empty(),
        "a direct-tier JVR may explicitly mark the council not applicable"
    );

    let mut wrong_reason = direct.clone();
    wrong_reason["bindings"]["council"]["reason"] = serde_json::json!("low_risk");
    assert!(
        !schemas
            .validate(&verification_schema, &wrong_reason, "jpd-fixture")
            .is_empty(),
        "the direct-tier branch must use the stable direct_tier reason"
    );

    let mut mixed_branch = direct;
    mixed_branch["bindings"]["council"]["resultRef"] = serde_json::json!({
        "referenceId": "council-result.issue-210.illegal-direct",
        "contentDigest": format!("sha256:{}", "9".repeat(64))
    });
    assert!(
        !schemas
            .validate(&verification_schema, &mixed_branch, "jpd-fixture")
            .is_empty(),
        "a direct-tier not_applicable binding cannot also masquerade as an executed result"
    );
}

#[test]
fn jpd_proven_result_rejects_blocked_or_unresolved_executed_council() {
    let root = package_root();
    let schemas = package_schemas(&root);
    let verification_schema = schema_id(&root, "journey-verification-result.schema.json");
    let mut proven =
        load_json(&root.join("fixtures/positive/journey-verification-first-pass.json"));
    let executed =
        load_json(&root.join("fixtures/positive/journey-verification-accepted-with-waiver.json"));

    proven["bindings"]["council"] = executed["bindings"]["council"].clone();
    assert!(
        !schemas
            .validate(&verification_schema, &proven, "jpd-fixture")
            .is_empty(),
        "a proven result cannot carry unresolved or accepted-risk council dissent"
    );

    proven["bindings"]["council"]["result"]["status"] = serde_json::json!("blocked");
    proven["bindings"]["council"]["result"]["decision"]["status"] = serde_json::json!("blocked");
    assert!(
        !schemas
            .validate(&verification_schema, &proven, "jpd-fixture")
            .is_empty(),
        "a blocked council result cannot be relabeled as proven"
    );

    proven["bindings"]["council"]["result"]["status"] = serde_json::json!("completed");
    proven["bindings"]["council"]["result"]["dissent"]["items"][0]["status"] =
        serde_json::json!("resolved");
    proven["bindings"]["council"]["result"]["dissent"]["items"][0]["resolutionRef"] = serde_json::json!({
        "referenceId": "resolution.issue-210.claim",
        "contentDigest": "sha256:8686868686868686868686868686868686868686868686868686868686868686"
    });
    proven["bindings"]["council"]["result"]["decision"]["status"] =
        serde_json::json!("recommended");
    proven["bindings"]["council"]["result"]["decision"]["unresolvedDissentIds"] =
        serde_json::json!([]);
    assert!(
        !schemas
            .validate(&verification_schema, &proven, "jpd-fixture")
            .is_empty(),
        "an unresolved high-severity council claim cannot be hidden behind resolved dissent"
    );
}

#[test]
fn jpd_waivers_attach_only_to_active_local_blockers() {
    let root = package_root();
    let schemas = package_schemas(&root);
    let verification_schema = schema_id(&root, "journey-verification-result.schema.json");
    let accepted =
        load_json(&root.join("fixtures/positive/journey-verification-accepted-with-waiver.json"));

    let mut healthy_observer_waived = accepted.clone();
    healthy_observer_waived["bindings"]["observers"][0]["waiver"] = scoped_test_waiver(
        "verification/issue-210/waived",
        "observer",
        "graphhelm-cli-graph-contract",
        "policy-waiver.issue-210.unneeded-observer",
        "84",
    );
    assert!(
        !schemas
            .validate(
                &verification_schema,
                &healthy_observer_waived,
                "jpd-fixture",
            )
            .is_empty(),
        "fresh observer evidence has no observer blocker to waive"
    );

    let mut healthy_retry_waived = accepted;
    healthy_retry_waived["retry"]["waiver"] = scoped_test_waiver(
        "verification/issue-210/waived",
        "retry",
        "retry/retry-classifier",
        "policy-waiver.issue-210.unneeded-retry",
        "85",
    );
    assert!(
        !schemas
            .validate(&verification_schema, &healthy_retry_waived, "jpd-fixture")
            .is_empty(),
        "a healthy evaluated retry has no retry blocker to waive"
    );
}

#[test]
fn jpd_council_participants_and_decision_remain_task_local_advice() {
    let root = package_root();
    let schemas = package_schemas(&root);
    let council_schema = schema_id(&root, "council-result.schema.json");
    let verification =
        load_json(&root.join("fixtures/positive/journey-verification-accepted-with-waiver.json"));
    let result = verification["bindings"]["council"]["result"].clone();

    let mut unidentified_participant = result.clone();
    unidentified_participant["participants"]["items"][0]["identity"]
        .as_object_mut()
        .unwrap()
        .remove("participantId");
    assert!(
        !schemas
            .validate(&council_schema, &unidentified_participant, "jpd-fixture")
            .is_empty(),
        "every council participant must have a typed identity"
    );

    let mut unscoped_run = result.clone();
    unscoped_run["participants"]["items"][0]["run"]["authority"]
        .as_object_mut()
        .unwrap()
        .remove("grantRef");
    assert!(
        !schemas
            .validate(&council_schema, &unscoped_run, "jpd-fixture")
            .is_empty(),
        "every participant run must bind its task-local advisory grant"
    );

    let mut self_authorizing_result = result.clone();
    self_authorizing_result["authority"]["status"] = serde_json::json!("validated");
    assert!(
        !schemas
            .validate(&council_schema, &self_authorizing_result, "jpd-fixture")
            .is_empty(),
        "a data-only council result cannot self-assert validator authority"
    );

    let mut gate_deciding_council = result;
    gate_deciding_council["decision"]["mayDecideGate"] = serde_json::json!(true);
    assert!(
        !schemas
            .validate(&council_schema, &gate_deciding_council, "jpd-fixture")
            .is_empty(),
        "the council decision must remain advisory and cannot decide a gate"
    );
}

#[test]
fn jpd_observer_catalog_exposes_stable_capability_ids() {
    let root = package_root();
    let schemas = package_schemas(&root);
    let catalog_schema = schema_id(&root, "observer-catalog.schema.json");
    let catalog: Value =
        serde_yaml_ng::from_slice(&fs::read(root.join("observers/catalog.yaml")).unwrap()).unwrap();

    assert!(
        catalog["spec"]["observers"]
            .as_array()
            .unwrap()
            .iter()
            .all(|observer| observer["capabilityId"].as_str().is_some()),
        "every catalog entry needs a stable capability id that a receipt can bind"
    );
    assert!(
        schemas
            .validate(&catalog_schema, &catalog, "jpd-observer-catalog")
            .is_empty(),
        "the capability-bound observer catalog must validate"
    );

    let mut missing_capability_id = catalog;
    missing_capability_id["spec"]["observers"][0]
        .as_object_mut()
        .unwrap()
        .remove("capabilityId");
    assert!(
        !schemas
            .validate(
                &catalog_schema,
                &missing_capability_id,
                "jpd-observer-catalog",
            )
            .is_empty(),
        "an observer catalog entry without its capability id must be rejected"
    );
}

#[test]
fn jpd_observation_match_carries_replayable_capability_trust_and_time_bindings() {
    let root = package_root();
    let schemas = package_schemas(&root);
    let observation_schema = schema_id(&root, "observation-obligation.schema.json");
    let mut matched = load_json(&root.join("fixtures/positive/missing-observer-refusal.json"));
    matched["resolution"] = serde_json::json!({
        "status": "matched",
        "catalogBinding": {
            "kind": "observer_catalog",
            "catalogId": "graphhelm-jpd/observer-catalog",
            "catalogVersion": "1.0.0",
            "catalogDigest": format!("sha256:{}", "1".repeat(64))
        },
        "capabilityBinding": {
            "kind": "observer_capability",
            "capabilityId": "browser.semantic-journey",
            "observerId": "graphhelm-browser-user-journey",
            "observerVersion": "1.0.0",
            "capabilityDigest": format!("sha256:{}", "2".repeat(64))
        },
        "configurationDigest": format!("sha256:{}", "3".repeat(64)),
        "environmentDigest": format!("sha256:{}", "4".repeat(64)),
        "observedTrust": "first_party_instrumented",
        "trustCompatibilityCandidate": trust_compatibility_candidate(),
        "capabilityReceipt": {
            "capturedAtUnixSeconds": 1_777_000_000,
            "receipt": {
                "evidenceId": "evidence.observer-capability",
                "contentSha256": "5".repeat(64),
                "ciphertextSha256": "6".repeat(64)
            }
        },
        "matchedEvidence": [
            {
                "evidenceKind": "dom_semantic_snapshot",
                "capturedAtUnixSeconds": 1_777_000_001,
                "receipt": {
                    "evidenceId": "evidence.dom-semantic-snapshot",
                    "contentSha256": "7".repeat(64),
                    "ciphertextSha256": "8".repeat(64)
                }
            },
            {
                "evidenceKind": "accessibility_tree",
                "capturedAtUnixSeconds": 1_777_000_002,
                "receipt": {
                    "evidenceId": "evidence.accessibility-tree",
                    "contentSha256": "9".repeat(64),
                    "ciphertextSha256": "a".repeat(64)
                }
            }
        ],
        "freshnessEvaluation": {
            "clock": "unix_seconds",
            "evaluatedAtUnixSeconds": 1_777_000_005,
            "maximumObservedAgeSeconds": 5,
            "requiredMaxAgeSeconds": 30,
            "requiredAbsenceWindowSeconds": null,
            "absenceWindow": null,
            "result": "satisfied"
        },
        "evidenceMatchEvaluation": {
            "evaluatorId": "graphhelm-jpd/evidence-matcher",
            "evaluatorVersion": "1.0.0",
            "evaluationSemantics": "registered_deterministic",
            "inputDigest": format!("sha256:{}", "b".repeat(64)),
            "receipt": {
                "evidenceId": "evidence.evidence-match-evaluation",
                "contentSha256": "c".repeat(64),
                "ciphertextSha256": "d".repeat(64)
            }
        }
    });
    assert!(
        schemas
            .validate(&observation_schema, &matched, "jpd-observation")
            .is_empty(),
        "a candidate match must retain the inputs a deterministic validator will replay"
    );

    let mut unbound_catalog = matched.clone();
    unbound_catalog["resolution"]["catalogBinding"]
        .as_object_mut()
        .unwrap()
        .remove("catalogDigest");
    assert!(
        !schemas
            .validate(&observation_schema, &unbound_catalog, "jpd-observation")
            .is_empty(),
        "a catalog id without its content digest is not a replayable binding"
    );

    let mut unsupported_evidence = matched.clone();
    unsupported_evidence["resolution"]["matchedEvidence"][0]["evidenceKind"] =
        serde_json::json!("screenshot_claim");
    assert!(
        !schemas
            .validate(
                &observation_schema,
                &unsupported_evidence,
                "jpd-observation",
            )
            .is_empty(),
        "an unsupported evidence kind must be rejected"
    );

    let mut partial_evidence = matched.clone();
    partial_evidence["resolution"]["matchedEvidence"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert!(
        !schemas
            .validate(&observation_schema, &partial_evidence, "jpd-observation")
            .is_empty(),
        "matched evidence must retain a receipt for every required evidence kind"
    );

    let mut malformed_absence_window = matched;
    malformed_absence_window["resolution"]["freshnessEvaluation"]["absenceWindow"] = serde_json::json!({
        "startedAtUnixSeconds": 1_777_000_000,
        "endedAtUnixSeconds": 1_777_000_005,
        "observedDurationSeconds": 5,
        "verificationReceipt": {
            "evidenceId": "evidence.absence-window",
            "contentSha256": "e".repeat(64),
            "ciphertextSha256": "f".repeat(64)
        }
    });
    assert!(
        !schemas
            .validate(
                &observation_schema,
                &malformed_absence_window,
                "jpd-observation",
            )
            .is_empty(),
        "a null absence requirement cannot carry a contradictory observed window"
    );
}

#[test]
fn jpd_confirmed_defect_requires_a_regression_obligation() {
    let root = package_root();
    let schemas = package_schemas(&root);
    let defect_schema = schema_id(&root, "journey-defect-claim.schema.json");
    let confirmed = load_json(&root.join("fixtures/positive/semantic-journey-defect-claim.json"));
    assert!(
        schemas
            .validate(&defect_schema, &confirmed, "jpd-fixture")
            .is_empty(),
        "the positive confirmed defect fixture must validate"
    );

    let mut missing_obligation = confirmed;
    missing_obligation
        .as_object_mut()
        .unwrap()
        .remove("regressionObligationId");
    assert!(
        !schemas
            .validate(&defect_schema, &missing_obligation, "jpd-fixture")
            .is_empty(),
        "a confirmed defect must name the regression obligation it creates"
    );
}

#[test]
fn jpd_confirmed_defect_requires_a_reproduced_attempt() {
    let root = package_root();
    let schemas = package_schemas(&root);
    let defect_schema = schema_id(&root, "journey-defect-claim.schema.json");
    let mut never_reproduced =
        load_json(&root.join("fixtures/positive/semantic-journey-defect-claim.json"));
    for attempt in never_reproduced["reproduction"]["attempts"]
        .as_array_mut()
        .unwrap()
    {
        attempt["result"] = serde_json::json!("inconclusive");
    }

    assert!(
        !schemas
            .validate(&defect_schema, &never_reproduced, "jpd-fixture")
            .is_empty(),
        "a confirmed defect cannot be inferred without a reproduced attempt"
    );
}

#[test]
fn jpd_unresolved_defect_requires_a_missing_fact_and_residual_uncertainty() {
    let root = package_root();
    let schemas = package_schemas(&root);
    let defect_schema = schema_id(&root, "journey-defect-claim.schema.json");
    let mut unresolved =
        load_json(&root.join("fixtures/positive/semantic-journey-defect-claim.json"));
    unresolved["status"] = serde_json::json!("unresolved");
    unresolved["review"]["result"] = serde_json::json!("unresolved");
    unresolved["review"]["missingFact"] = serde_json::json!(
        "Whether the second request reached the authoritative installation queue."
    );
    unresolved["review"]["residualUncertainty"] = serde_json::json!([
        "The local interaction trace cannot observe the authoritative queue boundary."
    ]);
    unresolved
        .as_object_mut()
        .unwrap()
        .remove("regressionObligationId");
    assert!(
        schemas
            .validate(&defect_schema, &unresolved, "jpd-fixture")
            .is_empty(),
        "an unresolved defect with an exact missing fact and uncertainty must validate"
    );

    let mut missing_fact = unresolved.clone();
    missing_fact["review"]
        .as_object_mut()
        .unwrap()
        .remove("missingFact");
    assert!(
        !schemas
            .validate(&defect_schema, &missing_fact, "jpd-fixture")
            .is_empty(),
        "an unresolved defect must name the exact fact still missing"
    );

    unresolved["review"]["residualUncertainty"] = serde_json::json!([]);
    assert!(
        !schemas
            .validate(&defect_schema, &unresolved, "jpd-fixture")
            .is_empty(),
        "an unresolved defect must preserve at least one residual uncertainty"
    );
}

#[test]
fn jpd_reviewed_defect_requires_typed_reporter_and_reviewer_run_authority() {
    let root = package_root();
    let schemas = package_schemas(&root);
    let defect_schema = schema_id(&root, "journey-defect-claim.schema.json");
    let confirmed = load_json(&root.join("fixtures/positive/semantic-journey-defect-claim.json"));

    let mut missing_reporter = confirmed.clone();
    missing_reporter.as_object_mut().unwrap().remove("reporter");
    assert!(
        !schemas
            .validate(&defect_schema, &missing_reporter, "jpd-fixture")
            .is_empty(),
        "a defect claim must bind its reporter to a typed actor and authorized run"
    );

    let mut missing_reviewer = confirmed.clone();
    missing_reviewer["review"]
        .as_object_mut()
        .unwrap()
        .remove("reviewer");
    assert!(
        !schemas
            .validate(&defect_schema, &missing_reviewer, "jpd-fixture")
            .is_empty(),
        "a completed defect review must bind its reviewer to a typed actor and authorized run"
    );

    let mut wrong_review_scope = confirmed;
    wrong_review_scope["review"]["reviewer"]["run"]["authority"]["scope"] =
        serde_json::json!("journey-defect.report");
    assert!(
        !schemas
            .validate(&defect_schema, &wrong_review_scope, "jpd-fixture")
            .is_empty(),
        "report and review runs must carry their own typed authority scopes"
    );
}

#[test]
fn jpd_defect_independence_is_distinct_input_with_candidate_authority() {
    let root = package_root();
    let schemas = package_schemas(&root);
    let defect_schema = schema_id(&root, "journey-defect-claim.schema.json");
    let confirmed = load_json(&root.join("fixtures/positive/semantic-journey-defect-claim.json"));

    let mut missing_validation = confirmed.clone();
    missing_validation["review"]
        .as_object_mut()
        .unwrap()
        .remove("identityDistinctValidation");
    assert!(
        !schemas
            .validate(&defect_schema, &missing_validation, "jpd-fixture")
            .is_empty(),
        "an independent review must carry the deterministic identity-distinct input"
    );

    let mut duplicate_actor = confirmed.clone();
    duplicate_actor["review"]["identityDistinctValidation"]["input"]["actorIds"][1] =
        duplicate_actor["review"]["identityDistinctValidation"]["input"]["actorIds"][0].clone();
    assert!(
        !schemas
            .validate(&defect_schema, &duplicate_actor, "jpd-fixture")
            .is_empty(),
        "reporter and reviewer identity inputs must be distinct"
    );

    let mut duplicate_run = confirmed.clone();
    duplicate_run["review"]["identityDistinctValidation"]["input"]["runIds"][1] =
        duplicate_run["review"]["identityDistinctValidation"]["input"]["runIds"][0].clone();
    assert!(
        !schemas
            .validate(&defect_schema, &duplicate_run, "jpd-fixture")
            .is_empty(),
        "reporter and reviewer run inputs must be distinct"
    );

    let mut duplicate_authority = confirmed.clone();
    duplicate_authority["review"]["identityDistinctValidation"]["input"]["runAuthorityRefs"][1] =
        duplicate_authority["review"]["identityDistinctValidation"]["input"]["runAuthorityRefs"][0]
            .clone();
    assert!(
        !schemas
            .validate(&defect_schema, &duplicate_authority, "jpd-fixture")
            .is_empty(),
        "reporter and reviewer run-authority inputs must be distinct"
    );

    let mut false_authority = confirmed;
    false_authority["review"]["identityDistinctValidation"]["authority"]["status"] =
        serde_json::json!("validated");
    assert!(
        !schemas
            .validate(&defect_schema, &false_authority, "jpd-fixture")
            .is_empty(),
        "shape validation cannot claim authoritative independence without the registered validator"
    );
}
