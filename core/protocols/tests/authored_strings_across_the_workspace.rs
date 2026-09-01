//! The collapsed-run guard, once for the whole workspace instead of once per crate (#577).
//!
//! WHY THE FORM CHANGED. Adoption used to be per-crate and manual, so a crate joined the workspace
//! UNGUARDED by default and nothing said so. Measured by L on `249bda8`: twelve of twenty-four
//! members had a guard and twelve did not — and the unguarded half already carried the defect,
//! including eighteen spaces inside an `eprintln!` an operator reads when their run is refused.
//!
//! Deriving the population from `Cargo.toml` dissolves that question rather than moving it: the
//! alternative considered was a guard-of-adoption asserting each member HAS a guard file, and its
//! exemption list would have been the same hand-typed population one level up.
//!
//! **THE TRADE-OFF THIS SHAPE ACCEPTS, named so the next reader knows it was chosen.** Two costs,
//! both real:
//!
//! 1. **Locality.** A defect in `tools/development-benchmark` now reddens a test in
//!    `core/protocols`. The crate's own suite stays green and the author loses the "my crate is
//!    red" signal. Mitigated only by the message naming file and line, never by the test's address.
//! 2. **Central exemptions.** A role exemption that belonged to one crate is now visible to all of
//!    them. Better for audit — every exemption is in one place and each must earn its keep below —
//!    and worse for locality, in the same direction as (1).
//!
//! WHAT THIS DOES NOT DO: it does not delete the twelve per-crate guards. They carry more than this
//! scan — hardened-walker cells, root-coverage assertions, the no-follow metadata binding — and
//! removing only their collapsed-run test is twelve edits whose combined effect is "trust the new
//! one", which is worth reviewing on its own after this has been red on something real. The two
//! share `has_run_in_literal` by inclusion, so they cannot disagree about what the defect IS; only
//! their exemption lists can drift, and that is the follow-up this file is asking for rather than
//! assuming.

use std::collections::BTreeSet;

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/workspace-walk/walk.rs"
));

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/source-invariants/detect.rs"
));

/// Blank out literals whose ENTIRE content is spaces, leaving the quotes in place.
///
/// **Copied from `core/runtime/tests/authored_string_invariants.rs`, reason included**, rather than
/// re-derived — the issue is explicit that migrated exemptions carry the reason already written for
/// them, and a re-derivation is a second author reaching the same conclusion by luck.
///
/// Its reason, as that file states it: a literal that is nothing but spaces is not indentation that
/// leaked into a message, it is a VALUE whose being whitespace is the point. `prompt_assembly.rs`
/// sets an objective to `"   "` to test what happens when an objective is only blanks — rewriting it
/// to satisfy this guard would change what the test measures, which is the one edit a guard must
/// never provoke.
///
/// The quotes are KEPT (`"   "` becomes `""`) rather than the literal being deleted. The shared
/// predicate tracks whether it is inside a literal by counting quote transitions, so removing a
/// quote pair would shift the parity of everything after it on the line — the guard would then read
/// code as string and string as code.
fn without_whitespace_valued_literals(line: &str) -> String {
    let characters: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut index = 0;
    while index < characters.len() {
        if characters[index] == '"' {
            let mut end = index + 1;
            while end < characters.len() && characters[end] == ' ' {
                end += 1;
            }
            if end > index + 1 && end < characters.len() && characters[end] == '"' {
                out.push_str("\"\"");
                index = end + 1;
                continue;
            }
        }
        out.push(characters[index]);
        index += 1;
    }
    out
}

fn offends(line: &str) -> bool {
    !is_line_comment(line) && has_run_in_literal(&without_whitespace_valued_literals(line))
}

