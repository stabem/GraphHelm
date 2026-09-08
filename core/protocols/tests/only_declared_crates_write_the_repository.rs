//! The crates that append to the repository are a CLOSED LIST, and a sixth one must say so (#820).
//!
//! WHAT THIS PROTECTS, AND WHY IT IS NOT ABOUT APPENDING. Three readers in `apps/cli` locate a
//! stream by id alone and take the first match (`serve/monitor.rs`, `serve/wake.rs` twice). They are
//! correct only because no two streams can share a `stream_id`, and that holds only because every
//! production write DERIVES its scope from the id rather than choosing one: `(scope, stream_id)`
//! then carries no more information than `stream_id`.
//!
//! `apps/cli/tests/source_invariants.rs` already guards the construction half **inside `apps/cli`**
//! -- `user_scoped_writer_offenders` and `no_production_repository_scope_spells_the_addressing_rule_by_hand`,
//! both from #942, both carrying #820's number. This file is the half that walk cannot reach, and it
//! is deliberately a different question.
//!
//! **WHY THE QUESTION HERE IS "WHICH CRATES", NOT "WHICH SITES".** Measured on `origin/main`, with
//! the `#[cfg(test)]` boundary read per file rather than guessed from the path:
//!
//! ```text
//! core/events        cfg(test) at :315 of 8301   0 constructions before it, 6 after
//! core/simulation    cfg(test) at :545 of  677   0 before, 1 after
//! core/governor      no construction at all
//! core/runtime       no construction at all
//! ```
//!
//! **Outside `apps/cli`, production code never constructs a scope -- it receives one.** So porting
//! the construction cells to those crates asserts something vacuously true, and an absence guard
//! that cannot fire is the kind this board keeps having to retire. What is NOT true today, and is
//! what nothing watches, is that the list of crates able to write is fixed: the addressing invariant
//! has only ever been checked for these five, and a sixth writer is a new place a scope could be
//! chosen with nothing to notice.
//!
//! **THE ALLOW-LIST IS THE GUARDED THING.** The population is derived from `Cargo.toml` so the SCAN
//! cannot silently miss a crate; only `declared_writers` is hand-written, and that is the point --
//! it fails until a human adds the crate deliberately, which is the moment to check the new writer
//! against the addressing rule.
//!
//! WHAT THIS CANNOT SEE, and it is the same residual the sibling sweeps carry: a write reached
//! through a trait object or a re-export names none of these spellings. Nor does a module pulled
//! in from outside `src/` via `#[path = "../writer.rs"] mod writer;` -- the file compiles into the
//! crate like any other, but this sweep only reads what its filesystem walk visits, and that walk
//! never leaves `src/` (Codex, on this PR). Nor does a Cargo.toml target header or path spelled
//! differently than this text-only check reads it: a unicode escape inside the path string
//! (`"src/../writer.rs"`) hides a `..` from `escapes_src`; an explicit `[lib]`/`[[bin]]` target
//! with a non-`.rs` extension is legal Cargo and stays under `src/`, but `walk_all`'s own
//! extension filter never visits it; and a header spelled `[ lib ]` or `[ "lib" ]` -- both valid
//! TOML, both recognized by `cargo metadata` -- is not recognized by the exact `header == "[lib]"`
//! comparison, so its target is read as unrelated rather than as production (Codex, on this PR;
//! confirmed no member in this workspace uses any of the three shapes today). All three would need
//! a real TOML parser or `cargo metadata` to close, which this sweep deliberately is not. That is
//! why the append BOUNDARY remains the
//! better observer and this is second-best -- stated here rather than discovered by someone trusting a
//! green.

use std::collections::{BTreeMap, BTreeSet};

// ------------------------------------------------------------------------------------------
// Shared with the other workspace sweeps, because two sweeps that must agree on what they SKIP
// are a duplicated ORACLE and not a duplicated mechanism.
// ------------------------------------------------------------------------------------------

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/workspace-walk/walk.rs"
));

