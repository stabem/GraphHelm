use std::fs::File;
use std::io::{Read, Take};
use std::path::Path;

use graphhelm_protocols::{Diagnostic, ExecutionGraph};

use crate::validate_graph_value;

const MAX_DOCUMENT_BYTES: u64 = 4 * 1024 * 1024;

/// A parsed, schema-validated and typed graph document.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadedGraph {
    pub source: String,
    pub raw: serde_json::Value,
    pub graph: ExecutionGraph,
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

    let mut contents = String::new();
    let mut bounded: Take<File> = file.take(MAX_DOCUMENT_BYTES + 1);
    bounded.read_to_string(&mut contents).map_err(|error| {
        vec![parse_diagnostic(
            &source,
            format!("cannot decode graph: {error}"),
        )]
    })?;
    if contents.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err(vec![parse_diagnostic(
            &source,
            "graph exceeds the 4 MiB input limit",
        )]);
    }

    let raw = if extension.eq_ignore_ascii_case("json") {
        serde_json::from_str(&contents)
            .map_err(|error| vec![parse_diagnostic(&source, format!("invalid JSON: {error}"))])?
    } else {
        serde_yaml_ng::from_str(&contents)
            .map_err(|error| vec![parse_diagnostic(&source, format!("invalid YAML: {error}"))])?
    };

    let diagnostics = validate_graph_value(&raw, &source);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    let graph = serde_json::from_value(raw.clone()).map_err(|error| {
        vec![Diagnostic::error(
            "GHS003_TYPED",
            format!("graph is outside the typed v1 subset: {error}"),
            "/",
            &source,
        )]
    })?;

    Ok(LoadedGraph { source, raw, graph })
}

fn parse_diagnostic(source: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error("GHS001_PARSE", message, "/", source)
}
