//! #1378 (plan wave E, Canonical JSON): the live schema digests are pinned absolutely, and this
//! crate keeps its depth refusal at exactly `MAX_JSON_DEPTH`.
//!
//! Since the writers merged, `canonical_json` here is the protocols writer behind a depth check,
//! so comparing the two proved nothing. The pin below compares each committed `sha256` in
//! `schemas/catalog.json` with the digest of the file it names: those strings were written by the
//! writer as it was, so a writer change that moves one byte of any live schema goes red here.
//! (Release 1.0.0 is pinned the same way by `frozen_releases.rs`.) Cost: 24 small file reads.

use std::path::Path;

use graphhelm_schema_evolution::{SchemaCatalog, canonical_json, schema_digest};

#[test]
fn every_live_catalog_digest_is_the_digest_of_its_schema_file() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let catalog: SchemaCatalog = serde_json::from_slice(
        &std::fs::read(root.join("schemas/catalog.json")).expect("schemas/catalog.json"),
    )
    .expect("the live catalog parses");
    assert!(
        catalog.schemas.len() > 20,
        "HARNESS-BROKE: {} catalog entries",
        catalog.schemas.len()
    );
    for (name, entry) in &catalog.schemas {
        let document: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join(&entry.path)).unwrap()).unwrap();
        assert_eq!(
            schema_digest(&document).unwrap(),
            entry.sha256,
            "{name}: the committed digest no longer matches {}",
            entry.path
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
