use std::collections::BTreeMap;

use graphhelm_schema_evolution::{
    CatalogEntry, CatalogResources, CompatibilityChange, CompatibilityClass, SchemaCatalog,
    SemverImpact, compare_catalogs, schema_digest,
};
use semver::Version;
use serde_json::{Value, json};

fn document(name: &str, body: Value) -> Value {
    let mut object = body.as_object().cloned().unwrap();
    object.insert(
        "$id".into(),
        Value::String(format!("https://p50.dev/schemas/{name}.schema.json")),
    );
    object.insert(
        "x-graphhelm-schema-version".into(),
        Value::String("1.0.0".into()),
    );
    Value::Object(object)
}

fn resources(documents: impl IntoIterator<Item = (&'static str, Value)>) -> CatalogResources {
    resources_owned(
        documents
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value)),
    )
}

fn resources_owned(documents: impl IntoIterator<Item = (String, Value)>) -> CatalogResources {
    let mut schemas = BTreeMap::new();
    let mut entries = BTreeMap::new();
    for (name, body) in documents {
        let document = document(&name, body);
        let id = document["$id"].as_str().unwrap().to_owned();
        entries.insert(
            name.clone(),
            CatalogEntry {
                id,
                document_version: Version::parse("1.0.0").unwrap(),
                path: format!("schemas/{name}.schema.json"),
                sha256: schema_digest(&document).unwrap(),
            },
        );
        schemas.insert(name, document);
    }
    CatalogResources {
        catalog_source: "schemas/catalog.json".into(),
        catalog: SchemaCatalog {
            format_version: 1,
            release_version: Version::parse("1.0.0").unwrap(),
            schemas: entries,
        },
        schemas,
    }
}

fn reference_chain_resources(length: usize) -> CatalogResources {
    resources_owned((0..length).map(|index| {
        let name = format!("chain-{index:03}");
        let body = if index + 1 == length {
            json!({"type":"string"})
        } else {
            json!({"$ref":format!("chain-{:03}.schema.json", index + 1)})
        };
        (name, body)
    }))
}

fn compare(baseline: Value, candidate: Value) -> graphhelm_schema_evolution::CompatibilityReport {
    compare_catalogs(
        &resources([("graph", baseline)]),
        &resources([("graph", candidate)]),
    )
}

fn fixture(name: &str) -> Value {
    serde_json::from_str(match name {
        "annotation-only.json" => {
            include_str!("../../../conformance/compatibility/annotation-only.json")
        }
        "compatible-optional-property.json" => {
            include_str!("../../../conformance/compatibility/compatible-optional-property.json")
        }
        "breaking-required-property.json" => {
            include_str!("../../../conformance/compatibility/breaking-required-property.json")
        }
        "breaking-unknown-keyword.json" => {
            include_str!("../../../conformance/compatibility/breaking-unknown-keyword.json")
        }
        "unresolved-ref.json" => {
            include_str!("../../../conformance/compatibility/unresolved-ref.json")
        }
        _ => panic!("unknown fixture"),
    })
    .unwrap()
}

fn compare_fixture(name: &str) -> graphhelm_schema_evolution::CompatibilityReport {
    let fixture = fixture(name);
    compare(fixture["baseline"].clone(), fixture["candidate"].clone())
}

fn assert_change(
    report: &graphhelm_schema_evolution::CompatibilityReport,
    class: CompatibilityClass,
    impact: SemverImpact,
    code: &str,
    pointer: &str,
) {
    assert_eq!(report.class, class);
    assert_eq!(report.impact, impact);
    assert!(report.changes.iter().any(|change| {
        change.class == class
            && change.impact == impact
            && change.code == code
            && change.pointer == pointer
    }));
}

// Prevents the primary compatibility branches from being inverted or falling through to unknown.
#[test]
fn representative_changes_have_exact_impacts() {
    for (fixture, class, impact, code, pointer) in [
        (
            "annotation-only.json",
            CompatibilityClass::Annotation,
            SemverImpact::Patch,
            "GHC101_ANNOTATION_CHANGED",
            "/description",
        ),
        (
            "compatible-optional-property.json",
            CompatibilityClass::Compatible,
            SemverImpact::Minor,
            "GHC102_OPTIONAL_PROPERTY_ADDED",
            "/properties/nickname",
        ),
        (
            "breaking-required-property.json",
            CompatibilityClass::Breaking,
            SemverImpact::Major,
            "GHC003_BREAKING_CHANGE",
            "/required",
        ),
        (
            "breaking-unknown-keyword.json",
            CompatibilityClass::Breaking,
            SemverImpact::Major,
            "GHC003_BREAKING_CHANGE",
            "/mysteryConstraint",
        ),
    ] {
        assert_change(&compare_fixture(fixture), class, impact, code, pointer);
    }
}

// Prevents registered metadata edits from forcing a validation-level release.
#[test]
fn every_registered_annotation_is_patch_only() {
    let annotations = [
        "title",
        "description",
        "$comment",
        "examples",
        "default",
        "deprecated",
        "readOnly",
        "writeOnly",
    ];
    let baseline = Value::Object(
        annotations
            .into_iter()
            .map(|keyword| (keyword.to_owned(), json!("baseline-secret")))
            .collect(),
    );
    let candidate = Value::Object(
        annotations
            .into_iter()
            .map(|keyword| (keyword.to_owned(), json!("candidate-secret")))
            .collect(),
    );
    let report = compare(baseline, candidate);
    assert_eq!(report.class, CompatibilityClass::Annotation);
    assert_eq!(report.impact, SemverImpact::Patch);
    assert_eq!(report.changes.len(), 8);
    assert!(
        report
            .changes
            .iter()
            .all(|change| change.code == "GHC101_ANNOTATION_CHANGED")
    );
}

