//! The RULE is shared and the FEED was not (#176).
//!
//! `graphhelm_execution::attention` has one definition, so nobody worries about the rule drifting.
//! What drifts is what the rule is FED: every surface assembled `AttentionInputs` itself, and a
//! surface that assembled it from a different budget source would compile without complaint and
//! disagree in silence. The symptom would be "same rule, different verdicts", which is the last
//! thing anyone looks for, because the first thing they check is whether the rule is shared — and
//! it is.
//!
//! #176 deferred the shared constructor while there were two such sites, on the ground that a
//! constructor with two documented consumers is machinery against a hunch. It named its own
//! trigger: the third. At `95a7ad9d` there were FOUR — `execution/amend.rs`, `execution/list.rs`,
//! `execution/status.rs`, `serve/monitor.rs` — so the trigger had fired and the deferral's own
//! arithmetic no longer held.
//!
//! This cell is what makes the constructor a rule rather than a habit: it fails the moment a fifth
//! surface assembles the inputs by hand instead of asking for them.

use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_SOURCE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_TRAVERSAL_DEPTH: usize = 64;
const MAX_TRAVERSAL_ENTRIES: usize = 10_000;

/// The workspace root, from this crate's manifest rather than from the current directory.
///
/// `CARGO_MANIFEST_DIR` is `apps/cli`; the workspace is two levels up. Deliberately not
/// `include_str!`: that bakes the text into the binary at compile time, so a guard written over it
/// can pass against a source file that has since changed on disk without the test being rebuilt.
/// Reading at run time asks the tree the question the tree can answer.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("apps/cli sits two levels below the workspace root")
        .to_path_buf()
}

/// Every `.rs` file under the workspace that ships — tests excluded, target excluded.
///
/// Tests are excluded on purpose and it is not laziness: a test that builds `AttentionInputs`
/// by hand is exercising the type, not feeding a production surface, and forbidding that would
/// make the fixtures unwritable without saying anything about drift between surfaces.
fn walk_source_tree(
    dir: &Path,
    out: &mut Vec<PathBuf>,
    depth: usize,
    entries_seen: &mut usize,
    max_depth: usize,
    max_entries: usize,
) {
    assert!(
        depth <= max_depth,
        "source traversal exceeded its depth bound at {dir:?}"
    );
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("source traversal could not read {dir:?}: {error}"));
    for entry in entries {
        *entries_seen += 1;
        assert!(
            *entries_seen <= max_entries,
            "source traversal exceeded its entry bound"
        );
        let entry = entry.unwrap_or_else(|error| {
            panic!("source traversal could not inspect an entry under {dir:?}: {error}")
        });
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // Do not follow directory links from an untrusted checkout. `Path::is_dir()` follows
        // links, so checking it first can recurse back into the workspace or escape it.
        let file_type = entry
            .file_type()
            .unwrap_or_else(|error| panic!("source traversal could not inspect {path:?}: {error}"));
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            if name == "target" || name == ".git" || name == "tests" || name.starts_with('.') {
                continue;
            }
            walk_source_tree(&path, out, depth + 1, entries_seen, max_depth, max_entries);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

fn production_sources(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut entries_seen = 0;
    for directory in ["apps", "core", "adapters"] {
        let path = root.join(directory);
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => panic!("source root metadata read failed for {path:?}: {error}"),
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            walk_source_tree(
                &path,
                &mut out,
                0,
                &mut entries_seen,
                MAX_TRAVERSAL_DEPTH,
                MAX_TRAVERSAL_ENTRIES,
            );
        }
    }
    out.sort();
    out
}

fn read_source_text(path: &Path) -> String {
    let before = std::fs::metadata(path)
        .unwrap_or_else(|error| panic!("source metadata read failed for {path:?}: {error}"));
    assert!(before.is_file(), "source is not a regular file: {path:?}");
    assert!(
        before.len() <= MAX_SOURCE_BYTES,
        "source exceeds its byte bound: {path:?}"
    );
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .unwrap_or_else(|error| panic!("source open failed for {path:?}: {error}"))
        .take(MAX_SOURCE_BYTES + 1)
        .read_to_end(&mut bytes)
        .unwrap_or_else(|error| panic!("source read failed for {path:?}: {error}"));
    assert!(
        bytes.len() as u64 <= MAX_SOURCE_BYTES,
        "source grew beyond its byte bound: {path:?}"
    );
    let after = std::fs::metadata(path)
        .unwrap_or_else(|error| panic!("source metadata reread failed for {path:?}: {error}"));
    assert_eq!(
        before.len(),
        after.len(),
        "source changed while being read: {path:?}"
    );
    String::from_utf8(bytes)
        .unwrap_or_else(|error| panic!("source is not UTF-8: {path:?}: {error}"))
}

