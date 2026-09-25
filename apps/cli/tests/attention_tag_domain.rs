//! The set of `attention` values the CLI can emit must equal the set the documentation tells
//! operators to expect.
//!
//! `QUICKSTART.md` states there is no exit-code convention and hands the reader
//! `jq -r .data.attention` for cron, so **that field is the only signal by our own design**. A
//! reader who builds a `case` from the documented list and meets an undocumented fifth value falls
//! through to whatever their default branch is — in a script we do not control and cannot fix.
//!
//! This is not a hypothetical: at `d0b3f04` the producer emitted four tags while the docs listed
//! three, and the missing one (`calmed_by_amendment`) is reachable by exactly the remedy the
//! documentation prescribes for `unknown`. Prose drifting from code is the shape of the defect, so
//! the guard has to read both and compare them.
//!
//! **Set equality, never counts.** A count kept beside the thing instead of derived from it is its
//! own defect class: it agrees with reality until someone edits one side. The assertions below
//! compare membership and name the offending tag, so a failure says which value is unlisted rather
//! than that two numbers differ.

const CLI_SOURCE: &str = include_str!("../src/commands/execution/mod.rs");
const QUICKSTART: &str = include_str!("../../../QUICKSTART.md");
const README: &str = include_str!("../../../README.md");

/// The body of `verdict_tag`, from its signature to the line that closes it.
///
/// Sliced from `fn verdict_tag` rather than from the doc comment above it, deliberately: the doc
/// comment names the tags in prose, and letting prose feed this scan would make the test agree
/// with a comment instead of with the code it is supposed to measure.
fn verdict_tag_body() -> &'static str {
    let start = CLI_SOURCE
        .find("fn verdict_tag")
        .expect("the CLI still defines verdict_tag");
    let rest = &CLI_SOURCE[start..];
    let end = rest
        .find("\n}\n")
        .expect("verdict_tag's body is closed by a brace at column zero");
    &rest[..end]
}

/// Every string literal `verdict_tag` can return.
fn emitted_tags() -> Vec<String> {
    let body = verdict_tag_body();
    let mut tags: Vec<String> = body
        .split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect();
    tags.sort();
    tags.dedup();
    tags
}

/// The one section of QUICKSTART that documents this field, from its heading to the next one.
///
/// Scoped rather than global, and the reason is measured: QUICKSTART carries a prerequisites table
/// whose first column is also a backticked lowercase token (`jq`). A whole-file scan reads that as
/// a documented `attention` value and fails for the wrong reason — a guard that trips on an
/// unrelated table teaches people to loosen it.
fn attention_section() -> &'static str {
    let start = QUICKSTART
        .find("## The answer to the question")
        .expect("QUICKSTART still has the section that documents the attention field");
    let rest = &QUICKSTART[start..];
    let end = rest[3..]
        .find("\n## ")
        .map_or(rest.len(), |offset| offset + 3);
    &rest[..end]
}

/// Every value the QUICKSTART table names, taken from the rows that lead with a backticked token.
fn documented_tags() -> Vec<String> {
    let mut tags: Vec<String> = attention_section()
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            let rest = trimmed.strip_prefix("| `")?;
            let end = rest.find('`')?;
            Some(rest[..end].to_owned())
        })
        .filter(|token| {
            token
                .chars()
                .all(|character| character.is_ascii_lowercase() || character == '_')
        })
        .collect();
    tags.sort();
    tags.dedup();
    tags
}

/// The control that proves the source scan looked at something.
///
/// Without it, a `verdict_tag` that stopped matching this slicing strategy would yield an empty
/// set, and an empty set is a subset of everything — the equality assertion below would pass while
/// measuring nothing. This is the same posture `core/execution/tests/source_invariants.rs` takes
/// with `the_source_scan_actually_finds_the_crate`.
#[test]
fn the_source_scan_finds_the_tags_it_is_supposed_to_read() {
    let emitted = emitted_tags();
    assert!(
        emitted.len() >= 2,
        "the scan of verdict_tag found {} tag(s) ({emitted:?}); a scan that finds nothing would \
         make every comparison below vacuous",
        emitted.len()
    );
    assert!(
        emitted.iter().any(|tag| tag == "needs_you"),
        "the scan did not find `needs_you`, which the CLI certainly emits: the slicing strategy \
         has stopped matching the source (found {emitted:?})"
    );
}

