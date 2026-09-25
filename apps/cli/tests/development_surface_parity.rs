//! #223: the development-contract operation family exists identically on the CLI, MCP, and HTTP
//! surfaces. Existence-parity guard (blueprint §6 item 1, PR #235) — behavioral parity (item 2)
//! is separate, later work.
//!
//! **Deviates from the blueprint's original "three independently-derived SETS, asserted equal"**
//! in one respect, measured while building this rather than assumed: `apps/cli` has no library
//! target (`cargo test -p graphhelm-cli --lib` errors "no library targets found"), so #215/#231's
//! `#[doc(hidden)]` internal-accessor pattern — which works because `CLI_COMMANDS`/`MCP_TOOLS`
//! live in the separate `graphhelm_schema` LIBRARY crate — cannot reach anything inside this
//! binary crate's own `TOOLS` const or router. `mcp_capability.rs` already states the house
//! convention this follows: "each integration test file is self-contained... apps/cli is bin-only,
//! no lib target." What this guard does instead, per declared operation family: prove the SAME
//! name resolves correctly on all three surfaces, each probed against the REAL BUILT BINARY —
//! subprocess-level, the same discipline `extension_cli.rs`'s `--help` walk and
//! `mcp_capability.rs`'s `tools/list` reads already use.
//!
//! Grows by one line in [`DEVELOPMENT_OPERATION_FAMILIES`] per operation family landed. A family
//! present on this list and missing from any one surface fails on exactly that surface, naming it.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// The canonical operation-family names this guard covers, kebab-case (the CLI's own spelling —
/// `development <name>`). Checked NON-EMPTY first (below) so an accidentally-emptied list fails
/// loudly rather than making every assertion in the per-family loop pass vacuously.
///
/// **This list's own completeness is not free — J's review of #351.** The forward loop below
/// only visits what THIS list names, so a real CLI leaf added without a matching entry here
/// (forgotten in the SAME missed step that would also forget the HTTP route) would be invisible
/// to it. `every_real_cli_development_leaf_is_a_declared_family` (same file) is the other
/// direction: it walks the REAL `development` subtree and refuses any leaf this list does not
/// name, so the two loops together are what make this list actually closed rather than merely
/// looking closed with one entry in it.
const DEVELOPMENT_OPERATION_FAMILIES: &[FamilySurfaces] = &[
    FamilySurfaces {
        cli: "resolve-contract",
        mcp: "resolve_contract",
        http_method: "POST",
        // "resolve-contract" -> "/v1/development/contract": the CLI/MCP names the ACTION ("resolve"),
        // the HTTP path names the RESOURCE it acts on ("contract"), under the REST convention this
        // codebase already uses elsewhere (`/v1/executions/{id}/start`, not `/v1/start-execution`).
        http_probe_path: "/v1/development/contract",
    },
    FamilySurfaces {
        cli: "memory-status",
        mcp: "memory_status",
        http_method: "GET",
        // "/v1/development/memory", and deliberately WITHOUT the `{id}` the original scope list
        // carried. Measured while building this: nothing persists a `MemoryRecord` — it exists only
        // in `core/governor`, constructed in memory — so an `{id}` here would have nothing to resolve
        // against, and the only ways to serve it were a refusal code that does not exist or a constant
        // answer that ignores the parameter. A parameter that cannot change the response is a lie with
        // a route attached.
        //
        // WHEN THIS WAKES UP: `{id}` returns as a FEATURE the day a store lands, and whoever writes
        // that store is the consumer of this sentence. Until then this reads the shipped
        // memory-transition policy, which is real, digest-bound and schema-owned.
        http_probe_path: "/v1/development/memory",
    },
    FamilySurfaces {
        cli: "present",
        mcp: "present",
        http_method: "POST",
        // "/v1/development/present", with NO suffix stripped and none added -- and that is a fact
        // about this family rather than a rule about names. The other two families split a verb
        // from a noun because their action and their resource are different words. Here they are
        // the SAME word: the thing being done IS the thing being acted on, so the whole name is
        // the segment.
        //
        // Worth stating rather than leaving to be re-derived: before this struct existed, the path
        // was computed by dropping the text before the first hyphen. "present" has no hyphen, so
        // that computation returned the whole name -- the right answer, reached by falling through
        // rather than by deciding. The old `to_http_path` asked whoever brought the first exception
        // to write the decision down; this is it, in the place decisions now live.
        http_probe_path: "/v1/development/present",
    },
    FamilySurfaces {
        cli: "compile-context",
        mcp: "compile_context",
        http_method: "POST",
        // "compile-context" -> "/v1/development/context": the CLI/MCP names the ACTION
        // ("compile"), the HTTP path names the RESOURCE it acts on ("context") -- the same
        // verb-noun split `resolve-contract` above uses, under the same REST convention.
        http_probe_path: "/v1/development/context",
    },
    FamilySurfaces {
        cli: "memory-propose",
        mcp: "memory_propose",
        http_method: "POST",
        // The SAME path as memory-status, under a different verb -- "/v1/development/memory" reads
        // and is written by the two families that act on one resource. That is the REST convention
        // this codebase already uses, and the probe can tell them apart: it refuses 404 (no route)
        // AND 405 (route exists under another verb), so a POST family wired only as GET fails here
        // as loudly as one not wired at all.
        //
        // The alternative was a sub-resource, "/v1/development/memory/proposal". Rejected on a
        // measurement rather than on taste: nothing persists a MemoryCandidate or a MemoryRecord --
        // both exist only in core/governor, built in memory -- so a proposal resource would be a
        // noun that can never be fetched. A route that cannot answer GET is not a resource.
        http_probe_path: "/v1/development/memory",
    },
    FamilySurfaces {
        cli: "accounting",
        mcp: "accounting",
        http_method: "GET",
        // "accounting" has no verb-noun split at all -- same shape as "present" above, the whole
        // name is the segment because there is no separate action word.
        http_probe_path: "/v1/development/accounting",
    },
];