/// The lines that ship, as `(1-based line number, line)`.
///
/// A `#[cfg(test)]` module under `src/` is not production, and this repository keeps large ones
/// there -- so a sweep that reads every line accuses test code (the defect I reported against
/// #1000's closed-set guard, then shipped here myself).
///
/// THE FIRST FIX WAS WORSE THAN THE DEFECT, and G measured it: it cut the file at the FIRST
/// column-zero `#[cfg(test)]` and kept nothing after. `serve/routes.rs` has FOUR of them, and the
/// first, at line 90, opens `mod off_reactor_witness` -- a small witness module that closes at
/// 108. Production continues for another 2,500 lines, including `drive` at 1609 and the very call
/// site this pull request fixes at 1682. The guard could not see 96% of that file, including its
/// own fix, and reported green.
///
/// A truncation is only sound when what it truncates runs to the end of the file, and "the test
/// module is last" is a convention rather than a rule. So this opens a REGION instead: a
/// column-zero `#[cfg(test)]` whose next line opens a module skips to that module's own
/// column-zero `}` (optionally followed by a comment) and then KEEPS SCANNING. An external
/// `mod tests;` or a module entirely on one line opens no region. A `#[cfg(test)]` on a single item opens no region --
/// only its attribute line is dropped and the item stays under the guard, which is the
/// conservative direction.
fn production_lines(text: &str) -> Vec<(usize, &str)> {
    let mut shipped = Vec::new();
    let mut lines = text.lines().enumerate().peekable();
    while let Some((index, line)) = lines.next() {
        if line != "#[cfg(test)]" {
            shipped.push((index + 1, line));
            continue;
        }
        let mut attributes = Vec::new();
        while lines
            .peek()
            .is_some_and(|(_, next)| next.trim_start().starts_with("#["))
        {
            attributes.push(lines.next().expect("peeked attribute"));
        }
        if lines.peek().is_some_and(|(_, next)| opens_a_module(next)) {
            for (_, inner) in lines.by_ref() {
                if inner.strip_prefix('}').is_some_and(|tail| {
                    let tail = tail.trim_start();
                    tail.is_empty() || tail.starts_with("//") || tail.starts_with("/*")
                }) {
                    break;
                }
            }
        } else {
            shipped.extend(
                attributes
                    .into_iter()
                    .map(|(line_number, text)| (line_number + 1, text)),
            );
        }
    }
    shipped
}

/// Recognize only a standalone inline-module header. Other shapes remain under the census.
/// A semicolon declaration, a same-line body, or braces inside comments must not open a region.
fn opens_a_module(line: &str) -> bool {
    let rest = line.trim_start();
    let rest = rest.strip_prefix("pub").map_or(rest, |after| {
        after
            .strip_prefix("(crate)")
            .or_else(|| after.strip_prefix("(super)"))
            .unwrap_or(after)
            .trim_start()
    });
    let Some(declaration) = rest.strip_prefix("mod ") else {
        return false;
    };
    let Some(name) = declaration.trim_end().strip_suffix('{') else {
        return false;
    };
    let name = name.trim();
    let name = name.strip_prefix("r#").unwrap_or(name);
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_alphabetic())
        && characters.all(|character| character == '_' || character.is_alphanumeric())
}

