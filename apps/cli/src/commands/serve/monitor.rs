//! The monitor's pure renderer (Milestone 05f Task 1): HTML as a THIRD formatter over the
//! same `ExecutionProjection` the `status` command and the API render — one store, one
//! truth. Zero JavaScript by construction (D-040): the page carries no `<script>`, updates
//! ride `<meta http-equiv="refresh">` whose URL carries the `since` cursor, so the delta
//! strip is stateless — the browser tells the server what the operator last saw.

use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use graphhelm_events::ExecutionProjection;
use graphhelm_protocols::{EventEnvelope, NodeOutcome, NodeState, NodeType};

use crate::commands::remediation::{self, RemediationAction};

/// The refresh cadence in seconds — the page's whole update contract (no SSE, no push).
const REFRESH_SECONDS: u32 = 2;
/// The event tail renders at most this many lines, newest last.
const TAIL_LIMIT: usize = 50;

/// Staleness thresholds, per node kind where the stream carries the graph (a tool silent
/// for 30s is wedged; an agent quietly thinking for two minutes is normal), and a single
/// default where it does not — Task 3's reconciled reality: the CLI/serve start paths do
/// not publish the graph onto the stream (the 05d Task 7 discovery), so kinds and edges
/// exist only on streams with an M03-era publication.
const STALE_TOOL_SECONDS: i64 = 30;
const STALE_AGENT_SECONDS: i64 = 120;
const STALE_DEFAULT_SECONDS: i64 = 90;

/// Escapes every dynamic string before it touches the page. Minimal on purpose: the five
/// characters HTML assigns meaning to, nothing else — no template engine enters the tree.
fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// One tail line's grammar: `ACTOR verb TARGET` — the kill-feed shape that makes a
/// multi-actor tail legible. The verb is the event kind's own wire tag (internally tagged
/// serde), the target the `nodeId` when the payload carries one; no per-kind match exists
/// here to drift when the 27th kind lands.
fn tail_line(event: &EventEnvelope) -> String {
    let kind = serde_json::to_value(&event.kind).unwrap_or(serde_json::Value::Null);
    let verb = kind["type"].as_str().unwrap_or("event").to_owned();
    let target = kind["data"]["nodeId"].as_str().unwrap_or("").to_owned();
    format!(
        "<li>#{seq} <b>{actor}</b> {verb} {target} <span class=\"t\">{at}</span></li>",
        seq = event.sequence,
        actor = escape(event.actor.id().as_str()),
        verb = escape(&verb),
        target = escape(&target),
        at = escape(&event.occurred_at.as_datetime().to_rfc3339()),
    )
}

/// The untriaged-interruption triage list — the SAME rule `execution::render` reports
/// (`Blocked` + last outcome `Interrupted`), restated here over the same projection so the
/// two surfaces cannot disagree.
fn untriaged(projection: &ExecutionProjection) -> Vec<String> {
    projection
        .node_states
        .iter()
        .filter(|(node, state)| {
            **state == NodeState::Blocked
                && projection.last_outcome.get(*node) == Some(&NodeOutcome::Interrupted)
        })
        .map(|(node, _)| node.clone())
        .collect()
}

/// The newest event timestamp per node, folded from the tail's own `nodeId` payloads —
/// silence rendered as signal: a hung worker emits nothing, and the gap IS the alarm.
fn last_event_per_node(events: &[EventEnvelope]) -> BTreeMap<String, DateTime<Utc>> {
    let mut latest = BTreeMap::new();
    for event in events {
        let kind = serde_json::to_value(&event.kind).unwrap_or(serde_json::Value::Null);
        if let Some(node) = kind["data"]["nodeId"].as_str() {
            latest.insert(node.to_owned(), *event.occurred_at.as_datetime());
        }
    }
    latest
}

/// What the graph publication adds when the stream carries one: node kinds (per-kind
/// staleness) and the edge list (blast radius). Absent on CLI/serve-started streams —
/// rendered honestly as absent, never guessed.
struct GraphExtras {
    kinds: BTreeMap<String, NodeType>,
    edges: Vec<(String, String)>,
    entrypoints: Vec<String>,
    /// Sink nodes stand in for terminals: the completion control's own terminal list is a
    /// typed expression this slice does not evaluate (declared approximation).
    terminals: Vec<String>,
}