// Prevents removed fields or newly required fields from silently rejecting existing payloads.
#[test]
fn property_and_required_sets_follow_acceptance_direction() {
    let property = json!({"type":"object","properties":{"name":{"type":"string"}}});
    assert_change(
        &compare(property, json!({"type":"object","properties":{}})),
        CompatibilityClass::Breaking,
        SemverImpact::Major,
        "GHC003_BREAKING_CHANGE",
        "/properties/name",
    );
    assert_change(
        &compare(
            json!({"type":"object","required":["name"]}),
            json!({"type":"object","required":[]}),
        ),
        CompatibilityClass::Compatible,
        SemverImpact::Minor,
        "GHC103_COMPATIBLE_CHANGE",
        "/required",
    );
}

// Prevents set-direction mistakes that label a narrowed accepted domain as compatible.
#[test]
fn type_and_enum_sets_classify_widening_and_narrowing() {
    for keyword in ["type", "enum"] {
        let baseline = json!({keyword:["string"]});
        let widened = json!({keyword:["string","null"]});
        assert_change(
            &compare(baseline.clone(), widened.clone()),
            CompatibilityClass::Compatible,
            SemverImpact::Minor,
            "GHC103_COMPATIBLE_CHANGE",
            &format!("/{keyword}"),
        );
        assert_change(
            &compare(widened, baseline),
            CompatibilityClass::Breaking,
            SemverImpact::Major,
            "GHC003_BREAKING_CHANGE",
            &format!("/{keyword}"),
        );
        assert_change(
            &compare(json!({}), json!({keyword:["string"]})),
            CompatibilityClass::Breaking,
            SemverImpact::Major,
            "GHC003_BREAKING_CHANGE",
            &format!("/{keyword}"),
        );
        assert_change(
            &compare(json!({keyword:["string"]}), json!({})),
            CompatibilityClass::Compatible,
            SemverImpact::Minor,
            "GHC103_COMPATIBLE_CHANGE",
            &format!("/{keyword}"),
        );
        let reordered = compare(
            json!({keyword:["string","null"]}),
            json!({keyword:["null","string"]}),
        );
        assert_eq!(reordered.class, CompatibilityClass::Unchanged);
        assert!(reordered.changes.is_empty());
    }
}

// Prevents JSON Schema's integer subset of number from being treated as unrelated types.
#[test]
fn integer_to_number_is_widening_for_primary_and_union_types() {
    for (baseline, candidate, class, impact, code) in [
        (
            json!({"type":"integer"}),
            json!({"type":"number"}),
            CompatibilityClass::Compatible,
            SemverImpact::Minor,
            "GHC103_COMPATIBLE_CHANGE",
        ),
        (
            json!({"type":"number"}),
            json!({"type":"integer"}),
            CompatibilityClass::Breaking,
            SemverImpact::Major,
            "GHC003_BREAKING_CHANGE",
        ),
        (
            json!({"type":["string","integer"]}),
            json!({"type":["string","number"]}),
            CompatibilityClass::Compatible,
            SemverImpact::Minor,
            "GHC103_COMPATIBLE_CHANGE",
        ),
        (
            json!({"type":["string","number"]}),
            json!({"type":["string","integer"]}),
            CompatibilityClass::Breaking,
            SemverImpact::Major,
            "GHC003_BREAKING_CHANGE",
        ),
    ] {
        assert_change(&compare(baseline, candidate), class, impact, code, "/type");
    }
}

// Prevents const constraints from accepting a silent identity change or misclassifying relaxation.
#[test]
fn const_add_change_and_removal_are_directional() {
    assert_change(
        &compare(json!({}), json!({"const":"x"})),
        CompatibilityClass::Breaking,
        SemverImpact::Major,
        "GHC003_BREAKING_CHANGE",
        "/const",
    );
    assert_change(
        &compare(json!({"const":"x"}), json!({"const":"y"})),
        CompatibilityClass::Breaking,
        SemverImpact::Major,
        "GHC003_BREAKING_CHANGE",
        "/const",
    );
    assert_change(
        &compare(json!({"const":"x"}), json!({})),
        CompatibilityClass::Compatible,
        SemverImpact::Minor,
        "GHC103_COMPATIBLE_CHANGE",
        "/const",
    );
}

// Prevents inverted lower/upper bound logic across numbers, strings, and arrays.
#[test]
fn validation_bounds_classify_relaxation_and_restriction() {
    for keyword in ["minimum", "exclusiveMinimum", "minLength", "minItems"] {
        assert_keyword_change(keyword, json!(5), json!(4), CompatibilityClass::Compatible);
        assert_keyword_change(keyword, json!(4), json!(5), CompatibilityClass::Breaking);
    }
    for keyword in ["maximum", "exclusiveMaximum", "maxLength", "maxItems"] {
        assert_keyword_change(keyword, json!(4), json!(5), CompatibilityClass::Compatible);
        assert_keyword_change(keyword, json!(5), json!(4), CompatibilityClass::Breaking);
    }
}

fn assert_keyword_change(
    keyword: &str,
    baseline: Value,
    candidate: Value,
    class: CompatibilityClass,
) {
    let report = compare(json!({keyword:baseline}), json!({keyword:candidate}));
    let impact = if class == CompatibilityClass::Compatible {
        SemverImpact::Minor
    } else {
        SemverImpact::Major
    };
    let code = if class == CompatibilityClass::Compatible {
        "GHC103_COMPATIBLE_CHANGE"
    } else {
        "GHC003_BREAKING_CHANGE"
    };
    assert_change(&report, class, impact, code, &format!("/{keyword}"));
}

