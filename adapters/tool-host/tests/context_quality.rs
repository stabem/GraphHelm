//! #1065: a deterministic, offline quality measurement of the context chain.
//!
//! Ten objective -> expected-file pairs, each written from a real file's own vocabulary before
//! the chain was run against them. The floor is asserted over a FROZEN corpus
//! (`tests/fixtures/context-quality/tree`, fifteen files copied byte-for-byte from this
//! repository and pinned by sha256 in `MANIFEST.md`), so the number is a property of those bytes
//! and of the ranking, and of nothing that moves. The first shape asserted the same floor over
//! the live repository tree, and three merges of documentation moved it from 0.60 to 0.40 with
//! no retrieval code changing: a floor over a moving corpus is not a deterministic test. The live
//! tree is still measured here and the number printed, with no floor.
//!
//! It lives in `adapters/tool-host` rather than `core/runtime` because the workspace-backed
//! channel is what is being measured, and the runtime crate must not name this adapter even as
//! a dev-dependency (the source-invariant suite pins the arrow from both sides).
//!
//! What this measures and what it does not: the search ranks by (distinct terms matched, path)
//! — a lexical channel with no idf, no proximity, no structure. Hit rate@3 (success@3: the one
//! relevant file is in the top three; with one relevant document per query precision@3 would
//! max out at 1/3, so that is not the number measured) here is the number that lexical
//! retrieval alone earns on real questions; the recipe's later stages (#302:
//! entity and graph-neighbour RRF, vectors, authority) are the ones that would move it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use graphhelm_runtime::context::{
    ContextLedger, ContextPorts, SEARCH_BOUNDS, objective_terms, retrieve_and_compile,
};
use graphhelm_tool_host::source_channel::WorkspaceSourceChannel;
use graphhelm_tool_host::source_reader::WorkspaceExcerptReader;
use sha2::{Digest, Sha256};

/// The floor over the FROZEN corpus, measured on 2026-09-14 at `1ac2438e` (the corpus origin):
/// 9 of 10 expected files in the top three, hit rate@3 (success@3) = 0.90; tokens over the ten cases
/// eligible 902,886 / shipped 55,782 / saved 847,104 (bytes-div-4/v1). Stated, not tuned: the
/// one miss (`context_accounting.rs`, outranked by the changelog, the decision register and
/// `serve/mod.rs`) is a real property of a term-count ranking, and the corpus was not chosen for
/// the number — the targets are fixed by the objectives and the decoys are the documents observed
/// displacing them on the live tree. The number is higher than the live tree's because fifteen
/// files compete instead of the whole repository; it is asserted at the measured value because
/// the corpus is frozen and the ranking is deterministic, so any drop is a change in the ranking.
/// For the record, the live tree measured 6/10 at `0e398e75` (eligible 1,629,696 / shipped 60,713
/// / saved 1,568,983; after the secret-shape refusal 1,556,434 / 61,810 / 1,494,624) and 4/10 at
/// `1ac2438e` after three merges of documentation, which is why the floor moved off it.
const HIT_RATE_AT_3_FLOOR: f64 = 0.9;

/// This test's own path, repository-relative — excluded from the LIVE count only (it holds every
/// objective verbatim, so it would rank first for all ten and measure the test rather than the
/// tree). The frozen corpus does not contain it.
const SELF_PATH: &str = "adapters/tool-host/tests/context_quality.rs";

/// The frozen corpus, repository-relative — excluded from the LIVE count for the same reason:
/// it is the instrument, not the sample. A copy of `CHANGELOG.md` under this prefix would
/// otherwise take a top-three slot from the original and the live number would measure the
/// fixture rather than the tree.
const FROZEN_CORPUS_PREFIX: &str = "adapters/tool-host/tests/fixtures/context-quality/tree/";

/// The live count's exclusion: the test itself and its frozen corpus.
fn is_instrument(path: &str) -> bool {
    path == SELF_PATH || path.starts_with(FROZEN_CORPUS_PREFIX)
}

