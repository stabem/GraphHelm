//! Context Capsule compilation (#222).
//!
//! Item identity lives here rather than in the capsule, and that is forced rather than preferred:
//! the capsule schema is closed (`additionalProperties: false` throughout), its sections are arrays
//! of bare strings, and it must stay byte-identical to its pinned release copy. It has no place to
//! put an ID and no permission to grow one.
//!
//! # Declared gap: deltas are verified here, never applied
//!
//! #222's criterion reads *"Delta/expansion retains provenance and budgets."* **This module
//! implements the base-identity half of that and nothing else.** `DeltaProvenance` names ONE base
//! — capsule, version, digest — and `verify_delta_base` checks that such a record matches the base
//! capsule offered. Nothing here applies a delta and yields a capsule.
//!
//! Measured rather than estimated, **at `13286b8`, before this paragraph existed**: `apply_delta`,
//! `compose_delta`, `expand_capsule` and `DeltaCapsule` returned **zero files** across
//! `core/runtime`, against **eight** hits for `Delta` in this file as the live positive control. The
//! instrument saw delta vocabulary; what was absent is delta APPLICATION.
//!
//! **The base is named because the sentence otherwise falsifies itself.** Naming those four symbols
//! puts them in this file, so a present-tense "returns zero" becomes false the moment it is
//! written — measured: each now returns one file, this one. A named base stays true forever and
//! needs no exclusion clause; "zero outside this paragraph" would break again the day someone cites
//! the names in a test. The guard in `tests/declared_gap_delta_application.rs` matches DECLARATION
//! forms for the same reason.
//!
//! **So "retains provenance" over more than one hop is unanswered rather than unguarded.** A
//! delta-of-a-delta is not merely untested here — it is not constructible, because a delta is not a
//! constructible thing in this slice. There is no chain for provenance to survive, so no cell can
//! be written that would fail if it did not.
//!
//! **And the criterion's other word collides with a type in this same file.** `ExpansionRequest`
//! (below) is the budget-refusal request naming a larger `required_budget`. That is not capsule
//! expansion; the two share a word and nothing else. A reader checking whether *expansion* is
//! covered finds it, twenty lines from here, answering a different question — which makes this
//! paragraph more necessary rather than less, because the gap is not merely silent, it is silent
//! behind a name that looks like coverage.
//!
//! It is written here for the reason the sibling module states for its own eight: **an undeclared
//! gap and an implemented category read identically from outside — both are silence.** The
//! base-identity half that IS here is good, and its three-way refusal split
//! (`DifferentCapsule` / `DifferentVersion` / `BaseRewritten`) keeps apart three causes with
//! different remedies. Precisely because that half reads finished, nothing signals that the other
//! half was never begun.
//!
//! **Condition for closing it, and who owns it.** It closes when some caller needs a delta APPLIED
//! rather than checked — at which point `verify_delta_base` becomes the precondition of an
//! application function rather than the whole surface, and a chain becomes constructible and
//! therefore testable. Owner is whoever lands that caller. The shape to reuse is `DeltaProvenance`
//! itself, which already forces a base to be named by identity, version and digest together; a
//! chain that carried less would lose exactly the distinction `BaseRewritten` exists to make.

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

/// The sections this compiler emits, in the order it emits them.
///
/// **The schema owns the set; this constant owns the order.** An earlier version of this comment
/// claimed the schema declared the order too, which was a claim about the code that nothing
/// verified — and measuring it showed it could not be true. The capsule schema names its sections
/// in two places, `sections.required` (a JSON array) and `sections.properties` (a JSON object),
/// and JSON Schema reads both as **sets**: order carries no meaning to a validator. There is no
/// ordered authority in that document to defer to.
///
/// Binding the emitted order to the schema file's textual order would be worse than unfounded. Key
/// order in JSON is not semantic, so reordering two keys is a no-op edit to every reader of the
/// document — and it would silently change the bytes of every capsule this compiler produces,
/// hence every digest binding one, hence every cache key holding one.
///
/// What the schema does own is which sections exist, and that agreement is now checked rather than
/// asserted here: `the_compilers_section_set_is_the_capsule_schemas_section_set` in
/// `core/runtime/tests/context_compiler.rs` fails when either side gains or loses a name. The
/// order itself is held by the determinism guard in the same file — alphabetical would also be
/// deterministic, and the point is not which order but that every caller obtains the same one.
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
/// The section names this compiler treats as declared, in the order it emits them.
///
/// Exposed so the agreement with the capsule schema is *checked* rather than asserted in a comment.
/// It returns a slice rather than the array so that callers cannot come to depend on the count, and
/// the ordering stays an implementation choice of this module — what the schema is entitled to
/// constrain is the set, and that is what the guard compares.
pub fn declared_section_order() -> &'static [&'static str] {
    &DECLARED_SECTION_ORDER
}

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

