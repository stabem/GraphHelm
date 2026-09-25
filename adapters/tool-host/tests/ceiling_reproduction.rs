//! SEAL (c), NARROWED TO WHAT IT ACTUALLY MEASURES — and the narrowing is a correction of mine.
//!
//! This file was called "the ceiling reproduces" and it does not measure the ceiling. The 9/12
//! figure is the union of FOUR channels (graph BM25, semantic, name_pattern, lexical) against a
//! PINNED store; this cell runs the lexical channel alone against the LIVE repository. G named
//! it on #622 and is right: a seal whose title claims the decision-driving number while its
//! assertion checks one component is the same defect as a guard whose name outruns its subject —
//! and I spent the day naming that defect in other people's code and in my own.
//!
//! So the claim is cut to the measurement: **the lexical channel's own contribution reproduces.**
//! The composed 9/12 ceiling stays UNVERIFIED by any shipped code, and is recorded as such in
//! #637's neighbourhood rather than implied by a green tick here. Verifying it needs the pinned
//! store the artifacts were generated from, which is exactly the object #637 says disagrees with
//! the corpus — so the honest order is: settle that first, then seal the composed number.
//!
//! The prototype was a Python sweep over the snapshot: per case, keywords extracted from the
//! objective, files scored by DISTINCT term presence, top-K taken, union against the graph
//! channel. It measured a ceiling of 9/12 cases with complete recall. That number now steers a
//! decision — the spend gate stays shut — so the code that ships has to produce it too. If the
//! two disagree, one of them is measuring something else, and finding out WHICH is worth more
//! than either number.
//!
//! This cell runs only the LEXICAL half against the real corpus, because that is the half this
//! crate owns; the graph half needs a pinned index and lives with the benchmark. What it pins is
//! the lexical channel's own contribution: which cases it can carry alone.
//!
//! Ignored by default: it walks the repository and reads the frozen corpus, so it is a
//! deliberate measurement rather than a suite tax on every run.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use graphhelm_runtime::ports::{BoundedSourceSearch, SourceSearchBounds};
use graphhelm_tool_host::source_channel::WorkspaceSourceChannel;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn corpus() -> PathBuf {
    repository_root().join("tools/development-benchmark/corpus")
}

/// The prototype's term rule, transcribed: words of four or more letters, lowercased, common
/// English dropped, first appearance order, deduplicated. Transcribed rather than imported on
/// purpose — if the two implementations of the rule disagree, this cell should SEE it.
fn terms_of(objective: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "the", "a", "an", "of", "to", "in", "on", "for", "and", "or", "is", "are", "was", "were",
        "which", "what", "how", "why", "when", "where", "name", "this", "that", "with", "from",
        "by", "as", "at", "it", "its", "into", "never", "no", "not", "single", "file", "function",
        "decides", "mapping", "receive", "client", "stable", "does", "must", "two", "one", "each",
        "other",
    ];
    let mut seen = BTreeSet::new();
    let mut terms = Vec::new();
    let mut word = String::new();
    let push = |word: &mut String, terms: &mut Vec<String>, seen: &mut BTreeSet<String>| {
        if word.len() >= 4 {
            let lowered = word.to_lowercase();
            if !STOP.contains(&lowered.as_str()) && seen.insert(lowered.clone()) {
                terms.push(lowered);
            }
        }
        word.clear();
    };
    for character in objective.chars() {
        if character.is_ascii_alphabetic() || character == '_' {
            word.push(character);
        } else {
            push(&mut word, &mut terms, &mut seen);
        }
    }
    push(&mut word, &mut terms, &mut seen);
    terms
}

fn read_json(path: &Path) -> serde_json::Value {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("{} is unreadable: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

#[test]
#[ignore = "walks the repository: run deliberately, not as a suite tax"]
fn the_lexical_channels_own_contribution_reproduces_not_the_composed_ceiling() {
    let corpus = corpus();
    let manifest = read_json(&corpus.join("manifest.json"));
    let cases: Vec<String> = manifest["cases"]
        .as_array()
        .expect("the manifest lists cases")
        .iter()
        .map(|case| case["id"].as_str().expect("a case id").to_owned())
        .collect();
    assert_eq!(cases.len(), 12, "the corpus was frozen at twelve cases");

    let channel = WorkspaceSourceChannel::open(&repository_root())
        .expect("the repository root is a readable workspace");
    let bounds = SourceSearchBounds {
        max_entries_visited: 1_000_000,
        max_files_scanned: 100_000,
        max_bytes_scanned: 512 * 1024 * 1024,
        max_results: 10,
        max_terms: 64,
        max_term_bytes: 64 * 256,
    };

    let mut carried = Vec::new();
    let mut partial = Vec::new();
    for case in &cases {
        let objective = read_json(&corpus.join(format!("objectives/{case}.json")))["objective"]
            .as_str()
            .expect("an objective string")
            .to_owned();
        let required: BTreeSet<String> =
            read_json(&corpus.join(format!("oracle/{case}.json")))["requiredEvidence"]
                .as_array()
                .expect("required evidence")
                .iter()
                .map(|entry| entry.as_str().expect("a path").to_owned())
                .collect();

        let hits: BTreeSet<String> = channel
            .search(&terms_of(&objective), &bounds)
            .expect("the repository fits the declared bounds")
            .into_iter()
            .collect();

        let reached = required.intersection(&hits).count();
        if reached == required.len() {
            carried.push(case.clone());
        } else if reached > 0 {
            partial.push(case.clone());
        }
    }

    // NOT the 9/12 ceiling: this is the lexical half alone, against the live tree. The composed
    // number remains unverified by shipped code — see this file's header.
    //
    // The prototype's lexical half carried FOUR cases alone (benchmark-recall-order,
    // cli-gate-workspace, events-idempotency, runtime-capsule-bytes) and reached some evidence in
    // several more. The assertion is a FLOOR rather than an equality: the shipped channel walks
    // the live repository while the prototype walked a pinned snapshot, so the populations differ
    // by every commit since — an equality here would be a clock, not a seal.
    //
    // A number BELOW the floor means the shipped channel is weaker than the thing the 9/12
    // ceiling was computed from, and that ceiling is steering a spend decision. That is the
    // disagreement worth stopping for.
    // A seal that only says "ok" hides the measurement it was built to take. Printed on every
    // deliberate run so the number is READ, not inferred from a green tick.
    println!(
        "lexical channel alone: {}/12 cases carried {carried:?}; partial {partial:?}",
        carried.len()
    );
    assert!(
        carried.len() >= 4,
        "the shipped lexical channel carries {} cases alone, fewer than the {} the prototype \
         measured. The 9/12 ceiling was computed from the prototype and it is steering the spend \
         gate, so this disagreement is the finding: carried={carried:?}, partial={partial:?}",
        carried.len(),
        4
    );
}
