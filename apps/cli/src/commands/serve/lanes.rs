//! #1798: the owner's lane controls, as public routes.
//!
//! `POST /v1/executions/{id}/lanes/{lane}/wake|stop|reassign` each compose records the task fold
//! already reads, and append them through the signal route, so admission, sealing and idempotency
//! are the signal route's own:
//!
//! - wake: an owner `operator_note` addressed to the lane, which rings its wake lease like any note;
//! - stop: `task.lane_retired` for that one lane (`retired: false` restores it);
//! - reassign: the owner's release of one of the lane's live slices by `sliceKey`, then an owner
//!   `operator_note` asking the new lane to claim that issue. The claim itself stays the new lane's
//!   own record: a `task.claimed` is signed by the lane it names.
//!
//! A caller that retries resends the same body, `at` included (the instant its task records
//! carry); without `at` the Runtime stamps now, and a retry of that is a different body.
//!
//! Only the owner acts here. An agent token never reaches these paths (`agent_route_allowed`), and
//! the owner token declaring `agent` is refused at the route, before anything is read.

use axum::body::Bytes;
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use graphhelm_protocols::PersistedActorType;

use super::{
    ServeState, mutation_bad_request, owner_required_response, parse_mutation_headers, respond,
    routes,
};
use crate::commands::execution;
use crate::output::Outcome;

const WAKE_COMMAND: &str = "execution.lane_wake";
const STOP_COMMAND: &str = "execution.lane_stop";
const REASSIGN_COMMAND: &str = "execution.lane_reassign";
const TASK_EVENT_SCHEMA: &str = "graphhelm-task-event-v1";
const MAX_REASON_CHARS: usize = 500;

/// A lane id as the task records spell one (the tasks route's own rule), or `None`.
fn lane_id(raw: &str) -> Option<String> {
    execution::tasks::parse_lane(raw).ok()
}

/// One nonempty printable reason of at most 500 characters, the bound the fold applies.
fn reason(payload: &serde_json::Value) -> Option<String> {
    let reason = payload.get("reason")?.as_str()?;
    (!reason.trim().is_empty()
        && reason.chars().count() <= MAX_REASON_CHARS
        && !reason.chars().any(char::is_control))
    .then(|| reason.to_owned())
}

/// What every control reads first: the body, the lane, the reason and an owner identity. A refusal
/// is the response; nothing has been read from the store or appended.
struct Request {
    payload: serde_json::Value,
    lane: String,
    reason: String,
    owner: String,
    /// The task record's `at`: the caller's, so a retry resends the same bytes, else now.
    at: String,
}

#[allow(clippy::result_large_err)] // the refusal IS the response, as in `routes::start`
fn admit(
    command: &'static str,
    execution_id: &str,
    lane: &str,
    headers: &HeaderMap,
    body: &Bytes,
) -> Result<Request, Response> {
    let Ok(payload) = serde_json::from_slice::<serde_json::Value>(body) else {
        return Err(mutation_bad_request(
            command,
            "the request body must be a JSON object",
            "/",
        ));
    };
    if !payload.is_object() {
        return Err(mutation_bad_request(
            command,
            "the request body must be a JSON object",
            "/",
        ));
    }
    let identity = parse_mutation_headers(headers, command, execution_id, &payload, &[])?;
    if identity.actor.actor_type() != PersistedActorType::Owner {
        return Err(owner_required_response());
    }
    let Some(lane) = lane_id(lane) else {
        return Err(mutation_bad_request(
            command,
            "lane must be 1 to 128 characters of [A-Za-z0-9._:-]",
            "/lane",
        ));
    };
    let Some(reason) = reason(&payload) else {
        return Err(mutation_bad_request(
            command,
            "reason must be one nonempty printable line of at most 500 characters",
            "/reason",
        ));
    };
    let at = match payload.get("at") {
        None => chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        Some(serde_json::Value::String(at))
            if chrono::DateTime::parse_from_rfc3339(at).is_ok() && at.len() <= 64 =>
        {
            at.clone()
        }
        Some(_) => {
            return Err(mutation_bad_request(
                command,
                "at must be an RFC 3339 instant",
                "/at",
            ));
        }
    };
    Ok(Request {
        at,
        payload,
        lane,
        reason,
        owner: identity.actor.id().as_str().to_owned(),
    })
}

