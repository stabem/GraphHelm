use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

fn repo() -> TempDir {
    let d = tempfile::tempdir().unwrap();
    Command::new("git")
        .args(["init", "-q", d.path().to_str().unwrap()])
        .assert()
        .success();
    fs::write(
        d.path().join("lib.rs"),
        "pub fn hello() {}\nfn private() {}\n",
    )
    .unwrap();
    fs::write(d.path().join("README.md"), "hello\n").unwrap();
    Command::new("git")
        .args(["-C", d.path().to_str().unwrap(), "add", "."])
        .assert()
        .success();
    d
}

#[test]
fn scan_is_deterministic_and_reports_unsupported_without_source() {
    let d = repo();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("index.json");
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "scan",
            "--repo",
            d.path().to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    let text = fs::read_to_string(&out).unwrap();
    assert!(text.contains("\"path\": \"README.md\""));
    assert!(text.contains("\"parse\": \"unsupported\""));
    assert!(text.contains("\"name\": \"hello\""));
    assert!(!text.contains("pub fn hello"));
    let first = text.clone();
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "scan",
            "--repo",
            d.path().to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert_eq!(first, fs::read_to_string(out).unwrap());
}

#[test]
fn verify_rejects_source_mutation() {
    let d = repo();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("index.json");
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "scan",
            "--repo",
            d.path().to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    fs::write(d.path().join("lib.rs"), "pub fn changed() {}\n").unwrap();
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "verify",
            "--repo",
            d.path().to_str().unwrap(),
            "--index",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("GHKEEL002_STALE"));
}

#[test]
fn query_selects_by_symbol() {
    let d = repo();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("index.json");
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "scan",
            "--repo",
            d.path().to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "query",
            "--repo",
            d.path().to_str().unwrap(),
            "--index",
            out.to_str().unwrap(),
            "--term",
            "hello",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("lib.rs"));
}

#[test]
fn new_untracked_source_invalidates_the_coverage_snapshot() {
    let d = repo();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("index.json");
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "scan",
            "--repo",
            d.path().to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    fs::write(d.path().join("new.rs"), "pub fn added() {}\n").unwrap();
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "query",
            "--repo",
            d.path().to_str().unwrap(),
            "--index",
            out.to_str().unwrap(),
            "--term",
            "hello",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("GHKEEL002_STALE"));
}

#[test]
fn scan_refuses_output_inside_the_repository_even_in_a_new_directory() {
    let d = repo();
    let out = d.path().join("new-directory").join("index.json");
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "scan",
            "--repo",
            d.path().to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("GHKEEL005_UNSAFE_PATH"));
    assert!(!out.exists());
}

#[test]
fn ignored_files_are_reported_as_omissions() {
    let d = repo();
    fs::write(d.path().join(".gitignore"), "ignored.txt\n").unwrap();
    fs::write(d.path().join("ignored.txt"), "secret\n").unwrap();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("index.json");
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "scan",
            "--repo",
            d.path().to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    let text = fs::read_to_string(out).unwrap();
    assert!(text.contains("ignored.txt"));
    assert!(text.contains("ignored_excluded"));
}

#[test]
fn invalid_utf8_is_explicitly_unparseable() {
    let d = repo();
    fs::write(d.path().join("broken.rs"), [0xff, 0xfe]).unwrap();
    Command::new("git")
        .args(["-C", d.path().to_str().unwrap(), "add", "broken.rs"])
        .assert()
        .success();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("index.json");
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "scan",
            "--repo",
            d.path().to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(fs::read_to_string(out).unwrap().contains("invalid_utf8"));
}

#[test]
fn proposal_is_source_bound_and_has_no_acceptance_promise() {
    let d = repo();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("index.json");
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "scan",
            "--repo",
            d.path().to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "propose-card",
            "--repo",
            d.path().to_str().unwrap(),
            "--index",
            out.to_str().unwrap(),
            "--term",
            "lib.rs",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("needs_product_contract"))
        .stdout(predicate::str::contains("acceptanceCriteria").and(predicate::str::contains("[]")));
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "propose-card",
            "--repo",
            d.path().to_str().unwrap(),
            "--index",
            out.to_str().unwrap(),
            "--term",
            "missing",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("GHKEEL006_AMBIGUOUS"));
}