/// The companion control for the documentation side.
#[test]
fn the_documentation_scan_finds_the_table_it_is_supposed_to_read() {
    let documented = documented_tags();
    assert!(
        documented.iter().any(|tag| tag == "needs_you"),
        "the QUICKSTART scan did not find `needs_you` in a table row: the table's shape changed \
         and this guard is now reading nothing (found {documented:?})"
    );
}

/// The guard itself.
#[test]
fn every_emitted_attention_tag_is_documented_and_no_more() {
    let emitted = emitted_tags();
    let documented = documented_tags();

    let undocumented: Vec<&String> = emitted
        .iter()
        .filter(|tag| !documented.contains(tag))
        .collect();
    assert!(
        undocumented.is_empty(),
        "verdict_tag can emit {undocumented:?}, which QUICKSTART.md does not list. An operator \
         scripting `jq -r .data.attention` from that table meets a value the documentation says \
         cannot exist. Add a row, or stop emitting it."
    );

    let unemitted: Vec<&String> = documented
        .iter()
        .filter(|tag| !emitted.contains(tag))
        .collect();
    assert!(
        unemitted.is_empty(),
        "QUICKSTART.md documents {unemitted:?}, which verdict_tag never emits. A documented value \
         nobody can receive sends the reader looking for a state that does not occur."
    );
}

/// The contract is closed by the test above; THIS one closes the door.
///
/// `every_emitted_attention_tag_is_documented_and_no_more` reads `verdict_tag`'s body and so is
/// only ever right about `verdict_tag`. Nothing in it asserts that `verdict_tag` REMAINS THE ONLY
/// PRODUCER: a second emission path born elsewhere is outside the slice, the scan never sees it,
/// and the test stays green because the function it reads is still correct. Same shape as the
/// helper guard in #186 — it protected the helper's behaviour and not the helper's USE.
///
/// Derived, not a hand-kept list: the tags come from the same slice, so this grows by itself when
/// a fifth arm is added.
///
/// BOUND, stated because the guard is narrower than its name suggests: this reads `CLI_SOURCE`
/// only. A second producer in ANOTHER file is not covered, and closing that would mean naming
/// every file — a hand-kept list, which is the thing this whole test exists to avoid. What it does
/// buy is the likeliest case: the next `match` on `Verdict` written beside the first.
#[test]
fn verdict_tag_is_the_only_place_these_literals_are_emitted() {
    let body = verdict_tag_body();
    let outside = CLI_SOURCE.replace(body, "");
    for tag in emitted_tags() {
        let quoted = format!("\"{tag}\"");
        assert!(
            !outside.contains(&quoted),
            "the literal {quoted} occurs OUTSIDE verdict_tag's body in this file. Either a second emission path exists — in which case the domain guard above cannot see it and is no longer sufficient — or the literal is being used for something else and this test needs to say which."
        );
    }
}

/// The README carries the same list in prose rather than in a table, so it is checked for
/// membership only — a sentence has no rows to parse, and inventing a grammar for it would make
/// this guard fail on rewording rather than on drift.
///
/// ONE DIRECTION ONLY, and the name of this test promises more symmetry than it delivers: it
/// asserts EMITTED ⊆ README and never README ⊆ EMITTED. **A README that lists a value nobody emits
/// stays green here.** That is half of #189's own defect seen from the other side — this file's
/// QUICKSTART assertion says a documented value nobody can receive "sends the reader looking for a
/// state that does not occur", and that protection covers the table and NOT the prose. Left
/// uncovered deliberately rather than by oversight; parsing a sentence to close it would trade a
/// real gap for a guard that fails on rewording. (Named in review by C Agent.)
#[test]
fn the_readme_names_every_tag_the_cli_can_emit() {
    for tag in emitted_tags() {
        assert!(
            README.contains(&format!("`{tag}`")),
            "README.md does not mention `{tag}`, which the CLI can emit"
        );
    }
}
