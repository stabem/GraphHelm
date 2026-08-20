// #152: rewritten by `ci/gate.ps1` before every gate run — its only job is to change on every
// run, so `src/` always has SOMETHING different to force `build.rs` to actually rerun (and
// re-hash, and re-bake `CANARY_SRC_HASH`) even when nothing else under `src/` changed. The value
// itself carries no meaning; do not read anything into it beyond "the gate touched this file this
// run". Placeholder committed value below — every real run overwrites it.
pub const RUN_NONCE: &str = "unset-outside-a-gate-run";
