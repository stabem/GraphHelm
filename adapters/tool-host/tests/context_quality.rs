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

/// The holdout's frozen corpus (#1705), excluded from every LIVE count for the same reason.
const HOLDOUT_CORPUS_PREFIX: &str = "adapters/tool-host/tests/fixtures/context-holdout/tree/";

/// The second holdout's frozen corpus (#1705), excluded from every LIVE count for the same reason.
const HOLDOUT_V2_CORPUS_PREFIX: &str = "adapters/tool-host/tests/fixtures/context-holdout-v2/tree/";

/// The live count's exclusion: the test itself and every frozen corpus.
fn is_instrument(path: &str) -> bool {
    path == SELF_PATH
        || path.starts_with(FROZEN_CORPUS_PREFIX)
        || path.starts_with(HOLDOUT_CORPUS_PREFIX)
        || path.starts_with(HOLDOUT_V2_CORPUS_PREFIX)
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

/// #1705: the HOLDOUT. Ten questions written in a maintainer's plain words, each with the one
/// file that answers it, fixed before the ranking was run on any of them and committed before the
/// baseline was measured. Six ask about implementation, four about normative documents.
/// This set is never tuned against: a ranking change is developed on `CASES` above and only then
/// read here. Rewriting a question or its oracle after seeing a result is retuning; freeze a new
/// holdout instead (fixtures/context-holdout/MANIFEST.md).
const HOLDOUT_CASES: [(&str, &str); 10] = [
    (
        "where do we make sure a spawned helper program never pops up a visible window on windows",
        "core/process/src/lib.rs",
    ),
    (
        "how big can a single event or the journal file get before the store says no",
        "core/events/src/limits.rs",
    ),
    (
        "what removes layout-only fields before the graph fingerprint is computed",
        "core/graph/src/canonical.rs",
    ),
    (
        "how does the simulator stop a graph that loops forever",
        "core/simulation/src/engine.rs",
    ),
    (
        "which obligations must be satisfied before a node may move to its next state",
        "core/policy/src/evaluator.rs",
    ),
    (
        "if one check fails while applying a draft, how do we keep the current version untouched",
        "core/governor/src/apply.rs",
    ),
    (
        "who is allowed to press merge on a pull request and what must they recheck first",
        "docs/process/DELIVERY.md",
    ),
    (
        "how do several agent sessions on one machine share the build without stepping on each other",
        "docs/process/LANES.md",
    ),
    (
        "what are the values every code change follows, and which step file do I read next",
        "KEEL.md",
    ),
    (
        "which crate may depend on which, and what is the studio allowed to import",
        "AGENTS.md",
    ),
];

/// The holdout floor over its FROZEN corpus: the baseline measured when the holdout was frozen
/// (the PR of #1705 carries the pre-registration and the measured value). Asserted at the
/// measured value for the same reason as the development floor: corpus and ranking are both
/// fixed, so any drop is a change in the ranking.
const HOLDOUT_HITS_AT_3_FLOOR: usize = 4;

/// #1705: HOLDOUT v2. Ten questions in a contributor's plain words, each with the one file that
/// answers it. Written by reading the repository's code and documents, before the ranking code
/// or any ranking result was read, and committed before the ranking was run on any of them.
/// Seven ask about implementation, three about normative or process documents. Never tuned
/// against: a rewritten question or oracle after a result is seen is retuning; freeze a new
/// holdout instead (fixtures/context-holdout-v2/MANIFEST.md).
const HOLDOUT_V2_CASES: [(&str, &str); 10] = [
    (
        "how does the scheduler work out which steps of a graph can start right now",
        "core/execution/src/ready.rs",
    ),
    (
        "why can the final message to the owner never say everything went fine when the work actually failed",
        "core/runtime/src/owner_output.rs",
    ),
    (
        "why does the checker only complain about code I just added, and how do I excuse one finding",
        "core/policy/src/keel_lines.rs",
    ),
    (
        "how do we tell which stage of delivery a stuck pull request is in and who should fix it",
        "core/execution/src/autopilot.rs",
    ),
    (
        "how deeply nested or how large can a JSON document be before it is rejected",
        "core/schema/src/bounded_value.rs",
    ),
    (
        "why can a model route not be both a paid API key and a subscription login",
        "core/gateway/src/manifest.rs",
    ),
    (
        "when a step keeps failing its checks, how do we decide to hand it to a different agent",
        "core/runtime/src/delegation.rs",
    ),
    (
        "what attacks did we think about for the journal and the stored evidence, and how is each one tested",
        "docs/security/EVENT_EVIDENCE_STORE_THREAT_MODEL.md",
    ),
    (
        "how do I back up the event data and bring it back on a new server after an upgrade",
        "docs/operations/VPS_UPGRADE_BACKUP_RESTORE.md",
    ),
    (
        "what must a reviewer who never saw the author's reasoning read and run before giving a verdict",
        "keel/KEEL_REVIEW.md",
    ),
];

/// The holdout v2 floor over its FROZEN corpus: 2/10, measured on main's ranker (`d8f173ef`) in
/// the commit after the cases were pushed (pre-registration `b2dd4ef7`). Asserted at the measured
/// value: corpus and ranking are fixed, so any drop is a change in the ranking. The low value is
/// the point of a fresh holdout, not a defect of it. Raised to 7/10 by the BM25 ranker, accepted
/// under the rule pre-registered on #1705 before this set was read: +5 over main, no main hit
/// lost, dev 10/10. The v1 holdout floor stays at 4 because BM25 lost one v1 hit (the merge
/// question, oracle `docs/process/DELIVERY.md`), already on record.
const HOLDOUT_V2_HITS_AT_3_FLOOR: usize = 7;

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

fn holdout_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/context-holdout")
}

