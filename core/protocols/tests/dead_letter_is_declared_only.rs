//! `NodeType::DeadLetter` is DECLARED, and this file is the mechanism that keeps it unproduced.
//!
//! WHAT THE VERSION PROMISE RESTS ON. `dead_letter` entered `node.schema.json` and
//! `persisted-graph-version.schema.json` while `documentVersion` stayed 1.0.0 (#412). Widening an
//! enum in a persisted schema is compatible for WRITERS and a break for READERS: a consumer holding
//! the older 1.0.0 refuses a document carrying the new member. That trade is defensible only while
//! nothing produces one, so "nothing produces one" is not a description here -- it is the premise,
//! and a premise nothing checks is a comment.
//!
//! WHY THE RELEASE GATE CANNOT OBJECT (#401, and #425 is its predicted case). The gate compares
//! `schemas/releases/1.0.0/` against the live catalog. #412 moved BOTH, so `compare_catalogs` sees
//! no change, `impacts` is empty, and `SEMVER_MISMATCH` never executes. There is no arrangement of
//! inputs that makes the gate speak about this, which is why the check has to live at the source.
//!
//! TWO PRODUCTION PATHS, AND A SOURCE SWEEP ONLY SEES ONE. Measured while writing this file:
//!
//! ```text
//! deserializing the wire name into NodeType   ->  DeadLetter        (1 test, ok)
//! the identifier in workspace Rust            ->  3 hits, 2 files
//! ```
//!
//! The document path produces the variant with ZERO mentions in Rust, and it is not a loophole
//! somebody forgot to close: `node_type_vocabularies_agree.rs` REQUIRES the authoring schema to
//! list every variant the enum can emit, so `dead_letter` is a legal value of `type` in a graph
//! document by the deliberate action of another guard. A guard that swept Rust alone would report
//! a clean workspace while a fixture two directories away authored the node.
//!
//! So there are two sweeps below, and they fail differently on purpose.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

// ------------------------------------------------------------------------------------------
// Population. Derived, never listed.
// ------------------------------------------------------------------------------------------

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the workspace root is two levels above core/protocols")
        .to_path_buf()
}

/// The crate directories, read from `Cargo.toml` at RUN time.
///
/// A guard has two hand-chosen parameters -- its POPULATION and its FORM -- and measuring one
/// leaves the other a guess. This removes the population from the guessing: a crate added to the
/// workspace is swept the day it is added, with nobody remembering to widen a list here.
fn workspace_members() -> Vec<PathBuf> {
    let root = workspace_root();
    let text = std::fs::read_to_string(root.join("Cargo.toml")).expect("workspace Cargo.toml");
    let mut inside = false;
    let mut members = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("members") {
            inside = true;
            continue;
        }
        if inside {
            if trimmed.starts_with(']') {
                break;
            }
            if let Some(name) = trimmed.split('"').nth(1) {
                members.push(root.join(name));
            }
        }
    }
    members
}

const MAX_ENTRIES: usize = 8_192;

/// Every file under `start` with one of `extensions`, skipping build and VCS trees.
///
/// `.claude` is skipped for a reason worth writing down: in the primary checkout it holds
/// `worktrees/`, which contains COMPLETE copies of this repository belonging to other sessions.
/// A walk that descended into it would sweep other people's uncommitted work and report findings
/// about files that are not in this change set at all.
fn walk(start: &Path, extensions: &[&str], out: &mut Vec<PathBuf>) {
    assert!(
        out.len() <= MAX_ENTRIES,
        "HARNESS-BROKE: the walk passed {MAX_ENTRIES} entries; it is not measuring what it claims"
    );
    let Ok(entries) = std::fs::read_dir(start) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if matches!(
                name.as_ref(),
                "target" | ".git" | ".claude" | "node_modules"
            ) {
                continue;
            }
            walk(&path, extensions, out);
        } else if extensions
            .iter()
            .any(|extension| path.extension().is_some_and(|found| found == *extension))
        {
            out.push(path);
        }
    }
}

