//! The `NodeType` enum and the schemas that list node types must name the same set.
//!
//! WHY THIS FILE EXISTS, measured rather than assumed. `EventKind` carries `EVERY_WIRE_NAME`, and
//! that list is why a forgotten event kind goes red BY NAME — #162 was caught by it twice, at a
//! conformance table and at a round-trip table, after its author had already declared the kinds
//! "everywhere". **`NodeType` had no equivalent.** Measured on `27727a5`:
//!
//! ```text
//! NodeType equivalent of EVERY_WIRE_NAME                NONE
//! conformance.rs mentions of NodeType                   0
//! any test comparing the Rust enum to the schema enum   0
//! ```
//!
//! So a variant the Rust type accepts and the schemas refuse produced **no symptom at all**. That
//! is the `customs` defect of #162 in a different vocabulary: there, a budget the type accepted and
//! the schema forbade made an entire feature inert while every suite stayed green, and it was found
//! only because one fixture went through the public door.
//!
//! THE GUARD READS BOTH REAL SOURCES. It compares the actual enum against the actual files on
//! disk. A guard holding one side and a copy of the other agrees with itself while the real pair
//! diverges — the defect lives only in the RELATION, so both ends must be real.
//!
//! AND IT READS BOTH KEYS, which is the trap this lane walked into first. The two schemas name the
//! same concept differently:
//!
//! ```text
//! schemas/node.schema.json                     key: "type"       (authoring)
//! schemas/persisted-graph-version.schema.json  key: "nodeType"   (persisted)
//! ```
//!
//! A sweep for one key finds two of the four files and reports success. That is how the enumeration
//! for this issue missed the authoring schema on its first pass.

use std::collections::BTreeSet;

use graphhelm_protocols::NodeType;

/// Every spelling the enum can emit, from the macro-derived list rather than a copy written here.
fn enum_vocabulary() -> BTreeSet<String> {
    NodeType::EVERY_WIRE_NAME
        .iter()
        .map(|name| (*name).to_owned())
        .collect()
}

fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("the workspace root is two levels above core/protocols")
        .to_path_buf()
}

/// The enum list at a named key in one schema document, read from disk at RUN time.
///
/// Never `include_str!`: a schema baked into the test binary is a copy from build time, and under a
/// stale build the assertion passes while the file on disk says something else.
fn schema_vocabulary(relative: &str, key: &str) -> BTreeSet<String> {
    let path = workspace_root().join(relative);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{relative} must be readable: {error}"));
    let document: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|error| panic!("{relative}: {error}"));

    let mut found: Option<BTreeSet<String>> = None;
    collect(&document, key, &mut found);
    found.unwrap_or_else(|| panic!("{relative}: no closed enum found at key {key:?}"))
}

fn collect(value: &serde_json::Value, key: &str, out: &mut Option<BTreeSet<String>>) {
    match value {
        serde_json::Value::Object(map) => {
            for (name, child) in map {
                if name == key
                    && let Some(members) = child.get("enum").and_then(serde_json::Value::as_array)
                {
                    let set = members
                        .iter()
                        .map(|member| {
                            member
                                .as_str()
                                .expect("enum members are strings")
                                .to_owned()
                        })
                        .collect();
                    *out = Some(set);
                    return;
                }
                collect(child, key, out);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect(item, key, out);
            }
        }
        _ => {}
    }
}

/// Every schema that lists node types must list EXACTLY what the enum can emit.
///
/// Equality in both directions, not containment. A schema missing a variant makes that variant
/// unusable on the wire while the type accepts it happily; a schema carrying a variant the enum
/// cannot produce is a legal document nothing can construct. Both are silent, and only equality
/// sees either.
#[test]
fn every_schema_that_lists_node_types_lists_exactly_the_enum() {
    let expected = enum_vocabulary();

    // Non-vacuity FIRST. An extraction that silently produced nothing would make every comparison
    // below trivially agree, and the guard would read as coverage while measuring an empty set.
    assert!(
        !expected.is_empty(),
        "an empty enum vocabulary would satisfy every comparison below vacuously"
    );

    for (relative, key) in [
        ("schemas/node.schema.json", "type"),
        ("schemas/releases/1.0.0/node.schema.json", "type"),
        ("schemas/persisted-graph-version.schema.json", "nodeType"),
        (
            "schemas/releases/1.0.0/persisted-graph-version.schema.json",
            "nodeType",
        ),
    ] {
        let declared = schema_vocabulary(relative, key);
        assert_eq!(
            declared,
            expected,
            "{relative} (key {key:?}) must list exactly what NodeType emits; \
             missing from the schema: {:?}; present only in the schema: {:?}",
            expected.difference(&declared).collect::<Vec<_>>(),
            declared.difference(&expected).collect::<Vec<_>>()
        );
    }
}

/// The two spellings are a real pair, and this cell exists so the pair cannot quietly become one.
///
/// If someone ever renames the authoring key to `nodeType`, or the persisted key to `type`, the
/// test above keeps passing — it would simply look under the new name in the file that has it and
/// fail to find it in the other, which reads as a schema bug rather than a rename. This asserts the
/// pair itself, so a rename is a decision someone makes on purpose.
#[test]
fn the_two_schemas_still_name_the_concept_with_different_keys() {
    assert!(
        !schema_vocabulary("schemas/node.schema.json", "type").is_empty(),
        "the authoring schema names node types under `type`"
    );
    assert!(
        !schema_vocabulary("schemas/persisted-graph-version.schema.json", "nodeType").is_empty(),
        "the persisted schema names them under `nodeType` -- a sweep for one key finds two of the \
         four files and reports success, which is how this lane's first enumeration missed the \
         authoring schema"
    );
}
