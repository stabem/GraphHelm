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

/// `{executionId}` (how a tool description names it) and `{id}` (how axum registers it) are the
/// same parameter. Normalise toward the registration, which is the side that decides.
fn normalise(path: &str) -> String {
    path.replace("{executionId}", "{id}")
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
const NOT_A_RUNTIME_VERB: [(&str, &str, &str); 4] = [
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
        "/monitor",
        "the human HTML monitor index; its surface is a browser, and it is GET-only by \
         construction",
    ),
    (
        "GET",
        "/monitor/{id}",
        "the human HTML monitor page, same reason as the index",
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