/// The three spellings of an append, and the reason there are exactly three.
///
/// MEASURED across `apps/cli/src` when #942 widened the matcher it shares this population with:
/// `PreparedAppend::new(` 19 sites, `.append_atomic(` 19, `append_event(` 11 -- and that third one
/// was invisible to the first version of that guard for a day. `.append(` is NOT here: three hits,
/// all `OpenOptions::append(true)`, which is file I/O and correctly out.
///
/// Kept as a LINE predicate rather than a text one so a comment can be excluded per line: a file
/// that documents the rule must not redden the guard that enforces it.
///
/// **THE BARE IDENTIFIER, because a generic parameter sits between the name and the paren.**
/// The first version of this file matched `append_atomic(` and found **zero** hits in
/// `adapters/postgres-event-store/src/lib.rs:344`, which reads `fn append_atomic<'a>(` -- an
/// `AsyncEventRepository` implementation forwarding to `journal::append`. A closed list that
/// silently omits an existing writer is worse than no list, because it reports the omission as
/// agreement (Codex, on this PR). The trait's own declarations carry the same shape
/// (`core/events/src/repository.rs:183`).
fn names_an_append(line: &str) -> bool {
    ["append_atomic", "append_event", "PreparedAppend::new"]
        .iter()
        .any(|needle| names_identifier(line, needle))
}

/// `needle` as a whole identifier: neither side may continue it.
///
/// Without the leading check `journal_append_event` matches `append_event`; without the trailing
/// one `append_atomic_retry` matches `append_atomic`. Same form the sibling sweeps use for the same
/// reason -- see `names_the_kind` in `countersign_append_is_declared_only.rs`.
fn names_identifier(line: &str, needle: &str) -> bool {
    let continues = |character: char| character.is_alphanumeric() || character == '_';
    let mut rest = line;
    while let Some(at) = rest.find(needle) {
        let before_ok = rest[..at].chars().next_back().is_none_or(|c| !continues(c));
        let after = &rest[at + needle.len()..];
        let after_ok = after.chars().next().is_none_or(|c| !continues(c));
        if before_ok && after_ok {
            return true;
        }
        rest = after;
    }
    false
}

/// A `//` line, doc comments included -- see `names_an_append`.
fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with("//")
}

// WHY THERE IS NO `#[cfg(test)]` HANDLING HERE, and why removing it made this guard SOUND.
//
// Earlier versions of this file tried to read only production code: first truncating at the first
// `#[cfg(test)]`, then at the first test MODULE, then removing each module span by counting braces.
// Each version was wrong, and the third was wrong in the dangerous direction -- an unmatched `{`
// inside a string literal in a test module leaves the depth above zero, so the removal runs to EOF
// and every production append after it disappears from the scan. A guard that can silently drop
// its subject is worse than one that over-reports (Codex, on this PR; five of its findings were
// about this one function).
//
// MEASURED, which is what settled it: counting appends everywhere under `src/` -- test modules
// included -- produces the SAME SIX CRATES as the careful version.
//
// adapters/postgres-event-store 1 · apps/cli 55 · core/events 51
// core/governor 7 · core/runtime 6 · core/simulation 3
//
// That is the whole argument. At the FILE grain the distinction matters; at the CRATE grain it does
// not, because a crate whose tests append also ships code that appends. So the lexing was buying an
// answer it already had, at the price of three ways to be silently wrong.
//
// The residual is stated and it is the safe direction: a crate whose ONLY appends were in fixtures
// would appear here and cost one row and a question. None does today.

/// The workspace-relative member roots (e.g. `"core/events"`), derived once from the manifest so
/// `crate_of` resolves ownership against the ACTUAL member list rather than guessing from path
/// shape.
///
/// `crate_of`'s first version used `path.find("/src/")`, which returns the FIRST occurrence -- a
/// member nested inside another member's `src/` (e.g. a hypothetical `core/events/src/plugin`)
/// would misattribute every one of its appends to the outer crate, which is already declared, so
/// the new writer would be silently absorbed rather than reported (Codex, on this PR). No member
/// nests today, so this is a guard against the shape rather than a fix for a live miscount --
/// stated rather than left to be discovered the day one does.
fn member_roots() -> Vec<String> {
    workspace_members()
        .into_iter()
        .map(|member| relative(&member))
        .collect()
}

