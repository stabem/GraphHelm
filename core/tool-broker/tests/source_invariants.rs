//! Enforces the purity boundary this crate claims for itself, mirroring
//! `core/gateway/tests/source_invariants.rs` and `core/execution/tests/source_invariants.rs`.
//!
//! `core/tool-broker/src/lib.rs` documents that this crate is "total and side-effect free: no
//! clock, no randomness, no filesystem, no network and no subprocess." A documented control which
//! no test enforces is not a control (Milestone 03's review) — these tests make that statement
//! fail loudly the first time it stops being true.

const MANIFEST: &str = include_str!("../Cargo.toml");

/// Every `.rs` file under `src/`, discovered by WALKING the directory.
///
/// **The population is the directory, not a list.** A hand-written list guards the file that MOVES
/// and is blind to the file that is ADDED, and nothing says so. That is not a prediction: this
/// crate's `mcp_capability.rs` arrived in `6b0b058` (#307) while this guard had not been touched
/// since `9d15bf4` (#47), so the invariant below simply never ran on it -- and a probe file written
/// to BREAK the invariant passed unnoticed before this change.
///
/// **The population stops at `src/` on purpose (#503).** Purity is a claim about the CRATE, not
/// about its tests: a test that touches the filesystem or reads a clock is doing its job, not
/// violating this invariant. Widening this walk to `tests/` — the natural move for a reader
/// arriving from #438's sweeps-cover-both-roots work — turns every legitimate test IO call into a
/// red.
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
/// The floor is the REAL count, not a round number below it: a floor set loosely is a floor that
/// tolerates exactly the silent shrinkage this guard exists to stop. **Lowering it is legitimate
/// only alongside a NAMED removal in the same change** -- if a file was deleted, say which.
///
/// The landmark is the second half: a count can be met by the wrong files.
#[test]
fn the_scan_covers_the_whole_crate() {
    let found = sources();
    assert!(
        found.len() >= 7,
        "HARNESS-BROKE: the walk found {} source files; this crate has 7. If one was deleted, \
         lower this floor in the same change that removes it and name the file here",
        found.len()
    );
    for landmark in ["lib.rs", "mcp_capability.rs"] {
        assert!(
            found.iter().any(|(name, _)| name == landmark),
            "HARNESS-BROKE: {landmark} is known to exist and is absent from the walk"
        );
    }
}

/// The `[dependencies]` table only, stopping at the next `[section]` header.
///
/// Same scoping rationale as `core/gateway/tests/source_invariants.rs`: purity is a claim about
/// what ships in the compiled library, not about what a test file needs to compose a fixture, so
/// `[dev-dependencies]` (`graphhelm-graph` for the tier-vocabulary alignment test, `proptest`)
/// is out of scope for this scan.
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
/// `name = { ... }` (a path dependency) or `name.workspace = true` (a workspace-managed
/// dependency) — same dual-form parsing as `core/gateway`'s equivalent helper.
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
/// subprocess dependency would let a future edit make the broker's tier decision depend on
/// anything other than the declared effect — the defect class `core/execution`'s purity tests
/// exist to catch, asserted here before `core/tool-broker` grows past the two source files
/// Task 1 shipped.
#[test]
fn the_tool_broker_crate_has_no_impure_dependency() {
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
            "core/tool-broker must not depend on {forbidden}"
        );
    }
}

/// Pins the exact production-dependency set committed alongside Task 1: `serde`, `serde_json`,
/// `sha2`, `hex` and `thiserror`, and nothing else. `sha2` and `hex` are pure functions over
/// bytes — they are the point of allowing them. Adding a dependency is a deliberate edit to this
/// test, not a silent manifest change.
#[test]
fn the_tool_broker_crate_depends_on_exactly_the_declared_crates() {
    assert_eq!(
        declared_crate_names(MANIFEST),
        [
            "graphhelm-protocols",
            "serde",
            "serde_json",
            "sha2",
            "hex",
            "thiserror"
        ]
    );
}

/// No source file under `src/` reads a clock, a random source, the environment or the
/// filesystem, opens a socket, or spawns a process. Unlike `core/gateway` (whose `manifest.rs`
/// legitimately imports `std::net::Ipv4Addr`), nothing in this crate has any business near
/// `std::net` at all, so the bare path is banned outright.
///
/// NOTE: this list covers exactly the files Task 1 shipped — later tasks extend it as they add
/// source files.
#[test]
fn no_source_file_performs_io_or_reads_a_clock_or_randomness() {
    for (name, source) in sources() {
        let code = code_only(&source);
        for forbidden in [
            "std::fs",
            "std::net",
            "std::process",
            "std::time",
            "std::env",
            "SystemTime",
            "Instant",
            "rand",
            "getrandom",
            "tokio",
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
