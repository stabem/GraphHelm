//! The bounded source channel (#219): the second evidence path a non-complete coverage state
//! licenses, implemented workspace-scoped and bounded the same way its sibling reader is.
//!
//! **Why it exists, measured rather than argued.** Against the shipped index, required evidence
//! living in non-code files is unreachable through `StructuralCodeIndex` at ANY query: the
//! provider filters non-construct nodes by design, so JSON schemas and changelogs never appear in
//! a result. That is a true finding about the instrument, so the compiler grows a channel.
//!
//! Every cell here is about a REFUSAL or a BOUND, because the dangerous version of this component
//! is the one that quietly walks a repository.

use std::path::Path;

use graphhelm_runtime::ports::{BoundedSourceSearch, SourceSearchBounds, SourceSearchError};
use graphhelm_tool_host::source_channel::WorkspaceSourceChannel;

fn bounds(
    max_files_scanned: usize,
    max_bytes_scanned: u64,
    max_results: u32,
) -> SourceSearchBounds {
    SourceSearchBounds {
        max_entries_visited: 100_000,
        max_files_scanned,
        max_bytes_scanned,
        max_results,
        max_terms: 64,
        max_term_bytes: 64 * 256,
    }
}

fn generous() -> SourceSearchBounds {
    bounds(10_000, 64 * 1024 * 1024, 10)
}

/// A workspace with code, a schema, a changelog, and a process directory that must never be
/// served as evidence.
fn workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let write = |rel: &str, body: &str| {
        let path = dir.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    };
    write(
        "core/events/src/local.rs",
        "fn load_state() { /* verdict mapping */ }\n",
    );
    write(
        "schemas/node.schema.json",
        "{\"title\": \"node\", \"verdict\": \"ceiling\"}\n",
    );
    write(
        "schemas/CHANGELOG.md",
        "# ceiling raised for verdict nodes\n",
    );
    write("core/quiet.rs", "fn unrelated() {}\n");
    // Process directories: the factory's own diary. Excluded BY PRINCIPLE, not by tuning -- a
    // production selector must not serve its own working notes as repository evidence.
    write(
        ".factory/board.md",
        "verdict ceiling node schema local_state\n",
    );
    write(
        "docs/superpowers/plans/notes.md",
        "verdict ceiling node schema\n",
    );
    dir
}

/// #1086 item 9: agent and tool state directories are skipped by NAME at any depth. A checkout
/// with many agent worktrees under `.claude/` otherwise crossed the 50,000-entry ceiling with no
/// code change, and a worktree's copy of a file outranked the file itself.
#[test]
fn agent_and_tool_state_directories_are_never_entered_at_any_depth() {
    let dir = tempfile::tempdir().unwrap();
    let write = |rel: &str| {
        let path = dir.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "zanzibar_quokka\n").unwrap();
    };
    for rel in [
        ".claude/worktrees/lane/src/real.rs",
        "nested/.claude/notes.md",
        ".codex/sessions/a.md",
        ".cursor/rules.md",
        ".worktrees/deploy-main/src/real.rs",
        "src/real.rs",
    ] {
        write(rel);
    }
    let channel = WorkspaceSourceChannel::open(dir.path()).unwrap();
    let hits = channel
        .search(&["zanzibar_quokka".to_owned()], &generous())
        .unwrap();
    assert_eq!(hits, ["src/real.rs"]);
}

/// #1086 (Codex P1 on #1092): a scan whose drive gave it up stops at its next check and
/// refuses, rather than walking on; the same channel without a set token still answers.
#[test]
fn a_cancelled_scan_refuses_at_its_next_check() {
    let dir = workspace();
    let cancel = graphhelm_runtime::ports::ScanCancel::new();
    let channel = WorkspaceSourceChannel::open(dir.path())
        .unwrap()
        .with_cancel(cancel.clone());
    assert!(
        !channel
            .search(&["verdict".to_owned()], &generous())
            .unwrap()
            .is_empty()
    );
    cancel.cancel();
    assert_eq!(
        channel.search(&["verdict".to_owned()], &generous()),
        Err(SourceSearchError::Unavailable)
    );
}

