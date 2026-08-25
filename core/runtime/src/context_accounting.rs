//! Context Capsule accounting (#222).
//!
//! # Declared gap: eight of the nine cost categories are not here
//!
//! #222's criterion names nine things that must be counted — orientation, zero results, pages,
//! retries, fallbacks, summaries, compiled input, output, and formatting — plus cold and amortized
//! index cost reported separately. **This module implements the index pair and the receipt
//! mechanism, and nothing for the other eight.** Measured rather than estimated, **at `591dac7`,
//! before this paragraph existed**: a case-insensitive sweep of this file for each category name
//! returned zero for all eight, against **three** hits for `index` as the live positive control.
//!
//! **The base is named because the sentence otherwise falsifies itself**, and this one did. Naming
//! the eight categories puts them in this file: each returns one hit today, and the claim of zero
//! has been false since the commit that made it. Re-measured while fixing that (#428): the counts
//! it cited for the positive control — fifteen for `index`, eleven for `amortiz` — were true at no
//! commit at all. At `591dac7` they are three and zero; at `0754cda`, where this paragraph landed,
//! seventeen and thirteen. They appear to be a draft's numbers, kept while the prose around them
//! grew. `amortiz` is dropped from the control rather than re-cited: at the named base it is zero,
//! which cannot serve as evidence that the sweep reads anything.
//!
//! It is written here because an undeclared gap and an implemented category read identically from
//! outside: both are silence. A reader summing this receipt would get a number that looks total.
//!
//! **Condition for closing it, and who owns it.** The eight are all costs of *retrieval*, and this
//! slice has no retriever — no production caller constructs anything in this module, measured as
//! zero call sites outside the tests. They become writable when a caller wires compilation to a
//! retrieval path, which is #219's surface, not this one. Owner is whoever lands that wiring; the
//! shape to reuse is `CostField`, which already forces each number to carry how it came to exist,
//! so the eight arrive with provenance or not at all.
//!
//! Every type here exists to keep two states apart that a careless representation folds into one:
//! a measured zero versus nothing measured, a real run versus a capsule that was only built, an
//! omitted cache dimension versus a miss. In each pair the flattened form fails in the direction
//! that flatters the metric, which is why each is a separate value rather than a sentinel.

/// The semantic inputs a compiled-context cache entry is keyed by.
///
/// Every field here is a dimension the key must carry. The guard enumerates them one by one,
/// because the failure mode is **omission**: a dimension left out of the derivation does not cause
/// a cache miss, it causes a HIT across that dimension.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextCacheKeyInputs {
    /// D-027 scope. A key that ignores it lets one project read another's compiled context.
    pub scope_project: String,
    /// The permission set in force. Ignoring it lets a narrower grant reuse a broader one's entry.
    pub permissions: Vec<String>,
    /// `SnapshotBinding::repo_snapshot` — the identity of the BYTES a reader reads.
    pub repo_snapshot: String,
    /// `SnapshotBinding::index_generation` — the snapshot an index was BUILT FROM. Independent of
    /// `repo_snapshot`; their relation is the freshness verdict, so they are two dimensions.
    pub index_generation: String,
    /// Schema identity of the bound capsule.
    pub schema_id: String,
    /// Schema version of the bound capsule.
    pub schema_version: String,
    /// The objective the capsule was compiled for.
    pub objective: String,
    /// Digest of the bound capsule, over CONTENT — which is why `producer` is a separate dimension.
    ///
    /// **This is a requirement on the caller, not a description of the value.** Measured while J
    /// audited the claim: the only construction of this field anywhere in the repository is a test
    /// literal, so there is no producer to read and nothing enforces what a caller puts here. A
    /// field whose meaning lives only in its doc comment is a convention, and a convention is what
    /// the first caller decides it is.
    ///
    /// The value must be `context_compiler::base_digest(compiled_bytes)` — over the compiled
    /// **bytes**, not over a canonical form. A canonical digest is blind to written order by
    /// design, so keying on one would make two capsules that serialise differently share a cache
    /// entry, which is the cross-caller determinism this compiler exists to provide, defeated at
    /// the cache instead of at the compiler.
    pub capsule_digest: String,
    /// `ArtifactBinding::producer`. NOT subsumed by `capsule_digest`: two producers emitting
    /// byte-identical capsules share a digest, so keying on content alone would let a less-trusted
    /// producer hit an entry warmed by a more-trusted one.
    pub producer: String,
    /// Version of `context-utilization.yaml` in force. Moves independently of every other field:
    /// change the policy and the same capsule with the same inputs must yield a different verdict.
    pub utilization_policy_version: String,
}