/// The remainder of a trimmed manifest line after a `path` key, bare or quoted with either TOML
/// quote character.
///
/// TOML accepts `"path" = "..."` and `'path' = '...'` as well as the bare `path = "..."`, and
/// cargo treats all three identically -- a manifest that quotes the key is not malformed, so a
/// parser that only recognizes the bare spelling (or only one quote character) silently drops the
/// target rather than refusing it (Codex, on this PR -- twice, once per quote character).
fn after_path_key(trimmed: &str) -> Option<&str> {
    trimmed.strip_prefix("path").or_else(|| {
        ['"', '\''].iter().find_map(|quote| {
            trimmed
                .strip_prefix(*quote)
                .and_then(|rest| rest.strip_prefix("path"))
                .and_then(|rest| rest.strip_prefix(*quote))
        })
    })
}

/// Whether a target string that starts with `"src/"` LEXICALLY resolves outside `src/` once its
/// `..` components are walked -- `cargo metadata` resolves `"src/../writer.rs"` to the member
/// root, not to `src/`, so the caller's plain `starts_with("src/")` accepts a target the walk
/// never visits (Codex, on this PR). No filesystem access: the manifest is read before any walk,
/// so this has to work from the string alone.
fn escapes_src(target: &str) -> bool {
    let mut depth: i32 = 0;
    for component in target.split('/').skip(1) {
        match component {
            "" | "." => {}
            ".." => {
                depth -= 1;
                if depth < 0 {
                    return true;
                }
            }
            _ => depth += 1,
        }
    }
    false
}

/// `core/events/src/local.rs` -> `core/events`, by the LONGEST member root that owns the path --
/// see `member_roots` for why the first `/src/` in the string is not enough.
fn crate_of(path: &str, members: &[String]) -> Option<String> {
    members
        .iter()
        .filter(|member| {
            path.strip_prefix(member.as_str())
                .is_some_and(|rest| rest.starts_with("/src/"))
        })
        .max_by_key(|member| member.len())
        .cloned()
}

/// The crates whose production code may append to the repository `serve` reads.
///
/// EARNED ONE AT A TIME, and each row says what makes it safe under the addressing rule:
///
/// - `apps/cli` -- constructs scopes, and is the crate the two construction guards in
///   `apps/cli/tests/source_invariants.rs` already cover. It is on this list because it writes, not
///   because it is exempt.
/// - `core/events` -- the store itself. Its production appends take the scope they are handed.
/// - `core/governor`, `core/runtime`, `core/simulation` -- take the scope from a parameter or a
///   struct field; measured, zero production constructions in any of them.
/// - `adapters/postgres-event-store` -- `src/lib.rs:344` implements `AsyncEventRepository` and
///   forwards to `journal::append`. **It was missing from the first version of this list**, because
///   the needle then required `append_atomic(` and this one reads `append_atomic<'"'"'a>(`. A closed
///   list is worth exactly what the sweep that fills it is worth, and one that omits a writer
///   reports the omission as agreement.
///
/// **A crate arriving here is not a formality.** It means a new place a scope could be chosen, and
/// the question to answer before adding the row is the one the three id-only `find`s depend on:
/// does every write in it derive its scope from the stream id, or does something choose one?
fn declared_writers() -> BTreeSet<&'static str> {
    BTreeSet::from([
        "adapters/postgres-event-store",
        "apps/cli",
        "core/events",
        "core/governor",
        "core/runtime",
        "core/simulation",
    ])
}

/// Which crates append, over sources given as `(relative path, text)`.
///
/// Takes its population as an argument so the canary below can hand it a crate that is not in the
/// tree. A guard whose input can only ever be the real workspace passes by finding nothing on the
/// day its matcher breaks, and nothing distinguishes that from a clean tree.
///
/// `members` is likewise taken as an argument rather than derived here, for the same reason: the
/// canary's synthetic crate is not a real workspace member, and `crate_of` now resolves ownership
/// against exactly the list it is given.
fn appending_crates(sources: &[(String, String)], members: &[String]) -> BTreeMap<String, usize> {
    let mut found: BTreeMap<String, usize> = BTreeMap::new();
    for (path, text) in sources {
        let Some(owner) = crate_of(path, members) else {
            continue;
        };
        let hits = text
            .lines()
            .filter(|line| !is_comment(line) && names_an_append(line))
            .count();
        if hits > 0 {
            *found.entry(owner).or_insert(0) += hits;
        }
    }
    found
}

