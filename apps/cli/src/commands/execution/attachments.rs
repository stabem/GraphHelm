//! Image attachments on a signal (#313). One validator for every transport: the CLI reads files,
//! HTTP and MCP decode `attachments: [{mediaType, base64}]`; both end as `ImageAttachment` values
//! that already passed every cap, so the signal core only seals them.

use std::path::Path;

use super::{Failure, signal_invalid};

pub(crate) const MAX_ATTACHMENTS: usize = 4;
pub(crate) const MAX_ATTACHMENT_BYTES: usize = 8 * 1024 * 1024;
/// Longest padded base64 text that can decode to `MAX_ATTACHMENT_BYTES`; checked before decoding.
const MAX_ENCODED_BYTES: usize = 4 * MAX_ATTACHMENT_BYTES.div_ceil(3);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ImageAttachment {
    pub(crate) media_type: &'static str,
    pub(crate) bytes: Vec<u8>,
}

// HTTP and MCP call this in the next step of #313; until then only the tests do.
#[allow(dead_code)]
/// Parses the transport form. Every refusal is `GHCLI003` and happens before any write.
pub(crate) fn parse_json(value: &serde_json::Value) -> Result<Vec<ImageAttachment>, Failure> {
    let items = value
        .as_array()
        .ok_or_else(|| signal_invalid("attachments must be an array", "/attachments"))?;
    if items.len() > MAX_ATTACHMENTS {
        return Err(too_many());
    }
    items
        .iter()
        .enumerate()
        .map(|(index, item)| parse_item(index, item))
        .collect()
}

fn parse_item(index: usize, item: &serde_json::Value) -> Result<ImageAttachment, Failure> {
    let pointer = format!("/attachments/{index}");
    let object = item
        .as_object()
        .filter(|object| {
            object.len() == 2 && object.contains_key("mediaType") && object.contains_key("base64")
        })
        .ok_or_else(|| {
            signal_invalid(
                "an attachment is an object with exactly mediaType and base64",
                &pointer,
            )
        })?;
    let declared = object["mediaType"]
        .as_str()
        .and_then(known_type)
        .ok_or_else(|| {
            signal_invalid(
                "an attachment must be image/png, image/jpeg or image/webp",
                &format!("{pointer}/mediaType"),
            )
        })?;
    let encoded = object["base64"]
        .as_str()
        .ok_or_else(|| signal_invalid("base64 must be a string", &format!("{pointer}/base64")))?;
    if encoded.len() > MAX_ENCODED_BYTES {
        return Err(too_large(&format!("{pointer}/base64")));
    }
    let bytes = decode_base64(encoded).ok_or_else(|| {
        signal_invalid(
            "base64 must be standard padded RFC 4648 with no whitespace",
            &format!("{pointer}/base64"),
        )
    })?;
    checked(declared, bytes, &pointer)
}

/// The CLI form. The media type comes from the magic bytes; the extension is ignored. Size is
/// refused from metadata before the file is read.
pub(crate) fn from_file(path: &Path) -> Result<ImageAttachment, Failure> {
    let unreadable = || signal_invalid("--attach does not name a readable file", "/attachments");
    let length = std::fs::metadata(path).map_err(|_| unreadable())?.len();
    if length > MAX_ATTACHMENT_BYTES as u64 {
        return Err(too_large("/attachments"));
    }
    let bytes = std::fs::read(path).map_err(|_| unreadable())?;
    let media_type = sniff(&bytes).ok_or_else(|| {
        signal_invalid(
            "an attachment must be a PNG, JPEG or WebP image",
            "/attachments",
        )
    })?;
    checked(media_type, bytes, "/attachments")
}

/// Reads `--attach` files, refusing more than four before any is opened.
pub(crate) fn from_files(paths: &[std::path::PathBuf]) -> Result<Vec<ImageAttachment>, Failure> {
    if paths.len() > MAX_ATTACHMENTS {
        return Err(too_many());
    }
    paths.iter().map(|path| from_file(path)).collect()
}

fn checked(
    media_type: &'static str,
    bytes: Vec<u8>,
    pointer: &str,
) -> Result<ImageAttachment, Failure> {
    if bytes.is_empty() || bytes.len() > MAX_ATTACHMENT_BYTES {
        return Err(too_large(pointer));
    }
    if sniff(&bytes) != Some(media_type) {
        return Err(signal_invalid(
            "the attachment bytes do not match its media type",
            pointer,
        ));
    }
    Ok(ImageAttachment { media_type, bytes })
}

fn too_many() -> Failure {
    signal_invalid("a signal carries at most 4 attachments", "/attachments")
}

fn too_large(pointer: &str) -> Failure {
    signal_invalid("an attachment must be 1 byte to 8 MiB", pointer)
}

fn known_type(media_type: &str) -> Option<&'static str> {
    ["image/png", "image/jpeg", "image/webp"]
        .into_iter()
        .find(|known| *known == media_type)
}