/// The append's own `Idempotency-Key`: the caller's key bound to the control, the lane and the
/// step, hashed to the header's 64-character bound. A retry derives the same key, so the signal
/// route answers it as the replay it is; the same caller key with another body is that route's
/// divergence refusal.
fn step_headers(headers: &HeaderMap, control: &str, lane: &str, step: &str) -> HeaderMap {
    let caller = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let digest = graphhelm_protocols::hash::sha256_hex(
        format!("lane-control\n{control}\n{lane}\n{step}\n{caller}").as_bytes(),
    );
    let mut headers = headers.clone();
    headers.insert(
        "idempotency-key",
        HeaderValue::from_str(&format!("lane-{}", &digest[..56]))
            .expect("a hex digest is a valid header value"),
    );
    headers
}

async fn append(
    state: &ServeState,
    execution_id: &str,
    headers: HeaderMap,
    mut signal: serde_json::Value,
) -> Response {
    // The signal carries its own `id`, derived from the step's key: the signal route recognizes a
    // retry only by an `id` the request itself names, so a Runtime-filled one would refuse it.
    let key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let digest = graphhelm_protocols::hash::sha256_hex(key.as_bytes());
    signal["id"] = serde_json::json!(format!("signal-{}", &digest[..16]));
    let body = serde_json::json!({ "signal": signal });
    routes::signal(
        State(state.clone()),
        UrlPath(execution_id.to_owned()),
        headers,
        Bytes::from(body.to_string()),
    )
    .await
}

fn operator_note(
    owner: &str,
    at: &str,
    to: &str,
    description: String,
    evidence: &str,
) -> serde_json::Value {
    serde_json::json!({
        "emittedAt": at,
        "type": "operator_note",
        "severity": "low",
        "source": {"type": "user", "id": owner},
        "to": to,
        "description": description,
        "evidence": [evidence],
    })
}

fn task_record(
    owner: &str,
    at: &str,
    kind: &str,
    document: &serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "emittedAt": at,
        "type": kind,
        "severity": "low",
        "source": {"type": "user", "id": owner},
        "description": document.to_string(),
        "evidence": ["lane-control"],
    })
}

/// `POST /v1/executions/{id}/lanes/{lane}/wake` `{reason}`: an owner note to the lane.
pub(super) async fn wake(
    State(state): State<ServeState>,
    UrlPath((execution_id, lane)): UrlPath<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let request = match admit(WAKE_COMMAND, &execution_id, &lane, &headers, &body) {
        Ok(request) => request,
        Err(response) => return response,
    };
    let note = operator_note(
        &request.owner,
        &request.at,
        &request.lane,
        request.reason,
        "lane-control:wake",
    );
    let headers = step_headers(&headers, "wake", &request.lane, "note");
    append(&state, &execution_id, headers, note).await
}

/// `POST /v1/executions/{id}/lanes/{lane}/stop` `{reason, retired?}`: retires the lane, or with
/// `retired: false` restores it.
pub(super) async fn stop(
    State(state): State<ServeState>,
    UrlPath((execution_id, lane)): UrlPath<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let request = match admit(STOP_COMMAND, &execution_id, &lane, &headers, &body) {
        Ok(request) => request,
        Err(response) => return response,
    };
    let retired = match request.payload.get("retired") {
        None => true,
        Some(serde_json::Value::Bool(retired)) => *retired,
        Some(_) => {
            return mutation_bad_request(STOP_COMMAND, "retired must be a boolean", "/retired");
        }
    };
    let document = serde_json::json!({
        "schema": TASK_EVENT_SCHEMA,
        "taskId": "lanes",
        "revision": 1,
        "at": request.at,
        "lanes": [request.lane],
        "retired": retired,
        "reason": request.reason,
    });
    let step = if retired { "retire" } else { "restore" };
    let headers = step_headers(&headers, "stop", &request.lane, step);
    append(
        &state,
        &execution_id,
        headers,
        task_record(&request.owner, &request.at, "task.lane_retired", &document),
    )
    .await
}

