use std::{fs, path::Path};

use graphhelm_graph::{
    canonical_content_bytes, derive_content_slot_id, derive_content_slot_profile,
    encode_persisted_reference, parse_persisted_binding_reference_node, persisted_hashes,
    validate_evidence_bijection, validate_persisted_projection,
};
use graphhelm_protocols::{
    ContentFieldKind, ContentOwnerKind, ContentSlot, EvidenceId, EvidenceReference, OpaqueId,
    PersistedGraphVersion, RawSha256,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

fn fixture() -> PersistedGraphVersion {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/schemas/valid/persisted-graph-version.json");
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn digest(byte: char) -> RawSha256 {
    RawSha256::parse(byte.to_string().repeat(64)).unwrap()
}

fn independently_sorted(value: Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .map(|(key, value)| (key, independently_sorted(value)))
                .collect(),
        ),
        Value::Array(values) => {
            Value::Array(values.into_iter().map(independently_sorted).collect())
        }
        scalar => scalar,
    }
}

fn independent_hash(value: Value) -> String {
    let bytes = serde_json::to_vec(&independently_sorted(value)).unwrap();
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

fn mutated_fixture(mut mutate: impl FnMut(&mut serde_json::Value)) -> PersistedGraphVersion {
    let mut value = serde_json::to_value(fixture()).unwrap();
    mutate(&mut value);
    let provisional: PersistedGraphVersion = serde_json::from_value(value.clone()).unwrap();
    let hashes = persisted_hashes(provisional.topology(), provisional.content_slots()).unwrap();
    value["topologyHash"] = serde_json::json!(hashes.topology_hash().as_str());
    value["semanticHash"] = serde_json::json!(hashes.semantic_hash().as_str());
    serde_json::from_value(value).unwrap()
}

fn add_fixture_agent(value: &mut serde_json::Value, id: &str) {
    value["topology"]["budgets"]["maxNodes"] = serde_json::json!(2);
    value["topology"]["budgets"]["maxDepth"] = serde_json::json!(2);
    let mut node = value["topology"]["nodes"]["start"].clone();
    let owner = OpaqueId::parse(id).unwrap();
    let display = derive_content_slot_id(
        ContentOwnerKind::Node,
        &owner,
        ContentFieldKind::DisplayName,
        0,
    )
    .unwrap();
    let objective = derive_content_slot_id(
        ContentOwnerKind::Node,
        &owner,
        ContentFieldKind::Objective,
        0,
    )
    .unwrap();
    node["contentSlotIds"] = serde_json::json!([display.as_str(), objective.as_str()]);
    value["topology"]["nodes"][id] = node;
    let slots = value["contentSlots"].as_array_mut().unwrap();
    for (index, slot_id) in [display, objective].into_iter().enumerate() {
        let mut slot = slots[index + 1].clone();
        slot["slotId"] = serde_json::json!(slot_id.as_str());
        slot["ownerId"] = serde_json::json!(id);
        slot["evidenceId"] = serde_json::json!(format!("evidence-{id}-{index}"));
        slots.push(slot);
    }
}

#[test]
fn content_slot_identity_is_derived_from_the_complete_typed_position() {
    let owner = OpaqueId::parse("owner-one").unwrap();
    let baseline = derive_content_slot_id(
        ContentOwnerKind::Node,
        &owner,
        ContentFieldKind::Objective,
        0,
    )
    .unwrap();

    assert_eq!(
        baseline,
        derive_content_slot_id(
            ContentOwnerKind::Node,
            &owner,
            ContentFieldKind::Objective,
            0,
        )
        .unwrap()
    );
    assert_ne!(
        baseline,
        derive_content_slot_id(
            ContentOwnerKind::Policy,
            &owner,
            ContentFieldKind::PolicyText,
            0,
        )
        .unwrap()
    );
    assert_ne!(
        baseline,
        derive_content_slot_id(
            ContentOwnerKind::Node,
            &owner,
            ContentFieldKind::Objective,
            1,
        )
        .unwrap()
    );
}

#[test]
fn content_slot_profile_is_derived_from_the_complete_typed_position() {
    let owner = OpaqueId::parse("owner-one").unwrap();
    let display = derive_content_slot_profile(
        ContentOwnerKind::Graph,
        &owner,
        ContentFieldKind::DisplayName,
        0,
    )
    .unwrap();
    assert_eq!(
        display.sensitivity(),
        graphhelm_protocols::Sensitivity::Internal
    );
    assert!(!display.required_for_execution());

    let objective = derive_content_slot_profile(
        ContentOwnerKind::Node,
        &owner,
        ContentFieldKind::Objective,
        0,
    )
    .unwrap();
    assert_eq!(
        objective.sensitivity(),
        graphhelm_protocols::Sensitivity::Restricted
    );
    assert!(objective.required_for_execution());

    assert!(
        derive_content_slot_profile(
            ContentOwnerKind::Node,
            &owner,
            ContentFieldKind::Objective,
            1,
        )
        .is_err()
    );
}

#[test]
fn projection_rejects_a_rehashed_renamed_graph_slot() {
    let version = mutated_fixture(|value| {
        value["contentSlots"][0]["slotId"] = serde_json::json!("slot-renamed-graph");
    });
    assert_eq!(
        validate_persisted_projection(&version).unwrap_err(),
        graphhelm_graph::GraphError::InvalidProjection
    );
}

#[test]
fn topology_hash_ignores_content_digest_while_semantic_hash_tracks_it() {
    let version = fixture();
    let first = version.content_slots().to_vec();
    let mut changed_value = serde_json::to_value(&first).unwrap();
    changed_value[0]["contentSha256"] = serde_json::json!("b".repeat(64));
    let changed: Vec<ContentSlot> = serde_json::from_value(changed_value).unwrap();

    let first_hashes = persisted_hashes(version.topology(), &first).unwrap();
    let changed_hashes = persisted_hashes(version.topology(), &changed).unwrap();

    assert_eq!(first_hashes.topology_hash(), changed_hashes.topology_hash());
    assert_ne!(first_hashes.semantic_hash(), changed_hashes.semantic_hash());
}

#[test]
fn persisted_hashes_match_the_normative_position_and_content_tuple_materials() {
    let version = fixture();
    let positions = version
        .content_slots()
        .iter()
        .map(|slot| {
            serde_json::json!({
                "ownerKind": slot.owner_kind(),
                "ownerId": slot.owner_id(),
                "fieldKind": slot.field_kind(),
                "ordinal": slot.ordinal(),
            })
        })
        .collect::<Vec<_>>();
    let topology_material = serde_json::json!({
        "topology": version.topology(),
        "contentPositions": positions,
    });
    let content_tuples = version
        .content_slots()
        .iter()
        .map(|slot| {
            serde_json::json!({
                "ownerKind": slot.owner_kind(),
                "ownerId": slot.owner_id(),
                "fieldKind": slot.field_kind(),
                "ordinal": slot.ordinal(),
                "contentSha256": slot.content_sha256(),
            })
        })
        .collect::<Vec<_>>();
    let semantic_material = serde_json::json!({
        "topology": topology_material,
        "contentDigests": content_tuples,
    });

    let hashes = persisted_hashes(version.topology(), version.content_slots()).unwrap();
    assert_eq!(
        hashes.topology_hash().as_str(),
        independent_hash(topology_material)
    );
    assert_eq!(
        hashes.semantic_hash().as_str(),
        independent_hash(semantic_material)
    );
}

#[test]
fn persisted_hashes_are_independent_of_map_insertion_order() {
    let version = fixture();
    let mut topology_value = serde_json::to_value(version.topology()).unwrap();
    topology_value["labels"] = serde_json::json!({"zeta": "last", "alpha": "first"});
    let forward = serde_json::from_value(topology_value.clone()).unwrap();
    topology_value["labels"] = serde_json::json!({"alpha": "first", "zeta": "last"});
    let reverse = serde_json::from_value(topology_value).unwrap();

    assert_eq!(
        persisted_hashes(&forward, version.content_slots()).unwrap(),
        persisted_hashes(&reverse, version.content_slots()).unwrap()
    );
}

#[test]
fn slot_reference_bijection_rejects_missing_extra_reordered_and_mismatched_refs() {
    let version = fixture();
    let slots = version.content_slots().to_vec();
    let refs = slots
        .iter()
        .enumerate()
        .map(|(index, slot)| {
            EvidenceReference::new(
                slot.evidence_id().clone(),
                slot.content_sha256().clone(),
                digest(char::from(b'c' + u8::try_from(index).unwrap())),
            )
        })
        .collect::<Vec<_>>();
    validate_evidence_bijection(&slots, &refs).unwrap();

    assert!(validate_evidence_bijection(&slots, &[]).is_err());
    let mut duplicate = refs.clone();
    duplicate[1] = duplicate[0].clone();
    assert!(validate_evidence_bijection(&slots, &duplicate).is_err());
    let mut reordered = refs.clone();
    reordered.swap(0, 1);
    assert!(validate_evidence_bijection(&slots, &reordered).is_err());

    let wrong_id = EvidenceReference::new(
        EvidenceId::parse("wrong-evidence").unwrap(),
        slots[0].content_sha256().clone(),
        digest('d'),
    );
    assert!(validate_evidence_bijection(&slots[..1], &[wrong_id]).is_err());

    let wrong_digest =
        EvidenceReference::new(slots[0].evidence_id().clone(), digest('e'), digest('f'));
    assert!(validate_evidence_bijection(&slots[..1], &[wrong_digest]).is_err());
}

#[test]
fn safe_projection_validator_rejects_relational_topology_corruption() {
    let cases = [
        mutated_fixture(|value| {
            value["predecessor"] = serde_json::Value::Null;
        }),
        mutated_fixture(|value| {
            value["topology"]["entrypoints"] = serde_json::json!(["missing-node"]);
        }),
        mutated_fixture(|value| {
            let edge = serde_json::json!({
                "id": "edge-duplicate",
                "from": "start",
                "to": "start",
                "edgeType": "control",
                "priority": null,
                "bindings": {},
                "condition": null
            });
            value["topology"]["edges"] = serde_json::json!([edge.clone(), edge]);
        }),
        mutated_fixture(|value| {
            value["topology"]["edges"] = serde_json::json!([{
                "id": "edge-dangling",
                "from": "missing-node",
                "to": "start",
                "edgeType": "control",
                "priority": null,
                "bindings": {},
                "condition": null
            }]);
        }),
        mutated_fixture(|value| {
            value["contentSlots"][1]["ownerId"] = serde_json::json!("missing-node");
        }),
        mutated_fixture(|value| {
            value["topology"]["nodes"]["start"]["contentSlotIds"] = serde_json::json!([]);
        }),
        mutated_fixture(|value| {
            value["contentSlots"][2]["ownerKind"] = serde_json::json!("policy");
            value["topology"]["nodes"]["start"]["contentSlotIds"]
                .as_array_mut()
                .unwrap()
                .pop();
        }),
        mutated_fixture(|value| {
            value["topology"]["completion"]["identifiers"]["terminal.000"] =
                serde_json::json!("missing-terminal");
        }),
        mutated_fixture(|value| {
            value["topology"]["nodes"]["start"]["controls"] = serde_json::json!([{
                "controlType": "gate_configuration",
                "identifiers": {"failureRoute": "missing-route"},
                "digests": {},
                "integers": {},
                "flags": {}
            }]);
        }),
        mutated_fixture(|value| {
            value["topology"]["nodes"]["start"]["controls"] = serde_json::json!([{
                "controlType": "deploy_configuration",
                "identifiers": {"compensationNode": "missing-compensation"},
                "digests": {},
                "integers": {},
                "flags": {}
            }]);
        }),
    ];

    for version in cases {
        assert_eq!(
            validate_persisted_projection(&version).unwrap_err(),
            graphhelm_graph::GraphError::InvalidProjection
        );
    }
}

#[test]
fn safe_projection_validator_accepts_a_relationally_consistent_fixture() {
    let version = fixture();
    validate_persisted_projection(&version).unwrap();

    let genesis = mutated_fixture(|value| {
        value["number"] = serde_json::json!(1);
        value["predecessor"] = serde_json::Value::Null;
    });
    validate_persisted_projection(&genesis).unwrap();
}

#[test]
fn safe_projection_validator_rejects_unknown_foreign_and_wrong_map_controls() {
    let cases = [
        mutated_fixture(|value| {
            value["topology"]["nodes"]["start"]["controls"] = serde_json::json!([{
                "controlType": "unknown_control",
                "identifiers": {},
                "digests": {},
                "integers": {},
                "flags": {"present": true}
            }]);
        }),
        mutated_fixture(|value| {
            value["topology"]["nodes"]["start"]["controls"] = serde_json::json!([{
                "controlType": "node_common",
                "identifiers": {"foreignKey": "harmless"},
                "digests": {},
                "integers": {},
                "flags": {"present": true}
            }]);
        }),
        mutated_fixture(|value| {
            value["topology"]["nodes"]["start"]["controls"] = serde_json::json!([{
                "controlType": "node_common",
                "identifiers": {},
                "digests": {},
                "integers": {"tag.000": 1},
                "flags": {"present": true}
            }]);
        }),
        mutated_fixture(|value| {
            value["topology"]["nodes"]["start"]["controls"] = serde_json::json!([{
                "controlType": "tool_configuration",
                "identifiers": {
                    "toolRef": encode_persisted_reference("builtin/repository-reader@1")
                        .unwrap()
                },
                "digests": {},
                "integers": {},
                "flags": {"present": true}
            }]);
        }),
        mutated_fixture(|value| {
            value["topology"]["edges"] = serde_json::json!([{
                "id": "edge-empty-condition",
                "from": "start",
                "to": "start",
                "edgeType": "control",
                "priority": null,
                "bindings": {},
                "condition": {
                    "controlType": "edge_condition",
                    "identifiers": {},
                    "digests": {},
                    "integers": {},
                    "flags": {}
                }
            }]);
        }),
    ];

    for version in cases {
        assert_eq!(
            validate_persisted_projection(&version).unwrap_err(),
            graphhelm_graph::GraphError::InvalidProjection
        );
    }
}

#[test]
fn replay_rejects_target_reference_on_a_foreign_node_kind() {
    let version = mutated_fixture(|value| {
        value["topology"]["nodes"]["start"]["controls"] = serde_json::json!([{
            "controlType": "node_configuration",
            "identifiers": {
                "targetRef": encode_persisted_reference("environment://staging").unwrap()
            },
            "digests": {},
            "integers": {},
            "flags": {"present": true}
        }]);
    });

    assert_eq!(
        validate_persisted_projection(&version).unwrap_err(),
        graphhelm_graph::GraphError::InvalidProjection
    );
}

#[test]
fn safe_projection_validator_rejects_secret_shaped_nominal_surfaces() {
    let secret = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";
    let cases = [
        mutated_fixture(|value| {
            value["topology"]["labels"]["release"] = serde_json::json!(secret);
        }),
        mutated_fixture(|value| {
            value["topology"]["graphId"] = serde_json::json!(secret);
        }),
        mutated_fixture(|value| {
            value["createdBy"]["id"] = serde_json::json!(secret);
        }),
        mutated_fixture(|value| {
            value["topology"]["nodes"]["start"]["controls"] = serde_json::json!([{
                "controlType": "node_common",
                "identifiers": {"onCancel": secret},
                "digests": {},
                "integers": {},
                "flags": {"present": true}
            }]);
        }),
        mutated_fixture(|value| {
            value["topology"]["edges"] = serde_json::json!([{
                "id": "edge-binding",
                "from": "start",
                "to": "start",
                "edgeType": "control",
                "priority": null,
                "bindings": {
                    "binding": "refv1:ZW52aXJvbm1lbnQ6Ly9wcm9kdWN0aW9uLWFwaS1rZXk"
                },
                "condition": null
            }]);
        }),
    ];

    for version in cases {
        assert_eq!(
            validate_persisted_projection(&version).unwrap_err(),
            graphhelm_graph::GraphError::InvalidProjection
        );
    }
}

#[test]
fn replay_rejects_every_compact_environment_secret_reference_variant() {
    for encoded in [
        "refv1:ZW52aXJvbm1lbnQ6Ly9wcm9kdWN0aW9uL2FwaUtleQ",
        "refv1:ZW52aXJvbm1lbnQ6Ly9wcm9kdWN0aW9uL3ByaXZhdGVLZXk",
        "refv1:ZW52aXJvbm1lbnQ6Ly9wcm9kdWN0aW9uL2FjY2Vzc0tleQ",
        "refv1:ZW52aXJvbm1lbnQ6Ly9wcm9kdWN0aW9uL2RhdGFiYXNlVXJs",
        "refv1:ZW52aXJvbm1lbnQ6Ly9wcm9kdWN0aW9uL2Nvbm5lY3Rpb25TdHJpbmc",
        "refv1:ZW52aXJvbm1lbnQ6Ly9wcm9kdWN0aW9uX0FQSV9LRVk",
        "refv1:ZW52aXJvbm1lbnQ6Ly9wcm9kdWN0aW9uLXByaXZhdGUta2V5",
        "refv1:ZW52aXJvbm1lbnQ6Ly9wcm9kdWN0aW9uX2FjY2Vzcy1rZXk",
        "refv1:ZW52aXJvbm1lbnQ6Ly9wcm9kdWN0aW9uLWRhdGFiYXNlLXVybA",
        "refv1:ZW52aXJvbm1lbnQ6Ly9wcm9kdWN0aW9uX2Nvbm5lY3Rpb24tc3RyaW5n",
    ] {
        let version = mutated_fixture(|value| {
            value["topology"]["edges"] = serde_json::json!([{
                "id": "edge-secret-ref",
                "from": "start",
                "to": "start",
                "edgeType": "control",
                "priority": null,
                "bindings": {"binding": encoded},
                "condition": null
            }]);
        });
        assert!(
            validate_persisted_projection(&version).is_err(),
            "{encoded}"
        );
    }
}

#[test]
fn persisted_topology_rejects_uncontrolled_cycles_and_accepts_registered_limits() {
    let uncontrolled = mutated_fixture(|value| {
        value["topology"]["edges"] = serde_json::json!([{
            "id": "cycle", "from": "start", "to": "start", "edgeType": "control",
            "priority": null, "bindings": {}, "condition": null
        }]);
    });
    assert_eq!(
        validate_persisted_projection(&uncontrolled).unwrap_err(),
        graphhelm_graph::GraphError::InvalidProjection
    );

    let controlled = mutated_fixture(|value| {
        value["topology"]["edges"] = serde_json::json!([{
            "id": "cycle", "from": "start", "to": "start", "edgeType": "control",
            "priority": null, "bindings": {}, "condition": null
        }]);
        value["topology"]["nodes"]["start"]["controls"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "controlType": "node_loop", "identifiers": {}, "digests": {},
                "integers": {"maxIterations": 2}, "flags": {"present": true}
            }));
    });
    validate_persisted_projection(&controlled).unwrap();
}

