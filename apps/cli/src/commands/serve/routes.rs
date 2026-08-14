//! The Public Runtime API's endpoint handlers (Milestone 05a). Every handler delegates to the same
//! `commands::execution` command layer the CLI uses — D-039's "never a second path" rule as code:
//! the store's own resolve/replay read is the only read path, and a mutation's own `execute()` is
//! the only write path, whether reached from a terminal or from HTTP.

use std::path::{Path, PathBuf};

use axum::body::Bytes;
use axum::extract::{Path as UrlPath, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use graphhelm_graph::GraphVersion;
use graphhelm_protocols::Diagnostic;

use super::{
    MutationError, ServeState, parse_mutation_headers, respond, respond_failure,
    run_idempotent_mutation,
};
use crate::commands::{event_store, execution, owner, publish_loaded};
use crate::output::Outcome;

const STATUS_COMMAND: &str = "execution.status";
const EVENTS_COMMAND: &str = "execution.events";
const START_COMMAND: &str = "execution.start";
const SIGNAL_COMMAND: &str = "execution.signal";
const APPROVE_COMMAND: &str = "execution.approve";
const PAUSE_COMMAND: &str = "execution.pause";
const RESUME_COMMAND: &str = "execution.resume";
const CANCEL_COMMAND: &str = "execution.cancel";
const SOURCE: &str = "serve-cli";

/// The events tail's default page size when `limit` is absent.
const DEFAULT_EVENTS_LIMIT: usize = 100;
/// The largest page `limit` may request. A larger request is refused with 400 rather than silently
/// truncated, per the plan: the caller must always be able to tell "there is more" from the
/// response shape, not guess it from getting back fewer events than asked for.
const MAX_EVENTS_LIMIT: usize = 1000;

/// `GET /v1/executions/{id}`: replies `execution.status`'s own `data` — the exact same
/// `execution::status::execute` the CLI's `execution status` runs, so the two report byte-identical
/// output for the same stream ("one store, one truth").
pub(super) async fn status(
    State(state): State<ServeState>,
    UrlPath(execution_id): UrlPath<String>,
) -> Response {
    match execution::status::execute(&state.events, Some(&execution_id)) {
        Ok(value) => respond(
            StatusCode::OK,
            Outcome::success(STATUS_COMMAND, value).output,
        ),
        Err(failure) => respond_failure(STATUS_COMMAND, failure),
    }
}

/// `GET /v1/executions/{id}/events?after=N&limit=M`: a page of the raw event envelope tail, read
/// through the same `execution::resolve_stream` the CLI's replay path uses, sliced by `after`
/// (exclusive) and `limit` (default 100, max 1000). Envelopes serialize verbatim — their payloads
/// are safe by construction (D-036: no free-form content ever reaches the wire inside an event),
/// which is what makes serving them raw legitimate. The reply's `head` is the stream's current last
/// sequence (0 for an empty stream), matching `status`'s own `headSequence`.
pub(super) async fn events(
    State(state): State<ServeState>,
    UrlPath(execution_id): UrlPath<String>,
    RawQuery(query): RawQuery,
) -> Response {
    let (after, limit) = match parse_events_query(query.as_deref().unwrap_or("")) {
        Ok(parsed) => parsed,
        Err((message, pointer)) => return bad_request(EVENTS_COMMAND, message, pointer),
    };

    match execute_events_tail(&state.events, &execution_id, after, limit) {
        Ok(value) => respond(
            StatusCode::OK,
            Outcome::success(EVENTS_COMMAND, value).output,
        ),
        Err(failure) => respond_failure(EVENTS_COMMAND, failure),
    }
}

fn execute_events_tail(
    events_dir: &Path,
    execution_id: &str,
    after: u64,
    limit: usize,
) -> Result<serde_json::Value, execution::Failure> {
    let store = event_store(events_dir).map_err(|error| execution::repository_failure(&error))?;
    let (_, _, history) = execution::resolve_stream(&store, Some(execution_id))?;
    let head = history.last().map_or(0, |event| event.sequence);
    let page: Vec<_> = history
        .iter()
        .filter(|event| event.sequence > after)
        .take(limit)
        .collect();
    Ok(serde_json::json!({ "events": page, "head": head }))
}

/// Hand-parsed rather than axum's `Query` extractor: a malformed value must still reply the
/// standard four-key envelope, not axum's own rejection body. `after` defaults to 0 (from the
/// beginning); `limit` defaults to `DEFAULT_EVENTS_LIMIT`. Returns `(message, pointer)` on failure.
fn parse_events_query(query: &str) -> Result<(u64, usize), (&'static str, &'static str)> {
    let mut after = 0_u64;
    let mut limit = DEFAULT_EVENTS_LIMIT;
    for pair in query.split('&').filter(|segment| !segment.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        match key {
            "after" => {
                after = value
                    .parse()
                    .map_err(|_| ("after must be a non-negative integer", "/after"))?;
            }
            "limit" => {
                limit = value
                    .parse()
                    .map_err(|_| ("limit must be a positive integer", "/limit"))?;
            }
            _ => {}
        }
    }
    if limit == 0 || limit > MAX_EVENTS_LIMIT {
        return Err(("limit must be between 1 and 1000", "/limit"));
    }
    Ok((after, limit))
}

fn bad_request(command: &'static str, message: &str, pointer: &str) -> Response {
    respond(
        StatusCode::BAD_REQUEST,
        Outcome::domain(
            command,
            vec![Diagnostic::error(
                "GHCLI001_ARGUMENT_INVALID",
                message,
                pointer,
                SOURCE,
            )],
        )
        .output,
    )
}

/// Loads, lints, and publishes the graph at `file` — the same three steps `start::run`/
/// `resume::run` perform on the CLI side before calling their own `execute`, reproduced here (rather
/// than widening `execution/mod.rs`, outside this task's file set) so the API's `start`/`resume`
/// handlers can drive the identical shared `execute` from a request body's `"file"` path instead of
/// a CLI flag — the same file-read/publish-vs-execute split `signal::run_from_file` established in
/// Task 3, applied to the one extra step `start`/`resume` need that `signal`/`approve` do not.
///
/// A schema or lint failure is a 400 (argument-shaped: an unreadable file or an invalid graph); a
/// publish failure is a 500 (unexpected, not a caller mistake) — the same `Outcome::domain` /
/// `Outcome::internal` distinction `run`'s own CLI-side error handling already makes for identical
/// failures, relayed through `respond` here instead of to stdout.
///
/// `Err` carries the already-built `Response` to return directly (every call site is `match ... {
/// Err(response) => return response, ... }`), the same `clippy::result_large_err` accepted-not-
/// worked-around tradeoff `parse_mutation_headers` in `serve/mod.rs` already documents — boxing it
/// would add an allocation to a failure path this function exists to keep cheap, for a function
/// called at most once per `start`/`resume` request.
#[allow(clippy::result_large_err)]
fn load_and_publish(file: &Path, command: &'static str) -> Result<GraphVersion, Response> {
    let loaded = graphhelm_schema::load_graph(file).map_err(|diagnostics| {
        respond(
            StatusCode::BAD_REQUEST,
            Outcome::domain(command, diagnostics).output,
        )
    })?;
    let report = graphhelm_graph::lint(&loaded.graph, &loaded.source);
    if !report.errors.is_empty() {
        let mut diagnostics = report.errors;
        diagnostics.extend(report.warnings);
        return Err(respond(
            StatusCode::BAD_REQUEST,
            Outcome::domain(command, diagnostics).output,
        ));
    }
    publish_loaded(&loaded, owner("owner-local")).map_err(|error| {
        respond(
            StatusCode::INTERNAL_SERVER_ERROR,
            Outcome::internal(command, error).output,
        )
    })
}

/// `POST /v1/executions/{id}/start`: body `{"file": "<path>", "fixtures": "<path, omitted for
/// none>", "mode": "autopilot|supervised|manual"}`, mirroring the CLI's `--file`/`--fixtures`/
/// `--mode`. The operator-supplied `file`/`fixtures` paths are used exactly as the CLI would use
/// them (D-040's premise: the server is local by construction) — the same trust seam `signal`'s
/// `evidenceOut` and `approve`'s `node` already rely on.
///
/// Milestone 05a follow-up Minor 1: `load_and_publish` used to run unconditionally before the
/// idempotency pre-flight, so a recognized retry re-read, re-linted and re-published the graph
/// file for nothing (its result was simply discarded once the pre-flight then classified the
/// command `Complete`). The body is now parsed once, up front — parsing bytes already in memory
/// touches neither the filesystem nor the store, so doing it before `parse_mutation_headers` (which
/// needs the parsed `Value` to compute the request digest, see `request_digest16`) does not weaken
/// "headers validated before anything touches the store" — and `load_and_publish` itself moves
/// *inside* the closure `run_idempotent_mutation` only invokes on the `Absent` (genuinely fresh)
/// path, via the `MutationError::Prepared` arm.
///
/// `run_idempotent_mutation`'s closure parameter returns `Result<_, MutationError>`, whose
/// `Prepared(Response)` variant is `clippy::result_large_err`-sized — the same accepted-not-
/// worked-around tradeoff `parse_mutation_headers`/`load_and_publish`/`request_digest16` already
/// document in `serve/mod.rs`. Every mutation handler below carries the same allow for the same
/// reason; noted once here rather than repeated in full at each of the other five.
#[allow(clippy::result_large_err)]
pub(super) async fn start(
    State(state): State<ServeState>,
    UrlPath(execution_id): UrlPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return bad_request(START_COMMAND, "the request body is not valid JSON", "/"),
    };
    let identity = match parse_mutation_headers(
        &headers,
        START_COMMAND,
        &execution_id,
        &payload,
        &["started"],
    ) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let Some(file) = payload.get("file").and_then(serde_json::Value::as_str) else {
        return bad_request(
            START_COMMAND,
            "the request body must carry \"file\"",
            "/file",
        );
    };
    let file = PathBuf::from(file);
    let fixtures = payload
        .get("fixtures")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from);
    let Some(mode) = payload.get("mode").and_then(serde_json::Value::as_str) else {
        return bad_request(
            START_COMMAND,
            "the request body must carry \"mode\"",
            "/mode",
        );
    };
    let mode = mode.to_owned();

    run_idempotent_mutation(
        &state.events,
        &execution_id,
        START_COMMAND,
        identity,
        |actor, key| {
            let version =
                load_and_publish(&file, START_COMMAND).map_err(MutationError::Prepared)?;
            Ok(execution::start::execute(
                &version,
                &state.events,
                fixtures.as_deref(),
                &mode,
                Some(execution_id.as_str()),
                actor,
                key,
            )?)
        },
    )
}

