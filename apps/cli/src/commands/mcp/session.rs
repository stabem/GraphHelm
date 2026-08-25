//! The MCP lifecycle over the rpc layer: `initialize` → `notifications/initialized` →
//! tools. The session handler owns the method table; the rpc layer below owns framing and
//! the JSON-RPC envelope (ADR-026).

use super::rpc::{HandlerOutcome, INVALID_REQUEST, METHOD_NOT_FOUND};

/// The pinned MCP revision this server implements — the handshake-based lifecycle
/// (`initialize`/`notifications/initialized`). Verified against modelcontextprotocol.io at
/// implementation time (2026-08-16): the spec's current revision is 2026-07-28, a
/// meta-versioned model without the handshake; handshake revisions (2025-11-25 and earlier)
/// remain interoperable per its backward-compatibility section, and this one is the
/// revision the hosts this milestone targets both accept. Moving off the handshake model is
/// a named candidate for ADR-026's revisit trigger.
pub(crate) const SUPPORTED_PROTOCOL_VERSION: &str = "2025-06-18";

pub(crate) struct SessionState {
    /// True only after `notifications/initialized` — the lifecycle gate for tools.
    pub(crate) initialized: bool,
    /// 8 bytes of OS randomness, hex — the session's one impure input; Task 3's
    /// idempotency derivation consumes it per rpc id.
    #[allow(dead_code)]
    pub(crate) nonce: String,
    /// The loopback-only API client (Task 3). The tool table (Task 5) drives it.
    #[allow(dead_code)]
    pub(crate) client: Option<super::client::ApiClient>,
    /// #213: the presented per-contribution capability, the package it was minted against, and
    /// where every decision this gate makes is recorded. All three present or all three absent
    /// — `mod.rs::build_client` refuses the mixed cases before the session ever starts.
    /// `package_root` is re-validated fresh on every `tools/call`, never cached, so a package
    /// edited mid-session immediately stales a token minted before the edit (blueprint T2).
    pub(crate) capability: Option<CapabilityConfig>,
}

pub(crate) struct CapabilityConfig {
    pub(crate) token: graphhelm_tool_broker::mcp_capability::McpCapabilityToken,
    pub(crate) package_root: std::path::PathBuf,
    pub(crate) audit_log: std::path::PathBuf,
}

impl SessionState {
    pub(crate) fn new(nonce: String) -> Self {
        Self {
            initialized: false,
            nonce,
            client: None,
            capability: None,
        }
    }

    /// Attaches the Task 3 API client. Consumed by the tool table (Task 5); until then it
    /// only exists — construction and refusals are still exercised end to end.
    pub(crate) fn with_client(mut self, client: super::client::ApiClient) -> Self {
        self.client = Some(client);
        self
    }

    /// Attaches the #213 capability config, opt-in by presence.
    pub(crate) fn with_capability(mut self, capability: CapabilityConfig) -> Self {
        self.capability = Some(capability);
        self
    }
}

/// #213: re-validates the package FRESH (never a cached digest — blueprint T2) and runs the
/// pure pipeline. A package that no longer validates at all refuses closed under the SAME code
/// as a digest mismatch: there is no trustworthy fresh digest to compare, so nothing proceeds,
/// and the audit trail records one meaningful reason rather than a second ad hoc failure shape.
fn check_capability(
    token: &graphhelm_tool_broker::mcp_capability::McpCapabilityToken,
    package_root: &std::path::Path,
    tool_name: &str,
    actor: &str,
) -> Result<(), graphhelm_tool_broker::mcp_capability::McpCapabilityRefusal> {
    let package_digest = graphhelm_schema::validate_extension_package(package_root)
        .map(|validated| validated.package_digest)
        .unwrap_or_default(); // never equals a real "sha256:..." token digest -> StaleDigest
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(u64::MAX);
    graphhelm_tool_broker::mcp_capability::authorize_mcp_call(
        token,
        tool_name,
        actor,
        &package_digest,
        now,
    )
}

