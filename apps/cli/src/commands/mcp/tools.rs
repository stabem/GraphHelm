//! The ten tools (Task 5): a static table mapping 1:1 onto Public Runtime API requests —
//! each description names the API call it maps to (§6 parity in the tool's own metadata),
//! every input schema is closed (`additionalProperties: false`), and no credential tool
//! exists by design (rule 5; §6 "no secret through chat" — omission is the enforcement, the
//! closed-list test its guard).

use super::client::{ApiClient, derive_key};
use super::rpc::{HandlerOutcome, INVALID_PARAMS};

/// A deliberate, narrow heuristic (CHAT_SURFACE_SPEC §6), not a scanner: string arguments
/// whose value starts with one of these prefixes are refused before any dispatch, and the
/// value itself is never echoed. Credentials travel the broker's stdin path, never the chat.
pub(crate) const SECRET_PREFIXES: [&str; 3] = ["sk-ant-", "sk-proj-", "-----BEGIN "];

struct ToolSpec {
    name: &'static str,
    description: &'static str,
    schema: fn() -> serde_json::Value,
}

/// The closed list, in the plan's order. Nothing else — the sabotage target.
const TOOLS: [ToolSpec; 14] = [
    ToolSpec {
        name: "start",
        description: "Start an execution (POST /v1/executions/{executionId}/start): load the \
                      graph file, optionally a fixtures file, in the given mode.",
        schema: start_schema,
    },
    ToolSpec {
        name: "status",
        description: "Read an execution's status (GET /v1/executions/{executionId}).",
        schema: execution_only_schema,
    },
    ToolSpec {
        name: "events",
        description: "Read an execution's event tail (GET /v1/executions/{executionId}/events \
                      with after/limit passed straight through; the API's bounds are the bounds).",
        schema: events_schema,
    },
    ToolSpec {
        name: "signal",
        description: "Record a signal envelope (POST /v1/executions/{executionId}/signal).",
        schema: signal_schema,
    },
    ToolSpec {
        name: "approve",
        description: "Approve a blocked or ghost node (POST /v1/executions/{executionId}/approve).",
        schema: approve_schema,
    },
    ToolSpec {
        name: "pause",
        description: "Pause an execution (POST /v1/executions/{executionId}/pause; mode \
                      \"immediate\" interrupts in-flight work, absent is the graceful default).",
        schema: pause_schema,
    },
    ToolSpec {
        name: "resume",
        description: "Resume a paused execution (POST /v1/executions/{executionId}/resume) \
                      against the graph file it started from.",
        schema: resume_schema,
    },
    ToolSpec {
        name: "cancel",
        description: "Cancel an execution (POST /v1/executions/{executionId}/cancel).",
        schema: cancel_schema,
    },
    ToolSpec {
        name: "routes",
        description: "List the gateway's routes (GET /v1/gateway/routes, optional manifest \
                      override).",
        schema: routes_schema,
    },
    ToolSpec {
        name: "wake_arm",
        description: "Arm THIS session's wake lease (POST /v1/executions/{executionId}/\
                      wake-lease): one content-free ring when the log moves past the cursor. \
                      A session can only ever arm itself; no tool rings another session.",
        schema: wake_arm_schema,
    },
    ToolSpec {
        name: "wake_status",
        description: "Read THIS session's live wake lease (GET /v1/executions/{executionId}/\
                      wake-lease).",
        schema: wake_status_schema,
    },
    ToolSpec {
        name: "amend_budget",
        description: "Declare a silence bound for one node AFTER the run began (POST                       /v1/executions/{executionId}/amend-budget), valid from that sequence                       forward. This is the operation the attention verdict's own remedy names:                       the seventh judge run found every reason pointing at declareNodeBudget                       while no exposed tool could declare one. Answers with the RECOMPUTED                       verdict, never a bare ok.",
        schema: amend_budget_schema,
    },
    ToolSpec {
        name: "wake_wait",
        description: "Block until THIS session's armed lease rings, or until the bound                       expires. Reads GET /v1/executions/{executionId}/wake-lease first to                       refuse any rendezvous this session does not hold, then blocks locally                       -- the block itself is NOT an API call, which is why this tool alone                       names the request it consults rather than the one it performs.                       Content-free by construction: the reply says THAT something happened,                       never what -- re-read the log to learn anything.",
        schema: wake_wait_schema,
    },
    ToolSpec {
        name: "probe",
        description: "Probe one gateway route's health (GET /v1/gateway/probe).",
        schema: probe_schema,
    },
];

