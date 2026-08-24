//! #219 task-003: snapshot-bound retrieval plan compilation.
//!
//! Under construction by TDD. Only what a currently-failing test demanded exists here.

use graphhelm_protocols::{CoverageState, DeclaredLimits, DevelopmentRefusalCode, SnapshotBinding};

/// What an index returned, with its coverage verdict **in the same value**.
///
/// Coverage is a RETURN VALUE rather than a query option on purpose: results cannot be obtained
/// without obtaining the coverage that says what a zero among them would mean, so "forgot to check
/// coverage" is unrepresentable rather than merely discouraged.
pub struct IndexResponse {
    pub hits: Vec<String>,
    pub coverage: CoverageState,
    /// Free prose the provider attached to its answer.
    ///
    /// Carried so it can be preserved as candidate evidence, and **never read by plan
    /// compilation**. A provider can write anything here, including text shaped like an
    /// instruction about how its own structured fields should be interpreted. The structured
    /// fields are the input; this is a claim about the input, which is a different kind of thing.
    pub summary: Option<String>,
    /// How many pages the runtime walked to assemble this response.
    ///
    /// Counted by the side that DROVE the pagination, never reported by the provider. A page count
    /// taken from the thing being bounded is a self-report about the budget it is spending.
    pub pages: u32,
}

impl IndexResponse {
    #[must_use]
    pub const fn new(hits: Vec<String>, coverage: CoverageState) -> Self {
        Self {
            hits,
            coverage,
            summary: None,
            pages: 1,
        }
    }

    /// Record how many pages the runtime walked for this response.
    #[must_use]
    pub const fn after_pages(mut self, pages: u32) -> Self {
        self.pages = pages;
        self
    }

    /// Attach the provider's prose. Deliberately does not participate in compilation.
    #[must_use]
    pub fn with_summary(mut self, summary: &str) -> Self {
        self.summary = Some(summary.to_owned());
        self
    }
}

/// A compiled plan, or the typed refusal that replaced it.
#[derive(Debug, PartialEq, Eq)]
pub enum RetrievalOutcome {
    Claim {
        hits: Vec<String>,
        /// The coverage the hits were found under.
        ///
        /// Carried rather than dropped: a partial search returning three hits reports a FLOOR, a
        /// complete one returning three reports a TOTAL, and a caller handed the bare list cannot
        /// tell which it was given. The input boundary refuses to hand over results without this
        /// verdict; the output boundary must not undo that.
        coverage: CoverageState,
    },
    /// A zero that coverage licenses as a fact about the subject.
    VerifiedAbsence,
    Refused {
        code: DevelopmentRefusalCode,
    },
}

/// Compile a retrieval plan from an index response bound to a snapshot pair.
pub fn compile_plan(binding: &SnapshotBinding, response: &IndexResponse) -> RetrievalOutcome {
    // The binding is consulted BEFORE coverage, and the order is the point. Coverage is the
    // provider's claim about its own search; the binding is a fact about which bytes the
    // coordinates were computed against. A provider reporting `Complete` over a stale binding is
    // the most confident wrong answer available, so the binding wins.
    //
    // There are two independent staleness signals and they are not redundant: this one is
    // staleness DETECTED here, the `CoverageState::Stale` arm below is the provider DECLARING it.
    // Either alone must refuse.
    if !binding.is_fresh() {
        return RetrievalOutcome::Refused {
            code: DevelopmentRefusalCode::IndexStale,
        };
    }
    if response.hits.is_empty() {
        // Matched exhaustively rather than with a wildcard, and that is the point: `CoverageState`
        // is a CLOSED set, so a state added upstream must break this build instead of silently
        // falling into whatever the catch-all happened to say. A `_ =>` arm here would make the
        // closed-world claim shrink in silence the day a variant is added.
        return match response.coverage {
            CoverageState::Complete => RetrievalOutcome::VerifiedAbsence,
            // Stale is NOT pooled with the rest: it is the one state whose repair is known.
            // Reindexing answers staleness and says nothing about an unfinished search, so a
            // caller that wants to fix the situation needs the two told apart.
            CoverageState::Stale => RetrievalOutcome::Refused {
                code: DevelopmentRefusalCode::IndexStale,
            },
            CoverageState::Partial
            | CoverageState::Excluded
            | CoverageState::Skipped
            | CoverageState::ExtractionGap
            | CoverageState::Unknown
            | CoverageState::Unresolved => RetrievalOutcome::Refused {
                code: DevelopmentRefusalCode::NegativeClaimUnverified,
            },
        };
    }
    // Provider output is untrusted typed evidence, and a hit is a path this plan would hand to a
    // reader. Validated BEFORE the claim is built, so nothing escaping ever reaches a reader.
    if response.hits.iter().any(|hit| !is_repository_relative(hit)) {
        return RetrievalOutcome::Refused {
            code: DevelopmentRefusalCode::ScopeMismatch,
        };
    }
    RetrievalOutcome::Claim {
        hits: response.hits.iter().map(|hit| canonical_hit(hit)).collect(),
        coverage: response.coverage,
    }
}