fn graph_extras(projection: &ExecutionProjection) -> Option<GraphExtras> {
    let topology = projection.current_graph.as_ref()?.topology();
    let kinds = topology
        .nodes()
        .iter()
        .map(|(id, node)| (id.as_str().to_owned(), node.node_type().clone()))
        .collect();
    let edges: Vec<(String, String)> = topology
        .edges()
        .iter()
        .map(|edge| {
            (
                edge.from().as_str().to_owned(),
                edge.to().as_str().to_owned(),
            )
        })
        .collect();
    let entrypoints = topology
        .entrypoints()
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect();
    let terminals = topology
        .nodes()
        .keys()
        .filter(|id| edges.iter().all(|(source, _)| source != id.as_str()))
        .map(|id| id.as_str().to_owned())
        .collect();
    Some(GraphExtras {
        kinds,
        edges,
        entrypoints,
        terminals,
    })
}

fn stale_bound(kind: Option<&NodeType>) -> i64 {
    match kind {
        Some(NodeType::Tool) => STALE_TOOL_SECONDS,
        Some(NodeType::Agent) => STALE_AGENT_SECONDS,
        _ => STALE_DEFAULT_SECONDS,
    }
}

/// The pure renderer: projection + tail + the operator's last-seen cursor + now. `since`
/// bounds the delta strip; the emitted refresh URL carries the CURRENT head as the next
/// cursor, which is what makes the strip stateless across refreshes.
pub(super) fn render_monitor(
    projection: &ExecutionProjection,
    events: &[EventEnvelope],
    since: u64,
    now: DateTime<Utc>,
    events_dir: &Path,
) -> String {
    render_page(projection, events, since, now, events_dir, true)
}

/// The incident snapshot (`graphhelm execution status --html`, 05f Task 5): the SAME page
/// minus the refresh tag — a frozen, self-contained artifact for postmortems, byte-equal to
/// the live page apart from that one tag because it IS the live renderer, not a fork.
pub(in crate::commands) fn render_snapshot(
    projection: &ExecutionProjection,
    events: &[EventEnvelope],
    now: DateTime<Utc>,
    events_dir: &Path,
) -> String {
    render_page(projection, events, 0, now, events_dir, false)
}

