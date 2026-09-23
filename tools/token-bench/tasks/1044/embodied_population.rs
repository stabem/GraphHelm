//! The staleness population is the binary's dependency closure, not the whole workspace (#1044).
//!
//! `measurable_binary` refuses when a source is newer than the binary. That question is only
//! meaningful over sources the binary CONTAINS. Comparing against every `src/*.rs` in the
//! workspace answered a wider question, and the wider question has a standing false positive
//! here: the gate's canary rewrites `tools/ci-canary/src/nonce.rs` at the start of every run,
//! and `ci-canary` is not a dependency of `apps/cli`. Cold, everything is rebuilt after that
//! write and nothing is noticed; warm, a correctly reused binary reads as stale.
//!
//! These cells assert the POPULATION, because that is where the defect lived. The refusal's own
//! behaviour is asserted by `subject_refusals.rs`, which still ages a source INSIDE the closure
//! and must still redden -- a narrower population that also stopped refusing would be the
//! always-passing guard this crate exists to prevent.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn population_contains(population: &[PathBuf], relative: &Path) -> bool {
    // Compared by suffix: the population is canonicalised, so on Windows its entries carry a
    // `\?\` prefix that a joined path does not.
    population
        .iter()
        .any(|candidate| candidate.ends_with(relative))
}

#[test]
fn the_canarys_nonce_is_not_in_the_population_and_the_file_it_is_absent_from_exists() {
    let root = workspace_root();

    // PRESENCE FIRST. An absence assertion over a file that does not exist passes for the wrong
    // reason and would keep passing if `ci-canary` were deleted, renamed, or moved -- at which
    // point this cell would be guarding nothing while still reporting green.
    let nonce = root
        .join("tools")
        .join("ci-canary")
        .join("src")
        .join("nonce.rs");
    assert!(
        nonce.is_file(),
        "the subject of this absence must exist, or the absence proves nothing: {}",
        nonce.display()
    );

    let population = pathogens::subject::embodied_sources();
    assert!(
        !population_contains(&population, Path::new("tools/ci-canary/src/nonce.rs")),
        "the binary does not link ci-canary, so rewriting its nonce must not age the binary"
    );
}

#[test]
fn the_population_holds_the_sources_the_binary_does_contain() {
    let population = pathogens::subject::embodied_sources();

    // POSITIVE CONTROL. Without this, a closure walker that returned an empty list would satisfy
    // the absence cell above perfectly -- and an empty population is exactly the shape that makes
    // `measurable_binary` stop refusing anything at all.
    assert!(
        population_contains(&population, Path::new("tools/pathogens/src/subject.rs")),
        "`apps/cli` depends on `pathogens`, and `subject_refusals` ages this very file to prove \
         the refusal still fires -- if it left the population that sabotage would stop reddening"
    );
    assert!(
        population_contains(&population, Path::new("core/protocols/src/lib.rs")),
        "a transitive workspace dependency of the binary must be reached, not just the direct ones"
    );
    assert!(
        population.len() > 100,
        "the closure collapsed to {} files, which is not this workspace",
        population.len()
    );
}