#[test]
fn verify_rejects_oversized_index_before_json_parsing() {
    let d = repo();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("index.json");
    fs::write(&out, vec![b' '; 16 * 1024 * 1024 + 1]).unwrap();
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "verify",
            "--repo",
            d.path().to_str().unwrap(),
            "--index",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("GHKEEL004_LIMIT"));
}

#[test]
fn verify_rejects_many_json_values_before_deserialization() {
    let d = repo();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("index.json");
    let mut json = String::from(
        r#"{"schema":"keel.contract-index.v1","repo":"working-tree","snapshot_digest":"x","files":["#,
    );
    json.push_str(&"{},".repeat(10_000));
    json.push_str("{}],\"omissions\":[]}");
    fs::write(&out, json).unwrap();
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "verify",
            "--repo",
            d.path().to_str().unwrap(),
            "--index",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("GHKEEL004_LIMIT"));
}

#[test]
fn verify_reads_near_maximum_index_with_multiple_declarations() {
    let d = repo();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("index.json");
    let mut json = String::from(
        r#"{"schema":"keel.contract-index.v1","repo":"working-tree","snapshot_digest":"x","files":["#,
    );
    let declaration = format!(r#"{{"name":"{}","kind":"function"}}"#, "item".repeat(24));
    let declarations = (0..10)
        .map(|_| declaration.as_str())
        .collect::<Vec<_>>()
        .join(",");
    for index in 0..10_000 {
        if index != 0 {
            json.push(',');
        }
        json.push_str(&format!(
            r#"{{"path":"src/{index}.rs","sha256":"x","bytes":1,"language":"rust","parse":"parsed","declarations":[{declarations}]}}"#
        ));
    }
    json.push_str("],\"omissions\":[]}");
    fs::write(&out, json).unwrap();
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "verify",
            "--repo",
            d.path().to_str().unwrap(),
            "--index",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("GHKEEL002_STALE"));
}

#[test]
fn verify_rejects_deep_or_long_json_before_deserialization() {
    let d = repo();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("index.json");
    let mut json = String::from("{\"schema\":\"");
    json.push_str(&"a".repeat(4097));
    json.push_str(
        "\",\"repo\":\"working-tree\",\"snapshot_digest\":\"x\",\"files\":[],\"omissions\":[]}",
    );
    fs::write(&out, json).unwrap();
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "verify",
            "--repo",
            d.path().to_str().unwrap(),
            "--index",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("GHKEEL004_LIMIT"));
}

#[test]
fn verify_rejects_json_deeper_than_the_preflight_bound() {
    let d = repo();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("index.json");
    let mut json = "[".repeat(65);
    json.push_str("null");
    json.push_str(&"]".repeat(65));
    fs::write(&out, json).unwrap();
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "verify",
            "--repo",
            d.path().to_str().unwrap(),
            "--index",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("GHKEEL004_LIMIT"));
}

#[test]
fn standalone_errors_do_not_echo_untrusted_paths() {
    let d = repo();
    let out_dir = tempfile::tempdir().unwrap();
    let missing = out_dir.path().join("index-with-secret-sentinel.json");
    let output = Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "verify",
            "--repo",
            d.path().to_str().unwrap(),
            "--index",
            missing.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("GHKEEL001_INDEX"));
    assert!(!stdout.contains("secret-sentinel"));
}

#[test]
fn a_new_ignored_file_invalidates_the_coverage_snapshot() {
    let d = repo();
    fs::write(d.path().join(".gitignore"), "*.secret\n").unwrap();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("index.json");
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "scan",
            "--repo",
            d.path().to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    fs::write(d.path().join("new.secret"), "secret\n").unwrap();
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "query",
            "--repo",
            d.path().to_str().unwrap(),
            "--index",
            out.to_str().unwrap(),
            "--term",
            "lib.rs",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("GHKEEL002_STALE"));
}

#[test]
fn candidate_discloses_unobserved_ignored_directory_contents() {
    let d = repo();
    fs::write(d.path().join(".gitignore"), "ignored/\n").unwrap();
    fs::create_dir(d.path().join("ignored")).unwrap();
    fs::write(
        d.path().join("ignored").join("hidden.rs"),
        "pub fn hidden() {}\n",
    )
    .unwrap();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("index.json");
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "scan",
            "--repo",
            d.path().to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "propose-card",
            "--repo",
            d.path().to_str().unwrap(),
            "--index",
            out.to_str().unwrap(),
            "--term",
            "lib.rs",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"ignoredDirectoryContentsUnobserved\":true",
        ));
}

