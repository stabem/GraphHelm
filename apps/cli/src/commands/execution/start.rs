use std::path::Path;

use graphhelm_events::PreparedAppend;
use graphhelm_graph::GraphVersion;
use graphhelm_protocols::{
    EventKind, ExecutionId, ExecutionMode, ExecutionStarted, IdGenerator, NewEvent, OpaqueId,
    ProjectId, RepositoryScope, Sensitivity, WireHash, WorkspaceId,
};

use super::driver::drive_to_quiescence;
use super::{
    Failure, PROJECT, WORKSPACE, argument, execution_state, finish, load_fixtures, render,
    replay_failure, repository_failure,
};
use crate::commands::{UuidIds, event_store, owner, publish_loaded};
use crate::output::Outcome;

const COMMAND: &str = "execution.start";

pub fn run(
    file: &Path,
    events: &Path,
    fixtures: Option<&Path>,
    mode: &str,
    execution: Option<&str>,
) -> Outcome {
    let loaded = match graphhelm_schema::load_graph(file) {
        Ok(loaded) => loaded,
        Err(diagnostics) => return Outcome::domain(COMMAND, diagnostics),
    };
    let report = graphhelm_graph::lint(&loaded.graph, &loaded.source);
    if !report.errors.is_empty() {
        let mut diagnostics = report.errors;
        diagnostics.extend(report.warnings);
        return Outcome::domain(COMMAND, diagnostics);
    }
    let version = match publish_loaded(&loaded, owner("owner-local")) {
        Ok(version) => version,
        Err(error) => return Outcome::internal(COMMAND, error),
    };
    finish(
        COMMAND,
        execute(&version, events, fixtures, mode, execution),
        |value| value,
    )
}

fn execute(
    version: &GraphVersion,
    events: &Path,
    fixtures: Option<&Path>,
    mode: &str,
    execution: Option<&str>,
) -> Result<serde_json::Value, Failure> {
    let mode = parse_mode(mode)?;
    let store = event_store(events).map_err(|error| repository_failure(&error))?;
    let fixtures = load_fixtures(fixtures)?;
    let (stream_id, execution_scope_id) = resolve_execution_id(version, execution)?;
    let scope = RepositoryScope::new(
        WorkspaceId::parse(WORKSPACE).expect("constant workspace id is valid"),
        ProjectId::parse(PROJECT).expect("constant project id is valid"),
        Some(execution_scope_id),
    );

    let history = store
        .read_replay_stream(&scope, stream_id.as_str())
        .map_err(|error| repository_failure(&error))?;
    let existing = graphhelm_events::replay(&scope, stream_id.as_str(), &history)
        .map_err(|error| replay_failure(&error))?;
    // The fold's own `ExecutionStarted` arm would call a second one corrupt; the CLI refuses it
    // here so the diagnostic names the real reason instead of a bare integrity failure.
    if existing.execution_id.is_some() {
        return Err(execution_state(
            "an execution has already started on this stream",
            "/execution",
        ));
    }

    let actor = super::system_actor();
    let graph_hash = WireHash::parse(version.content_hash().as_str()).map_err(|_| {
        execution_state(
            "the graph hash could not be represented on the wire",
            "/execution",
        )
    })?;

    let next_sequence = store
        .next_sequence(&scope, stream_id.as_str())
        .map_err(|error| repository_failure(&error))?;
    let request = PreparedAppend::new(
        scope.clone(),
        stream_id.clone(),
        next_sequence,
        vec![NewEvent::new(
            OpaqueId::parse(UuidIds.next_id("execution-started"))
                .expect("uuid-derived id is wire-safe"),
            actor.clone(),
            Sensitivity::Internal,
            EventKind::ExecutionStarted(ExecutionStarted {
                execution_id: stream_id.clone(),
                graph_version: version.number(),
                graph_hash,
                mode,
            }),
            vec![],
            vec![],
        )],
        vec![],
        vec![],
    )
    .map_err(|error| repository_failure(&error))?;
    store
        .append_atomic(&request)
        .map_err(|error| repository_failure(&error))?;

    let projection = drive_to_quiescence(
        &store,
        &scope,
        stream_id.as_str(),
        &version.graph().spec,
        &fixtures,
        &actor,
    )?;

    Ok(render(&projection))
}

fn parse_mode(mode: &str) -> Result<ExecutionMode, Failure> {
    match mode {
        "autopilot" => Ok(ExecutionMode::Autopilot),
        "supervised" => Ok(ExecutionMode::Supervised),
        "manual" => Ok(ExecutionMode::Manual),
        _ => Err(argument(
            "--mode must be one of autopilot, supervised or manual",
            "/mode",
        )),
    }
}

/// The execution identity to drive: `--execution` when given, otherwise the graph's own
/// `metadata.executionId` — the same convention `graph simulate` uses unconditionally. Returns
/// both the `OpaqueId` (stream identity) and `ExecutionId` (scope identity) parsed from the same
/// string, matching `simulate.rs`'s own double-parse of `version.graph().metadata.execution_id`.
fn resolve_execution_id(
    version: &GraphVersion,
    execution: Option<&str>,
) -> Result<(OpaqueId, ExecutionId), Failure> {
    match execution {
        Some(value) => {
            let stream_id = OpaqueId::parse(value)
                .map_err(|_| argument("--execution is not a valid identifier", "/execution"))?;
            let scope_id = ExecutionId::parse(value)
                .map_err(|_| argument("--execution is not a valid identifier", "/execution"))?;
            Ok((stream_id, scope_id))
        }
        None => {
            let raw = &version.graph().metadata.execution_id;
            let stream_id =
                OpaqueId::parse(raw).expect("validated graph execution id is wire-safe");
            let scope_id =
                ExecutionId::parse(raw).expect("validated graph execution id is wire-safe");
            Ok((stream_id, scope_id))
        }
    }
}
