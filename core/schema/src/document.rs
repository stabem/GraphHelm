use std::fs::File;
use std::io::{Read, Take};
use std::path::Path;

use graphhelm_protocols::{Diagnostic, ExecutionGraph};

use crate::bounded_value::{BoundedValueError, ValueBudget, ValueLimits, parse_json, parse_yaml};
use crate::{validate_extension_value, validate_graph_value};

const MAX_DOCUMENT_BYTES: u64 = 4 * 1024 * 1024;
const DOCUMENT_VALUE_LIMITS: ValueLimits = ValueLimits {
    max_depth: 128,
    max_values: 128 * 1024,
    max_key_bytes: MAX_DOCUMENT_BYTES as usize,
    max_string_bytes: MAX_DOCUMENT_BYTES as usize,
};

/// A parsed, schema-validated and typed graph document.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadedGraph {
    pub source: String,
    pub raw: serde_json::Value,
    pub graph: ExecutionGraph,
}

/// A parsed and schema-validated extension manifest.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadedExtension {
    pub source: String,
    pub raw: serde_json::Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum YamlValueError {
    AnchorOrAlias,
    ByteOrderMark,
    LimitExceeded,
    Invalid,
}

pub(crate) fn parse_yaml_value(bytes: &[u8]) -> Result<serde_json::Value, YamlValueError> {
    let mut budget = ValueBudget::default();
    let value =
        parse_yaml(bytes, DOCUMENT_VALUE_LIMITS, &mut budget).map_err(|error| match error {
            BoundedValueError::AnchorOrAlias => YamlValueError::AnchorOrAlias,
            BoundedValueError::ByteOrderMark | BoundedValueError::Nul => {
                YamlValueError::ByteOrderMark
            }
            BoundedValueError::DepthLimit
            | BoundedValueError::ValueLimit
            | BoundedValueError::KeyBytesLimit
            | BoundedValueError::StringBytesLimit => YamlValueError::LimitExceeded,
            BoundedValueError::Invalid | BoundedValueError::InvalidUtf8 => YamlValueError::Invalid,
        })?;
    debug_assert!(budget.metrics().values <= DOCUMENT_VALUE_LIMITS.max_values);
    Ok(value)
}

/// Loads one bounded JSON extension manifest and validates it offline.
pub fn load_extension(path: &Path) -> Result<LoadedExtension, Vec<Diagnostic>> {
    let source = path.to_string_lossy().into_owned();
    if path.extension().and_then(|value| value.to_str()) != Some("json") {
        return Err(vec![parse_diagnostic(
            &source,
            "unsupported extension manifest; expected .json",
        )]);
    }
    let file = File::open(path).map_err(|error| {
        vec![parse_diagnostic(
            &source,
            format!("cannot read extension manifest: {error}"),
        )]
    })?;
    let metadata = file.metadata().map_err(|error| {
        vec![parse_diagnostic(
            &source,
            format!("cannot inspect extension manifest: {error}"),
        )]
    })?;
    if metadata.len() > MAX_DOCUMENT_BYTES {
        return Err(vec![parse_diagnostic(
            &source,
            "extension manifest exceeds the 4 MiB input limit",
        )]);
    }
    let mut contents = String::new();
    let mut bounded: Take<File> = file.take(MAX_DOCUMENT_BYTES + 1);
    bounded.read_to_string(&mut contents).map_err(|error| {
        vec![parse_diagnostic(
            &source,
            format!("cannot decode extension manifest: {error}"),
        )]
    })?;
    if contents.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err(vec![parse_diagnostic(
            &source,
            "extension manifest exceeds the 4 MiB input limit",
        )]);
    }
    let mut budget = ValueBudget::default();
    let raw =
        parse_json(contents.as_bytes(), DOCUMENT_VALUE_LIMITS, &mut budget).map_err(|error| {
            vec![parse_diagnostic(
                &source,
                extension_json_error_message(error),
            )]
        })?;
    debug_assert!(budget.metrics().values <= DOCUMENT_VALUE_LIMITS.max_values);
    let diagnostics = validate_extension_value(&raw, &source);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    Ok(LoadedExtension { source, raw })
}

/// Loads YAML or JSON from a bounded local file and validates it offline.
pub fn load_graph(path: &Path) -> Result<LoadedGraph, Vec<Diagnostic>> {
    let source = path.to_string_lossy().into_owned();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    if !matches!(
        extension.to_ascii_lowercase().as_str(),
        "json" | "yaml" | "yml"
    ) {
        return Err(vec![parse_diagnostic(
            &source,
            "unsupported graph extension; expected .json, .yaml, or .yml",
        )]);
    }

    let file = File::open(path).map_err(|error| {
        vec![parse_diagnostic(
            &source,
            format!("cannot read graph: {error}"),
        )]
    })?;
    let metadata = file.metadata().map_err(|error| {
        vec![parse_diagnostic(
            &source,
            format!("cannot inspect graph: {error}"),
        )]
    })?;
    if metadata.len() > MAX_DOCUMENT_BYTES {
        return Err(vec![parse_diagnostic(
            &source,
            "graph exceeds the 4 MiB input limit",
        )]);
    }

    let mut contents = Vec::with_capacity(metadata.len() as usize);
    let mut bounded: Take<File> = file.take(MAX_DOCUMENT_BYTES + 1);
    bounded.read_to_end(&mut contents).map_err(|error| {
        vec![parse_diagnostic(
            &source,
            format!("cannot read graph: {error}"),
        )]
    })?;
    if contents.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err(vec![parse_diagnostic(
            &source,
            "graph exceeds the 4 MiB input limit",
        )]);
    }

    load_graph_bytes(&contents, extension, &source)
}

