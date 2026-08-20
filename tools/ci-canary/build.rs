// #152: bakes a hash of `src/` into the binary at compile time, via the SAME routine
// (`hashing.rs`, `include!`d, never duplicated) the runtime test re-derives from disk. This half
// of the pair only proves anything if it actually reruns on every gate invocation — hence the
// unconditional `rerun-if-changed=src` below, and `ci/gate.ps1`'s own job of rewriting
// `src/nonce.rs` before every run so there is always something under `src/` that changed, even on
// an otherwise byte-identical tree. A build.rs that DIDN'T rerun would keep baking in a stale
// hash forever, which is exactly the silent failure mode this crate exists to rule out.

include!("hashing.rs");

fn main() {
    println!("cargo:rerun-if-changed=src");
    let manifest_dir =
        std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets this"));
    let hash = hash_src_tree(&manifest_dir.join("src"));
    println!("cargo:rustc-env=CANARY_SRC_HASH={hash}");
}
