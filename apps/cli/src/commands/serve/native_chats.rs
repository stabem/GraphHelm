use std::path::{Path, PathBuf};

use axum::Json;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;

use super::{ServeState, mutation_bad_request, respond, respond_failure};
use crate::commands::execution::signal::SignalKeyring;
use crate::commands::{event_store, execution};
use crate::output::Outcome;

const COMMAND: &str = "serve.native_chats";
const MAX_MESSAGE: usize = 2_000;
const LEGACY_RESUME_ERROR: &str = "Codex request thread/resume failed: server error";
const RESUME_BLOCKED_DETAIL: &str =
    "native thread resume was rejected; no instruction was dispatched";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SendRequest {
    request_id: String,
    node_id: String,
    thread_id: String,
    message: String,
    source_directory: String,
    title: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Receipt {
    request_id: String,
    node_id: String,
    thread_id: String,
    title: Option<String>,
    source_directory: String,
    state: String,
    request_digest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    turn_id: Option<String>,
}

fn config() -> Result<graphhelm_model_gateway::native_chats::NativeChatConfig, String> {
    let program = std::env::var_os("GRAPHHELM_CODEX_HOST_PROGRAM")
        .map(PathBuf::from)
        .ok_or_else(|| "GRAPHHELM_CODEX_HOST_PROGRAM is required".to_owned())?;
    if !program.is_absolute() {
        return Err("GRAPHHELM_CODEX_HOST_PROGRAM must be absolute".into());
    }
    let program = program
        .canonicalize()
        .map_err(|_| "GRAPHHELM_CODEX_HOST_PROGRAM must name an existing executable".to_owned())?;
    if !program.is_file() {
        return Err("GRAPHHELM_CODEX_HOST_PROGRAM must name an existing executable".into());
    }
    let sqlite_home = std::env::var_os("GRAPHHELM_CODEX_SQLITE_HOME")
        .map(PathBuf::from)
        .map(|path| {
            if !path.is_absolute() {
                return Err("GRAPHHELM_CODEX_SQLITE_HOME must be absolute".into());
            }
            let path = path.canonicalize().map_err(|_| {
                "GRAPHHELM_CODEX_SQLITE_HOME must name an existing directory".to_owned()
            })?;
            if !path.is_dir() {
                return Err("GRAPHHELM_CODEX_SQLITE_HOME must name an existing directory".into());
            }
            Ok::<PathBuf, String>(path)
        })
        .transpose()?;
    Ok(graphhelm_model_gateway::native_chats::NativeChatConfig {
        program,
        sqlite_home,
    })
}

fn error(message: impl Into<String>) -> Response {
    respond(
        StatusCode::SERVICE_UNAVAILABLE,
        Outcome::domain(
            COMMAND,
            vec![graphhelm_protocols::Diagnostic::error(
                crate::error_codes::GHCLI005_EXECUTION_STATE,
                message,
                "/nativeChats",
                "serve-cli",
            )],
        )
        .output,
    )
}

#[derive(Deserialize)]
pub(super) struct ListQuery {
    cursor: Option<String>,
}

pub(super) async fn list(
    State(_state): State<ServeState>,
    Query(query): Query<ListQuery>,
) -> Response {
    if query
        .cursor
        .as_ref()
        .is_some_and(|cursor| cursor.len() > 4096)
    {
        return mutation_bad_request(COMMAND, "cursor is too long", "/cursor");
    }
    let cfg = match config() {
        Ok(value) => value,
        Err(message) => return error(message),
    };
    let cursor = query.cursor;
    match tokio::task::spawn_blocking(move || {
        graphhelm_model_gateway::native_chats::list(&cfg, cursor.as_deref())
    })
    .await
    {
        Ok(Ok(value)) => respond(StatusCode::OK, Outcome::success(COMMAND, value).output),
        Ok(Err(message)) => error(message),
        Err(_) => error("native chat listing stopped before producing a result"),
    }
}

pub(super) async fn requests(
    State(state): State<ServeState>,
    AxumPath(execution_id): AxumPath<String>,
) -> Response {
    let Some(sealing) = state.sealing.as_deref() else {
        return error("native chat receipts require a sealed keyring");
    };
    let busy = state.native_chat_busy.lock().await;
    let result = load_receipts(&state.events, &execution_id, sealing).await;
    match result {
        Ok(mut receipts) => {
            for receipt in &mut receipts {
                if matches!(receipt.state.as_str(), "requested" | "received")
                    && busy.get(&receipt.thread_id) != Some(&receipt.request_id)
                {
                    receipt.state = "unobserved".into();
                    receipt.detail = Some("The native outcome was not confirmed. Inspect this request before starting new work.".into());
                }
            }
            respond(
                StatusCode::OK,
                Outcome::success(COMMAND, json!({"requests": receipts})).output,
            )
        }
        Err(failure) => respond_failure(COMMAND, failure),
    }
}

pub(super) async fn send(
    State(state): State<ServeState>,
    AxumPath(execution_id): AxumPath<String>,
    headers: HeaderMap,
    Json(body): Json<SendRequest>,
) -> Response {
    if headers
        .get("x-graphhelm-actor-type")
        .and_then(|v| v.to_str().ok())
        != Some("owner")
    {
        return mutation_bad_request(
            COMMAND,
            "native chat requests require the owner actor",
            "/actorType",
        );
    }
    if headers.get("idempotency-key").and_then(|v| v.to_str().ok())
        != Some(body.request_id.as_str())
    {
        return mutation_bad_request(
            COMMAND,
            "Idempotency-Key must equal requestId",
            "/idempotencyKey",
        );
    }
    if body.request_id.is_empty()
        || body.request_id.len() > 64
        || !body
            .request_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return mutation_bad_request(COMMAND, "requestId is not wire-safe", "/requestId");
    }
    if body.message.trim().is_empty()
        || body.message.chars().count() > MAX_MESSAGE
        || body.node_id.is_empty()
        || body.node_id.len() > 128
        || body.source_directory.is_empty()
        || body.source_directory.len() > 4096
        || body
            .title
            .as_ref()
            .is_none_or(|title| title.trim().is_empty() || title.chars().count() > 512)
        || uuid::Uuid::parse_str(&body.thread_id).is_err()
    {
        return mutation_bad_request(
            COMMAND,
            "native chat request metadata is invalid or exceeds its bounds",
            "/nativeChats",
        );
    }
    let wire = serde_json::to_value(&body).expect("native request is serializable");
    let identity =
        match super::parse_mutation_headers(&headers, COMMAND, &execution_id, &wire, &["intent"]) {
            Ok(identity) => identity,
            Err(response) => return response,
        };
    let request_digest = match graphhelm_graph::raw_content_sha256(&serde_json::to_vec(&json!({"execution":execution_id,"actor":headers.get("x-graphhelm-actor").and_then(|v|v.to_str().ok()),"body":wire})).expect("request identity is serializable")) {
        Ok(digest) => digest.as_str().to_owned(),
        Err(_) => return error("native request identity could not be computed"),
    };
    let Some(sealing) = state.sealing.clone() else {
        return error("native chat requests require a sealed keyring");
    };
    // Hold the short intent transaction lock through durable read/append. It also reserves the
    // selected native thread until its worker terminates. Native I/O never holds this mutex.
    let mut busy = state.native_chat_busy.lock().await;
    let existing = match load_receipts(&state.events, &execution_id, &sealing).await {
        Ok(receipts) => receipts,
        Err(failure) => return respond_failure(COMMAND, failure),
    };
    if let Some(found) = existing
        .iter()
        .find(|item| item.request_id == body.request_id)
    {
        if found.request_digest != request_digest {
            return mutation_bad_request(
                COMMAND,
                "requestId was reused for a different immutable request",
                "/requestId",
            );
        }
        return respond(
            StatusCode::OK,
            Outcome::success(COMMAND, json!({"requestId":found.request_id})).output,
        );
    }
    if busy.contains_key(&body.thread_id) {
        return mutation_bad_request(
            COMMAND,
            "a native turn is already in flight for this thread",
            "/threadId",
        );
    }
    let store = match event_store(&state.events) {
        Ok(store) => store,
        Err(failure) => return respond_failure(COMMAND, execution::repository_failure(&failure)),
    };
    let (scope, stream, projection) = match execution::load_projection(&store, Some(&execution_id))
    {
        Ok(value) => value,
        Err(failure) => return respond_failure(COMMAND, failure),
    };
    let history = match store.read_replay_stream(&scope, &stream) {
        Ok(value) => value,
        Err(failure) => return respond_failure(COMMAND, execution::repository_failure(&failure)),
    };
    if history.iter().any(|event| {
        matches!(
            event.kind,
            graphhelm_protocols::EventKind::ExecutionCompleted(_)
        )
    }) {
        return mutation_bad_request(COMMAND, "the execution is terminal", "/execution");
    }
    let declared = projection.node_states.contains_key(&body.node_id)
        || projection
            .declared_form
            .as_ref()
            .is_some_and(|form| form.node_ids.iter().any(|id| id.as_str() == body.node_id));
    if !declared {
        return mutation_bad_request(
            COMMAND,
            "nodeId is not declared by this execution",
            "/nodeId",
        );
    }
    if projection
        .node_states
        .get(&body.node_id)
        .is_some_and(|node| {
            matches!(
                node,
                graphhelm_protocols::NodeState::Succeeded
                    | graphhelm_protocols::NodeState::Failed
                    | graphhelm_protocols::NodeState::Waived
                    | graphhelm_protocols::NodeState::Skipped
                    | graphhelm_protocols::NodeState::Cancelled
                    | graphhelm_protocols::NodeState::Invalidated
            )
        })
    {
        return mutation_bad_request(COMMAND, "the selected node is terminal", "/nodeId");
    }
    let cfg = match config() {
        Ok(value) => value,
        Err(message) => return error(message),
    };
    let mut receipt = Receipt {
        request_id: body.request_id.clone(),
        node_id: body.node_id.clone(),
        thread_id: body.thread_id.clone(),
        title: body.title.clone(),
        source_directory: body.source_directory.clone(),
        state: "requested".into(),
        request_digest,
        text: Some(body.message.clone()),
        detail: None,
        turn_id: None,
    };
    let intent = receipt.clone();
    let events = state.events.clone();
    let execution_for_intent = execution_id.clone();
    let sealing_for_intent = sealing.clone();
    let actor = identity.actor;
    let intent_actor = actor.clone();
    match tokio::task::spawn_blocking(move || {
        record(
            &events,
            &execution_for_intent,
            &sealing_for_intent,
            intent_actor,
            &intent,
        )
    })
    .await
    {
        Ok(Ok(_)) => {}
        Ok(Err(failure)) => return respond_failure(COMMAND, failure),
        Err(_) => return error("native chat intent could not be sealed"),
    }
    busy.insert(body.thread_id.clone(), body.request_id.clone());
    drop(busy);
    let request_id = body.request_id.clone();
    let busy_thread_id = body.thread_id.clone();
    tokio::spawn(async move {
        let worker_events = state.events.clone();
        let worker_execution = execution_id.clone();
        let worker_sealing = sealing.clone();
        let worker_actor = actor.clone();
        let mut observed = receipt.clone();
        let source = PathBuf::from(&body.source_directory);
        let result = tokio::task::spawn_blocking(move || {
            graphhelm_model_gateway::native_chats::send(
                &cfg,
                &body.thread_id,
                &body.message,
                &source,
                |event| {
                    let phase = event
                        .get("phase")
                        .and_then(Value::as_str)
                        .ok_or_else(|| "native phase is missing".to_owned())?;
                    if !matches!(phase, "received" | "completed") {
                        return Err("native phase is invalid".into());
                    }
                    observed.state = phase.to_owned();
                    observed.text = event
                        .get("finalText")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    observed.turn_id = event
                        .get("turnId")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    record(
                        &worker_events,
                        &worker_execution,
                        &worker_sealing,
                        worker_actor.clone(),
                        &observed,
                    )
                    .map(|_| ())
                    .map_err(|_| "native receipt could not be sealed".to_owned())
                },
            )
        })
        .await;
        let failure = match result {
            Ok(Ok(())) => None,
            Ok(Err(message)) => Some(message),
            Err(_) => Some("native worker stopped before its outcome was confirmed".to_owned()),
        };
        if let Some(message) = failure {
            receipt.state = if message.starts_with("blocked:") {
                "blocked"
            } else {
                "unobserved"
            }
            .into();
            receipt.text = None;
            receipt.detail = Some(message.chars().take(512).collect());
            let events = state.events.clone();
            let execution = execution_id.clone();
            let sealing = sealing.clone();
            // If this append fails, durable readback still exposes the earlier unfinished intent;
            // the absent busy lease below makes that outcome explicitly unobserved.
            let _ = tokio::task::spawn_blocking(move || {
                record(&events, &execution, &sealing, actor, &receipt)
            })
            .await;
        }
        state.native_chat_busy.lock().await.remove(&busy_thread_id);
    });
    respond(
        StatusCode::ACCEPTED,
        Outcome::success(COMMAND, json!({"requestId":request_id})).output,
    )
}

