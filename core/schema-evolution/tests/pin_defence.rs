//! The pin tripwire guards a digest. This guards the DEFENCE attached to it.
//!
//! `catalog_integrity.rs` pins the persisted-graph-version digest so that a schema edit is a
//! decision someone defends in a diff. The pin fired correctly and the suite stayed green over a
//! defence whose every identifier had been eaten by a shell — 31 passed, 0 failed, over prose with
//! its nouns removed. **The tripwire guards the digest; nothing guarded the justification.**
//!
//! A guard cannot judge whether a defence is TRUE. It can judge FORM: that something was written,
//! and that it was not gutted on the way in. Those are two different failures and each check below
//! names which one it catches, because a bar published without its blind spot reads as stronger
//! than it is.
//!
//! **This file does NOT adopt `tools/source-invariants/detect.rs`, and the reason is that file's
//! own rule.** It requires every adopter to consume every item it exports, since `include!` copies
//! the whole file and an unused item is `dead_code` under `-D warnings`. Its predicate finds runs
//! inside STRING LITERALS; the defect here is a run inside COMMENT PROSE, which that predicate
//! deliberately exempts. Adopting it to use half would break the build for this crate.

use std::path::PathBuf;

/// The pinned digest, as `catalog_integrity.rs` spells it.
///
/// This is the anchor, and it is the right one because it IS the pinned thing: if the pin moves,
/// this literal changes in the same edit that must rewrite the defence.
const PIN: &str = "sha256:1a6980bd47eeecf8eb2bb624db0a640b0d0abb5391d1a5cb4bbd39a8215dadfb";

fn tripwire_source() -> String {
    let path: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("catalog_integrity.rs");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

/// The comment block defending the pin.
///
/// **The locator carries its own guard, and that is not caution in the abstract — it is the defect
/// that arrived on my first attempt.** That version anchored on the digest literal and walked
/// straight up. The literal sits INSIDE a multi-line `assert_eq!`, so the walk hit `.unwrap()`,
/// stopped, and measured ZERO lines. It reported a defence as intact while reading nothing at all:
/// a guard that reads the wrong region and passes is the same defect one level up.
///
/// So three things are asserted rather than assumed:
///
/// * the anchor appears EXACTLY once — two pins would make "the defence" ambiguous, and zero means
///   the pin moved without this guard being updated;
/// * the walk climbs PAST the assertion to the first comment line before collecting;
/// * the block carries a `DELIBERATE (` marker. If the assertion is ever moved somewhere with no
///   defence above it, the block found is whatever comment happened to be there, and the marker is
///   what turns that into a loud failure instead of a passing measurement of the wrong lines.
fn defence_block(source: &str) -> Vec<String> {
    let lines: Vec<&str> = source.lines().collect();

    let anchors: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.contains(PIN))
        .map(|(index, _)| index)
        .collect();
    assert_eq!(
        anchors.len(),
        1,
        "HARNESS-BROKE: the pinned digest appears {} times in catalog_integrity.rs. This guard \
         locates the defence by that literal, so zero means the pin moved without updating this \
         file, and more than one makes `the defence` ambiguous",
        anchors.len()
    );

    let mut index = anchors[0];
    while index > 0 && !lines[index].trim_start().starts_with("//") {
        index -= 1;
    }
    let end = index;
    while index > 0 && lines[index - 1].trim_start().starts_with("//") {
        index -= 1;
    }

    let block: Vec<String> = lines[index..=end].iter().map(|l| (*l).to_owned()).collect();

    assert!(
        block.iter().any(|line| line.contains("DELIBERATE (")),
        "HARNESS-BROKE: the comment block above the pin carries no `DELIBERATE (` marker, so this \
         guard cannot tell a defence from whatever comment happens to sit there. Either the \
         assertion moved away from its defence, or the defence lost its marker — measuring these \
         lines anyway would be a guard reading the wrong region and passing"
    );

    block
}

