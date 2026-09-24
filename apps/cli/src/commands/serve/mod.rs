mod documents;
pub(super) mod monitor;
mod notices;
pub(super) mod ports;
mod routes;
mod wake;

use std::collections::HashMap;
use std::future::Future;
use std::io::Write;
use std::net::SocketAddr;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use graphhelm_events::PreparedAppend;
use graphhelm_gateway::manifest::RouteManifest;
use graphhelm_graph::raw_content_sha256;
use graphhelm_protocols::{
    ActorId, AgentPresenceDeclared, DeclaredEffort, Diagnostic, EventEnvelope, EventKind, NewEvent,
    OpaqueId, PersistedActor, PersistedActorType, RepositoryScope, Sensitivity, SweepCaller,
};
use graphhelm_runtime::driver::ImmediateCancelRequest;

use crate::args::ServeArgs;
use crate::commands::events::runtime;
use crate::commands::execution::signal::SignalKeyring;
use crate::commands::{event_store, execution, secret_file};
use crate::output::{CommandOutput, Outcome};
use ports::RuntimeWiring;

/// Failures of `serve` itself: a malformed or non-loopback `--bind`, or the token/events
/// directory could not be prepared. Reused from `events`/`execution`'s identical shape (see
/// their `Failure`) — kept as its own copy per this codebase's established convention of a
/// small, local, per-module redaction-safe failure type rather than a shared central one.
pub(super) struct Failure {
    code: &'static str,
    message: String,
    pointer: String,
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

const SOURCE: &str = "serve-cli";
/// Reused from `events`/`execution`'s identical vocabulary for malformed CLI arguments.
const ARGUMENT_CODE: &str = crate::error_codes::GHCLI001_ARGUMENT_INVALID;
/// A `--bind` that parses but is not loopback, or whose listener/token setup cannot proceed —
/// the plan's own name for this new code (Milestone 05a Task 1).
const SERVE_INVALID_CODE: &str = crate::error_codes::GHCLI006_SERVE_INVALID;
/// A request to anything but `/health` without a valid `Authorization: Bearer <token>`.
const UNAUTHORIZED_CODE: &str = crate::error_codes::GHCLI007_SERVE_UNAUTHORIZED;
/// No route matches the request (method+path), reported once auth has already passed.
const NOT_FOUND_CODE: &str = crate::error_codes::GHCLI008_SERVE_NOT_FOUND;
/// The read audit was asked for and could not be written. Reported rather than swallowed: an
/// audit that silently stops recording reads as "the caller asked nothing", which is the exact
/// lie the audit exists to prevent.
const AUDIT_CODE: &str = crate::error_codes::GHCLI009_SERVE_AUDIT_FAILED;
/// A recognized retry could not be annotated because the status command violated its object
/// response contract. Fail closed: without the marker, HTTP 200 would make an already-applied
/// request indistinguishable from a fresh mutation whose attributable event is missing.
const IDEMPOTENCY_REPLY_CODE: &str = crate::error_codes::GHCLI023_IDEMPOTENCY_REPLY_INVALID;
const AUDIT_COMMAND: &str = "serve.read_audit";

/// The command name `serve`'s own pre-bind failures report under — there is no verb, unlike
/// `execution`/`events`, so the bare subcommand name is the closest existing precedent
/// (`main.rs`'s own `"schema"` pre-dispatch failure uses the same bare-name convention).
const COMMAND: &str = "serve";
const STARTED_COMMAND: &str = "serve.started";
const HEALTH_COMMAND: &str = "serve.health";
const UNAUTHORIZED_COMMAND: &str = "serve.unauthorized";
const NOT_FOUND_COMMAND: &str = "serve.not_found";

fn argument(message: &str, pointer: &str) -> Failure {
    Failure {
        code: ARGUMENT_CODE,
        message: message.to_owned(),
        pointer: pointer.to_owned(),
    }
}

fn serve_invalid(message: &str, pointer: &str) -> Failure {
    Failure {
        code: SERVE_INVALID_CODE,
        message: message.to_owned(),
        pointer: pointer.to_owned(),
    }
}

/// State shared by every handler and the auth layer — the ADR's own constraint (see
/// `docs/reference/REFERENCE_STACK_AND_ADRS.md`, ADR-024): no handler may hold state beyond
/// this. `token` is the 64 lowercase-hex-character bearer token, compared in constant time.
#[derive(Clone)]
struct ServeState {
    token: Arc<[u8]>,
    /// Explicit document root, independent of model/tool executor wiring.
    project: Option<Arc<Path>>,
    /// The events directory. Every handler opens a fresh `LocalEventRepository` against it via
    /// `commands::event_store` — exactly the call every CLI command makes — does its one command's
    /// work, and lets the handle drop before the response is sent. `ServeState` deliberately does
    /// *not* cache an open repository handle: Milestone 05a Task 1 confirmed empirically that
    /// `LocalEventRepository::open` holds an OS-level exclusive lock for the handle's entire
    /// lifetime, so a handle cached here would hold that lock for the server's whole run and lock
    /// out every concurrent CLI process against the same events directory — defeating the
    /// multi-agent premise this API exists for. The per-request open/use/drop cycle *is* the
    /// concurrency model: the store's own exclusive append lock is what serializes concurrent
    /// writers safely, not anything this server does.
    events: Arc<Path>,
    /// Milestone 05d Task 9: the real-executor wiring, present when `serve` was launched with
    /// at least one real half — the model half `{manifest, broker, route}` or the tool half
    /// `{staging, allow-program}` (#1066), each with `{keyring, key-id}`. `None` keeps the 05a
    /// fixture-only shape unchanged.
    runtime: Option<Arc<RuntimeWiring>>,
    /// The keyring coordinates alone, present whenever `--keyring`/`--key-id` were given —
    /// independently of `runtime` (STEP 2: `{keyring, key-id}` is its own all-or-none group,
    /// usable on its own as sealing-only configuration for the `signal` route). When `runtime` is
    /// `Some`, this is always `Some` too (real-executor mode requires both groups).
    sealing: Option<Arc<SignalKeyring>>,
    /// One cancellation sender per in-flight async drive, keyed by execution id — registered
    /// before `drive_to_quiescence_async` starts and deregistered once it returns. `pause
    /// {"mode":"immediate"}` (STEP 5) looks a live execution up here to interrupt it.
    cancels: Arc<
        tokio::sync::Mutex<
            HashMap<String, tokio::sync::watch::Sender<Option<ImmediateCancelRequest>>>,
        >,
    >,
    /// The customs sweep tick's period in seconds, or `None` for no tick at all.
    ///
    /// It lives on the state rather than being passed to `serve_forever` separately because the
    /// tick is part of what this server IS once launched, not a parameter of one call -- and a
    /// reader asking "does this deployment write on its own?" should find the answer beside the
    /// events directory it writes to.
    sweep_interval: Option<u64>,
    /// Where to append the read audit, when `--read-audit` asked for one. `None` - the default -
    /// means nothing is recorded at all.
    read_audit: Option<Arc<Path>>,
}

/// `graphhelm serve --events <dir> --bind <addr>`: creates or loads the bearer token, binds
/// loopback-only, and runs the router on the shared `runtime()` helper until killed. The
/// function only ever *returns* on a setup failure — once the server starts accepting
/// connections it runs until the process is killed, by design (there is no shutdown endpoint in
/// this milestone).
pub fn run(args: &ServeArgs) -> Outcome {
    match execute(args) {
        Ok(()) => Outcome::success(COMMAND, serde_json::json!({})),
        Err(failure) => failure.into_outcome(COMMAND),
    }
}

fn execute(args: &ServeArgs) -> Result<(), Failure> {
    let address = parse_loopback_bind(&args.bind)?;
    std::fs::create_dir_all(&args.events)
        .map_err(|_| serve_invalid("the events directory could not be created", "/events"))?;
    // One implementation with `init` (#1062): the token `init` minted is the one `serve` reads.
    let (_, token) = secret_file::ensure_token(&args.events)
        .map_err(|error| serve_invalid(error.message(), "/token"))?;
    let (runtime_wiring, sealing, startup_warnings) = build_wiring(args)?;
    let state = ServeState {
        token: Arc::from(token.into_bytes()),
        project: args.project.as_deref().map(Arc::from),
        events: Arc::from(args.events.as_path()),
        runtime: runtime_wiring.map(Arc::new),
        sealing: sealing.map(Arc::new),
        cancels: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        read_audit: args.read_audit.as_deref().map(Arc::from),
        sweep_interval: args.sweep_interval,
    };

    let rt =
        runtime().map_err(|_| serve_invalid("the operator runtime could not be started", "/"))?;
    rt.block_on(serve_forever(address, state, startup_warnings))
}

// #583: there is deliberately NO default program allowlist here.
//
// `DEFAULT_ALLOW_PROGRAMS = ["git", "cargo"]` used to live at this spot and was substituted when
// the operator declared none. It is gone, and re-adding it would undo the point: with a default,
// a deliberate choice and an inherited one produce byte-identical journals, and the record keeps
// the list while losing the decision. The executor now refuses an undeclared allowlist outright
// (see the argument checks above), so the ambiguous state cannot be reached rather than being
// recorded and explained.
//
// #177's closed-vocabulary guard over that constant was retired in the same commit. It was doing
// real work -- it made GROWTH of the default a visible test change -- but its subject no longer
// exists, and a guard whose subject is gone certifies nothing while still looking like coverage.

/// STEP 2's grouping rule as #1066 reshaped it, enforced once at startup (`serve_invalid` on
/// violation) rather than per-request. Two independent all-or-none groups make the real halves:
/// `{manifest, broker, route}` is the MODEL half, `{staging, allow-program}` is the TOOL half;
/// either, both, or neither. `{keyring, key-id}` is its own all-or-none group, usable alone as
/// sealing-only configuration, and REQUIRED whenever either real half is present — real work
/// seals evidence, and a keyring is what it seals under. A manifest is loaded and validated
/// here (`RouteManifest::from_json`, fail fast) and the configured `--route` is resolved to a
/// cloned `ModelRoute` — never re-parsed per drive.
///
/// Before this, the four executor flags were ONE group and a tool node could not run a real
/// `cargo test` unless a model route was also configured — the "invent an API key to get past
/// setup" smell `gateway/keyring.rs` already names for messages, applied to tools.
///
/// The third element is the startup WARNINGS: conditions that do not stop `serve` but that the
/// operator should read on the `serve.started` line (today: a keyring the key does not open).
#[allow(clippy::type_complexity)]
fn build_wiring(
    args: &ServeArgs,
) -> Result<
    (
        Option<RuntimeWiring>,
        Option<SignalKeyring>,
        Vec<Diagnostic>,
    ),
    Failure,
> {
    let mut warnings = Vec::new();
    let model_group = [
        args.manifest.is_some(),
        args.broker.is_some(),
        args.route.is_some(),
    ];
    let model_all = model_group.iter().all(|present| *present);
    let model_none = model_group.iter().all(|present| !*present);
    if !model_all && !model_none {
        return Err(serve_invalid(
            "--manifest, --broker and --route must be given together or not at all",
            "/arguments",
        ));
    }
    // #583: the program allowlist is the set of executables an execution may spawn, so it is
    // DECLARED or the execution does not start. It used to default to two names when the operator
    // gave none, which made a deliberate choice and an inherited one produce byte-identical
    // journals -- the record kept the list and lost the decision.
    //
    // The cure makes the ambiguous state unrepresentable instead of recording it: with no default,
    // every journal's allowlist was chosen by someone. #1066 folds the allowlist into the tool
    // half's own all-or-none group: `--staging` without `--allow-program` is the undeclared
    // allowlist #583 refuses, and `--allow-program` without `--staging` is a list nothing would
    // ever consult.
    //
    // This sits BEFORE the manifest is read, with the other argument-shape refusals, so an
    // operator learns what is missing without first needing every path to be valid.
    //
    // The message names the FLAGS and never the programs. If it suggested a list, every operator
    // would paste that list back and "declared deliberately" would be theatre -- the refusal asks
    // the question, it does not hand over an answer.
    let tool_group = [args.staging.is_some(), !args.allow_program.is_empty()];
    let tool_all = tool_group.iter().all(|present| *present);
    let tool_none = tool_group.iter().all(|present| !*present);
    if !tool_all && !tool_none {
        return Err(serve_invalid(
            "--staging and --allow-program must be given together or not at all: the set of \
             programs an execution may spawn is declared per run, never defaulted",
            "/arguments",
        ));
    }
    let keyring_group = [args.keyring.is_some(), args.key_id.is_some()];
    let keyring_all = keyring_group.iter().all(|present| *present);
    let keyring_none = keyring_group.iter().all(|present| !*present);
    if !keyring_all && !keyring_none {
        return Err(serve_invalid(
            "--keyring and --key-id must be given together or not at all",
            "/arguments",
        ));
    }
    if (model_all || tool_all) && !keyring_all {
        return Err(serve_invalid(
            "the real-executor flags require --keyring and --key-id as well",
            "/arguments",
        ));
    }

    let sealing = if keyring_all {
        let keyring = args.keyring.clone().expect("keyring_all guarantees Some");
        let key_id = args.key_id.clone().expect("keyring_all guarantees Some");
        let sealing = SignalKeyring {
            directory: keyring,
            key_id,
        };
        // Pre-flight the open ONCE, at startup (PR #1070 review): with `--keyring` given, an
        // unset `GRAPHHELM_EVENTS_KEY` or a `--key-id` the keyring does not hold used to produce
        // a silent `serve.started` and surface only on the first message send, as a refused
        // seal. The same `open_sealer` the send path uses answers here, so the two cannot
        // disagree; the provider is dropped and re-opened per send exactly as before.
        //
        // A WARNING on the `serve.started` reply, not a refusal: a fixture story never seals
        // (`resume_atomicity.rs` starts `serve` with the keyring flags and no key on purpose),
        // so startup must succeed; the operator who WILL seal reads the consequence at start.
        if let Err(failure) = execution::signal::open_sealer(&sealing) {
            warnings.push(Diagnostic::warning(
                SERVE_INVALID_CODE,
                format!(
                    "the keyring could not be opened with GRAPHHELM_EVENTS_KEY ({}); sealed operations (messages, real executors) will refuse until serve is restarted with the right key",
                    failure.message
                ),
                "/keyring",
                SOURCE,
            ));
        }
        Some(sealing)
    } else {
        None
    };

    let model = if model_all {
        let manifest_path = args.manifest.as_ref().expect("model_all guarantees Some");
        // The same bounded, regular-file-only read the per-request re-read uses (#559), so the
        // manifest `serve` starts on cannot be one it would later refuse.
        let bytes =
            crate::commands::gateway::read_bounded_manifest(manifest_path).map_err(|error| {
                match error {
                    crate::commands::gateway::ManifestReadError::Unreadable => {
                        serve_invalid("--manifest does not name a readable file", "/manifest")
                    }
                    crate::commands::gateway::ManifestReadError::TooLarge => {
                        serve_invalid("--manifest exceeds the maximum supported size", "/manifest")
                    }
                }
            })?;
        let text = String::from_utf8(bytes)
            .map_err(|_| serve_invalid("--manifest is not valid UTF-8", "/manifest"))?;
        let manifest = RouteManifest::from_json(&text)
            .map_err(|error| serve_invalid(&error.to_string(), "/manifest"))?;
        let route_id = args.route.as_ref().expect("model_all guarantees Some");
        // Through `ports::find_route`, not an inline `find` here: a request can now name a route
        // too, and the day these two lookups are written twice is the day they disagree. The
        // lookup also filters `enabled` (PR #467 review), so a disabled route refuses at startup
        // exactly as it does per request.
        let route = ports::find_route(&manifest, route_id).ok_or_else(|| {
            serve_invalid(
                "--route does not name an enabled route in the manifest",
                "/route",
            )
        })?;
        Some(ports::ModelWiring {
            manifest_path: manifest_path.clone(),
            route,
            broker_dir: args.broker.clone().expect("model_all guarantees Some"),
        })
    } else {
        None
    };

    let tools = tool_all.then(|| ports::ToolWiring {
        staging: args.staging.clone().expect("tool_all guarantees Some"),
        project: args.project.clone(),
        tests_runner: args.tests_runner.clone(),
        // Never empty: the group check above refuses a staging area that declared none.
        allow_programs: args.allow_program.clone(),
        path_prepend: args.path_prepend.clone(),
    });

    let runtime = if model.is_some() || tools.is_some() {
        Some(RuntimeWiring {
            model,
            tools,
            keyring_dir: args
                .keyring
                .clone()
                .expect("a real half guarantees the keyring"),
            key_id: args
                .key_id
                .clone()
                .expect("a real half guarantees the key id"),
        })
    } else {
        None
    };

    Ok((runtime, sealing, warnings))
}

async fn serve_forever(
    address: SocketAddr,
    state: ServeState,
    startup_warnings: Vec<Diagnostic>,
) -> Result<(), Failure> {
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|_| serve_invalid("the requested address could not be bound", "/bind"))?;
    let bound = listener
        .local_addr()
        .map_err(|_| serve_invalid("the bound address could not be read back", "/bind"))?;

    // The test harness (and any co-located operator tooling) discovers the actually-bound port
    // by parsing this one line, so it must be exactly one JSON object on stdout. `println!`
    // writes through a `LineWriter` and so flushes on the trailing newline regardless of
    // piping, but the explicit flush below makes the ordering (bind, print, flush, then serve
    // forever) airtight rather than relying on that implementation detail.
    // `executors` says which halves are REAL on this server (#1073): a client reading the
    // startup line learns whether its cognitive nodes and its tool nodes will run or be answered
    // by fixtures, without having to infer it from a parked node later.
    let executors = ExecutorWiring::from_state(&state);
    let started = Outcome::success(
        STARTED_COMMAND,
        serde_json::json!({
            "address": bound.to_string(),
            "executors": { "model": executors.model, "tools": executors.tools },
        }),
    )
    .with_warnings(startup_warnings);
    crate::output::print(&started.output, false);
    let _ = std::io::stdout().flush();

    if let Some(seconds) = state.sweep_interval {
        tokio::spawn(sweep_tick(Arc::clone(&state.events), seconds));
    }

    let app = build_router(state);
    axum::serve(listener, app)
        .await
        .map_err(|_| serve_invalid("the server loop ended unexpectedly", "/"))
}

