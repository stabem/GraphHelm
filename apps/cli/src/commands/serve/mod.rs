pub(super) mod monitor;
pub(super) mod ports;
mod routes;
mod wake;

use std::collections::HashMap;
use std::future::Future;
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use graphhelm_gateway::manifest::RouteManifest;
use graphhelm_graph::raw_content_sha256;
use graphhelm_protocols::{
    ActorId, Diagnostic, EventEnvelope, OpaqueId, PersistedActor, PersistedActorType,
};

use crate::args::ServeArgs;
use crate::commands::events::runtime;
use crate::commands::execution::signal::SignalKeyring;
use crate::commands::{event_store, execution};
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
const ARGUMENT_CODE: &str = "GHCLI001_ARGUMENT_INVALID";
/// A `--bind` that parses but is not loopback, or whose listener/token setup cannot proceed —
/// the plan's own name for this new code (Milestone 05a Task 1).
const SERVE_INVALID_CODE: &str = "GHCLI006_SERVE_INVALID";
/// A request to anything but `/health` without a valid `Authorization: Bearer <token>`.
const UNAUTHORIZED_CODE: &str = "GHCLI007_SERVE_UNAUTHORIZED";
/// No route matches the request (method+path), reported once auth has already passed.
const NOT_FOUND_CODE: &str = "GHCLI008_SERVE_NOT_FOUND";

/// The command name `serve`'s own pre-bind failures report under — there is no verb, unlike
/// `execution`/`events`, so the bare subcommand name is the closest existing precedent
/// (`main.rs`'s own `"schema"` pre-dispatch failure uses the same bare-name convention).
const COMMAND: &str = "serve";
const STARTED_COMMAND: &str = "serve.started";
const HEALTH_COMMAND: &str = "serve.health";
const UNAUTHORIZED_COMMAND: &str = "serve.unauthorized";
const NOT_FOUND_COMMAND: &str = "serve.not_found";

const TOKEN_SUFFIX: &str = ".token";
const TOKEN_BYTES: usize = 32;
const TOKEN_HEX_LEN: usize = TOKEN_BYTES * 2;

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
    /// Milestone 05d Task 9: the real-executor wiring, present only when `serve` was launched
    /// with the full `{manifest, broker, keyring, key-id, route, staging}` group (STEP 2's
    /// grouping rule). `None` keeps the 05a fixture-only shape unchanged.
    runtime: Option<Arc<RuntimeWiring>>,
    /// The keyring coordinates alone, present whenever `--keyring`/`--key-id` were given —
    /// independently of `runtime` (STEP 2: `{keyring, key-id}` is its own all-or-none group,
    /// usable on its own as sealing-only configuration for the `signal` route). When `runtime` is
    /// `Some`, this is always `Some` too (real-executor mode requires both groups).
    sealing: Option<Arc<SignalKeyring>>,
    /// One cancellation sender per in-flight async drive, keyed by execution id — registered
    /// before `drive_to_quiescence_async` starts and deregistered once it returns. `pause
    /// {"mode":"immediate"}` (STEP 5) looks a live execution up here to interrupt it.
    cancels: Arc<tokio::sync::Mutex<HashMap<String, tokio::sync::watch::Sender<bool>>>>,
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
    let token = ensure_token(&args.events)?;
    let (runtime_wiring, sealing) = build_wiring(args)?;
    let state = ServeState {
        token: Arc::from(token.into_bytes()),
        events: Arc::from(args.events.as_path()),
        runtime: runtime_wiring.map(Arc::new),
        sealing: sealing.map(Arc::new),
        cancels: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
    };

    let rt =
        runtime().map_err(|_| serve_invalid("the operator runtime could not be started", "/"))?;
    rt.block_on(serve_forever(address, state))
}