/// The tools that are deliberately NOT development operations -- the exceptions that make
/// `every_real_mcp_tool_is_classified` below a closed question rather than an open one.
///
/// **Hand-written on purpose, never derived.** Computing this as `tools/list` minus
/// [`DEVELOPMENT_OPERATION_FAMILIES`] would make the completeness guard a tautology: the list
/// would agree with the real surface by construction and could never disagree with it. What is
/// written here is a CLAIM -- "these seventeen are not development operations" -- and the two
/// guards below check that claim against the real built binary in both directions.
///
/// **What the arity does and does not do.** `[&str; 17]` forces this LITERAL to hold seventeen
/// entries. It does not force those seventeen to be the right ones, or the complete set -- that is
/// exactly the trap #272 names for hand-sized vocabulary arrays. The arity is a tripwire that
/// makes an edit here deliberate; the completeness comes from
/// `every_real_mcp_tool_is_classified` and `every_named_exception_still_names_a_real_tool`, not
/// from the number. Do not read the 17 as the guarantee.
///
/// **The `MCP_TOOLS` overlap -- measured, and deliberately NOT bound.**
/// `core/schema/src/extension.rs`'s `MCP_TOOLS` holds fourteen of these seventeen strings, as of
/// this edit: `sweep` (#288), `list` and `topology` (#105) are exceptions here and are NOT in
/// that allowlist.
/// The overlap is a fact of today, not a shared definition, and no test asserts the two are
/// equal. `MCP_TOOLS` is a narrow allowlist of the domain surface a skill journey may claim
/// to drive, documented there as deliberately NOT the full surface, with mutating and destructive
/// operations excluded on purpose. This list is the complement of the development families over
/// the WHOLE tool surface. The day a destructive tool is added the two diverge correctly: it
/// belongs here and must never enter `MCP_TOOLS`. A guard asserting equality would fire on that
/// correct divergence and pressure the next reader to "fix" it by widening the allowlist, turning
/// a safety boundary into bookkeeping.
///
/// Order follows `TOOLS`'s own declaration order ("the closed list, in the plan's order"), not
/// alphabetical, so a reader can diff the two surfaces by eye.
const NON_DEVELOPMENT_TOOLS: [&str; 23] = [
    "start",
    "list",
    "topology",
    "status",
    // #1063. The resume briefing is a READ over an execution's store, beside status.
    "briefing",
    "events",
    "evidence",
    "signal",
    "approve",
    "pause",
    "resume",
    "cancel",
    "routes",
    "route_set",
    "wake_arm",
    "wake_status",
    "amend_budget",
    "wake_wait",
    "probe",
    // #288. The customs sweep is a RUNTIME verb on an execution -- it journals a
    // sweep_performed and one overdue_exception per lapsed episode -- and has nothing to do with
    // the development contract. It sits beside pause and cancel, not beside resolve_contract.
    "sweep",
    // #159. The customs claim and clear are RUNTIME verbs on an execution -- testimony that a
    // parked node's external work is done, and the machine-replay countersignature that
    // releases it -- and sit beside sweep, not beside resolve_contract.
    "claim",
    "clear",
    // #107. The Graph Architect compiles a goal into a graph document: a RUNTIME verb over the
    // graph surface (it sits beside topology), not a development operation family.
    "synthesize",
];

