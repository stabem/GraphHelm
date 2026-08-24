//! Memory refusal records in the journal (#220).
//!
//! A refusal is a durable event. Whatever it carries is carried forever: the journal is
//! append-only, so a secret written here cannot be taken back out by editing, only by destroying
//! the key that opens it.

/// Append a refusal to the journal.
///
/// The refusal names the CODE and the LOCATION. It does not name the value.
/// The content is taken so that the LENGTH can be recorded -- a refusal is more useful with a size
/// than without one -- and so that the signature makes it obvious the value was available here and
/// deliberately not written. A refusal that quotes the offending value is the natural way to make a
/// diagnostic useful; it is also how a secret reaches an append-only journal through the one path
/// that runs before anything is supposed to be persisted.
pub fn append_memory_refusal(journal: &mut Vec<u8>, code: &str, field: &str, content: &str) {
    // Built as a VALUE and serialised, never interpolated. `code` and `field` are `&str`, so a
    // caller can hand this a quote or a backslash and split the record into two -- the same
    // discipline already applied to `content` one function up, applied to the fields beside it.
    let line = serde_json::json!({
        "event": "memory_refused",
        "code": code,
        "field": field,
        "contentBytes": content.len(),
    });

    let mut encoded = serde_json::to_vec(&line).unwrap_or_else(|_| {
        // A map of strings and one integer cannot fail to serialise; if it ever does, the record
        // still has to exist, and it still must not carry the value.
        br#"{"event":"memory_refused","code":"unencodable","field":"unencodable"}"#.to_vec()
    });
    encoded.push(b'\n');
    journal.extend_from_slice(&encoded);
}

/// Whether the journal is free of a given value.
///
/// Scans the WHOLE journal rather than one entry: a path that appends elsewhere is covered by this
/// the day it is written, instead of the day someone remembers to extend the check.
#[must_use]
pub fn memory_journal_is_clean(journal: &[u8], value: &str) -> bool {
    // `windows(0)` panics, and an empty needle is not exotic -- it is what a caller passes when the
    // value it meant to search for was itself empty. FALSE rather than true on purpose: this is
    // consumed as `assert!(memory_journal_is_clean(..))`, so answering "clean" for a needle that
    // cannot be searched would turn a broken caller into a passing guard.
    if value.is_empty() {
        return false;
    }

    !journal
        .windows(value.len())
        .any(|window| window == value.as_bytes())
}
