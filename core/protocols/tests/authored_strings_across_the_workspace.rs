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
/// defect elsewhere in an exempt file is invisible HERE. Six of the nine are guard files that
/// their own crate's per-crate guard still scans — which is the first thing making the twelve
/// load-bearing rather than redundant, and a reason not to delete them without replacing that
/// coverage. The exceptions are named in their entries, and each entry states its own gap.
const EXEMPT: [(&str, &str); 9] = [
    // ---- Species A: a guard quoting the defect on purpose. -------------------------------------
    // These files exist to hold examples of collapsed runs. Rewriting them to satisfy this sweep
    // would delete the samples the predicate is tested against, which is the one edit a guard must
    // never provoke.
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
    (
        "core/protocols/tests/only_declared_crates_write_the_repository.rs",
        "the_form_matches_the_three_spellings_and_leaves_their_siblings_alone's fixture lines: each \
         is a sample of REAL Rust source code (an append call, a comment) whose leading indentation \
         is the input `names_an_append`/`is_comment` are measured against, not authored prose that \
         leaked whitespace",
    ),
    (
        "adapters/tool-host/tests/cancellation_record_asymmetry.rs",
        "fixture source snippets for the cancellation-record census; indentation is the value the \
         lexical projection must see. The census projects Rust source while PRESERVING byte \
         offsets and line numbers, so a snippet's leading run is the input its truth table and its \
         BOM/shebang offset cells are measured against -- re-indenting one would change the answer \
         it asserts. Since #1132 that crate HAS a per-crate `source_invariants.rs` and it reads \
         this file: its exemption is per-LITERAL and by ROLE -- a literal naming \
         `CapturedProcess` or `tree_kill` is census input -- so authored prose here is covered \
         there. What is exempt is this sweep, no longer every sweep",
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

/// The per-item bound, then the aggregate, in the order that makes the aggregate mean anything:
/// debited BEFORE any read, so a file the per-item check would refuse never reaches the aggregate
/// at all. Pulled out of `offending_lines` so a test can drive this exact code against a fixture
/// tree without either duplicating it (a twin proves nothing about the shipped function) or routing
/// through the workspace-wide walk, which has no injection point (#656).
///
/// Size from METADATA before the read, not after (Codex, #578). `read_to_string` allocates the
/// whole file, and the walk's entry bound counts paths rather than the bytes behind any one of
/// them -- so one oversized file kills this process with no diagnostic, in a guard whose only job
/// is to produce one.
///
/// Debited before the read, from one budget for the whole sweep. Every file passing the per-item
/// bound independently is not a bound on the sweep -- the same question as the per-item one, a
/// floor up (Codex, #578).
fn debit_sweep_budget(relative_path: &str, file_bytes: u64, spent: &mut u64) {
    assert!(
        file_bytes <= MAX_AUTHORED_FILE_BYTES,
        "HARNESS-BROKE: {relative_path} is {file_bytes} bytes, past the \
         {MAX_AUTHORED_FILE_BYTES}-byte bound this sweep reads under; it is not measuring what it \
         claims"
    );
    *spent = spent.saturating_add(file_bytes);
    assert!(
        *spent <= MAX_AUTHORED_SWEEP_BYTES,
        "HARNESS-BROKE: the sweep passed {MAX_AUTHORED_SWEEP_BYTES} bytes of source at \
         {relative_path} ({spent} so far); it is not measuring what it claims"
    );
}

/// The collapsed-run sweep over a GIVEN list of files.
///
/// Split for the same reason as `control_character_offenders_over` below, and X measured it here
/// first: replacing this body with `Vec::new()` -- the walk untouched, every file still found, the
/// predicate still correct -- left 9 of 9 GREEN. The population cell proves the walk found 300+
/// files; the predicate cells prove the predicate tells the defect apart; **nothing proved this
/// function applies the second to the first.**
///
/// That junction matters more here than anywhere else, because #577 turned twelve per-crate guards
/// into one sweep. An unmeasured collector here is not one vacuous guard, it is twelve at once.
fn offending_lines_over(files: &[std::path::PathBuf]) -> Vec<String> {
    let mut out = Vec::new();
    let mut spent = 0_u64;
    for path in files {
        let path = path.clone();
        let relative_path = relative(&path);
        if exempt(&relative_path).is_some() {
            continue;
        }
        // Measured before choosing the bound, because a bound tighter than its subject turns this
        // red on a legitimate checkout: the largest Rust file in the workspace is
        // `core/events/src/local.rs` at 290_676 bytes, against 4 MiB here.
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        debit_sweep_budget(&relative_path, metadata.len(), &mut spent);
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
    // The new cell for the aggregate bound proves the FUNCTION, not the WIRING: it calls
    // `debit_sweep_budget` directly, so deleting the call at the site above reddens nothing --
    // the real sweep still reads under its 67 MiB budget with 8x headroom (found by L reviewing
    // #657; the seam the extraction created was never checked from this side of it). `spent` is
    // this loop's only witness that the call ran at all, so it is the one thing worth asserting
    // here: zero would mean either an empty workspace or a deleted call, and this workspace is
    // never empty.
    assert!(
        spent > 0,
        "HARNESS-BROKE: the sweep read zero bytes of source, so nothing called \
         debit_sweep_budget; the aggregate bound's own test cell would not catch that call being \
         deleted here"
    );
    out
}

fn offending_lines() -> Vec<String> {
    offending_lines_over(&authored_files())
}

/// What a source file may carry that is not text a reader can see (#522).
///
/// Two arms, and they are one subject: **a byte that survives the compiler, the suite and clippy
/// while changing what a human reads.**
///
/// A raw NUL landed in `core/events/tests/execution_projection.rs` where the source should have
/// read `\0`. `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` and all
/// twelve per-crate source guards passed it, and the test it sat in was semantically CORRECT --
/// `"<NUL>".repeat(8)` produces exactly the bytes `"\0".repeat(8)` produces. Nothing measuring
/// behaviour could catch it, because no behaviour was wrong. The only signal in the whole
/// toolchain was `grep` saying `Binary file … matches`, in the incidental output of a command run
/// for another reason. **That is how it was found, not a detector**: it fires only if somebody
/// happens to grep that file, it sees NUL and nothing else, and it is not on the delivery path.
///
/// The second arm is one this sweep did not have. `offending_lines` above reads with
/// `read_to_string` and `continue`s on failure, so a file this walk cannot DECODE is skipped in
/// silence -- the population shrinks and the sweep still reports success. The per-crate guards
/// surface that case by `map_err`; at the workspace level it was swallowed. A file that cannot be
/// read as UTF-8 is now named rather than dropped.
///
/// C0 controls other than `\t`, `\n` and `\r` are refused, and so is DEL. Everything else is left
/// alone: this is about bytes with no glyph, not about what characters source may use.
///
/// **No exemption list, deliberately.** `EXEMPT` above is about indentation inside a literal,
/// where the run can legitimately BE the value. There is no counterpart here -- a fixture needing
/// a control byte writes the escape, which is what the repaired file now does. If a real case ever
/// appears it earns its own list with its own reason, rather than borrowing one written for a
/// different defect.
#[derive(Debug, PartialEq, Eq)]
enum SourceByteOffence {
    Undecodable,
    Control {
        line: usize,
        column: usize,
        byte: u8,
    },
}

/// Pure, so the cell below can put both arms to it without writing a bad byte into the tree.
fn source_byte_offence(bytes: &[u8]) -> Option<SourceByteOffence> {
    if std::str::from_utf8(bytes).is_err() {
        return Some(SourceByteOffence::Undecodable);
    }
    let mut line = 1_usize;
    let mut column = 1_usize;
    for &byte in bytes {
        if byte == b'\n' {
            line += 1;
            column = 1;
            continue;
        }
        if (byte < 0x20 && byte != b'\t' && byte != b'\r') || byte == 0x7f {
            return Some(SourceByteOffence::Control { line, column, byte });
        }
        column += 1;
    }
    None
}

/// The sweep over a GIVEN list of files.
///
/// **Separated so a cell can stand between the walk and the predicate** (#939's review). The
/// no-argument version below is the real one; every assertion about it is an absence, and an
/// absence cannot tell a working refusal from a collector that returns nothing. Measured: replacing
/// the whole body with `Vec::new()` left 9 of 9 green -- the `spent > 0` guard inside it went with
/// the body, and the population control above only proves the WALK found files, not that anything
/// read them.
///
/// With this split, a cell hands it one real file holding one bad byte and asks for the message
/// back, which joins walk -> read -> predicate -> text. That junction is the thing no absence can
/// assert about itself.
fn control_character_offenders_over(files: &[std::path::PathBuf]) -> Vec<String> {
    let mut out = Vec::new();
    let mut spent = 0_u64;
    for path in files {
        let path = path.clone();
        let relative_path = relative(&path);
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        debit_sweep_budget(&relative_path, metadata.len(), &mut spent);
        // READ AS BYTES, not as a string. Decoding first would make the undecodable arm
        // unreachable from here, which is half of what this guard is for.
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        match source_byte_offence(&bytes) {
            None => {}
            Some(SourceByteOffence::Undecodable) => {
                out.push(format!("{relative_path}: cannot be decoded as UTF-8"));
            }
            Some(SourceByteOffence::Control { line, column, byte }) => {
                out.push(format!(
                    "{relative_path}:{line}:{column}: carries the raw byte 0x{byte:02x}, which has \
                     no glyph"
                ));
            }
        }
    }
    assert!(
        spent > 0,
        "HARNESS-BROKE: the control-character sweep read zero bytes of source"
    );
    out
}

fn control_character_offenders() -> Vec<String> {
    control_character_offenders_over(&authored_files())
}

#[test]
fn no_source_file_in_the_workspace_carries_a_control_character() {
    let files = authored_files();
    assert!(
        files.len() > 300,
        "the walk found only {} authored Rust files across the workspace members, so it is not \
         covering the tree it claims to cover",
        files.len()
    );

    let offenders = control_character_offenders();
    assert!(
        offenders.is_empty(),
        "these source files carry a byte a reader cannot see.\n\nA control byte in source is \
         almost always a generator that consumed an escape: the file should read `\0`, `\x1b` or \
         `\\u{{7f}}` and instead holds the byte itself. The compiler, the suite and clippy all \
         accept it, and the code can be semantically correct while what a human copies out of it \
         is not what they think.\n\nWrite the escape. If the raw byte is genuinely the point, it \
         belongs in a fixture built at runtime rather than in the source text.\n\nAn `undecodable` \
         line means this walk could not read the file as UTF-8 at all -- it used to be skipped in \
         silence.\n{}",
        offenders.join("\n")
    );
}

/// The guard above cannot demonstrate itself: it is green precisely when the tree is clean, and a
/// green cell over a clean population is satisfied by a predicate that never fires. So the
/// predicate is put to both arms and to ordinary Rust here, with no file written.
#[test]
fn the_control_character_predicate_catches_the_byte_and_spares_ordinary_rust() {
    assert_eq!(
        source_byte_offence(b"let s = \"alpha\\0beta\";\n"),
        None,
        "the ESCAPE is how a NUL is meant to be written, and it is two ordinary characters"
    );
    assert_eq!(
        source_byte_offence("let s = \"alpha\0beta\";\n".as_bytes()),
        Some(SourceByteOffence::Control {
            line: 1,
            column: 15,
            byte: 0
        }),
        "and the raw byte in the same position is the defect this exists for"
    );
    assert_eq!(
        source_byte_offence(b"fn main() {\r\n\t// tab, CR and LF are ordinary source\r\n}\n"),
        None,
        "tabs and CRLF are how this repository's files legitimately arrive on Windows"
    );
    assert_eq!(
        source_byte_offence(b"let del = \"a\x7fb\";\n"),
        Some(SourceByteOffence::Control {
            line: 1,
            column: 13,
            byte: 0x7f
        }),
        "DEL has no glyph either, and is not a C0 control, so it needs saying separately"
    );
    assert_eq!(
        source_byte_offence(&[b'/', b'/', b' ', 0xff, 0xfe, b'\n']),
        Some(SourceByteOffence::Undecodable),
        "and a file this walk cannot decode is an offence rather than a file to skip"
    );
    assert_eq!(
        source_byte_offence("// caf\u{e9} \u{1f600} \u{2014}\nfn f() {}\n".as_bytes()),
        None,
        "ordinary non-ASCII source is not the subject: this is about bytes with no glyph"
    );
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

/// The sweep's AGGREGATE byte budget refuses a tree whose total exceeds it, even though every
/// individual file stays under the per-file bound (#578, #656).
///
/// `MAX_AUTHORED_SWEEP_BYTES` composes the two limits that already existed --
/// `WORKSPACE_WALK_MAX_ENTRIES` times `MAX_AUTHORED_FILE_BYTES` is ~32 GiB of legal reads -- and
/// shipped without a cell: arming it against the real workspace needs over 64 MiB of Rust, which
/// this repository does not carry and should not grow to have on purpose. Generated here instead,
/// in a tmpdir, never committed.
///
/// Drives `debit_sweep_budget` directly rather than `offending_lines`: that function has no
/// injection point (it always walks the real workspace), and duplicating its bound-checking logic
/// here would sabotage a TWIN rather than the shipped function -- proving nothing about the code
/// that ships. This is the exact function `offending_lines` calls, unchanged.
#[test]
fn the_sweep_byte_budget_refuses_a_tree_over_it() {
    let dir = tempfile::tempdir().expect("a temp dir");
    // Each file sits comfortably under the PER-FILE bound (4 MiB), so only the AGGREGATE can be
    // what trips -- a file this size would never redden the per-item assert first.
    let per_file_bytes: usize = 3 * 1024 * 1024;
    let content = "// filler line, repeated to build a cheap oversized fixture\n"
        .repeat(per_file_bytes / 60 + 1)
        .into_bytes();
    let content = &content[..per_file_bytes];
    // 24 * 3 MiB = 72 MiB, comfortably over the 64 MiB aggregate bound with margin for filesystem
    // rounding, and comfortably under it for the first ~21 files so the budget genuinely runs out
    // partway through rather than on file one.
    let file_count = 24;
    let mut paths = Vec::new();
    for index in 0..file_count {
        let path = dir.path().join(format!("filler_{index}.rs"));
        std::fs::write(&path, content).expect("the fixture file writes");
        paths.push(path);
    }

    // ARRANGEMENT, asserted rather than assumed: the fixture's own total must exceed the bound
    // before the guard is judged, or a refusal below could be about anything.
    let total: u64 = paths
        .iter()
        .map(|path| {
            std::fs::metadata(path)
                .expect("fixture metadata reads")
                .len()
        })
        .sum();
    assert!(
        total > MAX_AUTHORED_SWEEP_BYTES,
        "arrangement: the fixture must exceed the aggregate bound to test refusing it, got \
         {total} bytes against a {MAX_AUTHORED_SWEEP_BYTES}-byte budget"
    );

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut spent = 0_u64;
        for path in &paths {
            let metadata = std::fs::metadata(path).expect("fixture metadata reads");
            debit_sweep_budget(&path.display().to_string(), metadata.len(), &mut spent);
        }
    }));

    let error = result.expect_err(
        "the sweep byte budget must refuse a tree whose cumulative size exceeds it, not read \
         all of it",
    );
    let message = error
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| error.downcast_ref::<&str>().map(|text| (*text).to_owned()))
        .expect("the panic payload is a string");
    assert!(
        message.contains("HARNESS-BROKE") && message.contains("passed"),
        "the refusal must name the limit it hit, not just panic: {message}"
    );
}