fn render_page(
    projection: &ExecutionProjection,
    events: &[EventEnvelope],
    since: u64,
    now: DateTime<Utc>,
    events_dir: &Path,
    refresh: bool,
) -> String {
    let latest = last_event_per_node(events);
    let extras = graph_extras(projection);
    let head = events.last().map_or(0, |event| event.sequence);
    let execution = projection.execution_id.as_deref().unwrap_or("(none)");
    let status = projection
        .simulation_status
        .as_ref()
        .map_or_else(|| "unset".to_owned(), |status| format!("{status:?}"));

    let mut page = String::new();
    page.push_str("<!doctype html>\n<html><head><meta charset=\"utf-8\">\n");
    if refresh {
        page.push_str(&format!(
            "<meta http-equiv=\"refresh\" content=\"{REFRESH_SECONDS};url=/monitor/{id}?since={head}\">\n",
            id = escape(execution),
        ));
    }
    page.push_str(&format!(
        "<title>GraphHelm monitor — {}</title>\n",
        escape(execution)
    ));
    page.push_str(
        "<style>body{font-family:monospace;margin:1.5rem}table{border-collapse:collapse}\
         td,th{border:1px solid #999;padding:.2rem .6rem;text-align:left}\
         .t{color:#777}.delta{background:#ffd}.stale{color:#b00}</style>\n</head><body>\n",
    );
    page.push_str(&format!(
        "<h1>{id}</h1><p>status: <b>{status}</b> · head: {head} · rendered: {now} · \
         read-only (D-040): this page mutates nothing and offers nothing that does</p>\n",
        id = escape(execution),
        status = escape(&status),
        now = escape(&now.to_rfc3339()),
    ));

    // The delta strip: what changed since the operator last saw the page — transitions,
    // not state. At 3am the question is what it STOPPED being, not what it is.
    let fresh: Vec<&EventEnvelope> = events
        .iter()
        .filter(|event| event.sequence > since)
        .collect();
    page.push_str(&format!(
        "<h2>changed since #{since}</h2><ul class=\"delta\">\n"
    ));
    if fresh.is_empty() {
        page.push_str("<li>nothing — the world is as you left it</li>\n");
    }
    for event in &fresh {
        page.push_str(&tail_line(event));
        page.push('\n');
    }
    page.push_str("</ul>\n");

    // The node table over the same node_states map status reports, with the silence
    // clock (the loudest thing a hung worker produces is nothing) and, when the stream
    // carries the graph, the blast radius of a failure.
    page.push_str(
        "<h2>nodes</h2><table><tr><th>node</th><th>state</th><th>attempts</th>\
         <th>last event</th><th>blast radius</th></tr>\n",
    );
    for (node, state) in &projection.node_states {
        let silence = latest.get(node).map_or_else(
            || "never".to_owned(),
            |at| {
                let age = (now - *at).num_seconds().max(0);
                let kind = extras.as_ref().and_then(|extras| extras.kinds.get(node));
                let running = *state == NodeState::Running;
                if running && age > stale_bound(kind) {
                    format!("<span class=\"stale\">{age}s ago — silent past its bound</span>")
                } else {
                    format!("{age}s ago")
                }
            },
        );
        let radius = match (&extras, state) {
            (Some(extras), NodeState::Failed | NodeState::Blocked) => {
                let radius = remediation::blast_radius(
                    &extras.edges,
                    &extras.entrypoints,
                    &extras.terminals,
                    node,
                );
                format!(
                    "blocks {} downstream; terminal {}",
                    radius.blocked_downstream,
                    if radius.terminal_reachable {
                        "still reachable — can wait"
                    } else {
                        "unreachable through this path"
                    }
                )
            }
            (None, NodeState::Failed | NodeState::Blocked) => {
                "unknown — the stream carries no graph publication".to_owned()
            }
            _ => String::new(),
        };
        page.push_str(&format!(
            "<tr><td>{node}</td><td>{state:?}</td><td>{attempts}</td><td>{silence}</td>\
             <td>{radius}</td></tr>\n",
            node = escape(node),
            attempts = projection.node_attempts.get(node).copied().unwrap_or(0),
        ));
    }
    page.push_str("</table>\n");

    // The triage list — same rule as execution::render's untriagedInterruptions.
    page.push_str("<h2>triage</h2><ul>\n");
    let triage = untriaged(projection);
    if triage.is_empty() {
        page.push_str("<li>nothing untriaged</li>\n");
    }
    for node in &triage {
        // The positive answer to scope gravity: never a button — the exact command,
        // rendered from the CLI's own vocabulary (remediation.rs, parse-round-trip-tested).
        let command = remediation::render_invocation(
            &RemediationAction::Approve,
            events_dir,
            execution,
            node,
        );
        page.push_str(&format!(
            "<li><b>{}</b> interrupted and blocked — run: <code>{}</code></li>\n",
            escape(node),
            escape(&command)
        ));
    }
    page.push_str("</ul>\n");

    // The tail, newest last, bounded.
    page.push_str(&format!("<h2>events (last {TAIL_LIMIT})</h2><ul>\n"));
    for event in events.iter().rev().take(TAIL_LIMIT).rev() {
        page.push_str(&tail_line(event));
        page.push('\n');
    }
    page.push_str("</ul>\n</body></html>\n");
    page
}

// ---------------------------------------------------------------------------------------------
// Task 2: the GET-only routes and the cookie bootstrap — read-only structurally.
// ---------------------------------------------------------------------------------------------

use axum::extract::{Path as UrlPath, RawQuery, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};

use super::ServeState;

const COOKIE_NAME: &str = "graphhelm_monitor";
/// Every 200 carries this: no scripts, no frames, no fetch — inline style only. The page is
/// zero-JS by construction (Task 1); the CSP makes the refusal defense-in-depth.
const CSP: &str = "default-src 'none'; style-src 'unsafe-inline'";

/// Verifies a presented token value against the server's token bytes — the SAME
/// constant-time verifier `require_token` uses (`super::constant_time_eq`): one authority,
/// no second comparison ever drifting from it.
fn verified(state: &ServeState, presented: &str) -> bool {
    super::constant_time_eq(presented.as_bytes(), &state.token)
}

fn cookie_token(headers: &axum::http::HeaderMap) -> Option<String> {
    let cookies = headers.get(header::COOKIE)?.to_str().ok()?;
    cookies.split(';').find_map(|pair| {
        let (name, value) = pair.trim().split_once('=')?;
        (name == COOKIE_NAME).then(|| value.to_owned())
    })
}