/// THE PURPOSE: non-code evidence the structural index cannot reach IS reachable here.
#[test]
fn the_channel_reaches_non_code_evidence_the_index_filters_away() {
    let dir = workspace();
    let channel = WorkspaceSourceChannel::open(dir.path()).unwrap();

    let hits = channel
        .search(&["verdict".to_owned(), "ceiling".to_owned()], &generous())
        .expect("a bounded search over a small workspace succeeds");

    assert!(
        hits.iter().any(|path| path == "schemas/node.schema.json"),
        "the JSON schema is exactly what the graph channel cannot serve, got: {hits:?}"
    );
    assert!(
        hits.iter().any(|path| path == "schemas/CHANGELOG.md"),
        "so is the changelog, got: {hits:?}"
    );
}

/// Paths are repository-relative and forward-slashed on every platform: they flow into a plan
/// whose escape checks compare TEXT, so a backslash here would read as a different path.
#[test]
fn hits_are_repository_relative_and_forward_slashed() {
    let dir = workspace();
    let channel = WorkspaceSourceChannel::open(dir.path()).unwrap();

    let hits = channel
        .search(&["verdict".to_owned()], &generous())
        .unwrap();

    for hit in &hits {
        assert!(
            !hit.contains('\\'),
            "a backslash reaches the plan as a different path: {hit}"
        );
        assert!(
            !hit.starts_with('/'),
            "absolute paths escape the workspace: {hit}"
        );
        assert!(
            dir.path().join(hit).is_file(),
            "every hit must exist under the workspace: {hit}"
        );
    }
}

/// EXCLUSION BY PRINCIPLE, not by tuning: the factory's own process directories are never
/// evidence about the repository, however well they match. Declared in the code, asserted here.
#[test]
fn process_directories_are_never_served_as_evidence() {
    let dir = workspace();
    let channel = WorkspaceSourceChannel::open(dir.path()).unwrap();

    let hits = channel
        .search(&["verdict".to_owned(), "ceiling".to_owned()], &generous())
        .unwrap();

    assert!(
        !hits.iter().any(|path| path.starts_with(".factory/")),
        "the factory's own diary was served as repository evidence: {hits:?}"
    );
    assert!(
        !hits
            .iter()
            .any(|path| path.starts_with("docs/superpowers/")),
        "the process plans were served as repository evidence: {hits:?}"
    );
}

/// Codex #622: a case-insensitive filesystem (Windows, but also casefold ext4 or a Linux-mounted
/// VFAT/NTFS workspace) resolves `.FACTORY` and `.factory` to one directory, and an untrusted
/// workspace could spell the alias in any case to walk its diary past the exclusion. The exclusion
/// folds case UNCONDITIONALLY now, so this runs on every platform (it created `.FACTORY`, and on a
/// case-sensitive fs that is simply a distinct directory the fold also excludes -- harmless).
#[test]
fn an_uppercased_excluded_prefix_is_still_excluded() {
    let dir = tempfile::tempdir().unwrap();
    let write = |rel: &str, body: &str| {
        let path = dir.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    };
    // A real evidence file so the search is not vacuously empty, plus the aliased diary.
    write("core/real.rs", "verdict ceiling\n");
    write(".FACTORY/board.md", "verdict ceiling node schema\n");
    write("Docs/Superpowers/Plans/notes.md", "verdict ceiling\n");

    let channel = WorkspaceSourceChannel::open(dir.path()).unwrap();
    let hits = channel
        .search(&["verdict".to_owned(), "ceiling".to_owned()], &generous())
        .unwrap();

    assert!(
        hits.iter().any(|path| path == "core/real.rs"),
        "arrangement: the real evidence must be found, or the exclusion is not what is tested: {hits:?}"
    );
    assert!(
        !hits
            .iter()
            .any(|path| path.to_lowercase().starts_with(".factory/")),
        "an uppercased alias of the excluded diary was served on a case-insensitive fs: {hits:?}"
    );
    assert!(
        !hits
            .iter()
            .any(|path| path.to_lowercase().starts_with("docs/superpowers/plans/")),
        "an alias-cased process-plans path was served: {hits:?}"
    );
}