/// `POST /v1/executions/{id}/signal`: the CLI's `--signal <file>` becomes an inline JSON body field
/// (`"signal": {...}`), and `--evidence-out <path>` becomes `"evidenceOut": "<path>"` — the same
/// operator-supplied path string, used exactly as the CLI would (D-040's premise: the server is
/// local by construction). Re-serializing the inline value back to bytes and handing those to
/// `execution::signal::execute` — which still does its own `serde_json::from_slice` and admission
/// logic entirely unchanged — is the split the plan asks for: the file *read* moved to the CLI side
/// of the seam (`signal::run_from_file`), and this is the API side passing the JSON straight
/// through.
#[allow(clippy::result_large_err)] // see `start`'s doc comment
pub(super) async fn signal(
    State(state): State<ServeState>,
    UrlPath(execution_id): UrlPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return bad_request(SIGNAL_COMMAND, "the request body is not valid JSON", "/"),
    };
    let identity = match parse_mutation_headers(
        &headers,
        SIGNAL_COMMAND,
        &execution_id,
        &payload,
        &["record"],
    ) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let Some(signal_value) = payload.get("signal") else {
        return bad_request(
            SIGNAL_COMMAND,
            "the request body must carry \"signal\"",
            "/signal",
        );
    };
    let signal_bytes = match serde_json::to_vec(signal_value) {
        Ok(bytes) => bytes,
        Err(_) => {
            return bad_request(
                SIGNAL_COMMAND,
                "\"signal\" could not be serialized",
                "/signal",
            );
        }
    };
    let Some(evidence_out) = payload
        .get("evidenceOut")
        .and_then(serde_json::Value::as_str)
    else {
        return bad_request(
            SIGNAL_COMMAND,
            "the request body must carry \"evidenceOut\"",
            "/evidenceOut",
        );
    };
    let evidence_out = PathBuf::from(evidence_out);

    run_idempotent_mutation(
        &state.events,
        &execution_id,
        SIGNAL_COMMAND,
        identity,
        |actor, key| {
            Ok(execution::signal::execute(
                &state.events,
                Some(execution_id.as_str()),
                &signal_bytes,
                &evidence_out,
                actor,
                key,
            )?)
        },
    )
}