// Prevents opaque regex/format edits from being guessed equivalent while allowing constraint removal.
#[test]
fn pattern_and_format_changes_are_conservative() {
    for keyword in ["pattern", "format"] {
        assert_change(
            &compare(json!({}), json!({keyword:"private-expression"})),
            CompatibilityClass::Breaking,
            SemverImpact::Major,
            "GHC003_BREAKING_CHANGE",
            &format!("/{keyword}"),
        );
        assert_change(
            &compare(json!({keyword:"private-expression"}), json!({})),
            CompatibilityClass::Compatible,
            SemverImpact::Minor,
            "GHC103_COMPATIBLE_CHANGE",
            &format!("/{keyword}"),
        );
    }
}

// Prevents wire identity and discriminator changes from being treated as validation relaxation.
#[test]
fn schema_identity_refs_and_discriminators_are_breaking() {
    let mut candidate = document("graph", json!({}));
    candidate["$id"] = json!("https://p50.dev/schemas/renamed.schema.json");
    let baseline = resources([("graph", json!({}))]);
    let mut candidate_resources = resources([("graph", json!({}))]);
    candidate_resources
        .schemas
        .insert("graph".into(), candidate);
    assert_change(
        &compare_catalogs(&baseline, &candidate_resources),
        CompatibilityClass::Breaking,
        SemverImpact::Major,
        "GHC003_BREAKING_CHANGE",
        "/$id",
    );

    assert_change(
        &compare(
            json!({"properties":{"apiVersion":{"const":"v1"}}}),
            json!({"properties":{"apiVersion":{"const":"v2"}}}),
        ),
        CompatibilityClass::Breaking,
        SemverImpact::Major,
        "GHC003_BREAKING_CHANGE",
        "/properties/apiVersion/const",
    );
    assert_change(
        &compare(
            json!({"discriminator":{"propertyName":"kind"}}),
            json!({"discriminator":{"propertyName":"type"}}),
        ),
        CompatibilityClass::Breaking,
        SemverImpact::Major,
        "GHC003_BREAKING_CHANGE",
        "/discriminator",
    );
}

// Prevents ref spelling from creating a false change and blocks missing or remote resources offline.
#[test]
fn refs_resolve_to_stable_supplied_resource_ids() {
    let baseline = resources([
        ("graph", json!({"$ref":"node.schema.json"})),
        ("node", json!({"type":"string"})),
    ]);
    let candidate = resources([
        (
            "graph",
            json!({"$ref":"https://p50.dev/schemas/node.schema.json"}),
        ),
        ("node", json!({"type":"string"})),
    ]);
    let report = compare_catalogs(&baseline, &candidate);
    assert_eq!(report.class, CompatibilityClass::Unchanged);
    assert_eq!(report.impact, SemverImpact::None);
    assert!(report.changes.is_empty());

    for candidate_ref in ["missing.schema.json", "https://example.com/remote.json"] {
        assert_change(
            &compare(json!({"type":"object"}), json!({"$ref":candidate_ref})),
            CompatibilityClass::Breaking,
            SemverImpact::Major,
            "GHC003_BREAKING_CHANGE",
            "/$ref",
        );
    }
    assert_change(
        &compare_fixture("unresolved-ref.json"),
        CompatibilityClass::Breaking,
        SemverImpact::Major,
        "GHC003_BREAKING_CHANGE",
        "/$ref",
    );

    let unresolved_on_both_sides = compare(
        json!({"$ref":"missing.schema.json"}),
        json!({"$ref":"missing.schema.json"}),
    );
    assert_change(
        &unresolved_on_both_sides,
        CompatibilityClass::Breaking,
        SemverImpact::Major,
        "GHC003_BREAKING_CHANGE",
        "/$ref",
    );
}

// Prevents candidate-only schema subtrees from hiding unresolved or remote references.
#[test]
fn candidate_only_schema_subtrees_fail_at_the_exact_ref_pointer() {
    let optional_property = compare(
        json!({"type":"object","properties":{}}),
        json!({"type":"object","properties":{"nickname":{"$ref":"missing.schema.json"}}}),
    );
    assert_change(
        &optional_property,
        CompatibilityClass::Breaking,
        SemverImpact::Major,
        "GHC003_BREAKING_CHANGE",
        "/properties/nickname/$ref",
    );

    let added_schema = compare_catalogs(
        &resources(std::iter::empty()),
        &resources([(
            "graph",
            json!({"$ref":"https://example.com/remote.schema.json"}),
        )]),
    );
    assert_change(
        &added_schema,
        CompatibilityClass::Breaking,
        SemverImpact::Major,
        "GHC003_BREAKING_CHANGE",
        "/$ref",
    );

    let added_definition = compare(
        json!({"$defs":{}}),
        json!({"$defs":{"new":{"$ref":"missing.schema.json"}}}),
    );
    assert_change(
        &added_definition,
        CompatibilityClass::Breaking,
        SemverImpact::Major,
        "GHC003_BREAKING_CHANGE",
        "/$defs/new/$ref",
    );
}

// Prevents a candidate-only draft-2020-12 unevaluatedItems schema from bypassing ref preflight.
#[test]
fn candidate_unevaluated_items_fails_at_the_exact_unresolved_ref() {
    let report = compare_catalogs(
        &resources(std::iter::empty()),
        &resources([(
            "graph",
            json!({"unevaluatedItems":{"$ref":"missing.schema.json"}}),
        )]),
    );
    assert_change(
        &report,
        CompatibilityClass::Breaking,
        SemverImpact::Major,
        "GHC003_BREAKING_CHANGE",
        "/unevaluatedItems/$ref",
    );
}