fn object_schema(properties: serde_json::Value, required: &[&str]) -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

/// A mutating tool's schema: the same closed object plus the optional `ifMatch` head pin
/// every mutation carries per CHAT_SURFACE_SPEC §3 (optimistic concurrency by default —
/// mutate with `If-Match`, re-read and retry once on 409).
fn mutating_schema(mut properties: serde_json::Value, required: &[&str]) -> serde_json::Value {
    properties["ifMatch"] = serde_json::json!({
        "type": "integer",
        "description": "Optional head-sequence pin (the If-Match header): the mutation is \
                        refused with 409 and the current head when the store moved past it.",
    });
    object_schema(properties, required)
}

fn execution_only_schema() -> serde_json::Value {
    object_schema(
        serde_json::json!({"executionId": {"type": "string"}}),
        &["executionId"],
    )
}

fn cancel_schema() -> serde_json::Value {
    mutating_schema(
        serde_json::json!({"executionId": {"type": "string"}}),
        &["executionId"],
    )
}

fn start_schema() -> serde_json::Value {
    mutating_schema(
        serde_json::json!({
            "executionId": {"type": "string"},
            "file": {"type": "string"},
            "fixtures": {"type": "string"},
            "mode": {"type": "string", "enum": ["autopilot", "supervised", "manual"]},
        }),
        &["executionId", "file", "mode"],
    )
}

fn events_schema() -> serde_json::Value {
    object_schema(
        serde_json::json!({
            "executionId": {"type": "string"},
            "after": {"type": "integer"},
            "limit": {"type": "integer"},
        }),
        &["executionId"],
    )
}

fn signal_schema() -> serde_json::Value {
    mutating_schema(
        serde_json::json!({
            "executionId": {"type": "string"},
            "signal": {"type": "object"},
            "evidenceOut": {"type": "string"},
        }),
        &["executionId", "signal", "evidenceOut"],
    )
}

fn approve_schema() -> serde_json::Value {
    mutating_schema(
        serde_json::json!({
            "executionId": {"type": "string"},
            "node": {"type": "string"},
        }),
        &["executionId", "node"],
    )
}

fn pause_schema() -> serde_json::Value {
    mutating_schema(
        serde_json::json!({
            "executionId": {"type": "string"},
            "mode": {"type": "string", "enum": ["immediate"]},
        }),
        &["executionId"],
    )
}

fn resume_schema() -> serde_json::Value {
    mutating_schema(
        serde_json::json!({
            "executionId": {"type": "string"},
            "file": {"type": "string"},
            "fixtures": {"type": "string"},
        }),
        &["executionId", "file"],
    )
}

fn routes_schema() -> serde_json::Value {
    object_schema(serde_json::json!({"manifest": {"type": "string"}}), &[])
}

fn wake_arm_schema() -> serde_json::Value {
    mutating_schema(
        serde_json::json!({
            "executionId": {"type": "string"},
            "rendezvousId": {"type": "string",
                "description": "Opaque rendezvous identity — never a filesystem path; the \
                                sidecar derives the platform rendezvous from it."},
            "cursor": {"type": "integer",
                "description": "Ring for appends AFTER this sequence; defaults to the head."},
        }),
        &["executionId", "rendezvousId"],
    )
}

fn wake_status_schema() -> serde_json::Value {
    object_schema(
        serde_json::json!({"executionId": {"type": "string"}}),
        &["executionId"],
    )
}

/// The wait is bounded IN THE SCHEMA, not only in the code: a client reading the tool table
/// sees the ceiling without calling anything. An absent bound would make "wait" mean "hang",
/// and a chat client that hangs is indistinguishable from one that died.
/// The remedy handed back to the caller, as arguments. `seconds` has NO default and no
/// suggestion: the number is the operator's decision, and a hint here would be the invented
/// threshold this milestone deleted on day one, returning through the tool surface.
fn amend_budget_schema() -> serde_json::Value {
    mutating_schema(
        serde_json::json!({
            "executionId": {"type": "string"},
            "node": {"type": "string"},
            "seconds": {"type": "integer", "minimum": 1,
                "description": "The bound YOU decide. Nothing here suggests one."},
            "computedAtSequence": {"type": "integer", "minimum": 0,
                "description": "The frontier the verdict you are answering was computed at.                                 A stale amendment is refused with the current one."},
        }),
        &["executionId", "node", "seconds", "computedAtSequence"],
    )
}