/// Magic-byte type of `bytes`, if it is one of the three accepted images.
pub(crate) fn sniff(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// Strict RFC 4648 standard alphabet with padding; anything else (whitespace included) is `None`.
fn decode_base64(text: &str) -> Option<Vec<u8>> {
    fn value(byte: u8) -> Option<u32> {
        match byte {
            b'A'..=b'Z' => Some(u32::from(byte - b'A')),
            b'a'..=b'z' => Some(u32::from(byte - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(byte - b'0') + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let chunks = bytes.len() / 4;
    for (index, chunk) in bytes.chunks(4).enumerate() {
        let padding = chunk.iter().rev().take_while(|byte| **byte == b'=').count();
        if padding > 2 || (padding > 0 && index + 1 != chunks) {
            return None;
        }
        let mut word = 0u32;
        for byte in &chunk[..4 - padding] {
            word = (word << 6) | value(*byte)?;
        }
        word <<= 6 * padding as u32;
        let decoded = [(word >> 16) as u8, (word >> 8) as u8, word as u8];
        // Non-canonical trailing bits are refused so one image has exactly one encoding.
        if padding > 0 && decoded[3 - padding..].iter().any(|byte| *byte != 0) {
            return None;
        }
        out.extend_from_slice(&decoded[..3 - padding]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0];
    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0];
    const WEBP: &[u8] = b"RIFF\0\0\0\0WEBPVP8 ";

    fn encode(bytes: &[u8]) -> String {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let mut word = 0u32;
            for (i, byte) in chunk.iter().enumerate() {
                word |= u32::from(*byte) << (16 - 8 * i);
            }
            for i in 0..4 {
                if i <= chunk.len() {
                    out.push(ALPHABET[(word >> (18 - 6 * i) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    fn item(media_type: &str, bytes: &[u8]) -> serde_json::Value {
        json!({"mediaType": media_type, "base64": encode(bytes)})
    }

    #[test]
    fn attachments_sniff_each_accepted_type() {
        assert_eq!(sniff(PNG), Some("image/png"));
        assert_eq!(sniff(JPEG), Some("image/jpeg"));
        assert_eq!(sniff(WEBP), Some("image/webp"));
        assert_eq!(sniff(b"<svg/>"), None);
        let parsed = parse_json(&json!([
            item("image/png", PNG),
            item("image/jpeg", JPEG),
            item("image/webp", WEBP)
        ]))
        .ok()
        .unwrap();
        assert_eq!(parsed[0].bytes, PNG);
        assert_eq!(parsed[2].media_type, "image/webp");
    }

    #[test]
    fn attachments_refuse_wrong_magic_svg_and_extra_keys() {
        let wrong = parse_json(&json!([item("image/png", JPEG)])).unwrap_err();
        assert_eq!(wrong.pointer, "/attachments/0");
        let svg = parse_json(&json!([item("image/svg+xml", b"<svg/>")])).unwrap_err();
        assert_eq!(svg.pointer, "/attachments/0/mediaType");
        let mut extra = item("image/png", PNG);
        extra["name"] = json!("x");
        assert_eq!(
            parse_json(&json!([extra])).unwrap_err().pointer,
            "/attachments/0"
        );
        assert_eq!(parse_json(&json!({})).unwrap_err().pointer, "/attachments");
    }

    #[test]
    fn attachments_base64_is_strict() {
        assert_eq!(decode_base64("TWE=").unwrap(), b"Ma");
        assert_eq!(decode_base64("TQ==").unwrap(), b"M");
        assert_eq!(decode_base64("TWFu").unwrap(), b"Man");
        assert_eq!(decode_base64("").unwrap(), b"");
        assert!(decode_base64("TWE").is_none(), "missing padding");
        assert!(decode_base64("TW E=").is_none(), "whitespace");
        assert!(decode_base64("TWE=\n").is_none(), "newline");
        assert!(decode_base64("TW-_").is_none(), "url alphabet");
        assert!(decode_base64("TQ==TWFu").is_none(), "padding mid-stream");
        assert!(decode_base64("T===").is_none(), "three pads");
        assert!(decode_base64("TR==").is_none(), "non-canonical bits");
        for len in 0..10 {
            let bytes: Vec<u8> = (0..len).map(|i: u8| i.wrapping_mul(37)).collect();
            assert_eq!(decode_base64(&encode(&bytes)).unwrap(), bytes);
        }
    }

    #[test]
    fn attachments_count_over_four_refused() {
        let five = json!(vec![item("image/png", PNG); 5]);
        assert_eq!(parse_json(&five).unwrap_err().pointer, "/attachments");
        assert_eq!(
            parse_json(&json!(vec![item("image/png", PNG); 4]))
                .ok()
                .unwrap()
                .len(),
            4
        );
    }

    #[test]
    fn attachments_size_caps() {
        let mut big = PNG.to_vec();
        big.resize(MAX_ATTACHMENT_BYTES + 1, 0);
        let refused = parse_json(&json!([item("image/png", &big)])).unwrap_err();
        assert_eq!(refused.pointer, "/attachments/0");
        big.truncate(MAX_ATTACHMENT_BYTES);
        assert!(parse_json(&json!([item("image/png", &big)])).is_ok());
        // Over the encoded cap with invalid characters: the length refusal wins, so no decode ran.
        let huge = "!".repeat(MAX_ENCODED_BYTES + 4);
        let refused = parse_json(&json!([{"mediaType": "image/png", "base64": huge}])).unwrap_err();
        assert_eq!(refused.message, "an attachment must be 1 byte to 8 MiB");
        let empty = parse_json(&json!([{"mediaType": "image/png", "base64": ""}])).unwrap_err();
        assert_eq!(empty.message, "an attachment must be 1 byte to 8 MiB");
    }

    #[test]
    fn attachments_from_file_sniffs_and_caps() {
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("shot.jpg");
        std::fs::write(&png, PNG).unwrap();
        assert_eq!(from_file(&png).ok().unwrap().media_type, "image/png");
        let svg = dir.path().join("x.png");
        std::fs::write(&svg, b"<svg/>").unwrap();
        assert!(from_file(&svg).is_err());
        let big = dir.path().join("big.png");
        let mut bytes = PNG.to_vec();
        bytes.resize(MAX_ATTACHMENT_BYTES + 1, 0);
        std::fs::write(&big, bytes).unwrap();
        assert_eq!(
            from_file(&big).unwrap_err().message,
            "an attachment must be 1 byte to 8 MiB"
        );
        assert!(from_files(&vec![png; 5]).is_err());
    }
}