#[test]
fn persisted_topology_validates_multi_node_scc_and_terminal_reachability_iteratively() {
    let uncontrolled = mutated_fixture(|value| {
        add_fixture_agent(value, "z-second");
        value["topology"]["edges"] = serde_json::json!([
            {"id":"forward","from":"start","to":"z-second","edgeType":"control","priority":null,"bindings":{},"condition":null},
            {"id":"back","from":"z-second","to":"start","edgeType":"control","priority":null,"bindings":{},"condition":null}
        ]);
    });
    assert!(validate_persisted_projection(&uncontrolled).is_err());

    let controlled = mutated_fixture(|value| {
        add_fixture_agent(value, "z-second");
        value["topology"]["nodes"]["z-second"]["controls"].as_array_mut().unwrap().push(
            serde_json::json!({"controlType":"node_loop","identifiers":{},"digests":{},"integers":{"maxIterations":3},"flags":{"present":true}})
        );
        value["topology"]["edges"] = serde_json::json!([
            {"id":"forward","from":"start","to":"z-second","edgeType":"control","priority":null,"bindings":{},"condition":null},
            {"id":"back","from":"z-second","to":"start","edgeType":"control","priority":null,"bindings":{},"condition":null}
        ]);
    });
    validate_persisted_projection(&controlled).unwrap();

    let no_terminal_path = mutated_fixture(|value| {
        add_fixture_agent(value, "z-dead-end");
        value["topology"]["edges"] = serde_json::json!([
            {"id":"stranded","from":"start","to":"z-dead-end","edgeType":"control","priority":null,"bindings":{},"condition":null}
        ]);
    });
    assert!(validate_persisted_projection(&no_terminal_path).is_err());
}