/// Execution operations that share the same MCP/HTTP/CLI contract but do not belong to the
/// development namespace. Keeping these as families makes their three-surface wiring explicit
/// rather than hiding them in the non-development exception list.
const EXECUTION_OPERATION_FAMILIES: &[FamilySurfaces] = &[
    FamilySurfaces {
        cli: "document-read",
        mcp: "document_read",
        http_method: "POST",
        http_probe_path: "/v1/executions/run-1/documents/read",
    },
    FamilySurfaces {
        cli: "document-save",
        mcp: "document_save",
        http_method: "POST",
        http_probe_path: "/v1/executions/run-1/documents/save",
    },
];

/// Where one operation family lives on each of the three surfaces.
///
/// **These were derived from the family's own name until this struct existed, and that stopped
/// working at the second family.** The rule was `"verb-noun"` -> drop the verb, and it carried a
/// SEMANTIC decision — which word names the resource — inside a lexical split. It is correct for
/// `resolve-contract` and wrong for a `noun-verb` family, wrong for any family whose route is not
/// `POST`, and unable to express a path parameter at all. The old `to_http_path` said as much in
/// its own comment: *"the day a family needs a different split, this function is where that
/// decision gets written down, not re-derived per call site."* This struct is that day, and the
/// decision is written down per family rather than guessed from a name.
///
/// A rule that is only ever exercised by the one example it was written for is not a rule yet.
struct FamilySurfaces {
    /// The CLI subcommand under `development`, kebab-case, and the family's canonical name.
    cli: &'static str,
    /// The MCP tool name. Declared, not transformed from `cli`: a derived name computes the
    /// expectation as confidently when it is wrong as when it is right, so a misnamed tool would
    /// be reported as a MISSING one and the reader would go looking for the wrong defect.
    ///
    /// **Carrying it here makes half of the reverse MCP direction expressible, and the other half
    /// is still missing — J's review of #351.** `every_real_cli_development_leaf_is_a_declared_family`
    /// closes the reverse direction for the CLI because `development --help` enumerates exactly the
    /// development leaves. The MCP side has no equivalent: `ToolSpec` is `{ name, description,
    /// schema }`, and the table is flat — nothing marks a tool as belonging to this family group,
    /// so walking it and asking "which of these should be declared here?" can only be answered by
    /// consulting this const, which is the thing under test.
    ///
    /// **Recovering it from the name is not available either, and that is measured rather than
    /// assumed:** the table already ships `status` and `wake_status`, so a family named
    /// `memory-status` would map to `memory_status` and sit beside two existing tools whose names
    /// no convention separates from it. A prefix rule would have to be invented, and an invented
    /// rule with one instance is the shape this whole struct exists to replace.
    ///
    /// **Where the decision wakes up:** the missing half is a domain marker on `ToolSpec` itself —
    /// one field, and the reverse loop becomes writable. It is scheduled as the first item of the
    /// next round rather than added here, because it changes a production table every entry must
    /// then fill in, in a file other families are writing.
    ///
    /// **And before harmonising this field with `cli`: do not.** The two are kebab and snake of one
    /// word today, and a guard asserting that relation was considered and refused — **a convention
    /// with one instance does not distinguish itself from a coincidence.** Enforcing it now would
    /// pin an accident and refuse the first family that legitimately needs a different tool name.
    /// If a second and third family arrive spelling it the same way, the convention has earned a
    /// guard; until then this field is a declaration, not a derivation, and that is the point of it.
    mcp: &'static str,
    /// The HTTP method the route is registered under. Required because the probe asserts
    /// non-405, so a family served by a different verb than the probe sends fails as loudly as
    /// one that was never wired.
    http_method: &'static str,
    /// A CONCRETE path to probe, path parameters already filled in.
    ///
    /// Concrete rather than a template plus a value, because a template forces the prober to
    /// invent one, and inventing it is the same class of decision the derived path was making
    /// badly. The value need not name anything that exists: **no handler under `/v1` returns 404**
    /// — measured across `apps/cli/src/commands/serve/`, where `NOT_FOUND` appears only in the
    /// router's own `fallback` (`serve/mod.rs`) and in the separate HTML monitor sub-router — so a
    /// 404 here can only mean the path matched no route. That is what makes probing a parameterised
    /// route sound, and it is a constraint on future handlers as much as a fact about today's: a
    /// `/v1/development` handler that answers 404 for an unknown id would make this guard unable to
    /// tell "not wired" from "not found".
    http_probe_path: &'static str,
}