/// Sweep every stream in the repository, forever, every `seconds`.
///
/// THE TICK IS THE CALLER `SweepCaller::Tick` EXISTS FOR, and it passes no idempotency key. The
/// verb mints its own per-call key here deliberately: a tick asks the SAME question at a NEW
/// instant every time, against the same possibly-lapsed episode, so collapsing two ticks onto one
/// key would silently drop the second sweep. Idempotence for a tick lives in the FOLD -- one
/// `exception_marked` per episode -- which is what makes repeating the question harmless.
///
/// EVERY FAILURE IS SWALLOWED, PER STREAM, ON PURPOSE. A stream this tick cannot read must not
/// stop the tick from reaching the others, and a background writer that kills the server it lives
/// in would turn a bookkeeping problem into an outage. What it must never do is hide a failure
/// that CHANGED something: the verb appends its batch atomically, so a stream either gains its
/// record or gains nothing.
///
/// The store is opened and dropped inside `spawn_blocking` for the same reason `ServeState` holds
/// no cached handle: `LocalEventRepository::open` takes an OS-level exclusive lock for the
/// handle's lifetime, and a tick holding one across its sleep would lock out every co-located CLI
/// process between beats.
async fn sweep_tick(events: Arc<Path>, seconds: u64) {
    let period = std::time::Duration::from_secs(seconds.max(1));
    loop {
        tokio::time::sleep(period).await;
        let events = Arc::clone(&events);
        let _ = tokio::task::spawn_blocking(move || {
            let Ok(store) = crate::commands::event_store(&events) else {
                return;
            };
            let Ok(streams) = store.list_streams() else {
                return;
            };
            let Ok(now) = store.now() else {
                return;
            };
            let actor = PersistedActor::new(
                PersistedActorType::System,
                ActorId::parse("system-sweep-tick").expect("constant actor id is valid"),
            );
            for stream in streams {
                let _ = graphhelm_events::sweep(
                    &store,
                    &stream.scope,
                    stream.stream_id.as_str(),
                    &now,
                    &actor,
                    SweepCaller::Tick,
                    None,
                );
            }
        })
        .await;
    }
}

fn build_router(state: ServeState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/executions", get(routes::list_executions))
        .route("/v1/executions/{id}", get(routes::status))
        .route("/v1/executions/{id}/briefing", get(routes::briefing))
        .route("/v1/executions/{id}/events", get(routes::events))
        .route(
            "/v1/executions/{id}/evidence/{evidenceId}",
            get(routes::evidence),
        )
        .route("/v1/executions/{id}/start", post(routes::start))
        .route("/v1/executions/{id}/signal", post(routes::signal))
        .route("/v1/executions/{id}/documents/read", post(documents::read))
        .route("/v1/executions/{id}/documents/save", post(documents::save))
        .route("/v1/executions/{id}/approve", post(routes::approve))
        .route(
            "/v1/executions/{id}/amend-budget",
            post(routes::amend_budget),
        )
        .route("/v1/executions/{id}/pause", post(routes::pause))
        .route("/v1/executions/{id}/resume", post(routes::resume))
        .route("/v1/executions/{id}/cancel", post(routes::cancel))
        .route("/v1/executions/{id}/sweep", post(routes::sweep))
        .route("/v1/executions/{id}/claim", post(routes::claim))
        .route("/v1/executions/{id}/clear", post(routes::clear))
        .route(
            "/v1/executions/{id}/wake-lease",
            post(routes::wake_lease).get(routes::wake_lease_status),
        )
        .route("/v1/graph/topology", post(routes::graph_topology))
        .route("/v1/graphs/synthesize", post(routes::synthesize))
        .route(
            "/v1/gateway/routes",
            get(routes::gateway_routes).put(routes::gateway_route_set),
        )
        .route("/v1/gateway/probe", get(routes::gateway_probe))
        .route(
            "/v1/gateway/credentials/{reference}",
            put(routes::gateway_credential_set),
        )
        .route(
            "/v1/development/contract",
            post(routes::development_resolve_contract),
        )
        .route(
            "/v1/development/memory",
            get(routes::development_memory_status).post(routes::development_memory_propose),
        )
        .route("/v1/development/present", post(routes::development_present))
        .route(
            "/v1/development/context",
            post(routes::development_compile_context),
        )
        .route(
            "/v1/development/accounting",
            get(routes::development_accounting),
        )
        .fallback(not_found)
        // BOUNDED ABOVE THE DOCUMENT LIMIT, NOT BELOW IT. Axum's default `Bytes` extractor caps
        // bodies at 2 MiB, and `load_graph_json` advertises 4 MiB - so an inline graph between
        // the two was refused by the extractor with Axum's own 413 before the schema loader ever
        // saw a document it is explicitly prepared to accept (PR #467 review). 5 MiB admits the
        // full supported graph plus its JSON request wrapper, and stays a hard bound.
        .layer(axum::extract::DefaultBodyLimit::max(5 * 1024 * 1024))
        // `.layer` (not `.route_layer`) wraps the fallback too: an unauthenticated request to a
        // path with no route must still be refused 401, not fall through to a 404 that would
        // leak whether the path exists to an unauthenticated caller.
        .layer(middleware::from_fn_with_state(state.clone(), require_token))
        // OUTSIDE the auth layer, so a refused request is recorded too: "the judge was told
        // 401" is exactly the kind of fact that was unrecoverable before. Headers never reach
        // the recorder, so the bearer token cannot land on disk.
        .layer(middleware::from_fn_with_state(state.clone(), record_read))
        // The monitor sub-router merges AFTER the auth layer: its cookie bootstrap replaces
        // the Bearer scheme (same token bytes, same constant-time verifier — one authority),
        // and it is GET-only by construction — a mutating verb never has a handler to reach,
        // so 405 is the router's answer, not a handler's choice (D-040 as structure).
        .merge(
            Router::new()
                .route("/monitor", get(monitor::monitor_index))
                .route("/monitor/{id}", get(monitor::monitor_page)),
        )
        // Closes out the router's pending `ServeState` (needed by `routes::status`/`routes::events`
        // above, which extract it via `State<ServeState>`) into a plain `Router` axum::serve can
        // run directly.
        .with_state(state)
}

async fn health() -> impl IntoResponse {
    respond(
        StatusCode::OK,
        Outcome::success(HEALTH_COMMAND, serde_json::json!({})).output,
    )
}

async fn not_found() -> impl IntoResponse {
    respond(
        StatusCode::NOT_FOUND,
        Outcome::domain(
            NOT_FOUND_COMMAND,
            vec![Diagnostic::error(
                NOT_FOUND_CODE,
                "no route matches this request",
                "/",
                SOURCE,
            )],
        )
        .output,
    )
}

/// The auth layer: every request but `/health` requires `Authorization: Bearer <token>`,
/// compared in constant time. A hand-written fold over bytes rather than the `subtle` crate,
/// matching D-038's minimalism precedent (`docs/DECISION_REGISTER.md`) — `subtle` is not in the
/// workspace and this comparison is small enough not to need it.
async fn require_token(State(state): State<ServeState>, request: Request, next: Next) -> Response {
    if request.uri().path() == "/health" {
        return next.run(request).await;
    }
    let presented = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let authorized =
        presented.is_some_and(|token| constant_time_eq(token.as_bytes(), &state.token));
    if authorized {
        return next.run(request).await;
    }
    respond(
        StatusCode::UNAUTHORIZED,
        Outcome::domain(
            UNAUTHORIZED_COMMAND,
            vec![Diagnostic::error(
                UNAUTHORIZED_CODE,
                "a valid Authorization: Bearer token is required",
                "/authorization",
                SOURCE,
            )],
        )
        .output,
    )
}

fn respond(status: StatusCode, output: CommandOutput) -> Response {
    (status, Json(output)).into_response()
}

/// Maps one `execution::Failure` onto an HTTP response in the standard four-key envelope, per the
/// endpoint contract's failure-mapping table (public-runtime-api plan):
/// argument/validation problems (`GHCLI001_ARGUMENT_INVALID`, `GHCLI003_SIGNAL_INVALID`,
/// `GHCLI004_SIGNAL_UNRECORDABLE`) → 400; precondition refusals the commands already produce
/// (`GHCLI005_EXECUTION_STATE`) → 409; store conflicts (`GHE001_SEQUENCE_CONFLICT`,
/// `GHE003_IDEMPOTENCY_CONFLICT`) → 409; everything else → 500. Shared by every handler —
/// `routes::status`/`routes::events` (Task 2) use it directly; the mutation wrapper (Task 3) falls
/// back to it once its own more specific `currentHead`-carrying 409s do not apply. Never includes a
/// path, DSN or token in the response: `failure`'s own fields already guarantee that (the same
/// redaction-safe `Failure` type the CLI prints, relayed verbatim via the widened
/// `Failure::into_outcome`).
fn respond_failure(command: &'static str, failure: execution::Failure) -> Response {
    let status = match failure.code {
        crate::error_codes::GHCLI001_ARGUMENT_INVALID
        | crate::error_codes::GHCLI003_SIGNAL_INVALID
        | crate::error_codes::GHCLI004_SIGNAL_UNRECORDABLE => StatusCode::BAD_REQUEST,
        crate::error_codes::GHCLI005_EXECUTION_STATE
        | "GHE001_SEQUENCE_CONFLICT"
        | "GHE003_IDEMPOTENCY_CONFLICT" => StatusCode::CONFLICT,
        // #1083 F1: a well-formed id naming no stream. 404, the same status the router's own
        // fallback uses, told apart from it by the code (`GHCLI008_SERVE_NOT_FOUND` is "no
        // route", this is "no execution").
        crate::error_codes::GHCLI028_EXECUTION_NOT_FOUND => StatusCode::NOT_FOUND,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    respond(status, failure.into_outcome(command).output)
}

// -------------------------------------------------------------------------------------------
// Milestone 05a Task 3: attributed, idempotent mutations.
//
// Every mutation's event carries a key derived deterministically from the caller's
// `Idempotency-Key` header plus a fixed per-event suffix, instead of the CLI's fresh
// per-invocation UUID (`execution::idempotency_key`) — a byte-identical retry therefore
// re-derives the exact same key.
//
// CHECKPOINT (required before building on this): what does the local store actually do with a
// duplicate idempotency key? Empirically probed with a temporary test added to and then reverted
// from `core/events/tests/local_atomicity.rs` (mirroring that file's own
// `exact_retry_is_resolved_before_sequence_and_divergent_reuse_fails_closed` harness, which already
// proves the *silent-dedupe* fast path exists for a byte-identical retry of the *same*
// `PreparedAppend` — including the same `expected_next_sequence`). The scenario this API path
// actually produces is different: `execution::append_event` recomputes `next_sequence` fresh
// immediately before every append (the CLI has always done this), so a genuine retry lands the
// *same* idempotency key at a *different*, now-advanced sequence. The probe confirmed this
// collides at the store's own key-set-intersection check before the sequence check ever runs
// (`local.rs`'s `append_locked`), and — because the request's digest (which folds in
// `expected_next_sequence`) no longer matches the original committed batch's digest — the store
// does **not** silently dedupe it: it rejects the append with `GHE003_IDEMPOTENCY_CONFLICT`
// (`error.code()`), exactly as the M03 suite's own naming (`local_atomicity.rs`) suggested it
// would for a duplicate key it cannot recognize as an exact retry. This module maps that rejection
// onto the caller-visible idempotency contract below.
//
// FOLLOW-UP FINDING (post-Task-3 review): the pre-flight built on the checkpoint above was itself
// content-blind — `classify_existing_keys` checked derived-key *presence* only, never whether a
// previously committed event under that same key actually carried the *same request*. A caller
// reusing one `Idempotency-Key` for two different request bodies (e.g. `approve {"node":"a"}` then
// `approve {"node":"b"}` under the same key) got the second request silently absorbed as a
// "recognized retry" of the first: 200 with current state, and `b` never actually approved. The
// fix folds a content digest into the derived key itself (`DerivedKey`/`request_digest16` below),
// so a genuine retry (identical body) still re-derives the identical key — `Complete`, unchanged
// semantics — while a divergent reuse (same header, different body) now derives a *different* key
// that nonetheless shares the same `{command-key}-{suffix}-` prefix as the earlier one, which
// `classify_one_key` recognizes explicitly as `KeyState::Divergent` and refuses with 409 rather
// than ever reaching the store. The digest is a divergence *detector*, not a security boundary: a
// bearer-authenticated callers can still assert different durable actors, so digest equality is
// never sufficient proof by itself. Retry classification below also verifies the committed actor,
// execution scope/stream, and exact decision event kind.
// -------------------------------------------------------------------------------------------

/// One derived event key: `prefix` is `{Idempotency-Key header}-{suffix}`, and `full` is `prefix`
/// with the request's content digest appended (`{prefix}-{digest16}`, see `request_digest16`) —
/// the actual `idempotency_key` an appended event carries. Kept as a pair, rather than deriving
/// `prefix` back out of `full` by slicing off a known digest length, so `classify_one_key` never
/// has to assume the digest's exact width from the outside — both are produced together, once, in
/// `parse_mutation_headers`.
struct DerivedKey {
    prefix: String,
    full: OpaqueId,
}

/// The one committed event kind that proves each mutation happened. This is separate from the
/// key's suffix: `approve` and `amend_budget` both historically use `"outcome"`, but their durable
/// decisions are different event kinds. `Unrecognized` is deliberately fail-closed for a future
/// route that forgets to register its proof kind: the first write may still run, but no later
/// request can manufacture a recognized-retry marker from an event this layer cannot identify.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MutationDecisionKind {
    ExecutionStarted,
    SignalRecorded,
    NodeOutcomeRecorded,
    ExecutionFormAmended,
    ExecutionPaused,
    ExecutionResumed,
    ExecutionCompleted,
    SweepPerformed,
    WakeLease,
    /// #159: the ONE event `claim` appends is `completion_claimed` OR `completion_refused` — a
    /// refusal is the journaled outcome of the same command, so a retry of a refused claim is
    /// recognized as the retry it is rather than re-run to a second refusal.
    CompletionClaim,
    CompletionCleared,
    Unrecognized,
}

impl MutationDecisionKind {
    fn for_command(command: &str) -> Self {
        match command {
            "execution.start" => Self::ExecutionStarted,
            "execution.signal" => Self::SignalRecorded,
            "execution.approve" => Self::NodeOutcomeRecorded,
            "execution.amend_budget" => Self::ExecutionFormAmended,
            "execution.pause" => Self::ExecutionPaused,
            "execution.resume" => Self::ExecutionResumed,
            "execution.cancel" => Self::ExecutionCompleted,
            "execution.sweep" => Self::SweepPerformed,
            "execution.wake_lease" => Self::WakeLease,
            "execution.claim" => Self::CompletionClaim,
            "execution.clear" => Self::CompletionCleared,
            _ => Self::Unrecognized,
        }
    }

    /// Matches both the closed event variant and the execution identity inside its payload. An
    /// envelope scope alone is not enough: repository files are untrusted, and a payload claiming
    /// a different execution must never authenticate this request.
    fn matches(self, kind: &EventKind, execution: &str, request_body: &serde_json::Value) -> bool {
        match (self, kind) {
            (Self::ExecutionStarted, EventKind::ExecutionStarted(payload)) => {
                payload.execution_id.as_str() == execution
                    && serialized_field_matches(&payload.mode, request_body.get("mode"))
            }
            (Self::SignalRecorded, EventKind::SignalRecorded(payload)) => {
                payload.execution_id.as_str() == execution
                    && request_body.get("signal").is_some_and(|signal| {
                        signal.get("id").and_then(serde_json::Value::as_str)
                            == Some(payload.signal_id.as_str())
                            && signal
                                .pointer("/source/id")
                                .and_then(serde_json::Value::as_str)
                                == Some(payload.source_id.as_str())
                            && serialized_field_matches(
                                &payload.source_kind,
                                signal.pointer("/source/type"),
                            )
                            && signal.get("type").and_then(serde_json::Value::as_str)
                                == Some(payload.kind.as_str())
                            && serialized_field_matches(&payload.severity, signal.get("severity"))
                            && serde_json::to_vec(signal)
                                .ok()
                                .and_then(|bytes| raw_content_sha256(&bytes).ok())
                                .is_some_and(|digest| digest == payload.envelope_sha256)
                    })
            }
            (Self::NodeOutcomeRecorded, EventKind::NodeOutcomeRecorded(payload)) => {
                payload.execution_id.as_str() == execution
                    && request_body.get("node").and_then(serde_json::Value::as_str)
                        == Some(payload.node_id.as_str())
                    && payload.outcome == graphhelm_protocols::NodeOutcome::Approved
                    && payload.next_state == graphhelm_protocols::NodeState::Ready
                    && payload.reason.is_none()
            }
            (Self::ExecutionFormAmended, EventKind::ExecutionFormAmended(payload)) => {
                payload.execution_id.as_str() == execution
                    && request_body
                        .get("computedAtSequence")
                        .and_then(serde_json::Value::as_u64)
                        == Some(payload.computed_at_sequence)
                    && request_body
                        .get("node")
                        .and_then(serde_json::Value::as_str)
                        .zip(
                            request_body
                                .get("seconds")
                                .and_then(serde_json::Value::as_u64),
                        )
                        .is_some_and(|(node, seconds)| {
                            payload.node_timeout_seconds.len() == 1
                                && payload.node_timeout_seconds.iter().next().is_some_and(
                                    |(event_node, event_seconds)| {
                                        event_node.as_str() == node && *event_seconds == seconds
                                    },
                                )
                        })
            }
            (Self::ExecutionPaused, EventKind::ExecutionPaused(payload)) => {
                payload.execution_id.as_str() == execution
            }
            (Self::ExecutionResumed, EventKind::ExecutionResumed(payload)) => {
                payload.execution_id.as_str() == execution
            }
            (Self::ExecutionCompleted, EventKind::ExecutionCompleted(payload)) => {
                payload.execution_id.as_str() == execution
                    && payload.status == graphhelm_protocols::SimulationStatus::Cancelled
            }
            (Self::SweepPerformed, EventKind::SweepPerformed(payload)) => {
                payload.execution_id.as_str() == execution
                    && payload.caller == SweepCaller::Operator
                    && match request_body.get("asOf") {
                        None | Some(serde_json::Value::Null) => true,
                        Some(expected) => expected
                            .as_str()
                            .and_then(|value| {
                                graphhelm_protocols::PersistedTimestamp::parse(value).ok()
                            })
                            .is_some_and(|expected| expected == payload.as_of),
                    }
            }
            (Self::WakeLease, EventKind::WakeLease(payload)) => {
                payload.execution_id.as_str() == execution
                    && request_body
                        .get("sessionId")
                        .and_then(serde_json::Value::as_str)
                        == Some(payload.session_id.as_str())
                    && request_body
                        .get("rendezvousId")
                        .and_then(serde_json::Value::as_str)
                        == Some(payload.rendezvous_id.as_str())
                    && request_body
                        .get("cursor")
                        .and_then(serde_json::Value::as_u64)
                        .is_none_or(|cursor| cursor == payload.cursor)
                    && request_body
                        .get("maturesInSeconds")
                        .and_then(serde_json::Value::as_u64)
                        == payload.matures_in_seconds
            }
            // Bound to the node the request named and, when it named one, the wait: the body
            // digest inside the derived key already covers the evidence bundle, so these are the
            // fields a same-key event of the right kind could still differ on.
            (Self::CompletionClaim, EventKind::CompletionClaimed(payload)) => {
                payload.execution_id.as_str() == execution
                    && request_body.get("node").and_then(serde_json::Value::as_str)
                        == Some(payload.node.as_str())
                    && request_body
                        .get("waitSeq")
                        .and_then(serde_json::Value::as_u64)
                        .is_none_or(|wait_seq| wait_seq == payload.completes_wait_seq)
            }
            (Self::CompletionClaim, EventKind::CompletionRefused(payload)) => {
                payload.execution_id.as_str() == execution
                    && request_body.get("node").and_then(serde_json::Value::as_str)
                        == Some(payload.node.as_str())
                    && request_body
                        .get("waitSeq")
                        .and_then(serde_json::Value::as_u64)
                        .is_none_or(|wait_seq| wait_seq == payload.claimed_wait_seq)
            }
            (Self::CompletionCleared, EventKind::CompletionCleared(payload)) => {
                payload.execution_id.as_str() == execution
                    && request_body
                        .get("claimSeq")
                        .and_then(serde_json::Value::as_u64)
                        == Some(payload.claim_seq)
            }
            _ => false,
        }
    }
}

