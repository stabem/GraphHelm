//! Enforces the purity boundary this crate claims for itself.
//!
//! Milestone 03's review established that a documented control which no test enforces is not a
//! control. `core/execution` documents that it has no clock, no randomness and no adapter
//! dependency; these tests make that statement fail loudly the first time it stops being true.

const MANIFEST: &str = include_str!("../Cargo.toml");

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/source-invariants/detect.rs"
));

use std::path::{Path, PathBuf};

/// The `[dependencies]` table only, stopping at the next `[section]` header.
///
/// Purity is a claim about what ships in the compiled library, not about what a test file needs
/// to compose a fixture. Task 7 (04e) added `graphhelm-simulation`, `chrono` and `tempfile` under
/// `[dev-dependencies]` so `execution_lifecycle.rs` can drive `FixtureExecutor` and a real
/// repository — none of that links into the crate a downstream consumer builds. Scoping the scan
/// to `[dependencies]` keeps the invariant meaningful instead of forbidding legitimate dev-only
/// test tooling.
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

/// The production-dependency slice reads only the `[dependencies]` table, so a dependency
/// smuggled into a table the slice never reaches would be invisible to both scans above. There is
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
    let production = production_dependencies(MANIFEST);
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
            !production.contains(forbidden),
            "core/execution must not depend on {forbidden}"
        );
    }
}

/// The narrowing above must not become a licence to depend on anything. This pins the exact set,
/// so adding a dependency is a deliberate edit to a test rather than a silent manifest change.
#[test]
fn the_execution_crate_depends_on_exactly_the_declared_crates() {
    let declared: Vec<&str> = production_dependencies(MANIFEST)
        .lines()
        .filter(|line| line.starts_with("graphhelm-"))
        .map(|line| line.split_whitespace().next().unwrap_or_default())
        .collect();
    assert_eq!(declared, ["graphhelm-protocols", "graphhelm-events"]);
}

/// Every `.rs` file under `src/`, DERIVED — never a hand-written list.
///
/// The list this replaced enumerated eight files and silently missed the ninth
/// (`attention.rs`, added in M07 and never covered). A hand-maintained roster does not
/// guard a crate; it guards the roster, and it guarantees the NEXT file escapes too. The
/// same defect family as a fixture that invents state production never writes: the check
/// measures what it was told, not what is there.
///
/// Anchored on `CARGO_MANIFEST_DIR` rather than a relative path: `include_str!` resolved at
/// compile time and never cared about the working directory, but reading a directory at
/// runtime does, and a `cd` that did not survive a backgrounded command bit this pair the
/// same day this test was written.
///
/// **The population stops at `src/` on purpose (#503).** Purity is a claim about the CRATE, not
/// about its tests: a test that reads a clock or seeds randomness is doing its job, not
/// violating this invariant. The authored-string sweep lower in this same file walks BOTH roots
/// — the two populations differ because the INVARIANTS differ, not because one walk is
/// unfinished, and a reader arriving from #438's sweeps-cover-both-roots work should widen
/// neither on the strength of the other.
fn every_source_file() -> Vec<(String, String)> {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&src).expect("the crate's src/ directory is readable") {
        let entry = entry.expect("a readable directory entry");
        let path = entry.path();
        // Refuse an unforeseen subdirectory instead of walking it silently: a module tree
        // that grows a directory is a decision, and an unreviewed decision must not inherit
        // this invariant by accident.
        assert!(
            path.is_file(),
            "unexpected subdirectory in src/: {}. Decide explicitly whether this invariant \
             covers it, then teach this test — do not let it inherit coverage silently.",
            path.display()
        );
        if path.extension().is_some_and(|extension| extension == "rs") {
            let name = path
                .file_name()
                .expect("a file name")
                .to_string_lossy()
                .into_owned();
            let source = std::fs::read_to_string(&path).expect("a readable source file");
            files.push((name, source));
        }
    }
    files.sort();
    files
}

