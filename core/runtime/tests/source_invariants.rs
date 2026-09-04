//! The boundary, pinned from both sides: this crate's dependency table is exactly the declared
//! set (async and the pure vocabularies — never an adapter, never HTTP), and `core/execution`
//! must never name this crate back (runtime-design §9's purity-leak risk as a compile-adjacent
//! test).

const MANIFEST: &str = include_str!("../Cargo.toml");
const DRIVER_CONTRACT: &str = include_str!("driver_contract.rs");

/// Every `.rs` file under `src/`, discovered by WALKING the directory.
///
/// **The population is the directory, not a list.** A hand-written list guards the file that MOVES
/// and is blind to the file that is ADDED, and nothing says so. Demonstrated before this change: a
/// probe file written to BREAK the invariant below was invisible to the hand-listed guard.
///
/// **The population stops at `src/` on purpose (#503).** Purity is a claim about the CRATE, not
/// about its tests: a test that spawns a process or speaks HTTP is doing its job, not violating
/// this invariant. This walk has already refused a widening once — #407 adopted the
/// authored-string class in `authored_string_invariants.rs` with its OWN two-root walk precisely
/// because pointing THIS `sources()` at `tests/` would turn every legitimate test subprocess and
/// HTTP call into a red, and the tempting remedy would be to weaken the purity guard to fit.
fn sources() -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let entries =
            std::fs::read_dir(dir).unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = Vec::new();
    walk(&root, &mut found);
    found.sort();
    found
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
            let name = path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            (name, text)
        })
        .collect()
}

/// The walk must reach the whole crate.
///
/// The floor is the REAL count, not a round number below it: a floor set loosely tolerates exactly
/// the silent shrinkage this guard exists to stop. **Lowering it is legitimate only alongside a
/// NAMED removal in the same change.** The landmarks are the second half -- a count can be met by
/// the wrong files.
#[test]
fn the_scan_covers_the_whole_crate() {
    let found = sources();
    assert!(
        found.len() >= 13,
        "HARNESS-BROKE: the walk found {} source files; this crate has 13. If one was \
         deleted, lower this floor in the same change that removes it and name the file here",
        found.len()
    );
    for landmark in ["lib.rs", "retrieval.rs"] {
        assert!(
            found.iter().any(|(name, _)| name == landmark),
            "HARNESS-BROKE: {landmark} is known to exist and is absent from the walk"
        );
    }
}
const EXECUTION_MANIFEST: &str = include_str!("../../execution/Cargo.toml");

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
    // 12 since #668, and the sentence this count exists to force is about a dependency LEAVING.
    //
    // `graphhelm-quality` entered at M06 Task 4 because the gate check WAS core/quality's
    // evaluators running under the runtime: `gate_check_outcome` called `evaluate_geometry`
    // directly, so geometry was not one registered gate among several, it was what a gate MEANT
    // here. #668 made the runtime dispatch through `GateRegistryPort` instead, and the binary
    // supplies the evaluators — so this crate now names no particular gate's evaluator, which is
    // the same posture it already holds toward adapters two tests below.
    //
    // It survives as a DEV-dependency, which this table cannot see by construction
    // (`production_dependencies` stops at the next `[section]` header): the gate cells still
    // choose geometry as the evaluator they wire into a test registry, and a test choosing a
    // concrete evaluator is not the crate depending on one.
    assert_eq!(count, 12, "the dependency table grew or shrank: {table}");
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
fn the_runtime_package_has_no_adapter_dependency_or_shipped_fixture_binary() {
    assert!(
        !MANIFEST.contains("graphhelm-tool-host"),
        "core/runtime must exercise adapters only through ToolPort"
    );
    assert!(
        !MANIFEST.contains("[[bin]]"),
        "a test fixture must not become an installable runtime binary"
    );
}

#[test]
fn retry_contract_tests_do_not_decide_outcomes_from_wall_clock_time() {
    assert!(
        !DRIVER_CONTRACT.contains("std::time::Instant::now()"),
        "retry outcomes must come from an explicitly controlled ToolPort"
    );
}

#[test]
fn no_source_file_spawns_processes_or_speaks_http() {
    // tokio and async are this crate's point; subprocesses and HTTP are the adapters'.
    for (name, source) in sources() {
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