fn serialized_field_matches<T: serde::Serialize>(
    actual: &T,
    expected: Option<&serde_json::Value>,
) -> bool {
    expected.is_some_and(|expected| serde_json::to_value(actual).ok().as_ref() == Some(expected))
}

/// One mutation's derived identity: the actor its event is attributed to, the deterministic
/// key(s) its event(s) use — one `DerivedKey` per suffix every mutation wires up
/// (`signal` → `"record"`, `approve` → `"outcome"`, `start` → `"started"`, `pause` → `"paused"`,
/// `resume` → `"resumed"`, `cancel` → `"cancelled"`) — the caller's bare `Idempotency-Key` header
/// value (named directly in a divergent-reuse diagnostic), and the optional `If-Match`
/// precondition (Milestone 05a Task 4).
///
/// `keys` stays a `Vec` (rather than a single `DerivedKey`) so the classification helpers below
/// generalize to a future multi-event mutation without redesign; every mutation wired up through
/// Task 4 still only ever supplies one suffix, for the reason each of `start`/`pause`/`resume`/
/// `cancel`'s own doc comments give: a command's *decision* event (the one, always-present event
/// that means "this command happened") derives from the command key, while any variable-count
/// fan-out the command also appends (`pause`'s per-node holds, `cancel`'s per-node cancels,
/// `resume`'s crash-recovery and redispatch records, `start`/`resume`'s `drive_to_quiescence` hops)
/// keeps minting fresh keys exactly as before this task — a variable count cannot derive
/// deterministic keys from a fixed suffix, and (per `run_idempotent_mutation`'s own doc comment) a
/// retry recognized as `Complete` from the one decision key never re-enters the command at all, so
/// the fan-out's fresh keys are never at risk of a caller-triggered double-apply. `keys` staying a
/// `Vec` (rather than becoming a single `DerivedKey` now that every caller supplies exactly one)
/// keeps `Partial` reachable in the type even though it stays practically unreachable today —
/// documented here rather than redesigned away, matching Task 3's own precedent for this same
/// field.
struct MutationIdentity {
    actor: PersistedActor,
    keys: Vec<DerivedKey>,
    decision_kind: MutationDecisionKind,
    request_body: serde_json::Value,
    /// The bare `Idempotency-Key` header value the caller sent, before any suffix or content
    /// digest — named in a `KeyState::Divergent` diagnostic so the caller sees exactly which
    /// header value was reused across two different request bodies.
    idempotency_header: String,
    if_match: Option<u64>,
    /// #1054: the model this session DECLARED for itself, or `None` because it declared
    /// nothing. There is no third state and no default: absent stays absent all the way to
    /// the journal, where it is the absence of an event rather than an event carrying a
    /// guess.
    declared_model: Option<String>,
    /// #1054: only ever `Some` alongside `declared_model` -- an effort without a model is
    /// refused at the header edge, because it describes a thing nobody named.
    declared_effort: Option<DeclaredEffort>,
    /// #1057: WHICH SESSION is speaking, opaque and minted by the client, or `None` from a caller
    /// that names no session at all.
    ///
    /// An actor id is stable across sessions and a model is a property of ONE session, so without
    /// this the two cannot be told apart in the journal: a later session reusing the id and
    /// declaring nothing inherited the previous session's model forever, because "declared
    /// nothing" was said by writing no event, and silence cannot supersede a record.
    declared_session: Option<String>,
}

/// The maximum accepted length of the caller-supplied `Idempotency-Key` header, before any suffix
/// or content digest is appended. Chosen so the derived per-event key —
/// `"{header}-{suffix}-{digest16}"` — always stays within `OpaqueId`'s own 128-character cap
/// (`core/protocols/src/persistence.rs`'s `is_opaque_id`, read and confirmed: non-empty, <= 128
/// bytes, a closed punctuation/alphanumeric set that a lowercase-hex digest and every suffix below
/// both satisfy) with headroom to spare. The arithmetic: 64 (header) + 1 (dash) + 9 (the longest
/// suffix any mutation uses, `"cancelled"`) + 1 (dash) + 16 (digest, `DIGEST_HEX_LEN`) = 91 <= 128.
const IDEMPOTENCY_HEADER_MAX_LEN: usize = 64;
/// #1054: the maximum accepted length of `X-GraphHelm-Actor-Model`, in bytes.
///
/// 128 is not a fresh number: it is `OpaqueId`'s own cap (`core/protocols/src/persistence.rs`'s
/// `is_opaque_id`), and a model name is exactly the shape that cap was chosen for -- an opaque,
/// caller-chosen, identifier-like string this repository must not enumerate. The longest vendor
/// name in play today is under 50 bytes, so the cap refuses abuse without refusing anyone's real
/// model. The SAME number is written into the schema as `maxLength`, so a producer that never
/// passes through this header still cannot put an unbounded string in the journal.
const ACTOR_MODEL_HEADER_MAX_LEN: usize = 128;
/// Hex characters of the content digest folded into every derived event key — see
/// `IDEMPOTENCY_HEADER_MAX_LEN`'s doc comment for the arithmetic this feeds, and
/// `request_digest16`'s for why 16 is enough.
const DIGEST_HEX_LEN: usize = 16;

/// Validates the three required mutation headers *before anything touches the store*, per the
/// endpoint contract, and derives this command's event key(s), one per entry in `suffixes` —
/// each folding in a digest of `body` so a caller reusing this header for a genuinely different
/// request is detectable later (`classify_one_key`) rather than silently absorbed. `execution_id`
/// and `body` both feed that digest; see `request_digest16`.
/// `X-GraphHelm-Actor-Type` accepts only `owner` or `agent` — `human` and `system` are real
/// `PersistedActorType` variants but are refused from the wire (humans arrive with Studio auth, not
/// yet built; `system` stays reserved for the driver's own hops, whose attribution this API never
/// overrides).
///
/// `Err` carries the already-built `Response` to return directly (this function's every call site
/// is `match ... { Err(response) => return response, ... }`), which is larger than clippy's
/// default threshold; boxing it would add an allocation to every one of the header-validation
/// failure paths this function exists to keep cheap, for a function called once off a cold path
/// per request, so the lint is accepted here rather than worked around.
#[allow(clippy::result_large_err)]
fn parse_mutation_headers(
    headers: &HeaderMap,
    command: &'static str,
    execution_id: &str,
    body: &serde_json::Value,
    suffixes: &[&str],
) -> Result<MutationIdentity, Response> {
    let idempotency = header_value(headers, "idempotency-key").ok_or_else(|| {
        mutation_bad_request(command, "Idempotency-Key is required", "/idempotencyKey")
    })?;
    if idempotency.len() > IDEMPOTENCY_HEADER_MAX_LEN {
        return Err(mutation_bad_request(
            command,
            "Idempotency-Key must be at most 64 characters",
            "/idempotencyKey",
        ));
    }
    let actor_id_header = header_value(headers, "x-graphhelm-actor")
        .ok_or_else(|| mutation_bad_request(command, "X-GraphHelm-Actor is required", "/actor"))?;
    let actor_type_header = header_value(headers, "x-graphhelm-actor-type").ok_or_else(|| {
        mutation_bad_request(command, "X-GraphHelm-Actor-Type is required", "/actorType")
    })?;

    let actor_type = match actor_type_header {
        "owner" => PersistedActorType::Owner,
        "agent" => PersistedActorType::Agent,
        _ => {
            return Err(mutation_bad_request(
                command,
                "X-GraphHelm-Actor-Type must be \"owner\" or \"agent\"",
                "/actorType",
            ));
        }
    };
    let actor_id = ActorId::parse(actor_id_header).map_err(|_| {
        mutation_bad_request(
            command,
            "X-GraphHelm-Actor is not a valid identifier",
            "/actor",
        )
    })?;

    // #1054: the presence declaration, read HERE beside the actor it describes and refused the same
    // way, through the same `mutation_bad_request` -- before anything touches the store, which is
    // what makes "a refused request appends nothing, not even the signal it carried" true by
    // construction rather than by a cleanup path that could be forgotten.
    //
    // Both headers are OPTIONAL, and their absence is not a failure: a session that declares
    // nothing is a normal session, it simply gets no presence event. Neither header has a default.
    //
    // The model is CALLER-CONTROLLED AND PERSISTED, so it is BOUNDED here rather than trusted.
    // Three bounds, each refused with its own message naming what was wrong -- one message covering
    // three causes sends the next reader to the wrong one:
    //
    // - CHARSET. `HeaderValue` holds bytes 0x20..=0xFF, while `to_str` succeeds only for visible
    //   ASCII, so a Latin-1 or otherwise non-ASCII byte is REPRESENTABLE on the wire and refused
    //   here. This guard can fire; it is not decoration for something hyper already did.
    // - LENGTH. `ACTOR_MODEL_HEADER_MAX_LEN`. Nothing upstream caps one header value at a useful
    //   size, and the store's own scan bounds the WHOLE envelope rather than this field -- see
    //   `AgentPresenceDeclared`'s doc comment, which cites that scan by file and line and says
    //   exactly what it does and does not do.
    // - NON-BLANK. `minLength: 1` accepts `" "`, which renders as a blank badge: present,
    //   unreadable, and indistinguishable from a rendering bug.
    //
    // Read RAW rather than through `header_value`, on purpose: that helper maps an EMPTY value to
    // `None`, i.e. to "this session declared nothing". For a header a client sends only when it has
    // something to say, an empty value is a client defect, and answering it with silent absence
    // hides the defect in the one place nobody looks. Present-and-blank is refused; truly absent
    // stays absent.
    let declared_model = match headers.get("x-graphhelm-actor-model") {
        None => None,
        Some(raw_model) => {
            let Ok(value) = raw_model.to_str() else {
                return Err(mutation_bad_request(
                    command,
                    "X-GraphHelm-Actor-Model must contain only visible ASCII characters",
                    "/actorModel",
                ));
            };
            if value.len() > ACTOR_MODEL_HEADER_MAX_LEN {
                return Err(mutation_bad_request(
                    command,
                    "X-GraphHelm-Actor-Model must be at most 128 characters",
                    "/actorModel",
                ));
            }
            if value.trim().is_empty() {
                return Err(mutation_bad_request(
                    command,
                    "X-GraphHelm-Actor-Model must not be blank",
                    "/actorModel",
                ));
            }
            Some(value.to_owned())
        }
    };
    // The vocabulary is CLOSED, which is the only reason this is refusable at all. A free string
    // could only be recorded and hoped about; three named members can be checked.
    //
    // Read RAW rather than through `header_value`, for the reason the model is (#1057, Codex P2).
    // That helper answers `None` for BOTH a missing header and a present-but-empty one, and it
    // answers `None` again for a value `to_str()` cannot decode -- so `X-GraphHelm-Actor-Effort:`
    // with an empty value, and one carrying a Latin-1 byte, each reached the store as "no effort
    // declared" and the request succeeded. Both are PRESENT values outside the advertised closed
    // vocabulary, and a client that sent either has a defect that silent absence hides in the one
    // place nobody looks. Truly absent still stays absent.
    let declared_effort = match headers.get("x-graphhelm-actor-effort") {
        None => None,
        Some(raw_effort) => {
            let Ok(value) = raw_effort.to_str() else {
                return Err(mutation_bad_request(
                    command,
                    "X-GraphHelm-Actor-Effort must contain only visible ASCII characters",
                    "/actorEffort",
                ));
            };
            match value {
                "low" => Some(DeclaredEffort::Low),
                "medium" => Some(DeclaredEffort::Medium),
                "high" => Some(DeclaredEffort::High),
                _ => {
                    return Err(mutation_bad_request(
                        command,
                        "X-GraphHelm-Actor-Effort must be \"low\", \"medium\" or \"high\"",
                        "/actorEffort",
                    ));
                }
            }
        }
    };
    // An effort with no model is half a declaration. Recording it would leave a renderer to invent
    // the other half, which is exactly the inference this whole feature forbids.
    if declared_effort.is_some() && declared_model.is_none() {
        return Err(mutation_bad_request(
            command,
            "X-GraphHelm-Actor-Effort requires X-GraphHelm-Actor-Model",
            "/actorModel",
        ));
    }

    // #1057: the session token, read RAW for the same reason the model is -- `header_value` maps an
    // empty value to `None`, and a client that sends this header at all has something to say, so
    // present-and-blank is a client defect rather than "no session". Bounded identically: visible
    // ASCII, at most `ACTOR_MODEL_HEADER_MAX_LEN` bytes, not blank. It is caller-controlled and it
    // is PERSISTED, so it is bounded at the door and again in the schema.
    let declared_session = match headers.get("x-graphhelm-actor-session") {
        None => None,
        Some(raw_session) => {
            let Ok(value) = raw_session.to_str() else {
                return Err(mutation_bad_request(
                    command,
                    "X-GraphHelm-Actor-Session must contain only visible ASCII characters",
                    "/actorSession",
                ));
            };
            if value.len() > ACTOR_MODEL_HEADER_MAX_LEN {
                return Err(mutation_bad_request(
                    command,
                    "X-GraphHelm-Actor-Session must be at most 128 characters",
                    "/actorSession",
                ));
            }
            if value.trim().is_empty() {
                return Err(mutation_bad_request(
                    command,
                    "X-GraphHelm-Actor-Session must not be blank",
                    "/actorSession",
                ));
            }
            Some(value.to_owned())
        }
    };

    let digest = request_digest16(command, execution_id, body)?;

    let mut keys = Vec::with_capacity(suffixes.len());
    for suffix in suffixes {
        let prefix = format!("{idempotency}-{suffix}");
        let full = OpaqueId::parse(format!("{prefix}-{digest}")).map_err(|_| {
            mutation_bad_request(
                command,
                "Idempotency-Key is not a valid identifier",
                "/idempotencyKey",
            )
        })?;
        keys.push(DerivedKey { prefix, full });
    }

    // Milestone 05a Task 4: `If-Match`, optional on every mutation. A plain non-negative integer
    // head sequence (not an HTTP ETag's quoted-string form — the endpoint contract names the value
    // as `<head-sequence>` directly), validated here alongside the other mutation headers, before
    // anything touches the store.
    let if_match = match header_value(headers, "if-match") {
        None => None,
        Some(raw) => match raw.parse::<u64>() {
            Ok(value) => Some(value),
            Err(_) => {
                return Err(mutation_bad_request(
                    command,
                    "If-Match must be a non-negative integer head sequence",
                    "/ifMatch",
                ));
            }
        },
    };

    Ok(MutationIdentity {
        actor: PersistedActor::new(actor_type, actor_id),
        keys,
        decision_kind: MutationDecisionKind::for_command(command),
        request_body: body.clone(),
        idempotency_header: idempotency.to_owned(),
        if_match,
        declared_model,
        declared_effort,
        declared_session,
    })
}

