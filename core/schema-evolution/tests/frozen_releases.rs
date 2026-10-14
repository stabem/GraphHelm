//! A published release under `schemas/releases/<version>/` is history: it does not change (#1233).
//!
//! `schema migrate` builds `schemas/releases/{from}/catalog.json` to migrate FROM, and the
//! PostgreSQL backup pins the catalog twice, so a frozen release that is silently rewritten
//! changes what old history means without anything going red. The natural way to do that is the
//! maintenance gesture itself: regenerating the schema set rewrites the frozen copy too.
//!
//! This used to be `ci/frozen-release-guard.ps1`, a stage of the retired gate, which stopped running
//! on 2026-09-24 and was deleted in #1232. It comes back here so that the reached-test plan runs it:
//! `tools/reached-tests` maps a data file to a test only when Rust source names that path
//! literally, so every frozen file below is read through its own literal path. The second cell
//! keeps that list honest against the directory.
//!
//! THREE LAYERS, because each catches what the one before cannot:
//!   1. each file's canonical digest equals its catalog entry (a file edited alone);
//!   2. the catalog's own canonical digest equals the pin below (a file edited TOGETHER with its
//!      catalog entry, which layer 1 cannot see);
//!   3. the release directories and their files are exactly the pinned ones (a release or a file
//!      added or removed without saying so).
//!
//! Reissuing a release is a decision someone may make; it is made by moving the pin in the same
//! diff, which is the "saying so" the old guard asked for.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use graphhelm_schema_evolution::{SchemaCatalog, schema_digest};
use serde_json::Value;

/// The canonical digest of each published release's `catalog.json`. Moving one of these is the
/// explicit act of reissuing a release; nothing else should ever change it.
const PINNED_RELEASES: &[(&str, &str)] = &[(
    "1.0.0",
    "sha256:4e921639e9bf943c12eb8acf4f091c6b0877e14b67e40c9649042f208221500a",
)];

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every file of release 1.0.0, each read through its literal path so that a change to any of
/// them reaches this target in the reached-test plan. Keyed by the repository path.
fn release_1_0_0() -> BTreeMap<&'static str, Vec<u8>> {
    let root = repository_root();
    let mut files = BTreeMap::new();
    let mut put = |path: &'static str, bytes: std::io::Result<Vec<u8>>| {
        files.insert(
            path,
            bytes.unwrap_or_else(|error| panic!("{path}: {error}")),
        );
    };
    put(
        "schemas/releases/1.0.0/catalog.json",
        fs::read(root.join("schemas/releases/1.0.0/catalog.json")),
    );
    put(
        "schemas/releases/1.0.0/agent.schema.json",
        fs::read(root.join("schemas/releases/1.0.0/agent.schema.json")),
    );
    put(
        "schemas/releases/1.0.0/artifact-reference.schema.json",
        fs::read(root.join("schemas/releases/1.0.0/artifact-reference.schema.json")),
    );
    put(
        "schemas/releases/1.0.0/claim.schema.json",
        fs::read(root.join("schemas/releases/1.0.0/claim.schema.json")),
    );
    put(
        "schemas/releases/1.0.0/context-capsule.schema.json",
        fs::read(root.join("schemas/releases/1.0.0/context-capsule.schema.json")),
    );
    put(
        "schemas/releases/1.0.0/edge.schema.json",
        fs::read(root.join("schemas/releases/1.0.0/edge.schema.json")),
    );
    put(
        "schemas/releases/1.0.0/event-envelope.schema.json",
        fs::read(root.join("schemas/releases/1.0.0/event-envelope.schema.json")),
    );
    put(
        "schemas/releases/1.0.0/evidence-record.schema.json",
        fs::read(root.join("schemas/releases/1.0.0/evidence-record.schema.json")),
    );
    put(
        "schemas/releases/1.0.0/extension.schema.json",
        fs::read(root.join("schemas/releases/1.0.0/extension.schema.json")),
    );
    put(
        "schemas/releases/1.0.0/graph.schema.json",
        fs::read(root.join("schemas/releases/1.0.0/graph.schema.json")),
    );
    put(
        "schemas/releases/1.0.0/graph-signal.schema.json",
        fs::read(root.join("schemas/releases/1.0.0/graph-signal.schema.json")),
    );
    put(
        "schemas/releases/1.0.0/node.schema.json",
        fs::read(root.join("schemas/releases/1.0.0/node.schema.json")),
    );
    put(
        "schemas/releases/1.0.0/persisted-graph-version.schema.json",
        fs::read(root.join("schemas/releases/1.0.0/persisted-graph-version.schema.json")),
    );
    put(
        "schemas/releases/1.0.0/policy-waiver.schema.json",
        fs::read(root.join("schemas/releases/1.0.0/policy-waiver.schema.json")),
    );
    put(
        "schemas/releases/1.0.0/repository-scope.schema.json",
        fs::read(root.join("schemas/releases/1.0.0/repository-scope.schema.json")),
    );
    put(
        "schemas/releases/1.0.0/sensitivity.schema.json",
        fs::read(root.join("schemas/releases/1.0.0/sensitivity.schema.json")),
    );
    files
}