#[test]
fn the_form_matches_the_three_spellings_and_leaves_their_siblings_alone() {
    assert!(names_an_append(
        "        repository.append_atomic(&request)?;"
    ));
    assert!(names_an_append(
        "    let prepared = PreparedAppend::new(scope.clone(), events);"
    ));
    assert!(
        names_an_append("            append_event(store, &scope, &stream, event)?;"),
        "the helper spelling was invisible to the first version of the sibling guard for a day"
    );
    assert!(
        names_an_append("    fn append_atomic<'a>("),
        "a generic parameter sits between the name and the paren in the postgres adapter, and the \
         first version of this guard scored that file zero"
    );
    assert!(
        !names_an_append("    journal_append_event_count += 1;"),
        "an identifier that CONTAINS the needle is not the needle; without the leading boundary \
         this reads as an append"
    );
    assert!(
        !names_an_append("    append_atomic_retry(store)?;"),
        "and without the trailing boundary, so does this"
    );
    assert!(
        !names_an_append("    let file = OpenOptions::new().append(true).open(path)?;"),
        "file I/O is not a repository append, and matching it would put an unrelated crate on the \
         list every time someone opens a log"
    );
    assert!(
        !is_comment("repository.append_atomic(&request)?;"),
        "a real call must not be read as a comment"
    );
    assert!(
        is_comment("        // append_atomic( is named here to explain the rule"),
        "documenting the rule must never redden the guard that enforces it"
    );
}

#[test]
fn after_path_key_accepts_the_bare_and_the_quoted_spelling() {
    assert_eq!(
        after_path_key("path = \"src/lib.rs\""),
        Some(" = \"src/lib.rs\"")
    );
    assert_eq!(
        after_path_key("\"path\" = \"src/lib.rs\""),
        Some(" = \"src/lib.rs\"")
    );
    assert_eq!(
        after_path_key("'path' = 'src/lib.rs'"),
        Some(" = 'src/lib.rs'")
    );
    // THE MISMATCHED PAIR, which is refused BY CONSTRUCTION and had nothing pinning it: the
    // `find_map` spends the same quote character on both sides. Simplifying the second
    // `strip_prefix(*quote)` to `strip_prefix(['"', '\''])` passes every other assertion here,
    // because every other one uses a matched pair. `"path' = 'x'` is not valid TOML, so the
    // failure direction is benign -- which is exactly why it costs nothing to pin now.
    assert_eq!(
        after_path_key("\"path' = 'x'"),
        None,
        "a key opened with one quote character and closed with the other is not a `path` key"
    );
    assert_eq!(after_path_key("name = \"graphhelm-cli\""), None);
    assert_eq!(
        after_path_key("pathological = \"x\""),
        Some("ological = \"x\"")
    );
}

#[test]
fn escapes_src_catches_a_lexical_parent_reference_that_starts_with_src() {
    assert!(
        escapes_src("src/../writer.rs"),
        "cargo resolves this to the member root, not to src/ -- the walk never reaches it"
    );
    assert!(
        !escapes_src("src/foo/../bar.rs"),
        "src/foo/../bar.rs normalizes to src/bar.rs, which stays under src/"
    );
    assert!(!escapes_src("src/lib.rs"));
    assert!(!escapes_src("src/commands/execution/mod.rs"));
}

#[test]
fn crate_of_matches_the_longest_member_root_not_the_first_slash_src_slash() {
    let members = vec!["core/events".to_owned(), "core/events/plugin".to_owned()];
    assert_eq!(
        crate_of("core/events/plugin/src/lib.rs", &members),
        Some("core/events/plugin".to_owned()),
        "a member nested inside another member's tree must own its own sites, not the outer crate \
         -- `.find(\"/src/\")` would stop at the first match and misattribute this to core/events"
    );
    assert_eq!(
        crate_of("core/events/src/local.rs", &members),
        Some("core/events".to_owned())
    );
    assert_eq!(crate_of("tools/scratch/src/lib.rs", &members), None);
}