/// A bound is a CEILING THAT REFUSES, never a silent truncation -- the same posture
/// `compile_plan_within` takes with over-budget responses. A truncated search wearing a finished
/// search's clothes is the defect the whole freeze exists to prevent.
#[test]
fn exceeding_the_file_bound_refuses_rather_than_truncating() {
    let dir = workspace();
    let channel = WorkspaceSourceChannel::open(dir.path()).unwrap();

    let refused = channel
        .search(&["verdict".to_owned()], &bounds(2, 64 * 1024 * 1024, 10))
        .expect_err("a workspace beyond the file bound was searched anyway");

    assert_eq!(refused, SourceSearchError::BoundExceeded);
}

/// Same for the byte bound, and it is a SEPARATE cell because the two fail independently: many
/// tiny files pass the byte bound and fail the file bound, one enormous file is the reverse.
#[test]
fn exceeding_the_byte_bound_refuses_rather_than_truncating() {
    let dir = workspace();
    let channel = WorkspaceSourceChannel::open(dir.path()).unwrap();

    let refused = channel
        .search(&["verdict".to_owned()], &bounds(10_000, 16, 10))
        .expect_err("a workspace beyond the byte bound was searched anyway");

    assert_eq!(refused, SourceSearchError::BoundExceeded);
}

/// `max_results` is a RANKING bound, not an admission bound: the search legitimately finishes and
/// returns its best N. Distinct from the two above on purpose -- refusing here would make an
/// ordinary successful search look like a breached ceiling.
#[test]
fn the_result_bound_caps_the_answer_without_refusing() {
    let dir = workspace();
    let channel = WorkspaceSourceChannel::open(dir.path()).unwrap();

    let hits = channel
        .search(
            &["verdict".to_owned()],
            &bounds(10_000, 64 * 1024 * 1024, 1),
        )
        .expect("a search within its scan bounds succeeds");

    assert_eq!(
        hits.len(),
        1,
        "the answer is capped, the search is not refused"
    );
}

/// No terms is not "everything" -- and this cell asserts the half the scoring does NOT already
/// give for free.
///
/// Found by sabotage: deleting the early return left this test GREEN, because a zero-term score
/// is zero and nothing is collected anyway. The blade was redundant with the logic, so the cell
/// proved nothing about the guard (`redundant-blade-blinds-the-sabotage`). Two different weights
/// live here and only one was being carried:
///
/// - CORRECTNESS ("empty matches nothing") is carried by the scoring, which cannot match on an
///   empty needle set. Asserted below, and it survives the guard's removal because it should.
/// - COST ("empty does not WALK the workspace") is carried by the guard alone. An unconstrained
///   walk is the denial of service the bounds exist for, and it is invisible to a result
///   assertion because the walk produces the same empty answer.
///
/// So the cost half is asserted where it is observable: with a scan bound of ZERO files, a
/// channel that walks breaches the ceiling and refuses, and a channel that returns early cannot.
#[test]
fn an_empty_term_list_returns_nothing_without_walking_the_workspace() {
    let dir = workspace();
    let channel = WorkspaceSourceChannel::open(dir.path()).unwrap();

    let hits = channel.search(&[], &generous()).unwrap();
    assert!(
        hits.is_empty(),
        "an empty query matched the workspace: {hits:?}"
    );

    // The bound that no walk can survive: one scanned file is already too many.
    let unwalked = channel
        .search(&[], &bounds(0, 64 * 1024 * 1024, 10))
        .expect("an empty query must not scan a single file, so no ceiling can be breached");
    assert!(
        unwalked.is_empty(),
        "the empty query answered without scanning: {unwalked:?}"
    );
}

/// A workspace that does not exist is UNAVAILABLE, never an empty answer. An empty answer would
/// read as "the repository does not contain this", which is the absence claim this whole
/// mechanism refuses to fabricate.
#[test]
fn a_missing_workspace_is_unavailable_not_an_empty_answer() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("no-such-workspace");

    let refused = WorkspaceSourceChannel::open(&missing)
        .expect_err("a channel opened over a workspace that does not exist");

    assert_eq!(refused, SourceSearchError::Unavailable);
    let _ = Path::new(&missing);
}

