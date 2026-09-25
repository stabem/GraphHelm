//! ADR-034's WIDE clauses, over a population DERIVED FROM THE TREE (#628).
//!
//! `request_set_ceiling.rs` measures the shape of one function in five cells and the POPULATION in
//! none: it reaches its subject through `include_str!("../src/provider.rs")`, a path written by
//! hand. A second hand-rolled MCP client anywhere else in the repository is invisible to it — and
//! "no other speaker exists" is exactly the kind of claim that rots without anybody editing a
//! line. L found this against the guard I had just landed.
//!
//! So the speakers are ENUMERATED rather than listed. The guarded set and the found set must be
//! equal in both directions: a new speaker fails because nobody remembered to declare it, and a
//! removed one fails because the declaration outlived its subject.
//!
//! **Client, not server.** ADR-026 governs GraphHelm's own MCP *server*; this is about code that
//! ACTS as a client. The mechanical separator is the `initialize` request: only a client
//! constructs one. A server — or a test double — matches on the method name instead, which is a
//! different token (`Some("initialize")`, not `"method": "initialize"`).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The declared population. Every entry is a hand-rolled MCP CLIENT under ADR-034's wide clauses.
const DECLARED_SPEAKERS: &[&str] = &[
    "adapters/codebase-memory-mcp/src/provider.rs",
    "tools/development-benchmark/src/bin/generate-retrieval.rs",
];

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every `.rs` file in the source tree, excluding build output and the fixtures that carry wire
/// samples rather than code.
fn source_files(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                if !matches!(name.as_str(), "target" | ".git" | "node_modules") {
                    stack.push(path);
                }
            } else if name.ends_with(".rs") {
                found.push(path);
            }
        }
    }
    found
}

/// Files that CONSTRUCT an `initialize` request — the one message only a client sends.
fn hand_rolled_clients(root: &Path) -> BTreeSet<String> {
    let mut speakers = BTreeSet::new();
    for path in source_files(root) {
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        // The field form, as it appears in a constructed message. A server or double matches the
        // method name instead and never writes this.
        let constructs = source.contains("\"method\": \"initialize\"")
            || source.contains("\"method\":\"initialize\"");
        if !constructs {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        // Tests are consumers of the speakers, not speakers with their own ceiling: they drive a
        // double or assert over one. Excluded by PATH, which is a property of the tree rather
        // than of the file's content.
        if relative.contains("/tests/") {
            continue;
        }
        speakers.insert(relative);
    }
    speakers
}

/// THE POPULATION, in both directions.
#[test]
fn the_hand_rolled_mcp_clients_are_exactly_the_declared_ones() {
    let root = repository_root();
    let found = hand_rolled_clients(&root);
    let declared: BTreeSet<String> = DECLARED_SPEAKERS
        .iter()
        .map(|entry| (*entry).to_owned())
        .collect();

    // POSITIVE CONTROL first: an enumeration that finds nothing would make every assertion below
    // vacuously true, which is how a sweep passes while measuring an empty tree.
    assert!(
        !found.is_empty(),
        "the enumeration found no MCP clients at all, so it is measuring nothing — check the \
         walk before trusting any result from it"
    );

    let undeclared: Vec<&String> = found.difference(&declared).collect();
    assert!(
        undeclared.is_empty(),
        "a hand-rolled MCP client exists that ADR-034's clauses do not cover: {undeclared:?}\n\n\
         Either it is bounded by the same clauses (three methods, no negotiated capabilities, \
         stdio only) and belongs in DECLARED_SPEAKERS, or the decision does not cover it and \
         must be reopened (#628). A speaker nobody declared is the growth the ADR bounds."
    );
    let vanished: Vec<&String> = declared.difference(&found).collect();
    assert!(
        vanished.is_empty(),
        "a declared speaker no longer constructs an initialize request: {vanished:?}\n\n\
         The declaration outlived its subject, and a population guard that keeps naming absent \
         members drifts into fiction the same way a stale digest does."
    );
}

/// THE WIDE CLAUSES, applied to every speaker the tree yields rather than to one named file.
#[test]
fn every_hand_rolled_client_obeys_the_wide_clauses() {
    let root = repository_root();
    let speakers = hand_rolled_clients(&root);
    assert!(
        !speakers.is_empty(),
        "positive control: the walk found speakers"
    );

    for speaker in &speakers {
        let source = std::fs::read_to_string(root.join(speaker)).expect("a listed speaker reads");

        // No negotiated capabilities: an empty object declares that nothing is negotiated.
        assert!(
            source.contains("\"capabilities\": {}") || source.contains("\"capabilities\":{}"),
            "{speaker} negotiates capabilities; ADR-034's wide clause allows none"
        );

        // Only the three methods. Anything else is a different exchange, whatever its count.
        for forbidden in [
            "\"method\": \"tools/list\"",
            "\"method\": \"resources/",
            "\"method\": \"prompts/",
            "\"method\": \"completion/",
            "\"method\": \"logging/",
            "\"method\": \"ping\"",
        ] {
            assert!(
                !source.contains(forbidden),
                "{speaker} sends {forbidden}, outside ADR-034's three-method set"
            );
        }

        // Stdio only: a socket or an HTTP endpoint is a connection with a lifetime, which is the
        // session ADR-026 rejected.
        for transport in ["TcpStream", "UnixStream", "reqwest", "hyper", "ws://"] {
            assert!(
                !source.contains(transport),
                "{speaker} mentions {transport}; the wide clause is stdio-only"
            );
        }
    }
}