/// `POST /v1/executions/{id}/approve`: body `{"node": "<name>"}`, mirroring the CLI's `--node`.
#[allow(clippy::result_large_err)] // see `start`'s doc comment
pub(super) async fn approve(
    State(state): State<ServeState>,
    UrlPath(execution_id): UrlPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return bad_request(APPROVE_COMMAND, "the request body is not valid JSON", "/"),
    };
    let identity = match parse_mutation_headers(
        &headers,
        APPROVE_COMMAND,
        &execution_id,
        &payload,
        &["outcome"],
    ) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let Some(node) = payload.get("node").and_then(serde_json::Value::as_str) else {
        return bad_request(
            APPROVE_COMMAND,
            "the request body must carry \"node\"",
            "/node",
        );
    };
    let node = node.to_owned();

    run_idempotent_mutation(
        &state.events,
        &execution_id,
        APPROVE_COMMAND,
        identity,
        |actor, key| {
            Ok(execution::approve::execute(
                &state.events,
                Some(execution_id.as_str()),
                &node,
                actor,
                key,
            )?)
        },
    )
}

/// `POST /v1/executions/{id}/pause`: no request body — mirrors the CLI's `execution pause`, which
/// takes only `--events`/`--execution`. `pause` takes no meaningful parameters beyond the URL's own
/// execution id, so a fixed `Value::Null` stands in for "no body" in the request digest
/// (`parse_mutation_headers`): there is no field here that could make two `pause` calls under the
/// same `Idempotency-Key` logically different requests, so this command's derived key never varies
/// with whatever bytes, if any, a caller happens to send.
#[allow(clippy::result_large_err)] // see `start`'s doc comment
pub(super) async fn pause(
    State(state): State<ServeState>,
    UrlPath(execution_id): UrlPath<String>,
    headers: HeaderMap,
) -> Response {
    let identity = match parse_mutation_headers(
        &headers,
        PAUSE_COMMAND,
        &execution_id,
        &serde_json::Value::Null,
        &["paused"],
    ) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    run_idempotent_mutation(
        &state.events,
        &execution_id,
        PAUSE_COMMAND,
        identity,
        |actor, key| {
            Ok(execution::pause::execute(
                &state.events,
                Some(execution_id.as_str()),
                actor,
                key,
            )?)
        },
    )
}