#[cfg(windows)]
fn make_directory_link(link: &Path, target: &Path) {
    let status = std::process::Command::new("cmd")
        .args([
            "/c",
            "mklink",
            "/J",
            link.to_str().expect("link path is unicode"),
            target.to_str().expect("target path is unicode"),
        ])
        .status()
        .expect("mklink runs");
    assert!(
        status.success(),
        "ARRANGEMENT: the directory junction was not created"
    );
}

#[cfg(unix)]
fn make_directory_link(link: &Path, target: &Path) {
    std::os::unix::fs::symlink(target, link).expect("ARRANGEMENT: the symlink was not created");
}

/// A member root that resolves outside the workspace via a directory link is refused, not
/// silently traversed (#578, #656).
///
/// A junction needs no admin privilege on Windows, unlike a symlink -- measured with a throwaway
/// probe before writing this cell: `std::fs::canonicalize` resolves a junction to its target
/// exactly as it resolves a symlink, so the same code path (`member_root_inside`'s
/// `canonicalize` + `starts_with`) is genuinely exercised on every platform, just through a
/// different link kind. Unix uses a real symlink; Windows uses a junction; the assertion under
/// test is identical either way.
#[test]
fn a_member_root_escaping_via_a_directory_link_is_refused() {
    let workspace = tempfile::tempdir().expect("a temp dir");
    let outside = tempfile::tempdir().expect("a temp dir");
    let workspace_root = std::fs::canonicalize(workspace.path()).expect("workspace root resolves");

    let link_path = workspace.path().join("escaped-member");
    make_directory_link(&link_path, outside.path());

    // ARRANGEMENT, asserted rather than assumed: the link must actually resolve OUTSIDE the
    // workspace root before the guard is judged, on THIS platform's link mechanism.
    let resolved = std::fs::canonicalize(&link_path).expect("the link resolves");
    assert!(
        !resolved.starts_with(&workspace_root),
        "arrangement: the directory link must resolve outside the workspace root, got {} under \
         {}",
        resolved.display(),
        workspace_root.display()
    );

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        member_root_inside(&workspace_root, &link_path)
    }));

    let error = result.expect_err(
        "a member root that resolves outside the workspace via a directory link must be refused, \
         not traversed",
    );
    let message = error
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| error.downcast_ref::<&str>().map(|text| (*text).to_owned()))
        .expect("the panic payload is a string");
    assert!(
        message.contains("HARNESS-BROKE") && message.contains("outside the workspace"),
        "the refusal must name why, not just panic: {message}"
    );
}