/// What happened when context was fitted to a budget.
///
/// **The budget is spent in BYTES** -- `fit_within_budget` sums `String::len()`. This said
/// "a token budget", which it has never been: nothing tokenizes anywhere in the workspace,
/// and no dependency does either. The wording mattered because #222's
/// `compiled_input_tokens` counter is still an `unavailableField` looking for an honest
/// producer, and this byte total is the nearest number that LOOKS like one -- a counter
/// filled from here would publish bytes under the name of tokens.
///
/// Naming the unit is not a claim that bytes is the RIGHT unit for a context budget. It is
/// pinned by `the_budget_is_measured_in_bytes_not_characters`, so changing it is a
/// deliberate act with a red test attached rather than a silent redefinition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BudgetOutcome {
    /// Everything required fits. `dropped_optional` counts what was left out, never silently.
    Fits {
        included: Vec<String>,
        dropped_optional: usize,
    },
    /// Required context does not fit. Carries the allocated refusal code and what would fit.
    Refused {
        code: graphhelm_protocols::DevelopmentRefusalCode,
        expansion: ExpansionRequest,
    },
}

/// How much budget the required context actually needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpansionRequest {
    pub required_budget: usize,
}

/// Fit context to a token budget, refusing rather than trimming what is required.
///
/// The asymmetry is the whole point. Optional context is droppable and its drops are COUNTED;
/// required context is not droppable at any budget. An implementation that trims required items
/// to fit **succeeds** — it returns a capsule, under budget, with every number healthy and
/// without the evidence the caller was required to see. A refusal is visible; a silent trim is
/// not, and the trimmed capsule flatters every metric this task reports.
///
/// The refusal carries both halves because they can go missing separately: the allocated code
/// `context_budget_insufficient`, and an expansion request naming a budget that would actually
/// fit. Refusing without saying how much more is needed leaves the operator with no move.
///
/// The code is deliberately not `cardinality_violation`: nothing here is malformed, and folding
/// two causes with opposite operator responses — correct it, versus grant more budget — into one
/// code is the flattening this milestone exists to remove.
pub fn fit_within_budget(required: &[String], optional: &[String], budget: usize) -> BudgetOutcome {
    let required_budget: usize = required.iter().map(String::len).sum();
    if required_budget > budget {
        return BudgetOutcome::Refused {
            code: graphhelm_protocols::DevelopmentRefusalCode::ContextBudgetInsufficient,
            expansion: ExpansionRequest { required_budget },
        };
    }

    let mut included: Vec<String> = required.to_vec();
    let mut used = required_budget;
    let mut dropped_optional = 0usize;
    for item in optional {
        if used + item.len() <= budget {
            used += item.len();
            included.push(item.clone());
        } else {
            dropped_optional += 1;
        }
    }
    BudgetOutcome::Fits {
        included,
        dropped_optional,
    }
}

/// Whether a result's citations account for the capsule items it was required to rely on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CitationVerdict {
    Accepted,
    /// `missing` = required items nobody cited. `unknown` = citations the capsule cannot resolve.
    /// Two lists rather than one count: they are different causes with different fixes.
    Refused {
        missing: Vec<String>,
        unknown: Vec<String>,
    },
}

impl CitationVerdict {
    /// The allocated refusal codes this verdict carries, one per cause actually present.
    ///
    /// A list rather than a single code, because the two causes co-occur constantly and for one
    /// reason: an answer citing something the capsule does not contain has usually also failed to
    /// cite an item it was handed. Reporting the first and stopping gives the operator half a cure,
    /// they apply it, and the verdict refuses again for the half they were never shown — which they
    /// will then attribute to the fix they just made.
    ///
    /// Derived from the verdict rather than from the input lists, so an accepted result carries
    /// nothing. A consumer counting refusals must not see two on a clean run.
    pub fn refusal_codes(&self) -> Vec<graphhelm_protocols::DevelopmentRefusalCode> {
        use graphhelm_protocols::DevelopmentRefusalCode as Code;
        match self {
            Self::Accepted => Vec::new(),
            Self::Refused { missing, unknown } => {
                let mut codes = Vec::new();
                if !missing.is_empty() {
                    codes.push(Code::RequiredCitationMissing);
                }
                if !unknown.is_empty() {
                    codes.push(Code::CitationUnresolved);
                }
                codes
            }
        }
    }
}