fn json(path: &str, bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap_or_else(|error| panic!("{path} is not JSON: {error}"))
}

#[test]
fn a_published_release_matches_its_catalog_and_its_pin() {
    let files = release_1_0_0();
    let catalog_path = "schemas/releases/1.0.0/catalog.json";
    let catalog_value = json(catalog_path, &files[catalog_path]);

    // Layer 2 first: the catalog itself is what everything else is checked against.
    let pinned = PINNED_RELEASES
        .iter()
        .find(|(version, _)| *version == "1.0.0")
        .map(|(_, digest)| *digest)
        .expect("release 1.0.0 is pinned");
    let actual = schema_digest(&catalog_value).expect("the catalog canonicalises");
    assert_eq!(
        actual.as_str(),
        pinned,
        "{catalog_path} changed after publication. A frozen release is history: migrations and \
         backups read it as it was. If this release is genuinely being reissued, move the pin in \
         PINNED_RELEASES in this same diff and say why in the pull request."
    );

    // Layer 1: every schema the catalog lists is what the catalog says it is.
    let catalog: SchemaCatalog =
        serde_json::from_value(catalog_value).expect("the release catalog parses");
    assert!(
        !catalog.schemas.is_empty(),
        "a release with no schemas checks nothing"
    );
    for (name, entry) in &catalog.schemas {
        let bytes = files.get(entry.path.as_str()).unwrap_or_else(|| {
            panic!(
                "catalog entry {name} names {}, which this test does not read",
                entry.path
            )
        });
        let digest = schema_digest(&json(&entry.path, bytes)).expect("the schema canonicalises");
        assert_eq!(
            digest, entry.sha256,
            "{} changed after publication (catalog entry {name}). A frozen release is history.",
            entry.path
        );
    }
    assert_eq!(
        catalog.schemas.len() + 1,
        files.len(),
        "every file this test reads is the catalog or one of its entries"
    );
}

#[test]
fn the_pinned_releases_and_their_files_are_exactly_what_is_on_disk() {
    let releases = repository_root().join("schemas/releases");
    let on_disk: BTreeMap<String, BTreeSet<String>> = fs::read_dir(&releases)
        .expect("schemas/releases is readable")
        .map(|entry| entry.expect("a readable entry").path())
        .filter(|path| path.is_dir())
        .map(|dir| {
            let version = dir.file_name().unwrap().to_string_lossy().into_owned();
            let files = fs::read_dir(&dir)
                .expect("a readable release")
                .map(|entry| {
                    entry
                        .expect("a readable file")
                        .file_name()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            (version, files)
        })
        .collect();

    // CONTROL: an empty listing would make both comparisons below trivially true.
    assert!(
        !on_disk.is_empty(),
        "schemas/releases lists no release; the cells above checked nothing"
    );

    let pinned: BTreeSet<String> = PINNED_RELEASES
        .iter()
        .map(|(version, _)| (*version).to_owned())
        .collect();
    assert_eq!(
        on_disk.keys().cloned().collect::<BTreeSet<_>>(),
        pinned,
        "a release directory was added or removed. A new release is pinned in PINNED_RELEASES and \
         its files are read by literal path, like release_1_0_0(), in the same diff."
    );
    let read: BTreeSet<String> = release_1_0_0()
        .keys()
        .map(|path| path.rsplit('/').next().unwrap().to_owned())
        .collect();
    assert_eq!(
        on_disk["1.0.0"], read,
        "schemas/releases/1.0.0 holds a file this test does not read, or lost one it does"
    );
}
