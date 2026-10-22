//! A verb that reaches HTTP must reach MCP too, and every route a tool claims must exist.
//!
//! WHY THIS FILE EXISTS. The surfaces already have AGREEMENT guards — `api_http.rs` and
//! `mcp_stdio.rs` both drive the same story on two surfaces and compare the answers. Every one of
//! them proves the surfaces do not disagree **about the verbs they both have**. Nothing asserted
//! that a verb reached all of them. A verb landing on CLI and HTTP and silently missing MCP is
//! green on every existing guard, which is the shape of #162's seventh carrier one layer up: the
//! defect is not a wrong answer, it is a QUESTION NOBODY ASKS.
//!
//! THE POPULATION COMES FROM THE SURFACES, NOT FROM A LIST WRITTEN HERE. A guard whose expected
//! set is typed into the guard is a second hand-maintained table, and it drifts in exactly the
//! situation it exists to catch — the author who forgets the MCP tool forgets the guard's row in
//! the same edit. So both ends are read from the registration sites at RUN time:
//!
//! ```text
//! HTTP   apps/cli/src/commands/serve/mod.rs      the `.route(...)` chain in build_router
//! MCP    apps/cli/src/commands/mcp/tools.rs      the API call each tool NAMES in its description
//! ```
//!
//! The MCP half works only because the tool table was built to carry it: that module's own doc
//! says each description names the API call it maps to. This guard makes that documented habit
//! load-bearing — the descriptions stop being prose and become the join key.
//!
//! READING SOURCE TEXT IS A REAL LIMIT AND IT IS STATED, NOT HIDDEN. This parses the registration
//! sites; it does not interrogate a live `axum::Router` (axum exposes no route listing) nor a live
//! MCP `tools/list` (that table is `pub(crate)`, and an integration test is a separate crate). So a
//! route reached by some mechanism other than a literal `.route("...", ...)` in `build_router` is
//! invisible here. What it does catch is the whole population as it is registered today, and it
//! fails LOUDLY rather than silently if the file's shape changes: both parsers assert a non-trivial
//! count before any comparison, because an empty population makes every subset assertion pass.
//!
//! TWO PARSING TRAPS, BOTH MEASURED ON THIS FILE PAIR BEFORE THE GUARD WAS WRITTEN — the first
//! draft of the throwaway instrument hit both, and each produced a plausible false finding:
//!
//! 1. **The parameter is spelled differently on the two surfaces.** The descriptions say
//!    `{executionId}`; axum registers `{id}`. Joining the raw strings gives an EMPTY intersection,
//!    which reads as "the surfaces agree about nothing" when in truth nothing was compared.
//! 2. **One route can register several methods.** `post(routes::wake_lease).get(routes::
//!    wake_lease_status)` is one `.route` call and two verbs. Taking the first method only reported
//!    `GET /v1/executions/{id}/wake-lease` as claimed-but-unregistered — a defect that did not
//!    exist, in a file that was correct.

use std::collections::BTreeSet;

/// One registered or claimed endpoint, normalised so the two surfaces are comparable.
type Endpoint = (String, String);

fn crate_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// Read a source file at RUN time rather than `include_str!`ing it.
///
/// The same reason the schema vocabulary guard gives: a file baked into the test binary is a copy
/// from build time, and under a stale build the assertion passes while the file on disk says
/// something else.
fn source(relative: &str) -> String {
    let path = crate_root().join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

/// `{executionId}` and `{sessionId}` (how a tool description names them, after the tool's own
/// argument) and `{id}` (how axum registers both) are the same parameter. Normalise toward the
/// registration, which is the side that decides.
fn normalise(path: &str) -> String {
    path.replace("{executionId}", "{id}")
        .replace("{sessionId}", "{id}")
}

/// Every `(METHOD, path)` the router registers, including the extra methods of a chained
/// `post(..).get(..)`.
fn registered_endpoints() -> BTreeSet<Endpoint> {
    let text = source("src/commands/serve/mod.rs");
    let mut found = BTreeSet::new();

    for (index, _) in text.match_indices(".route(") {
        let rest = &text[index + ".route(".len()..];

        // The path is the first string literal in the call. Route paths carry no escapes, so a
        // plain scan to the closing quote is exact rather than approximate.
        let open = match rest.find('"') {
            Some(offset) => offset,
            None => continue,
        };
        let close = match rest[open + 1..].find('"') {
            Some(offset) => open + 1 + offset,
            None => continue,
        };
        let path = normalise(&rest[open + 1..close]);

        // The handler argument runs to the paren that closes `.route(`. Balance from the call's
        // own opening paren rather than stopping at the next `.route(`, which would bleed one
        // call's methods into the next when a route spans several lines.
        let mut depth = 1usize;
        let mut end = close + 1;
        for (offset, character) in rest[close + 1..].char_indices() {
            match character {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = close + 1 + offset;
                        break;
                    }
                }
                _ => {}
            }
        }
        let handlers = &rest[close + 1..end];

        for method in ["get", "post", "delete", "put", "patch"] {
            if handlers.contains(&format!("{method}(")) {
                found.insert((method.to_uppercase(), path.clone()));
            }
        }
    }

    found
}