/// G's #622 P1, and the useful shame is that THIS CRATE ALREADY HAD THE FIX.
///
/// The channel walked the workspace with `read_dir` and followed whatever it found. A directory
/// symlink or a Windows junction pointing outside therefore served paths from outside the
/// workspace as repository evidence — the containment the whole D-042 chain exists for, defeated
/// by a link.
///
/// `workspace::resolve_within` has refused exactly this since #538, in two layers (a per-component
/// `symlink_metadata` walk, then a canonicalisation of the deepest existing ancestor back inside
/// the root), and it is tested by `workspace_containment.rs`. My own module doc calls
/// `source_reader` a "sibling" — I described the neighbourhood and did not look at what the
/// neighbour had already solved.
#[test]
fn a_directory_link_pointing_outside_is_not_served_as_evidence() {
    let dir = workspace();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(
        outside.path().join("secret.rs"),
        "fn verdict_ceiling_node() {}\n",
    )
    .unwrap();

    // A DIRECTORY link inside the workspace, pointing out of it. Skipped rather than silently
    // passing where the platform refuses to create one without privilege: a cell that cannot
    // arrange its own subject must say so, not report green.
    // On Windows a JUNCTION is the right arrangement, not a symlink: `mklink /J` needs no
    // privilege, so the cell actually runs instead of skipping — and `resolve_within`'s own
    // comment says a junction reports as a symlink through `symlink_metadata`, which is exactly
    // the surface under test. The first version of this cell used `symlink_dir`, was SKIPPED
    // silently on this machine, and passed green while testing nothing.
    let link = dir.path().join("escape-hatch");
    #[cfg(windows)]
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(outside.path())
        .output()
        .is_ok_and(|output| output.status.success());
    #[cfg(not(windows))]
    let made = std::os::unix::fs::symlink(outside.path(), &link).is_ok();
    assert!(
        made,
        "arrangement: the cell could not create the link it exists to test, so a green here would prove nothing"
    );

    let channel = WorkspaceSourceChannel::open(dir.path()).unwrap();
    let hits = channel
        .search(&["verdict".to_owned(), "ceiling".to_owned()], &generous())
        .expect("a link inside the tree is not itself a search failure");

    assert!(
        !hits.iter().any(|hit| hit.contains("escape-hatch")),
        "the channel served bytes from OUTSIDE the workspace through a link: {hits:?}"
    );
    assert!(
        !hits.iter().any(|hit| hit.contains("secret")),
        "the linked-to file reached the evidence set: {hits:?}"
    );
}

/// The TRAVERSAL bound, which the read bounds cannot express (G's #622 finding).
///
/// A tree that opens no files and reads no bytes still costs: directories the walk must enter,
/// entries every suffix filter rejects. `max_files_scanned` and `max_bytes_scanned` are both
/// satisfied by such a tree while the walk runs unbounded, so only a bound on what is VISITED
/// can stop it. Arranged as the case the other two are blind to: entries that are never opened.
#[test]
fn exceeding_the_traversal_bound_refuses_even_when_nothing_is_read() {
    let dir = tempfile::tempdir().unwrap();
    // Twenty entries with suffixes the channel does not serve: zero files opened, zero bytes
    // read, and both read bounds left generous.
    for index in 0..20 {
        std::fs::write(dir.path().join(format!("blob-{index}.bin")), b"x").unwrap();
    }
    let channel = WorkspaceSourceChannel::open(dir.path()).unwrap();

    let refused = channel
        .search(
            &["verdict".to_owned()],
            &SourceSearchBounds {
                max_entries_visited: 5,
                max_files_scanned: 10_000,
                max_bytes_scanned: 64 * 1024 * 1024,
                max_results: 10,
                max_terms: 64,
                max_term_bytes: 64 * 256,
            },
        )
        .expect_err("a walk beyond the traversal bound ran anyway");

    assert_eq!(refused, SourceSearchError::BoundExceeded);
}

