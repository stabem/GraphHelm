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