// -------------------------------------------------------------------------------------------
// CLI surface: walk `graphhelm development --help`'s own Commands: section.
// -------------------------------------------------------------------------------------------

fn parse_help_subcommand_names(help_text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_commands_section = false;
    for line in help_text.lines() {
        if line.trim() == "Commands:" {
            in_commands_section = true;
            continue;
        }
        if !in_commands_section {
            continue;
        }
        if line.trim().is_empty() || !line.starts_with(char::is_whitespace) {
            break;
        }
        if let Some(name) = line.split_whitespace().next()
            && name != "help"
        {
            names.push(name.to_owned());
        }
    }
    names
}

fn real_cli_development_leaves() -> Vec<String> {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["development", "--help"])
        .output()
        .expect("the built binary runs `development --help`");
    parse_help_subcommand_names(&String::from_utf8_lossy(&output.stdout))
}

fn real_cli_execution_leaves() -> Vec<String> {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["execution", "--help"])
        .output()
        .expect("the built binary runs `execution --help`");
    parse_help_subcommand_names(&String::from_utf8_lossy(&output.stdout))
}

// -------------------------------------------------------------------------------------------
// MCP surface: one `tools/list` round trip over stdio, no live backend needed for listing.
// -------------------------------------------------------------------------------------------

fn real_mcp_tool_names() -> Vec<String> {
    let mut input = String::new();
    for line in [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": "2025-06-18", "capabilities": {},
                       "clientInfo": {"name": "test", "version": "0"}}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    ] {
        input.push_str(&line.to_string());
        input.push('\n');
    }
    let output = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["mcp", "--url", "http://127.0.0.1:9", "--actor", "agent-x"])
        .env("GRAPHHELM_API_TOKEN", "test-token")
        .write_stdin(input)
        .timeout(Duration::from_secs(30))
        .output()
        .expect("the mcp server runs to EOF");
    let replies: Vec<Value> = String::from_utf8(output.stdout)
        .expect("stdout is UTF-8")
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|value| value.get("jsonrpc").is_some())
        .collect();
    let list_reply = replies
        .get(1)
        .unwrap_or_else(|| panic!("expected 2 replies (initialize, tools/list), got: {replies:?}"));
    list_reply["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("tools/list must reply with a \"tools\" array: {list_reply}"))
        .iter()
        .map(|tool| {
            tool["name"]
                .as_str()
                .expect("each listed tool has a string \"name\"")
                .to_owned()
        })
        .collect()
}

// -------------------------------------------------------------------------------------------
// HTTP surface: spawn a real `graphhelm serve`, probe the declared path, confirm it is NOT a
// fallback 404 -- proof the route is actually wired, not merely declared in two places that
// happen to agree. Deliberately minimal next to `api_http.rs`'s `ServerGuard`: this test sends
// exactly one request and exits, so it does not need that struct's drain-thread machinery,
// which exists there for long-running concurrent tests this one is not.
// -------------------------------------------------------------------------------------------

