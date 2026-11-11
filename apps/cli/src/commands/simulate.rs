use std::io::Read;
use std::path::Path;

use graphhelm_protocols::{ActorId, OpaqueId, PersistedActor, PersistedActorType};
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
    // THE ADDRESSING RULE IS CALLED, NOT RESTATED (#820, enforcing #560). This spelled the rule
    // out -- the two constants plus the id as the third component -- while `addressable_scope` is
    // the one place allowed to know it. The two spellings agreed, which is what a duplicated ORACLE
    // does right up until the constants move; then this writer addresses streams that the three
    // id-only lookups in `serve` cannot find.
    //
    // `let ... else` rather than `expect`: `Failure` carries no `Debug`, and the branch is
    // unreachable for the same reason the previous `expect` was -- the id came off a graph that
    // `load_graph` and `publish_loaded` have both already accepted.
    let Ok(scope) = super::execution::addressable_scope(&version.graph().metadata.execution_id)
    else {
        panic!("validated graph execution id is wire-safe")
    };
    let services = SimulationServices {
        event_repository: &store,
        scope,
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

/// The graph loader's input bound (`MAX_DOCUMENT_BYTES` in `graphhelm_schema`, which does not export it).
const MAX_FIXTURES_BYTES: u64 = 4 * 1024 * 1024;

fn load_fixtures(path: &Path) -> Result<SimulationFixtures, graphhelm_protocols::Diagnostic> {
    let source = path.to_string_lossy().into_owned();
    let unreadable = || {
        graphhelm_protocols::Diagnostic::error(
            "GHS001_PARSE",
            "cannot read simulation fixtures",
            "/",
            &source,
        )
    };
    let oversized = || {
        graphhelm_protocols::Diagnostic::error(
            "GHS001_PARSE",
            "simulation fixtures exceed the size bound",
            "/",
            &source,
        )
    };
    if std::fs::metadata(path).map_err(|_| unreadable())?.len() > MAX_FIXTURES_BYTES {
        return Err(oversized());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(MAX_FIXTURES_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|_| unreadable())?;
    if bytes.len() as u64 > MAX_FIXTURES_BYTES {
        return Err(oversized());
    }
    serde_json::from_slice(&bytes).map_err(|_| {
        graphhelm_protocols::Diagnostic::error(
            "GHS001_PARSE",
            "invalid simulation fixture JSON",
            "/",
            source,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Observable input bound: an oversized fixtures file must be refused before it is parsed,
    // while a small one still loads. Sparse file, milliseconds, no production seam.
    #[test]
    fn oversized_fixtures_are_refused_before_parsing() {
        let directory = tempfile::tempdir().unwrap();
        let small = directory.path().join("small.json");
        std::fs::write(&small, "{}").unwrap();
        assert!(load_fixtures(&small).is_ok());

        let big = directory.path().join("big.json");
        let file = std::fs::File::create(&big).unwrap();
        file.set_len(MAX_FIXTURES_BYTES + 1).unwrap();
        drop(file);
        let error = load_fixtures(&big).unwrap_err();
        assert_eq!(error.code, "GHS001_PARSE");
        assert!(error.message.contains("size bound"), "{}", error.message);
    }
}