// Prevents candidate contentSchema branches from bypassing offline reference preflight.
#[test]
fn candidate_content_schema_fails_at_the_exact_unresolved_ref() {
    for reference in [
        "missing.schema.json",
        "https://example.com/remote.schema.json",
    ] {
        let report = compare_catalogs(
            &resources(std::iter::empty()),
            &resources([("graph", json!({"contentSchema":{"$ref":reference}}))]),
        );
        assert_change(
            &report,
            CompatibilityClass::Breaking,
            SemverImpact::Major,
            "GHC003_BREAKING_CHANGE",
            "/contentSchema/$ref",
        );
    }
}

// Prevents composition fingerprints from treating equivalent refs under unevaluatedItems as different.
#[test]
fn unevaluated_items_refs_normalize_only_in_schema_context() {
    let baseline = resources([
        (
            "graph",
            json!({"anyOf":[
                {"unevaluatedItems":{
                    "$ref":"node.schema.json",
                    "const":{"$ref":"https://example.com/const-literal"},
                    "examples":[{"$ref":"https://example.com/example-literal"}]
                }},
                {"type":"null"}
            ]}),
        ),
        ("node", json!({"type":"string"})),
    ]);
    let candidate = resources([
        (
            "graph",
            json!({"anyOf":[
                {"type":"null"},
                {"unevaluatedItems":{
                    "$ref":"https://p50.dev/schemas/node.schema.json",
                    "const":{"$ref":"https://example.com/const-literal"},
                    "examples":[{"$ref":"https://example.com/example-literal"}]
                }}
            ]}),
        ),
        ("node", json!({"type":"string"})),
    ]);
    let report = compare_catalogs(&baseline, &candidate);
    assert_eq!(report.class, CompatibilityClass::Unchanged);
    assert_eq!(report.impact, SemverImpact::None);
    assert!(report.changes.is_empty());
}

// Prevents removing a reference binding from being treated as a validation relaxation.
#[test]
fn ref_removal_is_a_breaking_binding_change() {
    let baseline = resources([
        ("graph", json!({"$ref":"node.schema.json"})),
        ("node", json!({"type":"string"})),
    ]);
    let candidate = resources([
        ("graph", json!({"type":"string"})),
        ("node", json!({"type":"string"})),
    ]);
    assert_change(
        &compare_catalogs(&baseline, &candidate),
        CompatibilityClass::Breaking,
        SemverImpact::Major,
        "GHC003_BREAKING_CHANGE",
        "/$ref",
    );
}

// Prevents an attacker-controlled acyclic reference chain from bypassing the fixed depth budget.
#[test]
fn reference_chains_longer_than_the_depth_limit_fail_closed() {
    let baseline = reference_chain_resources(130);
    let candidate = reference_chain_resources(130);
    let report = compare_catalogs(&baseline, &candidate);
    assert_change(
        &report,
        CompatibilityClass::Breaking,
        SemverImpact::Major,
        "GHC003_BREAKING_CHANGE",
        "/$ref",
    );
    assert!(
        report
            .changes
            .iter()
            .any(|change| change.schema == "chain-000" && change.pointer == "/$ref")
    );
}

// Prevents literal instance data named `$ref` from being interpreted as schema syntax.
#[test]
fn literal_ref_keys_in_data_keywords_are_not_resolved() {
    let schema = json!({
        "const":{"$ref":"https://example.com/const-literal"},
        "enum":[{"$ref":"https://example.com/enum-literal"}],
        "default":{"$ref":"https://example.com/default-literal"},
        "examples":[{"$ref":"https://example.com/example-literal"}]
    });
    let report = compare(schema.clone(), schema);
    assert_eq!(report.class, CompatibilityClass::Unchanged);
    assert_eq!(report.impact, SemverImpact::None);
    assert!(report.changes.is_empty());

    let reordered_composition = compare(
        json!({"anyOf":[
            {"const":{"$ref":"https://example.com/const-literal"}},
            {"enum":[{"$ref":"https://example.com/enum-literal"}]}
        ]}),
        json!({"anyOf":[
            {"enum":[{"$ref":"https://example.com/enum-literal"}]},
            {"const":{"$ref":"https://example.com/const-literal"}}
        ]}),
    );
    assert_eq!(reordered_composition.class, CompatibilityClass::Unchanged);
    assert!(reordered_composition.changes.is_empty());
}

// Prevents recursive local schemas from causing unbounded reference traversal.
#[test]
fn recursive_local_ref_cycles_terminate_deterministically() {
    let report = compare(
        json!({"$ref":"#"}),
        json!({"$ref":"https://p50.dev/schemas/graph.schema.json"}),
    );
    assert_eq!(report.class, CompatibilityClass::Unchanged);
    assert!(report.changes.is_empty());
}

// Prevents permissiveness transitions for object tails from being reversed.
#[test]
fn additional_and_unevaluated_properties_classify_boolean_or_schema_transitions() {
    for keyword in ["additionalProperties", "unevaluatedProperties"] {
        assert_keyword_change(
            keyword,
            json!(true),
            json!(false),
            CompatibilityClass::Breaking,
        );
        assert_keyword_change(
            keyword,
            json!(false),
            json!({"type":"string"}),
            CompatibilityClass::Compatible,
        );
        assert_keyword_change(
            keyword,
            json!({"type":"string"}),
            json!(true),
            CompatibilityClass::Compatible,
        );
        assert_keyword_change(
            keyword,
            json!(true),
            json!({"type":"string"}),
            CompatibilityClass::Breaking,
        );
    }
}