/// Check a result's citations against the capsule it was compiled from.
///
/// Two failures are reported separately because they are different causes with different fixes.
/// **Missing** means a required item was never cited: the evidence exists and the answer does not
/// connect to it, while the accounting downstream still counts the capsule as used, so utilization
/// rises with nothing behind it. **Unknown** means a citation resolves to no capsule item at all —
/// citation spoofing, which this task's threat assessment names. Item IDs are content-derived, so
/// an ID nothing hashes to is a typo or a fabrication, and either way the citation reads as
/// provenance in any report that counts citations instead of checking them.
///
/// Collapsing them into one count would make each read as the other, and the two have opposite
/// remedies: cite the evidence, versus stop citing something that does not exist.
pub fn verify_citations(
    capsule_item_ids: &[String],
    required_item_ids: &[String],
    cited_item_ids: &[String],
) -> CitationVerdict {
    let missing: Vec<String> = required_item_ids
        .iter()
        .filter(|id| !cited_item_ids.contains(id))
        .cloned()
        .collect();
    let unknown: Vec<String> = cited_item_ids
        .iter()
        .filter(|id| !capsule_item_ids.contains(id))
        .cloned()
        .collect();

    if missing.is_empty() && unknown.is_empty() {
        CitationVerdict::Accepted
    } else {
        CitationVerdict::Refused { missing, unknown }
    }
}

/// What a delta capsule must carry about the base it was computed against.
///
/// A delta is not readable without its base: it is the increment, and read alone it looks exactly
/// like a small complete capsule. That is the failure this type exists to make impossible — a delta
/// whose provenance is lost gets applied to whatever base is at hand and **succeeds**, producing a
/// capsule that is well-formed, under budget, and assembled from evidence the delta was never
/// computed against.
/// **A delta of a delta cannot be expressed here, and that is a limit of this slice rather than a
/// decision about chaining.** The provenance names exactly one base and there is no composition
/// operator, so `verify_delta_base` has no second hop to check and the four cases guarding it are
/// exhaustive over what this type can build — four single-hop cells because single-hop is the
/// whole space, not because the multi-hop cell was skipped. Raised on review, where the reviewer
/// could not tell those two apart from the outside, which is the point of writing it down.
///
/// When chaining arrives, the case that has to arrive with it is the one this shape cannot fail:
/// a chain whose every adjacent pair verifies while the chain as a whole does not reach the base
/// it claims. Each hop is locally correct and the composition is not, which is invisible to a
/// check that only ever sees one pair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeltaProvenance {
    pub base_capsule_id: String,
    pub base_version: u32,
    /// Digest over the base's compiled BYTES, not over its meaning. See `capsules_identical`.
    pub base_digest: String,
}

/// Why a delta could not be applied to the base it was offered.
///
/// Three variants rather than one "base mismatch", because the operator response differs for each
/// and the third is invisible if folded into the others.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeltaBaseRefusal {
    /// The delta was computed against a different capsule entirely. Usually a routing mistake.
    DifferentCapsule { expected: String, offered: String },
    /// The right capsule, the wrong revision. The delta may still be recomputable against this one.
    DifferentVersion { expected: u32, offered: u32 },
    /// Same identity, different bytes: the base was **rewritten underneath the delta**. Nothing in
    /// the identity can show this, which is why the digest is carried at all — and why this cause
    /// must not be folded into the two above, whose remedy is to find the right base. Here the
    /// right base is the one in hand, and it is no longer the one the delta was computed from.
    BaseRewritten { expected: String, actual: String },
}

/// Check that a delta's provenance names the base actually offered.
///
/// Digest first would be tempting and wrong: a digest mismatch is also what a completely different
/// capsule produces, so checking it first reports every routing mistake as a rewritten base and the
/// distinction dies. Identity is checked first so `BaseRewritten` means what it says.
///
/// The digest is checked last and is the only cause identity cannot see: same capsule, same
/// version, different bytes means the base was rewritten underneath the delta, and every identity
/// check downstream agrees while the applied result is assembled from evidence the delta was never
/// computed against.
pub fn verify_delta_base(
    delta: &DeltaProvenance,
    base_capsule_id: &str,
    base_version: u32,
    base_bytes: &[u8],
) -> Result<(), DeltaBaseRefusal> {
    if delta.base_capsule_id != base_capsule_id {
        return Err(DeltaBaseRefusal::DifferentCapsule {
            expected: delta.base_capsule_id.clone(),
            offered: base_capsule_id.to_owned(),
        });
    }
    if delta.base_version != base_version {
        return Err(DeltaBaseRefusal::DifferentVersion {
            expected: delta.base_version,
            offered: base_version,
        });
    }
    let actual = base_digest(base_bytes);
    if delta.base_digest != actual {
        return Err(DeltaBaseRefusal::BaseRewritten {
            expected: delta.base_digest.clone(),
            actual,
        });
    }
    Ok(())
}

/// The digest a `DeltaProvenance` must carry for a given base.
///
/// Over the compiled bytes, deliberately: a canonical digest is blind to written order by design,
/// so a base that was reserialised into a different order would pass while the delta's positions
/// no longer describe it.
pub fn base_digest(base_bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(base_bytes);
    format!("sha256:{}", hex::encode(hasher.finalize()))
}