/// Every `(METHOD, path)` an MCP tool description names, keyed by the tool that names it.
fn mcp_claims() -> BTreeSet<(String, String, String)> {
    let text = source("src/commands/mcp/tools.rs");
    let table_start = text
        .find("const TOOLS")
        .expect("the MCP tool table is declared as `const TOOLS`");
    let table_end = text
        .find("pub(crate) fn tool_list")
        .expect("`tool_list` follows the table and bounds it");
    let table = &text[table_start..table_end];

    let mut found = BTreeSet::new();
    for (index, _) in table.match_indices("name: \"") {
        let rest = &table[index + "name: \"".len()..];
        let name_end = rest.find('"').expect("a tool name is a closed literal");
        let name = rest[..name_end].to_owned();

        let description_at = match rest.find("description: \"") {
            Some(offset) => offset + "description: \"".len(),
            None => continue,
        };
        let description = read_rust_string(&rest[description_at..]);

        for method in ["GET", "POST", "DELETE", "PUT", "PATCH"] {
            let needle = format!("{method} /v1/");
            for (hit, _) in description.match_indices(&needle) {
                let tail = &description[hit + method.len() + 1..];
                let path: String = tail
                    .chars()
                    .take_while(|character| {
                        character.is_ascii_alphanumeric()
                            || matches!(character, '/' | '-' | '_' | '{' | '}')
                    })
                    .collect();
                found.insert((method.to_owned(), normalise(&path), name.clone()));
            }
        }
    }

    found
}

/// The contents of a Rust string literal whose opening quote has already been consumed.
///
/// Handles the two escapes these descriptions use: `\"` for an inner quote, and a trailing `\`
/// before a newline that swallows the indentation of the next line. THE SECOND ONE IS NOT A
/// FLOURISH — a matcher that cannot cross it stops at the first wrapped line, which read as "seven
/// of the fourteen tools name no route at all" on the first attempt at this measurement.
fn read_rust_string(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(character) = chars.next() {
        match character {
            '"' => break,
            '\\' => match chars.next() {
                Some('\n') => {
                    // Continuation: drop the newline AND the indentation that follows it.
                    while matches!(chars.clone().next(), Some(' ') | Some('\t')) {
                        chars.next();
                    }
                }
                Some(escaped) => out.push(escaped),
                None => break,
            },
            other => out.push(other),
        }
    }
    out
}

