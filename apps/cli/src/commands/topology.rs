//! `graph topology` / `POST /v1/graph/topology`: a graph file's shape, with the hash that proves
//! which graph it is.
//!
//! WHY THIS EXISTS. An execution's log names every node and every state it passed through, but it
//! never says which node feeds which: `execution_started` carries the graph's HASH, and the
//! topology itself is not persisted anywhere a client can read. So a surface that wants to DRAW
//! the graph has exactly two options - invent the edges, or read them from the file the run was
//! started from. This is the second one.
//!
//! THE HASH IS THE POINT, NOT A CONVENIENCE. A file path is a guess about which graph an
//! execution ran; `semanticHash` is what settles it. A caller compares this reply's hash against
//! the `graphHash` in the execution's own `execution_started` event, and only then may it claim
//! the edges belong to that run. Without that comparison a client is drawing a picture of some
//! file, captioned with a run it may have nothing to do with - and a drawn arrow reads as
//! evidence. The hash comes from `graphhelm_graph::semantic_hash`, the same function
//! `graph hash` prints, so the two answers cannot disagree.
//!
//! WHAT THIS ADDS TO THE API'S AUTHORITY: nothing. `start` and `resume` already take a `file` and
//! load it, so any bearer-token holder could already ask the Runtime to read a graph file by
//! path. This is the same read with a read-only result and no append - it grants no reach those
//! two did not already have, which is why it is a route rather than a new permission.
//!
//! WHAT IT DELIBERATELY DOES NOT RETURN: anything about a node except its stable id. The execution
//! roster already supplies the boxes; this read contributes only the endpoint identities and
//! edges needed to place them. Names, types, optionality, objectives, agent blocks, instructions,
//! policies, budgets and completion controls are unnecessary disclosure and can carry operator
//! prose.

use std::path::Path;

use graphhelm_protocols::Diagnostic;

use crate::output::Outcome;

pub(crate) const COMMAND: &str = "graph.topology";

/// Why a topology read could not answer.
///
/// `Debug` because this crate's own tests `expect()` on it, and `expect` needs it. Its absence
/// compiled fine under `cargo test --test <name>` - integration targets do not build the crate's
/// unit tests - and broke every stage of the gate that runs a plain `cargo test`.
#[derive(Debug)]
pub(crate) enum Failure {
    /// The file is unreadable, not a graph, or not schema-valid. The caller's mistake, and the
    /// loader's own diagnostics say which - relayed unchanged so the CLI and the API refuse an
    /// identical file with an identical body.
    Invalid(Vec<Diagnostic>),
    /// Canonicalisation refused a graph the schema accepted. Not a caller mistake.
    Internal(String),
}

/// Loads the graph at `file` and returns its shape plus its semantic hash.
///
/// No lint, deliberately. `start` lints because it is about to RUN the graph; drawing one is a
/// read, and refusing to show an operator the shape of a graph that has a lint warning would hide
/// the picture exactly when they most need it. Schema validity is still required - an unparsable
/// file has no shape to report.
pub(crate) fn execute(file: &Path) -> Result<serde_json::Value, Failure> {
    let loaded = graphhelm_schema::load_graph(file).map_err(Failure::Invalid)?;
    let hash = graphhelm_graph::semantic_hash(&loaded.graph)
        .map_err(|error| Failure::Internal(error.to_string()))?;

    let spec = &loaded.graph.spec;
    let nodes: Vec<serde_json::Value> = spec
        .nodes
        .keys()
        .map(|id| serde_json::json!({ "id": id }))
        .collect();

    // Edges keep the authored order. It is the only order the document defines, and sorting them
    // would silently change which of two edges out of one node a reader meets first.
    let edges: Vec<serde_json::Value> = spec
        .edges
        .iter()
        .map(|edge| {
            serde_json::json!({
                "id": edge.id,
                "from": edge.from,
                "to": edge.to,
                "type": edge.edge_type,
            })
        })
        .collect();

    Ok(serde_json::json!({
        "graphId": loaded.graph.metadata.id,
        "graphVersion": loaded.graph.metadata.version,
        "executionId": loaded.graph.metadata.execution_id,
        "semanticHash": hash,
        "entrypoints": spec.entrypoints,
        "nodes": nodes,
        "edges": edges,
    }))
}

/// The keys that carry CONTENT rather than shape - prompts, operator prose, execution controls.
/// None of them may appear in a topology reply, at any nesting depth.
#[cfg(test)]
const CONTENT_KEYS: [&str; 6] = [
    "objective",
    "instructions",
    "ephemeral",
    "completion",
    "policies",
    "inputSchema",
];