/// One file named two ways is one hit.
///
/// This repository is developed on Windows and runs on Linux, so the same file arrives with one
/// separator from one provider and the other from another. Carrying the provider's separators into
/// the plan splits every downstream identity — digests, caches, comparisons — by the platform the
/// provider happened to run on, and the split is silent because both plans look right.
///
/// Canonicalised where the claim is BUILT rather than at every point one is compared: a
/// normalisation each consumer has to remember is one some consumer will forget.
fn canonical_hit(hit: &str) -> String {
    hit.replace('\\', "/")
}

/// Whether a provider-supplied hit stays inside the repository.
///
/// Checked by SEGMENT rather than by substring: a substring test for `".."` also rejects the
/// perfectly ordinary `src/..foo.rs`, and a rule that fires on innocent input gets relaxed by the
/// next person who hits it.
fn is_repository_relative(hit: &str) -> bool {
    // A hit may carry a trailing `:<line>`, and on Windows `:` also separates a DRIVE. Splitting on
    // the first colon conflates the two: `C:/Windows/System32` yields `"C"`, which has no `..` and
    // no leading slash, so a drive-qualified escape reads as an ordinary relative path. Strip only
    // a trailing all-digit suffix, from the END.
    let path = match hit.rsplit_once(':') {
        Some((head, tail)) if !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) => head,
        _ => hit,
    };

    if path.is_empty() {
        return false;
    }
    // Rooted at a separator: absolute POSIX, or a Windows UNC/rooted path.
    if path.starts_with('/') || path.starts_with('\\') {
        return false;
    }
    // Drive-qualified (`C:` or `C:/...`): an escape carrying no `..` at all.
    let mut chars = path.chars();
    if matches!((chars.next(), chars.next()), (Some(letter), Some(':')) if letter.is_ascii_alphabetic())
    {
        return false;
    }
    // Parent traversal, checked by SEGMENT rather than substring: a substring test for `".."` also
    // rejects the perfectly ordinary `src/..foo.rs`, and a rule that fires on innocent input gets
    // relaxed by the next person who hits it.
    !path.split(['/', '\\']).any(|segment| segment == "..")
}

/// Compile a plan, verifying the bytes a reader would serve against the binding first.
///
/// This is the half [`compile_plan`] structurally cannot do. A [`SnapshotBinding`] compares its two
/// ids to each other, so it catches staleness that was already visible and is blind to the case
/// where both ids agree and the bytes underneath moved. Asking the reader what it would actually
/// serve is the only way to reach that case, and it is why `repo_snapshot` must be derived from
/// CONTENT rather than from a ref — see [`crate::ports::SourceReader`].
pub fn compile_plan_against<R: crate::ports::SourceReader + ?Sized>(
    binding: &SnapshotBinding,
    response: &IndexResponse,
    reader: &R,
) -> RetrievalOutcome {
    if reader.current_snapshot() != binding.repo_snapshot {
        return RetrievalOutcome::Refused {
            code: DevelopmentRefusalCode::IndexStale,
        };
    }
    compile_plan(binding, response)
}

