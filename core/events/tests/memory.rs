//! The journal half of #220's G9.
//!
//! The governor decides that a candidate is refused. This side decides what the refusal LEAVES
//! BEHIND. A refusal is an append to an append-only journal, so "nothing has been persisted" is
//! true of the instant before it and false of the refusal itself, and the journal cannot be
//! rewritten afterwards to take the secret back out.

use graphhelm_events::{append_memory_refusal, memory_journal_is_clean};

/// Present in the INPUT by construction. That is what makes the scan below meaningful: the string
/// is known to exist upstream, so an empty result downstream is either a clean journal or a broken
/// scan, and the input tells the two apart.
const SENTINEL: &str = "ghp_G9SENTINELSECRETdoNotPersistMe0000000";

#[test]
fn a_refusal_append_leaves_no_trace_of_the_value_that_caused_it() {
    let content = format!("please remember my token {SENTINEL} for later");

    let mut journal = Vec::new();
    append_memory_refusal(&mut journal, "secret_detected", "content", &content);

    let text = String::from_utf8(journal.clone()).expect("the journal is utf-8");

    // Landmark. A scan over an empty journal finds nothing and proves nothing, so the entry must
    // first be shown to exist and to carry what a refusal is FOR.
    assert!(
        text.contains("secret_detected"),
        "HARNESS-BROKE: no refusal was appended, so the scan below would pass over an empty journal"
    );
    assert!(
        text.contains("content"),
        "HARNESS-BROKE: the appended refusal does not name the location"
    );

    // The SIZE is one of the three things the record claims to carry, and until this assertion it
    // was written by the code and checked by nothing: deleting the field left every test green.
    let entry: serde_json::Value =
        serde_json::from_str(text.trim_end()).expect("the appended line is JSON");
    assert_eq!(
        entry["contentBytes"].as_u64(),
        Some(content.len() as u64),
        "the refusal must record the size of what it refused"
    );
    assert_eq!(entry["code"].as_str(), Some("secret_detected"));
    assert_eq!(entry["field"].as_str(), Some("content"));

    // The whole journal is scanned, not the entry the test happens to know about. A second entry
    // added later by another path is covered by this the day it is written.
    assert!(
        memory_journal_is_clean(&journal, SENTINEL),
        "the refusal carried the offending value into an append-only journal: {text}"
    );
}

/// A public function must not panic on an input a caller can reach.
///
/// `memory_journal_is_clean` scanned with `windows(value.len())`, and `windows(0)` panics. An empty
/// needle is not exotic: it is what a caller passes when the value it meant to search for was
/// itself empty -- a config miss, a stripped env var, a field that was never set.
///
/// The answer is FALSE rather than true on purpose. This function is consumed as
/// `assert!(memory_journal_is_clean(..))`, so answering "clean" for a needle that cannot be
/// searched would turn a broken caller into a passing guard.
#[test]
fn an_empty_value_is_answered_rather_than_panicked_on() {
    let mut journal = Vec::new();
    append_memory_refusal(&mut journal, "secret_detected", "content", "anything");

    assert!(
        !memory_journal_is_clean(&journal, ""),
        "an empty needle must not be reported as clean: the caller cannot have meant it, and \
         'clean' is the answer that makes their assertion pass"
    );
}

/// `code` and `field` are `&str`, so a caller can hand them a quote. Interpolated, that closes the
/// string early and the record becomes two records or none.
///
/// The production change this catches: building the line with `format!` instead of serialising a
/// value. It is the discipline already applied to `content`, applied to the fields beside it.
#[test]
fn a_refusal_survives_a_code_that_contains_json_punctuation() {
    let mut journal = Vec::new();
    append_memory_refusal(
        &mut journal,
        r#"secret","field":"injected","x":"#,
        "content",
        "some content",
    );

    let text = String::from_utf8(journal).expect("the journal is utf-8");
    let entry: serde_json::Value =
        serde_json::from_str(text.trim_end()).expect("a hostile code must not break the record");

    assert_eq!(
        entry["field"].as_str(),
        Some("content"),
        "the injected field overwrote the real one: {text}"
    );
    assert_eq!(
        entry.as_object().map(serde_json::Map::len),
        Some(4),
        "the record grew or shrank under a hostile code: {text}"
    );
}