// Prevents array item constraints from being classified in the wrong direction.
#[test]
fn array_items_changes_follow_nested_acceptance_direction() {
    assert_change(
        &compare(json!({}), json!({"items":{"type":"string"}})),
        CompatibilityClass::Breaking,
        SemverImpact::Major,
        "GHC003_BREAKING_CHANGE",
        "/items",
    );
    assert_change(
        &compare(json!({"items":{"type":"string"}}), json!({})),
        CompatibilityClass::Compatible,
        SemverImpact::Minor,
        "GHC103_COMPATIBLE_CHANGE",
        "/items",
    );
    assert_change(
        &compare(
            json!({"items":{"type":"string"}}),
            json!({"items":{"type":["string","null"]}}),
        ),
        CompatibilityClass::Compatible,
        SemverImpact::Minor,
        "GHC103_COMPATIBLE_CHANGE",
        "/items/type",
    );
}

// Prevents boolean schemas from reversing the empty/schema/universal acceptance ordering.
#[test]
fn boolean_schema_relaxations_are_directional_for_items_and_tail_policies() {
    for keyword in ["items", "additionalProperties"] {
        assert_keyword_change(
            keyword,
            json!(false),
            json!({"type":"string"}),
            CompatibilityClass::Compatible,
        );
        assert_keyword_change(
            keyword,
            json!({"type":"string"}),
            json!(true),
            CompatibilityClass::Compatible,
        );
        assert_keyword_change(
            keyword,
            json!(true),
            json!({"type":"string"}),
            CompatibilityClass::Breaking,
        );
        assert_keyword_change(
            keyword,
            json!({"type":"string"}),
            json!(false),
            CompatibilityClass::Breaking,
        );
    }
}

// Prevents union/intersection branch direction from being swapped and ambiguous negation from passing.
#[test]
fn composition_changes_use_explicit_conservative_rules() {
    let string = json!({"type":"string"});
    let number = json!({"type":"number"});
    for keyword in ["anyOf", "oneOf"] {
        assert_keyword_change(
            keyword,
            json!([string.clone()]),
            json!([string.clone(), number.clone()]),
            CompatibilityClass::Compatible,
        );
        assert_keyword_change(
            keyword,
            json!([string.clone(), number.clone()]),
            json!([string.clone()]),
            CompatibilityClass::Breaking,
        );
    }
    assert_keyword_change(
        "oneOf",
        json!([string.clone()]),
        json!([string.clone(), {"type":"string","maxLength":8}]),
        CompatibilityClass::Breaking,
    );
    assert_keyword_change(
        "allOf",
        json!([string.clone(), {"maxLength": 8}]),
        json!([string.clone()]),
        CompatibilityClass::Compatible,
    );
    assert_keyword_change(
        "allOf",
        json!([string.clone()]),
        json!([string.clone(), {"maxLength": 8}]),
        CompatibilityClass::Breaking,
    );
    for keyword in ["allOf", "anyOf", "oneOf"] {
        assert_change(
            &compare(json!({keyword:[string.clone()]}), json!({})),
            CompatibilityClass::Compatible,
            SemverImpact::Minor,
            "GHC103_COMPATIBLE_CHANGE",
            &format!("/{keyword}"),
        );
        assert_change(
            &compare(json!({}), json!({keyword:[string.clone()]})),
            CompatibilityClass::Breaking,
            SemverImpact::Major,
            "GHC003_BREAKING_CHANGE",
            &format!("/{keyword}"),
        );
        let reordered = compare(
            json!({keyword:[string.clone(), number.clone()]}),
            json!({keyword:[number.clone(), string.clone()]}),
        );
        assert_eq!(reordered.class, CompatibilityClass::Unchanged);
        assert!(reordered.changes.is_empty());
    }
    assert_keyword_change(
        "not",
        json!({"type":"string"}),
        json!({"type":"number"}),
        CompatibilityClass::Breaking,
    );
    assert_keyword_change(
        "not",
        json!({"type":"string"}),
        Value::Null,
        CompatibilityClass::Breaking,
    );
    assert_change(
        &compare(json!({"not":{"type":"string"}}), json!({})),
        CompatibilityClass::Compatible,
        SemverImpact::Minor,
        "GHC103_COMPATIBLE_CHANGE",
        "/not",
    );
}

// Prevents no-op catalogs from requiring a release or emitting unstable noise.
#[test]
fn unchanged_schemas_have_no_impact_or_changes() {
    let schema = json!({"type":"object","properties":{"name":{"type":"string"}}});
    let report = compare(schema.clone(), schema);
    assert_eq!(report.class, CompatibilityClass::Unchanged);
    assert_eq!(report.impact, SemverImpact::None);
    assert!(report.compatible);
    assert!(report.changes.is_empty());
}

// Prevents map/input order and sensitive schema values from affecting or leaking report payloads.
#[test]
fn reports_are_sorted_deterministic_and_payload_safe() {
    let report = compare(
        json!({"pattern":"baseline-secret","description":"baseline-example"}),
        json!({"pattern":"candidate-secret","description":"candidate-example","zUnknown":"raw-secret"}),
    );
    let keys = report
        .changes
        .iter()
        .map(|change| (&change.schema, &change.pointer, &change.code))
        .collect::<Vec<_>>();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
    let encoded = serde_json::to_string(&report).unwrap();
    for secret in [
        "baseline-secret",
        "candidate-secret",
        "baseline-example",
        "candidate-example",
        "raw-secret",
    ] {
        assert!(!encoded.contains(secret));
    }
    assert!(report.changes.iter().all(|change| {
        change.baseline_summary.len() <= 64 && change.candidate_summary.len() <= 64
    }));
}

// Prevents aggregate severity from depending on which change happened to be visited last.
#[test]
fn report_uses_the_most_severe_change() {
    let report = compare(
        json!({"description":"before","enum":["a"]}),
        json!({"description":"after","enum":["a","b"],"unknown":true}),
    );
    assert_eq!(report.class, CompatibilityClass::Breaking);
    assert_eq!(report.impact, SemverImpact::Major);
    assert!(!report.compatible);
}

