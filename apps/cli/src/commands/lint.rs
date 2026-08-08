use std::path::Path;

use crate::output::Outcome;

pub fn run(file: &Path) -> Outcome {
    let loaded = match graphhelm_schema::load_graph(file) {
        Ok(loaded) => loaded,
        Err(diagnostics) => return Outcome::domain("graph.lint", diagnostics),
    };
    let report = graphhelm_graph::lint(&loaded.graph, &loaded.source);
    if report.errors.is_empty() {
        Outcome::success(
            "graph.lint",
            serde_json::json!({"errors": [], "warnings": report.warnings}),
        )
    } else {
        let mut diagnostics = report.errors;
        diagnostics.extend(report.warnings);
        Outcome::domain("graph.lint", diagnostics)
    }
}
