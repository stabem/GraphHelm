//! SHA-256 in the two forms the wire uses (#1272).
//!
//! `sha256_hex` is the lowercase 64-character hex digest. `wire_sha256` prefixes it with
//! `sha256:`, the form persisted in event hashes, catalog digests and package digests. Both are
//! pinned to published vectors by `tests/hash_golden.rs`; changing either changes what history
//! already written means.

use sha2::{Digest as _, Sha256};

/// SHA-256 of `bytes`, lowercase hex (64 characters).
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// `sha256:` followed by [`sha256_hex`] of `bytes`: the persisted wire form.
#[must_use]
pub fn wire_sha256(bytes: &[u8]) -> String {
    format!("sha256:{}", sha256_hex(bytes))
}