pub fn run(file: &Path) -> Outcome {
    match execute(file) {
        Ok(value) => Outcome::success(COMMAND, value),
        Err(Failure::Invalid(diagnostics)) => Outcome::domain(COMMAND, diagnostics),
        Err(Failure::Internal(message)) => Outcome::internal(COMMAND, message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("examples/graphs")
            .join(name)
    }

    /// The shape is REPORTED, not derived: the edges this returns must be the edges the document
    /// authored, in the order it authored them.
    #[test]
    fn the_reply_carries_the_documents_own_edges_in_order() {
        let value = execute(&example("software-feature.yaml")).expect("the example graph loads");
        let edges = value["edges"].as_array().expect("edges is an array");
        assert!(!edges.is_empty(), "the example graph has edges: {value}");

        let source = std::fs::read_to_string(example("software-feature.yaml")).expect("readable");
        let mut previous = 0;
        for edge in edges {
            let id = edge["id"].as_str().expect("an edge id");
            let needle = format!("id: {id}");
            let at = source
                .find(&needle)
                .unwrap_or_else(|| panic!("edge {id} is not in the document: {value}"));
            assert!(
                at >= previous,
                "edges must keep the authored order; {id} appears before its predecessor"
            );
            previous = at;
            for key in ["from", "to", "type"] {
                assert!(edge[key].is_string(), "edge {id} is missing {key}: {edge}");
            }
        }
    }

    /// The hash is what lets a caller tie these edges to a run, so it must be the same value
    /// `graph hash` prints for the same file - not a second hash computed some other way.
    #[test]
    fn the_semantic_hash_is_the_one_graph_hash_reports() {
        let file = example("software-feature.yaml");
        let value = execute(&file).expect("the example graph loads");
        let loaded = graphhelm_schema::load_graph(&file).expect("the example graph loads");
        let expected = graphhelm_graph::semantic_hash(&loaded.graph).expect("it hashes");
        assert_eq!(value["semanticHash"], serde_json::json!(expected));
    }

    /// A topology read draws boxes that the execution roster already names. The file therefore
    /// contributes only the stable endpoint identity needed to prove edge endpoints; every other
    /// node field is unnecessary disclosure. A guard rather than a comment: widening the
    /// projection fails here.
    #[test]
    fn a_node_carries_its_identity_and_no_content() {
        let value = execute(&example("software-feature.yaml")).expect("the example graph loads");
        for node in value["nodes"].as_array().expect("nodes is an array") {
            let mut keys: Vec<&str> = node
                .as_object()
                .expect("a node object")
                .keys()
                .map(String::as_str)
                .collect();
            keys.sort_unstable();
            assert_eq!(
                keys,
                vec!["id"],
                "a node must carry only its endpoint identity: {node}"
            );
        }
        // The whole reply, not just a node: a content key smuggled in anywhere still leaks.
        //
        // KEYS, NOT WORDS. An earlier version scanned for the bare substring "agent" and failed
        // on this very graph, whose nodes are OF TYPE `agent` - a legitimate value. A node's
        // `name` is operator prose and may contain any of these words too. What must never appear
        // is the KEY, so that is what is matched.
        let whole = value.to_string();
        for key in CONTENT_KEYS {
            let shaped = format!("\"{key}\":");
            assert!(
                !whole.contains(&shaped),
                "{key} is content and must not reach a topology reply: {whole}"
            );
        }
    }

    /// Every entrypoint and every edge endpoint must name a node the reply also carries -
    /// otherwise a client draws an arrow into empty space.
    #[test]
    fn every_endpoint_names_a_node_in_the_same_reply() {
        let value = execute(&example("research-to-publish.yaml")).expect("the example loads");
        let ids: Vec<&str> = value["nodes"]
            .as_array()
            .expect("nodes")
            .iter()
            .map(|node| node["id"].as_str().expect("an id"))
            .collect();
        for entry in value["entrypoints"].as_array().expect("entrypoints") {
            let id = entry.as_str().expect("an entrypoint id");
            assert!(ids.contains(&id), "entrypoint {id} is not a node: {value}");
        }
        for edge in value["edges"].as_array().expect("edges") {
            for end in ["from", "to"] {
                let id = edge[end].as_str().expect("an endpoint");
                assert!(
                    ids.contains(&id),
                    "edge endpoint {id} is not a node: {value}"
                );
            }
        }
    }

    #[test]
    fn a_file_that_is_not_a_graph_is_a_caller_mistake_with_diagnostics() {
        let directory = tempfile::tempdir().expect("a temp dir");
        let path = directory.path().join("not-a-graph.yaml");
        std::fs::write(&path, "just: text\n").expect("writable");
        match execute(&path) {
            Err(Failure::Invalid(diagnostics)) => {
                assert!(!diagnostics.is_empty(), "a refusal names what was wrong");
            }
            Err(Failure::Internal(message)) => panic!("expected a caller mistake, got {message}"),
            Ok(value) => panic!("a non-graph file must not report a topology: {value}"),
        }
    }
}