/// The walk root is `src/`, and that is an ASSUMPTION about every member's layout -- so it is
/// guarded rather than trusted.
///
/// Three members already declare explicit target paths (`apps/cli`, `tools/development-benchmark`,
/// `adapters/codebase-memory-mcp`) and every one of them points inside `src/`. A member that moved a
/// `[lib]` or `[[bin]]` target elsewhere would put production code outside this sweep's reach, and
/// the file-count floor would not notice because the other members still supply the count
/// (Codex, on this PR).
/// A declared target path in the ONE form the comparisons below are written against.
///
/// Two normalizations, both of them the difference between reading the path cargo resolves and
/// reading the characters someone typed:
///
/// - **Separators.** `src/..\writer.rs` is a valid TOML literal string and a path cargo resolves to
///   the member root. Split on `/` alone, its second component is the single name `..\writer.rs`,
///   which is neither `..` nor `.` and therefore counts as a step DOWN -- so the escape reads as
///   conforming. The mirror is `src\lib.rs`, a conforming target that fails `starts_with("src/")`.
/// - **`.` components.** `./src/lib.rs` and `src/./lib.rs` are `src/lib.rs`. `escapes_src` already
///   treats `.` as a no-op; the caller's `starts_with("src/")` does not, and it is the caller that
///   decides.
///
/// `..` is deliberately NOT resolved here: walking it is `escapes_src`'s job, and a normalizer that
/// collapsed `src/../writer.rs` to `writer.rs` would answer the escape question by accident, in a
/// function whose name promises only a spelling change.
fn normalized_target(target: &str) -> String {
    let one_separator = target.replace('\\', "/");
    one_separator
        .split('/')
        .filter(|component| !component.is_empty() && *component != ".")
        .collect::<Vec<&str>>()
        .join("/")
}

/// Whether any DIRECTORY on the way to a target is one `walk_bounded` refuses to enter.
///
/// The walk skips `WORKSPACE_WALK_SKIPPED` by directory name at any depth, so `src/target/writer.rs`
/// starts with `src/`, escapes nothing, and is still never read -- the file check passes and the
/// sweep's own claim quietly stops covering it.
///
/// The LAST component is dropped before the comparison because the walk skips directories, not
/// files: `src/target.rs` is a source file whose stem happens to match, and it is read like any
/// other.
fn skipped_by_the_walk(normalized: &str) -> bool {
    let mut components: Vec<&str> = normalized.split('/').collect();
    components.pop();
    components
        .iter()
        .any(|component| WORKSPACE_WALK_SKIPPED.contains(component))
}

/// The production targets ONE manifest declares that this sweep's walk does not reach, as written.
///
/// Split out from the cell below so the DECISION can be driven with a manifest a test composes.
/// Every manifest on disk is conforming today, so a green in that cell says only that this
/// predicate stayed quiet -- which is also what a predicate that stopped working says.
fn unreachable_targets(text: &str) -> Vec<String> {
    let mut unreachable = Vec::new();
    // SECTION-AWARE, because `[[test]]` and `[[bench]]` declare paths under `tests/` and
    // `benches/` legitimately -- adapters/postgres-event-store has two, and the first version of
    // this cell reported both as production escapes. Only `[lib]` and `[[bin]]` build the code
    // this sweep is about.
    let mut in_production_target = false;
    for line in text.lines() {
        let trimmed = line.trim();
        // THE COMMENT IS STRIPPED FIRST. `[lib] # primary library` is a valid header, and an
        // exact comparison reads it as "not a production target" -- which turns the guard off
        // for exactly the manifest that bothered to annotate itself (Codex, on this PR).
        let header = trimmed.split('#').next().unwrap_or("").trim();
        if header.starts_with('[') {
            in_production_target = header == "[lib]" || header == "[[bin]]";
            continue;
        }
        if !in_production_target {
            continue;
        }
        let Some(rest) = after_path_key(trimmed) else {
            continue;
        };
        let Some(value) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        let target = value.trim().trim_matches(['"', '\'']);
        // REPORTED AS WRITTEN, decided on the normalized form: the reader has to find the line in
        // the manifest, and the normalized spelling is not in it.
        let normalized = normalized_target(target);
        if !normalized.is_empty()
            && (!normalized.starts_with("src/")
                || escapes_src(&normalized)
                || skipped_by_the_walk(&normalized))
        {
            unreachable.push(target.to_owned());
        }
    }
    unreachable
}