fn query_value(query: &Option<String>, name: &str) -> Option<String> {
    query.as_deref()?.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == name).then(|| value.to_owned())
    })
}

fn unauthorized() -> Response {
    (StatusCode::UNAUTHORIZED, "unauthorized\n").into_response()
}

fn html(page: String) -> Response {
    let mut response = (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        page,
    )
        .into_response();
    response
        .headers_mut()
        .insert("Content-Security-Policy", HeaderValue::from_static(CSP));
    response
}

/// The bootstrap: `?token=<value>` verified against the one authority → the cookie whose
/// value IS the token (re-verified per request against the file-loaded bytes — no session
/// store, no second state), a 303 to the clean URL. The token never rides the Location and
/// never reaches a page byte.
fn bootstrap(state: &ServeState, presented: &str, clean_url: &str) -> Response {
    if !verified(state, presented) {
        return unauthorized();
    }
    // FIX-1 (Task 2 review): axum's Path extractor percent-decodes the segment, so a hostile
    // id can carry CR/LF into the redirect target. A value HeaderValue refuses is a 400 with
    // a reply — never a panic, never a dropped connection. The same uniform fallback covers
    // the cookie value: cheaper than reasoning about which arm is actually reachable.
    let Ok(location) = HeaderValue::from_str(clean_url) else {
        return (
            StatusCode::BAD_REQUEST,
            "the execution id is not addressable\n",
        )
            .into_response();
    };
    let cookie = format!("{COOKIE_NAME}={presented}; HttpOnly; SameSite=Strict; Path=/monitor");
    let Ok(cookie) = HeaderValue::from_str(&cookie) else {
        return (StatusCode::BAD_REQUEST, "the token is not header-safe\n").into_response();
    };
    let mut response = (StatusCode::SEE_OTHER, "").into_response();
    response.headers_mut().insert(header::LOCATION, location);
    response.headers_mut().insert(header::SET_COOKIE, cookie);
    response
}

/// `GET /monitor` — the index: every execution stream as a link. Cookie-gated.
pub(super) async fn monitor_index(
    State(state): State<ServeState>,
    RawQuery(query): RawQuery,
    headers: axum::http::HeaderMap,
) -> Response {
    if let Some(token) = query_value(&query, "token") {
        return bootstrap(&state, &token, "/monitor");
    }
    let Some(cookie) = cookie_token(&headers) else {
        return unauthorized();
    };
    if !verified(&state, &cookie) {
        return unauthorized();
    }
    let events = state.events.clone();
    let listed = tokio::task::spawn_blocking(move || {
        let store = crate::commands::event_store(&events)?;
        store.list_streams()
    })
    .await
    .expect("the listing task is never cancelled");
    let mut page = String::from(
        "<!doctype html>\n<html><head><meta charset=\"utf-8\"><title>GraphHelm monitor</title>\
         </head><body>\n<h1>Executions</h1>\n<ul>\n",
    );
    match listed {
        Ok(streams) => {
            for stream in streams {
                let id = escape(&stream.stream_id);
                page.push_str(&format!("<li><a href=\"/monitor/{id}\">{id}</a></li>\n"));
            }
        }
        Err(_) => page.push_str("<li>(the store could not be read)</li>\n"),
    }
    page.push_str("</ul>\n</body></html>\n");
    html(page)
}