#[test]
fn replay_rejects_foreign_edge_endpoints_without_unwinding_for_any_topology_budget() {
    let budget_cases = [
        ("none", None),
        ("maxNodes", Some(("maxNodes", serde_json::json!(1)))),
        ("maxDepth", Some(("maxDepth", serde_json::json!(1)))),
        ("maxMutations", Some(("maxMutations", serde_json::json!(0)))),
        (
            "maxRetriesPerNode",
            Some(("maxRetriesPerNode", serde_json::json!(0))),
        ),
        (
            "maxWallClockSeconds",
            Some(("maxWallClockSeconds", serde_json::json!(1))),
        ),
        (
            "maxApiCostUsd",
            Some(("maxApiCostUsd", serde_json::json!(1.0))),
        ),
        (
            "maxParallelModelCalls",
            Some(("maxParallelModelCalls", serde_json::json!(1))),
        ),
    ];
    for endpoint in ["from", "to"] {
        for (budget_name, budget) in &budget_cases {
            for self_loop in [false, true] {
                let projection = mutated_fixture(|value| {
                    value["topology"]["budgets"] = serde_json::json!({});
                    if let Some((key, budget_value)) = budget {
                        value["topology"]["budgets"][key] = budget_value.clone();
                    }
                    let foreign = "foreign-endpoint";
                    value["topology"]["edges"] = if self_loop {
                        serde_json::json!([{
                            "id":"foreign-loop", "from":foreign, "to":foreign,
                            "edgeType":"control", "priority":null, "bindings":{}, "condition":null
                        }])
                    } else {
                        serde_json::json!([{
                            "id":"foreign-edge",
                            "from": if endpoint == "from" { foreign } else { "start" },
                            "to": if endpoint == "to" { foreign } else { "start" },
                            "edgeType":"control", "priority":null, "bindings":{}, "condition":null
                        }])
                    };
                });
                let result =
                    std::panic::catch_unwind(|| validate_persisted_projection(&projection));
                assert!(
                    result.is_ok(),
                    "endpoint={endpoint} budget={budget_name} loop={self_loop}"
                );
                assert_eq!(
                    result.unwrap(),
                    Err(graphhelm_graph::GraphError::InvalidProjection),
                    "endpoint={endpoint} budget={budget_name} loop={self_loop}"
                );
            }
        }
    }
}