/// `POST /v1/executions/{id}/resume`: body `{"file": "<path>", "fixtures": "<path, omitted for
/// none>"}`, mirroring the CLI's `--file`/`--fixtures` — the same trust seam `start` already
/// documents, including the same Minor 1 reordering: `load_and_publish` runs inside the closure,
/// only on the `Absent` (genuinely fresh) path — see `start`'s own doc comment for the reasoning.
#[allow(clippy::result_large_err)] // see `start`'s doc comment
pub(super) async fn resume(
    State(state): State<ServeState>,
    UrlPath(execution_id): UrlPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return bad_request(RESUME_COMMAND, "the request body is not valid JSON", "/"),
    };
    let identity = match parse_mutation_headers(
        &headers,
        RESUME_COMMAND,
        &execution_id,
        &payload,
        &["resumed"],
    ) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let Some(file) = payload.get("file").and_then(serde_json::Value::as_str) else {
        return bad_request(
            RESUME_COMMAND,
            "the request body must carry \"file\"",
            "/file",
        );
    };
    let file = PathBuf::from(file);
    let fixtures = payload
        .get("fixtures")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from);

    run_idempotent_mutation(
        &state.events,
        &execution_id,
        RESUME_COMMAND,
        identity,
        |actor, key| {
            let version =
                load_and_publish(&file, RESUME_COMMAND).map_err(MutationError::Prepared)?;
            Ok(execution::resume::execute(
                &version,
                &state.events,
                fixtures.as_deref(),
                Some(execution_id.as_str()),
                actor,
                key,
            )?)
        },
    )
}

/// `POST /v1/executions/{id}/cancel`: no request body — mirrors the CLI's `execution cancel`, which
/// takes only `--events`/`--execution`. As with `pause`, a fixed `Value::Null` stands in for "no
/// body" in the request digest — see `pause`'s own doc comment.
#[allow(clippy::result_large_err)] // see `start`'s doc comment
pub(super) async fn cancel(
    State(state): State<ServeState>,
    UrlPath(execution_id): UrlPath<String>,
    headers: HeaderMap,
) -> Response {
    let identity = match parse_mutation_headers(
        &headers,
        CANCEL_COMMAND,
        &execution_id,
        &serde_json::Value::Null,
        &["cancelled"],
    ) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    run_idempotent_mutation(
        &state.events,
        &execution_id,
        CANCEL_COMMAND,
        identity,
        |actor, key| {
            Ok(execution::cancel::execute(
                &state.events,
                Some(execution_id.as_str()),
                actor,
                key,
            )?)
        },
    )
}
