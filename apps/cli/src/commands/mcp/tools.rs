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
const TOOLS: [ToolSpec; 31] = [
    ToolSpec {
        name: "start",
        description: "Start an execution (POST /v1/executions/{executionId}/start). Minimal \
                      call: {executionId: <an id YOU choose, new on this store>, mode: \
                      \"supervised\", file: <graph path on the RUNTIME's host, e.g. \
                      examples/graphs/software-feature.yaml>} - or \"graph\" (an inline graph \
                      object) instead of \"file\"; one of the two is required. Pass held: true \
                      to start WITHOUT driving: the Runtime otherwise drives the graph to \
                      quiescence before it answers, which with a model-backed executor can \
                      outlast this client's 30 s timeout - the start then DID happen, and a \
                      retry is refused as \"already started\"; read \"status\" instead of \
                      retrying. Optionally a fixtures file. Mode governs graph-mutation \
                      autonomy only (autopilot accepts proposals automatically, supervised \
                      holds a proposal queued until the owner approves it, manual rejects every \
                      proposal outright with nothing queued) - it does NOT hold dispatch, a \
                      ready node runs the same way in every mode; use \"pause\" to hold \
                      dispatch.",
        schema: start_schema,
    },
    ToolSpec {
        name: "list",
        description: "List the executions this store holds (GET /v1/executions): one \
                      summary row per stream - execution id, mode, status, attention, head \
                      sequence, and the start/last-event instants - ordered by execution id. \
                      \"after\" is an EXCLUSIVE cursor naming the last id already read; \
                      \"limit\" is 1..=100 and defaults to 20. A larger limit is refused rather \
                      than clamped, so a short page always means a short store. Read-only: it \
                      appends nothing.",
        schema: list_schema,
    },
    ToolSpec {
        name: "topology",
        description: "Read a graph file's shape (POST /v1/graph/topology): entrypoints, \
                      nodes and edges, plus the semantic hash that says WHICH graph it is. The \
                      hash is the point - an execution's log records its graph's hash and never \
                      its topology, so before claiming these edges belong to a run, compare this \
                      hash against the graphHash in that run's execution_started event. \
                      \"file\" is a path on the RUNTIME's host, not yours. Nodes carry identity \
                      only: no objectives, agents or completion controls. Read-only.",
        schema: topology_schema,
    },
    ToolSpec {
        name: "status",
        description: "Read an execution's status (GET /v1/executions/{executionId}).",
        schema: execution_only_schema,
    },
    ToolSpec {
        name: "briefing",
        description: "Read an execution's resume briefing (GET /v1/executions/{executionId}/briefing): \
                      what the run is for (name, objective), what runs its nodes, the graph \
                      hash to verify the file `resume` needs, every decision in order with the \
                      actor that made it, the work done, what is pending, and the next step. \
                      Call this FIRST when picking up an execution another session or harness \
                      drove - it is derived from the store alone and is identical on CLI, HTTP \
                      and MCP. Read-only: it appends nothing.",
        schema: execution_only_schema,
    },
    ToolSpec {
        name: "events",
        description: "Read an execution's event tail (GET /v1/executions/{executionId}/events \
                      with after/limit passed straight through; the API's bounds are the bounds).",
        schema: events_schema,
    },
    ToolSpec {
        name: "evidence",
        description: "Open the content behind an evidence reference (GET /v1/executions/{executionId}/evidence/{evidenceId}).",
        schema: evidence_schema,
    },
    ToolSpec {
        name: "signal",
        description: "Record a signal envelope (POST /v1/executions/{executionId}/signal).",
        schema: signal_schema,
    },
    ToolSpec {
        name: "document_read",
        description: "Read registered project text (POST /v1/executions/{executionId}/documents/read). Uses a delivery evidence reference; no arbitrary path or project root is accepted.",
        schema: document_read_schema,
    },
    ToolSpec {
        name: "document_save",
        description: "Save an owner edit (POST /v1/executions/{executionId}/documents/save). Requires an owner-configured MCP session and expectedSha256; preserves session actor headers. Retry the same RPC id for the same edit. Notifications report recording, not agent acknowledgment.",
        schema: document_save_schema,
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
        name: "route_set",
        description: "Add or replace one direct_api route in the gateway manifest (PUT /v1/gateway/routes). Writes the route's declaration only -- its API key is never an argument here, because a value passed as a tool argument lands in the harness's own transcript; key the route with `gateway credential set` or the HTTP body.",
        schema: route_set_schema,
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
    ToolSpec {
        name: "accounting",
        description: "Report a context-accounting receipt (GET /v1/development/accounting). \
                      #223 existence-slice: no execution id yet, and the one field reports as \
                      unavailable rather than zero -- nothing was measured.",
        schema: accounting_schema,
    },
    ToolSpec {
        name: "sweep",
        description: "Evaluate an execution's customs stages and journal the result (POST \
                      /v1/executions/{executionId}/sweep): one \"sweep_performed\", plus one \
                      \"overdue_exception\" per episode found lapsed, appended together. A sweep \
                      that finds nothing STILL writes its record - otherwise \"no exceptions\" \
                      and \"no sweep ever ran\" are the same absence in the log. Optional \
                      \"asOf\" asks about a past instant and defaults to now; THE FUTURE IS \
                      REFUSED, because a future-dated answer is indistinguishable from a real one \
                      while permanently spending the episodes it touches.",
        schema: sweep_schema,
    },
    ToolSpec {
        name: "claim",
        description: "Claim that a waiting_input node's external work is done, presenting \
                      evidence (POST /v1/executions/{executionId}/claim). Testimony only: nothing \
                      is released until \"clear\" countersigns. A claim the pipeline cannot accept \
                      is journaled as \"completion_refused\" with its registry code, and the reply \
                      says so under \"claim\". Requires the graph the execution started from \
                      (\"file\" or inline \"graph\"); \"evidence\" is an array of \
                      {kind, contentHash, size}; \"waitSeq\" names the exact wait and defaults to \
                      the node's open one; \"asserter\" defaults to the calling actor.",
        schema: claim_schema,
    },
    ToolSpec {
        name: "clear",
        description: "Countersign a claim by machine replay (POST /v1/executions/{executionId}/clear): \
                      present \"manifestHash\" or the \"evidence\" bundle whose digest is computed \
                      here. A match clears the node and drives its dependents; a mismatch is \
                      journaled as a rejection and drives nothing. \"countersign\" is refused at \
                      the door until the wire carries a signature (#529). Requires the graph \
                      (\"file\" or \"graph\") and the \"claimSeq\" the claim reply named.",
        schema: clear_schema,
    },
    ToolSpec {
        name: "synthesize",
        description: "Compile a goal into a graph document (POST /v1/graphs/synthesize): the \
                      Graph Architect asks a model for ONE draft, validates it through the \
                      same schema, lint and executor checks an authored file takes, repairs at \
                      most twice, and returns the document with the compiler's rationale - or \
                      refuses with the diagnostics. It publishes nothing and starts nothing: \
                      the document is what \"start\" takes as an inline \"graph\". \
                      \"allowPrograms\" is the allowlist a shell node may name; it defaults to \
                      the server's own and is never widened by the compiler. \"fixture\" is a \
                      recorded-replies file on the RUNTIME's host (the keyless door); without \
                      it the server's model route, or \"route\", answers.",
        schema: synthesize_schema,
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
    // `budget` and `require` are what make the budget refusal reachable from this surface (#393).
    // Both are optional: omitting them is the pre-#393 call, which compiles the degenerate
    // capsule, and the existence-parity guard still sends exactly that.
    //
    // Capsule CONTENT is still not an argument here. `require` sizes the budget decision and does
    // not become capsule sections -- those are a closed vocabulary and choosing which one caller
    // input lands in is behavioral-parity work (blueprint §6 item 2), not a side effect of this.
    object_schema(
        serde_json::json!({
            "budget": {
                "type": "integer",
                "minimum": 0,
                "description": "Token budget the capsule must fit within. Required context that \
                                does not fit is refused, never trimmed.",
            },
            "require": {
                "type": "array",
                "items": {"type": "string"},
                "description": "Required context sections. Never dropped to fit.",
            },
        }),
        &[],
    )
}

fn memory_propose_schema() -> serde_json::Value {
    // No fields yet: the existence-slice proposes fixed content under a fixed scope. Content and
    // scope become real arguments when behavioral parity wires this to caller input.
    object_schema(serde_json::json!({}), &[])
}

fn accounting_schema() -> serde_json::Value {
    // No fields yet: there is no execution to name until the existence-slice grows an id
    // argument, which is behavioral-parity work, not this guard's job.
    object_schema(serde_json::json!({}), &[])
}

fn list_schema() -> serde_json::Value {
    // No `executionId`: this is the tool a caller reaches for when it does not yet know one.
    object_schema(
        serde_json::json!({
            "after": {
                "type": "string",
                "description": "Exclusive cursor: the last execution id already read.",
            },
            "limit": {"type": "integer", "minimum": 1, "maximum": 100},
        }),
        &[],
    )
}

fn topology_schema() -> serde_json::Value {
    object_schema(
        serde_json::json!({
            "file": {
                "type": "string",
                "minLength": 1,
                "description": "Graph file path on the Runtime host.",
            },
        }),
        &["file"],
    )
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

/// `file` IS NO LONGER REQUIRED HERE, and the requirement did not move into this schema.
///
/// A start now takes its graph as `file` OR `graph` — exactly one — and JSON Schema can only say
/// that with a `oneOf` across two required-lists, which this repository's closed-object schemas do
/// not build. Declaring `file` required would lock every MCP client out of inline graphs; declaring
/// neither required and stopping there would let a caller send both.
///
/// So the rule lives in ONE place, on the server: `graph_source`
/// (`commands::serve::routes`) refuses "neither" and refuses "both", each with a message naming
/// the field. That is the authority either way — an MCP client is not the only caller — and a
/// second copy of the rule expressed in schema would be a copy that can disagree with it.
fn start_schema() -> serde_json::Value {
    mutating_schema(
        serde_json::json!({
            "executionId": {"type": "string"},
            "file": {"type": "string"},
            "graph": {"type": "object"},
            "fixtures": {"type": "string"},
            "mode": {"type": "string", "enum": ["autopilot", "supervised", "manual"]},
            "route": {"type": "string"},
            "held": {
                "type": "boolean",
                "description": "true starts the execution without driving it (the HTTP \
                                spelling of `execution start --held`, #90): the call returns \
                                as soon as the start is recorded, so it cannot outlast the \
                                client timeout. Omitted or false drives to quiescence first.",
            },
        }),
        &["executionId", "mode"],
    )
}

/// Read-only and closed, like every other read schema here. Both ids are required: an evidence
/// id without the execution that recorded it has no scope to be read in, and the route refuses a
/// reference one execution holds when another asks for it.
fn evidence_schema() -> serde_json::Value {
    object_schema(
        serde_json::json!({
            "executionId": {"type": "string"},
            "evidenceId": {"type": "string"},
        }),
        &["executionId", "evidenceId"],
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

/// `evidenceOut` is NO LONGER REQUIRED. An agent recording a note through this surface has a path
/// on the Runtime's host and could always supply one, but it should not have to: when the Runtime
/// was started with a keyring the envelope seals into the Evidence store before the event appends,
/// which is the durable copy, and an operator file is then a second copy of something already safe.
/// A Runtime with no keyring still needs the path, and says so in a diagnostic rather than dropping
/// the envelope.
fn signal_schema() -> serde_json::Value {
    mutating_schema(
        serde_json::json!({
            "executionId": {"type": "string"},
            "signal": {"type": "object"},
            "evidenceOut": {"type": "string"},
        }),
        &["executionId", "signal"],
    )
}

fn document_reference_schema() -> serde_json::Value {
    object_schema(
        serde_json::json!({
            "evidenceId": {"type":"string","minLength":1},
            "index": {"type":"integer","minimum":0,"maximum":31}
        }),
        &["evidenceId", "index"],
    )
}

fn document_read_schema() -> serde_json::Value {
    object_schema(
        serde_json::json!({
            "executionId":{"type":"string","minLength":1},
            "document":document_reference_schema()
        }),
        &["executionId", "document"],
    )
}

fn document_save_schema() -> serde_json::Value {
    // File revisions replace stream-head concurrency here. Neither If-Match nor caller-supplied
    // idempotency headers are accepted: the same logical MCP act derives both key copies.
    object_schema(
        serde_json::json!({
            "executionId":{"type":"string","minLength":1},
            "document":document_reference_schema(),
            "content":{"type":"string","maxLength":131072},
            "expectedSha256":{"type":"string","pattern":"^[a-f0-9]{64}$"},
            "reason":{"type":"string","minLength":1,"maxLength":2048}
        }),
        &[
            "executionId",
            "document",
            "content",
            "expectedSha256",
            "reason",
        ],
    )
}

fn document_request(
    name: &str,
    arguments: &serde_json::Value,
    key: &str,
) -> Result<(String, serde_json::Value), HandlerOutcome> {
    let invalid = || {
        HandlerOutcome::Error { code: INVALID_PARAMS, message: "document tools require a bounded registered reference and edit; If-Match and caller-supplied idempotency keys are not accepted".to_owned() }
    };
    let fields: &[&str] = if name == "document_read" {
        &["executionId", "document"]
    } else {
        &[
            "executionId",
            "document",
            "content",
            "expectedSha256",
            "reason",
        ]
    };
    let object = arguments.as_object().ok_or_else(invalid)?;
    if object.keys().any(|field| !fields.contains(&field.as_str())) {
        return Err(invalid());
    }
    let execution = require(arguments, "executionId")?;
    graphhelm_protocols::OpaqueId::parse(execution).map_err(|_| invalid())?;
    let reference = arguments.get("document").ok_or_else(invalid)?;
    let document: crate::commands::execution::documents::DocumentReference =
        serde_json::from_value(reference.clone()).map_err(|_| invalid())?;
    if document.index >= 32
        || graphhelm_protocols::EvidenceId::parse(&document.evidence_id).is_err()
    {
        return Err(invalid());
    }
    if name == "document_read" {
        return Ok((execution.to_owned(), reference.clone()));
    }
    let content = require(arguments, "content")?;
    let reason = require(arguments, "reason")?;
    let hash = require(arguments, "expectedSha256")?;
    if content.len() > graphhelm_tool_host::documents::MAX_DOCUMENT_BYTES
        || reason.trim().is_empty()
        || reason.len() > 2048
        || hash.len() != 64
        || !hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid());
    }
    Ok((
        execution.to_owned(),
        serde_json::json!({"document":reference,"content":content,"expectedSha256":hash,"reason":reason,"idempotencyKey":key}),
    ))
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

/// Same shape as `start_schema`, for the same reason — see its comment for why `file` stopped
/// being required here rather than the rule being restated in schema.
fn resume_schema() -> serde_json::Value {
    mutating_schema(
        serde_json::json!({
            "executionId": {"type": "string"},
            "file": {"type": "string"},
            "graph": {"type": "object"},
            "fixtures": {"type": "string"},
            "route": {"type": "string"},
        }),
        &["executionId"],
    )
}

fn sweep_schema() -> serde_json::Value {
    mutating_schema(
        serde_json::json!({
            "executionId": {"type": "string"},
            "asOf": {"type": "string"},
        }),
        &["executionId"],
    )
}

fn claim_schema() -> serde_json::Value {
    mutating_schema(
        serde_json::json!({
            "executionId": {"type": "string"},
            "file": {"type": "string"},
            "graph": {"type": "object"},
            "node": {"type": "string"},
            "waitSeq": {"type": "integer"},
            "evidence": {"type": "array", "items": {"type": "object"}},
            "asserter": {"type": "string"},
            "mode": {"type": "string"},
        }),
        &["executionId", "node"],
    )
}

fn clear_schema() -> serde_json::Value {
    mutating_schema(
        serde_json::json!({
            "executionId": {"type": "string"},
            "file": {"type": "string"},
            "graph": {"type": "object"},
            "fixtures": {"type": "string"},
            "route": {"type": "string"},
            "claimSeq": {"type": "integer"},
            "manifestHash": {"type": "string"},
            "evidence": {"type": "array", "items": {"type": "object"}},
            "verifier": {"type": "string"},
        }),
        &["executionId", "claimSeq"],
    )
}

fn routes_schema() -> serde_json::Value {
    object_schema(serde_json::json!({"manifest": {"type": "string"}}), &[])
}

fn route_set_schema() -> serde_json::Value {
    object_schema(
        serde_json::json!({
            "id": {"type": "string",
                "description": "The route id callers name. Unique in the manifest: an existing id needs replace."},
            "provider": {"type": "string",
                "description": "The WIRE FORMAT the adapter speaks -- anthropic, openai or typesafe -- not the vendor. A DeepSeek endpoint is an openai route with its own baseUrl."},
            "baseUrl": {"type": "string",
                "description": "https://..., or http:// to a loopback address. No trailing slash."},
            "model": {"type": "string"},
            "credentialRef": {"type": "string",
                "description": "The broker reference holding this route's key. Absent: secret_<id>."},
            "enabled": {"type": "boolean",
                "description": "Absent: true. False parks the route: listed, and refused at dispatch."},
            "replace": {"type": "boolean",
                "description": "Absent: false, and an existing id is refused with the manifest left byte-identical."},
            "manifest": {"type": "string",
                "description": "Manifest path override; absent, the Runtime's own."},
        }),
        &["id", "provider", "baseUrl", "model"],
    )
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

/// Closed like every other schema here. `fixture` and `route` are both optional and the route
/// refuses the pair: the "exactly one door" rule lives on the server (`serve::routes::
/// synthesize`), where the CLI's `--fixture`/`--manifest --route` rule already lives, for the
/// same reason `start_schema` does not encode file-or-graph.
fn synthesize_schema() -> serde_json::Value {
    object_schema(
        serde_json::json!({
            "goal": {
                "type": "string",
                "minLength": 1,
                "description": "What the graph must achieve, in the operator's words.",
            },
            "mode": {"type": "string", "enum": ["autopilot", "supervised", "manual"]},
            "maxNodes": {"type": "integer", "minimum": 1, "maximum": 50},
            "allowPrograms": {
                "type": "array",
                "items": {"type": "string"},
                "description": "Programs a shell node may name; defaults to the server's own \
                                allowlist and is never widened by the compiler.",
            },
            "fixture": {
                "type": "string",
                "description": "Recorded-replies file path on the Runtime host: the keyless \
                                model door.",
            },
            "route": {
                "type": "string",
                "description": "A route id of the server's manifest; the deployer's default \
                                when absent.",
            },
            "judgeRoute": {
                "type": "string",
                "description": "A direct_api typesafe route id of the server's manifest: the \
                                judge door over the gateway. Exclusive with judgeFixture; no \
                                judge means no judgment is asked.",
            },
            "judgeFixture": {
                "type": "string",
                "description": "Recorded-answers file path on the Runtime host: the keyless \
                                judge door.",
            },
            "drafts": {
                "type": "integer",
                "minimum": 1,
                "maximum": 3,
                "description": "How many drafts to ask for and rank; more than one needs a \
                                judge.",
            },
            "library": {
                "type": "string",
                "description": "A template directory path on the Runtime host the judge may \
                                choose to reuse or adapt.",
            },
        }),
        &["goal"],
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

/// An argument that is a JSON OBJECT, for the one tool argument that carries a document rather
/// than a scalar (`graph`). Returns `None` for a present-but-wrong-typed value exactly as
/// `str_arg` does; the server refuses that case with a message naming the field, so forwarding a
/// number as if it were a graph is not this layer's job to invent an error for.
fn object_arg<'a>(arguments: &'a serde_json::Value, name: &str) -> Option<&'a serde_json::Value> {
    arguments.get(name).filter(|value| value.is_object())
}

/// An argument that is a JSON ARRAY, copied whole — the customs `evidence` bundle. The same
/// posture as `object_arg`: a present-but-wrong-typed value is `None` here and refused by the
/// server with a message naming the field.
fn array_arg<'a>(arguments: &'a serde_json::Value, name: &str) -> Option<&'a serde_json::Value> {
    arguments.get(name).filter(|value| value.is_array())
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

/// The codes the Runtime answers with when the BODY had the wrong shape - the refusals a
/// minimal example can cure. Any other `ok:false` (state, authority, unreadable evidence by a real
/// id) is left as the envelope alone: an example would be noise beside a diagnostic about state.
const SHAPE_REFUSAL_CODES: [&str; 2] = [
    crate::error_codes::GHCLI001_ARGUMENT_INVALID,
    crate::error_codes::GHCLI003_SIGNAL_INVALID,
];

/// The smallest call this server accepts for `name`, derived from the tool's OWN schema: every
/// `required` property, a placeholder each, and - for the few whose required list is not the
/// whole story (`start` needs `file` or `graph`; a signal is a whole envelope) - the extra shape.
///
/// Derived, not hand-written, because a hand-written table of 31 examples is a second copy of the
/// schemas that drifts the day a field is added (the same reason `MCP_TOOL_NAMES` lives in one
/// place). Placeholders are angle-bracketed so nothing here can be mistaken for a value that
/// exists; NO caller argument is ever echoed back, so no secret can travel in a refusal.
///
/// Measured before this existed (token-bench, task 1044, GraphHelm arm): 7 of 11 tool calls in
/// one session were refused for shape - `start` x2, `evidence` x2, `signal` x2, `probe` - each
/// answered with one missing field name, and the session guessed the next shape wrong again.
pub(crate) fn minimal_example(name: &str) -> serde_json::Value {
    let Some(tool) = TOOLS.iter().find(|tool| tool.name == name) else {
        return serde_json::Value::Null;
    };
    let schema = (tool.schema)();
    let mut example = example_from_schema(name, &schema);
    // `file` or `graph` is required by the Runtime, not by the schema (see `start_schema`).
    if name == "start" {
        example.insert(
            "file".to_owned(),
            serde_json::json!(
                "<graph path on the Runtime's host, e.g. examples/graphs/software-feature.yaml>"
            ),
        );
    }
    serde_json::Value::Object(example)
}

/// Every `required` property of an object schema, a placeholder each - and a nested object schema
/// (one carrying its own `properties`/`required`, as `document` does in `document_read`) yields
/// the nested example rather than `{}`. Review of #1199 measured the difference: `"document": {}`
/// was PRESENT, so a presence-only guard was green, and it told the caller nothing - the caller
/// copied it and was refused again, the loop this surface exists to end.
fn example_from_schema(
    tool: &str,
    schema: &serde_json::Value,
) -> serde_json::Map<String, serde_json::Value> {
    let mut example = serde_json::Map::new();
    let Some(required) = schema.get("required").and_then(serde_json::Value::as_array) else {
        return example;
    };
    for field in required.iter().filter_map(serde_json::Value::as_str) {
        let property = schema
            .get("properties")
            .and_then(|properties| properties.get(field));
        let declared = property
            .and_then(|property| property.get("type"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("string");
        // An object property derives from its own schema whenever that schema names properties,
        // required or not; `required` alone was the gate before, which left an object WITHOUT a
        // required list falling through to the `{}` placeholder this surface exists to abolish.
        let nested = property
            .filter(|property| declared == "object" && property.get("properties").is_some());
        let value = match nested {
            Some(inner) => serde_json::Value::Object(example_from_schema(tool, inner)),
            None => placeholder(tool, field, declared),
        };
        example.insert(field.to_owned(), value);
    }
    example
}

/// A placeholder for one required field: a real example for the fields whose shape the schema
/// does not spell out (a signal envelope, a mode's enum, a route id), a typed marker otherwise.
fn placeholder(tool: &str, field: &str, declared: &str) -> serde_json::Value {
    match (tool, field) {
        (_, "executionId") => {
            serde_json::json!("<an execution id: new for start, existing otherwise>")
        }
        (_, "mode") => serde_json::json!("supervised"),
        (_, "evidenceId") => serde_json::json!("<an evidenceRefs id from an events row>"),
        (_, "route") => serde_json::json!("<a route id from `routes`, e.g. judge>"),
        (_, "node") => serde_json::json!("<a nodeId from the execution's form>"),
        // Every field `schemas/graph-signal.schema.json` requires - the cell
        // `the_signal_example_carries_every_field_the_signal_schema_requires` reads that file, so
        // a required field added to the schema reddens this example instead of the next session.
        // The first version of this example lacked `evidence` and `emittedAt`: measured, a session
        // copied it and was refused twice more (token-bench 1044, v5).
        ("signal", "signal") => serde_json::json!({
            "id": "<a new signal id you choose>",
            "source": {"type": "node", "id": "<a nodeId>"},
            "type": "finding.root_cause",
            "severity": "high",
            "description": "<what was found, in one paragraph>",
            "evidence": ["<one observation per entry: a command and what it printed>"],
            "emittedAt": "<RFC 3339 instant, e.g. 2026-09-22T16:45:00Z>"
        }),
        _ => match declared {
            "integer" => serde_json::json!(1),
            "number" => serde_json::json!(1.0),
            "boolean" => serde_json::json!(true),
            // Never `{}`: an empty object is PRESENT, so a presence check passes, and it tells the
            // caller nothing (measured on document_read, review of #1199). An object schema that
            // names no properties cannot be derived; say that instead of shipping a blank. Not
            // `unreachable!()` - this runs inside a refusal path, which must not panic.
            "object" => serde_json::json!({
                "<not derivable>": format!("the schema for {field:?} names no properties; read the tool's schema")
            }),
            "array" => serde_json::json!([]),
            _ => serde_json::json!(format!("<{field}>")),
        },
    }
}

/// A refusal carries its reason AND the smallest valid call. The reason keeps its code
/// (`INVALID_PARAMS` for an argument the client refused; the Runtime's own `GHCLI…` diagnostics
/// verbatim in the envelope, which is never edited) - the example rides beside it, never
/// instead of it.
fn with_example(name: &str, outcome: HandlerOutcome) -> HandlerOutcome {
    let example = minimal_example(name);
    match outcome {
        HandlerOutcome::Error { code, message } if code == INVALID_PARAMS => {
            HandlerOutcome::Error {
                code,
                message: format!("{message}; minimal valid call for {name:?}: {example}"),
            }
        }
        HandlerOutcome::Result(mut result) => {
            let shape_refusal = result
                .get("isError")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
                && result
                    .get("content")
                    .and_then(serde_json::Value::as_array)
                    .and_then(|content| content.first())
                    .and_then(|first| first.get("text"))
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|text| SHAPE_REFUSAL_CODES.iter().any(|code| text.contains(code)));
            if let Some(content) = shape_refusal
                .then(|| result.get_mut("content"))
                .flatten()
                .and_then(serde_json::Value::as_array_mut)
            {
                content.push(serde_json::json!({
                    "type": "text",
                    "text": format!("minimal valid call for {name:?}: {example}"),
                }));
            }
            HandlerOutcome::Result(result)
        }
        other => other,
    }
}

/// What a timed-out `start` reports once the client has looked: the status envelope when the
/// Runtime holds the execution (the start happened; not an error, the caller must NOT retry it),
/// and the transport error with the next step when it does not.
pub(crate) fn start_after_timeout(
    transport: &str,
    observed: Option<(u16, serde_json::Value)>,
) -> HandlerOutcome {
    match observed {
        Some((_, envelope)) if envelope.get("ok") == Some(&serde_json::Value::Bool(true)) => {
            HandlerOutcome::Result(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": format!(
                        "the start reached the Runtime and is driving; this client timed out waiting \
                         for the drive ({transport}). Do NOT call start again for this executionId \
                         - it would be refused as already started. Current status: {envelope}"
                    ),
                }],
                "isError": false,
            }))
        }
        // Reachable, and the RUNTIME ITSELF says it holds no execution under that id: a status
        // reply the server answered (2xx/404 class) whose diagnostics carry the execution-state
        // code. Only then is "not recorded; retry" a fact the input carries. This is the arm
        // review found had no cell: without the `ok:true` guard above it fell into the success
        // text and told the caller not to retry a start that never happened.
        Some((status, envelope)) if runtime_denies_execution(status, &envelope) => {
            HandlerOutcome::Result(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": format!(
                        "the start request timed out ({transport}) and the Runtime does NOT report a \
                         running execution for this executionId - the start was not recorded; retry \
                         it, or start with \"held\": true. Status reply: {envelope}"
                    ),
                }],
                "isError": true,
            }))
        }
        // Reachable, but the reply says nothing about the execution: a 401 on a rotated token, a
        // 500, a 429, an envelope without the execution-state code. Review of bde1867c measured
        // the hazard of treating this as the arm above: the start HAD landed and was driving, the
        // GET failed for its own reason, the tool said "retry", and the agent drove it twice - the
        // outcome the observer exists to prevent, reached through the other door. So: hedge, and
        // send the caller to `status` rather than to either action.
        Some((status, envelope)) => HandlerOutcome::Result(serde_json::json!({
            "content": [{
                "type": "text",
                "text": format!(
                    "the start request timed out ({transport}) and the follow-up status read did \
                     not answer whether the start was recorded (HTTP {status}). Do not assume \
                     either way: read \"status\" for this executionId until it answers, and only \
                     then retry or continue. Reply: {envelope}"
                ),
            }],
            "isError": true,
        })),
        None => HandlerOutcome::Result(serde_json::json!({
            "content": [{
                "type": "text",
                "text": format!(
                    "the API is unreachable: {transport} - the start was NOT observed on the Runtime \
                     either; retry, or start with \"held\": true so the call returns before driving"
                ),
            }],
            "isError": true,
        })),
    }
}