#[test]
fn persisted_topology_rejects_missing_deploy_target_and_empty_completion() {
    let deploy_without_target = mutated_fixture(|value| {
        value["topology"]["nodes"]["start"]["nodeType"] = serde_json::json!("deploy");
        value["topology"]["nodes"]["start"]["controls"] = serde_json::json!([{
            "controlType": "deploy_configuration", "identifiers": {
                "adapterRef": encode_persisted_reference("deploy://docker-compose@1").unwrap()
            }, "digests": {}, "integers": {}, "flags": {"present": true}
        }]);
    });
    assert!(validate_persisted_projection(&deploy_without_target).is_err());

    let empty_completion = mutated_fixture(|value| {
        value["topology"]["completion"] = serde_json::json!({
            "controlType": "graph_completion", "identifiers": {}, "digests": {},
            "integers": {"terminalCount": 0}, "flags": {"present": true}
        });
    });
    assert!(validate_persisted_projection(&empty_completion).is_err());
}

#[test]
fn replay_mirrors_compensation_requiredness_and_target_type() {
    fn deployment_with(compensation: Option<&str>, target_type: &str) -> PersistedGraphVersion {
        mutated_fixture(|value| {
            add_fixture_agent(value, "z-rollback-node");
            value["topology"]["nodes"]["start"]["nodeType"] = serde_json::json!("deploy");
            value["topology"]["nodes"]["z-rollback-node"]["nodeType"] =
                serde_json::json!(target_type);
            value["topology"]["nodes"]["z-rollback-node"]["controls"] = serde_json::json!([]);
            value["topology"]["nodes"]["start"]["controls"] = serde_json::json!([
                {"controlType":"deploy_configuration","identifiers":compensation.map(|id| serde_json::json!({"compensationNode":id})).unwrap_or_else(|| serde_json::json!({})),"digests":{},"integers":{},"flags":{"present":true,"reversible":true}},
                {"controlType":"node_configuration","identifiers":{"targetRef":encode_persisted_reference("environment://staging").unwrap()},"digests":{},"integers":{},"flags":{"present":true}}
            ]);
        })
    }

    assert!(validate_persisted_projection(&deployment_with(None, "rollback")).is_err());
    assert!(validate_persisted_projection(&deployment_with(Some("start"), "rollback")).is_err());
    assert!(validate_persisted_projection(&deployment_with(Some("missing"), "rollback")).is_err());
    assert!(
        validate_persisted_projection(&deployment_with(Some("z-rollback-node"), "agent")).is_err()
    );
    validate_persisted_projection(&deployment_with(Some("z-rollback-node"), "rollback")).unwrap();

    let required_without_target = mutated_fixture(|value| {
        value["topology"]["nodes"]["start"]["nodeType"] = serde_json::json!("deploy");
        value["topology"]["nodes"]["start"]["controls"] = serde_json::json!([
            {"controlType":"deploy_configuration","identifiers":{},"digests":{},"integers":{},"flags":{"present":true,"compensationRequired":true}},
            {"controlType":"node_configuration","identifiers":{"targetRef":encode_persisted_reference("environment://staging").unwrap()},"digests":{},"integers":{},"flags":{"present":true}}
        ]);
    });
    assert!(validate_persisted_projection(&required_without_target).is_err());
}

