//! Guards for Context Capsule accounting (#222).
//!
//! First property, and the one the issue's threat assessment turns on: **the cache key must carry
//! every semantic input**. The failure this blocks is not a collision — it is an *omission*, and an
//! omitted dimension produces a cache **HIT** across that dimension. A cross-scope or stale-policy
//! hit is a correct-looking cheap answer, which is the shape this milestone exists to prevent.
//!
//! So the property is deliberately NOT "same inputs produce the same key" (that would pass with a
//! constant). It is **"inputs differing in exactly one dimension produce different keys"**, one case
//! per dimension.

use graphhelm_runtime::context_accounting::ContextCacheKeyInputs;

/// The baseline every case mutates exactly one field of.
fn baseline() -> ContextCacheKeyInputs {
    ContextCacheKeyInputs {
        scope_project: "proj-a".to_owned(),
        permissions: vec!["repo.read".to_owned()],
        repo_snapshot: "snap-1".to_owned(),
        index_generation: "gen-1".to_owned(),
        schema_id: "https://p50.dev/schemas/context-capsule.schema.json".to_owned(),
        schema_version: "1.0.0".to_owned(),
        objective: "explain the wake path".to_owned(),
        capsule_digest: "sha256:aaaa".to_owned(),
        producer: "compiler-a".to_owned(),
        utilization_policy_version: "1.0.0".to_owned(),
    }
}

