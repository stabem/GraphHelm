use std::path::Path;

use graphhelm_events::LocalEventRepository;

use crate::commands::repository_error;
use crate::output::Outcome;

/// Draft publication is intentionally fail-closed until the operator CLI has
/// an externally mediated KeyProvider configured under ADR-022.
pub fn run(base_file: &Path, draft_file: &Path, _actor_id: &str, events: &Path) -> Outcome {
    let loaded = match graphhelm_schema::load_graph(base_file) {
        Ok(loaded) => loaded,
        Err(diagnostics) => return Outcome::domain("graph.draft.apply", diagnostics),
    };
    let report = graphhelm_graph::lint(&loaded.graph, &loaded.source);
    if !report.errors.is_empty() {
        let mut diagnostics = report.errors;
        diagnostics.extend(report.warnings);
        return Outcome::domain("graph.draft.apply", diagnostics);
    }
    // #192: a warning-only lint pass reaches every exit past this point — this command is
    // fail-closed by design (ADR-022) and never reaches a real success today, but the lint
    // warnings were still genuinely computed and must not read as withheld just because the
    // eventual failure is for an unrelated (key-provider) reason.
    let warnings = report.warnings;
    if let Err(diagnostic) = load_draft(draft_file) {
        return Outcome::domain("graph.draft.apply", vec![diagnostic]).with_warnings(warnings);
    }
    if let Err(error) = LocalEventRepository::inspect_format(events) {
        return repository_error("graph.draft.apply", &error).with_warnings(warnings);
    }

    Outcome::application(
        "graph.draft.apply",
        graphhelm_protocols::Diagnostic::error(
            "GHK001_KEY_UNAVAILABLE",
            "external key provider configuration is required",
            "/keyProvider",
            "operator-configuration",
        ),
    )
    .with_warnings(warnings)
}

fn load_draft(
    path: &Path,
) -> Result<graphhelm_protocols::GraphDraft, graphhelm_protocols::Diagnostic> {
    let source = "graph-draft";
    let text = std::fs::read_to_string(path).map_err(|_| {
        graphhelm_protocols::Diagnostic::error(
            "GHS001_PARSE",
            "cannot read graph draft",
            "/",
            source,
        )
    })?;
    let parsed = if path.extension().and_then(|value| value.to_str()) == Some("json") {
        serde_json::from_str(&text).map_err(|_| ())
    } else {
        serde_yaml_ng::from_str(&text).map_err(|_| ())
    };
    parsed.map_err(|()| {
        graphhelm_protocols::Diagnostic::error("GHS001_PARSE", "invalid graph draft", "/", source)
    })
}
