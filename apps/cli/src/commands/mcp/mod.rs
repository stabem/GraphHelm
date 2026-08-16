//! The chat surface's MCP server (Milestone 05e): a stateless stdio JSON-RPC layer whose tools
//! map 1:1 onto Public Runtime API requests. `rpc` is the wire layer (Task 1), `session` the
//! lifecycle and method table (Task 2), `client` the loopback-only API half (Task 3); the
//! closed tool list arrives with Task 5.

pub(crate) mod client;
pub(crate) mod rpc;
pub(crate) mod session;
pub(crate) mod tools;

use graphhelm_protocols::Diagnostic;
use zeroize::Zeroizing;

use crate::args::McpArgs;
use crate::output::Outcome;

const COMMAND: &str = "mcp";

/// CLI-level argument/config failures of `graphhelm mcp` only — protocol-level errors are
/// JSON-RPC error objects, never GHCLI envelopes (the plan's registry note; 015 confirmed
/// free at Task 0, 05d took 016).
const MCP_INVALID: &str = "GHCLI015_MCP_INVALID";

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
fn build_client(args: &McpArgs) -> Result<client::ApiClient, Outcome> {
    if !client::is_loopback_url(&args.url) {
        return Err(refuse(
            "--url must name a loopback authority (http://127.0.0.1:PORT, localhost, or \
             [::1]); the MCP server never talks to a remote API",
            "/url",
        ));
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
    if graphhelm_protocols::ActorId::parse(args.actor.clone()).is_err() {
        return Err(refuse("--actor is not a wire-safe actor id", "/actor"));
    }
    if !matches!(args.actor_type.as_str(), "agent" | "owner") {
        return Err(refuse(
            "--actor-type must be \"agent\" or \"owner\"",
            "/actorType",
        ));
    }
    Ok(client::ApiClient::new(
        args.url.clone(),
        token,
        args.actor.clone(),
        args.actor_type.clone(),
    ))
}

pub fn run(args: &McpArgs) -> Outcome {
    let api = match build_client(args) {
        Ok(api) => api,
        Err(outcome) => return outcome,
    };

    let mut nonce_bytes = [0_u8; 8];
    if getrandom::fill(&mut nonce_bytes).is_err() {
        return Outcome::internal(COMMAND, "the OS random source is unavailable");
    }
    let nonce = nonce_bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();

    let mut state = session::SessionState::new(nonce).with_client(api);
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    match rpc::run(stdin.lock(), &mut stdout, session::handle, &mut state) {
        Ok(()) => Outcome::success(COMMAND, serde_json::json!({})),
        Err(error) => Outcome::internal(COMMAND, format!("the stdio transport failed: {error}")),
    }
}