impl ContextCacheKeyInputs {
    /// Derive the cache key from every semantic dimension, **unambiguously**.
    ///
    /// Each segment is length-prefixed (`<byte-len>:<value>`), and the variable-length list carries
    /// its element count before its elements. That makes the encoding injective: distinct inputs
    /// cannot produce the same string, because the boundaries are recoverable from the output.
    ///
    /// A separator-joined encoding is not enough, and the reason is the failure direction that
    /// matters here. Separators answer "did the key change when I changed a field?" correctly while
    /// still letting two DIFFERENT inputs collide — one permission literally named
    /// `"repo.read,repo.write"` against the two permissions `repo.read` and `repo.write`. A
    /// collision is a cache HIT for a request that was never computed, which is the same
    /// correct-looking cheap success an omitted dimension produces, arriving by another route.
    pub fn cache_key(&self) -> String {
        let mut key = String::new();
        push_segment(&mut key, &self.scope_project);
        push_segment(&mut key, &self.permissions.len().to_string());
        for permission in &self.permissions {
            push_segment(&mut key, permission);
        }
        push_segment(&mut key, &self.repo_snapshot);
        push_segment(&mut key, &self.index_generation);
        push_segment(&mut key, &self.schema_id);
        push_segment(&mut key, &self.schema_version);
        push_segment(&mut key, &self.objective);
        push_segment(&mut key, &self.capsule_digest);
        push_segment(&mut key, &self.producer);
        push_segment(&mut key, &self.utilization_policy_version);
        key
    }
}

/// Append one length-prefixed segment: `<byte-len>:<value>`.
///
/// The length goes first so the reader knows where the value ends without needing a delimiter that
/// the value must not contain. Any byte is then legal inside a value, which is the property a
/// separator-joined encoding cannot offer.
pub(crate) fn push_segment(out: &mut String, value: &str) {
    out.push_str(&value.len().to_string());
    out.push(':');
    out.push_str(value);
}

/// How a cost number came to exist. The three states have different consequences, so they are three
/// values rather than a boolean plus a convention.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CostProvenance {
    /// Observed at the site that performed the work.
    Measured,
    /// Computed from measured fields. Carries no observer.
    Derived,
    /// The runtime cannot see this number. **Not zero.**
    Unavailable,
}

/// One cost line in an accounting receipt, carrying its own provenance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CostField {
    value: Option<u64>,
    provenance: CostProvenance,
    producer: Option<String>,
    note: String,
}

impl CostField {
    /// Observed by `producer` at the site that did the work.
    pub fn measured(value: u64, producer: &str) -> Self {
        Self {
            value: Some(value),
            provenance: CostProvenance::Measured,
            producer: Some(producer.to_owned()),
            note: String::new(),
        }
    }

    /// Computed from measured fields; `basis` records what from.
    pub fn derived(value: u64, basis: &str) -> Self {
        Self {
            value: Some(value),
            provenance: CostProvenance::Derived,
            producer: None,
            note: basis.to_owned(),
        }
    }

