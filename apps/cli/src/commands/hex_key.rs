//! #1325: the one decoder for a 32-byte key written as 64 lowercase hexadecimal characters.
//!
//! The events key (`GRAPHHELM_EVENTS_KEY`) and the gateway key (`GRAPHHELM_GATEWAY_KEY`,
//! `serve.key`) had their own copies with the same alphabet and length. Each caller keeps its own
//! failure (code, message naming its variable, pointer); this returns `None` for any refusal so
//! the wording stays where the caller's diagnostic is decided.

use graphhelm_events::SecretBytes;

/// The 32 bytes `encoded` spells, or `None` unless it is exactly 64 characters of `0-9a-f`.
/// Uppercase, whitespace and any other byte are refused; nothing is trimmed. The intermediate
/// buffer is zeroized on every path, including a refusal half way through.
pub(crate) fn decode_hex_key(encoded: &[u8]) -> Option<SecretBytes> {
    if encoded.len() != 64 {
        return None;
    }
    let mut bytes = zeroize::Zeroizing::new(Vec::<u8>::with_capacity(32));
    for pair in encoded.chunks_exact(2) {
        bytes.push((hex_value(pair[0])? << 4) | hex_value(pair[1])?);
    }
    Some(SecretBytes::new(std::mem::take(&mut *bytes)))
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}
