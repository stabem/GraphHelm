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
use rpc::HandlerOutcome;

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
    let (base_url, token) = if let Some(routed) = &args.routed_url {
        // #1523: a child answering one request for a running router, which hands on the endpoint
        // an earlier child of the session discovered (health-proven then), so no child repeats
        // the probes. The token is read from its file here, never carried in argv.
        if !client::is_loopback_url(routed) || url::validate_base(routed).is_err() {
            return Err(refuse(
                "--routed-url must name a loopback Runtime",
                "/routedUrl",
            ));
        }
        let path = args
            .routed_token_file
            .as_ref()
            .ok_or_else(|| refuse("--routed-url needs --routed-token-file", "/routedTokenFile"))?;
        let token = crate::commands::secret_file::read_existing(path, "routed agent session token")
            .map_err(|_| {
                refuse(
                    "--routed-token-file does not name a readable token file",
                    "/routedTokenFile",
                )
            })?;
        (
            routed.clone(),
            client::TokenSource::Fixed(Zeroizing::new(token)),
        )
    } else if args.discover && args.url.is_none() {
        let mut project = match &args.project {
            Some(project) => project.clone(),
            None => std::env::current_dir().map_err(|_| {
                refuse(
                    "the MCP process working directory is unavailable; pass --project",
                    "/project",
                )
            })?,
        };
        // A linked worktree belongs to the main checkout's Runtime. Keep ordinary
        // directories (including non-Git projects) on their existing path identity.
        if let Ok(output) = graphhelm_process::command("git")
            .arg("-C")
            .arg(&project)
            .args([
                "rev-parse",
                "--path-format=absolute",
                "--git-common-dir",
                "--git-dir",
            ])
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_COMMON_DIR")
            .stdin(std::process::Stdio::null())
            .output()
            && output.status.success()
            && let Ok(paths) = std::str::from_utf8(&output.stdout)
        {
            let mut paths = paths.lines();
            if let (Some(common), Some(private)) = (paths.next(), paths.next())
                && common != private
                && let Some(main) = std::path::Path::new(common).parent()
            {
                project = main.to_path_buf();
            }
        }
        let project_id =
            crate::commands::execution::delivery::project_id(&project).map_err(|_| {
                refuse(
                    "--project must name an existing project directory",
                    "/project",
                )
            })?;
        (
            String::new(),
            client::TokenSource::discover_project(project_id),
        )
    } else {
        let base_url = args.url.as_ref().ok_or_else(|| {
            refuse(
                "supply --url for a direct connection, or omit it with --discover to use the project Runtime",
                "/url",
            )
        })?;
        if !client::is_loopback_url(base_url) {
            return Err(refuse(
                "--url must name a loopback authority (http://127.0.0.1:PORT, localhost, or \
                 [::1]); the MCP server never talks to a remote API",
                "/url",
            ));
        }
        if let Err(reason) = url::validate_base(base_url) {
            return Err(refuse(&reason, "/url"));
        }
        let token = if args.discover {
            // Legacy mode remains for older user registrations pinned to one port.
            let port = url::port_of(base_url).ok_or_else(|| {
                refuse(
                    "legacy port discovery needs an explicit --url; omit --url to discover by project",
                    "/url",
                )
            })?;
            client::TokenSource::discover(port)
        } else {
            let token = match &args.token_file {
                Some(path) => std::fs::read_to_string(path).map_err(|_| {
                    refuse("--token-file does not name a readable file", "/tokenFile")
                })?,
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
            client::TokenSource::Fixed(token)
        };
        (base_url.clone(), token)
    };
    // #389: the environment wins. One registration (`--actor agent-chat`, written by `init` and
    // the host adoption) is shared by every session, so the literal must not override the name a
    // lane exports for itself; it stays the fallback for every session that sets nothing.
    let from_env = std::env::var("GRAPHHELM_ACTOR")
        .ok()
        .filter(|name| !name.is_empty());
    let (actor, actor_from_env) = match (from_env, &args.actor) {
        (Some(name), _) => (name, true),
        (None, Some(name)) => (name.clone(), false),
        (None, None) => {
            return Err(refuse(
                "no actor: supply --actor <id> or the GRAPHHELM_ACTOR environment variable \
                 (one .mcp.json is shared by every session, so a literal there makes them all \
                 one actor)",
                "/actor",
            ));
        }
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
    // #380: discovery only ever yields the agent session token, so an owner session must name
    // the owner token itself; the declared type alone never selects the owner credential.
    if args.discover && args.actor_type == "owner" {
        return Err(refuse(
            "an owner session is not discovered: pass --url with --token-file naming the owner \
             token (discovery hands out only the agent session token)",
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
        base_url,
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
    if args.one_request {
        return answer_one_request(args);
    }
    // Minted BEFORE the client, because #1057 makes it part of the client's own attribution: this
    // one nonce is the process's session identity everywhere -- the idempotency keys, the wake
    // leases, and now `X-GraphHelm-Actor-Session`. One value rather than three means a reader of
    // the journal can join a declaration to the leases and keys the same session took out. #1523:
    // every child this server starts is handed the same nonce, so that stays one identity.
    let mut nonce_bytes = [0_u8; 8];
    if getrandom::fill(&mut nonce_bytes).is_err() {
        return Outcome::internal(COMMAND, "the OS random source is unavailable");
    }
    let nonce = hex::encode(nonce_bytes);

    // The whole config is still refused here, before any protocol byte, exactly as the child
    // that answers each tool request will check it again.
    let state = match session_state(args, nonce) {
        Ok(state) => state,
        Err(outcome) => return outcome,
    };
    let (launch, argv) = launch_path();
    let mut router = Router {
        state,
        launch,
        argv,
        routed: None,
    };
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    match rpc::run(stdin.lock(), &mut stdout, route, &mut router) {
        Ok(()) => Outcome::success(COMMAND, serde_json::json!({})),
        Err(error) => Outcome::internal(COMMAND, format!("the stdio transport failed: {error}")),
    }
}

/// The session a process serves: the API client and, when presented, the #213 capability.
fn session_state(args: &McpArgs, nonce: String) -> Result<session::SessionState, Outcome> {
    let api = build_client(args, nonce.clone())?;
    let mut state = session::SessionState::new(nonce).with_client(api);
    if let Some(token_path) = &args.capability_token_file {
        let token = read_capability_token(token_path)?;
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
    Ok(state)
}

/// #1523: a running `graphhelm mcp` keeps the stdio session (`initialize`, `ping`, the lifecycle
/// gate) and answers every `tools/list` and `tools/call` by starting the binary that is at its
/// launch path NOW, so a session that outlives `graphhelm update` gets the new tools and the new
/// wire on its next call, with no restart and no second `initialize`.
struct Router {
    state: session::SessionState,
    launch: std::ffi::OsString,
    argv: Vec<std::ffi::OsString>,
    /// The endpoint (URL and agent token FILE) a child discovered, handed to the next child so
    /// discovery and its health probes run once per session, again only after the endpoint
    /// failed a child.
    routed: Option<(String, std::path::PathBuf)>,
}

/// The longest one tool request may take before its child is stopped: the gateway probe's own
/// wait, one request, the one retry after a refused token, and the project discovery budget.
/// Not for `wake_wait`: see [`child_wait`].
const CHILD_WAIT: std::time::Duration = tools::PROBE_CALLER_WAIT
    .saturating_add(client::REQUEST_TIMEOUT)
    .saturating_add(client::REQUEST_TIMEOUT)
    .saturating_add(std::time::Duration::from_secs(15));

/// How long the router waits for the child answering `request`. `wake_wait` blocks in the child
/// until its own lease rings or matures, and a lease may mature years out (`maturesInSeconds`), so
/// any router bound would cut a legitimate wait short and answer it as an error. The child bounds
/// that wait itself: it refuses a lease with no bound, and ends at the lease's deadline. Every
/// other tool places at most one request and its retry, which [`CHILD_WAIT`] covers.
fn child_wait(request: &serde_json::Value) -> Option<std::time::Duration> {
    let waits_on_its_lease =
        request["method"] == "tools/call" && request["params"]["name"] == "wake_wait";
    (!waits_on_its_lease).then_some(CHILD_WAIT)
}

/// Where this process was started from, resolved once at start to an absolute path: `argv[0]`
/// as the host gave it (a bare name is looked up on `PATH`, a relative path against the working
/// directory). Not `current_exe`, which names the build that is running even after an update has
/// put a new one at the path the host launches. The rest of argv is what each child repeats.
fn launch_path() -> (std::ffi::OsString, Vec<std::ffi::OsString>) {
    let mut argv = std::env::args_os();
    let first = argv.next().map(std::path::PathBuf::from);
    let rest = argv.collect();
    let resolved = first.and_then(|path| {
        if path.is_absolute() {
            return Some(path);
        }
        if path.components().count() > 1 {
            return std::env::current_dir().ok().map(|dir| dir.join(path));
        }
        let names = if cfg!(windows) && path.extension().is_none() {
            vec![path.with_extension("exe"), path.clone()]
        } else {
            vec![path.clone()]
        };
        std::env::split_paths(&std::env::var_os("PATH")?)
            .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
            .find(|candidate| candidate.is_file())
    });
    let launch = resolved
        .or_else(|| std::env::current_exe().ok())
        .map_or_else(|| std::ffi::OsString::from("graphhelm"), Into::into);
    (launch, rest)
}

fn route(
    method: &str,
    params: &serde_json::Value,
    rpc_id: &serde_json::Value,
    router: &mut Router,
) -> HandlerOutcome {
    // Only a tool request that has an answer to give leaves this process: the lifecycle, `ping`,
    // the not-initialized refusal and every notification are this server's own.
    if !matches!(method, "tools/list" | "tools/call")
        || !router.state.initialized
        || rpc_id.is_null()
    {
        return session::handle(method, params, rpc_id, &mut router.state);
    }
    let mut request = serde_json::json!({"jsonrpc": "2.0", "id": rpc_id, "method": method});
    if !params.is_null() {
        request["params"] = params.clone();
    }
    let routed = router.routed.take();
    let answer = match one_request(router, &request, routed.as_ref()) {
        // A token the Runtime refused did nothing, so asking once more, discovering afresh, is
        // safe: the Runtime may have been replaced since the endpoint was handed on.
        Ok(answer)
            if routed.is_some() && answer.exit == client::EndpointSeen::UNAUTHORIZED_EXIT =>
        {
            one_request(router, &request, None)
        }
        other => other,
    };
    match answer {
        Ok(answer) => {
            // An endpoint that failed is never handed on; one a child discovered replaces it.
            router.routed = answer
                .discovered
                .or_else(|| routed.filter(|_| answer.exit == 0));
            outcome_of(answer.reply)
        }
        Err(message) => {
            router.routed = None;
            HandlerOutcome::Error {
                code: INTERNAL_ERROR,
                message,
                data: None,
            }
        }
    }
}

/// JSON-RPC's internal error: the server could not produce the answer at all.
const INTERNAL_ERROR: i64 = -32603;

struct Answer {
    reply: Option<serde_json::Value>,
    exit: i32,
    discovered: Option<(String, std::path::PathBuf)>,
}

/// Starts the installed binary hidden for one request line and reads its answer, bounded by
/// [`child_wait`]. The child's stderr is this server's, so its warnings reach the host as before.
fn one_request(
    router: &Router,
    request: &serde_json::Value,
    routed: Option<&(String, std::path::PathBuf)>,
) -> Result<Answer, String> {
    use std::io::{Read as _, Write as _};
    let mut command = graphhelm_process::command(&router.launch);
    command
        .args(&router.argv)
        .arg("--one-request")
        .arg("--session-nonce")
        .arg(&router.state.nonce);
    if let Some((url, token_file)) = routed {
        command
            .arg("--routed-url")
            .arg(url)
            .arg("--routed-token-file")
            .arg(token_file);
    }
    let mut child = command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .map_err(|_| {
            "the graphhelm installed at this server's launch path could not be started; \
             reinstall it (`graphhelm update`)"
                .to_owned()
        })?;
    let stdout = child.stdout.take().expect("piped stdout");
    let (sender, received) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let limit = u64::try_from(rpc::MAX_LINE_BYTES)
            .unwrap_or(u64::MAX)
            .saturating_mul(2);
        let _ = stdout.take(limit).read_to_end(&mut bytes);
        let _ = sender.send(bytes);
    });
    if let Some(mut stdin) = child.stdin.take() {
        // A child that exits before reading is answered below by what it wrote, or by nothing.
        let _ = writeln!(stdin, "{request}");
    }
    let bound = child_wait(request);
    let read = match bound {
        Some(bound) => received.recv_timeout(bound).ok(),
        None => received.recv().ok(),
    };
    let Some(bytes) = read else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!(
            "the tool request did not finish within {} s and was stopped",
            bound.unwrap_or_default().as_secs()
        ));
    };
    let exit = child
        .wait()
        .ok()
        .and_then(|status| status.code())
        .unwrap_or(1);
    let mut reply = None;
    let mut discovered = None;
    for line in String::from_utf8_lossy(&bytes).lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if value.get("jsonrpc").is_some() {
            reply.get_or_insert(value);
        } else if let (Some(url), Some(token_file)) = (
            value["graphhelmRouted"]["url"].as_str(),
            value["graphhelmRouted"]["tokenFile"].as_str(),
        ) {
            discovered = Some((url.to_owned(), std::path::PathBuf::from(token_file)));
        }
    }
    Ok(Answer {
        reply,
        exit,
        discovered,
    })
}

