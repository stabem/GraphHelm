//! #1378 (plan wave E, Canonical JSON): golden digests of canonical JSON, committed BEFORE the
//! writer changes. Persisted digests (development artifact digests, schema catalog entries, event
//! chain hashes) are computed over these bytes, so a change in the writer that moves one byte
//! silently invalidates history. This cell pins the absolute bytes main produced:
//!
//! - every JSON file of each builtin extension package (the corpus their digests are taken over);
//! - `fixtures/canonical-tricky.json`, an array of documents: escapes, control characters,
//!   non-ASCII, `/`, floats, integers at the edges, key order at depth, empty containers.
//!
//! The goldens live in `fixtures/canonical-goldens/`, one file per package plus `tricky.json`
//! (#1459: one shared file made every two skill PRs conflict). A package's file moves with its
//! content: `python tools/reached-tests/plugin_version_guard.py --base origin/main --write`
//! regenerates it with the rest of the skill-change chain, and `GRAPHHELM_WRITE_FIXTURES=1` here
//! writes the same bytes. A rewrite is never part of a writer refactor: if this cell goes red
//! after a writer change with no corpus change, the change is wrong, not the golden.

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

/// The golden sets: one per builtin extension package, plus the tricky documents.
fn digests() -> BTreeMap<String, BTreeMap<String, String>> {
    let mut sets = BTreeMap::new();
    let builtin = root().join("extensions/builtin");
    let mut packages: Vec<PathBuf> = std::fs::read_dir(&builtin)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.join("extension.json").is_file())
        .collect();
    packages.sort();
    for package in packages {
        let mut out = BTreeMap::new();
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
        let name = package.file_name().unwrap().to_string_lossy().into_owned();
        sets.insert(name, out);
    }
    let mut tricky = BTreeMap::new();
    for (index, value) in tricky_documents().iter().enumerate() {
        tricky.insert(
            format!("tricky/{index:02}"),
            sha256_hex(canonical_json(value).as_bytes()),
        );
    }
    sets.insert("tricky".to_owned(), tricky);
    sets
}

#[test]
fn canonical_json_digests_equal_the_committed_goldens() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/canonical-goldens");
    let sets = digests();
    let total: usize = sets.values().map(BTreeMap::len).sum();
    assert!(
        sets.len() >= 3 && total > 60,
        "HARNESS-BROKE: only {total} canonical inputs in {} sets",
        sets.len()
    );
    let mut moved = Vec::new();
    let mut gone = Vec::new();
    for (set, actual) in &sets {
        let path = dir.join(format!("{set}.json"));
        if std::env::var_os("GRAPHHELM_WRITE_FIXTURES").is_some() {
            std::fs::write(
                &path,
                format!("{}\n", serde_json::to_string_pretty(actual).unwrap()),
            )
            .unwrap();
        }
        let golden: BTreeMap<String, String> = serde_json::from_str(
            &std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display())),
        )
        .unwrap();
        moved.extend(
            actual
                .iter()
                .filter(|(name, digest)| golden.get(*name) != Some(*digest))
                .map(|(name, _)| name.clone()),
        );
        gone.extend(
            golden
                .keys()
                .filter(|name| !actual.contains_key(*name))
                .cloned(),
        );
    }
    assert!(
        moved.is_empty() && gone.is_empty(),
        "canonical JSON moved (STOP: never rewrite these goldens in a refactor).\n  changed or new: {moved:?}\n  missing: {gone:?}"
    );
}
