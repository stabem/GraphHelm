//! #1272: the one SHA-256 helper pair, pinned to published vectors.
//!
//! `wire_sha256` writes the `sha256:` form that event hashes, catalog digests and package digests
//! persist. Every site that used to hand-roll it now calls this pair, so these vectors are what
//! keeps already-written history readable: if either helper ever changes its bytes, this file is
//! red before anything is written in the new form.

use graphhelm_protocols::hash::{sha256_hex, wire_sha256};

/// FIPS 180-2 / NIST CAVP vectors: the empty message, "abc", and the two-block 448-bit message.
const VECTORS: &[(&[u8], &str)] = &[
    (
        b"",
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    ),
    (
        b"abc",
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    ),
    (
        b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
    ),
];

#[test]
fn sha256_hex_is_lowercase_hex_of_the_published_vectors() {
    for (input, expected) in VECTORS {
        let actual = sha256_hex(input);
        assert_eq!(
            actual,
            *expected,
            "input {:?}",
            String::from_utf8_lossy(input)
        );
        assert_eq!(actual.len(), 64);
        assert!(
            actual
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "lowercase hex only: {actual}"
        );
    }
}

#[test]
fn wire_sha256_is_the_persisted_prefix_plus_the_hex() {
    for (input, expected) in VECTORS {
        assert_eq!(
            wire_sha256(input),
            format!("sha256:{expected}"),
            "input {:?}",
            String::from_utf8_lossy(input)
        );
    }
    // The wire form parses as the protocol's own hash type.
    assert!(graphhelm_protocols::WireHash::parse(wire_sha256(b"abc")).is_ok());
}
