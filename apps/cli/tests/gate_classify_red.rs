//! `graphhelm gate classify-red` — SHADOW classification of a RED gate run (#1138, edge 1).
//! The judge in every cell is the recorded door (`--judge-fixture`): no network, no
//! credentials, no clock. The log fixtures under `fixtures/classify-red/` are authored from the
//! SHAPE of two real runner transcripts (the #886 flake on #1128; the disk-full canary abort on
//! #1116), not copied from them.
//!
//! Each reply fixture carries an authored half (`rounds`, the one reply) and a derived half
//! (`answers`, that reply filed under the request digest), re-recorded under
//! `ARCHITECT_RECORD=1` the way `core/architect/tests/judgment_nodes.rs` records: run the
//! command with an empty recording, read the digest the `judgeMissing` refusal names, file the
//! round under it. A cell proves the two halves agree before it reads anything else.
//!
//! The shadow rule in one sentence, proved by every cell here: a classification of ANY kind
//! exits 0, and only the command's own failures exit non-zero.

use std::path::{Path, PathBuf};
use std::process::Output;

use assert_cmd::Command;
use serde_json::Value;

const ARGUMENT_CODE: &str = "GHCLI001_ARGUMENT_INVALID";
const REFUSED_CODE: &str = "GHCLI026_ARCHITECT_REFUSED";
const RECORD_VARIABLE: &str = "ARCHITECT_RECORD";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/classify-red")
}

fn fixture(name: &str) -> PathBuf {
    fixtures().join(name)
}

fn command() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
}

fn classify(log: &Path, judge: &Path, extra: &[&str]) -> Output {
    let known = fixture("known-flakes.json");
    let mut args = vec![
        "gate",
        "classify-red",
        "--log",
        log.to_str().unwrap(),
        "--known-flakes",
        known.to_str().unwrap(),
        "--judge-fixture",
        judge.to_str().unwrap(),
    ];
    args.extend_from_slice(extra);
    command().args(args).output().unwrap()
}