/// Files whose runs are their POINT, each with the reason and where the reason came from.
///
/// BY FILE and not by line, deliberately: a line number is a location and rots on the next edit,
/// while "this file's formatting is its subject" is a property of the file. Each entry must
/// suppress something today — `every_exemption_is_load_bearing` below fails on an entry that has
/// stopped mattering, which is what stops this list becoming the hand-typed population the issue
/// warned about.
///
/// The two species are NOT the same exemption and are not merged: a fixture that QUOTES the defect
/// on purpose is a guard talking about itself, while data whose indentation IS the value is a fact
/// about the format. Merging them would let either survive on the other's justification.
/// **The cost of exempting by file, stated because it is the sharp edge of this shape.** A real
/// defect elsewhere in an exempt file is invisible HERE. Six of the eight are guard files that
/// their own crate's per-crate guard still scans — which is the first thing making the twelve
/// load-bearing rather than redundant, and a reason not to delete them without replacing that
/// coverage. The exceptions are named in their entries.
const EXEMPT: [(&str, &str); 8] = [
    // ---- Species A: a guard quoting the defect on purpose. -------------------------------------
    // These files exist to hold examples of collapsed runs. Rewriting them to satisfy this sweep
    // would delete the samples the predicate is tested against, which is the one edit a guard must
    // never provoke.
    (
        "core/quality/tests/shared_source_invariant_predicate.rs",
        "the shared predicate's own detection fixtures (9 of them). NOTE: core/quality has no \
         per-crate guard, so this file is scanned by nothing once exempt here -- the narrowest \
         gap this list opens, and the one to close first if the exemption is ever made finer. \
         THE METHOD IS IN THIS FILE (found by L reviewing #578): the cells below never write a run \
         into the source, they BUILD one at runtime with `\" \".repeat(n)`, which is why a file \
         full of detection fixtures needs no exemption and scans itself. Those nine are written as \
         literals with the runs in the source and use repeat() zero times; rewriting them the same \
         way closes both this entry and the gap. Not free -- some of them must LOOK like real Rust \
         (aligned trailing comments, two literals on one line) and constructing that reads worse \
         than writing it -- and out of scope here, but the follow-up has a method rather than an \
         open question",
    ),
    (
        "tools/pathogens/tests/source_invariants.rs",
        "that crate's own detection fixtures; its per-crate guard scans this file",
    ),
    (
        "apps/cli/tests/source_invariants.rs",
        "that crate's own detection fixtures; its per-crate guard scans this file",
    ),
    (
        "core/tool-broker/tests/source_invariants.rs",
        "the comment-filter fixture: a multi-line sample whose indented `//` line is the input the \
         filter is measured on; its per-crate guard scans this file",
    ),
    (
        "core/gateway/tests/source_invariants.rs",
        "the comment-filter fixture, same sample and same reason as tool-broker's",
    ),
    (
        "core/execution/tests/source_invariants.rs",
        "the comment-filter fixture, same sample and same reason as tool-broker's",
    ),
    // ---- Species B: data whose formatting IS the value. -----------------------------------------
    // Not the same exemption as A and deliberately not merged with it: A is a guard talking about
    // itself, B is a fact about a format. Either would otherwise survive on the other's reason.
    (
        "tools/pathogens/src/lib.rs",
        "rendered HTML specimens: the spacing between elements is part of the payload the pathogen \
         models, not indentation that leaked into a message. NOTE: this is a `src/` file, so an \
         authored-string defect elsewhere in it is outside every sweep",
    ),
    (
        "apps/cli/tests/schema_cli.rs",
        "a canonical-JSON sample whose exact two- and four-space indentation is what the assertion \
         compares; migrated from apps/cli's `canonical_json_fixture_source_line`, which carries the \
         same reason",
    ),
];

fn exempt(path: &str) -> Option<&'static str> {
    EXEMPT
        .iter()
        .find(|(prefix, _)| path == *prefix)
        .map(|(_, reason)| *reason)
}

