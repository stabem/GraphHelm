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
use graphhelm_events::{EventRepositoryError, EvidenceRead, EvidenceSealer};
use graphhelm_graph::GraphVersion;
use graphhelm_protocols::{ActorId, Diagnostic, EvidenceId, PersistedActor, PersistedActorType};
use graphhelm_runtime::driver::{StoreOpen, drive_to_quiescence_async};
use graphhelm_runtime::executor::{AsyncNodeExecutor, PortExecutor};
use graphhelm_runtime::fixture::FixtureAsyncExecutor;
use graphhelm_simulation::FixtureExecutor;
use graphhelm_tool_broker::lease::{Capability, ToolLease};

use graphhelm_gateway::manifest::{ModelRoute, RouteManifest};

use super::ports::{
    RuntimeWiring, ServeModelPort, ServeToolPort, build_opener, build_sealer, find_route,
};
use super::{
    ExecutorWiring, MutationError, ServeState, parse_mutation_headers, respond, respond_failure,
    run_idempotent_mutation,
};
use crate::commands::execution::PreparedDrive;
use crate::commands::{event_store, execution, owner, publish_loaded, topology};
use crate::output::Outcome;

const LIST_COMMAND: &str = "execution.list";
const TOPOLOGY_COMMAND: &str = "graph.topology";
const STATUS_COMMAND: &str = "execution.status";
const EVENTS_COMMAND: &str = "execution.events";
const START_COMMAND: &str = "execution.start";
const SIGNAL_COMMAND: &str = "execution.signal";
const APPROVE_COMMAND: &str = "execution.approve";
const AMEND_BUDGET_COMMAND: &str = "execution.amend_budget";
const PAUSE_COMMAND: &str = "execution.pause";
const RESUME_COMMAND: &str = "execution.resume";
const EVIDENCE_COMMAND: &str = "execution.evidence";
const CANCEL_COMMAND: &str = "execution.cancel";
const SWEEP_COMMAND: &str = "execution.sweep";
const SOURCE: &str = "serve-cli";

/// The events tail's default page size when `limit` is absent.
const DEFAULT_EVENTS_LIMIT: usize = 100;
/// The largest page `limit` may request. A larger request is refused with 400 rather than silently
/// truncated, per the plan: the caller must always be able to tell "there is more" from the
/// response shape, not guess it from getting back fewer events than asked for.
const MAX_EVENTS_LIMIT: usize = 1000;

/// `POST /v1/graph/topology` with `{"file": "..."}`: the graph document's shape and its semantic
/// hash, replying `graph.topology`'s own `data` - the exact same `topology::execute` the CLI's
/// `graph topology` runs.
///
/// A READ THAT APPENDS NOTHING, which is why it is a POST rather than a GET: the graph is
/// addressed by a filesystem path, and a path does not belong in a URL. It would land in request
/// logs, in a browser's history and in any `Referer` the page sends onward - a directory layout
/// leaked through the one field this Runtime cannot redact. The body keeps it out of all three.
///
/// It grants no reach `start` and `resume` did not already have: both take a `file` and load it,
/// so the ability to ask this Runtime to read a graph file by path already existed. This is the
/// same load with a read-only result.
pub(super) async fn graph_topology(body: Bytes) -> Response {
    let payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return bad_request(TOPOLOGY_COMMAND, "the request body is not valid JSON", "/");
        }
    };
    let Some(file) = payload.get("file").and_then(serde_json::Value::as_str) else {
        return bad_request(
            TOPOLOGY_COMMAND,
            "the request body must carry \"file\"",
            "/file",
        );
    };

    match topology::execute(Path::new(file)) {
        Ok(value) => respond(
            StatusCode::OK,
            Outcome::success(TOPOLOGY_COMMAND, value).output,
        ),
        // A file that is not a schema-valid graph is the CALLER's mistake, so 400 and the
        // loader's own diagnostics - the same body the CLI prints for the same file, EXCEPT the
        // `source`: the loader stamps it with the absolute filesystem path, which over HTTP is a
        // directory-layout disclosure (and with --read-audit on, one that lands on disk in the
        // audit line - PR #467 review). The doc comment above spends a whole paragraph keeping
        // this path out of URLs; letting it back in through diagnostics would undo that.
        Err(topology::Failure::Invalid(mut diagnostics)) => {
            for diagnostic in &mut diagnostics {
                diagnostic.source = "graph-file".to_owned();
            }
            respond(
                StatusCode::BAD_REQUEST,
                Outcome::domain(TOPOLOGY_COMMAND, diagnostics).output,
            )
        }
        Err(topology::Failure::Internal(message)) => respond(
            StatusCode::INTERNAL_SERVER_ERROR,
            Outcome::internal(TOPOLOGY_COMMAND, message).output,
        ),
    }
}

/// `GET /v1/executions?after=<executionId>&limit=N`: the execution index, replying
/// `execution.list`'s own `data` - the exact same `execution::list::execute` the CLI's
/// `execution list` runs, so the two report byte-identical output for the same store.
///
/// THIS ROUTE EXISTS SO NOTHING HAS TO SCRAPE `/monitor`. That page is HTML for a human browser
/// (D-040 keeps it read-only and simple); a client that parsed its anchors to discover execution
/// ids was depending on markup, not on a contract. Bounds and cursor semantics live in
/// `execution::list`, not here - this handler only parses the query and relays the refusal.
pub(super) async fn list_executions(
    State(state): State<ServeState>,
    RawQuery(query): RawQuery,
) -> Response {
    let (after, limit) = match parse_list_query(query.as_deref().unwrap_or("")) {
        Ok(parsed) => parsed,
        Err((message, pointer)) => return bad_request(LIST_COMMAND, message, pointer),
    };
    match execution::list::execute(&state.events, after.as_deref(), limit) {
        Ok(value) => respond(StatusCode::OK, Outcome::success(LIST_COMMAND, value).output),
        Err(failure) => respond_failure(LIST_COMMAND, failure),
    }
}