/// `GET /monitor/{id}` — the page: bootstrap branch on `?token=`, steady-state branch on
/// the cookie, rendering Task 1's pure page over the same projection `status` folds.
pub(super) async fn monitor_page(
    State(state): State<ServeState>,
    UrlPath(id): UrlPath<String>,
    RawQuery(query): RawQuery,
    headers: axum::http::HeaderMap,
) -> Response {
    if let Some(token) = query_value(&query, "token") {
        // The id is a path segment the router already matched; the clean URL re-renders it.
        return bootstrap(&state, &token, &format!("/monitor/{id}"));
    }
    let Some(cookie) = cookie_token(&headers) else {
        return unauthorized();
    };
    if !verified(&state, &cookie) {
        return unauthorized();
    }
    let since: u64 = query_value(&query, "since")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let events_dir = state.events.clone();
    let target = id.clone();
    let rendered = tokio::task::spawn_blocking(
        move || -> Result<Option<String>, graphhelm_events::EventRepositoryError> {
            let store = crate::commands::event_store(&events_dir)?;
            let streams = store.list_streams()?;
            let Some(stream) = streams
                .into_iter()
                .find(|stream| stream.stream_id == target)
            else {
                return Ok(None);
            };
            let history = store.read_replay_stream(&stream.scope, &stream.stream_id)?;
            let projection = graphhelm_events::replay(&stream.scope, &stream.stream_id, &history)
                .map_err(|_| graphhelm_events::EventRepositoryError::Integrity)?;
            Ok(Some(render_monitor(
                &projection,
                &history,
                since,
                chrono::Utc::now(),
                &events_dir,
            )))
        },
    )
    .await
    .expect("the render task is never cancelled");
    match rendered {
        Ok(Some(page)) => html(page),
        Ok(None) => (StatusCode::NOT_FOUND, "no such execution\n").into_response(),
        // The error's own text never reaches the page: a monitor viewer holds a cookie, not
        // an operator role, and store diagnostics belong to the CLI surface.
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "the store could not be read\n",
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use graphhelm_protocols::{
        ActorId, EventHash, EventKind, ExecutionId, NewEvent, NodeOutcomeRecorded, OpaqueId,
        PersistedActor, PersistedActorType, PersistedTimestamp, ProjectId, RepositoryScope,
        Sensitivity, WorkspaceId,
    };

    const GENESIS: &str = "sha256:35c8ab0717bef1684ad07efcf3bedd4648c778a2c944cbd2c7e6a4802e2237b3";

    fn projection_fixture(hostile_node: &str) -> ExecutionProjection {
        let mut projection = ExecutionProjection {
            execution_id: Some("exec-monitor".to_owned()),
            ..ExecutionProjection::default()
        };
        projection
            .node_states
            .insert(hostile_node.to_owned(), NodeState::Running);
        projection
            .node_states
            .insert("review".to_owned(), NodeState::Blocked);
        projection
            .node_states
            .insert("deploy".to_owned(), NodeState::Ready);
        projection
            .last_outcome
            .insert("review".to_owned(), NodeOutcome::Interrupted);
        projection.node_attempts.insert("review".to_owned(), 2);
        projection
    }

    fn outcome_event(sequence: u64, node: &str, actor: &str) -> EventEnvelope {
        EventEnvelope::new(
            OpaqueId::parse(format!("event-{sequence}")).unwrap(),
            RepositoryScope::new(
                WorkspaceId::parse("workspace-1").unwrap(),
                ProjectId::parse("project-1").unwrap(),
                Some(ExecutionId::parse("exec-monitor").unwrap()),
            ),
            OpaqueId::parse("stream-1").unwrap(),
            sequence,
            PersistedTimestamp::from_datetime(Utc.with_ymd_and_hms(2026, 8, 16, 12, 0, 0).unwrap())
                .unwrap(),
            NewEvent::new(
                OpaqueId::parse(format!("request-{sequence}")).unwrap(),
                PersistedActor::new(
                    PersistedActorType::Agent,
                    ActorId::parse(actor.to_owned()).unwrap(),
                ),
                Sensitivity::Internal,
                EventKind::NodeOutcomeRecorded(NodeOutcomeRecorded {
                    execution_id: OpaqueId::parse("exec-monitor").unwrap(),
                    node_id: OpaqueId::parse(node.to_owned()).unwrap(),
                    outcome: NodeOutcome::Succeeded,
                    next_state: NodeState::Succeeded,
                }),
                vec![],
                vec![],
            ),
            EventHash::parse(GENESIS).unwrap(),
            EventHash::parse(GENESIS).unwrap(),
        )
    }

    #[test]
    fn the_monitor_page_contains_no_script_and_escapes_every_dynamic_string() {
        let hostile = "nodeimgonerror"; // node ids are wire-safe; hostility rides free text
        let mut projection = projection_fixture(hostile);
        // The hostile string enters through the one field the projection does not
        // wire-validate on this path: the execution id rendered into title/header/URL.
        projection.execution_id = Some("<img onerror=alert(1) src=x>".to_owned());
        let events = vec![outcome_event(1, "implement", "agent-scout")];
        let page = render_monitor(
            &projection,
            &events,
            0,
            Utc::now(),
            Path::new("C:/data/events"),
        );

        assert!(
            !page.to_ascii_lowercase().contains("<script"),
            "zero-JS by construction: {page}"
        );
        assert!(
            !page.contains("<img onerror"),
            "dynamic strings are escaped: {page}"
        );
        assert!(
            page.contains("&lt;img onerror=alert(1) src=x&gt;"),
            "the escaped form renders instead: {page}"
        );
        assert!(
            page.contains(&format!(
                "http-equiv=\"refresh\" content=\"{REFRESH_SECONDS};url=/monitor/"
            )) && page.contains("?since=1\">"),
            "the refresh tag carries the rendered head as the next cursor: {page}"
        );
    }

    #[test]
    fn the_snapshot_is_the_live_page_minus_exactly_the_refresh_tag() {
        // Same inputs, same now: the snapshot must be byte-equal to the live page with the
        // one refresh line removed — the tripwire against the snapshot forking into a
        // second renderer.
        let projection = projection_fixture("implement");
        let events = vec![outcome_event(1, "implement", "agent-scout")];
        let now = Utc.with_ymd_and_hms(2026, 8, 16, 12, 0, 5).unwrap();
        let live = render_monitor(&projection, &events, 0, now, Path::new("C:/data/events"));
        let snapshot = render_snapshot(&projection, &events, now, Path::new("C:/data/events"));

        assert!(!snapshot.contains("http-equiv=\"refresh\""), "{snapshot}");
        let live_without_refresh: String = live
            .lines()
            .filter(|line| !line.contains("http-equiv=\"refresh\""))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        assert_eq!(
            snapshot, live_without_refresh,
            "the snapshot is the live renderer, not a fork"
        );
    }

    #[test]
    fn staleness_blast_radius_and_remediation_render_for_a_stuck_story() {
        // A stuck story: implement Running with an OLD last event (silent past any bound),
        // review Blocked+Interrupted (the triage case). The fixture stream carries no graph
        // publication — the CLI/serve reality — so the blast radius column states that
        // honestly instead of guessing.
        let projection = projection_fixture("implement");
        let events = vec![outcome_event(1, "implement", "agent-scout")];
        let now = Utc.with_ymd_and_hms(2026, 8, 16, 12, 10, 0).unwrap(); // 600s after fixture
        let page = render_monitor(&projection, &events, 0, now, Path::new("C:/data/events"));

        assert!(
            page.contains("<span class=\"stale\">600s ago"),
            "a running node silent past its bound renders the silence loud: {page}"
        );
        assert!(
            page.contains("never"),
            "a node with no event yet says so: {page}"
        );
        let command = crate::commands::remediation::render_invocation(
            &RemediationAction::Approve,
            Path::new("C:/data/events"),
            "exec-monitor",
            "review",
        );
        assert!(
            page.contains(&format!("<code>{}</code>", command)),
            "the triage row hands the operator the EXACT command: {page}"
        );
        assert!(
            page.contains("unknown — the stream carries no graph publication"),
            "blast radius never guesses without edges: {page}"
        );
    }

    #[test]
    fn the_monitor_renders_states_triage_and_tail_from_the_same_projection_fixture() {
        let projection = projection_fixture("implement");
        let events = vec![
            outcome_event(1, "implement", "agent-scout"),
            outcome_event(2, "review", "agent-builder"),
        ];
        let page = render_monitor(
            &projection,
            &events,
            1,
            Utc::now(),
            Path::new("C:/data/events"),
        );

        // Every node state exactly once, from the same node_states map status reports.
        for (node, state) in [
            ("implement", "Running"),
            ("review", "Blocked"),
            ("deploy", "Ready"),
        ] {
            let row = format!("<tr><td>{node}</td><td>{state}</td>");
            assert_eq!(
                page.matches(&row).count(),
                1,
                "{node} renders its state exactly once: {page}"
            );
        }
        // The triage list is execution::render's untriaged rule over the same projection.
        assert!(
            page.contains("<b>review</b> interrupted and blocked"),
            "{page}"
        );
        // The tail line grammar: ACTOR verb TARGET, attributed.
        assert!(
            page.contains("<b>agent-builder</b> node_outcome_recorded review"),
            "the kill-feed grammar attributes the actor: {page}"
        );
        // The delta strip honors the cursor: sequence 1 is old news, 2 is fresh.
        let delta = page
            .split("changed since #1")
            .nth(1)
            .and_then(|rest| rest.split("</ul>").next())
            .unwrap();
        assert!(delta.contains("#2"), "{delta}");
        assert!(!delta.contains("#1 <b>"), "{delta}");
    }
}