/// `silence_budget_seconds` as a WORD, never as a substring.
///
/// The distinction is load-bearing in both directions. A substring match accuses
/// `node_silence_budget_seconds` — a comment at `apps/cli/src/commands/execution/mod.rs:548`, a
/// different field entirely. A match that requires punctuation (`.name` or `name:`) misses
/// struct-literal field-init shorthand, which is how the third evasion of this guard was written.
/// A word boundary is the only rule that admits both real spellings and neither false one.
fn names_the_budget_field(line: &str) -> bool {
    const FIELD: &str = "silence_budget_seconds";
    let bytes = line.as_bytes();
    line.match_indices(FIELD).any(|(start, _)| {
        let before_ok = start == 0 || {
            let previous = bytes[start - 1];
            !(previous.is_ascii_alphanumeric() || previous == b'_')
        };
        let end = start + FIELD.len();
        let after_ok = end == bytes.len() || {
            let next = bytes[end];
            !(next.is_ascii_alphanumeric() || next == b'_')
        };
        before_ok && after_ok
    })
}

/// A file, its line number, and the line — enough for the failure to be actionable without a
/// second command.
fn per_field_constructions(root: &Path) -> Vec<(String, usize, String)> {
    let mut hits = Vec::new();
    for path in production_sources(root) {
        let text = read_source_text(&path);
        // The constructor itself is the one place allowed to name the fields.
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if relative == "core/execution/src/attention.rs" {
            continue;
        }
        for (line_number, line) in production_lines(&text) {
            // THREE spellings, because the first one alone was measurably evadable.
            //
            // Codex raised both evasions on the PR and I did not take them on the argument: I
            // wrote them into `list.rs` as a fifth and sixth feed and ran the guard, which
            // reported `2 passed; 0 failed` with both sitting in the tree. A matcher that a
            // reviewer can walk past in two shapes is a convention with a test attached, which
            // is the thing this file exists to replace.
            //
            // 1. The struct-literal spelling.
            let literal = line.contains("AttentionInputs {");
            // 2. An ALIASED import — `use ...AttentionInputs as Feed;` renames the type, and the
            //    construction then spells `Feed { .. }`, which check 1 cannot see. Aliasing this
            //    type in production is refused outright rather than followed: a guard that
            //    chases renames is a parser, and this is not one.
            let aliased = line.contains("AttentionInputs as ");
            // 3. The FIELD, written by name anywhere outside the constructor — which catches
            //    `default()`-then-assign, the shape that starts from a legitimate default and
            //    then supplies a private budget map. `.silence_budget_seconds` (an access) and
            //    `silence_budget_seconds:` (a literal field) are matched; the bare substring is
            //    NOT, because `node_silence_budget_seconds` appears in a comment at
            //    `apps/cli/src/commands/execution/mod.rs:548` and would be a false accusation.
            //    WIDENED after G's reproduction: matched as a WORD, so struct-literal field-init
            //    SHORTHAND (`AttentionInputs { silence_budget_seconds, .. }` -- no colon, no dot)
            //    is caught too. The word boundary is what keeps `node_silence_budget_seconds` out.
            let field = names_the_budget_field(line);
            // 4. A `type` ALIAS -- `type Feed = AttentionInputs;` renames without `use ... as`,
            //    which check 2 cannot see. G reproduced exactly this on #1013: the THIRD spelling
            //    to evade this census, which is the argument for the compiler form rather than a
            //    fourth pattern. Added because it costs one line; not claimed as the last one.
            //    Matched by the KEYWORD, not by `= AttentionInputs`: the alias is free to name the
            //    type through a path (`= graphhelm_execution::AttentionInputs;`), and my first
            //    attempt at this check missed exactly that -- the sabotage was caught by rule 3
            //    instead, which is how I found it. A check whose comment claims more than the
            //    check does is the defect this whole file is about.
            let type_aliased =
                line.trim_start().starts_with("type ") && line.contains("AttentionInputs");
            // 5. `::default()` IN PRODUCTION, which this guard used to exempt by argument.
            //    The argument was mine and it was wrong: I wrote that "a default derives from no
            //    budget source, so it cannot drift from one". True, and beside the point. A
            //    default does not ABSTAIN from the budget -- it asserts there is none, `attention`
            //    takes the `(None, measured)` arm, and the operator is told to declare a budget
            //    they have already declared (G's measurement, #1013). Six production sites shipped
            //    that verdict; they now call `for_surface`, which derives the budget from the
            //    projection every one of them already held. With none left, the exemption has
            //    nothing to defend and becomes a rule.
            let defaulted = line.contains("AttentionInputs::default()");
            // 6. THE TRAIT FORM, which rule 5 cannot see: `let inputs: AttentionInputs =
            //    Default::default();` never spells `AttentionInputs::default()` (Codex, fifth
            //    spelling on this PR). `<AttentionInputs as Default>::default()` is already caught
            //    by rule 2, which refuses `AttentionInputs as ` outright.
            //
            //    AND THIS IS WHERE A LINE-BASED CENSUS RUNS OUT. Split the same expression across
            //    two lines and rule 6 misses it. That is not a gap to patch with a seventh rule --
            //    it is the argument for the type boundary, which is recorded as a declared gap in
            //    this pull request's body with the twelve call sites that price it.
            let trait_defaulted =
                line.contains("AttentionInputs") && line.contains("Default::default()");
            if literal || aliased || type_aliased || field || defaulted || trait_defaulted {
                hits.push((relative.clone(), line_number, line.trim().to_owned()));
            }
        }
    }
    hits
}