/// Hand-parsed for the same reason `parse_events_query` is: a malformed value must still reply
/// the standard four-key envelope rather than axum's own rejection body. `limit` is only checked
/// for SHAPE here - the 1..=100 range is `execution::list`'s to enforce, so the CLI and the API
/// refuse the same value with the same diagnostic instead of two independently drifting bounds.
fn parse_list_query(query: &str) -> Result<(Option<String>, usize), (&'static str, &'static str)> {
    let mut after = None;
    let mut limit = execution::list::DEFAULT_LIMIT;
    for pair in query.split('&').filter(|segment| !segment.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        match key {
            "after" => {
                let decoded =
                    percent_decode(value).ok_or(("after is not a valid execution id", "/after"))?;
                if !decoded.is_empty() {
                    after = Some(decoded);
                }
            }
            "limit" => {
                limit = value
                    .parse()
                    .map_err(|_| ("limit must be a positive integer", "/limit"))?;
            }
            _ => {}
        }
    }
    Ok((after, limit))
}

/// The minimal percent-decoder this one query value needs. Refuses a malformed escape rather
/// than dropping it: a cursor silently mangled into a different string would page from the wrong
/// place, and a caller has no way to see that happen.
///
/// `+` decodes to a space, the ordinary form-encoding convention, and that was CHECKED rather
/// than assumed. The tempting objection is that `+` is a legal `OpaqueId` byte (0x2b sits inside
/// the 0x21..=0x2e run `is_opaque_id` accepts), so decoding it would mangle a real cursor. That
/// is true about the id predicate and false about the system: the local event store refuses to
/// open a repository whose stream id carries one. Measured, one command per id --
/// `execution start --execution plusa+b` is refused `GHE005_INTEGRITY_FAILURE`, while
/// `plaina-b` and `dot.a` are accepted. No reachable execution id contains a `+`, so no cursor
/// this endpoint can be handed does either, and departing from the convention here would buy a
/// behaviour nothing can exercise. Named rather than left implicit so the next reader meets the
/// measurement instead of re-deriving the objection.
fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                let hex = value.get(index + 1..index + 3)?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                index += 3;
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

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
/// The label inline graph diagnostics are reported against, where a file-backed graph would carry
/// its path. Not a path, and not shaped like one: it names the only place the caller can look.
const INLINE_GRAPH_SOURCE: &str = "<request body>";

/// Where a start/resume request's graph document came from.
///
/// `File` is the original contract (D-040: the server is local by construction, so an operator's
/// path is used as the CLI would use it). `Inline` is what lets a caller that has no filesystem on
/// the server — a browser — start work: the document travels in the request body as JSON and is
/// parsed from memory, never written to disk on the way in.
enum GraphSource {
    File(PathBuf),
    Inline(serde_json::Value),
}

/// Reads the request body's graph, refusing every shape that could be read two ways.
///
/// BOTH FIELDS PRESENT IS A REFUSAL, not a precedence rule. A caller that sends `"file"` and
/// `"graph"` together holds two different beliefs about which document is about to run, and any
/// precedence this picked would be right for half of them and silently wrong for the other half.
/// The wrong half runs a graph they did not intend, under an execution id they did.
// `Response` in the `Err` arm, the same accepted tradeoff every mutation handler in this
// file documents: a refusal here IS a built response, and boxing it would buy a smaller
// `Result` at the cost of an allocation on the one path that is already returning.
#[allow(clippy::result_large_err)]
fn graph_source(
    payload: &serde_json::Value,
    command: &'static str,
) -> Result<GraphSource, Response> {
    let file = payload.get("file");
    let graph = payload.get("graph");
    let present =
        |value: Option<&serde_json::Value>| !matches!(value, None | Some(serde_json::Value::Null));

    match (present(file), present(graph)) {
        (true, true) => Err(bad_request(
            command,
            "the request body carries both \"file\" and \"graph\"; give exactly one",
            "/graph",
        )),
        (false, false) => Err(bad_request(
            command,
            "the request body must carry \"file\" or \"graph\"",
            "/file",
        )),
        (true, false) => match file {
            Some(serde_json::Value::String(path)) => Ok(GraphSource::File(PathBuf::from(path))),
            _ => Err(bad_request(
                command,
                "\"file\" must be a string naming a graph document",
                "/file",
            )),
        },
        (false, true) => match graph {
            // An object, never a string of JSON. A string would make the caller escape a document
            // inside a document, and would leave this deciding whether to parse the string as YAML
            // or JSON — the ambiguity `load_graph_json` exists to avoid.
            Some(value @ serde_json::Value::Object(_)) => Ok(GraphSource::Inline(value.clone())),
            _ => Err(bad_request(
                command,
                "\"graph\" must be the graph document as a JSON object",
                "/graph",
            )),
        },
    }
}

