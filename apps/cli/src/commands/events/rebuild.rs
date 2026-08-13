use std::path::Path;
use std::sync::Arc;

use graphhelm_events::{ProjectionRebuildRequest, ProjectionRebuilder};
use serde_json::json;

use super::{Failure, argument, config, finish, repository_failure};
use crate::output::Outcome;

const COMMAND: &str = "events.rebuild";
const DEFAULT_PROJECTION: &str = "execution";
const DEFAULT_PROJECTION_VERSION: u32 = 1;
const DEFAULT_PAGE_SIZE: u32 = 1_000;

pub(in crate::commands) struct Request<'a> {
    pub(in crate::commands) config: Option<&'a Path>,
    pub(in crate::commands) workspace: Option<&'a str>,
    pub(in crate::commands) project: Option<&'a str>,
    pub(in crate::commands) execution: Option<&'a str>,
    pub(in crate::commands) stream: Option<&'a str>,
    pub(in crate::commands) generation: Option<u64>,
    pub(in crate::commands) page_size: Option<u32>,
}

pub(in crate::commands) fn run(request: Request<'_>) -> Outcome {
    finish(COMMAND, execute(&request), |value| value)
}

/// Projections are durable only in the PostgreSQL adapter, so rebuild is a PostgreSQL-only
/// operation. The local repository has no projection generation storage to swap.
fn execute(request: &Request<'_>) -> Result<serde_json::Value, Failure> {
    let config_path = config::resolve_path(request.config)?;
    let Some(stream) = request.stream else {
        return Err(argument("--stream is required", "/stream"));
    };
    let scope = super::scope(request.workspace, request.project, request.execution)?;
    let generation = request.generation.unwrap_or(1);
    if generation == 0 {
        return Err(argument("--generation must be at least 1", "/generation"));
    }
    let page_size = request.page_size.unwrap_or(DEFAULT_PAGE_SIZE);
    if page_size == 0 {
        return Err(argument("--page-size must be at least 1", "/page-size"));
    }
    let rebuild = ProjectionRebuildRequest::new(
        scope,
        stream.to_owned(),
        DEFAULT_PROJECTION.to_owned(),
        DEFAULT_PROJECTION_VERSION,
        generation,
        page_size,
    )
    .map_err(|_| {
        argument(
            "the requested projection generation and page size are out of range",
            "/generation",
        )
    })?;

    let configuration = config::load(&config_path)?;
    let provider = configuration.key_provider()?;
    let admin_url = configuration.admin_url().to_owned();
    let watermark = super::runtime()?.block_on(async move {
        let store = Arc::new(
            graphhelm_postgres_event_store::PostgresEventStore::connect(&admin_url, 1, provider)
                .await
                .map_err(|e| repository_failure(&e))?,
        );
        let rebuilder = ProjectionRebuilder::new(store.clone(), store);
        rebuilder
            .rebuild(rebuild)
            .await
            .map_err(|e| repository_failure(&e))
    })?;
    Ok(json!({
        "generation": watermark.watermark().generation(),
        "lastSequence": watermark.watermark().last_sequence(),
        "projectionVersion": watermark.watermark().projection_version(),
    }))
}
