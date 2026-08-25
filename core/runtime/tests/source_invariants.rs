//! The boundary, pinned from both sides: this crate's dependency table is exactly the declared
//! set (async and the pure vocabularies — never an adapter, never HTTP), and `core/execution`
//! must never name this crate back (runtime-design §9's purity-leak risk as a compile-adjacent
//! test).

const MANIFEST: &str = include_str!("../Cargo.toml");

/// Every `.rs` file under `src/`, discovered by WALKING the directory.
///
/// **The population is the directory, not a list.** A hand-written list guards the file that MOVES
/// and is blind to the file that is ADDED, and nothing says so. Demonstrated before this change: a
/// probe file written to BREAK the invariant below was invisible to the hand-listed guard.
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
