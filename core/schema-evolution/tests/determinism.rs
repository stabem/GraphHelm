use graphhelm_protocols::{Diagnostic, Severity};
use graphhelm_schema_evolution::{
    MAX_JSON_DEPTH, SchemaDigest, canonical_json, schema_digest, sort_diagnostics,
};
use proptest::prelude::*;

#[test]
fn object_order_and_whitespace_do_not_change_schema_digest() {
    let left = serde_json::json!({"type":"object","properties":{"b":{"type":"string"},"a":{"type":"integer"}}});
    let right: serde_json::Value = serde_json::from_str(
        r#"{ "properties": { "a": {"type":"integer"}, "b": {"type":"string"} }, "type":"object" }"#,
    )
    .unwrap();
    assert_eq!(
        canonical_json(&left).unwrap(),
        canonical_json(&right).unwrap()
    );
    assert_eq!(
        schema_digest(&left).unwrap(),
        schema_digest(&right).unwrap()
    );
}

#[test]
fn arrays_remain_order_sensitive() {
    assert_ne!(
        schema_digest(&serde_json::json!({"enum":["a","b"]})).unwrap(),
        schema_digest(&serde_json::json!({"enum":["b","a"]})).unwrap(),
    );
}

proptest! {
    #[test]
    fn object_permutations_have_identical_canonical_bytes_and_digests(
        first in "[a-z]{1,8}",
        second in "[a-z]{1,8}",
        number in any::<i64>(),
    ) {
        prop_assume!(first != second);
        let left = serde_json::json!({first.clone(): number, second.clone(): true});
        let right = serde_json::json!({second: true, first: number});

        prop_assert_eq!(canonical_json(&left).unwrap(), canonical_json(&right).unwrap());
        prop_assert_eq!(schema_digest(&left).unwrap(), schema_digest(&right).unwrap());
    }
}

#[test]
fn schema_digest_parse_requires_lowercase_sha256_hex() {
    let digest = SchemaDigest::parse(&format!("sha256:{}", "a".repeat(64))).unwrap();
    assert_eq!(digest.as_str(), format!("sha256:{}", "a".repeat(64)));
    assert!(SchemaDigest::parse("sha256:ABCDEF").is_err());
    assert!(SchemaDigest::parse(&format!("sha256:{}", "a".repeat(63))).is_err());
}

#[test]
fn schema_digest_deserialization_enforces_lowercase_sha256_hex() {
    let valid = format!("\"sha256:{}\"", "a".repeat(64));
    let uppercase = format!("\"sha256:{}\"", "A".repeat(64));

    assert_eq!(
        serde_json::from_str::<SchemaDigest>(&valid)
            .unwrap()
            .as_str(),
        valid.trim_matches('\"'),
    );
    assert!(serde_json::from_str::<SchemaDigest>(&uppercase).is_err());
    assert!(serde_json::from_str::<SchemaDigest>("\"sha256:not-a-digest\"").is_err());
}

#[test]
fn depth_above_limit_returns_a_redacted_catalog_diagnostic() {
    let mut value = serde_json::Value::Null;
    for _ in 0..=MAX_JSON_DEPTH {
        value = serde_json::Value::Array(vec![value]);
    }

    let error = canonical_json(&value).unwrap_err();
    let diagnostic = error.diagnostic();
    assert_eq!(diagnostic.code, "GHC001_CATALOG_INVALID");
    assert!(!diagnostic.message.contains('['));
    assert!(!diagnostic.message.contains("null"));
}

#[test]
fn diagnostics_sort_by_source_path_code_and_message() {
    let mut diagnostics = vec![
        Diagnostic::error("B", "z", "/b", "source-b"),
        Diagnostic::error("A", "z", "/b", "source-a"),
        Diagnostic::error("A", "a", "/b", "source-a"),
        Diagnostic::error("A", "a", "/a", "source-a"),
    ];

    sort_diagnostics(&mut diagnostics);

    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.source.as_str(),
                    diagnostic.path.as_str(),
                    diagnostic.code.as_str(),
                    diagnostic.message.as_str(),
                )
            })
            .collect::<Vec<_>>(),
        vec![
            ("source-a", "/a", "A", "a"),
            ("source-a", "/b", "A", "a"),
            ("source-a", "/b", "A", "z"),
            ("source-b", "/b", "B", "z"),
        ],
    );
}

#[test]
fn diagnostics_with_identical_fields_sort_by_severity_independent_of_input_order() {
    let error = Diagnostic::error("GHC001", "message", "/path", "source");
    let warning = Diagnostic::warning("GHC001", "message", "/path", "source");
    let mut error_then_warning = vec![error.clone(), warning.clone()];
    let mut warning_then_error = vec![warning, error];

    sort_diagnostics(&mut error_then_warning);
    sort_diagnostics(&mut warning_then_error);

    assert_eq!(error_then_warning, warning_then_error);
    assert_eq!(
        error_then_warning
            .iter()
            .map(|diagnostic| &diagnostic.severity)
            .collect::<Vec<_>>(),
        vec![&Severity::Error, &Severity::Warning],
    );
}
