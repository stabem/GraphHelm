//! The chat surface's MCP server (Milestone 05e): a stateless stdio JSON-RPC layer whose tools
//! map 1:1 onto Public Runtime API requests. `rpc` is the wire layer (Task 1), `session` the
//! lifecycle and method table (Task 2), `client` the loopback-only API half (Task 3); the
//! closed tool list arrives with Task 5.

pub(crate) mod client;
pub(crate) mod rpc;
pub(crate) mod session;
pub(crate) mod tools;
pub(crate) mod url;

use graphhelm_protocols::Diagnostic;
use zeroize::Zeroizing;

use crate::args::McpArgs;
use crate::output::Outcome;

const COMMAND: &str = "mcp";

/// CLI-level argument/config failures of `graphhelm mcp` only — protocol-level errors are
/// JSON-RPC error objects, never GHCLI envelopes (the plan's registry note; 015 confirmed
/// free at Task 0, 05d took 016).
const MCP_INVALID: &str = crate::error_codes::GHCLI015_MCP_INVALID;

fn refuse(message: &str, pointer: &str) -> Outcome {
    Outcome::domain(
        COMMAND,
        vec![Diagnostic::error(MCP_INVALID, message, pointer, COMMAND)],
    )
}

/// Parses and validates the config, fail-closed and before any protocol byte: loopback-only
/// URL (userinfo-stripping rule), token from `--token-file` (the flag wins) or
/// `GRAPHHELM_API_TOKEN` — never argv — with both-absent a refusal naming the two options,
/// and the actor admitted by the same wire rules the serve layer applies.
fn build_client(args: &McpArgs, session: String) -> Result<client::ApiClient, Outcome> {
    if !client::is_loopback_url(&args.url) {
        return Err(refuse(
            "--url must name a loopback authority (http://127.0.0.1:PORT, localhost, or \
             [::1]); the MCP server never talks to a remote API",
            "/url",
        ));
    }
    if let Err(reason) = url::validate_base(&args.url) {
        return Err(refuse(&reason, "/url"));
    }
    let token = match &args.token_file {
        Some(path) => std::fs::read_to_string(path)
            .map_err(|_| refuse("--token-file does not name a readable file", "/tokenFile"))?,
        None => std::env::var("GRAPHHELM_API_TOKEN").map_err(|_| {
            refuse(
                "no token: supply --token-file <path> or the GRAPHHELM_API_TOKEN \
                 environment variable (the token value never travels via argv)",
                "/tokenFile",
            )
        })?,
    };
    let token = Zeroizing::new(token.trim().to_owned());
    if token.is_empty() {
        return Err(refuse("the token is empty", "/tokenFile"));
    }
    let (actor, actor_from_env) = match &args.actor {
        Some(name) => (name.clone(), false),
        None => (
            std::env::var("GRAPHHELM_ACTOR").map_err(|_| {
                refuse(
                    "no actor: supply --actor <id> or the GRAPHHELM_ACTOR environment variable \
                     (one .mcp.json is shared by every session, so a literal there makes them all \
                     one actor)",
                    "/actor",
                )
            })?,
            true,
        ),
    };
    if graphhelm_protocols::ActorId::parse(actor.clone()).is_err() {
        return Err(refuse(
            if actor_from_env {
                "GRAPHHELM_ACTOR is not a wire-safe actor id"
            } else {
                "--actor is not a wire-safe actor id"
            },
            "/actor",
        ));
    }
    if !matches!(args.actor_type.as_str(), "agent" | "owner") {
        return Err(refuse(
            "--actor-type must be \"agent\" or \"owner\"",
            "/actorType",
        ));
    }
    if let Some(effort) = args.effort.as_deref()
        && !matches!(effort, "low" | "medium" | "high")
    {
        return Err(refuse(
            "--effort must be \"low\", \"medium\" or \"high\"",
            "/effort",
        ));
    }
    if args.effort.is_some() && args.model.is_none() {
        return Err(refuse(
            "--effort requires --model: an effort with no model names nothing",
            "/model",
        ));
    }
    if args.capability_token_file.is_some() && args.package.is_none() {
        return Err(refuse(
            "--capability-token-file requires --package: the digest every presented tool call \
             is checked fresh against (#213)",
            "/package",
        ));
    }
    if args.capability_token_file.is_some() && args.capability_audit_log.is_none() {
        return Err(refuse(
            "--capability-token-file requires --capability-audit-log: every decision this \
             gate makes is recorded, allowed or refused (#213)",
            "/capabilityAuditLog",
        ));
    }
    Ok(client::ApiClient::new(
        args.url.clone(),
        token,
        actor,
        args.actor_type.clone(),
        args.model.clone(),
        args.effort.clone(),
        session,
    ))
}

/// #213: reads the presented capability token from `--capability-token-file`, refusing before
/// any protocol byte on a missing/unreadable/unparseable file -- the same fail-closed shape as
/// the bearer token read above, never a silent "no capability" fallback.
fn read_capability_token(
    path: &std::path::Path,
) -> Result<graphhelm_tool_broker::mcp_capability::McpCapabilityToken, Outcome> {
    let bytes = std::fs::read(path).map_err(|_| {
        refuse(
            "--capability-token-file does not name a readable file",
            "/capabilityTokenFile",
        )
    })?;
    serde_json::from_slice(&bytes).map_err(|_| {
        refuse(
            "--capability-token-file does not hold a valid capability token",
            "/capabilityTokenFile",
        )
    })
}

pub fn run(args: &McpArgs) -> Outcome {
    // Minted BEFORE the client, because #1057 makes it part of the client's own attribution: this
    // one nonce is the process's session identity everywhere -- the idempotency keys, the wake
    // leases, and now `X-GraphHelm-Actor-Session`. One value rather than three means a reader of
    // the journal can join a declaration to the leases and keys the same session took out.
    let mut nonce_bytes = [0_u8; 8];
    if getrandom::fill(&mut nonce_bytes).is_err() {
        return Outcome::internal(COMMAND, "the OS random source is unavailable");
    }
    let nonce = nonce_bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();

    let api = match build_client(args, nonce.clone()) {
        Ok(api) => api,
        Err(outcome) => return outcome,
    };

    let mut state = session::SessionState::new(nonce).with_client(api);
    if let Some(token_path) = &args.capability_token_file {
        let token = match read_capability_token(token_path) {
            Ok(token) => token,
            Err(outcome) => return outcome,
        };
        // build_client already refused capability_token_file without package/audit_log.
        state = state.with_capability(session::CapabilityConfig {
            token,
            package_root: args.package.clone().expect("checked by build_client"),
            audit_log: args
                .capability_audit_log
                .clone()
                .expect("checked by build_client"),
        });
    }
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    match rpc::run(stdin.lock(), &mut stdout, session::handle, &mut state) {
        Ok(()) => Outcome::success(COMMAND, serde_json::json!({})),
        Err(error) => Outcome::internal(COMMAND, format!("the stdio transport failed: {error}")),
    }
}