/// Every authored Rust file a member owns -- the member ROOT, not `src` and `tests` only.
///
/// The first version swept those two directories, and the workspace already compiles Rust outside
/// both: `tools/ci-canary/build.rs` and the `hashing.rs` it includes (found by Codex on #578). A
/// collapsed run introduced in either left this guard green while the crate shipped it, which is
/// precisely the adoption gap #577 measured, one directory in.
///
/// Walking the root also means a member's `benches`, `examples` and any future layout are covered
/// the day they appear -- the same reason the POPULATION comes from `Cargo.toml` rather than a
/// list: a guard that has to be widened by hand is one somebody forgets to widen.
fn authored_files() -> Vec<PathBuf> {
    let mut roots = workspace_members();
    // The shared files this sweep is BUILT FROM are not workspace members, so the population
    // derived from `Cargo.toml` cannot reach them -- and both are `include!`d into crates that
    // are. Found by reading this file's own output: `walk.rs` was carrying an eighteen-space run
    // in its own HARNESS-BROKE message, invisible to the guard it implements.
    //
    // Named explicitly rather than derived, because there is nothing to derive them FROM: they are
    // authored Rust that belongs to no crate. If a third shared file appears it must be added
    // here, and that is a worse property than the members list has -- said plainly rather than
    // hidden, since "a list somebody must remember to widen" is the defect #577 was about.
    for shared in ["tools/workspace-walk", "tools/source-invariants"] {
        roots.push(workspace_root().join(shared));
    }
    // ONE budget across the whole sweep, and each root walked once. Calling `walk` per member gave
    // every root its own fresh counter, so each root was bounded and the sweep was not — and a
    // manifest that legally repeats a member path traversed that tree twice while both walks
    // stayed under the bound (Codex, #578).
    walk_all(&roots, &["rs"])
}

/// The largest file this sweep will read into memory. Fourteen times the largest Rust file in the
/// workspace as measured (`core/events/src/local.rs`, 290_676 bytes).
const MAX_AUTHORED_FILE_BYTES: u64 = 4 * 1024 * 1024;

/// The whole sweep's read budget, because a per-ITEM bound with no aggregate is not a bound.
///
/// The two limits that already existed COMPOSE rather than cap: 8 192 entries times 4 MiB is
/// ~32 GiB of legal reads (Codex, #578). That product is the worst case being bounded, not a
/// number to adopt — so this is derived twice over instead of invented a third time. It is
/// **sixteen times the per-file bound**, and that lands at 8.6x the measured sweep: 403 Rust files
/// totalling 7_791_271 bytes.
///
/// Bounding the bytes READ also bounds what `offending_lines` retains, since every line it keeps is
/// copied out of those bytes.
const MAX_AUTHORED_SWEEP_BYTES: u64 = 16 * MAX_AUTHORED_FILE_BYTES;

fn offending_lines() -> Vec<String> {
    let mut out = Vec::new();
    let mut spent = 0_u64;
    for path in authored_files() {
        let relative_path = relative(&path);
        if exempt(&relative_path).is_some() {
            continue;
        }
        // Size from METADATA before the read, not after (Codex, #578). `read_to_string` allocates
        // the whole file, and the walk's entry bound counts paths rather than the bytes behind any
        // one of them -- so one oversized file kills this process with no diagnostic, in a guard
        // whose only job is to produce one. The repository already reads sizes this way before
        // acting on them (`core/events/src/local.rs:935`, `core/extension-host/src/activation.rs:305`);
        // this site was the one not following the convention.
        //
        // Measured before choosing the bound, because a bound tighter than its subject turns this
        // red on a legitimate checkout: the largest Rust file in the workspace is
        // `core/events/src/local.rs` at 290_676 bytes, against 4 MiB here.
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        assert!(
            metadata.len() <= MAX_AUTHORED_FILE_BYTES,
            "HARNESS-BROKE: {relative_path} is {} bytes, past the {MAX_AUTHORED_FILE_BYTES}-byte \
             bound this sweep reads under; it is not measuring what it claims",
            metadata.len()
        );
        // Debited BEFORE the read, from one budget for the whole sweep. Every file passing the
        // per-item bound independently is not a bound on the sweep -- the same question as the
        // per-item one, a floor up (Codex, #578).
        spent = spent.saturating_add(metadata.len());
        assert!(
            spent <= MAX_AUTHORED_SWEEP_BYTES,
            "HARNESS-BROKE: the sweep passed {MAX_AUTHORED_SWEEP_BYTES} bytes of source at \
             {relative_path} ({spent} so far); it is not measuring what it claims"
        );
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (number, line) in text.lines().enumerate() {
            if offends(line) {
                out.push(format!(
                    "{relative_path}:{}: {}",
                    number + 1,
                    line.trim_start()
                ));
            }
        }
    }
    out
}

