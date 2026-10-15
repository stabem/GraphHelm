//! #1378: the workspace's one canonical JSON writer.
//!
//! Canonical form: object keys sorted by their bytes, no insignificant whitespace, every string,
//! key and number written by `serde_json` itself (so escaping and number spelling are the
//! library's, byte for byte). Persisted digests are taken over these bytes — development artifact
//! digests, schema catalog entries, event chain hashes — so this output is history: the goldens in
//! `tests/fixtures/canonical-goldens.json` and the fixture store's event hashes pin it.
//!
//! It streams into one buffer instead of building a sorted copy of the value first, which is what
//! the three writers it replaces did.
//!
//! THE SORT IS EXPLICIT AND THAT IS DELIBERATE, even though it is redundant today: this workspace
//! builds `serde_json` without `preserve_order`, so its object map already iterates sorted. Sorting
//! here is what keeps the output fixed if someone enables `preserve_order` later.

use serde_json::Value;

/// A value nested deeper than the caller's limit (see [`canonical_bytes_within`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonicalDepthExceeded;

/// The canonical bytes of `value`.
#[must_use]
pub fn canonical_bytes(value: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    write(value, &mut out, 0, usize::MAX).expect("no depth limit");
    out
}

/// The canonical bytes of `value`, refusing any value that sits deeper than `max_depth` (the root
/// is depth 0, each array item or object member one deeper).
pub fn canonical_bytes_within(
    value: &Value,
    max_depth: usize,
) -> Result<Vec<u8>, CanonicalDepthExceeded> {
    let mut out = Vec::new();
    write(value, &mut out, 0, max_depth)?;
    Ok(out)
}

fn write(
    value: &Value,
    out: &mut Vec<u8>,
    depth: usize,
    max_depth: usize,
) -> Result<(), CanonicalDepthExceeded> {
    if depth > max_depth {
        return Err(CanonicalDepthExceeded);
    }
    match value {
        Value::Object(map) => {
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by(|left, right| left.0.cmp(right.0));
            out.push(b'{');
            for (index, (key, nested)) in entries.into_iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                scalar(&Value::String(key.clone()), out);
                out.push(b':');
                write(nested, out, depth + 1, max_depth)?;
            }
            out.push(b'}');
        }
        Value::Array(items) => {
            out.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                write(item, out, depth + 1, max_depth)?;
            }
            out.push(b']');
        }
        other => scalar(other, out),
    }
    Ok(())
}

/// A string, number, boolean or null, spelled by `serde_json`.
fn scalar(value: &Value, out: &mut Vec<u8>) {
    serde_json::to_writer(&mut *out, value).expect("writing JSON into a Vec cannot fail");
}