/// Routes that are deliberately not MCP tools, each with the reason it is exempt.
///
/// AN EXCEPTION LIST IS THE HONEST HALF OF A COMPLETENESS GUARD, and it earns its keep only if
/// adding a row is harder than adding the tool. Each entry names WHY, so a future author who wants
/// to silence this guard has to write a false sentence rather than paste a path.
const NOT_A_RUNTIME_VERB: [(&str, &str, &str); 27] = [
    (
        "GET",
        "/v1/executions/{id}/issues/{issue}/journeys",
        "the Studio's review modal reads an issue's journeys with their Before = main frames (#1795); owner credential only, like the frame routes it points at. An agent reads the same journeys from tasks and the runs from journey_preview",
    ),
    (
        "GET",
        "/v1/executions/{id}/requests",
        "the owner's requests joined with their issues and PRs, the read model of the Studio's Graph (#1792); an agent records requests with task.request_recorded / task.request_linked and reads the same records from tasks (full=1), so no separate agent verb is needed",
    ),
    (
        "GET",
        "/v1/executions/{id}/lanes/{lane}/activity",
        "one lane's last actions for the owner's Studio lanes board (#1793); owner credentials only, and agent credentials are refused, so it is not an agent verb. A lane reads its own records through tasks (lane= filter)",
    ),
    (
        "POST",
        "/v1/executions/{id}/lanes/{lane}/wake",
        "the owner's lane control (#1798): it speaks for the owner to a lane, and only the owner credential reaches it. An agent that wants another lane's attention already sends its own note through the signal tool, signed as itself",
    ),
    (
        "POST",
        "/v1/executions/{id}/lanes/{lane}/stop",
        "the owner's lane control (#1798): retiring a lane is the owner's decision, refused to every agent credential; an MCP tool would hand an agent a way to stop its peers",
    ),
    (
        "POST",
        "/v1/executions/{id}/lanes/{lane}/reassign",
        "the owner's lane control (#1798): releasing one lane's work to another is the owner's decision, refused to every agent credential; a lane releases only its own claim, through its own task record",
    ),
    (
        "GET",
        "/v1/journey-observer/setup",
        "the owner reads the last outcome of their software install after reopening the Studio (#627); like the POST this belongs to the owner's setup UI, not the agent's operational verbs",
    ),
    (
        "POST",
        "/v1/journey-observer/setup",
        "it edits the owner's package.json and downloads Chromium into the owner's machine, an act the owner confirms by a button in the Studio (#519); an agent must not install software on the owner's behalf, and the CLI's `setup --install-observer playwright` stays the agent-free door",
    ),
    (
        "GET",
        "/v1/workspaces/slots",
        "the Studio owner's view of host build queues across configured roots, not an execution verb; \
         agent credentials are refused and agents inspect their own slot through the workspace CLI",
    ),
    (
        "GET",
        "/v1/journey-flows/{id}/screens/{screen}/frame",
        "a PNG of one screen from a journey's preview, drawn as a card in the Studio canvas for the owner's eyes (#519); an MCP answer is text in a model's context, and the frame is a view, never proof. An agent reads the same per-screen results from journey_preview",
    ),
    (
        "GET",
        "/v1/journey-flows/{id}/history",
        "a journey's earlier finished runs, so the Studio can compare a screen with what it looked like before (#history); the owner reads it beside the frames. An agent reads the current run from journey_preview and its git history for what changed",
    ),
    (
        "GET",
        "/v1/journey-flows/{id}/history/{run}/screens/{screen}/frame",
        "a PNG of one screen as an earlier run kept it, drawn beside the current frame in the Studio for the owner's eyes (#history); a frame is a view, never proof",
    ),
    (
        "GET",
        "/v1/journeys/sessions/{id}/frame",
        "a JPEG of a watch's page that the Studio canvas polls a few times a second for the owner's eyes (#519); an MCP answer is text in a model's context, and the frame is a view, never proof. An agent reads the same play, step by step, from journey_sessions",
    ),
    (
        "PUT",
        "/v1/gateway/credentials/{reference}",
        "its argument IS a secret, and an MCP tool argument is written into the harness transcript before it reaches this process; the CLI's stdin and an HTTP body stay its two doors",
    ),
    (
        "GET",
        "/health",
        "a liveness probe for whatever is watching the process, not an operation on an execution",
    ),
    (
        "GET",
        "/v1/seen-actors",
        "in-memory authenticated request telemetry for the Studio liveness display; it is process-local UI evidence, not event-backed execution state or an agent operation",
    ),
    (
        "GET",
        "/monitor",
        "the human HTML monitor index; its surface is a browser, and it is GET-only by \
         construction",
    ),
    (
        "GET",
        "/monitor/{id}",
        "the human HTML monitor page, same reason as the index",
    ),
    (
        "GET",
        "/v1/executions/{id}/reply-suggestions",
        "Studio's owner-reply helper: it drafts candidate answers to an agent's pending question \
         for the HUMAN owner to pick from. Handing it to the chat surface would let the agent that \
         asked draft its own owner answer, which is self-validation, not a runtime verb",
    ),
    (
        "GET",
        "/v1/native-chats",
        "Studio lists the OWNER's own Desktop chats so the owner can pick one to hand work to; \
         it reads the human's host session list, not an execution, and an agent has no business \
         enumerating the owner's other conversations",
    ),
    (
        "GET",
        "/v1/executions/{id}/native-chats",
        "the receipts of the owner's hand-offs into Desktop chats, read by the same Studio panel \
         that sends them; it is the owner's dispatch log, same reason as the list",
    ),
    (
        "POST",
        "/v1/executions/{id}/native-chats",
        "types an instruction into one of the OWNER's existing Desktop chats on the owner's \
         behalf. On the chat surface an agent could push instructions into other sessions as if \
         the owner had written them, which is impersonation, not a runtime verb",
    ),
    (
        "POST",
        "/v1/journey-flows/{id}/edges/{edge}/safe",
        "the owner's word that a draft's destructive-looking act may be played in a watch (#518). \
         Drafts and their acts are written by agents, so on the chat surface the hand that wrote \
         the act would be offered the mark that excuses it, which is self-validation, not a \
         runtime verb; its doors are the Studio and `graphhelm journey mark-safe`",
    ),
    (
        "POST",
        "/v1/executions/{id}/journey-credential-grants",
        "the owner grants one lane a journey credential scope (#1309); a grant is authority over agents, so an agent must never issue one, and the route refuses every agent and task bearer. An MCP tool runs with an agent's credential and would only ever be refused",
    ),
    (
        "POST",
        "/v1/executions/{id}/journey-credential-grants/{grant}/revoke",
        "the owner withdraws a journey credential grant (#1309); like issuing one, it is owner authority over agents and refuses every agent and task bearer, so no agent-credentialed MCP tool may name it",
    ),
    (
        "POST",
        "/v1/executions/{id}/journey-credentials",
        "its response IS a secret: the journey credential, delivered once to the lane that asked (#1309, RFC-1309 \"Protected delivery channel\"); an MCP or WebMCP reply is written into a model transcript, so a tool naming this route would disclose the token. The lane calls it directly with its own lane credential",
    ),
    (
        "POST",
        "/v1/executions/{id}/journey-credentials/{credential}/revoke",
        "the owner withdraws one journey credential, e.g. one whose delivery was uncertain (#1309); it is owner authority over agents and refuses every agent and task bearer, so no agent-credentialed MCP tool may name it",
    ),
];