/// The scanner reads real files and can find a real symbol — so a zero above means absence in the
/// tree, not a walker that never opened anything.
///
/// Without this, every assertion in this file is satisfiable by a `production_sources` that
/// returns an empty vector, which is the failure mode a filter-written zero always has.
/// A TEST MODULE IN THE MIDDLE MUST NOT HIDE THE REST OF THE FILE.
///
/// This is `serve/routes.rs`'s shape in miniature, and it is the receipt for a defect that shipped
/// here: the first version of this skip cut the file at the first column-zero `#[cfg(test)]`, so a
/// witness module at line 90 hid the remaining 2,500 lines -- including the call site this pull
/// request fixes. The guard reported green while blind to 96% of the file, and G found it by
/// putting a real `default()` after that module and watching nothing happen.
///
/// Driven through synthetic text rather than the tree, because the tree is currently clean: a cell
/// that can only ever read the real workspace passes by finding nothing on the day it breaks.
/// THE CLASS, NOT THE INSTANCE -- and the real tree, not a fixture.
///
/// `a_non_trailing_test_module_hides_only_itself` proves the region logic works on text I wrote.
/// It cannot notice the day the workspace stops containing files that NEED it, and on that day the
/// synthetic cell would keep passing while the region logic guarded nothing real.
///
/// Measured when this was written: TWELVE files under the walked roots carry more than one
/// column-zero `#[cfg(test)]`, and the truncation this replaced would have cut each at its first.
/// The worst was not `routes.rs`: `core/schema/src/extension.rs` opens with a `#[cfg(test)] use`
/// at line 6 of 3,197, so the old rule read six lines and reported on the file.
/// THE OTHER TWO SHAPES IN THE TREE, because one fixture calibrates on one instance.
///
/// C measured the class over exactly what `production_sources` walks: 53 production files carry a
/// column-zero `#[cfg(test)]`, 12 carry more than one, and 31 carry a single one far from the end.
/// The three shapes are not interchangeable, so each gets a fixture:
///
///   a module in the middle          `a_non_trailing_test_module_hides_only_itself`
///   ITEM attributes clustered high  here -- `sealed-key-provider/src/lib.rs` has nine positions,
///                                   eight in its first 70 lines, on items rather than modules
///   a module at the very top        here -- `core/schema/src/extension.rs` opens at line 6
#[test]
fn external_and_single_line_test_modules_do_not_hide_following_production() {
    for declaration in [
        "mod tests;",
        "pub mod tests;",
        "pub(crate) mod tests;",
        "pub(super) mod tests;",
        "mod tests; // an external module",
        "mod tests; // { is only a comment",
        "mod tests {}",
        "mod tests { fn helper() {} }",
    ] {
        let fixture = tempfile::tempdir().expect("source fixture must be creatable");
        let source_dir = fixture.path().join("apps").join("fixture").join("src");
        std::fs::create_dir_all(&source_dir).expect("source directory must be creatable");
        // The indentation inside this fixture is CONTENT -- it is Rust source being fed to the
        // census, not prose an operator reads -- so `operator_strings_carry_no_collapsed_indentation`
        // was right to flag it and wrong to be exempted. Built with `repeat` instead, which is the
        // idiom `source_invariants.rs` already uses for its own detector fixtures. No exemption is
        // added, so that guard keeps its full population over this file.
        let pad = " ".repeat(4);
        let source = format!(
            "#[cfg(test)]\n#[allow(dead_code)]\n{declaration}\nfn ships() {{\n{pad}let _ = AttentionInputs::default();\n}}\n"
        );
        std::fs::write(source_dir.join("lib.rs"), source).expect("source must be writable");
        assert_eq!(
            per_field_constructions(fixture.path()),
            vec![(
                "apps/fixture/src/lib.rs".to_owned(),
                5,
                "let _ = AttentionInputs::default();".to_owned(),
            )],
            "the full census must retain the production feed after {declaration}"
        );
    }
}

