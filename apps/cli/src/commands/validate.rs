use std::path::Path;

use crate::output::Outcome;

pub fn run(file: &Path) -> Outcome {
    match graphhelm_schema::load_graph(file) {
        Ok(loaded) => Outcome::success(
            "graph.validate",
            serde_json::json!({
                "source": loaded.source,
                "apiVersion": loaded.graph.api_version,
                "kind": loaded.graph.kind,
                "executionId": loaded.graph.metadata.execution_id,
                "version": loaded.graph.metadata.version
            }),
        ),
        Err(diagnostics) => Outcome::domain("graph.validate", diagnostics),
    }
}