fn wake_wait_schema() -> serde_json::Value {
    object_schema(
        serde_json::json!({
            "executionId": {"type": "string"},
            "rendezvousId": {"type": "string",
                "description": "Opaque rendezvous identity -- never a filesystem path."},
            "timeoutSeconds": {"type": "integer", "minimum": 1,
                "maximum": MAX_WAIT_SECONDS,
                "description": "Upper bound on the block; defaults to the maximum."},
        }),
        &["executionId", "rendezvousId"],
    )
}

fn probe_schema() -> serde_json::Value {
    object_schema(
        serde_json::json!({
            "route": {"type": "string"},
            "manifest": {"type": "string"},
        }),
        &["route"],
    )
}

/// The ceiling on a blocked MCP wait. Chosen to be shorter than any sane client's own
/// request timeout: the sidecar may block for far longer, but a chat transport that has
/// stopped answering looks dead, and "looks dead" is a worse failure than "timed out".
const MAX_WAIT_SECONDS: u64 = 300;

/// The blocking wait as an MCP primitive (M08 Task 4), with the 05g sleeper-only rule
/// intact: a session may wait ONLY on a lease it holds itself. That is enforced by asking
/// the API which lease THIS session (`nonce`) has live, and refusing any other rendezvous --
/// so a hostile or careless client cannot park itself on a peer's doorbell and consume the
/// ring that peer was waiting for.
///
/// The reply is content-free by construction: `rung` or `timeout`, nothing else. Whatever
/// bytes crossed the rendezvous die in the sidecar's wait; the caller learns only THAT it
/// should re-read its log.
fn wake_wait_tool(api: &ApiClient, nonce: &str, arguments: &serde_json::Value) -> HandlerOutcome {
    let (Some(execution), Some(rendezvous)) = (
        str_arg(arguments, "executionId"),
        str_arg(arguments, "rendezvousId"),
    ) else {
        return HandlerOutcome::Error {
            code: INVALID_PARAMS,
            message: "wake_wait needs both executionId and rendezvousId".to_owned(),
        };
    };
    let bound = arguments
        .get("timeoutSeconds")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(MAX_WAIT_SECONDS)
        .clamp(1, MAX_WAIT_SECONDS);

    // Sleeper-only, asked of the API rather than assumed: the lease this session holds is
    // the only rendezvous it may block on.
    let live = api.request(
        "GET",
        &format!("/v1/executions/{execution}/wake-lease?sessionId={nonce}"),
        None,
        None,
        None,
    );
    let held = match live {
        Ok((_status, envelope)) => envelope["data"]["rendezvousId"].as_str().map(str::to_owned),
        Err(transport) => {
            return HandlerOutcome::Result(serde_json::json!({
                "content": [{"type": "text", "text": format!("the API is unreachable: {transport}")}],
                "isError": true,
            }));
        }
    };
    if held.as_deref() != Some(rendezvous) {
        return HandlerOutcome::Result(serde_json::json!({
            "content": [{"type": "text", "text": format!(
                "refused: this session holds {held:?}, not {rendezvous:?} -- a session waits only on its own lease (05g sleeper-only)"
            )}],
            "isError": true,
        }));
    }

    let outcome = match crate::commands::wake_wait::wait(rendezvous, bound) {
        crate::commands::wake_wait::WaitEnd::Rung => "rung",
        crate::commands::wake_wait::WaitEnd::TimedOut => "timeout",
        crate::commands::wake_wait::WaitEnd::Unusable(reason) => {
            return HandlerOutcome::Result(serde_json::json!({
                "content": [{"type": "text", "text": format!("the rendezvous is unusable: {reason}")}],
                "isError": true,
            }));
        }
    };
    // Content-free: the outcome word and the bound that produced it. Never a payload.
    HandlerOutcome::Result(serde_json::json!({
        "content": [{"type": "text", "text": serde_json::json!({
            "ok": true,
            "command": "wake.wait",
            "data": {"outcome": outcome, "timeoutSeconds": bound},
        }).to_string()}],
        "isError": false,
    }))
}