#[test]
fn a_comment_on_the_test_module_closing_brace_preserves_following_production() {
    // Indentation as content again; see the note on the fixture above.
    let pad = " ".repeat(4);
    let source = format!(
        "#[cfg(test)]\nmod tests {{\n{pad}fn helper() {{}}\n}} // test module ends here\nfn ships() {{\n{pad}let _ = AttentionInputs::default();\n}}\n"
    );
    let source = source.as_str();
    assert!(
        production_lines(source)
            .iter()
            .any(|(number, line)| *number == 6 && line.contains("AttentionInputs::default()")),
        "a closing-brace comment must not make the scanner consume the following function"
    );
}

#[test]
fn item_level_attributes_open_no_region_and_a_module_at_the_top_closes_at_its_brace() {
    // SHAPE 2: attributes on ITEMS, clustered at the top. No region may open, every item stays
    // under the guard, and the file must continue.
    let items = "#[cfg(test)]
use std::path::PathBuf;

#[cfg(test)]
static COUNTER: usize = 0;

fn ships() {
    let _ = AttentionInputs::default();
}
";
    let shipped = production_lines(items);
    assert!(
        shipped.iter().any(|(_, line)| line.contains("PathBuf")),
        "an item-level `#[cfg(test)]` must not swallow its own item: {shipped:?}"
    );
    assert!(
        shipped.iter().any(|(_, line)| line.contains("COUNTER")),
        "a second clustered item-level attribute must not swallow its item either: {shipped:?}"
    );
    let offender = shipped
        .iter()
        .find(|(_, line)| line.contains("AttentionInputs::default()"))
        .expect("the file must continue past clustered item attributes");
    assert_eq!(offender.0, 8, "line numbers must survive: {shipped:?}");

    // SHAPE 3: a test MODULE at the very top. The region closes at its own brace and the rest of
    // the file is scanned -- the shape `extension.rs` would have had if its line 6 opened a module.
    let top = "#[cfg(test)]
mod early {
    fn hidden() {
        let _ = AttentionInputs::default();
    }
}

fn ships_after() {
    let _ = AttentionInputs::default();
}
";
    let shipped = production_lines(top);
    assert!(
        !shipped.iter().any(|(_, line)| line.contains("hidden")),
        "a module at the top must still hide its own body: {shipped:?}"
    );
    let offender = shipped
        .iter()
        .find(|(_, line)| line.contains("AttentionInputs::default()"))
        .expect("a module at line 1 must not hide the whole file");
    assert_eq!(offender.0, 9, "line numbers must survive: {shipped:?}");
}