/// The one reply that licenses the categorical "not recorded": the Runtime answered the status
/// read itself (a 2xx or 404-class reply, not a proxy's 401/5xx) AND named the execution state
/// in its diagnostics. Anything else is the transport or the server talking about something other
/// than this execution, and gets the hedged text.
fn runtime_denies_execution(status: u16, envelope: &serde_json::Value) -> bool {
    let answered = (200..300).contains(&status) || status == 404;
    let denies = envelope
        .get("diagnostics")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|diagnostics| {
            diagnostics.iter().any(|diagnostic| {
                diagnostic.get("code").and_then(serde_json::Value::as_str)
                    == Some(crate::error_codes::GHCLI005_EXECUTION_STATE)
            })
        });
    answered && denies
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
        return with_example(name, wake_wait_tool(api, nonce, arguments));
    }

    let key = derive_key(nonce, rpc_id);
    let if_match = arguments.get("ifMatch").and_then(serde_json::Value::as_u64);
    let outcome = match name {
        "document_read" | "document_save" => {
            document_request(name, arguments, &key).map(|(id, body)| {
                let save = name == "document_save";
                api.request(
                    "POST",
                    &url::segment_path(&[
                        "v1",
                        "executions",
                        &id,
                        "documents",
                        if save { "save" } else { "read" },
                    ]),
                    Some(&body),
                    save.then_some(key.as_str()),
                    None,
                )
            })
        }
        "topology" => require(arguments, "file").map(|file| {
            api.request(
                "POST",
                &url::segment_path(&["v1", "graph", "topology"]),
                Some(&serde_json::json!({ "file": file })),
                None,
                None,
            )
        }),
        "list" => {
            let mut query = String::new();
            if let Some(after) = str_arg(arguments, "after") {
                query.push_str(&format!("after={}", url::encode_component(after)));
            }
            if let Some(limit) = arguments.get("limit").and_then(serde_json::Value::as_u64) {
                if !query.is_empty() {
                    query.push('&');
                }
                query.push_str(&format!("limit={limit}"));
            }
            let path = if query.is_empty() {
                url::segment_path(&["v1", "executions"])
            } else {
                format!("{}?{query}", url::segment_path(&["v1", "executions"]))
            };
            Ok(api.request("GET", &path, None, None, None))
        }
        "status" => require(arguments, "executionId").map(|id| {
            api.request(
                "GET",
                &url::segment_path(&["v1", "executions", id]),
                None,
                None,
                None,
            )
        }),
        "briefing" => require(arguments, "executionId").map(|id| {
            api.request(
                "GET",
                &url::segment_path(&["v1", "executions", id, "briefing"]),
                None,
                None,
                None,
            )
        }),
        "evidence" => require(arguments, "executionId").and_then(|id| {
            let evidence_id = require(arguments, "evidenceId")?;
            Ok(api.request(
                "GET",
                &url::segment_path(&["v1", "executions", id, "evidence", evidence_id]),
                None,
                None,
                None,
            ))
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
        "start" => require(arguments, "executionId")
            .map_err(|_| HandlerOutcome::Error {
                code: INVALID_PARAMS,
                // The bare "requires executionId" cost a bench session three calls before its
                // first accepted start (token-bench, task 1044): the contract is named whole.
                message: "start requires a string \"executionId\" (an id you choose), a \
                          \"mode\", and one of \"file\" (a graph path on the Runtime's host) \
                          or \"graph\" (an inline graph object); pass \"held\": true to \
                          start without driving"
                    .to_owned(),
            })
            .map(|id| {
                let mut body = serde_json::json!({
                    "mode": str_arg(arguments, "mode").unwrap_or_default(),
                });
                if let Some(held) = arguments.get("held").and_then(serde_json::Value::as_bool) {
                    body["held"] = serde_json::json!(held);
                }
                // `file` IS COPIED ONLY WHEN GIVEN. It used to be built with `unwrap_or_default()`,
                // which put `"file": ""` in the body whenever the argument was absent. That was
                // invisible while the schema made `file` required, and becomes a lie the moment it is
                // optional: an empty string is a PRESENT file, so the server would refuse it as an
                // unreadable path instead of reading the `graph` the caller actually sent.
                if let Some(file) = str_arg(arguments, "file") {
                    body["file"] = serde_json::json!(file);
                }
                if let Some(graph) = object_arg(arguments, "graph") {
                    body["graph"] = graph.clone();
                }
                if let Some(fixtures) = str_arg(arguments, "fixtures") {
                    body["fixtures"] = serde_json::json!(fixtures);
                }
                if let Some(route) = str_arg(arguments, "route") {
                    body["route"] = serde_json::json!(route);
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
            let mut body = serde_json::json!({
                "signal": arguments.get("signal").cloned().unwrap_or(serde_json::Value::Null),
            });
            // COPIED ONLY WHEN GIVEN, for the reason `file` above is: `unwrap_or_default()` put
            // `"evidenceOut": ""` in the body whenever the argument was absent, and an empty string
            // is a PRESENT path. Harmless while the field was required and nobody omitted it; a lie
            // the moment it is optional, because the server would take the empty string as a path
            // it cannot write instead of sealing the envelope, which is what absence now means.
            if let Some(evidence_out) = str_arg(arguments, "evidenceOut") {
                body["evidenceOut"] = serde_json::json!(evidence_out);
            }
            api.request(
                "POST",
                &url::segment_path(&["v1", "executions", id, "signal"]),
                Some(&body),
                Some(&key),
                if_match,
            )
        }),
        "sweep" => require(arguments, "executionId").map(|id| {
            // Only a supplied `asOf` travels. Sending an explicit null, or this process's own idea
            // of "now", would replace the STORE'S clock with a second one -- and the store's is the
            // reference the verb's future-refusal is enforced against.
            let mut body = serde_json::json!({});
            if let Some(as_of) = str_arg(arguments, "asOf") {
                body["asOf"] = serde_json::json!(as_of);
            }
            api.request(
                "POST",
                &url::segment_path(&["v1", "executions", id, "sweep"]),
                Some(&body),
                Some(&key),
                if_match,
            )
        }),
        "claim" => require(arguments, "executionId").map(|id| {
            // Every field is copied ONLY WHEN GIVEN (see `start`'s branch): a defaulted empty
            // string is a PRESENT value the server would refuse for the wrong reason, and an
            // absent `waitSeq` MEANS "the node's open wait".
            let mut body = serde_json::json!({});
            if let Some(file) = str_arg(arguments, "file") {
                body["file"] = serde_json::json!(file);
            }
            if let Some(graph) = object_arg(arguments, "graph") {
                body["graph"] = graph.clone();
            }
            if let Some(node) = str_arg(arguments, "node") {
                body["node"] = serde_json::json!(node);
            }
            if let Some(wait_seq) = arguments.get("waitSeq").and_then(serde_json::Value::as_u64) {
                body["waitSeq"] = serde_json::json!(wait_seq);
            }
            if let Some(evidence) = array_arg(arguments, "evidence") {
                body["evidence"] = evidence.clone();
            }
            if let Some(asserter) = str_arg(arguments, "asserter") {
                body["asserter"] = serde_json::json!(asserter);
            }
            if let Some(mode) = str_arg(arguments, "mode") {
                body["mode"] = serde_json::json!(mode);
            }
            api.request(
                "POST",
                &url::segment_path(&["v1", "executions", id, "claim"]),
                Some(&body),
                Some(&key),
                if_match,
            )
        }),
        "clear" => require(arguments, "executionId").map(|id| {
            let mut body = serde_json::json!({});
            if let Some(file) = str_arg(arguments, "file") {
                body["file"] = serde_json::json!(file);
            }
            if let Some(graph) = object_arg(arguments, "graph") {
                body["graph"] = graph.clone();
            }
            if let Some(fixtures) = str_arg(arguments, "fixtures") {
                body["fixtures"] = serde_json::json!(fixtures);
            }
            if let Some(route) = str_arg(arguments, "route") {
                body["route"] = serde_json::json!(route);
            }
            if let Some(claim_seq) = arguments
                .get("claimSeq")
                .and_then(serde_json::Value::as_u64)
            {
                body["claimSeq"] = serde_json::json!(claim_seq);
            }
            if let Some(manifest_hash) = str_arg(arguments, "manifestHash") {
                body["manifestHash"] = serde_json::json!(manifest_hash);
            }
            if let Some(evidence) = array_arg(arguments, "evidence") {
                body["evidence"] = evidence.clone();
            }
            if let Some(verifier) = str_arg(arguments, "verifier") {
                body["verifier"] = serde_json::json!(verifier);
            }
            api.request(
                "POST",
                &url::segment_path(&["v1", "executions", id, "clear"]),
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
            // See `start`'s branch for why `file` is conditional rather than defaulted.
            let mut body = serde_json::json!({});
            if let Some(file) = str_arg(arguments, "file") {
                body["file"] = serde_json::json!(file);
            }
            if let Some(graph) = object_arg(arguments, "graph") {
                body["graph"] = graph.clone();
            }
            if let Some(fixtures) = str_arg(arguments, "fixtures") {
                body["fixtures"] = serde_json::json!(fixtures);
            }
            if let Some(route) = str_arg(arguments, "route") {
                body["route"] = serde_json::json!(route);
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
        "route_set" => require(arguments, "id").and_then(|id| {
            let provider = require(arguments, "provider")?;
            let base_url = require(arguments, "baseUrl")?;
            let model = require(arguments, "model")?;
            let mut body = serde_json::json!({
                "id": id,
                "provider": provider,
                "baseUrl": base_url,
                "model": model,
            });
            // Absent stays ABSENT rather than becoming a default here: the HTTP handler owns the
            // defaults, and a tool that filled them in would be a second place they are decided.
            for field in ["credentialRef"] {
                if let Some(value) = str_arg(arguments, field) {
                    body[field] = serde_json::Value::String(value.to_owned());
                }
            }
            for field in ["enabled", "replace"] {
                if let Some(value) = arguments.get(field) {
                    body[field] = value.clone();
                }
            }
            let path = match str_arg(arguments, "manifest") {
                Some(manifest) => {
                    format!("/v1/gateway/routes?manifest={}", url::query_value(manifest))
                }
                None => "/v1/gateway/routes".to_owned(),
            };
            Ok(api.request("PUT", &path, Some(&body), Some(&key), if_match))
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
        // Forwards `budget` and `require` so the budget refusal is reachable here too (#393).
        // Both default to the pre-#393 degenerate call, so a caller that sends neither gets what
        // it always got. The refusal needs no special handling below: the route answers 422 with
        // `ok: false`, and the shared tail already turns a non-ok envelope into `isError: true`
        // with the envelope as the content -- which is where a tool-execution failure belongs,
        // rather than in a JSON-RPC protocol error.
        "compile_context" => Ok(api.request(
            "POST",
            &url::segment_path(&["v1", "development", "context"]),
            Some(&serde_json::json!({
                "budget": arguments
                    .get("budget")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0),
                "require": arguments
                    .get("require")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!([])),
            })),
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
        // A READ: GET, no body, no idempotency key -- matching status/memory_status above.
        "accounting" => Ok(api.request(
            "GET",
            &url::segment_path(&["v1", "development", "accounting"]),
            None,
            None,
            None,
        )),
        // A READ-SHAPED POST like topology: no idempotency key, no If-Match -- synthesis
        // publishes, starts and appends nothing. Optional fields travel only when given, so
        // the route's own defaults (the profile's mode, the server's allowlist) apply exactly
        // as they do to a raw HTTP caller.
        "synthesize" => require(arguments, "goal").map(|goal| {
            let mut body = serde_json::json!({ "goal": goal });
            for field in [
                "mode",
                "fixture",
                "route",
                "judgeRoute",
                "judgeFixture",
                "library",
            ] {
                if let Some(value) = str_arg(arguments, field) {
                    body[field] = serde_json::Value::String(value.to_owned());
                }
            }
            for field in ["maxNodes", "drafts"] {
                if let Some(count) = arguments.get(field).and_then(serde_json::Value::as_u64) {
                    body[field] = serde_json::json!(count);
                }
            }
            if let Some(programs) = arguments.get("allowPrograms") {
                body["allowPrograms"] = programs.clone();
            }
            api.request(
                "POST",
                &url::segment_path(&["v1", "graphs", "synthesize"]),
                Some(&body),
                None,
                None,
            )
        }),
        _ => unreachable!("the closed-list check above already refused unknown names"),
    };

    let response = match outcome {
        Err(refusal) => return with_example(name, refusal),
        Ok(response) => response,
    };
    with_example(
        name,
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
            Err(transport) => {
                // A start drives before it answers, so a timeout here does not mean the start was
                // lost: it usually means it happened. The same rule #1202 states for a process tree
                // - a timeout returns an OBSERVED state, never a guess - applied to the one tool
                // whose request outlives the client's patience: observe once, bounded to a single
                // GET, and report what the Runtime holds. (Measured: token-bench task 1044, four
                // calls for one start before this existed.)
                if let Some(id) = (name == "start")
                    .then(|| str_arg(arguments, "executionId"))
                    .flatten()
                {
                    let observed = api.request(
                        "GET",
                        &url::segment_path(&["v1", "executions", id]),
                        None,
                        None,
                        None,
                    );
                    return start_after_timeout(&transport.to_string(), observed.ok());
                }
                HandlerOutcome::Result(serde_json::json!({
                    "content": [{"type": "text", "text": format!("the API is unreachable: {transport}")}],
                    "isError": true,
                }))
            }
        },
    )
}

#[cfg(test)]
mod envelope_tests {
    use super::*;
    use zeroize::Zeroizing;

    /// Does a reply tell the caller not to retry the start? Case-folded, over every phrasing the
    /// arms use, so a rewording of the success arm cannot slip past it the way a byte-match on
    /// one sentence would.
    fn advises_against_retry(text: &str) -> bool {
        let folded = text.to_ascii_lowercase();
        [
            "not call start again",
            "do not retry",
            "don't retry",
            "not retry",
            "do not start again",
        ]
        .iter()
        .any(|phrase| folded.contains(phrase))
    }

    /// Every tool's minimal example carries every field its own schema requires - the invariant
    /// that makes the example worth sending. A tool added without a placeholder for a new
    /// required field still passes (the typed marker covers it); a tool whose example LOST a
    /// required field does not.
    #[test]
    fn every_tools_minimal_example_satisfies_its_own_required_list() {
        for tool in TOOLS.iter() {
            let schema = (tool.schema)();
            let example = minimal_example(tool.name);
            let required = schema["required"].as_array().cloned().unwrap_or_default();
            for field in required.iter().filter_map(serde_json::Value::as_str) {
                assert!(
                    example.get(field).is_some(),
                    "tool {:?}: example {example} lacks required field {field:?}",
                    tool.name
                );
            }
        }
        assert!(
            minimal_example("start")["file"].is_string(),
            "start's example names a graph file"
        );
        assert!(minimal_example("no-such-tool").is_null());
    }

    /// The signal example is checked against the SCHEMA FILE, not against a list typed here: the
    /// example's first version was missing two required fields and nothing in this module could
    /// know it.
    #[test]
    fn the_signal_example_carries_every_field_the_signal_schema_requires() {
        let schema: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../../schemas/graph-signal.schema.json"
        ))
        .expect("the signal schema parses");
        let example = minimal_example("signal");
        let signal = &example["signal"];
        for field in schema["required"]
            .as_array()
            .expect("required")
            .iter()
            .filter_map(serde_json::Value::as_str)
        {
            assert!(
                signal.get(field).is_some(),
                "signal example lacks required {field:?}: {signal}"
            );
        }
        let source_required = schema["properties"]["source"]["required"]
            .as_array()
            .expect("source.required");
        for field in source_required.iter().filter_map(serde_json::Value::as_str) {
            assert!(
                signal["source"].get(field).is_some(),
                "signal.source lacks {field:?}"
            );
        }
        let severities = schema["properties"]["severity"]["enum"]
            .as_array()
            .expect("severity enum");
        assert!(
            severities.contains(&signal["severity"]),
            "severity is one of the schema's values"
        );
    }

    /// Presence is not usability: every placeholder must carry the TYPE its schema declares, and a
    /// nested object schema must yield the nested example. Review of #1199 replaced the generic
    /// placeholder with `{}` and the presence-only guard stayed green; this cell reddens on that
    /// sabotage, and on `"document": {}`.
    #[test]
    fn every_placeholder_has_the_declared_type_and_nested_objects_are_derived() {
        fn check(tool: &str, schema: &serde_json::Value, example: &serde_json::Value) {
            let required = schema["required"].as_array().cloned().unwrap_or_default();
            for field in required.iter().filter_map(serde_json::Value::as_str) {
                let property = &schema["properties"][field];
                let declared = property["type"].as_str().unwrap_or("string");
                let value = &example[field];
                let matches = match declared {
                    "string" => value.is_string(),
                    "integer" | "number" => value.is_number(),
                    "boolean" => value.is_boolean(),
                    "array" => value.is_array(),
                    "object" => value.is_object(),
                    other => {
                        panic!("tool {tool:?} field {field:?}: unknown declared type {other:?}")
                    }
                };
                assert!(
                    matches,
                    "tool {tool:?} field {field:?}: declared {declared:?}, example carries {value}"
                );
                if declared == "object" {
                    assert!(
                        !value.as_object().unwrap().is_empty(),
                        "tool {tool:?} field {field:?}: a declared object must never derive to {{}}"
                    );
                    if property.get("properties").is_some() {
                        assert!(
                            value.get("<not derivable>").is_none(),
                            "tool {tool:?} field {field:?}: a schema with properties derives them"
                        );
                        check(tool, property, value);
                    }
                }
            }
        }
        for tool in TOOLS.iter() {
            let schema = (tool.schema)();
            let example = minimal_example(tool.name);
            check(tool.name, &schema, &example);
        }
        let document = &minimal_example("document_read")["document"];
        assert!(
            document["evidenceId"].is_string() && document["index"].is_number(),
            "document is {{evidenceId, index}}: {document}"
        );
    }

    /// `wake_wait` returns from `call()` before the request table; its refusal must still carry the
    /// example. The refusal fires before any request is built, so a client pointed at a closed port
    /// exercises the real wiring with no network.
    #[test]
    fn wake_waits_refusal_goes_through_the_example_wrapper() {
        let api = ApiClient::new(
            "http://127.0.0.1:1".to_owned(),
            Zeroizing::new("tok".to_owned()),
            "a".into(),
            "agent".into(),
            None,
            None,
            "0123456789abcdef".to_owned(),
        );
        match call(
            &api,
            "0123456789abcdef",
            &serde_json::json!(1),
            "wake_wait",
            &serde_json::json!({}),
        ) {
            HandlerOutcome::Error { code, message } => {
                assert_eq!(code, INVALID_PARAMS);
                assert!(
                    message.starts_with("wake_wait needs executionId"),
                    "{message}"
                );
                assert!(
                    message.contains("minimal valid call for \"wake_wait\""),
                    "{message}"
                );
                assert!(message.contains("\"executionId\""), "{message}");
            }
            HandlerOutcome::Result(value) => panic!("a refusal stays a refusal: {value}"),
        }
    }

    /// The refusal keeps its code and its reason; the example is appended, never substituted.
    #[test]
    fn a_missing_argument_refusal_keeps_its_code_and_gains_the_example() {
        let refused = with_example(
            "evidence",
            HandlerOutcome::Error {
                code: INVALID_PARAMS,
                message: "the tool requires a string \"evidenceId\" argument".to_owned(),
            },
        );
        match refused {
            HandlerOutcome::Error { code, message } => {
                assert_eq!(code, INVALID_PARAMS);
                assert!(message.starts_with("the tool requires a string \"evidenceId\" argument"));
                assert!(message.contains("minimal valid call for \"evidence\""));
                assert!(message.contains("\"evidenceId\""));
            }
            HandlerOutcome::Result(_) => panic!("a refusal stays a refusal"),
        }
    }

    /// A Runtime shape refusal keeps its envelope byte-for-byte as the first block and gains a
    /// second block; a state refusal (a real diagnostic about the execution) gains nothing.
    #[test]
    fn a_shape_refusal_from_the_runtime_gains_a_second_block_and_a_state_refusal_does_not() {
        let envelope = serde_json::json!({"command":"execution.signal","data":null,"diagnostics":[{"code":crate::error_codes::GHCLI003_SIGNAL_INVALID,"message":"the signal envelope failed schema validation","path":"/signal","severity":"error","source":"execution-cli"}],"ok":false});
        let result = serde_json::json!({"content": [{"type": "text", "text": envelope.to_string()}], "isError": true});
        match with_example("signal", HandlerOutcome::Result(result)) {
            HandlerOutcome::Result(value) => {
                let content = value["content"].as_array().expect("content");
                assert_eq!(content.len(), 2);
                assert_eq!(
                    content[0]["text"].as_str(),
                    Some(envelope.to_string().as_str()),
                    "the envelope is untouched"
                );
                assert!(
                    content[1]["text"]
                        .as_str()
                        .unwrap()
                        .contains("finding.root_cause")
                );
                assert_eq!(
                    value["isError"],
                    serde_json::Value::Bool(true),
                    "still an error"
                );
            }
            HandlerOutcome::Error { .. } => panic!("a result stays a result"),
        }
        let state = serde_json::json!({"command":"execution.start","data":null,"diagnostics":[{"code":crate::error_codes::GHCLI005_EXECUTION_STATE,"message":"an execution has already started on this stream","path":"/execution","severity":"error","source":"execution-cli"}],"ok":false});
        let result = serde_json::json!({"content": [{"type": "text", "text": state.to_string()}], "isError": true});
        match with_example("start", HandlerOutcome::Result(result)) {
            HandlerOutcome::Result(value) => assert_eq!(
                value["content"].as_array().unwrap().len(),
                1,
                "no example beside a state diagnostic"
            ),
            HandlerOutcome::Error { .. } => panic!(),
        }
    }

    /// The bounded observer after a start timeout: an execution the Runtime holds is reported as
    /// started (not an error, with the warning not to retry); an absent one keeps the transport
    /// error and names the two ways out.
    #[test]
    fn a_timed_out_start_reports_the_observed_state_not_a_guess() {
        let held = serde_json::json!({"command":"execution.status","data":{"executionId":"exec-1","status":"running"},"diagnostics":[],"ok":true});
        match start_after_timeout("request timed out", Some((200, held))) {
            HandlerOutcome::Result(value) => {
                assert_eq!(value["isError"], serde_json::Value::Bool(false));
                let text = value["content"][0]["text"].as_str().unwrap();
                assert!(text.contains("Do NOT call start again"));
                assert!(text.contains("\"status\":\"running\""));
            }
            HandlerOutcome::Error { .. } => panic!(),
        }
        match start_after_timeout("request timed out", None) {
            HandlerOutcome::Result(value) => {
                assert_eq!(value["isError"], serde_json::Value::Bool(true));
                let text = value["content"][0]["text"].as_str().unwrap();
                assert!(text.contains("request timed out") && text.contains("\"held\": true"));
            }
            HandlerOutcome::Error { .. } => panic!(),
        }
        // The third input - reachable, but the Runtime does not hold the execution. Without the
        // `ok:true` guard this read as success with "do not retry"; the cell that was missing.
        let refused = serde_json::json!({"command":"execution.status","data":null,"diagnostics":[{"code":crate::error_codes::GHCLI005_EXECUTION_STATE,"message":"no execution on this stream","path":"/execution","severity":"error","source":"execution-cli"}],"ok":false});
        match start_after_timeout("request timed out", Some((200, refused))) {
            HandlerOutcome::Result(value) => {
                assert_eq!(
                    value["isError"],
                    serde_json::Value::Bool(true),
                    "not a success"
                );
                let text = value["content"][0]["text"].as_str().unwrap();
                // A narrow guard on the ERROR arm's own text, which `isError` cannot see: an error
                // reply must never advise against retrying. Semantic (case-folded, any phrasing
                // the arms use), not a byte-match on the sibling arm's sentence.
                assert!(
                    !advises_against_retry(text),
                    "an error reply must not advise against retrying: {text}"
                );
                assert!(
                    text.contains("was not recorded")
                        && text.contains("no execution on this stream"),
                    "{text}"
                );
            }
            HandlerOutcome::Error { .. } => panic!(),
        }
        // The fourth input, the one review of bde1867c found the arm above was wrong about: the
        // GET failed for a reason that says nothing about the execution (a 401 on a rotated
        // token). The start may well have landed. Neither "retry" nor "do not retry" may be said.
        let unrelated = serde_json::json!({"error": "unauthorized"});
        match start_after_timeout("request timed out", Some((401, unrelated))) {
            HandlerOutcome::Result(value) => {
                let text = value["content"][0]["text"].as_str().unwrap();
                assert!(
                    !text.contains("was not recorded"),
                    "no categorical denial on a 401: {text}"
                );
                assert!(
                    !advises_against_retry(text),
                    "no do-not-retry on a 401: {text}"
                );
                assert!(
                    text.contains("HTTP 401") && text.contains("Do not assume either way"),
                    "{text}"
                );
                assert_eq!(value["isError"], serde_json::Value::Bool(true));
            }
            HandlerOutcome::Error { .. } => panic!(),
        }
        // And an ok:false envelope the Runtime answered (200) WITHOUT the execution-state code is
        // not a denial either: the categorical arm needs the code, not the colour.
        let other = serde_json::json!({"command":"execution.status","data":null,"diagnostics":[{"code":crate::error_codes::GHCLI001_ARGUMENT_INVALID,"message":"bad id","path":"/executionId","severity":"error","source":"execution-cli"}],"ok":false});
        match start_after_timeout("request timed out", Some((200, other))) {
            HandlerOutcome::Result(value) => {
                let text = value["content"][0]["text"].as_str().unwrap();
                assert!(
                    !text.contains("was not recorded") && text.contains("Do not assume either way"),
                    "{text}"
                );
            }
            HandlerOutcome::Error { .. } => panic!(),
        }
    }
}