fn outcome_of(reply: Option<serde_json::Value>) -> HandlerOutcome {
    let Some(mut reply) = reply else {
        return HandlerOutcome::Error {
            code: INTERNAL_ERROR,
            message: "the graphhelm installed at this server's launch path gave no answer"
                .to_owned(),
            data: None,
        };
    };
    if let Some(result) = reply.get_mut("result") {
        return HandlerOutcome::Result(result.take());
    }
    let error = &mut reply["error"];
    HandlerOutcome::Error {
        code: error["code"].as_i64().unwrap_or(INTERNAL_ERROR),
        message: error["message"]
            .as_str()
            .unwrap_or("the tool request failed")
            .to_owned(),
        data: error.get_mut("data").map(serde_json::Value::take),
    }
}

/// #1523: the child side. The router already answered `initialize` and forwards only once the
/// session is initialized, so the one request line is served as an initialized session would.
/// Writes the reply, then the endpoint it discovered (a line with no `jsonrpc` member, read only
/// by the router), and exits: 0, or with a routed endpoint the worst it saw of that endpoint.
fn answer_one_request(args: &McpArgs) -> Outcome {
    use std::io::Write as _;
    let Some(nonce) = args
        .session_nonce
        .clone()
        .filter(|nonce| nonce.len() == 16 && nonce.bytes().all(|byte| byte.is_ascii_hexdigit()))
    else {
        return refuse(
            "--one-request needs the router's --session-nonce",
            "/sessionNonce",
        );
    };
    let mut state = match session_state(args, nonce) {
        Ok(state) => state,
        Err(outcome) => return outcome,
    };
    state.initialized = true;
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let served = rpc::run(stdin.lock(), &mut stdout, session::handle, &mut state);
    let client = state
        .client
        .as_ref()
        .expect("session_state attaches the client");
    if let Some((url, token_file)) = client.discovered()
        && let Some(token_file) = token_file.to_str()
    {
        let _ = writeln!(
            stdout,
            "{}",
            serde_json::json!({"graphhelmRouted": {"url": url, "tokenFile": token_file}})
        );
    }
    let _ = stdout.flush();
    let code = if served.is_err() {
        1
    } else if args.routed_url.is_some() {
        client.endpoint_seen().exit_code()
    } else {
        0
    };
    std::process::exit(code)
}

#[cfg(test)]
mod tests {
    /// #1677 review: `wake_wait` may legitimately block until a lease matures far past
    /// [`super::CHILD_WAIT`]; the router must not cut it short and answer a rung or matured wait
    /// with an error. Every other tool keeps the bound.
    #[test]
    fn only_a_lease_wait_is_left_to_bound_itself() {
        let call = |name: &str| {
            serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": {"name": name, "arguments": {}}})
        };
        assert_eq!(super::child_wait(&call("wake_wait")), None);
        assert_eq!(
            super::child_wait(&call("wake_arm")),
            Some(super::CHILD_WAIT)
        );
        assert_eq!(super::child_wait(&call("probe")), Some(super::CHILD_WAIT));
        let list = serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"});
        assert_eq!(super::child_wait(&list), Some(super::CHILD_WAIT));
    }
}