#[test]
fn no_authored_string_in_the_workspace_carries_a_collapsed_run() {
    let files = authored_files();
    assert!(
        files.len() > 300,
        "the walk found only {} authored Rust files across the workspace members, so it is not \
         covering the tree it claims to cover",
        files.len()
    );

    let offenders = offending_lines();
    assert!(
        offenders.is_empty(),
        "these string literals carry runs of whitespace, which whoever reads the message gets \
         verbatim.\n\nANSWER THIS BEFORE EDITING EITHER SIDE: at the line above, is the run a \
         DEFECT or the POINT?\n\nA DEFECT -- a continued literal kept the next line's indentation, \
         or a generator ate the backslash before the file was written -- is the case this guard \
         exists for. Put the string on one line, or end the line with a backslash so Rust drops \
         the newline and the indentation with it.\n\nTHE POINT -- a fixture quoting the defect, or \
         data whose indentation IS the value -- belongs in EXEMPT above, by file, with the reason \
         written beside it. Those two are different exemptions and must not share an entry.\n\nThe \
         question is asked rather than the edits offered, because an offered pair gets chosen by \
         distance, and here the cheap one is an exemption.\n\nDetection lives in {}.\n{}",
        shared_predicate_self_path(),
        offenders.join("\n")
    );
}

