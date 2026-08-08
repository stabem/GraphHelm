use std::path::Path;

use graphhelm_simulation::{SimulationFixtures, SimulationServices};

use crate::commands::{SystemClock, UuidIds, ensure_imported, event_store, owner, publish_loaded};
use crate::output::Outcome;

pub fn run(file: &Path, events: &Path, fixtures: Option<&Path>) -> Outcome {
    let loaded = match graphhelm_schema::load_graph(file) {
        Ok(loaded) => loaded,
        Err(diagnostics) => return Outcome::domain("graph.simulate", diagnostics),
    };
    let report = graphhelm_graph::lint(&loaded.graph, &loaded.source);
    if !report.errors.is_empty() {
        let mut diagnostics = report.errors;
        diagnostics.extend(report.warnings);
        return Outcome::domain("graph.simulate", diagnostics);
    }
    let version = match publish_loaded(&loaded, owner("owner-local")) {
        Ok(version) => version,
        Err(error) => return Outcome::internal("graph.simulate", error),
    };
    let store = event_store(events);
    if let Err(error) = ensure_imported(&store, &version, &loaded.source) {
        return Outcome::internal("graph.simulate", error);
    }
    let fixtures = match fixtures.map(load_fixtures).transpose() {
        Ok(value) => value.unwrap_or_default(),
        Err(error) => return Outcome::domain("graph.simulate", vec![error]),
    };
    let services = SimulationServices {
        event_store: &store,
        stream_id: &version.graph().metadata.execution_id,
        clock: &SystemClock,
        ids: &UuidIds,
    };
    match graphhelm_simulation::simulate(&version, &fixtures, &services) {
        Ok(result) => Outcome::success(
            "graph.simulate",
            serde_json::json!({
                "simulationId": result.simulation_id,
                "startedAt": result.started_at,
                "status": result.status,
                "nodeStates": result.node_states,
                "diagnostics": result.diagnostics,
                "events": result.events,
            }),
        ),
        Err(error) => Outcome::internal("graph.simulate", error.to_string()),
    }
}

fn load_fixtures(path: &Path) -> Result<SimulationFixtures, graphhelm_protocols::Diagnostic> {
    let source = path.to_string_lossy().into_owned();
    let bytes = std::fs::read(path).map_err(|_| {
        graphhelm_protocols::Diagnostic::error(
            "GHS001_PARSE",
            "cannot read simulation fixtures",
            "/",
            &source,
        )
    })?;
    serde_json::from_slice(&bytes).map_err(|_| {
        graphhelm_protocols::Diagnostic::error(
            "GHS001_PARSE",
            "invalid simulation fixture JSON",
            "/",
            source,
        )
    })
}
