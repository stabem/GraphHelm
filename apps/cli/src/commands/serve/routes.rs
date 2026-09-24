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
use graphhelm_architect::{
    ArchitectRefusal, DraftModel, DraftReply, GraphLibrary, JudgeModel, RecordedDraftModel,
    RecordedJudgeModel,
};
use graphhelm_events::{ClearanceOutcome, EventRepositoryError, EvidenceRead, EvidenceSealer};
use graphhelm_gateway::call::ModelCall;
use graphhelm_graph::GraphVersion;
use graphhelm_protocols::{ActorId, Diagnostic, EvidenceId, PersistedActor, PersistedActorType};
use graphhelm_runtime::driver::{ImmediateCancelRequest, StoreOpen, drive_to_quiescence_async};
use graphhelm_runtime::executor::{
    AsyncNodeExecutor, ModelExecutor, PortExecutor, SplitExecutor, ToolExecutor,
};
use graphhelm_runtime::fixture::FixtureAsyncExecutor;
use graphhelm_runtime::ports::ModelPort;
use graphhelm_simulation::FixtureExecutor;
use graphhelm_tool_broker::lease::{Capability, ToolLease};

use graphhelm_gateway::manifest::{ModelRoute, RouteManifest, Transport};
use graphhelm_model_gateway::systemone::TYPESAFE_PROVIDER;

use super::ports::{
    ModelWiring, RuntimeWiring, ServeModelPort, ServeToolPort, WorkspaceRelease, build_opener,
    build_sealer, find_route,
};
use super::{
    ExecutorWiring, MutationError, PausedUnderCaller, ServeState, execution_paused_under,
    parse_mutation_headers, respond, respond_failure, run_idempotent_mutation,
};
use crate::commands::architect::{self, SynthesizeRequest};
use crate::commands::execution::PreparedDrive;
use crate::commands::{event_store, execution, owner, publish_loaded, topology};
use crate::output::Outcome;

const LIST_COMMAND: &str = "execution.list";
const TOPOLOGY_COMMAND: &str = "graph.topology";
const SYNTHESIZE_COMMAND: &str = "graph.synthesize";
const STATUS_COMMAND: &str = "execution.status";
const BRIEFING_COMMAND: &str = "execution.briefing";
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
const CLAIM_COMMAND: &str = "execution.claim";
const CLEAR_COMMAND: &str = "execution.clear";
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
/// Runs synchronous filesystem or replay work OFF the reactor (#559).
///
/// `serve` runs on a current-thread runtime: one task doing a blocking read stalls every other
/// request -- health, status, the mutations -- for as long as the read takes. Three handlers did
/// exactly that (the execution index replaying up to twenty streams, the topology read, the
/// per-request manifest re-read) while the evidence and sweep paths had already moved to
/// `spawn_blocking`. Every such site now goes through this one function, and the tests below
/// hold each of them to it by a witness that records the thread the work ran on.
///
/// `None` means the blocking task itself failed (panicked or was cancelled); each caller says
/// what that means for its own reply.
async fn off_reactor<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    #[cfg(test)]
    let work = off_reactor_witness::observed(work);
    tokio::task::spawn_blocking(work).await.ok()
}

/// TEST-ONLY SEAM, in the style of `LocalEventRepository::shared_fast_open_count`: records, for
/// every piece of work `off_reactor` runs, which thread ran it. A cell that drives a handler on a
/// current-thread runtime can then ask two questions no timing can answer -- did this handler's
/// work go through `off_reactor` at all, and did it run somewhere other than the reactor thread.
#[cfg(test)]
mod off_reactor_witness {
    use std::sync::Mutex;
    use std::thread::ThreadId;

    static RUNS: Mutex<Vec<ThreadId>> = Mutex::new(Vec::new());

    pub(super) fn observed<T>(work: impl FnOnce() -> T) -> impl FnOnce() -> T {
        move || {
            RUNS.lock().unwrap().push(std::thread::current().id());
            work()
        }
    }

    /// The threads every run so far happened on, in order.
    pub(super) fn runs() -> Vec<ThreadId> {
        RUNS.lock().unwrap().clone()
    }
}

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

    // Open, read (bounded), parse, validate, hash: file work, off the reactor (#559).
    let file = file.to_owned();
    let Some(outcome) = off_reactor(move || topology::execute(Path::new(&file))).await else {
        return respond(
            StatusCode::INTERNAL_SERVER_ERROR,
            Outcome::internal(TOPOLOGY_COMMAND, "the topology task failed").output,
        );
    };
    match outcome {
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

/// `POST /v1/graphs/synthesize` with `{goal, mode?, maxNodes?, allowPrograms?, fixture?,
/// route?}`: the Graph Architect over HTTP (#107), replying `graph.synthesize`'s own `data` —
/// the exact `architect::execute` the CLI's `graph synthesize` runs, so the document, the
/// rationale and the template hash are one reply on every door (spec D8), minus only the `out`
/// path the CLI alone writes.
///
/// A READ-SHAPED POST like `graph_topology`: it publishes nothing, starts nothing and appends
/// nothing, so it carries no `Idempotency-Key` and names no execution. The body is CLOSED — an
/// unknown field is refused at its own pointer rather than defaulted — for the reason
/// `TaskProfile` is `deny_unknown_fields`: a misspelled `maxNodes` that quietly became the
/// default would be a ceiling nobody asked for.
///
/// THE MODEL DOOR. `fixture` names a recorded-replies file on the RUNTIME's host — the keyless
/// door every test uses, and the same trust seam as `start`'s `file` (the caller already holds
/// a bearer token that can make this Runtime read a graph by path). Without it, the server's
/// own wiring answers: `route` (or the deployer's default) is resolved against the FRESH
/// manifest by `resolve_requested_route`, and `ServeModelPort::build` leases the credential
/// exactly as a drive does — no new credential path. A server with neither is a fixture-only
/// deployment, and the 400 names both options. `allowPrograms` defaults to the server's own
/// `--allow-program` list when wiring exists, else to nothing: the compiler never invents a
/// program, and the request can narrow the allowlist but a wider one reaches only the
/// compiler's catalog check, never the tool lease.
///
/// The synthesis itself is synchronous compiler work plus, on the gateway door, a blocking
/// model call, so it runs OFF the reactor (#559) through `off_reactor`; the port's async `call`
/// is driven from inside that blocking task (see `ServeDraftModel`).
pub(super) async fn synthesize(State(state): State<ServeState>, body: Bytes) -> Response {
    const FIELDS: [&str; 10] = [
        "goal",
        "mode",
        "maxNodes",
        "allowPrograms",
        "fixture",
        "route",
        "judgeRoute",
        "judgeFixture",
        "drafts",
        "library",
    ];
    let payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return bad_request(
                SYNTHESIZE_COMMAND,
                "the request body is not valid JSON",
                "/",
            );
        }
    };
    let Some(object) = payload.as_object() else {
        return bad_request(
            SYNTHESIZE_COMMAND,
            "the request body must be a JSON object",
            "/",
        );
    };
    if let Some(unknown) = object.keys().find(|key| !FIELDS.contains(&key.as_str())) {
        return bad_request(
            SYNTHESIZE_COMMAND,
            &format!("the request body carries a field this route does not read: {unknown}"),
            &format!("/{unknown}"),
        );
    }
    let Some(goal) = object.get("goal").and_then(serde_json::Value::as_str) else {
        return bad_request(
            SYNTHESIZE_COMMAND,
            "the request body must carry \"goal\" as a string",
            "/goal",
        );
    };
    let mode = match object.get("mode") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(mode)) => Some(mode.clone()),
        Some(_) => {
            return bad_request(SYNTHESIZE_COMMAND, "\"mode\" must be a string", "/mode");
        }
    };
    let max_nodes = match object.get("maxNodes") {
        None | Some(serde_json::Value::Null) => None,
        Some(value) => match value.as_u64().and_then(|count| usize::try_from(count).ok()) {
            Some(count) => Some(count),
            None => {
                return bad_request(
                    SYNTHESIZE_COMMAND,
                    "\"maxNodes\" must be a non-negative integer",
                    "/maxNodes",
                );
            }
        },
    };
    let allow_programs: Option<Vec<String>> = match object.get("allowPrograms") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::Array(items)) => {
            let mut programs = Vec::with_capacity(items.len());
            for item in items {
                let Some(program) = item.as_str() else {
                    return bad_request(
                        SYNTHESIZE_COMMAND,
                        "\"allowPrograms\" must be an array of strings",
                        "/allowPrograms",
                    );
                };
                programs.push(program.to_owned());
            }
            Some(programs)
        }
        Some(_) => {
            return bad_request(
                SYNTHESIZE_COMMAND,
                "\"allowPrograms\" must be an array of strings",
                "/allowPrograms",
            );
        }
    };
    let fixture = match object.get("fixture") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(path)) => Some(PathBuf::from(path)),
        Some(_) => {
            return bad_request(
                SYNTHESIZE_COMMAND,
                "\"fixture\" must be a string naming a recorded-replies file on the Runtime host",
                "/fixture",
            );
        }
    };
    if fixture.is_some() && object.get("route").is_some_and(|route| !route.is_null()) {
        return bad_request(
            SYNTHESIZE_COMMAND,
            "\"fixture\" and \"route\" are mutually exclusive: one model door per request",
            "/fixture",
        );
    }
    let judge_fixture = match object.get("judgeFixture") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(path)) => Some(PathBuf::from(path)),
        Some(_) => {
            return bad_request(
                SYNTHESIZE_COMMAND,
                "\"judgeFixture\" must be a string naming a recorded-answers file on the \
                 Runtime host",
                "/judgeFixture",
            );
        }
    };
    let judge_route = match object.get("judgeRoute") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(id)) => Some(id.clone()),
        Some(_) => {
            return bad_request(
                SYNTHESIZE_COMMAND,
                "\"judgeRoute\" must be a string naming a direct_api typesafe route in the \
                 manifest",
                "/judgeRoute",
            );
        }
    };
    if judge_fixture.is_some() && judge_route.is_some() {
        return bad_request(
            SYNTHESIZE_COMMAND,
            "\"judgeFixture\" and \"judgeRoute\" are mutually exclusive: one judge door per \
             request",
            "/judgeFixture",
        );
    }
    // The bound `1..=3` and "more than one draft needs a judge" are the compiler's own
    // `InvalidProfile` refusal (`GHCLI026`), so every door refuses with the same words; this
    // only checks the shape.
    let drafts = match object.get("drafts") {
        None | Some(serde_json::Value::Null) => None,
        Some(value) => match value.as_u64().and_then(|count| u8::try_from(count).ok()) {
            Some(count) => Some(count),
            None => {
                return bad_request(
                    SYNTHESIZE_COMMAND,
                    "\"drafts\" must be a non-negative integer",
                    "/drafts",
                );
            }
        },
    };
    let library = match object.get("library") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(path)) => Some(PathBuf::from(path)),
        Some(_) => {
            return bad_request(
                SYNTHESIZE_COMMAND,
                "\"library\" must be a string naming a template directory on the Runtime host",
                "/library",
            );
        }
    };

    let model: Box<dyn DraftModel + Send> = match fixture {
        Some(path) => {
            // A bounded, regular-file-only read (the adapter's own `from_file`), off the reactor.
            let Some(loaded) = off_reactor(move || RecordedDraftModel::from_file(&path)).await
            else {
                return respond(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Outcome::internal(SYNTHESIZE_COMMAND, "the fixture read task failed").output,
                );
            };
            match loaded {
                Ok(model) => Box::new(model),
                // The refusal names the failure class and never the path: the same
                // `GHCLI026` the CLI prints for the same file.
                Err(refusal) => {
                    return respond_outcome(
                        architect::refused(&refusal).into_outcome(SYNTHESIZE_COMMAND),
                    );
                }
            }
        }
        None => match state
            .runtime
            .as_ref()
            .and_then(|wiring| wiring.model.as_ref().map(|model| (wiring, model)))
        {
            None => {
                return bad_request(
                    SYNTHESIZE_COMMAND,
                    "this server has no model route wired (fixture-only or tools-only mode): \
                     pass \"fixture\" naming a recorded-replies file, or start serve with \
                     --manifest/--route so a model route can answer",
                    "/fixture",
                );
            }
            Some((wiring, model_wiring)) => {
                let route =
                    match resolve_requested_route(model_wiring, SYNTHESIZE_COMMAND, &payload).await
                    {
                        Ok(route) => route,
                        Err(MutationError::Prepared(response)) => return response,
                        Err(MutationError::Command(failure)) => {
                            return respond_failure(SYNTHESIZE_COMMAND, failure);
                        }
                    };
                match ServeModelPort::build(wiring, model_wiring, &route).await {
                    Ok(port) => Box::new(ServeDraftModel {
                        route_id: route.id().to_owned(),
                        port,
                    }),
                    Err(message) => {
                        return respond_failure(SYNTHESIZE_COMMAND, setup_failure(&message));
                    }
                }
            }
        },
    };
    let judge: Option<Box<dyn JudgeModel + Send>> = match (judge_fixture, judge_route) {
        (None, None) => None,
        (Some(path), _) => {
            // The same bounded, regular-file-only read the draft fixture takes, off the reactor.
            let Some(loaded) = off_reactor(move || RecordedJudgeModel::from_file(&path)).await
            else {
                return respond(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Outcome::internal(SYNTHESIZE_COMMAND, "the judge fixture read task failed")
                        .output,
                );
            };
            match loaded {
                Ok(judge) => Some(Box::new(judge)),
                Err(refusal) => {
                    return respond_outcome(
                        architect::refused(&refusal).into_outcome(SYNTHESIZE_COMMAND),
                    );
                }
            }
        }
        (None, Some(id)) => match state
            .runtime
            .as_ref()
            .and_then(|wiring| wiring.model.as_ref().map(|model| (wiring, model)))
        {
            None => {
                return bad_request(
                    SYNTHESIZE_COMMAND,
                    "this server has no model route wired (fixture-only or tools-only mode): \
                     pass \"judgeFixture\" naming a recorded-answers file, or start serve with \
                     --manifest/--route so a judge route can be leased",
                    "/judgeRoute",
                );
            }
            Some((wiring, model_wiring)) => {
                match resolve_judge_route(wiring, model_wiring, SYNTHESIZE_COMMAND, &id).await {
                    Ok(judge) => Some(Box::new(judge)),
                    Err(MutationError::Prepared(response)) => return response,
                    Err(MutationError::Command(failure)) => {
                        return respond_failure(SYNTHESIZE_COMMAND, failure);
                    }
                }
            }
        },
    };
    let library: Option<GraphLibrary> = match library {
        None => None,
        Some(dir) => {
            // A bounded directory listing (the crate's own `GraphLibrary::load`), off the reactor.
            let Some(loaded) = off_reactor(move || architect::build_library(&dir)).await else {
                return respond(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Outcome::internal(SYNTHESIZE_COMMAND, "the library read task failed").output,
                );
            };
            match loaded {
                Ok(library) => Some(library),
                Err(failure) => return respond_outcome(failure.into_outcome(SYNTHESIZE_COMMAND)),
            }
        }
    };
    let allow_programs = allow_programs.unwrap_or_else(|| {
        state
            .runtime
            .as_ref()
            .and_then(|wiring| wiring.tools.as_ref())
            .map(|tools| tools.allow_programs.clone())
            .unwrap_or_default()
    });

    let goal = goal.to_owned();
    let outcome = off_reactor(move || {
        let request = SynthesizeRequest {
            goal: &goal,
            mode: mode.as_deref(),
            max_nodes,
            allow_programs: &allow_programs,
            wait_within_seconds: None,
            clearance_within_seconds: None,
            drafts,
        };
        architect::execute(
            &request,
            model.as_ref(),
            judge.as_deref().map(|judge| judge as &dyn JudgeModel),
            library.as_ref(),
        )
        .map_err(|failure| failure.into_outcome(SYNTHESIZE_COMMAND))
    })
    .await;
    match outcome {
        None => respond(
            StatusCode::INTERNAL_SERVER_ERROR,
            Outcome::internal(SYNTHESIZE_COMMAND, "the synthesis task failed").output,
        ),
        Some(Ok(value)) => respond(
            StatusCode::OK,
            Outcome::success(SYNTHESIZE_COMMAND, value).output,
        ),
        // `GHCLI026` is a domain refusal (exit 2): the caller's goal did not compile, a 400.
        Some(Err(outcome)) => respond_outcome(outcome),
    }
}