/// A crate is covered the day it enters `members`, demonstrated rather than argued.
///
/// The issue asks for a scratch member in a test and not a sentence, so the parse runs against a
/// manifest composed here — one that names a crate which does not exist. Reading the real
/// `Cargo.toml` could only ever show that today's members are found, which is the claim nobody
/// doubts.
#[test]
fn a_member_added_to_the_manifest_is_swept_without_anyone_remembering() {
    let manifest = "[workspace]\nresolver = \"3\"\nmembers = [\n  \"core/protocols\",\n  \
                    \"crates/not-written-yet\",\n]\n";
    let parsed = member_names_from_manifest(manifest);

    // The INLINE form is valid Cargo and the first parser returned nothing for it, which would
    // have made every workspace sweep scan an empty population (Codex, #578). The partial form is
    // the worse half: it dropped only the entries on the key's own line, so the population shrank
    // by two of twenty-four -- still clear of every non-vacuity floor, and therefore silent.
    assert_eq!(
        member_names_from_manifest(
            "[workspace]
members = [\"a\", \"b\"]
"
        ),
        vec!["a".to_owned(), "b".to_owned()],
        "an inline members array must yield its members, or the sweeps scan nothing"
    );
    assert_eq!(
        member_names_from_manifest(
            "[workspace]
members = [\"a\", \"b\",
  \"c\",
]
"
        ),
        vec!["a".to_owned(), "b".to_owned(), "c".to_owned()],
        "a partially inline array must not drop the entries on the key's own line -- that loss \
         is small enough to pass every floor, which is what makes it dangerous"
    );
    // `default-members` is a real Cargo key and CONTAINS the string this parser looks for. A
    // substring search read its array instead (L, #578), so a manifest declaring it first fed
    // every workspace sweep the wrong population.
    assert_eq!(
        member_names_from_manifest(
            "[workspace]\ndefault-members = [\"core/protocols\"]\nmembers = [\"a\", \"b\", \"c\"]\n"
        ),
        vec!["a".to_owned(), "b".to_owned(), "c".to_owned()],
        "default-members must not hijack the key: it contains `members`, so a substring search \
         parses the wrong array and every sweep inherits it"
    );
    // The FIFTH shape (Codex, #578). Anchoring to a line stopped `default-members`, and left the
    // search running over the whole document with no notion of table boundaries. A lawful manifest
    // may carry `members` inside a metadata table, and that key belongs to whoever wrote the
    // metadata -- not to Cargo. Note which way this one fails: the gate rejects a LEGAL layout,
    // so the damage is a false accusation rather than a silent miss.
    assert_eq!(
        member_names_from_manifest(
            "[workspace.metadata.someone]
members = [\"not-a-crate\"]

[workspace]
members = [\"a\", \"b\"]
"
        ),
        vec!["a".to_owned(), "b".to_owned()],
        "a `members` key inside `[workspace.metadata.*]` belongs to that table, not to Cargo; \
         reading it feeds every sweep a population the workspace never declared"
    );
    assert_eq!(
        member_names_from_manifest(
            "[package.metadata.other]
members = [\"nope\"]

[workspace]
members = [\"a\"]
"
        ),
        vec!["a".to_owned()],
        "the same holds for `[package.metadata.*]`: only the key under `[workspace]` names the \
         workspace"
    );
    // Cargo's glob form (Codex, #578). The parser returned the literal `core/*`, so the sweep
    // walked a path that does not exist and the set-equality cell compared a wildcard against real
    // directories -- the gate rejecting a LAWFUL layout. Exercised against the real tree, because
    // an expansion is only right against a filesystem.
    let expanded = expand_member_pattern(&workspace_root(), "core/*");
    assert!(
        expanded.len() > 5,
        "`core/*` must expand to the crates under core/, or a lawful manifest is rejected: \
         {expanded:?}"
    );
    assert!(
        expanded
            .iter()
            .all(|path| path.join("Cargo.toml").is_file()),
        "an expanded member must be a crate directory, not any subdirectory: {expanded:?}"
    );
    assert!(
        expanded
            .iter()
            .any(|path| path.file_name().is_some_and(|name| name == "protocols")),
        "the expansion must reach this crate's own directory: {expanded:?}"
    );
    assert!(
        parsed.contains(&"crates/not-written-yet".to_owned()),
        "a member this guard has never seen must arrive from the manifest, or adoption is still \
         something somebody has to remember: {parsed:?}"
    );

    // And the real manifest reaches the crates the per-crate guards never covered.
    let swept: BTreeSet<String> = workspace_members()
        .iter()
        .map(|member| relative(member))
        .collect();
    for unguarded in [
        "adapters/tool-host",
        "core/schema",
        "tools/development-benchmark",
    ] {
        assert!(
            swept.contains(unguarded),
            "{unguarded} had no per-crate guard and must be inside this sweep; found: {swept:?}"
        );
    }

    // And the sweep reaches Rust that lives at a member's ROOT, not only under src/ and tests/.
    // `tools/ci-canary/build.rs` compiles and shipped outside both (Codex, #578).
    let files: Vec<String> = authored_files().iter().map(|path| relative(path)).collect();
    for outside in ["tools/ci-canary/build.rs", "tools/ci-canary/hashing.rs"] {
        assert!(
            files.iter().any(|found| found == outside),
            "{outside} is authored Rust a member owns and must be swept; it lives at the crate \
             root, which a src/-and-tests/ walk never reaches"
        );
    }
}

/// The parsed population equals the one on disk — the cell that ends the series.
///
/// FOUR PARSER DEFECTS IN ONE REVIEW, each a floor below the last: adoption was per crate, then the
/// sweep missed a member's root, then an inline array parsed as empty, then `default-members`
/// hijacked the key by substring. Every fix answered the shape in front of it and moved the
/// question down one. L's observation is the one worth keeping: **a single number that cannot shrink
/// in silence ends the class, where four correct fixes only shorten it.**
///
/// The witness is the FILESYSTEM, which is what makes it independent: a manifest directory holding
/// a `Cargo.toml` exists whether or not the parser can read the manifest. Any future parse that
/// drops, shifts or truncates the member list fails HERE, by name, without anyone having predicted
/// the shape it takes.
///
/// Set equality and not a count, for the reason the count would hide: a count catches shrinkage and
/// says nothing about WHICH member went missing, and the first thing a reader needs is the name.
#[test]
fn every_crate_on_disk_is_a_parsed_member_and_the_reverse() {
    let root = workspace_root();
    let mut manifests = Vec::new();
    walk(&root, &["toml"], &mut manifests);
    let on_disk: BTreeSet<String> = manifests
        .iter()
        .filter(|path| path.file_name().is_some_and(|name| name == "Cargo.toml"))
        .filter_map(|path| path.parent().map(relative))
        .filter(|directory| !directory.is_empty())
        .collect();

    let parsed: BTreeSet<String> = workspace_members()
        .iter()
        .map(|member| relative(member))
        .collect();

    assert!(
        on_disk.len() > 10,
        "the manifest walk found only {} crate directories; it is not reading the tree",
        on_disk.len()
    );
    assert_eq!(
        parsed,
        on_disk,
        "the parsed member list and the crates on disk disagree. Parsed but absent: {:?}. On disk \
         but unparsed: {:?}. A member the parser cannot see is a crate no workspace sweep visits, \
         and every floor in this file passes over it",
        parsed.difference(&on_disk).collect::<Vec<_>>(),
        on_disk.difference(&parsed).collect::<Vec<_>>()
    );
}

/// Every exemption suppresses something TODAY.
///
/// An entry that has stopped mattering is a hole nobody is watching, and the list is exactly where
/// this shape's cost lands — so it is the thing that must be hardest to grow.
#[test]
fn every_exemption_is_load_bearing() {
    for (path, reason) in EXEMPT {
        let full = workspace_root().join(path);
        // The size bound belongs HERE too (Codex, #578). `offending_lines` skips exempt files
        // before its own check, so the earlier fix did not cover them and one exempt file could
        // still kill this process with an unbounded read.
        //
        // It ASSERTS rather than skips, and that distinction is the whole point. A bound that
        // skipped would be an exemption anyone could earn without asking — let the file grow and
        // the guard stops looking at it, precisely when there is more in it to look at. The bound
        // exists to protect the DIAGNOSTIC (an allocation failure aborts saying nothing about the
        // file it died on), not to excuse a file from the check, and there is no case where it
        // should win over the check. So it fails loudly and never narrows the population.
        let metadata = std::fs::metadata(&full)
            .unwrap_or_else(|error| panic!("exempt file {path} must exist: {error}"));
        assert!(
            metadata.len() <= MAX_AUTHORED_FILE_BYTES,
            "HARNESS-BROKE: exempt file {path} is {} bytes, past the \
             {MAX_AUTHORED_FILE_BYTES}-byte bound this check reads under; it is not measuring \
             what it claims",
            metadata.len()
        );
        let text = std::fs::read_to_string(&full)
            .unwrap_or_else(|error| panic!("exempt file {path} must exist: {error}"));
        assert!(
            text.lines().any(offends),
            "{path} is exempt for {reason:?} and no longer carries anything this guard would \
             flag. Remove the entry: an exemption with no subject cannot be told from one that \
             stopped working"
        );
    }
}

/// The predicate still catches the defect and still ignores ordinary Rust.
///
/// A control on the SHARED predicate as this file composes it, because the exemption above rewrites
/// the line before the predicate sees it -- and a rewriting step is exactly where a guard quietly
/// stops detecting.
#[test]
fn the_composed_predicate_catches_the_defect_and_spares_ordinary_rust() {
    let collapsed = format!("let s = \"alpha{}beta\";", " ".repeat(10));
    assert!(
        offends(&collapsed),
        "the composed predicate must still fire"
    );
    assert!(
        !offends("let s = \"alpha beta\";"),
        "one space is not a run"
    );
    assert!(
        !offends(&format!("// alpha{}beta", " ".repeat(10))),
        "a comment is prose, not an authored string"
    );
    let whitespace_value = format!("let objective = \"{}\";", " ".repeat(3));
    assert!(
        !offends(&whitespace_value),
        "a literal that is ONLY spaces is a value whose being whitespace is the point"
    );
}
