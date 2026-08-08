use std::path::Path;

use crate::output::Outcome;

pub fn run(file: &Path) -> Outcome {
    let loaded = match graphhelm_schema::load_graph(file) {
        Ok(loaded) => loaded,
        Err(diagnostics) => return Outcome::domain("graph.hash", diagnostics),
    };
    match graphhelm_graph::semantic_hash(&loaded.graph) {
        Ok(hash) => Outcome::success("graph.hash", serde_json::json!({"hash": hash})),
        Err(error) => Outcome::internal("graph.hash", error.to_string()),
    }
}