/// THE JUNCTION: the sweep really does read the files and really does ask the predicate (#939).
///
/// Every other assertion about `control_character_offenders` is an ABSENCE — zero offenders over a
/// clean tree — and an absence is equally satisfied by a collector that returns nothing at all.
/// Measured before this cell existed: replacing the whole body with `Vec::new()` left the suite
/// green, because the `spent > 0` guard lives inside the body it would have caught and the
/// population control above only proves the WALK found files, not that anything read them.
///
/// So this hands the sweep one real file on disk holding one bad byte and asks for the message
/// back. It joins walk → read → predicate → text, which is the chain no absence can assert about
/// itself, and it fails on a collector that has stopped collecting.
///
/// The file is written OUTSIDE the repository. Inside it, this suite would be creating the very
/// defect it exists to refuse — and the workspace sweep would find it on the next run.
#[test]
fn the_sweep_reports_a_bad_byte_in_a_file_it_is_given() {
    let directory = scratch_directory("gh-cc");

    let clean = directory.join("clean.rs");
    std::fs::write(&clean, b"fn main() {}\n").expect("the clean fixture is written");
    let dirty = directory.join("dirty.rs");
    std::fs::write(&dirty, b"fn main() {\0}\n").expect("the dirty fixture is written");

    let clean_result = control_character_offenders_over(std::slice::from_ref(&clean));
    let dirty_result = control_character_offenders_over(std::slice::from_ref(&dirty));

    // CONTROL FIRST. Without it, a collector that reported every file would satisfy the assertion
    // below and this cell would be measuring nothing but its own fixture.
    assert!(
        clean_result.is_empty(),
        "a file with no glyphless byte must produce nothing, or this cell cannot tell a working \
         sweep from one that reports everything: {clean_result:?}"
    );
    assert_eq!(
        dirty_result.len(),
        1,
        "the sweep was handed one file holding one NUL and must report exactly it: {dirty_result:?}"
    );
    assert!(
        dirty_result[0].contains("0x00"),
        "and the message must name the byte, which is what a reader acts on: {dirty_result:?}"
    );

    std::fs::remove_dir_all(&directory).ok();
}