#[test]
fn every_declared_target_path_is_under_src() {
    let mut manifests = Vec::new();
    for member in workspace_members() {
        manifests.push(member.join("Cargo.toml"));
    }
    assert!(
        manifests.len() > 5,
        "only {} member manifests found, so this cell is not covering the workspace",
        manifests.len()
    );

    let mut outside: Vec<String> = Vec::new();
    for manifest in &manifests {
        let Ok(text) = std::fs::read_to_string(manifest) else {
            continue;
        };
        for target in unreachable_targets(&text) {
            outside.push(format!("{}: {}", relative(manifest), target));
        }
    }

    assert!(
        outside.is_empty(),
        "a workspace member declares a Rust target outside src/, which this sweep's walk root does \
         not reach: {outside:?}\n\n\
         Either move the target under src/, or widen the roots in \
         only_declared_crates_append_to_the_repository to cover it. Leaving it means a crate can \
         write from a directory the closed-list observer never reads."
    );
}

/// A `[lib]` target whose path is spelled with the OTHER separator is the same target to cargo.
///
/// `'src/..\writer.rs'` is a valid TOML literal string and a path cargo resolves to the member
/// root, exactly like `src/../writer.rs`. Read one component at a time by splitting on `/` alone,
/// its second component is the single name `..\writer.rs`, which matches neither `".."` nor `"."`
/// and so counts as a step DOWN: the target reads as staying under `src/` and the manifest passes.
/// A false green, on the platform this workspace is developed on.
///
/// The mirror in the same half is `src\lib.rs`, all separators the other way: a conforming target
/// that fails `starts_with("src/")` and is reported as an escape it is not.
#[test]
fn a_target_spelled_with_a_windows_separator_is_read_as_the_path_cargo_resolves() {
    assert_eq!(
        unreachable_targets("[lib]\npath = 'src/..\\writer.rs'\n"),
        vec!["src/..\\writer.rs".to_owned()],
        "src/..\\writer.rs leaves src/ exactly as src/../writer.rs does, and cargo resolves both \
         to the member root -- read with `/` as the only separator it reads as a step down and the \
         walk never visits it"
    );
    // BOTH LEGAL SPELLINGS, because the subject is the raw line and not parsed TOML. A basic
    // string cannot carry a lone `\` -- `"src\..\writer.rs"` is an invalid escape and cargo
    // refuses the manifest -- so the two forms that reach a real build are the literal string
    // above (ONE `\` in the file) and the escaped basic string below (TWO `\` characters in the
    // file). A normalization that replaced the SEQUENCE `\\` rather than each character would
    // leave one of them alive, and a cell covering only the literal string would not notice.
    assert_eq!(
        unreachable_targets("[lib]\npath = \"src\\\\..\\\\writer.rs\"\n"),
        vec!["src\\\\..\\\\writer.rs".to_owned()],
        "the basic-string spelling carries two backslash characters per separator, and it is the \
         same escaping target as the literal-string form"
    );
    assert_eq!(
        unreachable_targets("[lib]\npath = 'src\\lib.rs'\n"),
        Vec::<String>::new(),
        "src\\lib.rs is src/lib.rs, which the walk does reach -- reporting it is a false red"
    );
    assert_eq!(
        unreachable_targets("[lib]\npath = \"src/writer.rs\"\n"),
        Vec::<String>::new(),
        "the ordinary spelling must stay reachable, or this cell would pass on a predicate that \
         reports everything"
    );
}

/// `./src/lib.rs` is `src/lib.rs`, and cargo builds it either way.
///
/// The decision that rejects it is NOT `escapes_src`, which walks the components and correctly
/// answers `false`. It is the caller's `starts_with("src/")`, which reads the leading `.` as the
/// first component of a path going somewhere else. A false red: the manifest is conforming and the
/// gate would refuse it.
#[test]
fn a_leading_dot_component_names_the_same_target_as_the_path_without_it() {
    assert_eq!(
        unreachable_targets("[lib]\npath = \"./src/lib.rs\"\n"),
        Vec::<String>::new(),
        "./src/lib.rs IS src/lib.rs -- cargo builds it and the walk reaches it, so refusing it \
         turns the gate red on a lawful manifest"
    );
    assert_eq!(
        unreachable_targets("[lib]\npath = \"src/./lib.rs\"\n"),
        Vec::<String>::new(),
        "a `.` component anywhere is a no-op in a path, not a directory named `.`"
    );
    assert_eq!(
        unreachable_targets("[lib]\npath = \"./writer.rs\"\n"),
        vec!["./writer.rs".to_owned()],
        "dropping the `.` must not make an escape look conforming: ./writer.rs is outside src/ \
         however it is spelled"
    );
    // THE CONTROL THAT KEEPS THIS CORRECTION FROM OPENING A FALSE GREEN. The obvious way to
    // write this normalization is `replace("./", "")`, which matches INSIDE `../` and turns a real
    // escape into a conforming path. Dropping whole `.` components cannot do that, and this is the
    // cell that says so rather than the doc comment.
    assert_eq!(
        unreachable_targets("[lib]\npath = \"../src/lib.rs\"\n"),
        vec!["../src/lib.rs".to_owned()],
        "../src/lib.rs is a real escape: it names a src/ in the PARENT of the member, which this \
         sweep's walk never reaches"
    );
}

