//! Context Capsule accounting (#222).
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
    /// Digest of the bound capsule. Note this is over CONTENT, which is why `producer` is separate.
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