fn probe_http_route_exists(method_path: &str) {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let mut child = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args([
            "serve",
            "--events",
            events.to_str().unwrap(),
            "--bind",
            "127.0.0.1:0",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("graphhelm serve spawns");

    let mut stdout = child.stdout.take().expect("stdout is piped");
    let mut buffer = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(30);
    let started: Value = loop {
        let mut byte = [0u8; 1];
        match stdout.read(&mut byte) {
            Ok(1) if byte[0] == b'\n' => break parse_startup_line(&buffer, &mut child),
            Ok(1) => buffer.push(byte[0]),
            _ => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    panic!("`graphhelm serve` printed no startup line within 30s");
                }
            }
        }
    };
    assert_eq!(
        started["ok"], true,
        "expected a successful startup: {started}"
    );
    let address = started["data"]["address"]
        .as_str()
        .unwrap_or_else(|| panic!("startup envelope must carry data.address: {started}"))
        .to_owned();

    let token_path = token_file_path(&events);
    let token = read_token(&token_path);

    let (method, path) = method_path
        .split_once(' ')
        .expect("probe target is \"METHOD /path\"");
    let status = send_request(&address, method, path, &token);

    let _ = child.kill();
    let _ = child.wait();

    assert_ne!(
        status, 404,
        "{method} {path} 404'd against the real server -- the route is declared but not wired \
         (or wired to a different path). Check build_router() in \
         apps/cli/src/commands/serve/mod.rs against this probe's target."
    );
    // 405, not 404, is what a PATH-registered-under-the-wrong-METHOD returns (J's review of
    // #351): the fallback-404 check alone would pass for a family wired as GET when the CLI/MCP
    // surfaces call it as POST -- the path exists, just not for this verb. Both checks are
    // needed; neither implies the other.
    assert_ne!(
        status, 405,
        "{method} {path} answered 405 against the real server -- a route exists at this path but \
         not for this method. Check build_router() registers {method}, not some other verb, for \
         this path."
    );
}

fn parse_startup_line(buffer: &[u8], child: &mut std::process::Child) -> Value {
    serde_json::from_slice(buffer).unwrap_or_else(|error| {
        let _ = child.kill();
        panic!(
            "the startup line was not valid JSON ({error}): {:?}",
            String::from_utf8_lossy(buffer)
        )
    })
}

fn token_file_path(events: &Path) -> std::path::PathBuf {
    let mut name = events.file_name().map_or_else(
        || std::ffi::OsString::from("events"),
        std::ffi::OsStr::to_os_string,
    );
    name.push(".token");
    events.with_file_name(name)
}