/// THE REAL `routes.rs`, SWEPT TO ITS END -- the control this guard did not have.
///
/// Everything else here proves the scanner reads A tree (`files.len() > 100`) or that the region
/// logic works on text I wrote. Nothing proved it reads a whole FILE, which is exactly how the
/// truncation shipped green: `routes.rs` alternates production, tests, production, tests, and the
/// old rule stopped at the first marker on line 90 of 2,627 -- 1,594 lines before the file's only
/// construction. This cell would have failed the day that was written.
/// THE EXEMPT FILE IS NOT UNGUARDED -- it is guarded HARDER, and this is that guard.
///
/// The sweep skips `core/execution/src/attention.rs` wholesale, and the comment saying so claimed
/// only the CONSTRUCTOR was exempt. Codex read the two against each other and was right: a helper
/// added anywhere in that module could build the struct by hand, or take its public default, and
/// the one-feed test would still pass. A file-level exemption is not a constructor-level one.
///
/// Scanning it with the general rules is the wrong repair. The field is named there five times
/// LEGITIMATELY -- the declaration, a doc line, the constructor's own initializer, and the two
/// reads inside the rule itself -- so the field rule would accuse the definition of being a caller.
/// What is illegitimate in that file is a second CONSTRUCTION, and that is what this counts.
#[test]
fn the_defining_file_holds_exactly_one_construction() {
    let root = workspace_root();
    let path = root.join("core/execution/src/attention.rs");
    let text = read_source_text(&path);
    let lines: Vec<&str> = text.lines().collect();

    // Nobody builds it by NAME in its own module -- the constructor uses `Self`.
    let by_name = lines
        .iter()
        .filter(|line| {
            let trimmed = line.trim_start();
            line.contains("AttentionInputs {")
                && !trimmed.starts_with("pub struct")
                && !trimmed.starts_with("impl ")
        })
        .count();
    assert_eq!(
        by_name, 0,
        "the defining file builds AttentionInputs by name somewhere: only `Self` inside the constructor may construct it here"
    );

    // No default, in either spelling.
    let defaulted = lines
        .iter()
        .filter(|line| {
            line.contains("AttentionInputs::default()")
                || (line.contains("AttentionInputs") && line.contains("Default::default()"))
        })
        .count();
    assert_eq!(
        defaulted, 0,
        "the defining file takes its own public default somewhere, which is the very posture the six production sites were moved off"
    );

    // EXACTLY ONE `Self {`. A second one is a second feed living inside the shared feed's own file,
    // which is precisely what the file-level exemption would otherwise hide.
    let constructions: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.trim() == "Self {")
        .map(|(index, _)| index + 1)
        .collect();
    assert_eq!(
        constructions.len(),
        1,
        "the defining file must hold exactly one construction -- the shared constructor. Found {constructions:?}. A second one is a second feed, exempted by file rather than by role."
    );

    // CONTROL: the one it holds is inside `impl AttentionInputs`, not in some other type's impl
    // that happens to live here.
    let impl_line = lines
        .iter()
        .position(|line| line.trim() == "impl AttentionInputs {")
        .expect("the constructor's impl block must be findable");
    assert!(
        constructions[0] > impl_line,
        "the single construction is not inside `impl AttentionInputs` ({constructions:?} vs impl at {impl_line}), so this cell is counting the wrong thing"
    );
}

#[test]
fn the_sweep_of_the_real_routes_file_reaches_its_own_construction() {
    let root = workspace_root();
    let path = root.join("apps/cli/src/commands/serve/routes.rs");
    let text = read_source_text(&path);
    let shipped = production_lines(&text);

    // CONTROL: this file is the one with the alternating shape. If it stops having four
    // column-zero markers, this cell is measuring a different file than the one it was written for.
    let markers = text.lines().filter(|line| *line == "#[cfg(test)]").count();
    assert!(
        markers > 1,
        "serve/routes.rs no longer carries multiple column-zero `#[cfg(test)]` ({markers}), so this cell no longer exercises the alternating shape it was written for"
    );
    assert!(
        shipped
            .iter()
            .any(|(_, line)| line.contains("AttentionInputs::for_surface")),
        "the sweep of serve/routes.rs never reached its own `for_surface` call -- the scanner is reporting on a region, not on the file"
    );
}

#[test]
fn the_tree_still_contains_files_the_region_logic_is_needed_for() {
    let root = workspace_root();
    let mut class: Vec<String> = Vec::new();
    for path in production_sources(&root) {
        let text = read_source_text(&path);
        let lines: Vec<&str> = text.lines().collect();
        let markers: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| **line == "#[cfg(test)]")
            .map(|(index, _)| index + 1)
            .collect();
        if markers.len() > 1 {
            let first = markers[0];
            class.push(format!(
                "{}: {} markers, first at {} of {} ({} lines the old truncation would have hidden)",
                path.strip_prefix(&root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/"),
                markers.len(),
                first,
                lines.len(),
                lines.len().saturating_sub(first)
            ));
        }
    }
    assert!(
        !class.is_empty(),
        "no file under the walked roots carries more than one column-zero `#[cfg(test)]`, so the region logic is exercised only by the synthetic cell above and this file no longer knows whether it guards anything real"
    );
}