/// `POST /v1/executions/{id}/lanes/{lane}/reassign` `{sliceKey, to, reason}`: releases one of the
/// lane's live slices and asks `to` to claim its issue.
pub(super) async fn reassign(
    State(state): State<ServeState>,
    UrlPath((execution_id, lane)): UrlPath<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let request = match admit(REASSIGN_COMMAND, &execution_id, &lane, &headers, &body) {
        Ok(request) => request,
        Err(response) => return response,
    };
    let Some(to) = request
        .payload
        .get("to")
        .and_then(serde_json::Value::as_str)
        .and_then(lane_id)
    else {
        return mutation_bad_request(
            REASSIGN_COMMAND,
            "to must be a lane id of 1 to 128 characters of [A-Za-z0-9._:-]",
            "/to",
        );
    };
    if to == request.lane {
        return mutation_bad_request(
            REASSIGN_COMMAND,
            "to must name another lane than the one the work is taken from",
            "/to",
        );
    }
    let Some(slice_key) = request
        .payload
        .get("sliceKey")
        .and_then(serde_json::Value::as_str)
        .filter(|key| !key.is_empty() && key.len() <= 256)
        .map(str::to_owned)
    else {
        return mutation_bad_request(
            REASSIGN_COMMAND,
            "sliceKey must name one of the lane's live slices",
            "/sliceKey",
        );
    };
    let Some(keyring) = state.sealing.clone() else {
        return super::respond_failure(
            REASSIGN_COMMAND,
            execution::execution_state("reassigning requires a sealed keyring", "/keyring"),
        );
    };
    // The slice is read from the same verified fold the tasks route serves. A retry finds it
    // already released, so the replay is recognized from the release's own idempotency first.
    let release_headers = step_headers(&headers, "reassign", &request.lane, "release");
    let events = state.events.clone();
    let read_execution = execution_id.clone();
    let wanted = slice_key.clone();
    let owner_lane = request.lane.clone();
    let Some(read) = super::routes::off_reactor(move || {
        let budget = crate::commands::tasks_read_budget();
        let key = execution::tasks::cache_key(&events, &read_execution);
        execution::tasks::with_shared_cache(&key, |cache| {
            execution::tasks::read_cached(&events, &read_execution, &keyring, &budget, cache)
        })
        .map(|view| {
            view.tasks
                .into_iter()
                .find(|task| task.key == wanted)
                .map(|task| {
                    let held = task.lane == Some(owner_lane)
                        && task.step != graphhelm_execution::tasks::TaskStep::Merged;
                    (task.task_id, task.issue, task.branch, held)
                })
        })
    })
    .await
    else {
        return respond(
            StatusCode::INTERNAL_SERVER_ERROR,
            Outcome::internal(REASSIGN_COMMAND, "the reassign read failed").output,
        );
    };
    let (task_id, issue, branch) = match read {
        Ok(Some((task_id, issue, branch, true))) => (task_id, issue, branch),
        Ok(_) => {
            return mutation_bad_request(
                REASSIGN_COMMAND,
                "sliceKey must name one of the lane's live slices",
                "/sliceKey",
            );
        }
        Err(failure) => return super::respond_failure(REASSIGN_COMMAND, failure),
    };
    let release = serde_json::json!({
        "schema": TASK_EVENT_SCHEMA,
        "taskId": task_id,
        "revision": 2,
        "at": request.at,
        "lane": request.lane,
        "sliceKey": slice_key,
        "releasedBy": request.owner,
        "reason": request.reason,
    });
    let released = append(
        &state,
        &execution_id,
        release_headers,
        task_record(&request.owner, &request.at, "task.released", &release),
    )
    .await;
    if !released.status().is_success() {
        return released;
    }
    let work = match (issue, branch) {
        (Some(issue), Some(branch)) => format!("issue #{issue} (branch {branch})"),
        (Some(issue), None) => format!("issue #{issue}"),
        _ => format!("task {task_id}"),
    };
    let note = operator_note(
        &request.owner,
        &request.at,
        &to,
        format!(
            "Reassigned from {}: claim {work}. Reason: {}",
            request.lane, request.reason
        ),
        "lane-control:reassign",
    );
    let note_headers = step_headers(&headers, "reassign", &request.lane, "note");
    append(&state, &execution_id, note_headers, note).await
}