#[allow(clippy::result_large_err)] // see `graph_source`
fn load_and_publish(source: &GraphSource, command: &'static str) -> Result<GraphVersion, Response> {
    let to_response = |diagnostics| {
        respond(
            StatusCode::BAD_REQUEST,
            Outcome::domain(command, diagnostics).output,
        )
    };
    let loaded = match source {
        GraphSource::File(path) => graphhelm_schema::load_graph(path).map_err(to_response)?,
        GraphSource::Inline(value) => {
            // `to_vec` on a `Value` that was itself parsed from the request body cannot fail, but
            // the bound that matters is applied downstream regardless: `load_graph_json` re-checks
            // the 4 MiB document limit against these bytes.
            let bytes = serde_json::to_vec(value).map_err(|_| {
                respond(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Outcome::internal(command, "the inline graph could not be re-serialized")
                        .output,
                )
            })?;
            graphhelm_schema::load_graph_json(&bytes, INLINE_GRAPH_SOURCE).map_err(to_response)?
        }
    };
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
    let source = match graph_source(&payload, START_COMMAND) {
        Ok(source) => source,
        Err(response) => return response,
    };
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
        ExecutorWiring::from_state(&state),
        |actor, key| {
            Box::pin(async move {
                let version =
                    load_and_publish(&source, START_COMMAND).map_err(MutationError::Prepared)?;
                // Reported deviation from the literal STEP 4 wording (see `drive_is_viable_for`'s
                // own doc comment): the async drive is used whenever it CAN run this graph — a
                // real executor is configured, or every node type classifies as Cognitive/Tool —
                // and the unchanged sync `execute` is kept otherwise, so a real example graph like
                // `examples/graphs/manual-override-deploy.yaml` (which carries a `deploy` node
                // `core/runtime`'s `build_work` refuses regardless of executor) still completes
                // exactly as the 05a fixture-only server always drove it.
                if drive_is_viable_for(&drive_state, &version.graph().spec) {
                    // #83: same ordering as `resume` — the shared shape is where the fix lands, so
                    // `start` cannot commit `ExecutionStarted` for a drive whose setup then refuses.
                    let setup = prepare_drive(&drive_state, START_COMMAND, &payload).await?;
                    let prepared = execution::start::execute_prepared(
                        &version,
                        &drive_state.events,
                        fixtures.as_deref(),
                        &mode,
                        Some(drive_execution_id.as_str()),
                        actor,
                        key,
                    )?;
                    drive(&drive_state, &drive_execution_id, prepared, setup).await
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
    // OPTIONAL, and refused when unusable rather than folded into absent - the same distinction
    // `"route"` needed on start. A caller who sent `"evidenceOut": 7` gets told, instead of having
    // their envelope quietly preserved somewhere else or nowhere.
    //
    // Whether absence is ALLOWED is not decided here: `signal::execute` owns that rule, because it
    // is the function that knows the seal happens before the append. Deciding it twice is how the
    // API and the CLI come to disagree about when an envelope is durable.
    let evidence_out = match payload.get("evidenceOut") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(path)) => Some(PathBuf::from(path)),
        Some(_) => {
            return bad_request(
                SIGNAL_COMMAND,
                "\"evidenceOut\" must be a string naming a path on the Runtime's host",
                "/evidenceOut",
            );
        }
    };
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
        ExecutorWiring::from_state(&state),
        |actor, key| {
            Box::pin(async move {
                // `signal::execute` is synchronous, but the sealing API is async, so it drives the
                // seal on a runtime it builds itself. On a reactor thread that construction is a
                // PANIC, not an error ("Cannot start a runtime from within a runtime", measured
                // 2026-08-28 at `execution/signal.rs:262`), and a panicking handler aborts the
                // connection: the caller sees a truncated HTTP response instead of a refusal, so
                // no diagnostic reaches them and nothing is recorded. `spawn_blocking` hands the
                // command layer a thread with no reactor on it — the same seam
                // `gateway_probe` above already uses for synchronous command work.
                //
                // This is load-bearing only when the server was started with a keyring; without
                // one `sealing` is `None`, the seal never runs, and the panic never fires. That
                // is why the defect outlived every earlier signal test: they all ran unsealed.
                let recorded = tokio::task::spawn_blocking(move || {
                    execution::signal::execute(
                        &events,
                        Some(drive_execution_id.as_str()),
                        &signal_bytes,
                        evidence_out.as_deref(),
                        actor,
                        key,
                        sealing.as_deref(),
                    )
                })
                .await
                .map_err(|_| {
                    MutationError::Prepared(respond(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Outcome::internal(SIGNAL_COMMAND, "the signal task failed").output,
                    ))
                })?;
                Ok(recorded?)
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
        ExecutorWiring::from_state(&state),
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

/// `POST /v1/executions/{id}/amend-budget`: the socket the attention verdict's own remedy
/// plugs into, over HTTP.
///
/// Body mirrors the remedy the caller was handed: `{"node", "seconds", "computedAtSequence"}`.
/// `seconds` is the operator's own decision and has no default here for the same reason it has
/// none in the seam -- a suggested value would turn "absent means unknown" into "absent means
/// 300s" through the back door.
#[allow(clippy::result_large_err)]
pub(super) async fn amend_budget(
    State(state): State<ServeState>,
    UrlPath(execution_id): UrlPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return bad_request(
                AMEND_BUDGET_COMMAND,
                "the request body is not valid JSON",
                "/",
            );
        }
    };
    let identity = match parse_mutation_headers(
        &headers,
        AMEND_BUDGET_COMMAND,
        &execution_id,
        &payload,
        &["outcome"],
    ) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let Some(node) = payload.get("node").and_then(serde_json::Value::as_str) else {
        return bad_request(
            AMEND_BUDGET_COMMAND,
            "the request body must carry \"node\"",
            "/node",
        );
    };
    let Some(seconds) = payload.get("seconds").and_then(serde_json::Value::as_u64) else {
        return bad_request(
            AMEND_BUDGET_COMMAND,
            "the request body must carry \"seconds\": the bound is the operator's to decide",
            "/seconds",
        );
    };
    let Some(at) = payload
        .get("computedAtSequence")
        .and_then(serde_json::Value::as_u64)
    else {
        return bad_request(
            AMEND_BUDGET_COMMAND,
            "the request body must carry \"computedAtSequence\": an amendment the store cannot place is refused, never guessed",
            "/computedAtSequence",
        );
    };
    let node = node.to_owned();
    let events = state.events.clone();
    let target = execution_id.clone();

    run_idempotent_mutation(
        &state.events,
        &execution_id,
        AMEND_BUDGET_COMMAND,
        identity,
        ExecutorWiring::from_state(&state),
        |actor, key| {
            Box::pin(async move {
                Ok(execution::amend::execute(
                    &events,
                    Some(target.as_str()),
                    &node,
                    seconds,
                    at,
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
                    // #130, the third class — found while #96 split the other two, and
                    // deliberately NOT split here. This is not a drive failure at all: it is
                    // `pause` waiting for `execution_paused` and running out of budget, so the
                    // operator's question is "did my pause take effect?" — and its remedy is a
                    // third kind, an UNKNOWN rather than a failure, because the pause may still
                    // record after the budget elapses. A failure says act; an unknown says look.
                    //
                    // NOT contradicted by #96: that change scopes `GHCLI016`'s narrowed meaning to
                    // the start/resume path explicitly, so this site's use stays a pre-existing
                    // approximation rather than becoming a false statement. Untidy and named, not
                    // wrong — a distinction I got wrong in #130's own opening and corrected there
                    // after a reviewer read this constant's doc instead of my summary of it.
                    //
                    // Left as-is because it belongs to a different command and a different
                    // operator story than #96's split, and widening that change to cover it would
                    // bundle two contracts in one diff. Named here so the next reader finds a
                    // decision instead of an oversight.
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
        ExecutorWiring::from_state(&state),
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
    let source = match graph_source(&payload, RESUME_COMMAND) {
        Ok(source) => source,
        Err(response) => return response,
    };
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
        ExecutorWiring::from_state(&state),
        |actor, key| {
            Box::pin(async move {
                let version =
                    load_and_publish(&source, RESUME_COMMAND).map_err(MutationError::Prepared)?;
                // See `start`'s matching branch for why the async drive is conditional.
                if drive_is_viable_for(&drive_state, &version.graph().spec) {
                    // #83: the drive's fallible setup runs FIRST, so the resume decision is the
                    // last thing that can fail rather than the first thing that commits. A setup
                    // refusal now leaves the operator's pause hold exactly where they left it.
                    let setup = prepare_drive(&drive_state, RESUME_COMMAND, &payload).await?;
                    let prepared = execution::resume::execute_prepared(
                        &version,
                        &drive_state.events,
                        fixtures.as_deref(),
                        Some(drive_execution_id.as_str()),
                        actor,
                        key,
                    )?;
                    drive(&drive_state, &drive_execution_id, prepared, setup).await
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
        ExecutorWiring::from_state(&state),
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

/// `POST /v1/executions/{id}/sweep`: evaluate the stream's customs stages and journal the result.
///
/// The body carries an optional `asOf`; absent means now, read from the STORE'S clock rather than
/// this process's. THE FUTURE IS REFUSED by the verb itself, before it reads, computes or writes
/// anything -- deliberately not re-checked here, because a second copy of that rule at the door
/// can drift from the first and makes the verb's own guard unreachable from this surface.
///
/// `asOf` IS IN THE IDEMPOTENCY DIGEST (it is a body field, and `request_digest16` covers the
/// body), so two requests carrying the same key and DIFFERENT instants are `Divergent` rather than
/// silently collapsed onto the first answer. Which is right: they are different questions.
pub(super) async fn sweep(
    State(state): State<ServeState>,
    UrlPath(execution_id): UrlPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // An empty body is a legal sweep -- every field is optional -- so absent bytes read as `{}`
    // rather than as a malformed request. Anything present must still be JSON.
    let payload: serde_json::Value = if body.is_empty() {
        serde_json::Value::Object(serde_json::Map::new())
    } else {
        match serde_json::from_slice(&body) {
            Ok(value) => value,
            Err(_) => return bad_request(SWEEP_COMMAND, "the request body is not valid JSON", "/"),
        }
    };

    let as_of = match payload.get("asOf") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(value)) => Some(value.clone()),
        Some(_) => {
            return bad_request(
                SWEEP_COMMAND,
                "\"asOf\" must be an RFC 3339 UTC timestamp string",
                "/asOf",
            );
        }
    };

    let identity = match parse_mutation_headers(
        &headers,
        SWEEP_COMMAND,
        &execution_id,
        &payload,
        &["sweep-performed"],
    ) {
        Ok(identity) => identity,
        Err(response) => return response,
    };

    let events = state.events.clone();
    let drive_execution_id = execution_id.clone();
    run_idempotent_mutation(
        &state.events,
        &execution_id,
        SWEEP_COMMAND,
        identity,
        ExecutorWiring::from_state(&state),
        |actor, key| {
            Box::pin(async move {
                Ok(execution::sweep::execute(
                    &events,
                    Some(drive_execution_id.as_str()),
                    as_of.as_deref(),
                    actor,
                    Some(key),
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
///
/// #96: this code now means ONE thing on the start/resume path — **the decision committed and the
/// work then failed**. The operator's hold is GONE and the execution is attended; re-pausing
/// blindly is wrong. A setup refusal, where nothing committed, answers [`SETUP_FAILURE_CODE`]
/// instead.
const DRIVER_FAILURE_CODE: &str = "GHCLI016_DRIVER_FAILURE";

/// #96. A refusal from [`prepare_drive`] — raised BEFORE the start/resume decision commits, so
/// **nothing was written and the operator's hold is exactly where they left it.** Fix the
/// environment and retry; you are where you were.
///
/// The distinction this carries is not new logic. #83's hoist already made it structural: every
/// site that raises this runs before `execute_prepared`, and every site that raises
/// [`DRIVER_FAILURE_CODE`] on this path runs after it. Until now both answered the same value, so
/// the response destroyed a distinction the code already had — "the call failed" and "your hold
/// still holds" are one fact to an operator, and one code for both made them two.
const SETUP_FAILURE_CODE: &str = "GHCLI019_DRIVER_SETUP";

fn driver_failure(message: &str) -> execution::Failure {
    execution::Failure {
        code: DRIVER_FAILURE_CODE,
        message: message.to_owned(),
        pointer: "/execution".to_owned(),
    }
}

/// Class (a): the drive's setup refused and no decision was committed. See [`SETUP_FAILURE_CODE`].
fn setup_failure(message: &str) -> execution::Failure {
    execution::Failure {
        code: SETUP_FAILURE_CODE,
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
/// The half of the drive's setup that can FAIL, resolved by [`prepare_drive`] BEFORE the caller
/// commits its decision. Issue #83: `execute_prepared` used to commit `ExecutionResumed` and drop
/// the operator's pause hold, and only then did this setup run — so a setup failure answered
/// `GHCLI016_DRIVER_FAILURE` over a store that had already recorded the resume. "The call failed"
/// and "your hold still holds" are one fact to an operator, and that ordering made them two.
struct DriveSetup {
    sealer: Arc<dyn EvidenceSealer>,
    ports: Option<PreparedPorts>,
}

/// The runtime-backed ports, built once and moved into the executor after the commit.
struct PreparedPorts {
    model: ServeModelPort,
    tools: ServeToolPort,
    route_id: String,
    lease: ToolLease,
}

/// Which model this drive runs on: the request's `"route"` when it names one, the deployer's
/// `--route` default otherwise.
///
/// THE MANIFEST IS RE-READ, not answered from the `ModelRoute` cloned at startup. That is what
/// `RuntimeWiring::manifest_path` is kept for, and it is what `GET /v1/gateway/routes` already
/// does — so the listing a caller picks from and the resolution of what they picked read the same
/// file. Answering from a startup snapshot would let the Studio offer a route this then refuses,
/// or accept one the listing no longer shows, and the caller could not tell which of the two
/// surfaces was lying.
///
/// A PRESENT-BUT-UNUSABLE `"route"` IS REFUSED, never quietly ignored. The tempting spelling is
/// `payload.get("route").and_then(Value::as_str)`, which folds "absent" and "present but not a
/// string" into the same `None` and runs the deployer's default. The operator asked for a
/// specific model; spending a different one and reporting success is the one outcome this cannot
/// produce.
#[allow(clippy::result_large_err)] // `MutationError::Prepared` carries a built response
fn resolve_requested_route(
    wiring: &RuntimeWiring,
    command: &'static str,
    payload: &serde_json::Value,
) -> Result<ModelRoute, MutationError> {
    // THE DEFAULT GOES THROUGH THE FRESH MANIFEST TOO. The startup `ModelRoute` clone answered
    // the omitted-route case directly, so an operator who edited the manifest after `serve`
    // started could see `GET /v1/gateway/routes` reflect the change while the default path went
    // on driving the removed, disabled, or re-keyed route from the snapshot (PR #467 review).
    // Only the default's ID survives from startup; what that id MEANS is read from the file.
    let requested = match payload.get("route") {
        None | Some(serde_json::Value::Null) => wiring.route.id(),
        Some(serde_json::Value::String(id)) => id,
        Some(_) => {
            return Err(MutationError::Prepared(bad_request(
                command,
                "\"route\" must be a string naming a route in the manifest",
                "/route",
            )));
        }
    };

    let bytes = std::fs::read(&wiring.manifest_path).map_err(|_| {
        // The path is NOT in the message. `manifest_path` is a filesystem location on the
        // deployer's machine and this text reaches an HTTP caller; the caller cannot act on it
        // and the operator already knows what they passed to `--manifest`.
        MutationError::from(setup_failure("the configured manifest could not be read"))
    })?;
    let text = String::from_utf8(bytes).map_err(|_| {
        MutationError::from(setup_failure("the configured manifest is not valid UTF-8"))
    })?;
    let manifest = RouteManifest::from_json(&text)
        .map_err(|error| MutationError::from(setup_failure(&error.to_string())))?;

    find_route(&manifest, requested).ok_or_else(|| {
        // The requested id is echoed back deliberately: it is the caller's OWN input (or the
        // deployer's default, which the deployer knows), it is the one fact that makes this
        // actionable, and `GET /v1/gateway/routes` is the listing that says what would have
        // worked.
        MutationError::Prepared(bad_request(
            command,
            &format!("\"route\" does not name an enabled route in the manifest: {requested}"),
            "/route",
        ))
    })
}

/// Builds everything in the drive's setup that can refuse, so the *decision* is the last thing that
/// can fail rather than the first thing that commits.
///
/// Every step here is READ-ONLY, which is what makes hoisting it safe: `build_sealer` validates an
/// environment variable; `ServeModelPort::build`'s `direct_api` arm opens the broker (`open`, which
/// reads — not `open_or_create`) and `lease`s a credential, and `CredentialBroker::lease` takes
/// `&self`, acquires no lock and persists nothing — it verifies a MAC and decrypts. `ServeToolPort::build`
/// validates a workspace config and constructs a host. So a decision that is refused after this ran
/// leaves nothing behind to release, and no lifetime story is owed.
async fn prepare_drive(
    state: &ServeState,
    command: &'static str,
    payload: &serde_json::Value,
) -> Result<DriveSetup, MutationError> {
    let sealer = build_sealer(state.sealing.as_deref())
        .map_err(|message| MutationError::from(setup_failure(&message)))?;
    let ports = match &state.runtime {
        Some(wiring) => {
            let route = resolve_requested_route(wiring, command, payload)?;
            let model = ServeModelPort::build(wiring, &route)
                .await
                .map_err(|message| MutationError::from(setup_failure(&message)))?;
            // Three-deep fallback (issue #82): the caller's own `"project"` wins when given (no
            // MCP tool currently exposes this field, but the raw HTTP body always could); absent
            // that, the deployer's own `--project` default (set once, the same way `--staging`
            // itself is); only when NEITHER is configured does this fall back to the server
            // process's own working directory — the default that collides with `--staging`
            // whenever `serve` happens to run from a `--staging` ancestor, which is exactly what
            // #82 documents. A deployer who hits that collision fixes it once with `--project`;
            // nothing changes for a deployment that never had the collision.
            let project = payload
                .get("project")
                .and_then(serde_json::Value::as_str)
                .map(PathBuf::from)
                .or_else(|| wiring.project.clone())
                .or_else(|| std::env::current_dir().ok())
                .ok_or_else(|| {
                    MutationError::from(setup_failure(
                        "no \"project\" was given, no --project default is configured, and the server's working directory could not be read",
                    ))
                })?;
            let tools = ServeToolPort::build(wiring, &project)
                .map_err(|message| MutationError::from(setup_failure(&message)))?;
            Some(PreparedPorts {
                model,
                tools,
                // The route the drive RESOLVED, never `wiring.route` again: the executor stamps
                // this id onto the work it dispatches, so a stale default here would label every
                // call with a model that did not answer it.
                route_id: route.id().to_owned(),
                lease: ToolLease {
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
                },
            })
        }
        None => None,
    };
    Ok(DriveSetup { sealer, ports })
}

async fn drive(
    state: &ServeState,
    execution_id: &str,
    prepared: PreparedDrive,
    setup: DriveSetup,
) -> Result<serde_json::Value, MutationError> {
    let DriveSetup { sealer, ports } = setup;
    let ids = Arc::new(crate::commands::UuidIds);
    let events = state.events.clone();
    let store_open: StoreOpen = Arc::new(move || event_store(&events));

    // Infallible by construction: everything that could refuse already did, in `prepare_drive`,
    // before the caller committed its decision. The fixture branch is the only part that needs
    // `prepared`, and building it cannot fail.
    let executor: Arc<dyn AsyncNodeExecutor> = match ports {
        Some(ports) => Arc::new(PortExecutor {
            model: Arc::new(ports.model),
            tools: Arc::new(ports.tools),
            route_id: ports.route_id,
            lease: ports.lease,
            actor: "runtime".to_owned(),
        }),
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
        prepared.release,
        // #123: the release is the OWNER's act even though the driver appends it — the actor is
        // data on the event, not a property of which loop wrote it.
        crate::commands::execution::owner_actor(),
        cancel_rx,
        // The certified-or-not-at-all precondition compares fold receipts against the
        // suite THIS binary carries: the digest is computed here because core never
        // depends on tools.
        Some(pathogens::suite_digest(&pathogens::suite())),
    )
    .await;

    state.cancels.lock().await.remove(execution_id);

    match result {
        Ok(projection) => Ok(execution::render(
            &projection,
            // This path holds a projection and no history: it states that it measured
            // nothing instead of implying calm.
            &graphhelm_execution::AttentionInputs::default(),
            &execution::Liveness::default(),
        )),
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

/// `POST /v1/development/contract`: #218's code-rule resolver over HTTP. #223 existence-slice —
/// no request body is read yet, matching the CLI and MCP surfaces (see
/// `crate::commands::development::run_resolve_contract`'s own doc for why an empty source list
/// is real behavior, not a stub).
pub(super) async fn development_resolve_contract() -> Response {
    respond_outcome(crate::commands::development::run_resolve_contract())
}

/// `GET /v1/development/memory`: the governed memory vocabulary and the moves policy allows.
///
/// **A GET, and without a record id** — the original scope list carried `/memory/{id}`. Nothing
/// persists a `MemoryRecord`, so an id had nothing to resolve against; see
/// `crate::commands::development::run_memory_status` for the measurement, the two alternatives that
/// were rejected, and when the id returns.
///
/// It answers from the same function the CLI and MCP surfaces call, so the three cannot drift into
/// three readings of one policy.
///
/// **This handler must never answer 404**, and that is a contract rather than an accident: the
/// surface-parity guard reads a 404 from this path as "the route was never wired", which is only
/// sound while no handler under `/v1` produces one. Today none does — the fallback is the sole
/// source. A future id-taking version that answered 404 for an unknown record would take that
/// distinction away from the guard.
pub(super) async fn development_memory_status() -> Response {
    respond_outcome(crate::commands::development::run_memory_status())
}

/// `POST /v1/development/memory`: propose content for governed memory, answering with the
/// admission verdict.
///
/// The same PATH as the GET above and a different VERB, because both act on one resource. The
/// parity probe refuses 405 as well as 404, so a family wired under the wrong verb fails there as
/// loudly as one not wired at all.
///
/// No identifier in the response: nothing persists a candidate, so an id would name something no
/// later call could resolve -- see `crate::commands::development::run_memory_propose` for the
/// measurement.
pub(super) async fn development_memory_propose() -> Response {
    respond_outcome(crate::commands::development::run_memory_propose())
}

/// `POST /v1/development/present`: #219's owner-output renderer over HTTP. #223 existence-slice --
/// no request body is read yet, matching the CLI and MCP surfaces (see
/// `crate::commands::development::run_present`'s own doc for why a fixed task result is real
/// behavior rather than a stub).
pub(super) async fn development_present() -> Response {
    respond_outcome(crate::commands::development::run_present())
}

/// `POST /v1/development/context`: #222/#273's context compiler over HTTP.
///
/// Reads `budget` and `require` and refuses required context that cannot fit, the same decision
/// the CLI makes -- through the SAME oracle, `development::compile_context_decision`, so the two
/// surfaces cannot drift into disagreeing about whether one input fits.
///
/// **The status comes from `development_refusal_http_status`, not from `respond_outcome`.** That
/// generic mapping sends every exit code above 3 to 500, and the allocated refusal exit code is
/// 32 -- so reusing it would have served a well-formed request that simply cannot be honoured at
/// the given budget as an internal server error, which is a lie about whose fault it is.
///
/// An absent or empty body keeps its previous meaning rather than becoming a 400. The
/// existence-parity guard posts `{}` here, and so does every caller written before #393; a change
/// that turned those into failures would break a surface contract older than the budget.
pub(super) async fn development_compile_context(body: Bytes) -> Response {
    let payload: serde_json::Value = if body.is_empty() {
        serde_json::Value::Object(serde_json::Map::new())
    } else {
        match serde_json::from_slice(&body) {
            Ok(value) => value,
            Err(_) => {
                return bad_request(
                    "development.compile-context",
                    "the request body is not valid JSON",
                    "/",
                );
            }
        }
    };
    let budget = usize::try_from(payload["budget"].as_u64().unwrap_or(0)).unwrap_or(usize::MAX);
    let require: Vec<String> = payload["require"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                .collect()
        })
        .unwrap_or_default();

    match crate::commands::development::compile_context_decision(budget, &require) {
        Ok(digest) => respond_outcome(Outcome::success(
            "development.compile-context",
            serde_json::json!({"digest": digest}),
        )),
        Err((code, message)) => respond(
            crate::commands::development::development_refusal_http_status(code),
            crate::commands::development::context_refusal(code, message).output,
        ),
    }
}

/// `GET /v1/development/accounting`: a context-accounting receipt over HTTP. #223
/// existence-slice — no execution id is read yet, matching the CLI and MCP surfaces (see
/// `crate::commands::development::run_accounting`'s own doc for why the one field reports as
/// unavailable rather than zero).
pub(super) async fn development_accounting() -> Response {
    respond_outcome(crate::commands::development::run_accounting())
}

// -------------------------------------------------------------------------------------------
// Milestone 05g Task 3: the wake lease's HTTP surface — the SLEEPER-ONLY half. POST arms the
// caller's own lease (idempotent, the three headers); GET reads it. No route rings: the ring
// is the Task 2 sweep's alone, and a lease's arming event never self-rings (the sweep skips
// wake bookkeeping kinds by design).
// -------------------------------------------------------------------------------------------

const WAKE_LEASE_COMMAND: &str = "execution.wake_lease";

pub(super) async fn wake_lease(
    State(state): State<ServeState>,
    UrlPath(execution_id): UrlPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return bad_request(
                WAKE_LEASE_COMMAND,
                "the request body is not valid JSON",
                "/",
            );
        }
    };
    let identity = match parse_mutation_headers(
        &headers,
        WAKE_LEASE_COMMAND,
        &execution_id,
        &payload,
        &["wake"],
    ) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let Some(session_id) = payload.get("sessionId").and_then(serde_json::Value::as_str) else {
        return bad_request(
            WAKE_LEASE_COMMAND,
            "the request body must carry \"sessionId\"",
            "/sessionId",
        );
    };
    let Some(rendezvous_id) = payload
        .get("rendezvousId")
        .and_then(serde_json::Value::as_str)
    else {
        return bad_request(
            WAKE_LEASE_COMMAND,
            "the request body must carry \"rendezvousId\" (an OPAQUE id, never a path)",
            "/rendezvousId",
        );
    };
    let cursor = payload.get("cursor").and_then(serde_json::Value::as_u64);
    // M09 decision B: how long the sleeper's quiet may last. Absent means absent — no horizon
    // is invented for a lease that declared none.
    let matures_in_seconds = payload
        .get("maturesInSeconds")
        .and_then(serde_json::Value::as_u64);
    // Bounded at BOTH ends, and the loose end is the dangerous one. Zero is not a bound; but a
    // bound of a trillion seconds is a horizon in the year 33715, and the surface would answer
    // with a DATE, which reads as a promise while meaning never. That is absence laundered into
    // calm through arithmetic. Ten years is the ceiling its neighbours already use for a
    // declared duration (`nodeTimeoutSeconds`, `observedSilenceSeconds`), so the refusal is the
    // house's existing answer rather than a number invented here.
    const TEN_YEARS_SECONDS: u64 = 315_576_000;
    if payload.get("maturesInSeconds").is_some()
        && matures_in_seconds.is_none_or(|seconds| seconds == 0 || seconds > TEN_YEARS_SECONDS)
    {
        return bad_request(
            WAKE_LEASE_COMMAND,
            &format!(
                "\"maturesInSeconds\" must be between 1 and {TEN_YEARS_SECONDS} (ten years): a bound of zero is not a bound, and a bound nobody will live to see is a promise of never wearing the shape of a date"
            ),
            "/maturesInSeconds",
        );
    }
    let session_id = session_id.to_owned();
    let rendezvous_id = rendezvous_id.to_owned();
    let events = state.events.clone();
    let target = execution_id.clone();

    run_idempotent_mutation(
        &state.events,
        &execution_id,
        WAKE_LEASE_COMMAND,
        identity,
        ExecutorWiring::from_state(&state),
        |actor, key| {
            Box::pin(async move {
                Ok(execution::wake::arm(
                    &events,
                    Some(target.as_str()),
                    &execution::wake::Arming {
                        session_id: &session_id,
                        rendezvous_id: &rendezvous_id,
                        cursor,
                        matures_in_seconds,
                    },
                    actor,
                    key,
                )?)
            })
        },
    )
    .await
}

/// `GET /v1/executions/{id}/wake-lease?sessionId=<id>`: the caller's own live lease, read
/// from the same projection every surface folds.
pub(super) async fn wake_lease_status(
    State(state): State<ServeState>,
    UrlPath(execution_id): UrlPath<String>,
    RawQuery(query): RawQuery,
) -> Response {
    let session_id = query.as_deref().unwrap_or("").split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == "sessionId").then(|| value.to_owned())
    });
    let Some(session_id) = session_id else {
        return bad_request(
            WAKE_LEASE_COMMAND,
            "the sessionId query parameter is required",
            "/sessionId",
        );
    };
    match execution::wake::status(&state.events, Some(&execution_id), &session_id) {
        Ok(value) => respond(
            StatusCode::OK,
            Outcome::success(WAKE_LEASE_COMMAND, value).output,
        ),
        Err(failure) => respond_failure(WAKE_LEASE_COMMAND, failure),
    }
}
/// `GET /v1/executions/{id}/evidence/{evidenceId}`: the content behind an evidence reference,
/// opened.
///
/// WHY THIS ROUTE EXISTS. Every outcome a node records seals its real content — a model's reply,
/// a tool's stdout — into encrypted Evidence, and the event carries only the reference and the
/// token counts (D-036, `core/runtime/src/executor.rs`). The event stream could therefore say
/// THAT a node replied and never what it said, which is a log an operator cannot read and a chat
/// surface cannot render. The sealing is right; the missing half was a way back in.
///
/// WHAT IT DOES NOT WIDEN. `evidence_exists` gates the lookup inside the store, and it answers
/// from the events actually recorded — so this serves Evidence some event already references and
/// refuses anything else, including a blob sitting in `blobs/` that nothing points at. The
/// execution in the URL must ITSELF reference the id — checked against that execution's own
/// replay below, because the store's gate is scope-wide and every execution here shares one
/// scope; the store gate alone let one execution's evidence answer under another's URL. And the
/// key is the server's own: a caller who could not already reach `serve`'s keyring gains nothing
/// here.
///
/// WHAT IT DOES WIDEN, said plainly rather than left for someone to discover: this server's
/// bearer token used to unlock metadata, and now unlocks `Confidential` plaintext — the class
/// replies and streams are sealed under. That is the point of the route and it is a real change
/// in what the token is worth. The reply names the `sensitivity` of what it returns so a caller
/// holding it knows which class it is, rather than having to know where it came from.
pub(super) async fn evidence(
    State(state): State<ServeState>,
    UrlPath((execution_id, evidence_id)): UrlPath<(String, String)>,
) -> Response {
    let Ok(id) = EvidenceId::parse(evidence_id.clone()) else {
        return bad_request(
            EVIDENCE_COMMAND,
            "the evidence id is not a valid identifier",
            "/evidenceId",
        );
    };
    // Built BEFORE the store read, so a server that was never wired to decrypt says so instead of
    // reading a blob it then cannot open — the refusal names the configuration, not the content.
    let opener = match build_opener(state.sealing.as_deref()) {
        Ok(opener) => opener,
        Err(message) => return evidence_refusal(&message),
    };

    let events = state.events.clone();
    let lookup = tokio::task::spawn_blocking(move || {
        let store = event_store(&events).map_err(|error| execution::repository_failure(&error))?;
        let (scope, _, history) = execution::resolve_stream(&store, Some(&execution_id))?;
        // THE URL'S EXECUTION MUST ITSELF REFERENCE THE ID. `sealed_evidence` gates against the
        // SCOPE-wide reachable set, and every execution here shares one workspace/project scope -
        // so without this check, execution A's confidential content answered under execution B's
        // URL, and a caller could misattribute whose evidence they were reading (PR #467 review).
        // The history is this execution's own replay, already in hand from `resolve_stream`.
        let referenced = history.iter().any(|event| {
            event
                .evidence_refs
                .iter()
                .any(|reference| reference.evidence_id() == &id)
        });
        if !referenced {
            return Ok((scope, None));
        }
        // `Invalid` IS SEPARATED FROM EVERY OTHER STORE ERROR, and the separation is what makes
        // the store's own gate visible from out here. `sealed_evidence` answers `Invalid` for an
        // id no recorded event references, and `Integrity` when the store finds itself damaged.
        // Folded together they are both "500, something went wrong", which would mean removing
        // the gate entirely still produced a non-200 — a test asserting only "not 200" would go on
        // passing, and the gate would be unguarded while looking guarded.
        let read = match store.sealed_evidence(&scope, &id) {
            Ok(read) => read,
            Err(EventRepositoryError::Invalid) => return Ok((scope, None)),
            Err(error) => return Err(execution::repository_failure(&error)),
        };
        Ok::<_, execution::Failure>((scope, Some(read)))
    })
    .await;

    let (scope, read) = match lookup {
        Ok(Ok(found)) => found,
        Ok(Err(failure)) => return respond_failure(EVIDENCE_COMMAND, failure),
        Err(_) => {
            return respond(
                StatusCode::INTERNAL_SERVER_ERROR,
                Outcome::internal(EVIDENCE_COMMAND, "the evidence read task failed").output,
            );
        }
    };
    let Some(read) = read else {
        return evidence_refusal(
            "no evidence with that id is referenced by this execution's events",
        );
    };

    let sealed = match read {
        EvidenceRead::Available(sealed) => sealed,
        // The store said the content is gone rather than that it never existed. The reason is
        // reported as-is: "erased" and "expired" are different facts about the same absence, and
        // collapsing them would tell an operator to go looking for something that was deleted on
        // purpose.
        EvidenceRead::Unavailable(reason) => {
            return evidence_refusal(&format!("the evidence is unavailable: {reason:?}"));
        }
    };

    // CHECKED BEFORE OPENING, the way the Governor checks before it materializes
    // (`core/governor/src/materialize.rs`). Decrypting bytes this surface has already decided it
    // cannot render would put plaintext in memory for nothing.
    let media_type = sealed.media_type().as_str().to_owned();
    if !renders_as_text(&media_type) {
        return evidence_refusal(&format!(
            "this evidence is {media_type}, which this route does not render; it serves JSON and text only"
        ));
    }
    let sensitivity = sealed.sensitivity();
    let content_sha256 = sealed.reference().content_sha256().to_owned();

    let plaintext = match opener.open(scope, &sealed).await {
        Ok(plaintext) => plaintext,
        Err(error) => {
            return evidence_refusal(&format!("the evidence could not be opened: {error}"));
        }
    };
    // `SecretBytes` exposes only through a callback and zeroizes on drop; the UTF-8 check and the
    // copy both happen inside that callback so nothing outstays it. From here the plaintext is an
    // ordinary `String` in a response body — the zeroization guarantee ends at this line, and it
    // ends here for every reader of this route, which is what the doc comment above is about.
    let text = plaintext.expose(|bytes| String::from_utf8(bytes.to_vec()));
    let Ok(text) = text else {
        return evidence_refusal(
            "this evidence is not valid UTF-8, so it cannot be rendered as text",
        );
    };

    respond(
        StatusCode::OK,
        Outcome::success(
            EVIDENCE_COMMAND,
            serde_json::json!({
                "evidenceId": evidence_id,
                "mediaType": media_type,
                "sensitivity": sensitivity,
                "contentSha256": content_sha256,
                "content": text,
            }),
        )
        .output,
    )
}

/// Evidence this route will render. The model reply — the reason the route exists — seals as
/// `application/json` (`core/runtime/src/evidence.rs`), and a tool node's streams seal as
/// `text/plain`. Anything else is refused rather than guessed at: a surface that base64s unknown
/// bytes into a JSON string is not showing an operator their content, it is moving it.
fn renders_as_text(media_type: &str) -> bool {
    media_type == "application/json" || media_type.starts_with("text/")
}

/// One refusal shape for every way this read can decline, all of them 409: the request was
/// well-formed and the server understood it, and what it could not do is a fact about this
/// server's configuration or this evidence's state rather than about the caller's syntax.
fn evidence_refusal(message: &str) -> Response {
    respond(
        StatusCode::CONFLICT,
        Outcome::domain(
            EVIDENCE_COMMAND,
            vec![Diagnostic::error(
                "GHCLI023_EVIDENCE_UNREADABLE",
                message,
                "/evidence",
                SOURCE,
            )],
        )
        .output,
    )
}
