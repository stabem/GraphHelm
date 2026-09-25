use std::path::Path;

use graphhelm_protocols::{ExecutionId, OpaqueId, ProjectId, RepositoryScope, WorkspaceId};

use crate::commands::{event_store, repository_error};
use crate::output::Outcome;

pub fn run(
    events: &Path,
    workspace: Option<&str>,
    project: Option<&str>,
    execution: Option<&str>,
    stream: Option<&str>,
) -> Outcome {
    let store = match event_store(events) {
        Ok(store) => store,
        Err(error) => return repository_error("graph.replay", &error),
    };
    let selected = match (workspace, project, stream) {
        (None, None, None) if execution.is_none() => store.read_unique_replay_stream(),
        (Some(workspace), Some(project), Some(stream)) => {
            parse_selection(workspace, project, execution, stream).and_then(|selection| {
                store
                    .read_replay_stream(&selection.scope, &selection.stream_id)
                    .map(|events| (selection, events))
            })
        }
        _ => Err(graphhelm_events::EventRepositoryError::StreamSelectionRequired),
    };
    let (selection, events) = match selected {
        Ok(selected) => selected,
        Err(error) => return repository_error("graph.replay", &error),
    };
    match graphhelm_events::replay(&selection.scope, &selection.stream_id, &events) {
        Ok(projection) => Outcome::success(
            "graph.replay",
            serde_json::to_value(projection).expect("projection is serializable"),
        ),
        Err(error) => Outcome::domain(
            "graph.replay",
            vec![graphhelm_protocols::Diagnostic::error(
                error.code(),
                error.to_string(),
                "/",
                "event-repository",
            )],
        ),
    }
}

fn parse_selection(
    workspace: &str,
    project: &str,
    execution: Option<&str>,
    stream: &str,
) -> Result<graphhelm_events::RepositoryStream, graphhelm_events::EventRepositoryError> {
    let scope = RepositoryScope::new(
        WorkspaceId::parse(workspace)
            .map_err(|_| graphhelm_events::EventRepositoryError::Invalid)?,
        ProjectId::parse(project).map_err(|_| graphhelm_events::EventRepositoryError::Invalid)?,
        execution
            .map(ExecutionId::parse)
            .transpose()
            .map_err(|_| graphhelm_events::EventRepositoryError::Invalid)?,
    );
    let stream =
        OpaqueId::parse(stream).map_err(|_| graphhelm_events::EventRepositoryError::Invalid)?;
    Ok(graphhelm_events::RepositoryStream {
        scope,
        stream_id: stream.to_string(),
    })
}
