//! Offline paired experiment for the Keel contract index.
//!
//! Observable contract: on one frozen fixture tree and fixed terms, indexed
//! retrieval returns every path found by the bounded source reader. A missing
//! index is refused and counted; a separate source probe demonstrates the
//! available exact-source observer. The test catches a silent indexed false
//! negative and an unclassified index refusal.

use assert_cmd::Command;
use serde_json::Value;
use std::{
    collections::BTreeSet, fs, path::Path, process::Command as ProcessCommand, time::Instant,
};

const QUERIES: &[&str] = &["alpha_contract", "BetaContract", "gamma_contract"];

#[test]
fn paired_index_and_source_reader_report_recall_and_costs() {
    let fixture = tempfile::tempdir().unwrap();
    write_fixture(fixture.path());
    git(fixture.path(), &["init", "--quiet"]);
    git(fixture.path(), &["add", "."]);
    git(
        fixture.path(),
        &[
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-m",
            "fixture",
        ],
    );

    let output = tempfile::tempdir().unwrap();
    let index = output.path().join("index.json");
    let build_started = Instant::now();
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args(["scan", "--repo"])
        .arg(path(fixture.path()))
        .args(["--out"])
        .arg(path(&index))
        .assert()
        .success();
    let index_build_us = build_started.elapsed().as_micros();
    let index_bytes = fs::metadata(&index).unwrap().len();
    let fixture_bytes = source_bytes_per_query(fixture.path());

    let mut true_positives = 0usize;
    let mut source_hits = 0usize;
    let mut false_negatives = 0usize;
    let mut false_positives = 0usize;
    let mut indexed_hits = 0usize;
    let mut index_query_us = 0u128;
    let mut source_query_us = 0u128;
    let mut source_bytes_read_total = 0u64;

    for term in QUERIES {
        let source_started = Instant::now();
        let source = source_query(fixture.path(), term);
        source_query_us += source_started.elapsed().as_micros();
        let source_bytes = source.1;
        source_bytes_read_total += source_bytes;

        let index_started = Instant::now();
        let result = Command::cargo_bin("keel-contract-index")
            .unwrap()
            .args(["query", "--repo"])
            .arg(path(fixture.path()))
            .args(["--index"])
            .arg(path(&index))
            .args(["--term", term])
            .output()
            .unwrap();
        index_query_us += index_started.elapsed().as_micros();
        assert!(result.status.success(), "indexed query failed: {term}");
        let json: Value = serde_json::from_slice(&result.stdout).unwrap();
        let indexed = json["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|file| file["path"].as_str().unwrap().to_owned())
            .collect::<BTreeSet<_>>();
        let true_positive = source.0.intersection(&indexed).count();
        let missing = source.0.difference(&indexed).count();
        let extra = indexed.difference(&source.0).count();
        true_positives += true_positive;
        false_negatives += missing;
        false_positives += extra;
        source_hits += source.0.len();
        indexed_hits += indexed.len();
        assert_eq!(missing, 0, "indexed false negative for {term}");
        assert!(source_bytes > 0);
    }

    // A broken index must be observable as a refusal. The source probe below
    // is out of band and is not counted as a production fallback.
    let missing_index = output.path().join("missing.json");
    let refusal = Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args(["query", "--repo"])
        .arg(path(fixture.path()))
        .args(["--index"])
        .arg(path(&missing_index))
        .args(["--term", QUERIES[0]])
        .output()
        .unwrap();
    assert!(!refusal.status.success());
    let refusal_json: Value = serde_json::from_slice(&refusal.stdout).unwrap();
    assert_eq!(refusal_json["error"]["code"], "GHKEEL001_INDEX");
    let source_probe = source_query(fixture.path(), QUERIES[0]);
    assert!(!source_probe.0.is_empty());

    println!("arm | hits | measured_size | elapsed_us");
    println!("index build (CLI) | n/a | index_artifact_bytes={index_bytes} | {index_build_us}");
    println!(
        "index query (CLI + freshness rescan) | {indexed_hits} | index_artifact_bytes={index_bytes}; fixture_bytes={fixture_bytes} | {index_query_us}"
    );
    println!(
        "source query (direct probe) | {source_hits} | fixture_bytes={fixture_bytes}; source_bytes_read_total={source_bytes_read_total} | {source_query_us}"
    );
    println!(
        "quality | tp={true_positives} fn={false_negatives} fp={false_positives} recall={:.3} precision={:.3} | missing_index_refusals=1; source_probe=out_of_band",
        true_positives as f64 / source_hits as f64,
        true_positives as f64 / indexed_hits as f64
    );
}

fn write_fixture(root: &Path) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/alpha.rs"), "pub fn alpha_contract() {}\n").unwrap();
    fs::write(root.join("src/beta.rs"), "pub struct BetaContract;\n").unwrap();
    fs::write(root.join("src/gamma.rs"), "pub fn gamma_contract() {}\n").unwrap();
    fs::write(root.join("README.md"), "fixed offline fixture\n").unwrap();
}

fn source_query(root: &Path, term: &str) -> (BTreeSet<String>, u64) {
    let mut hits = BTreeSet::new();
    let mut bytes = 0;
    for relative in tracked_paths(root) {
        let path = root.join(&relative);
        let body = fs::read(&path).unwrap();
        bytes += body.len() as u64;
        if relative
            .to_ascii_lowercase()
            .contains(&term.to_ascii_lowercase())
            || String::from_utf8_lossy(&body)
                .to_ascii_lowercase()
                .contains(&term.to_ascii_lowercase())
        {
            hits.insert(relative);
        }
    }
    (hits, bytes)
}

fn source_bytes_per_query(root: &Path) -> u64 {
    tracked_paths(root)
        .iter()
        .map(|relative| fs::metadata(root.join(relative)).unwrap().len())
        .sum()
}

fn tracked_paths(root: &Path) -> Vec<String> {
    let output = ProcessCommand::new("git")
        .args(["-C", &path(root), "ls-files"])
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn git(root: &Path, args: &[&str]) {
    let output = ProcessCommand::new("git")
        .args(["-C", &path(root)])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