pub(crate) fn load_graph_bytes(
    contents: &[u8],
    extension: &str,
    source: &str,
) -> Result<LoadedGraph, Vec<Diagnostic>> {
    if contents.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err(vec![parse_diagnostic(
            source,
            "graph exceeds the 4 MiB input limit",
        )]);
    }

    let raw = if extension.eq_ignore_ascii_case("json") {
        let mut budget = ValueBudget::default();
        let value = parse_json(contents, DOCUMENT_VALUE_LIMITS, &mut budget).map_err(|error| {
            vec![parse_diagnostic(
                source,
                document_value_error_message("JSON", error),
            )]
        })?;
        debug_assert!(budget.metrics().values <= DOCUMENT_VALUE_LIMITS.max_values);
        value
    } else if matches!(extension.to_ascii_lowercase().as_str(), "yaml" | "yml") {
        parse_yaml_value(contents).map_err(|error| {
            vec![parse_diagnostic(
                source,
                match error {
                    YamlValueError::AnchorOrAlias => {
                        "YAML anchors and aliases are not allowed in bounded documents"
                    }
                    YamlValueError::ByteOrderMark => {
                        "YAML byte-order marks are not allowed in bounded documents"
                    }
                    YamlValueError::LimitExceeded => "YAML document exceeds a bounded value limit",
                    YamlValueError::Invalid => "invalid YAML",
                },
            )]
        })?
    } else {
        return Err(vec![parse_diagnostic(
            source,
            "unsupported graph extension; expected .json, .yaml, or .yml",
        )]);
    };

    let diagnostics = validate_graph_value(&raw, source);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    let graph = serde_json::from_value(raw.clone()).map_err(|error| {
        vec![Diagnostic::error(
            "GHS003_TYPED",
            format!("graph is outside the typed v1 subset: {error}"),
            "/",
            source,
        )]
    })?;

    Ok(LoadedGraph {
        source: source.to_owned(),
        raw,
        graph,
    })
}

fn extension_json_error_message(error: BoundedValueError) -> &'static str {
    match error {
        BoundedValueError::DepthLimit => "extension manifest JSON depth limit exceeded",
        BoundedValueError::ValueLimit => "extension manifest JSON value limit exceeded",
        BoundedValueError::KeyBytesLimit => "extension manifest JSON key byte limit exceeded",
        BoundedValueError::StringBytesLimit => "extension manifest JSON string byte limit exceeded",
        _ => "extension manifest contains invalid JSON",
    }
}

fn document_value_error_message(format: &str, error: BoundedValueError) -> String {
    match error {
        BoundedValueError::DepthLimit => format!("{format} document depth limit exceeded"),
        BoundedValueError::ValueLimit => format!("{format} document value limit exceeded"),
        BoundedValueError::KeyBytesLimit => format!("{format} document key byte limit exceeded"),
        BoundedValueError::StringBytesLimit => {
            format!("{format} document string byte limit exceeded")
        }
        _ => format!("invalid {format}"),
    }
}

fn parse_diagnostic(source: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error("GHS001_PARSE", message, "/", source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_leading_utf8_bom_is_normalized_before_yaml_parsing() {
        let value = parse_yaml_value(b"\xEF\xBB\xBFvalue: accepted\n").unwrap();
        assert_eq!(value["value"], "accepted");
    }

    #[test]
    fn utf16_bom_documents_are_rejected_before_yaml_materialization() {
        let source = "shared: &shared value\nalias: *shared\n";
        for mut bytes in [vec![0xFF, 0xFE], vec![0xFE, 0xFF]] {
            for code_unit in source.encode_utf16() {
                if bytes.starts_with(&[0xFF, 0xFE]) {
                    bytes.extend_from_slice(&code_unit.to_le_bytes());
                } else {
                    bytes.extend_from_slice(&code_unit.to_be_bytes());
                }
            }
            assert_eq!(parse_yaml_value(&bytes), Err(YamlValueError::Invalid));
        }
    }

    #[test]
    fn an_embedded_utf8_bom_cannot_hide_yaml_alias_syntax() {
        assert_eq!(
            parse_yaml_value(b"shared: \xEF\xBB\xBF&shared value\nalias: \xEF\xBB\xBF*shared\n"),
            Err(YamlValueError::ByteOrderMark)
        );
    }

    #[test]
    fn graph_json_value_limit_is_reported_during_parsing() {
        let contents = format!("[{}]", vec!["null"; 128 * 1024].join(","));

        let diagnostics = load_graph_bytes(contents.as_bytes(), "json", "bounded.json")
            .expect_err("the excess value must be rejected before schema validation");

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "GHS001_PARSE");
        assert_eq!(diagnostics[0].message, "JSON document value limit exceeded");
    }

    #[test]
    fn normal_graph_and_extension_documents_still_parse() {
        let graph = load_graph_bytes(
            include_bytes!("../../../examples/graphs/software-feature.yaml"),
            "yaml",
            "software-feature.yaml",
        )
        .unwrap();
        assert_eq!(graph.raw["kind"], "ExecutionGraph");

        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../extensions/builtin/graphhelm-jpd/extension.json");
        let extension = load_extension(&manifest).unwrap();
        assert_eq!(extension.raw["metadata"]["id"], "graphhelm-jpd");
    }
}