/// The serve-side judge door: `"judgeRoute"` resolved against the FRESH manifest exactly as
/// `"route"` is (`resolve_requested_route`'s reader, so the two cannot drift), required to be a
/// `direct_api` route of provider `typesafe` (the only transport the System One adapter speaks;
/// refused before any lease), and its credential leased through the SAME `ServeModelPort::build`
/// a drive's executor uses — the broker and keyring are the server's, never the request's. The
/// leased key then goes into the CLI's own [`architect::GatewayJudgeModel`], so the HTTP door
/// and `--judge-route` place the identical call. The adapter is synchronous and the synthesis
/// already runs off the reactor, so no bridge like `ServeDraftModel`'s is needed.
#[allow(clippy::result_large_err)] // `MutationError::Prepared` carries a built response
async fn resolve_judge_route(
    wiring: &RuntimeWiring,
    model_wiring: &ModelWiring,
    command: &'static str,
    requested: &str,
) -> Result<architect::GatewayJudgeModel, MutationError> {
    let manifest = reread_manifest(model_wiring).await?;
    let route = find_route(&manifest, requested).ok_or_else(|| {
        MutationError::Prepared(bad_request(
            command,
            &format!("\"judgeRoute\" does not name an enabled route in the manifest: {requested}"),
            "/judgeRoute",
        ))
    })?;
    if route.transport() != Transport::DirectApi || route.provider() != TYPESAFE_PROVIDER {
        return Err(MutationError::Prepared(bad_request(
            command,
            "\"judgeRoute\" must name a direct_api typesafe route",
            "/judgeRoute",
        )));
    }
    match ServeModelPort::build(wiring, model_wiring, &route).await {
        Ok(ServeModelPort::DirectApi { route, key }) => {
            Ok(architect::GatewayJudgeModel::new(route, key))
        }
        // Unreachable after the transport check above; named rather than panicked on.
        Ok(ServeModelPort::NativeRuntime { .. }) => Err(MutationError::from(setup_failure(
            "the judge route resolved to a native_runtime port",
        ))),
        Err(message) => Err(MutationError::from(setup_failure(&message))),
    }
}

/// The serve-side [`DraftModel`]: the [`ServeModelPort`] built for the resolved route (the same
/// port a drive's executor uses, credential leased the same way), driven from inside the
/// blocking task the synthesis runs in. `synthesize` is synchronous and the port's `call` is
/// async, so `draft` bridges with `Handle::current().block_on`: legal on a blocking-pool
/// thread, which holds the runtime's handle but is not a task of the reactor, and — because
/// `serve` runs on a current-thread runtime (`commands::events::runtime`) — driven by the
/// reactor thread sitting in `Runtime::block_on`, which is what `Handle::block_on`'s own
/// contract requires there. No `block_in_place`: tokio ships it only under `rt-multi-thread`,
/// which this workspace does not enable, and on a current-thread runtime it would be a
/// no-op off the reactor and a panic on it. Calling `draft` ON the reactor is the one thing
/// this type must never do, and `synthesize` reaches it only through `off_reactor`.
struct ServeDraftModel {
    route_id: String,
    port: ServeModelPort,
}

