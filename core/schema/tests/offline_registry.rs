use std::collections::BTreeMap;

use graphhelm_schema::OfflineSchemaSet;
use serde_json::json;

const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;
const MAX_RESOURCE_BYTES: usize = 32 * 1024 * 1024;

fn resources(schema: serde_json::Value) -> BTreeMap<String, serde_json::Value> {
    BTreeMap::from([("test".into(), schema)])
}

// Prevents a registry from compiling a relative reference that was not explicitly supplied.
#[test]
fn compile_rejects_unresolved_relative_reference_offline() {
    let result = OfflineSchemaSet::compile(resources(json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "https://p50.dev/schemas/root.schema.json",
        "$ref": "missing.schema.json"
    })));
    let Err(diagnostics) = result else {
        panic!("unresolved relative reference compiled successfully");
    };
    assert_eq!(diagnostics[0].code, "GHS002_SCHEMA");
}

// Prevents compilation from retrieving an HTTP(S) reference that was not explicitly supplied.
#[test]
fn compile_rejects_remote_reference_offline() {
    let result = OfflineSchemaSet::compile(resources(json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "https://p50.dev/schemas/root.schema.json",
        "$ref": "https://example.com/remote.json"
    })));
    let Err(diagnostics) = result else {
        panic!("remote reference compiled successfully");
    };
    assert_eq!(diagnostics[0].code, "GHS002_SCHEMA");
}

// Prevents validation from accepting a root schema that was not compiled into the offline set.
#[test]
fn validate_rejects_unregistered_root_id() {
    let set = OfflineSchemaSet::compile(resources(json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "https://p50.dev/schemas/root.schema.json",
        "type": "object"
    })))
    .unwrap();
    let diagnostics = set.validate(
        "https://p50.dev/schemas/not-registered.schema.json",
        &json!({}),
        "fixture.json",
    );
    assert_eq!(diagnostics[0].code, "GHS002_SCHEMA");
    assert_eq!(diagnostics[0].source, "fixture.json");
}

#[test]
fn validation_diagnostics_never_echo_an_offending_secret_value() {
    let schema_id = "https://p50.dev/schemas/root.schema.json";
    let set = OfflineSchemaSet::compile(resources(json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": schema_id,
        "type": "object",
        "properties": {"token": {"type": "integer"}}
    })))
    .unwrap();
    let diagnostics = set.validate(
        schema_id,
        &json!({"token": "TOP-SECRET-DO-NOT-ECHO"}),
        "fixture.json",
    );

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].path, "/token");
    assert_eq!(diagnostics[0].message, "document has an invalid type");
    assert!(
        !serde_json::to_string(&diagnostics)
            .unwrap()
            .contains("TOP-SECRET")
    );
}

// Prevents an unbounded number of explicit resources from reaching Registry::prepare.
#[test]
fn compile_rejects_more_than_256_resources() {
    let resources = (0..257)
        .map(|index| {
            (
                format!("schema-{index}"),
                json!({
                    "$id": format!("https://p50.dev/schemas/{index}.schema.json"),
                    "type": "object"
                }),
            )
        })
        .collect();
    let Err(diagnostics) = OfflineSchemaSet::compile(resources) else {
        panic!("unbounded schema set compiled successfully");
    };
    assert_eq!(diagnostics[0].code, "GHS002_SCHEMA");
    assert_eq!(diagnostics[0].source, "offline-schema-set");
}

// Prevents one explicit schema resource from exceeding the bounded compiler input.
#[test]
fn compile_rejects_resource_larger_than_four_mebibytes() {
    let result = OfflineSchemaSet::compile(resources(json!({
        "$id": "https://p50.dev/schemas/root.schema.json",
        "$comment": "x".repeat(MAX_FILE_BYTES)
    })));
    let Err(diagnostics) = result else {
        panic!("oversized schema resource compiled successfully");
    };
    assert_eq!(diagnostics[0].code, "GHS002_SCHEMA");
    assert_eq!(diagnostics[0].source, "offline-schema-set");
    assert_eq!(diagnostics[0].path, "/resources/0/bytes");
}

// Prevents many individually bounded schemas from exceeding the aggregate compiler input.
#[test]
fn compile_rejects_resource_aggregate_larger_than_thirty_two_mebibytes() {
    let comment = "x".repeat(MAX_RESOURCE_BYTES / 9);
    let resources = (0..9)
        .map(|index| {
            (
                format!("schema-{index}"),
                json!({
                    "$id": format!("https://p50.dev/schemas/{index}.schema.json"),
                    "$comment": comment
                }),
            )
        })
        .collect();
    let Err(diagnostics) = OfflineSchemaSet::compile(resources) else {
        panic!("oversized aggregate compiled successfully");
    };
    assert_eq!(diagnostics[0].code, "GHS002_SCHEMA");
    assert_eq!(diagnostics[0].source, "offline-schema-set");
}

// Prevents nesting above the bounded compiler traversal depth.
#[test]
fn compile_rejects_resource_deeper_than_128_levels() {
    let mut nested = serde_json::Value::Null;
    for _ in 0..129 {
        nested = json!({"nested": nested});
    }
    let result = OfflineSchemaSet::compile(resources(json!({
        "$id": "https://p50.dev/schemas/root.schema.json",
        "extension": nested
    })));
    let Err(diagnostics) = result else {
        panic!("overly nested schema resource compiled successfully");
    };
    assert_eq!(diagnostics[0].code, "GHS002_SCHEMA");
    assert_eq!(diagnostics[0].source, "offline-schema-set");
}

// Prevents serialization from reaching a deep oversized leaf before depth is rejected.
#[test]
fn compile_reports_depth_before_bytes_for_deep_oversized_resource() {
    let mut nested = serde_json::Value::String("x".repeat(MAX_FILE_BYTES));
    for _ in 0..129 {
        nested = json!({"nested": nested});
    }
    let result = OfflineSchemaSet::compile(resources(json!({
        "$id": "https://p50.dev/schemas/root.schema.json",
        "extension": nested
    })));
    let Err(diagnostics) = result else {
        panic!("deep oversized schema resource compiled successfully");
    };
    assert_eq!(diagnostics[0].code, "GHS002_SCHEMA");
    assert_eq!(diagnostics[0].source, "offline-schema-set");
    assert_eq!(diagnostics[0].path, "/resources/0/depth");
}

// Prevents an untrusted schema $id from appearing in a bounded compiler diagnostic.
#[test]
fn compile_redacts_secret_shaped_schema_id_from_byte_diagnostic() {
    let result = OfflineSchemaSet::compile(resources(json!({
        "$id": "file:///C:/Users/example/TOP_SECRET_TOKEN/root.schema.json",
        "$comment": "x".repeat(MAX_FILE_BYTES)
    })));
    let Err(diagnostics) = result else {
        panic!("oversized schema resource compiled successfully");
    };
    let serialized = serde_json::to_string(&diagnostics).unwrap();
    assert!(!serialized.contains("TOP_SECRET_TOKEN"));
    assert!(!serialized.contains("C:"));
    assert_eq!(diagnostics[0].path, "/resources/0/bytes");
}
