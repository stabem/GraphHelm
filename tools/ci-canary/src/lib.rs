//! #152: the gate's own contamination canary. `build.rs` bakes a hash of this crate's `src/`
//! tree into the binary at compile time (`CANARY_SRC_HASH`, via the shared `hashing.rs` routine).
//! The one test below re-derives that same hash from whatever is actually on disk right now and
//! asserts they match. A mismatch means the binary that just ran is NOT the binary `src/` on this
//! machine would produce today — the exact shape of the shared-`CARGO_TARGET_DIR` contamination
//! class this crate exists to turn from a silent false green into a named, attributable red.
//!
//! `ci/gate.ps1` runs this crate's test FIRST, before any other stage, and aborts the whole gate
//! on failure rather than accumulating it alongside other stage results — a contaminated build
//! environment makes every other stage's result meaningless, so there is nothing to gain by
//! letting 26 more stages run against it.

// #152: `hash_src_tree` and `RUN_NONCE` are included INSIDE `#[cfg(test)]` below, not at the
// crate root - a real dead_code lint otherwise, caught live (`cargo clippy -p ci-canary
// --all-targets -D warnings`, not assumed clean): clippy analyzes the lib target and the test
// target as SEPARATE compilations even under --all-targets, so a crate-root `include!` used only
// by test code reads as genuinely unused from the lib target's own point of view, regardless of
// what the test target does with it. Scoping both includes to where they are actually used
// resolves this at the source rather than papering over it with #[allow(dead_code)].

#[cfg(test)]
mod tests {
    include!("../hashing.rs");
    include!("nonce.rs");

    /// The one assertion this whole crate exists for. Named deliberately so a gate log's panic
    /// site is unambiguous: `ci_canary::tests::the_running_binary_matches_the_src_tree_on_disk`,
    /// never an adjacent `unwrap()` or a generic `assert!` a future reader has to trace back.
    #[test]
    fn the_running_binary_matches_the_src_tree_on_disk() {
        // CARGO_MANIFEST_DIR is baked in at compile time too, but that is fine here: it names
        // WHERE src/ lives, not WHAT is in it - the content re-read below is what matters, and it
        // is read fresh, right now, from disk.
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let live_hash = hash_src_tree(&manifest_dir.join("src"));
        let baked_hash = env!("CANARY_SRC_HASH");
        assert_eq!(
            live_hash, baked_hash,
            "CONTAMINATION: this test binary's baked-in src/ hash ({baked_hash}) does not match \
             what is on disk right now ({live_hash}) - the binary that just ran was not built \
             from this tree. touch nonce (mentions {RUN_NONCE}) forced a rebuild attempt; if \
             this still mismatches, something reused a stale binary instead of rebuilding it."
        );
    }
}
