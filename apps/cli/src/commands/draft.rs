use std::io::Read;
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

/// The graph loader's input bound (`MAX_DOCUMENT_BYTES` in `graphhelm_schema`, which does not export it).
const MAX_DRAFT_BYTES: u64 = 4 * 1024 * 1024;

fn load_draft(
    path: &Path,
) -> Result<graphhelm_protocols::GraphDraft, graphhelm_protocols::Diagnostic> {
    let source = "graph-draft";
    let unreadable = || {
        graphhelm_protocols::Diagnostic::error(
            "GHS001_PARSE",
            "cannot read graph draft",
            "/",
            source,
        )
    };
    let oversized = || {
        graphhelm_protocols::Diagnostic::error(
            "GHS001_PARSE",
            "graph draft exceeds the size bound",
            "/",
            source,
        )
    };
    if std::fs::metadata(path).map_err(|_| unreadable())?.len() > MAX_DRAFT_BYTES {
        return Err(oversized());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(MAX_DRAFT_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|_| unreadable())?;
    if bytes.len() as u64 > MAX_DRAFT_BYTES {
        return Err(oversized());
    }
    let text = String::from_utf8(bytes).map_err(|_| unreadable())?;
    let parsed = if path.extension().and_then(|value| value.to_str()) == Some("json") {
        serde_json::from_str(&text).map_err(|_| ())
    } else {
        serde_yaml_ng::from_str(&text).map_err(|_| ())
    };
    parsed.map_err(|()| {
        graphhelm_protocols::Diagnostic::error("GHS001_PARSE", "invalid graph draft", "/", source)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Observable input bound: an oversized draft must be refused before it is parsed, while a
    // small one still loads. Sparse file, milliseconds, no production seam.
    #[test]
    fn an_oversized_draft_is_refused_before_parsing() {
        let directory = tempfile::tempdir().unwrap();
        let small = directory.path().join("small.json");
        std::fs::write(&small, r#"{"id":"d1","expectedVersion":1,"operations":[]}"#).unwrap();
        assert!(load_draft(&small).is_ok());

        let big = directory.path().join("big.json");
        let file = std::fs::File::create(&big).unwrap();
        file.set_len(MAX_DRAFT_BYTES + 1).unwrap();
        drop(file);
        let error = load_draft(&big).unwrap_err();
        assert_eq!(error.code, "GHS001_PARSE");
        assert!(error.message.contains("size bound"), "{}", error.message);
    }
}