/// The content identity folded into a mutation's derived event key(s) (see
/// `IDEMPOTENCY_HEADER_MAX_LEN`'s module doc comment for why the pre-flight needed this at all).
/// Hashes the command name, the execution id, and the request body's *canonical* JSON bytes —
/// `serde_json::to_vec` of the already-parsed `Value`, not the raw wire bytes, so two
/// byte-different-but-semantically-identical bodies (differing only in whitespace, or in JSON
/// object key order) still digest identically rather than falsely reading as divergent. This
/// relies on `serde_json::Value::Object` serializing in a canonical (sorted-key) order regardless
/// of insertion order — true for this workspace's `serde_json` because the `preserve_order`
/// feature is not enabled anywhere in the dependency graph (`Cargo.lock` carries no `indexmap`),
/// so `Map` is `BTreeMap`-backed; verified directly by `digest_is_stable_across_key_order`
/// below, not merely assumed.
///
/// Each part is length-prefixed (4-byte big-endian) before concatenation — the same
/// domain-separation convention `core/graph/src/persistence.rs`'s `push_slot_position_part` uses
/// for its own content identities — so `"ab"` + `"c"` can never collide with `"a"` + `"bc"`.
/// Returns the first `DIGEST_HEX_LEN` lowercase hex characters of the resulting SHA-256: short
/// enough to keep the derived key well inside `OpaqueId`'s cap, long enough (16 hex characters is
/// 64 bits of entropy) that an accidental collision between two genuinely different bodies is not
/// a practical concern for what this digest exists to do. This digest is a divergence *detector*,
/// not a security boundary. The bearer token authenticates access to the local Runtime, not the
/// durable actor header: `classify_existing_keys` must still bind an exact key to the committed
/// actor, execution scope/stream, and decision event kind before it can call the request a retry.
///
/// `Err` carries the already-built `Response` to return directly, the same
/// `clippy::result_large_err` accepted-not-worked-around tradeoff `parse_mutation_headers` and
/// `routes::load_and_publish` already document — boxing it would add an allocation to a failure
/// path this function exists to keep cheap, for a function called at most once per mutation
/// request.
#[allow(clippy::result_large_err)]
fn request_digest16(
    command: &'static str,
    execution_id: &str,
    body: &serde_json::Value,
) -> Result<String, Response> {
    let canonical = serde_json::to_vec(body).map_err(|_| {
        mutation_bad_request(command, "the request body could not be canonicalized", "/")
    })?;
    let mut identity =
        Vec::with_capacity(command.len() + execution_id.len() + canonical.len() + 12);
    for part in [
        command.as_bytes(),
        execution_id.as_bytes(),
        canonical.as_slice(),
    ] {
        let length = u32::try_from(part.len()).unwrap_or(u32::MAX);
        identity.extend_from_slice(&length.to_be_bytes());
        identity.extend_from_slice(part);
    }
    let digest = raw_content_sha256(&identity).map_err(|_| {
        mutation_bad_request(command, "the request digest could not be computed", "/")
    })?;
    Ok(digest.as_str()[..DIGEST_HEX_LEN].to_owned())
}

fn header_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
}

fn mutation_bad_request(command: &'static str, message: &str, pointer: &str) -> Response {
    respond(
        StatusCode::BAD_REQUEST,
        Outcome::domain(
            command,
            vec![Diagnostic::error(ARGUMENT_CODE, message, pointer, SOURCE)],
        )
        .output,
    )
}

/// The error a mutation's `run` closure can produce. `Command` is a normal `execution::Failure`
/// from the command layer — still routed through this function's own `GHE003`/`GHE001`
/// reclassification below, reachable via the `?`-friendly `From` impl just below. `Prepared` is an
/// already-built `Response` from a step upstream of the command layer: today, only `start`/
/// `resume`'s graph load/lint (`routes::load_and_publish`), deferred until the idempotency
/// pre-flight has established the command is genuinely fresh (Milestone 05a follow-up Minor 1: a
/// recognized retry must not re-parse/re-lint/re-publish — see `routes::start`/`routes::resume`,
/// which now call `load_and_publish` from *inside* the closure they pass here, on the `Absent`
/// path only). `Prepared` bypasses the code-based reclassification entirely and is returned
/// verbatim: a schema/lint failure is not a store conflict, and carries its own richer
/// multi-diagnostic shape (`Vec<Diagnostic>`) that `execution::Failure`'s single
/// code/message/pointer cannot represent without losing detail.
enum MutationError {
    Command(execution::Failure),
    Prepared(Response),
}

impl From<execution::Failure> for MutationError {
    fn from(failure: execution::Failure) -> Self {
        Self::Command(failure)
    }
}

/// Runs one attributed, idempotent mutation end to end: a pre-flight check for whether this exact
/// command (identified by `identity.keys`) has already fully or partially landed — or landed
/// *differently* under the same header, see `KeyState::Divergent` — then, only if genuinely fresh,
/// `run`, which gets the parsed actor and this mutation's one derived key (see
/// `MutationIdentity`'s doc comment on the one-event-per-command assumption every mutation this
/// task wires up satisfies).
///
/// The pre-flight check exists because a command's *own* domain preconditions are not safe to
/// blindly re-run on retry: `approve` refuses a node that is not `Ghost`/`Blocked`, which is
/// exactly the state a node is in *after* the first, already-committed attempt succeeded.
/// Re-running the command body on a retry would fail with `GHCLI005_EXECUTION_STATE` — a real
/// domain error — before ever reaching the store's own idempotency check, breaking "a full retry
/// is success, not conflict". Checking the store directly first for "has this command already
/// happened" sidesteps that: retry recognition depends only on the store's own record, never on
/// whether a given command's preconditions happen to tolerate being re-evaluated against
/// already-advanced state.
///
/// The store append inside `run` is still the authority for a *fresh* command (two racing fresh
/// attempts can't both win — the store's exclusive append lock serializes them), and the
/// post-append `GHE003_IDEMPOTENCY_CONFLICT` arm below is the same classification applied again for
/// the narrow race where two identical retries both pass the pre-flight check before either
/// commits.
/// The boxed-future shape every mutation's `run` closure now returns (Milestone 05d Task 9: the
/// `start`/`resume` drive half is genuinely async — `spawn_blocking`, `tokio::select!` — so
/// `run_idempotent_mutation` itself became `async fn` and awaits this rather than calling a plain
/// synchronous closure). Every other mutation (`signal`/`approve`/`pause`/`cancel`) simply wraps
/// its unchanged synchronous body in `Box::pin(async move { ... })`.
type MutationFuture<'a> =
    Pin<Box<dyn Future<Output = Result<serde_json::Value, MutationError>> + Send + 'a>>;

/// #248: which executor this deployment can run against. A positional `bool` here was flagged in
/// review (M's review of #424) as easy to pass backwards silently -- the wrong value still
/// compiles, and the failure mode would be exactly the silence this diagnostic exists to close.
/// The enum makes the wrong value a name mismatch a reader catches at the call site, not a
/// swapped `true`/`false` that reads the same either way.
///
/// Per KIND since #1066 (Codex, on #1073): the two halves are wired independently, so "real" is
/// a question about cognitive nodes and a question about tool nodes, and a single answer was
/// wrong for one of them in every half-wired deployment — a model-only server let a tool node
/// "succeed" from a fixture with no diagnostic, and a tools-only server printed "no
/// real-executor wiring" over a tool host that was running.
#[derive(Clone, Copy, PartialEq, Eq)]
struct ExecutorWiring {
    /// Cognitive nodes (agent, planner, classifier, evaluator) run on a real model route.
    model: bool,
    /// Tool nodes run on the real tool host.
    tools: bool,
}

impl ExecutorWiring {
    /// Neither half wired: every node is answered by fixtures — the 05a server.
    const FIXTURE_ONLY: Self = Self {
        model: false,
        tools: false,
    };

    fn from_state(state: &ServeState) -> Self {
        match state.runtime.as_ref() {
            Some(wiring) => Self {
                model: wiring.model.is_some(),
                tools: wiring.tools.is_some(),
            },
            None => Self::FIXTURE_ONLY,
        }
    }

    /// Whether any kind of node is answered by fixtures on this deployment.
    const fn any_fixture(self) -> bool {
        !self.model || !self.tools
    }

    /// The words the GHCLI021/022 diagnostics use for what fixtures answer here.
    fn fixture_kinds(self) -> &'static str {
        match (self.model, self.tools) {
            (false, false) => "every node (fixture-only mode)",
            (false, true) => "cognitive nodes (tools-only mode: the tool host is real)",
            (true, false) => "tool nodes (model-only mode: the model route is real)",
            (true, true) => "no node",
        }
    }

    /// The executor `start` declares on the form (#1063), read off the SAME predicate that
    /// decides the fixture-only warning above - one answer to "what runs the nodes here", so
    /// the declaration and the diagnostic cannot disagree.
    ///
    /// The declaration has two words (`fixture` | `gateway`) and the wiring is per kind
    /// (#1066): it is `gateway` the moment EITHER half is real, since a node then runs on a
    /// real port and a briefing must not say `fixture` over a tool host that ran `cargo test`.
    /// Which kinds fixtures still answer is what GHCLI021/022 name on the reply.
    pub(super) const fn declared(self) -> graphhelm_protocols::DeclaredExecutor {
        if self.model || self.tools {
            graphhelm_protocols::DeclaredExecutor::Gateway
        } else {
            graphhelm_protocols::DeclaredExecutor::Fixture
        }
    }
}

async fn run_idempotent_mutation<'a>(
    events: &Path,
    execution: &str,
    command: &'static str,
    identity: MutationIdentity,
    wiring: ExecutorWiring,
    run: impl FnOnce(PersistedActor, OpaqueId) -> MutationFuture<'a>,
) -> Response {
    run_idempotent_mutation_inner(
        events,
        execution,
        command,
        identity,
        wiring,
        MutationObservation {
            after_absent_preflight: std::future::ready(()),
            after_presence_read: std::future::ready(()),
            post_append_conflict: || {},
        },
        run,
    )
    .await
}

/// The mutation algorithm with two private observation points. Production always supplies a
/// ready future and a no-op closure through `run_idempotent_mutation`; in-process tests can inject
/// a Tokio rendezvous without adding an environment variable, wire route, sleep, filesystem
/// write, or other behavior reachable from the shipped server.
struct MutationObservation<A, P, C> {
    after_absent_preflight: A,
    /// #1054: between `record_presence_declaration`'s READ of the stream and its APPEND to it.
    ///
    /// That gap is the whole subject of the race this seam exists to measure. Every other
    /// interleaving in this function serializes by itself on a current-thread runtime -- the
    /// presence path is synchronous once it starts, so without a suspension point planted INSIDE
    /// it, two racing tasks simply take turns and the second one sees the first one's committed
    /// declaration. A cell written without this seam would pass while measuring nothing.
    after_presence_read: P,
    post_append_conflict: C,
}

async fn run_idempotent_mutation_inner<'a>(
    events: &Path,
    execution: &str,
    command: &'static str,
    identity: MutationIdentity,
    wiring: ExecutorWiring,
    observation: MutationObservation<
        impl Future<Output = ()> + Send,
        impl Future<Output = ()> + Send,
        impl FnOnce() + Send,
    >,
    run: impl FnOnce(PersistedActor, OpaqueId) -> MutationFuture<'a>,
) -> Response {
    match classify_existing_keys(events, execution, &identity) {
        Ok(KeyState::Complete(original_decision_sequence)) => {
            return reply_with_current_status(
                events,
                execution,
                command,
                wiring,
                original_decision_sequence,
            );
        }
        Ok(KeyState::Partial(stuck)) => {
            return partial_conflict_response(events, execution, command, &stuck);
        }
        Ok(KeyState::Divergent(header)) => {
            return divergent_conflict_response(events, execution, command, &header);
        }
        Ok(KeyState::Absent) => {}
        Err(failure) => return respond_failure(command, failure),
    }

    observation.after_absent_preflight.await;

    // Milestone 05a Task 4: `If-Match`, checked here — after the idempotency pre-flight has
    // already established this is a genuinely fresh command, never a retry of one that already
    // committed, and immediately before the command body (`run`) is invoked, per the endpoint
    // contract ("checked against the stream head immediately before the command body"). A retry of
    // an already-completed command is handled entirely by the classification above and never
    // reaches this check at all: the command already happened, so a caller's now-stale `If-Match`
    // (set before that first attempt) describes a race that is moot for an outcome already fixed.
    //
    // This is advisory-fast-fail, not the authority: the gap between this read and `run`'s own
    // append below is still open to a racing writer. The store remains the authority for real —
    // its own serialized append (`next_sequence` checked again immediately before every append) is
    // what actually closes that gap, surfacing as `GHE001_SEQUENCE_CONFLICT` from `run` itself,
    // caught by the `conflict_with_current_head` arm below exactly as it already was before this
    // task. `If-Match` only lets a caller fail fast on a head it already knows is stale, without
    // waiting for the store to discover the same thing one append later.
    if let Some(expected) = identity.if_match {
        let head = current_head(events, execution);
        if head != Some(expected) {
            return if_match_conflict(command, head);
        }
    }

    let key = identity.keys[0].full.clone();
    match run(identity.actor.clone(), key).await {
        Ok(mut value) => {
            // #1054 / #1057 Codex P1: THE DECLARATION IS RECORDED ONLY AFTER THE BODY
            // COMMITTED. It used to be appended immediately before `run`, which meant a refused
            // `start` (an invalid graph) or a refused `approve` (a state check) answered 400/409
            // and still left `agent_presence_declared` behind it -- and for `start`, a STREAM
            // that `execution list` reads as a run with no `ExecutionStarted` in it. The store is
            // append-only, so nothing takes that back afterwards.
            //
            // The order is therefore work-then-declaration, and a reader meets the declaration
            // beside the event that earned it rather than before it. That is the whole of what
            // was given up, and it buys the narrower claim: a declaration now asserts who sent a
            // request that SUCCEEDED.
            if let Err(failure) = record_presence_declaration(
                events,
                execution,
                &identity,
                observation.after_presence_read,
            )
            .await
            {
                // NEVER SILENT. The mutation itself is durable at this point and cannot be
                // unwound, so this arm reports a request that did half of what it was asked: the
                // work landed, the declaration did not. It is reachable only from a store that
                // stopped being writable between two adjacent calls, or from
                // `PRESENCE_COMMIT_ATTEMPTS` consecutive lost races -- and a caller told nothing
                // would leave a board showing a model that was never recorded.
                return respond_failure(command, failure);
            }
            // Milestone 05a follow-up Important: a fresh mutation success previously carried no
            // `headSequence` (only `status` and a recognized retry's `reply_with_current_status`
            // did, both via `execution::status::execute`), forcing an extra GET per
            // `If-Match`-chained write. `current_head` costs one more store open beyond what `run`
            // just did internally — every mutation's own `execute()` returns only
            // `render(&projection)`'s aggregate view, never the raw event list a head sequence
            // needs, so there is no history already in hand here to reuse instead. Avoiding that
            // extra open would mean widening every command's return shape across
            // `execution/{signal,approve,pause,resume,cancel,start}.rs`, outside this fix's
            // footprint in the serve layer alone.
            if let serde_json::Value::Object(ref mut map) = value {
                map.insert(
                    "headSequence".to_owned(),
                    serde_json::json!(current_head(events, execution)),
                );
            }
            // 05g: the mutation's append is durable at this point — sweep the wake leases,
            // fire-and-forget (a wake failure never fails the route that triggered it; the
            // ring only ever happens AFTER durability, which is the sabotage-pinned order).
            tokio::spawn(wake::sweep(
                std::sync::Arc::from(events),
                execution.to_owned(),
            ));
            let mut outcome = Outcome::success(command, value);
            // #248: `NodeState::WaitingInput` is produced by exactly one path in this codebase
            // today -- `FixtureExecutor::execute` answering `NeedsInput` for a node with no
            // fixture (`core/simulation/src/executor.rs`). The real executor (`PortExecutor`,
            // `core/runtime/src/executor.rs`) is deliberately built to never return `NeedsInput`,
            // and an unsupported node type is refused *before* dispatch rather than parked -- so
            // there is no other route to this state on `main` today. That makes it
            // indistinguishable from an operator-legitimate wait ONLY while this deployment has
            // no real-executor wiring: `ok: true`, `200`, `status: "running"` all read as normal
            // progress. Naming the mode here, rather than refusing the mutation outright, is the
            // smaller of the issue's two suggested fixes -- it does not risk breaking a caller
            // that (today, only in a fixture-driven test harness) relies on the park-and-200
            // shape for a deliberately unanswered fixture.
            //
            // Checked against the STORE, not against `value`'s own JSON shape: two of the nine
            // mutation endpoints sharing this function (`signal`, `wake-lease`) never render a
            // `nodeStateCounts` field at all (measured -- neither calls `execution::render`), so
            // a check keyed on that field would silently never fire for them regardless of the
            // execution's real state, which is exactly the "reads as normal progress" failure
            // this diagnostic exists to close (M's review of #424). Re-reading the projection
            // costs one more store open, the same shape `current_head` above already pays on
            // every successful mutation.
            // The check has a THIRD outcome, not just present/absent (M's second finding on
            // #424): a store read can fail. Answering `false` there would recreate exactly the
            // ambiguity this diagnostic exists to close -- an unreadable store reads as "no
            // problem" the same way a genuinely calm execution does, `ok: true`/`200` either
            // way. So a read failure gets its OWN diagnostic naming that the check could not run,
            // rather than silently agreeing with the calm case. Narrow path (this store was just
            // written to by the mutation above), kept non-silent anyway rather than assumed safe.
            annotate_fixture_only(&mut outcome.output, events, execution, wiring);
            respond(StatusCode::OK, outcome.output)
        }
        Err(MutationError::Prepared(response)) => response,
        Err(MutationError::Command(failure)) if failure.code == "GHE003_IDEMPOTENCY_CONFLICT" => {
            (observation.post_append_conflict)();
            // The race the module doc names: another attempt with the same key committed between
            // the pre-flight check above and this append. Classify again against the store's
            // now-current state rather than trusting the in-flight guess either way.
            match classify_existing_keys(events, execution, &identity) {
                Ok(KeyState::Complete(original_decision_sequence)) => reply_with_current_status(
                    events,
                    execution,
                    command,
                    wiring,
                    original_decision_sequence,
                ),
                Ok(KeyState::Partial(stuck)) => {
                    partial_conflict_response(events, execution, command, &stuck)
                }
                Ok(KeyState::Divergent(header)) => {
                    divergent_conflict_response(events, execution, command, &header)
                }
                // The store just told us this exact key set conflicted; the store's own read
                // reporting "not found" a moment later would be a genuine inconsistency, not a
                // case to paper over. Surface the original conflict honestly instead of guessing.
                Ok(KeyState::Absent) => respond_failure(command, failure),
                Err(lookup_failure) => respond_failure(command, lookup_failure),
            }
        }
        Err(MutationError::Command(failure)) if failure.code == "GHE001_SEQUENCE_CONFLICT" => {
            conflict_with_current_head(events, execution, command)
        }
        Err(MutationError::Command(failure)) => respond_failure(command, failure),
    }
}

/// Whether none, some, or all of a command's expected event keys are already committed —
/// distinguishing a clean retry (`Complete`) from a stuck partial application (`Partial`, naming
/// the first missing key) from a divergent reuse (`Divergent`, naming the reused header) — read
/// directly off `history`, the same raw stream `resolve_stream` already loads for this call (no
/// second store open): see `classify_one_key`.
enum KeyState {
    Absent,
    /// Every expected key exists. Carries the committed sequence of the command's first
    /// server-derived decision key, never the current head or a caller-supplied sequence.
    Complete(u64),
    Partial(OpaqueId),
    /// A committed event's key shares this command's `{command-key}-{suffix}-` prefix but was
    /// derived from a *different* request body (a different content digest) — the caller reused
    /// this bare `Idempotency-Key` (carried here) for two logically different requests.
    Divergent(String),
}

