//! #266: a `pub fn` in a test-support module with no caller has never been run by anything, and
//! it borrows the authority of the helpers beside it without any of their evidence.
//!
//! **Why the compiler cannot do this, and it is structural rather than an oversight.**
//! `adapters/postgres-event-store/tests/support/mod.rs` is compiled into **seven** separate test
//! binaries (`mod support;` in seven files). A helper used by one binary is genuinely unused in
//! the other six, so `dead_code` fires on healthy code — which is why line 1 of that module is
//! `#![allow(dead_code)]`. **That allow is load-bearing, not sloppiness**, and removing it is the
//! obvious-wrong remedy. `rustc` sees one binary at a time and cannot distinguish *"unused in this
//! binary"* from *"unused anywhere"*. Only a repository-wide sweep can, which is this file.
//!
//! **The cost is not the dead code.** `graph_published_event` sat in that module with zero callers
//! and did not pass `validate_persisted_projection` — it built a node with no content slots, which
//! is not a legal projection. Sitting beside `valid_graph_publication` (12 callers) and
//! `valid_graph_publication_for` (2 callers) made it read as "this shape is known good", a claim
//! nobody had made. Lifting it as a working example cost the #162 lane two red runs.
//!
//! **Callers are counted with comments stripped**, and that is not decoration: before it was
//! deleted, `graph_published_event`'s only other mention in the whole repository was a doc comment
//! in `core/events/tests/sweep_verb.rs` describing the defect. A sweep that counts prose as a call
//! reports the dead helper as live — measured, because the first version of this sweep did exactly
//! that and answered "zero dead helpers".

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Drops whole-line comments and keeps everything else verbatim.
///
/// LIMIT: whole-line `//` only. A trailing comment after code on the same line, a block comment,
/// or a helper's name inside a string literal would all still count as a call. Each is a false
/// NEGATIVE — it lets a dead helper pass, never fails a live one — so the sweep under-reports
/// rather than accuses. No instance of any of them exists today.
fn without_line_comments(source: &str) -> String {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The names declared `pub fn` in `module`, that no source in `sources` mentions outside the
/// module's own declaration.
///
/// A pure function over already-read text, deliberately: it makes the discriminator testable with
/// a constructed pair instead of depending on the repository happening to contain an instance.
fn dead_helpers(
    module_path: &str,
    module_text: &str,
    sources: &BTreeMap<String, String>,
) -> BTreeSet<String> {
    let declared = module_text
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("pub fn "))
        .filter_map(|rest| rest.split(['(', '<', ' ']).next())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();

    declared
        .into_iter()
        .filter(|name| {
            let declaration = format!("pub fn {name}");
            !sources.iter().any(|(path, text)| {
                let body = without_line_comments(text);
                let body = if path == module_path {
                    body.replace(&declaration, "")
                } else {
                    body
                };
                body.split(|c: char| !c.is_alphanumeric() && c != '_')
                    .any(|token| token == name)
            })
        })
        .collect()
}

fn rust_sources(root: &Path) -> BTreeMap<String, String> {
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
    for crate_root in ["adapters", "apps", "core", "tools"] {
        walk(&root.join(crate_root), &mut paths);
    }
    paths
        .into_iter()
        .filter_map(|path| {
            let text = fs::read_to_string(&path).ok()?;
            Some((path.display().to_string().replace('\\', "/"), text))
        })
        .collect()
}

/// A test-support module: a `.rs` file living in a SUBDIRECTORY of a `tests/` directory, so it is
/// shared machinery rather than a test binary of its own.
///
/// SEALED LIMIT, measured rather than assumed: `mod support;` could also resolve to a FLAT
/// `tests/support.rs`, which this filter would mistake for a test binary and skip. Every `mod X;`
/// declaration across every test binary in the repository was enumerated — there is exactly one,
/// `mod support`, and it resolves to the nested `tests/support/mod.rs` form. The flat form is
/// unusual here because cargo would also compile it as a test binary of its own. If one is ever
/// added, this discovery misses it silently, and the fix is to resolve modules from the `mod X;`
/// declarations rather than from the path shape.
fn support_modules(sources: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    sources
        .iter()
        .filter(|(path, _)| {
            let after_tests = path.split("/tests/").nth(1);
            after_tests.is_some_and(|rest| rest.contains('/'))
        })
        .map(|(path, text)| (path.clone(), text.clone()))
        .collect()
}

// Prevents a test-support helper from carrying its neighbours' authority with none of their
// evidence: no caller means nothing has ever run it.
#[test]
fn every_test_support_helper_has_a_caller() {
    let root = repository_root();
    let sources = rust_sources(&root);
    let modules = support_modules(&sources);

    // Both controls are non-empty-first. A walk that finds no modules, or modules that declare no
    // helpers, would make the sweep below iterate nothing and pass while checking nothing.
    assert!(
        !modules.is_empty(),
        "found no test-support modules at all under adapters/, apps/, core/ or tools/. A sweep \
         over an empty population passes vacuously, so this refuses instead."
    );
    let declared_total: usize = modules
        .values()
        .map(|text| text.matches("pub fn ").count())
        .sum();
    assert!(
        declared_total > 0,
        "the {} test-support module(s) declare no `pub fn` between them, so this sweep has no \
         subject: {:?}",
        modules.len(),
        modules.keys().collect::<Vec<_>>()
    );

    let dead = modules
        .iter()
        .map(|(path, text)| (path.clone(), dead_helpers(path, text, &sources)))
        .filter(|(_, names)| !names.is_empty())
        .collect::<BTreeMap<_, _>>();

    assert!(
        dead.is_empty(),
        "these test-support helpers have no caller anywhere, so nothing has ever run them:\n\
         {dead:#?}\n\n\
         A helper with zero callers sits beside exercised ones and reads as a known-good example \
         without being one. Either delete it, or give it a caller that asserts what it is for. If \
         it exists to be an INVALID fixture, say so in its name — a fixture whose invalidity is \
         deliberate and one whose invalidity is accidental are indistinguishable otherwise.\n\n\
         Note that `#![allow(dead_code)]` in a support module is load-bearing and is NOT the thing \
         to remove: the module compiles into several test binaries and rustc cannot tell \
         'unused in this binary' from 'unused anywhere'."
    );
}

// Prevents this sweep's own comment-stripping from being an untested claim.
//
// Built from a constructed pair rather than from whatever the repository happens to contain: the
// live instance that motivated the stripping was `graph_published_event`, and deleting it in this
// same commit would have taken the discriminator's only witness with it.
#[test]
fn a_helper_named_only_in_a_comment_is_still_dead() {
    let module_path = "adapters/x/tests/support/mod.rs";
    let module_text = "#![allow(dead_code)]\npub fn used_helper() {}\npub fn prose_only() {}\n";
    let sources = BTreeMap::from([
        (module_path.to_owned(), module_text.to_owned()),
        (
            "core/y/tests/consumer.rs".to_owned(),
            // `prose_only` appears here in full, and ONLY inside a comment. `used_helper` is
            // called. A sweep that counts prose as a call reports zero dead helpers.
            "//! prose_only is the fixture this note describes\nfn t() { used_helper(); }\n"
                .to_owned(),
        ),
    ]);

    assert_eq!(
        dead_helpers(module_path, module_text, &sources),
        BTreeSet::from(["prose_only".to_owned()]),
        "the comment-stripping is not discriminating: a helper whose only mention is prose must \
         still count as dead, and one that is actually called must not"
    );
}
