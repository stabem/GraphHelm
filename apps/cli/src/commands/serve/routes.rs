//! The Public Runtime API's endpoint handlers (Milestone 05a). Every handler delegates to the same
//! `commands::execution` command layer the CLI uses — D-039's "never a second path" rule as code:
//! the store's own resolve/replay read is the only read path, and a mutation's own `execute()` is
//! the only write path, whether reached from a terminal or from HTTP.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path as UrlPath, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use graphhelm_graph::GraphVersion;
use graphhelm_protocols::{ActorId, Diagnostic, PersistedActor, PersistedActorType};
use graphhelm_runtime::driver::{StoreOpen, drive_to_quiescence_async};
use graphhelm_runtime::executor::{AsyncNodeExecutor, PortExecutor};
use graphhelm_runtime::fixture::FixtureAsyncExecutor;
use graphhelm_simulation::FixtureExecutor;
use graphhelm_tool_broker::lease::{Capability, ToolLease};

use super::ports::{ServeModelPort, ServeToolPort, build_sealer};
use super::{
    MutationError, ServeState, parse_mutation_headers, respond, respond_failure,
    run_idempotent_mutation,
};
use crate::commands::execution::PreparedDrive;
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
    // Cloned ahead of the closure below — `state` is cheap to clone (every field is
    // `Arc`-backed) and `execution_id` is a plain owned `String` — so `async move` can take
    // ownership of its own copies while the outer call still borrows the originals directly.
    let drive_state = state.clone();
    let drive_execution_id = execution_id.clone();

    run_idempotent_mutation(
        &state.events,
        &execution_id,
        START_COMMAND,
        identity,
        |actor, key| {
            Box::pin(async move {
                let version =
                    load_and_publish(&file, START_COMMAND).map_err(MutationError::Prepared)?;
                // Reported deviation from the literal STEP 4 wording (see `drive_is_viable_for`'s
                // own doc comment): the async drive is used whenever it CAN run this graph — a
                // real executor is configured, or every node type classifies as Cognitive/Tool —
                // and the unchanged sync `execute` is kept otherwise, so a real example graph like
                // `examples/graphs/manual-override-deploy.yaml` (which carries a `deploy` node
                // `core/runtime`'s `build_work` refuses regardless of executor) still completes
                // exactly as the 05a fixture-only server always drove it.
                if drive_is_viable_for(&drive_state, &version.graph().spec) {
                    let prepared = execution::start::execute_prepared(
                        &version,
                        &drive_state.events,
                        fixtures.as_deref(),
                        &mode,
                        Some(drive_execution_id.as_str()),
                        actor,
                        key,
                    )?;
                    drive(&drive_state, &drive_execution_id, prepared, &payload).await
                } else {
                    Ok(execution::start::execute(
                        &version,
                        &drive_state.events,
                        fixtures.as_deref(),
                        &mode,
                        Some(drive_execution_id.as_str()),
                        actor,
                        key,
                    )?)
                }
            })
        },
    )
    .await
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
    // Milestone 05d Task 9 STEP 5: flips the Task 6 declared discrepancy — when the server was
    // launched with a keyring (`state.sealing`), the signal route now seals through it, exactly
    // as the CLI's own `execution signal --keyring` does; absent a keyring, the API seam keeps
    // its 05a behavior (operator file only, no seal). Cloned ahead of the closure (cheap: `Arc`s
    // and an owned `String`) so `async move` owns its copies while the outer call still borrows
    // `state.events`/`execution_id` directly.
    let sealing = state.sealing.clone();
    let events = state.events.clone();
    let drive_execution_id = execution_id.clone();

    run_idempotent_mutation(
        &state.events,
        &execution_id,
        SIGNAL_COMMAND,
        identity,
        |actor, key| {
            Box::pin(async move {
                Ok(execution::signal::execute(
                    &events,
                    Some(drive_execution_id.as_str()),
                    &signal_bytes,
                    &evidence_out,
                    actor,
                    key,
                    sealing.as_deref(),
                )?)
            })
        },
    )
    .await
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
    let events = state.events.clone();
    let drive_execution_id = execution_id.clone();

    run_idempotent_mutation(
        &state.events,
        &execution_id,
        APPROVE_COMMAND,
        identity,
        |actor, key| {
            Box::pin(async move {
                Ok(execution::approve::execute(
                    &events,
                    Some(drive_execution_id.as_str()),
                    &node,
                    actor,
                    key,
                )?)
            })
        },
    )
    .await
}