/// The `tools/list` reply body: the closed table, verbatim.
pub(crate) fn tool_list() -> serde_json::Value {
    let tools: Vec<serde_json::Value> = TOOLS
        .iter()
        .map(|tool| {
            serde_json::json!({
                "name": tool.name,
                "description": tool.description,
                "inputSchema": (tool.schema)(),
            })
        })
        .collect();
    serde_json::json!({"tools": tools})
}

/// Walks every string value in `arguments` (recursively) against [`SECRET_PREFIXES`].
/// Returns true when a secret-shaped value is present — the caller refuses WITHOUT echoing.
fn contains_secret_shaped(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(text) => SECRET_PREFIXES
            .iter()
            .any(|prefix| text.trim_start().starts_with(prefix)),
        serde_json::Value::Array(items) => items.iter().any(contains_secret_shaped),
        serde_json::Value::Object(map) => map.values().any(contains_secret_shaped),
        _ => false,
    }
}

fn str_arg<'a>(arguments: &'a serde_json::Value, name: &str) -> Option<&'a str> {
    arguments.get(name).and_then(serde_json::Value::as_str)
}

fn require<'a>(
    arguments: &'a serde_json::Value,
    name: &'static str,
) -> Result<&'a str, HandlerOutcome> {
    str_arg(arguments, name).ok_or(HandlerOutcome::Error {
        code: INVALID_PARAMS,
        message: format!("the tool requires a string {name:?} argument"),
    })
}