/// The byte ceiling is reconciled against what was READ, not what metadata quoted.
///
/// A declared size is a price quote; the read is what is actually paid. A file that grows between
/// the two — or a filesystem that misreports — left the ceiling bounding a number nobody spent
/// (G's TOCTOU finding). This cell pins the honest direction: real contents, tiny ceiling, refusal.
#[test]
fn the_byte_ceiling_is_measured_against_the_bytes_that_arrived() {
    let dir = workspace();
    let channel = WorkspaceSourceChannel::open(dir.path()).unwrap();

    let refused = channel
        .search(&["verdict".to_owned()], &bounds(10_000, 8, 10))
        .expect_err("a search past the byte ceiling completed");

    assert_eq!(refused, SourceSearchError::BoundExceeded);
}

/// Codex #622: on Unix a backslash is a legal filename byte, but the downstream consumer
/// (`compile_plan_composed_inner` → `canonical_hit`) rewrites every `\` to `/`, so a hit like
/// `dir\evidence.rs` would name a DIFFERENT file (or none) in the plan. The channel refuses such
/// a name rather than forwarding it to be mangled.
///
/// Unix-only: on Windows `\` is the path separator, so a file cannot be named with one, and the
/// separator rewrite there is faithful. The cell exists where the bug does.
#[cfg(unix)]
#[test]
fn a_unix_filename_with_a_backslash_refuses_rather_than_being_mangled_downstream() {
    let dir = tempfile::tempdir().unwrap();
    // A real, legal Unix file whose name contains a backslash, carrying a matchable term.
    let weird = dir.path().join(r"dir\evidence.rs");
    std::fs::write(&weird, "verdict ceiling\n").unwrap();
    // A control file the search WOULD return, so a refusal is about the backslash, not emptiness.
    std::fs::write(dir.path().join("plain.rs"), "verdict ceiling\n").unwrap();

    let channel = WorkspaceSourceChannel::open(dir.path()).unwrap();
    let refused = channel
        .search(&["verdict".to_owned(), "ceiling".to_owned()], &generous())
        .unwrap_err();

    assert_eq!(
        refused,
        SourceSearchError::Unavailable,
        "a name the channel cannot forward consistently with canonical_hit must refuse the search"
    );
}

/// Codex #622: the ROOT itself being a link, not just an entry inside it. `open` used `is_dir()`,
/// which FOLLOWS the link, so a junction/symlink root would be stored as its link spelling while
/// every search walked the TARGET outside it. `open` now inspects the root with no-follow metadata
/// and refuses a reparse point. A junction is used (needs no privilege on Windows) so the cell runs.
#[test]
fn a_symlinked_workspace_root_is_refused_at_admission() {
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.rs"), "verdict ceiling\n").unwrap();
    let holder = tempfile::tempdir().unwrap();
    let link = holder.path().join("linked-root");

    #[cfg(windows)]
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(outside.path())
        .output()
        .is_ok_and(|output| output.status.success());
    #[cfg(not(windows))]
    let made = std::os::unix::fs::symlink(outside.path(), &link).is_ok();
    assert!(
        made,
        "arrangement: the cell could not create the linked root it exists to test"
    );

    let refused = WorkspaceSourceChannel::open(&link)
        .expect_err("a linked root was admitted, so searches would read outside it");
    assert_eq!(refused, SourceSearchError::Unavailable);
}