    /// The runtime cannot observe this cost; `reason` records why.
    pub fn unavailable(reason: &str) -> Self {
        Self {
            value: None,
            provenance: CostProvenance::Unavailable,
            producer: None,
            note: reason.to_owned(),
        }
    }

    /// The observed value, or `None` when there is nothing to observe.
    ///
    /// Absence stays absence. A caller that wants to sum must decide what to do about `None`
    /// explicitly — which is the point: a receipt containing an unavailable field cannot silently
    /// produce a total that reads as complete.
    pub fn observed(&self) -> Option<u64> {
        self.value
    }

    pub fn is_measured(&self) -> bool {
        self.provenance == CostProvenance::Measured
    }

    pub fn producer(&self) -> Option<&str> {
        self.producer.as_deref()
    }

    pub fn provenance(&self) -> &CostProvenance {
        &self.provenance
    }
}

/// Whether a compiled capsule was actually used, and if so how much of it was cited.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Utilization {
    /// The capsule was compiled and never executed against. **Not a zero ratio.**
    NotExecuted,
    /// A real run: `cited` of `emitted` items were relied on.
    Ratio { cited: usize, emitted: usize },
}

impl Utilization {
    pub fn from_run(cited: usize, emitted: usize) -> Self {
        Self::Ratio { cited, emitted }
    }

    /// A capsule that was compiled and never executed against.
    ///
    /// Its own state, never `Ratio { cited: 0, .. }`. The two read the same to a careless caller
    /// and mean opposite things: a zero ratio is a finding about the compiler, this is the absence
    /// of a measurement.
    pub fn not_executed() -> Self {
        Self::NotExecuted
    }

    pub fn ratio(&self) -> Option<f64> {
        match self {
            Self::NotExecuted => None,
            Self::Ratio { emitted: 0, .. } => None,
            Self::Ratio { cited, emitted } => Some(*cited as f64 / *emitted as f64),
        }
    }
}

/// Whether two serialized capsules are the same **bytes**.
///
/// Deliberately not a canonical-digest comparison, and the distinction is load-bearing. A canonical
/// digest is blind to key order by design — it answers "same meaning?", which is the right question
/// for a pin and the wrong one for determinism. A determinism check built on it passes for a
/// compiler that silently reorders its output between runs, which is the exact criterion the check
/// claims to enforce.
///
/// Anything wanting "same meaning" should say so and use the canonical form explicitly, so that the
/// weaker comparison is a written choice rather than an accident of which helper was nearest.
pub fn capsules_identical(left: &[u8], right: &[u8]) -> bool {
    left == right
}

/// The name this module reports itself as. A measured field may never carry it: this module records
/// numbers, it does not observe them.
pub const ACCOUNTING_MODULE: &str = "context_accounting";

/// A cost receipt under construction.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AccountingReceipt {
    fields: Vec<(String, CostField)>,
}

impl AccountingReceipt {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one cost line, refusing a measured field this module claims to have observed itself.
    ///
    /// The refusal is structural rather than advisory: a number produced by the recorder is
    /// derived, and letting it sign as the observer would make a recomputed value read exactly like
    /// a watched one. `derived` and `unavailable` fields carry no producer at all, so only
    /// `measured` can commit this error.
    pub fn with_field(mut self, name: &str, field: CostField) -> Result<Self, String> {
        if field.is_measured() && field.producer() == Some(ACCOUNTING_MODULE) {
            return Err(format!(
                "field `{name}` is marked measured but names `{ACCOUNTING_MODULE}` as its observer; this module records numbers, it does not observe them. Name the component that did the work, or mark the field derived."
            ));
        }
        self.fields.push((name.to_owned(), field));
        Ok(self)
    }

    pub fn field(&self, name: &str) -> Option<&CostField> {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, f)| f)
    }
}

