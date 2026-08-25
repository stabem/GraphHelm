//! #415: every shipped extension package is named by a validating guard, and a new one is not
//! born uncovered.
//!
//! **What this does NOT do, deliberately.** It does not validate a single digest. Two guards
//! already do that over the two shipped packages —
//! `apps/cli/tests/development_package_inventory.rs` through the library
//! `validate_extension_package`, and `apps/cli/tests/jpd_plugin.rs` through the CLI binary
//! `extension validate`. A third pass over the same property would be a duplicated ORACLE, which
//! is worse than a duplicated mechanism: two copies can disagree in silence. This file asserts
//! only that a validator EXISTS per package.
//!
//! **Why the gap was invisible (#255).** Both packages are covered, by two guards in two files,
//! each naming its package by hand, in TWO DIFFERENT SPELLINGS. Nothing anywhere stated "every
//! package under `extensions/builtin/` is validated", so the coverage was complete by coincidence
//! rather than by construction, and tomorrow's third package would be born uncovered with nothing
//! going red. Enumerating by ONE spelling is also what hid `graphhelm-jpd` from the search that
//! produced #255 in the first place: a wrapper is a factory for second spellings, and a library
//! call and a binary invocation never share a token.
//!
//! **Why comments are stripped, measured rather than assumed.** The obvious version of this guard
//! — "does this package's path appear in some test file?" — is a text search, and it is satisfied
//! by a MENTION IN PROSE. That is not hypothetical here. `apps/cli/tests/extension_cli.rs`
//! contains the validating spelling (line 421, over constructed fixture packages) AND, at line
//! 1244, a whole-line comment naming `extensions/builtin/graphhelm-jpd/extension.json` while
//! describing a historical defect. Without stripping, that comment alone would credit
//! `extension_cli.rs` with covering the real jpd package — and the guard would stay GREEN even if
//! jpd's actual guard were deleted. `stripping_comments_is_load_bearing` below keeps that
//! discriminator honest instead of trusting it.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const BUILTIN_ROOT: &str = "extensions/builtin";

/// The two spellings of "this source validates an extension package".
///
/// Both are required, and that is the whole point of the pair: `validate_extension_package` is the
/// library entry point, `"extension", "validate"` is the CLI binary's argument vector. A source
/// that shells out to the binary contains no occurrence of the library symbol, so a search for
/// either one alone reports a package as uncovered when it is covered by the other.
///
/// SEALED LIMIT: a source can also validate INDIRECTLY, by calling something that validates on its
/// behalf, and no spelling of that appears in its own text. `core/extension-host/tests/staging.rs`
/// is a live instance — it points at the shipped development-contracts package and reaches the
/// validator through `graphhelm_extension_host`, naming `validate_extension_package` only in a doc
/// comment. This guard does not credit it, and that is a false NEGATIVE: the direction that
/// demands another guard be pointed at a package, never the direction that lets one ship
/// unwatched. If a package is ever covered only indirectly, the fix is to name it in a guard, not
/// to widen this list until it matches prose.
const VALIDATING_SPELLINGS: [&str; 2] =
    ["validate_extension_package", "\"extension\", \"validate\""];

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every directory under `extensions/builtin/` that carries an `extension.json`.
///
/// Read from the filesystem rather than listed here: a hand-written population is born out of
/// date, which is the defect this file exists to close rather than to repeat.
fn shipped_packages(root: &Path) -> BTreeSet<String> {
    let directory = root.join(BUILTIN_ROOT);
    let entries = fs::read_dir(&directory).unwrap_or_else(|error| {
        panic!(
            "cannot enumerate {}: {error}. Refusing rather than reporting zero packages, since an \
             unreadable directory and a directory with nothing in it produce the same empty set \
             and only one of them means 'nothing to cover'.",
            directory.display()
        )
    });
    entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().to_string_lossy().into_owned();
            directory
                .join(&name)
                .join("extension.json")
                .is_file()
                .then_some(name)
        })
        .collect()
}

/// Every `tests/*.rs` source in the workspace, with its text.
///
/// Restricted to test directories on purpose: `apps/cli/src/commands/extension.rs` contains the
/// library spelling because it IS the command, and a command is not a guard.
fn test_sources(root: &Path) -> Vec<(PathBuf, String)> {
    fn walk(directory: &Path, found: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                if name != "target" && !name.starts_with('.') {
                    walk(&path, found);
                }
            } else if name.ends_with(".rs") {
                found.push(path);
            }
        }
    }

    let mut paths = Vec::new();
    for crate_root in ["apps", "core", "tools"] {
        walk(&root.join(crate_root), &mut paths);
    }
    let own_file = Path::new(file!())
        .file_name()
        .expect("file!() always names a file");
    paths
        .into_iter()
        .filter(|path| {
            path.components()
                .any(|component| component.as_os_str() == "tests")
        })
        // This file is not a guard, and it must never be allowed to credit itself. It contains
        // both validating spellings as string CONSTANTS, so it reads as a validating source to its
        // own predicate; a package path written in code here would then satisfy the guard with
        // nothing having validated anything. Measured, not feared: the pre-check run over this
        // tree credited `shipped_package_coverage.rs` with covering jpd, purely on the strength of
        // its own doc comment.
        .filter(|path| path.file_name() != Some(own_file))
        .filter_map(|path| fs::read_to_string(&path).ok().map(|text| (path, text)))
        .collect()
}

