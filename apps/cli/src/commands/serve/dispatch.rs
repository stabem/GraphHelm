//! #1981: owner requests reach a lane without the owner.
//!
//! An owner request (`task.request_recorded`, a Proof "Send to fix" among them) used to wait until
//! someone pressed Hand off. Now every signal the signal route admits is followed by one check of
//! that run: each request that has no linked issue and was never handed goes, oldest first, to the
//! first free live lane (the tasks read's own `free && !silent`, by name, and #2005: a lane that
//! ever owned or reviewed a slice and never recorded a request, so never the orchestrator) that
//! holds no other
//! handed request still waiting for its issue (`graphhelm_execution::requests::plan_dispatch`).
//!
//! The hand-off is an `operator_note` addressed to that lane, so it rings the lane's wake lease
//! like any note. It ends: when the lane links no issue within the lease
//! (`GRAPHHELM_DISPATCH_LEASE_SECONDS`, default 600) or goes silent, it lapses, the owner's Hand
//! off returns, and the next check hands the request to another free lane, at most
//! `MAX_DISPATCH_ATTEMPTS` times in all. It is appended through the signal route under the Runtime's own owner actor
//! (`graphhelm`), with a signal id and an `Idempotency-Key` derived from the request alone: two
//! checks racing on one request name the same key, and the store admits only the first body. The
//! note's plaintext `to` is what the requests route serves as `dispatchedTo`. No model is asked and
//! nothing outside the Runtime is called.

use axum::body::Bytes;
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use graphhelm_execution::requests::{RequestView, dispatch_signal_id, plan_dispatch};

use super::{ServeState, routes};
use crate::commands::execution;

/// The Runtime's owner actor; the same name the tasks read uses for its own verdicts.
const DISPATCHER: &str = execution::tasks::RUNTIME_ACTOR;

/// `POST /v1/executions/{id}/signal`: the signal route, then one dispatch check of the run when it
/// admitted the signal. The check runs after the reply, so it never slows or changes it.
pub(super) async fn signal(
    State(state): State<ServeState>,
    UrlPath(execution_id): UrlPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let response = routes::signal(
        State(state.clone()),
        UrlPath(execution_id.clone()),
        headers,
        body,
    )
    .await;
    if response.status().is_success() && state.sealing.is_some() {
        super::autopilot::after_signal(&state, &execution_id);
        tokio::spawn(dispatch(state, execution_id));
    }
    response
}

/// The requests of the run with their hand-offs, and its free live lanes by name.
fn read(state: &ServeState, execution_id: &str) -> Option<(Vec<RequestView>, Vec<String>)> {
    let keyring = state.sealing.clone()?;
    let budget = crate::commands::tasks_read_budget();
    let key = execution::tasks::cache_key(&state.events, execution_id);
    let view = execution::tasks::with_shared_cache(&key, |cache| {
        execution::tasks::read_cached(&state.events, execution_id, &keyring, &budget, cache)
    })
    .ok()?;
    let views = execution::tasks::owner_requests(&view);
    let mut free: Vec<String> = view
        .liveness
        .lanes
        .iter()
        .filter(|row| row.free && !row.silent)
        // #2005: the lane that records requests (the orchestrator) or never owned or reviewed a
        // slice is not a lane that takes work.
        .filter(|row| {
            graphhelm_execution::autopilot::takes_work(&row.lane, &view.tasks, &view.records)
        })
        .map(|row| row.lane.clone())
        .collect();
    free.sort();
    Some((views, free))
}

/// What the lane is asked to do, with the owner's words.
fn note_text(view: &RequestView) -> String {
    let title = view.title.as_deref().unwrap_or("owner request");
    format!(
        "Owner request {id} is yours: {title}. Open a GitHub issue for it, record claimed on it, \
         then record request_linked --request-id {id} on that issue so the owner sees it in \
         progress. The ask:\n{ask}",
        id = view.request_id,
        ask = view.ask,
    )
}

/// One dispatch check of `execution_id`: append one hand-off note per planned request.
pub(super) async fn dispatch(state: ServeState, execution_id: String) {
    let read_state = state.clone();
    let read_execution = execution_id.clone();
    let Some(Some((views, free))) =
        routes::off_reactor(move || read(&read_state, &read_execution)).await
    else {
        return;
    };
    for (request_id, lane, attempt) in plan_dispatch(&views, &free) {
        let Some(view) = views.iter().find(|view| view.request_id == request_id) else {
            continue;
        };
        let id = dispatch_signal_id(&request_id, attempt);
        let at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let body = serde_json::json!({ "signal": {
            "id": id,
            "type": "operator_note",
            "severity": "low",
            "source": {"type": "user", "id": DISPATCHER},
            "to": lane,
            "emittedAt": at,
            "description": note_text(view),
            "evidence": ["request-dispatch"],
        }});
        let mut headers = HeaderMap::new();
        for (name, value) in [
            ("idempotency-key", id.as_str()),
            ("x-graphhelm-actor", DISPATCHER),
            ("x-graphhelm-actor-type", "owner"),
            ("content-type", "application/json"),
        ] {
            if let Ok(value) = HeaderValue::from_str(value) {
                headers.insert(name, value);
            }
        }
        let reply = routes::signal(
            State(state.clone()),
            UrlPath(execution_id.clone()),
            headers,
            Bytes::from(body.to_string()),
        )
        .await;
        // A conflict is the expected race: another check appended this hand-off first. Any other
        // refusal is named on stderr (ids and status only; the note's text is not printed).
        let status = reply.status();
        if !status.is_success() && status != StatusCode::CONFLICT {
            eprintln!(
                "graphhelm serve: hand-off of request {request_id} to lane {lane} was refused \
                 ({status})"
            );
        }
    }
}