fn _assert_public_change_type(_: &CompatibilityChange) {}

/// #517 R1 — THE CELL THE TICKET EXISTS FOR, and it is red before the arm lands.
///
/// `description` is in the annotation keyword set and `Annotation => Patch`, but only for a
/// position the checker DESCENDS INTO. `prefixItems` has no arm in the keyword `match`, so the
/// whole keyword falls to the default and any change under it — including a sentence of prose —
/// is priced `GHC003_BREAKING_CHANGE`. Measured on #515: the same description in `$defs` needs no
/// bump; inside `prefixItems` it demands a major one.
///
/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL once the arm exists: removing the `prefixItems`
/// arm, or making it compare the arrays as opaque values instead of position by position.
#[test]
fn an_annotation_inside_prefix_items_is_an_annotation() {
    let baseline = json!({
        "type": "array",
        "prefixItems": [{"type": "string"}, {"type": "integer"}]
    });
    let candidate = json!({
        "type": "array",
        "prefixItems": [
            {"type": "string", "description": "what this position carries"},
            {"type": "integer"}
        ]
    });

    let report = compare(baseline, candidate);

    assert_eq!(
        report.class,
        CompatibilityClass::Annotation,
        "a sentence added to a tuple position is prose, not a contract change; got {:?}",
        report.changes
    );
    assert_eq!(report.impact, SemverImpact::Patch);
    assert_eq!(report.changes.len(), 1);
    assert_eq!(report.changes[0].code, "GHC101_ANNOTATION_CHANGED");
    assert_eq!(
        report.changes[0].pointer, "/prefixItems/0/description",
        "the pointer must name the POSITION, or a reader cannot tell which tuple slot moved"
    );
}

/// #517 R2 — the other direction, red today for the same reason.
///
/// Dropping a trailing `prefixItems` entry leaves that position unconstrained, which is the
/// loosening `compare_items` already classifies as `Compatible` when `items` disappears. Today it
/// prices as breaking, like everything else under this keyword.
///
/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL: treating any length difference as breaking, which
/// is the cheapest arm that satisfies R1 alone.
#[test]
fn dropping_a_prefix_items_position_is_compatible() {
    let baseline = json!({
        "type": "array",
        "prefixItems": [{"type": "string"}, {"type": "integer"}]
    });
    let candidate = json!({"type": "array", "prefixItems": [{"type": "string"}]});

    let report = compare(baseline, candidate);

    assert_eq!(
        report.class,
        CompatibilityClass::Compatible,
        "a position that stops being constrained accepts everything it used to accept; got {:?}",
        report.changes
    );
    assert_eq!(report.impact, SemverImpact::Minor);
}

/// #517 R3 — GREEN BEFORE THE FIX, AND SAID SO RATHER THAN COUNTED AS A RED.
///
/// Adding a tuple position constrains input that used to be free, so it must stay breaking. Today
/// it passes because EVERYTHING under `prefixItems` is breaking — it passes for the wrong reason,
/// and a green here proves nothing until the arm exists. It is here because R1 and R2 both push
/// toward leniency, and the arm that satisfies them most cheaply is one that never reports
/// breaking at all. Its worth is measured by sabotage after the arm lands, not by this run.
#[test]
fn adding_a_prefix_items_position_stays_breaking() {
    let baseline = json!({"type": "array", "prefixItems": [{"type": "string"}]});
    let candidate = json!({
        "type": "array",
        "prefixItems": [{"type": "string"}, {"type": "integer"}]
    });

    let report = compare(baseline, candidate);

    assert_eq!(
        report.class,
        CompatibilityClass::Breaking,
        "a newly constrained position rejects documents the baseline accepted; got {:?}",
        report.changes
    );
    assert_eq!(report.impact, SemverImpact::Major);
}

/// #517 R4 — the cell that says POSITIONAL rather than merely "descends".
///
/// R1-R3 are all satisfied by an arm that treats `prefixItems` as a SET, the way
/// `compare_composition` treats `allOf`/`anyOf`/`oneOf`. This one is not: swapping two positions
/// leaves the set identical while changing the contract completely — index 0 stops accepting the
/// strings it accepted and starts demanding integers. A set comparison reports "unchanged" here,
/// which is a breaking change rendered as no change at all.
///
/// The comparator's doc comment makes exactly this claim about why it is not a copy of
/// `compare_composition`. This is that claim written as a test rather than left as prose.
///
/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL: sorting or set-comparing the two arrays before
/// walking them.
#[test]
fn swapping_two_prefix_items_positions_is_breaking() {
    let baseline = json!({
        "type": "array",
        "prefixItems": [{"type": "string"}, {"type": "integer"}]
    });
    let candidate = json!({
        "type": "array",
        "prefixItems": [{"type": "integer"}, {"type": "string"}]
    });

    let report = compare(baseline, candidate);

    assert_eq!(
        report.class,
        CompatibilityClass::Breaking,
        "the SET of tuple positions is unchanged here and the CONTRACT is not; a comparison that \
         cannot tell those apart reports a breaking change as no change; got {:?}",
        report.changes
    );
    assert!(
        report
            .changes
            .iter()
            .any(|change| change.pointer.starts_with("/prefixItems/0")),
        "the first position is the one that changed and the report must name it: {:?}",
        report.changes
    );
}