#[test]
fn replay_enforces_node_depth_and_retry_budgets_at_the_boundary() {
    let node_equal =
        mutated_fixture(|value| value["topology"]["budgets"]["maxNodes"] = serde_json::json!(1));
    validate_persisted_projection(&node_equal).unwrap();
    let node_exceeded = mutated_fixture(|value| {
        add_fixture_agent(value, "z-second");
        value["topology"]["budgets"]["maxNodes"] = serde_json::json!(1);
    });
    assert!(validate_persisted_projection(&node_exceeded).is_err());

    let depth = |limit| {
        mutated_fixture(|value| {
            add_fixture_agent(value, "z-second");
            value["topology"]["budgets"]["maxDepth"] = serde_json::json!(limit);
            value["topology"]["edges"] = serde_json::json!([{"id":"step","from":"start","to":"z-second","edgeType":"control","priority":null,"bindings":{},"condition":null}]);
            value["topology"]["completion"]["identifiers"]["terminal.000"] =
                serde_json::json!("z-second");
        })
    };
    validate_persisted_projection(&depth(2)).unwrap();
    assert!(validate_persisted_projection(&depth(1)).is_err());

    let retries = |attempts| {
        mutated_fixture(|value| {
            value["topology"]["budgets"]["maxRetriesPerNode"] = serde_json::json!(2);
            value["topology"]["nodes"]["start"]["controls"].as_array_mut().unwrap().push(
            serde_json::json!({"controlType":"node_retry","identifiers":{},"digests":{},"integers":{"maxAttempts":attempts},"flags":{"present":true}})
        );
        })
    };
    validate_persisted_projection(&retries(3)).unwrap();
    assert!(validate_persisted_projection(&retries(4)).is_err());
    let zero_retry_budget = mutated_fixture(|value| {
        value["topology"]["budgets"]["maxRetriesPerNode"] = serde_json::json!(0);
        value["topology"]["nodes"]["start"]["controls"].as_array_mut().unwrap().push(
            serde_json::json!({"controlType":"node_retry","identifiers":{},"digests":{},"integers":{"maxAttempts":1},"flags":{"present":true}})
        );
    });
    validate_persisted_projection(&zero_retry_budget).unwrap();
    let zero_retry_exceeded = mutated_fixture(|value| {
        value["topology"]["budgets"]["maxRetriesPerNode"] = serde_json::json!(0);
        value["topology"]["nodes"]["start"]["controls"].as_array_mut().unwrap().push(
            serde_json::json!({"controlType":"node_retry","identifiers":{},"digests":{},"integers":{"maxAttempts":2},"flags":{"present":true}})
        );
    });
    assert!(validate_persisted_projection(&zero_retry_exceeded).is_err());
}