/// One derived key's status against already-committed `history`. An exact match (suffix *and*
/// content digest both) means this exact command already happened — `Present`, unconditionally,
/// regardless of any other sibling below (this is what keeps `Complete` classification "unchanged
/// semantics" even against legacy history that predates this digest, or against the losing side of
/// a two-writer race — see the second call site in `run_idempotent_mutation`). Absent an exact
/// match, a committed event whose key shares this one's `prefix` but carries a *different* digest
/// means the caller's header was already spent on a different authenticated mutation identity —
/// `Divergent`. Neither found at all — genuinely fresh — `Missing`.
enum KeyPresence {
    /// The exact derived key exists at this committed event sequence.
    Present(u64),
    Divergent,
    Missing,
}

/// The full proof identity for one lookup. Every field comes from the authenticated request or the
/// exact stream `resolve_stream` selected. Nothing from a mismatched event is returned to the
/// caller; it changes only the classification to `Divergent`.
struct ExpectedDecision<'a> {
    actor: &'a PersistedActor,
    scope: &'a RepositoryScope,
    stream: &'a str,
    execution: &'a str,
    kind: MutationDecisionKind,
    request_body: &'a serde_json::Value,
}

impl ExpectedDecision<'_> {
    fn matches(&self, event: &EventEnvelope) -> bool {
        event.actor == *self.actor
            && event.scope == *self.scope
            && event.stream_id.as_str() == self.stream
            && event
                .scope
                .execution_id()
                .is_some_and(|execution| execution.as_str() == self.execution)
            && self
                .kind
                .matches(&event.kind, self.execution, self.request_body)
    }
}

/// Prefix matching is a plain string `starts_with`, not a delimited-field comparison, so a caller
/// whose own `Idempotency-Key` header happens to literally contain another of its own headers plus
/// suffix as a leading substring (e.g. sending `"sig-cmd-record-extra"` to `signal` after
/// previously sending `"sig-cmd"` to the same command) could trigger a false `Divergent` against
/// itself. This is deliberately not hardened against: every key this pre-flight ever compares
/// belongs to requests made under the one bearer token this server issues (Milestone 05a Task 1).
/// A prefix collision can therefore refuse work inside that authenticated local deployment, but
/// cannot authorize it: the exact-key path below additionally requires the committed actor,
/// scope/stream, execution and decision kind. The digest is divergence *detection*, not a
/// cryptographic boundary; see `request_digest16`'s doc comment.
fn classify_one_key(
    history: &[EventEnvelope],
    derived: &DerivedKey,
    expected: &ExpectedDecision<'_>,
) -> KeyPresence {
    let reused_prefix = format!("{}-", derived.prefix);
    let mut divergent = false;
    for event in history {
        let existing = event.idempotency_key.as_str();
        if existing == derived.full.as_str() {
            return if expected.matches(event) {
                KeyPresence::Present(event.sequence)
            } else {
                KeyPresence::Divergent
            };
        }
        if existing.starts_with(&reused_prefix) {
            divergent = true;
        }
    }
    if divergent {
        KeyPresence::Divergent
    } else {
        KeyPresence::Missing
    }
}

fn classify_existing_keys(
    events: &Path,
    execution: &str,
    identity: &MutationIdentity,
) -> Result<KeyState, execution::Failure> {
    let store = event_store(events).map_err(|error| execution::repository_failure(&error))?;
    // `resolve_stream`'s raw `history` is reused directly below for every key's classification —
    // Complete/Partial *and* the new Divergent check — rather than a second, per-key store read
    // (the pre-fix version called `committed_events_for_idempotency` once per key; both it and
    // `resolve_stream`/`read_replay_stream` read from the exact same underlying committed batches,
    // so scanning `history` once here is strictly equivalent for Complete/Partial and additionally
    // enables Divergent detection for free).
    let (scope, stream, history) = execution::resolve_stream(&store, Some(execution))?;
    let expected = ExpectedDecision {
        actor: &identity.actor,
        scope: &scope,
        stream: &stream,
        execution,
        kind: identity.decision_kind,
        request_body: &identity.request_body,
    };
    let mut present = 0_usize;
    let mut original_decision_sequence = None;
    let mut first_missing = None;
    for derived in &identity.keys {
        match classify_one_key(&history, derived, &expected) {
            KeyPresence::Present(sequence) => {
                present += 1;
                if original_decision_sequence.is_none() {
                    original_decision_sequence = Some(sequence);
                }
            }
            KeyPresence::Divergent => {
                return Ok(KeyState::Divergent(identity.idempotency_header.clone()));
            }
            KeyPresence::Missing => {
                if first_missing.is_none() {
                    first_missing = Some(derived.full.clone());
                }
            }
        }
    }
    Ok(if present == 0 {
        KeyState::Absent
    } else if present == identity.keys.len() {
        KeyState::Complete(
            original_decision_sequence
                .expect("a complete non-empty derived-key set has a first decision sequence"),
        )
    } else {
        KeyState::Partial(first_missing.expect("some but not all present implies one missing"))
    })
}

/// "Reply 200 with current state, because the caller's command has already happened" (the plan's
/// own words for the full-retry case) — literally the same `execution.status` read the CLI and
/// `GET /v1/executions/{id}` both use, plus one additive `idempotency` proof naming the original
/// decision sequence. This is a deliberate divergence from a *fresh* success's own bespoke
/// response shape (`signal`'s `decision`/`mayProposeMutation`, for one) — recomputing that bespoke
/// shape on retry would mean re-deriving a governance verdict against already-advanced state, the
/// same category of problem `run_idempotent_mutation`'s pre-flight check exists to avoid;
/// "current status" is the one shape every mutation can report honestly no matter how long ago
/// the original attempt actually committed, while the proof distinguishes that history from a
/// fresh application.
fn reply_with_current_status(
    events: &Path,
    execution: &str,
    command: &'static str,
    wiring: ExecutorWiring,
    original_decision_sequence: u64,
) -> Response {
    match execution::status::execute(events, Some(execution)) {
        Ok(value) => reply_with_status_value(
            events,
            execution,
            command,
            wiring,
            value,
            original_decision_sequence,
        ),
        Err(failure) => respond_failure(command, failure),
    }
}

fn reply_with_status_value(
    events: &Path,
    execution: &str,
    command: &'static str,
    wiring: ExecutorWiring,
    mut value: serde_json::Value,
    original_decision_sequence: u64,
) -> Response {
    if let Err(diagnostic) = annotate_recognized_retry(&mut value, original_decision_sequence) {
        return respond(
            StatusCode::INTERNAL_SERVER_ERROR,
            Outcome::domain(command, vec![diagnostic]).output,
        );
    }
    let mut output = Outcome::success(command, value).output;
    annotate_fixture_only(&mut output, events, execution, wiring);
    respond(StatusCode::OK, output)
}

fn annotate_recognized_retry(
    status: &mut serde_json::Value,
    original_decision_sequence: u64,
) -> Result<(), Diagnostic> {
    let serde_json::Value::Object(data) = status else {
        return Err(Diagnostic::error(
            IDEMPOTENCY_REPLY_CODE,
            "the recognized retry status could not carry its idempotency proof",
            "/data",
            SOURCE,
        ));
    };
    data.insert(
        "idempotency".to_owned(),
        serde_json::json!({
            "recognizedRetry": true,
            "originalDecisionSequence": original_decision_sequence,
        }),
    );
    Ok(())
}

/// A previous attempt at this command applied only part of it (possible only if that attempt died
/// mid-command, before every one of its events committed) — refused rather than silently continued
/// from a half-applied state, naming the first event key that never landed so the caller knows
/// exactly what did not happen and can decide how to recover.
fn partial_conflict_response(
    events: &Path,
    execution: &str,
    command: &'static str,
    stuck_key: &OpaqueId,
) -> Response {
    let head = current_head(events, execution);
    respond(
        StatusCode::CONFLICT,
        CommandOutput {
            ok: false,
            command,
            data: Some(serde_json::json!({ "currentHead": head })),
            diagnostics: vec![Diagnostic::error(
                "GHE003_IDEMPOTENCY_CONFLICT",
                format!(
                    "a previous attempt at this command applied only part of it; the event \
                     keyed \"{stuck_key}\" was never committed, so this Idempotency-Key cannot be \
                     retried as-is"
                ),
                "/idempotencyKey",
                SOURCE,
            )],
        },
    )
}

/// The caller's `Idempotency-Key` resolves to a different authenticated mutation identity — body,
/// actor, execution scope/stream, or decision kind. Refuse without returning which committed field
/// mismatched: the bare header is already caller-owned and is the only value safe and useful to
/// name in the diagnostic.
fn divergent_conflict_response(
    events: &Path,
    execution: &str,
    command: &'static str,
    header: &str,
) -> Response {
    let head = current_head(events, execution);
    respond(
        StatusCode::CONFLICT,
        CommandOutput {
            ok: false,
            command,
            data: Some(serde_json::json!({ "currentHead": head })),
            diagnostics: vec![Diagnostic::error(
                "GHE003_IDEMPOTENCY_CONFLICT",
                format!(
                    "the Idempotency-Key \"{header}\" is already committed to a different \
                     authenticated mutation identity; a retry must match the original request \
                     body, actor, execution, and decision kind"
                ),
                "/idempotencyKey",
                SOURCE,
            )],
        },
    )
}

/// `GHE001_SEQUENCE_CONFLICT` mapped to 409 with the current head attached, per the endpoint
/// contract's failure-mapping table ("store conflicts ... → 409 with `currentHead`"). Not exercised
/// by name in this task's own tests (it needs a genuine two-writer race, which Task 5's storm test
/// is what actually drives), but reachable any time two different commands race between this
/// mutation's own `next_sequence` read and its append, so it is handled honestly here rather than
/// left to fall through to the unattributed 500 default.
fn conflict_with_current_head(events: &Path, execution: &str, command: &'static str) -> Response {
    let head = current_head(events, execution);
    respond(
        StatusCode::CONFLICT,
        CommandOutput {
            ok: false,
            command,
            data: Some(serde_json::json!({ "currentHead": head })),
            diagnostics: vec![Diagnostic::error(
                "GHE001_SEQUENCE_CONFLICT",
                "the stream's head moved between this request's read and its append; re-read and \
                 retry"
                    .to_owned(),
                "/",
                SOURCE,
            )],
        },
    )
}

/// `If-Match` named a head the store does not currently have: 409 with the actual current head
/// attached (per the endpoint contract: "the 409 body's `data` carries `{"currentHead": N}` so the
/// caller re-reads and retries"), same JSON shape as `conflict_with_current_head` and the same
/// `GHE001_SEQUENCE_CONFLICT` code — both name the same class of problem (the caller's assumed head
/// does not match the store's real one), just detected at different times: that one reactively, from
/// the store's own append-time check; this one proactively, from the caller's own precondition.
/// `head` is `None` only if even this read fails (repository unavailable) — the conflict is still
/// real and worth reporting even then, matching `current_head`'s own documented best-effort
/// contract.
fn if_match_conflict(command: &'static str, head: Option<u64>) -> Response {
    respond(
        StatusCode::CONFLICT,
        CommandOutput {
            ok: false,
            command,
            data: Some(serde_json::json!({ "currentHead": head })),
            diagnostics: vec![Diagnostic::error(
                "GHE001_SEQUENCE_CONFLICT",
                "If-Match does not match the stream's current head; re-read and retry",
                "/ifMatch",
                SOURCE,
            )],
        },
    )
}

/// Best-effort current head for a conflict response's `currentHead`: `None` (omitted on the wire)
/// only if even this read fails, which would mean the repository itself is unavailable — the
/// conflict is still real and still worth reporting even then.
/// # THE PRESENCE INVARIANT (#1054). Read this before changing anything below it.
///
/// > A presence declaration is appended when it DIFFERS FROM THE NEWEST declaration this actor
/// > already has in this stream.
///
/// That is the rule for a request that REACHES this function. An earlier version ended the
/// sentence "Nothing else suppresses it", which is FALSE and is the same overclaim class as the
/// one the paragraph below exists to correct. Three things return before this function is called,
/// and none of them writes a declaration:
///
/// - a header refusal in `parse_mutation_headers` -- a bad effort, a blank/over-long/non-ASCII
///   model, or an effort with no model -- which is a 400 before anything touches the store;
/// - any non-`Absent` outcome of the idempotency pre-flight (a recognised retry, a stuck partial,
///   a divergent key reuse, a failed classification) and an `If-Match` mismatch, all of which
///   return from `run_idempotent_mutation_inner` ABOVE its call to this function;
/// - every READ path, which never enters this code at all.
///
/// A fourth was added by #1057's P1 and is the reason this function now runs LATE: a command body
/// that REFUSES. The declaration is planned and written only after `run` answered `Ok`, so a
/// `start` whose graph does not validate and an `approve` whose state check refuses each leave the
/// journal exactly as they found it -- no declaration, and for `start` no stream either.
///
/// It is NOT "at most one per distinct `(actorId, model, effort)` per stream", which is what an
/// earlier version of this comment and of #1054's own task brief both said. That sentence is
/// FALSE about this code, and the sequence that separates the two readings is the FLIP-FLOP:
/// `medium -> high -> medium` appends THREE events, and the third carries a triple the stream
/// already holds. Under the false reading that third append is a bug; under the true one it is the
/// point. `a_flip_flop_appends_a_third_declaration_because_newest_is_what_a_roster_answers` pins
/// it, because a rule nobody can fail to obey is not a rule.
///
/// NEWEST-WINS IS THE RIGHT SEMANTICS, and the reason is what a roster is for: it answers "what is
/// this agent running NOW". A dedup keyed on "has this triple ever appeared" would refuse to record
/// a session going back to `medium`, and the roster would keep answering `high` forever -- a fact
/// that stopped being true, preserved by the very mechanism meant to keep the journal honest.
///
/// What the comparison buys is therefore narrower than "no duplicates ever", and exactly worth
/// having: an IMMEDIATE repeat is free. Without it every mutation would append, and a session
/// sending a hundred signals would put a hundred identical declarations in the stream -- a journal
/// whose size stopped meaning anything, and a roster that costs more to read on every request that
/// did not change it.
///
/// The comparison is against the JOURNAL, not against any in-memory session state, because the
/// Runtime holds no session state: a restarted server, a second client, and a replay all have to
/// reach the same answer, and only the stream is common to all three.
///
/// EFFORT IS PART OF THE IDENTITY. A session that turns its thinking from `medium` to `high` has
/// changed what it is, and a roster still answering `medium` would be reporting a fact that stopped
/// being true. That is why the triple and not just the model.
///
/// PER ACTOR, not per stream. Two agents working one execution each declare their own model;
/// comparing against the stream's newest would make each arrival erase the other's and the stream
/// would grow on every alternating request.
///
/// AS OF THE READ, and that phrase is enforced rather than hoped for: see `PlannedPresence`, whose
/// `expected_next_sequence` binds the append to the exact stream the decision was made against, so
/// a stream that moved in between refuses the write instead of absorbing a stale decision.
///
/// ---
///
/// Decides whether this mutation's declaration is NEWS, and returns what to write if it is.
///
/// Reads only. `Ok(None)` means one of the two ways there is nothing to write: the caller declared
/// nothing (and then no store is read at all), or this actor's NEWEST declaration in this stream
/// already carries exactly this triple.
/// A presence declaration that has been decided on but not yet written, CARRYING THE SEQUENCE THE
/// DECISION WAS MADE AGAINST.
///
/// That field is the whole point of splitting the write in two. The dedup decision ("is this
/// news?") is made from a READ of the stream, and the append happens later: without binding the two,
/// a second writer can commit in between and both requests append the same triple, each having read
/// a stream that honestly held no such declaration. That is not a hypothesis -- it is what this
/// path DID, measured through the `after_presence_read` seam before this field existed: two
/// attempts, two `200`s, two identical declarations in the journal.
///
/// `expected_next_sequence` closes it with the store's own optimistic-concurrency primitive.
/// `append_locked` refuses when the stream's actual next sequence is not the one the request names
/// (`EventRepositoryError::SequenceConflict`), and it makes that check INSIDE its exclusive lock,
/// which is the only place in this system where "check and append" is one operation.
///
/// THE NUMBER IS DERIVED FROM THE HISTORY THAT MADE THE DECISION -- `history.last().sequence + 1`,
/// or 1 for an empty stream -- and NOT from a `store.next_sequence(...)` call. That distinction is
/// the whole guarantee, and getting it wrong is how the first version of this comment came to
/// assert a safeguard the code did not provide.
///
/// `resolve_stream` and `next_sequence` are two INDEPENDENT `with_shared_lock` + `load_state`
/// acquisitions (`core/events/src/local.rs:2647` and `:2668`). A writer committing between them
/// answers a sequence NEWER than the snapshot the dedup decision was made against -- so the append
/// names a number the store agrees with, lands, and the duplicate this field exists to prevent
/// happens anyway. Two adjacent synchronous calls with no `await` between them is a small window,
/// not a closed one, and "small" is not a property a comment may round to "closed".
///
/// Deriving from `history` removes the second snapshot entirely. The history IS the first opinion;
/// `next_sequence` was a second one taken later against a state nobody consulted.
///
/// The two can DISAGREE only when something committed TO THIS STREAM in between. Not "when
/// nothing committed in between" -- that was a false biconditional, and the counter-example is
/// cheap: a writer committing to a DIFFERENT stream advances the store's state and leaves both
/// numbers exactly where they were, because `next_sequence` is keyed per `stream_key`. So the
/// agreement is not evidence that the store was idle; it is evidence about THIS stream only, and
/// that is the whole of what the append needs.
///
/// The derivation is equivalent to the store's own for a quiescent stream, and that is checked
/// rather than asserted (`the_planned_sequence_is_the_one_the_store_would_answer`). It holds
/// because `build_envelopes` numbers a batch `expected_next_sequence + offset` and `append_locked`
/// then stores `expected + count`, so a stream's sequences are contiguous and its next is always
/// its last plus one (`core/events/src/local.rs:1241` and `:2029`).
///
/// IT ALSO FAILS SAFE, which is the property worth having when a derivation could be wrong. Too
/// low, and the store refuses -- a conflict, never a duplicate. Too high cannot arise from
/// `last + 1` over a real history. So the worst outcome of a stale read is a 409 the caller
/// retries, and never a silently duplicated declaration.
///
/// This is deliberately NOT `execution::append_event`, which asks the store for `next_sequence`
/// itself immediately before appending. That helper is correct for a caller whose decision does not
/// depend on the stream's contents; here the decision does, so re-deriving the sequence at write
/// time would discard the very fact that makes the write safe.
struct PlannedPresence {
    scope: RepositoryScope,
    stream_id: OpaqueId,
    expected_next_sequence: u64,
    actor: PersistedActor,
    declared: AgentPresenceDeclared,
}

