use std::path::Path;

use graphhelm_protocols::{
    ActorId, ExecutionId, OpaqueId, PersistedActor, PersistedActorType, ProjectId, RepositoryScope,
    WorkspaceId,
};
use graphhelm_simulation::{SimulationFixtures, SimulationServices};

use crate::commands::{SystemClock, UuidIds, event_store, owner, publish_loaded, repository_error};
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
    // #192: a warning-only lint pass reaches every exit past this point, not only the success
    // one — a caller whose publish or store lookup then failed already knew about the lint
    // warnings, and the reply must not look like it withheld something it already computed.
    let warnings = report.warnings;
    let version = match publish_loaded(&loaded, owner("owner-local")) {
        Ok(version) => version,
        Err(error) => return Outcome::internal("graph.simulate", error).with_warnings(warnings),
    };
    let store = match event_store(events) {
        Ok(store) => store,
        Err(error) => return repository_error("graph.simulate", &error).with_warnings(warnings),
    };
    let fixtures = match fixtures.map(load_fixtures).transpose() {
        Ok(value) => value.unwrap_or_default(),
        Err(error) => {
            return Outcome::domain("graph.simulate", vec![error]).with_warnings(warnings);
        }
    };
    let services = SimulationServices {
        event_repository: &store,
        scope: RepositoryScope::new(
            WorkspaceId::parse("workspace-local").expect("constant workspace id is valid"),
            ProjectId::parse("project-local").expect("constant project id is valid"),
            Some(
                ExecutionId::parse(&version.graph().metadata.execution_id)
                    .expect("validated graph execution id is wire-safe"),
            ),
        ),
        stream_id: OpaqueId::parse(&version.graph().metadata.execution_id)
            .expect("validated graph execution id is wire-safe"),
        actor: PersistedActor::new(
            PersistedActorType::System,
            ActorId::parse("system-cli").expect("constant actor id is valid"),
        ),
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
        )
        .with_warnings(warnings),
        Err(graphhelm_simulation::SimulationError::Repository(error)) => {
            repository_error("graph.simulate", &error).with_warnings(warnings)
        }
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