/// Objective -> the file a maintainer would open. Each objective is a question a maintainer
/// might actually type, phrased in that file's own words. The paths are the same string in the
/// frozen corpus and in the live tree, because the corpus keeps the repository's layout.
const CASES: [(&str, &str); 10] = [
    (
        "why does serve refuse a non-loopback bind address",
        "apps/cli/src/commands/serve/mod.rs",
    ),
    (
        "which directory prefixes does the workspace source channel exclude and why is a junction not a directory",
        "adapters/tool-host/src/source_channel.rs",
    ),
    (
        "fit required context within a byte budget and refuse rather than trim, with an expansion request",
        "core/runtime/src/context_compiler.rs",
    ),
    (
        "the execution accounting receipt binds the persisted execution_started event hash and actor",
        "core/runtime/src/context_accounting.rs",
    ),
    (
        "drive an execution to quiescence with concurrent work and serialized writes through spawn_blocking",
        "core/runtime/src/driver.rs",
    ),
    (
        "assemble the prompt from the agent ephemeral purpose, instructions and schemas in a fixed field order",
        "core/runtime/src/prompt.rs",
    ),
    (
        "the source reader's current snapshot derives from tree_generation and fails closed with an unreadable marker",
        "adapters/tool-host/src/source_reader.rs",
    ),
    (
        "validate a retrieval coverage receipt: pagination, declared limits, broker record binding and canonical hits",
        "core/runtime/src/retrieval.rs",
    ),
    (
        "the gate registry port answers both the suite digest and the evaluation for a gate id",
        "core/runtime/src/ports.rs",
    ),
    (
        "compile-context decision maps the budget refusal to an exit code on the CLI and a status over HTTP",
        "apps/cli/src/commands/development.rs",
    ),
];

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/context-quality")
}

fn frozen_corpus() -> PathBuf {
    fixture_root().join("tree").canonicalize().unwrap()
}

/// Whether a measurement asserts the capsule shape (frozen corpus) or only prints it (live tree).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Capsules {
    Asserted,
    Printed,
}

/// One measured run of the ten cases over `root`. Returns (hits, eligible, shipped, saved).
///
/// `Capsules::Asserted` also asserts that every case ran with a capsule and shipped a source.
/// That is a property of the FROZEN corpus. On the live tree it is a property of the machine
/// (#1086 item 9): which agent worktrees and generated trees sit in the checkout decides whether
/// a search crosses its ceiling, so the live run prints the fallback and the source count and
/// asserts neither.
fn measure(
    root: &Path,
    label: &str,
    excluded: fn(&str) -> bool,
    capsules: Capsules,
) -> (usize, u64, u64, u64) {
    let channel = WorkspaceSourceChannel::open(root).unwrap();
    let reader = WorkspaceExcerptReader::open(root).unwrap();
    let ports = ContextPorts {
        search: Arc::new(channel),
        reader: Arc::new(reader),
        ledger: ContextLedger::new(),
        execution_tree: None,
    };

    let mut hits = 0usize;
    let mut eligible_total = 0u64;
    let mut shipped_total = 0u64;
    let mut saved_total = 0u64;
    let indent = " ".repeat(7);
    println!(
        "context quality over {label} {} ({} cases)",
        root.display(),
        CASES.len()
    );
    for (objective, expected) in CASES {
        let terms = objective_terms(objective);
        let ranked = match (ports.search.search(&terms, &SEARCH_BOUNDS), capsules) {
            (Ok(ranked), _) => ranked,
            (Err(error), Capsules::Asserted) => {
                panic!("{objective}: the search refused: {error:?}")
            }
            (Err(error), Capsules::Printed) => {
                println!("  [refused] {objective}: {error:?}");
                Vec::new()
            }
        };
        let top3: Vec<&str> = ranked
            .iter()
            .map(String::as_str)
            .filter(|path| !excluded(path))
            .take(3)
            .collect();
        let hit = top3.contains(&expected);
        hits += usize::from(hit);
        let compiled = retrieve_and_compile(
            ports.search.as_ref(),
            ports.reader.as_ref(),
            &terms,
            "quality/measure/a1",
            graphhelm_runtime::context::DEFAULT_BUDGET_BYTES,
        );
        let summary = &compiled.summary;
        eligible_total += summary.eligible_candidate_tokens;
        shipped_total += summary.compiled_input_tokens;
        saved_total += summary.tokens_saved;
        let verdict = if hit { "hit" } else { "miss" };
        println!("  [{verdict}] {objective}");
        println!("{indent}expected {expected}");
        println!("{indent}top3 {top3:?}");
        println!("{indent}terms {terms:?}");
        println!(
            "{indent}eligible {} shipped {} saved {} (tokens, {}) sources {} fallback {:?}",
            summary.eligible_candidate_tokens,
            summary.compiled_input_tokens,
            summary.tokens_saved,
            summary.estimator,
            summary.sources.len(),
            summary.fallback,
        );
        if capsules == Capsules::Asserted {
            assert_eq!(
                summary.retrieval_fallbacks, 0,
                "{objective}: the chain ran with a capsule"
            );
            assert!(
                !summary.sources.is_empty(),
                "{objective}: every case ships at least one source"
            );
        }
    }
    let hit_rate = hits as f64 / CASES.len() as f64;
    println!(
        "hit rate@3 (success@3) = {hits}/{} = {hit_rate:.2}; tokens eligible {eligible_total} shipped {shipped_total} saved {saved_total} (bytes-div-4/v1)",
        CASES.len()
    );
    (hits, eligible_total, shipped_total, saved_total)
}