fn holdout_corpus() -> PathBuf {
    holdout_root().join("tree").canonicalize().unwrap()
}

fn holdout_v2_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/context-holdout-v2")
}

fn holdout_v2_corpus() -> PathBuf {
    holdout_v2_root().join("tree").canonicalize().unwrap()
}

/// Whether a measurement asserts the capsule shape (frozen corpus) or only prints it (live tree).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Capsules {
    Asserted,
    Printed,
}

/// What one measured run returns: the hits, the tokens saved, and each case's top three in case
/// order (what the determinism test compares).
struct Measured {
    hits: usize,
    saved: u64,
    top3: Vec<Vec<String>>,
}

/// One measured run of `cases` over `root`.
///
/// `Capsules::Asserted` also asserts that every case ran with a capsule and shipped a source.
/// That is a property of the FROZEN corpus. On the live tree it is a property of the machine
/// (#1086 item 9): which agent worktrees and generated trees sit in the checkout decides whether
/// a search crosses its ceiling, so the live run prints the fallback and the source count and
/// asserts neither.
fn measure(
    root: &Path,
    label: &str,
    cases: &[(&str, &str)],
    excluded: fn(&str) -> bool,
    capsules: Capsules,
) -> Measured {
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
    let mut tops = Vec::new();
    let indent = " ".repeat(7);
    println!(
        "context quality over {label} {} ({} cases)",
        root.display(),
        cases.len()
    );
    for &(objective, expected) in cases {
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
        tops.push(top3.iter().map(|path| (*path).to_owned()).collect());
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
    let hit_rate = hits as f64 / cases.len() as f64;
    println!(
        "hit rate@3 (success@3) over {label} = {hits}/{} = {hit_rate:.2}; tokens eligible {eligible_total} shipped {shipped_total} saved {saved_total} (bytes-div-4/v1)",
        cases.len()
    );
    Measured {
        hits,
        saved: saved_total,
        top3: tops,
    }
}

#[test]
fn hit_rate_at_3_over_the_frozen_corpus_holds_the_measured_floor() {
    let Measured {
        hits,
        saved: saved_total,
        ..
    } = measure(
        &frozen_corpus(),
        "frozen corpus",
        &CASES,
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
    let hits = measure(
        &repository_root(),
        "live repository",
        &CASES,
        is_instrument,
        Capsules::Printed,
    )
    .hits;
    assert!(hits <= CASES.len(), "hits are bounded by the case count");
}

/// #1705: ranking quality, separated from drift: the holdout over its own frozen corpus.
#[test]
fn holdout_hits_at_3_over_its_frozen_corpus_hold_the_baseline() {
    let hits = measure(
        &holdout_corpus(),
        "holdout frozen corpus",
        &HOLDOUT_CASES,
        |_| false,
        Capsules::Asserted,
    )
    .hits;
    assert!(
        hits >= HOLDOUT_HITS_AT_3_FLOOR,
        "holdout hits@3 {hits}/{} fell below the frozen baseline {HOLDOUT_HITS_AT_3_FLOOR}",
        HOLDOUT_CASES.len()
    );
}

/// #1705: the same holdout questions over the moving tree: printed, never asserted. The gap
/// between this number and the frozen one is corpus drift, not ranking quality.
#[test]
fn holdout_hits_at_3_over_this_repository_are_printed_not_asserted() {
    let hits = measure(
        &repository_root(),
        "holdout live repository",
        &HOLDOUT_CASES,
        is_instrument,
        Capsules::Printed,
    )
    .hits;
    assert!(
        hits <= HOLDOUT_CASES.len(),
        "hits are bounded by the case count"
    );
}

/// #1705: holdout v2 over its own frozen corpus.
#[test]
fn holdout_v2_hits_at_3_over_its_frozen_corpus_hold_the_baseline() {
    let hits = measure(
        &holdout_v2_corpus(),
        "holdout v2 frozen corpus",
        &HOLDOUT_V2_CASES,
        |_| false,
        Capsules::Asserted,
    )
    .hits;
    assert!(
        hits >= HOLDOUT_V2_HITS_AT_3_FLOOR,
        "holdout v2 hits@3 {hits}/{} fell below the frozen baseline {HOLDOUT_V2_HITS_AT_3_FLOOR}",
        HOLDOUT_V2_CASES.len()
    );
}

/// #1705: holdout v2 questions over the moving tree: printed, never asserted.
#[test]
fn holdout_v2_hits_at_3_over_this_repository_are_printed_not_asserted() {
    let hits = measure(
        &repository_root(),
        "holdout v2 live repository",
        &HOLDOUT_V2_CASES,
        is_instrument,
        Capsules::Printed,
    )
    .hits;
    assert!(
        hits <= HOLDOUT_V2_CASES.len(),
        "hits are bounded by the case count"
    );
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

/// A corpus is frozen by its manifest: the set of files AND the bytes of each must match, or the
/// floor is being measured over something else; and every oracle path must be in it.
fn assert_corpus_matches_manifest(fixture: &Path, corpus: &Path, cases: &[(&str, &str)]) {
    let manifest = std::fs::read_to_string(fixture.join("MANIFEST.md")).unwrap();
    let pinned = manifest_digests(&manifest);
    let present = files_under(corpus);
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
    for (_, expected) in cases {
        assert!(
            pinned.contains_key(*expected),
            "{expected}: every expected file is in the frozen corpus"
        );
    }
}

#[test]
fn the_frozen_corpus_matches_its_manifest() {
    assert_corpus_matches_manifest(&fixture_root(), &frozen_corpus(), &CASES);
}

#[test]
fn the_holdout_corpus_matches_its_manifest() {
    assert_corpus_matches_manifest(&holdout_root(), &holdout_corpus(), &HOLDOUT_CASES);
}

#[test]
fn the_holdout_v2_corpus_matches_its_manifest() {
    assert_corpus_matches_manifest(&holdout_v2_root(), &holdout_v2_corpus(), &HOLDOUT_V2_CASES);
}

/// #1705: the evaluator itself is deterministic: two full holdout runs over the frozen corpus
/// give the same top three for every case, in the same order.
#[test]
fn the_holdout_evaluation_is_deterministic_across_two_runs() {
    let run = || {
        measure(
            &holdout_corpus(),
            "holdout determinism",
            &HOLDOUT_CASES,
            |_| false,
            Capsules::Asserted,
        )
        .top3
    };
    let first = run();
    assert_eq!(first.len(), HOLDOUT_CASES.len());
    assert_eq!(first, run());
}

/// Directory names the size corpus never copies: version control, build output, caches, and the
/// measuring instruments (this crate's tests hold every question verbatim).
const SIZE_CORPUS_SKIPPED: [&str; 6] = [
    ".git",
    "target",
    "node_modules",
    ".graphhelm",
    "__pycache__",
    ".pytest_cache",
];

/// Copy the repository's sources into `destination`, skipping [`SIZE_CORPUS_SKIPPED`] and this
/// crate's `tests/`; returns (files, bytes).
fn copy_sources(source: &Path, destination: &Path, instruments: &Path) -> (u64, u64) {
    let (mut files, mut bytes) = (0u64, 0u64);
    let mut stack = vec![(source.to_path_buf(), destination.to_path_buf())];
    while let Some((from, to)) = stack.pop() {
        if from == instruments {
            continue;
        }
        std::fs::create_dir_all(&to).unwrap();
        for entry in std::fs::read_dir(&from).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name();
            let kind = entry.file_type().unwrap();
            if kind.is_dir() {
                if !SIZE_CORPUS_SKIPPED.iter().any(|skip| name == *skip) {
                    stack.push((entry.path(), to.join(&name)));
                }
            } else if kind.is_file() {
                bytes += std::fs::copy(entry.path(), to.join(&name)).unwrap();
                files += 1;
            }
        }
    }
    (files, bytes)
}

/// #2304: the source search keeps answering at ten times the repository's size, in bounded time
/// and bytes. The 1x corpus is one copy of this repository's sources under `copy0/`; the 10x
/// corpus is ten copies, `copy0/` to `copy9/`. The holdout v2 questions run on both.
///
/// Limits, fixed before the first run (issue #2304):
/// 1. at 10x no case is refused and every case ships a capsule (`retrieval_fallbacks == 0`);
/// 2. the median per-question time at 10x is at most 15 times the 1x median (`keel.bigo.scale`);
/// 3. every capsule ships at most `MAX_TOTAL_CANDIDATE_BYTES`, at both sizes.
/// Hit@3 is printed, not asserted: ten copies make every expected file appear ten times.
#[test]
#[ignore = "copies the repository ten times into the temp dir (hundreds of MB); run explicitly"]
fn the_search_answers_at_ten_times_the_repository_in_bounded_time_and_bytes() {
    use graphhelm_runtime::context::MAX_TOTAL_CANDIDATE_BYTES;
    use std::time::{Duration, Instant};

    let base = std::env::temp_dir().join(format!("graphhelm-search-size-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = repository_root();
    let instruments = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let one = base.join("1x");
    let ten = base.join("10x");
    let (files, bytes) = copy_sources(&root, &one.join("copy0"), &instruments);
    for copy in 0..10 {
        copy_sources(&root, &ten.join(format!("copy{copy}")), &instruments);
    }
    println!(
        "size corpus: 1x = {files} files, {bytes} bytes; 10x = {} files, {} bytes",
        files * 10,
        bytes * 10
    );

    struct Row {
        refused: bool,
        fallbacks: u64,
        shipped_bytes: u64,
        elapsed: Duration,
        hit: bool,
    }
    let run = |corpus: &Path, label: &str| -> Vec<Row> {
        let ports = ContextPorts {
            search: Arc::new(WorkspaceSourceChannel::open(corpus).unwrap()),
            reader: Arc::new(WorkspaceExcerptReader::open(corpus).unwrap()),
            ledger: ContextLedger::new(),
            execution_tree: None,
        };
        HOLDOUT_V2_CASES
            .iter()
            .map(|&(objective, expected)| {
                let terms = objective_terms(objective);
                let started = Instant::now();
                let ranked = ports.search.search(&terms, &SEARCH_BOUNDS);
                let compiled = retrieve_and_compile(
                    ports.search.as_ref(),
                    ports.reader.as_ref(),
                    &terms,
                    "size/measure/a1",
                    32 * 1024,
                );
                let elapsed = started.elapsed();
                let refused = ranked.is_err();
                let hit = ranked
                    .map(|paths| paths.iter().take(3).any(|path| path.ends_with(expected)))
                    .unwrap_or(false);
                let row = Row {
                    refused,
                    fallbacks: compiled.summary.retrieval_fallbacks,
                    shipped_bytes: compiled.summary.compiled_input_tokens * 4,
                    elapsed,
                    hit,
                };
                println!(
                    "  [{label}] {} refused={} fallbacks={} shipped_bytes={} ms={}",
                    if row.hit { "hit " } else { "miss" },
                    row.refused,
                    row.fallbacks,
                    row.shipped_bytes,
                    row.elapsed.as_millis()
                );
                row
            })
            .collect()
    };
    let median = |rows: &[Row]| -> Duration {
        let mut times: Vec<Duration> = rows.iter().map(|row| row.elapsed).collect();
        times.sort();
        times[times.len() / 2]
    };
    let small = run(&one, "1x");
    let large = run(&ten, "10x");
    let _ = std::fs::remove_dir_all(&base);
    let (m1, m10) = (median(&small), median(&large));
    println!(
        "median ms 1x = {} 10x = {} ratio = {:.2}; hits@3 1x = {} 10x = {}",
        m1.as_millis(),
        m10.as_millis(),
        m10.as_secs_f64() / m1.as_secs_f64().max(1e-9),
        small.iter().filter(|row| row.hit).count(),
        large.iter().filter(|row| row.hit).count()
    );
    for (label, rows) in [("1x", &small), ("10x", &large)] {
        for row in rows {
            assert!(
                row.shipped_bytes <= MAX_TOTAL_CANDIDATE_BYTES,
                "{label}: a capsule shipped {} bytes",
                row.shipped_bytes
            );
        }
    }
    assert!(
        large.iter().all(|row| !row.refused && row.fallbacks == 0),
        "10x: a case was refused or fell back"
    );
    assert!(
        m10.as_secs_f64() <= 15.0 * m1.as_secs_f64(),
        "10x median {m10:?} exceeds 15 times the 1x median {m1:?}"
    );
}

/// #2319 (Tsunade): collapsing identical copies never bypasses the secret filter. A secret-shaped
/// file is copied three times beside one ordinary file; after the collapse one copy reaches the
/// compiler, which refuses it as before, and no copy of the secret ships. Cost: temp files only.
#[test]
fn a_duplicated_secret_shaped_file_never_ships_after_the_collapse() {
    let root = tempfile::tempdir().unwrap();
    let write = |relative: &str, body: &str| {
        let path = root.path().join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    };
    let secret = "-----BEGIN RSA PRIVATE KEY-----\nzebraquill zebraquill zebraquill\n-----END RSA PRIVATE KEY-----\n";
    for copy in ["a", "b", "c"] {
        write(&format!("{copy}/key.txt"), secret);
    }
    write("docs/zebra.md", "zebraquill notes about the topic\n");
    let terms = objective_terms("where is the zebraquill");
    let search = WorkspaceSourceChannel::open(root.path()).unwrap();
    let reader = WorkspaceExcerptReader::open(root.path()).unwrap();
    let compiled = retrieve_and_compile(&search, &reader, &terms, "dedup/secret/a1", 32 * 1024);
    assert_eq!(
        compiled.summary.sources,
        ["docs/zebra.md"],
        "{:?}",
        compiled.summary
    );
    assert_eq!(
        compiled.summary.candidates_secret_shaped, 1,
        "one copy reached the filter"
    );
    assert!(
        !compiled.text.contains("PRIVATE KEY"),
        "no copy of the secret shipped"
    );
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
