//! #1378 (plan wave E, Canonical JSON): this crate's canonical JSON is byte-identical to
//! `graphhelm_protocols::canonical_json` (whose absolute digests are pinned in
//! `core/protocols/tests/fixtures/canonical-goldens.json`), and it keeps its depth refusal at
//! exactly `MAX_JSON_DEPTH`. Written before the writers merge, so the merge has a fixed target.

use std::path::{Path, PathBuf};

use graphhelm_schema_evolution::canonical_json;

fn protocols_fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../protocols/tests/fixtures")
}

fn inputs() -> Vec<(String, serde_json::Value)> {
    let mut out = Vec::new();
    let tricky: Vec<serde_json::Value> = serde_json::from_str(
        &std::fs::read_to_string(protocols_fixtures().join("canonical-tricky.json"))
            .expect("canonical-tricky.json"),
    )
    .expect("canonical-tricky.json is a JSON array");
    for (index, value) in tricky.into_iter().enumerate() {
        out.push((format!("tricky/{index:02}"), value));
    }
    let schemas = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    let mut files: Vec<_> = std::fs::read_dir(&schemas)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    files.sort();
    for path in files {
        let value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        out.push((
            path.file_name().unwrap().to_string_lossy().into_owned(),
            value,
        ));
    }
    out
}

#[test]
fn schema_canonical_json_is_byte_identical_to_the_protocols_writer() {
    let inputs = inputs();
    assert!(inputs.len() > 30, "HARNESS-BROKE: {} inputs", inputs.len());
    for (name, value) in inputs {
        let ours = canonical_json(&value).unwrap();
        let theirs = graphhelm_protocols::canonical_json(&value);
        assert_eq!(
            ours,
            theirs.as_bytes(),
            "{name}: the two canonical writers disagree"
        );
    }
}

#[test]
fn canonical_json_refuses_past_the_depth_limit_and_accepts_at_it() {
    fn nested(depth: usize) -> serde_json::Value {
        let mut value = serde_json::json!(0);
        for _ in 0..depth {
            value = serde_json::json!([value]);
        }
        value
    }
    let limit = graphhelm_schema_evolution::MAX_JSON_DEPTH;
    assert!(
        canonical_json(&nested(limit)).is_ok(),
        "depth {limit} is accepted"
    );
    assert!(
        canonical_json(&nested(limit + 1)).is_err(),
        "depth {} is refused",
        limit + 1
    );
}