fn read_token(path: &Path) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(contents) = std::fs::read_to_string(path)
            && !contents.is_empty()
        {
            return contents;
        }
        if Instant::now() >= deadline {
            panic!("the server never wrote a readable token file at {path:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn send_request(address: &str, method: &str, path: &str, token: &str) -> u16 {
    let (host, port) = address
        .split_once(':')
        .expect("startup address always carries an explicit port");
    let mut stream = TcpStream::connect((host, port.parse::<u16>().unwrap()))
        .expect("the server accepts a connection right after startup");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let body = b"{}";
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    stream.write_all(request.as_bytes()).unwrap();
    stream.write_all(body).unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let text = String::from_utf8_lossy(&raw);
    let status_line = text
        .lines()
        .next()
        .unwrap_or_else(|| panic!("empty HTTP response from {method} {path}"));
    status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or_else(|| panic!("malformed status line: {status_line:?}"))
}

// -------------------------------------------------------------------------------------------
// The guard itself.
// -------------------------------------------------------------------------------------------

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before writing it: a family landed on one
/// or two surfaces and forgotten on the third -- exactly the drift #215's `CLI_COMMANDS`/
/// `MCP_TOOLS` divergence caught for the pre-existing surface, generalized to development's own
/// three-way case.
#[test]
fn every_declared_development_operation_family_exists_on_all_three_surfaces() {
    // Vacuity guard: an empty family list would make the loop below vacuously pass, proving
    // nothing about any surface. Asserted before anything else runs.
    assert!(
        !DEVELOPMENT_OPERATION_FAMILIES.is_empty(),
        "DEVELOPMENT_OPERATION_FAMILIES is empty -- the per-family checks below would pass \
         vacuously, checking zero surfaces"
    );

    let cli_leaves = real_cli_development_leaves();
    assert!(
        !cli_leaves.is_empty(),
        "HARNESS-BROKE: `development --help` listed no subcommands at all, so the per-family \
         CLI check below would fail for the wrong reason (broken instrument, not a missing \
         family)"
    );

    let mcp_tools = real_mcp_tool_names();
    assert!(
        !mcp_tools.is_empty(),
        "HARNESS-BROKE: tools/list returned no tools at all, so the per-family MCP check below \
         would fail for the wrong reason"
    );

    for family in DEVELOPMENT_OPERATION_FAMILIES {
        let name = family.cli;
        assert!(
            cli_leaves.contains(&name.to_owned()),
            "{name:?} is declared but `development --help` does not list it as a CLI \
             subcommand. Real subcommands seen: {cli_leaves:?}"
        );

        assert!(
            mcp_tools.contains(&family.mcp.to_owned()),
            "{name:?} is declared but the real MCP tools/list has no {:?} entry. \
             Real tools seen: {mcp_tools:?}",
            family.mcp
        );

        probe_http_route_exists(&format!(
            "{} {}",
            family.http_method, family.http_probe_path
        ));
    }
}

#[test]
fn every_declared_execution_operation_family_exists_on_all_three_surfaces() {
    let cli_leaves = real_cli_execution_leaves();
    let mcp_tools = real_mcp_tool_names();
    assert!(
        !cli_leaves.is_empty(),
        "execution --help listed no subcommands"
    );
    assert!(!mcp_tools.is_empty(), "tools/list returned no tools");

    for family in EXECUTION_OPERATION_FAMILIES {
        assert!(
            cli_leaves.contains(&family.cli.to_owned()),
            "{} missing from execution CLI: {cli_leaves:?}",
            family.cli
        );
        assert!(
            mcp_tools.contains(&family.mcp.to_owned()),
            "{} missing from MCP tools/list: {mcp_tools:?}",
            family.mcp
        );
        probe_http_route_exists(&format!(
            "{} {}",
            family.http_method, family.http_probe_path
        ));
    }
}

/// THE OTHER DIRECTION — J's review of #351. The test above walks `DEVELOPMENT_OPERATION_FAMILIES`
/// and checks each name is real; nothing in it notices a real CLI leaf that was never added to
/// that list. A second family wired into the CLI and MCP dispatch and forgotten on BOTH the HTTP
/// route and this list is invisible to the forward test -- forgetting the list entry is the SAME
/// missed step as forgetting the HTTP route, not an independent one, so the two omissions land
/// together far more often than a hand-audit would catch.
///
/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before writing it: a new CLI subcommand
/// under `development` that never gets a line added to `DEVELOPMENT_OPERATION_FAMILIES`.
#[test]
fn every_real_cli_development_leaf_is_a_declared_family() {
    let cli_leaves = real_cli_development_leaves();
    assert!(
        !cli_leaves.is_empty(),
        "HARNESS-BROKE: `development --help` listed no subcommands at all, so this loop would \
         pass vacuously over zero leaves"
    );

    for leaf in &cli_leaves {
        assert!(
            DEVELOPMENT_OPERATION_FAMILIES
                .iter()
                .any(|family| family.cli == leaf.as_str()),
            "{leaf:?} is a real `development` CLI subcommand that DEVELOPMENT_OPERATION_FAMILIES \
             does not name. Either it was never added to the const (this test's whole reason to \
             exist), or it is a leaf the forward test above never checked against MCP/HTTP at \
             all -- add it to DEVELOPMENT_OPERATION_FAMILIES in this file."
        );
    }
}

/// **The other half of the reverse direction, and a different population from the CLI loop above.**
///
/// `every_real_cli_development_leaf_is_a_declared_family` walks the real `development` CLI subtree
/// -- so it only ever sees leaves that are ALREADY under `development`. It cannot see a tool that
/// was added to the MCP surface and never given a development home, because such a tool is not in
/// the subtree it walks. This loop starts from the whole real tool surface instead, and asks of
/// EVERY tool: is it accounted for?
///
/// The production change this catches: a nineteenth tool added to `TOOLS` and classified nowhere.
/// Today that is completely silent -- the forward loop checks only what
/// `DEVELOPMENT_OPERATION_FAMILIES` names, and the CLI reverse loop checks only the `development`
/// subtree, so an unclassified tool falls between them and every test in the repository stays
/// green.
///
/// **XOR, not "at least one".** Being in both lists is also red: a tool named by a family AND
/// excepted here means two owners disagree about what it is, and the more permissive reading
/// (it is fine, something covers it) is the one a reader reaches for.
#[test]
fn every_real_mcp_tool_is_classified() {
    let tools = real_mcp_tool_names();

    // non-empty-is-not-a-control: asserted as its own assertion, BEFORE the sweep. A broken
    // stdio round trip would otherwise yield an empty population and make every check below
    // pass over zero items.
    assert!(
        !tools.is_empty(),
        "HARNESS-BROKE: tools/list returned no tools at all, so the classification sweep below \
         would pass vacuously over zero tools"
    );
    assert!(
        !DEVELOPMENT_OPERATION_FAMILIES.is_empty(),
        "HARNESS-BROKE: DEVELOPMENT_OPERATION_FAMILIES is empty, so every tool below would be \
         reported as an unclassified tool rather than as the classification gap this test is for"
    );

    for tool in &tools {
        let claimed_by_family = DEVELOPMENT_OPERATION_FAMILIES
            .iter()
            .any(|family| family.mcp == tool.as_str());
        let claimed_by_execution_family = EXECUTION_OPERATION_FAMILIES
            .iter()
            .any(|family| family.mcp == tool.as_str());
        let claimed_as_exception = NON_DEVELOPMENT_TOOLS.contains(&tool.as_str());

        assert!(
            claimed_by_family || claimed_by_execution_family || claimed_as_exception,
            "{tool:?} is a real MCP tool that nothing classifies. Does it belong to a \
             development operation family? If YES, add it to DEVELOPMENT_OPERATION_FAMILIES -- \
             that also puts it under the three-surface parity check, which is the point. \
             NON_DEVELOPMENT_TOOLS is only for tools that are NOT development operations; \
             putting it there instead makes this red go away without answering the question, \
             and nothing downstream will notice."
        );
        assert!(
            !((claimed_by_family || claimed_by_execution_family) && claimed_as_exception),
            "{tool:?} is claimed BOTH by a development operation family and by \
             NON_DEVELOPMENT_TOOLS. Those are opposite claims about the same tool, and the \
             permissive reading -- that something covers it -- is the one a reader reaches for. \
             Remove whichever entry is wrong."
        );
    }
}

/// The direction the sweep above structurally cannot see: a stale exception.
///
/// `every_real_mcp_tool_is_classified` iterates the REAL surface, so an entry in
/// `NON_DEVELOPMENT_TOOLS` naming a tool that no longer exists is never visited by it -- the loop
/// simply never reaches that name. A removed or renamed tool would leave a dead exception behind,
/// and the dead entry would go on silently excusing a name nothing serves.
///
/// Same shape, and the same reasoning, as `MCP_TOOLS`'s own subset policy in
/// `core/schema/src/extension.rs`: the dangerous direction is naming a surface that does not
/// exist.
#[test]
fn every_named_exception_still_names_a_real_tool() {
    let tools = real_mcp_tool_names();
    assert!(
        !tools.is_empty(),
        "HARNESS-BROKE: tools/list returned no tools at all, so every exception below would be \
         reported as stale rather than checked"
    );

    for exception in NON_DEVELOPMENT_TOOLS {
        assert!(
            tools.iter().any(|tool| tool == exception),
            "NON_DEVELOPMENT_TOOLS excepts {exception:?}, but the real tools/list has no such \
             tool. The entry outlived the tool it excepted -- a rename or a removal left it \
             behind. Real tools seen: {tools:?}"
        );
    }
}
