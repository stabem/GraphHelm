// #152: the ONE routine both `build.rs` (compile time, bakes the answer in) and `src/lib.rs`
// (run time, re-derives the answer from whatever is on disk right now) use to hash `src/`.
// `include!`d verbatim into both, never duplicated by hand: two independently-typed copies of a
// hashing algorithm are exactly the kind of thing that drifts silently and turns a real
// contamination signal into a false alarm (or worse, a false all-clear) on the next edit to one
// copy and not the other. Deliberately outside `src/` — a canary hashing itself while also being
// part of what it hashes is fine (tamper with the check, the hash changes, the OLD binary still
// catches it); a canary that can only see itself via one code path and not the other is not.
//
// Lives at the crate root as a free-standing file specifically so `include!` can reach it by a
// short relative path from both call sites without a module system detour.

/// Every file under `src_dir`, sorted by relative path for determinism, folded into one SHA-256:
/// each file contributes its relative path bytes, a NUL separator (so `"ab"+"c"` cannot collide
/// with `"a"+"bc"` across a path/content boundary), then its raw content bytes.
fn hash_src_tree(src_dir: &std::path::Path) -> String {
    use sha2::{Digest, Sha256};

    let mut files = Vec::new();
    collect_files_sorted(src_dir, src_dir, &mut files);

    let mut hasher = Sha256::new();
    for (relative, absolute) in &files {
        hasher.update(relative.as_bytes());
        hasher.update([0u8]);
        let contents = std::fs::read(absolute)
            .unwrap_or_else(|error| panic!("ci-canary: cannot read {absolute:?}: {error}"));
        hasher.update(&contents);
    }
    hex::encode(hasher.finalize())
}

/// Recursively collects `(relative_path_as_forward_slashes, absolute_path)` pairs, sorted by the
/// relative path — sorted so the hash does not depend on the filesystem's own directory-entry
/// order, which is unspecified and platform-dependent.
fn collect_files_sorted(
    dir: &std::path::Path,
    root: &std::path::Path,
    out: &mut Vec<(String, std::path::PathBuf)>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut children: Vec<std::path::PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    children.sort();
    for path in children {
        if path.is_dir() {
            collect_files_sorted(&path, root, out);
        } else {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push((relative, path));
        }
    }
}