fn envelope(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{error}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn is_sha256_hex(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// The refusal a `judgeMissing` run prints, parsed: the diagnostic message is the refusal as
/// compact JSON, `kind`-tagged.
fn refusal(output: &Output) -> Value {
    let value = envelope(output);
    assert_eq!(value["ok"], false, "{value}");
    let diagnostic = &value["diagnostics"][0];
    assert_eq!(diagnostic["code"], REFUSED_CODE, "{value}");
    serde_json::from_str(diagnostic["message"].as_str().unwrap()).unwrap()
}

/// The reply fixture `name`, re-recorded first under `ARCHITECT_RECORD` against `log`, and
/// proved coherent: exactly one recorded answer, and it is the authored round.
fn judge_fixture(name: &str, log: &Path) -> PathBuf {
    let path = fixture(name);
    let mut value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let rounds = value["rounds"].as_array().cloned().unwrap();
    assert_eq!(rounds.len(), 1, "{name}: one authored round");
    if std::env::var_os(RECORD_VARIABLE).is_some() {
        let directory = tempfile::tempdir().unwrap();
        let empty = directory.path().join("empty.json");
        std::fs::write(&empty, br#"{"answers": {}}"#).unwrap();
        let output = classify(log, &empty, &[]);
        let missing = refusal(&output);
        assert_eq!(missing["kind"], "judgeMissing", "{missing}");
        let digest = missing["requestSha256"].as_str().unwrap().to_owned();
        value["answers"] = serde_json::json!({ digest: rounds[0] });
        let mut bytes = serde_json::to_vec_pretty(&value).unwrap();
        bytes.push(b'\n');
        std::fs::write(&path, bytes).unwrap();
    }
    let answers = value["answers"].as_object().unwrap();
    assert_eq!(
        answers.len(),
        1,
        "{name}: one recorded answer per authored round; re-record with {RECORD_VARIABLE}=1"
    );
    for (key, reply) in answers {
        assert!(
            is_sha256_hex(key),
            "{name}: key {key} is not a request digest"
        );
        assert_eq!(
            reply, &rounds[0],
            "{name}: the recorded answer must be the authored round; re-record"
        );
    }
    path
}

fn classification(log: &str, reply: &str) -> Value {
    let log = fixture(log);
    let judge = judge_fixture(reply, &log);
    let output = classify(&log, &judge, &[]);
    let value = envelope(&output);
    assert_eq!(
        output.status.code(),
        Some(0),
        "a classification never exits non-zero: {value}"
    );
    assert_eq!(value["ok"], true, "{value}");
    assert_eq!(value["command"], "gate.classify-red");
    let data = value["data"].clone();
    assert!(is_sha256_hex(data["excerptDigest"].as_str().unwrap()));
    data
}

#[test]
fn the_known_flake_is_classified_known_flake_and_named_same_as_886() {
    let data = classification("flake.log.txt", "flake-reply.json");
    assert_eq!(data["class"], "known_flake");
    assert_eq!(data["sameAs"], serde_json::json!([886]));
    assert_eq!(data["acts"], true);
    assert_eq!(data["unresolved"], false);
    assert!((data["confidence"].as_f64().unwrap() - 0.92).abs() < 1e-9);
    assert_eq!(data["judgeUsage"]["inputTokens"], 1800);
    assert!(data.get("out").is_none(), "no --out, no out field: {data}");
}

#[test]
fn the_disk_full_canary_abort_is_environment_void() {
    let data = classification("disk-full.log.txt", "disk-full-reply.json");
    assert_eq!(data["class"], "environment_void");
    assert_eq!(data["sameAs"], serde_json::json!([]));
    assert_eq!(data["acts"], true);
    assert_eq!(data["unresolved"], false);
}

#[test]
fn a_low_confidence_answer_is_unresolved_and_still_exits_zero() {
    let data = classification("flake.log.txt", "low-confidence-reply.json");
    assert_eq!(data["class"], "real_defect");
    assert_eq!(data["unresolved"], true);
    assert_eq!(data["acts"], false);
    assert_eq!(
        data["sameAs"],
        serde_json::json!([]),
        "a Noul of 0.5 is neither yes nor no"
    );
}

/// The runner's transcripts are UTF-16 with a BOM; the same log in either encoding is the same
/// excerpt, so one recorded reply answers both.
#[test]
fn a_utf16_log_is_the_same_excerpt_as_its_utf8_twin() {
    let bytes = std::fs::read(fixture("flake-utf16.log.txt")).unwrap();
    assert_eq!(
        &bytes[..2],
        &[0xFF, 0xFE],
        "the fixture carries a UTF-16LE BOM"
    );
    let utf16 = classification("flake-utf16.log.txt", "flake-reply.json");
    let utf8 = classification("flake.log.txt", "flake-reply.json");
    assert_eq!(utf16["excerptDigest"], utf8["excerptDigest"]);
    assert_eq!(utf16, utf8);
}

#[test]
fn out_is_written_once_and_never_overwritten() {
    let directory = tempfile::tempdir().unwrap();
    let out = directory.path().join("classification.json");
    let log = fixture("flake.log.txt");
    let judge = judge_fixture("flake-reply.json", &log);

    let first = classify(&log, &judge, &["--out", out.to_str().unwrap()]);
    let value = envelope(&first);
    assert_eq!(first.status.code(), Some(0), "{value}");
    assert_eq!(value["data"]["out"], out.to_str().unwrap());
    let written: Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    let mut expected = value["data"].clone();
    expected.as_object_mut().unwrap().remove("out");
    assert_eq!(
        written, expected,
        "the record is the printed data without `out`"
    );
    let bytes_before = std::fs::read(&out).unwrap();

    let second = classify(&log, &judge, &["--out", out.to_str().unwrap()]);
    let refused = envelope(&second);
    assert_ne!(second.status.code(), Some(0), "{refused}");
    assert_eq!(refused["diagnostics"][0]["code"], ARGUMENT_CODE);
    assert_eq!(refused["diagnostics"][0]["path"], "/out");
    assert_eq!(
        std::fs::read(&out).unwrap(),
        bytes_before,
        "the record was touched"
    );

    let not_json = directory.path().join("classification.txt");
    let third = classify(&log, &judge, &["--out", not_json.to_str().unwrap()]);
    assert_eq!(envelope(&third)["diagnostics"][0]["code"], ARGUMENT_CODE);
    assert!(!not_json.exists());
}

/// The one failure that IS the command's own: a recording that holds no reply for this request.
/// The digest reaches stderr so the reply can be authored under it.
#[test]
fn a_recording_without_the_reply_names_the_request_digest_on_stderr() {
    let output = classify(&fixture("flake.log.txt"), &fixture("empty-reply.json"), &[]);
    assert_ne!(output.status.code(), Some(0));
    let missing = refusal(&output);
    assert_eq!(missing["kind"], "judgeMissing", "{missing}");
    let digest = missing["requestSha256"].as_str().unwrap();
    assert!(is_sha256_hex(digest));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(digest),
        "stderr does not name the request digest {digest}: {stderr}"
    );
}

#[test]
fn an_unreadable_log_or_a_bad_flakes_file_is_the_commands_own_failure() {
    let directory = tempfile::tempdir().unwrap();
    let judge = fixture("flake-reply.json");
    let missing_log = classify(&directory.path().join("absent.log.txt"), &judge, &[]);
    let value = envelope(&missing_log);
    assert_ne!(missing_log.status.code(), Some(0));
    assert_eq!(value["diagnostics"][0]["code"], ARGUMENT_CODE);
    assert_eq!(value["diagnostics"][0]["path"], "/log");

    let bad_flakes = directory.path().join("flakes.json");
    std::fs::write(&bad_flakes, br#"{"issue": 886}"#).unwrap();
    let output = command()
        .args([
            "gate",
            "classify-red",
            "--log",
            fixture("flake.log.txt").to_str().unwrap(),
            "--known-flakes",
            bad_flakes.to_str().unwrap(),
            "--judge-fixture",
            judge.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let value = envelope(&output);
    assert_ne!(output.status.code(), Some(0));
    assert_eq!(value["diagnostics"][0]["code"], ARGUMENT_CODE);
    assert_eq!(value["diagnostics"][0]["path"], "/knownFlakes");
}