#[test]
fn replay_requires_unique_terminals() {
    let duplicate_terminal = mutated_fixture(|value| {
        value["topology"]["completion"]["integers"]["terminalCount"] = serde_json::json!(2);
        value["topology"]["completion"]["identifiers"]["terminal.001"] = serde_json::json!("start");
    });
    assert!(validate_persisted_projection(&duplicate_terminal).is_err());
}

#[test]
fn replay_requires_one_capability_per_permission() {
    let missing_capability = mutated_fixture(|value| {
        value["topology"]["nodes"]["start"]["controls"].as_array_mut().unwrap().push(
            serde_json::json!({"controlType":"node_permissions","identifiers":{"duration.000":"call"},"digests":{},"integers":{"permissionCount":1},"flags":{"present":true}})
        );
    });
    assert!(validate_persisted_projection(&missing_capability).is_err());
}

#[test]
fn replay_requires_the_closed_inline_schema_digest_one_of() {
    let inline = mutated_fixture(|value| {
        value["topology"]["nodes"]["start"]["controls"].as_array_mut().unwrap().push(
            serde_json::json!({"controlType":"input_contract","identifiers":{},"digests":{"schema":"a".repeat(64)},"integers":{},"flags":{"present":true}})
        );
    });
    validate_persisted_projection(&inline).unwrap();

    let both = mutated_fixture(|value| {
        value["topology"]["nodes"]["start"]["controls"].as_array_mut().unwrap().push(
            serde_json::json!({"controlType":"input_contract","identifiers":{"schema":encode_persisted_reference("schema://Input@1").unwrap()},"digests":{"schema":"a".repeat(64)},"integers":{},"flags":{"present":true}})
        );
    });
    assert!(validate_persisted_projection(&both).is_err());

    let wrong_digest = mutated_fixture(|value| {
        value["topology"]["nodes"]["start"]["controls"].as_array_mut().unwrap().push(
            serde_json::json!({"controlType":"input_contract","identifiers":{},"digests":{"foreign":"a".repeat(64)},"integers":{},"flags":{"present":true}})
        );
    });
    assert!(validate_persisted_projection(&wrong_digest).is_err());
}

#[test]
fn dot_binding_parser_exposes_and_replay_validates_the_referenced_node() {
    assert_eq!(
        parse_persisted_binding_reference_node("outputs.start.patch").unwrap(),
        Some("start")
    );
    assert_eq!(
        parse_persisted_binding_reference_node(
            encode_persisted_reference("context://task/request")
                .unwrap()
                .as_str()
        )
        .unwrap(),
        None
    );
    let ghost = mutated_fixture(|value| {
        value["topology"]["nodes"]["start"]["controls"].as_array_mut().unwrap().push(
            serde_json::json!({
                "controlType": "input_contract",
                "identifiers": {"bindingKey.000": "patch", "bindingValue.000": "outputs.ghost.patch"},
                "digests": {}, "integers": {"bindingCount": 1}, "flags": {"present": true}
            })
        );
    });
    assert!(validate_persisted_projection(&ghost).is_err());
}