/// One tool call: the secret guard first, then exactly one API request; the tool result is
/// the API envelope verbatim as text content, `isError` mirroring the envelope's `ok`.
pub(crate) fn call(
    api: &ApiClient,
    nonce: &str,
    rpc_id: &serde_json::Value,
    name: &str,
    arguments: &serde_json::Value,
) -> HandlerOutcome {
    if !TOOLS.iter().any(|tool| tool.name == name) {
        return HandlerOutcome::Error {
            code: INVALID_PARAMS,
            message: format!("no tool named {name:?} is part of this server"),
        };
    }
    if contains_secret_shaped(arguments) {
        return HandlerOutcome::Error {
            code: INVALID_PARAMS,
            message: "a secret-shaped value was refused; use the broker's stdin path — \
                      credentials never travel the chat"
                .to_owned(),
        };
    }

    // `wake_wait` is the one tool whose work is NOT an API request: it blocks on the local
    // rendezvous. It is handled before the request table rather than inside it, because
    // folding a blocking local wait into the arm that builds HTTP calls is how a surface
    // grows a second meaning for the same shape.
    if name == "wake_wait" {
        return wake_wait_tool(api, nonce, arguments);
    }

    let key = derive_key(nonce, rpc_id);
    let if_match = arguments.get("ifMatch").and_then(serde_json::Value::as_u64);
    let outcome = match name {
        "status" => require(arguments, "executionId")
            .map(|id| api.request("GET", &format!("/v1/executions/{id}"), None, None, None)),
        "events" => require(arguments, "executionId").map(|id| {
            let mut query = String::new();
            if let Some(after) = arguments.get("after").and_then(serde_json::Value::as_u64) {
                query.push_str(&format!("after={after}"));
            }
            if let Some(limit) = arguments.get("limit").and_then(serde_json::Value::as_u64) {
                if !query.is_empty() {
                    query.push('&');
                }
                query.push_str(&format!("limit={limit}"));
            }
            let path = if query.is_empty() {
                format!("/v1/executions/{id}/events")
            } else {
                format!("/v1/executions/{id}/events?{query}")
            };
            api.request("GET", &path, None, None, None)
        }),
        "start" => require(arguments, "executionId").map(|id| {
            let mut body = serde_json::json!({
                "file": str_arg(arguments, "file").unwrap_or_default(),
                "mode": str_arg(arguments, "mode").unwrap_or_default(),
            });
            if let Some(fixtures) = str_arg(arguments, "fixtures") {
                body["fixtures"] = serde_json::json!(fixtures);
            }
            api.request(
                "POST",
                &format!("/v1/executions/{id}/start"),
                Some(&body),
                Some(&key),
                if_match,
            )
        }),
        "signal" => require(arguments, "executionId").map(|id| {
            let body = serde_json::json!({
                "signal": arguments.get("signal").cloned().unwrap_or(serde_json::Value::Null),
                "evidenceOut": str_arg(arguments, "evidenceOut").unwrap_or_default(),
            });
            api.request(
                "POST",
                &format!("/v1/executions/{id}/signal"),
                Some(&body),
                Some(&key),
                if_match,
            )
        }),
        "approve" => require(arguments, "executionId").map(|id| {
            let body = serde_json::json!({"node": str_arg(arguments, "node").unwrap_or_default()});
            api.request(
                "POST",
                &format!("/v1/executions/{id}/approve"),
                Some(&body),
                Some(&key),
                if_match,
            )
        }),
        "pause" => require(arguments, "executionId").map(|id| {
            let body = match str_arg(arguments, "mode") {
                Some(mode) => serde_json::json!({"mode": mode}),
                None => serde_json::json!({}),
            };
            api.request(
                "POST",
                &format!("/v1/executions/{id}/pause"),
                Some(&body),
                Some(&key),
                if_match,
            )
        }),
        "resume" => require(arguments, "executionId").map(|id| {
            let mut body =
                serde_json::json!({"file": str_arg(arguments, "file").unwrap_or_default()});
            if let Some(fixtures) = str_arg(arguments, "fixtures") {
                body["fixtures"] = serde_json::json!(fixtures);
            }
            api.request(
                "POST",
                &format!("/v1/executions/{id}/resume"),
                Some(&body),
                Some(&key),
                if_match,
            )
        }),
        "cancel" => require(arguments, "executionId").map(|id| {
            api.request(
                "POST",
                &format!("/v1/executions/{id}/cancel"),
                Some(&serde_json::json!({})),
                Some(&key),
                if_match,
            )
        }),
        "wake_arm" => require(arguments, "executionId").map(|id| {
            // The session can only arm ITSELF: sessionId is the session's own nonce, never
            // an argument — the sleeper-only rule in the dispatch itself.
            let mut body = serde_json::json!({
                "sessionId": nonce,
                "rendezvousId": str_arg(arguments, "rendezvousId").unwrap_or_default(),
            });
            if let Some(cursor) = arguments.get("cursor").and_then(serde_json::Value::as_u64) {
                body["cursor"] = serde_json::json!(cursor);
            }
            api.request(
                "POST",
                &format!("/v1/executions/{id}/wake-lease"),
                Some(&body),
                Some(&key),
                if_match,
            )
        }),
        "amend_budget" => require(arguments, "executionId").map(|id| {
            let body = serde_json::json!({
                "node": str_arg(arguments, "node").unwrap_or_default(),
                "seconds": arguments.get("seconds").and_then(serde_json::Value::as_u64),
                "computedAtSequence": arguments
                    .get("computedAtSequence")
                    .and_then(serde_json::Value::as_u64),
            });
            api.request(
                "POST",
                &format!("/v1/executions/{id}/amend-budget"),
                Some(&body),
                Some(&key),
                if_match,
            )
        }),
        "wake_status" => require(arguments, "executionId").map(|id| {
            api.request(
                "GET",
                &format!("/v1/executions/{id}/wake-lease?sessionId={nonce}"),
                None,
                None,
                None,
            )
        }),
        "routes" => Ok(match str_arg(arguments, "manifest") {
            Some(manifest) => api.request(
                "GET",
                &format!("/v1/gateway/routes?manifest={manifest}"),
                None,
                None,
                None,
            ),
            None => api.request("GET", "/v1/gateway/routes", None, None, None),
        }),
        "probe" => require(arguments, "route").map(|route| {
            let mut path = format!("/v1/gateway/probe?route={route}");
            if let Some(manifest) = str_arg(arguments, "manifest") {
                path.push_str(&format!("&manifest={manifest}"));
            }
            api.request("GET", &path, None, None, None)
        }),
        _ => unreachable!("the closed-list check above already refused unknown names"),
    };

    let response = match outcome {
        Err(refusal) => return refusal,
        Ok(response) => response,
    };
    match response {
        Ok((_status, envelope)) => {
            let is_error = envelope.get("ok") != Some(&serde_json::Value::Bool(true));
            HandlerOutcome::Result(serde_json::json!({
                "content": [{"type": "text", "text": envelope.to_string()}],
                "isError": is_error,
            }))
        }
        // No HTTP response at all (server down, timeout): a tool-level error result, not a
        // protocol error — the chat can see and retry. The transport's Display never carries
        // a header value, so relaying it is redaction-safe.
        Err(transport) => HandlerOutcome::Result(serde_json::json!({
            "content": [{"type": "text", "text": format!("the API is unreachable: {transport}")}],
            "isError": true,
        })),
    }
}