/// A tagged union grows by one tag, and the discriminator proves the branches cannot overlap.
///
/// **Every branch of a tagged union has the SAME top-level type.** `event-envelope.schema.json`'s
/// `$defs/eventKind` is 44 branches, all `"type": "object"`, all discriminated one level down by
/// `properties.type.const` -- and `branch_types_are_disjoint` compares only the top-level `"type"`
/// keyword, so `"object" != "object"` is false for every pair. The prover cannot classify ANY
/// addition to a const-discriminated union as compatible, no matter how obviously the tags
/// separate (#626). The cost lands on `baseline_origin.rs`: a purely additive wire-vocabulary
/// growth reads as `GHC003_BREAKING_CHANGE` and then demands the exact next major to count as
/// declared, which is a semantic lie about an additive change.
#[test]
fn a_new_const_discriminated_branch_is_compatible_because_the_tags_cannot_overlap() {
    let imported = json!({
        "type": "object",
        "required": ["type", "data"],
        "properties": {"type": {"const": "graph_imported"}, "data": {"type": "object"}},
    });
    let published = json!({
        "type": "object",
        "required": ["type", "data"],
        "properties": {"type": {"const": "graph_version_published"}, "data": {"type": "object"}},
    });

    assert_keyword_change(
        "oneOf",
        json!([imported.clone()]),
        json!([imported, published]),
        CompatibilityClass::Compatible,
    );
}

/// The discriminator proves disjointness only when an instance CANNOT omit it.
///
/// **The trap for the fix this file is about to receive.** Comparing `properties.<tag>.const` and
/// stopping there is unsound: if the tag is not `required`, an instance carrying neither tag
/// satisfies BOTH branches, they overlap, and a `oneOf` that must match exactly one is broken by
/// the addition. A prover that called this pair disjoint would turn today's false BREAKING into a
/// false COMPATIBLE -- silent, and strictly worse, because nothing downstream re-checks it.
///
/// The three arrangements below stay unprovable for three different reasons, and each is a
/// separate way the naive fix goes wrong: the tag is optional, the tags are EQUAL, or there is no
/// tag at all. Only the first is new; the last is the behaviour that must not regress.
#[test]
fn a_const_discriminator_proves_nothing_when_an_instance_could_omit_it() {
    let optional_tag = |tag: &str| {
        json!({
            "type": "object",
            "properties": {"type": {"const": tag}, "data": {"type": "object"}},
        })
    };
    let required_tag = |tag: &str| {
        json!({
            "type": "object",
            "required": ["type", "data"],
            "properties": {"type": {"const": tag}, "data": {"type": "object"}},
        })
    };
    let untagged = json!({"type": "object", "properties": {"data": {"type": "object"}}});

    for (baseline, candidate) in [
        // The tag is present and different in both, but neither branch REQUIRES it.
        (optional_tag("a"), optional_tag("b")),
        // Required in both, and the same tag: the discriminator separates nothing.
        (required_tag("a"), required_tag("a")),
        // No discriminator at all -- the pre-existing conservative answer, unchanged.
        (untagged.clone(), json!({"type": "object"})),
        // NUMERIC tags that JSON Schema considers EQUAL. `const: 1` and `const: 1.0` are the same
        // value to a validator, so these branches overlap -- but `serde_json`'s `PartialEq` on
        // `Number` does not say they are equal, and a proof that trusted it would call them
        // disjoint. The prover answers only for string tags, so this stays unprovable.
        (
            json!({
                "type": "object",
                "required": ["type"],
                "properties": {"type": {"const": 1}},
            }),
            json!({
                "type": "object",
                "required": ["type"],
                "properties": {"type": {"const": 1.0}},
            }),
        ),
        // The tag is required and the string consts differ, but neither branch says
        // `"type": "object"` -- and `required`/`properties` are NO-OPS for a non-object instance.
        // The number `42` satisfies both branches, so they overlap, and the discriminator argument
        // never applied: it reasons about properties an instance need not have at all
        // (Codex P1 on PR #627).
        (
            json!({"required": ["type"], "properties": {"type": {"const": "a"}}}),
            json!({"required": ["type"], "properties": {"type": {"const": "b"}}}),
        ),
        // Object is ALLOWED but not the only option. A string instance satisfies both branches for
        // the same reason, so a type SET wider than exactly `{object}` proves nothing either.
        (
            json!({
                "type": ["object", "string"],
                "required": ["type"],
                "properties": {"type": {"const": "a"}},
            }),
            json!({
                "type": ["object", "string"],
                "required": ["type"],
                "properties": {"type": {"const": "b"}},
            }),
        ),
    ] {
        assert_keyword_change(
            "oneOf",
            json!([baseline.clone()]),
            json!([baseline, candidate]),
            CompatibilityClass::Breaking,
        );
    }
}

/// A document whose top-level `oneOf` branches carry their discriminator two `properties` hops
/// down (`kind.type`) and declare no `type`/`required` of their own -- `event-envelope`'s own
/// shape (D-049), reproduced here at fixture scale. `branches` becomes the document's own root
/// `oneOf`; the rest (`type`, `required: ["kind"]`, `properties.kind` `$ref`-ing a SEPARATE
/// `$defs/tag` union whose two branches both require `type`) stays identical between baseline and
/// candidate in every test below, so the only diff `compare_value` walks is the `oneOf` keyword
/// itself.
fn nested_discriminator_document(branches: Value) -> Value {
    json!({
        "type": "object",
        "required": ["kind"],
        "properties": {"kind": {"$ref": "#/$defs/tag"}},
        "$defs": {
            "tag": {
                "oneOf": [
                    {"type": "object", "required": ["type"], "properties": {"type": {"const": "a"}}},
                    {"type": "object", "required": ["type"], "properties": {"type": {"const": "b"}}},
                ],
            },
        },
        "oneOf": branches,
    })
}

fn nested_kind_branch(tag: &str) -> Value {
    json!({"properties": {"kind": {"properties": {"type": {"const": tag}}}}})
}