/// The prose of a comment line, with the `//` and one following space removed.
fn prose(line: &str) -> &str {
    let trimmed = line.trim_start();
    let body = trimmed.strip_prefix("//").unwrap_or(trimmed);
    body.strip_prefix(' ').unwrap_or(body)
}

/// **Catches the CORRUPTION.** Not an empty defence, and not a wrong one.
///
/// Removing `` `foo` `` from *"to `foo` and"* leaves *"to  and"* — the identifier is gone and a
/// gap is left where it stood. That gap is the readable trace of exactly the accident that
/// happened: a shell whose backticks executed instead of quoting.
///
/// Measured on the real block: **zero** such lines intact, **six** after the mutilation. It does
/// not fire on the legitimate text and it fires on every corrupted line.
///
/// It is the same class as #314 one place over — a run of whitespace inside authored text — which
/// is why the tell was findable at all.
#[test]
fn the_defence_carries_no_gap_where_an_identifier_was() {
    let source = tripwire_source();
    let scarred: Vec<String> = defence_block(&source)
        .iter()
        .filter(|line| {
            let body = prose(line);
            body.split("  ").count() > 1
                && body
                    .split("  ")
                    .skip(1)
                    .any(|part| part.starts_with(|c: char| !c.is_whitespace()))
                && body.trim_end().contains("  ")
        })
        .map(|line| line.trim_start().to_owned())
        .collect();

    assert!(
        scarred.is_empty(),
        "the defence has gaps where identifiers used to be — the shape a shell leaves when its \
         backticks execute instead of quoting. This is the accident that already happened once \
         and passed 31 tests. Rewrite the affected lines with the identifiers restored:\n{}",
        scarred.join("\n")
    );
}

/// **Catches the EMPTY defence** — the failure this issue's title names.
///
/// It does NOT catch the corruption: measured, the gutted block still holds 313 words and still
/// names six schema identifiers, because the M08 paragraph writes `timeoutSeconds` without
/// backticks and carries the whole block. That is why the check above exists separately, and
/// saying so here is half of what this file is worth.
///
/// The floor is deliberately low. This is a bar against nothing-at-all, not a word quota: a
/// threshold high enough to be an opinion about writing is one somebody games with filler.
#[test]
fn the_defence_says_something_about_this_schema() {
    let source = tripwire_source();
    let block = defence_block(&source);
    let text: String = block
        .iter()
        .map(|line| prose(line))
        .collect::<Vec<_>>()
        .join(" ");

    let words = text.split_whitespace().filter(|w| !w.is_empty()).count();
    assert!(
        words >= 40,
        "the defence holds {words} words. The pin exists so a schema edit is a decision someone \
         defends in a diff; a defence this short is the empty string wearing a comment"
    );

    let schema = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../schemas/persisted-graph-version.schema.json"),
    )
    .expect("the schema the pin covers must be readable");
    let value: serde_json::Value =
        serde_json::from_str(&schema).expect("the schema the pin covers must parse");

    let mut names: Vec<String> = Vec::new();
    collect_property_names(&value, &mut names);
    assert!(
        names.len() > 10,
        "HARNESS-BROKE: only {} property names read from the schema, so the check below would be \
         nearly vacuous",
        names.len()
    );

    let named: Vec<&String> = names
        .iter()
        .filter(|name| name.len() >= 6 && text.contains(name.as_str()))
        .collect();
    assert!(
        !named.is_empty(),
        "the defence names no property of the schema it defends. It may be prose about the commit \
         rather than about the change: say which field moved. (Names shorter than six characters \
         are not counted — `to` and `type` are schema properties AND ordinary English, so they \
         would let any sentence satisfy this.)"
    );
}

fn collect_property_names(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                if key == "properties"
                    && let Some(properties) = child.as_object()
                {
                    out.extend(properties.keys().cloned());
                }
                collect_property_names(child, out);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_property_names(item, out);
            }
        }
        _ => {}
    }
}