/// Drops whole-line comments and keeps everything else verbatim.
///
/// A one-line predicate rather than an adoption of `tools/source-invariants/detect.rs`: that file
/// states its own condition for adoption — "EVERY ITEM HERE MUST BE USED BY EVERY ADOPTER",
/// because `include!` copies the whole file and the gate runs `clippy -- -D warnings`, so an
/// unused item is `dead_code` and fails the build. This file consumes one of its three items, so
/// its own rule says do not adopt it.
///
/// LIMIT, named rather than discovered later: this drops lines that BEGIN with `//`, not trailing
/// comments. A package path parked after code on the same line — `let x = 1; // extensions/…` —
/// would still be counted as coverage. No such line exists today (measured: every mention of a
/// real package path in a non-code position is a whole-line comment), and
/// `stripping_comments_is_load_bearing` fires if the one live instance of the class disappears.
/// Block comments are the same limit and are equally unhandled.
fn without_line_comments(source: &str) -> String {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Maps each shipped package to the test sources that both validate something AND name that
/// package in code.
fn guards_by_package(
    root: &Path,
    packages: &BTreeSet<String>,
    strip_comments: bool,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut found: BTreeMap<String, BTreeSet<String>> = packages
        .iter()
        .map(|package| (package.clone(), BTreeSet::new()))
        .collect();

    for (path, text) in test_sources(root) {
        let body = if strip_comments {
            without_line_comments(&text)
        } else {
            text
        };
        if !VALIDATING_SPELLINGS
            .iter()
            .any(|spelling| body.contains(spelling))
        {
            continue;
        }
        for package in packages {
            let needle = format!("{BUILTIN_ROOT}/{package}");
            if body.contains(&needle) {
                found
                    .get_mut(package)
                    .expect("the map was built from this same set")
                    .insert(path.display().to_string());
            }
        }
    }
    found
}

// Prevents a new extensions/builtin package from shipping with no guard validating it.
#[test]
fn every_shipped_package_is_named_by_a_validating_guard() {
    let root = repository_root();
    let packages = shipped_packages(&root);

    assert!(
        packages.len() >= 2,
        "expected at least the two packages that exist today, found {packages:?}. A sweep that \
         enumerates nothing passes vacuously, so this refuses instead: either the population moved \
         and this bound needs re-arguing, or the enumeration is looking in the wrong place."
    );

    let guards = guards_by_package(&root, &packages, true);
    let uncovered = guards
        .iter()
        .filter(|(_, sources)| sources.is_empty())
        .map(|(package, _)| package.as_str())
        .collect::<Vec<_>>();

    assert!(
        uncovered.is_empty(),
        "these shipped packages are validated by nothing: {uncovered:?}\n\n\
         A package under {BUILTIN_ROOT}/ must be named, in code rather than in a comment, by a \
         test that also runs one of {VALIDATING_SPELLINGS:?}. Point a guard at it — see \
         apps/cli/tests/development_package_inventory.rs for the library spelling or \
         apps/cli/tests/jpd_plugin.rs for the CLI-binary one.\n\n\
         Coverage found: {guards:#?}"
    );
}

// Prevents this file's own comment-stripping from becoming an untested claim.
#[test]
fn stripping_comments_is_load_bearing() {
    let root = repository_root();
    let packages = shipped_packages(&root);

    let stripped = guards_by_package(&root, &packages, true);
    let raw = guards_by_package(&root, &packages, false);

    let credited_only_by_prose = packages
        .iter()
        .filter_map(|package| {
            let extra = raw[package]
                .difference(&stripped[package])
                .cloned()
                .collect::<BTreeSet<_>>();
            (!extra.is_empty()).then(|| (package.clone(), extra))
        })
        .collect::<BTreeMap<_, _>>();

    assert!(
        !credited_only_by_prose.is_empty(),
        "no package is credited by a COMMENT any more, so the comment-stripping in \
         `without_line_comments` is no longer exercised by anything and the guard above is \
         untested in that dimension. Today the live instance is apps/cli/tests/extension_cli.rs, \
         which validates fixture packages and mentions the real jpd package only in a whole-line \
         comment at line 1244. If that comment was removed on purpose, either find another \
         instance of the class or delete this test and say in the same commit that the \
         discriminator now rests on reading rather than measurement."
    );
}
