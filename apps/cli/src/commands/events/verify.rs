use std::path::Path;

use graphhelm_events::{
    AsyncEventRepository, EventRepositoryError, IntegrityReport, LocalEventRepository,
    LocalRepositoryInspection, VerifyRangeRequest,
};
use graphhelm_protocols::RepositoryScope;
use serde_json::json;

use super::{Failure, Selector, argument, config, finish, repository_failure};
use crate::output::Outcome;

const COMMAND: &str = "events.verify";
const MAX_EVENTS_LIMIT: u32 = 100_000;
const DEFAULT_MAX_EVENTS: u32 = 1_000;

pub(in crate::commands) struct Request<'a> {
    pub(in crate::commands) repository: Option<&'a Path>,
    pub(in crate::commands) config: Option<&'a Path>,
    pub(in crate::commands) workspace: Option<&'a str>,
    pub(in crate::commands) project: Option<&'a str>,
    pub(in crate::commands) execution: Option<&'a str>,
    pub(in crate::commands) stream: Option<&'a str>,
    pub(in crate::commands) start: Option<u64>,
    pub(in crate::commands) max_events: Option<u32>,
}

pub(in crate::commands) fn run(request: Request<'_>) -> Outcome {
    finish(COMMAND, execute(&request), |value| value)
}

fn execute(request: &Request<'_>) -> Result<serde_json::Value, Failure> {
    let selector = super::single_selector(request.repository, request.config)?;
    let range = plan_range(request)?;
    match selector {
        Selector::Local(root) => verify_local(root, range),
        Selector::Postgres(path) => verify_postgres(path, range),
    }
}

struct Range {
    stream: String,
    scope: RepositoryScope,
    start: u64,
    max_events: u32,
}

/// Validates the requested window before any repository is opened or any connection is made.
fn plan_range(request: &Request<'_>) -> Result<Option<Range>, Failure> {
    let scoped = request.workspace.is_some()
        || request.project.is_some()
        || request.execution.is_some()
        || request.start.is_some()
        || request.max_events.is_some();
    let Some(stream) = request.stream else {
        if scoped {
            return Err(argument(
                "--stream is required when a scope or range is supplied",
                "/stream",
            ));
        }
        return Ok(None);
    };
    let scope = super::scope(request.workspace, request.project, request.execution)?;
    let start = request.start.unwrap_or(1);
    let max_events = request.max_events.unwrap_or(DEFAULT_MAX_EVENTS);
    if start == 0 {
        return Err(argument("--start must be at least 1", "/start"));
    }
    if max_events == 0 || max_events > MAX_EVENTS_LIMIT {
        return Err(argument(
            "--max-events must be between 1 and 100000",
            "/max-events",
        ));
    }
    Ok(Some(Range {
        stream: stream.to_owned(),
        scope,
        start,
        max_events,
    }))
}

/// The local repository exposes format recognition but no chain-verification entry point; range
/// verification is an `AsyncEventRepository` capability that only the PostgreSQL adapter provides.
/// A range against a local repository is therefore refused rather than silently ignored.
fn verify_local(root: &Path, range: Option<Range>) -> Result<serde_json::Value, Failure> {
    if range.is_some() {
        return Err(argument(
            "range verification requires --config; --repository only recognizes the stored format",
            "/arguments",
        ));
    }
    inspection_result(LocalEventRepository::inspect_repository(root))?;
    Ok(json!({"formatSupported": true, "verified": false}))
}

fn inspection_result(
    result: Result<LocalRepositoryInspection, EventRepositoryError>,
) -> Result<(), Failure> {
    match result {
        Ok(LocalRepositoryInspection::Missing) => {
            Err(argument("--repository does not exist", "/repository"))
        }
        Ok(LocalRepositoryInspection::Storage) => {
            Err(repository_failure(&EventRepositoryError::Storage))
        }
        Ok(LocalRepositoryInspection::Integrity) => {
            Err(repository_failure(&EventRepositoryError::Integrity))
        }
        Ok(LocalRepositoryInspection::Recognized) => Ok(()),
        Err(error) => Err(repository_failure(&error)),
    }
}

fn verify_postgres(config_path: &Path, range: Option<Range>) -> Result<serde_json::Value, Failure> {
    let configuration = config::load(config_path)?;
    let Some(range) = range else {
        return Err(argument(
            "--stream, --workspace and --project are required when verifying a PostgreSQL repository",
            "/stream",
        ));
    };
    let provider = configuration.key_provider()?;
    let request = range.into_request()?;
    let admin_url = configuration.admin_url().to_owned();
    let report = super::runtime()?.block_on(async move {
        let store =
            graphhelm_postgres_event_store::PostgresEventStore::connect(&admin_url, 1, provider)
                .await
                .map_err(|e| repository_failure(&e))?;
        AsyncEventRepository::verify_range(&store, request)
            .await
            .map_err(|e| repository_failure(&e))
    })?;
    Ok(report_json(&report))
}

impl Range {
    fn into_request(self) -> Result<VerifyRangeRequest, Failure> {
        VerifyRangeRequest::new(self.scope, self.stream, self.start, self.max_events)
            .map_err(|e| repository_failure(&e))
    }
}

fn report_json(report: &IntegrityReport) -> serde_json::Value {
    json!({
        "formatSupported": true,
        "verified": true,
        "verifiedEvents": report.verified_events,
        "verifiedThrough": report.verified_through,
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use graphhelm_events::LocalRepositoryInspection;

    use super::{inspection_result, verify_local};

    #[test]
    fn storage_inspection_outcome_keeps_the_repository_storage_diagnostic() {
        let failure = inspection_result(Ok(LocalRepositoryInspection::Storage)).unwrap_err();
        assert_eq!(failure.code, "GHE008_STORAGE_FAILURE");
        assert_eq!(failure.pointer, "/repository");
        assert!(!failure.message.contains(':'));
    }

    #[test]
    fn empty_repository_selection_is_an_argument_failure_without_a_path_leak() {
        let failure = verify_local(Path::new(""), None).unwrap_err();

        assert_eq!(failure.code, crate::error_codes::GHCLI001_ARGUMENT_INVALID);
        assert_eq!(failure.pointer, "/repository");
        assert!(!failure.message.contains(':'));
        assert!(
            !failure
                .message
                .contains(&std::env::current_dir().unwrap().display().to_string())
        );
    }
}
