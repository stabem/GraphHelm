//! Context Capsule compilation (#222).
//!
//! Item identity lives here rather than in the capsule, and that is forced rather than preferred:
//! the capsule schema is closed (`additionalProperties: false` throughout), its sections are arrays
//! of bare strings, and it must stay byte-identical to its pinned release copy. It has no place to
//! put an ID and no permission to grow one.

use crate::context_accounting::push_segment;
use sha2::{Digest, Sha256};

/// Derive a stable identity for one capsule item, from its **content**.
///
/// `position` is accepted and deliberately ignored. Keeping it in the signature is the point: the
/// caller always has it, and the function refusing to use it is what makes the refusal visible at
/// every call site rather than buried in this doc comment. A positional identity is stable only
/// until something is inserted above it, and a citation recorded against a position does not dangle
/// when the content shifts — it **retargets**, resolving cleanly to the wrong evidence.
///
/// Every component is length-prefixed before hashing, so the derivation is injective: two different
/// input tuples cannot produce the same pre-image. Without that, a section named `ab` with text `c`
/// and a section named `a` with text `bc` would hash identically.
pub fn item_id(
    capsule_id: &str,
    capsule_version: u32,
    section: &str,
    _position: usize,
    text: &str,
) -> String {
    let mut hasher = Sha256::new();
    for component in [
        capsule_id,
        capsule_version.to_string().as_str(),
        section,
        text,
    ] {
        hasher.update(component.len().to_string().as_bytes());
        hasher.update(b":");
        hasher.update(component.as_bytes());
    }
    format!("item-{}", hex::encode(&hasher.finalize()[..16]))
}

/// The section order the capsule schema declares, and therefore the only order the compiler may
/// emit in.
///
/// Alphabetical would also be deterministic and would be wrong: the schema is the authority on
/// this document's shape, and a compiler that invents its own ordering has quietly become a
/// second opinion about the contract.
const DECLARED_SECTION_ORDER: [&str; 6] = [
    "projectKernel",
    "task",
    "node",
    "evidence",
    "dependencyOutputs",
    "agentExperience",
];

/// Compile a capsule's sections to bytes, in the schema's declared section order.
///
/// Determinism here is not per-call reproducibility — that is the easy half and nobody breaks
/// it. It is that two components assembling the same logical capsule obtain the same bytes, so
/// the digest binding it is the same, so the cache hits. Emitting in caller order is
/// deterministic per-call and non-deterministic across callers, which is the failure this
/// refuses.
///
/// A section outside the declared set is emitted after the declared ones, ordered by name, so
/// the output stays a function of the input even for a caller that supplies something the
/// schema would reject. Refusing it belongs to schema validation, not to the byte layout.
/// Every part is length-prefixed by the same helper the cache key uses, so the encoding is
/// injective. A bare separator is not enough and the reason is not theoretical: sections hold
/// free text, so an item containing the separator is ordinary, and one multi-line item would
/// compile to the same bytes as two items split at that point. These bytes are what gets
/// digested and what the binding carries, so the two capsules would share one identity and a
/// cache hit could return content assembled from different evidence.
///
/// One shared helper rather than a third copy of the rule: this change had already
/// length-prefixed in two places, each with the reason written above it, and still shipped a
/// third site that did not. A rule restated is a rule that can be forgotten at the next site.
pub fn compile_capsule(
    capsule_id: &str,
    capsule_version: u32,
    sections: &[(String, Vec<String>)],
) -> Vec<u8> {
    let mut out = String::new();
    push_segment(&mut out, capsule_id);
    push_segment(&mut out, &capsule_version.to_string());
    push_segment(&mut out, &sections.len().to_string());
    let mut ordered: Vec<&(String, Vec<String>)> = sections.iter().collect();
    ordered.sort_by_key(|(name, _)| {
        let declared = DECLARED_SECTION_ORDER
            .iter()
            .position(|known| known == name)
            .unwrap_or(DECLARED_SECTION_ORDER.len());
        (declared, name.clone())
    });
    for (name, items) in ordered {
        push_segment(&mut out, name);
        push_segment(&mut out, &items.len().to_string());
        for item in items {
            push_segment(&mut out, item);
        }
    }
    out.into_bytes()
}