/// Compile a plan under declared bounds.
///
/// **Order: the plan is compiled FIRST, bounds are applied to what it produced.** A stale binding
/// or an escaping path invalidates the response outright; the payload being too large is a fact
/// about a response that was otherwise usable. Reporting the budget over the staleness sends the
/// caller to trim their query, which is the wrong repair and leaves the stale index in place.
/// (Found by F: the first version checked bounds first and returned `cardinality_violation` for a
/// response that was ALSO stale.)
///
/// Bounds are then enforced HERE, over whatever the provider actually returned, rather than trusted
/// to the provider: a flooding provider is the threat the bound exists for, so asking it to respect
/// a limit it is the one violating is not a bound at all.
///
/// Over-budget REFUSES rather than truncating. A silently truncated result set is a partial search
/// wearing a complete search's clothes: the caller sees a plausible number of hits under a
/// `Complete` coverage verdict and no way to tell the rest were dropped.
pub fn compile_plan_within(
    binding: &SnapshotBinding,
    response: &IndexResponse,
    limits: &DeclaredLimits,
) -> RetrievalOutcome {
    let outcome = compile_plan(binding, response);
    if !matches!(outcome, RetrievalOutcome::Claim { .. }) {
        return outcome;
    }

    if response.hits.len() as u64 > u64::from(limits.max_results) {
        return RetrievalOutcome::Refused {
            code: DevelopmentRefusalCode::CardinalityViolation,
        };
    }
    // Checked separately from the result bound because they fail independently: many tiny hits are
    // under the byte bound and over the result bound, and one enormous hit is the reverse. Measured
    // over the representation this plan would carry.
    let payload_bytes = response
        .hits
        .iter()
        .map(|hit| hit.len() as u64)
        .sum::<u64>()
        + response.summary.as_ref().map_or(0, |s| s.len() as u64);
    if payload_bytes > limits.max_bytes {
        return RetrievalOutcome::Refused {
            code: DevelopmentRefusalCode::ArtifactTooLarge,
        };
    }
    if response.pages > limits.max_pages {
        return RetrievalOutcome::Refused {
            code: DevelopmentRefusalCode::CardinalityViolation,
        };
    }
    // Estimated, deliberately crude, and deliberately OURS. A provider's own token accounting is a
    // self-report about the very thing it is being bounded on. Four bytes per token is wrong in
    // detail and right in the property that matters: it grows with the payload the caller pays for.
    if payload_bytes.div_ceil(4) > u64::from(limits.max_tokens) {
        return RetrievalOutcome::Refused {
            code: DevelopmentRefusalCode::ArtifactTooLarge,
        };
    }
    outcome
}

/// Whether bounded source fallback exists yet. It does not.
///
/// This is not a feature flag and must never become one. It exists so a guard can assert the REASON
/// a non-complete coverage state refuses, rather than only that it refuses.
///
/// Today `ExtractionGap` refuses because there is nothing to fall back TO. The acceptance criterion
/// gives that state two exits — bounded source fallback, or `negative_claim_unverified` — and only
/// the second is built. "Refused after trying the source" and "refused because no source path
/// exists" are identical in the outcome and are different facts.
///
/// **When fallback lands, flip this to `true` and the guard that reads it goes RED on purpose.**
/// That is the point: an assertion that survives the change which makes it meaningless is the one
/// nobody looks at again. Whoever builds the fallback is then forced to rewrite that arm
/// deliberately, asserting that the fallback was ATTEMPTED, instead of inheriting a green cell that
/// silently changed meaning underneath them.
#[must_use]
pub const fn source_fallback_available() -> bool {
    false
}