#[test]
fn replay_rejects_registered_reference_families_outside_the_binding_domain() {
    for reference in [
        "schema://Input@1",
        "policy://project/security@1",
        "builtin/repository.read@1",
        "deploy://docker-compose@1",
        "evaluator://quality@1",
        "project/security-reviewer@3",
        "rules://classifier@1",
        "graph-template://release@1",
        "contract://completion@1",
        "document://architecture/auth.md",
    ] {
        let encoded = encode_persisted_reference(reference).unwrap();
        let projection = mutated_fixture(|value| {
            value["topology"]["nodes"]["start"]["controls"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({
                    "controlType": "input_contract",
                    "identifiers": {
                        "bindingKey.000": "request",
                        "bindingValue.000": encoded,
                    },
                    "digests": {}, "integers": {"bindingCount": 1},
                    "flags": {"present": true}
                }));
        });
        assert!(
            validate_persisted_projection(&projection).is_err(),
            "{reference}"
        );
    }

    for reference in [
        "artifact://implementation.diff",
        "artifact://sha256/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "context://task/request",
        "context://claims/accepted",
        "context://user-decisions/release",
        "context://project/settings",
        "environment://staging",
    ] {
        let encoded = encode_persisted_reference(reference).unwrap();
        let projection = mutated_fixture(|value| {
            value["topology"]["nodes"]["start"]["controls"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({
                    "controlType": "input_contract",
                    "identifiers": {
                        "bindingKey.000": "request",
                        "bindingValue.000": encoded,
                    },
                    "digests": {}, "integers": {"bindingCount": 1},
                    "flags": {"present": true}
                }));
        });
        validate_persisted_projection(&projection).unwrap_or_else(|_| panic!("{reference}"));
    }
}

fn add_agent_owner_slot(value: &mut serde_json::Value, link: bool) {
    let owner = OpaqueId::parse("start").unwrap();
    let slot_id = derive_content_slot_id(
        ContentOwnerKind::Agent,
        &owner,
        ContentFieldKind::Purpose,
        0,
    )
    .unwrap();
    let profile = derive_content_slot_profile(
        ContentOwnerKind::Agent,
        &owner,
        ContentFieldKind::Purpose,
        0,
    )
    .unwrap();
    let mut slot = value["contentSlots"][2].clone();
    slot["slotId"] = serde_json::json!(slot_id.as_str());
    slot["ownerKind"] = serde_json::json!("agent");
    slot["ownerId"] = serde_json::json!("start");
    slot["fieldKind"] = serde_json::json!("purpose");
    slot["ordinal"] = serde_json::json!(0);
    slot["evidenceId"] = serde_json::json!("evidence-agent-purpose");
    slot["sensitivity"] = serde_json::json!(match profile.sensitivity() {
        graphhelm_protocols::Sensitivity::Public => "public",
        graphhelm_protocols::Sensitivity::Internal => "internal",
        graphhelm_protocols::Sensitivity::Confidential => "confidential",
        graphhelm_protocols::Sensitivity::Restricted => "restricted",
    });
    slot["requiredForExecution"] = serde_json::json!(profile.required_for_execution());
    value["contentSlots"].as_array_mut().unwrap().push(slot);
    if link {
        value["topology"]["nodes"]["start"]["contentSlotIds"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!(slot_id.as_str()));
    }
}

#[test]
fn agent_owner_slots_belong_only_to_agent_nodes_and_their_exact_image() {
    let non_agent_kinds = [
        "tool",
        "classifier",
        "planner",
        "gate",
        "evaluator",
        "fork",
        "join",
        "human_decision",
        "timer",
        "trigger",
        "subgraph",
        "materializer",
        "deploy",
        "rollback",
        "artifact_transform",
    ];
    for node_type in non_agent_kinds {
        let baseline = mutated_fixture(|value| {
            value["topology"]["nodes"]["start"]["nodeType"] = serde_json::json!(node_type);
            value["topology"]["nodes"]["start"]["controls"] = match node_type {
                "gate" => serde_json::json!([{
                    "controlType":"node_completion",
                    "identifiers":{"contractRef":encode_persisted_reference("contract://done@1").unwrap()},
                    "digests":{}, "integers":{}, "flags":{"present":true}
                }]),
                "deploy" => serde_json::json!([{
                    "controlType":"node_configuration",
                    "identifiers":{"targetRef":encode_persisted_reference("environment://staging").unwrap()},
                    "digests":{}, "integers":{}, "flags":{"present":true}
                }]),
                _ => serde_json::json!([]),
            };
        });
        validate_persisted_projection(&baseline).unwrap_or_else(|_| panic!("baseline {node_type}"));

        let invalid = mutated_fixture(|value| {
            value["topology"]["nodes"]["start"]["nodeType"] = serde_json::json!(node_type);
            value["topology"]["nodes"]["start"]["controls"] = match node_type {
                "gate" => serde_json::json!([{
                    "controlType":"node_completion",
                    "identifiers":{"contractRef":encode_persisted_reference("contract://done@1").unwrap()},
                    "digests":{}, "integers":{}, "flags":{"present":true}
                }]),
                "deploy" => serde_json::json!([{
                    "controlType":"node_configuration",
                    "identifiers":{"targetRef":encode_persisted_reference("environment://staging").unwrap()},
                    "digests":{}, "integers":{}, "flags":{"present":true}
                }]),
                _ => serde_json::json!([]),
            };
            add_agent_owner_slot(value, true);
        });
        assert!(
            validate_persisted_projection(&invalid).is_err(),
            "{node_type}"
        );
    }

    let unlinked = mutated_fixture(|value| add_agent_owner_slot(value, false));
    assert!(validate_persisted_projection(&unlinked).is_err());
}