/// `POST /v1/executions/{id}/pause`: no request body — mirrors the CLI's `execution pause`, which
/// takes only `--events`/`--execution`. `pause` takes no meaningful parameters beyond the URL's own
/// execution id, so a fixed `Value::Null` stands in for "no body" in the request digest
/// (`parse_mutation_headers`): there is no field here that could make two `pause` calls under the
/// same `Idempotency-Key` logically different requests, so this command's derived key never varies
/// with whatever bytes, if any, a caller happens to send.
///
/// Milestone 05d Task 9 STEP 5: an optional body `{"mode": "immediate"}` — absent, or any other
/// value, keeps the graceful behavior above byte-identical. `"immediate"` looks the execution up
/// in `state.cancels`: a live async drive gets `send(true)` on its cancel channel, then this
/// handler polls `execution::status::execute` (the same read `GET /v1/executions/{id}` uses)
/// every 100ms for up to 10s until `execution_paused` has folded, and replies with that status.
/// The route itself appends NOTHING in immediate mode — `drive_to_quiescence_async` appends
/// `execution_paused` itself once every in-flight node is recorded `Interrupted` — so a caller's
/// `Idempotency-Key` is still validated (header shape only) but never turned into an event here.
/// No live sender (an idle execution, or a fixture drive with nothing in flight) falls through to
/// the existing graceful `execute` below, the 04f pause.
#[allow(clippy::result_large_err)] // see `start`'s doc comment
pub(super) async fn pause(
    State(state): State<ServeState>,
    UrlPath(execution_id): UrlPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let payload: serde_json::Value = if body.is_empty() {
        serde_json::Value::Null
    } else {
        match serde_json::from_slice(&body) {
            Ok(value) => value,
            Err(_) => return bad_request(PAUSE_COMMAND, "the request body is not valid JSON", "/"),
        }
    };
    let immediate = payload.get("mode").and_then(serde_json::Value::as_str) == Some("immediate");

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

    if immediate {
        let sender = {
            let cancels = state.cancels.lock().await;
            cancels.get(&execution_id).cloned()
        };
        if let Some(sender) = sender {
            let _ = sender.send(true);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                if let Ok(value) = execution::status::execute(&state.events, Some(&execution_id))
                    && value.get("status") == Some(&serde_json::json!("paused"))
                {
                    return respond(
                        StatusCode::OK,
                        Outcome::success(PAUSE_COMMAND, value).output,
                    );
                }
                if std::time::Instant::now() >= deadline {
                    return respond_failure(
                        PAUSE_COMMAND,
                        driver_failure(
                            "the execution did not record execution_paused within the immediate-stop budget",
                        ),
                    );
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }

    let events = state.events.clone();
    let drive_execution_id = execution_id.clone();
    run_idempotent_mutation(
        &state.events,
        &execution_id,
        PAUSE_COMMAND,
        identity,
        |actor, key| {
            Box::pin(async move {
                Ok(execution::pause::execute(
                    &events,
                    Some(drive_execution_id.as_str()),
                    actor,
                    key,
                )?)
            })
        },
    )
    .await
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
    let drive_state = state.clone();
    let drive_execution_id = execution_id.clone();

    run_idempotent_mutation(
        &state.events,
        &execution_id,
        RESUME_COMMAND,
        identity,
        |actor, key| {
            Box::pin(async move {
                let version =
                    load_and_publish(&file, RESUME_COMMAND).map_err(MutationError::Prepared)?;
                // See `start`'s matching branch for why the async drive is conditional.
                if drive_is_viable_for(&drive_state, &version.graph().spec) {
                    let prepared = execution::resume::execute_prepared(
                        &version,
                        &drive_state.events,
                        fixtures.as_deref(),
                        Some(drive_execution_id.as_str()),
                        actor,
                        key,
                    )?;
                    drive(&drive_state, &drive_execution_id, prepared, &payload).await
                } else {
                    Ok(execution::resume::execute(
                        &version,
                        &drive_state.events,
                        fixtures.as_deref(),
                        Some(drive_execution_id.as_str()),
                        actor,
                        key,
                    )?)
                }
            })
        },
    )
    .await
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
    let events = state.events.clone();
    let drive_execution_id = execution_id.clone();
    run_idempotent_mutation(
        &state.events,
        &execution_id,
        CANCEL_COMMAND,
        identity,
        |actor, key| {
            Box::pin(async move {
                Ok(execution::cancel::execute(
                    &events,
                    Some(drive_execution_id.as_str()),
                    actor,
                    key,
                )?)
            })
        },
    )
    .await
}

// -------------------------------------------------------------------------------------------
// Milestone 05d Task 9 STEP 4: the async drive half `start`/`resume` share, once their own
// decision event has already committed via `execute_prepared`.
// -------------------------------------------------------------------------------------------

/// Whether `drive` (the async driver) can run `spec` at all: a real executor is configured, or
/// every node type in the graph classifies as Cognitive/Tool (`graphhelm_runtime::classify`).
///
/// Reported deviation from the task's literal STEP 4 wording ("None → `FixtureAsyncExecutor`
/// unconditionally"): `drive_to_quiescence_async`'s own `build_work` (`core/runtime/src/driver.rs`,
/// already-done Task 8 code, out of this task's scope to redesign) refuses to dispatch ANY node
/// whose type is not Agent/Planner/Classifier/Evaluator/Tool — regardless of which
/// `AsyncNodeExecutor` is wired, fixture or real. A `FixtureExecutor` (the CLI's own sync
/// `NodeExecutor`) has no such restriction: it answers by node id alone, so it has always been
/// able to drive graphs like `examples/graphs/manual-override-deploy.yaml` (a `deploy` node) that
/// `api_http.rs`'s existing suite depends on completing. Routing every fixture-only `start`/
/// `resume` through the async driver unconditionally, as first attempted, broke that suite
/// outright (observed directly: three tests failed with the drive stalling at the first
/// unsupported node, `SimulationStatus` staying `null`/`running` forever — see this task's final
/// report for the full trace). This predicate keeps the async drive for graphs it can actually
/// finish (letting a fixture story exercise the real async path — STEP 6 test 1 — and every
/// real-executor story use it, per the design) while falling back to the unchanged sync `execute`
/// for anything else, which is what keeps the existing suite green.
fn drive_is_viable_for(state: &ServeState, spec: &graphhelm_protocols::GraphSpec) -> bool {
    state.runtime.is_some()
        || spec
            .nodes
            .values()
            .all(|node| graphhelm_runtime::classify::work_kind(&node.node_type).is_ok())
}

/// A driver-side failure (a `DriverError` from `drive_to_quiescence_async`) mapped onto the
/// existing `execution::Failure` surface: a stable code that is not one of `respond_failure`'s
/// specifically-classified codes, so it falls through to that function's 500 default — "an app
/// failure with a stable code", never the driver's own internal error text.
const DRIVER_FAILURE_CODE: &str = "GHCLI016_DRIVER_FAILURE";

fn driver_failure(message: &str) -> execution::Failure {
    execution::Failure {
        code: DRIVER_FAILURE_CODE,
        message: message.to_owned(),
        pointer: "/execution".to_owned(),
    }
}

/// The `PersistedActor` every async drive's own bookkeeping is attributed to — the async mirror
/// of the CLI's `system_actor()`, distinct so the log can still tell "an HTTP-driven story's own
/// hops" from a CLI-driven one if that ever matters, though today both fold to the same `System`
/// actor type.
fn runtime_actor() -> PersistedActor {
    PersistedActor::new(
        PersistedActorType::System,
        ActorId::parse("system-runtime").expect("constant actor id is valid"),
    )
}

/// Runs the async drive for a `PreparedDrive` the decision half (`execute_prepared`) already
/// committed: builds the sealer/executor from `state`, registers a cancel channel under
/// `execution_id` (STEP 5's `pause {"mode":"immediate"}` needs it live), drives to quiescence,
/// deregisters, and renders the projection into the SAME reply shape `execution::render` (the
/// sync path) produces — `execution::start`/`execution::resume`'s own `execute` calls the exact
/// same `render`, so the two paths cannot drift.
///
/// `payload` is the request's own JSON body: an optional `"project"` field names the directory
/// `ServeToolPort` is built against (FIXED decision — defaults to the server process's current
/// working directory when absent, since the request shape carries no other signal and the tool
/// host needs a project directory to root Tier 0 reads and Tier 1 worktrees against).
async fn drive(
    state: &ServeState,
    execution_id: &str,
    prepared: PreparedDrive,
    payload: &serde_json::Value,
) -> Result<serde_json::Value, MutationError> {
    let sealer = build_sealer(state.sealing.as_deref())
        .map_err(|message| MutationError::from(driver_failure(&message)))?;
    let ids = Arc::new(crate::commands::UuidIds);
    let events = state.events.clone();
    let store_open: StoreOpen = Arc::new(move || event_store(&events));

    let executor: Arc<dyn AsyncNodeExecutor> = match &state.runtime {
        Some(wiring) => {
            let model = ServeModelPort::build(wiring)
                .await
                .map_err(|message| MutationError::from(driver_failure(&message)))?;
            let project = payload
                .get("project")
                .and_then(serde_json::Value::as_str)
                .map(PathBuf::from)
                .or_else(|| std::env::current_dir().ok())
                .ok_or_else(|| {
                    MutationError::from(driver_failure(
                        "no \"project\" was given and the server's working directory could not be read",
                    ))
                })?;
            let tools = ServeToolPort::build(wiring, &project)
                .map_err(|message| MutationError::from(driver_failure(&message)))?;
            let lease = ToolLease {
                actor: "runtime".to_owned(),
                capabilities: [
                    Capability::RepositoryRead,
                    Capability::RepositoryWrite,
                    Capability::ShellExecute,
                    Capability::TestsExecute,
                ]
                .into_iter()
                .collect(),
                programs: wiring.allow_programs.iter().cloned().collect(),
            };
            Arc::new(PortExecutor {
                model: Arc::new(model),
                tools: Arc::new(tools),
                route_id: wiring.route.id().to_owned(),
                lease,
                actor: "runtime".to_owned(),
            })
        }
        None => {
            let fixtures = FixtureExecutor::new(prepared.fixtures.clone());
            Arc::new(FixtureAsyncExecutor::new(fixtures))
        }
    };

    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    state
        .cancels
        .lock()
        .await
        .insert(execution_id.to_owned(), cancel_tx);

    let result = drive_to_quiescence_async(
        store_open,
        sealer,
        ids,
        prepared.scope,
        prepared.stream,
        prepared.execution_id,
        prepared.spec,
        executor,
        runtime_actor(),
        cancel_rx,
    )
    .await;

    state.cancels.lock().await.remove(execution_id);

    match result {
        Ok(projection) => Ok(execution::render(&projection)),
        Err(error) => Err(MutationError::from(driver_failure(&error.to_string()))),
    }
}

// -------------------------------------------------------------------------------------------
// Milestone 05e Task 4: the gateway read surface — `GET /v1/gateway/routes` and
// `GET /v1/gateway/probe`, each delegating to the SAME `commands::gateway` functions the CLI
// subcommands run (D-039's "never a second path" rule applied to the gateway): the handlers
// below own only query parsing and the manifest default; listing and probing stay one code
// path whether reached from a terminal or from HTTP.
// -------------------------------------------------------------------------------------------

const GATEWAY_ROUTES_COMMAND: &str = "gateway.routes";
const GATEWAY_PROBE_COMMAND: &str = "gateway.probe";

/// The Task 0 reconciled decision: a server launched with `--manifest` serves that manifest
/// by default and treats the `manifest` query param as an explicit override; a fixture-only
/// server (no `--manifest`) requires the query param or answers 400 naming it.
fn effective_manifest(state: &ServeState, query_manifest: Option<PathBuf>) -> Option<PathBuf> {
    query_manifest.or_else(|| {
        state
            .runtime
            .as_ref()
            .map(|wiring| wiring.manifest_path.clone())
    })
}

/// Query values ride verbatim (no percent-decoding): every input here is a filesystem path or
/// an identifier the operator chose, mirroring the CLI flags they stand in for. A value that
/// genuinely needs `&`/`=` cannot be expressed — the CLI flag form remains for those.
fn query_pairs(query: &str) -> impl Iterator<Item = (&str, &str)> {
    query
        .split('&')
        .filter(|segment| !segment.is_empty())
        .map(|pair| pair.split_once('=').unwrap_or((pair, "")))
}

/// Maps a command-layer `Outcome` onto the HTTP surface: the envelope rides unchanged (the
/// parity rule — the body IS the CLI's own printed envelope), only the transport's status
/// code is derived. Domain and application refusals are the caller's fault here (every input
/// is a query param), internal failures are ours.
fn respond_outcome(outcome: Outcome) -> Response {
    let status = match outcome.exit_code {
        0 => StatusCode::OK,
        2 | 3 => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    respond(status, outcome.output)
}

/// `GET /v1/gateway/routes[?manifest=<path>]`: the CLI's `gateway routes` listing over HTTP.
pub(super) async fn gateway_routes(
    State(state): State<ServeState>,
    RawQuery(query): RawQuery,
) -> Response {
    let query = query.unwrap_or_default();
    let manifest = query_pairs(&query)
        .find(|(key, _)| *key == "manifest")
        .map(|(_, value)| PathBuf::from(value));
    let Some(manifest) = effective_manifest(&state, manifest) else {
        return bad_request(
            GATEWAY_ROUTES_COMMAND,
            "this server has no configured manifest: pass the manifest query parameter",
            "/manifest",
        );
    };
    // The command layer is synchronous file work; `spawn_blocking` keeps it off the
    // reactor. A join error only happens on panic/cancellation — reported as internal.
    match tokio::task::spawn_blocking(move || crate::commands::gateway::routes::run(&manifest))
        .await
    {
        Ok(outcome) => respond_outcome(outcome),
        Err(_) => respond(
            StatusCode::INTERNAL_SERVER_ERROR,
            Outcome::internal(GATEWAY_ROUTES_COMMAND, "the listing task failed").output,
        ),
    }
}

/// `GET /v1/gateway/probe?route=<id>[&manifest=<path>&broker=<dir>&keyring=<dir>&keyId=<id>]`:
/// the CLI's quota-free `gateway probe` over HTTP. The broker/keyring/keyId params default to
/// the server's own wiring when present, mirroring the manifest rule; the passphrase STAYS an
/// environment variable of the serve process (`GRAPHHELM_GATEWAY_KEY`), never a query param.
pub(super) async fn gateway_probe(
    State(state): State<ServeState>,
    RawQuery(query): RawQuery,
) -> Response {
    let query = query.unwrap_or_default();
    let mut manifest: Option<PathBuf> = None;
    let mut route: Option<String> = None;
    let mut broker: Option<PathBuf> = None;
    let mut keyring: Option<PathBuf> = None;
    let mut key_id: Option<String> = None;
    for (key, value) in query_pairs(&query) {
        match key {
            "manifest" => manifest = Some(PathBuf::from(value)),
            "route" => route = Some(value.to_owned()),
            "broker" => broker = Some(PathBuf::from(value)),
            "keyring" => keyring = Some(PathBuf::from(value)),
            "keyId" => key_id = Some(value.to_owned()),
            _ => {}
        }
    }
    let Some(manifest) = effective_manifest(&state, manifest) else {
        return bad_request(
            GATEWAY_PROBE_COMMAND,
            "this server has no configured manifest: pass the manifest query parameter",
            "/manifest",
        );
    };
    let Some(route) = route else {
        return bad_request(
            GATEWAY_PROBE_COMMAND,
            "the route query parameter is required",
            "/route",
        );
    };
    if let Some(wiring) = state.runtime.as_ref() {
        broker = broker.or_else(|| Some(wiring.broker_dir.clone()));
        keyring = keyring.or_else(|| Some(wiring.keyring_dir.clone()));
        key_id = key_id.or_else(|| Some(wiring.key_id.clone()));
    }
    match tokio::task::spawn_blocking(move || {
        crate::commands::gateway::probe::run(
            &manifest,
            &route,
            broker.as_deref(),
            keyring.as_deref(),
            key_id.as_deref(),
        )
    })
    .await
    {
        Ok(outcome) => respond_outcome(outcome),
        Err(_) => respond(
            StatusCode::INTERNAL_SERVER_ERROR,
            Outcome::internal(GATEWAY_PROBE_COMMAND, "the probe task failed").output,
        ),
    }
}