/// Codex #622: the channel honours the CALLER's `max_terms` / `max_term_bytes`, not hard-coded
/// constants. A tighter count ceiling refuses, and a tighter aggregate-byte ceiling refuses —
/// both read from `bounds`, so a caller's declared query budget is the one enforced.
#[test]
fn the_channel_honours_the_callers_term_ceilings() {
    let dir = workspace();
    let channel = WorkspaceSourceChannel::open(dir.path()).unwrap();

    // Tight COUNT: two terms over max_terms=1.
    let mut tight_count = generous();
    tight_count.max_terms = 1;
    assert_eq!(
        channel
            .search(&["verdict".to_owned(), "ceiling".to_owned()], &tight_count)
            .unwrap_err(),
        SourceSearchError::BoundExceeded,
        "two terms over the caller's max_terms=1 must refuse"
    );

    // COUNT checked before the byte sum: max_terms=0 refuses any non-empty slice, even one whose
    // aggregate bytes are tiny, without traversing it to sum (Codex #622 ordering).
    let mut zero_terms = generous();
    zero_terms.max_terms = 0;
    assert_eq!(
        channel.search(&["a".to_owned()], &zero_terms).unwrap_err(),
        SourceSearchError::BoundExceeded,
        "max_terms=0 must refuse before summing bytes"
    );

    // Tight AGGREGATE bytes: two 7-byte terms over max_term_bytes=8.
    let mut tight_bytes = generous();
    tight_bytes.max_term_bytes = 8;
    assert_eq!(
        channel
            .search(&["verdict".to_owned(), "ceiling".to_owned()], &tight_bytes)
            .unwrap_err(),
        SourceSearchError::BoundExceeded,
        "14 aggregate bytes over the caller's max_term_bytes=8 must refuse"
    );

    // A generous budget still SUCCEEDS with the same terms (control).
    assert!(
        channel
            .search(&["verdict".to_owned(), "ceiling".to_owned()], &generous())
            .is_ok(),
        "the same terms under a generous budget must not refuse"
    );
}

/// Codex #622: the read is capped at EXACTLY the remaining byte budget, never a sentinel byte over.
/// A single file whose size equals `max_bytes_scanned` fills the budget exactly and is refused
/// conservatively (the ceiling is bytes READ, and a refusal cannot un-read a sentinel); a file one
/// byte under is served. Under the old `remaining + 1` read the exact-fit file was served, so this
/// cell is red there.
#[test]
fn a_file_exactly_at_the_byte_ceiling_refuses_without_a_sentinel_read() {
    let make = |body: &str| {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("only.rs"), body).unwrap();
        dir
    };
    // "verdict\n" is 8 bytes and contains the search term.
    let body = "verdict\n";
    let n = body.len() as u64;

    // Budget == file size: filled exactly -> refuse.
    let dir = make(body);
    let channel = WorkspaceSourceChannel::open(dir.path()).unwrap();
    assert_eq!(
        channel
            .search(&["verdict".to_owned()], &bounds(10_000, n, 10))
            .unwrap_err(),
        SourceSearchError::BoundExceeded,
        "a file filling the byte budget exactly must refuse, not read a sentinel over it"
    );

    // Budget == file size + 1: fits with room -> served.
    let dir = make(body);
    let channel = WorkspaceSourceChannel::open(dir.path()).unwrap();
    let hits = channel
        .search(&["verdict".to_owned()], &bounds(10_000, n + 1, 10))
        .expect("a file one byte under the budget is served");
    assert_eq!(hits, vec!["only.rs".to_owned()]);
}

/// Codex #622: a symlink/junction in an INTERMEDIATE component of the root is resolved by `open`'s
/// canonicalize, and the search through it still works with repo-relative hits.
///
/// This is the BEHAVIOURAL half only. The stored root being the canonical spelling rather than the
/// link spelling cannot be witnessed from out here — the hit strings strip either prefix to the
/// same `evidence.rs` — so that assertion lives in the unit test beside the module, reading the
/// private field directly (K on #707: no production accessor is added for a test's sake).
#[test]
fn a_workspace_reached_through_a_symlinked_ancestor_is_searchable() {
    let target = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(target.path().join("inner")).unwrap();
    std::fs::write(target.path().join("inner/evidence.rs"), "verdict ceiling\n").unwrap();
    let holder = tempfile::tempdir().unwrap();
    let link = holder.path().join("link");

    #[cfg(windows)]
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(target.path())
        .output()
        .is_ok_and(|o| o.status.success());
    #[cfg(not(windows))]
    let made = std::os::unix::fs::symlink(target.path(), &link).is_ok();
    assert!(
        made,
        "arrangement: could not create the ancestor link under test"
    );

    // Open THROUGH the link: `link` is an intermediate component, `inner` the real final one.
    let channel = WorkspaceSourceChannel::open(&link.join("inner")).unwrap();

    // The search works through the ancestor and the hit is repo-relative — canonicalize did not
    // break it.
    let hits = channel
        .search(&["verdict".to_owned(), "ceiling".to_owned()], &generous())
        .expect("a workspace reached through a linked ancestor is searchable");
    assert_eq!(hits, vec!["evidence.rs".to_owned()], "{hits:?}");
}