/// Index cost, in the two shapes that must never become one number.
///
/// The acceptance criterion asks for cold and amortized index cost **reported separately**, and the
/// reason is not presentation. They answer different questions and fail in opposite directions. A
/// cold cost is what this run actually paid to make an index usable: somebody watched it happen. An
/// amortized share is arithmetic over a cost paid earlier by a run that is not this one — nobody
/// observed this run paying it, and no observer can be named for it.
///
/// Summed, they produce a number that is neither: too large to be what this run paid and too small
/// to be what the index cost, and reported under whichever provenance the summing code happened to
/// pick. The type keeps them apart so the sum has to be written on purpose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IndexCost {
    /// Paid by this run, observed by `producer`.
    Cold { tokens: u64, producer: String },
    /// A share of a cost paid earlier, spread over the runs that use it.
    AmortizedShare {
        total_tokens: u64,
        runs_sharing: u32,
    },
}

impl IndexCost {
    pub fn cold(tokens: u64, producer: &str) -> Self {
        Self::Cold {
            tokens,
            producer: producer.to_owned(),
        }
    }

    /// An amortized share, refusing a denominator of zero and **truncating the remainder**.
    ///
    /// The truncation is a decision, not an artefact of writing `/`. Raised on review: every
    /// fixture here divides exactly (900 over 3), so nothing made the behaviour visible, and a
    /// reader could not tell a chosen policy from an unnoticed one. It is chosen, and this is the
    /// property that makes it safe to choose: **the shares sum to at most the real cost, never
    /// more, and the shortfall is strictly less than `runs_sharing` tokens in total** — one token
    /// per run at worst, spread across the whole fleet that shares one index.
    ///
    /// The direction is the reason. Rounding up would let the reported cost of an index exceed
    /// what was ever paid for it, and a cost line that can overstate is one a reader cannot use to
    /// bound anything. Understating by less than a token per run is a floor that still holds.
    ///
    /// `an_amortized_share_truncates_and_the_loss_is_bounded` pins it with a denominator that does
    /// not divide, so a future change to rounding has to change that test on purpose.
    ///
    /// Zero runs sharing is not a small number, it is an undefined one: dividing by it produces no
    /// value, and a "share" of a cost nothing uses is a category error rather than a rounding
    /// problem. The refusal is at construction so no later reader has to check.
    pub fn amortized(total_tokens: u64, runs_sharing: u32) -> Result<Self, String> {
        if runs_sharing == 0 {
            return Err(format!(
                "an amortized share of {total_tokens} tokens over ZERO runs has no value: the \
                 denominator is what makes a share checkable, and a share of a cost nothing uses \
                 is not a small number, it is an undefined one. Report the cost as cold against \
                 the run that paid it, or name the runs it is spread over."
            ));
        }
        Ok(Self::AmortizedShare {
            total_tokens,
            runs_sharing,
        })
    }

    /// The receipt line this cost becomes, carrying the provenance the cost actually has.
    ///
    /// Cold is `measured` and names the component that watched the work. An amortized share is
    /// `derived`, and this is the load-bearing half: nobody observed *this* run paying it. Marked
    /// measured it would acquire an observer that watched nothing, and a reader auditing which
    /// numbers were seen would get a yes for a number that was computed.
    ///
    /// The basis records the arithmetic rather than only its result, so the share stays checkable:
    /// a share without its denominator is a number nobody can re-derive or contradict.
    pub fn as_cost_field(&self) -> CostField {
        match self {
            Self::Cold { tokens, producer } => CostField::measured(*tokens, producer),
            Self::AmortizedShare {
                total_tokens,
                runs_sharing,
            } => CostField::derived(
                total_tokens / u64::from(*runs_sharing),
                &format!("{total_tokens} tokens spread over {runs_sharing} runs sharing the index"),
            ),
        }
    }
}

/// The receipt field names index cost is reported under. There is deliberately no `index_cost`.
pub const INDEX_COST_COLD_FIELD: &str = "index_cost_cold";
pub const INDEX_COST_AMORTIZED_FIELD: &str = "index_cost_amortized";