fn exempt(endpoint: &Endpoint) -> bool {
    NOT_A_RUNTIME_VERB
        .iter()
        .any(|(method, path, _)| *method == endpoint.0 && *path == endpoint.1)
}

#[test]
fn every_http_route_is_reachable_from_mcp_or_is_a_named_exception() {
    let registered = registered_endpoints();
    let claimed: BTreeSet<Endpoint> = mcp_claims()
        .into_iter()
        .map(|(method, path, _)| (method, path))
        .collect();

    // BOTH POPULATIONS FIRST. A parser that silently matched nothing makes every line below pass:
    // an empty `registered` has no unreachable member, and the guard reports success over a file
    // it could not read. These are floors, not exact counts — an exact count would have to be
    // edited by the same commit that adds a verb, which is the drift this guard exists to stop.
    assert!(
        registered.len() >= 12,
        "the router parser found only {} endpoints; build_router's shape must have changed, and \
         an empty population would make the assertions below pass vacuously",
        registered.len()
    );
    assert!(
        claimed.len() >= 12,
        "the tool-description parser found only {} claimed routes; the table's shape must have \
         changed",
        claimed.len()
    );

    let unreachable: Vec<String> = registered
        .iter()
        .filter(|endpoint| !claimed.contains(*endpoint) && !exempt(endpoint))
        .map(|(method, path)| format!("{method} {path}"))
        .collect();

    assert!(
        unreachable.is_empty(),
        "these HTTP routes are registered and no MCP tool names them: {unreachable:?}\n\
         A verb on HTTP and not on MCP is invisible to every parity guard in this crate, because \
         parity compares the verbs both surfaces HAVE.\n\
         Add the tool to apps/cli/src/commands/mcp/tools.rs, naming the route in its description \
         the way the other thirteen do. If the route genuinely is not a runtime verb, add it to \
         NOT_A_RUNTIME_VERB in this file WITH THE REASON — and write the reason, do not paste the \
         path."
    );
}