fn relative(path: &Path) -> String {
    path.strip_prefix(workspace_root())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

// ------------------------------------------------------------------------------------------
// HALF A -- the Rust sweep.
// ------------------------------------------------------------------------------------------

/// The identifier, and NOT the identifier that merely starts with it.
///
/// `CustomsStage::DeadLettered` is a different enum in a different lane, and #288's blueprint
/// recorded the trap after counting its seven hits as if they were this variant. A sweep for the
/// bare prefix matches six files here; three of them are the customs stage, and allowlisting them
/// would put the real thing and its decoy inside the same exemption.
fn names_the_variant(line: &str) -> bool {
    const NEEDLE: &str = "DeadLetter";
    let mut rest = line;
    while let Some(at) = rest.find(NEEDLE) {
        let after = &rest[at + NEEDLE.len()..];
        let continues = after
            .chars()
            .next()
            .is_some_and(|character| character.is_alphanumeric() || character == '_');
        if !continues {
            return true;
        }
        rest = after;
    }
    false
}

/// A `//` line, doc comments included. Prose ABOUT the variant does not produce one, and counting
/// it would couple this guard to how often somebody explains the rule.
fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with("//")
}

/// Every site allowed to name the variant, and how many times.
///
/// A SET of paths would let a producer be added beside the refusal arm inside a file that is
/// already allowed. The count is the finer grain, and it is what makes the sabotage fail.
fn allowed_sites() -> BTreeMap<&'static str, usize> {
    BTreeMap::from([
        // The declaration, and the macro row that gives it a wire name.
        ("core/protocols/src/graph.rs", 2),
        // The refusal arm. That is a MATCH PATTERN, not a construction: the variant is named there
        // in order to be refused, which is the opposite of emitting it.
        ("core/runtime/src/classify.rs", 1),
        // The whole-table pin: the variant import, and its row.
        ("core/runtime/tests/gate_nodes.rs", 2),
        // This file. It scans itself deliberately, exactly as
        // `core/protocols/tests/source_invariants.rs` does: exempting the guard's own file would
        // leave a whole file unswept, and the exemption would be invisible in the failure.
        ("core/protocols/tests/dead_letter_is_declared_only.rs", 4),
    ])
}

/// The fixtures carry NO leading indentation, and that is not tidiness.
///
/// A guard's own detection fixtures are one of the data species that force a whitespace-role
/// exemption in the crates that have them, and `core/protocols/tests/source_invariants.rs`
/// currently records, as a measurement, that no such species lives in this crate. Indenting these
/// three strings the way the source lines they imitate are indented would have made that sentence
/// false and bought an exemption to carry it. The matcher never looks at leading space, so the
/// indentation was decoration -- and decoration is a poor reason to widen a guard.
#[test]
fn the_narrow_form_separates_the_variant_from_the_customs_stage() {
    assert!(names_the_variant("NodeType::DeadLetter => Err(refusal),"));
    assert!(names_the_variant("DeadLetter,"));
    assert!(
        !names_the_variant("graphhelm_protocols::CustomsStage::DeadLettered => stage,"),
        "the customs stage is a different enum in a different lane; matching it here would force \
         an exemption that hides the real thing beside its decoy"
    );
}

#[test]
fn no_rust_outside_the_declared_sites_names_the_dead_letter_variant() {
    let mut files = Vec::new();
    for member in workspace_members() {
        walk(&member, &["rs"], &mut files);
    }

    // Non-vacuity: an empty walk agrees with any allowlist by finding nothing.
    assert!(
        files.len() > 200,
        "the walk found only {} Rust files across the workspace members, so it is not covering \
         the tree it claims to cover",
        files.len()
    );

    let mut found: BTreeMap<String, usize> = BTreeMap::new();
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let hits = text
            .lines()
            .filter(|line| !is_comment(line) && names_the_variant(line))
            .count();
        if hits > 0 {
            found.insert(relative(path), hits);
        }
    }

    let expected: BTreeMap<String, usize> = allowed_sites()
        .into_iter()
        .map(|(path, count)| (path.to_owned(), count))
        .collect();

    assert_eq!(
        found, expected,
        "the workspace names NodeType::DeadLetter somewhere this file does not allow, or has \
         stopped naming it where it must. Adding a producer is a decision about the 1.0.0 node \
         schema: a document carrying dead_letter is refused by every consumer holding the older \
         1.0.0, and the release gate cannot see it because #412 moved the mirror and the live \
         catalog together. Widen this table in the same change that bumps documentVersion."
    );
}

// ------------------------------------------------------------------------------------------
// HALF B -- the document sweep, which the Rust sweep cannot do.
// ------------------------------------------------------------------------------------------

