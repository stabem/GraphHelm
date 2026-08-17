//! The boundary, pinned from both sides: this crate's dependency table is exactly the declared
//! set (async and the pure vocabularies — never an adapter, never HTTP), and `core/execution`
//! must never name this crate back (runtime-design §9's purity-leak risk as a compile-adjacent
//! test).

const MANIFEST: &str = include_str!("../Cargo.toml");
const EXECUTION_MANIFEST: &str = include_str!("../../execution/Cargo.toml");
const LIB: &str = include_str!("../src/lib.rs");
const EXECUTOR: &str = include_str!("../src/executor.rs");
const PORTS: &str = include_str!("../src/ports.rs");
const PROMPT: &str = include_str!("../src/prompt.rs");
const CLASSIFY: &str = include_str!("../src/classify.rs");
const DRIVER: &str = include_str!("../src/driver.rs");
const EVIDENCE: &str = include_str!("../src/evidence.rs");

/// The `[dependencies]` table only, stopping at the next `[section]` header.
fn production_dependencies(manifest: &str) -> &str {
    let start = manifest
        .find("[dependencies]")
        .expect("a [dependencies] table");
    let rest = &manifest[start + "[dependencies]".len()..];
    match rest.find("\n[") {
        Some(end) => &rest[..end],
        None => rest,
    }
}

#[test]
fn the_runtime_crate_depends_on_exactly_the_declared_crates() {
    let table = production_dependencies(MANIFEST);
    for expected in [
        "graphhelm-events",
        "graphhelm-execution",
        "graphhelm-gateway",
        "graphhelm-protocols",
        "graphhelm-simulation",
        "graphhelm-tool-broker",
        "hex",
        "serde",
        "graphhelm-quality",
        "serde_json",
        "sha2",
        "thiserror",
        "tokio",
    ] {
        assert!(
            table.contains(expected),
            "{expected} missing from the table"
        );
    }
    let count = table.lines().filter(|line| line.contains('=')).count();
    // 13 since M06 Task 4: graphhelm-quality entered deliberately (the gate check IS
    // core/quality's evaluators running under the runtime); recorded here consciously —
    // this count exists precisely to force this sentence to be written.
    assert_eq!(count, 13, "the dependency table grew or shrank: {table}");
}

#[test]
fn the_runtime_crate_never_names_an_adapter() {
    // The ports invert the dependency; an adapter name here would flip the arrow back.
    let table = production_dependencies(MANIFEST);
    for forbidden in ["model-gateway", "tool-host", "postgres-event-store"] {
        assert!(!table.contains(forbidden), "{forbidden} must not appear");
    }
}

#[test]
fn no_source_file_spawns_processes_or_speaks_http() {
    // tokio and async are this crate's point; subprocesses and HTTP are the adapters'.
    for (name, source) in [
        ("lib.rs", LIB),
        ("executor.rs", EXECUTOR),
        ("ports.rs", PORTS),
        ("prompt.rs", PROMPT),
        ("classify.rs", CLASSIFY),
        ("driver.rs", DRIVER),
        ("evidence.rs", EVIDENCE),
    ] {
        for token in ["std::process", "ureq", "axum"] {
            assert!(!source.contains(token), "{name} must not contain {token}");
        }
    }
}

#[test]
fn core_execution_never_names_the_runtime_crate_back() {
    // The §9 risk pinned from the other side: the moment core/runtime types appear in a pure
    // crate's dependency table, the 04 property suite stops meaning anything.
    assert!(
        !EXECUTION_MANIFEST.contains("graphhelm-runtime"),
        "core/execution must never depend on core/runtime"
    );
}