/// Every authored Rust file in this crate, including tests.
///
/// This is separate from `every_source_file`: that older scan intentionally covers only the flat
/// production `src/` tree. This walk serves an authored-prose control, so omitting `tests/` would
/// omit the only defect population found when the control was introduced.
fn authored_rust_files() -> Vec<(String, String)> {
    const MAX_DEPTH: usize = 8;
    const MAX_ENTRIES: usize = 512;
    const MAX_FILES: usize = 128;

    fn walk(directory: &Path, depth: usize, entries_seen: &mut usize, files: &mut Vec<PathBuf>) {
        assert!(
            depth <= MAX_DEPTH,
            "authored Rust scan exceeded depth {MAX_DEPTH}"
        );
        let entries = std::fs::read_dir(directory)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", directory.display()));
        for entry in entries {
            let entry = entry.unwrap_or_else(|error| {
                panic!(
                    "cannot read an entry under {}: {error}",
                    directory.display()
                )
            });
            *entries_seen += 1;
            assert!(
                *entries_seen <= MAX_ENTRIES,
                "authored Rust scan exceeded {MAX_ENTRIES} directory entries"
            );

            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path)
                .unwrap_or_else(|error| panic!("cannot inspect {}: {error}", path.display()));
            assert!(
                !metadata.file_type().is_symlink(),
                "authored Rust scan refuses symlink: {}",
                path.display()
            );
            if metadata.is_dir() {
                walk(&path, depth + 1, entries_seen, files);
            } else {
                assert!(
                    metadata.is_file(),
                    "unexpected file type: {}",
                    path.display()
                );
                if path.extension().is_some_and(|extension| extension == "rs") {
                    files.push(path);
                    assert!(
                        files.len() <= MAX_FILES,
                        "authored Rust scan exceeded {MAX_FILES} Rust files"
                    );
                }
            }
        }
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut paths = Vec::new();
    let mut entries_seen = 0;
    walk(&root.join("src"), 0, &mut entries_seen, &mut paths);
    walk(&root.join("tests"), 0, &mut entries_seen, &mut paths);
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let shown = path
                .strip_prefix(root)
                .expect("authored Rust file remains inside the crate")
                .display()
                .to_string();
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
            (shown, source)
        })
        .collect()
}

/// The comment-filter sample below deliberately contains indented prose inside one literal.
/// Exempt that fixture by its exact role and location, never by exempting this whole file.
fn is_comment_filter_fixture(path: &str, line: &str) -> bool {
    if path.replace('\\', "/") != "tests/source_invariants.rs" {
        return false;
    }
    let expected = format!(
        "\"// mentions rand and SystemTime\\nlet x = 1; // trailing\\n{}// indented\\ncode();\";",
        " ".repeat(4)
    );
    line.trim_start() == expected
}

fn has_collapsed_authored_literal(path: &str, line: &str) -> bool {
    !is_line_comment(line) && !is_comment_filter_fixture(path, line) && has_run_in_literal(line)
}

#[test]
fn authored_strings_carry_no_collapsed_indentation() {
    let offenders: Vec<String> = authored_rust_files()
        .iter()
        .flat_map(|(path, source)| {
            source
                .lines()
                .enumerate()
                .filter(|(_, line)| has_collapsed_authored_literal(path, line))
                .map(move |(number, line)| format!("{path}:{}: {}", number + 1, line.trim_start()))
        })
        .collect();

    assert!(
        offenders.is_empty(),
        "these authored string literals contain collapsed indentation:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn authored_string_scan_covers_source_and_tests() {
    let files = authored_rust_files();
    assert!(
        files.len() >= 15,
        "authored Rust scan found only {} files: {:?}",
        files.len(),
        files.iter().map(|(path, _)| path).collect::<Vec<_>>()
    );
    let normalized: Vec<String> = files
        .iter()
        .map(|(path, _)| path.replace('\\', "/"))
        .collect();
    for required in [
        "src/lib.rs",
        "tests/attention.rs",
        "tests/source_invariants.rs",
    ] {
        assert!(
            normalized.iter().any(|path| path == required),
            "authored Rust scan must include {required}: {normalized:?}"
        );
    }
}

/// A derived scan that finds nothing would pass for every possible defect — the exact
/// failure mode that let a `git -C` check report on the parent repository and read as
/// evidence. So the derivation is itself checked before it is trusted.
#[test]
fn the_source_scan_actually_finds_the_crate() {
    let files = every_source_file();
    assert!(
        files.len() >= 9,
        "the scan found only {} files; a scan that finds nothing passes silently: {files:?}",
        files.len()
    );
    for required in ["lib.rs", "attention.rs", "transition.rs"] {
        assert!(
            files.iter().any(|(name, _)| name == required),
            "{required} must be in the derived scan: {:?}",
            files.iter().map(|(name, _)| name).collect::<Vec<_>>()
        );
    }
    assert!(
        files.iter().all(|(_, source)| !source.is_empty()),
        "an empty source file would satisfy every assertion below"
    );
}

#[test]
fn no_source_file_reads_a_clock_or_randomness() {
    for (name, source) in every_source_file() {
        let code = code_only(&source);
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