#[test]
fn query_and_candidate_disclose_untracked_coverage_gaps() {
    let d = repo();
    fs::write(d.path().join("extra.rs"), "pub fn unseen() {}\n").unwrap();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("index.json");
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "scan",
            "--repo",
            d.path().to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    for command in ["query", "propose-card"] {
        let output = Command::cargo_bin("keel-contract-index")
            .unwrap()
            .args([
                command,
                "--repo",
                d.path().to_str().unwrap(),
                "--index",
                out.to_str().unwrap(),
                "--term",
                "lib.rs",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let result: serde_json::Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(result["coverageGaps"]["omittedPaths"], 1);
        assert_eq!(
            result["coverageGaps"]["omissionReasons"]["untracked_excluded"],
            1
        );
        assert_eq!(result["coverageGaps"]["unsupportedFiles"], 1);
    }
}

#[test]
fn javascript_and_tsx_exports_are_ast_bound_and_aliases_are_preserved() {
    // Contract: exported declarations are indexed by syntax, including aliases and default
    // exports. Defect caught: a text scanner reports names found only in comments or strings.
    let d = repo();
    fs::write(
        d.path().join("feature.tsx"),
        r#"// export function CommentOnly() {}
const text = "export const StringOnly = 1";
export function realFeature(value: string): string { return value; }
export { realFeature as publicFeature };
export default function entry() { return <main />; }
export const { destructured, nested: renamedDestructured } = source;
interface Hidden { value: string }
"#,
    )
    .unwrap();
    fs::write(
        d.path().join("feature.js"),
        "export const javascriptFeature = 1;\nexport { javascriptFeature as renamedFeature };\n",
    )
    .unwrap();
    fs::write(
        d.path().join("star.js"),
        "export * from './other.js';\nexport * as starFeature from './other.js';\n",
    )
    .unwrap();
    fs::write(
        d.path().join("module.cjs"),
        "module.exports = { hidden: true };\n",
    )
    .unwrap();
    Command::new("git")
        .args(["-C", d.path().to_str().unwrap(), "add", "."])
        .assert()
        .success();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("index.json");
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "scan",
            "--repo",
            d.path().to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    let index: serde_json::Value = serde_json::from_str(&fs::read_to_string(out).unwrap()).unwrap();
    let tsx = index["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["path"] == "feature.tsx")
        .unwrap();
    assert_eq!(tsx["parse"], "parsed");
    let names: Vec<&str> = tsx["declarations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"realFeature"));
    assert!(names.contains(&"publicFeature"));
    assert!(names.contains(&"default"));
    assert!(names.contains(&"entry"));
    assert!(names.contains(&"destructured"));
    assert!(names.contains(&"renamedDestructured"));
    assert!(!names.contains(&"CommentOnly"));
    assert!(!names.contains(&"StringOnly"));
    let star = index["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["path"] == "star.js")
        .unwrap();
    let star_names: Vec<&str> = star["declarations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|declaration| declaration["name"].as_str().unwrap())
        .collect();
    assert!(star_names.contains(&"*"));
    assert!(star_names.contains(&"starFeature"));
    let commonjs = index["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["path"] == "module.cjs")
        .unwrap();
    assert_eq!(commonjs["language"], "commonjs");
    assert_eq!(commonjs["parse"], "unsupported");
}

#[test]
fn vue_indexes_only_scripts_and_fails_closed_on_invalid_script() {
    // Contract: Vue script blocks are parsed with explicit partial coverage. Defect caught:
    // treating template text as JavaScript or retaining partial symbols after a parse error.
    let d = repo();
    fs::write(
        d.path().join("Good.vue"),
        r#"<!-- <script>export const commented = 1;</script> -->
<template>Olá {{ "<span>" }} {{ /}}/.test("<span>") }} {{ (() => /}}/.test("<span>"))() }} <script-widget /></template>
<div :title="'<script>'" />
<script setup lang = "ts">
export const setupFeature: string = "ok";
</script>
<style>.x { color: red }</style>
"#,
    )
    .unwrap();
    fs::write(
        d.path().join("Bad.vue"),
        "<script>export const before = 1; ???</script>\n",
    )
    .unwrap();
    fs::write(
        d.path().join("DataLang.vue"),
        "<script data-lang=\"ts\">interface InvalidInJavaScript { value: string }</script>\n",
    )
    .unwrap();
    Command::new("git")
        .args(["-C", d.path().to_str().unwrap(), "add", "."])
        .assert()
        .success();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("index.json");
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "scan",
            "--repo",
            d.path().to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    let index: serde_json::Value = serde_json::from_str(&fs::read_to_string(out).unwrap()).unwrap();
    let good = index["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["path"] == "Good.vue")
        .unwrap();
    assert_eq!(good["parse"], "partial");
    assert_eq!(good["declarations"][0]["name"], "setupFeature");
    let bad = index["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["path"] == "Bad.vue")
        .unwrap();
    assert_eq!(bad["parse"], "invalid");
    assert_eq!(bad["declarations"].as_array().unwrap().len(), 0);
    let data_lang = index["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["path"] == "DataLang.vue")
        .unwrap();
    assert_eq!(data_lang["parse"], "invalid");
    assert_eq!(data_lang["declarations"].as_array().unwrap().len(), 0);
}

#[test]
fn scan_disables_repository_fsmonitor_commands() {
    let d = repo();
    let out_dir = tempfile::tempdir().unwrap();
    let marker = out_dir.path().join("fsmonitor-ran");
    let hook = out_dir.path().join("fsmonitor.sh");
    let hook_path = hook.to_string_lossy().replace('\\', "/");
    let marker_path = marker.to_string_lossy().replace('\\', "/");
    fs::write(
        &hook,
        format!("#!/bin/sh\nprintf ran > \"{marker_path}\"\n"),
    )
    .unwrap();
    Command::new("git")
        .args([
            "-C",
            d.path().to_str().unwrap(),
            "config",
            "core.fsmonitor",
            &format!("sh \"{hook_path}\""),
        ])
        .assert()
        .success();
    Command::new("git")
        .args(["-C", d.path().to_str().unwrap(), "status", "--porcelain"])
        .assert()
        .success();
    assert!(marker.exists(), "hostile fsmonitor fixture did not execute");
    fs::remove_file(&marker).unwrap();
    let out = out_dir.path().join("index.json");
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "scan",
            "--repo",
            d.path().to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(!marker.exists(), "scanner executed repository fsmonitor");
}

#[test]
fn scan_does_not_run_repository_clean_filters() {
    let d = repo();
    let out_dir = tempfile::tempdir().unwrap();
    let marker = out_dir.path().join("clean-filter-ran");
    let hook = out_dir.path().join("clean-filter.sh");
    let hook_path = hook.to_string_lossy().replace('\\', "/");
    let marker_path = marker.to_string_lossy().replace('\\', "/");
    fs::write(
        &hook,
        format!("#!/bin/sh\nprintf ran > \"{marker_path}\"\ncat\n"),
    )
    .unwrap();
    fs::write(
        d.path().join(".gitattributes"),
        "filtered.txt filter=hostile\n",
    )
    .unwrap();
    fs::write(d.path().join("filtered.txt"), "before\n").unwrap();
    Command::new("git")
        .args([
            "-C",
            d.path().to_str().unwrap(),
            "config",
            "filter.hostile.clean",
            &format!("sh \"{hook_path}\""),
        ])
        .assert()
        .success();
    Command::new("git")
        .args([
            "-C",
            d.path().to_str().unwrap(),
            "add",
            ".gitattributes",
            "filtered.txt",
        ])
        .assert()
        .success();
    assert!(
        marker.exists(),
        "hostile clean-filter fixture did not execute"
    );
    fs::remove_file(&marker).unwrap();
    let filtered = d.path().join("filtered.txt");
    fs::write(&filtered, "after!\n").unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&filtered)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000))
        .unwrap();
    Command::new("git")
        .args(["-C", d.path().to_str().unwrap(), "status", "--porcelain"])
        .assert()
        .success();
    assert!(
        marker.exists(),
        "status did not reproduce the filter hazard"
    );
    fs::remove_file(&marker).unwrap();
    let out = out_dir.path().join("index.json");
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "scan",
            "--repo",
            d.path().to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(!marker.exists(), "scanner executed repository clean filter");
}