fn plan_presence_declaration(
    events: &Path,
    execution: &str,
    identity: &MutationIdentity,
) -> Result<Option<PlannedPresence>, execution::Failure> {
    // #1057: TWO ways there is something to record, and the second one is the fix.
    //
    // A caller that declares a MODEL records it, as it always did. A caller that names a SESSION
    // records that session's boundary even with no model at all, because "this session declares
    // nothing" has to be SAID: written as an event with no `model`. Expressed by silence it could
    // never supersede the previous session's record, and a stable actor id reused by a new session
    // inherited a model that had stopped being true.
    //
    // A caller that does neither is unchanged: no session, no model, nothing read and nothing
    // written. That is every plain HTTP caller that never learned about either header.
    if identity.declared_model.is_none() && identity.declared_session.is_none() {
        return Ok(None);
    }
    let declared = AgentPresenceDeclared {
        actor_id: identity.actor.id().clone(),
        actor_type: identity.actor.actor_type(),
        model: identity.declared_model.clone(),
        effort: identity.declared_effort,
        session: identity.declared_session.clone(),
    };

    let store = event_store(events).map_err(|error| execution::repository_failure(&error))?;
    let (scope, stream, history) = execution::resolve_stream(&store, Some(execution))?;
    if newest_declaration_for(&history, &declared).is_some_and(|newest| *newest == declared) {
        return Ok(None);
    }

    // `resolve_stream` derived this string from an id it had already parsed, so the refusal below
    // is unreachable on this path today. It is a refusal rather than an `expect` because
    // unreachable-today is a property of the current arrangement, not of this function: a future
    // `resolve_stream` that answered a stream name of its own choosing would turn an `expect` into
    // a panic inside a live request.
    let stream_id = OpaqueId::parse(stream).map_err(|_| {
        execution::argument(
            "the resolved stream id is not a valid identifier",
            "/execution",
        )
    })?;
    // Derived from THE HISTORY THE DECISION WAS MADE FROM, never from a fresh
    // `store.next_sequence(...)` -- see `PlannedPresence` for why a second store read reopens the
    // window this whole field exists to close. `history` is `read_replay_stream`'s raw, unfiltered
    // stream, so its last event is the stream's head.
    let expected_next_sequence = history
        .last()
        .map_or(1, |event| event.sequence.saturating_add(1));
    Ok(Some(PlannedPresence {
        scope,
        stream_id,
        expected_next_sequence,
        actor: identity.actor.clone(),
        declared,
    }))
}

/// Writes a planned declaration AT THE SEQUENCE ITS DECISION WAS MADE AGAINST, so a stream that
/// moved in between refuses the write instead of absorbing a stale decision.
///
/// Opens the store again rather than carrying one across the seam: `LocalEventRepository` takes its
/// exclusive lock per call, and holding one open across a suspension point is how a test seam turns
/// into a deadlock in production. Re-opening is safe precisely because the sequence travels in
/// `planned` -- the second handle cannot quietly answer a newer number.
fn commit_presence_declaration(
    events: &Path,
    planned: PlannedPresence,
) -> Result<(), execution::Failure> {
    let store = event_store(events).map_err(|error| execution::repository_failure(&error))?;
    let request = PreparedAppend::new(
        planned.scope,
        planned.stream_id,
        planned.expected_next_sequence,
        vec![NewEvent::new(
            execution::idempotency_key("presence"),
            planned.actor,
            Sensitivity::Internal,
            EventKind::AgentPresenceDeclared(planned.declared),
            vec![],
            vec![],
        )],
        vec![],
        vec![],
    )
    .map_err(|error| execution::repository_failure(&error))?;
    store
        .append_atomic(&request)
        .map(|_| ())
        .map_err(|error| execution::repository_failure(&error))
}

/// How many times a presence append may lose the head race before the request says so.
///
/// A conflict here is not a caller error and not a lost mutation: the command body has ALREADY
/// committed, so retrying costs one more read of a stream the loser is not contending for on any
/// second pass -- its re-plan either finds the winner's identical declaration and writes nothing,
/// or names the new head. Three is a bound, not a tuned number: it exists so a pathological writer
/// cannot spin this loop, and the third failure is REPORTED rather than swallowed.
const PRESENCE_COMMIT_ATTEMPTS: usize = 3;

/// Plans and writes this mutation's declaration, AFTER its command body committed.
///
/// `seam` is awaited exactly once, between the first plan's READ and its APPEND: that is the only
/// suspension point the presence path has, and the race cell plants a barrier in it. Later attempts
/// do not re-await it, because a seam that fired on every pass would rendezvous a retry with a task
/// that already finished and hang the cell.
///
/// The loop exists because the order changed. While the declaration was written BEFORE the body, a
/// lost race could be answered with a 409 and the caller could retry the whole request, nothing
/// having landed. Now the body has already committed when this runs, so a 409 would name a conflict
/// for a mutation that succeeded -- the caller would retry work that is already done. Re-planning
/// is the honest answer instead: it reads the stream the winner just moved and, for the same triple,
/// finds its own declaration already newest and writes nothing at all.
async fn record_presence_declaration<P>(
    events: &Path,
    execution: &str,
    identity: &MutationIdentity,
    seam: P,
) -> Result<(), execution::Failure>
where
    P: std::future::Future<Output = ()>,
{
    let mut seam = Some(seam);
    let mut attempts = 0;
    loop {
        attempts += 1;
        let Some(planned) = plan_presence_declaration(events, execution, identity)? else {
            return Ok(());
        };
        if let Some(seam) = seam.take() {
            seam.await;
        }
        match commit_presence_declaration(events, planned) {
            Ok(()) => return Ok(()),
            Err(failure)
                if failure.code == "GHE001_SEQUENCE_CONFLICT"
                    && attempts < PRESENCE_COMMIT_ATTEMPTS => {}
            Err(failure) => return Err(failure),
        }
    }
}

/// The newest declaration in `history` made BY THE SAME ACTOR as `subject`.
///
/// Per actor, and that is the whole grain: two agents working one execution each declare their own
/// model, and a comparison against "the newest declaration in the stream" would make each one's
/// arrival erase the other's -- every mutation would then be news, and the stream would grow on
/// every request, which is precisely the failure the comparison exists to prevent.
fn newest_declaration_for<'a>(
    history: &'a [EventEnvelope],
    subject: &AgentPresenceDeclared,
) -> Option<&'a AgentPresenceDeclared> {
    history.iter().rev().find_map(|event| match &event.kind {
        EventKind::AgentPresenceDeclared(declared) if declared.actor_id == subject.actor_id => {
            Some(declared)
        }
        _ => None,
    })
}

fn current_head(events: &Path, execution: &str) -> Option<u64> {
    let store = event_store(events).ok()?;
    let (_, _, history) = execution::resolve_stream(&store, Some(execution)).ok()?;
    Some(history.last().map_or(0, |event| event.sequence))
}

/// Whether ANY committed `ExecutionPaused` event in this stream's history carries exactly this
/// (key, actor) pair.
///
/// **Was "the LAST one", and that lost committed requests** (#710, Codex P2 on `4f56cafe`). A
/// pause is not the end of a stream: X pauses under caller A's key, resumes, and pauses again
/// under caller B's key. A's event is still durably in history and A's request genuinely
/// committed — but it is no longer the LATEST, so A's handler compared against B's pair, found no
/// match, ran out its ten-second budget and answered `409 GHE003_IDEMPOTENCY_CONFLICT` for a
/// request that succeeded. A conflict is what a caller is told when someone else holds its key;
/// telling it that about its OWN committed pause is the opposite of what the code means.
///
/// The whole history is the population because the append-only log is the record: an event that
/// committed does not stop having committed when a later one lands on top of it. The Studio's
/// client half reads the same way and for the same reason — it accepts a pause as proof of its
/// own request only when the event carries its derived key prefix AND its actor, never merely
/// because an `execution_paused` exists (`apps/studio/src/runtime/client.ts`, PR #662).
///
/// #681, Codex P1's second finding: the immediate branch's success condition was "the execution
/// is paused", which two concurrently racing immediate-pause requests against the same live
/// execution can BOTH observe true for -- the driver commits only ONE request's payload
/// (`ImmediateCancelRequest`, captured once at detection), and polling the aggregate status
/// cannot tell a caller whether ITS OWN signal is what committed or a racing caller's did. This
/// answers that: the caller compares its own derived (key, actor) against what's actually on the
/// ledger before claiming success.
///
/// Actor is part of the comparison, not just the key (Codex P1, later review round):
/// `request_digest16` folds in command, execution id and body -- never actor -- so two different
/// actors racing with the SAME literal `Idempotency-Key` header and body derive the identical
/// `signalled_key`. A key-only check would let both callers read their own key on the ledger and
/// both claim success, even though only one of their (actor, key) pairs is what actually
/// committed -- exactly the attribution gap `classify_existing_keys`'s own `ExpectedDecision`
/// already guards against at the pre-flight, undone by checking the key alone after the fact.
fn execution_paused_under(
    events: &Path,
    execution: &str,
    key: &OpaqueId,
    actor: &PersistedActor,
) -> PausedUnderCaller {
    let Ok(store) = event_store(events) else {
        return PausedUnderCaller::Unreadable;
    };
    let Ok((_, _, history)) = execution::resolve_stream(&store, Some(execution)) else {
        return PausedUnderCaller::Unreadable;
    };
    // BACKWARDS, and the direction is the whole cost of this function (#734's shape, found by
    // ISSUES 4 reviewing this PR). The widened population is the fix -- a committed pause stays
    // findable after a later one lands on top of it -- but the caller's own pause is at or near
    // the TAIL, so a forward scan finds it last, after touching every event in the stream. This
    // runs inside the immediate-pause poll loop, once per poll, for up to ten seconds, over a
    // history that grows for the execution's life. Scanning from the end restores the early exit
    // the predecessor had by only ever looking at the last event, without giving back the
    // population that made it wrong.
    let found = history.iter().rev().any(|event| {
        matches!(event.kind, EventKind::ExecutionPaused(_))
            && &event.idempotency_key == key
            && &event.actor == actor
    });
    if found {
        PausedUnderCaller::Committed
    } else {
        PausedUnderCaller::Absent
    }
}

/// Three answers, because a store that could not be read is not a store that says no (#710).
///
/// The predecessor returned `Option<(key, actor)>` and the caller compared it for equality, so an
/// unreadable store and a genuinely different pair produced the same `false` — and the poll loop
/// treated both as "keep waiting", then reported a conflict. Collapsing a read failure into a
/// negative verdict is the shape this file already refuses one function down, for the same reason
/// it refuses it there: an unreadable store and a calm one are different facts and only one of
/// them is the caller's problem.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Whether THIS caller's pause is on the ledger.
///
/// `Unreadable` is deliberately distinct from `Absent` even though the poll loop treats both as
/// "keep waiting" (ISSUES 4, reviewing this PR): the two are opposite facts -- one says the store
/// answered and this pair is not there, the other says the store did not answer at all -- and
/// collapsing them at the TYPE is what let the predecessor report a conflict for a store it never
/// read. Control flow acting the same on both is correct here, because a caller cannot distinguish
/// "not yet" from "cannot tell" while the budget is still running. The distinction is carried so a
/// future diagnostic can say WHICH of the two exhausted the budget, and so that a later change
/// cannot silently start treating an unreadable store as a negative answer.
enum PausedUnderCaller {
    /// An `ExecutionPaused` carrying exactly this (key, actor) is in the history.
    Committed,
    /// The history was read and holds no such event.
    Absent,
    /// The store could not be opened or replayed; this says nothing about the caller's request.
    Unreadable,
}

/// #248: the three answers to "is any node in this execution parked in `waiting_input`", read
/// fresh from the store rather than from a mutation response's own JSON shape (see the call
/// site's own comment for why: `nodeStateCounts` is not a field every mutation renders).
/// `Undetermined` is its own variant rather than folded into `Absent` (M's second finding on
/// #424): collapsing a read failure into "no problem" would recreate the exact ambiguity this
/// diagnostic exists to close, just one layer down -- an unreadable store and a genuinely calm
/// execution would both answer with no warning.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum WaitingInputCheck {
    Present,
    Absent,
    Undetermined,
}

fn fixture_only_diagnostic(check: WaitingInputCheck, wiring: ExecutorWiring) -> Option<Diagnostic> {
    let kinds = wiring.fixture_kinds();
    match check {
        WaitingInputCheck::Present => Some(Diagnostic::warning(
            crate::error_codes::GHCLI021_FIXTURE_ONLY_WAITING_INPUT,
            format!(
                "this deployment answers {kinds} from node fixtures; such a node with no fixture \
                 answer parks in waiting_input and will never proceed on its own -- this is a \
                 configuration state, not a workflow wait"
            ),
            "/",
            SOURCE,
        )),
        WaitingInputCheck::Undetermined => Some(Diagnostic::warning(
            crate::error_codes::GHCLI022_FIXTURE_ONLY_STATE_UNDETERMINED,
            format!(
                "this deployment answers {kinds} from node fixtures, and whether any node is \
                 parked in waiting_input could not be determined -- the store could not be \
                 re-read after this mutation"
            ),
            "/",
            SOURCE,
        )),
        WaitingInputCheck::Absent => None,
    }
}

fn annotate_fixture_only(
    output: &mut CommandOutput,
    events: &Path,
    execution: &str,
    wiring: ExecutorWiring,
) {
    if wiring.any_fixture()
        && let Some(diagnostic) =
            fixture_only_diagnostic(check_waiting_input(events, execution), wiring)
    {
        output.diagnostics.push(diagnostic);
    }
}

fn check_waiting_input(events: &Path, execution: &str) -> WaitingInputCheck {
    let Ok(store) = event_store(events) else {
        return WaitingInputCheck::Undetermined;
    };
    let Ok((scope, stream, history)) = execution::resolve_stream(&store, Some(execution)) else {
        return WaitingInputCheck::Undetermined;
    };
    let Ok(projection) = graphhelm_events::replay(&scope, &stream, &history) else {
        return WaitingInputCheck::Undetermined;
    };
    if projection
        .node_states
        .values()
        .any(|state| *state == graphhelm_protocols::NodeState::WaitingInput)
    {
        WaitingInputCheck::Present
    } else {
        WaitingInputCheck::Absent
    }
}

/// Equal-length fold with `|=` so no early return leaks which byte differed. Lengths are not
/// secret (the token is always the fixed hex length in practice), so the length short-circuit
/// above this does not weaken the comparison.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn parse_loopback_bind(bind: &str) -> Result<SocketAddr, Failure> {
    let address: SocketAddr = bind
        .parse()
        .map_err(|_| argument("--bind must be a numeric host:port address", "/bind"))?;
    if !address.ip().is_loopback() {
        return Err(serve_invalid(
            "--bind must name a loopback address; the Public Runtime API is never exposed beyond localhost",
            "/bind",
        ));
    }
    Ok(address)
}

/// Appends one JSON line per request: what was asked, and the exact bytes served.
///
/// This exists because four paid blind-judge runs produced findings that could not be checked:
/// reads do not write to the store, the server logged nothing, and the findings' own evidence
/// field came back empty. A probe that leaves no trace can only be re-run, never verified.
///
/// Three rules the shape encodes:
///
/// - It writes OUTSIDE the event store. A recorder that appended to the execution stream would
///   advance `headSequence` without advancing `lastEventAt`, and head movement would stop
///   implying progress - the surface poisoning the signal it exists to serve. That failure has
///   already been paid for once, through the wake lease.
/// - It never sees headers, so the bearer token cannot reach disk. Only method, path, query,
///   status and body are recorded.
/// - A failure to record is reported, never swallowed. An audit that silently stops recording
///   is worse than no audit: it reads as "the caller asked nothing".
async fn record_read(
    State(state): State<ServeState>,
    request: Request,
    next: axum::middleware::Next,
) -> Response {
    let Some(path_to_audit) = state.read_audit.clone() else {
        return next.run(request).await;
    };
    let method = request.method().to_string();
    let uri = request.uri().clone();
    let response = next.run(request).await;

    let (parts, body) = response.into_parts();
    let bytes = match axum::body::to_bytes(body, usize::MAX).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return respond(
                StatusCode::INTERNAL_SERVER_ERROR,
                Outcome::domain(
                    AUDIT_COMMAND,
                    vec![Diagnostic::error(
                        AUDIT_CODE,
                        "the response body could not be read for the audit",
                        "/read-audit",
                        "serve",
                    )],
                )
                .output,
            );
        }
    };

    // The body is recorded as a value when it parses as JSON and as a string otherwise, so a
    // reader can compare it against what a caller received without unquoting anything.
    let mut recorded_body =
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap_or_else(|_| {
            serde_json::Value::String(String::from_utf8_lossy(&bytes).into_owned())
        });
    // THE EVIDENCE ROUTE'S PLAINTEXT NEVER REACHES THE AUDIT. The repo spends real effort keeping
    // evidence out of durable plaintext (#488's refusal payload carries {code, local, bytes} and
    // deliberately no string field); an audit line transcribing the opened body would undo that
    // for anyone who turns --read-audit on (PR #467 review: the route is new, the middleware is
    // old, and the defect is the composition). The `content` field is replaced by its byte count;
    // the envelope's own `contentSha256` stays, so the audit still proves WHAT was served.
    if uri.path().contains("/evidence/") {
        if let Some(content) = recorded_body
            .get_mut("data")
            .and_then(|data| data.get_mut("content"))
        {
            let served = content.as_str().map(str::len).unwrap_or(0);
            *content = serde_json::json!({ "redacted": "evidence plaintext", "bytes": served });
        } else if recorded_body.is_string() {
            // A body that did not parse as JSON is recorded as one string - which on this route
            // could still be the plaintext. Replace it wholesale rather than trusting the shape.
            recorded_body =
                serde_json::json!({ "redacted": "evidence response", "bytes": bytes.len() });
        }
    }
    let line = serde_json::json!({
        "method": method,
        "path": uri.path(),
        "query": uri.query(),
        "status": parts.status.as_u16(),
        "body": recorded_body,
    });

    if let Err(error) = append_audit_line(&path_to_audit, &line) {
        return respond(
            StatusCode::INTERNAL_SERVER_ERROR,
            Outcome::domain(
                AUDIT_COMMAND,
                vec![Diagnostic::error(
                    AUDIT_CODE,
                    format!("the read audit could not be written: {error}"),
                    "/read-audit",
                    "serve",
                )],
            )
            .output,
        );
    }
    Response::from_parts(parts, axum::body::Body::from(bytes))
}

