use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use graphhelm_schema::OfflineSchemaSet;
use serde_json::Value;

const TRUST_LEVELS: [&str; 5] = [
    "first_party_deterministic",
    "first_party_instrumented",
    "third_party_receipt",
    "visual_capture",
    "operator_attestation",
];

const FACTS: [&str; 8] = [
    "command_completed",
    "request_accepted",
    "state_persisted",
    "message_delivered",
    "content_rendered",
    "control_operable",
    "recovery_safe",
    "event_history_intact",
];

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

fn load_yaml(path: &Path) -> Value {
    serde_yaml_ng::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn package_schemas(root: &Path) -> OfflineSchemaSet {
    let mut paths = fs::read_dir(root.join("schemas"))
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

fn matched_observation(minimum_trust: &str, observed_trust: &str) -> Value {
    serde_json::json!({
        "obligationId": "obligation.content-rendered",
        "contractId": "contract.issue-210",
        "contractDigest": format!("sha256:{}", "0".repeat(64)),
        "promiseId": "promise.content-rendered",
        "fact": "content_rendered",
        "requiredEvidenceKinds": ["dom_semantic_snapshot"],
        "observerRequirements": {
            "capability": "browser.semantic-journey",
            "minimumTrust": minimum_trust,
            "maxAgeSeconds": 30,
            "absenceWindowSeconds": null
        },
        "resolution": {
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
            "observedTrust": observed_trust,
            "capabilityReceipt": {
                "capturedAtUnixSeconds": 1_777_000_000_u64,
                "receipt": {
                    "evidenceId": "evidence.observer-capability",
                    "contentSha256": "5".repeat(64),
                    "ciphertextSha256": "6".repeat(64)
                }
            },
            "matchedEvidence": [{
                "evidenceKind": "dom_semantic_snapshot",
                "capturedAtUnixSeconds": 1_777_000_001_u64,
                "receipt": {
                    "evidenceId": "evidence.dom-semantic-snapshot",
                    "contentSha256": "7".repeat(64),
                    "ciphertextSha256": "8".repeat(64)
                }
            }],
            "freshnessEvaluation": {
                "clock": "unix_seconds",
                "evaluatedAtUnixSeconds": 1_777_000_005_u64,
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
                "inputDigest": format!("sha256:{}", "9".repeat(64)),
                "receipt": {
                    "evidenceId": "evidence.evidence-match-evaluation",
                    "contentSha256": "a".repeat(64),
                    "ciphertextSha256": "b".repeat(64)
                }
            }
        }
    })
}

fn bind_trust_relation(candidate: &mut Value, lattice_digest: &str) {
    candidate["resolution"]["trustCompatibilityCandidate"] = serde_json::json!({
        "authority": "candidate",
        "latticeBinding": {
            "latticeId": "graphhelm-jpd/evidence-strength-lattice",
            "latticeVersion": "1.0.0",
            "latticeDigest": lattice_digest
        },
        "relationId": "graphhelm-jpd/trust-compatibility",
        "relationVersion": "1.0.0",
        "decision": "compatible_candidate",
        "inputDigest": format!("sha256:{}", "c".repeat(64)),
        "receipt": {
            "evidenceId": "evidence.trust-compatibility-candidate",
            "contentSha256": "d".repeat(64),
            "ciphertextSha256": "e".repeat(64)
        }
    });
}

#[test]
fn evidence_lattice_declares_a_fact_specific_trust_partial_order() {
    let root = package_root();
    let schemas = package_schemas(&root);
    let lattice = load_yaml(&root.join("evaluators/evidence-strength-lattice.yaml"));
    let lattice_schema = schema_id(&root, "evidence-strength-lattice.schema.json");

    assert!(
        schemas
            .validate(&lattice_schema, &lattice, "jpd-trust-lattice")
            .is_empty(),
        "the normative trust relation must remain schema-valid"
    );

    let relation = lattice["spec"]["trustCompatibility"]
        .as_object()
        .expect("the evidence lattice must declare its trust compatibility relation");
    assert_eq!(relation["relationId"], "graphhelm-jpd/trust-compatibility");
    assert_eq!(relation["relationVersion"], "1.0.0");
    assert_eq!(relation["semantics"], "explicit_fact_scoped_partial_order");
    assert_eq!(relation["defaultDecision"], "incompatible");

    let declared_levels = relation["levels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(declared_levels, TRUST_LEVELS.into_iter().collect());

    let fact_table = relation["facts"].as_object().unwrap();
    assert_eq!(
        fact_table
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        FACTS.into_iter().collect()
    );

    for fact in FACTS {
        let satisfies = fact_table[fact]["satisfiesMinimum"].as_object().unwrap();
        assert_eq!(
            satisfies
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>(),
            TRUST_LEVELS.into_iter().collect(),
            "{fact} must decide every declared minimum trust level"
        );

        let allowed = TRUST_LEVELS
            .into_iter()
            .map(|minimum| {
                let observed = satisfies[minimum]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_str().unwrap())
                    .collect::<BTreeSet<_>>();
                assert!(
                    observed.contains(minimum),
                    "the {fact} relation must be reflexive for {minimum}"
                );
                (minimum, observed)
            })
            .collect::<BTreeMap<_, _>>();

        for minimum in TRUST_LEVELS {
            for observed in &allowed[minimum] {
                if minimum != *observed {
                    assert!(
                        !allowed[*observed].contains(minimum),
                        "the {fact} relation must be antisymmetric for {minimum} and {observed}"
                    );
                }
                for stronger in &allowed[*observed] {
                    assert!(
                        allowed[minimum].contains(stronger),
                        "the {fact} relation must be transitive for {minimum}, {observed}, and {stronger}"
                    );
                }
            }
        }
    }

    assert!(
        fact_table["message_delivered"]["satisfiesMinimum"]["operator_attestation"]
            .as_array()
            .unwrap()
            .contains(&Value::String("third_party_receipt".to_owned())),
        "a provider receipt may satisfy the operator-attestation floor for delivery"
    );
    assert!(
        !fact_table["control_operable"]["satisfiesMinimum"]["operator_attestation"]
            .as_array()
            .unwrap()
            .contains(&Value::String("visual_capture".to_owned())),
        "a visual capture must not be upgraded into proof that a control is operable"
    );
}

#[test]
fn matched_observation_requires_a_trust_relation_binding() {
    let root = package_root();
    let schemas = package_schemas(&root);
    let observation_schema = schema_id(&root, "observation-obligation.schema.json");
    let candidate = matched_observation("first_party_instrumented", "first_party_instrumented");

    assert!(
        !schemas
            .validate(&observation_schema, &candidate, "jpd-observer-trust")
            .is_empty(),
        "status: matched must bind the normative trust relation used for its compatibility decision"
    );
}

#[test]
fn instrumented_minimum_rejects_operator_attestation_match() {
    let root = package_root();
    let schemas = package_schemas(&root);
    let observation_schema = schema_id(&root, "observation-obligation.schema.json");
    let lattice_digest = format!(
        "sha256:{}",
        sha256_hex(&fs::read(root.join("evaluators/evidence-strength-lattice.yaml")).unwrap())
    );

    let mut compatible =
        matched_observation("first_party_instrumented", "first_party_instrumented");
    bind_trust_relation(&mut compatible, &lattice_digest);
    assert!(
        schemas
            .validate(&observation_schema, &compatible, "jpd-observer-trust")
            .is_empty(),
        "a digest-bound compatible candidate must retain schema-valid candidate authority"
    );

    let mut incompatible = matched_observation("first_party_instrumented", "operator_attestation");
    bind_trust_relation(&mut incompatible, &lattice_digest);
    assert!(
        !schemas
            .validate(&observation_schema, &incompatible, "jpd-observer-trust")
            .is_empty(),
        "operator attestation cannot satisfy a first-party instrumented minimum"
    );
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};

    hex::encode(Sha256::digest(bytes))
}

fn set_observed_fact(candidate: &mut Value, fact: &str) {
    let evidence_kind = match fact {
        "command_completed" => "process_exit",
        "request_accepted" => "http_acceptance",
        "state_persisted" => "durable_readback",
        "message_delivered" => "provider_delivery_receipt",
        "content_rendered" => "dom_semantic_snapshot",
        "control_operable" | "recovery_safe" => "interaction_trace",
        "event_history_intact" => "event_chain_verification",
        _ => panic!("unhandled test fact: {fact}"),
    };
    candidate["fact"] = Value::String(fact.to_owned());
    candidate["requiredEvidenceKinds"] = serde_json::json!([evidence_kind]);
    candidate["resolution"]["matchedEvidence"][0]["evidenceKind"] =
        Value::String(evidence_kind.to_owned());
}

#[test]
fn observation_schema_enforces_every_declared_trust_pair() {
    let root = package_root();
    let schemas = package_schemas(&root);
    let observation_schema = schema_id(&root, "observation-obligation.schema.json");
    let lattice_bytes = fs::read(root.join("evaluators/evidence-strength-lattice.yaml")).unwrap();
    let lattice: Value = serde_yaml_ng::from_slice(&lattice_bytes).unwrap();
    let lattice_digest = format!("sha256:{}", sha256_hex(&lattice_bytes));
    let fact_table = lattice["spec"]["trustCompatibility"]["facts"]
        .as_object()
        .unwrap();

    for fact in FACTS {
        for minimum in TRUST_LEVELS {
            let allowed = fact_table[fact]["satisfiesMinimum"][minimum]
                .as_array()
                .unwrap();
            for observed in TRUST_LEVELS {
                let mut candidate = matched_observation(minimum, observed);
                set_observed_fact(&mut candidate, fact);
                bind_trust_relation(&mut candidate, &lattice_digest);
                let valid = schemas
                    .validate(&observation_schema, &candidate, "jpd-observer-trust")
                    .is_empty();
                let expected = allowed.contains(&Value::String(observed.to_owned()));
                assert_eq!(
                    valid, expected,
                    "schema/table drift for fact={fact}, minimum={minimum}, observed={observed}"
                );
            }
        }
    }
}

#[test]
fn trust_relation_binding_is_digest_bound_and_shape_closed() {
    let root = package_root();
    let schemas = package_schemas(&root);
    let observation_schema = schema_id(&root, "observation-obligation.schema.json");
    let lattice_digest = format!(
        "sha256:{}",
        sha256_hex(&fs::read(root.join("evaluators/evidence-strength-lattice.yaml")).unwrap())
    );
    let mut candidate = matched_observation("first_party_instrumented", "first_party_instrumented");
    bind_trust_relation(&mut candidate, &lattice_digest);

    let mut stale = candidate.clone();
    stale["resolution"]["trustCompatibilityCandidate"]["latticeBinding"]["latticeDigest"] =
        format!("sha256:{}", "f".repeat(64)).into();
    assert!(
        !schemas
            .validate(&observation_schema, &stale, "jpd-observer-trust")
            .is_empty(),
        "a match candidate cannot bind a different lattice digest"
    );

    candidate["resolution"]["trustCompatibilityCandidate"]["operationallyValidated"] =
        Value::Bool(true);
    assert!(
        !schemas
            .validate(&observation_schema, &candidate, "jpd-observer-trust")
            .is_empty(),
        "candidate authority cannot acquire an undeclared operational-validation claim"
    );
}

#[test]
fn provider_delivery_observer_uses_the_canonical_journey_vocabulary() {
    let root = package_root();
    let schemas = package_schemas(&root);
    let observer_schema = schema_id(&root, "observer-capability.schema.json");
    let provider_observer = serde_json::json!({
        "observerId": "provider-delivery-receipt",
        "capabilityId": "provider.message-delivery",
        "version": "1.0.0",
        "surface": "http",
        "availability": "requires_probe",
        "facts": ["message_delivered"],
        "evidenceKinds": ["provider_delivery_receipt"],
        "trust": "third_party_receipt",
        "effects": ["external_read"],
        "commands": ["provider:delivery-status"],
        "freshness": {
            "class": "drifting",
            "maxAgeSeconds": 30,
            "absenceWindowSeconds": null
        },
        "custody": {
            "producer": "destination provider",
            "digestAlgorithm": "sha256",
            "ordered": false
        },
        "configurationDigestRequired": true,
        "environmentBindingRequired": true,
        "limitations": [
            "The observer reports only the provider receipt and does not prove that a person read the message."
        ]
    });

    assert!(
        schemas
            .validate(
                &observer_schema,
                &provider_observer,
                "jpd-provider-observer"
            )
            .is_empty(),
        "an independently installed provider observer must be able to declare delivery facts and receipts"
    );
}