#[test]
fn hit_rate_at_3_over_the_frozen_corpus_holds_the_measured_floor() {
    let (hits, _, _, saved_total) = measure(
        &frozen_corpus(),
        "frozen corpus",
        |_| false,
        Capsules::Asserted,
    );
    let hit_rate = hits as f64 / CASES.len() as f64;
    assert!(
        hit_rate >= HIT_RATE_AT_3_FLOOR,
        "hit rate@3 {hit_rate:.2} fell below the measured floor {HIT_RATE_AT_3_FLOOR}"
    );
    assert!(
        saved_total > 0,
        "the capsule must ship less than the eligible candidates would have cost"
    );
}

/// The live tree is a MOVING corpus: every merge changes what outranks what. Measured and
/// printed so a reader of the gate log can watch the number, never asserted against a floor.
#[test]
fn hit_rate_at_3_over_this_repository_is_printed_not_asserted() {
    let (hits, _, _, _) = measure(
        &repository_root(),
        "live repository",
        is_instrument,
        Capsules::Printed,
    );
    assert!(hits <= CASES.len(), "hits are bounded by the case count");
}

/// Every file under `root`, repository-relative with forward slashes, sorted — never in
/// `read_dir` order.
fn files_under(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let relative = path.strip_prefix(root).unwrap();
                out.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    out.sort();
    out
}

/// The `| \`path\` | \`sha256\` |` rows of `MANIFEST.md`, path -> digest.
fn manifest_digests(manifest: &str) -> BTreeMap<String, String> {
    manifest
        .lines()
        .filter_map(|line| {
            let mut cells = line
                .split('|')
                .map(str::trim)
                .filter(|cell| !cell.is_empty());
            let path = cells.next()?.strip_prefix('`')?.strip_suffix('`')?;
            let digest = cells.next()?.strip_prefix('`')?.strip_suffix('`')?;
            (digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
                .then(|| (path.to_owned(), digest.to_owned()))
        })
        .collect()
}

/// The corpus is frozen by its manifest: the set of files AND the bytes of each must match, or
/// the floor above is being measured over something else.
#[test]
fn the_frozen_corpus_matches_its_manifest() {
    let manifest = std::fs::read_to_string(fixture_root().join("MANIFEST.md")).unwrap();
    let pinned = manifest_digests(&manifest);
    let corpus = frozen_corpus();
    let present = files_under(&corpus);
    let pinned_paths: Vec<&String> = pinned.keys().collect();
    let present_paths: Vec<&String> = present.iter().collect();
    assert_eq!(
        present_paths, pinned_paths,
        "the files under tree/ must be exactly the paths MANIFEST.md pins"
    );
    for (path, expected) in &pinned {
        let bytes = std::fs::read(corpus.join(path)).unwrap();
        let actual = hex::encode(Sha256::digest(&bytes));
        assert_eq!(
            &actual, expected,
            "{path}: bytes differ from the manifest's digest"
        );
    }
    for (_, expected) in CASES {
        assert!(
            pinned.contains_key(expected),
            "{expected}: every expected file is in the frozen corpus"
        );
    }
}

#[test]
fn the_measurement_is_deterministic_across_two_runs() {
    let root = frozen_corpus();
    let (objective, _) = CASES[0];
    let terms = objective_terms(objective);
    let run = || {
        let channel = WorkspaceSourceChannel::open(&root).unwrap();
        let reader = WorkspaceExcerptReader::open(&root).unwrap();
        retrieve_and_compile(
            &channel,
            &reader,
            &terms,
            "quality/determinism/a1",
            32 * 1024,
        )
    };
    assert_eq!(run(), run());
}