/// Every value sitting at a node-type key, at any depth.
///
/// BY ROLE, NOT BY FILENAME. In a schema the key `type` holds an OBJECT whose `enum` lists the
/// member; in a document it holds the STRING. Reading the position rather than the file is what
/// lets the schemas keep listing `dead_letter` -- which another guard requires them to do -- while
/// an authored node carrying it is still caught.
fn node_type_values(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                if (key == "type" || key == "nodeType")
                    && let Some(text) = child.as_str()
                {
                    out.push(text.to_owned());
                }
                node_type_values(child, out);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                node_type_values(item, out);
            }
        }
        _ => {}
    }
}

fn parse(path: &Path, text: &str) -> Option<serde_json::Value> {
    if path
        .extension()
        .is_some_and(|extension| extension == "json")
    {
        serde_json::from_str(text).ok()
    } else {
        serde_yaml_ng::from_str(text).ok()
    }
}

#[test]
fn the_document_extractor_reads_the_role_and_not_the_file() {
    // POSITIVE: it fires on an authored node. Without this cell a broken extractor makes the
    // sweep below green forever, which is the failure a bare zero cannot distinguish.
    let authored: serde_json::Value =
        serde_json::from_str(r#"{"spec":{"nodes":{"n1":{"type":"dead_letter","name":"x"}}}}"#)
            .expect("fixture parses");
    let mut found = Vec::new();
    node_type_values(&authored, &mut found);
    assert!(
        found.contains(&"dead_letter".to_owned()),
        "the extractor must see an authored dead_letter node"
    );

    // NEGATIVE, CARRYING THE FULL PAYLOAD. This is the schema's real shape, member included -- a
    // stripped fixture would pass for lacking the string, which proves nothing about the rule.
    let schema: serde_json::Value =
        serde_json::from_str(r#"{"properties":{"type":{"enum":["agent","tool","dead_letter"]}}}"#)
            .expect("fixture parses");
    let mut found = Vec::new();
    node_type_values(&schema, &mut found);
    assert!(
        found.is_empty(),
        "an enum MEMBER is the vocabulary, not a node; the schemas are required to list it"
    );

    // And the authoring format is YAML, which is where this nearly went wrong: a first sweep
    // written in JSON syntax found only JSON files and read that as the population.
    //
    // Written as a RAW MULTI-LINE literal rather than with `\n` escapes. YAML indentation is
    // semantic, so an escaped one-liner would carry genuine runs of spaces inside a literal and
    // trip this crate's collapsed-run guard -- for a true reason, which is the worst kind of
    // exemption to have to write. Real newlines put each level on its own physical line, where
    // the leading space is indentation rather than payload.
    let yaml: serde_json::Value = serde_yaml_ng::from_str(
        r"
spec:
  nodes:
    n1:
      type: dead_letter
",
    )
    .expect("fixture parses");
    let mut found = Vec::new();
    node_type_values(&yaml, &mut found);
    assert!(
        found.contains(&"dead_letter".to_owned()),
        "graph documents in this repository are YAML; a JSON-only sweep is blind to all of them"
    );
}

#[test]
fn no_document_in_the_repository_authors_a_dead_letter_node() {
    let mut documents = Vec::new();
    walk(&workspace_root(), &["json", "yaml", "yml"], &mut documents);

    let mut parsed = 0_usize;
    let mut offenders: BTreeSet<String> = BTreeSet::new();
    let mut vocabulary: BTreeSet<String> = BTreeSet::new();
    for path in &documents {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let Some(document) = parse(path, &text) else {
            continue;
        };
        parsed += 1;
        let mut values = Vec::new();
        node_type_values(&document, &mut values);
        for value in values {
            if value == "dead_letter" {
                offenders.insert(relative(path));
            }
            vocabulary.insert(value);
        }
    }

    // Two controls, because a zero on its own says nothing about whether anything was read.
    assert!(
        parsed > 100,
        "only {parsed} documents parsed, so the sweep is not reading the tree it claims to read"
    );
    assert!(
        vocabulary.contains("agent"),
        "the sweep found no node of type `agent` anywhere, so it is not reaching the graph \
         documents at all; a clean result would be about the walk rather than about dead_letter. \
         Found: {vocabulary:?}"
    );

    assert!(
        offenders.is_empty(),
        "these documents author a dead_letter node: {offenders:?}. The variant is DECLARED ONLY \
         (graph.rs) and refused by classify::work_kind. Authoring one makes the document illegal \
         for every consumer holding the pre-#412 1.0.0, and the release gate cannot object."
    );
}