/// D-049's reopening, cell 1: two nested discriminators that genuinely cannot overlap must PROVE.
///
/// Neither branch declares `type` or `required` locally -- exactly the shape the flat
/// `discriminators_are_disjoint` proof is silent on, and exactly why this union sat pinned as
/// unprovable class B in `tests/union_provability.rs` (D-049) until this proof existed. Without the
/// fix this asserts `CompatibilityClass::Breaking` ("oneOf overlap unprovable"); the fix must turn
/// it `Compatible`.
#[test]
fn a_nested_discriminator_addition_at_the_document_root_is_compatible_because_the_tags_cannot_overlap()
 {
    let baseline = nested_discriminator_document(json!([nested_kind_branch("a")]));
    let candidate =
        nested_discriminator_document(json!([nested_kind_branch("a"), nested_kind_branch("c")]));

    let report = compare(baseline, candidate);
    assert_change(
        &report,
        CompatibilityClass::Compatible,
        SemverImpact::Minor,
        "GHC103_COMPATIBLE_CHANGE",
        "/oneOf",
    );
    assert!(
        !report
            .changes
            .iter()
            .any(|change| change.candidate_summary.contains("unprovable")),
        "a genuinely disjoint nested-discriminator addition still read as unprovable: {:?}",
        report.changes
    );
}

/// D-049's reopening, cell 2: two nested discriminators sharing the SAME tag must stay
/// unprovable and BREAKING -- the cell that stops a rubber-stamp implementation.
///
/// A prover that only ran cell 1 above could be `true` unconditionally and still pass it. This new
/// candidate branch shares its `kind.type` const with the baseline's OWN existing branch -- a real
/// overlap, not a duplicate: it is NOT the same branch (a sibling property differs, so its
/// fingerprint does not match the baseline branch and the trivial "already exists" path is not
/// what is under test here), it is a second, distinct way to reach `kind.type == "a"`. Nothing sound
/// can certify that disjoint from the original `"a"` branch, and this must stay `Breaking` before
/// and after the nested-discriminator proof lands.
#[test]
fn a_nested_discriminator_sharing_an_existing_tag_stays_unprovable_and_breaking() {
    let mut overlapping_branch = nested_kind_branch("a");
    overlapping_branch
        .as_object_mut()
        .unwrap()
        .insert("description".into(), json!("a second way to tag \"a\""));

    let baseline = nested_discriminator_document(json!([nested_kind_branch("a")]));
    let candidate =
        nested_discriminator_document(json!([nested_kind_branch("a"), overlapping_branch]));

    let report = compare(baseline, candidate);
    assert_change(
        &report,
        CompatibilityClass::Breaking,
        SemverImpact::Major,
        "GHC003_BREAKING_CHANGE",
        "/oneOf",
    );
    assert!(
        report
            .changes
            .iter()
            .any(|change| change.pointer == "/oneOf"
                && change.candidate_summary.contains("unprovable")),
        "a genuinely overlapping nested-discriminator addition was certified disjoint: {:?}",
        report.changes
    );
}

/// D-049's reopening, cell 3 (K's review of #758): the referenced inner union must ITSELF be
/// constrained to objects, or the propagated `required` proves nothing.
///
/// `properties.<name>.const` and `required` are no-ops on a non-object instance -- the same #627
/// argument `branch_is_object_only` exists for at the flat level. `nested_discriminator_document`'s
/// `$defs/tag` always declares `"type": "object"` on every branch, which is what makes cells 1 and 2
/// above sound; this fixture drops that from BOTH `$defs/tag` itself and every one of its branches,
/// keeping only `required: ["type"]`. A `kind` value that is a bare STRING then trivially clears
/// `required` (a no-op on a non-object) and matches every branch of `$defs/tag` regardless of its
/// `type` const, so `kind.type.const` constrains nothing and the two top-level branches below --
/// otherwise identical to cell 1's genuinely-disjoint pair -- must NOT be certified disjoint. Before
/// the object-only check in `document_required_nested_pairs`, this fixture went green on the
/// strength of `$defs/tag`'s branches merely REQUIRING `type`, exactly the gap K's review named:
/// today's real `$defs/eventKind` happens to have every branch declare `"type": "object"`, but
/// nothing here required it, so a hypothetical branch that didn't would have reopened #627's hole
/// with nothing to catch it. This cell is that hypothetical branch, built by hand.
#[test]
fn a_nested_discriminator_addition_is_not_certified_when_the_inner_union_admits_non_objects() {
    let untyped_inner_document = |branches: Value| {
        json!({
            "type": "object",
            "required": ["kind"],
            "properties": {"kind": {"$ref": "#/$defs/tag"}},
            "$defs": {
                "tag": {
                    "oneOf": [
                        {"required": ["type"], "properties": {"type": {"const": "a"}}},
                        {"required": ["type"], "properties": {"type": {"const": "b"}}},
                    ],
                },
            },
            "oneOf": branches,
        })
    };

    let baseline = untyped_inner_document(json!([nested_kind_branch("a")]));
    let candidate =
        untyped_inner_document(json!([nested_kind_branch("a"), nested_kind_branch("c")]));

    let report = compare(baseline, candidate);
    assert_change(
        &report,
        CompatibilityClass::Breaking,
        SemverImpact::Major,
        "GHC003_BREAKING_CHANGE",
        "/oneOf",
    );
    assert!(
        report
            .changes
            .iter()
            .any(|change| change.pointer == "/oneOf"
                && change.candidate_summary.contains("unprovable")),
        "a nested-discriminator addition was certified disjoint through an inner union that does \
         not itself constrain its instances to objects: {:?}",
        report.changes
    );
}
