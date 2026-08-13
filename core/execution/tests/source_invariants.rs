//! Enforces the purity boundary this crate claims for itself.
//!
//! Milestone 03's review established that a documented control which no test enforces is not a
//! control. `core/execution` documents that it has no clock, no randomness and no adapter
//! dependency; these tests make that statement fail loudly the first time it stops being true.

const MANIFEST: &str = include_str!("../Cargo.toml");
const LIB: &str = include_str!("../src/lib.rs");
const BOUNDS: &str = include_str!("../src/bounds.rs");
const DISPATCH: &str = include_str!("../src/dispatch.rs");
const TRANSITION: &str = include_str!("../src/transition.rs");
const SIGNAL: &str = include_str!("../src/signal.rs");
const READY: &str = include_str!("../src/ready.rs");
const PROGRESS: &str = include_str!("../src/progress.rs");

/// Strips line comments so prose cannot decide the outcome in either direction.
///
/// Without this, the doc comment "no clock, no randomness" would trip a search for `rand`, and the
/// test would fail for saying the right thing. Worse, someone could then weaken the token list to
/// make it pass and quietly lose the real check.
fn code_only(source: &str) -> String {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// This crate must stay pure. A clock, a random source or an *adapter* dependency would make replay
/// reproduce a different decision from the same history, which is exactly the defect class the
/// milestone-03 review had to correct twice.
///
/// Scope: this reads one manifest, so it is a **first-party** control. `graphhelm-events` links
/// `chrono`, `getrandom` and `fs2` transitively, and nothing here can see that. What the crate
/// guarantees is that its own code calls none of them — which is what the source scan below
/// enforces, and why that scan must cover every source file.
///
/// Depending on another `core` crate is not impurity and never was. The original list forbade
/// `graphhelm-events`, `graphhelm-graph` and `graphhelm-policy`, which the design explicitly says
/// this crate depends on — the invariant was over-broad, not the design.
#[test]
fn the_execution_crate_has_no_impure_dependency() {
    for forbidden in [
        "tokio",
        "sqlx",
        "chrono",
        "getrandom",
        "rand",
        "reqwest",
        "graphhelm-postgres",
        "graphhelm-sealed",
        "adapters/",
    ] {
        assert!(
            !MANIFEST.contains(forbidden),
            "core/execution must not depend on {forbidden}"
        );
    }
}

/// The narrowing above must not become a licence to depend on anything. This pins the exact set,
/// so adding a dependency is a deliberate edit to a test rather than a silent manifest change.
#[test]
fn the_execution_crate_depends_on_exactly_the_declared_crates() {
    let declared: Vec<&str> = MANIFEST
        .lines()
        .filter(|line| line.starts_with("graphhelm-"))
        .map(|line| line.split_whitespace().next().unwrap_or_default())
        .collect();
    assert_eq!(declared, ["graphhelm-protocols", "graphhelm-events"]);
}

#[test]
fn no_source_file_reads_a_clock_or_randomness() {
    for (name, source) in [
        ("lib.rs", LIB),
        ("bounds.rs", BOUNDS),
        ("dispatch.rs", DISPATCH),
        ("transition.rs", TRANSITION),
        ("signal.rs", SIGNAL),
        ("ready.rs", READY),
        ("progress.rs", PROGRESS),
    ] {
        let code = code_only(source);
        for forbidden in [
            "SystemTime",
            "Instant",
            "now()",
            "rand::",
            "thread_rng",
            "random",
            "std::fs",
            "std::net",
            "std::env",
            "File::",
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
    let sample =
        "// mentions rand and SystemTime\nlet x = 1; // trailing\n    // indented\ncode();";
    let filtered = code_only(sample);
    assert!(!filtered.contains("mentions rand"));
    assert!(!filtered.contains("indented"));
    assert!(filtered.contains("let x = 1;"));
    assert!(filtered.contains("code();"));
}