#[test]
fn replay_rejects_semantic_empty_contracts_and_incomplete_correlated_groups() {
    let empty = mutated_fixture(|value| {
        value["topology"]["nodes"]["start"]["controls"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "controlType": "input_contract", "identifiers": {}, "digests": {},
                "integers": {}, "flags": {"present": true}
            }));
    });
    assert!(validate_persisted_projection(&empty).is_err());

    let half_binding = mutated_fixture(|value| {
        value["topology"]["nodes"]["start"]["controls"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "controlType": "input_contract", "identifiers": {"bindingKey.000": "request"},
                "digests": {}, "integers": {"bindingCount": 1}, "flags": {"present": true}
            }));
    });
    assert!(validate_persisted_projection(&half_binding).is_err());
}

#[test]
fn replay_rejects_hybrid_agent_configuration() {
    let hybrid = mutated_fixture(|value| {
        value["topology"]["nodes"]["start"]["controls"][0]["integers"] =
            serde_json::json!({"capabilityCount": 0});
    });
    assert!(validate_persisted_projection(&hybrid).is_err());
}

#[test]
fn logical_artifact_locators_round_trip_only_through_refv1() {
    let encoded = encode_persisted_reference("artifact://implementation.diff").unwrap();
    assert!(encoded.as_str().starts_with("refv1:"));
    assert!(!encoded.as_str().contains("implementation.diff"));
}

#[test]
fn canonical_content_rejects_values_over_each_shared_preflight_bound() {
    let mut deep = serde_json::json!(null);
    for _ in 0..=64 {
        deep = serde_json::json!([deep]);
    }
    assert!(canonical_content_bytes(&deep).is_err());

    let too_many =
        serde_json::Value::Array((0..131_072).map(|_| serde_json::Value::Null).collect());
    assert!(canonical_content_bytes(&too_many).is_err());

    let too_large = serde_json::Value::String("x".repeat(64 * 1024 * 1024 + 1));
    assert!(canonical_content_bytes(&too_large).is_err());
}

/// The other half of the documented divergence, owned by the crate that implements it (#691).
///
/// `docs/harness/NATIVE_DEVELOPMENT_CONTRACTS.md:579` says this crate requires a prefix **and** a
/// tail minimum, and `:599` states the consequence: a bare `ghp_` is refused by
/// `core/governor` and NOT here. `core/governor/tests/memory.rs` asserts its side; this asserts
/// this one. Neither crate is made to know the other's rule -- each pins the behaviour the doc
/// attributes to IT, which is what keeps this a behaviour test rather than a second copy of the
/// oracle.
///
/// **The boundary is the assertion, not the example.** A cell that only showed a long tail being
/// refused would pass against a detector with no tail rule at all -- which is precisely the graph
/// crate turning into the governor crate, the change this divergence exists to notice. So the pair
/// straddles the documented minimum: `prefix + 15` must pass and `prefix + 16` must not.
///
/// **WHICH condition these values discriminate, because there are two of similar shape.**
/// `contains_prefixed_secret` has a total-length early-out (`text.len() < prefix.len() +
/// minimum_tail`) AND a consecutive-run check that counts bytes after the prefix and breaks on the
/// first one outside `[A-Za-z0-9_-]`. The values below are chosen to reach the SECOND: they sit
/// either side of the run length, not either side of the total length. Anything that mutates only
/// total length leaves these untouched -- which is exactly the mistake made while proving this
/// cell, where a sabotage of overall length reddened nothing because the strings were long enough
/// on both sides of it.
///
/// The charset is therefore load-bearing and is named in the message: widening the run to accept
/// `.` or `/` would make `prefix + 15` pass again for an unrelated reason, and this cell would go
/// on passing while measuring something else.
#[test]
fn a_tail_shorter_than_the_documented_minimum_is_not_refused_here() {
    // ARRANGEMENT: the unmutated fixture validates, so a later Ok is a statement about the VALUE
    // rather than about a validator that accepts anything, and a later Err is about the tail rather
    // than about a fixture that was already invalid.
    validate_persisted_projection(&fixture()).expect(
        "HARNESS-BROKE: the unmutated fixture must validate before any mutation means anything",
    );

    let bare = mutated_fixture(|value| {
        value["topology"]["labels"]["release"] = serde_json::json!("ghp_");
    });
    validate_persisted_projection(&bare).expect(
        "a bare `ghp_` carries no tail, and this crate requires prefix + 16 (docs/harness/NATIVE_DEVELOPMENT_CONTRACTS.md:599)",
    );

    let one_short = mutated_fixture(|value| {
        value["topology"]["labels"]["release"] =
            serde_json::json!(format!("ghp_{}", "a".repeat(15)));
    });
    validate_persisted_projection(&one_short).expect(
        "fifteen [A-Za-z0-9_-] bytes is one short of the documented minimum run and must still pass",
    );

    let at_the_minimum = mutated_fixture(|value| {
        value["topology"]["labels"]["release"] =
            serde_json::json!(format!("ghp_{}", "a".repeat(16)));
    });
    assert_eq!(
        validate_persisted_projection(&at_the_minimum).unwrap_err(),
        graphhelm_graph::GraphError::InvalidProjection,
        "sixteen consecutive [A-Za-z0-9_-] bytes reach the documented minimum run and must be refused"
    );
}
