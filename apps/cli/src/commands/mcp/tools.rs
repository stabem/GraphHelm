//! The tool table: a static table over Public Runtime API requests and the local wake wait —
//! each description names the API call it maps to (§6 parity in the tool's own metadata),
//! every input schema is closed (`additionalProperties: false`), and no credential tool
//! exists by design (rule 5; §6 "no secret through chat" — omission is the enforcement, the
//! closed-list test its guard).
//!
//! **No count is stated here on purpose.** This prose said "fourteen" while `TOOLS` held
//! eighteen, having drifted as each development operation family landed (#372, the class #272
//! names). The population lives in the array's own arity, and
//! `apps/cli/tests/development_surface_parity.rs` is what keeps that population honest: every
//! tool there must be a declared development family or a named exception. Prose cannot be
//! guarded, so it no longer carries a number to be wrong about.

use super::client::{ApiClient, derive_key};
use super::rpc::{HandlerOutcome, INVALID_PARAMS};
use super::url;

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
const TOOLS: [ToolSpec; 19] = [
    ToolSpec {
        name: "start",
        description: "Start an execution (POST /v1/executions/{executionId}/start): load the \
                      graph file, optionally a fixtures file, in the given mode. Mode governs \
                      graph-mutation autonomy only (autopilot accepts proposals automatically, \
                      supervised holds a proposal queued until the owner approves it, manual \
                      rejects every proposal outright with nothing queued) - it does NOT hold \
                      dispatch, a ready node runs the same way in every mode; use \"pause\" to \
                      hold dispatch.",
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
        description: "Declare how long one node may stay silent before it needs you (POST /v1/executions/{executionId}/amend-budget), valid from that point forward. Use it when `status` answers `unknown` for a node: the answer names this operation as its remedy. Replies with the recomputed verdict, so you do not have to read again to find out what changed.",
        schema: amend_budget_schema,
    },
    ToolSpec {
        name: "wake_wait",
        description: "Block until THIS session's armed lease rings, or until the deadline that lease declared. Reads GET /v1/executions/{executionId}/wake-lease first and takes BOTH the rendezvous and the deadline from it -- the caller supplies an identity, never a duration, so arming with one horizon and waiting on another cannot be expressed. Then it blocks locally; the block itself is NOT an API call, which is why this tool alone names the request it consults rather than the one it performs. Content-free by construction: the reply says THAT something happened, never what -- re-read the log to learn anything.",
        schema: wake_wait_schema,
    },
    ToolSpec {
        name: "probe",
        description: "Probe one gateway route's health (GET /v1/gateway/probe).",
        schema: probe_schema,
    },
    ToolSpec {
        name: "resolve_contract",
        description: "Resolve the code-rule contract from layered rule sources (POST \
                      /v1/development/contract). #223 existence-slice: no sources argument yet.",
        schema: resolve_contract_schema,
    },
    ToolSpec {
        name: "memory_status",
        description: "Report the governed memory states, transitions, and the moves policy \
                      allows (GET /v1/development/memory). Reads the shipped transition policy; \
                      no record id, because nothing persists a memory record yet.",
        schema: memory_status_schema,
    },
    ToolSpec {
        name: "present",
        description: "Render the owner-facing presentation of a task result (POST \
                      /v1/development/present). #223 existence-slice: no result argument yet.",
        schema: present_schema,
    },
    ToolSpec {
        name: "compile_context",
        description: "Compile a context capsule and report its digest (POST \
                      /v1/development/context). #223 existence-slice: no capsule content \
                      argument yet.",
        schema: compile_context_schema,
    },
    ToolSpec {
        name: "memory_propose",
        description: "Propose content for governed memory and report the admission verdict \
                      (POST /v1/development/memory). Returns the verdict and no id: nothing \
                      persists a candidate, so an id would name what cannot be fetched.",
        schema: memory_propose_schema,
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

fn resolve_contract_schema() -> serde_json::Value {
    // No fields yet: #223's existence-slice always resolves against an empty source list.
    // Sources become a real argument when the behavioral-parity guard (blueprint §6 item 2)
    // wires this to actual rule artifacts.
    object_schema(serde_json::json!({}), &[])
}

fn memory_status_schema() -> serde_json::Value {
    // No fields, and NOT because the arguments have not been designed yet: the operation reads the
    // shipped transition policy, which takes no parameter. A record id would be the argument, and
    // nothing persists a record to name -- see `commands::development::run_memory_status` for the
    // measurement and for when the id returns.
    object_schema(serde_json::json!({}), &[])
}

fn present_schema() -> serde_json::Value {
    // No fields yet, for the same reason as resolve_contract_schema: #223's existence-slice
    // renders a fixed no-decision result. The task result becomes a real argument when the
    // behavioral-parity guard (blueprint section 6 item 2) wires this to actual task outcomes.
    object_schema(serde_json::json!({}), &[])
}

fn compile_context_schema() -> serde_json::Value {
    // No fields yet: #223's existence-slice always compiles the empty/degenerate capsule.
    // Capsule content becomes a real argument when the behavioral-parity guard (blueprint §6
    // item 2) wires this to actual sections.
    object_schema(serde_json::json!({}), &[])
}

fn memory_propose_schema() -> serde_json::Value {
    // No fields yet: the existence-slice proposes fixed content under a fixed scope. Content and
    // scope become real arguments when behavioral parity wires this to caller input.
    object_schema(serde_json::json!({}), &[])
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
            "maturesInSeconds": {"type": "integer", "minimum": 1, "maximum": 315576000,
                "description": "How long quiet may last before the wait ends by itself. Omit it and nothing promises to end the wait: the lease rings on an append or not at all."},
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
                "description": "The frontier the verdict you are answering was computed at. A stale amendment is refused with the current one."},
        }),
        &["executionId", "node", "seconds", "computedAtSequence"],
    )
}