/// Generated directories are skipped by NAME at any depth (#1065 review): a `node_modules/`
/// alone is tens of thousands of entries, and walking it spends the traversal ceiling on files
/// the answer cannot cite — turning a bounded search over the SOURCES into a refusal caused by
/// the ARTIFACTS. The directory entry itself is still counted, so the ceiling stays a ceiling.
#[test]
fn a_generated_directory_does_not_consume_the_traversal_bound() {
    let dir = tempfile::tempdir().unwrap();
    let write = |rel: &str, body: &str| {
        let path = dir.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    };
    write("src/verdict.rs", "fn verdict() {}\n");
    // Far more entries than the bound below, every one of them matching the query — and every
    // one of them under a generated directory, at two depths.
    for index in 0..40 {
        write(
            &format!("node_modules/pkg-{index}/index.js"),
            "export const verdict = 1;\n",
        );
        write(&format!("web/dist/chunk-{index}.js"), "verdict\n");
    }
    let channel = WorkspaceSourceChannel::open(dir.path()).unwrap();

    let hits = channel
        .search(
            &["verdict".to_owned()],
            &SourceSearchBounds {
                // Root: `node_modules`, `src`, `web`; then `src/verdict.rs`, `web/dist`. Five
                // entries visited if the generated trees are skipped; a hundred and more if not.
                max_entries_visited: 8,
                max_files_scanned: 10_000,
                max_bytes_scanned: 64 * 1024 * 1024,
                max_results: 10,
                max_terms: 64,
                max_term_bytes: 64 * 256,
            },
        )
        .expect("a search that skips generated trees stays inside the traversal bound");

    assert_eq!(hits, vec!["src/verdict.rs".to_owned()]);
}

/// Credential directories are skipped by NAME at any depth (#1065 review), not only at the root
/// where `EXCLUDED_PREFIXES` sees them: a nested checkout or a vendored copy carries its own
/// `.graphhelm/keyring`, and a prefix rule is blind to it.
#[test]
fn a_nested_credential_directory_is_never_entered() {
    let dir = tempfile::tempdir().unwrap();
    let write = |rel: &str, body: &str| {
        let path = dir.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    };
    write("src/verdict.rs", "fn verdict() {}\n");
    write(
        "apps/inner/.graphhelm/keyring/main.json",
        "{\"verdict\": \"secret\"}\n",
    );
    write(
        "apps/inner/.graphhelm/state.json",
        "{\"verdict\": \"state\"}\n",
    );
    write("ops/keyring/backup.json", "{\"verdict\": \"backup\"}\n");
    let channel = WorkspaceSourceChannel::open(dir.path()).unwrap();

    let hits = channel
        .search(&["verdict".to_owned()], &generous())
        .unwrap();

    assert_eq!(hits, vec!["src/verdict.rs".to_owned()]);
}

/// The factory's own process directories are skipped by NAME at any depth too (#1078 review):
/// `EXCLUDED_PREFIXES` sees `.factory/` and `.git/` only at the root, and a nested package or a
/// vendored checkout carries the same names deeper. `packages/app/.factory/notes.md` is never a
/// candidate, however well it matches.
#[test]
fn a_nested_process_directory_is_never_entered() {
    let dir = tempfile::tempdir().unwrap();
    let write = |rel: &str, body: &str| {
        let path = dir.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    };
    write("src/verdict.rs", "fn verdict() {}\n");
    write(
        "packages/app/.factory/notes.md",
        "# verdict verdict verdict\n",
    );
    write("packages/app/.FACTORY/board.md", "# verdict board\n");
    write("vendor/x/.git/hooks/README.md", "# verdict hook\n");
    write("tools/.superpowers/plan.md", "# verdict plan\n");
    let channel = WorkspaceSourceChannel::open(dir.path()).unwrap();

    let hits = channel
        .search(&["verdict".to_owned()], &generous())
        .unwrap();

    assert_eq!(hits, vec!["src/verdict.rs".to_owned()]);
}