/// A target under a directory the walk REFUSES TO DESCEND INTO passes the path check and is never
/// read.
///
/// `walk_bounded` skips any directory whose name is in `WORKSPACE_WALK_SKIPPED`, at any depth. So
/// `src/target/writer.rs` starts with `src/`, escapes nothing, and is still invisible to the sweep
/// that this whole file's claim rests on -- a false green inside the PR's own thesis rather than
/// beside it.
///
/// Driven from the constant rather than from one hand-picked name, so a name added to the skip
/// list is covered the day it is added; the two literal cases below are what keeps that loop from
/// going vacuous if the constant is ever emptied.
#[test]
fn a_target_under_a_directory_the_walk_skips_is_not_read_as_covered() {
    assert_eq!(
        unreachable_targets("[lib]\npath = \"src/target/writer.rs\"\n"),
        vec!["src/target/writer.rs".to_owned()],
        "a build directory nested under src/ is skipped by the walk at any depth, so a target \
         inside it is production code this sweep never reads"
    );
    assert_eq!(
        unreachable_targets("[lib]\npath = \"src/node_modules/writer.rs\"\n"),
        vec!["src/node_modules/writer.rs".to_owned()],
        "and it is not only `target`: every name in WORKSPACE_WALK_SKIPPED hides its subtree the \
         same way"
    );

    assert!(
        WORKSPACE_WALK_SKIPPED.len() >= 4,
        "only {} skipped directory names, so the loop below is not covering the walk's exclusion \
         set",
        WORKSPACE_WALK_SKIPPED.len()
    );
    for skipped in WORKSPACE_WALK_SKIPPED {
        let target = format!("src/{skipped}/writer.rs");
        assert_eq!(
            unreachable_targets(&format!("[lib]\npath = \"{target}\"\n")),
            vec![target.clone()],
            "{target} sits under a directory the walk refuses to enter, so the file is never read"
        );
    }

    assert_eq!(
        unreachable_targets("[lib]\npath = \"src/target.rs\"\n"),
        Vec::<String>::new(),
        "the walk skips DIRECTORIES by name; a source FILE whose stem matches one is read like any \
         other, and refusing it would be a false red"
    );
}

