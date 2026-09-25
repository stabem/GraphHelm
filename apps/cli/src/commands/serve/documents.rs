//! HTTP adapter for the same registered-document commands exposed by the CLI.
use axum::body::Bytes;
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;

use super::{ServeState, mutation_bad_request, parse_mutation_headers, respond, respond_failure};
use crate::commands::execution::{self, documents};
use crate::output::Outcome;

const READ: &str = "execution.document_read";
const SAVE: &str = "execution.document_save";

fn protected_directories(state: &ServeState) -> Vec<std::path::PathBuf> {
    let mut paths = vec![state.events.to_path_buf()];
    if let Some(keyring) = &state.sealing {
        paths.push(keyring.directory.clone());
    }
    if let Some(wiring) = &state.runtime {
        paths.push(wiring.keyring_dir.clone());
        if let Some(model) = &wiring.model {
            paths.push(model.broker_dir.clone());
        }
    }
    paths
}

fn configuration(
    state: &ServeState,
    command: &'static str,
) -> Result<
    (
        std::path::PathBuf,
        std::sync::Arc<execution::signal::SignalKeyring>,
    ),
    Box<Response>,
> {
    let Some(project) = state.project.as_deref().map(std::path::Path::to_path_buf) else {
        return Err(Box::new(respond_failure(
            command,
            execution::execution_state(
                "document editing requires an explicit --project on this Runtime",
                "/project",
            ),
        )));
    };
    let Some(keyring) = state.sealing.clone() else {
        return Err(Box::new(respond_failure(
            command,
            execution::execution_state("document editing requires a sealed keyring", "/keyring"),
        )));
    };
    let root = project.canonicalize().map_err(|_| {
        respond_failure(
            command,
            execution::execution_state("the configured project is unavailable", "/project"),
        )
    })?;
    for directory in protected_directories(state) {
        let canonical = directory.canonicalize().map_err(|_| {
            respond_failure(
                command,
                execution::execution_state(
                    "a protected directory could not be verified",
                    "/project",
                ),
            )
        })?;
        if root.starts_with(&canonical) {
            return Err(Box::new(respond_failure(
                command,
                execution::execution_state(
                    "the configured project overlaps a protected directory",
                    "/project",
                ),
            )));
        }
    }
    Ok((project, keyring))
}

pub(super) async fn read(
    State(state): State<ServeState>,
    UrlPath(execution): UrlPath<String>,
    body: Bytes,
) -> Response {
    let request = match serde_json::from_slice::<documents::DocumentReference>(&body) {
        Ok(request) => request,
        Err(_) => return mutation_bad_request(READ, "invalid document reference", "/document"),
    };
    let (project, keyring) = match configuration(&state, READ) {
        Ok(c) => c,
        Err(r) => return *r,
    };
    let protected = protected_directories(&state);
    let result = tokio::task::spawn_blocking(move || {
        documents::check_protected_reference(
            &state.events,
            &execution,
            &project,
            &request,
            &keyring,
            &protected,
        )?;
        documents::read(&state.events, &execution, &project, &request, &keyring)
    })
    .await;
    reply(READ, result)
}

pub(super) async fn save(
    State(state): State<ServeState>,
    UrlPath(execution): UrlPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(_) => return mutation_bad_request(SAVE, "invalid document edit", "/document"),
    };
    let identity = match parse_mutation_headers(&headers, SAVE, &execution, &payload, &["edit"]) {
        Ok(i) => i,
        Err(r) => return r,
    };
    // File revisions, not stream heads, are the concurrency precondition for this command.
    if identity.if_match.is_some() {
        return mutation_bad_request(
            SAVE,
            "use expectedSha256 for document revisions; If-Match is not supported here",
            "/ifMatch",
        );
    }
    if headers
        .get("x-graphhelm-actor-type")
        .and_then(|h| h.to_str().ok())
        != Some("owner")
    {
        return mutation_bad_request(
            SAVE,
            "only the owner can save project documents",
            "/actorType",
        );
    }
    let request = match serde_json::from_value::<documents::SaveDocument>(payload) {
        Ok(request) if request.idempotency_key == identity.idempotency_header => request,
        _ => {
            return mutation_bad_request(
                SAVE,
                "invalid edit or mismatched Idempotency-Key",
                "/document",
            );
        }
    };
    let (project, keyring) = match configuration(&state, SAVE) {
        Ok(c) => c,
        Err(r) => return *r,
    };
    let protected = protected_directories(&state);
    let result = tokio::task::spawn_blocking(move || {
        documents::save_with_protected(
            &state.events,
            &execution,
            &project,
            &request,
            identity.actor,
            &keyring,
            &protected,
        )
    })
    .await;
    reply(SAVE, result)
}

fn reply(
    command: &'static str,
    result: Result<Result<serde_json::Value, execution::Failure>, tokio::task::JoinError>,
) -> Response {
    match result {
        Ok(Ok(value)) => respond(StatusCode::OK, Outcome::success(command, value).output),
        Ok(Err(error)) => respond_failure(command, error),
        Err(_) => respond(
            StatusCode::INTERNAL_SERVER_ERROR,
            Outcome::internal(command, "the document operation could not be confirmed").output,
        ),
    }
}