#[test]
fn a_non_trailing_test_module_hides_only_itself() {
    let source = "use crate::thing;

#[cfg(test)]
#[allow(dead_code)]
mod witness {
    fn helper() {
        let _ = AttentionInputs::default();
    }
}

fn ships_after_the_module() {
    let _ = AttentionInputs::default();
}
";
    let shipped = production_lines(source);
    let numbers: Vec<usize> = shipped.iter().map(|(number, _)| *number).collect();

    // The module's own body is gone -- including a `default()` that must NOT be accused.
    assert!(
        !shipped.iter().any(|(_, line)| line.contains("helper")),
        "the witness module's body still reached the scanner: {shipped:?}"
    );
    // CONTROL, and the whole point: the file continues afterwards.
    assert!(
        shipped
            .iter()
            .any(|(_, line)| line.contains("ships_after_the_module")),
        "the scanner stopped at the test module and never saw the rest of the file: {shipped:?}"
    );
    // The line the guard must accuse, at its REAL line number -- 12 in the text above.
    let offender = shipped
        .iter()
        .find(|(_, line)| line.contains("AttentionInputs::default()"))
        .expect("the production default() after the module must survive the region skip");
    assert_eq!(
        offender.0, 12,
        "the surviving line must keep its own number or the failure names the wrong place: {shipped:?}"
    );
    // Line numbers stay honest across a skipped region rather than being re-counted.
    assert!(
        numbers.windows(2).all(|pair| pair[0] < pair[1]),
        "line numbers must stay strictly increasing: {numbers:?}"
    );
}

#[test]
fn the_scanner_reads_the_tree_it_claims_to_scan() {
    let root = workspace_root();
    let files = production_sources(&root);
    assert!(
        files.len() > 100,
        "the source walk found only {} files under apps/ core/ adapters/ — the walker, not the \
         tree, is what this file would then be measuring",
        files.len()
    );
    let mentions = files
        .iter()
        .filter(|path| read_source_text(path).contains("AttentionInputs"))
        .count();
    assert!(
        mentions > 0,
        "no production file mentions AttentionInputs at all, which contradicts the type having \
         production callers — the scanner is reading the wrong tree"
    );
}

/// One feed: no production surface assembles `AttentionInputs` field by field.
///
/// RED FIRST against the four sites this issue measured; green only once each of them asks the
/// shared constructor instead. The sabotage that proves it still bites is a fifth inline
/// construction anywhere under `apps/`, `core/` or `adapters/`.
///
/// What this does NOT assert, said plainly so nobody reads more into a green: it does not check
/// that the constructor derives budgets correctly — that is the constructor's own cell in
/// `core/execution`. This one checks that there is exactly one place where that question can be
/// asked at all.
#[test]
fn every_surface_takes_its_attention_inputs_from_the_shared_feed() {
    let root = workspace_root();
    let hits = per_field_constructions(&root);
    assert!(
        hits.is_empty(),
        "{} production site(s) still assemble AttentionInputs field by field instead of calling \
         the shared constructor, so the FEED can drift while the RULE stays shared (#176):\n{}",
        hits.len(),
        hits.iter()
            .map(|(file, line, text)| format!("  {file}:{line}  {text}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn the_source_walk_does_not_follow_directory_symlinks() {
    let root_temp = tempfile::tempdir().expect("temporary walk root must be creatable");
    let outside_temp = tempfile::tempdir().expect("temporary outside root must be creatable");
    let root = root_temp.path().join("space % & root");
    let outside = outside_temp.path().join("space % & outside");
    let nested = root.join("apps").join("nested");
    let external_link = nested.join("external");
    std::fs::create_dir_all(&nested).expect("temporary walk fixture must be creatable");
    std::fs::write(nested.join("fixture.rs"), "fn fixture() {}\n")
        .expect("temporary source fixture must be writable");
    std::fs::create_dir_all(&outside).expect("temporary outside root must be creatable");
    std::fs::write(outside.join("must-not-be-scanned.rs"), "fn hidden() {}\n")
        .expect("temporary outside marker must be writable");

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, &external_link)
            .expect("ARRANGEMENT: the Unix directory symlink must be creatable");
    }
    #[cfg(windows)]
    {
        // A junction is a Windows directory reparse point like the link the walker must reject,
        // but it does not require the administrator privilege needed by `symlink_dir`. Keep the
        // script fixed and pass paths through the environment so `%`, `&`, and spaces cannot be
        // interpreted by a command shell.
        let status = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "New-Item -ItemType Junction -Path $env:GRAPHHELM_TEST_LINK -Target $env:GRAPHHELM_TEST_TARGET -ErrorAction Stop | Out-Null",
            ])
            .env("GRAPHHELM_TEST_LINK", &external_link)
            .env("GRAPHHELM_TEST_TARGET", &outside)
            .status()
            .expect("ARRANGEMENT: PowerShell must be available for the junction fixture");
        assert!(
            status.success(),
            "ARRANGEMENT: Windows junction creation failed with {status}"
        );
    }

    let files = production_sources(&root);
    assert_eq!(files, vec![nested.join("fixture.rs")]);
}