#[test]
fn only_declared_crates_append_to_the_repository() {
    // ONE budget across the sweep. `walk` per member gives every root a fresh entry counter, so
    // each root is bounded and the sweep is not -- a manifest with many or repeated members
    // multiplies the cap instead of enforcing it. `walk_all` dedupes canonical roots and carries a
    // single count, which is why the shared helper exposes it, and its own doc records the same
    // defect from #578. The first version of this file called `walk` per member (Codex, on this PR).
    let roots: Vec<PathBuf> = workspace_members()
        .into_iter()
        .map(|member| member.join("src"))
        .collect();
    let files = walk_all(&roots, &["rs"]);

    // Non-vacuity: an empty walk agrees with any allow-list by finding nothing. The floor is well
    // under today's count and exists to catch a broken walk, not to pin a population.
    assert!(
        files.len() > 100,
        "the walk found only {} production sources across the workspace members, so it is not \
         covering the tree it claims to cover",
        files.len()
    );

    // BOUNDED READS, per file and in aggregate. The entry-count cap on the walk bounds how many
    // paths are visited, not how many BYTES are read: one oversized `.rs` under a member's `src/`
    // -- including a file rustc never loads -- would be read whole, kept, and then cloned for the
    // canary below (Codex, on this PR). The two limits are the sibling sweeps' own, for the same
    // reason and so the two cannot drift apart on what "too big" means.
    const MAX_SOURCE_BYTES: u64 = 4 * 1024 * 1024;
    const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
    let mut budget = MAX_TOTAL_BYTES;
    let mut sources: Vec<(String, String)> = Vec::new();
    for path in &files {
        let Ok(meta) = std::fs::metadata(path) else {
            continue;
        };
        assert!(
            meta.len() <= MAX_SOURCE_BYTES,
            "{} is {} bytes, over the {} byte per-file limit this sweep reads. A source that large \
             is either a defect or a new kind of file; decide which before widening the bound.",
            relative(path),
            meta.len(),
            MAX_SOURCE_BYTES
        );
        budget = budget.checked_sub(meta.len()).unwrap_or_else(|| {
            panic!(
                "the workspace's `src/` sources exceed this sweep's {MAX_TOTAL_BYTES} byte read \
                 budget; the population has changed shape and the bound needs a deliberate look"
            )
        });
        if let Ok(text) = std::fs::read_to_string(path) {
            sources.push((relative(path), text));
        }
    }

    // THE MATCHER, PROVEN ABLE TO FIND ONE, on this run's own population. The cells above drive it
    // with hand-built text; this drives it with the real walk plus one synthetic row, so a walk that
    // returned unreadable paths cannot pass here either.
    let members = member_roots();
    let mut seeded = sources.clone();
    seeded.push((
        "core/not-a-real-crate/src/lib.rs".to_owned(),
        "fn write() { repository.append_atomic(&request); }\n".to_owned(),
    ));
    let mut seeded_members = members.clone();
    seeded_members.push("core/not-a-real-crate".to_owned());
    let seeded_found = appending_crates(&seeded, &seeded_members);
    assert!(
        seeded_found.contains_key("core/not-a-real-crate"),
        "HARNESS-BROKE: the sweep does not report an undeclared writer even when one is handed to \
         it, so a green here says nothing about the tree"
    );

    // `silent` is ASYMMETRIC with `undeclared`, and that asymmetry is accepted rather than fixed.
    // Because this scan does not distinguish test code from production (see the note above
    // `crate_of`), a crate that stops appending in `src/` production but still appends inside its
    // own `#[cfg(test)]` fixtures stays in `writers` and never reaches `silent` (Codex, on this
    // PR). The safe direction is the one this list exists to protect: `undeclared` catches a NEW
    // place a scope can be chosen, which is the addressing hazard. `silent` staying stale means a
    // crate lingers on the list longer than strictly necessary -- one extra row and a question,
    // not a missed hazard. Splitting test appends from production ones to fix this would resurrect
    // the exact truncation logic three earlier versions of this file got wrong (see that note).
    let found = appending_crates(&sources, &members);
    let writers: BTreeSet<&str> = found.keys().map(String::as_str).collect();
    let declared = declared_writers();

    let undeclared: Vec<&str> = writers.difference(&declared).copied().collect();
    let silent: Vec<&str> = declared.difference(&writers).copied().collect();

    assert!(
        undeclared.is_empty() && silent.is_empty(),
        "the set of crates whose production code appends to the repository has changed.\n\n\
         appends and is NOT declared: {undeclared:?}\n\
         declared and no longer appends: {silent:?}\n\n\
         found: {found:?}\n\n\
         ANSWER THIS BEFORE EDITING THE LIST. A crate that has started appending is a new place a \
         repository scope can be CHOSEN rather than derived from the stream id. Three readers in \
         apps/cli (serve/monitor.rs, serve/wake.rs twice) locate a stream by id alone and take the \
         first match; they are correct only while no two streams share a stream_id, which holds \
         only while every write derives its scope. So: does every append in the new crate take a \
         scope it was handed, or does something in it construct one from operator-supplied values? \
         If it derives or receives, add the row with that reason. If it constructs, the guard has \
         found the thing it exists for and the construction is the bug, not this list.\n\n\
         A crate that has STOPPED appending is the cheaper case: drop its row, so the list keeps \
         meaning what it says. Do not leave it -- a list that names crates which no longer write \
         stops being read.\n\n\
         The question is asked rather than the edit offered, because an offered edit gets chosen \
         by distance."
    );
}