#[test]
fn every_route_an_mcp_tool_claims_is_actually_registered() {
    let registered = registered_endpoints();
    let claims = mcp_claims();

    assert!(
        registered.len() >= 12,
        "the router parser found only {} endpoints; see the sibling test",
        registered.len()
    );

    // THE MIRROR, and it is not symmetry for its own sake. The first test can only see a route
    // that EXISTS and is unclaimed. A tool whose description names a route that was renamed or
    // never built is a lie in the metadata this guard just made load-bearing, and the first test
    // is blind to it by construction — an over-claim shrinks the unreachable set rather than
    // growing it, so the defect makes its own detector quieter.
    let phantom: Vec<String> = claims
        .iter()
        .filter(|(method, path, _)| !registered.contains(&(method.clone(), path.clone())))
        .map(|(method, path, tool)| format!("{method} {path} (claimed by tool {tool:?})"))
        .collect();

    assert!(
        phantom.is_empty(),
        "these MCP tools name a route the router does not register: {phantom:?}\n\
         The descriptions are the join key the sibling test uses, so a wrong one does not merely \
         mislead a reader — it silences the completeness check for that route."
    );
}

/// #1309 (#1488 disposition, condition 4): the journey credential route is the one response that
/// carries a secret, so no middleware may write response bodies anywhere without excluding it.
/// Credible regression: a later body-logging layer (an audit, a trace, a debug dump) wraps the
/// router and transcribes the token, while every functional test still passes, because none of
/// them reads the new log. This reads the router's middleware from the source at run time and
/// requires every one that buffers a response body after `next.run` to call the exact-path
/// redaction, and refuses a `tower_http` trace layer in serve outright.
/// Cost: string scans of the serve sources; no build artifact, process or network.
#[test]
fn no_serve_middleware_buffers_a_response_without_redacting_the_journey_credential() {
    let serve_dir = crate_root().join("src/commands/serve");
    let mut sources = Vec::new();
    for entry in std::fs::read_dir(&serve_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|extension| extension == "rs") {
            sources.push((path.clone(), std::fs::read_to_string(&path).unwrap()));
        }
    }
    assert!(sources.len() >= 3, "the serve sources were not found");
    for (path, text) in &sources {
        assert!(
            !text.contains("TraceLayer") && !text.contains("tower_http::trace"),
            "{} adds a tracing layer; it must not record the body of \
             /v1/executions/{{id}}/journey-credentials (RFC-1309 \"Protected delivery channel\")",
            path.display()
        );
    }

    let router = source("src/commands/serve/mod.rs");
    let mut middleware = Vec::new();
    for marker in ["from_fn_with_state(state.clone(), ", "middleware::from_fn("] {
        let mut rest = router.as_str();
        while let Some(index) = rest.find(marker) {
            rest = &rest[index + marker.len()..];
            let name: String = rest
                .chars()
                .take_while(|character| character.is_alphanumeric() || *character == '_')
                .collect();
            if !name.is_empty() && !middleware.contains(&name) {
                middleware.push(name);
            }
        }
    }
    // Floor: the auth layer and the read audit are both middleware today; finding fewer means
    // the scan stopped seeing the router, and every assertion below would pass vacuously.
    assert!(
        middleware.iter().any(|name| name == "require_token")
            && middleware.iter().any(|name| name == "record_read"),
        "the middleware scan found {middleware:?}; build_router's shape must have changed"
    );
    let mut checked = 0;
    for name in &middleware {
        let start = sources
            .iter()
            .find_map(|(_, text)| {
                text.find(&format!("fn {name}("))
                    .map(|index| (text.as_str(), index))
            })
            .unwrap_or_else(|| panic!("middleware {name} is not defined in serve"));
        let (text, index) = start;
        let body = &text[index..];
        let end = body[1..]
            .find("\nasync fn ")
            .into_iter()
            .chain(body[1..].find("\nfn "))
            .chain(body[1..].find("\npub"))
            .min()
            .map_or(body.len(), |end| end + 1);
        let body = &body[..end];
        if body.contains("next.run(") && body.contains("to_bytes(") {
            checked += 1;
            assert!(
                body.contains("journey_credential_issue_path("),
                "middleware {name} buffers a response body but never calls \
                 journey_credential_issue_path: the journey credential would reach whatever it \
                 writes (RFC-1309 \"Protected delivery channel\")"
            );
        }
    }
    assert!(
        checked >= 1,
        "no body-buffering middleware was found; record_read buffers response bodies today, so \
         the scan no longer sees it"
    );
}
