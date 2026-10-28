use std::collections::BTreeMap;

use graphhelm_protocols::{ExecutionGraph, SemanticHash};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Failures while deriving or validating a graph's immutable semantic identity.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum GraphError {
    #[error("graph metadata version must be at least one")]
    InvalidVersion,
    #[error("successor version must be exactly predecessor version plus one")]
    InvalidPredecessor,
    #[error("persisted semantic projection does not match the graph")]
    SemanticMismatch,
    #[error("persisted semantic hash does not match the graph")]
    HashMismatch,
    #[error("safe persistence projection is invalid")]
    InvalidProjection,
    #[error("graph cannot be represented as canonical JSON: {0}")]
    Serialization(String),
}

/// Human-reviewable canonical semantic JSON and its exact compact UTF-8 bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalGraph {
    pub value: Value,
    pub bytes: Vec<u8>,
}

/// Projects a graph onto fields that affect execution and serializes deterministically.
pub fn canonicalize(graph: &ExecutionGraph) -> Result<CanonicalGraph, GraphError> {
    let full = serde_json::to_value(graph)
        .map_err(|error| GraphError::Serialization(error.to_string()))?;
    let spec = full
        .get("spec")
        .cloned()
        .ok_or_else(|| GraphError::Serialization("missing spec".into()))?;
    let mut spec = sort_value(spec);

    if let Some(nodes) = spec.get_mut("nodes").and_then(Value::as_object_mut) {
        for node in nodes.values_mut() {
            if let Some(object) = node.as_object_mut() {
                object.remove("ui");
            }
        }
    }

    let mut metadata = graph
        .metadata
        .properties
        .iter()
        .filter(|(key, _)| {
            !matches!(
                key.as_str(),
                "description" | "annotations" | "createdAt" | "createdBy" | "mutationId" | "ui"
            )
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<Map<_, _>>();
    metadata.insert(
        "labels".into(),
        Value::Object(
            graph
                .metadata
                .labels
                .iter()
                .map(|(key, value)| (key.clone(), Value::String(value.clone())))
                .collect(),
        ),
    );
    let value = sort_value(serde_json::json!({
        "apiVersion": graph.api_version,
        "kind": graph.kind,
        "metadata": metadata,
        "spec": spec,
    }));
    let bytes =
        serde_json::to_vec(&value).map_err(|error| GraphError::Serialization(error.to_string()))?;
    Ok(CanonicalGraph { value, bytes })
}

/// Computes the stable SHA-256 semantic digest for a graph.
pub fn semantic_hash(graph: &ExecutionGraph) -> Result<SemanticHash, GraphError> {
    let canonical = canonicalize(graph)?;
    let digest = Sha256::digest(&canonical.bytes);
    Ok(SemanticHash::new(format!("sha256:{}", hex::encode(digest))))
}

pub(crate) fn sort_value(value: Value) -> Value {
    match value {
        Value::Object(object) => {
            let sorted: BTreeMap<_, _> = object
                .into_iter()
                .map(|(key, value)| (key, sort_value(value)))
                .collect();
            Value::Object(sorted.into_iter().collect())
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sort_value).collect()),
        scalar => scalar,
    }
}