#[test]
fn the_source_walk_does_not_follow_a_root_directory_link() {
    let root_temp = tempfile::tempdir().expect("temporary root fixture must be creatable");
    let outside_temp = tempfile::tempdir().expect("temporary outside fixture must be creatable");
    let root = root_temp.path().join("workspace");
    let outside = outside_temp.path().join("apps-target");
    std::fs::create_dir_all(&root).expect("temporary workspace must be creatable");
    std::fs::create_dir_all(&outside).expect("temporary root target must be creatable");
    std::fs::write(outside.join("escaped.rs"), "fn escaped() {}\n")
        .expect("outside marker must be writable");
    let linked_apps = root.join("apps");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &linked_apps)
        .expect("ARRANGEMENT: the Unix root directory link must be creatable");
    #[cfg(windows)]
    {
        let status = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "New-Item -ItemType Junction -Path $env:GRAPHHELM_TEST_LINK -Target $env:GRAPHHELM_TEST_TARGET -ErrorAction Stop | Out-Null",
            ])
            .env("GRAPHHELM_TEST_LINK", &linked_apps)
            .env("GRAPHHELM_TEST_TARGET", &outside)
            .status()
            .expect("ARRANGEMENT: PowerShell must be available for the root junction fixture");
        assert!(
            status.success(),
            "ARRANGEMENT: root junction creation failed with {status}"
        );
    }
    assert!(
        production_sources(&root).is_empty(),
        "a root directory link must not expose outside files"
    );
}

#[test]
fn bounded_walk_and_source_reader_refuse_small_injected_limits() {
    let fixture = tempfile::tempdir().expect("bounded walk fixture must be creatable");
    let root = fixture.path().join("apps");
    let deep = root.join("a").join("b").join("c");
    std::fs::create_dir_all(&deep).expect("deep fixture must be creatable");
    std::fs::write(deep.join("deep.rs"), "fn deep() {}\n").expect("deep marker must be writable");
    let depth_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut files = Vec::new();
        let mut seen = 0;
        walk_source_tree(&root, &mut files, 0, &mut seen, 2, MAX_TRAVERSAL_ENTRIES);
    }));
    assert!(
        depth_result.is_err(),
        "a traversal beyond the injected depth bound must refuse"
    );

    std::fs::write(root.join("one.rs"), "fn one() {}\n").expect("first marker must be writable");
    std::fs::write(root.join("two.rs"), "fn two() {}\n").expect("second marker must be writable");
    let entries_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut files = Vec::new();
        let mut seen = 0;
        walk_source_tree(&root, &mut files, 0, &mut seen, MAX_TRAVERSAL_DEPTH, 1);
    }));
    assert!(
        entries_result.is_err(),
        "a traversal beyond the injected entry bound must refuse"
    );

    let oversized = fixture.path().join("oversized.rs");
    let file = std::fs::File::create(&oversized).expect("oversized fixture must be creatable");
    file.set_len(MAX_SOURCE_BYTES + 1)
        .expect("oversized fixture must be extendible");
    let source_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        read_source_text(&oversized)
    }));
    assert!(
        source_result.is_err(),
        "a source beyond the injected byte bound must refuse"
    );
}