impl DraftModel for ServeDraftModel {
    fn draft(&self, prompt: &str) -> Result<DraftReply, ArchitectRefusal> {
        let call = ModelCall {
            prompt: prompt.to_owned(),
            max_tokens: architect::MAX_TOKENS,
        };
        let reply =
            tokio::runtime::Handle::current().block_on(self.port.call(&self.route_id, &call));
        reply
            .map(|reply| DraftReply {
                text: reply.text,
                usage: Some(reply.usage),
            })
            // The taxonomy's `Display` is fixed static prose: never a path, never a key.
            .map_err(|error| ArchitectRefusal::ModelUnavailable {
                message: error.to_string(),
            })
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
    list_executions_over(state.events.clone(), query.as_deref().unwrap_or("")).await
}

/// The index over `events`, split from the extractor so a cell can drive it without a server.
async fn list_executions_over(events: Arc<Path>, query: &str) -> Response {
    let (after, limit) = match parse_list_query(query) {
        Ok(parsed) => parsed,
        Err((message, pointer)) => return bad_request(LIST_COMMAND, message, pointer),
    };
    // Up to `limit` streams replayed, each up to the store's event ceiling: replay work, off the
    // reactor (#559).
    let listed = off_reactor(move || execution::list::execute(&events, after.as_deref(), limit));
    match listed.await {
        Some(Ok(value)) => respond(StatusCode::OK, Outcome::success(LIST_COMMAND, value).output),
        Some(Err(failure)) => respond_failure(LIST_COMMAND, failure),
        None => respond(
            StatusCode::INTERNAL_SERVER_ERROR,
            Outcome::internal(LIST_COMMAND, "the listing task failed").output,
        ),
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
/// `execution::status` read the CLI's `execution status` runs, so the two report byte-identical
/// output for the same stream ("one store, one truth").
///
/// `budgeted`, not `execute`: this is the read a caller WAITS on, and #750's measurement is that
/// it is linear in the history with nothing bounding the time. The mutation replies further down
/// this file keep the unbounded `execute` - a read that ran out of time must never turn a write
/// that already committed into a refusal.
pub(super) async fn status(
    State(state): State<ServeState>,
    UrlPath(execution_id): UrlPath<String>,
) -> Response {
    match execution::status::budgeted(&state.events, Some(&execution_id), None) {
        Ok(value) => respond(
            StatusCode::OK,
            Outcome::success(STATUS_COMMAND, value).output,
        ),
        Err(failure) => respond_failure(STATUS_COMMAND, failure),
    }
}

/// `GET /v1/executions/{id}/briefing` (#1063): replies `execution.briefing`'s own `data` - the
/// exact same `execution::briefing` read the CLI runs, under the same read budget and the same
/// token as `status`, so a harness picking the run up over HTTP or MCP reads byte-identical
/// bytes to one reading the store directly.
pub(super) async fn briefing(
    State(state): State<ServeState>,
    UrlPath(execution_id): UrlPath<String>,
) -> Response {
    match execution::briefing::budgeted(&state.events, Some(&execution_id)) {
        Ok(value) => respond(
            StatusCode::OK,
            Outcome::success(BRIEFING_COMMAND, value).output,
        ),
        Err(failure) => respond_failure(BRIEFING_COMMAND, failure),
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
                crate::error_codes::GHCLI001_ARGUMENT_INVALID,
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
/// none>", "mode": "autopilot|supervised|manual", "held": <bool, omitted for false>}`, mirroring
/// the CLI's `--file`/`--fixtures`/`--mode`/`--held`. The operator-supplied `file`/`fixtures` paths are used exactly as the CLI would use
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
    // #90: the HTTP spelling of `execution start --held`. Absent means false (every existing
    // caller drives, unchanged); anything but a boolean is refused rather than read as truthy.
    let held = match payload.get("held") {
        None => false,
        Some(serde_json::Value::Bool(held)) => *held,
        Some(_) => return bad_request(START_COMMAND, "\"held\" must be a boolean", "/held"),
    };
    // Cloned ahead of the closure below — `state` is cheap to clone (every field is
    // `Arc`-backed) and `execution_id` is a plain owned `String` — so `async move` can take
    // ownership of its own copies while the outer call still borrows the originals directly.
    let drive_state = state.clone();
    // #1066: the drive's own copy of the id — the closure below moves what it captures, and the
    // mutation runner still borrows `execution_id` for its own journalling.
    let drive_execution = execution_id.clone();
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
                // #90: a held start is the publish half and NO drive half, whichever drive this
                // graph would otherwise get - the same `execute_held` the CLI's `--held` calls.
                if held {
                    return Ok(execution::start::execute_held(
                        &version,
                        &drive_state.events,
                        fixtures.as_deref(),
                        &mode,
                        Some(drive_execution_id.as_str()),
                        actor,
                        key,
                    )?);
                }
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
                    let setup =
                        prepare_drive(&drive_state, START_COMMAND, &drive_execution, &payload)
                            .await?;
                    let prepared = execution::start::execute_prepared(
                        &version,
                        &drive_state.events,
                        fixtures.as_deref(),
                        &mode,
                        Some(drive_execution_id.as_str()),
                        execution::start::Attribution {
                            actor,
                            key,
                            // #1063: gateway when the real-executor wiring is configured,
                            // fixture otherwise - the same predicate `annotate_fixture_only`
                            // reads, so the form and the warning never disagree.
                            executor: Some(ExecutorWiring::from_state(&drive_state).declared()),
                        },
                        // #90: `held` returned above, so this start drives.
                        false,
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

/// `POST /v1/executions/{id}/pause`: mirrors the CLI's `execution pause`, which takes only
/// `--events`/`--execution`, plus the optional `{"mode": "immediate"}` body Milestone 05d Task 9
/// STEP 5 added — absent, or any other value, keeps the graceful behaviour below byte-identical.
///
/// `immediate` (a plain boolean, not the raw body — unrelated body noise must still classify two
/// otherwise-identical GRACEFUL calls as the same command) stands in for the request in the
/// digest `parse_mutation_headers` derives the key from. **Not `Value::Null`, as it once was: the
/// two modes are observably different commands sharing one field capable of telling them apart,
/// and folding it in is what makes a graceful pause under key K and a later immediate pause reusing
/// K conflict instead of the second one silently classifying `Complete` against the first's commit
/// and returning 200 with no cancellation ever sent (#681, Codex P1).**
///
/// `"immediate"` looks the execution up in `state.cancels`: a live async drive gets `send(Some(..))`
/// on its cancel channel, then this handler polls `execution::status::execute` (the same read
/// `GET /v1/executions/{id}` uses) every 100ms for up to 10s until `execution_paused` has folded,
/// and replies with that status. No live sender (an idle execution, or a fixture drive with
/// nothing in flight) falls through to the existing graceful `execute` below, the 04f pause — that
/// routing decision stays outside the wrapper, unchanged, since it decides WHICH command body runs,
/// not whether this request is fresh.
///
/// **#681: everything past that routing decision now goes through `run_idempotent_mutation`, same
/// as the graceful path.** Before this, the immediate branch skipped it entirely: `If-Match` was
/// parsed and ignored, `Idempotency-Key` was validated for shape only and never classified against
/// history, and the eventual `execution_paused` append (inside `drive_to_quiescence_async`, once
/// every in-flight node is recorded `Interrupted`) carried the DRIVE's own actor rather than this
/// request's caller. Wrapping closes the first two; the third needed the cancel channel itself to
/// carry `actor`/`idempotency_key` through to that later, asynchronous append (see
/// `ImmediateCancelRequest`) — this route no longer appends anything itself in either mode, exactly
/// as before, it just hands the wrapper's parsed identity down the channel instead of discarding it.
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

    // #681, Codex P1: graceful and immediate are OBSERVABLY DIFFERENT commands -- one holds every
    // dispatchable node and lets in-flight work finish, the other interrupts it now -- so they
    // must not share a derived key. Before #681 routed the immediate branch through this
    // wrapper's own pre-flight, they harmlessly did share one (`Value::Null`): the immediate
    // branch never consulted `classify_existing_keys` at all, so a reused key across modes was
    // never actually compared. Now that it is, a caller who gracefully paused under key K and
    // later retries with the SAME key but `{"mode":"immediate"}` would classify `Complete` against
    // the graceful commit and get a silent 200 with NO cancellation sent -- the immediate stop
    // dropped entirely. Immediate mode gets a distinct digest to close that.
    //
    // Graceful keeps the ORIGINAL `Value::Null` digest, not a `{"immediate":false}` one (Codex
    // P1, later round): a graceful pause committed before this change carries a key derived from
    // `Null`. Deriving graceful's digest differently now would make every already-committed
    // graceful-pause event's key stop matching what a durable retry computes --
    // `classify_existing_keys` would see the same key PREFIX (same header) but a different
    // digest and return `Divergent` (409) for what is actually the same retried command. Only
    // immediate needs a new digest: it never went through this digest path before #681, so there
    // is no historical event whose key it could stop matching.
    let digest_body = pause_digest_body(immediate);
    let identity = match parse_mutation_headers(
        &headers,
        PAUSE_COMMAND,
        &execution_id,
        &digest_body,
        &["paused"],
    ) {
        Ok(identity) => identity,
        Err(response) => return response,
    };

    let sender = if immediate {
        let cancels = state.cancels.lock().await;
        cancels.get(&execution_id).cloned()
    } else {
        None
    };

    if let Some(sender) = sender {
        let events = state.events.clone();
        let drive_execution_id = execution_id.clone();
        return run_idempotent_mutation(
            &state.events,
            &execution_id,
            PAUSE_COMMAND,
            identity,
            ExecutorWiring::from_state(&state),
            move |actor, key| {
                Box::pin(async move {
                    let signalled_key = key.clone();
                    let signalled_actor = actor.clone();
                    let _ = sender.send(Some(ImmediateCancelRequest {
                        actor,
                        idempotency_key: key,
                    }));
                    // THIS BOUNDS POLL COUNT, NOT WALL TIME, and the difference is deliberate
                    // (peer review of #750). The read below is the UNBOUNDED `execute`, and the
                    // deadline is only consulted BETWEEN iterations -- so the real ceiling is ten
                    // seconds PLUS one read, and a single slow read overruns the stated budget by
                    // however long it takes. #750 was raised with an eighteen-second read.
                    //
                    // The alternative is worse HERE, and only here. A budgeted read would answer
                    // `GHE013_READ_BUDGET_EXCEEDED` on a slow store, and this loop would then have
                    // to report `unknown` for a pause that may well have COMMITTED -- the caller
                    // cannot tell "your interrupt did not land" from "the read after it was slow",
                    // which is the exact ambiguity the unbounded post-mutation render exists to
                    // remove. A wrong answer inside its budget is worse than a right answer late.
                    //
                    // Written down because it was not, and an undocumented trade is a trap: a
                    // bound consulted outside the thing it means to bound looks correct in every
                    // line taken alone, and a grep for either half finds nothing. If this loop
                    // ever needs a real wall-time ceiling, the honest shape is a budget that
                    // reports WHICH of the two happened, not a smaller number here.
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
                    loop {
                        if let Ok(value) =
                            execution::status::execute(&events, Some(drive_execution_id.as_str()))
                            && value.get("status") == Some(&serde_json::json!("paused"))
                            // #681, Codex P1: "paused" is the AGGREGATE status -- it can be true
                            // because a DIFFERENT racing immediate-pause request's payload is what
                            // the driver committed (only one payload survives when two race), OR
                            // because an EARLIER graceful pause already landed while this request's
                            // own signal is still in flight (the drive quiesces slowly with a node
                            // still draining, per #681's own second-round review). Requiring THIS
                            // request's own key on the paused event before reporting success
                            // catches the first; looping instead of failing on a mismatch catches
                            // the second -- a mismatched key is "not yet, keep checking", not "lost
                            // the race", until the deadline below has actually given this request a
                            // fair chance to land.
                            //
                            // Key alone is not enough (Codex P1, later round): `request_digest16`
                            // never folds in actor, so two DIFFERENT actors racing with the same
                            // literal Idempotency-Key header and body derive the identical
                            // `signalled_key`. Comparing actor too closes that gap -- the same
                            // attribution `classify_existing_keys`'s pre-flight already enforces,
                            // now also enforced on the post-append read.
                            //
                            // WHOLE HISTORY, not the latest event (#710, Codex P2): a pause is not
                            // the end of a stream. If this execution pauses under this caller's
                            // key, resumes, and pauses again under another caller's key before this
                            // loop looks, the caller's own event is still durably committed but is
                            // no longer the newest -- and comparing against the newest reported a
                            // conflict for a request that had succeeded.
                            && execution_paused_under(
                                &events,
                                &drive_execution_id,
                                &signalled_key,
                                &signalled_actor,
                            ) == PausedUnderCaller::Committed
                        {
                            return Ok(value);
                        }
                        if std::time::Instant::now() >= deadline {
                            // Two distinct timeout outcomes, not one: a paused event under a
                            // DIFFERENT key at the deadline means SOME OTHER request's own pause
                            // committed first -- a racing immediate request that won the driver's
                            // single-payload capture, OR a graceful pause that landed while this
                            // one's own signal was still draining a node (#695, Codex P1: the
                            // driver deliberately skips its own append rather than double-commit
                            // `ExecutionPaused` in that second case, so this request's own key
                            // never lands either way) -- the same post-append reconciliation every
                            // other mutation's genuine two-writer race already uses. No paused
                            // event that matches at all is the pre-existing #96/#130 case below:
                            // an unknown, because this request's own pause may still record after
                            // the budget elapses.
                            if execution::status::execute(
                                &events,
                                Some(drive_execution_id.as_str()),
                            )
                            .is_ok_and(|value| {
                                value.get("status") == Some(&serde_json::json!("paused"))
                            }) {
                                return Err(execution::Failure {
                                    code: "GHE003_IDEMPOTENCY_CONFLICT",
                                    message: "the execution is already paused under a different \
                                              idempotency key -- a concurrent pause request, \
                                              graceful or racing immediate, committed it first"
                                        .to_owned(),
                                    pointer: "/idempotencyKey".to_owned(),
                                }
                                .into());
                            }
                            // #130, the third class — found while #96 split the other two, and
                            // deliberately NOT split here. This is not a drive failure at all: it
                            // is `pause` waiting for `execution_paused` and running out of budget,
                            // so the operator's question is "did my pause take effect?" — and its
                            // remedy is a third kind, an UNKNOWN rather than a failure, because the
                            // pause may still record after the budget elapses. A failure says act;
                            // an unknown says look.
                            //
                            // NOT contradicted by #96: that change scopes `GHCLI016`'s narrowed
                            // meaning to the start/resume path explicitly, so this site's use stays
                            // a pre-existing approximation rather than becoming a false statement.
                            // Untidy and named, not wrong — a distinction I got wrong in #130's own
                            // opening and corrected there after a reviewer read this constant's doc
                            // instead of my summary of it.
                            //
                            // Left as-is because it belongs to a different command and a different
                            // operator story than #96's split, and widening that change to cover it
                            // would bundle two contracts in one diff. Named here so the next reader
                            // finds a decision instead of an oversight. Reached through the wrapper
                            // now (#681) rather than inline, but the reasoning is unmoved.
                            return Err(pause_outcome_unknown().into());
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                })
            },
        )
        .await;
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
                    // No graph on this door: `POST /pause` carries no `file`, so the hold is
                    // bare-state and the reply's `heldNodesGated: false` says so (#157).
                    //
                    // LIMIT: that flag is reply-only. The Paused events this path appends are
                    // the same wide ones as before. An idempotent retry reconstructs current
                    // status from events; it does not replay a stored response, so it carries no
                    // edge evidence. #157 stays open for a persisted form.
                    None,
                )?)
            })
        },
    )
    .await
}

/// The pause command's idempotency digest body, pulled out of `pause` so its contract can be
/// pinned directly (below) rather than only through the full HTTP handler.
///
/// Graceful MUST stay `Value::Null` -- the digest every already-committed graceful-pause event on
/// a real, durable ledger carries, before this PR existed and unchanged by it. Deriving it any
/// other way (this PR shipped `{"immediate":false}` for one round, Codex P1 caught it) makes every
/// pre-existing graceful-pause event's key stop matching what a durable retry computes post-
/// deploy: `classify_existing_keys` reads the same key PREFIX but a different digest and returns
/// `Divergent` for what is actually the same retried command. Immediate is free to pick anything
/// distinct from `Null` -- it never went through this digest path before #681, so there is no
/// historical event for it to stop matching.
fn pause_digest_body(immediate: bool) -> serde_json::Value {
    if immediate {
        serde_json::json!({ "immediate": true })
    } else {
        serde_json::Value::Null
    }
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
    // #1066: the drive's own copy of the id — the closure below moves what it captures, and the
    // mutation runner still borrows `execution_id` for its own journalling.
    let drive_execution = execution_id.clone();
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
                    let setup =
                        prepare_drive(&drive_state, RESUME_COMMAND, &drive_execution, &payload)
                            .await?;
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

/// `POST /v1/executions/{id}/claim` (#159): testimony that a `waiting_input` node's external
/// work is done. Body `{"file" | "graph", "node", "waitSeq"?, "evidence"?, "asserter"?, "mode"?}`,
/// mirroring the CLI's `execution claim`; `asserter` defaults to the `X-GraphHelm-Actor` header.
///
/// The decision is the verb's (`core/events`), the door is `execution::claim::execute` — the
/// same one the CLI calls — and a refusal is a JOURNAL EVENT with a registry code, so the reply
/// is 200 with `data.claim.outcome == "refused"`, never a 4xx. The graph travels through the
/// same file-trust seam `resume` uses (D2): the node's declared `proofKinds` are read from it.
///
/// One decision event per request, key suffix `claim` (`completion_claimed` OR
/// `completion_refused` — both are the one event this command appends, and the retry proof in
/// `serve/mod.rs` recognizes either).
#[allow(clippy::result_large_err)] // see `start`'s doc comment
pub(super) async fn claim(
    State(state): State<ServeState>,
    UrlPath(execution_id): UrlPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return bad_request(CLAIM_COMMAND, "the request body is not valid JSON", "/"),
    };
    let identity = match parse_mutation_headers(
        &headers,
        CLAIM_COMMAND,
        &execution_id,
        &payload,
        &["claim"],
    ) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let source = match graph_source(&payload, CLAIM_COMMAND) {
        Ok(source) => source,
        Err(response) => return response,
    };
    let Some(node) = payload.get("node").and_then(serde_json::Value::as_str) else {
        return bad_request(
            CLAIM_COMMAND,
            "the request body must carry \"node\"",
            "/node",
        );
    };
    let node = node.to_owned();
    let wait_seq = match payload.get("waitSeq") {
        None | Some(serde_json::Value::Null) => None,
        Some(value) => match value.as_u64() {
            Some(sequence) => Some(sequence),
            None => {
                return bad_request(
                    CLAIM_COMMAND,
                    "\"waitSeq\" must be a non-negative integer",
                    "/waitSeq",
                );
            }
        },
    };
    // The same parser the CLI's `--evidence` file goes through, so both doors accept exactly the
    // same shape and refuse exactly the same way.
    let evidence = match payload.get("evidence") {
        None | Some(serde_json::Value::Null) => Vec::new(),
        Some(value) => match execution::parse_claim_evidence(value) {
            Ok(evidence) => evidence,
            Err(failure) => return respond_failure(CLAIM_COMMAND, failure),
        },
    };
    let asserter = payload
        .get("asserter")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let mode = payload
        .get("mode")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("operator_attested")
        .to_owned();
    let events = state.events.clone();
    let drive_execution_id = execution_id.clone();

    run_idempotent_mutation(
        &state.events,
        &execution_id,
        CLAIM_COMMAND,
        identity,
        ExecutorWiring::from_state(&state),
        |actor, key| {
            Box::pin(async move {
                let version =
                    load_and_publish(&source, CLAIM_COMMAND).map_err(MutationError::Prepared)?;
                let attestation =
                    execution::claim::attestation(asserter.as_deref(), &mode, &actor)?;
                Ok(execution::claim::execute(
                    &version,
                    &events,
                    Some(drive_execution_id.as_str()),
                    &node,
                    wait_seq,
                    evidence,
                    attestation,
                    actor,
                    key,
                )?)
            })
        },
    )
    .await
}

/// `POST /v1/executions/{id}/clear` (#159): countersign an open claim by machine replay. Body
/// `{"file" | "graph", "claimSeq", "manifestHash" | "evidence", "fixtures"?, "route"?,
/// "verifier"?}`, mirroring the CLI's `execution clear`.
///
/// `countersign` is refused AT THE DOOR (D1) — before `run_idempotent_mutation` opens the store,
/// so nothing is appended and no retry marker exists — because the wire carries no signature to
/// verify until D-047 / #529. The verifier is built from what the caller presented BEFORE the
/// store is touched for the same reason; `execution::clear::verifier` is the one place that
/// rule lives, on both doors.
///
/// A CLEARANCE THAT CLEARS DRIVES (D3): after `completion_cleared` folds to `Cleared` the
/// node's dependents are dispatchable and nothing else dispatches them, so this runs the same
/// drive `resume` runs — the async driver when it can run this graph, the CLI's sync drive
/// otherwise — with an EMPTY release set. A rejected clearance drives nothing and replies with
/// the status render plus the `clearance` verdict.
#[allow(clippy::result_large_err)] // see `start`'s doc comment
pub(super) async fn clear(
    State(state): State<ServeState>,
    UrlPath(execution_id): UrlPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return bad_request(CLEAR_COMMAND, "the request body is not valid JSON", "/"),
    };
    let identity = match parse_mutation_headers(
        &headers,
        CLEAR_COMMAND,
        &execution_id,
        &payload,
        &["clear"],
    ) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let source = match graph_source(&payload, CLEAR_COMMAND) {
        Ok(source) => source,
        Err(response) => return response,
    };
    let Some(claim_seq) = payload.get("claimSeq").and_then(serde_json::Value::as_u64) else {
        return bad_request(
            CLEAR_COMMAND,
            "the request body must carry \"claimSeq\" as a non-negative integer",
            "/claimSeq",
        );
    };
    let manifest_hash = match payload.get("manifestHash") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(hash)) => Some(hash.clone()),
        Some(_) => {
            return bad_request(
                CLEAR_COMMAND,
                "\"manifestHash\" must be a string",
                "/manifestHash",
            );
        }
    };
    let evidence = match payload.get("evidence") {
        None | Some(serde_json::Value::Null) => None,
        Some(value) => match execution::parse_claim_evidence(value) {
            Ok(evidence) => Some(evidence),
            Err(failure) => return respond_failure(CLEAR_COMMAND, failure),
        },
    };
    let verifier_kind = payload
        .get("verifier")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("machine_replay");
    // AT THE DOOR: built before any store access, so a `countersign` request appends nothing.
    let verifier = match execution::clear::verifier(
        verifier_kind,
        manifest_hash.as_deref(),
        evidence.as_deref(),
    ) {
        Ok(verifier) => verifier,
        Err(failure) => return respond_failure(CLEAR_COMMAND, failure),
    };
    let fixtures = payload
        .get("fixtures")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from);
    let drive_state = state.clone();
    // #1066: the drive's own copy of the id — the closure below moves what it captures, and the
    // mutation runner still borrows `execution_id` for its own journalling.
    let drive_execution = execution_id.clone();
    let drive_execution_id = execution_id.clone();

    run_idempotent_mutation(
        &state.events,
        &execution_id,
        CLEAR_COMMAND,
        identity,
        ExecutorWiring::from_state(&state),
        |actor, key| {
            Box::pin(async move {
                let version =
                    load_and_publish(&source, CLEAR_COMMAND).map_err(MutationError::Prepared)?;
                // See `start`'s matching branch for why the async drive is conditional.
                if drive_is_viable_for(&drive_state, &version.graph().spec) {
                    // #83: the drive's fallible setup runs FIRST, so the clearance is the last
                    // thing that can fail rather than the first thing that commits.
                    let setup =
                        prepare_drive(&drive_state, CLEAR_COMMAND, &drive_execution, &payload)
                            .await?;
                    let (outcome, prepared) = execution::clear::decide(
                        &version,
                        &drive_state.events,
                        fixtures.as_deref(),
                        Some(drive_execution_id.as_str()),
                        claim_seq,
                        &verifier,
                        actor,
                        key,
                    )?;
                    let mut value = if outcome == ClearanceOutcome::Cleared {
                        drive(&drive_state, &drive_execution_id, prepared, setup).await?
                    } else {
                        // Rejected: nothing to drive. The reply is the same status render every
                        // other mutation replies with, plus the verdict.
                        execution::status::execute(
                            &drive_state.events,
                            Some(drive_execution_id.as_str()),
                        )?
                    };
                    execution::clear::annotate(&mut value, &outcome, claim_seq);
                    Ok(value)
                } else {
                    Ok(execution::clear::execute(
                        &version,
                        &drive_state.events,
                        fixtures.as_deref(),
                        Some(drive_execution_id.as_str()),
                        claim_seq,
                        &verifier,
                        actor,
                        key,
                    )?)
                }
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
const DRIVER_FAILURE_CODE: &str = crate::error_codes::GHCLI016_DRIVER_FAILURE;

/// #96. A refusal from [`prepare_drive`] — raised BEFORE the start/resume decision commits, so
/// **nothing was written and the operator's hold is exactly where they left it.** Fix the
/// environment and retry; you are where you were.
///
/// The distinction this carries is not new logic. #83's hoist already made it structural: every
/// site that raises this runs before `execute_prepared`, and every site that raises
/// [`DRIVER_FAILURE_CODE`] on this path runs after it. Until now both answered the same value, so
/// the response destroyed a distinction the code already had — "the call failed" and "your hold
/// still holds" are one fact to an operator, and one code for both made them two.
const SETUP_FAILURE_CODE: &str = crate::error_codes::GHCLI019_DRIVER_SETUP;

/// #130. The immediate-`pause` budget elapsed without `execution_paused` appearing. This is the
/// THIRD operator question on this surface, and its remedy is neither of the two #96 split:
///
/// | path | the operator question | what they should do |
/// |---|---|---|
/// | setup refusal | did anything commit? (no) | fix the environment and retry |
/// | mid-drive failure | did anything commit? (yes) | the execution is attended; do not re-pause |
/// | **this** | **did my pause take effect?** | **go and look** |
///
/// **UNKNOWN, not FAILURE, and the distinction is the whole point.** The pause may still record
/// after the budget elapses: the cancel was sent, the drive is folding, and `execution_paused`
/// lands once every in-flight node is recorded `Interrupted`. Answering `GHCLI016` here tells the
/// operator a drive failed, which is a claim about something that did not happen -- nothing about
/// a drive is being reported. A failure says ACT; an unknown says LOOK.
///
/// This is not a correction of #96. That change scopes `GHCLI016` narrowed meaning to the
/// start/resume path explicitly, so this site use was a pre-existing approximation rather than a
/// false statement -- a distinction #130 own opening got wrong and corrected in the issue after a
/// reviewer read the constant doc instead of the summary of it. What is fixed here is the
/// flattening: one code for a question whose remedy differs.
///
/// It is also the shape the poll above already asks for. The comment at the deadline says that if
/// this loop ever needs a real wall-time ceiling, "the honest shape is a budget that reports WHICH
/// of the two happened, not a smaller number here." This is that shape for the case that exists.
const PAUSE_OUTCOME_UNKNOWN_CODE: &str = crate::error_codes::GHCLI025_PAUSE_OUTCOME_UNKNOWN;

/// The immediate-pause budget answer, message included, so the sentence has ONE producer.
///
/// No `message` parameter, deliberately: the wording is the contract here, and a caller free to
/// pass its own could reintroduce "did not record" phrasing that reads as a failure under a code
/// that says unknown. One producer also makes the arming site testable -- a cell asserts the
/// sentence occurs exactly once in this file, so re-inlining it beside `driver_failure` reddens.
fn pause_outcome_unknown() -> execution::Failure {
    execution::Failure {
        code: PAUSE_OUTCOME_UNKNOWN_CODE,
        message: "the immediate-stop budget elapsed before execution_paused was observed; the pause may still record -- read the execution to see whether it did".to_owned(),
        pointer: "/execution".to_owned(),
    }
}

fn driver_failure(message: &str) -> execution::Failure {
    execution::Failure {
        code: DRIVER_FAILURE_CODE,
        message: message.to_owned(),
        pointer: "/execution".to_owned(),
    }
}

/// The directory names the context walk never enters and the runtime never reads through, at
/// any depth (`source_channel::CREDENTIAL_DIRS`, `context::sensitive_path`): a protected
/// directory inside the project is shielded from the walk only when its path below the root
/// carries one of them. Case-folded, as both of those are.
const WALK_SHIELDING_SEGMENTS: [&str; 2] = [".graphhelm", "keyring"];

/// #1065: refuse a project root that IS, or lies INSIDE, a protected directory (the keyring, the
/// broker), because the context ports would search and read it as repository evidence — and
/// refuse the REVERSE when the protected directory lies inside the project on a path the walk
/// would enter.
///
/// Compared canonical against canonical, so a relative spelling or a link cannot dodge the rule.
/// A project that cannot be canonicalized is left to the builders after this check, which refuse
/// it with their own reason (not a directory, not readable); a protected directory that cannot
/// be canonicalized is compared as spelled.
///
/// The reverse direction is refused by what the walk does, not by containment alone: the
/// documented default layout is `<project>/.graphhelm/keyring`, and the walk skips a
/// `.graphhelm` or `keyring` segment at any depth, so a protected directory whose relative path
/// carries one of [`WALK_SHIELDING_SEGMENTS`] stays allowed. One at `<project>/credentials`
/// carries neither: its `<id>.json` files end in a text suffix and hold base64 that matches no
/// secret shape, so every per-file refusal is blind to them and the walk would cite them. Both
/// messages name the direction and never the path.
fn refuse_protected_project(project: &Path, protected: &[PathBuf]) -> Result<(), String> {
    let Ok(canonical) = project.canonicalize() else {
        return Ok(());
    };
    for path in protected {
        let path = path.canonicalize().unwrap_or_else(|_| path.clone());
        if canonical.starts_with(&path) {
            return Err(
                "the project root must not be, or lie inside, the keyring or broker directory"
                    .to_owned(),
            );
        }
        if let Ok(relative) = path.strip_prefix(&canonical) {
            let shielded = relative.components().any(|component| match component {
                std::path::Component::Normal(name) => name.to_str().is_some_and(|name| {
                    WALK_SHIELDING_SEGMENTS.contains(&name.to_lowercase().as_str())
                }),
                _ => false,
            });
            if !shielded {
                return Err(
                    "the keyring or broker directory must not lie inside the project root on a path the context walk enters: below the root its path needs a `.graphhelm` or `keyring` segment"
                        .to_owned(),
                );
            }
        }
    }
    Ok(())
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

/// The runtime-backed ports, built once and moved into the executor after the commit. Each
/// half is present exactly when its wiring is (#1066): `model` with `{manifest, broker, route}`,
/// `tools` with `{staging, allow-program}`; `drive` composes the executor from what is here.
struct PreparedPorts {
    model: Option<(ServeModelPort, String)>,
    tools: Option<(ServeToolPort, ToolLease)>,
    /// #1065: the bounded search and reader over the project root this drive resolved, and the
    /// ledger the drive reply publishes from. Present only when the MODEL half is wired: the
    /// capsule is a prompt field, and a tools-only server answers its cognitive nodes from node
    /// fixtures, which read no prompt and seal no provenance — compiling a capsule there would
    /// publish `context.nodes` for nodes that never saw one. The root is the one the tool half is
    /// built on when there is one, and the same three-deep resolution (request, `--project`,
    /// working directory) when there is not.
    context: Option<graphhelm_runtime::context::ContextPorts>,
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
async fn resolve_requested_route(
    wiring: &ModelWiring,
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

    let manifest = reread_manifest(wiring).await?;

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

/// The manifest, re-read from the file `--manifest` named, for every per-request route lookup.
///
/// The re-read is BOUNDED and regular-file-only -- the same reader `serve` started on and
/// `gateway routes` uses -- and it runs off the reactor (#559): before this it was an
/// unbounded `std::fs::read` on the request's own task.
///
/// The path is NOT in any message. `manifest_path` is a filesystem location on the server,
/// which a caller with a bearer token is not owed; the reply is a diagnostic and the operator
/// already knows what they passed to `--manifest`.
#[allow(clippy::result_large_err)] // `MutationError::Prepared` carries a built response
async fn reread_manifest(wiring: &ModelWiring) -> Result<RouteManifest, MutationError> {
    let manifest_path = wiring.manifest_path.clone();
    let read = off_reactor(move || crate::commands::gateway::read_bounded_manifest(&manifest_path))
        .await
        .ok_or_else(|| MutationError::from(setup_failure("the manifest read task failed")))?;
    let bytes = read.map_err(|error| {
        MutationError::from(setup_failure(match error {
            crate::commands::gateway::ManifestReadError::Unreadable => {
                "the configured manifest could not be read"
            }
            crate::commands::gateway::ManifestReadError::TooLarge => {
                "the configured manifest exceeds the maximum supported size"
            }
        }))
    })?;
    let text = String::from_utf8(bytes).map_err(|_| {
        MutationError::from(setup_failure("the configured manifest is not valid UTF-8"))
    })?;
    RouteManifest::from_json(&text)
        .map_err(|error| MutationError::from(setup_failure(&error.to_string())))
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
    execution_id: &str,
    payload: &serde_json::Value,
) -> Result<DriveSetup, MutationError> {
    let sealer = build_sealer(state.sealing.as_deref())
        .map_err(|message| MutationError::from(setup_failure(&message)))?;
    let ports = match &state.runtime {
        Some(wiring) => {
            let model = match wiring.model.as_ref() {
                Some(model_wiring) => {
                    let route = resolve_requested_route(model_wiring, command, payload).await?;
                    let port = ServeModelPort::build(wiring, model_wiring, &route)
                        .await
                        .map_err(|message| MutationError::from(setup_failure(&message)))?;
                    // The route the drive RESOLVED, never the startup default again: the
                    // executor stamps this id onto the work it dispatches, so a stale default
                    // here would label every call with a model that did not answer it.
                    Some((port, route.id().to_owned()))
                }
                None => None,
            };
            // Three-deep fallback (issue #82): the caller's own `"project"` wins when given (no
            // MCP tool currently exposes this field, but the raw HTTP body always could); absent
            // that, the deployer's own `--project` default (set once, the same way `--staging`
            // itself is, so it lives on the tool half); only when NEITHER is configured does
            // this fall back to the server process's own working directory — the default that
            // collides with `--staging` whenever `serve` happens to run from a `--staging`
            // ancestor, which is exactly what #82 documents. A deployer who hits that collision
            // fixes it once with `--project`; nothing changes for a deployment that never had
            // the collision. Resolved ONCE, here, because the tool workspace and the context
            // ports (#1065) must be built over the same root.
            //
            // A PRESENT-BUT-UNUSABLE `"project"` IS REFUSED, the same way `"route"` is: the
            // `and_then(Value::as_str)` spelling folds "absent" and "present but not a string"
            // into one `None`, and the `None` branch searches and reads the DEFAULT project —
            // the caller named a directory and a different tree was cited back as evidence.
            let requested_project = match payload.get("project") {
                None | Some(serde_json::Value::Null) => None,
                Some(serde_json::Value::String(path)) => Some(PathBuf::from(path)),
                Some(_) => {
                    return Err(MutationError::Prepared(bad_request(
                        command,
                        "\"project\" must be a string naming a directory",
                        "/project",
                    )));
                }
            };
            let project = requested_project
                .or_else(|| wiring.tools.as_ref().and_then(|tools| tools.project.clone()))
                .or_else(|| std::env::current_dir().ok())
                .ok_or_else(|| {
                    MutationError::from(setup_failure(
                        "no \"project\" was given, no --project default is configured, and the server's working directory could not be read",
                    ))
                })?;
            // The directories the workspace must never overlap: the keyring always, the broker
            // when a model half holds one (#1066 — a tools-only server has no broker directory
            // to protect).
            let mut protected = vec![wiring.keyring_dir.clone()];
            if let Some(model_wiring) = wiring.model.as_ref() {
                protected.push(model_wiring.broker_dir.clone());
            }
            // #1065: the project root is what the context ports SEARCH and READ, and the request
            // body may name any directory — the keyring itself included, whose `<id>.json` files
            // carry no `keyring` segment in their RELATIVE names, end in a text suffix and hold
            // base64 that matches no secret shape. Refused before a port is built over it. The
            // reverse — the keyring inside the project, which the default
            // `<project>/.graphhelm/keyring` is — stays allowed: the walk skips those names.
            refuse_protected_project(&project, &protected)
                .map_err(|message| MutationError::from(setup_failure(&message)))?;
            let tools = match wiring.tools.as_ref() {
                Some(tool_wiring) => {
                    let port =
                        ServeToolPort::build(tool_wiring, &protected, &project, execution_id)
                            .map_err(|message| MutationError::from(setup_failure(&message)))?;
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
                        programs: tool_wiring.allow_programs.iter().cloned().collect(),
                    };
                    Some((port, lease))
                }
                None => None,
            };
            // #1065: context ports only beside a real model — see `PreparedPorts::context`.
            // #1086: with a tool half, the execution's own Tier 1 tree is offered to the compile,
            // so a cognitive node after a tool node reads the tool's work, not the checkout.
            let context = match model {
                Some(_) => Some(
                    super::ports::build_context_ports(
                        &project,
                        tools.as_ref().map(|(port, _)| port.context_tree()),
                    )
                    .map_err(|message| MutationError::from(setup_failure(&message)))?,
                ),
                None => None,
            };
            Some(PreparedPorts {
                model,
                tools,
                context,
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

    // The gates THIS binary carries. Built once and handed to both the executor and the driver:
    // core never depends on `tools`, so the pathogen suites, their digests and their evaluators
    // all arrive here as one piece of configuration.
    let gates: Arc<dyn graphhelm_runtime::ports::GateRegistryPort> =
        Arc::new(crate::commands::quality::RegisteredGates);
    // #1065: the context ports travel with the real executor and nowhere else. A fixture drive
    // has no project root to search, so it runs exactly as before and its reply says so.
    let context_ports = ports.as_ref().and_then(|ports| ports.context.clone());
    // The workspace release handle is taken BEFORE the tool port moves into the executor and
    // used AFTER the drive returns, whatever it returned (#1066): the execution's Tier 1 tree
    // lives exactly as long as this drive, and the ref it landed outlives it.
    let release: Option<WorkspaceRelease> = ports
        .as_ref()
        .and_then(|ports| ports.tools.as_ref())
        .map(|(port, _)| port.releaser());
    let fixtures = || {
        let fixtures = FixtureExecutor::new(prepared.fixtures.clone());
        Arc::new(FixtureAsyncExecutor::new(fixtures)) as Arc<dyn AsyncNodeExecutor>
    };
    let executor: Arc<dyn AsyncNodeExecutor> = match ports {
        // Both halves real: the all-real composition, unchanged.
        Some(PreparedPorts {
            model: Some((model, route_id)),
            tools: Some((tools, lease)),
            context: _,
        }) => Arc::new(PortExecutor {
            model: Arc::new(model),
            tools: Arc::new(tools),
            route_id,
            lease,
            actor: "runtime".to_owned(),
            // #668: the SAME registry object the drive call below reads digests from, so a
            // node cannot be certified against one gate and judged by another.
            gates: gates.clone(),
        }),
        // One half real (#1066): that half's executor for its kind, the fixture executor for
        // the other — so a tools-only deployment runs a real `apply_patch`/`tests`/`commit`
        // while its cognitive nodes are answered by node fixtures exactly as before, and a
        // model-only deployment runs a real model while its tool nodes are.
        Some(PreparedPorts {
            model,
            tools,
            context: _,
        }) => {
            let cognitive: Arc<dyn AsyncNodeExecutor> = match model {
                Some((model, route_id)) => Arc::new(ModelExecutor {
                    model: Arc::new(model),
                    route_id,
                }),
                None => fixtures(),
            };
            let tool: Arc<dyn AsyncNodeExecutor> = match tools {
                Some((tools, lease)) => Arc::new(ToolExecutor {
                    tools: Arc::new(tools),
                    lease,
                    actor: "runtime".to_owned(),
                }),
                None => fixtures(),
            };
            Arc::new(SplitExecutor {
                cognitive,
                tool,
                gates: gates.clone(),
            })
        }
        None => fixtures(),
    };

    let executor: Arc<dyn AsyncNodeExecutor> = if state.sealing.is_some()
        && state
            .runtime
            .as_ref()
            .is_some_and(|wiring| wiring.model.is_some())
    {
        Arc::new(super::notices::OwnerNoticeExecutor::new(
            executor,
            state.events.to_path_buf(),
            state.sealing.clone(),
        ))
    } else {
        executor
    };

    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(None::<ImmediateCancelRequest>);
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
        // The certified-or-not-at-all precondition compares each fold receipt against the
        // digest of THAT gate's own suite, read from this registry (#668). It used to be one
        // digest -- geometry's -- for every gate.
        Some(gates),
        context_ports.clone(),
    )
    .await;

    state.cancels.lock().await.remove(execution_id);

    // #1065: the per-node context summaries this drive compiled, read back from the ledger the
    // driver wrote as it went. Content-free (paths, counts, digests). Only THIS reply — the one
    // door that held the ports — can publish them without opening sealed evidence; every other
    // door renders the projection alone and says the field is unavailable there.
    let context = context_ports.map(|ports| ports.ledger.snapshot());

    // The tree goes, the ref stays (#1066). Off the reactor: it is a `git worktree remove`. A
    // removal failure is reported over a drive that otherwise succeeded, because a leaked
    // workspace is a leaked write capability and "completed" must not also mean "clean" when it
    // is not; a drive that already failed keeps its own error, which is the one the operator
    // needs first.
    if let Some(release) = release {
        let released = tokio::task::spawn_blocking(move || release.release())
            .await
            .unwrap_or_else(|_| Err("the workspace release task failed".to_owned()));
        if let (Err(message), Ok(_)) = (&released, &result) {
            return Err(MutationError::from(driver_failure(message)));
        }
    }

    match result {
        Ok(projection) => Ok(execution::render_with_context(
            &projection,
            // This path holds a projection and no history: it states that it measured
            // nothing instead of implying calm. The BUDGET is not part of that absence --
            // it is declared in the graph this projection holds, and `default()` claimed
            // otherwise (#1013).
            &graphhelm_execution::AttentionInputs::for_surface(
                &projection,
                std::collections::BTreeMap::new(),
                None,
            ),
            &execution::Liveness::default(),
            // #134: no graph on this path either -- `ServeState` holds one, but publishing the
            // dispatch view here and not from the CLI command would split the two surfaces'
            // replies; the follow-up adds it to both from the same source.
            None,
            context.as_ref(),
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
const GATEWAY_ROUTE_SET_COMMAND: &str = "gateway.route.set";
const GATEWAY_CREDENTIAL_SET_COMMAND: &str = "gateway.credential.set";

/// The Task 0 reconciled decision: a server launched with `--manifest` serves that manifest
/// by default and treats the `manifest` query param as an explicit override. A fixture-only
/// server (no `--manifest`) asked for its routes WITHOUT the query param answers 200 with
/// `configured: false` (#1083 F6) - see `gateway_routes`; `probe`, which needs a route to act on,
/// still answers 400 naming the parameter.
fn effective_manifest(state: &ServeState, query_manifest: Option<PathBuf>) -> Option<PathBuf> {
    query_manifest.or_else(|| {
        state
            .runtime
            .as_ref()
            .and_then(|wiring| wiring.model.as_ref())
            .map(|model| model.manifest_path.clone())
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
///
/// **No manifest anywhere is an answer, not a refusal (#1083 F6).** A server started without
/// `--manifest`, asked without the `manifest` query param, replies `200` with
/// `{"configured": false, "routes": [], "reason": ...}`. It used to reply `400`, and every browser
/// that connected to a fixture-only Runtime logged that 400 as a red console error the page could
/// not suppress - for a question whose true answer is simply "no routes". A manifest that IS named
/// (flag or query) but cannot be loaded keeps its refusal: that is a configuration error the
/// operator can fix, and reading it as "no routes" would hide it.
pub(super) async fn gateway_routes(
    State(state): State<ServeState>,
    RawQuery(query): RawQuery,
) -> Response {
    let query = query.unwrap_or_default();
    let manifest = query_pairs(&query)
        .find(|(key, _)| *key == "manifest")
        .map(|(_, value)| PathBuf::from(value));
    let Some(manifest) = effective_manifest(&state, manifest) else {
        return respond(
            StatusCode::OK,
            Outcome::success(
                GATEWAY_ROUTES_COMMAND,
                serde_json::json!({
                    "configured": false,
                    "routes": [],
                    "reason": "no gateway manifest is configured on this server",
                }),
            )
            .output,
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
        broker = broker.or_else(|| wiring.model.as_ref().map(|model| model.broker_dir.clone()));
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
        // #1083 F1: an execution that does not exist is a 404, the answer `status` and `briefing`
        // give for the same id - not "that evidence is not referenced", which reads as a run
        // that exists and simply never produced it.
        if history.is_empty() {
            return Err(execution::not_found());
        }
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
/// `text/plain`. A structured-syntax `+json` type is JSON too (RFC 6839): the sealed
/// `context-provenance@1` record (`application/vnd.graphhelm.context-provenance+json`) and the
/// accounting receipt are documents an operator must be able to open, not only count (#1086).
/// Anything else is refused rather than guessed at: a surface that base64s unknown bytes into a
/// JSON string is not showing an operator their content, it is moving it.
fn renders_as_text(media_type: &str) -> bool {
    media_type == "application/json"
        || (media_type.starts_with("application/") && media_type.ends_with("+json"))
        || media_type.starts_with("text/")
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
                crate::error_codes::GHCLI023_EVIDENCE_UNREADABLE,
                message,
                "/evidence",
                SOURCE,
            )],
        )
        .output,
    )
}

#[cfg(test)]
mod pause_digest_body_tests {
    use super::pause_digest_body;

    /// Pinned, not inferred: this is the one fact the backward-compatibility claim in
    /// `pause_digest_body`'s own doc comment depends on. A real end-to-end proof (commit a
    /// graceful-pause event under the pre-#681 digest, retry it against this code, confirm
    /// `Complete` rather than `Divergent`) needs a durable ledger event that predates this code by
    /// construction -- not reproducible inside one process running one build. This pins the
    /// narrower, checkable fact instead: change graceful's digest body away from `Null` and this
    /// goes red before it ever reaches a real deploy's retries.
    #[test]
    fn graceful_digest_body_is_pinned_to_null() {
        assert_eq!(pause_digest_body(false), serde_json::Value::Null);
    }

    /// Immediate's digest must differ from graceful's -- the whole point of #681's mode-aware fix
    /// (a shared digest let a graceful-then-immediate retry classify `Complete` against the
    /// graceful commit and silently drop the immediate stop).
    #[test]
    fn immediate_digest_body_differs_from_graceful() {
        assert_ne!(pause_digest_body(true), pause_digest_body(false));
    }
}

/// #559: the three handlers that did synchronous file or replay work on the reactor, each held
/// to `off_reactor` by the witness -- not by timing. A current-thread runtime is built per cell,
/// the runs recorded before and after are compared, and the thread the new run happened on must
/// not be the runtime's own. Serialised through one lock because the witness is process-wide.
#[cfg(test)]
mod off_reactor_tests {
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    use axum::body::Bytes;
    use axum::http::StatusCode;

    use super::{
        MutationError, graph_topology, list_executions_over, off_reactor, off_reactor_witness,
        resolve_requested_route,
    };
    use crate::commands::serve::ports::{ModelWiring, RuntimeWiring, ToolWiring};

    static SERIAL: Mutex<()> = Mutex::new(());

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    /// Runs `future` on a fresh current-thread runtime and returns its output together with the
    /// witness's new runs -- the threads work went through `off_reactor` on during the call.
    fn measured<T>(
        future: impl std::future::Future<Output = T>,
    ) -> (T, Vec<std::thread::ThreadId>) {
        let before = off_reactor_witness::runs().len();
        let output = runtime().block_on(future);
        let runs = off_reactor_witness::runs();
        (output, runs[before..].to_vec())
    }

    fn example_graph() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/graphs/manual-override-deploy.yaml")
    }

    async fn body_text(response: axum::response::Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    /// The helper's own contract, with a closure that can observe its thread: the work runs
    /// somewhere that is not the reactor thread, and its value comes back.
    #[test]
    fn off_reactor_runs_the_work_on_another_thread_and_returns_its_value() {
        let _serial = SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let reactor = std::thread::current().id();
        let (value, runs) = measured(off_reactor(move || (std::thread::current().id(), 41 + 1)));
        let (worker, answer) = value.expect("the blocking task completes");
        assert_eq!(answer, 42);
        assert_ne!(worker, reactor, "the work ran ON the reactor thread");
        assert_eq!(
            runs,
            vec![worker],
            "the witness saw exactly this run, on the worker"
        );
    }

    #[test]
    fn the_topology_read_goes_through_off_reactor() {
        let _serial = SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let reactor = std::thread::current().id();
        let body = serde_json::json!({ "file": example_graph().to_str().unwrap() }).to_string();
        let (response, runs) = measured(graph_topology(Bytes::from(body)));
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            runs.len(),
            1,
            "the topology read did not go through off_reactor"
        );
        assert_ne!(
            runs[0], reactor,
            "the topology read ran on the reactor thread"
        );
    }

    /// The regular-file refusal arrives as the same 400 any other graph diagnostic does, and
    /// names the rule; a directory is the platform-neutral non-regular path.
    #[test]
    fn a_topology_read_of_a_directory_is_a_graph_diagnostic_not_a_hang_or_a_500() {
        let _serial = SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("graph.yaml");
        std::fs::create_dir(&path).unwrap();
        let body = serde_json::json!({ "file": path.to_str().unwrap() }).to_string();
        let (text, _) = measured(async {
            let response = graph_topology(Bytes::from(body)).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            body_text(response).await
        });
        assert!(
            text.contains(graphhelm_schema::NOT_A_REGULAR_FILE),
            "the reply must name the regular-file rule: {text}"
        );
    }

    #[test]
    fn the_execution_index_goes_through_off_reactor() {
        let _serial = SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let reactor = std::thread::current().id();
        let directory = tempfile::tempdir().unwrap();
        let events: Arc<Path> = Arc::from(directory.path().join("events"));
        drop(crate::commands::event_store(&events).unwrap());
        let (text, runs) = measured(async {
            let response = list_executions_over(events, "").await;
            assert_eq!(response.status(), StatusCode::OK);
            body_text(response).await
        });
        assert!(text.contains("\"executions\":[]"), "{text}");
        assert_eq!(
            runs.len(),
            1,
            "the execution index did not go through off_reactor"
        );
        assert_ne!(
            runs[0], reactor,
            "the execution index replayed on the reactor thread"
        );
    }

    fn wiring_for(manifest_path: PathBuf) -> RuntimeWiring {
        let manifest =
            graphhelm_gateway::manifest::RouteManifest::from_json(&manifest_json()).unwrap();
        let route = super::find_route(&manifest, "anthropic_byok").unwrap();
        RuntimeWiring {
            model: Some(ModelWiring {
                manifest_path,
                route,
                broker_dir: PathBuf::from("unused"),
            }),
            tools: Some(ToolWiring {
                staging: PathBuf::from("unused"),
                project: None,
                tests_runner: "unused".to_owned(),
                allow_programs: Vec::new(),
                path_prepend: Vec::new(),
            }),
            keyring_dir: PathBuf::from("unused"),
            key_id: "unused".to_owned(),
        }
    }

    fn manifest_json() -> String {
        serde_json::json!({
            "manifestVersion": 1,
            "routes": [{
                "id": "anthropic_byok",
                "provider": "anthropic",
                "transport": "direct_api",
                "authentication": "api_key",
                "billingMode": "per_token",
                "baseUrl": "https://api.anthropic.com",
                "model": "claude-sonnet-5",
                "credentialRef": "cred_anthropic",
                "profiles": ["critical_reasoning"],
                "enabled": true
            }]
        })
        .to_string()
    }

    fn setup_message(error: MutationError) -> String {
        match error {
            MutationError::Command(failure) => failure.message,
            MutationError::Prepared(_) => panic!("expected a setup failure, got a prepared reply"),
        }
    }

    #[test]
    fn the_manifest_re_read_goes_through_off_reactor() {
        let _serial = SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let reactor = std::thread::current().id();
        let directory = tempfile::tempdir().unwrap();
        let manifest_path = directory.path().join("manifest.json");
        std::fs::write(&manifest_path, manifest_json()).unwrap();
        let wiring = wiring_for(manifest_path);
        let payload = serde_json::json!({ "route": "anthropic_byok" });
        let (route, runs) = measured(resolve_requested_route(
            wiring.model.as_ref().unwrap(),
            "execution.start",
            &payload,
        ));
        let Ok(route) = route else {
            panic!("the route resolves");
        };
        assert_eq!(route.id(), "anthropic_byok");
        assert_eq!(
            runs.len(),
            1,
            "the manifest re-read did not go through off_reactor"
        );
        assert_ne!(
            runs[0], reactor,
            "the manifest was re-read on the reactor thread"
        );
    }

    /// The re-read is bounded the way the startup read is: a manifest grown past
    /// `MAX_MANIFEST_BYTES` after `serve` started is refused, not read whole.
    #[test]
    fn an_oversize_manifest_is_refused_by_the_re_read_without_being_read() {
        let _serial = SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let directory = tempfile::tempdir().unwrap();
        let manifest_path = directory.path().join("manifest.json");
        let file = std::fs::File::create(&manifest_path).unwrap();
        file.set_len(graphhelm_gateway::manifest::MAX_MANIFEST_BYTES as u64 + 1)
            .unwrap();
        drop(file);
        let wiring = wiring_for(manifest_path);
        let payload = serde_json::json!({});
        let (outcome, _) = measured(resolve_requested_route(
            wiring.model.as_ref().unwrap(),
            "execution.start",
            &payload,
        ));
        let Err(error) = outcome else {
            panic!("an oversize manifest is refused");
        };
        let message = setup_message(error);
        assert_eq!(
            message,
            "the configured manifest exceeds the maximum supported size"
        );
    }

    #[test]
    fn a_manifest_path_that_is_a_directory_is_not_readable_to_the_re_read() {
        let _serial = SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let directory = tempfile::tempdir().unwrap();
        let wiring = wiring_for(directory.path().to_path_buf());
        let payload = serde_json::json!({});
        let (outcome, _) = measured(resolve_requested_route(
            wiring.model.as_ref().unwrap(),
            "execution.start",
            &payload,
        ));
        let Err(error) = outcome else {
            panic!("a directory is refused");
        };
        let message = setup_message(error);
        assert_eq!(message, "the configured manifest could not be read");
    }
}

/// #130: the immediate-pause budget answers UNKNOWN, and the site actually asks it.
#[cfg(test)]
mod pause_outcome_tests {
    use super::{
        DRIVER_FAILURE_CODE, PAUSE_OUTCOME_UNKNOWN_CODE, driver_failure, pause_outcome_unknown,
    };

    /// The failure this budget produces carries its own code, and the WORDS match it. A code that
    /// says unknown under a message that says "did not record" would put the flattening back one
    /// layer down, where a reader takes the sentence and not the code.
    #[test]
    fn the_pause_budget_answers_unknown_rather_than_driver_failure() {
        let failure = pause_outcome_unknown();
        // Against the CONSTANT, not a literal, and that is not a tautology here: the registry
        // owns the other half. `error_codes::tests::every_code_string_is_registered_once`
        // asserts each constant name EQUALS its own string, and
        // `the_registry_is_the_only_source_of_code_literals` refuses a `"GHCLI###_` literal
        // anywhere else under src/ -- it caught the first draft of this cell, which spelled
        // three of them. So the wire value is pinned there and the PATH is pinned here.
        assert_eq!(failure.code, PAUSE_OUTCOME_UNKNOWN_CODE);
        assert_eq!(failure.pointer, "/execution");
        assert!(
            failure.message.contains("may still record"),
            "the code says unknown while the sentence says failed: {}",
            failure.message
        );
        assert!(
            !failure.message.contains("did not record"),
            "the old wording asserts a negative the budget cannot establish: {}",
            failure.message
        );
    }

    /// CONTROL, and it is what makes the assertion above a claim about THIS path rather than about
    /// the file: the sibling wrapper did not move. Without it, renaming `GHCLI016` everywhere would
    /// leave the cell above green while destroying the distinction it exists to protect.
    #[test]
    fn the_mid_drive_failure_still_answers_ghcli016() {
        assert_eq!(driver_failure("x").code, DRIVER_FAILURE_CODE);
        assert_ne!(PAUSE_OUTCOME_UNKNOWN_CODE, DRIVER_FAILURE_CODE);
    }

    /// THE ARMING SITE. A correct wrapper nobody calls is the failure mode a slice test cannot see,
    /// and this file is where the call has to be. Read from source because the budget itself has no
    /// seam: the deadline is a hard-coded ten seconds and reaching it end to end would need a live
    /// drive that never appends `execution_paused`. That limit is declared rather than papered over.
    ///
    /// The sentence has ONE producer by construction (`pause_outcome_unknown` takes no message), so
    /// re-inlining it beside `driver_failure` makes the count two and reddens here.
    #[test]
    fn the_immediate_pause_site_asks_the_unknown_wrapper() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/commands/serve/routes.rs");
        let text = std::fs::read_to_string(&path).expect("this source file is readable");
        assert!(
            text.len() > 10_000,
            "ARRANGEMENT: read {} bytes of {} -- the scan, not the code, is wrong",
            text.len(),
            path.display()
        );
        // THE NEEDLE IS SPLIT, and it has to be: the first run of this cell failed with
        // `left: 2, right: 1` because the search string was itself a literal in this file --
        // the instrument counted itself. A source-scanning cell must not contain the string it
        // scans for. `concat!` joins at compile time, so the joined sentence exists only in
        // `pause_outcome_unknown` while this file holds the two halves separately.
        let sentence = concat!("the immediate-stop budget ", "elapsed before");
        assert_eq!(
            text.matches(sentence).count(),
            1,
            "the budget sentence must have exactly one producer"
        );
        // THE CALL AT THE SITE, not an occurrence count. The first version of this asserted
        // `text.matches("pause_outcome_unknown()").count() >= 2`, and X measured that the
        // substring appears FOUR times -- the site, the `fn` definition, this cell's own call and
        // the needle itself -- so swapping the site for `driver_failure("x")` left 3 and every
        // cell green. The sabotage that reddened was the one that put the RETIRED WORDING back,
        // which is a different property: "the old sentence is gone" is not "the new wrapper is
        // called". A guard that passes for the wrong reason is worse than a missing one, because
        // its green is read as coverage.
        //
        // Split, as above, so the joined form exists only at the site.
        let armed = concat!("Err(pause_outcome_unknown", "().into())");
        assert_eq!(
            text.matches(armed).count(),
            1,
            "the immediate-pause budget must return the unknown wrapper, once"
        );
        let retired = concat!("did not record ", "execution_paused");
        assert!(
            !text.contains(retired),
            "the old driver-failure wording is still in this file"
        );
    }
}

/// `PUT /v1/gateway/routes`: the HTTP half of `gateway route set` (#1171).
///
/// D-039's rule is why this exists at all: a mutation reachable from the CLI and not from the API
/// is a second operational path, and the Studio can only reach the API. The handler owns the
/// request's shape and nothing else — the write, its refusals and its atomicity are
/// `commands::gateway::route`'s, the same function the CLI verb calls.
///
/// **A wrongly-typed field is refused, never coerced.** `payload.get("enabled").and_then(as_bool)`
/// folds "absent" and "present but not a boolean" into one answer, and the second is an operator
/// who asked for something this surface would then silently do differently — the same trap
/// `resolve_requested_route` names for `"route"`. Absent means the documented default; present and
/// wrong means 400.
///
/// **No manifest is a refusal here, unlike the listing.** `GET /v1/gateway/routes` answers "no
/// routes" for a server with no manifest configured, because that is the true answer to its
/// question. A write has no such answer: there is no file to write into, and inventing one would
/// put an operator's provider in a path nobody asked for.
pub(super) async fn gateway_route_set(
    State(state): State<ServeState>,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> Response {
    // A bearer authorizes the operation, not a filesystem namespace. The destination remains a
    // deployment fact selected by `serve --manifest`; accepting it from the request would turn
    // this local API into an arbitrary-path write primitive.
    let query = query.unwrap_or_default();
    if query_pairs(&query).any(|(key, _)| key == "manifest") {
        return bad_request(
            GATEWAY_ROUTE_SET_COMMAND,
            "a mutation cannot override the manifest configured when the server started",
            "/manifest",
        );
    }
    let Some(manifest) = state
        .runtime
        .as_ref()
        .and_then(|wiring| wiring.model.as_ref())
        .map(|model| model.manifest_path.clone())
    else {
        return bad_request(
            GATEWAY_ROUTE_SET_COMMAND,
            "this server has no gateway manifest configured",
            "/manifest",
        );
    };
    let payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return bad_request(
                GATEWAY_ROUTE_SET_COMMAND,
                "the request body is not valid JSON",
                "/",
            );
        }
    };

    let mut fields = Vec::new();
    for field in ["id", "provider", "baseUrl", "model"] {
        match payload.get(field) {
            Some(serde_json::Value::String(value)) => fields.push(value.clone()),
            _ => {
                return bad_request(
                    GATEWAY_ROUTE_SET_COMMAND,
                    &format!("the request body must carry \"{field}\" as a string"),
                    &format!("/{field}"),
                );
            }
        }
    }
    let credential_ref = match payload.get("credentialRef") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(value)) => Some(value.clone()),
        Some(_) => {
            return bad_request(
                GATEWAY_ROUTE_SET_COMMAND,
                "\"credentialRef\" must be a string when it is given",
                "/credentialRef",
            );
        }
    };
    let profiles = match payload.get("profiles") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::Array(values)) => {
            let Some(values) = values
                .iter()
                .map(|value| value.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
            else {
                return bad_request(
                    GATEWAY_ROUTE_SET_COMMAND,
                    "\"profiles\" must be an array of strings when it is given",
                    "/profiles",
                );
            };
            Some(values)
        }
        Some(_) => {
            return bad_request(
                GATEWAY_ROUTE_SET_COMMAND,
                "\"profiles\" must be an array of strings when it is given",
                "/profiles",
            );
        }
    };
    let (Some(enabled), Some(replace)) = (
        optional_flag(&payload, "enabled", true),
        optional_flag(&payload, "replace", false),
    ) else {
        let field = if optional_flag(&payload, "enabled", true).is_none() {
            "enabled"
        } else {
            "replace"
        };
        return bad_request(
            GATEWAY_ROUTE_SET_COMMAND,
            &format!("\"{field}\" must be a boolean when it is given"),
            &format!("/{field}"),
        );
    };

    let write = crate::commands::gateway::route::RouteWrite {
        id: fields[0].clone(),
        provider: fields[1].clone(),
        base_url: fields[2].clone(),
        model: fields[3].clone(),
        credential_ref,
        profiles,
        enabled,
        replace,
    };
    // Reading the manifest, composing it and renaming the temporary file is blocking filesystem
    // work, so it runs off the reactor like every other file path on this surface (#559).
    match tokio::task::spawn_blocking(move || {
        crate::commands::gateway::route::set(&manifest, &write)
    })
    .await
    {
        Ok(outcome) => respond_outcome(outcome),
        Err(_) => respond(
            StatusCode::INTERNAL_SERVER_ERROR,
            Outcome::internal(GATEWAY_ROUTE_SET_COMMAND, "the write task failed").output,
        ),
    }
}

/// An optional boolean body field: absent is the default, present-and-not-a-boolean is `None`.
///
/// It answers `Option` rather than carrying a built `Response` in an `Err`: a `Response` is at
/// least 128 bytes and `clippy::result_large_err` refuses that shape, which matters here because
/// the gate runs Clippy with `-D warnings`. The caller turns `None` into the refusal, which also
/// keeps this function free of any opinion about status codes.
fn optional_flag(payload: &serde_json::Value, field: &str, default: bool) -> Option<bool> {
    match payload.get(field) {
        None | Some(serde_json::Value::Null) => Some(default),
        Some(serde_json::Value::Bool(value)) => Some(*value),
        Some(_) => None,
    }
}

/// `PUT /v1/gateway/credentials/{reference}`: the HTTP half of `gateway credential set` (#1171).
///
/// **The value travels in the BODY, and that is a security decision, not a style one.** The read
/// audit (`--read-audit`, `record_read`) writes the method, the path, the QUERY and the response
/// body to a plaintext file; it never sees a request body. A key in the path or the query would
/// therefore land on disk in the clear for any operator who turned the audit on, which is the
/// composition defect #467's review caught once already on the evidence route. The reply names the
/// reference and its routes and never the value, so the audit line stays as empty of the secret as
/// the rest of this surface.
///
/// **The passphrase is not a request field.** It stays `GRAPHHELM_GATEWAY_KEY` in the serve
/// process, read by the command layer at the moment of the store, exactly as the CLI reads it. A
/// caller who could name the passphrase could open a broker this server was never given.
pub(super) async fn gateway_credential_set(
    State(state): State<ServeState>,
    UrlPath(reference): UrlPath<String>,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> Response {
    // As above, the request supplies credential CONTENT only. Broker and keyring coordinates are
    // deployment facts; letting a bearer replace them would cross the server's resource boundary.
    let query = query.unwrap_or_default();
    if query_pairs(&query).any(|(key, _)| matches!(key, "broker" | "keyring" | "keyId")) {
        return bad_request(
            GATEWAY_CREDENTIAL_SET_COMMAND,
            "a mutation cannot override the broker or keyring configured when the server started",
            "/broker",
        );
    }
    let Some(wiring) = state.runtime.as_ref() else {
        return bad_request(
            GATEWAY_CREDENTIAL_SET_COMMAND,
            "this server has no broker and keyring configured",
            "/broker",
        );
    };
    let Some(model) = wiring.model.as_ref() else {
        return bad_request(
            GATEWAY_CREDENTIAL_SET_COMMAND,
            "this server has no broker configured",
            "/broker",
        );
    };
    let broker = model.broker_dir.clone();
    let manifest = model.manifest_path.clone();
    let keyring = wiring.keyring_dir.clone();
    let key_id = wiring.key_id.clone();

    let payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return bad_request(
                GATEWAY_CREDENTIAL_SET_COMMAND,
                "the request body is not valid JSON",
                "/",
            );
        }
    };
    let Some(provider) = payload.get("provider").and_then(serde_json::Value::as_str) else {
        return bad_request(
            GATEWAY_CREDENTIAL_SET_COMMAND,
            "the request body must carry \"provider\" as a string",
            "/provider",
        );
    };
    let Some(usable_by) = payload
        .get("usableBy")
        .and_then(serde_json::Value::as_array)
    else {
        return bad_request(
            GATEWAY_CREDENTIAL_SET_COMMAND,
            "the request body must carry \"usableBy\" as an array of route ids",
            "/usableBy",
        );
    };
    let mut routes = Vec::with_capacity(usable_by.len());
    for route in usable_by {
        match route.as_str() {
            Some(id) if !id.trim().is_empty() => routes.push(id.to_owned()),
            _ => {
                return bad_request(
                    GATEWAY_CREDENTIAL_SET_COMMAND,
                    "every entry of \"usableBy\" must be a non-empty route id",
                    "/usableBy",
                );
            }
        }
    }
    if routes.is_empty() {
        return bad_request(
            GATEWAY_CREDENTIAL_SET_COMMAND,
            "\"usableBy\" must name at least one route: a credential nobody may lease is one nobody can use",
            "/usableBy",
        );
    }
    // The value is read LAST, so a malformed request is refused before the secret is copied out of
    // the body at all, and the refusal paths above never had it in scope.
    let value = match payload.get("value").and_then(serde_json::Value::as_str) {
        Some(value) if !value.trim().is_empty() => {
            graphhelm_events::SecretBytes::new(value.trim().as_bytes().to_vec())
        }
        _ => {
            return bad_request(
                GATEWAY_CREDENTIAL_SET_COMMAND,
                "the request body must carry \"value\" as a non-empty string",
                "/value",
            );
        }
    };
    let provider = provider.to_owned();

    match tokio::task::spawn_blocking(move || {
        crate::commands::gateway::credential::set_value_preserving_scope(
            &broker, &keyring, &key_id, &manifest, &reference, &provider, routes, value,
        )
    })
    .await
    {
        Ok(outcome) => respond_outcome(outcome),
        Err(_) => respond(
            StatusCode::INTERNAL_SERVER_ERROR,
            Outcome::internal(GATEWAY_CREDENTIAL_SET_COMMAND, "the store task failed").output,
        ),
    }
}
