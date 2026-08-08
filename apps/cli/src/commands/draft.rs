use std::path::Path;

use graphhelm_governor::ApplyServices;

use crate::commands::{SystemClock, UuidIds, ensure_imported, event_store, owner, publish_loaded};
use crate::output::Outcome;

pub fn run(base_file: &Path, draft_file: &Path, actor_id: &str, events: &Path) -> Outcome {
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
    let actor = owner(actor_id);
    let base = match publish_loaded(&loaded, actor.clone()) {
        Ok(version) => version,
        Err(error) => return Outcome::internal("graph.draft.apply", error),
    };
    let draft = match load_draft(draft_file) {
        Ok(draft) => draft,
        Err(diagnostic) => return Outcome::domain("graph.draft.apply", vec![diagnostic]),
    };
    let store = event_store(events);
    if let Err(error) = ensure_imported(&store, &base, &loaded.source) {
        return Outcome::internal("graph.draft.apply", error);
    }
    let services = ApplyServices {
        event_store: &store,
        stream_id: &base.graph().metadata.execution_id,
        actor,
        clock: &SystemClock,
        ids: &UuidIds,
    };
    match graphhelm_governor::apply_draft(&base, &draft, &services) {
        Ok(result) => Outcome::success(
            "graph.draft.apply",
            serde_json::json!({
                "version": result.version.to_record(),
                "waivers": result.waivers,
                "events": result.events,
                "policyReport": result.policy_report,
            }),
        ),
        Err(error) if error.is_io() => Outcome::internal("graph.draft.apply", error.to_string()),
        Err(error) => Outcome::application(
            "graph.draft.apply",
            graphhelm_protocols::Diagnostic::error(
                error.code(),
                error.to_string(),
                "/",
                draft_file.to_string_lossy(),
            ),
        ),
    }
}

fn load_draft(
    path: &Path,
) -> Result<graphhelm_protocols::GraphDraft, graphhelm_protocols::Diagnostic> {
    let source = path.to_string_lossy().into_owned();
    let text = std::fs::read_to_string(path).map_err(|_| {
        graphhelm_protocols::Diagnostic::error(
            "GHS001_PARSE",
            "cannot read graph draft",
            "/",
            &source,
        )
    })?;
    let parsed = if path.extension().and_then(|value| value.to_str()) == Some("json") {
        serde_json::from_str(&text).map_err(|error| error.to_string())
    } else {
        serde_yaml_ng::from_str(&text).map_err(|error| error.to_string())
    };
    parsed.map_err(|error| {
        graphhelm_protocols::Diagnostic::error(
            "GHS001_PARSE",
            format!("invalid graph draft: {error}"),
            "/",
            source,
        )
    })
}
