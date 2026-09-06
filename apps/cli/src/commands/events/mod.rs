pub(super) mod backup;
mod config;
pub(super) mod rebuild;
pub(super) mod restore;
pub(super) mod verify;

use std::path::Path;

use graphhelm_events::EventRepositoryError;
use graphhelm_protocols::{Diagnostic, ExecutionId, ProjectId, RepositoryScope, WorkspaceId};

use crate::output::Outcome;

pub(super) const ARGUMENT_CODE: &str = crate::error_codes::GHCLI001_ARGUMENT_INVALID;
pub(super) const CONFIG_CODE: &str = crate::error_codes::GHCLI002_CONFIG_INVALID;
const SOURCE: &str = "events-cli";

/// A redaction-safe operator failure.
///
/// Only a stable code, a fixed message, and a JSON Pointer ever reach the user. Filesystem paths,
/// DSNs, credentials, and backtraces are deliberately unrepresentable here.
pub(super) struct Failure {
    pub(super) code: &'static str,
    pub(super) message: String,
    pub(super) pointer: String,
}

impl Failure {
    fn into_outcome(self, command: &'static str) -> Outcome {
        Outcome::domain(
            command,
            vec![Diagnostic::error(
                self.code,
                self.message,
                &self.pointer,
                SOURCE,
            )],
        )
    }
}

pub(super) fn argument(message: &str, pointer: &str) -> Failure {
    Failure {
        code: ARGUMENT_CODE,
        message: message.to_owned(),
        pointer: pointer.to_owned(),
    }
}

pub(super) fn config_error(message: &str, pointer: &str) -> Failure {
    Failure {
        code: CONFIG_CODE,
        message: message.to_owned(),
        pointer: pointer.to_owned(),
    }
}

/// Maps a repository error onto its stable code without exposing the underlying path.
pub(super) fn repository_failure(error: &EventRepositoryError) -> Failure {
    Failure {
        code: error.code(),
        message: error.to_string(),
        pointer: "/repository".to_owned(),
    }
}

pub(super) fn finish<T>(
    command: &'static str,
    result: Result<T, Failure>,
    render: impl FnOnce(T) -> serde_json::Value,
) -> Outcome {
    match result {
        Ok(value) => Outcome::success(command, render(value)),
        Err(failure) => failure.into_outcome(command),
    }
}

/// Requires exactly one of the two repository selectors.
pub(super) fn single_selector<'a>(
    repository: Option<&'a Path>,
    config: Option<&'a Path>,
) -> Result<Selector<'a>, Failure> {
    match (repository, config) {
        (Some(_), Some(_)) => Err(argument(
            "--repository and --config are mutually exclusive",
            "/arguments",
        )),
        (Some(path), None) => Ok(Selector::Local(path)),
        (None, Some(path)) => Ok(Selector::Postgres(path)),
        (None, None) => Err(argument(
            "exactly one of --repository or --config is required",
            "/arguments",
        )),
    }
}

pub(super) enum Selector<'a> {
    Local(&'a Path),
    Postgres(&'a Path),
}

pub(super) fn scope(
    workspace: Option<&str>,
    project: Option<&str>,
    execution: Option<&str>,
) -> Result<RepositoryScope, Failure> {
    let (Some(workspace), Some(project)) = (workspace, project) else {
        return Err(argument(
            "--workspace and --project are required for a scoped operation",
            "/arguments",
        ));
    };
    let workspace = WorkspaceId::parse(workspace)
        .map_err(|_| argument("--workspace is not a valid identifier", "/workspace"))?;
    let project = ProjectId::parse(project)
        .map_err(|_| argument("--project is not a valid identifier", "/project"))?;
    let execution = match execution {
        Some(value) => Some(
            ExecutionId::parse(value)
                .map_err(|_| argument("--execution is not a valid identifier", "/execution"))?,
        ),
        None => None,
    };
    Ok(RepositoryScope::new(workspace, project, execution))
}

/// Confirms the on-disk repository declares a format this build supports.
pub(super) fn require_supported_format(root: &Path) -> Result<(), Failure> {
    graphhelm_events::LocalEventRepository::inspect_format(root)
        .map_err(|error| repository_failure(&error))
}

pub(super) fn require_absent_file(path: &Path, pointer: &str, flag: &str) -> Result<(), Failure> {
    if path.symlink_metadata().is_ok() {
        return Err(argument(
            &format!("{flag} already exists and is never overwritten"),
            pointer,
        ));
    }
    Ok(())
}

pub(super) fn require_existing_file(path: &Path, pointer: &str, flag: &str) -> Result<(), Failure> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| argument(&format!("{flag} does not exist"), pointer))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(argument(
            &format!("{flag} must be an existing regular file"),
            pointer,
        ));
    }
    Ok(())
}

/// Builds the bounded current-thread runtime used by the PostgreSQL-backed commands.
pub(super) fn runtime() -> Result<tokio::runtime::Runtime, Failure> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| config_error("the operator runtime could not be started", "/"))
}