/// Every dimension, with the mutation that must change the key.
///
/// `producer` and `utilization_policy_version` are here for reasons that differ, and both were
/// established before this test was written:
///
/// - **`producer`** is not subsumed by `capsule_digest`. A digest is over CONTENT, so two producers
///   emitting byte-identical capsules share a digest — keying on content alone lets a less-trusted
///   producer hit an entry warmed by a more-trusted one.
/// - **`utilization_policy_version`** moves independently of everything else: change the policy and
///   the same capsule with the same inputs must yield a different verdict, while no other field
///   moves. Omitting it serves stale-policy entries.
///
/// `repo_snapshot` and `index_generation` are TWO cases rather than one because
/// `SnapshotBinding` carries them as independent identities — the bytes read versus what the index
/// was built from — and their *relation* is the freshness verdict. Folding them into one case would
/// be the flattening the guard exists to catch.
/// One dimension case: the name of the semantic input, and the mutation that changes exactly
/// that input and nothing else.
///
/// Named rather than written inline because clippy refuses the inline form at this depth --
/// and it was right to: the tuple-of-function-pointer says what the compiler needs and nothing
/// about what the case IS, which is the whole content of this table.
type DimensionCase = (&'static str, fn(&mut ContextCacheKeyInputs));

fn dimension_cases() -> Vec<DimensionCase> {
    vec![
        ("scope_project", |i| i.scope_project = "proj-b".to_owned()),
        ("permissions", |i| i.permissions = vec!["repo.write".to_owned()]),
        ("repo_snapshot", |i| i.repo_snapshot = "snap-2".to_owned()),
        ("index_generation", |i| i.index_generation = "gen-2".to_owned()),
        ("schema_id", |i| i.schema_id = "https://p50.dev/schemas/other.schema.json".to_owned()),
        ("schema_version", |i| i.schema_version = "2.0.0".to_owned()),
        ("objective", |i| i.objective = "explain the sweep path".to_owned()),
        ("capsule_digest", |i| i.capsule_digest = "sha256:bbbb".to_owned()),
        ("producer", |i| i.producer = "compiler-b".to_owned()),
        ("utilization_policy_version", |i| {
            i.utilization_policy_version = "2.0.0".to_owned()
        }),
    ]
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written (TDD skill's rule,
/// and the same discipline as a sealed prediction): **dropping any one field from the key's
/// derivation**. That mutation is invisible to a "same inputs, same key" test and invisible to a
/// collision test; only a per-dimension difference assertion sees it, and the red must name the
/// dimension that was dropped rather than failing somewhere adjacent.
#[test]
fn every_semantic_dimension_changes_the_cache_key() {
    let base = baseline();
    let base_key = base.cache_key();

    // LANDMARK, load-bearing: an empty or trivially-constant derivation would make the loop below
    // fail in a way that looks like a real finding. If this fires, the instrument is broken, not
    // the invariant.
    assert!(
        !base_key.is_empty(),
        "extraction is broken, not the invariant: the baseline key is empty"
    );

    for (dimension, mutate) in dimension_cases() {
        let mut varied = baseline();
        mutate(&mut varied);
        assert_ne!(
            varied.cache_key(),
            base_key,
            "cache key ignores the `{dimension}` dimension (#222).\n\
             Two requests differing only in `{dimension}` would share a cache entry, so the second \
             receives an answer computed for the first. An omitted dimension does not cause a \
             miss -- it causes a HIT across that dimension, which is a correct-looking cheap \
             success.\n\
             Either include `{dimension}` in the key derivation, or delete the field and its case \
             together."
        );
    }
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before writing it: **encoding a
/// variable-length field by joining its elements with a separator that can occur inside them.**
///
/// The per-dimension guard above cannot see this. It varies one field at a time and asks "did the
/// key change?", which a naive join answers correctly. This asks the harder question: **can two
/// DIFFERENT inputs produce the SAME key?** — the collision direction, which is the one that grants
/// a cache hit to a request that was never computed.
///
/// `permissions` is the field that exposes it because it is the only variable-length one: the set
/// `["repo.read,repo.write"]` (one permission whose name contains the separator) and the set
/// `["repo.read", "repo.write"]` (two permissions) are different authority and must never share an
/// entry.
#[test]
fn different_permission_sets_cannot_share_a_cache_key() {
    let mut one_permission = baseline();
    one_permission.permissions = vec!["repo.read,repo.write".to_owned()];

    let mut two_permissions = baseline();
    two_permissions.permissions = vec!["repo.read".to_owned(), "repo.write".to_owned()];

    assert_ne!(
        one_permission.permissions, two_permissions.permissions,
        "arrangement check: the two permission sets must actually differ, or this proves nothing"
    );

    assert_ne!(
        one_permission.cache_key(),
        two_permissions.cache_key(),
        "two DIFFERENT permission sets derive the SAME cache key (#222).\n\
         The list encoding is ambiguous: joining elements with a separator that can appear inside \
         an element loses the boundary. A request holding one broad permission would receive an \
         entry computed for a request holding two narrower ones, or the reverse.\n\
         Encode the list unambiguously (length-prefixed, or count plus per-element length)."
    );
}

// ---------------------------------------------------------------------------------------------
// Cost provenance. The issue's out-of-scope names the failure directly: "token estimates presented
// as observed usage". A tag is what makes that enforceable instead of aspirational.
// ---------------------------------------------------------------------------------------------

use graphhelm_runtime::context_accounting::{CostField, CostProvenance};

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before writing it: **representing an
/// unobservable cost as `0`** — a bare integer with zero as the sentinel for "no data".
///
/// That is the flattening this whole task exists to refuse. `0` is a legitimate measured value
/// (a step that genuinely cost nothing), and "we could not observe this" is a different state with
/// a different consequence: one may be summed, the other must stop a cheap-success claim. A type
/// that cannot tell them apart makes every downstream total silently wrong in the flattering
/// direction.
#[test]
fn an_unavailable_cost_is_not_zero() {
    let unavailable = CostField::unavailable("provider does not report input tokens");
    let genuinely_zero = CostField::measured(0, "context_compiler");

    assert_eq!(
        genuinely_zero.observed(),
        Some(0),
        "arrangement check: a measured zero must read as an observed zero, or this proves nothing"
    );

    assert_eq!(
        unavailable.observed(),
        None,
        "an unobservable cost reads as an observed value (#222).\n\
         `0` is a legitimate measurement -- a step that really cost nothing -- and 'not observable' \
         is a different state with the opposite consequence: a measured zero may be summed, an \
         unavailable one must refuse the cheap-success claim.\n\
         Represent absence as absence, never as a sentinel value."
    );

    assert!(
        !unavailable.is_measured(),
        "an unavailable cost must not claim to be measured"
    );
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL: **letting a field be built as `measured` without
/// naming the component that observed it.**
///
/// The blueprint's rule is that a number is born where its inputs are born and a reader only reads;
/// a field whose producer is the accounting module itself is derived by construction. Unnamed
/// producers are how a recomputed number passes as an observed one.
#[test]
fn a_measured_cost_names_the_component_that_observed_it() {
    let measured = CostField::measured(1234, "retrieval");
    assert_eq!(measured.producer(), Some("retrieval"));

    let derived = CostField::derived(99, "amortized over declared window");
    assert_eq!(
        derived.producer(),
        None,
        "a derived value has no observer -- claiming one would make a computation look observed"
    );
    assert!(matches!(derived.provenance(), CostProvenance::Derived));
}

// ---------------------------------------------------------------------------------------------
// The named trap: a receipt that counts capsules which never ran.
// ---------------------------------------------------------------------------------------------

use graphhelm_runtime::context_accounting::Utilization;

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before writing it: **computing the ratio from
/// the emitted-item count alone**, so a capsule that was compiled and never used reports
/// `0 cited / N emitted` = 0% utilization.
///
/// That number is not wrong-looking. It reads as a real finding — "the compiler over-fetched" —
/// when the truth is that nothing was measured at all. **Utilization is a property of having been
/// USED, not of having been BUILT**, and the error runs in the direction of the metric's own
/// incentive: a task judged on utilization benefits from counting compilations it never spent.
///
/// Same family as `an_unavailable_cost_is_not_zero`, one level up: absence rendered as a measured
/// value, where the two states carry opposite consequences.
#[test]
fn a_capsule_that_never_ran_has_no_utilization_ratio() {
    let compiled_and_used = Utilization::from_run(3, 10);
    let compiled_never_used = Utilization::not_executed();

    assert!(
        matches!(compiled_and_used, Utilization::Ratio { cited: 3, emitted: 10 }),
        "arrangement check: a real run must produce a ratio, or this proves nothing"
    );

    assert!(
        matches!(compiled_never_used, Utilization::NotExecuted),
        "a capsule that was compiled but never executed reports a utilization ratio (#222).\n\
         `0 cited / N emitted` reads as a finding about the compiler when it is the absence of a \
         measurement. Utilization is a property of having been USED, not of having been BUILT.\n\
         Report not-executed as its own state; never as a ratio."
    );

    assert_eq!(
        compiled_never_used.ratio(),
        None,
        "not-executed must have no numeric ratio at all -- a caller that averages receipts must be \
         unable to fold it in as a zero"
    );
}

// ---------------------------------------------------------------------------------------------
// Byte-identity. The acceptance criterion says "byte-identical capsules and accounting receipts",
// and this is the guard that pins the INSTRUMENT rather than restating the property.
// ---------------------------------------------------------------------------------------------

use graphhelm_runtime::context_accounting::capsules_identical;

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before writing it: **deciding byte-identity by
/// comparing canonical digests.**
///
/// A canonical digest is deliberately blind to key order — that is what makes it useful for asking
/// "is this the same MEANING?". It is therefore the wrong instrument for "are these the same
/// BYTES?", and the failure is silent: the check passes while the criterion it claims to verify is
/// violated. A sibling task found this trap live on this milestone, which is why the instrument is
/// pinned here rather than left to whoever implements it.
///
/// The two documents below carry the same fields with the same values in a different written order.
/// Same meaning, same canonical digest, different bytes. A byte comparison must call them
/// different; a digest comparison cannot.
#[test]
fn byte_identity_is_decided_on_bytes_not_on_canonical_meaning() {
    let written_one_way = br#"{"objective":"a","nodeId":"n1"}"#;
    let written_the_other = br#"{"nodeId":"n1","objective":"a"}"#;

    // Arrangement: they really are the same meaning, or the test proves nothing about the trap.
    let left: serde_json::Value = serde_json::from_slice(written_one_way).unwrap();
    let right: serde_json::Value = serde_json::from_slice(written_the_other).unwrap();
    assert_eq!(
        left, right,
        "arrangement check: the two documents must carry the same MEANING, or this does not \
         exercise the digest trap at all"
    );
    assert_ne!(
        written_one_way.as_slice(),
        written_the_other.as_slice(),
        "arrangement check: the two documents must differ in BYTES"
    );

    assert!(
        !capsules_identical(written_one_way, written_the_other),
        "byte-identity was decided by meaning rather than by bytes (#222).\n\
         These two documents canonicalize to the same value and differ in their written form. A \
         determinism check built on a canonical digest passes here, which means it would pass for \
         a compiler that silently changed its output ordering between runs -- exactly the \
         criterion it is supposed to enforce.\n\
         Compare the raw serialized bytes."
    );

    assert!(
        capsules_identical(written_one_way, written_one_way),
        "identical bytes must compare identical"
    );
}

// ---------------------------------------------------------------------------------------------
// Where a number is born. A number is born where its inputs are born; a reader only reads.
// ---------------------------------------------------------------------------------------------

use graphhelm_runtime::context_accounting::{AccountingReceipt, ACCOUNTING_MODULE};

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before writing it: **accepting the accounting
/// module itself as the producer of a measured field.**
///
/// Rule-resolution cost is observed inside Task 002's resolver, retrieval counts inside the
/// retrieval port, output tokens at the response boundary. The accounting module *records* them. If
/// it can name itself as the observer, then a number it recomputed — from a log, from a re-parse,
/// from an estimate — is indistinguishable in the receipt from one that was actually watched.
///
/// This is the mechanical form of the blueprint's rule: **a field whose producer is the accounting
/// module is derived by construction**, so `measured` plus that producer is a contradiction the
/// type must refuse rather than a convention a reviewer must remember.
#[test]
fn the_accounting_module_cannot_be_the_observer_of_a_measured_cost() {
    let honest = AccountingReceipt::new().with_field(
        "ruleResolutionTokens",
        CostField::measured(412, "code_contract_resolver"),
    );
    assert!(
        honest.is_ok(),
        "arrangement check: a field observed by a real producer must be accepted"
    );

    let self_attributed = AccountingReceipt::new().with_field(
        "ruleResolutionTokens",
        CostField::measured(412, ACCOUNTING_MODULE),
    );
    assert!(
        self_attributed.is_err(),
        "the accounting module was accepted as the observer of a MEASURED cost (#222).\n\
         A number the recorder produced is derived, not observed. Allowing it to sign as the \
         observer makes a recomputed value indistinguishable from a watched one, which is the \
         whole distinction the provenance tag exists to carry.\n\
         Either name the component that did the work, or mark the field derived."
    );
}

// ---------------------------------------------------------------------------------------------
// Cold and amortized index cost, which the acceptance criterion requires reported SEPARATELY.
// ---------------------------------------------------------------------------------------------

use graphhelm_runtime::context_accounting::{
    IndexCost, INDEX_COST_AMORTIZED_FIELD, INDEX_COST_COLD_FIELD,
};


/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **reporting an
/// amortized share as a measured cost** -- `CostField::measured(total / runs, "index")`.
///
/// It is the tempting implementation, because a share is a number and every other number in a
/// receipt is measured. But nobody observed *this* run paying it: the cost was paid earlier by a
/// run that is not this one, and the share is arithmetic over that. Marked measured, it acquires an
/// observer that never watched anything, and a reader auditing "which numbers were seen" gets a
/// yes for a number that was computed.
#[test]
fn an_amortized_share_is_derived_because_nobody_watched_this_run_pay_it() {
    let share = IndexCost::amortized(900, 3).expect("three runs is a usable denominator");
    let field = share.as_cost_field();

    assert!(
        !field.is_measured(),
        "the amortized share reported itself as MEASURED. It names {:?} as its observer, and that \
         component did not watch this run pay anything -- the cost was paid by an earlier run.",
        field.producer()
    );
    assert_eq!(
        field.provenance(),
        &CostProvenance::Derived,
        "an amortized share is computed from a cost paid elsewhere, so its provenance is Derived"
    );
    assert_eq!(
        field.observed(),
        Some(300),
        "the share of 900 tokens over 3 runs is 300; the value must still be present, since \
         Derived means computed, not absent"
    );
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **letting the
/// cold arm lose its observer** -- reporting it derived, or measured against a generic name rather
/// than the component that did the work.
///
/// This is the discriminating half. A `as_cost_field` that returns Derived for everything satisfies
/// the test above perfectly and destroys the distinction it exists to protect: cold cost IS
/// observed, and marking it derived means no receipt anywhere can say a real index build was
/// watched.
#[test]
fn a_cold_index_cost_keeps_the_observer_that_watched_it() {
    let field = IndexCost::cold(1200, "native_index_builder").as_cost_field();

    assert!(
        field.is_measured(),
        "a cold index cost was paid by this run and watched; it must be measured, not {:?}",
        field.provenance()
    );
    assert_eq!(
        field.producer(),
        Some("native_index_builder"),
        "the cold cost must name the component that did the work, not a generic label"
    );
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **accepting a
/// denominator of zero**, returning a share of zero or of the full total.
///
/// A share over zero runs is not small, it is undefined. Returning the full total quietly turns an
/// amortized line into a cold one; returning zero quietly deletes a real cost. Both read as a
/// number and neither is one.
#[test]
fn an_amortized_share_over_zero_runs_is_refused_rather_than_valued() {
    let refusal = IndexCost::amortized(900, 0);
    let message = refusal.expect_err("a zero denominator must be refused, not valued");
    assert!(
        message.contains("ZERO"),
        "the refusal must say what was wrong with the input; it said: {message}"
    );
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **adding the two
/// costs into a single receipt line** -- an `index_cost` field carrying cold plus amortized.
///
/// The merged number is neither of its parts: larger than what this run paid and smaller than what
/// the index cost, carrying whichever provenance the summing code picked. The absence assertion
/// below is paired with the presence of both real lines, so a receipt that simply lost its index
/// accounting cannot satisfy it -- an absence guard with nothing present is satisfied by an empty
/// receipt.
#[test]
fn index_cost_lands_as_two_lines_and_never_as_one() {
    let receipt = AccountingReceipt::new()
        .with_field(
            INDEX_COST_COLD_FIELD,
            IndexCost::cold(1200, "native_index_builder").as_cost_field(),
        )
        .expect("a cold index cost is a legal receipt line")
        .with_field(
            INDEX_COST_AMORTIZED_FIELD,
            IndexCost::amortized(900, 3)
                .expect("three runs is a usable denominator")
                .as_cost_field(),
        )
        .expect("an amortized index cost is a legal receipt line");

    assert_eq!(
        receipt.field(INDEX_COST_COLD_FIELD).and_then(|f| f.observed()),
        Some(1200),
        "the cold line must be present and hold what this run paid"
    );
    assert_eq!(
        receipt
            .field(INDEX_COST_AMORTIZED_FIELD)
            .and_then(|f| f.observed()),
        Some(300),
        "the amortized line must be present and hold this run's share"
    );
    assert!(
        receipt.field("index_cost").is_none(),
        "the receipt carries a merged `index_cost` line. It is neither number: too large to be \
         what this run paid, too small to be what the index cost, and reported under whichever \
         provenance the summing code picked."
    );
}

/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL, named before the test was written: **changing the
/// rounding of an amortized share** — to `div_ceil`, to a float, or to anything that reports more
/// than was paid.
///
/// Raised by F's cold pass: every other case here divides exactly (900 over 3), so the truncation
/// was invisible and a reader could not tell a chosen policy from an unnoticed one. This case uses
/// a denominator that does not divide, so the policy is pinned by a number rather than by a
/// comment, and the bound is asserted rather than asserted-about: the shares sum to at most the
/// real cost, short by strictly less than one token per sharing run.
#[test]
fn an_amortized_share_truncates_and_the_loss_is_bounded() {
    let total = 901u64;
    let runs = 3u32;
    let share = IndexCost::amortized(total, runs)
        .expect("three runs is a usable denominator")
        .as_cost_field();

    assert_eq!(
        share.observed(),
        Some(300),
        "901 over 3 truncates to 300. If this says 301, the rounding now reports more than was \
         ever paid, and a cost line that can overstate cannot be used to bound anything."
    );

    let summed = share.observed().expect("the share has a value") * u64::from(runs);
    assert!(
        summed <= total,
        "the shares sum to {summed}, which is MORE than the {total} actually paid"
    );
    assert!(
        total - summed < u64::from(runs),
        "the shortfall is {}, which is not under one token per run ({runs}). The bound is what \
         makes truncation safe to choose; without it the understatement is unbounded.",
        total - summed
    );
}

/// THIS ONE DOES NOT REDDEN — IT FAILS TO COMPILE, deliberately, and that is the point.
///
/// Raised by L's primary review, and it is the ironic one: the dimension table above is the guard
/// against a cache key that silently omits an input, and the table itself was **maintained by
/// hand** against the struct. Add a field to `ContextCacheKeyInputs` and every existing case still
/// passes — the omitted dimension is exactly the failure the table exists to catch, arriving
/// through the table.
///
/// The destructuring below has no `..`, so a new field stops this file compiling. A compile error
/// cannot be read as a pass, cannot be skipped, and lands on the author rather than on whoever
/// reads the report later. Then the count assertion forces the new name into the case list rather
/// than letting it be named and left uncovered.
///
/// **If you are here because this stopped compiling, do not take rustc's advice.** Verified by
/// sabotage — adding a field produced `error[E0027]: pattern does not mention field ...`, exactly
/// once and here, with the tree restored afterwards. The compiler then offers two remedies:
/// *"you can explicitly ignore it"* and *"or always ignore missing fields here"*. Both mean adding
/// `..`, both make this compile again in one keystroke, and both silently delete the guard —
/// leaving a cache key with a dimension nothing covers, which is a HIT across that dimension and a
/// correct-looking cheap answer. The remedy is to add the field to the list below **and** give it
/// a case in `dimension_cases`.
#[test]
fn every_field_of_the_key_inputs_has_a_dimension_case() {
    let ContextCacheKeyInputs {
        scope_project,
        permissions,
        repo_snapshot,
        index_generation,
        schema_id,
        schema_version,
        objective,
        capsule_digest,
        producer,
        utilization_policy_version,
    } = baseline();

    // Bound so the destructuring cannot be dismissed as unused and quietly replaced with `..`.
    let fields: [(&str, bool); 10] = [
        ("scope_project", !scope_project.is_empty()),
        ("permissions", !permissions.is_empty()),
        ("repo_snapshot", !repo_snapshot.is_empty()),
        ("index_generation", !index_generation.is_empty()),
        ("schema_id", !schema_id.is_empty()),
        ("schema_version", !schema_version.is_empty()),
        ("objective", !objective.is_empty()),
        ("capsule_digest", !capsule_digest.is_empty()),
        ("producer", !producer.is_empty()),
        ("utilization_policy_version", !utilization_policy_version.is_empty()),
    ];

    for (name, populated) in fields {
        assert!(
            populated,
            "the baseline leaves `{name}` empty, so the case that mutates it may not be changing \
             anything observable"
        );
        assert!(
            dimension_cases().iter().any(|(case, _)| *case == name),
            "`{name}` is a field of the cache key with no dimension case. A dimension with no case \
             is not a weaker guard, it is no guard: an omitted dimension produces a cache HIT \
             across it, which is a correct-looking cheap answer."
        );
    }

    assert_eq!(
        dimension_cases().len(),
        fields.len(),
        "there are {} dimension cases for {} fields. A case naming something that is not a field \
         is dead weight claiming to be coverage.",
        dimension_cases().len(),
        fields.len()
    );
}
