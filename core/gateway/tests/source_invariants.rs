//! Enforces the purity boundary this crate claims for itself, mirroring
//! `core/execution/tests/source_invariants.rs`.
//!
//! `core/gateway/src/lib.rs` documents that this crate is "total and side-effect free: no clock,
//! no randomness, no filesystem, no network and no adapter dependency." A documented control which
//! no test enforces is not a control (Milestone 03's review) — these tests make that statement
//! fail loudly the first time it stops being true.

const MANIFEST: &str = include_str!("../Cargo.toml");
const LIB: &str = include_str!("../src/lib.rs");
const MANIFEST_SRC: &str = include_str!("../src/manifest.rs");
const TAXONOMY: &str = include_str!("../src/taxonomy.rs");
const ELIGIBILITY: &str = include_str!("../src/eligibility.rs");
const CALL: &str = include_str!("../src/call.rs");

/// The `[dependencies]` table only, stopping at the next `[section]` header.
///
/// Same scoping rationale as `core/execution/tests/source_invariants.rs`: purity is a claim about
/// what ships in the compiled library, not about what a test file needs to compose a fixture, so
/// `[dev-dependencies]` is out of scope for this scan. `core/gateway` currently declares no
/// `[dev-dependencies]` at all — Task 2 needed none beyond what Task 1 already brought in.
fn production_dependencies(manifest: &str) -> &str {
    let start = manifest
        .find("[dependencies]")
        .expect("manifest must declare a [dependencies] table")
        + "[dependencies]".len();
    let rest = &manifest[start..];
    let end = rest.find("\n[").map_or(rest.len(), |offset| offset);
    &rest[..end]
}

/// Strips line comments so prose cannot decide the outcome in either direction.
///
/// Without this, a doc comment describing what the crate must *not* do would trip the very scan it
/// is documenting, and the test would fail for saying the right thing.
fn code_only(source: &str) -> String {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The first identifier-like token on a `[dependencies]` line, whether the line is declared
/// `name = { ... }` (a path dependency, like `graphhelm-protocols`) or `name.workspace = true` (a
/// workspace-managed dependency, like `serde`/`serde_json`) — `core/execution`'s equivalent helper
/// only had to look for `graphhelm-` prefixes because every one of its dependencies used the first
/// form; `core/gateway` needs both.
fn declared_crate_names(manifest: &str) -> Vec<&str> {
    production_dependencies(manifest)
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            let end = trimmed.find(['.', ' ', '=']).unwrap_or(trimmed.len());
            Some(&trimmed[..end])
        })
        .collect()
}

/// The production-dependency slice reads only the `[dependencies]` table, so a dependency
/// smuggled into a table the slice never reaches would be invisible to the scans below. There is
/// exactly one legitimate place for a production dependency in this crate, so any other
/// dependency-carrying table is forbidden outright.
#[test]
fn no_other_dependency_table_exists() {
    for line in MANIFEST.lines() {
        let trimmed = line.trim();
        let is_table = trimmed.starts_with('[');
        let carries_dependencies = trimmed.contains("dependencies");
        let allowed = trimmed == "[dependencies]" || trimmed == "[dev-dependencies]";
        assert!(
            !(is_table && carries_dependencies && !allowed),
            "unexpected dependency table: {trimmed}"
        );
    }
}

/// This crate must stay pure. A clock, a random source, a filesystem, a network socket or a
/// subprocess dependency would let a future edit make replay reproduce a different decision from
/// the same history — the defect class `core/execution`'s purity tests exist to catch, asserted
/// here before `core/gateway` grows past the two source files Task 1 shipped.
#[test]
fn the_gateway_crate_has_no_impure_dependency() {
    let production = production_dependencies(MANIFEST);
    for forbidden in [
        "tokio",
        "sqlx",
        "chrono",
        "getrandom",
        "rand",
        "reqwest",
        "ureq",
        "graphhelm-postgres",
        "graphhelm-sealed",
        "graphhelm-model-gateway",
        "adapters/",
    ] {
        assert!(
            !production.contains(forbidden),
            "core/gateway must not depend on {forbidden}"
        );
    }
}

/// Pins the exact production-dependency set committed alongside Task 1
/// (`core/gateway/src/manifest.rs`): `graphhelm-protocols`, `serde` and `serde_json`, and nothing
/// else. Task 1 added no error-derive crate — `ManifestError` has a hand-written `Display` impl,
/// not a `thiserror` derive — so unlike `core/execution` (which also depends on `graphhelm-events`)
/// this crate's table is exactly these three. Adding a dependency is a deliberate edit to this
/// test, not a silent manifest change.
#[test]
fn the_gateway_crate_depends_on_exactly_the_declared_crates() {
    assert_eq!(
        declared_crate_names(MANIFEST),
        ["graphhelm-protocols", "serde", "serde_json"]
    );
}

/// No source file under `src/` reads a clock, a random source, the filesystem, or spawns a
/// process.
///
/// `std::net` is checked for concrete I/O-performing types (`TcpStream`, `TcpListener`,
/// `UdpSocket`, `ToSocketAddrs`) rather than the bare `std::net` path. `manifest.rs` legitimately
/// imports `std::net::Ipv4Addr` (Task 1,
/// `docs/superpowers/plans/2026-08-14-gateway-slice.md` Task 1 Step 4) to decide whether a
/// `baseUrl` host is loopback: that is a pure value-type parse-and-compare, not a network call —
/// nothing under `src/` ever opens a socket. Banning the bare `std::net` path would fail this
/// invariant on that already-reviewed, already-tested code without catching anything real; banning
/// the concrete I/O types still catches an adapter that lands in this crate by mistake.
#[test]
fn no_source_file_performs_io_or_reads_a_clock_or_randomness() {
    for (name, source) in [
        ("lib.rs", LIB),
        ("manifest.rs", MANIFEST_SRC),
        ("taxonomy.rs", TAXONOMY),
        ("eligibility.rs", ELIGIBILITY),
        ("call.rs", CALL),
    ] {
        let code = code_only(source);
        for forbidden in [
            "std::fs",
            "std::time",
            "std::process",
            "rand",
            "TcpStream",
            "TcpListener",
            "UdpSocket",
            "ToSocketAddrs",
        ] {
            assert!(
                !code.contains(forbidden),
                "{name} must not reference {forbidden}"
            );
        }
    }
}

/// The comment filter is load-bearing, so it is itself tested. A filter that silently matched
/// nothing would make the invariant above pass for every possible source file.
#[test]
fn the_comment_filter_removes_prose_without_removing_code() {
    let sample = "// mentions rand and std::fs\nlet x = 1; // trailing\n    // indented\ncode();";
    let filtered = code_only(sample);
    assert!(!filtered.contains("mentions rand"));
    assert!(!filtered.contains("indented"));
    assert!(filtered.contains("let x = 1;"));
    assert!(filtered.contains("code();"));
}