fn record(
    events: &Path,
    execution_id: &str,
    sealing: &SignalKeyring,
    actor: graphhelm_protocols::PersistedActor,
    receipt: &Receipt,
) -> Result<Value, execution::Failure> {
    let id = format!("native-chat-{}-{}", receipt.request_id, receipt.state);
    let envelope = json!({"id":id,"type":format!("native_chat_{}",receipt.state),"source":{"type":"user","id":actor.id().as_str()},"severity":"low","description":serde_json::to_string(receipt).expect("native receipt is serializable"),"evidence":[format!("native-request/{}",receipt.request_id)],"emittedAt":chrono::Utc::now().to_rfc3339()});
    let raw = serde_json::to_vec(&envelope).expect("native envelope is serializable");
    let key = graphhelm_protocols::OpaqueId::parse(id).map_err(|_| {
        execution::execution_state("native receipt identity is invalid", "/nativeChats")
    })?;
    execution::signal::execute_native_observer(events, execution_id, &raw, actor, key, sealing)
}

async fn load_receipts(
    events: &Path,
    execution_id: &str,
    sealing: &SignalKeyring,
) -> Result<Vec<Receipt>, execution::Failure> {
    let store = event_store(events).map_err(|error| execution::repository_failure(&error))?;
    let (scope, stream, _) = execution::load_projection(&store, Some(execution_id))?;
    let history = store
        .read_replay_stream(&scope, &stream)
        .map_err(|error| execution::repository_failure(&error))?;
    let opener = super::ports::build_opener(Some(sealing))
        .map_err(|message| execution::execution_state(&message, "/keyring"))?;
    let mut records: Vec<Receipt> = Vec::new();
    let mut observed_requests = HashSet::new();
    for event in history {
        let graphhelm_protocols::EventKind::SignalRecorded(signal) = &event.kind else {
            continue;
        };
        if !signal.kind.starts_with("native_chat_") {
            continue;
        }
        if event.evidence_refs.is_empty() {
            return Err(execution::execution_state(
                "native chat receipt has no sealed evidence",
                "/nativeChats",
            ));
        }
        for reference in &event.evidence_refs {
            let sealed = match store.sealed_evidence(&scope, reference.evidence_id()) {
                Ok(graphhelm_events::EvidenceRead::Available(sealed)) => sealed,
                Ok(_) => {
                    return Err(execution::execution_state(
                        "native chat evidence is unavailable",
                        "/nativeChats",
                    ));
                }
                Err(error) => return Err(execution::repository_failure(&error)),
            };
            let plaintext = opener.open(scope.clone(), &sealed).await.map_err(|_| {
                execution::execution_state(
                    "native chat evidence could not be opened",
                    "/nativeChats",
                )
            })?;
            let text = plaintext
                .expose(|bytes| String::from_utf8(bytes.to_vec()))
                .map_err(|_| {
                    execution::execution_state("native chat evidence was not UTF-8", "/nativeChats")
                })?;
            let value: Value = serde_json::from_str(&text).map_err(|_| {
                execution::execution_state("native chat evidence was malformed", "/nativeChats")
            })?;
            let description = value
                .get("description")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    execution::execution_state(
                        "native chat evidence has no receipt",
                        "/nativeChats",
                    )
                })?;
            let mut receipt: Receipt = serde_json::from_str(description).map_err(|_| {
                execution::execution_state(
                    "native chat evidence has a malformed receipt",
                    "/nativeChats",
                )
            })?;
            receipt.state = signal.kind.trim_start_matches("native_chat_").to_owned();
            if matches!(receipt.state.as_str(), "received" | "completed")
                || receipt.turn_id.is_some()
            {
                observed_requests.insert(receipt.request_id.clone());
            }
            if receipt.state == "unobserved"
                && receipt.turn_id.is_none()
                && receipt.detail.as_deref() == Some(LEGACY_RESUME_ERROR)
                && !observed_requests.contains(&receipt.request_id)
            {
                receipt.state = "blocked".into();
                receipt.detail = Some(RESUME_BLOCKED_DETAIL.into());
            }
            if let Some(index) = records
                .iter()
                .position(|prior| prior.request_id == receipt.request_id)
            {
                records[index] = receipt;
            } else {
                records.push(receipt);
            }
        }
    }
    Ok(records)
}