/// Appends one redacted decision line. Fails CLOSED: if the record cannot be written, the
/// call is refused rather than allowed to proceed unaudited (an unaudited allow is functionally
/// an ungoverned one, the same gap #213 exists to close).
fn record_and_append(
    audit_log: &std::path::Path,
    actor: &str,
    contribution_id: &str,
    package_digest: &str,
    tool_name: &str,
    decision: &Result<(), graphhelm_tool_broker::mcp_capability::McpCapabilityRefusal>,
) -> Result<(), String> {
    use std::io::Write as _;
    let record = graphhelm_tool_broker::mcp_capability::record_call(
        actor,
        contribution_id,
        package_digest,
        tool_name,
        decision,
    );
    let line = serde_json::to_string(&record)
        .map_err(|_| "the audit record could not be serialized".to_owned())?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(audit_log)
        .map_err(|_| "the capability audit log could not be opened".to_owned())?;
    writeln!(file, "{line}").map_err(|_| "the capability audit log could not be written".to_owned())
}

/// The method table. `ping` and `initialize` are answerable at any time; everything else
/// waits for the lifecycle (`initialize` + `notifications/initialized`).
pub(crate) fn handle(
    method: &str,
    _params: &serde_json::Value,
    rpc_id: &serde_json::Value,
    state: &mut SessionState,
) -> HandlerOutcome {
    match method {
        "initialize" => HandlerOutcome::Result(serde_json::json!({
            // The spec's negotiation rule: answer with the version we support, whatever
            // the client asked for — the client decides whether to continue.
            "protocolVersion": SUPPORTED_PROTOCOL_VERSION,
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "graphhelm", "version": env!("CARGO_PKG_VERSION")},
        })),
        "notifications/initialized" => {
            state.initialized = true;
            HandlerOutcome::Result(serde_json::Value::Null) // a notification; never written
        }
        "ping" => HandlerOutcome::Result(serde_json::json!({})),
        "tools/list" | "tools/call" if !state.initialized => HandlerOutcome::Error {
            code: INVALID_REQUEST,
            message: "the server is not initialized: send initialize and \
                      notifications/initialized first"
                .to_owned(),
        },
        "tools/list" => HandlerOutcome::Result(super::tools::tool_list()),
        // A tools/call without an id (notification form) is never executed: a mutation with
        // no response channel cannot participate in the retry choreography (its key would be
        // unretryable), so the server refuses to run it at all — and per the notification
        // rule no reply is written either (the rpc layer discards this outcome's envelope).
        "tools/call" if rpc_id.is_null() => HandlerOutcome::Error {
            code: INVALID_REQUEST,
            message: "tools/call requires an id".to_owned(),
        },
        "tools/call" => {
            let Some(name) = _params.get("name").and_then(serde_json::Value::as_str) else {
                return HandlerOutcome::Error {
                    code: super::rpc::INVALID_PARAMS,
                    message: "tools/call requires a string \"name\"".to_owned(),
                };
            };
            let arguments = _params
                .get("arguments")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            let Some(client) = state.client.as_ref() else {
                return HandlerOutcome::Error {
                    code: super::rpc::INVALID_REQUEST,
                    message: "the server has no API client configured".to_owned(),
                };
            };
            if let Some(capability) = state.capability.as_ref() {
                let decision = check_capability(
                    &capability.token,
                    &capability.package_root,
                    name,
                    &client.actor,
                );
                if let Err(write_error) = record_and_append(
                    &capability.audit_log,
                    &client.actor,
                    &capability.token.contribution_id,
                    &capability.token.package_digest,
                    name,
                    &decision,
                ) {
                    return HandlerOutcome::Error {
                        code: super::rpc::INVALID_REQUEST,
                        message: format!("capability refused: {write_error}"),
                    };
                }
                if let Err(refusal) = decision {
                    return HandlerOutcome::Error {
                        code: super::rpc::INVALID_REQUEST,
                        message: format!("capability refused: {refusal}"),
                    };
                }
            }
            super::tools::call(client, &state.nonce, rpc_id, name, &arguments)
        }
        _ => HandlerOutcome::Error {
            code: METHOD_NOT_FOUND,
            message: format!("method {method:?} is not part of this server"),
        },
    }
}