fn wake_wait_schema() -> serde_json::Value {
    // Identity in, never a duration and never a rendezvous. Both come from the lease this
    // session armed, so arming with one horizon and waiting on another cannot be expressed
    // here either -- the CLI half of this surface was fixed first, and two definitions of one
    // tool is the very defect being removed.
    object_schema(
        serde_json::json!({"executionId": {"type": "string"}}),
        &["executionId"],
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
    let Some(execution) = str_arg(arguments, "executionId") else {
        return HandlerOutcome::Error {
            code: INVALID_PARAMS,
            message: "wake_wait needs executionId".to_owned(),
        };
    };

    // ONE read, and exactly one thing read from it: the lease belonging to THIS session. The
    // rendezvous and the deadline both come from there. This tool used to take a rendezvous id
    // and a `timeoutSeconds` from its caller, which left two numbers answering "how long before
    // I give up" -- the bound the sleeper declared at arming, and whatever the call happened to
    // carry. Two surfaces of one tool disagreeing about when an operator should wake is the
    // defect §8 promises against, and this half was the one the pair itself sleeps on.
    let live = api.request(
        "GET",
        &format!(
            "{}?sessionId={nonce}",
            url::segment_path(&["v1", "executions", execution, "wake-lease"])
        ),
        None,
        None,
        None,
    );
    let lease = match live {
        Ok((_status, envelope)) => envelope["data"].clone(),
        Err(transport) => {
            return refused(&format!("the API is unreachable: {transport}"));
        }
    };
    let Some(rendezvous) = lease["rendezvousId"].as_str().map(str::to_owned) else {
        return refused(
            "refused: this session holds no live lease -- a session waits only on its own (05g sleeper-only)",
        );
    };
    let Some(matures_at) = lease["maturesAt"].as_str() else {
        return refused(
            "refused: this session's lease declared no bound, so nothing here promises to end the wait -- arm again with maturesInSeconds",
        );
    };
    let Ok(matures_at) = chrono::DateTime::parse_from_rfc3339(matures_at) else {
        return refused("refused: the lease carries a horizon that cannot be read");
    };

    let remaining = matures_at
        .with_timezone(&chrono::Utc)
        .signed_duration_since(chrono::Utc::now());
    if remaining <= chrono::Duration::zero() {
        // Already past when asked: answer at once. Blocking would make the one case where
        // something has already gone wrong the one case this tool sits quiet through.
        return matured_reply(true);
    }

    let outcome = match crate::commands::wake_wait::wait(
        &rendezvous,
        u64::try_from(remaining.num_seconds()).unwrap_or(1).max(1),
    ) {
        crate::commands::wake_wait::WaitEnd::Rung => "rung",
        // The only bound is the declared one, so this cannot mean "the number I passed ran
        // out" any more.
        crate::commands::wake_wait::WaitEnd::TimedOut => return matured_reply(false),
        crate::commands::wake_wait::WaitEnd::Unusable(reason) => {
            return refused(&format!("the rendezvous is unusable: {reason}"));
        }
    };
    // Content-free: the outcome word alone. Never a payload.
    HandlerOutcome::Result(serde_json::json!({
        "content": [{"type": "text", "text": serde_json::json!({
            "ok": true,
            "command": "wake.wait",
            "data": {"outcome": outcome},
        }).to_string()}],
        "isError": false,
    }))
}

fn refused(message: &str) -> HandlerOutcome {
    HandlerOutcome::Result(serde_json::json!({
        "content": [{"type": "text", "text": message}],
        "isError": true,
    }))
}

fn matured_reply(already_past: bool) -> HandlerOutcome {
    HandlerOutcome::Result(serde_json::json!({
        "content": [{"type": "text", "text": serde_json::json!({
            "ok": true,
            "command": "wake.wait",
            "data": {"outcome": "timeout", "matured": true, "alreadyPast": already_past},
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
        "status" => require(arguments, "executionId").map(|id| {
            api.request(
                "GET",
                &url::segment_path(&["v1", "executions", id]),
                None,
                None,
                None,
            )
        }),
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
                url::segment_path(&["v1", "executions", id, "events"])
            } else {
                format!(
                    "{}?{query}",
                    url::segment_path(&["v1", "executions", id, "events"])
                )
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
                &url::segment_path(&["v1", "executions", id, "start"]),
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
                &url::segment_path(&["v1", "executions", id, "signal"]),
                Some(&body),
                Some(&key),
                if_match,
            )
        }),
        "approve" => require(arguments, "executionId").map(|id| {
            let body = serde_json::json!({"node": str_arg(arguments, "node").unwrap_or_default()});
            api.request(
                "POST",
                &url::segment_path(&["v1", "executions", id, "approve"]),
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
                &url::segment_path(&["v1", "executions", id, "pause"]),
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
                &url::segment_path(&["v1", "executions", id, "resume"]),
                Some(&body),
                Some(&key),
                if_match,
            )
        }),
        "cancel" => require(arguments, "executionId").map(|id| {
            api.request(
                "POST",
                &url::segment_path(&["v1", "executions", id, "cancel"]),
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
            // M09 decision B: the bound the sleeper declares. Passed through untouched —
            // absent means absent, and no default is supplied here or anywhere else.
            if let Some(seconds) = arguments
                .get("maturesInSeconds")
                .and_then(serde_json::Value::as_u64)
            {
                body["maturesInSeconds"] = serde_json::json!(seconds);
            }
            api.request(
                "POST",
                &url::segment_path(&["v1", "executions", id, "wake-lease"]),
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
                &url::segment_path(&["v1", "executions", id, "amend-budget"]),
                Some(&body),
                Some(&key),
                if_match,
            )
        }),
        "wake_status" => require(arguments, "executionId").map(|id| {
            api.request(
                "GET",
                &format!(
                    "{}?sessionId={nonce}",
                    url::segment_path(&["v1", "executions", id, "wake-lease"])
                ),
                None,
                None,
                None,
            )
        }),
        "routes" => Ok(match str_arg(arguments, "manifest") {
            Some(manifest) => api.request(
                "GET",
                &format!("/v1/gateway/routes?manifest={}", url::query_value(manifest)),
                None,
                None,
                None,
            ),
            None => api.request("GET", "/v1/gateway/routes", None, None, None),
        }),
        "probe" => require(arguments, "route").map(|route| {
            let mut path = format!("/v1/gateway/probe?route={}", url::query_value(route));
            if let Some(manifest) = str_arg(arguments, "manifest") {
                path.push_str(&format!("&manifest={}", url::query_value(manifest)));
            }
            api.request("GET", &path, None, None, None)
        }),
        // #223 existence-slice: no required arguments yet (see resolve_contract_schema).
        "resolve_contract" => Ok(api.request(
            "POST",
            &url::segment_path(&["v1", "development", "contract"]),
            Some(&serde_json::json!({})),
            Some(&key),
            if_match,
        )),
        // A READ: GET, no body, and no idempotency key. The key exists to make a mutation safe to
        // repeat; attaching one to a read would claim this changes something.
        "memory_status" => Ok(api.request(
            "GET",
            &url::segment_path(&["v1", "development", "memory"]),
            None,
            None,
            None,
        )),
        // #223 existence-slice: no required arguments yet (see present_schema).
        "present" => Ok(api.request(
            "POST",
            &url::segment_path(&["v1", "development", "present"]),
            Some(&serde_json::json!({})),
            Some(&key),
            if_match,
        )),
        // #223 existence-slice: no required arguments yet (see compile_context_schema).
        "compile_context" => Ok(api.request(
            "POST",
            &url::segment_path(&["v1", "development", "context"]),
            Some(&serde_json::json!({})),
            Some(&key),
            if_match,
        )),
        // A MUTATION in shape -- POST with a body and an idempotency key -- even though the
        // existence-slice persists nothing: the verb and the key describe what this operation IS,
        // and wiring it as a read would have to be undone the day it stores anything.
        "memory_propose" => Ok(api.request(
            "POST",
            &url::segment_path(&["v1", "development", "memory"]),
            Some(&serde_json::json!({})),
            Some(&key),
            if_match,
        )),
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
