//! The declared gap in `context_compiler.rs` wakes when it stops being true (#281).
//!
//! That module's header states that deltas are VERIFIED here and never APPLIED. A declared gap with
//! no guard is prose nobody re-reads: the day someone lands delta application, the paragraph keeps
//! saying the opposite and there is nothing to notice. This test makes closing the gap a deliberate
//! act — implement one of these and it fails, pointing at the sentence that has to change.
//!
//! It is a source sweep rather than a type-level check on purpose. There is nothing to name yet: the
//! gap is precisely that no such function or type exists, and a check that referred to one could not
//! compile.

use std::path::Path;

/// Every `.rs` file under this crate's `src/`, discovered by WALKING the directory.
///
/// The population is the directory rather than a list: a file ADDED to the crate must be swept, and
/// a hand-written list is silent about exactly that.
fn sources() -> Vec<(String, String)> {
    fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
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
    // ONE ROOT, deliberately, and this is the arming site rather than the firing site -- the
    // warning belongs where the edit would be made. The house convention (#403) is to walk `src/`
    // AND `tests/`, and several crates here do. **Do not widen this one.**
    //
    // This test's own source contains every form in `APPLICATION_SURFACE`, as string literals.
    // Adding `tests/` puts the guard inside its own population: it finds ITSELF, and the subject
    // check reports a declared gap that has not closed.
    //
    // Widening it fails LOUD rather than silently, which is why this is a courtesy and not a
    // guard: the control below names the carrier file, so a two-root walk hits `HARNESS-BROKE`
    // before the subject is ever reached. Measured, both before and after that control was
    // tightened. **This comment does not substitute for the control; it saves someone the trip.**
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = Vec::new();
    walk(&root, &mut found);
    found
        .into_iter()
        .map(|path| {
            let shown = path.strip_prefix(&root).map_or_else(
                |_| path.display().to_string(),
                |rel| rel.display().to_string(),
            );
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("cannot read {shown}: {e}"));
            (shown, text)
        })
        .collect()
}

/// The DECLARATION forms whose arrival means the declared gap has closed.
///
/// Each is a delta APPLICATION surface: something that consumes a delta and yields a capsule, or a
/// type representing a delta as a thing in its own right. `verify_delta_base` and `DeltaProvenance`
/// are deliberately absent — they are the base-identity half that IS implemented.
///
/// **Declaration forms rather than bare names, and that is not fastidiousness.** The first version
/// of this test swept for the bare names and went RED immediately: the declared-gap paragraph in
/// `context_compiler.rs` NAMES all four while asserting they do not exist, so **the prose falsified
/// its own claim by stating it**. Exempting that file would have been the obvious repair and the
/// wrong one — it is the single file where a real implementation would land. Matching `fn` and
/// `struct` instead keeps the whole crate scanned and lets the paragraph say what it must.
const APPLICATION_SURFACE: [&str; 5] = [
    "fn apply_delta",
    "fn compose_delta",
    "fn expand_capsule",
    "struct DeltaCapsule",
    "enum DeltaCapsule",
];

#[test]
fn the_declared_gap_still_holds_and_the_sweep_can_see() {
    let sources = sources();

    // CONTROL FIRST, and it names the FILE rather than counting matches. A count is satisfiable
    // from this test's own text: `APPLICATION_SURFACE` and the control's own search strings are
    // string literals here, so a sweep that reached `tests/` would find every form it looks for,
    // in itself, and both halves would pass for the wrong reason. Measured -- pointing the walk at
    // `tests/` did exactly that. Naming the file the declarations actually live in cannot be
    // satisfied that way.
    let carrier = sources
        .iter()
        .find(|(path, _)| path == "context_compiler.rs");
    let (_, carrier_text) = carrier.unwrap_or_else(|| {
        panic!(
            "HARNESS-BROKE: the walk did not reach context_compiler.rs, so every absence below would pass for free. The walk is broken, not the gap."
        )
    });
    assert!(
        carrier_text.contains("struct DeltaProvenance")
            && carrier_text.contains("fn verify_delta_base"),
        "HARNESS-BROKE: context_compiler.rs was read but does not contain the two declarations that are certainly in it. The sweep cannot see declaration forms, so every absence below would pass for free."
    );

    let found: Vec<String> = APPLICATION_SURFACE
        .iter()
        .flat_map(|form| {
            sources
                .iter()
                .filter(move |(_, text)| text.contains(form))
                .map(move |(path, _)| format!("`{form}` in src/{path}"))
        })
        .collect();

    assert!(
        found.is_empty(),
        "the DECLARED GAP in context_compiler.rs no longer holds: {found:?}.\nThat header says \
         deltas are verified here and never applied, and that provenance across a chain is \
         unanswered because a chain is not constructible. If application has landed, the paragraph \
         is now false — rewrite it, and write the chain-provenance cell it says could not exist."
    );
}