fn append_audit_line(path: &Path, line: &serde_json::Value) -> std::io::Result<()> {
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{line}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn a_cancel_retry_does_not_accept_a_successful_completion_with_the_same_execution() {
        let event = EventKind::ExecutionCompleted(graphhelm_protocols::ExecutionCompleted {
            execution_id: OpaqueId::parse("exec-cancel-proof").unwrap(),
            status: graphhelm_protocols::SimulationStatus::Completed,
        });

        assert!(
            !MutationDecisionKind::ExecutionCompleted.matches(
                &event,
                "exec-cancel-proof",
                &serde_json::Value::Null,
            ),
            "cancel must recognize only the cancelled decision, never an unrelated completion"
        );
    }

    #[test]
    fn same_kind_retry_proofs_are_bound_to_the_requested_action_payload() {
        use graphhelm_protocols::{
            ExecutionFormAmended, ExecutionMode, ExecutionStarted, NodeOutcome,
            NodeOutcomeRecorded, NodeState, PersistedTimestamp, SignalRecorded, SignalSeverity,
            SignalSourceKind, SweepCaller, SweepPerformed, WakeLease, WireHash,
        };

        let execution = "exec-action-proof";
        let execution_id = OpaqueId::parse(execution).unwrap();
        let assert_bound = |decision: MutationDecisionKind,
                            body: serde_json::Value,
                            matching: EventKind,
                            poisoned: EventKind| {
            assert!(
                decision.matches(&matching, execution, &body),
                "the real decision must remain recognizable: {matching:?}"
            );
            assert!(
                !decision.matches(&poisoned, execution, &body),
                "a same-kind event with different action data must not authenticate: {poisoned:?}"
            );
        };

        let start = |mode| {
            EventKind::ExecutionStarted(ExecutionStarted {
                execution_id: execution_id.clone(),
                graph_version: 1,
                graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
                mode,
            })
        };
        assert_bound(
            MutationDecisionKind::ExecutionStarted,
            serde_json::json!({"file": "graph.yaml", "mode": "supervised"}),
            start(ExecutionMode::Supervised),
            start(ExecutionMode::Manual),
        );

        let signal_body = serde_json::json!({
            "signal": {
                "id": "signal-requested",
                "source": {"type": "node", "id": "implementation"},
                "type": "no_progress",
                "severity": "high",
                "description": "bound by the envelope digest",
                "evidence": [],
                "emittedAt": "2026-08-30T00:00:00Z"
            },
            "evidenceOut": "evidence.json"
        });
        let signal_hash = graphhelm_graph::raw_content_sha256(
            &serde_json::to_vec(&signal_body["signal"]).unwrap(),
        )
        .unwrap();
        let signal = |id: &str| {
            EventKind::SignalRecorded(SignalRecorded {
                execution_id: execution_id.clone(),
                signal_id: OpaqueId::parse(id).unwrap(),
                source_kind: SignalSourceKind::Node,
                source_id: OpaqueId::parse("implementation").unwrap(),
                kind: "no_progress".to_owned(),
                severity: SignalSeverity::High,
                envelope_sha256: signal_hash.clone(),
            })
        };
        assert_bound(
            MutationDecisionKind::SignalRecorded,
            signal_body,
            signal("signal-requested"),
            signal("signal-poisoned"),
        );

        let outcome = |node: &str| {
            EventKind::NodeOutcomeRecorded(NodeOutcomeRecorded {
                execution_id: execution_id.clone(),
                node_id: OpaqueId::parse(node).unwrap(),
                outcome: NodeOutcome::Approved,
                next_state: NodeState::Ready,
                reason: None,
            })
        };
        assert_bound(
            MutationDecisionKind::NodeOutcomeRecorded,
            serde_json::json!({"node": "implementation"}),
            outcome("implementation"),
            outcome("deploy"),
        );

        let amendment = |node: &str, seconds| {
            let node = OpaqueId::parse(node).unwrap();
            EventKind::ExecutionFormAmended(ExecutionFormAmended {
                execution_id: execution_id.clone(),
                computed_at_sequence: 7,
                node_timeout_seconds: [(node.clone(), seconds)].into_iter().collect(),
                observed_silence_seconds: [(node, 3)].into_iter().collect(),
            })
        };
        assert_bound(
            MutationDecisionKind::ExecutionFormAmended,
            serde_json::json!({
                "node": "implementation",
                "seconds": 30,
                "computedAtSequence": 7
            }),
            amendment("implementation", 30),
            amendment("implementation", 60),
        );

        let sweep = |instant: &str| {
            EventKind::SweepPerformed(SweepPerformed {
                execution_id: execution_id.clone(),
                as_of: PersistedTimestamp::parse(instant).unwrap(),
                caller: SweepCaller::Operator,
            })
        };
        assert_bound(
            MutationDecisionKind::SweepPerformed,
            serde_json::json!({"asOf": "2026-08-30T00:00:00Z"}),
            sweep("2026-08-30T00:00:00Z"),
            sweep("2026-08-29T00:00:00Z"),
        );

        let wake = |session: &str| {
            EventKind::WakeLease(WakeLease {
                execution_id: execution_id.clone(),
                session_id: OpaqueId::parse(session).unwrap(),
                cursor: 11,
                rendezvous_id: OpaqueId::parse("rendezvous-requested").unwrap(),
                matures_in_seconds: Some(30),
            })
        };
        assert_bound(
            MutationDecisionKind::WakeLease,
            serde_json::json!({
                "sessionId": "session-requested",
                "rendezvousId": "rendezvous-requested",
                "cursor": 11,
                "maturesInSeconds": 30
            }),
            wake("session-requested"),
            wake("session-poisoned"),
        );

        // #159: the claim command's one decision event is claimed OR refused; both are bound to
        // the node the request named, and a clearance is bound to the claim it judged.
        use graphhelm_protocols::{
            ClaimAttestation, ClaimAttestationMode, ClearanceVerifier, CompletionClaimed,
            CompletionCleared, CompletionRefused, SafeCode,
        };
        let claimed = |node: &str| {
            EventKind::CompletionClaimed(CompletionClaimed {
                execution_id: execution_id.clone(),
                node: OpaqueId::parse(node).unwrap(),
                completes_wait_seq: 4,
                evidence: Vec::new(),
                attestation: ClaimAttestation {
                    asserter: OpaqueId::parse("agent-claimer").unwrap(),
                    mode: ClaimAttestationMode::OperatorAttested,
                },
            })
        };
        assert_bound(
            MutationDecisionKind::CompletionClaim,
            serde_json::json!({"file": "graph.yaml", "node": "implementation"}),
            claimed("implementation"),
            claimed("deploy"),
        );
        let refused = |wait_seq: u64| {
            EventKind::CompletionRefused(CompletionRefused {
                execution_id: execution_id.clone(),
                node: OpaqueId::parse("implementation").unwrap(),
                claimed_wait_seq: wait_seq,
                reason_code: SafeCode::parse("unknown_wait").unwrap(),
            })
        };
        assert_bound(
            MutationDecisionKind::CompletionClaim,
            serde_json::json!({"file": "graph.yaml", "node": "implementation", "waitSeq": 1}),
            refused(1),
            refused(4),
        );
        let cleared = |claim_seq: u64| {
            EventKind::CompletionCleared(CompletionCleared {
                execution_id: execution_id.clone(),
                claim_seq,
                verifier: ClearanceVerifier::MachineReplay {
                    manifest_hash: WireHash::parse(format!("sha256:{}", "c".repeat(64))).unwrap(),
                },
            })
        };
        assert_bound(
            MutationDecisionKind::CompletionCleared,
            serde_json::json!({"file": "graph.yaml", "claimSeq": 5}),
            cleared(5),
            cleared(6),
        );
    }

    #[tokio::test]
    async fn a_non_object_retry_status_is_a_serialized_structured_500_response() {
        let response = reply_with_status_value(
            Path::new("unused-because-annotation-fails-first"),
            "exec-unused",
            "execution.signal",
            ExecutorWiring::FIXTURE_ONLY,
            serde_json::json!("not-an-object"),
            17,
        );
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["ok"], false);
        assert_eq!(body["command"], "execution.signal");
        assert_eq!(body["diagnostics"][0]["code"], IDEMPOTENCY_REPLY_CODE);
        assert_eq!(body["diagnostics"][0]["path"], "/data");
        assert_eq!(body["diagnostics"][0]["source"], SOURCE);
    }

    #[tokio::test]
    async fn a_post_append_same_key_race_marks_only_the_loser_and_appends_once() {
        let directory = tempfile::tempdir().unwrap();
        let events = directory.path().join("events");
        let fixtures = directory.path().join("fixtures.json");
        std::fs::write(
            &fixtures,
            serde_json::to_vec(&serde_json::json!({
                "nodeOutcomes": {"implementation": "success", "deploy": "success"}
            }))
            .unwrap(),
        )
        .unwrap();
        let graph = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("examples/graphs/software-feature.yaml");
        let loaded = graphhelm_schema::load_graph(&graph).unwrap();
        let version =
            crate::commands::publish_loaded(&loaded, crate::commands::owner("owner-test")).unwrap();
        let execution_id = "exec-unit-retry-race";
        let started = execution::start::execute(
            &version,
            &events,
            Some(&fixtures),
            "supervised",
            Some(execution_id),
            execution::system_actor(),
            OpaqueId::parse("unit-start-key").unwrap(),
        );
        assert!(started.is_ok(), "the race fixture execution must start");

        let decision_key = OpaqueId::parse("unit-race-record-0123456789abcdef").unwrap();
        let actor = PersistedActor::new(
            PersistedActorType::Agent,
            ActorId::parse("agent-racer").unwrap(),
        );
        let signal_value = serde_json::json!({
            "id": "signal-unit-race",
            "source": {"type": "node", "id": "implementation"},
            "type": "no_progress",
            "severity": "high",
            "description": "the deploy stage needs a manual review",
            "evidence": ["exec-1"],
            "emittedAt": "2026-08-13T00:00:00Z"
        });
        let signal = serde_json::to_vec(&signal_value).unwrap();
        let evidence_out = directory.path().join("race-evidence.json");
        let identity = || MutationIdentity {
            actor: actor.clone(),
            keys: vec![DerivedKey {
                prefix: "unit-race-record".to_owned(),
                full: decision_key.clone(),
            }],
            decision_kind: MutationDecisionKind::SignalRecorded,
            request_body: serde_json::json!({
                "signal": signal_value.clone(),
                "evidenceOut": evidence_out.clone(),
            }),
            idempotency_header: "unit-race".to_owned(),
            if_match: None,
            // #1054: this cell is about a two-writer race on ONE derived key, so both attempts
            // declare nothing. A declaration here would put a second append in front of the one
            // being raced and change the subject.
            declared_model: None,
            declared_effort: None,
            declared_session: None,
        };
        let before = current_head(&events, execution_id).unwrap();
        let rendezvous = Arc::new(tokio::sync::Barrier::new(2));
        let post_append_conflicts = Arc::new(AtomicUsize::new(0));

        let attempt = |identity: MutationIdentity| {
            let events_for_run = events.clone();
            let signal = signal.clone();
            let evidence_out = evidence_out.clone();
            let rendezvous = Arc::clone(&rendezvous);
            let post_append_conflicts = Arc::clone(&post_append_conflicts);
            run_idempotent_mutation_inner(
                &events,
                execution_id,
                "execution.signal",
                identity,
                ExecutorWiring::FIXTURE_ONLY,
                MutationObservation {
                    after_absent_preflight: async move {
                        rendezvous.wait().await;
                    },
                    // Neither attempt declares, so the presence path returns `Ok(None)` before
                    // this seam is ever reached and a ready future is the honest value here.
                    after_presence_read: std::future::ready(()),
                    post_append_conflict: move || {
                        post_append_conflicts.fetch_add(1, Ordering::SeqCst);
                    },
                },
                move |event_actor, key| {
                    Box::pin(async move {
                        Ok(execution::signal::execute(
                            &events_for_run,
                            Some(execution_id),
                            &signal,
                            Some(&evidence_out),
                            event_actor,
                            key,
                            None,
                        )?)
                    })
                },
            )
        };

        let (first, second) = tokio::join!(attempt(identity()), attempt(identity()));
        let decode = |response: Response| async move {
            let status = response.status();
            let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap();
            (
                status,
                serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
            )
        };
        let (first, second) = tokio::join!(decode(first), decode(second));
        assert_eq!(first.0, StatusCode::OK, "{}", first.1);
        assert_eq!(second.0, StatusCode::OK, "{}", second.1);

        let store = event_store(&events).unwrap();
        let Ok((_scope, _stream, history)) = execution::resolve_stream(&store, Some(execution_id))
        else {
            panic!("the race fixture history must remain readable");
        };
        let matching = history
            .iter()
            .filter(|event| event.idempotency_key == decision_key)
            .collect::<Vec<_>>();
        assert_eq!(
            matching.len(),
            1,
            "the same decision key must commit exactly once"
        );
        assert!(matches!(
            matching[0].kind,
            graphhelm_protocols::EventKind::SignalRecorded(_)
        ));
        assert_eq!(
            current_head(&events, execution_id),
            Some(before + 1),
            "the two attempts must produce exactly one committed event"
        );
        assert_eq!(
            post_append_conflicts.load(Ordering::SeqCst),
            1,
            "exactly the append loser must enter post-conflict reclassification"
        );

        let replies = [&first.1, &second.1];
        let recognized = replies
            .iter()
            .filter(|reply| reply["data"]["idempotency"]["recognizedRetry"] == true)
            .collect::<Vec<_>>();
        assert_eq!(
            recognized.len(),
            1,
            "exactly one reply is the recognized loser"
        );
        assert_eq!(
            recognized[0]["data"]["idempotency"],
            serde_json::json!({
                "recognizedRetry": true,
                "originalDecisionSequence": matching[0].sequence,
            })
        );
    }

    /// #1054 fix round 2: the planned sequence is the one the STORE would answer, on a quiescent
    /// stream.
    ///
    /// Round 2 replaced a `store.next_sequence(...)` call with a value derived from the history the
    /// dedup decision was made from. That is a strictly better BINDING -- one snapshot instead of
    /// two -- but it is only an improvement if the NUMBER is the same when nothing has moved. A
    /// derivation that quietly answered something else would swap a rare duplicate for a constant
    /// 409, which is worse and would look like a flaky store rather than like this line.
    ///
    /// So the store is asked directly and the two are compared. Not a reimplementation of the
    /// derivation -- that would only assert that a line equals itself -- but the independent
    /// authority the derivation replaced.
    ///
    /// WHAT IT DOES NOT DISCRIMINATE, stated so nobody reads more into it: this store numbers
    /// streams from 1 with no gaps, so `history.len() + 1` and `history.last().sequence + 1` are
    /// equal here and this cell cannot tell them apart. It pins equivalence with the store, which
    /// is the property round 2 needed, and not the choice between those two spellings.
    #[test]
    fn the_planned_sequence_is_the_one_the_store_would_answer() {
        let directory = tempfile::tempdir().unwrap();
        let events = directory.path().join("events");
        let fixtures = directory.path().join("fixtures.json");
        std::fs::write(
            &fixtures,
            serde_json::to_vec(&serde_json::json!({
                "nodeOutcomes": {"implementation": "success", "deploy": "success"}
            }))
            .unwrap(),
        )
        .unwrap();
        let graph = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("examples/graphs/software-feature.yaml");
        let loaded = graphhelm_schema::load_graph(&graph).unwrap();
        let version =
            crate::commands::publish_loaded(&loaded, crate::commands::owner("owner-test")).unwrap();
        let execution_id = "exec-unit-presence-sequence";
        assert!(
            execution::start::execute(
                &version,
                &events,
                Some(&fixtures),
                "supervised",
                Some(execution_id),
                execution::system_actor(),
                OpaqueId::parse("unit-sequence-start-key").unwrap(),
            )
            .is_ok(),
            "the fixture execution must start"
        );

        let identity = MutationIdentity {
            actor: PersistedActor::new(
                PersistedActorType::Agent,
                ActorId::parse("agent-planner").unwrap(),
            ),
            keys: vec![DerivedKey {
                prefix: "unit-sequence".to_owned(),
                full: OpaqueId::parse("unit-sequence-0123456789abcdef").unwrap(),
            }],
            decision_kind: MutationDecisionKind::SignalRecorded,
            request_body: serde_json::json!({}),
            idempotency_header: "unit-sequence".to_owned(),
            if_match: None,
            declared_model: Some("claude-opus-5".to_owned()),
            declared_effort: Some(DeclaredEffort::High),
            declared_session: None,
        };

        // `execution::Failure` is not `Debug` (it carries operator-facing text and this crate keeps
        // it off the derive list), so the failure arm is matched rather than `expect`ed.
        let Ok(planned) = plan_presence_declaration(&events, execution_id, &identity) else {
            panic!("planning must succeed on a readable store");
        };
        let planned = planned.expect("an undeclared triple on a fresh execution is news");

        let store = event_store(&events).unwrap();
        let from_the_store = store
            .next_sequence(&planned.scope, planned.stream_id.as_str())
            .unwrap();
        assert_eq!(
            planned.expected_next_sequence, from_the_store,
            "the derived sequence must equal the store's own on a stream nothing else is writing"
        );
        // CONTROL: the fixture execution really has a history, so the equality above is not two
        // ways of saying "empty". Without this a store that answered 1 to everything would pass.
        assert!(
            from_the_store > 1,
            "CONTROL: the fixture stream must be non-empty for this comparison to mean anything"
        );
    }

    /// #1054 fix round 1, Important 2 + #1057 Codex P1: TWO DECLARING WRITERS, and what actually
    /// happens to the loser now that the declaration is written AFTER the command body.
    ///
    /// `plan_presence_declaration` READS the stream and `commit_presence_declaration` APPENDS to
    /// it: two store calls, so the head can move in between. The reviewer asked which of two
    /// outcomes occurs -- both attempts append the same triple, or one legitimate mutation turns
    /// into an error -- and the answer is still NEITHER, but it is reached differently.
    ///
    /// While the declaration was written BEFORE the body, the loser was answered 409 and nothing
    /// of its request had landed, so a retry was free. That ordering is what #1057's P1 removed: a
    /// refused body must leave no declaration, so the declaration now follows the body. The loser's
    /// signal has therefore ALREADY COMMITTED by the time its append is refused, and a 409 would
    /// name a conflict for a mutation that succeeded. `record_presence_declaration` re-plans
    /// instead, and the re-plan reads the winner's declaration and writes nothing.
    ///
    /// Measured here rather than argued:
    ///
    /// - exactly ONE presence event commits, so the duplicate outcome does not occur;
    /// - BOTH attempts answer 200, because both signals really did land;
    /// - the stream grows by exactly three events -- two signals and one declaration.
    ///
    /// THE SEAM IS LOAD-BEARING. `after_presence_read` suspends both tasks between their reads and
    /// their appends. Without it, on this current-thread runtime, the presence path runs start to
    /// finish without yielding, the two tasks simply take turns, and the second one READS the
    /// first one's committed declaration and dedups -- a green cell measuring no race at all. It
    /// fires on the FIRST plan only, deliberately: a seam that fired again on the retry would
    /// rendezvous with a task that already finished and hang the cell.
    #[tokio::test]
    async fn two_declaring_writers_commit_one_declaration_and_both_mutations_land() {
        let directory = tempfile::tempdir().unwrap();
        let events = directory.path().join("events");
        let fixtures = directory.path().join("fixtures.json");
        std::fs::write(
            &fixtures,
            serde_json::to_vec(&serde_json::json!({
                "nodeOutcomes": {"implementation": "success", "deploy": "success"}
            }))
            .unwrap(),
        )
        .unwrap();
        let graph = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("examples/graphs/software-feature.yaml");
        let loaded = graphhelm_schema::load_graph(&graph).unwrap();
        let version =
            crate::commands::publish_loaded(&loaded, crate::commands::owner("owner-test")).unwrap();
        let execution_id = "exec-unit-presence-race";
        assert!(
            execution::start::execute(
                &version,
                &events,
                Some(&fixtures),
                "supervised",
                Some(execution_id),
                execution::system_actor(),
                OpaqueId::parse("unit-presence-start-key").unwrap(),
            )
            .is_ok(),
            "the race fixture execution must start"
        );

        let actor = PersistedActor::new(
            PersistedActorType::Agent,
            ActorId::parse("agent-racer").unwrap(),
        );
        let evidence_out = directory.path().join("presence-race-evidence.json");
        // DIFFERENT decision keys, deliberately. Sharing one would make the attempts race on the
        // idempotency key -- the subject of the cell above -- and the presence race would never be
        // reached. Here each command body is legitimately its own mutation, and the only thing
        // they contend for is the stream head.
        let identity = |nth: u8| {
            let signal_value = serde_json::json!({
                "id": format!("signal-unit-presence-{nth}"),
                "source": {"type": "node", "id": "implementation"},
                "type": "no_progress",
                "severity": "high",
                "description": "the deploy stage needs a manual review",
                "evidence": ["exec-1"],
                "emittedAt": "2026-08-13T00:00:00Z"
            });
            let prefix = format!("unit-presence-{nth}");
            (
                MutationIdentity {
                    actor: actor.clone(),
                    keys: vec![DerivedKey {
                        full: OpaqueId::parse(format!("{prefix}-0123456789abcdef")).unwrap(),
                        prefix,
                    }],
                    decision_kind: MutationDecisionKind::SignalRecorded,
                    request_body: serde_json::json!({
                        "signal": signal_value.clone(),
                        "evidenceOut": evidence_out.clone(),
                    }),
                    idempotency_header: format!("unit-presence-{nth}"),
                    if_match: None,
                    // BOTH declare, and the same triple. That is the whole point: the dedup read
                    // says "news" to both of them, because neither has committed yet.
                    declared_model: Some("claude-opus-5".to_owned()),
                    declared_effort: Some(DeclaredEffort::High),
                    declared_session: None,
                },
                serde_json::to_vec(&signal_value).unwrap(),
            )
        };

        let before = current_head(&events, execution_id).unwrap();
        let rendezvous = Arc::new(tokio::sync::Barrier::new(2));

        let attempt = |(identity, signal): (MutationIdentity, Vec<u8>)| {
            let events_for_run = events.clone();
            let evidence_out = evidence_out.clone();
            let rendezvous = Arc::clone(&rendezvous);
            run_idempotent_mutation_inner(
                &events,
                execution_id,
                "execution.signal",
                identity,
                ExecutorWiring::FIXTURE_ONLY,
                MutationObservation {
                    after_absent_preflight: std::future::ready(()),
                    after_presence_read: async move {
                        rendezvous.wait().await;
                    },
                    post_append_conflict: || {},
                },
                move |event_actor, key| {
                    Box::pin(async move {
                        Ok(execution::signal::execute(
                            &events_for_run,
                            Some(execution_id),
                            &signal,
                            Some(&evidence_out),
                            event_actor,
                            key,
                            None,
                        )?)
                    })
                },
            )
        };

        let (first, second) = tokio::join!(attempt(identity(1)), attempt(identity(2)));
        let decode = |response: Response| async move {
            let status = response.status();
            let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap();
            (
                status,
                serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
            )
        };
        let (first, second) = tokio::join!(decode(first), decode(second));

        // Which task wins the presence race is a scheduling detail. What is NOT a detail is
        // that neither caller is punished for it: both command bodies committed before the race
        // began, so both answer 200.
        assert_eq!(
            [first.0, second.0],
            [StatusCode::OK, StatusCode::OK],
            "both mutations landed, so neither is told its own work conflicted: {} / {}",
            first.1,
            second.1
        );

        let store = event_store(&events).unwrap();
        let Ok((_scope, _stream, history)) = execution::resolve_stream(&store, Some(execution_id))
        else {
            panic!("the race fixture history must remain readable");
        };
        let declarations = history
            .iter()
            .filter(|event| {
                matches!(
                    event.kind,
                    graphhelm_protocols::EventKind::AgentPresenceDeclared(_)
                )
            })
            .count();
        assert_eq!(
            declarations, 1,
            "two racing declarations of the same triple commit exactly one event"
        );
        let signals = history
            .iter()
            .filter(|event| {
                matches!(
                    event.kind,
                    graphhelm_protocols::EventKind::SignalRecorded(_)
                )
            })
            .count();
        assert_eq!(
            signals, 2,
            "both command bodies ran to completion before either declaration was planned"
        );
        assert_eq!(
            current_head(&events, execution_id),
            Some(before + 3),
            "exactly three events committed in total: two signals and one declaration"
        );
    }

    /// The two-key probe `request_digest16`'s own doc comment promises: two JSON objects carrying
    /// the *same* two keys in *different* source/insertion order must serialize to byte-identical
    /// canonical output. Confirmed here directly, rather than merely assumed, because the whole
    /// point of canonicalizing through a parsed `Value` (instead of hashing the raw wire bytes) is
    /// that this holds — if this workspace's `serde_json` ever gained the `preserve_order` feature
    /// (nothing in `Cargo.lock` currently pulls in `indexmap`, which that feature requires), this
    /// test would be the first thing to catch it, before it silently turned every differently-
    /// key-ordered-but-identical retry into a false `Divergent`.
    #[test]
    fn sorted_key_json_serialization_probe() {
        let a: serde_json::Value = serde_json::from_str(r#"{"zebra":1,"apple":2}"#).unwrap();
        let b: serde_json::Value = serde_json::from_str(r#"{"apple":2,"zebra":1}"#).unwrap();
        let bytes_a = serde_json::to_vec(&a).unwrap();
        let bytes_b = serde_json::to_vec(&b).unwrap();
        assert_eq!(
            bytes_a, bytes_b,
            "serde_json::Value must serialize object keys in a canonical (sorted) order \
             regardless of the source's insertion order"
        );
        assert_eq!(
            String::from_utf8(bytes_a).unwrap(),
            r#"{"apple":2,"zebra":1}"#,
            "confirms the order is specifically sorted, not merely some other stable order"
        );
    }

    /// #248/M's second finding: a store read failure must answer `Undetermined`, never silently
    /// agree with `Absent` — folding the two would recreate this diagnostic's own reason to
    /// exist one layer down (an unreadable store and a genuinely calm execution both reading as
    /// "no problem"). Triggered here with a path that cannot possibly open as an event
    /// repository (a plain file, not a directory) rather than a directory that merely lacks the
    /// named execution's stream — the point is a STORE failure, not a resolvable-but-empty read.
    #[test]
    fn a_store_that_cannot_be_read_answers_undetermined_not_absent() {
        let directory = tempfile::tempdir().unwrap();
        let not_a_store_dir = directory.path().join("this-is-a-file-not-a-directory");
        std::fs::write(&not_a_store_dir, b"not an event store").unwrap();
        let check = check_waiting_input(&not_a_store_dir, "exec-does-not-matter");
        assert_eq!(
            check,
            WaitingInputCheck::Undetermined,
            "an unreadable store must not answer the same as a store that read cleanly and \
             found nothing waiting"
        );
        let diagnostic = fixture_only_diagnostic(check, ExecutorWiring::FIXTURE_ONLY)
            .expect("an undetermined fixture-only state must produce GHCLI022");
        assert_eq!(
            diagnostic.code,
            crate::error_codes::GHCLI022_FIXTURE_ONLY_STATE_UNDETERMINED
        );
        assert_eq!(diagnostic.path, "/");
        assert_eq!(diagnostic.source, SOURCE);
    }

    /// `request_digest16` must be stable across the same two-key-order variation the probe above
    /// confirms `serde_json` itself gives, and must actually change when the body's real content
    /// changes — the two properties `classify_one_key`'s Complete-vs-Divergent split depends on.
    #[test]
    fn request_digest16_is_stable_across_key_order_and_diverges_on_real_change() {
        let a: serde_json::Value = serde_json::from_str(r#"{"node":"a","extra":"x"}"#).unwrap();
        let a_reordered: serde_json::Value =
            serde_json::from_str(r#"{"extra":"x","node":"a"}"#).unwrap();
        let b: serde_json::Value = serde_json::from_str(r#"{"node":"b","extra":"x"}"#).unwrap();

        let digest_a = request_digest16("execution.approve", "exec-1", &a).unwrap();
        let digest_a_reordered =
            request_digest16("execution.approve", "exec-1", &a_reordered).unwrap();
        let digest_b = request_digest16("execution.approve", "exec-1", &b).unwrap();

        assert_eq!(
            digest_a, digest_a_reordered,
            "the same body's digest must not depend on JSON key order"
        );
        assert_ne!(
            digest_a, digest_b,
            "a genuinely different body must digest differently"
        );
        assert_eq!(digest_a.len(), DIGEST_HEX_LEN);
        assert!(digest_a.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    /// The digest also folds in the command name and execution id (not just the body) — confirmed
    /// directly rather than left to the doc comment's word alone.
    #[test]
    fn request_digest16_diverges_on_command_or_execution_change() {
        let body: serde_json::Value = serde_json::from_str(r#"{"node":"a"}"#).unwrap();
        let base = request_digest16("execution.approve", "exec-1", &body).unwrap();
        let other_command = request_digest16("execution.signal", "exec-1", &body).unwrap();
        let other_execution = request_digest16("execution.approve", "exec-2", &body).unwrap();
        assert_ne!(base, other_command);
        assert_ne!(base, other_execution);
    }

    /// The `IDEMPOTENCY_HEADER_MAX_LEN` doc comment's arithmetic, checked against the real
    /// validator rather than just asserted in prose: a derived key built from the longest possible
    /// header, the longest suffix any mutation uses, and a real digest must still parse as a valid
    /// `OpaqueId`.
    #[test]
    fn derived_key_at_the_maximum_header_length_stays_within_the_opaque_id_cap() {
        let longest_suffix = "cancelled";
        let header = "k".repeat(IDEMPOTENCY_HEADER_MAX_LEN);
        let body: serde_json::Value = serde_json::from_str(r#"{}"#).unwrap();
        let digest = request_digest16("execution.cancel", "exec-1", &body).unwrap();
        let derived = format!("{header}-{longest_suffix}-{digest}");
        assert!(
            derived.len() <= 128,
            "derived key length {} must stay within OpaqueId's 128-character cap: {derived:?}",
            derived.len()
        );
        assert!(
            OpaqueId::parse(derived.clone()).is_ok(),
            "the maximum-length derived key must still be a valid OpaqueId: {derived:?}"
        );
    }

    /// A pause is not the end of a stream (#710, Codex P2).
    ///
    /// Caller A pauses, someone resumes, caller B pauses. A's event is still durably committed,
    /// and A's handler must still find it — the predecessor read only the LATEST `ExecutionPaused`
    /// and so compared A against B, found no match, and answered `409 GHE003_IDEMPOTENCY_CONFLICT`
    /// for a request that had succeeded.
    ///
    /// The two negative arms are the controls: a key that never committed must be Absent (or the
    /// search would accept anyone), and A's key under a DIFFERENT actor must be Absent too (or the
    /// attribution guard #681 added at the pre-flight would be undone here, one layer down).
    #[test]
    fn a_committed_pause_is_found_after_a_later_pause_lands_on_top_of_it() {
        use graphhelm_events::{LocalEventRepository, PreparedAppend};
        use graphhelm_protocols::{
            ActorId, ExecutionMode, ExecutionPaused, ExecutionResumed, ExecutionStarted, NewEvent,
            PersistedActorType, ProjectId, RepositoryScope, Sensitivity, WireHash, WorkspaceId,
        };

        struct FixedClock;
        impl graphhelm_protocols::Clock for FixedClock {
            fn now(&self) -> chrono::DateTime<chrono::Utc> {
                chrono::TimeZone::with_ymd_and_hms(&chrono::Utc, 2026, 9, 4, 1, 0, 0).unwrap()
            }
        }
        struct Ids(AtomicUsize);
        impl graphhelm_protocols::IdGenerator for Ids {
            fn next_id(&self, prefix: &'static str) -> String {
                format!("{prefix}-{}", self.0.fetch_add(1, Ordering::SeqCst) + 1)
            }
        }

        let execution = "exec-two-pauses";
        let scope = RepositoryScope::new(
            WorkspaceId::parse(crate::commands::execution::WORKSPACE).unwrap(),
            ProjectId::parse(crate::commands::execution::PROJECT).unwrap(),
            Some(graphhelm_protocols::ExecutionId::parse(execution).unwrap()),
        );
        let caller =
            |id: &str| PersistedActor::new(PersistedActorType::Owner, ActorId::parse(id).unwrap());
        let event = |key: &str, actor: PersistedActor, kind: EventKind| {
            NewEvent::new(
                OpaqueId::parse(key).unwrap(),
                actor,
                Sensitivity::Internal,
                kind,
                vec![],
                vec![],
            )
        };

        let directory = tempfile::tempdir().unwrap();
        let store = LocalEventRepository::open(
            directory.path(),
            std::sync::Arc::new(FixedClock),
            std::sync::Arc::new(Ids(AtomicUsize::new(0))),
        )
        .unwrap();
        store
            .append_atomic(
                &PreparedAppend::new(
                    scope,
                    OpaqueId::parse(execution).unwrap(),
                    1,
                    vec![
                        event(
                            "key-start",
                            caller("studio-operator"),
                            EventKind::ExecutionStarted(ExecutionStarted {
                                execution_id: OpaqueId::parse(execution).unwrap(),
                                graph_version: 1,
                                graph_hash: WireHash::parse(format!("sha256:{}", "d".repeat(64)))
                                    .unwrap(),
                                mode: ExecutionMode::Supervised,
                            }),
                        ),
                        event(
                            "key-caller-a",
                            caller("caller-a"),
                            EventKind::ExecutionPaused(ExecutionPaused {
                                execution_id: OpaqueId::parse(execution).unwrap(),
                            }),
                        ),
                        event(
                            "key-resume",
                            caller("studio-operator"),
                            EventKind::ExecutionResumed(ExecutionResumed {
                                execution_id: OpaqueId::parse(execution).unwrap(),
                            }),
                        ),
                        event(
                            "key-caller-b",
                            caller("caller-b"),
                            EventKind::ExecutionPaused(ExecutionPaused {
                                execution_id: OpaqueId::parse(execution).unwrap(),
                            }),
                        ),
                    ],
                    vec![],
                    vec![],
                )
                .unwrap(),
            )
            .unwrap();

        let key_a = OpaqueId::parse("key-caller-a").unwrap();
        let key_b = OpaqueId::parse("key-caller-b").unwrap();

        // ARRANGEMENT: B's pause is the LATEST, which is what made A's unfindable before.
        assert_eq!(
            execution_paused_under(directory.path(), execution, &key_b, &caller("caller-b")),
            PausedUnderCaller::Committed,
            "the later pause must be findable, or this fixture is not the shape the defect needs"
        );

        assert_eq!(
            execution_paused_under(directory.path(), execution, &key_a, &caller("caller-a")),
            PausedUnderCaller::Committed,
            "caller A's pause committed and is still in history; a later pause on top of it does not make A's request a conflict"
        );
        assert_eq!(
            execution_paused_under(
                directory.path(),
                execution,
                &OpaqueId::parse("key-never-sent").unwrap(),
                &caller("caller-a"),
            ),
            PausedUnderCaller::Absent,
            "a key that never committed must not be found, or the search accepts anyone"
        );
        assert_eq!(
            execution_paused_under(directory.path(), execution, &key_a, &caller("caller-b")),
            PausedUnderCaller::Absent,
            "A's key under B's actor is not A's request: two actors can derive the same key"
        );
    }
}
