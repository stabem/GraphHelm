//! #1378 (plan wave E, Canonical JSON): golden digests of canonical JSON, committed BEFORE the
//! writer changes. Persisted digests (development artifact digests, schema catalog entries, event
//! chain hashes) are computed over these bytes, so a change in the writer that moves one byte
//! silently invalidates history. This cell pins the absolute bytes main produced:
//!
//! - every JSON file of the development-contracts package (the corpus its digests are taken over);
//! - `fixtures/canonical-tricky.json`, an array of documents: escapes, control characters,
//!   non-ASCII, `/`, floats, integers at the edges, key order at depth, empty containers.
//!
//! The goldens live in `fixtures/canonical-goldens.json` and were written from main by
//! `GRAPHHELM_WRITE_FIXTURES=1`. A rewrite is never part of a refactor: if this cell goes red
//! after a writer change, the change is wrong, not the golden.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use graphhelm_protocols::canonical_json;
use graphhelm_protocols::hash::sha256_hex;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn corpus_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("{}: {error}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            corpus_files(&path, out);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            out.push(path);
        }
    }
}

/// The tricky documents, one per line, shared with the schema-evolution and events cells.
pub fn tricky_documents() -> Vec<serde_json::Value> {
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/canonical-tricky.json"),
    )
    .expect("canonical-tricky.json");
    serde_json::from_str(&text).expect("canonical-tricky.json is a JSON array")
}

fn digests() -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let package = root().join("extensions/builtin/graphhelm-development-contracts");
    let mut files = Vec::new();
    corpus_files(&package, &mut files);
    for path in files {
        let bytes = std::fs::read(&path).unwrap();
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            continue; // a deliberately malformed fixture is not a canonical input
        };
        let name = path
            .strip_prefix(root())
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        out.insert(name, sha256_hex(canonical_json(&value).as_bytes()));
    }
    for (index, value) in tricky_documents().iter().enumerate() {
        out.insert(
            format!("tricky/{index:02}"),
            sha256_hex(canonical_json(value).as_bytes()),
        );
    }
    out
}

#[test]
fn canonical_json_digests_equal_the_committed_goldens() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/canonical-goldens.json");
    let actual = digests();
    assert!(
        actual.len() > 60,
        "HARNESS-BROKE: only {} canonical inputs found",
        actual.len()
    );
    if std::env::var_os("GRAPHHELM_WRITE_FIXTURES").is_some() {
        std::fs::write(
            &path,
            format!("{}\n", serde_json::to_string_pretty(&actual).unwrap()),
        )
        .unwrap();
    }
    let golden: BTreeMap<String, String> =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("canonical-goldens.json"))
            .unwrap();
    let moved: Vec<&String> = actual
        .iter()
        .filter(|(name, digest)| golden.get(*name) != Some(*digest))
        .map(|(name, _)| name)
        .collect();
    let gone: Vec<&String> = golden
        .keys()
        .filter(|name| !actual.contains_key(*name))
        .collect();
    assert!(
        moved.is_empty() && gone.is_empty(),
        "canonical JSON moved (STOP: never rewrite these goldens in a refactor).\n  changed or new: {moved:?}\n  missing: {gone:?}"
    );
}