/// STEP 2's grouping rule, enforced once at startup (`serve_invalid` on violation) rather than
/// per-request: `{manifest, broker, route, staging}` is all-or-none; `{keyring, key-id}` is
/// all-or-none; real-executor mode (a non-`None` `RuntimeWiring`) additionally requires BOTH
/// groups present together. A manifest is loaded and validated here (`RouteManifest::from_json`,
/// fail fast) and the configured `--route` is resolved to a cloned `ModelRoute` — never re-parsed
/// per drive.
#[allow(clippy::type_complexity)]
fn build_wiring(
    args: &ServeArgs,
) -> Result<(Option<RuntimeWiring>, Option<SignalKeyring>), Failure> {
    let executor_group = [
        args.manifest.is_some(),
        args.broker.is_some(),
        args.route.is_some(),
        args.staging.is_some(),
    ];
    let executor_all = executor_group.iter().all(|present| *present);
    let executor_none = executor_group.iter().all(|present| !*present);
    if !executor_all && !executor_none {
        return Err(serve_invalid(
            "--manifest, --broker, --route and --staging must be given together or not at all",
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
    if executor_all && !keyring_all {
        return Err(serve_invalid(
            "the real-executor flags require --keyring and --key-id as well",
            "/arguments",
        ));
    }

    let sealing = if keyring_all {
        let keyring = args.keyring.clone().expect("keyring_all guarantees Some");
        let key_id = args.key_id.clone().expect("keyring_all guarantees Some");
        Some(SignalKeyring {
            directory: keyring,
            key_id,
        })
    } else {
        None
    };

    let runtime = if executor_all {
        let manifest_path = args
            .manifest
            .as_ref()
            .expect("executor_all guarantees Some");
        let bytes = std::fs::read(manifest_path)
            .map_err(|_| serve_invalid("--manifest does not name a readable file", "/manifest"))?;
        let text = String::from_utf8(bytes)
            .map_err(|_| serve_invalid("--manifest is not valid UTF-8", "/manifest"))?;
        let manifest = RouteManifest::from_json(&text)
            .map_err(|error| serve_invalid(&error.to_string(), "/manifest"))?;
        let route_id = args.route.as_ref().expect("executor_all guarantees Some");
        let route = manifest
            .routes()
            .iter()
            .find(|route| route.id() == route_id)
            .ok_or_else(|| {
                serve_invalid("--route does not name a route in the manifest", "/route")
            })?
            .clone();
        let mut allow_programs = args.allow_program.clone();
        if allow_programs.is_empty() {
            allow_programs = vec!["git".to_owned(), "cargo".to_owned()];
        }
        Some(RuntimeWiring {
            manifest_path: manifest_path.clone(),
            route,
            broker_dir: args.broker.clone().expect("executor_all guarantees Some"),
            keyring_dir: args.keyring.clone().expect("executor_all guarantees Some"),
            key_id: args.key_id.clone().expect("executor_all guarantees Some"),
            staging: args.staging.clone().expect("executor_all guarantees Some"),
            tests_runner: args.tests_runner.clone(),
            allow_programs,
            path_prepend: args.path_prepend.clone(),
        })
    } else {
        None
    };

    Ok((runtime, sealing))
}

async fn serve_forever(address: SocketAddr, state: ServeState) -> Result<(), Failure> {
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
    let started = Outcome::success(
        STARTED_COMMAND,
        serde_json::json!({ "address": bound.to_string() }),
    );
    crate::output::print(&started.output, false);
    let _ = std::io::stdout().flush();

    let app = build_router(state);
    axum::serve(listener, app)
        .await
        .map_err(|_| serve_invalid("the server loop ended unexpectedly", "/"))
}

fn build_router(state: ServeState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/executions/{id}", get(routes::status))
        .route("/v1/executions/{id}/events", get(routes::events))
        .route("/v1/executions/{id}/start", post(routes::start))
        .route("/v1/executions/{id}/signal", post(routes::signal))
        .route("/v1/executions/{id}/approve", post(routes::approve))
        .route("/v1/executions/{id}/pause", post(routes::pause))
        .route("/v1/executions/{id}/resume", post(routes::resume))
        .route("/v1/executions/{id}/cancel", post(routes::cancel))
        .route(
            "/v1/executions/{id}/wake-lease",
            post(routes::wake_lease).get(routes::wake_lease_status),
        )
        .route("/v1/gateway/routes", get(routes::gateway_routes))
        .route("/v1/gateway/probe", get(routes::gateway_probe))
        .fallback(not_found)
        // `.layer` (not `.route_layer`) wraps the fallback too: an unauthenticated request to a
        // path with no route must still be refused 401, not fall through to a 404 that would
        // leak whether the path exists to an unauthenticated caller.
        .layer(middleware::from_fn_with_state(state.clone(), require_token))
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
/// endpoint contract's failure-mapping table (`docs/superpowers/plans/2026-08-13-public-runtime-api.md`):
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
        "GHCLI001_ARGUMENT_INVALID"
        | "GHCLI003_SIGNAL_INVALID"
        | "GHCLI004_SIGNAL_UNRECORDABLE" => StatusCode::BAD_REQUEST,
        "GHCLI005_EXECUTION_STATE" | "GHE001_SEQUENCE_CONFLICT" | "GHE003_IDEMPOTENCY_CONFLICT" => {
            StatusCode::CONFLICT
        }
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
// caller can only ever collide with its own past requests, made under its own bearer token — see
// `request_digest16`'s own doc comment.
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
    /// The bare `Idempotency-Key` header value the caller sent, before any suffix or content
    /// digest — named in a `KeyState::Divergent` diagnostic so the caller sees exactly which
    /// header value was reused across two different request bodies.
    idempotency_header: String,
    if_match: Option<u64>,
}

/// The maximum accepted length of the caller-supplied `Idempotency-Key` header, before any suffix
/// or content digest is appended. Chosen so the derived per-event key —
/// `"{header}-{suffix}-{digest16}"` — always stays within `OpaqueId`'s own 128-character cap
/// (`core/protocols/src/persistence.rs`'s `is_opaque_id`, read and confirmed: non-empty, <= 128
/// bytes, a closed punctuation/alphanumeric set that a lowercase-hex digest and every suffix below
/// both satisfy) with headroom to spare. The arithmetic: 64 (header) + 1 (dash) + 9 (the longest
/// suffix any mutation uses, `"cancelled"`) + 1 (dash) + 16 (digest, `DIGEST_HEX_LEN`) = 91 <= 128.
const IDEMPOTENCY_HEADER_MAX_LEN: usize = 64;
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
        idempotency_header: idempotency.to_owned(),
        if_match,
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
/// not a security boundary — a caller can only ever collide with its own past requests, made under
/// its own bearer token; there is no cross-caller boundary here to defend (`run_idempotent_mutation`
/// module doc, Task 3).
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

async fn run_idempotent_mutation<'a>(
    events: &Path,
    execution: &str,
    command: &'static str,
    identity: MutationIdentity,
    run: impl FnOnce(PersistedActor, OpaqueId) -> MutationFuture<'a>,
) -> Response {
    match classify_existing_keys(
        events,
        execution,
        &identity.keys,
        &identity.idempotency_header,
    ) {
        Ok(KeyState::Complete) => return reply_with_current_status(events, execution, command),
        Ok(KeyState::Partial(stuck)) => {
            return partial_conflict_response(events, execution, command, &stuck);
        }
        Ok(KeyState::Divergent(header)) => {
            return divergent_conflict_response(events, execution, command, &header);
        }
        Ok(KeyState::Absent) => {}
        Err(failure) => return respond_failure(command, failure),
    }

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
    match run(identity.actor, key).await {
        Ok(mut value) => {
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
            respond(StatusCode::OK, Outcome::success(command, value).output)
        }
        Err(MutationError::Prepared(response)) => response,
        Err(MutationError::Command(failure)) if failure.code == "GHE003_IDEMPOTENCY_CONFLICT" => {
            // The race the module doc names: another attempt with the same key committed between
            // the pre-flight check above and this append. Classify again against the store's
            // now-current state rather than trusting the in-flight guess either way.
            match classify_existing_keys(
                events,
                execution,
                &identity.keys,
                &identity.idempotency_header,
            ) {
                Ok(KeyState::Complete) => reply_with_current_status(events, execution, command),
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
    Complete,
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
/// means the caller's header was already spent on a different body — `Divergent`. Neither found at
/// all — genuinely fresh — `Missing`.
enum KeyPresence {
    Present,
    Divergent,
    Missing,
}

/// Prefix matching is a plain string `starts_with`, not a delimited-field comparison, so a caller
/// whose own `Idempotency-Key` header happens to literally contain another of its own headers plus
/// suffix as a leading substring (e.g. sending `"sig-cmd-record-extra"` to `signal` after
/// previously sending `"sig-cmd"` to the same command) could trigger a false `Divergent` against
/// itself. This is deliberately not hardened against: every key this pre-flight ever compares
/// belongs to requests made under the one bearer token this server issues (Milestone 05a Task 1) —
/// there is no other caller to collide with, so a self-inflicted prefix collision only ever costs
/// the crafting caller its own request, exactly as an accidental one would. The digest is
/// divergence *detection*, not a cryptographic boundary; see `request_digest16`'s doc comment.
fn classify_one_key(history: &[EventEnvelope], derived: &DerivedKey) -> KeyPresence {
    let reused_prefix = format!("{}-", derived.prefix);
    let mut divergent = false;
    for event in history {
        let existing = event.idempotency_key.as_str();
        if existing == derived.full.as_str() {
            return KeyPresence::Present;
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
    keys: &[DerivedKey],
    idempotency_header: &str,
) -> Result<KeyState, execution::Failure> {
    let store = event_store(events).map_err(|error| execution::repository_failure(&error))?;
    // `resolve_stream`'s raw `history` is reused directly below for every key's classification —
    // Complete/Partial *and* the new Divergent check — rather than a second, per-key store read
    // (the pre-fix version called `committed_events_for_idempotency` once per key; both it and
    // `resolve_stream`/`read_replay_stream` read from the exact same underlying committed batches,
    // so scanning `history` once here is strictly equivalent for Complete/Partial and additionally
    // enables Divergent detection for free).
    let (_scope, _stream, history) = execution::resolve_stream(&store, Some(execution))?;
    let mut present = 0_usize;
    let mut first_missing = None;
    for derived in keys {
        match classify_one_key(&history, derived) {
            KeyPresence::Present => present += 1,
            KeyPresence::Divergent => {
                return Ok(KeyState::Divergent(idempotency_header.to_owned()));
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
    } else if present == keys.len() {
        KeyState::Complete
    } else {
        KeyState::Partial(first_missing.expect("some but not all present implies one missing"))
    })
}

/// "Reply 200 with current state, because the caller's command has already happened" (the plan's
/// own words for the full-retry case) — literally the same `execution.status` read the CLI and
/// `GET /v1/executions/{id}` both use, so a retried mutation's reply has exactly the shape a
/// following status read would show. This is a deliberate divergence from a *fresh* success's own
/// bespoke response shape (`signal`'s `decision`/`mayProposeMutation`, for one) — recomputing that
/// bespoke shape on retry would mean re-deriving a governance verdict against already-advanced
/// state, the same category of problem `run_idempotent_mutation`'s pre-flight check exists to
/// avoid; "current status" is the one shape every mutation can report honestly no matter how long
/// ago the original attempt actually committed.
fn reply_with_current_status(events: &Path, execution: &str, command: &'static str) -> Response {
    match execution::status::execute(events, Some(execution)) {
        Ok(value) => respond(StatusCode::OK, Outcome::success(command, value).output),
        Err(failure) => respond_failure(command, failure),
    }
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

/// The caller reused an `Idempotency-Key` across two different request bodies (Milestone 05a
/// follow-up Critical: "the idempotency pre-flight is content-blind") — refused rather than
/// silently absorbed as a retry of the first. `header` is the bare `Idempotency-Key` value the
/// caller sent (not the derived per-event key with its suffix and content digest), named directly
/// so the caller can see exactly which header value was reused.
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
                    "the Idempotency-Key \"{header}\" was already used to record a different \
                     request; an Idempotency-Key identifies one logical request and must not be \
                     reused with a different body"
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
fn current_head(events: &Path, execution: &str) -> Option<u64> {
    let store = event_store(events).ok()?;
    let (_, _, history) = execution::resolve_stream(&store, Some(execution)).ok()?;
    Some(history.last().map_or(0, |event| event.sequence))
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

/// The token's path: a *sibling* of the events directory, never a child of it — named
/// `<events-directory-name>.token` in the same parent directory. `commands::event_store`'s
/// `LocalEventRepository::open` treats the events directory as its own exclusively-owned
/// namespace: `classify_layout` in `core/events/src/local.rs` enumerates the directory's entries
/// against a closed allowlist (`blobs`, `.tmp`, `active`, `format.json`, `journal.jsonl`,
/// `repository.lock`) and refuses the *entire* repository with `GHE007_UNSUPPORTED_FORMAT` the
/// moment it finds anything else — a deliberate integrity guard, not a bug to work around from the
/// inside. Milestone 05a Task 1 originally wrote the token to `events/token`, which satisfies that
/// guard only until a real repository also exists there; from that point on, *every* command
/// against the directory — the CLI's own, not just this server's — starts failing
/// `GHE007_UNSUPPORTED_FORMAT`. This was caught empirically while building Task 2's first test
/// (`status_over_http_matches_the_cli_and_the_events_tail_pages`): a `graphhelm execution status`
/// run by hand against a directory `serve` had already touched reproduced the same failure with no
/// server involved, confirming the cause sits in the token's location, not in anything Tasks 2/3
/// added. Keeping the token outside the directory `LocalEventRepository` owns avoids the collision
/// entirely without weakening that crate's allowlist — the correct fix is on the operator side of
/// the boundary, not a loosened integrity guard on the store side. The parent directory is
/// guaranteed to exist by the time this runs: `serve::execute` calls `create_dir_all(events)` first,
/// which creates every ancestor, including `events`'s own parent.
fn token_path(events: &Path) -> PathBuf {
    let mut name = events.file_name().map_or_else(
        || std::ffi::OsString::from("events"),
        std::ffi::OsStr::to_os_string,
    );
    name.push(TOKEN_SUFFIX);
    events.with_file_name(name)
}

/// Creates the bearer token on first `serve` and validates it thereafter through the same
/// read-side checks `events/config.rs`'s `read_bounded`/`reject_insecure_permissions` apply to
/// the operator configuration: not a symlink, a regular file, no group/world access on Unix, and
/// a bounded size. Those helpers are `pub(super)` to `commands::events` and read-side only —
/// there is no creation-time counterpart there to widen, and this task's file set does not
/// include `config.rs` — so this is a local, second copy scoped to `serve`, matching this
/// codebase's established precedent of keeping such copies local rather than reaching across a
/// task's declared file set (see `commands::execution::record_outcome`'s doc comment for the
/// same pattern).
fn ensure_token(events: &Path) -> Result<String, Failure> {
    let path = token_path(events);
    match create_token_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => {
            return Err(serve_invalid(
                "the bearer token file could not be created",
                "/token",
            ));
        }
    }
    read_validated_token(&path)
}

/// Writes a fresh 32-byte OS-random token, hex-encoded, refusing to overwrite an existing file
/// (`create_new`, atomic against a racing second `serve` on the same directory) and restricting
/// access to the owner alone on Unix (`0o600`) at creation time. The CLI binary may use OS
/// randomness directly — the purity rules bind the core crates, not the operator binary.
fn create_token_file(path: &Path) -> std::io::Result<()> {
    let mut bytes = [0_u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes)
        .map_err(|_| std::io::Error::other("the OS random source is unavailable"))?;
    let hex = encode_hex(&bytes);

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(hex.as_bytes())?;
    file.sync_all()
}

/// The read-side safety checks, narrowed from `events/config.rs`'s `read_bounded` to what a
/// fixed 64-character hex token needs.
fn read_validated_token(path: &Path) -> Result<String, Failure> {
    let unreadable = || serve_invalid("the bearer token file could not be read", "/token");
    let metadata = std::fs::symlink_metadata(path).map_err(|_| unreadable())?;
    if metadata.file_type().is_symlink() {
        return Err(serve_invalid(
            "the bearer token file must not be a symbolic link",
            "/token",
        ));
    }
    if !metadata.is_file() {
        return Err(serve_invalid(
            "the bearer token file must be a regular file",
            "/token",
        ));
    }
    reject_insecure_permissions(&metadata)?;
    if metadata.len() != TOKEN_HEX_LEN as u64 {
        return Err(serve_invalid(
            "the bearer token file is not the expected size",
            "/token",
        ));
    }
    let raw = std::fs::read(path).map_err(|_| unreadable())?;
    let token = String::from_utf8(raw)
        .map_err(|_| serve_invalid("the bearer token file is not valid UTF-8", "/token"))?;
    if token.len() != TOKEN_HEX_LEN
        || !token
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(serve_invalid(
            "the bearer token file is not 64 lowercase hexadecimal characters",
            "/token",
        ));
    }
    Ok(token)
}

#[cfg(unix)]
fn reject_insecure_permissions(metadata: &std::fs::Metadata) -> Result<(), Failure> {
    use std::os::unix::fs::MetadataExt;

    if metadata.mode() & 0o077 != 0 {
        return Err(serve_invalid(
            "the bearer token file must not be group or world accessible",
            "/token",
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn reject_insecure_permissions(_: &std::fs::Metadata) -> Result<(), Failure> {
    // Windows ACL evaluation is not attempted; the symlink and regular-file rules still apply —
    // identical no-op to `events/config.rs`'s own Windows branch.
    Ok(())
}

/// Hand-rolled to match `events/config.rs`'s own house style: that file validates and decodes
/// hexadecimal by hand (`hex_value`, the sha256 pin check) rather than pulling in the `hex`
/// crate for a few lines of work, even though `hex` is already an approved, pinned workspace
/// dependency used elsewhere. Encoding here follows the same convention.
fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