/// THE JUNCTION FOR THE COLLAPSED-RUN SWEEP: it really does apply the predicate to the files
/// (#939, required by the Orquestrador's 18:42Z decision, measured by X).
///
/// Replacing `offending_lines`'s body with `Vec::new()` left 9 of 9 green. The walk was untouched,
/// every file was still found, the predicate still told the defect apart — and nothing anywhere
/// asserted that the second was applied to the first.
///
/// **This is the cell that matters most in this file.** #577 turned twelve per-crate guards into
/// one sweep, so an unmeasured collector here is not one vacuous guard, it is twelve going vacuous
/// at once. The two halves that were already asserted — a population over 300 and a predicate that
/// discriminates — are exactly the two an empty collector satisfies.
///
/// The fixture is written OUTSIDE the repository. Inside it, this suite would be planting the very
/// defect it exists to refuse, and the workspace sweep would find it on the next run.
#[test]
fn the_collapsed_run_sweep_reports_an_offender_in_a_file_it_is_given() {
    let directory = scratch_directory("gh-run");
    let clean = directory.join("clean.rs");
    std::fs::write(&clean, "let s = \"alpha beta\";\n").expect("the clean fixture is written");
    let dirty = directory.join("dirty.rs");
    std::fs::write(
        &dirty,
        format!("let s = \"alpha{}beta\";\n", " ".repeat(10)),
    )
    .expect("the dirty fixture is written");

    let clean_result = offending_lines_over(std::slice::from_ref(&clean));
    let dirty_result = offending_lines_over(std::slice::from_ref(&dirty));

    // CONTROL FIRST, or this cell cannot tell a working sweep from one that reports every file.
    assert!(
        clean_result.is_empty(),
        "a literal with a single space is ordinary and must produce nothing: {clean_result:?}"
    );
    assert_eq!(
        dirty_result.len(),
        1,
        "the sweep was handed one file carrying one collapsed run and must report exactly it: \
         {dirty_result:?}"
    );

    std::fs::remove_dir_all(&directory).ok();
}

/// A directory outside the repository, named so two cells cannot collide.
fn scratch_directory(prefix: &str) -> std::path::PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "{prefix}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&directory).expect("a scratch directory outside the repository");
    directory
}
